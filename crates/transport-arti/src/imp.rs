//! The real arti backend — compiled only with the `arti` feature.
//!
//! This module owns the async runtime and the arti SDK. It bootstraps an
//! in-process Tor client (no separate `tor` daemon binary), launches an onion
//! service to accept inbound peer streams, opens outbound Tor streams to each
//! configured peer `.onion`, and moves opaque bytes framed with transport-core's
//! length-prefixed framing. It contains zero privacy/threat-model reasoning:
//! anonymity is arti's property, we just move bytes over the stream it gives us.
//!
//! ## Async → sync bridge
//!
//! `AnonymousChannel` is synchronous; arti is async (tokio). We own one
//! `tokio::runtime::Runtime` and `block_on` per `send`/`recv`, exactly the
//! pattern the iroh sibling transport uses. A background task drains our onion
//! service's inbound streams into a shared buffer so `recv` is a cheap snapshot.

use std::sync::{Arc, Mutex};

use arti_client::{TorClient, TorClientConfig};
use arti_client::config::onion_service::OnionServiceConfigBuilder;
use futures::AsyncReadExt as _;
use futures::AsyncWriteExt as _;
use futures::Stream;
use futures::StreamExt as _;
use tokio::runtime::Runtime;
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{RendRequest, RunningOnionService, StreamRequest};
use tor_proto::stream::IncomingStreamRequest;
use tor_rtcompat::PreferredRuntime;

use transport_core::{Error, Result, MAX_FRAME_LEN};

use super::ArtiConfig;

/// The maximum number of buffered inbound records we retain between polls.
/// A poll-based caller drains these on each `recv`; the cap simply bounds memory
/// if a caller stops polling. (No dedup/ordering — the lattice join owns that.)
const INBOUND_BUFFER_CAP: usize = 4096;

/// The live arti backend.
pub struct Inner {
    config: ArtiConfig,
    rt: Runtime,
    client: TorClient<PreferredRuntime>,
    /// The running onion service handle. HELD ONLY to keep the service alive:
    /// dropping it tears the service down, so it must outlive the transport.
    _service: Arc<RunningOnionService>,
    /// Our onion service's published `.onion` address, once known.
    onion: Arc<Mutex<Option<String>>>,
    /// Framed records peers have written to our onion service since last poll.
    /// Drained (taken) by `recv`. Bare bytes only — no sender identity, because
    /// a Tor stream carries no peer identity (this is why arti is anonymous).
    inbound: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Inner {
    /// Bootstrap the in-process Tor client and launch our onion service.
    ///
    /// This is the slow step (Tor bootstrap + descriptor publication can take
    /// tens of seconds — see the crate `uxNotes`). We block on it here so that a
    /// returned `Inner` is ready to `send`/`recv`.
    pub fn new(config: ArtiConfig) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::new(format!("arti: building tokio runtime: {e}")))?;

        let onion: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let inbound: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));

        let (client, service) = {
            let onion = Arc::clone(&onion);
            let inbound = Arc::clone(&inbound);
            let nickname = config.service_nickname.clone();
            let listen_port = config.listen_port;

            rt.block_on(async move {
                // Bootstrap an in-process Tor client. `create_bootstrapped`
                // connects to the Tor network before returning.
                let tor_config = TorClientConfig::default();
                let client = TorClient::create_bootstrapped(tor_config)
                    .await
                    .map_err(|e| format!("bootstrapping Tor client: {e}"))?;

                // Launch our onion service so peers can dial us. Its inbound
                // streams are drained by a background task into `inbound`.
                let svc_config = OnionServiceConfigBuilder::default()
                    .nickname(
                        nickname
                            .parse()
                            .map_err(|e| format!("invalid onion-service nickname: {e}"))?,
                    )
                    .build()
                    .map_err(|e| format!("building onion-service config: {e}"))?;

                let (service, request_stream) = client
                    .launch_onion_service(svc_config)
                    .map_err(|e| format!("launching onion service: {e}"))?;

                if let Some(addr) = service.onion_address() {
                    *onion.lock().expect("onion mutex not poisoned") = Some(addr.to_string());
                }

                // Background task: accept inbound rendezvous requests, accept the
                // stream on our listen port, read one framed record off it, and
                // stash the bare bytes. No sender identity is recorded.
                let inbound_bg = Arc::clone(&inbound);
                tokio::spawn(accept_loop(request_stream, listen_port, inbound_bg));

                Ok::<_, String>((client, service))
            })
            .map_err(|message| Error::new(format!("arti: {message}")))?
        };

        Ok(Self {
            config,
            _service: service,
            rt,
            client,
            onion,
            inbound,
        })
    }

    /// Our published `.onion` address, if the descriptor is up yet.
    pub fn onion_address(&self) -> Result<String> {
        self.onion
            .lock()
            .expect("onion mutex not poisoned")
            .clone()
            .ok_or_else(|| Error::new("arti: onion address not yet published"))
    }

    /// Write one framed opaque record to every configured peer over Tor.
    pub fn send(&mut self, message: Vec<u8>) -> Result<()> {
        if message.len() > MAX_FRAME_LEN {
            return Err(Error::new(format!(
                "arti send: message length {} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}",
                message.len()
            )));
        }

        let client = self.client.clone();
        let peers = self.config.peers.clone();

        self.rt.block_on(async move {
            for peer in &peers {
                // Open a Tor stream to the peer's onion endpoint and write one
                // length-prefixed frame (u32 BE len + value). arti parses the
                // "<base32>.onion:<port>" target string itself.
                let mut stream = client
                    .connect(peer.as_str())
                    .await
                    .map_err(|e| Error::new(format!("arti send: connecting to {peer}: {e}")))?;

                write_frame_async(&mut stream, &message)
                    .await
                    .map_err(|e| Error::new(format!("arti send: framing to {peer}: {e}")))?;

                stream
                    .flush()
                    .await
                    .map_err(|e| Error::new(format!("arti send: flushing to {peer}: {e}")))?;
                stream
                    .close()
                    .await
                    .map_err(|e| Error::new(format!("arti send: closing to {peer}: {e}")))?;
            }
            Ok(())
        })
    }

    /// Drain and return the framed records peers have written to us since the
    /// last poll, as bare opaque bytes (no sender identity).
    pub fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut guard = self.inbound.lock().expect("inbound mutex not poisoned");
        Ok(std::mem::take(&mut *guard))
    }
}

/// Accept inbound onion-service streams forever, reading one framed record from
/// each and stashing the bare bytes. Errors on a single request/stream are
/// dropped (a misbehaving peer must not kill the accept loop); the transport
/// moves bytes only and never reasons about who the peer is.
///
/// arti's onion-service inbound surface is two nested streams: the outer yields
/// one [`RendRequest`] per client rendezvous; accepting it yields an inner
/// stream of [`StreamRequest`]s (the client's `BEGIN` cells). We accept only
/// those targeting our advertised `listen_port` and read one framed record off
/// each resulting data stream.
async fn accept_loop<S>(rend_requests: S, listen_port: u16, inbound: Arc<Mutex<Vec<Vec<u8>>>>)
where
    S: Stream<Item = RendRequest> + Send + 'static,
{
    // Pin the (possibly !Unpin) request stream on the stack so we can poll it.
    futures::pin_mut!(rend_requests);
    while let Some(rend) = rend_requests.next().await {
        // Accept the rendezvous; this yields the per-circuit stream requests.
        let stream_requests = match rend.accept().await {
            Ok(s) => s,
            Err(_) => continue, // drop a bad rendezvous, keep serving others
        };
        let inbound = Arc::clone(&inbound);
        // Box the inner stream so the spawned future has a concrete Unpin type.
        tokio::spawn(serve_circuit(Box::pin(stream_requests), listen_port, inbound));
    }
}

/// Serve one client circuit: accept each `BEGIN` targeting our `listen_port`,
/// read one framed opaque record off the resulting data stream, and stash the
/// bare bytes (no sender identity — a Tor stream carries no peer identity).
async fn serve_circuit<S>(mut stream_requests: S, listen_port: u16, inbound: Arc<Mutex<Vec<Vec<u8>>>>)
where
    S: Stream<Item = StreamRequest> + Unpin + Send + 'static,
{
    while let Some(stream_request) = stream_requests.next().await {
        // Only accept data streams targeting our advertised virtual port.
        let wants_our_port = match stream_request.request() {
            IncomingStreamRequest::Begin(begin) => begin.port() == listen_port,
            // Resolve/other request kinds are not our data path; reject them.
            _ => false,
        };
        if !wants_our_port {
            // Reject anything not aimed at our listen port; keep serving.
            let _ = stream_request.reject().await;
            continue;
        }

        let mut data_stream = match stream_request.accept(Connected::new_empty()).await {
            Ok(s) => s,
            Err(_) => continue, // drop a bad stream, keep serving the circuit
        };

        let inbound = Arc::clone(&inbound);
        tokio::spawn(async move {
            if let Ok(Some(record)) = read_frame_async(&mut data_stream).await {
                let mut guard = inbound.lock().expect("inbound mutex not poisoned");
                if guard.len() < INBOUND_BUFFER_CAP {
                    guard.push(record);
                }
            }
        });
    }
}

/// Write one transport-core frame (u32 BE length prefix + value) to an async
/// writer. Mirrors [`transport_core::write_frame`] over `futures::AsyncWrite`,
/// enforcing the same [`MAX_FRAME_LEN`] cap so a peer cannot make us reserve an
/// unbounded buffer on the read side.
async fn write_frame_async<W>(writer: &mut W, value: &[u8]) -> Result<()>
where
    W: futures::AsyncWrite + Unpin,
{
    if value.len() > MAX_FRAME_LEN {
        return Err(Error::new(format!(
            "write_frame_async: value length {} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}",
            value.len()
        )));
    }
    let len = value.len() as u32;
    writer
        .write_all(&len.to_be_bytes())
        .await
        .map_err(Error::from)?;
    writer.write_all(value).await.map_err(Error::from)?;
    Ok(())
}

/// Read exactly one transport-core frame from an async reader.
///
/// Mirrors [`transport_core::read_frame`] over `futures::AsyncRead`: reads the
/// 4-byte length prefix, rejects a declared length above [`MAX_FRAME_LEN`], then
/// reads exactly that many value bytes. `Ok(None)` = clean EOF on a record
/// boundary; `Err` on truncation or an oversize declared length.
async fn read_frame_async<R>(reader: &mut R) -> Result<Option<Vec<u8>>>
where
    R: futures::AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    match read_exact_or_eof(reader, &mut len_buf).await? {
        ReadOutcome::Eof => return Ok(None),
        ReadOutcome::PartialEof(n) => {
            return Err(Error::new(format!(
                "read_frame_async: stream ended mid length-prefix after {n} of 4 bytes"
            )));
        }
        ReadOutcome::Full => {}
    }

    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(Error::new(format!(
            "read_frame_async: declared length {len} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}"
        )));
    }

    let mut value = vec![0u8; len];
    match read_exact_or_eof(reader, &mut value).await? {
        ReadOutcome::Full => Ok(Some(value)),
        ReadOutcome::Eof if len == 0 => Ok(Some(value)),
        ReadOutcome::Eof | ReadOutcome::PartialEof(_) => Err(Error::new(format!(
            "read_frame_async: stream ended mid record (expected {len} value bytes)"
        ))),
    }
}

enum ReadOutcome {
    Full,
    Eof,
    PartialEof(usize),
}

/// Async analogue of transport-core's `read_exact_or_eof`: fill `buf`,
/// distinguishing "clean EOF at start" from "EOF partway through".
async fn read_exact_or_eof<R>(reader: &mut R, buf: &mut [u8]) -> Result<ReadOutcome>
where
    R: futures::AsyncRead + Unpin,
{
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]).await {
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
    use futures::io::Cursor;

    // A network-free async framing roundtrip over an in-memory cursor, proving
    // the async framing helpers this backend uses on a Tor stream agree with the
    // transport-core wire format (u32 BE len + value, 16 MiB cap).
    #[tokio::test]
    async fn async_framing_roundtrips_without_network() {
        let a = b"opaque-a".to_vec();
        let b = vec![0x5Au8; 1024];

        let mut sink: Vec<u8> = Vec::new();
        {
            let mut w = Cursor::new(&mut sink);
            write_frame_async(&mut w, &a).await.unwrap();
            write_frame_async(&mut w, &b).await.unwrap();
        }

        // The async writer emits the exact transport-core wire format, so the
        // synchronous transport_core::read_frame reads it back.
        let mut sync_cursor = std::io::Cursor::new(sink.clone());
        assert_eq!(
            transport_core::read_frame(&mut sync_cursor).unwrap(),
            Some(a.clone())
        );
        assert_eq!(
            transport_core::read_frame(&mut sync_cursor).unwrap(),
            Some(b.clone())
        );

        // And read_frame_async reads back what transport_core::write_frame wrote.
        let mut sync_out: Vec<u8> = Vec::new();
        transport_core::write_frame(&mut sync_out, &a).unwrap();
        let mut r = Cursor::new(sync_out);
        assert_eq!(read_frame_async(&mut r).await.unwrap(), Some(a));
    }

    #[tokio::test]
    async fn read_frame_async_rejects_oversize_length() {
        let big = (MAX_FRAME_LEN as u32 + 1).to_be_bytes();
        let mut r = Cursor::new(big.to_vec());
        assert!(read_frame_async(&mut r).await.is_err());
    }

    #[tokio::test]
    async fn read_frame_async_clean_eof_is_none() {
        let mut r = Cursor::new(Vec::<u8>::new());
        assert_eq!(read_frame_async(&mut r).await.unwrap(), None);
    }
}
