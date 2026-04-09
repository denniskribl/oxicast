# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
