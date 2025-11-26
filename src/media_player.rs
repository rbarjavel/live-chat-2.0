use crate::media::MediaType;
use std::path::Path;

/// Trait for media playback
/// Implementations can display/play media in different ways
pub trait MediaPlayer: Send + Sync {
    /// Play media with cross-platform path
    ///
    /// # Arguments
    /// * `media_path` - Path to the cached media file
    /// * `media_type` - Type of media (Image or Video)
    /// * `caption` - Optional caption text
    /// * `sender` - Optional sender username
    ///
    /// # Errors
    /// Returns error if media cannot be displayed/played
    fn play_media(
        &self,
        media_path: &Path,
        media_type: &MediaType,
        caption: Option<&str>,
        sender: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

/// Default media player that prints media info and simulates playback
#[allow(dead_code)]
pub struct DefaultMediaPlayer;

impl MediaPlayer for DefaultMediaPlayer {
    fn play_media(
        &self,
        media_path: &Path,
        media_type: &MediaType,
        caption: Option<&str>,
        sender: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n{}", "=".repeat(60));
        println!("MEDIA PLAYBACK");
        println!("{}", "=".repeat(60));

        // Use .display() for cross-platform path printing
        println!("File: {}", media_path.display());
        println!("Type: {media_type:?}");

        if let Some(sender_name) = sender {
            println!("From: {sender_name}");
        }

        if let Some(caption_text) = caption {
            println!("\n📷 Caption: {caption_text}\n");
        }

        println!("\n[The media file is available at: {}]", media_path.display());

        // Simulate playback duration
        let duration = match media_type {
            MediaType::Image => 3,
            MediaType::Video { duration_secs } => *duration_secs,
        };

        if duration > 0 {
            println!("Simulating {duration}s playback...\n");
            std::thread::sleep(std::time::Duration::from_secs(u64::from(duration)));
        }

        println!("{}\n", "=".repeat(60));

        Ok(())
    }
}

/// Media player that uses system default viewer
/// Opens media files with the system's default application
pub struct SystemViewerPlayer;

impl MediaPlayer for SystemViewerPlayer {
    fn play_media(
        &self,
        media_path: &Path,
        media_type: &MediaType,
        caption: Option<&str>,
        sender: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(sender_name) = sender {
            println!("\n👤 From: {sender_name}");
        }

        if let Some(caption_text) = caption {
            println!("📷 Caption: {caption_text}\n");
        }

        // Open with system default viewer (works on Linux, Windows, macOS)
        println!("Opening media file with system viewer...");
        println!("File: {}", media_path.display());
        println!("Type: {media_type:?}");

        opener::open(media_path)?;

        // Simulate playback duration for videos
        let duration = match media_type {
            MediaType::Image => 3,
            MediaType::Video { duration_secs } => *duration_secs,
        };

        if duration > 0 {
            println!("[Media opened in system viewer, displaying for {duration}s...]");
            std::thread::sleep(std::time::Duration::from_secs(u64::from(duration)));
        }

        Ok(())
    }
}

/// Console-only media player that just prints info without opening files
#[allow(dead_code)]
pub struct ConsoleMediaPlayer;

impl MediaPlayer for ConsoleMediaPlayer {
    fn play_media(
        &self,
        media_path: &Path,
        media_type: &MediaType,
        caption: Option<&str>,
        sender: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match media_type {
            MediaType::Image => println!("\n🖼️  Image received"),
            MediaType::Video { duration_secs } => {
                println!("\n🎬 Video received ({duration_secs}s)");
            }
        }

        if let Some(sender_name) = sender {
            println!("   From: {sender_name}");
        }

        if let Some(caption_text) = caption {
            println!("   Caption: {caption_text}");
        }

        println!("   File: {}", media_path.display());

        Ok(())
    }
}

/// Media player that uses the Godot video player for videos
/// Falls back to system viewer for images
pub struct GodotMediaPlayer {
    pub player_path: std::path::PathBuf,
}

impl GodotMediaPlayer {
    /// Create a new Godot media player
    ///
    /// # Arguments
    /// * `player_path` - Path to the Godot player executable
    ///
    /// # Examples
    /// ```no_run
    /// use std::path::PathBuf;
    /// let player = GodotMediaPlayer::new(PathBuf::from("godot-player/godot-player.exe"));
    /// ```
    #[must_use]
    pub const fn new(player_path: std::path::PathBuf) -> Self {
        Self { player_path }
    }

    /// Try to find the Godot player executable in common locations
    ///
    /// Searches in the following order:
    /// 1. Environment variable `GODOT_PLAYER_PATH`
    /// 2. `./godot-player/godot-player.exe` (Windows)
    /// 3. `./godot-player/godot-player` (Linux/macOS)
    /// 4. `godot-player.exe` in current directory
    /// 5. `godot-player` in current directory
    ///
    /// # Errors
    /// Returns `None` if the player cannot be found
    pub fn find_player() -> Option<Self> {
        use std::path::PathBuf;

        // Check environment variable
        if let Ok(path) = std::env::var("GODOT_PLAYER_PATH") {
            let player_path = PathBuf::from(path);
            if player_path.exists() {
                return Some(Self::new(player_path));
            }
        }

        // Check common locations
        let possible_paths = vec![
            "godot-player/Homies Video Player.x86_64",     // Linux export
            "godot-player/Homies Video Player.exe",        // Windows export
            "Homies Video Player.x86_64",                  // Linux in current dir
            "Homies Video Player.exe",                     // Windows in current dir
            "../godot-player/Homies Video Player.x86_64",  // Linux one level up
            "../godot-player/Homies Video Player.exe",     // Windows one level up
        ];

        for path_str in possible_paths {
            let path = PathBuf::from(path_str);
            if path.exists() {
                return Some(Self::new(path));
            }
        }

        None
    }

    /// Get video dimensions from file using ffprobe
    /// Returns None if ffprobe is not available or if dimensions cannot be determined
    ///
    /// # Arguments
    /// * `video_path` - Path to the video file
    ///
    /// # Examples
    /// ```no_run
    /// use std::path::Path;
    /// let player = GodotMediaPlayer::find_player().unwrap();
    /// let dimensions = player.get_video_dimensions(Path::new("video.mp4"));
    /// ```
    #[allow(clippy::unused_self)]
    fn get_video_dimensions(&self, video_path: &Path) -> Option<(u32, u32)> {
        // Try to use ffprobe to get video dimensions
        let output = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "json",
            ])
            .arg(video_path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        // Parse JSON output
        let json_str = String::from_utf8(output.stdout).ok()?;
        let json: serde_json::Value = serde_json::from_str(&json_str).ok()?;

        let width = u32::try_from(json["streams"][0]["width"].as_u64()?).ok()?;
        let height = u32::try_from(json["streams"][0]["height"].as_u64()?).ok()?;

        Some((width, height))
    }

    /// Get image dimensions using `ImageMagick`'s identify command
    /// Returns None if identify is not available or if dimensions cannot be determined
    ///
    /// # Arguments
    /// * `image_path` - Path to the image file
    ///
    /// # Examples
    /// ```no_run
    /// use std::path::Path;
    /// let player = GodotMediaPlayer::find_player().unwrap();
    /// let dimensions = player.get_image_dimensions(Path::new("image.jpg"));
    /// ```
    #[allow(clippy::unused_self)]
    fn get_image_dimensions(&self, image_path: &Path) -> Option<(u32, u32)> {
        // Try to use ImageMagick's identify to get image dimensions
        let output = std::process::Command::new("identify")
            .args(["-format", "%w %h"])
            .arg(image_path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        // Parse output: "width height"
        let output_str = String::from_utf8(output.stdout).ok()?;
        let parts: Vec<&str> = output_str.split_whitespace().collect();

        if parts.len() != 2 {
            return None;
        }

        let width = parts[0].parse::<u32>().ok()?;
        let height = parts[1].parse::<u32>().ok()?;

        Some((width, height))
    }
}

impl MediaPlayer for GodotMediaPlayer {
    fn play_media(
        &self,
        media_path: &Path,
        media_type: &MediaType,
        caption: Option<&str>,
        sender: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Determine media type string and get dimensions
        let (media_type_str, dimensions) = match media_type {
            MediaType::Video { duration_secs } => {
                println!("\n🎬 Playing video with Godot player...");
                println!("   File: {}", media_path.display());
                println!("   Duration: {duration_secs}s");
                ("video", self.get_video_dimensions(media_path))
            }
            MediaType::Image => {
                println!("\n🖼️  Displaying image with Godot player...");
                println!("   File: {}", media_path.display());
                ("image", self.get_image_dimensions(media_path))
            }
        };

        if let Some(sender_name) = sender {
            println!("   From: {sender_name}");
        }

        if let Some(caption_text) = caption {
            println!("   Caption: {caption_text}");
        }

        // Build command
        let mut cmd = std::process::Command::new(&self.player_path);

        // Add file path (convert to absolute path)
        let absolute_path = if media_path.is_absolute() {
            media_path.to_path_buf()
        } else {
            std::env::current_dir()?.join(media_path)
        };
        cmd.arg(absolute_path.to_string_lossy().to_string());

        // Add media type
        cmd.arg(media_type_str);

        // Add caption if provided
        if let Some(caption_text) = caption {
            cmd.arg(caption_text);
        } else {
            cmd.arg("");  // Empty caption
        }

        // Add sender if provided
        if let Some(sender_name) = sender {
            cmd.arg(sender_name);
        } else {
            cmd.arg("");  // Empty sender
        }

        // Add dimensions if available
        if let Some((width, height)) = dimensions {
            println!("   Dimensions: {width}x{height}");
            cmd.arg(width.to_string());
            cmd.arg(height.to_string());
        } else {
            println!("   Dimensions: Using Godot defaults (detection tool not available)");
        }

        // Execute and wait
        println!("\n   Launching Godot player...");
        let status = cmd.status()?;

        if !status.success() {
            return Err(format!(
                "Godot player exited with code: {}",
                status.code().unwrap_or(-1)
            )
            .into());
        }

        println!("   Media playback complete!\n");
        Ok(())
    }
}
