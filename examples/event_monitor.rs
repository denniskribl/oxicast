//! Monitor all events from a Cast device in real-time.
//!
//! Usage:
//!   CAST_IP=192.168.1.5 cargo run --example event_monitor
//!
//! Press Ctrl+C to stop.

use oxicast::{CastClient, CastEvent};

#[tokio::main]
async fn main() -> oxicast::Result<()> {
    tracing_subscriber::fmt().with_env_filter("oxicast=debug").init();

    let ip = std::env::var("CAST_IP").expect("Set CAST_IP=<device_ip>");
    let port: u16 = std::env::var("CAST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8009);

    println!("Connecting to {ip}:{port}...");
    let client = CastClient::connect(&ip, port).await?;
    println!("Connected! Monitoring events (Ctrl+C to stop)...\n");

    // Get initial status
    let status = client.receiver_status().await?;
    println!(
        "Device volume: {:.0}%{}",
        status.volume.level * 100.0,
        if status.volume.muted { " (muted)" } else { "" }
    );
    for app in &status.applications {
        println!("Running app: {} ({})", app.display_name, app.app_id);
    }
    println!();

    loop {
        tokio::select! {
            Some(event) = client.next_event() => {
                match &event {
                    CastEvent::Connected => println!("[CONNECTED]"),
                    CastEvent::Disconnected(reason) => {
                        println!("[DISCONNECTED] {reason:?}");
                    }
                    CastEvent::Reconnecting { attempt } => {
                        println!("[RECONNECTING] attempt {attempt}");
                    }
                    CastEvent::Reconnected => println!("[RECONNECTED]"),
                    CastEvent::HeartbeatTimeout => println!("[HEARTBEAT TIMEOUT]"),
                    CastEvent::ReceiverStatusChanged(s) => {
                        println!("[RECEIVER] vol={:.0}% muted={} apps={}",
                            s.volume.level * 100.0, s.volume.muted, s.applications.len());
                    }
                    CastEvent::MediaStatusChanged(s) => {
                        println!("[MEDIA] {:?} at {:.1}s / {:.1}s (session={})",
                            s.player_state, s.current_time,
                            s.duration.unwrap_or(0.0), s.media_session_id);
                    }
                    CastEvent::MediaSessionEnded { media_session_id, idle_reason } => {
                        println!("[SESSION END] session={media_session_id} reason={idle_reason:?}");
                    }
                    CastEvent::RawMessage { namespace, payload, .. } => {
                        println!("[RAW] {namespace}: {payload}");
                    }
                    _ => {}
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\nDisconnecting...");
                client.disconnect().await?;
                break;
            }
        }
    }

    Ok(())
}
