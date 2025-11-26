use crate::error::{ChatError, Result};
use flate2::read::{DeflateDecoder, DeflateEncoder};
use flate2::Compression;
use std::io::Read;

/// Compression threshold: only compress files larger than 1KB
const COMPRESSION_THRESHOLD: usize = 1024;

/// Compress data using flate2 with default compression level
/// Only compresses if data size exceeds the threshold
pub fn compress(data: &[u8]) -> Result<Vec<u8>> {
    // Don't compress if below threshold
    if data.len() <= COMPRESSION_THRESHOLD {
        return Ok(data.to_vec());
    }

    let mut encoder = DeflateEncoder::new(data, Compression::default());
    let mut compressed = Vec::new();

    encoder
        .read_to_end(&mut compressed)
        .map_err(|e| ChatError::Compression(e.to_string()))?;

    // Only use compression if it actually reduces size
    if compressed.len() < data.len() {
        Ok(compressed)
    } else {
        Ok(data.to_vec())
    }
}

/// Decompress data using flate2
/// Verifies the decompressed size matches the expected original size
pub fn decompress(compressed_data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    // If data is below threshold, it wasn't compressed
    if expected_size <= COMPRESSION_THRESHOLD {
        if compressed_data.len() != expected_size {
            return Err(ChatError::Decompression(format!(
                "Size mismatch: expected {}, got {}",
                expected_size,
                compressed_data.len()
            )));
        }
        return Ok(compressed_data.to_vec());
    }

    let mut decoder = DeflateDecoder::new(compressed_data);
    let mut decompressed = Vec::new();

    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| ChatError::Decompression(e.to_string()))?;

    // Verify size matches
    if decompressed.len() != expected_size {
        return Err(ChatError::Decompression(format!(
            "Size mismatch after decompression: expected {}, got {}",
            expected_size,
            decompressed.len()
        )));
    }

    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_small_data() {
        let data = b"Hello";
        let compressed = compress(data).expect("Compression failed");
        // Small data should not be compressed
        assert_eq!(compressed, data);
    }

    #[test]
    fn test_compress_decompress_large_data() {
        let data = vec![b'A'; 5000]; // 5KB of 'A's
        let original_size = data.len();

        let compressed = compress(&data).expect("Compression failed");
        let decompressed =
            decompress(&compressed, original_size).expect("Decompression failed");

        assert_eq!(data, decompressed);
        assert!(compressed.len() < data.len()); // Should be compressed
    }

    #[test]
    fn test_decompress_size_mismatch() {
        let data = vec![b'B'; 2000];
        let compressed = compress(&data).expect("Compression failed");

        // Try to decompress with wrong expected size
        let result = decompress(&compressed, 1000);
        assert!(result.is_err());
    }
}
