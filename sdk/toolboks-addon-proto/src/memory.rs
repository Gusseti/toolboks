//! An in-memory, blocking, one-direction byte pipe. Two of them make a duplex connection
//! for tests and in-process harnesses. Dropping the writer ends the stream for the reader
//! (EOF); dropping the reader makes later writes fail with `BrokenPipe`.

use crate::Transport;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[derive(Default)]
struct State {
    buffer: VecDeque<u8>,
    writer_closed: bool,
    reader_closed: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

pub struct PipeReader(Arc<Shared>);
pub struct PipeWriter(Arc<Shared>);

pub fn pipe() -> (PipeReader, PipeWriter) {
    let shared = Arc::new(Shared::default());
    (PipeReader(shared.clone()), PipeWriter(shared))
}

/// A memory-pipe connection end.
pub type MemoryTransport = Transport<PipeReader, PipeWriter>;

/// Two pipes wired as a connection: `(host side, add-on side)`.
pub fn duplex() -> (MemoryTransport, MemoryTransport) {
    let (to_addon_r, to_addon_w) = pipe();
    let (to_host_r, to_host_w) = pipe();
    (
        Transport::new(to_host_r, to_addon_w),
        Transport::new(to_addon_r, to_host_w),
    )
}

impl PipeWriter {
    /// Writes raw bytes, bypassing framing; tests use it to inject garbage.
    pub fn write_raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write_all(bytes)
    }
}

fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut state = lock(&self.0);
        while state.buffer.is_empty() && !state.writer_closed {
            state = self
                .0
                .ready
                .wait(state)
                .unwrap_or_else(|poison| poison.into_inner());
        }
        let n = out.len().min(state.buffer.len());
        for (slot, byte) in out.iter_mut().zip(state.buffer.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        lock(&self.0).reader_closed = true;
        self.0.ready.notify_all();
    }
}

impl Write for PipeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = lock(&self.0);
        if state.reader_closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "reader dropped"));
        }
        state.buffer.extend(bytes);
        self.0.ready.notify_all();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        lock(&self.0).writer_closed = true;
        self.0.ready.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostMessage, ProtocolError};

    #[test]
    fn eof_after_writer_drop_and_broken_pipe_after_reader_drop() {
        let (mut host, addon) = duplex();
        host.writer.write(&HostMessage::Quit).unwrap();
        let (mut reader, writer) = addon.split();
        drop(writer);
        assert_eq!(reader.read::<HostMessage>().unwrap(), HostMessage::Quit);
        drop(reader);
        assert!(matches!(
            host.writer.write(&HostMessage::Quit),
            Err(ProtocolError::Closed)
        ));
        assert!(host.reader.read::<HostMessage>().unwrap_err().is_closed());
    }
}
