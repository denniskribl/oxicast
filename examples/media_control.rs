//! Media playback control example — pause, seek, volume, resume.
//!
//! Usage:
//!   CAST_IP=192.168.1.5 CAST_URL=http://example.com/video.mp4 cargo run --example media_control

use oxicast::{CastApp, CastClient, MediaInfo, StreamType};
use std::time::Duration;

#[tokio::main]
async fn main() -> oxicast::Result<()> {
    tracing_subscriber::fmt().with_env_filter("oxicast=info").init();

    let ip = std::env::var("CAST_IP").expect("Set CAST_IP=<device_ip>");
    let url = std::env::var("CAST_URL").expect("Set CAST_URL=<media_url>");

    println!("Connecting to {ip}...");
    let client = CastClient::connect(&ip, 8009).await?;
    println!("Connected!");

    // Launch and load
    client.launch_app(&CastApp::DefaultMediaReceiver).await?;
    let media = MediaInfo::new(&url, "video/mp4").stream_type(StreamType::Buffered);
    client.load_media(&media, true, 0.0, None).await?;
    println!("Playing...");

    // Wait for playback to start
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Pause
    println!("Pausing...");
    client.pause().await?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Seek to 30 seconds
    println!("Seeking to 30s...");
    client.seek(30.0).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Set volume to 50%
    println!("Setting volume to 50%...");
    client.set_volume(0.5).await?;

    // Resume
    println!("Resuming...");
    client.play().await?;

    // Watch status for 10 seconds
    let status_rx = client.watch_media_status();
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Some(status) = status_rx.borrow().as_ref() {
            println!(
                "  {:?} at {:.1}s / {:.1}s",
                status.player_state,
                status.current_time,
                status.duration.unwrap_or(0.0)
            );
        }
    }

    // Stop
    println!("Stopping...");
    client.stop_media().await.ok();
    client.disconnect().await?;
    println!("Done.");

    Ok(())
}
