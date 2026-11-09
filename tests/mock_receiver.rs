//! End-to-end tests against an in-process mock Cast receiver over TLS.
//!
//! The mock speaks the real wire format (TLS + length-prefixed protobuf
//! CastMessage with JSON payloads) so these tests exercise the client's
//! connection, routing and request-correlation logic.

use std::sync::Arc;
use std::time::Duration;

use prost::encoding::{WireType, decode_key, decode_varint, skip_field};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";

/// A message the client sent to the mock receiver.
#[derive(Debug, Clone)]
struct Received {
    destination: String,
    namespace: String,
    payload: Value,
}

fn encode(namespace: &str, source: &str, dest: &str, payload: &Value) -> Vec<u8> {
    let mut buf = Vec::new();
    prost::encoding::int32::encode(1, &0, &mut buf);
    prost::encoding::string::encode(2, &source.to_string(), &mut buf);
    prost::encoding::string::encode(3, &dest.to_string(), &mut buf);
    prost::encoding::string::encode(4, &namespace.to_string(), &mut buf);
    prost::encoding::int32::encode(5, &0, &mut buf);
    prost::encoding::string::encode(6, &payload.to_string(), &mut buf);
    let mut out = (buf.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&buf);
    out
}

fn decode(mut buf: &[u8]) -> Received {
    let (mut destination, mut namespace, mut payload) = (String::new(), String::new(), Value::Null);
    while !buf.is_empty() {
        let (tag, wire_type) = decode_key(&mut buf).expect("key");
        match (tag, wire_type) {
            (3 | 4 | 6, WireType::LengthDelimited) => {
                let len = decode_varint(&mut buf).expect("len") as usize;
                let s = String::from_utf8(buf[..len].to_vec()).expect("utf8");
                buf = &buf[len..];
                match tag {
                    3 => destination = s,
                    4 => namespace = s,
                    _ => payload = serde_json::from_str(&s).unwrap_or(Value::Null),
                }
            }
            _ => skip_field(wire_type, tag, &mut buf, Default::default()).expect("skip"),
        }
    }
    Received { destination, namespace, payload }
}

fn app(transport: &str, session: &str) -> Value {
    json!({
        "appId": "CC1AD845",
        "displayName": "Default Media Receiver",
        "sessionId": session,
        "transportId": transport,
        "namespaces": [{"name": NS_MEDIA}],
        "statusText": "Casting",
    })
}

/// Starts a mock receiver that runs three apps (`app-1` with media session 11,
/// `app-2` with media session 22, `silent-app` never answers media requests). Returns its port and a stream of every
/// non-heartbeat message the client sends.
async fn start_mock_receiver() -> (u16, mpsc::UnboundedReceiver<Received>) {
    let certs = vec![
        CertificateDer::from_pem_slice(include_bytes!("fixtures/mock_device.crt")).expect("cert"),
    ];
    let key =
        PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/mock_device.key")).expect("key");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("server config");
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (seen_tx, seen_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut tls = acceptor.accept(tcp).await.expect("tls accept");
        loop {
            let mut len = [0u8; 4];
            if tls.read_exact(&mut len).await.is_err() {
                return;
            }
            let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
            if tls.read_exact(&mut buf).await.is_err() {
                return;
            }
            let msg = decode(&buf);
            let request_id = msg.payload.get("requestId").cloned().unwrap_or(json!(0));

            let reply = match (msg.namespace.as_str(), msg.payload["type"].as_str()) {
                (NS_HEARTBEAT, Some("PING")) => {
                    Some((NS_HEARTBEAT, "receiver-0", json!({"type": "PONG"})))
                }
                (NS_HEARTBEAT, _) => None,
                (NS_RECEIVER, Some("GET_STATUS")) => Some((
                    NS_RECEIVER,
                    "receiver-0",
                    json!({
                        "type": "RECEIVER_STATUS",
                        "requestId": request_id,
                        "status": {
                            "volume": {"level": 0.5, "muted": false},
                            "applications": [app("app-1", "session-1"), app("app-2", "session-2"), app("silent-app", "session-3")],
                        }
                    }),
                )),
                (NS_MEDIA, Some(_)) if request_id != json!(0) => {
                    let (transport, session_id) = match msg.destination.as_str() {
                        "app-1" => ("app-1", 11),
                        "app-2" => ("app-2", 22),
                        _ => continue,
                    };
                    let state = if msg.payload["type"] == "PAUSE" { "PAUSED" } else { "PLAYING" };
                    Some((
                        NS_MEDIA,
                        transport,
                        json!({
                            "type": "MEDIA_STATUS",
                            "requestId": request_id,
                            "status": [{
                                "mediaSessionId": session_id,
                                "playerState": state,
                                "currentTime": 1.0,
                                "volume": {"level": 1.0, "muted": false},
                                "media": {
                                    "contentUrl": format!("http://example.com/{transport}.mp4"),
                                    "contentType": "video/mp4",
                                    "streamType": "BUFFERED",
                                },
                            }],
                        }),
                    ))
                }
                _ => None,
            };

            if msg.namespace != NS_HEARTBEAT {
                let _ = seen_tx.send(msg.clone());
            }
            if let Some((ns, source, payload)) = reply {
                let frame = encode(ns, source, "sender-0", &payload);
                if tls.write_all(&frame).await.is_err() {
                    return;
                }
            }
        }
    });

    (port, seen_rx)
}

/// Waits for the next client message matching `pred`, skipping others.
async fn expect_msg(
    rx: &mut mpsc::UnboundedReceiver<Received>,
    pred: impl Fn(&Received) -> bool,
) -> Received {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let msg = rx.recv().await.expect("mock receiver closed");
            if pred(&msg) {
                return msg;
            }
        }
    })
    .await
    .expect("timed out waiting for expected message")
}

async fn connect(port: u16) -> oxicast::CastClient {
    oxicast::CastClient::builder("127.0.0.1", port)
        .request_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("connect to mock receiver")
}

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
