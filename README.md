<p align="center">
  <h1 align="center">🛰️ oxicast</h1>
  <p align="center">
    Async Google Cast (Chromecast) client for Rust — discover, connect, and control Cast devices with tokio.
  </p>
</p>

<p align="center">
  <a href="https://crates.io/crates/oxicast"><img src="https://img.shields.io/crates/v/oxicast.svg" alt="crates.io"></a>
  <a href="https://docs.rs/oxicast"><img src="https://img.shields.io/docsrs/oxicast" alt="docs.rs"></a>
  <a href="https://github.com/denniskribl/oxicast/actions"><img src="https://github.com/denniskribl/oxicast/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://codecov.io/gh/denniskribl/oxicast"><img src="https://codecov.io/gh/denniskribl/oxicast/branch/main/graph/badge.svg" alt="Coverage"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue" alt="License"></a>
</p>

<p align="center">
  <a href="#-quickstart">Quickstart</a> ·
  <a href="https://docs.rs/oxicast">API Docs</a> ·
  <a href="./examples">Examples</a> ·
  <a href="./docs">Guide</a>
</p>

---

## What is this?

A fully async Rust client for the Google Cast protocol (Chromecast, Google TV, Cast-enabled speakers). Connect by IP or discover devices via mDNS, launch apps, load media, control playback, and react to device events — all through a clean `async`/`await` API built on [tokio](https://tokio.rs).

```rust
let client = CastClient::connect("192.168.1.100", 8009).await?;
client.launch_app(&CastApp::DefaultMediaReceiver).await?;
client.load_media(&MediaInfo::new(url, "video/mp4"), true, 0.0, None).await?;
client.pause().await?;
client.seek(60.0).await?;
client.play().await?;
```

---

<a id="-quickstart"></a>

## 🚀 Quickstart

### 1. Install

```toml
[dependencies]
oxicast = "0.0.2"
tokio = { version = "1", features = ["full"] }
```

### 2. Connect and play

```rust
use oxicast::{CastClient, CastApp, CastEvent, MediaInfo};
use std::time::Duration;

#[tokio::main]
async fn main() -> oxicast::Result<()> {
    // Discover devices on the network
    let devices = oxicast::discovery::discover_devices(Duration::from_secs(3)).await?;
    let device = match devices.first() {
        Some(d) => d,
        None => { eprintln!("No devices found"); return Ok(()); }
    };
    println!("Found: {} at {}", device.name, device.ip);

    // Connect and play
    let client = CastClient::connect(&device.ip.to_string(), device.port).await?;
    client.launch_app(&CastApp::DefaultMediaReceiver).await?;
    client.load_media(
        &MediaInfo::new("https://example.com/video.mp4", "video/mp4"),
        true, 0.0,
        None,
    ).await?;

    // React to events
    loop {
        tokio::select! {
            Some(event) = client.next_event() => match event {
                CastEvent::MediaStatusChanged(s) => {
                    println!("{:?} at {:.1}s", s.player_state, s.current_time);
                }
                CastEvent::Disconnected(_) => break,
                _ => {}
            },
            _ = tokio::signal::ctrl_c() => {
                client.disconnect().await?;
                break;
            }
        }
    }
    Ok(())
}
```

Or connect directly by IP — discovery is optional:

```rust
let client = CastClient::connect("192.168.1.100", 8009).await?;
```

---

## ✨ Features

| Feature | Description |
|---------|-------------|
| 🔌 **Async-native** | Built on tokio — full `async`/`await`, no blocking calls |
| 💓 **Auto heartbeat** | PING/PONG managed invisibly in the background |
| ⚡ **Instant commands** | Separate reader/writer tasks — commands never wait for the read loop |
| 📊 **Reactive status** | `watch` channels for always-fresh media and receiver state |
| 🔄 **Auto-reconnect** | Exponential backoff with jitter, serialized with manual reconnect |
| 🔍 **mDNS discovery** | Scan the network or stream devices as they appear |
| 🛡️ **Typed errors** | Structured enum for connection, protocol, and media failures |
| 🧵 **Thread-safe** | `Clone + Send + Sync` — share across tasks freely |
| 📁 **Local file casting** | Built-in HTTP server serves files to Chromecast (opt-in `serve` feature) |

### Feature flags

| Flag | Default | What it adds |
|------|---------|--------------|
| `discovery` | ✅ | mDNS device scanning via `mdns-sd` |
| `serve` | ❌ | HTTP file server for casting local files |

```toml
# Disable discovery, enable local file serving
oxicast = { version = "0.0.2", default-features = false, features = ["serve"] }
```

---

## 📖 Documentation

| Document | Description |
|----------|-------------|
| **[Getting Started](docs/getting-started.md)** | Connect, discover, play media, handle events |
| **[API Overview](docs/api-overview.md)** | Full API surface with code examples |
| **[Architecture](docs/architecture.md)** | Split-stream reactor, task lifecycle, reconnect, event delivery |
| **[Error Handling](docs/error-handling.md)** | Error enum, device errors, timeouts |
| **[CHANGELOG](CHANGELOG.md)** | Version history |

---

## 📂 Examples

| Example | What it does |
|---------|-------------|
| [`discover_and_play`](./examples/discover_and_play.rs) | mDNS scan, connect, load HLS stream, monitor events |
| [`media_control`](./examples/media_control.rs) | Pause, seek, volume, resume |
| [`event_monitor`](./examples/event_monitor.rs) | Print all events in real-time |
| [`custom_namespace`](./examples/custom_namespace.rs) | Send/receive on custom namespaces |
| [`device_test`](./examples/device_test.rs) | 14-step integration test against real hardware |

```sh
# Run the device test (auto-discovers or pass IP)
cargo run --example device_test --all-features
cargo run --example device_test --all-features -- 192.168.1.100

# With protocol-level tracing
RUST_LOG=oxicast=trace cargo run --example device_test --all-features
```

---

## License

Licensed under either of [Apache License 2.0](./LICENSE-APACHE) or [MIT](./LICENSE-MIT) at your option.

---

## Trademarks

Google Cast and Chromecast are trademarks of Google LLC. This project is not affiliated with or endorsed by Google.
