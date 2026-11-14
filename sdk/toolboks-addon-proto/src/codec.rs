//! Framing: every message is a little-endian `u32` body length followed by a postcard body.
//!
//! The reader checks the length against a hard cap before it allocates, so a bogus length
//! from a broken or hostile peer cannot exhaust memory. Every failure is returned as a
//! [`ProtocolError`]; nothing here panics on bad input.

use crate::ProtocolError;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{ErrorKind, Read, Write};

/// The hard cap on one message body. A first frame with the font atlas is about 370 KB.
pub const MAX_FRAME_BYTES: u32 = 16 * 1024 * 1024;

/// Reads length-prefixed messages from any byte stream (an anonymous pipe, one half of a
/// named pipe pair, a socket or an in-memory pipe).
pub struct FrameReader<R> {
    inner: R,
    max: u32,
}

impl<R: Read> FrameReader<R> {
    pub fn new(inner: R) -> Self {
        Self::with_limit(inner, MAX_FRAME_BYTES)
    }

    pub fn with_limit(inner: R, max: u32) -> Self {
        Self { inner, max }
    }

    /// Reads one message. A stream that ends before the first length byte is
    /// [`ProtocolError::Closed`]; one that ends later is [`ProtocolError::Truncated`].
    pub fn read<T: DeserializeOwned>(&mut self) -> Result<T, ProtocolError> {
        let mut prefix = [0u8; 4];
        let mut filled = 0;
        while filled < prefix.len() {
            match self.inner.read(&mut prefix[filled..]) {
                Ok(0) if filled == 0 => return Err(ProtocolError::Closed),
                Ok(0) => return Err(ProtocolError::Truncated),
                Ok(n) => filled += n,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
        let len = u32::from_le_bytes(prefix);
        if len > self.max {
            return Err(ProtocolError::FrameTooLarge { len, max: self.max });
        }
        let mut body = vec![0u8; len as usize];
        if let Err(error) = self.inner.read_exact(&mut body) {
            return Err(if error.kind() == ErrorKind::UnexpectedEof {
                ProtocolError::Truncated
            } else {
                error.into()
            });
        }
        decode(&body)
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

/// Decodes one message body and rejects trailing bytes.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, ProtocolError> {
    let (value, rest) = postcard::take_from_bytes::<T>(body)
        .map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if !rest.is_empty() {
        return Err(ProtocolError::Malformed(format!(
            "{} trailing bytes",
            rest.len()
        )));
    }
    Ok(value)
}

/// Writes length-prefixed messages and flushes after each one.
pub struct FrameWriter<W> {
    inner: W,
    max: u32,
}

impl<W: Write> FrameWriter<W> {
    pub fn new(inner: W) -> Self {
        Self::with_limit(inner, MAX_FRAME_BYTES)
    }

    pub fn with_limit(inner: W, max: u32) -> Self {
        Self { inner, max }
    }

    pub fn write<T: Serialize>(&mut self, message: &T) -> Result<(), ProtocolError> {
        let body = postcard::to_stdvec(message)
            .map_err(|error| ProtocolError::Malformed(error.to_string()))?;
        let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
        if len > self.max {
            return Err(ProtocolError::FrameTooLarge { len, max: self.max });
        }
        self.inner.write_all(&len.to_le_bytes())?;
        self.inner.write_all(&body)?;
        self.inner.flush()?;
        Ok(())
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

/// A connection made of two independent byte streams, one per direction. Nothing here is
/// tied to stdin and stdout: #80 plugs in a named pipe pair, tests use [`crate::memory`].
pub struct Transport<R, W> {
    pub reader: FrameReader<R>,
    pub writer: FrameWriter<W>,
}

impl<R: Read, W: Write> Transport<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: FrameReader::new(reader),
            writer: FrameWriter::new(writer),
        }
    }

    pub fn split(self) -> (FrameReader<R>, FrameWriter<W>) {
        (self.reader, self.writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostMessage, InputBatch, InputEvent, Theme};

    fn sample() -> HostMessage {
        HostMessage::Input(InputBatch {
            seq: 7,
            size: [640.0, 480.0],
            pixels_per_point: 1.5,
            theme: Theme::Dark,
            window_focused: true,
            events: vec![InputEvent::Text("hé".into()), InputEvent::PointerLeft],
        })
    }

    fn encoded(message: &HostMessage) -> Vec<u8> {
        let mut writer = FrameWriter::new(Vec::new());
        writer.write(message).unwrap();
        writer.into_inner()
    }

    #[test]
    fn round_trip() {
        let bytes = encoded(&sample());
        let mut reader = FrameReader::new(bytes.as_slice());
        assert_eq!(reader.read::<HostMessage>().unwrap(), sample());
        assert!(reader.read::<HostMessage>().unwrap_err().is_closed());
    }

    #[test]
    fn garbage_is_malformed() {
        let mut bytes = 6u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        let error = FrameReader::new(bytes.as_slice())
            .read::<HostMessage>()
            .unwrap_err();
        assert!(matches!(error, ProtocolError::Malformed(_)), "{error:?}");
    }

    #[test]
    fn oversize_length_is_refused_before_allocation() {
        let bytes = u32::MAX.to_le_bytes();
        let error = FrameReader::new(bytes.as_slice())
            .read::<HostMessage>()
            .unwrap_err();
        assert!(matches!(
            error,
            ProtocolError::FrameTooLarge {
                len: u32::MAX,
                max: MAX_FRAME_BYTES
            }
        ));
        let mut writer = FrameWriter::with_limit(Vec::new(), 4);
        assert!(matches!(
            writer.write(&sample()),
            Err(ProtocolError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn truncated_prefix_and_body() {
        let bytes = encoded(&sample());
        for cut in [1, 3, 5, bytes.len() - 1] {
            let error = FrameReader::new(&bytes[..cut])
                .read::<HostMessage>()
                .unwrap_err();
            assert!(
                matches!(error, ProtocolError::Truncated),
                "cut {cut}: {error:?}"
            );
        }
    }

    #[test]
    fn trailing_bytes_are_malformed() {
        let mut body = postcard::to_stdvec(&HostMessage::Quit).unwrap();
        body.push(0);
        let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&body);
        let error = FrameReader::new(bytes.as_slice())
            .read::<HostMessage>()
            .unwrap_err();
        assert!(matches!(error, ProtocolError::Malformed(_)));
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2000 {
            let mut bytes = Vec::new();
            let len = (state % 64) as usize;
            for _ in 0..len {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                bytes.push(state as u8);
            }
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut framed = (bytes.len() as u32).to_le_bytes().to_vec();
            framed.extend_from_slice(&bytes);
            let _ = FrameReader::new(framed.as_slice()).read::<HostMessage>();
            let _ = FrameReader::new(framed.as_slice()).read::<crate::AddonMessage>();
        }
    }
}
