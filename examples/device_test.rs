//! Real device integration test.
//!
//! Run with your Chromecast IP:
//!
//!   cargo run --example device_test --all-features -- 192.168.1.XXX
//!
//! This will:
//! 1. Connect to the device
//! 2. Get receiver status
//! 3. Launch Default Media Receiver
//! 4. Load a test video (Big Buck Bunny)
//! 5. Wait for playback to start
//! 6. Test pause/play/seek
//! 7. Set volume
//! 8. Stop and disconnect
//!
//! Each step prints pass/fail so you can see exactly what works.

use oxicast::{CastApp, CastClient, CastEvent, MediaInfo, PlayerState, StreamType};
use std::time::Duration;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "oxicast=debug,device_test=info".parse().unwrap()),
        )
        .init();

    let ip = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: cargo run --example device_test --all-features -- <CHROMECAST_IP>");
        eprintln!();
        eprintln!("Trying mDNS discovery...");
        String::new()
    });

    let (ip, port) = if ip.is_empty() {
        match discover_device().await {
            Some(d) => {
                println!("Found: {} at {}:{}", d.name, d.ip, d.port);
                (d.ip.to_string(), d.port)
            }
            None => {
                eprintln!("No devices found. Pass IP manually.");
                std::process::exit(1);
            }
        }
    } else {
        (ip, 8009)
    };

    let mut passed = 0u32;
    let mut failed = 0u32;

    macro_rules! step {
        ($name:expr, $body:expr) => {{
            print!("  {} ... ", $name);
            match $body {
                Ok(val) => {
                    println!("OK");
                    passed += 1;
                    Some(val)
                }
                Err(e) => {
                    println!("FAIL: {e}");
                    failed += 1;
                    None
                }
            }
        }};
    }

    println!("\n=== oxicast device test against {ip}:{port} ===\n");

    // 1. Connect
    let client = match step!("Connect", CastClient::connect(&ip, port).await) {
        Some(c) => c,
        None => {
            println!("\nCannot continue without connection.");
            std::process::exit(1);
        }
    };

    assert!(client.is_connected(), "should be connected");

    // 2. Receiver status
    if let Some(status) = step!("Get receiver status", client.receiver_status().await) {
        println!("    Volume: {:.0}%, muted: {}", status.volume.level * 100.0, status.volume.muted);
        println!("    Apps: {}", status.applications.len());
    }

    // 3. Launch app
    let _app = step!(
        "Launch Default Media Receiver",
        client.launch_app(&CastApp::DefaultMediaReceiver).await
    );

    // 4. Load media
    // Default: HLS stream (Chromecast natively supports HLS, most reliable)
    // Override with second CLI arg: mp4 | hls | <custom-url>
    let media_arg = std::env::args().nth(2).unwrap_or_else(|| "hls".into());
    let media = match media_arg.as_str() {
        "hls" => {
            println!("    Using: Mux HLS test stream (H264/AAC, up to 1080p)");
            MediaInfo::new(
                "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8",
                "application/x-mpegURL",
            )
            .stream_type(StreamType::Buffered)
        }
        "mp4" => {
            println!("    Using: TU Ilmenau Big Buck Bunny MP4 (H264, 1080p)");
            MediaInfo::new(
                "https://avtshare01.rz.tu-ilmenau.de/avt-vqdb-uhd-1/test_1/segments/bigbuck_bunny_8bit_15000kbps_1080p_60.0fps_h264.mp4",
                "video/mp4",
            )
            .stream_type(StreamType::Buffered)
        }
        url => {
            println!("    Using: custom URL {url}");
            let content_type =
                if url.contains(".m3u8") { "application/x-mpegURL" } else { "video/mp4" };
            MediaInfo::new(url, content_type).stream_type(StreamType::Buffered)
        }
    };

    let load_result =
        step!("Load media (Big Buck Bunny)", client.load_media(&media, true, 0.0).await);

    if let Some(ref status) = load_result {
        println!("    State: {:?}, Duration: {:?}s", status.player_state, status.duration);
    }

    // 5. Wait for PLAYING state
    if load_result.is_some() {
        print!("  Wait for PLAYING ... ");
        let playing = wait_for_state(&client, PlayerState::Playing, Duration::from_secs(15)).await;
        if playing {
            println!("OK");
            passed += 1;
        } else {
            println!("FAIL: timeout");
            failed += 1;
        }
    }

    // 6. Pause
    step!("Pause", client.pause().await);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 7. Resume
    step!("Play (resume)", client.play().await);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 8. Seek to 60s
    if let Some(status) = step!("Seek to 60s", client.seek(60.0).await) {
        println!("    Position after seek: {:.1}s", status.current_time);
    }

    // 9. Get media status
    if let Some(Some(s)) = step!("Get media status", client.media_status().await) {
        println!("    State: {:?}, Position: {:.1}s", s.player_state, s.current_time);
    }

    // 10. Volume control
    step!("Set volume to 30%", client.set_volume(0.3).await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    step!("Restore volume to 80%", client.set_volume(0.8).await);

    // 11. Watch channels
    {
        print!("  Watch channels have data ... ");
        let media_rx = client.watch_media_status();
        let recv_rx = client.watch_receiver_status();
        if media_rx.borrow().is_some() && recv_rx.borrow().is_some() {
            println!("OK");
            passed += 1;
        } else {
            println!(
                "FAIL (media={}, receiver={})",
                media_rx.borrow().is_some(),
                recv_rx.borrow().is_some()
            );
            failed += 1;
        }
    }

    // 12. Stop media
    step!("Stop media", client.stop_media().await);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 13. Disconnect
    step!("Disconnect", client.disconnect().await);
    assert!(!client.is_connected(), "should be disconnected");

    // Summary
    println!("\n=== Results: {passed} passed, {failed} failed ===\n");
    if failed > 0 {
        std::process::exit(1);
    }
}

async fn wait_for_state(client: &CastClient, target: PlayerState, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        tokio::select! {
            Some(event) = client.next_event() => {
                if let CastEvent::MediaStatusChanged(s) = event {
                    if s.player_state == target {
                        return true;
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return false;
            }
        }
    }
}

async fn discover_device() -> Option<oxicast::DeviceInfo> {
    let devices = oxicast::discovery::discover_devices(Duration::from_secs(3)).await.ok()?;
    devices.into_iter().next()
}
