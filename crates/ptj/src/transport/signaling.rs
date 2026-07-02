//! Manual file-based signaling for the WebRTC transports (str0m / webrtc-rs).
//!
//! WebRTC needs an out-of-band exchange of SDP offer/answer + trickle-ICE
//! candidate blobs BEFORE a data channel exists. The intended carrier is the
//! oblivious BIP-77 payjoin-directory mailbox over OHTTP (its own transport
//! crate, transport-payjoin-dir — owned externally; see
//! TODO(transport-payjoin-dir) in `commands/sync.rs`). Until that lands, this
//! module provides the MANUAL stopgap the CLI/webgui can use point-to-point
//! today: each peer appends its outbound blobs to its `--signal-out` file and
//! polls its `--signal-in` file (the peer's `--signal-out`, moved by any
//! out-of-band means — a shared directory, sneakernet, chat).
//!
//! # Wire format: one hex line per blob
//!
//! Every blob (an SDP offer/answer or an ICE candidate — both multi-line text,
//! opaque to ptj) is lowercase-hex encoded and written as ONE line. Lines make
//! incremental polling trivial (count lines consumed so far) and survive
//! copy-paste; hex avoids a base64 dependency and keeps blobs 8-bit clean.
//! Blank lines are ignored so hand-assembled files are forgiving.
//!
//! No privacy/threat-model reasoning lives here: this moves opaque bytes
//! between two files. (The MANUAL mode's privacy posture is the human's — the
//! files travel however the human moves them.)

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{Error, Result};

/// How often the incoming signal file is re-read while waiting for a blob.
// NOTE on the `cfg_attr` allows below: the blocking-wait machinery (this
// const, the `timeout` field, and `wait_next_blob`) is called by the str0m
// MANUAL handshake driver in `commands/sync.rs` — and by this module's tests
// in every feature state. webrtc-rs runs its own handshake over plain
// push/poll, so a webrtc-rs-only lib build never reaches it; allow the dead
// code there rather than cfg-splitting the type per feature (the same posture
// as transport-webrtc-rs's own feature-off helpers).
#[cfg_attr(not(feature = "str0m"), allow(dead_code))]
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// A pair of signal files acting as a manual signaling channel: `push_blob`
/// appends hex lines to `outgoing`, `poll_blobs` returns the not-yet-consumed
/// lines of `incoming` (a fresh snapshot, like a transport `recv`).
pub(crate) struct FileSignaling {
    outgoing: PathBuf,
    incoming: PathBuf,
    /// Lines of `incoming` already consumed (the file is append-only).
    consumed: usize,
    /// Decoded-but-not-yet-returned blobs (a read may surface several lines;
    /// `wait_next_blob` hands them out one at a time).
    pending: VecDeque<Vec<u8>>,
    /// Bound on every blocking wait (`--signal-timeout-ms`).
    #[cfg_attr(not(feature = "str0m"), allow(dead_code))]
    timeout: Duration,
}

impl FileSignaling {
    /// A signaling channel appending to `outgoing` and polling `incoming`,
    /// with every blocking wait bounded by `timeout`.
    pub(crate) fn new(outgoing: PathBuf, incoming: PathBuf, timeout: Duration) -> Self {
        Self {
            outgoing,
            incoming,
            consumed: 0,
            pending: VecDeque::new(),
            timeout,
        }
    }

    /// Append one opaque blob to the outgoing signal file as a hex line.
    pub(crate) fn push_blob(&mut self, blob: &[u8]) -> Result<()> {
        use std::io::Write as _;
        let mut line = hex_encode(blob);
        line.push('\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.outgoing)
            .map_err(|error| {
                Error::new(format!(
                    "opening signal-out file {}: {error}",
                    self.outgoing.display()
                ))
            })?;
        file.write_all(line.as_bytes()).map_err(|error| {
            Error::new(format!(
                "appending to signal-out file {}: {error}",
                self.outgoing.display()
            ))
        })?;
        Ok(())
    }

    /// Return every blob that has arrived since the last poll (non-blocking).
    /// A missing incoming file is simply "nothing yet", not an error — the
    /// peer may not have started.
    pub(crate) fn poll_blobs(&mut self) -> Result<Vec<Vec<u8>>> {
        self.fetch()?;
        Ok(self.pending.drain(..).collect())
    }

    /// Block (poll + sleep) until one blob is available and return it,
    /// retaining any extras for later polls. `what` names the expected blob
    /// ("SDP offer", "SDP answer") for the timeout error.
    #[cfg_attr(not(feature = "str0m"), allow(dead_code))]
    pub(crate) fn wait_next_blob(&mut self, what: &str) -> Result<Vec<u8>> {
        let deadline = Instant::now() + self.timeout;
        loop {
            self.fetch()?;
            if let Some(blob) = self.pending.pop_front() {
                return Ok(blob);
            }
            if Instant::now() >= deadline {
                return Err(Error::new(format!(
                    "timed out after {}ms waiting for the peer's {what} in {} \
                     (is the peer running, and is its --signal-out delivered to this file?)",
                    self.timeout.as_millis(),
                    self.incoming.display()
                )));
            }
            std::thread::sleep(POLL_INTERVAL.min(self.timeout));
        }
    }

    /// Read any new lines of the incoming file into `pending`.
    fn fetch(&mut self) -> Result<()> {
        let text = match std::fs::read_to_string(&self.incoming) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(Error::new(format!(
                    "reading signal-in file {}: {error}",
                    self.incoming.display()
                )));
            }
        };
        for line in text.lines().skip(self.consumed) {
            self.consumed += 1;
            if line.trim().is_empty() {
                continue;
            }
            self.pending
                .push_back(hex_decode(line.trim()).map_err(|error| {
                    Error::new(format!(
                        "signal-in file {} line {}: {error}",
                        self.incoming.display(),
                        self.consumed
                    ))
                })?);
        }
        Ok(())
    }
}

// The webrtc-rs backend runs its own offer/answer + trickle-ICE handshake over
// this narrow port (its blobs are self-describing JSON, so plain push/poll is
// the whole contract). str0m needs no trait — ptj drives its manual handshake
// directly (see `commands/sync.rs`).
#[cfg(feature = "webrtc-rs")]
impl transport_webrtc_rs::Signaling for FileSignaling {
    fn push(&mut self, blob: transport_webrtc_rs::SignalBlob) -> transport_core::Result<()> {
        self.push_blob(blob.as_bytes())
            .map_err(|error| transport_core::Error::new(error.to_string()))
    }

    fn poll(&mut self) -> transport_core::Result<Vec<transport_webrtc_rs::SignalBlob>> {
        Ok(self
            .poll_blobs()
            .map_err(|error| transport_core::Error::new(error.to_string()))?
            .into_iter()
            .map(transport_webrtc_rs::SignalBlob)
            .collect())
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> std::result::Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err(format!("hex line has odd length {}", value.len()));
    }
    let nibble = |byte: u8| -> std::result::Result<u8, String> {
        match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            b'A'..=b'F' => Ok(byte - b'A' + 10),
            other => Err(format!("invalid hex byte 0x{other:02x}")),
        }
    };
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok((nibble(pair[0])? << 4) | nibble(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(dir: &std::path::Path, timeout_ms: u64) -> (FileSignaling, FileSignaling) {
        let a_to_b = dir.join("a-to-b.sig");
        let b_to_a = dir.join("b-to-a.sig");
        (
            FileSignaling::new(
                a_to_b.clone(),
                b_to_a.clone(),
                Duration::from_millis(timeout_ms),
            ),
            FileSignaling::new(b_to_a, a_to_b, Duration::from_millis(timeout_ms)),
        )
    }

    #[test]
    fn blobs_roundtrip_between_crossed_files() {
        let dir = tempfile::tempdir().unwrap();
        let (mut alice, mut bob) = pair(dir.path(), 100);

        // Multi-line SDP-shaped text and raw binary both survive the hex lines.
        let sdp = b"v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=-\r\n".to_vec();
        let binary = vec![0x00, 0xff, 0x0a, 0x0d];
        alice.push_blob(&sdp).unwrap();
        alice.push_blob(&binary).unwrap();

        assert_eq!(bob.poll_blobs().unwrap(), vec![sdp, binary]);
        // A second poll is empty: only NEW lines are returned.
        assert!(bob.poll_blobs().unwrap().is_empty());
    }

    #[test]
    fn poll_is_incremental_across_appends() {
        let dir = tempfile::tempdir().unwrap();
        let (mut alice, mut bob) = pair(dir.path(), 100);

        alice.push_blob(b"first").unwrap();
        assert_eq!(bob.poll_blobs().unwrap(), vec![b"first".to_vec()]);
        alice.push_blob(b"second").unwrap();
        assert_eq!(bob.poll_blobs().unwrap(), vec![b"second".to_vec()]);
    }

    #[test]
    fn missing_incoming_file_is_nothing_yet_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (_alice, mut bob) = pair(dir.path(), 100);
        assert!(bob.poll_blobs().unwrap().is_empty());
    }

    #[test]
    fn wait_next_blob_returns_first_and_retains_extras() {
        let dir = tempfile::tempdir().unwrap();
        let (mut alice, mut bob) = pair(dir.path(), 500);

        alice.push_blob(b"offer").unwrap();
        alice.push_blob(b"candidate").unwrap();
        assert_eq!(bob.wait_next_blob("SDP offer").unwrap(), b"offer".to_vec());
        assert_eq!(bob.poll_blobs().unwrap(), vec![b"candidate".to_vec()]);
    }

    #[test]
    fn wait_next_blob_times_out_with_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let (_alice, mut bob) = pair(dir.path(), 50);
        let error = bob.wait_next_blob("SDP answer").unwrap_err().to_string();
        assert!(error.contains("timed out"), "got: {error}");
        assert!(error.contains("SDP answer"), "got: {error}");
        assert!(error.contains("a-to-b.sig"), "got: {error}");
    }

    #[test]
    fn corrupt_hex_line_reports_file_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let (_alice, mut bob) = pair(dir.path(), 100);
        std::fs::write(dir.path().join("a-to-b.sig"), "zz-not-hex\n").unwrap();
        let error = bob.poll_blobs().unwrap_err().to_string();
        assert!(error.contains("line 1"), "got: {error}");
        assert!(error.contains("a-to-b.sig"), "got: {error}");
    }

    #[test]
    fn blank_lines_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let (_alice, mut bob) = pair(dir.path(), 100);
        std::fs::write(dir.path().join("a-to-b.sig"), "\n6869\n\n").unwrap();
        assert_eq!(bob.poll_blobs().unwrap(), vec![b"hi".to_vec()]);
    }
}
