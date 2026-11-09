# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.4] - 2026-11-09

### Added
- `CastClient::join_session` to attach to an application that is already running on the device, e.g. to control media started from another sender or resume a previous session. It fetches and returns the current `MediaStatus` so playback controls work immediately ([#22](https://github.com/denniskribl/oxicast/pull/22), [#19](https://github.com/denniskribl/oxicast/issues/19)). Thanks @Mossop!
- `DeviceInfo::capabilities` exposing the device's audio/video output capabilities from the mDNS `ca` record, plus the new `Capability` enum ([#20](https://github.com/denniskribl/oxicast/pull/20)). Thanks @Mossop!
- `MediaInfo::content_url` to set the `contentUrl` property in LOAD and QUEUE_LOAD/QUEUE_INSERT requests ([#21](https://github.com/denniskribl/oxicast/pull/21)). Thanks @Mossop!
- `CastClientBuilder::connect_timeout` (default 10s) for the TCP connect + TLS handshake.
- `CastClient::close()`: transport-only shutdown that closes the app and receiver channels without sending a media STOP, so playback (e.g. of a joined session) keeps running. `disconnect()` is unchanged and still stops media.

### Fixed
- `disconnect()` could drop its final STOP/CLOSE messages because the writer task was aborted before flushing its queue; shutdown now flushes queued messages (bounded to 500 ms) and closes TLS cleanly.
- MEDIA_STATUS entries that only carry `contentUrl` (e.g. media started by other senders) are now parsed instead of yielding `media: None`.
- Connecting and heartbeat detection can no longer hang indefinitely ([#22](https://github.com/denniskribl/oxicast/pull/22)). Thanks @Mossop!
- `tokio-rustls` no longer enables its default `aws-lc-rs` feature, so dependents don't pull in `aws-lc-sys` even though `ring` is the only backend used ([#25](https://github.com/denniskribl/oxicast/pull/25)). Thanks @vyrti!

### Security
- Raised the minimum `rustls` to 0.23.46 (RUSTSEC-2026-0285, RUSTSEC-2026-0346, RUSTSEC-2026-0348 and related `rustls-webpki` advisories).

### Changed
- **Breaking:** `MediaInfo` and `DeviceInfo` have new public fields; code constructing them with struct literals needs to set `content_url` / `capabilities`.
- Dependency updates: `mdns-sd` 0.20, `tokio` 1.52, `axum` 0.8.9, `serde_json` 1.0.150; CI actions bumped (`actions/checkout` v6, `actions/deploy-pages` v5, `cargo-deny-action` 2.0.17).

## [0.0.3] - 2026-04-09

### Changed
- `proto/` source definitions are no longer shipped in the published crate.
- Cast protobuf bindings are now consumed from checked-in `src/proto_gen.rs` at build time.
- CI/release setup no longer requires `protoc` for normal build/test/publish flows.

## [0.0.2] - 2026-04-09

### Changed
- **Breaking:** `CastClient::load_media` now takes a fourth `custom_data: Option<&serde_json::Value>` argument for Custom Web Receiver integrations.
- Added support to pass `customData` in LOAD requests (falls back to `{}` when omitted).
- Updated examples, docs, and API snippets to use the new `load_media(..., None)` call shape.

## [0.0.1] - 2026-04-08

### Added
- Async Cast protocol client built on tokio
- Split-stream reactor architecture (reader/writer/heartbeat tasks)
- mDNS device discovery (`discover_devices`, `discover` streaming)
- Media control: load, play, pause, seek, stop, volume, mute
- Queue management: queue_load, queue_insert with all repeat modes
- Receiver control: launch app, stop app, get status
- Automatic heartbeat management (PING/PONG)
- Auto-reconnect with exponential backoff, centered jitter, serialized mutex
- Request-response correlation via requestId
- Reactive status model via `tokio::sync::watch` channels
- Event stream via bounded mpsc (non-blocking `try_send`)
- Custom namespace support via `send_raw()` / `send_raw_no_reply()`
- All 6 metadata types (Generic, Movie, TvShow, MusicTrack, Photo, AudiobookChapter)
- Typed error enum with device error surfacing (`LOAD_FAILED`, `INVALID_REQUEST`, etc.)
- Local file casting via built-in HTTP server (`serve` feature)
- `serve_and_cast()` one-call convenience method
- TLS with self-signed cert support (rustls, `verify_tls` option)
- Deterministic task lifecycle: `CancellationToken` hierarchy, `Drop` impl on `ClientInner`
- `CastClient` is `Clone + Send + Sync`
- `#![warn(missing_docs)]`, `#![deny(unsafe_code)]`
- 5 examples: discover_and_play, media_control, event_monitor, custom_namespace, device_test
- 145 tests (84 unit + 54 integration + 7 doctests)
- Astro Starlight documentation site
