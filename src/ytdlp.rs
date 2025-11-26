use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

/// Metadata extracted from yt-dlp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub title: String,
    pub duration: Option<u32>,     // seconds
    pub ext: String,                // File extension
    pub filesize: Option<u64>,      // bytes
    pub thumbnail: Option<String>,  // URL
}

/// Check if yt-dlp is installed
pub fn check_ytdlp_installed() -> bool {
    Command::new("yt-dlp")
        .arg("--version")
        .output()
        .is_ok()
}

/// Extract metadata without downloading (fast)
pub async fn get_media_info(url: &str) -> Result<MediaInfo, String> {
    let output = TokioCommand::new("yt-dlp")
        .arg("--dump-json")
        .arg("--no-download")
        .arg(url)
        .output()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // Parse JSON output
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse yt-dlp JSON: {e}"))?;

    Ok(MediaInfo {
        title: json["title"].as_str().unwrap_or("Unknown").to_string(),
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        duration: json["duration"].as_f64().map(|d| d as u32),
        ext: json["ext"].as_str().unwrap_or("mp4").to_string(),
        filesize: json["filesize"].as_u64(),
        thumbnail: json["thumbnail"].as_str().map(String::from),
    })
}

/// Download media to specified path with progress callback
pub async fn download_media<F>(
    url: &str,
    output_path: &Path,
    cookies_file: Option<&Path>,
    mut progress_callback: F,
) -> Result<PathBuf, String>
where
    F: FnMut(f32, String) + Send + 'static,
{
    let mut cmd = TokioCommand::new("yt-dlp");

    // Output file
    cmd.arg("-o").arg(output_path);

    // Use cookies if provided
    if let Some(cookies) = cookies_file {
        cmd.arg("--cookies").arg(cookies);
    }

    // Format selection (best quality video, prefer MP4 but accept others)
    cmd.arg("-f")
        .arg("bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b");

    // Progress tracking
    cmd.arg("--newline"); // Output each progress line
    cmd.arg("--no-warnings");

    // Add URL
    cmd.arg(url);

    // Spawn process
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn yt-dlp: {e}"))?;

    // Read both stdout and stderr to prevent deadlock
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Spawn task to read and parse stdout for progress
    if let Some(stdout) = stdout {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                // Parse yt-dlp progress lines
                // Format: "[download]  45.2% of 10.50MiB at 1.20MiB/s ETA 00:04"
                if line.contains("[download]") && line.contains('%') {
                    if let Some(percent_str) = line.split('%').next() {
                        if let Some(percent_part) = percent_str.split_whitespace().last() {
                            if let Ok(percent) = percent_part.parse::<f32>() {
                                progress_callback(percent, line.clone());
                            }
                        }
                    }
                }
            }
        });
    }

    // Spawn task to consume stderr (prevents deadlock)
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // Log stderr for debugging
                eprintln!("[yt-dlp] {line}");
            }
        });
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("yt-dlp process error: {e}"))?;

    if !status.success() {
        return Err(format!(
            "yt-dlp exited with code: {}",
            status.code().unwrap_or(-1)
        ));
    }

    Ok(output_path.to_path_buf())
}
