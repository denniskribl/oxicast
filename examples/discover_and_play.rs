//! Discover Cast devices on the network and play a test video.
//!
//! Usage:
//!   cargo run --example discover_and_play
//!
//! Or with a specific device IP:
//!   CAST_IP=192.168.178.32 cargo run --example discover_and_play

use oxicast::{CastApp, CastClient, CastEvent, MediaInfo, StreamType};
use std::time::Duration;

#[tokio::main]
async fn main() -> oxicast::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,oxicast=debug".parse().unwrap()),
        )
        .init();

    // Connect — either via env var or discovery
    let client = if let Ok(ip) = std::env::var("CAST_IP") {
        let port: u16 =
            std::env::var("CAST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8009);
        println!("Connecting to {ip}:{port}...");
        CastClient::connect(&ip, port).await?
    } else {
        println!("Discovering Cast devices (3s)...");
        let devices = oxicast::discovery::discover_devices(Duration::from_secs(3)).await?;

        if devices.is_empty() {
            eprintln!("No devices found. Set CAST_IP=<ip> to connect directly.");
            std::process::exit(1);
        }

        for (i, d) in devices.iter().enumerate() {
            println!(
                "  [{}] {} at {}:{} ({})",
                i,
                d.name,
                d.ip,
                d.port,
                d.model.as_deref().unwrap_or("unknown model")
            );
        }

        let device = &devices[0];
        println!("\nConnecting to {}...", device.name);
        CastClient::connect(&device.ip.to_string(), device.port).await?
    };

    println!("Connected!");

    // Stop any stale app, then launch fresh
    let status = client.receiver_status().await?;
    for app in &status.applications {
        println!("Stopping stale app: {}", app.display_name);
        client.stop_app(&app.session_id).await.ok();
    }

    println!("Launching media receiver...");
    // Small delay for the device to clean up
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let app = client.launch_app(&CastApp::DefaultMediaReceiver).await?;
    println!("App: {} (session={})", app.display_name, app.session_id);

    // Load a public test video (Big Buck Bunny via HTTPS)
    let url = std::env::var("CAST_URL")
        .unwrap_or_else(|_| "https://media.w3.org/2010/05/sintel/trailer_hd.mp4".into());
    let content_type = std::env::var("CAST_CT").unwrap_or_else(|_| "video/mp4".into());

    let media = MediaInfo::new(&url, &content_type).stream_type(StreamType::Buffered);

    println!("Loading: {url}");
    let status = client.load_media(&media, true, 0.0, None).await?;
    println!("Loaded! State: {:?}, Duration: {:?}s", status.player_state, status.duration);

    // Listen for events
    println!("\nListening for events (60s)...\n");
    let timeout = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(event) = client.next_event() => {
                match &event {
                    CastEvent::MediaStatusChanged(s) => {
                        println!(
                            "  {:?} at {:.1}s / {:.1}s",
                            s.player_state,
                            s.current_time,
                            s.duration.unwrap_or(0.0)
                        );
                    }
                    CastEvent::ReceiverStatusChanged(s) => {
                        println!("  Volume: {:.0}% {}", s.volume.level * 100.0,
                            if s.volume.muted { "(muted)" } else { "" });
                    }
                    CastEvent::MediaSessionEnded { media_session_id, idle_reason } => {
                        println!("  Session {media_session_id} ended: {idle_reason:?}");
                    }
                    CastEvent::Disconnected(reason) => {
                        println!("  Disconnected: {reason:?}");
                        break;
                    }
                    CastEvent::RawMessage { namespace, payload, .. } => {
                        println!("  Raw [{namespace}]: {payload}");
                    }
                    other => {
                        println!("  Event: {other:?}");
                    }
                }
            }
            () = &mut timeout => {
                println!("\n60s elapsed. Stopping...");
                break;
            }
        }
    }

    // Clean up — stop_media may fail if media already finished, that's ok
    if client.is_connected() {
        client.stop_media().await.ok();
        client.disconnect().await?;
    }
    println!("Done.");

    Ok(())
}
