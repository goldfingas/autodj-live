use rmp_serde::{decode, encode};
use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::EngineCommand;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("protocol major version {received} is incompatible with required major {expected}")]
    IncompatibleMajor { expected: u16, received: u16 },
    #[error(
        "invalid frame length {length}; expected 1..={}",
        FrameCodec::MAX_FRAME_LEN
    )]
    InvalidFrameLength { length: u32 },
    #[error(
        "encoded frame is too large: {length} bytes (maximum {})",
        FrameCodec::MAX_FRAME_LEN
    )]
    FrameTooLarge { length: usize },
    #[error("failed to encode MessagePack frame: {0}")]
    Encode(#[from] encode::Error),
    #[error("failed to decode MessagePack frame: {0}")]
    Decode(#[from] decode::Error),
    #[error("frame I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub struct FrameCodec;

impl FrameCodec {
    pub const MAX_FRAME_LEN: usize = 1024 * 1024;

    pub async fn read<R>(reader: &mut R) -> Result<EngineCommand, ProtocolError>
    where
        R: AsyncRead + Unpin,
    {
        let mut header = [0_u8; 4];
        reader.read_exact(&mut header).await?;
        let length = u32::from_be_bytes(header);
        if length == 0 || length as usize > Self::MAX_FRAME_LEN {
            return Err(ProtocolError::InvalidFrameLength { length });
        }

        let mut payload = vec![0_u8; length as usize];
        reader.read_exact(&mut payload).await?;
        Ok(rmp_serde::from_slice(&payload)?)
    }

    pub async fn write<W, T>(writer: &mut W, message: &T) -> Result<(), ProtocolError>
    where
        W: AsyncWrite + Unpin,
        T: Serialize,
    {
        let payload = rmp_serde::to_vec_named(message)?;
        if payload.len() > Self::MAX_FRAME_LEN {
            return Err(ProtocolError::FrameTooLarge {
                length: payload.len(),
            });
        }

        let length = u32::try_from(payload.len()).map_err(|_| ProtocolError::FrameTooLarge {
            length: payload.len(),
        })?;
        writer.write_all(&length.to_be_bytes()).await?;
        writer.write_all(&payload).await?;
        Ok(())
    }
}
