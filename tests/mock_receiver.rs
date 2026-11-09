//! End-to-end tests against an in-process mock Cast receiver over TLS.
//!
//! The mock speaks the real wire format (TLS + length-prefixed protobuf
//! CastMessage with JSON payloads) so these tests exercise the client's
//! connection, routing and request-correlation logic.

mod common;

use common::*;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::test]
async fn join_session_fetches_status_so_controls_work() {
    let (port, mut seen) = start_mock_receiver().await;
    let client = connect(port).await;

    let status = client.receiver_status().await.expect("receiver status");
    let app1 = status.applications.iter().find(|a| a.transport_id == "app-1").expect("app-1");

    let media = client.join_session(app1).await.expect("join").expect("media status");
    assert_eq!(media.media_session_id, 11);
    assert_eq!(
        media.media.as_ref().and_then(|m| m.content_url.as_deref()),
        Some("http://example.com/app-1.mp4")
    );

    let connect_msg = expect_msg(&mut seen, |m| m.destination == "app-1").await;
    assert_eq!(connect_msg.namespace, NS_CONNECTION);
    assert_eq!(connect_msg.payload["type"], "CONNECT");

    // No explicit media_status() call: pause must use the joined session.
    let paused = client.pause().await.expect("pause after join");
    assert_eq!(paused.media_session_id, 11);
    let pause = expect_msg(&mut seen, |m| m.payload["type"] == "PAUSE").await;
    assert_eq!(pause.destination, "app-1");
    assert_eq!(pause.payload["mediaSessionId"], 11);

    client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn join_another_session_closes_previous_and_drops_stale_media_session() {
    let (port, mut seen) = start_mock_receiver().await;
    let client = connect(port).await;

    let status = client.receiver_status().await.expect("receiver status");
    let app1 = status.applications.iter().find(|a| a.transport_id == "app-1").expect("app-1");
    let app2 = status.applications.iter().find(|a| a.transport_id == "app-2").expect("app-2");

    client.join_session(app1).await.expect("join app-1");
    let media = client.join_session(app2).await.expect("join app-2").expect("media status");
    assert_eq!(media.media_session_id, 22);

    let close = expect_msg(&mut seen, |m| m.payload["type"] == "CLOSE").await;
    assert_eq!(close.destination, "app-1");
    assert_eq!(close.namespace, NS_CONNECTION);

    client.pause().await.expect("pause after second join");
    let pause = expect_msg(&mut seen, |m| m.payload["type"] == "PAUSE").await;
    assert_eq!(pause.destination, "app-2");
    assert_eq!(pause.payload["mediaSessionId"], 22, "must not reuse app-1's session id");

    client.disconnect().await.expect("disconnect");
}

/// Collects everything the client sent until the mock connection closes.
async fn drain(rx: &mut mpsc::UnboundedReceiver<Received>) -> Vec<Received> {
    let mut out = Vec::new();
    while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
        out.push(msg);
    }
    out
}

#[tokio::test]
async fn close_leaves_playback_running() {
    let (port, mut seen) = start_mock_receiver().await;
    let client = connect(port).await;

    let status = client.receiver_status().await.expect("receiver status");
    let app1 = status.applications.iter().find(|a| a.transport_id == "app-1").expect("app-1");
    client.join_session(app1).await.expect("join");

    client.close().await.expect("close");
    assert!(!client.is_connected());

    let sent = drain(&mut seen).await;
    assert!(
        !sent.iter().any(|m| m.payload["type"] == "STOP"),
        "close() must not stop media: {sent:?}"
    );
    assert!(sent.iter().any(|m| m.payload["type"] == "CLOSE" && m.destination == "app-1"));
    assert!(sent.iter().any(|m| m.payload["type"] == "CLOSE" && m.destination == "receiver-0"));
}

#[tokio::test]
async fn disconnect_still_stops_media() {
    let (port, mut seen) = start_mock_receiver().await;
    let client = connect(port).await;

    let status = client.receiver_status().await.expect("receiver status");
    let app1 = status.applications.iter().find(|a| a.transport_id == "app-1").expect("app-1");
    client.join_session(app1).await.expect("join");

    client.disconnect().await.expect("disconnect");

    let sent = drain(&mut seen).await;
    let stop =
        sent.iter().position(|m| m.payload["type"] == "STOP").expect("disconnect sends STOP");
    assert_eq!(sent[stop].destination, "app-1");
    assert_eq!(sent[stop].payload["mediaSessionId"], 11);
    let close = sent
        .iter()
        .position(|m| m.payload["type"] == "CLOSE" && m.destination == "receiver-0")
        .expect("disconnect sends CLOSE to receiver-0");
    assert!(stop < close, "STOP must be sent before CLOSE");
    // Behaviour downstream users rely on: disconnect() does not CLOSE the app transport.
    assert!(!sent.iter().any(|m| m.payload["type"] == "CLOSE" && m.destination == "app-1"));
}

#[tokio::test]
async fn join_session_succeeds_when_status_fetch_times_out() {
    let (port, mut seen) = start_mock_receiver().await;
    let client = oxicast::CastClient::builder("127.0.0.1", port)
        .request_timeout(Duration::from_millis(300))
        .connect()
        .await
        .expect("connect");

    let status = client.receiver_status().await.expect("receiver status");
    let silent = status.applications.iter().find(|a| a.transport_id == "silent-app").expect("app");

    let media = client.join_session(silent).await.expect("join must not fail on status timeout");
    assert!(media.is_none());
    let connect = expect_msg(&mut seen, |m| m.destination == "silent-app").await;
    assert_eq!(connect.payload["type"], "CONNECT");
    client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn connects_to_ipv6_literals_with_and_without_brackets() {
    for host in ["::1", "[::1]"] {
        let Some((port, _seen)) = start_mock_receiver_on("[::1]:0").await else {
            eprintln!("IPv6 loopback unavailable, skipping");
            return;
        };
        let client = oxicast::CastClient::builder(host, port)
            .request_timeout(Duration::from_secs(5))
            .connect()
            .await
            .unwrap_or_else(|e| panic!("connect to {host}: {e}"));
        client.receiver_status().await.expect("receiver status over IPv6");
        client.close().await.expect("close");
    }
}

#[tokio::test]
async fn connection_loss_is_reported_exactly_once() {
    let (port, _seen) = start_mock_receiver().await;
    let client = oxicast::CastClient::builder("127.0.0.1", port)
        .auto_reconnect(false)
        .request_timeout(Duration::from_millis(500))
        .connect()
        .await
        .expect("connect");

    client
        .send_raw_no_reply(NS_TEST, "receiver-0", json!({"type": "DROP_CONNECTION"}))
        .await
        .expect("send drop trigger");
    // Keep writing so both the reader and the writer hit the dead socket.
    for _ in 0..20 {
        let _ = client
            .send_raw_no_reply(NS_RECEIVER, "receiver-0", json!({"type": "GET_STATUS"}))
            .await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let mut disconnects = 0;
    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_millis(500), client.next_event()).await
    {
        if matches!(event, oxicast::CastEvent::Disconnected(_)) {
            disconnects += 1;
        }
    }
    assert_eq!(disconnects, 1, "a lost connection must be reported exactly once");
    assert!(!client.is_connected());
}
