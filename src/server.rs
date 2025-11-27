use crate::error::Result;
use crate::media::MediaType;
use crate::message::Message;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};

type ClientId = usize;

#[derive(Clone)]
struct ClientInfo {
    username: String,
    sender: mpsc::UnboundedSender<Message>,
}

/// Tracks pending synchronized media playback
#[allow(dead_code)]
struct PendingPlayback {
    media_id: String,
    filename: String,
    media_type: MediaType,
    caption: Option<String>,
    sender: String,
    ready_clients: HashSet<ClientId>,
    all_clients: HashSet<ClientId>,
    broadcast_time: std::time::Instant,
}

struct ServerState {
    clients: HashMap<ClientId, ClientInfo>,
    next_id: ClientId,
    pending_playbacks: HashMap<String, PendingPlayback>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            next_id: 1,
            pending_playbacks: HashMap::new(),
        }
    }

    fn add_client(&mut self, username: String, sender: mpsc::UnboundedSender<Message>) -> ClientId {
        let id = self.next_id;
        self.next_id += 1;
        self.clients.insert(id, ClientInfo { username, sender });
        id
    }

    fn remove_client(&mut self, id: ClientId) -> Option<String> {
        let username = self.clients.remove(&id).map(|info| info.username);

        // Remove client from all pending playbacks
        let mut playbacks_to_check = Vec::new();

        for (media_id, pending) in &mut self.pending_playbacks {
            pending.all_clients.remove(&id);
            pending.ready_clients.remove(&id);

            // Check if this playback should now start
            if !pending.all_clients.is_empty()
                && pending.ready_clients.len() == pending.all_clients.len()
            {
                playbacks_to_check.push(media_id.clone());
            }
        }

        // Send PlaybackStart for any playbacks that are now ready
        for media_id in playbacks_to_check {
            if let Some(pending) = self.pending_playbacks.remove(&media_id) {
                eprintln!(
                    "🎉 All remaining clients ready! Starting playback for {}",
                    &pending.filename
                );

                let playback_start = Message::PlaybackStart {
                    media_id,
                    countdown: 0,
                };

                self.broadcast(&playback_start, None);
            }
        }

        username
    }

    fn broadcast(&self, message: &Message, exclude_id: Option<ClientId>) {
        for (id, client) in &self.clients {
            if Some(*id) != exclude_id {
                // Ignore send errors (client might have disconnected)
                let _ = client.sender.send(message.clone());
            }
        }
    }
}

/// Run the TCP chat server on the specified port
pub async fn run_server(port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;

    eprintln!("Server listening on {addr}");

    let state = Arc::new(Mutex::new(ServerState::new()));

    loop {
        let (stream, addr) = listener.accept().await?;
        eprintln!("New connection from: {addr}");

        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, state).await {
                eprintln!("Client error: {e}");
            }
        });
    }
}

#[allow(clippy::too_many_lines)]
async fn handle_client(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) -> Result<()> {
    // First message should be UserJoined with the username
    let first_msg = Message::read_from_stream(&mut stream).await?;

    let username = match first_msg {
        Message::UserJoined { username } => {
            if username.is_empty() || username.len() > 50 {
                return Err(crate::error::ChatError::InvalidUsername(
                    "Username must be 1-50 characters".to_string(),
                ));
            }
            username
        }
        _ => {
            return Err(crate::error::ChatError::Protocol(
                "Expected UserJoined message".to_string(),
            ));
        }
    };

    eprintln!("User '{username}' joined");

    // Create channel for sending messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Register client
    let client_id = {
        let mut state = state.lock().await;
        let id = state.add_client(username.clone(), tx.clone());

        // Send current user list to the new client
        let usernames: Vec<String> = state
            .clients
            .values()
            .map(|client| client.username.clone())
            .collect();

        let user_list_msg = Message::UserList { usernames };
        let _ = tx.send(user_list_msg);

        // Broadcast join notification to all OTHER clients
        state.broadcast(
            &Message::UserJoined {
                username: username.clone(),
            },
            Some(id),
        );

        id
    };

    // Split into read and write halves
    let (mut read_half, mut write_half) = stream.into_split();

    // Spawn task to handle outgoing messages
    let write_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Err(e) = message.write_to_writer(&mut write_half).await {
                eprintln!("Error writing to client: {e}");
                break;
            }
        }
    });

    // Handle incoming messages
    let result: Result<()> = async {
        loop {
            let message = Message::read_from_reader(&mut read_half).await?;

            if let Message::UrlDownloadRequest {
                requester,
                url,
                caption,
            } = &message
            {
                eprintln!("📥 Received URL download request from {requester}: {url}");
                // Handle in background task to not block server
                let state_clone = Arc::clone(&state);
                let url = url.clone();
                let caption = caption.clone();
                let requester = requester.clone();
                let client_id_for_progress = client_id;

                tokio::spawn(async move {
                    if let Err(e) = handle_url_download(
                        &state_clone,
                        &requester,
                        &url,
                        caption.as_deref(),
                        client_id_for_progress,
                    )
                    .await
                    {
                        eprintln!("Download error for {url}: {e}");

                        // Send error to requester
                        let error_msg = Message::DownloadError {
                            requester: requester.clone(),
                            url,
                            error: e.to_string(),
                        };

                        let state = state_clone.lock().await;
                        if let Some(client) = state.clients.get(&client_id_for_progress) {
                            let _ = client.sender.send(error_msg);
                        }
                    }
                });
            } else if let Message::MediaTransfer {
                sender,
                filename,
                media_type,
                checksum,
                compressed_data: _,
                original_size: _,
                caption,
            } = &message
            {
                // Handle synchronized media playback
                eprintln!("🎬 MediaTransfer from {sender}: {filename} ({checksum})");

                let mut state = state.lock().await;

                // Create pending playback entry
                let all_client_ids: HashSet<ClientId> = state.clients.keys().copied().collect();

                let pending = PendingPlayback {
                    media_id: checksum.clone(),
                    filename: filename.clone(),
                    media_type: media_type.clone(),
                    caption: caption.clone(),
                    sender: sender.clone(),
                    ready_clients: HashSet::new(),
                    all_clients: all_client_ids,
                    broadcast_time: std::time::Instant::now(),
                };

                let client_count = pending.all_clients.len();
                state.pending_playbacks.insert(checksum.clone(), pending);

                // Broadcast to ALL clients (including sender)
                state.broadcast(&message, None);

                drop(state);

                eprintln!("   Waiting for {client_count} clients to acknowledge");
            } else if let Message::MediaReady { media_id, username } = &message {
                // Handle client ready acknowledgment
                eprintln!("✅ {username} ready for media {}", &media_id[..8]);

                let state_clone = Arc::clone(&state);
                handle_media_ready(state_clone, client_id, media_id.clone()).await;
            } else {
                // Broadcast to all clients (including sender for text messages)
                let state = state.lock().await;
                state.broadcast(&message, None);
            }
        }
    }
    .await;

    // Cleanup on disconnect
    write_task.abort();

    let username = {
        let mut state = state.lock().await;
        let username = state.remove_client(client_id);

        // Broadcast leave notification
        if let Some(ref username) = username {
            state.broadcast(
                &Message::UserLeft {
                    username: username.clone(),
                },
                None,
            );
        }

        username
    };

    if let Some(username) = username {
        eprintln!("User '{username}' left");
    }

    result
}

/// Handle a client marking themselves as ready for synchronized playback
async fn handle_media_ready(state: Arc<Mutex<ServerState>>, client_id: ClientId, media_id: String) {
    let mut state = state.lock().await;

    // Find the pending playback and update it
    let (ready_count, total_count, filename, all_ready) = {
        let Some(pending) = state.pending_playbacks.get_mut(&media_id) else {
            eprintln!("⚠️  MediaReady for unknown media_id: {}", &media_id[..8]);
            return;
        };

        // Mark this client as ready
        pending.ready_clients.insert(client_id);

        let ready_count = pending.ready_clients.len();
        let total_count = pending.all_clients.len();
        let filename = pending.filename.clone();
        let all_ready = ready_count == total_count;

        (ready_count, total_count, filename, all_ready)
    };

    eprintln!("   Progress: {ready_count}/{total_count} clients ready for {filename}");

    // Check if all clients are ready
    if all_ready {
        eprintln!("🎉 All clients ready! Starting playback for {filename}");

        // Send PlaybackStart to all clients
        let playback_start = Message::PlaybackStart {
            media_id: media_id.clone(),
            countdown: 0, // Play immediately
        };

        state.broadcast(&playback_start, None);

        // Remove pending playback
        state.pending_playbacks.remove(&media_id);
    } else {
        // Collect IDs first, then look up usernames
        let (ready_ids, waiting_ids) = {
            let Some(pending) = state.pending_playbacks.get(&media_id) else {
                // This shouldn't happen, but handle it gracefully
                return;
            };
            let ready_ids: Vec<ClientId> = pending.ready_clients.iter().copied().collect();
            let waiting_ids: Vec<ClientId> = pending
                .all_clients
                .iter()
                .filter(|id| !pending.ready_clients.contains(id))
                .copied()
                .collect();
            (ready_ids, waiting_ids)
        };

        // Now look up usernames
        let ready_usernames: Vec<String> = ready_ids
            .iter()
            .filter_map(|id| state.clients.get(id).map(|c| c.username.clone()))
            .collect();

        let waiting_usernames: Vec<String> = waiting_ids
            .iter()
            .filter_map(|id| state.clients.get(id).map(|c| c.username.clone()))
            .collect();

        let waiting_msg = Message::PlaybackWaiting {
            media_id: media_id.clone(),
            ready_users: ready_usernames,
            waiting_users: waiting_usernames,
        };

        state.broadcast(&waiting_msg, None);
    }
}

async fn handle_url_download(
    state: &Arc<Mutex<ServerState>>,
    requester: &str,
    url: &str,
    caption: Option<&str>,
    requester_client_id: ClientId,
) -> Result<()> {
    use crate::{compression, media, ytdlp};

    eprintln!("🔄 Starting download for: {url}");

    // 1. Check yt-dlp installed
    if !ytdlp::check_ytdlp_installed() {
        return Err(crate::error::ChatError::Protocol(
            "yt-dlp not installed on server".to_string(),
        ));
    }

    // 2. Extract metadata (fast, no download)
    let info = ytdlp::get_media_info(url)
        .await
        .map_err(|e| crate::error::ChatError::Protocol(format!("Failed to get media info: {e}")))?;

    // Send initial progress
    send_progress_to_requester(
        state,
        requester_client_id,
        requester,
        url,
        0.0,
        &format!("Starting download: {}", info.title),
    )
    .await;

    // 3. Create temp file for download
    let temp_dir = std::env::temp_dir();
    let temp_filename = format!(
        "ytdlp_{}_{}.{}",
        chrono::Utc::now().timestamp(),
        uuid::Uuid::new_v4(),
        info.ext
    );
    let temp_path = temp_dir.join(temp_filename);

    // 4. Check for cookies file
    let cookies_path = find_cookies_file();
    if let Some(ref path) = cookies_path {
        eprintln!("   Using cookies from: {}", path.display());
    } else {
        eprintln!("   ⚠️  No cookies file found - some videos may fail");
    }

    // 5. Download with progress callback
    let state_clone = Arc::clone(state);
    let requester_owned = requester.to_string();
    let url_owned = url.to_string();

    let downloaded_path = ytdlp::download_media(
        url,
        &temp_path,
        cookies_path.as_deref(),
        move |percent, status| {
            let state = Arc::clone(&state_clone);
            let requester = requester_owned.clone();
            let url = url_owned.clone();

            tokio::spawn(async move {
                send_progress_to_requester(&state, requester_client_id, &requester, &url, percent, &status)
                    .await;
            });
        },
    )
    .await
    .map_err(|e| crate::error::ChatError::Protocol(format!("Download failed: {e}")))?;

    // 6. Read downloaded file
    let file_data = tokio::fs::read(&downloaded_path).await?;
    let original_size = file_data.len();

    // 7. Detect media type
    let media_type = media::detect_media_type(
        downloaded_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown"),
    )?;

    // Update duration if we have it
    let media_type = match media_type {
        media::MediaType::Video { .. } => media::MediaType::Video {
            duration_secs: info.duration.unwrap_or(5),
        },
        other @ media::MediaType::Image => other,
    };

    // 8. Calculate checksum
    let checksum = media::calculate_sha256(&file_data);

    // 9. Compress
    let compressed_data = compression::compress(&file_data)?;

    // 10. Create MediaTransfer message
    let filename = format!("{}.{}", info.title, info.ext);
    let media_msg = Message::MediaTransfer {
        sender: requester.to_string(),
        filename: filename.clone(),
        media_type: media_type.clone(),
        checksum: checksum.clone(),
        compressed_data,
        original_size,
        caption: caption.map(String::from),
    };

    // 11. Create pending playback entry and broadcast to all clients
    {
        let mut state = state.lock().await;

        // Create pending playback entry for synchronized playback
        let all_client_ids: HashSet<ClientId> = state.clients.keys().copied().collect();

        let pending = PendingPlayback {
            media_id: checksum.clone(),
            filename: filename.clone(),
            media_type: media_type.clone(),
            caption: caption.map(String::from),
            sender: requester.to_string(),
            ready_clients: HashSet::new(),
            all_clients: all_client_ids,
            broadcast_time: std::time::Instant::now(),
        };

        let client_count = pending.all_clients.len();
        state.pending_playbacks.insert(checksum.clone(), pending);

        eprintln!("🎬 Broadcasting downloaded media to {client_count} clients");

        // Broadcast to ALL clients (including requester)
        state.broadcast(&media_msg, None);
    }

    // 12. Cleanup temp file
    let _ = tokio::fs::remove_file(downloaded_path).await;

    Ok(())
}

async fn send_progress_to_requester(
    state: &Arc<Mutex<ServerState>>,
    requester_id: ClientId,
    requester: &str,
    url: &str,
    percent: f32,
    status: &str,
) {
    let progress_msg = Message::DownloadProgress {
        requester: requester.to_string(),
        url: url.to_string(),
        percent,
        status: status.to_string(),
    };

    let state = state.lock().await;
    if let Some(client) = state.clients.get(&requester_id) {
        let _ = client.sender.send(progress_msg);
    }
}

fn find_cookies_file() -> Option<std::path::PathBuf> {
    // Check ~/.config/image-chat/cookies.txt
    if let Some(config_dir) = dirs::config_dir() {
        let cookies = config_dir.join("image-chat").join("cookies.txt");
        if cookies.exists() {
            return Some(cookies);
        }
    }
    None
}
