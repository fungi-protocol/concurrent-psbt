//! Generic length-prefixed framing for stream transports.
//!
//! A stream transport (TCP-like: a Tor circuit, a WebRTC data channel, a
//! WebSocket) carries a byte stream with no inherent record boundaries. To put
//! more than one [`crate::Message`] envelope on a single connection it needs to
//! delimit records. This module supplies that delimiter: a `u32` big-endian
//! length prefix followed by the value bytes.
//!
//! This is ORTHOGONAL to [`crate::Message`]: framing delimits records on a
//! stream; `Message` tags what a record *is*. Datagram / document transports
//! (an iroh doc entry, a QR frame, a payjoin mailbox slot) already have record
//! boundaries and skip framing entirely.
//!
//! Two API shapes are provided, sharing the same wire format and the same
//! [`MAX_FRAME_LEN`] cap:
//!
//!   * Buffer form — [`frame`] / [`deframe`]: pure functions over `Vec<u8>`,
//!     for transports that already own their receive buffer and drive their own
//!     I/O (push -> pull transports that buffer bytes as they arrive).
//!   * `std::io` form — [`write_frame`] / [`read_frame`]: for transports that
//!     hold a live [`std::io::Write`] / [`std::io::Read`] stream and want to
//!     read/write one framed record synchronously.

use crate::{Error, Result};
use std::io::{Read, Write};

/// Maximum framed value length: 16 MiB. A length prefix larger than this is
/// rejected rather than trusted (it would ask us to allocate/await an
/// unbounded buffer), so a peer cannot make us reserve arbitrary memory.
pub const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

/// Frame one record: a `u32` big-endian length prefix followed by `value`.
///
/// # Panics
///
/// Panics if `value` is longer than [`MAX_FRAME_LEN`] — that is a local
/// programming error (a transport tried to send a record too large for the
/// protocol), not untrusted input, so it fails loudly at the send site rather
/// than silently truncating.
pub fn frame(value: &[u8]) -> Vec<u8> {
    assert!(
        value.len() <= MAX_FRAME_LEN,
        "frame: value length {} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}",
        value.len()
    );
    // len <= MAX_FRAME_LEN (16 MiB) always fits in u32.
    let len = value.len() as u32;
    let mut out = Vec::with_capacity(4 + value.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(value);
    out
}

/// Try to pull one complete framed record off the front of `buf`.
///
/// Returns:
///   * `Ok(Some(value))` — one full record was present; it has been removed
///     from the front of `buf` and any trailing bytes are retained for the next
///     call.
///   * `Ok(None)` — the buffer does not yet hold a full record (not even the
///     4-byte length prefix, or the prefix but not all the value bytes). Read
///     more from the wire and call again; `buf` is left unchanged.
///   * `Err(_)` — the length prefix declares a record larger than
///     [`MAX_FRAME_LEN`]. `buf` is left unchanged so the caller can surface the
///     error and tear the connection down.
pub fn deframe(buf: &mut Vec<u8>) -> Result<Option<Vec<u8>>> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME_LEN {
        return Err(Error::new(format!(
            "framed record length {len} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}"
        )));
    }
    if buf.len() < 4 + len {
        // Header present but the value hasn't fully arrived yet.
        return Ok(None);
    }
    let value = buf[4..4 + len].to_vec();
    // Drop the consumed header+value, keep any trailing bytes for the next call.
    buf.drain(..4 + len);
    Ok(Some(value))
}

/// Write one framed record to a [`std::io::Write`] stream (length prefix +
/// value), flushing before returning.
///
/// # Errors
///
/// Returns an error if `value` exceeds [`MAX_FRAME_LEN`], or if the underlying
/// write/flush fails.
pub fn write_frame<W: Write>(writer: &mut W, value: &[u8]) -> Result<()> {
    if value.len() > MAX_FRAME_LEN {
        return Err(Error::new(format!(
            "write_frame: value length {} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}",
            value.len()
        )));
    }
    let len = value.len() as u32;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(value)?;
    writer.flush()?;
    Ok(())
}

/// Read exactly one framed record from a [`std::io::Read`] stream.
///
/// Blocks until a full record (length prefix + all value bytes) has been read.
///
/// Returns:
///   * `Ok(Some(value))` — one complete record.
///   * `Ok(None)` — the stream was cleanly at EOF *before any byte of a record*
///     (a clean close on a record boundary).
///   * `Err(_)` — the declared length exceeds [`MAX_FRAME_LEN`], the stream hit
///     EOF partway through a record (truncated), or the underlying read failed.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match read_exact_or_eof(reader, &mut len_buf)? {
        // Nothing at all left on the wire: a clean close on a record boundary.
        ReadOutcome::Eof => return Ok(None),
        // Some prefix bytes arrived but not all four: the record is truncated.
        ReadOutcome::PartialEof(n) => {
            return Err(Error::new(format!(
                "read_frame: stream ended mid length-prefix after {n} of 4 bytes"
            )));
        }
        ReadOutcome::Full => {}
    }

    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(Error::new(format!(
            "read_frame: declared length {len} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}"
        )));
    }

    let mut value = vec![0u8; len];
    match read_exact_or_eof(reader, &mut value)? {
        ReadOutcome::Full => Ok(Some(value)),
        ReadOutcome::Eof if len == 0 => Ok(Some(value)),
        ReadOutcome::Eof | ReadOutcome::PartialEof(_) => Err(Error::new(format!(
            "read_frame: stream ended mid record (expected {len} value bytes)"
        ))),
    }
}

enum ReadOutcome {
    /// The whole buffer was filled.
    Full,
    /// EOF hit before any byte was read.
    Eof,
    /// EOF hit after `n` bytes (0 < n < buf.len()).
    PartialEof(usize),
}

/// Like `Read::read_exact` but distinguishes "clean EOF at start" from "EOF
/// partway through". Retries on `ErrorKind::Interrupted`.
fn read_exact_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<ReadOutcome> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => {
                return Ok(if filled == 0 {
                    ReadOutcome::Eof
                } else {
                    ReadOutcome::PartialEof(filled)
                });
            }
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::from(e)),
        }
    }
    Ok(ReadOutcome::Full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_deframe_roundtrip() {
        let mut buf = frame(b"hello");
        assert_eq!(buf.len(), 4 + 5);
        assert_eq!(deframe(&mut buf).unwrap(), Some(b"hello".to_vec()));
        assert!(buf.is_empty());
    }

    #[test]
    fn deframe_needs_full_header() {
        let mut buf = vec![0, 0, 0]; // only 3 of 4 prefix bytes
        assert_eq!(deframe(&mut buf).unwrap(), None);
        assert_eq!(buf, vec![0, 0, 0]); // left unchanged
    }

    #[test]
    fn deframe_needs_full_value() {
        let mut buf = frame(b"abcdef");
        buf.truncate(buf.len() - 2); // value cut short
        assert_eq!(deframe(&mut buf).unwrap(), None);
        // Buffer untouched: still waiting for the rest of the value.
        assert_eq!(buf.len(), 4 + 6 - 2);
    }

    #[test]
    fn deframe_consumes_one_leaves_rest() {
        let mut buf = frame(b"one");
        buf.extend_from_slice(&frame(b"two"));
        buf.extend_from_slice(&[0xFF]); // partial next header byte
        assert_eq!(deframe(&mut buf).unwrap(), Some(b"one".to_vec()));
        assert_eq!(deframe(&mut buf).unwrap(), Some(b"two".to_vec()));
        assert_eq!(deframe(&mut buf).unwrap(), None);
        assert_eq!(buf, vec![0xFF]); // trailing partial header retained
    }

    #[test]
    fn deframe_empty_value() {
        let mut buf = frame(b"");
        assert_eq!(buf, vec![0, 0, 0, 0]);
        assert_eq!(deframe(&mut buf).unwrap(), Some(Vec::new()));
        assert!(buf.is_empty());
    }

    #[test]
    fn deframe_rejects_oversize_length() {
        // Craft a header claiming MAX_FRAME_LEN + 1 without allocating it.
        let big = (MAX_FRAME_LEN as u32 + 1).to_be_bytes();
        let mut buf = big.to_vec();
        let before = buf.clone();
        assert!(deframe(&mut buf).is_err());
        assert_eq!(buf, before); // left unchanged for the caller to tear down
    }

    #[test]
    #[should_panic(expected = "exceeds MAX_FRAME_LEN")]
    fn frame_panics_on_oversize() {
        // Zero-fill vec of MAX_FRAME_LEN + 1 is large but bounded; keep the test
        // fast by asserting the panic contract only.
        let oversize = vec![0u8; MAX_FRAME_LEN + 1];
        let _ = frame(&oversize);
    }

    #[test]
    fn write_then_read_frame_roundtrip() {
        let mut stream = Vec::new();
        write_frame(&mut stream, b"first").unwrap();
        write_frame(&mut stream, b"second").unwrap();

        let mut cursor = Cursor::new(stream);
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(b"first".to_vec()));
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(b"second".to_vec()));
        // Clean EOF on a record boundary.
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
    }

    #[test]
    fn read_frame_empty_value_roundtrip() {
        let mut stream = Vec::new();
        write_frame(&mut stream, b"").unwrap();
        let mut cursor = Cursor::new(stream);
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(Vec::new()));
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
    }

    #[test]
    fn read_frame_truncated_prefix_is_error() {
        let mut cursor = Cursor::new(vec![0, 0]); // 2 of 4 prefix bytes
        assert!(read_frame(&mut cursor).is_err());
    }

    #[test]
    fn read_frame_truncated_value_is_error() {
        let mut stream = frame(b"abcdef");
        stream.truncate(stream.len() - 2);
        let mut cursor = Cursor::new(stream);
        assert!(read_frame(&mut cursor).is_err());
    }

    #[test]
    fn read_frame_rejects_oversize_length() {
        let big = (MAX_FRAME_LEN as u32 + 1).to_be_bytes();
        let mut cursor = Cursor::new(big.to_vec());
        assert!(read_frame(&mut cursor).is_err());
    }

    #[test]
    fn write_frame_rejects_oversize() {
        let mut sink = Vec::new();
        let oversize = vec![0u8; MAX_FRAME_LEN + 1];
        assert!(write_frame(&mut sink, &oversize).is_err());
        assert!(sink.is_empty()); // nothing written
    }

    #[test]
    fn buffer_and_stream_forms_share_wire_format() {
        // A frame produced by `frame` reads back via `read_frame`, and vice
        // versa — the two API shapes are wire-compatible.
        let via_buffer = frame(b"interop");
        let mut cursor = Cursor::new(via_buffer);
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(b"interop".to_vec()));

        let mut via_stream = Vec::new();
        write_frame(&mut via_stream, b"interop").unwrap();
        assert_eq!(deframe(&mut via_stream).unwrap(), Some(b"interop".to_vec()));
    }
}
