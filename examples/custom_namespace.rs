//! Send and receive messages on a custom Cast namespace.
//!
//! This demonstrates how to interact with custom Cast receiver apps
//! that use their own namespaces beyond the standard media protocol.
//!
//! Usage:
//!   CAST_IP=192.168.1.5 cargo run --example custom_namespace

use oxicast::{CastClient, CastEvent};
use serde_json::json;

#[tokio::main]
async fn main() -> oxicast::Result<()> {
    tracing_subscriber::fmt().with_env_filter("oxicast=info").init();

    let ip = std::env::var("CAST_IP").expect("Set CAST_IP=<device_ip>");

    println!("Connecting to {ip}...");
    let client = CastClient::connect(&ip, 8009).await?;
    println!("Connected!");

    // Get receiver status to see what's running
    let status = client.receiver_status().await?;
    if status.applications.is_empty() {
        println!("No app running. Launch one first.");
        client.disconnect().await?;
        return Ok(());
    }

    let app = &status.applications[0];
    println!("Active app: {} (transport={})", app.display_name, app.transport_id);
    println!("Supported namespaces:");
    for ns in &app.namespaces {
        println!("  {ns}");
    }

    // Send a raw message on a custom namespace
    // All unknown namespaces are automatically emitted as CastEvent::RawMessage
    let custom_ns = "urn:x-cast:com.google.cast.debugoverlay";
    println!("\nSending raw message to toggle debug overlay...");
    client.send_raw_no_reply(custom_ns, &app.transport_id, json!({"type": "SHOW"})).await?;

    // Listen for responses
    println!("Listening for 10s...\n");
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(event) = client.next_event() => {
                if let CastEvent::RawMessage { namespace, payload, .. } = &event {
                    println!("[{namespace}] {payload}");
                }
            }
            () = &mut timeout => break,
        }
    }

    client.disconnect().await?;
    println!("Done.");
    Ok(())
}
