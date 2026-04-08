---
title: Getting Started
---

# 🚀 Getting Started

This guide walks you through connecting to a Cast device, playing media, and reacting to events.

---

## Installation

```toml
[dependencies]
oxicast = "0.0.1"
tokio = { version = "1", features = ["full"] }
```

---

## Connecting

### By IP (simplest)

If you know the device's IP address:

```rust
use oxicast::CastClient;

let client = CastClient::connect("192.168.1.100", 8009).await?;
```

### With custom settings

```rust
use std::time::Duration;

let client = CastClient::builder("192.168.1.100", 8009)
    .heartbeat_interval(Duration::from_secs(5))
    .heartbeat_timeout(Duration::from_secs(15))
    .request_timeout(Duration::from_secs(10))
    .auto_reconnect(true)
    .max_reconnect_attempts(5)
    .reconnect_delay(Duration::from_secs(2))
    .verify_tls(false) // Cast devices use self-signed certs
    .connect()
    .await?;
```

### Via mDNS discovery

Scan the local network for Cast devices:

```rust
use std::time::Duration;

let devices = oxicast::discovery::discover_devices(Duration::from_secs(3)).await?;
for device in &devices {
    println!("{} at {}:{}", device.name, device.ip, device.port);
}

// Connect to the first one
if let Some(device) = devices.first() {
    let client = device.connect().await?;
}
```

Or stream devices as they're found (useful for real-time UI updates):

```rust
let mut stream = oxicast::discovery::discover(Duration::from_secs(5))?;
while let Some(device) = stream.recv().await {
    println!("Found: {} at {}", device.name, device.ip);
}
```

---

## Playing media

Cast devices need a running application to play media. The Default Media Receiver handles most formats (MP4, HLS, WebM).

```rust
use oxicast::{CastApp, MediaInfo};

// 1. Launch the media receiver
client.launch_app(&CastApp::DefaultMediaReceiver).await?;

// 2. Load media
let status = client.load_media(
    &MediaInfo::new("https://example.com/video.mp4", "video/mp4"),
    true,  // autoplay
    0.0,   // start position in seconds
).await?;

println!("State: {:?}", status.player_state);
```

### With metadata

```rust
use oxicast::{MediaInfo, MediaMetadata, Image};

let media = MediaInfo::new("https://example.com/movie.mp4", "video/mp4")
    .duration(7200.0)
    .metadata(MediaMetadata::Movie {
        title: Some("My Movie".into()),
        subtitle: Some("A great film".into()),
        studio: None,
        images: vec![Image {
            url: "https://example.com/poster.jpg".into(),
            width: Some(800),
            height: Some(1200),
        }],
    });
```

### HLS streams

```rust
let media = MediaInfo::new(
    "https://example.com/stream.m3u8",
    "application/x-mpegURL",
);
```

### Live content

```rust
use oxicast::StreamType;

let media = MediaInfo::live("https://example.com/live.m3u8", "application/x-mpegURL");
// or
let media = MediaInfo::new(url, content_type).stream_type(StreamType::Live);
```

---

## Controlling playback

```rust
client.pause().await?;
client.play().await?;
client.seek(120.0).await?;       // seek to 2 minutes
client.stop_media().await?;      // end the media session
client.set_volume(0.5).await?;   // 0.0 to 1.0, clamped
client.set_muted(true).await?;
```

---

## Reacting to events

### Option A: Event stream

Use `next_event()` in a `tokio::select!` loop. This is the most common pattern:

```rust
use oxicast::CastEvent;

loop {
    tokio::select! {
        Some(event) = client.next_event() => match event {
            CastEvent::MediaStatusChanged(status) => {
                println!("{:?} at {:.1}s", status.player_state, status.current_time);
            }
            CastEvent::MediaSessionEnded { idle_reason, .. } => {
                println!("Ended: {idle_reason}");
            }
            CastEvent::Disconnected(reason) => {
                println!("Lost connection: {reason:?}");
                break;
            }
            CastEvent::Reconnected => {
                println!("Connection restored");
            }
            _ => {}
        },
        _ = tokio::signal::ctrl_c() => {
            client.disconnect().await?;
            break;
        }
    }
}
```

> **Note:** `next_event()` is single-consumer. Only one task should call it at a time. The event channel is bounded — if you don't drain it, events are dropped (not blocked). The reader task is never stalled.

### Option B: Watch channels

If you only care about the latest state and don't need every event:

```rust
let mut media_rx = client.watch_media_status();
let mut receiver_rx = client.watch_receiver_status();

// Always has the latest value, no draining needed
loop {
    media_rx.changed().await?;
    if let Some(status) = media_rx.borrow_and_update().as_ref() {
        println!("Position: {:.1}s", status.current_time);
    }
}
```

Watch channels work regardless of whether anyone calls `next_event()`.

---

## Casting local files

With the `serve` feature, you can cast files from your local filesystem:

```toml
oxicast = { version = "0.0.1", features = ["serve"] }
```

### One-call convenience

```rust
let (server, status) = client.serve_and_cast(
    "/path/to/video.mp4",
    "video/mp4",
    true,
    0.0,
).await?;
// Keep `server` alive for the duration of playback!
```

### Manual setup

```rust
use oxicast::serve::FileServer;

let server = FileServer::start("0.0.0.0:0").await?;
let url = server.serve_file("/path/to/video.mp4", "video/mp4")?;
println!("Serving at: {url}");

client.load_media(&MediaInfo::new(&url, "video/mp4"), true, 0.0).await?;
```

---

## Disconnecting

```rust
// Graceful shutdown — stops all background tasks, next_event() returns None
client.disconnect().await?;

// Check connection state
client.is_connected();     // false after disconnect
client.connection_state(); // ConnectionState::Disconnected
```

After `disconnect()`, you can call `reconnect()` to re-establish the connection.
