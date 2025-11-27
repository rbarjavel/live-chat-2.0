use crate::compression;
use crate::error::{ChatError, Result};
use crate::media::{self, MediaType};
use crate::media_cache::MediaCache;
use crate::media_player::{GodotMediaPlayer, MediaPlayer, SystemViewerPlayer};
use crate::message::Message;
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex as TokioMutex};

/// Commands sent from GUI thread to network thread
#[derive(Debug)]
enum ClientCommand {
    Text(String),
    File(PathBuf),
    Media {
        path: PathBuf,
        caption: Option<String>,
    },
    Url {
        url: String,
        caption: Option<String>,
    },
}

/// Log levels for debug logging
#[derive(Debug, Clone)]
enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Log entry for debug window
#[derive(Debug, Clone)]
struct LogEntry {
    timestamp: String,
    level: LogLevel,
    message: String,
}

/// Events sent from network thread to GUI thread
#[derive(Debug, Clone)]
enum NetworkEvent {
    Connected,
    Disconnected,
    MessageReceived(DisplayMessage),
    ConnectionError(String),
    LogMessage(LogEntry),
}

/// GUI-friendly message representation
#[derive(Debug, Clone)]
struct DisplayMessage {
    timestamp: String,
    sender: String,
    content: MessageContent,
}

#[derive(Debug, Clone)]
enum MessageContent {
    Text(String),
    MediaReceived {
        filename: String,
        media_type: String,
    },
    UserJoined,
    UserLeft,
    UserList {
        usernames: Vec<String>,
    },
    SystemInfo(String),
    DownloadProgress {
        percent: f32,
        status: String,
    },
    SyncWaiting {
        ready_users: Vec<String>,
        waiting_users: Vec<String>,
    },
    SyncStart {
        filename: String,
    },
    Error(String),
}

/// Pending media waiting for `PlaybackStart` signal
struct PendingMedia {
    sender: String,
    filename: String,
    media_type: MediaType,
    caption: Option<String>,
    cached_path: PathBuf,
}

/// Dialog state for file operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogState {
    None,
    Caption,
    Url,
}

/// Main GUI application state
struct ChatApp {
    // Network communication
    command_tx: mpsc::UnboundedSender<ClientCommand>,
    event_rx: mpsc::UnboundedReceiver<NetworkEvent>,

    // UI state
    messages: Vec<DisplayMessage>,
    input_text: String,
    username: String,
    connected: bool,

    // User tracking
    connected_users: std::collections::HashSet<String>,

    // Debug logging
    show_log_window: bool,
    log_messages: Vec<LogEntry>,

    // File operations and dialogs
    dialog_state: DialogState,
    pending_file: Option<PathBuf>,
    caption_input: String,
    url_input: String,
    url_caption_input: String,
}

impl ChatApp {
    #[allow(clippy::missing_const_for_fn)]
    fn new(
        username: String,
        command_tx: mpsc::UnboundedSender<ClientCommand>,
        event_rx: mpsc::UnboundedReceiver<NetworkEvent>,
    ) -> Self {
        Self {
            command_tx,
            event_rx,
            messages: Vec::new(),
            input_text: String::new(),
            username,
            connected: false,
            connected_users: std::collections::HashSet::new(),
            show_log_window: false,
            log_messages: Vec::new(),
            dialog_state: DialogState::None,
            pending_file: None,
            caption_input: String::new(),
            url_input: String::new(),
            url_caption_input: String::new(),
        }
    }

    fn handle_network_event(&mut self, event: NetworkEvent) {
        match event {
            NetworkEvent::Connected => {
                self.connected = true;
                self.connected_users.clear();
                self.connected_users.insert(self.username.clone());
                self.add_system_message("Connected to server");
            }
            NetworkEvent::Disconnected => {
                self.connected = false;
                self.connected_users.clear();
                self.add_system_message("Disconnected from server");
            }
            NetworkEvent::MessageReceived(msg) => {
                // Track user joins/leaves/list
                match &msg.content {
                    MessageContent::UserList { usernames } => {
                        // Populate initial user list
                        self.connected_users.clear();
                        for username in usernames {
                            self.connected_users.insert(username.clone());
                        }
                    }
                    MessageContent::UserJoined => {
                        self.connected_users.insert(msg.sender.clone());
                    }
                    MessageContent::UserLeft => {
                        self.connected_users.remove(&msg.sender);
                    }
                    _ => {}
                }

                // Special handling for progress/status messages - update existing instead of adding new
                match &msg.content {
                    MessageContent::DownloadProgress { .. } => {
                        // Update existing download progress
                        if let Some(existing) = self.messages.iter_mut().rev().find(|m| {
                            matches!(m.content, MessageContent::DownloadProgress { .. })
                                && m.sender == msg.sender
                        }) {
                            *existing = msg;
                        } else {
                            self.messages.push(msg);
                        }
                    }
                    MessageContent::SyncWaiting { .. } => {
                        // Update existing sync waiting message
                        if let Some(existing) = self.messages.iter_mut().rev().find(|m| {
                            matches!(m.content, MessageContent::SyncWaiting { .. })
                        }) {
                            *existing = msg;
                        } else {
                            self.messages.push(msg);
                        }
                    }
                    _ => {
                        self.messages.push(msg);
                    }
                }
            }
            NetworkEvent::ConnectionError(err) => {
                self.add_system_message(&format!("Connection error: {err}"));
            }
            NetworkEvent::LogMessage(entry) => {
                self.log_messages.push(entry);
                // Keep last 1000 entries
                if self.log_messages.len() > 1000 {
                    self.log_messages.remove(0);
                }
            }
        }
    }

    fn add_system_message(&mut self, text: &str) {
        self.messages.push(DisplayMessage {
            timestamp: format_timestamp(),
            sender: "System".to_string(),
            content: MessageContent::SystemInfo(text.to_string()),
        });
    }

    fn send_text(&mut self) {
        if !self.input_text.is_empty() {
            let text = std::mem::take(&mut self.input_text);
            let _ = self.command_tx.send(ClientCommand::Text(text));
        }
    }

    fn open_file_picker_media(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Media", &["jpg", "jpeg", "png", "mp4", "mov", "webm", "ogv"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.pending_file = Some(path);
            self.dialog_state = DialogState::Caption;
        }
    }

    fn open_file_picker_regular(&self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            let _ = self.command_tx.send(ClientCommand::File(path));
        }
    }

    fn handle_dropped_file(&mut self, path: PathBuf) {
        // Check if it's a media file
        if let Ok(_media_type) = media::detect_media_type(
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown"),
        ) {
            self.pending_file = Some(path);
            self.dialog_state = DialogState::Caption;
        } else {
            // Send as regular file
            let _ = self.command_tx.send(ClientCommand::File(path));
        }
    }

    fn render_chat(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for msg in &self.messages {
                    self.render_message(ui, msg);
                }
            });
    }

    #[allow(clippy::too_many_lines)]
    fn render_message(&self, ui: &mut egui::Ui, msg: &DisplayMessage) {
        let is_own = msg.sender == self.username;

        match &msg.content {
            MessageContent::Text(text) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    if is_own {
                        ui.label(
                            egui::RichText::new(&msg.sender)
                                .strong()
                                .color(egui::Color32::LIGHT_BLUE),
                        );
                    } else {
                        ui.label(egui::RichText::new(&msg.sender).strong());
                    }
                    ui.label(text);
                });
            }

            MessageContent::MediaReceived {
                filename,
                media_type,
            } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    ui.label("📥");
                    ui.label(format!(
                        "{} from {}: {}",
                        media_type, msg.sender, filename
                    ));
                });
            }

            MessageContent::UserJoined => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    ui.label(
                        egui::RichText::new(format!("*** {} joined ***", msg.sender))
                            .weak()
                            .italics(),
                    );
                });
            }

            MessageContent::UserLeft => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    ui.label(
                        egui::RichText::new(format!("*** {} left ***", msg.sender))
                            .weak()
                            .italics(),
                    );
                });
            }

            MessageContent::UserList { usernames } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    let user_list = usernames.join(", ");
                    ui.label(
                        egui::RichText::new(format!("📋 Connected users: {user_list}"))
                            .weak()
                            .italics(),
                    );
                });
            }

            MessageContent::SystemInfo(info) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    ui.label(egui::RichText::new(info).weak());
                });
            }

            MessageContent::DownloadProgress { percent, status } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    ui.label("📥 Download:");
                    ui.add(
                        egui::ProgressBar::new(*percent / 100.0)
                            .text(format!("{status} ({percent:.1}%)"))
                            .desired_width(200.0),
                    );
                });
            }

            MessageContent::SyncWaiting {
                ready_users,
                waiting_users,
            } => {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                        ui.label("⏳ Waiting for synchronization...");
                    });
                    ui.label(format!("Ready: {} users", ready_users.len()));
                    if !waiting_users.is_empty() {
                        ui.label(format!("Waiting for: {}", waiting_users.join(", ")));
                    }
                });
            }

            MessageContent::SyncStart { filename } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    ui.label(
                        egui::RichText::new(format!("🎬 Playing: {filename}"))
                            .strong()
                            .color(egui::Color32::GREEN),
                    );
                });
            }

            MessageContent::Error(err) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&msg.timestamp).weak().small());
                    ui.label(egui::RichText::new(format!("❌ {err}")).color(egui::Color32::RED));
                });
            }
        }

        ui.add_space(4.0);
    }

    fn render_caption_dialog(&mut self, ctx: &egui::Context) {
        if self.dialog_state == DialogState::Caption {
            egui::Window::new("Add Caption")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Enter an optional caption:");
                    ui.text_edit_singleline(&mut self.caption_input);

                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        if ui.button("Send").clicked() {
                            if let Some(path) = self.pending_file.take() {
                                let caption = if self.caption_input.is_empty() {
                                    None
                                } else {
                                    Some(std::mem::take(&mut self.caption_input))
                                };
                                let _ = self
                                    .command_tx
                                    .send(ClientCommand::Media { path, caption });
                            }
                            self.dialog_state = DialogState::None;
                        }

                        if ui.button("Cancel").clicked() {
                            self.pending_file = None;
                            self.caption_input.clear();
                            self.dialog_state = DialogState::None;
                        }
                    });
                });
        }
    }

    fn render_url_dialog(&mut self, ctx: &egui::Context) {
        if self.dialog_state == DialogState::Url {
            egui::Window::new("Download from URL")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Enter URL (YouTube, Twitter, etc.):");
                    ui.text_edit_singleline(&mut self.url_input);

                    ui.add_space(4.0);
                    ui.label("Optional caption:");
                    ui.text_edit_singleline(&mut self.url_caption_input);

                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        if ui.button("Download").clicked() {
                            if !self.url_input.is_empty() {
                                let url = std::mem::take(&mut self.url_input);
                                let caption = if self.url_caption_input.is_empty() {
                                    None
                                } else {
                                    Some(std::mem::take(&mut self.url_caption_input))
                                };
                                let _ = self.command_tx.send(ClientCommand::Url { url, caption });
                            }
                            self.dialog_state = DialogState::None;
                        }

                        if ui.button("Cancel").clicked() {
                            self.url_input.clear();
                            self.url_caption_input.clear();
                            self.dialog_state = DialogState::None;
                        }
                    });
                });
        }
    }

    fn render_log_window(&mut self, ctx: &egui::Context) {
        if self.show_log_window {
            egui::Window::new("Debug Log")
                .default_size([600.0, 400.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            self.log_messages.clear();
                        }
                        ui.label(format!("{} entries", self.log_messages.len()));
                    });
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for entry in &self.log_messages {
                                ui.horizontal(|ui| {
                                    let (icon, color) = match entry.level {
                                        LogLevel::Info => ("ℹ️", egui::Color32::GRAY),
                                        LogLevel::Success => ("✅", egui::Color32::GREEN),
                                        LogLevel::Warning => ("⚠️", egui::Color32::YELLOW),
                                        LogLevel::Error => ("❌", egui::Color32::RED),
                                    };
                                    ui.label(icon);
                                    ui.label(
                                        egui::RichText::new(&entry.timestamp).weak().small(),
                                    );
                                    ui.label(egui::RichText::new(&entry.message).color(color));
                                });
                            }
                        });
                });
        }
    }

    fn render_users_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("👥 Users");

        let count = self.connected_users.len();
        ui.label(format!("Online: {count}"));
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let mut users: Vec<_> = self.connected_users.iter().collect();
                users.sort();

                for user in users {
                    let is_self = user == &self.username;
                    let text = if is_self {
                        egui::RichText::new(format!("• {user} (you)"))
                            .color(egui::Color32::LIGHT_BLUE)
                    } else {
                        egui::RichText::new(format!("• {user}"))
                    };
                    ui.label(text);
                }
            });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(8.0);
            if ui.button("📋 Log").clicked() {
                self.show_log_window = !self.show_log_window;
            }
        });
    }

    fn render_actions_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Actions");
        ui.separator();

        // Media Upload Section
        ui.group(|ui| {
            ui.label(egui::RichText::new("🖼️ Media Upload").strong());
            ui.add_space(5.0);

            if ui.button("📂 Select File").clicked() {
                self.open_file_picker_media();
            }

            ui.add_space(3.0);
            ui.label(egui::RichText::new("Drag & drop files here").weak().small());
        });

        ui.add_space(10.0);

        // URL Download Section
        ui.group(|ui| {
            ui.label(egui::RichText::new("🔗 URL Download").strong());
            ui.add_space(5.0);

            if ui.button("🌐 Download from URL").clicked() {
                self.dialog_state = DialogState::Url;
            }

            ui.add_space(3.0);
            ui.label(egui::RichText::new("YouTube, Twitter, etc.").weak().small());
        });

        ui.add_space(10.0);

        // File Transfer Section
        ui.group(|ui| {
            ui.label(egui::RichText::new("📎 File Transfer").strong());
            ui.add_space(5.0);

            if ui.button("📄 Send File").clicked() {
                self.open_file_picker_regular();
            }

            ui.add_space(3.0);
            ui.label(egui::RichText::new("Any file type").weak().small());
        });
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process network events
        while let Ok(event) = self.event_rx.try_recv() {
            self.handle_network_event(event);
        }

        // Handle dropped files
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    self.handle_dropped_file(path.clone());
                }
            }
        });

        // Top bar
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(format!("Image Chat - {}", self.username));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(if self.connected {
                        "🟢 Connected"
                    } else {
                        "🔴 Disconnected"
                    });
                });
            });
        });

        // Left sidebar - Users
        egui::SidePanel::left("users_panel")
            .default_width(150.0)
            .resizable(false)
            .show(ctx, |ui| {
                self.render_users_panel(ui);
            });

        // Right sidebar - Actions
        egui::SidePanel::right("actions_panel")
            .default_width(200.0)
            .resizable(false)
            .show(ctx, |ui| {
                self.render_actions_panel(ui);
            });

        // Center - Chat
        egui::CentralPanel::default().show(ctx, |ui| {
            // Chat area
            let available_height = ui.available_height() - 50.0;
            ui.allocate_ui(egui::vec2(ui.available_width(), available_height), |ui| {
                self.render_chat(ui);
            });

            ui.separator();

            // Input area (simplified - just text + send)
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.input_text)
                        .hint_text("Type a message...")
                        .desired_width(ui.available_width() - 70.0),
                );

                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.send_text();
                    response.request_focus();
                }

                if ui.button("Send").clicked() {
                    self.send_text();
                }
            });
        });

        // Render dialogs
        self.render_caption_dialog(ctx);
        self.render_url_dialog(ctx);
        self.render_log_window(ctx);

        // Request continuous repaint for animations and event processing
        ctx.request_repaint();
    }
}

/// Macro for logging to GUI log window
macro_rules! gui_log {
    ($tx:expr, $level:expr, $($arg:tt)*) => {{
        eprintln!($($arg)*);  // Still print to stderr
        let entry = LogEntry {
            timestamp: format_timestamp(),
            level: $level,
            message: format!($($arg)*),
        };
        let _ = $tx.send(NetworkEvent::LogMessage(entry));
    }};
}

/// Entry point for GUI client
pub fn run_gui_client(host: &str, port: u16, username: String) -> Result<()> {
    // Create channels
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    // Spawn network task in background thread
    let host = host.to_string();
    let username_clone = username.clone();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create tokio runtime: {e}");
                return;
            }
        };
        rt.block_on(network_task(
            host,
            port,
            username_clone,
            command_rx,
            event_tx,
        ));
    });

    // Run egui on main thread
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_title("Image Chat")
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Image Chat",
        options,
        Box::new(|_cc| Ok(Box::new(ChatApp::new(username, command_tx, event_rx)))),
    )
    .map_err(|e| ChatError::Protocol(format!("GUI error: {e}")))?;

    Ok(())
}

/// Network task running in background thread
#[allow(clippy::too_many_lines)]
async fn network_task(
    host: String,
    port: u16,
    username: String,
    mut command_rx: mpsc::UnboundedReceiver<ClientCommand>,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
) {
    // Connect to server
    let mut stream = match TcpStream::connect(&format!("{host}:{port}")).await {
        Ok(s) => {
            let _ = event_tx.send(NetworkEvent::Connected);
            s
        }
        Err(e) => {
            let _ = event_tx.send(NetworkEvent::ConnectionError(e.to_string()));
            return;
        }
    };

    // Send join message
    let join_msg = Message::UserJoined {
        username: username.clone(),
    };
    if let Err(e) = join_msg.write_to_stream(&mut stream).await {
        let _ = event_tx.send(NetworkEvent::ConnectionError(e.to_string()));
        return;
    }

    // Split stream
    let (read_half, write_half) = stream.into_split();
    let write_half = Arc::new(TokioMutex::new(write_half));

    // Initialize media cache and player
    let media_cache = match MediaCache::new() {
        Ok(cache) => Arc::new(TokioMutex::new(cache)),
        Err(e) => {
            let _ = event_tx.send(NetworkEvent::ConnectionError(format!(
                "Failed to initialize media cache: {e}"
            )));
            return;
        }
    };

    // Try to use Godot player, fall back to system viewer if not found
    let media_player: Arc<dyn MediaPlayer> = if let Some(godot) = GodotMediaPlayer::find_player() {
        gui_log!(event_tx, LogLevel::Success, "✅ Found Godot player at: {}", godot.player_path.display());
        Arc::new(godot)
    } else {
        gui_log!(event_tx, LogLevel::Warning, "⚠️  Godot player not found, using system default viewer");
        Arc::new(SystemViewerPlayer)
    };

    // Pending media for synchronized playback
    let pending_media: Arc<TokioMutex<HashMap<String, PendingMedia>>> =
        Arc::new(TokioMutex::new(HashMap::new()));

    // Create channel for MediaReady acknowledgments
    let (ready_tx, mut ready_rx) = mpsc::unbounded_channel::<Message>();

    // Spawn receiver task
    let event_tx_clone = event_tx.clone();
    let event_tx_for_logs = event_tx.clone();
    let username_clone = username.clone();
    let media_cache_clone = Arc::clone(&media_cache);
    let media_player_clone = Arc::clone(&media_player);
    let pending_media_clone = Arc::clone(&pending_media);
    let ready_tx_clone = ready_tx.clone();

    tokio::spawn(receive_task(
        read_half,
        event_tx_clone,
        event_tx_for_logs,
        username_clone,
        media_cache_clone,
        media_player_clone,
        pending_media_clone,
        ready_tx_clone,
    ));

    // Spawn MediaReady forwarder task
    let write_clone = Arc::clone(&write_half);
    tokio::spawn(async move {
        while let Some(msg) = ready_rx.recv().await {
            let mut writer = write_clone.lock().await;
            let _ = msg.write_to_writer(&mut writer).await;
        }
    });

    // Process commands from GUI
    while let Some(command) = command_rx.recv().await {
        if let Err(e) = handle_command(
            command,
            &write_half,
            &username,
            &media_cache,
            &event_tx,
        )
        .await
        {
            let _ = event_tx.send(NetworkEvent::MessageReceived(DisplayMessage {
                timestamp: format_timestamp(),
                sender: "Error".to_string(),
                content: MessageContent::Error(e.to_string()),
            }));
        }
    }

    let _ = event_tx.send(NetworkEvent::Disconnected);
}

/// Task that receives messages from server
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
async fn receive_task(
    mut read_half: OwnedReadHalf,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
    event_tx_for_logs: mpsc::UnboundedSender<NetworkEvent>,
    username: String,
    media_cache: Arc<TokioMutex<MediaCache>>,
    media_player: Arc<dyn MediaPlayer>,
    pending_media: Arc<TokioMutex<HashMap<String, PendingMedia>>>,
    ready_tx: mpsc::UnboundedSender<Message>,
) {
    loop {
        if let Ok(message) = Message::read_from_reader(&mut read_half).await {
            let display_msg = convert_message(
                &message,
                &username,
                &media_cache,
                &media_player,
                &pending_media,
                &ready_tx,
                &event_tx_for_logs,
            )
            .await;

            if let Some(msg) = display_msg {
                let _ = event_tx.send(NetworkEvent::MessageReceived(msg));
            }
        } else {
            let _ = event_tx.send(NetworkEvent::Disconnected);
            break;
        }
    }
}

/// Convert protocol Message to `DisplayMessage`
#[allow(clippy::too_many_lines)]
async fn convert_message(
    message: &Message,
    username: &str,
    media_cache: &Arc<TokioMutex<MediaCache>>,
    media_player: &Arc<dyn MediaPlayer>,
    pending_media: &Arc<TokioMutex<HashMap<String, PendingMedia>>>,
    ready_tx: &mpsc::UnboundedSender<Message>,
    event_tx: &mpsc::UnboundedSender<NetworkEvent>,
) -> Option<DisplayMessage> {
    match message {
        Message::TextMessage {
            sender,
            content,
            timestamp: _,
        } => Some(DisplayMessage {
            timestamp: format_timestamp(),
            sender: sender.clone(),
            content: MessageContent::Text(content.clone()),
        }),

        Message::FileTransfer {
            sender,
            filename,
            compressed_data,
            original_size,
        } => {
            // Save file to cache
            match handle_file_transfer(sender, filename, compressed_data, *original_size) {
                Ok(()) => Some(DisplayMessage {
                    timestamp: format_timestamp(),
                    sender: sender.clone(),
                    content: MessageContent::SystemInfo(format!("File received: {filename}")),
                }),
                Err(e) => Some(DisplayMessage {
                    timestamp: format_timestamp(),
                    sender: "Error".to_string(),
                    content: MessageContent::Error(format!("File transfer failed: {e}")),
                }),
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
            )
            .await
            {
                Ok(()) => {
                    gui_log!(event_tx, LogLevel::Success, "📥 Media cached successfully: {} ({})", filename, &checksum[..8]);

                    // Send MediaReady acknowledgment
                    let ready_msg = Message::MediaReady {
                        media_id: checksum.clone(),
                        username: username.to_string(),
                    };
                    gui_log!(event_tx, LogLevel::Success, "✅ Sending MediaReady for media_id: {}", &checksum[..8]);
                    let _ = ready_tx.send(ready_msg);

                    Some(DisplayMessage {
                        timestamp: format_timestamp(),
                        sender: sender.clone(),
                        content: MessageContent::MediaReceived {
                            filename: filename.clone(),
                            media_type: format!("{media_type:?}"),
                        },
                    })
                }
                Err(e) => Some(DisplayMessage {
                    timestamp: format_timestamp(),
                    sender: "Error".to_string(),
                    content: MessageContent::Error(format!("Media transfer failed: {e}")),
                }),
            }
        }

        Message::UserJoined { username: user } => {
            if user == username {
                None
            } else {
                Some(DisplayMessage {
                    timestamp: format_timestamp(),
                    sender: user.clone(),
                    content: MessageContent::UserJoined,
                })
            }
        }

        Message::UserLeft { username: user } => Some(DisplayMessage {
            timestamp: format_timestamp(),
            sender: user.clone(),
            content: MessageContent::UserLeft,
        }),

        Message::UserList { usernames } => {
            gui_log!(event_tx, LogLevel::Info, "📋 Received user list: {} users", usernames.len());

            Some(DisplayMessage {
                timestamp: format_timestamp(),
                sender: "System".to_string(),
                content: MessageContent::UserList {
                    usernames: usernames.clone(),
                },
            })
        }

        Message::ServerInfo { message: msg } => Some(DisplayMessage {
            timestamp: format_timestamp(),
            sender: "Server".to_string(),
            content: MessageContent::SystemInfo(msg.clone()),
        }),

        Message::DownloadProgress {
            requester,
            url: _,
            percent,
            status,
        } => {
            if requester == username {
                Some(DisplayMessage {
                    timestamp: format_timestamp(),
                    sender: "Download".to_string(),
                    content: MessageContent::DownloadProgress {
                        percent: *percent,
                        status: status.clone(),
                    },
                })
            } else {
                None
            }
        }

        Message::DownloadError {
            requester,
            url,
            error,
        } => {
            if requester == username {
                Some(DisplayMessage {
                    timestamp: format_timestamp(),
                    sender: "Error".to_string(),
                    content: MessageContent::Error(format!("Download failed ({url}): {error}")),
                })
            } else {
                None
            }
        }

        Message::PlaybackWaiting {
            media_id: _,
            ready_users,
            waiting_users,
        } => Some(DisplayMessage {
            timestamp: format_timestamp(),
            sender: "Sync".to_string(),
            content: MessageContent::SyncWaiting {
                ready_users: ready_users.clone(),
                waiting_users: waiting_users.clone(),
            },
        }),

        Message::PlaybackStart {
            media_id,
            countdown: _,
        } => {
            gui_log!(event_tx, LogLevel::Info, "🎬 Received PlaybackStart for media_id: {}", &media_id[..8]);
            let pending = pending_media.lock().await;

            if let Some(media) = pending.get(media_id) {
                let filename = media.filename.clone();
                let cached_path = media.cached_path.clone();
                let media_type = media.media_type.clone();
                let caption = media.caption.clone();
                let sender = media.sender.clone();

                gui_log!(event_tx, LogLevel::Info, "   Found pending media: {}", filename);
                gui_log!(event_tx, LogLevel::Info, "   Cached path: {}", cached_path.display());
                gui_log!(event_tx, LogLevel::Info, "   Media type: {:?}", media_type);

                drop(pending);

                // Play media in a separate blocking task to avoid blocking the receiver
                gui_log!(event_tx, LogLevel::Info, "   Attempting to play media...");
                let player = Arc::clone(media_player);
                let event_tx_clone = event_tx.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = player.play_media(
                        &cached_path,
                        &media_type,
                        caption.as_deref(),
                        Some(&sender),
                    ) {
                        gui_log!(event_tx_clone, LogLevel::Error, "❌ Error playing media: {e}");
                    } else {
                        gui_log!(event_tx_clone, LogLevel::Success, "✅ Media player launched successfully");
                    }
                });

                // Remove from pending
                {
                    let mut pending = pending_media.lock().await;
                    pending.remove(media_id);
                }

                Some(DisplayMessage {
                    timestamp: format_timestamp(),
                    sender: "Sync".to_string(),
                    content: MessageContent::SyncStart { filename },
                })
            } else {
                gui_log!(event_tx, LogLevel::Warning, "⚠️  PlaybackStart received but no pending media found for media_id: {}", &media_id[..8]);
                None
            }
        }

        Message::UrlDownloadRequest { .. } | Message::MediaReady { .. } => {
            // These messages are only sent by clients, not received
            None
        }
    }
}

/// Handle command from GUI
async fn handle_command(
    command: ClientCommand,
    write_half: &Arc<TokioMutex<OwnedWriteHalf>>,
    username: &str,
    media_cache: &Arc<TokioMutex<MediaCache>>,
    event_tx: &mpsc::UnboundedSender<NetworkEvent>,
) -> Result<()> {
    match command {
        ClientCommand::Text(text) => {
            let msg = Message::TextMessage {
                sender: username.to_string(),
                content: text,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };

            {
                let mut writer = write_half.lock().await;
                msg.write_to_writer(&mut writer).await?;
            }
        }

        ClientCommand::File(path) => {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| ChatError::Protocol("Invalid filename".to_string()))?
                .to_string();

            let file_data = tokio::fs::read(&path).await?;
            let original_size = file_data.len();
            let compressed_data = compression::compress(&file_data)?;

            let msg = Message::FileTransfer {
                sender: username.to_string(),
                filename,
                compressed_data,
                original_size,
            };

            {
                let mut writer = write_half.lock().await;
                msg.write_to_writer(&mut writer).await?;
            }
        }

        ClientCommand::Media { path, caption } => {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| ChatError::Protocol("Invalid filename".to_string()))?
                .to_string();

            // Read file
            let file_data = tokio::fs::read(&path).await?;
            let original_size = file_data.len();

            // Detect media type
            let media_type = media::detect_media_type(&filename)?;

            // Calculate checksum
            let checksum = media::calculate_sha256(&file_data);

            // Check if already cached, if not save it
            let mut cache = media_cache.lock().await;
            if cache.get(&checksum).is_none() {
                // Cache locally first
                cache.save(&checksum, &filename, &file_data, media_type.clone())?;
            }
            drop(cache);

            // Compress
            let compressed_data = compression::compress(&file_data)?;

            let msg = Message::MediaTransfer {
                sender: username.to_string(),
                filename,
                media_type,
                checksum,
                compressed_data,
                original_size,
                caption,
            };

            {
                let mut writer = write_half.lock().await;
                msg.write_to_writer(&mut writer).await?;
            }

            let _ = event_tx.send(NetworkEvent::MessageReceived(DisplayMessage {
                timestamp: format_timestamp(),
                sender: username.to_string(),
                content: MessageContent::SystemInfo("Media sent".to_string()),
            }));
        }

        ClientCommand::Url { url, caption } => {
            let msg = Message::UrlDownloadRequest {
                requester: username.to_string(),
                url,
                caption,
            };

            {
                let mut writer = write_half.lock().await;
                msg.write_to_writer(&mut writer).await?;
            }
        }
    }

    Ok(())
}

/// Handle file transfer (save to desktop)
fn handle_file_transfer(
    sender: &str,
    filename: &str,
    compressed_data: &[u8],
    original_size: usize,
) -> Result<()> {
    // Decompress
    let decompressed = compression::decompress(compressed_data, original_size)?;

    // Save to desktop
    let desktop = dirs::desktop_dir().ok_or_else(|| {
        ChatError::Protocol("Could not determine desktop directory".to_string())
    })?;

    let save_path = desktop.join(format!("{sender}_{filename}"));
    std::fs::write(&save_path, decompressed)?;

    eprintln!("File saved to: {}", save_path.display());
    Ok(())
}

/// Handle media transfer with synchronized playback
#[allow(clippy::too_many_arguments)]
async fn handle_media_transfer_sync(
    sender: &str,
    filename: &str,
    media_type: &MediaType,
    checksum: &str,
    compressed_data: &[u8],
    original_size: usize,
    caption: Option<&str>,
    media_cache: &Arc<TokioMutex<MediaCache>>,
    pending_media: &Arc<TokioMutex<HashMap<String, PendingMedia>>>,
) -> Result<()> {
    let mut cache = media_cache.lock().await;

    // Check if already cached
    let cached_path = if let Some(cached) = cache.get(checksum) {
        // Verify it exists
        if cache.verify(checksum)? {
            cached.local_path.clone()
        } else {
            // Re-save if verification fails
            let decompressed = compression::decompress(compressed_data, original_size)?;
            cache.save(checksum, filename, &decompressed, media_type.clone())?
        }
    } else {
        // Decompress and save to cache
        let decompressed = compression::decompress(compressed_data, original_size)?;
        cache.save(checksum, filename, &decompressed, media_type.clone())?
    };

    drop(cache);

    // Store in pending media
    {
        let mut pending = pending_media.lock().await;
        pending.insert(
            checksum.to_string(),
            PendingMedia {
                sender: sender.to_string(),
                filename: filename.to_string(),
                media_type: media_type.clone(),
                caption: caption.map(String::from),
                cached_path,
            },
        );
    }

    Ok(())
}

/// Format current timestamp
fn format_timestamp() -> String {
    let now = chrono::Local::now();
    now.format("[%H:%M:%S]").to_string()
}
