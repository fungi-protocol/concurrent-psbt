//! The real webrtc-rs data-channel backend, compiled only under
//! `#[cfg(feature = "webrtc-rs")]`.
//!
//! This is ordinary messaging plumbing over the off-the-shelf `webrtc` crate
//! (webrtc-rs). It moves OPAQUE bytes over one reliable-ordered SCTP data channel;
//! there is no threat-model reasoning here — build a peer connection, run the
//! offer/answer + trickle-ICE handshake over the out-of-band signaling port, open
//! a data channel, send bytes, drain received bytes.
//!
//! # Pull-based seam over an async, callback-driven SDK
//!
//! webrtc-rs is full-async (tokio) and PUSH-based: you register
//! `data_channel.on_message(|msg| ...)` and it fires on the runtime's tasks. The
//! channel seam is PULL-based. We bridge exactly like transport-iroh and
//! transport-nym: one owned [`tokio::runtime::Runtime`] at the edge, with
//! `rt.block_on(...)` where the SDK must be driven to completion. The
//! `on_message` callback appends each received binary payload to a shared inbox
//! buffer behind an `Arc<Mutex<Vec<u8>>>`; `recv` locks that buffer and drains
//! every complete framed record from it ([`crate::drain_frames`]), turning the
//! push stream into a polling snapshot. The caller's loop already polls on its
//! own interval, so a non-blocking drain is sufficient. NOTE: the owned-runtime
//! `block_on` edge assumes the caller is NOT itself inside a tokio runtime
//! (tokio panics on nested block_on) — the same posture as the sibling deferred
//! transports; revisit when this backend is first exercised end to end.
//!
//! # API grounding (webrtc-rs public surface) — COMPILED, NOT NETWORK-TESTED
//!
//! Grounded against the pinned `webrtc` crate (see Cargo.toml): this file
//! type-checks under `--features webrtc-rs`. No live peer connection has been
//! exercised, so runtime behavior stays unverified until the e2e path covers
//! it. Wiring mirrors the webrtc-rs `data-channels` example:
//!   * `APIBuilder::new().build()` -> `api.new_peer_connection(config).await?`;
//!   * `RTCConfiguration { ice_servers: vec![RTCIceServer { urls, .. }], .. }`;
//!   * offerer: `pc.create_data_channel(label, Some(RTCDataChannelInit {
//!       ordered: Some(true), max_retransmits: None, max_packet_life_time: None,
//!       ..Default::default() }))`;
//!     answerer: `pc.on_data_channel(|dc| ...)` to receive the offerer's channel;
//!   * `dc.on_open(...)`, `dc.on_message(|msg: DataChannelMessage| ...)`,
//!     `dc.send(&Bytes) -> Result<usize>`;
//!   * offer/answer: `pc.create_offer(None).await?` / `pc.create_answer(None).await?`,
//!     `pc.set_local_description(sdp).await?`, `pc.set_remote_description(sdp).await?`;
//!   * trickle ICE: `pc.on_ice_candidate(|c| ...)` to emit local candidates,
//!     `pc.add_ice_candidate(RTCIceCandidateInit { candidate, .. }).await?` to add
//!     remote ones;
//!   * SDP (de)serialization: `RTCSessionDescription` is serde-serializable;
//!     we move it as JSON bytes inside a `SignalBlob`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use bytes::Bytes;

use transport_core::{Error, Result};

use crate::{drain_frames, wrap_outgoing, Role, SignalBlob, Signaling, WebrtcRsConfig};

/// How long to wait for the SCTP data channel to open after the handshake.
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a single signaling poll waits for a blob before looping.
const SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// The feature-on guts of [`crate::WebrtcRsTransport`].
///
/// One owned multi-thread runtime; every method bridges sync->async through
/// `rt.block_on(...)`. The data channel and inbox are held for the transport's
/// lifetime. `_pc` keeps the peer connection alive (dropping it would tear the
/// channel down).
pub(crate) struct Inner {
    /// Owned multi-thread runtime; held for the transport's lifetime.
    rt: Runtime,
    /// The live peer connection. Held to keep the DTLS/SCTP association alive;
    /// never used again after setup, hence the leading underscore.
    _pc: Arc<RTCPeerConnection>,
    /// The open, reliable-ordered SCTP data channel we send framed records over.
    dc: Arc<RTCDataChannel>,
    /// Receive buffer fed by the `on_message` callback (push) and drained by
    /// `recv` (pull). SCTP may fragment one record across several messages and
    /// coalesce several small records, so `recv` deframes whatever is buffered.
    inbox: Arc<Mutex<Vec<u8>>>,
}

impl Inner {
    /// Build a peer connection, run the offer/answer + trickle-ICE handshake over
    /// the out-of-band signaling port, and block until the data channel opens.
    pub(crate) fn connect<S: Signaling>(config: WebrtcRsConfig<S>) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::new(format!("transport-webrtc-rs: building tokio runtime: {e}")))?;

        let WebrtcRsConfig {
            role,
            ice_servers,
            signaling,
        } = config;

        let inbox: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

        // The handshake is async; drive the whole thing on the owned runtime and
        // hand back the connected peer connection + open data channel.
        let (pc, dc) = rt.block_on(async {
            connect_async(role, ice_servers, signaling, inbox.clone()).await
        })?;

        Ok(Inner {
            rt,
            _pc: pc,
            dc,
            inbox,
        })
    }
}

/// The full async handshake, factored out of [`Inner::connect`] so the sync
/// wrapper stays tidy. Returns the connected peer connection and the OPEN data
/// channel; installs the `on_message` -> inbox pump before returning.
async fn connect_async<S: Signaling>(
    role: Role,
    ice_servers: Vec<String>,
    mut signaling: S,
    inbox: Arc<Mutex<Vec<u8>>>,
) -> Result<(Arc<RTCPeerConnection>, Arc<RTCDataChannel>)> {
    // 1. Build the API + peer connection with the caller's ICE servers.
    let api = APIBuilder::new().build();
    let rtc_config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: ice_servers,
            ..Default::default()
        }],
        ..Default::default()
    };
    let pc = Arc::new(
        api.new_peer_connection(rtc_config)
            .await
            .map_err(|e| Error::new(format!("transport-webrtc-rs: new_peer_connection: {e}")))?,
    );

    // 2. Emit our local ICE candidates over the signaling port as they trickle
    // in. webrtc-rs hands each candidate to the callback; we serialize it to JSON
    // and push it as an opaque SignalBlob. (We move `signaling` into a channel the
    // callback can feed, because `on_ice_candidate` takes a 'static closure; a
    // real impl would wrap `signaling` in an `Arc<Mutex<..>>` or forward through
    // an mpsc — sketched here with an mpsc to keep ownership sane.)
    let (cand_tx, mut cand_rx) = mpsc::unbounded_channel::<SignalBlob>();
    pc.on_ice_candidate(Box::new(move |candidate| {
        let cand_tx = cand_tx.clone();
        Box::pin(async move {
            if let Some(c) = candidate {
                // `to_json()` yields an RTCIceCandidateInit; serialize its
                // `candidate` string form as the opaque blob.
                if let Ok(init) = c.to_json() {
                    if let Ok(bytes) = serde_json::to_vec(&init) {
                        let _ = cand_tx.send(SignalBlob(bytes));
                    }
                }
            }
        })
    }));

    // 3. Obtain the data channel:
    //    * Offerer creates it up front and drives create_offer.
    //    * Answerer waits for on_data_channel after applying the remote offer.
    let dc: Arc<RTCDataChannel> = match role {
        Role::Offerer => {
            let dc = pc
                .create_data_channel(
                    "ptj",
                    Some(RTCDataChannelInit {
                        ordered: Some(true),
                        max_retransmits: None,
                        max_packet_life_time: None,
                        ..Default::default()
                    }),
                )
                .await
                .map_err(|e| {
                    Error::new(format!("transport-webrtc-rs: create_data_channel: {e}"))
                })?;
            install_inbox_pump(&dc, inbox.clone());

            // create_offer -> set_local -> push offer -> await answer -> set_remote
            let offer = pc
                .create_offer(None)
                .await
                .map_err(|e| Error::new(format!("transport-webrtc-rs: create_offer: {e}")))?;
            pc.set_local_description(offer.clone())
                .await
                .map_err(|e| Error::new(format!("transport-webrtc-rs: set_local(offer): {e}")))?;
            signaling.push(SignalBlob(serialize_sdp(&offer)?))?;

            let answer = recv_sdp(&mut signaling).await?;
            pc.set_remote_description(answer)
                .await
                .map_err(|e| Error::new(format!("transport-webrtc-rs: set_remote(answer): {e}")))?;
            dc
        }
        Role::Answerer => {
            // Register on_data_channel BEFORE applying the offer so we don't miss
            // the offerer's channel. Bridge it out via a oneshot-style mpsc.
            let (dc_tx, mut dc_rx) = mpsc::unbounded_channel::<Arc<RTCDataChannel>>();
            let inbox_for_cb = inbox.clone();
            pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
                install_inbox_pump(&dc, inbox_for_cb.clone());
                let _ = dc_tx.send(dc);
                Box::pin(async {})
            }));

            // await offer -> set_remote -> create_answer -> set_local -> push answer
            let offer = recv_sdp(&mut signaling).await?;
            pc.set_remote_description(offer)
                .await
                .map_err(|e| Error::new(format!("transport-webrtc-rs: set_remote(offer): {e}")))?;
            let answer = pc
                .create_answer(None)
                .await
                .map_err(|e| Error::new(format!("transport-webrtc-rs: create_answer: {e}")))?;
            pc.set_local_description(answer.clone())
                .await
                .map_err(|e| Error::new(format!("transport-webrtc-rs: set_local(answer): {e}")))?;
            signaling.push(SignalBlob(serialize_sdp(&answer)?))?;

            // Wait for the offerer's data channel to arrive via on_data_channel.
            tokio::time::timeout(OPEN_TIMEOUT, dc_rx.recv())
                .await
                .map_err(|_| Error::new("transport-webrtc-rs: timed out waiting for data channel"))?
                .ok_or_else(|| Error::new("transport-webrtc-rs: on_data_channel closed"))?
        }
    };

    // 4. Pump any trickled local candidates + drain remote candidates from the
    // signaling port until the channel opens. (Sketch: a real impl runs this as a
    // background task for the connection's lifetime; here we bound it by
    // OPEN_TIMEOUT and stop once open.) Remote candidates are added as they arrive.
    let open = wait_open(dc.clone());
    tokio::pin!(open);
    let deadline = tokio::time::sleep(OPEN_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            // Channel opened: done.
            _ = &mut open => break,
            // Deadline: give up.
            _ = &mut deadline => {
                return Err(Error::new("transport-webrtc-rs: data channel did not open in time"));
            }
            // Forward a locally-trickled candidate to the peer.
            Some(local_cand) = cand_rx.recv() => {
                signaling.push(local_cand)?;
            }
            // Poll the signaling port for remote candidates on an interval.
            _ = tokio::time::sleep(SIGNAL_POLL_INTERVAL) => {
                for blob in signaling.poll()? {
                    // A remote candidate blob; ignore anything that isn't one
                    // (the SDP answer/offer was already consumed above).
                    if let Ok(init) = serde_json::from_slice::<RTCIceCandidateInit>(blob.as_bytes()) {
                        let _ = pc.add_ice_candidate(init).await;
                    }
                }
            }
        }
    }

    Ok((pc, dc))
}

/// Install the `on_message` -> inbox pump on a data channel: every received
/// binary payload is appended to the shared inbox buffer, which `recv` drains.
fn install_inbox_pump(dc: &Arc<RTCDataChannel>, inbox: Arc<Mutex<Vec<u8>>>) {
    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let inbox = inbox.clone();
        Box::pin(async move {
            // Byte-transparent: append the raw payload; recv() deframes it. A
            // poisoned lock would only happen if a prior holder panicked; treat
            // it as best-effort (a transport moves bytes, never crashes on lock
            // contention) by recovering the guard.
            let mut guard = match inbox.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.extend_from_slice(&msg.data);
        })
    }));
}

/// Resolve once the data channel reports open. webrtc-rs fires `on_open`; we
/// bridge that single callback to an awaitable via a oneshot. Takes its own
/// `Arc` handle so the caller keeps free ownership of the channel.
async fn wait_open(dc: Arc<RTCDataChannel>) {
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = std::sync::Mutex::new(Some(tx));
    dc.on_open(Box::new(move || {
        if let Ok(mut guard) = tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
            }
        }
        Box::pin(async {})
    }));
    let _ = rx.await;
}

/// Serialize an SDP session description to opaque JSON bytes for a `SignalBlob`.
fn serialize_sdp(sdp: &RTCSessionDescription) -> Result<Vec<u8>> {
    serde_json::to_vec(sdp)
        .map_err(|e| Error::new(format!("transport-webrtc-rs: serializing SDP: {e}")))
}

/// Poll the signaling port until an SDP blob (offer or answer) arrives, then
/// deserialize it. Bounded by [`OPEN_TIMEOUT`]. Non-SDP blobs (early trickle
/// candidates) are ignored here; the candidate loop consumes those.
async fn recv_sdp<S: Signaling>(signaling: &mut S) -> Result<RTCSessionDescription> {
    let deadline = tokio::time::Instant::now() + OPEN_TIMEOUT;
    loop {
        for blob in signaling.poll()? {
            if let Ok(sdp) = serde_json::from_slice::<RTCSessionDescription>(blob.as_bytes()) {
                return Ok(sdp);
            }
            // else: not an SDP blob (a candidate); skip it here.
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::new(
                "transport-webrtc-rs: timed out waiting for SDP over signaling",
            ));
        }
        tokio::time::sleep(SIGNAL_POLL_INTERVAL).await;
    }
}

// Inherent (not trait) methods: the crate root's `WebrtcRsTransport` owns the
// `AnonymousChannel` impl (async seam) and forwards to these synchronous
// bodies, which drive the SDK on the owned runtime.
impl Inner {
    /// Broadcast one opaque message over the data channel as a single binary
    /// message: one framed [`transport_core::frame`] record wrapping the caller's
    /// opaque bytes. Anonymity is a property of the channel, not of this crate.
    pub(crate) fn send(&mut self, message: Vec<u8>) -> Result<()> {
        let payload = wrap_outgoing(&message);
        let dc = self.dc.clone();
        self.rt.block_on(async move {
            dc.send(&Bytes::from(payload))
                .await
                .map_err(|e| Error::new(format!("transport-webrtc-rs: data channel send: {e}")))?;
            Ok::<(), Error>(())
        })
    }

    /// Drain every complete framed record the data channel has delivered since
    /// the last poll, as BARE bytes (no sender identity — the anonymous
    /// contract). Non-blocking: returns whatever whole records are buffered now,
    /// retaining any trailing partial for the next poll.
    pub(crate) fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut guard = match self.inbox.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Deframe in place: whole records come out; a trailing partial stays in
        // the buffer for the next poll.
        drain_frames(&mut guard)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Best-effort clean close of the data channel + peer connection. Ignore
        // errors: we are tearing down.
        let dc = self.dc.clone();
        let pc = self._pc.clone();
        let _ = self.rt.block_on(async move {
            let _ = dc.close().await;
            let _ = pc.close().await;
        });
    }
}
