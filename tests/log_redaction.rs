//! Payloads must never reach the logs. Runs in its own test binary because a
//! thread-local tracing subscriber is unreliable while other tests run in
//! parallel (tracing caches callsite interest process-wide).

mod common;

use common::*;
use serde_json::json;
use std::sync::Arc;

/// Captures formatted tracing output for the current thread.
#[derive(Clone, Default)]
struct LogBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn trace_logs_never_contain_payloads() {
    let logs = LogBuffer::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let (port, _seen) = start_mock_receiver().await;
    let client = connect(port).await;
    let status = client.receiver_status().await.expect("receiver status");
    let app1 = status.applications.iter().find(|a| a.transport_id == "app-1").expect("app-1");
    client.join_session(app1).await.expect("join");

    let mut media =
        oxicast::MediaInfo::new("http://lan/video.mp4?token=SECRET-TOKEN-123", "video/mp4");
    media.content_url = Some("http://lan/video.mp4?token=SECRET-TOKEN-456".into());
    let custom = json!({"auth": "SECRET-TOKEN-789"});
    client.load_media(&media, true, 0.0, Some(&custom)).await.expect("load");
    client.close().await.expect("close");

    let out = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(out.contains("→ send") && out.contains("LOAD"), "summaries should still be logged");
    assert!(!out.contains("SECRET-TOKEN"), "payload leaked into logs:\n{out}");
    // Receiver replies (which echo media URLs) must not be logged either.
    assert!(!out.contains("example.com/app-1.mp4"), "reply payload leaked into logs:\n{out}");
}
