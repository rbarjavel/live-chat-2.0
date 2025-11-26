use crate::error::{ChatError, Result};
use crate::media::{calculate_sha256, get_extension, MediaType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Get the platform-appropriate cache directory
/// Linux: ~/.cache/image-chat/media-cache/
/// Windows: C:\Users\{username}\AppData\Local\image-chat\media-cache\
/// macOS: ~/Library/Caches/image-chat/media-cache/
fn get_cache_directory() -> Result<PathBuf> {
    let cache_base = dirs::cache_dir().ok_or_else(|| {
        ChatError::Protocol("Failed to determine cache directory for this platform".to_string())
    })?;

    let app_cache = cache_base.join("image-chat").join("media-cache");

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&app_cache)?;

    Ok(app_cache)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMedia {
    pub checksum: String,
    pub local_path: PathBuf,
    pub filename: String,
    pub media_type: MediaType,
    pub cached_at: SystemTime,
}

pub struct MediaCache {
    cache_dir: PathBuf,
    index_path: PathBuf,
    index: HashMap<String, CachedMedia>,
}

impl MediaCache {
    /// Initialize cache with platform-appropriate directory
    pub fn new() -> Result<Self> {
        let cache_dir = get_cache_directory()?;
        let index_path = cache_dir.join("index.json");

        let index = if index_path.exists() {
            Self::load_index(&index_path)?
        } else {
            HashMap::new()
        };

        Ok(Self {
            cache_dir,
            index_path,
            index,
        })
    }

    /// Get path for a cached file
    /// Returns: `{cache_dir}/{checksum}.{ext}`
    fn get_cache_path(&self, checksum: &str, extension: &str) -> PathBuf {
        self.cache_dir.join(format!("{checksum}.{extension}"))
    }

    /// Check if media is already cached
    pub fn get(&self, checksum: &str) -> Option<&CachedMedia> {
        self.index.get(checksum)
    }

    /// Save media to cache
    pub fn save(
        &mut self,
        checksum: &str,
        filename: &str,
        data: &[u8],
        media_type: MediaType,
    ) -> Result<PathBuf> {
        // Get extension from filename
        let extension = get_extension(filename);

        // Construct cache path
        let cache_path = self.get_cache_path(checksum, extension);

        // Write file
        std::fs::write(&cache_path, data)?;

        // Update cache index
        self.index.insert(
            checksum.to_string(),
            CachedMedia {
                checksum: checksum.to_string(),
                local_path: cache_path.clone(),
                filename: filename.to_string(),
                media_type,
                cached_at: SystemTime::now(),
            },
        );

        self.save_index()?;

        Ok(cache_path)
    }

    /// Verify cached file exists and matches checksum
    pub fn verify(&self, checksum: &str) -> Result<bool> {
        if let Some(cached) = self.index.get(checksum) {
            if !cached.local_path.exists() {
                return Ok(false);
            }

            // Read and verify checksum
            let data = std::fs::read(&cached.local_path)?;
            let actual_checksum = calculate_sha256(&data);
            Ok(actual_checksum == checksum)
        } else {
            Ok(false)
        }
    }

    /// Save index to disk (cross-platform)
    fn save_index(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.index)?;
        std::fs::write(&self.index_path, json)?;
        Ok(())
    }

    /// Load index from disk (cross-platform)
    fn load_index(path: &Path) -> Result<HashMap<String, CachedMedia>> {
        let json = std::fs::read_to_string(path)?;
        let index = serde_json::from_str(&json)?;
        Ok(index)
    }

    /// Get cache statistics
    #[allow(dead_code)]
    pub fn stats(&self) -> CacheStats {
        let total_files = self.index.len();
        let total_size: u64 = self
            .index
            .values()
            .filter_map(|cached| std::fs::metadata(&cached.local_path).ok())
            .map(|meta| meta.len())
            .sum();

        CacheStats {
            total_files,
            total_size,
        }
    }

    /// Clear all cached media
    #[allow(dead_code)]
    pub fn clear(&mut self) -> Result<()> {
        for cached in self.index.values() {
            if cached.local_path.exists() {
                std::fs::remove_file(&cached.local_path)?;
            }
        }

        self.index.clear();
        self.save_index()?;

        Ok(())
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct CacheStats {
    pub total_files: usize,
    pub total_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_directory_creation() {
        let cache_dir = get_cache_directory().expect("Should create cache dir");
        assert!(cache_dir.exists());

        // Verify it's in the right location for the platform
        #[cfg(target_os = "windows")]
        assert!(cache_dir.to_string_lossy().contains("AppData"));

        #[cfg(target_os = "linux")]
        assert!(cache_dir.to_string_lossy().contains(".cache"));

        #[cfg(target_os = "macos")]
        assert!(cache_dir.to_string_lossy().contains("Library"));
    }

    #[test]
    fn test_cache_operations() {
        let mut cache = MediaCache::new().expect("Should create cache");

        // Test saving
        let test_data = b"test image data";
        let checksum = calculate_sha256(test_data);
        let path = cache
            .save(&checksum, "test.jpg", test_data, MediaType::Image)
            .expect("Should save to cache");

        assert!(path.exists());

        // Test retrieval
        let cached = cache.get(&checksum).expect("Should find in cache");
        assert_eq!(cached.filename, "test.jpg");
        assert_eq!(cached.media_type, MediaType::Image);

        // Test verification
        assert!(cache.verify(&checksum).expect("Should verify"));

        // Cleanup
        std::fs::remove_file(path).ok();
    }
}
