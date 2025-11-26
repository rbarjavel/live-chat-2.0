use crate::error::{ChatError, Result};
use crate::media::MediaType;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Message {
    TextMessage {
        sender: String,
        content: String,
        timestamp: u64,
    },
    FileTransfer {
        sender: String,
        filename: String,
        compressed_data: Vec<u8>,
        original_size: usize,
    },
    MediaTransfer {
        sender: String,
        filename: String,
        media_type: MediaType,
        checksum: String,
        compressed_data: Vec<u8>,
        original_size: usize,
        caption: Option<String>,
    },
    UserJoined {
        username: String,
    },
    UserLeft {
        username: String,
    },
    ServerInfo {
        message: String,
    },
    /// Client requests server to download media from URL
    UrlDownloadRequest {
        requester: String,      // Username who initiated request
        url: String,            // URL to download
        caption: Option<String>, // Optional caption
    },
    /// Server sends download progress updates
    DownloadProgress {
        requester: String,       // Who to notify
        url: String,             // Which download
        percent: f32,            // 0.0-100.0
        status: String,          // Human-readable status
    },
    /// Server reports download failure
    DownloadError {
        requester: String,
        url: String,
        error: String,
    },
    /// Client acknowledges media received and cached (for synchronized playback)
    MediaReady {
        media_id: String,   // Checksum identifying the media
        username: String,    // Who is ready
    },
    /// Server signals all clients to start playback simultaneously
    PlaybackStart {
        media_id: String,   // Which media to play
        countdown: u8,       // Optional 3-2-1 countdown (0 = play now)
    },
    /// Server notifies clients about synchronization status
    PlaybackWaiting {
        media_id: String,
        ready_users: Vec<String>,    // Users who are ready
        waiting_users: Vec<String>,  // Users still receiving
    },
}

impl Message {
    /// Serialize the message to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(Into::into)
    }

    /// Deserialize a message from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(Into::into)
    }

    /// Write a message to a TCP stream with length framing
    /// Format: [4 bytes: length (big-endian)][N bytes: serialized message]
    pub async fn write_to_stream(&self, stream: &mut TcpStream) -> Result<()> {
        let bytes = self.to_bytes()?;
        #[allow(clippy::cast_possible_truncation)]
        let len = bytes.len() as u32;

        // Write 4-byte length header
        stream.write_all(&len.to_be_bytes()).await?;

        // Write message payload
        stream.write_all(&bytes).await?;

        stream.flush().await?;
        Ok(())
    }

    /// Read a message from a TCP stream with length framing
    /// Handles partial reads properly
    pub async fn read_from_stream(stream: &mut TcpStream) -> Result<Self> {
        // Read 4-byte length header
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ChatError::ConnectionClosed
            } else {
                ChatError::Io(e)
            }
        })?;

        let len = u32::from_be_bytes(len_bytes) as usize;

        // Validate message length (prevent DOS attacks)
        if len > 100_000_000 {
            // 100MB max
            return Err(ChatError::Protocol(format!(
                "Message too large: {len} bytes"
            )));
        }

        // Read message payload
        let mut buffer = vec![0u8; len];
        stream.read_exact(&mut buffer).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ChatError::Protocol("Incomplete message received".to_string())
            } else {
                ChatError::Io(e)
            }
        })?;

        // Deserialize
        Self::from_bytes(&buffer)
    }

    /// Write a message to an owned write half
    pub async fn write_to_writer(&self, writer: &mut OwnedWriteHalf) -> Result<()> {
        let bytes = self.to_bytes()?;
        #[allow(clippy::cast_possible_truncation)]
        let len = bytes.len() as u32;

        writer.write_all(&len.to_be_bytes()).await?;
        writer.write_all(&bytes).await?;
        writer.flush().await?;
        Ok(())
    }

    /// Read a message from an owned read half
    pub async fn read_from_reader(reader: &mut OwnedReadHalf) -> Result<Self> {
        let mut len_bytes = [0u8; 4];
        reader.read_exact(&mut len_bytes).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ChatError::ConnectionClosed
            } else {
                ChatError::Io(e)
            }
        })?;

        let len = u32::from_be_bytes(len_bytes) as usize;

        if len > 100_000_000 {
            return Err(ChatError::Protocol(format!(
                "Message too large: {len} bytes"
            )));
        }

        let mut buffer = vec![0u8; len];
        reader.read_exact(&mut buffer).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ChatError::Protocol("Incomplete message received".to_string())
            } else {
                ChatError::Io(e)
            }
        })?;

        Self::from_bytes(&buffer)
    }
}
