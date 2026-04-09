---
title: Error Handling
---

# 🛡️ Error Handling

All public methods return `oxicast::Result<T>`, which is `Result<T, oxicast::Error>`.

---

## Error variants

| Variant | When |
|---------|------|
| `Connect(io::Error)` | TCP connection to the device failed |
| `Tls(String)` | TLS handshake failed |
| `Disconnected` | Connection closed (by device or network) |
| `Framing(String)` | Wire-level framing error (corrupt message, oversized, etc.) |
| `Protobuf(prost::DecodeError)` | Failed to decode a protobuf message |
| `Json(serde_json::Error)` | Failed to parse/serialize JSON |
| `Timeout(Duration)` | Request timed out waiting for device response |
| `LaunchFailed { reason }` | Device refused to launch the app |
| `LoadFailed { reason, detailed_error }` | Device refused to load media |
| `InvalidRequest { request_id, reason }` | Device rejected a command (e.g. invalid media session) |
| `InvalidPayload` | Payload passed to `send_raw()` is not a JSON object |
| `NoMediaSession` | No active media session (call `load_media()` first) |
| `NoApplication` | No running app (call `launch_app()` first) |
| `Discovery(String)` | mDNS discovery failed |
| `FileNotFound(String)` | File not found (for the `serve` feature) |
| `Internal(String)` | Internal error (should not happen in normal use) |

---

## Device errors

When the Cast device rejects a command, oxicast surfaces the device's error type directly. Every public method calls `check_device_error()` on the response before parsing:

```rust
match client.load_media(&media, true, 0.0, None).await {
    Ok(status) => {
        println!("Playing: {:?}", status.player_state);
    }
    Err(oxicast::Error::LoadFailed { reason, detailed_error }) => {
        // The Chromecast rejected the media
        println!("Load failed: {reason}");
        if let Some(code) = detailed_error {
            println!("Detail: {code}");
        }
    }
    Err(oxicast::Error::InvalidRequest { request_id, reason }) => {
        // The device said this request doesn't make sense
        println!("Invalid request {request_id}: {reason}");
    }
    Err(oxicast::Error::Timeout(d)) => {
        println!("No response within {d:?}");
    }
    Err(oxicast::Error::Disconnected) => {
        println!("Connection lost");
    }
    Err(e) => {
        println!("Other error: {e}");
    }
}
```

---

## Timeouts

Every request-response operation has a configurable timeout (default 10s). If the device doesn't respond within that window, `Error::Timeout` is returned and the pending request tracker entry is cleaned up.

```rust
let client = CastClient::builder("192.168.1.100", 8009)
    .request_timeout(Duration::from_secs(5))
    .connect()
    .await?;
```

---

## Connection errors

Connection lifecycle errors are communicated through both the event stream and the watch channel:

```rust
// Via events
if let CastEvent::Disconnected(reason) = event {
    println!("Lost connection: {reason:?}");
}

// Via watch channel (always up-to-date)
if client.connection_state() == ConnectionState::Disconnected {
    println!("Not connected");
}
```

If auto-reconnect is enabled (default), the client will attempt to recover automatically. You'll see `Reconnecting { attempt }` events followed by either `Reconnected` or `Disconnected("reconnect failed")`.
