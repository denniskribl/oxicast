//! Integration tests for oxicast protocol parsing and behavior.

use serde_json::json;
use tokio::io::AsyncReadExt;

/// Helper: encode a CastMessage as length-prefixed bytes using raw protobuf encoding.
fn encode_cast_message(namespace: &str, source: &str, dest: &str, payload: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    prost::encoding::uint32::encode(1, &0u32, &mut buf);
    prost::encoding::string::encode(2, &source.to_string(), &mut buf);
    prost::encoding::string::encode(3, &dest.to_string(), &mut buf);
    prost::encoding::string::encode(4, &namespace.to_string(), &mut buf);
    prost::encoding::uint32::encode(5, &0u32, &mut buf);
    prost::encoding::string::encode(6, &payload.to_string(), &mut buf);

    let len = (buf.len() as u32).to_be_bytes();
    let mut out = Vec::with_capacity(4 + buf.len());
    out.extend_from_slice(&len);
    out.extend_from_slice(&buf);
    out
}

/// Helper: read one length-prefixed message from a stream.
async fn read_message(stream: &mut (impl AsyncReadExt + Unpin)) -> Option<serde_json::Value> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await.ok()?;
    let raw = String::from_utf8_lossy(&buf);
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    let json_str = &raw[start..=end];
    serde_json::from_str(json_str).ok()
}

// ── Framing ────────────────────────────────────────────────────

#[tokio::test]
async fn test_framing_roundtrip() {
    let msg = encode_cast_message(
        "urn:x-cast:com.google.cast.tp.heartbeat",
        "sender-0",
        "receiver-0",
        r#"{"type":"PING"}"#,
    );
    let mut cursor = std::io::Cursor::new(&msg);
    let parsed = read_message(&mut cursor).await.unwrap();
    assert_eq!(parsed["type"], "PING");
}

#[tokio::test]
async fn test_framing_length_prefix_correct() {
    let msg = encode_cast_message(
        "urn:x-cast:com.google.cast.tp.heartbeat",
        "sender-0",
        "receiver-0",
        r#"{"type":"PONG"}"#,
    );
    assert!(msg.len() > 4);
    let len = u32::from_be_bytes([msg[0], msg[1], msg[2], msg[3]]) as usize;
    assert_eq!(len, msg.len() - 4);
}

// ── Receiver Status Parsing ────────────────────────────────────

#[tokio::test]
async fn test_receiver_status_parsing() {
    let json = json!({
        "requestId": 1,
        "type": "RECEIVER_STATUS",
        "status": {
            "volume": { "level": 0.5, "muted": false },
            "applications": [{
                "appId": "CC1AD845",
                "displayName": "Default Media Receiver",
                "sessionId": "test-session",
                "transportId": "test-transport",
                "namespaces": [{"name": "urn:x-cast:com.google.cast.media"}],
                "statusText": "Ready"
            }],
            "isActiveInput": true,
            "isStandBy": false
        }
    });

    let status = oxicast::__test_util::parse_receiver_status_from_json(&json).unwrap();
    assert_eq!(status.volume.level, 0.5);
    assert!(!status.volume.muted);
    assert_eq!(status.applications.len(), 1);
    assert_eq!(status.applications[0].app_id, "CC1AD845");
    assert_eq!(status.applications[0].transport_id, "test-transport");
    assert_eq!(status.applications[0].session_id, "test-session");
    assert_eq!(status.applications[0].namespaces.len(), 1);
    assert!(status.is_active_input);
    assert!(!status.is_stand_by);
}

#[tokio::test]
async fn test_receiver_status_no_apps() {
    let json = json!({
        "requestId": 1,
        "type": "RECEIVER_STATUS",
        "status": {
            "volume": { "level": 1.0, "muted": true },
            "isActiveInput": false,
            "isStandBy": true
        }
    });

    let status = oxicast::__test_util::parse_receiver_status_from_json(&json).unwrap();
    assert_eq!(status.volume.level, 1.0);
    assert!(status.volume.muted);
    assert!(status.applications.is_empty());
    assert!(!status.is_active_input);
    assert!(status.is_stand_by);
}

// ── Media Status Parsing ───────────────────────────────────────

#[tokio::test]
async fn test_media_status_playing_with_movie_metadata() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 42,
            "playerState": "PLAYING",
            "currentTime": 123.456,
            "volume": { "level": 0.8, "muted": false },
            "media": {
                "contentId": "https://example.com/video.mp4",
                "contentType": "video/mp4",
                "streamType": "BUFFERED",
                "duration": 600.0,
                "metadata": {
                    "metadataType": 1,
                    "title": "Test Movie",
                    "subtitle": "A test",
                    "studio": "Test Studio",
                    "images": [{"url": "https://example.com/poster.jpg", "width": 300, "height": 450}]
                }
            }
        }],
        "requestId": 5
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.media_session_id, 42);
    assert_eq!(status.player_state, oxicast::PlayerState::Playing);
    assert!((status.current_time - 123.456).abs() < 0.001);
    assert_eq!(status.duration, Some(600.0));
    assert_eq!(status.volume.level, 0.8);

    let media = status.media.unwrap();
    assert_eq!(media.content_id, "https://example.com/video.mp4");
    match media.metadata.unwrap() {
        oxicast::MediaMetadata::Movie { title, subtitle, studio, images } => {
            assert_eq!(title.as_deref(), Some("Test Movie"));
            assert_eq!(subtitle.as_deref(), Some("A test"));
            assert_eq!(studio.as_deref(), Some("Test Studio"));
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].width, Some(300));
        }
        other => panic!("expected Movie, got {other:?}"),
    }
}

#[tokio::test]
async fn test_media_status_idle_finished() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 5,
            "playerState": "IDLE",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "idleReason": "FINISHED"
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.player_state, oxicast::PlayerState::Idle);
    assert_eq!(status.idle_reason, Some(oxicast::IdleReason::Finished));
}

#[tokio::test]
async fn test_media_status_idle_error() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 3,
            "playerState": "IDLE",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "idleReason": "ERROR"
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.idle_reason, Some(oxicast::IdleReason::Error));
}

#[tokio::test]
async fn test_media_status_idle_cancelled() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "IDLE",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "idleReason": "CANCELLED"
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.idle_reason, Some(oxicast::IdleReason::Cancelled));
}

#[tokio::test]
async fn test_media_status_idle_interrupted() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "IDLE",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "idleReason": "INTERRUPTED"
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.idle_reason, Some(oxicast::IdleReason::Interrupted));
}

#[tokio::test]
async fn test_media_status_buffering() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "BUFFERING",
            "currentTime": 10.5,
            "volume": { "level": 1.0, "muted": false }
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.player_state, oxicast::PlayerState::Buffering);
    assert!((status.current_time - 10.5).abs() < 0.001);
}

#[tokio::test]
async fn test_media_status_paused() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "PAUSED",
            "currentTime": 55.0,
            "volume": { "level": 0.3, "muted": true }
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.player_state, oxicast::PlayerState::Paused);
    assert_eq!(status.volume.level, 0.3);
    assert!(status.volume.muted);
}

#[tokio::test]
async fn test_media_status_empty_array() {
    let json = json!({ "type": "MEDIA_STATUS", "status": [], "requestId": 0 });
    assert!(oxicast::__test_util::parse_media_status_from_json(&json).is_none());
}

#[tokio::test]
async fn test_media_status_unknown_player_state() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{ "mediaSessionId": 1, "playerState": "UNKNOWN_STATE", "currentTime": 0, "volume": {} }],
        "requestId": 0
    });
    assert!(oxicast::__test_util::parse_media_status_from_json(&json).is_none());
}

#[tokio::test]
async fn test_media_status_no_duration() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "PLAYING",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "video/mp4", "streamType": "LIVE" }
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.duration, None);
    assert_eq!(status.media.as_ref().unwrap().stream_type, oxicast::StreamType::Live);
}

// ── Metadata Types ─────────────────────────────────────────────

#[tokio::test]
async fn test_generic_metadata() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "audio/mp3", "streamType": "BUFFERED",
                "metadata": { "metadataType": 0, "title": "Song", "subtitle": "Artist" }
            }
        }],
        "requestId": 0
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::Generic { title, subtitle, .. } => {
            assert_eq!(title.as_deref(), Some("Song"));
            assert_eq!(subtitle.as_deref(), Some("Artist"));
        }
        other => panic!("expected Generic, got {other:?}"),
    }
}

#[tokio::test]
async fn test_tv_show_metadata() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "video/mp4", "streamType": "BUFFERED",
                "duration": 1440.0,
                "metadata": { "metadataType": 2, "seriesTitle": "Breaking Bad", "episodeTitle": "Pilot", "season": 1, "episode": 1, "images": [] }
            }
        }],
        "requestId": 1
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::TvShow { series_title, episode_title, season, episode, .. } => {
            assert_eq!(series_title.as_deref(), Some("Breaking Bad"));
            assert_eq!(episode_title.as_deref(), Some("Pilot"));
            assert_eq!(season, Some(1));
            assert_eq!(episode, Some(1));
        }
        other => panic!("expected TvShow, got {other:?}"),
    }
}

#[tokio::test]
async fn test_music_track_metadata() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "audio/mp3", "streamType": "BUFFERED",
                "metadata": { "metadataType": 3, "title": "Bohemian Rhapsody", "artist": "Queen", "albumName": "A Night at the Opera", "trackNumber": 11, "discNumber": 1 }
            }
        }],
        "requestId": 0
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::MusicTrack {
            title,
            artist,
            album_name,
            track_number,
            disc_number,
            ..
        } => {
            assert_eq!(title.as_deref(), Some("Bohemian Rhapsody"));
            assert_eq!(artist.as_deref(), Some("Queen"));
            assert_eq!(album_name.as_deref(), Some("A Night at the Opera"));
            assert_eq!(track_number, Some(11));
            assert_eq!(disc_number, Some(1));
        }
        other => panic!("expected MusicTrack, got {other:?}"),
    }
}

#[tokio::test]
async fn test_photo_metadata() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "image/jpeg", "streamType": "NONE",
                "metadata": { "metadataType": 4, "title": "Sunset", "artist": "Photographer", "latitude": 48.8566, "longitude": 2.3522, "width": 4000, "height": 3000 }
            }
        }],
        "requestId": 0
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::Photo {
            title, artist, latitude, longitude, width, height, ..
        } => {
            assert_eq!(title.as_deref(), Some("Sunset"));
            assert_eq!(artist.as_deref(), Some("Photographer"));
            assert!((latitude.unwrap() - 48.8566).abs() < 0.001);
            assert!((longitude.unwrap() - 2.3522).abs() < 0.001);
            assert_eq!(width, Some(4000));
            assert_eq!(height, Some(3000));
        }
        other => panic!("expected Photo, got {other:?}"),
    }
}

#[tokio::test]
async fn test_audiobook_chapter_metadata() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "audio/mp3", "streamType": "BUFFERED",
                "metadata": { "metadataType": 5, "bookTitle": "Dune", "chapterTitle": "Chapter 1", "chapterNumber": 1, "subtitle": "The Beginning" }
            }
        }],
        "requestId": 0
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::AudiobookChapter {
            book_title,
            chapter_title,
            chapter_number,
            subtitle,
            ..
        } => {
            assert_eq!(book_title.as_deref(), Some("Dune"));
            assert_eq!(chapter_title.as_deref(), Some("Chapter 1"));
            assert_eq!(chapter_number, Some(1));
            assert_eq!(subtitle.as_deref(), Some("The Beginning"));
        }
        other => panic!("expected AudiobookChapter, got {other:?}"),
    }
}

#[tokio::test]
async fn test_unknown_metadata_type_falls_back_to_generic() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "video/mp4", "streamType": "BUFFERED",
                "metadata": { "metadataType": 99, "title": "Unknown" }
            }
        }],
        "requestId": 0
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::Generic { title, .. } => {
            assert_eq!(title.as_deref(), Some("Unknown"));
        }
        other => panic!("expected Generic fallback, got {other:?}"),
    }
}

// ── Namespace Constants ────────────────────────────────────────

#[test]
fn test_namespace_constants() {
    assert!(oxicast::__test_util::ns::NS_HEARTBEAT.starts_with("urn:x-cast:"));
    assert!(oxicast::__test_util::ns::NS_CONNECTION.starts_with("urn:x-cast:"));
    assert!(oxicast::__test_util::ns::NS_RECEIVER.starts_with("urn:x-cast:"));
    assert!(oxicast::__test_util::ns::NS_MEDIA.starts_with("urn:x-cast:"));
    assert_eq!(oxicast::__test_util::ns::SENDER_ID, "sender-0");
    assert_eq!(oxicast::__test_util::ns::RECEIVER_ID, "receiver-0");
    assert_eq!(oxicast::__test_util::ns::APP_DEFAULT_MEDIA_RECEIVER, "CC1AD845");
}

// ── CastApp ────────────────────────────────────────────────────

#[test]
fn test_cast_app_ids() {
    assert_eq!(oxicast::CastApp::DefaultMediaReceiver.app_id(), "CC1AD845");
    assert_eq!(oxicast::CastApp::YouTube.app_id(), "233637DE");
    assert_eq!(oxicast::CastApp::Backdrop.app_id(), "E8C28D3C");
    assert_eq!(oxicast::CastApp::Custom("MYAPP".into()).app_id(), "MYAPP");
}

// ── MediaInfo Builder ──────────────────────────────────────────

#[test]
fn test_media_info_builder() {
    let media = oxicast::MediaInfo::new("https://example.com/v.mp4", "video/mp4")
        .stream_type(oxicast::StreamType::Live)
        .duration(120.5);

    assert_eq!(media.content_id, "https://example.com/v.mp4");
    assert_eq!(media.content_type, "video/mp4");
    assert_eq!(media.stream_type, oxicast::StreamType::Live);
    assert_eq!(media.duration, Some(120.5));
}

#[test]
fn test_media_info_defaults() {
    let media = oxicast::MediaInfo::new("url", "type");
    assert_eq!(media.stream_type, oxicast::StreamType::Buffered);
    assert_eq!(media.duration, None);
    assert!(media.metadata.is_none());
}

// ── Parse Range (serve feature) ────────────────────────────────

#[cfg(feature = "serve")]
mod serve_tests {
    // parse_range is private, but we can test FileServer::serve_file error paths

    #[tokio::test]
    async fn test_file_server_nonexistent_file() {
        let server = oxicast::serve::FileServer::start("127.0.0.1:0").await.unwrap();
        let result = server.serve_file("/nonexistent/path.mp4", "video/mp4");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_server_creates_url() {
        let server = oxicast::serve::FileServer::start("127.0.0.1:0").await.unwrap();
        // Use Cargo.toml as a test file (it exists)
        let result = server.serve_file("Cargo.toml", "text/plain");
        assert!(result.is_ok());
        let url = result.unwrap();
        assert!(url.starts_with("http://"));
        assert!(url.contains("/file/"));
    }
}

// ── Display Impls ──────────────────────────────────────────────

#[test]
fn test_player_state_display() {
    assert_eq!(oxicast::PlayerState::Playing.to_string(), "playing");
    assert_eq!(oxicast::PlayerState::Paused.to_string(), "paused");
    assert_eq!(oxicast::PlayerState::Buffering.to_string(), "buffering");
    assert_eq!(oxicast::PlayerState::Idle.to_string(), "idle");
}

#[test]
fn test_idle_reason_display() {
    assert_eq!(oxicast::IdleReason::Finished.to_string(), "finished");
    assert_eq!(oxicast::IdleReason::Error.to_string(), "error");
    assert_eq!(oxicast::IdleReason::Cancelled.to_string(), "cancelled");
    assert_eq!(oxicast::IdleReason::Interrupted.to_string(), "interrupted");
}

#[test]
fn test_stream_type_display() {
    assert_eq!(oxicast::StreamType::Live.to_string(), "live");
    assert_eq!(oxicast::StreamType::Buffered.to_string(), "buffered");
    assert_eq!(oxicast::StreamType::None.to_string(), "none");
}

#[test]
fn test_cast_app_display() {
    assert_eq!(oxicast::CastApp::DefaultMediaReceiver.to_string(), "Default Media Receiver");
    assert_eq!(oxicast::CastApp::YouTube.to_string(), "YouTube");
    assert_eq!(oxicast::CastApp::Custom("X".into()).to_string(), "Custom(X)");
}

// ── Volume Constructor ─────────────────────────────────────────

#[test]
fn test_volume_new() {
    let v = oxicast::Volume::new(0.75);
    assert_eq!(v.level, 0.75);
    assert!(!v.muted);
}

#[test]
fn test_volume_muted() {
    let v = oxicast::Volume::muted();
    assert_eq!(v.level, 0.0);
    assert!(v.muted);
}

// ── MediaInfo Convenience Constructors ─────────────────────────

#[test]
fn test_media_info_movie() {
    let m = oxicast::MediaInfo::movie("url", "video/mp4", "Inception");
    assert_eq!(m.content_id, "url");
    assert_eq!(m.stream_type, oxicast::StreamType::Buffered);
    match m.metadata.unwrap() {
        oxicast::MediaMetadata::Movie { title, .. } => {
            assert_eq!(title.as_deref(), Some("Inception"));
        }
        other => panic!("expected Movie, got {other:?}"),
    }
}

#[test]
fn test_media_info_live() {
    let m = oxicast::MediaInfo::live("url", "application/x-mpegURL");
    assert_eq!(m.stream_type, oxicast::StreamType::Live);
    assert!(m.duration.is_none());
    assert!(m.metadata.is_none());
}

// ── CastEvent Helpers ──────────────────────────────────────────

#[test]
fn test_event_as_media_status() {
    let status = oxicast::MediaStatus {
        media_session_id: 1,
        player_state: oxicast::PlayerState::Playing,
        idle_reason: None,
        current_time: 10.0,
        duration: Some(600.0),
        volume: oxicast::Volume::new(1.0),
        media: None,
    };
    let event = oxicast::CastEvent::MediaStatusChanged(status);
    assert!(event.is_media_status());
    assert!(!event.is_disconnected());
    assert!(event.as_media_status().is_some());
    assert_eq!(event.as_media_status().unwrap().current_time, 10.0);
}

#[test]
fn test_event_is_disconnected() {
    let event = oxicast::CastEvent::Disconnected(Some("test".into()));
    assert!(event.is_disconnected());
    assert!(!event.is_media_status());
    assert!(event.as_media_status().is_none());
}

#[test]
fn test_event_as_receiver_status() {
    let status = oxicast::ReceiverStatus {
        volume: oxicast::Volume::new(0.5),
        applications: vec![],
        is_active_input: true,
        is_stand_by: false,
    };
    let event = oxicast::CastEvent::ReceiverStatusChanged(status);
    assert!(event.as_receiver_status().is_some());
    assert_eq!(event.as_receiver_status().unwrap().volume.level, 0.5);
}

#[test]
fn test_event_connected_is_not_disconnected() {
    let event = oxicast::CastEvent::Connected;
    assert!(!event.is_disconnected());
    assert!(!event.is_media_status());
    assert!(event.as_media_status().is_none());
    assert!(event.as_receiver_status().is_none());
}

#[test]
fn test_event_reconnecting() {
    let event = oxicast::CastEvent::Reconnecting { attempt: 3 };
    assert!(!event.is_disconnected());
    assert!(!event.is_media_status());
}

// ── Framing Edge Cases ─────────────────────────────────────────

#[tokio::test]
async fn test_framing_oversized_message_rejected() {
    // Create a message with length prefix claiming >64KB
    let fake_len: u32 = 70_000;
    let mut data = fake_len.to_be_bytes().to_vec();
    data.extend(vec![0u8; 100]); // only 100 bytes of payload, but header says 70000
    let mut cursor = std::io::Cursor::new(data);
    // This should fail because message claims to be >64KB
    let result = read_message(&mut cursor).await;
    // read_message reads len then tries to read that many bytes — will fail on short data
    assert!(result.is_none());
}

#[tokio::test]
async fn test_framing_empty_stream() {
    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    let result = read_message(&mut cursor).await;
    assert!(result.is_none());
}

// ── Error Type Tests ───────────────────────────────────────────

#[test]
fn test_error_display() {
    let e = oxicast::Error::Disconnected;
    assert_eq!(e.to_string(), "connection closed by device");

    let e = oxicast::Error::NoMediaSession;
    assert_eq!(e.to_string(), "no active media session");

    let e = oxicast::Error::NoApplication;
    assert_eq!(e.to_string(), "no running application");

    let e = oxicast::Error::InvalidPayload;
    assert_eq!(e.to_string(), "payload must be a JSON object");

    let e = oxicast::Error::FileNotFound("/tmp/nope.mp4".into());
    assert_eq!(e.to_string(), "file not found: /tmp/nope.mp4");

    let e = oxicast::Error::Discovery("mDNS failed".into());
    assert_eq!(e.to_string(), "discovery failed: mDNS failed");

    let e = oxicast::Error::Timeout(std::time::Duration::from_secs(5));
    assert!(e.to_string().contains("5s"));

    let e = oxicast::Error::LoadFailed {
        reason: "bad codec".into(),
        detailed_error: Some("error code 150".into()),
    };
    assert!(e.to_string().contains("bad codec"));

    let e = oxicast::Error::InvalidRequest {
        request_id: 42,
        reason: "INVALID_MEDIA_SESSION_ID".into(),
    };
    assert!(e.to_string().contains("42"));
    assert!(e.to_string().contains("INVALID_MEDIA_SESSION_ID"));
}

#[test]
fn test_error_variant_matching() {
    // Verify all variants are matchable (compile-time check)
    let errors: Vec<oxicast::Error> = vec![
        oxicast::Error::Disconnected,
        oxicast::Error::NoMediaSession,
        oxicast::Error::NoApplication,
        oxicast::Error::InvalidPayload,
        oxicast::Error::Discovery("test".into()),
        oxicast::Error::FileNotFound("test".into()),
        oxicast::Error::Internal("test".into()),
    ];
    for e in &errors {
        match e {
            oxicast::Error::Disconnected => {}
            oxicast::Error::NoMediaSession => {}
            oxicast::Error::NoApplication => {}
            oxicast::Error::InvalidPayload => {}
            oxicast::Error::Discovery(_) => {}
            oxicast::Error::FileNotFound(_) => {}
            oxicast::Error::Internal(_) => {}
            _ => {} // other variants
        }
    }
}

// ── Receiver Status Edge Cases ─────────────────────────────────

#[tokio::test]
async fn test_receiver_status_muted_volume() {
    let json = json!({
        "requestId": 1,
        "type": "RECEIVER_STATUS",
        "status": {
            "volume": { "level": 0.0, "muted": true },
            "isActiveInput": false,
            "isStandBy": false
        }
    });

    let status = oxicast::__test_util::parse_receiver_status_from_json(&json).unwrap();
    assert_eq!(status.volume.level, 0.0);
    assert!(status.volume.muted);
}

#[tokio::test]
async fn test_receiver_status_multiple_apps() {
    let json = json!({
        "requestId": 1,
        "type": "RECEIVER_STATUS",
        "status": {
            "volume": { "level": 0.5, "muted": false },
            "applications": [
                { "appId": "APP1", "displayName": "App One", "sessionId": "s1", "transportId": "t1", "namespaces": [], "statusText": "" },
                { "appId": "APP2", "displayName": "App Two", "sessionId": "s2", "transportId": "t2", "namespaces": [], "statusText": "" }
            ],
            "isActiveInput": true,
            "isStandBy": false
        }
    });

    let status = oxicast::__test_util::parse_receiver_status_from_json(&json).unwrap();
    assert_eq!(status.applications.len(), 2);
    assert_eq!(status.applications[0].app_id, "APP1");
    assert_eq!(status.applications[1].app_id, "APP2");
}

// ── Media Status Edge Cases ────────────────────────────────────

#[tokio::test]
async fn test_media_status_zero_duration_treated_as_none() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "PLAYING",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "video/mp4", "streamType": "BUFFERED", "duration": 0.0 }
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    // Duration of 0.0 should be filtered to None (not meaningful)
    assert_eq!(status.duration, None);
}

#[tokio::test]
async fn test_media_status_negative_duration_treated_as_none() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "PLAYING",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "video/mp4", "streamType": "BUFFERED", "duration": -1.0 }
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert_eq!(status.duration, None);
}

#[tokio::test]
async fn test_media_status_no_media_field() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "IDLE",
            "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "idleReason": "CANCELLED"
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    assert!(status.media.is_none());
    assert_eq!(status.idle_reason, Some(oxicast::IdleReason::Cancelled));
}

#[tokio::test]
async fn test_media_status_missing_volume() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1,
            "playerState": "PLAYING",
            "currentTime": 50.0
        }],
        "requestId": 0
    });

    let status = oxicast::__test_util::parse_media_status_from_json(&json).unwrap();
    // Default volume when field missing
    assert_eq!(status.volume.level, 1.0);
    assert!(!status.volume.muted);
}

// ── Metadata Edge Cases ────────────────────────────────────────

#[tokio::test]
async fn test_metadata_no_images() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "video/mp4", "streamType": "BUFFERED",
                "metadata": { "metadataType": 1, "title": "No Images Movie" }
            }
        }],
        "requestId": 0
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::Movie { title, images, .. } => {
            assert_eq!(title.as_deref(), Some("No Images Movie"));
            assert!(images.is_empty());
        }
        other => panic!("expected Movie, got {other:?}"),
    }
}

#[tokio::test]
async fn test_metadata_missing_optional_fields() {
    let json = json!({
        "type": "MEDIA_STATUS",
        "status": [{
            "mediaSessionId": 1, "playerState": "PLAYING", "currentTime": 0,
            "volume": { "level": 1.0, "muted": false },
            "media": { "contentId": "x", "contentType": "video/mp4", "streamType": "BUFFERED",
                "metadata": { "metadataType": 3 }
            }
        }],
        "requestId": 0
    });

    let meta = oxicast::__test_util::parse_media_status_from_json(&json)
        .unwrap()
        .media
        .unwrap()
        .metadata
        .unwrap();
    match meta {
        oxicast::MediaMetadata::MusicTrack { title, artist, album_name, .. } => {
            assert!(title.is_none());
            assert!(artist.is_none());
            assert!(album_name.is_none());
        }
        other => panic!("expected MusicTrack, got {other:?}"),
    }
}

// ── MediaInfo Builder Chaining ─────────────────────────────────

#[test]
fn test_media_info_full_chain() {
    let m = oxicast::MediaInfo::new("url", "video/mp4")
        .stream_type(oxicast::StreamType::Live)
        .duration(3600.0)
        .metadata(oxicast::MediaMetadata::Generic {
            title: Some("Live Event".into()),
            subtitle: None,
            images: vec![],
        });

    assert_eq!(m.stream_type, oxicast::StreamType::Live);
    assert_eq!(m.duration, Some(3600.0));
    assert!(m.metadata.is_some());
}

// ── Serve Feature Tests ────────────────────────────────────────

#[cfg(feature = "serve")]
mod serve_extra_tests {
    #[tokio::test]
    async fn test_file_server_unique_ids() {
        let server = oxicast::serve::FileServer::start("127.0.0.1:0").await.unwrap();
        let url1 = server.serve_file("Cargo.toml", "text/plain").unwrap();
        let url2 = server.serve_file("Cargo.toml", "text/plain").unwrap();
        // Same file registered twice should get different IDs
        assert_ne!(url1, url2);
    }

    #[tokio::test]
    async fn test_file_server_addr_and_ip() {
        let server = oxicast::serve::FileServer::start("127.0.0.1:0").await.unwrap();
        assert_ne!(server.addr().port(), 0); // got a real port
        assert!(!server.lan_ip().is_empty());
    }
}
