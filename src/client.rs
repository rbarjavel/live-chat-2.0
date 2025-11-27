#![allow(dead_code)]

use crate::compression;
use crate::error::{ChatError, Result};
use crate::media::{calculate_sha256, detect_media_type, MediaType};
use crate::media_cache::MediaCache;
use crate::media_player::{GodotMediaPlayer, MediaPlayer, SystemViewerPlayer};
use crate::message::Message;
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Pending media waiting for `PlaybackStart` signal
struct PendingMedia {
    sender: String,
    filename: String,
    media_type: MediaType,
    caption: Option<String>,
    cached_path: PathBuf,
}

/// Run the TCP chat client
#[allow(clippy::too_many_lines)]
pub async fn run_client(host: &str, port: u16, username: String) -> Result<()> {
    // Validate username
    if username.is_empty() || username.len() > 50 {
        return Err(ChatError::InvalidUsername(
            "Username must be 1-50 characters".to_string(),
        ));
    }

    // Initialize media cache
    let media_cache = Arc::new(Mutex::new(MediaCache::new()?));
    let cache_clone = Arc::clone(&media_cache);

    // Initialize media player - try Godot player first, fall back to system viewer
    let media_player: Arc<dyn MediaPlayer> = if let Some(godot_player) = GodotMediaPlayer::find_player() {
        eprintln!("Found Godot player at: {}", godot_player.player_path.display());
        Arc::new(godot_player)
    } else {
        eprintln!("Godot player not found, using system default viewer");
        Arc::new(SystemViewerPlayer)
    };
    let player_clone = Arc::clone(&media_player);

    // Initialize pending media buffer for synchronized playback
    let pending_media: Arc<Mutex<HashMap<String, PendingMedia>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let pending_clone = Arc::clone(&pending_media);

    // Create channel for sending MediaReady acknowledgments
    let (ready_tx, mut ready_rx) = mpsc::unbounded_channel::<Message>();

    // Connect to server
    let addr = format!("{host}:{port}");
    eprintln!("Connecting to {addr}...");

    let mut stream = TcpStream::connect(&addr).await?;
    eprintln!("Connected to server!");

    // Send username as first message
    let join_msg = Message::UserJoined {
        username: username.clone(),
    };
    join_msg.write_to_stream(&mut stream).await?;

    // Split stream
    let (mut read_half, write_half) = stream.into_split();

    // Wrap write_half in Arc<Mutex> for shared access
    let write_half_shared = Arc::new(tokio::sync::Mutex::new(write_half));
    let write_for_main = Arc::clone(&write_half_shared);
    let write_for_ready = Arc::clone(&write_half_shared);

    // Spawn task to receive messages
    let username_for_receive = username.clone();
    let receive_task = tokio::spawn(async move {
        loop {
            match Message::read_from_reader(&mut read_half).await {
                Ok(message) => {
                    display_message(
                        &message,
                        &cache_clone,
                        &player_clone,
                        &pending_clone,
                        &ready_tx,
                        &username_for_receive,
                    );
                }
                Err(ChatError::ConnectionClosed) => {
                    eprintln!("\nDisconnected from server");
                    break;
                }
                Err(e) => {
                    eprintln!("\nError receiving message: {e}");
                    break;
                }
            }
        }
    });

    // Spawn task to forward MediaReady acknowledgments to server
    tokio::spawn(async move {
        while let Some(message) = ready_rx.recv().await {
            let mut writer = write_for_ready.lock().await;
            if let Err(e) = message.write_to_writer(&mut writer).await {
                eprintln!("Error sending MediaReady: {e}");
                break;
            }
        }
    });

    // Read from stdin and send messages
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    eprintln!("\nType messages to chat");
    eprintln!("Commands:");
    eprintln!("  /send <filepath>                - Send a file");
    eprintln!("  /send-media <filepath> [caption] - Send an image or video");
    eprintln!("  /url <link> [caption]            - Download & share media from URL");
    eprintln!("  /setup-cookies                   - Show cookie setup instructions");
    eprintln!("  /check-cookies                   - Verify cookie file exists");
    eprintln!("Press Ctrl+C to exit\n");

    loop {
        print!("> ");
        // Note: Can't use print! macro's flush in async, so we accept this limitation
        line.clear();

        match reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF
                break;
            }
            Ok(_) => {
                let content = line.trim();
                if content.is_empty() {
                    continue;
                }

                // Check for /send-media command
                if let Some(args) = content.strip_prefix("/send-media ") {
                    let parts: Vec<&str> = args.splitn(2, ' ').collect();
                    let filepath = parts[0];
                    let caption = parts.get(1).map(|s| (*s).to_string());

                    let mut writer = write_for_main.lock().await;
                    match send_media(&mut writer, &username, filepath, caption, &media_cache).await {
                        Ok(()) => {
                            eprintln!("Media sent successfully!");
                        }
                        Err(e) => {
                            eprintln!("Error sending media: {e}");
                        }
                    }
                } else if let Some(filepath) = content.strip_prefix("/send ") {
                    // Check for /send command
                    let mut writer = write_for_main.lock().await;
                    match send_file(&mut writer, &username, filepath).await {
                        Ok(()) => {
                            eprintln!("File sent successfully!");
                        }
                        Err(e) => {
                            eprintln!("Error sending file: {e}");
                        }
                    }
                } else if let Some(args) = content.strip_prefix("/url ") {
                    // Check for /url command
                    let parts: Vec<&str> = args.splitn(2, ' ').collect();
                    let url = parts[0];
                    let caption = parts.get(1).map(|s| (*s).to_string());

                    // Send request to server
                    let msg = Message::UrlDownloadRequest {
                        requester: username.clone(),
                        url: url.to_string(),
                        caption,
                    };

                    let mut writer = write_for_main.lock().await;
                    match msg.write_to_writer(&mut writer).await {
                        Ok(()) => {
                            eprintln!("📡 Download request sent to server...");
                        }
                        Err(e) => {
                            eprintln!("Error sending URL request: {e}");
                        }
                    }
                } else if content == "/setup-cookies" {
                    crate::setup::print_cookie_setup_instructions();
                } else if content == "/check-cookies" {
                    if crate::setup::has_cookies() {
                        println!(
                            "✅ Cookies file found at: {}",
                            crate::setup::get_cookies_path().display()
                        );
                    } else {
                        println!("❌ Cookies file not found");
                        println!("Run /setup-cookies for instructions");
                    }
                } else {
                    // Send text message
                    let msg = Message::TextMessage {
                        sender: username.clone(),
                        content: content.to_string(),
                        timestamp: current_timestamp(),
                    };

                    let mut writer = write_for_main.lock().await;
                    if let Err(e) = msg.write_to_writer(&mut writer).await {
                        eprintln!("Error sending message: {e}");
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading input: {e}");
                break;
            }
        }
    }

    receive_task.abort();
    Ok(())
}

async fn send_file(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    username: &str,
    filepath: &str,
) -> Result<()> {
    let path = Path::new(filepath);

    if !path.exists() {
        return Err(ChatError::Protocol(format!("File not found: {filepath}")));
    }

    // Read file
    let file_data = fs::read(path).await?;
    let original_size = file_data.len();

    eprintln!("Reading file... ({original_size} bytes)");

    // Compress
    let compressed_data = compression::compress(&file_data)?;

    let compressed_len = compressed_data.len();
    #[allow(clippy::cast_precision_loss)]
    let percent = (compressed_len as f64 / original_size as f64) * 100.0;
    eprintln!("Compressed to {compressed_len} bytes ({percent:.1}% of original)");

    // Send file transfer message
    let msg = Message::FileTransfer {
        sender: username.to_string(),
        filename: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string(),
        compressed_data,
        original_size,
    };

    msg.write_to_writer(writer).await?;

    Ok(())
}

async fn send_media(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    username: &str,
    filepath: &str,
    caption: Option<String>,
    media_cache: &Arc<Mutex<MediaCache>>,
) -> Result<()> {
    let path = Path::new(filepath);

    if !path.exists() {
        return Err(ChatError::Protocol(format!("File not found: {filepath}")));
    }

    // Detect media type
    let media_type = detect_media_type(filepath)?;

    // Read file
    let file_data = fs::read(path).await?;
    let original_size = file_data.len();

    eprintln!("Reading media file... ({original_size} bytes)");
    eprintln!("Media type: {media_type:?}");

    // Calculate checksum
    let checksum = calculate_sha256(&file_data);

    // Compress
    let compressed_data = compression::compress(&file_data)?;

    let compressed_len = compressed_data.len();
    #[allow(clippy::cast_precision_loss)]
    let percent = (compressed_len as f64 / original_size as f64) * 100.0;
    eprintln!("Compressed to {compressed_len} bytes ({percent:.1}% of original)");

    // Cache media locally
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    media_cache
        .lock()
        .map_err(|_| ChatError::Protocol("Failed to lock cache".to_string()))?
        .save(&checksum, filename, &file_data, media_type.clone())?;
    eprintln!("Cached locally");

    // Send media transfer message
    let msg = Message::MediaTransfer {
        sender: username.to_string(),
        filename: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string(),
        media_type,
        checksum,
        compressed_data,
        original_size,
        caption,
    };

    msg.write_to_writer(writer).await?;

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn display_message(
    message: &Message,
    media_cache: &Arc<Mutex<MediaCache>>,
    media_player: &Arc<dyn MediaPlayer>,
    pending_media: &Arc<Mutex<HashMap<String, PendingMedia>>>,
    ready_tx: &mpsc::UnboundedSender<Message>,
    username: &str,
) {
    match message {
        Message::TextMessage {
            sender,
            content,
            timestamp,
        } => {
            let dt = timestamp_to_datetime(*timestamp);
            let time = dt.format("%H:%M:%S");
            println!("[{time}] {sender}: {content}");
        }
        Message::FileTransfer {
            sender,
            filename,
            compressed_data,
            original_size,
        } => {
            // Decompress and save
            match compression::decompress(compressed_data, *original_size) {
                Ok(data) => {
                    // Save to current directory
                    let save_path = format!("received_{filename}");
                    match std::fs::write(&save_path, data) {
                        Ok(()) => {
                            println!(
                                "\nFile received from {sender}: {filename} ({original_size} bytes) -> saved as {save_path}"
                            );
                        }
                        Err(e) => {
                            eprintln!("\nError saving file {filename}: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("\nError decompressing file from {sender}: {e}");
                }
            }
        }
        Message::MediaTransfer {
            sender,
            filename,
            media_type,
            checksum,
            compressed_data,
            original_size,
            caption,
        } => {
            // Synchronized playback: cache media, store in pending, send MediaReady
            eprintln!("\n📥 Receiving media from {sender}: {filename}");

            match handle_media_transfer_sync(
                sender,
                filename,
                media_type,
                checksum,
                compressed_data,
                *original_size,
                caption.as_deref(),
                media_cache,
                pending_media,
            ) {
                Ok(()) => {
                    // Send MediaReady acknowledgment
                    let ready_msg = Message::MediaReady {
                        media_id: checksum.clone(),
                        username: username.to_string(),
                    };
                    let _ = ready_tx.send(ready_msg);
                }
                Err(e) => {
                    eprintln!("Error handling media from {sender}: {e}");
                }
            }
        }
        Message::PlaybackStart { media_id, countdown: _ } => {
            // Server signals to start playback
            let Ok(pending) = pending_media.lock() else {
                eprintln!("\n⚠️  Failed to lock pending media");
                return;
            };

            if let Some(media) = pending.get(media_id) {
                eprintln!("\n🎬 Playing: {}", media.filename);

                // Play the media
                if let Err(e) = media_player.play_media(
                    &media.cached_path,
                    &media.media_type,
                    media.caption.as_deref(),
                    Some(&media.sender),
                ) {
                    eprintln!("Error playing media: {e}");
                }
            } else {
                eprintln!("\n⚠️  PlaybackStart for unknown media: {}", &media_id[..8]);
            }

            // Remove from pending
            drop(pending);
            if let Ok(mut pending) = pending_media.lock() {
                pending.remove(media_id);
            }
        }
        Message::PlaybackWaiting {
            media_id: _,
            ready_users,
            waiting_users,
        } => {
            // Server sends status update
            eprintln!("\n⏳ Waiting for synchronization...");
            eprintln!("   Ready: {} users", ready_users.len());
            if !waiting_users.is_empty() {
                eprintln!("   Waiting for: {}", waiting_users.join(", "));
            }
        }
        Message::MediaReady { .. } | Message::UrlDownloadRequest { .. } => {
            // Clients don't need to handle these - they're server-only
        }
        Message::UserJoined { username } => {
            println!("\n*** {username} joined the chat ***");
        }
        Message::UserLeft { username } => {
            println!("\n*** {username} left the chat ***");
        }
        Message::ServerInfo { message } => {
            println!("\n[SERVER] {message}");
        }
        Message::DownloadProgress {
            requester: _,
            url: _,
            percent,
            status,
        } => {
            // Only show to requester (server filters this)
            use std::io::Write;
            print!("\r\x1b[K📥 Download: {percent:.1}% - {status}");
            // \r returns to start of line, \x1b[K clears to end of line
            let _ = std::io::stdout().flush();
        }
        Message::DownloadError {
            requester: _,
            url,
            error,
        } => {
            eprintln!("\n❌ Download failed for {url}");
            eprintln!("   Error: {error}");
        }
        Message::UserList { usernames } => {
            println!("\n📋 Connected users: {}", usernames.join(", "));
        }
    }
}

/// Handle media transfer for synchronized playback - caches but doesn't play
#[allow(clippy::too_many_arguments)]
fn handle_media_transfer_sync(
    sender: &str,
    filename: &str,
    media_type: &MediaType,
    checksum: &str,
    compressed_data: &[u8],
    original_size: usize,
    caption: Option<&str>,
    media_cache: &Arc<Mutex<MediaCache>>,
    pending_media: &Arc<Mutex<HashMap<String, PendingMedia>>>,
) -> Result<()> {
    // Check if already cached
    let cache_path = {
        let cache = media_cache
            .lock()
            .map_err(|_| ChatError::Protocol("Failed to lock cache".to_string()))?;

        if let Some(cached) = cache.get(checksum) {
            // Verify cached file
            if cache.verify(checksum)? {
                eprintln!("   Using cached version");
                Some(cached.local_path.clone())
            } else {
                None
            }
        } else {
            None
        }
    };

    let media_path = if let Some(path) = cache_path {
        path
    } else {
        // Decompress media
        let data = compression::decompress(compressed_data, original_size)?;

        // Verify checksum
        let actual_checksum = calculate_sha256(&data);
        if actual_checksum != checksum {
            return Err(ChatError::Protocol(format!(
                "Checksum mismatch: expected {checksum}, got {actual_checksum}"
            )));
        }

        // Save to cache
        let path = media_cache
            .lock()
            .map_err(|_| ChatError::Protocol("Failed to lock cache".to_string()))?
            .save(checksum, filename, &data, media_type.clone())?;

        eprintln!("   Cached at: {}", path.display());

        path
    };

    // Store in pending buffer instead of playing
    let pending = PendingMedia {
        sender: sender.to_string(),
        filename: filename.to_string(),
        media_type: media_type.clone(),
        caption: caption.map(String::from),
        cached_path: media_path,
    };

    pending_media
        .lock()
        .map_err(|_| ChatError::Protocol("Failed to lock pending media".to_string()))?
        .insert(checksum.to_string(), pending);

    eprintln!("   Ready! Waiting for other users...");

    Ok(())
}

#[allow(clippy::too_many_arguments, dead_code)]
fn handle_media_transfer(
    sender: &str,
    filename: &str,
    media_type: &crate::media::MediaType,
    checksum: &str,
    compressed_data: &[u8],
    original_size: usize,
    caption: Option<&str>,
    media_cache: &Arc<Mutex<MediaCache>>,
    media_player: &Arc<dyn MediaPlayer>,
) -> Result<()> {
    // Check if already cached
    let cache_path = {
        let cache = media_cache
            .lock()
            .map_err(|_| ChatError::Protocol("Failed to lock cache".to_string()))?;

        if let Some(cached) = cache.get(checksum) {
            // Verify cached file
            if cache.verify(checksum)? {
                println!("\n📥 Media from {sender} (cached)");
                Some(cached.local_path.clone())
            } else {
                None
            }
        } else {
            None
        }
    };

    let media_path = if let Some(path) = cache_path {
        path
    } else {
        // Decompress media
        let data = compression::decompress(compressed_data, original_size)?;

        // Verify checksum
        let actual_checksum = calculate_sha256(&data);
        if actual_checksum != checksum {
            return Err(ChatError::Protocol(format!(
                "Checksum mismatch: expected {checksum}, got {actual_checksum}"
            )));
        }

        // Save to cache
        let path = media_cache
            .lock()
            .map_err(|_| ChatError::Protocol("Failed to lock cache".to_string()))?
            .save(checksum, filename, &data, media_type.clone())?;

        println!("\n📥 Media received from {sender}");
        println!("   Cached at: {}", path.display());

        path
    };

    // Play media
    if let Err(e) = media_player.play_media(&media_path, media_type, caption, Some(sender)) {
        eprintln!("Error playing media: {e}");
    }

    Ok(())
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn timestamp_to_datetime(timestamp: u64) -> DateTime<Local> {
    use chrono::TimeZone;
    #[allow(clippy::cast_possible_wrap)]
    Local.timestamp_opt(timestamp as i64, 0).unwrap()
}
