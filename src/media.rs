use crate::error::ChatError;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Video { duration_secs: u32 },
}

/// Extract file extension in a cross-platform way
pub fn get_extension(filename: &str) -> &str {
    Path::new(filename)
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("bin")
}

/// Detect media type from file extension (case-insensitive)
pub fn detect_media_type(filename: &str) -> Result<MediaType, ChatError> {
    let path = Path::new(filename);
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(std::string::ToString::to_string)
        .map(|s| s.to_lowercase())
        .ok_or_else(|| ChatError::Protocol("File has no extension".to_string()))?;

    match extension.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "ico" => Ok(MediaType::Image),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" | "m4v" => {
            Ok(MediaType::Video { duration_secs: 5 }) // Default 5s, can be updated
        }
        _ => Err(ChatError::Protocol(format!(
            "Unsupported media type: {extension}"
        ))),
    }
}

/// Calculate SHA-256 checksum of data
pub fn calculate_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_extraction() {
        assert_eq!(get_extension("photo.jpg"), "jpg");
        assert_eq!(get_extension("video.mp4"), "mp4");
        assert_eq!(get_extension("file.TAR.GZ"), "GZ");
        assert_eq!(get_extension("noext"), "bin");
    }

    #[test]
    fn test_media_type_detection() {
        assert!(matches!(
            detect_media_type("photo.jpg"),
            Ok(MediaType::Image)
        ));
        assert!(matches!(
            detect_media_type("photo.PNG"),
            Ok(MediaType::Image)
        ));
        assert!(matches!(
            detect_media_type("video.mp4"),
            Ok(MediaType::Video { .. })
        ));
        assert!(matches!(
            detect_media_type("video.MOV"),
            Ok(MediaType::Video { .. })
        ));
        assert!(detect_media_type("document.pdf").is_err());
    }

    #[test]
    fn test_sha256_calculation() {
        let data = b"Hello, World!";
        let checksum = calculate_sha256(data);
        assert_eq!(checksum.len(), 64); // SHA-256 produces 64 hex characters
        assert_eq!(
            checksum,
            "dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f"
        );
    }
}
