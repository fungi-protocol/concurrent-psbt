//! The real I2P streaming path (compiled only with the `emissary` feature).
//!
//! emissary-core is an EMBEDDED I2P router: it runs inside our process, exactly
//! like transport-arti's in-process Tor client. There is no external i2pd/Java
//! router and no SAMv3 bridge — we build the router here, let it warm up its
//! tunnels, and open a stream to the peer through its own API.
//!
//! The router is async; to keep this crate's synchronous `AnonymousChannel`
//! contract we own a small tokio runtime and `block_on` it (the same shape
//! transport-arti uses for arti-client). Once a stream is open it is just
//! bytes: we put transport-core length-prefixed frames on it, one framed
//! [`Message`] envelope per record.
//!
//! [`Message`]: transport_core::Message
//!
//! Flow (embedded router, in-process):
//!   1. Build the router from `state_dir` (persisted destination keys + netdb)
//!      and start it on our runtime; wait for tunnels to build.
//!   2. Take a streaming session from the router — this yields our own local
//!      destination and lets us dial peers.
//!   3. Connect a stream to the peer destination; from there it is a raw
//!      bidirectional byte pipe we frame over.
//!
//! GROUNDING CAVEAT: the exact emissary-core embedded API (router builder,
//! streaming-session, connect) is coded against the documented surface and is
//! NOT yet compiled here — like transport-arti's inbound path, the concrete
//! type/method names may shift across emissary-core releases and will need a
//! version bump/rename when the main loop compiles the `emissary` feature. The
//! framing (transport-core `frame`/`deframe`) and the pull/drain shape are
//! stable and SDK-independent. This module keeps zero privacy/threat-model
//! reasoning: anonymity is a property of the embedded I2P router, not of this
//! code. We only move opaque bytes.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;

use transport_core::{deframe, frame, Error, Result};

/// A connected I2P stream backed by an embedded emissary-core router.
///
/// Holds the owned runtime and the router handle alive for the lifetime of the
/// stream (dropping the router tears down its tunnels), plus the async stream
/// and a receive buffer for reassembling framed records that arrive in pieces.
pub struct I2pStream {
    /// Owned runtime we `block_on`; keeps the embedded router driven.
    rt: Runtime,
    /// The embedded router handle. Kept alive so its tunnels stay up; the async
    /// stream below borrows the network it provides.
    _router: Arc<emissary_core::Router>,
    /// The connected I2P stream to the peer (an async duplex byte pipe).
    stream: Arc<Mutex<emissary_core::streaming::Stream>>,
    /// Bytes read off the stream but not yet formed into a complete frame.
    /// `drain_framed` pulls whole records out of here; partial tails remain.
    rx: Vec<u8>,
}

impl I2pStream {
    /// Start the embedded router and connect a stream to the peer named in
    /// `config`. Returns a ready-to-frame byte stream.
    pub fn connect(config: &crate::EmissaryConfig) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(Error::from)?;

        let (router, stream) = rt.block_on(async {
            // 1. Build + start the embedded router (persisted state in state_dir).
            let router = emissary_core::Router::builder()
                .with_state_dir(&config.state_dir)
                .build()
                .await
                .map_err(|e| Error::new(format!("transport-emissary: router build failed: {e}")))?;
            let router = Arc::new(router);

            // Wait for tunnels to build before we can stream (cold start is slow;
            // this is the I2P analogue of arti's descriptor-publish wait).
            router
                .wait_for_tunnels(Duration::from_secs(60))
                .await
                .map_err(|e| {
                    Error::new(format!("transport-emissary: tunnels not ready: {e}"))
                })?;

            // 2 & 3. Take a streaming session and dial the peer destination.
            let session = router.streaming_session(&config.session_label).await.map_err(|e| {
                Error::new(format!("transport-emissary: streaming session failed: {e}"))
            })?;
            let stream = session
                .connect(&config.peer_destination)
                .await
                .map_err(|e| {
                    Error::new(format!(
                        "transport-emissary: cannot reach peer {}: {e}",
                        config.peer_destination
                    ))
                })?;
            Ok::<_, Error>((router, Arc::new(Mutex::new(stream))))
        })?;

        Ok(I2pStream {
            rt,
            _router: router,
            stream,
            rx: Vec::new(),
        })
    }

    /// Write one message as a single length-prefixed frame onto the stream.
    pub fn send_framed(&mut self, message: &[u8]) -> Result<()> {
        // transport-core buffer framing: u32 BE length prefix + value (enforces
        // the 16 MiB MAX_FRAME_LEN cap). We write the framed bytes to the async
        // stream under our owned runtime.
        let framed = frame(message);
        let stream = Arc::clone(&self.stream);
        self.rt.block_on(async move {
            let mut s = stream.lock().await;
            s.write_all(&framed).await.map_err(Error::from)?;
            s.flush().await.map_err(Error::from)
        })
    }

    /// Read whatever bytes are currently available and return every complete
    /// framed record among them (bare bytes — no sender identity). Partial
    /// tails are retained in `rx` for the next call. Returns an empty vec when
    /// nothing new has arrived (a timed-out read is not an error).
    pub fn drain_framed(&mut self) -> Result<Vec<Vec<u8>>> {
        self.fill_from_stream()?;

        let mut out = Vec::new();
        while let Some(record) = deframe(&mut self.rx)? {
            out.push(record);
        }
        Ok(out)
    }

    /// Pull any currently-available bytes off the stream into `rx`. A short read
    /// timeout means "nothing new right now" and is not an error — this is
    /// polling, matching the pull cadence transport-core's channels expect.
    fn fill_from_stream(&mut self) -> Result<()> {
        let stream = Arc::clone(&self.stream);
        let chunk = self.rt.block_on(async move {
            let mut s = stream.lock().await;
            let mut buf = [0u8; 8192];
            // Bounded wait: a read that does not complete within the poll window
            // yields nothing, rather than blocking the drain indefinitely.
            match tokio::time::timeout(Duration::from_millis(50), s.read(&mut buf)).await {
                Ok(Ok(0)) => Ok::<_, Error>(Vec::new()), // peer closed
                Ok(Ok(n)) => Ok(buf[..n].to_vec()),
                Ok(Err(e)) => Err(Error::from(e)),
                Err(_elapsed) => Ok(Vec::new()), // nothing available in the window
            }
        })?;
        self.rx.extend_from_slice(&chunk);
        Ok(())
    }
}
