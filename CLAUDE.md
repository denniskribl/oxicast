# CLAUDE.md

## What is this?

oxicast is an async Google Cast (Chromecast) client library for Rust, built on tokio. It provides device discovery, media control, and real-time status updates over the Cast v2 protocol.

## Build

```sh
# Requires protoc (protobuf compiler)
brew install protobuf        # macOS
sudo apt install protobuf-compiler  # Linux

cargo check
cargo test --all-features
```

## Quality gates (all must pass before any commit)

```sh
cargo check --all-features
cargo check --no-default-features
cargo check --examples --all-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps
cargo deny check
```

## Architecture

Split-stream reactor pattern. Three tokio tasks per connection:
- **Reader** — reads TLS, routes messages via `router.rs`, updates reactive state
- **Writer** — sends messages from mpsc channel, never blocks reader
- **Heartbeat** — automatic PING/PONG, transparent to user

All tasks use `CancellationToken` for deterministic shutdown. `TaskHandles` stores `JoinHandle`s for abort+join on reconnect/disconnect.

## Module layout

```
src/
  lib.rs              — re-exports, crate docs
  error.rs            — Error enum
  event.rs            — CastEvent enum
  types.rs            — MediaInfo, Volume, PlayerState, etc.
  state.rs            — Reactive watch channels (Arc<StateHolder>)
  client.rs           — CastClient + background task spawning
  client/
    builder.rs        — CastClientBuilder
    connection.rs     — TLS connect (NoCertVerification)
    framing.rs        — 4-byte length prefix + protobuf encode/decode
    heartbeat.rs      — PING/PONG task
    request_tracker.rs — requestId → oneshot correlation
    router.rs         — namespace dispatch + JSON parsing
  channel/
    ns.rs             — ALL protocol constants (namespaces, message types, IDs)
    connection.rs     — CONNECT/CLOSE messages
    receiver.rs       — LAUNCH/STOP/VOLUME messages
    media.rs          — LOAD/PLAY/PAUSE/SEEK/QUEUE messages
  discovery.rs        — mDNS (behind "discovery" feature)
  serve.rs            — Local HTTP file server (behind "serve" feature)
```

## Key design decisions

- **tokio-only** — not runtime-agnostic. The entire Rust async ecosystem is tokio.
- **prost** for protobuf — not the `protobuf` crate. Generates idiomatic Rust.
- **CancellationToken** for task lifecycle — not bare AtomicBool flags.
- **Arc<StateHolder>** shared between client and reader — survives reconnect.
- **`alive` AtomicBool** set false by reader/writer on I/O failure — triggers auto-reconnect.
- **`shutting_down` AtomicBool** prevents auto-reconnect after user calls `disconnect()`.
- **All magic strings in `channel/ns.rs`** — no hardcoded protocol strings elsewhere.
- **Metadata types 0-5** from official Google Cast SDK docs (Generic, Movie, TvShow, MusicTrack, Photo, AudiobookChapter).

## Protocol reference

- No official spec. Reverse-engineered from Chromium Open Screen project.
- Protobuf: `proto/cast_channel.proto` from `chromium.googlesource.com/openscreen`
- TLS on port 8009, self-signed certs (skip verification by default).
- 4-byte big-endian length prefix + protobuf CastMessage.
- JSON payloads inside protobuf string field.
- Namespaces: connection, heartbeat, receiver, media (all in `ns.rs`).

## Testing

```sh
# Unit + integration tests
cargo test --all-features

# Real device test (requires Chromecast on LAN)
CAST_IP=192.168.1.5 cargo run --example discover_and_play

# With custom media URL
CAST_IP=192.168.1.5 CAST_URL=http://192.168.1.10:9999/video.mp4 cargo run --example discover_and_play
```

## Do NOT

- Add `unsafe` code.
- Use `unwrap()` in library code (examples are OK). Use typed errors or `unwrap_or`.
- Add runtime-agnostic abstractions — tokio is the runtime.
- Hardcode protocol strings outside `channel/ns.rs`.
- Remove the `#![allow(dead_code)]` on the proto module — generated code has unused items.
- Expose `heartbeat_timeout` publicly until timeout detection is implemented.
