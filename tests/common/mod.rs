//! Shared in-process mock Cast receiver for integration tests.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use prost::encoding::{WireType, decode_key, decode_varint, skip_field};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";
pub const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
pub const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
/// Test-only namespace: the mock resets the connection on `DROP_CONNECTION`.
pub const NS_TEST: &str = "urn:x-cast:oxicast.test";

/// A message the client sent to the mock receiver.
#[derive(Debug, Clone)]
pub struct Received {
    pub destination: String,
    pub namespace: String,
    pub payload: Value,
}

pub fn encode(namespace: &str, source: &str, dest: &str, payload: &Value) -> Vec<u8> {
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

pub fn decode(mut buf: &[u8]) -> Received {
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

pub fn app(transport: &str, session: &str) -> Value {
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
pub async fn start_mock_receiver() -> (u16, mpsc::UnboundedReceiver<Received>) {
    start_mock_receiver_on("127.0.0.1:0").await.expect("bind")
}

/// Like [`start_mock_receiver`], bound to `bind_addr`; `None` if it can't bind.
pub async fn start_mock_receiver_on(
    bind_addr: &str,
) -> Option<(u16, mpsc::UnboundedReceiver<Received>)> {
    let certs = vec![
        CertificateDer::from_pem_slice(include_bytes!("../fixtures/mock_device.crt"))
            .expect("cert"),
    ];
    let key =
        PrivateKeyDer::from_pem_slice(include_bytes!("../fixtures/mock_device.key")).expect("key");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("server config");
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind(bind_addr).await.ok()?;
    let port = listener.local_addr().expect("addr").port();
    let (seen_tx, seen_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        // So dropping the stream resets the connection (RST) like a vanished device.
        tcp.set_zero_linger().expect("linger");
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

            if msg.namespace == NS_TEST && msg.payload["type"] == "DROP_CONNECTION" {
                return; // drops `tls` -> RST
            }
            // `drop-app` behaves like a device that vanishes when told to stop.
            if msg.destination == "drop-app" && msg.payload["type"] == "STOP" {
                let _ = seen_tx.send(msg.clone());
                return;
            }
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
                            "applications": [app("app-1", "session-1"), app("app-2", "session-2"), app("silent-app", "session-3"), app("drop-app", "session-4")],
                        }
                    }),
                )),
                (NS_MEDIA, Some(_)) if request_id != json!(0) => {
                    let (transport, session_id) = match msg.destination.as_str() {
                        "app-1" => ("app-1", 11),
                        "app-2" => ("app-2", 22),
                        "drop-app" => ("drop-app", 44),
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

    Some((port, seen_rx))
}

/// Waits for the next client message matching `pred`, skipping others.
pub async fn expect_msg(
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

pub async fn connect(port: u16) -> oxicast::CastClient {
    oxicast::CastClient::builder("127.0.0.1", port)
        .request_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("connect to mock receiver")
}
