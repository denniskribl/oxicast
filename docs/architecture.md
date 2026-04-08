---
title: Architecture
---

# 🏗️ Architecture

This document explains how oxicast works internally.

---

## Overview

oxicast uses a **split-stream reactor** — three background tokio tasks handle the Cast protocol concurrently:

```
CastClient (your code)
    |
    |-- send() -----> [write_tx mpsc] --> Writer Task --> TLS WriteHalf --> Device
    |                                                                         |
    |                                     TLS ReadHalf <----------------------+
    |                                          |
    |<-- next_event()  <-- [event_tx mpsc] <-- Router <-- Reader Task
    |<-- watch_*()     <-- [watch channels] <--+
    |
    +-- Heartbeat Task (PING every 5s, timeout detection)
```

This design means:

- **Commands are instant.** `pause()`, `seek()`, `set_volume()` go through the Writer and never wait for the Reader.
- **Heartbeats are invisible.** The Heartbeat Task sends PINGs and the Reader auto-replies with PONGs. Your code never sees them.
- **Status is always fresh.** The Router updates `watch` channels on every inbound status message, regardless of whether anyone calls `next_event()`.

---

## Tasks

### Reader Task

Owns the TLS read half. Loops forever, reading length-prefixed protobuf messages (4-byte big-endian u32 + payload, max 64KB). Each message is decoded and passed to the Router.

On I/O error or EOF:
1. Sets `alive = false`
2. Sends `CastEvent::Disconnected` via `try_send` (non-blocking)
3. Sends `ConnectionState::Disconnected` to the watch channel
4. Clears all pending request trackers
5. Exits — this triggers auto-reconnect

On cancellation (during `disconnect()` or `reconnect()`):
- Does **not** emit Disconnected — the caller manages the state transition.

### Writer Task

Owns the TLS write half. Receives outbound messages from a 64-deep mpsc channel. Each message is protobuf-encoded, length-prefixed, and written as a single TLS record.

On write error:
1. Sets `alive = false`
2. Sends `ConnectionState::Disconnected` to the watch channel
3. Exits — triggers auto-reconnect

### Heartbeat Task

Sends PING messages on a configurable interval (default 5s). After each PING, checks the `last_activity` timestamp (monotonic `Instant`, updated by the Reader on every inbound message). If the elapsed time exceeds the timeout (default 15s):

1. Sets `alive = false`
2. Sends `ConnectionState::Disconnected` to the watch channel
3. Sends `CastEvent::HeartbeatTimeout` via `try_send`
4. Exits — triggers auto-reconnect

---

## Request-Response Correlation

Cast protocol commands use a `requestId` field in JSON payloads. oxicast tracks these with `RequestTracker`:

1. **Register**: Allocate an atomic u32 ID, insert a `oneshot::Sender` into a `HashMap<u32, Sender>`
2. **Send**: Include the ID in the outbound JSON
3. **Route**: When the Reader receives a response with a matching `requestId`, the Router removes the entry and sends the JSON through the oneshot
4. **Wait**: The client awaits the oneshot with a configurable timeout

If the timeout expires or the connection drops, the pending entry is cleaned up to prevent leaks.

---

## Event Delivery

Events are delivered through two independent mechanisms:

### Event channel (`next_event()`)

A bounded mpsc channel (default 64). The Router calls `try_send()` — if the buffer is full, the event is dropped. This guarantees the Reader is never blocked by a slow consumer.

After `disconnect()`, the sender is replaced with a dead channel, so `next_event()` returns `None` after draining buffered events.

After `reconnect()`, a fresh channel pair is created and swapped in atomically.

### Watch channels (`watch_media_status()`, `watch_receiver_status()`)

`tokio::sync::watch` channels always hold the latest value. Updated on every inbound `MEDIA_STATUS` and `RECEIVER_STATUS` message. These are never lossy — the latest state is always available regardless of consumer speed.

---

## Auto-Reconnect

When `ConnectionState::Disconnected` is observed on the watch channel (from any of the three task exit paths):

1. Wait with exponential backoff + jitter (wall-clock nanos for entropy)
2. Establish a new TLS connection
3. Create fresh write/event channels and swap them into the client
4. Spawn new Reader, Writer, and Heartbeat tasks
5. Re-send `CONNECT` to `receiver-0` and the previous app transport (if any)
6. Emit `CastEvent::Reconnected`

**Reconnect serialization**: A dedicated mutex ensures that manual `reconnect()` and auto-reconnect don't race. If one reconnect succeeds while another is waiting for the lock, the second one detects `alive == true` and returns immediately.

**Exhaustion**: After `max_reconnect_attempts` failures, the task emits `Disconnected("reconnect failed")`, sets `ConnectionState::Disconnected`, and exits permanently.

**Weak reference**: The auto-reconnect task holds a `Weak<ClientInner>`, not a strong `Arc`. When all user-held `CastClient` handles are dropped, the `Weak` upgrade fails and the task exits cleanly. No Arc cycle, no leaked connection.

---

## Connection Lifecycle

```
connect() -----> Connected
                     |
              (I/O error | heartbeat timeout | write failure)
                     |
                     v
               Disconnected
                     |
              (auto-reconnect or manual reconnect())
                     |
                     v
             Reconnecting { attempt: 1 }
                     |
              (TLS connect + CONNECT message)
                     |
                 success ──────> Connected
                     |
                 failure ──────> Reconnecting { attempt: 2 }
                     |                    ...
              (max attempts)
                     |
                     v
               Disconnected (permanent)
```

---

## TLS

Cast devices use self-signed certificates. Certificate verification is **disabled by default** — the connection is encrypted but not authenticated. This is the same trade-off made by every Cast client library across languages.

Set `verify_tls(true)` on the builder if your device has a CA-signed certificate (uncommon).

---

## Protocol Subset

oxicast implements the Cast v2 JSON control protocol:

| Namespace | Supported |
|-----------|-----------|
| `urn:x-cast:com.google.cast.tp.connection` | CONNECT, CLOSE |
| `urn:x-cast:com.google.cast.tp.heartbeat` | PING, PONG |
| `urn:x-cast:com.google.cast.receiver` | LAUNCH, STOP, GET_STATUS, SET_VOLUME |
| `urn:x-cast:com.google.cast.media` | LOAD, PLAY, PAUSE, SEEK, STOP, QUEUE_LOAD, QUEUE_INSERT |
| Custom namespaces | Via `send_raw()` / `CastEvent::RawMessage` |

**Not implemented**: Binary payloads (`payload_binary`), message chunking (`continued`/`remaining_length`), device authentication (`urn:x-cast:com.google.cast.tp.deviceauth`). These are not used by standard Cast media control.
