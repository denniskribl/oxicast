---
title: API Overview
---

# 🔧 API Overview

Complete reference of the public API surface. For detailed type documentation, see [docs.rs](https://docs.rs/oxicast).

---

## Connection

```rust
// Connect with defaults (port 8009 is standard for Cast)
let client = CastClient::connect("192.168.1.100", 8009).await?;

// Builder for custom settings
let client = CastClient::builder("192.168.1.100", 8009)
    .heartbeat_interval(Duration::from_secs(5))
    .heartbeat_timeout(Duration::from_secs(15))
    .request_timeout(Duration::from_secs(10))
    .auto_reconnect(true)
    .max_reconnect_attempts(5)
    .reconnect_delay(Duration::from_secs(2))
    .verify_tls(false)
    .event_buffer_size(64)
    .connect()
    .await?;
```

| Method | Returns | Description |
|--------|---------|-------------|
| `connect(host, port)` | `Result<CastClient>` | Connect with default settings |
| `builder(host, port)` | `CastClientBuilder` | Start building with custom config |
| `is_connected()` | `bool` | Check if the connection is alive |
| `connection_state()` | `ConnectionState` | `Connected`, `Disconnected`, or `Reconnecting { attempt }` |
| `disconnect()` | `Result<()>` | Graceful shutdown, closes event channel |
| `reconnect()` | `Result<()>` | Manual reconnect (serialized with auto-reconnect) |

---

## Receiver control

```rust
let status = client.receiver_status().await?;
let app = client.launch_app(&CastApp::DefaultMediaReceiver).await?;
client.stop_app(&app.session_id).await?;
let volume = client.set_volume(0.5).await?;  // clamped to [0.0, 1.0]
let volume = client.set_muted(true).await?;
```

| Method | Returns | Description |
|--------|---------|-------------|
| `receiver_status()` | `Result<ReceiverStatus>` | Get current device status (volume, apps) |
| `launch_app(app)` | `Result<Application>` | Launch an app, connects to its transport |
| `stop_app(session_id)` | `Result<()>` | Stop a running app |
| `set_volume(level)` | `Result<Volume>` | Set volume (0.0 to 1.0) |
| `set_muted(muted)` | `Result<Volume>` | Mute or unmute |

### Well-known apps

```rust
CastApp::DefaultMediaReceiver  // CC1AD845 — plays HLS, MP4, WebM, etc.
CastApp::YouTube               // 233637DE
CastApp::Backdrop              // E8C28D3C — ambient screen
CastApp::Custom("MY_APP_ID".into())
```

---

## Media playback

All media methods require a running app — call `launch_app()` first.

```rust
let status = client.load_media(&media, true, 0.0, None).await?;
client.play().await?;
client.pause().await?;
client.seek(120.0).await?;
client.stop_media().await?;
let status = client.media_status().await?;
```

| Method | Returns | Description |
|--------|---------|-------------|
| `load_media(media, autoplay, start_time, custom_data)` | `Result<MediaStatus>` | Load media onto the device |
| `play()` | `Result<MediaStatus>` | Resume playback |
| `pause()` | `Result<MediaStatus>` | Pause playback |
| `seek(position)` | `Result<MediaStatus>` | Seek to position in seconds |
| `stop_media()` | `Result<MediaStatus>` | End the media session |
| `media_status()` | `Result<Option<MediaStatus>>` | Get current media status |

### Building MediaInfo

```rust
// Minimal
let media = MediaInfo::new("https://example.com/v.mp4", "video/mp4");

// With metadata and duration
let media = MediaInfo::new(url, "video/mp4")
    .stream_type(StreamType::Buffered)
    .duration(3600.0)
    .metadata(MediaMetadata::Movie {
        title: Some("Title".into()),
        subtitle: None,
        studio: None,
        images: vec![],
    });

// Convenience constructors
let media = MediaInfo::movie(url, "video/mp4", "My Movie");
let media = MediaInfo::live(url, "application/x-mpegURL");
```

### Metadata types

| Type | Fields |
|------|--------|
| `Generic` | title, subtitle, images |
| `Movie` | title, subtitle, studio, images |
| `TvShow` | series_title, episode_title, season, episode, images |
| `MusicTrack` | title, artist, album_name, composer, track_number, disc_number, images |
| `Photo` | title, artist, location, latitude, longitude, width, height, images |
| `AudiobookChapter` | book_title, chapter_title, chapter_number, subtitle, images |

---

## Queue management

```rust
use oxicast::{QueueItem, RepeatMode};

let items = vec![
    QueueItem { media: MediaInfo::new(url1, "video/mp4"), autoplay: true, start_time: 0.0 },
    QueueItem { media: MediaInfo::new(url2, "video/mp4"), autoplay: true, start_time: 0.0 },
];

client.queue_load(&items, 0, RepeatMode::RepeatAll).await?;
client.queue_insert(&more_items, Some(1)).await?; // insert before index 1
```

| Method | Returns | Description |
|--------|---------|-------------|
| `queue_load(items, start_index, repeat_mode)` | `Result<MediaStatus>` | Load a queue |
| `queue_insert(items, insert_before)` | `Result<MediaStatus>` | Insert items into the queue |

---

## Events

```rust
// Event stream (single-consumer, bounded, non-blocking)
while let Some(event) = client.next_event().await {
    match event { /* ... */ }
}

// Watch channels (always-fresh latest state, multi-consumer)
let media_rx = client.watch_media_status();     // watch::Receiver<Option<MediaStatus>>
let receiver_rx = client.watch_receiver_status(); // watch::Receiver<Option<ReceiverStatus>>
```

### Event variants

| Event | When |
|-------|------|
| `Connected` | Initial connection established |
| `Disconnected(Option<String>)` | Connection lost (with optional reason) |
| `Reconnecting { attempt }` | Auto-reconnect in progress |
| `Reconnected` | Successfully reconnected |
| `HeartbeatTimeout` | Device stopped responding to PINGs |
| `ReceiverStatusChanged(ReceiverStatus)` | Volume, apps, or device state changed |
| `MediaStatusChanged(MediaStatus)` | Playback state, position, or media changed |
| `MediaSessionEnded { media_session_id, idle_reason }` | A media session ended |
| `RawMessage { namespace, source, destination, payload }` | Unhandled or custom namespace message |

---

## Raw / custom messages

For custom namespaces or commands not covered by the typed API:

```rust
// With request-response correlation (requestId injected automatically)
let response = client.send_raw(
    "urn:x-cast:com.example.custom",
    &app.transport_id,
    serde_json::json!({"action": "doThing"}),
).await?;

// Fire-and-forget
client.send_raw_no_reply(
    "urn:x-cast:com.example.custom",
    &transport_id,
    serde_json::json!({"type": "NOTIFY"}),
).await?;
```

Custom namespace messages without a `requestId` arrive as `CastEvent::RawMessage`.

---

## Discovery

Requires the `discovery` feature (enabled by default).

```rust
// Batch — collect all devices within timeout
let devices = oxicast::discovery::discover_devices(Duration::from_secs(3)).await?;

// Stream — yield devices as they're found
let mut stream = oxicast::discovery::discover(Duration::from_secs(5))?;
while let Some(device) = stream.recv().await {
    println!("{} ({})", device.name, device.model.as_deref().unwrap_or("unknown"));
}
```

### DeviceInfo

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | Friendly name (e.g. "Living Room TV") |
| `ip` | `IpAddr` | Device IP address |
| `port` | `u16` | Cast port (usually 8009) |
| `model` | `Option<String>` | Model name (e.g. "Chromecast") |
| `uuid` | `Option<String>` | Device UUID |

---

## Local file casting

Requires the `serve` feature.

```rust
// One-call convenience
let (server, status) = client.serve_and_cast("/path/to/video.mp4", "video/mp4", true, 0.0).await?;

// Manual setup
use oxicast::serve::FileServer;
let server = FileServer::start("0.0.0.0:0").await?;
let url = server.serve_file("/path/to/video.mp4", "video/mp4")?;
client.load_media(&MediaInfo::new(&url, "video/mp4"), true, 0.0, None).await?;
```

The `FileServer` must be kept alive for the duration of playback. Dropping it stops the HTTP server.
