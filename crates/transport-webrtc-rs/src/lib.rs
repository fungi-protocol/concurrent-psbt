//! `transport-webrtc-rs` — a WebRTC data-channel backed [`AnonymousChannel`],
//! using the full-async [`webrtc`](https://github.com/webrtc-rs/webrtc)
//! (webrtc-rs) crate.
//!
//! This is ordinary messaging plumbing: it moves OPAQUE bytes between two peers
//! over a WebRTC SCTP data channel. It is NOT security code. A DTLS handshake
//! authenticates the channel and yields a peer-certificate fingerprint, but this
//! crate does no threat-model reasoning and surfaces no identity — it just *uses*
//! webrtc-rs to move bytes.
//!
//! # The alternate backend (webrtc-rs), not the primary (str0m)
//!
//! Two WebRTC backends are contemplated for this project. The PREFERRED one is
//! sans-IO `str0m` (in the sibling crate `transport-str0m`): str0m *is* a poll
//! loop, so it needs no async runtime and drops cleanly into the synchronous,
//! pull-based channel contract. THIS crate is the heavier ALTERNATE: webrtc-rs is
//! a full-async, callback-driven stack that owns a tokio runtime. We bridge the
//! pull-based [`AnonymousChannel`] onto it EXACTLY as transport-iroh and
//! transport-nym bridge their async SDKs — an owned `tokio` runtime at the
//! edge and an internal inbox that turns webrtc-rs's `on_message` push
//! callbacks into a polling `recv` snapshot. The human's steer:
//! ship ONE of the two real; str0m is the one to prove, so this webrtc-rs crate
//! is authored in full with grounded deps (it type-checks under
//! `--features webrtc-rs`) but no live peer connection has been exercised.
//!
//! # Which channel kind, and why
//!
//! [`AnonymousChannel`] only. A data channel is a bare byte pipe: webrtc-rs hands
//! us `on_message(|msg: DataChannelMessage| ...)` with a payload and no verifiable
//! sender identity, so [`AnonymousChannel::recv`] yields bare `Vec<Vec<u8>>` with
//! no [`transport_core::SenderId`]. That is the entire reason this is anonymous
//! rather than attributable — a messaging distinction about what a received
//! message carries about who sent it. This mirrors transport-arti / transport-nym
//! / transport-emissary; it is NOT the iroh/mdk (attributable) shape. Do NOT wrap
//! it in [`Attributed`](transport_core::Attributed) — the blanket
//! `impl<C: AnonymousChannel> Transport for C` already makes it a driver-facing
//! [`Transport`](transport_core::Transport) for free.
//!
//! # Wire shape (framing)
//!
//! Although SCTP is message-oriented, a large PSBT can exceed the ~16 KiB safe
//! single-message size and get fragmented by the SCTP layer, and more than one
//! [`transport_core::Message`] envelope may ride the channel, so we delimit
//! records with transport-core's length-prefixed [`transport_core::frame`] /
//! [`transport_core::deframe`] (a `u32` big-endian length prefix + value, shared
//! 16 MiB [`transport_core::MAX_FRAME_LEN`] cap). Each outbound `send` writes one
//! `frame(bytes)` binary message; the inbound side APPENDS every received binary
//! payload to a per-channel receive buffer and loops `deframe` to pull out every
//! complete record, retaining trailing partials for the next poll — the same
//! push->pull-behind-a-buffer trick transport-arti and transport-nym use. Payload
//! is byte-transparent: on the ptj path it is a [`transport_core::Message`] TLV,
//! but the transport never parses it (framing delimits; `Message` tags — the two
//! are orthogonal per `framing.rs`).
//!
//! # Signaling is out of scope
//!
//! WebRTC needs an out-of-band exchange of the SDP offer/answer and trickle ICE
//! candidates BEFORE the data channel exists. That exchange MUST NOT go over a
//! direct/localhost signaling server or the PWA origin (either learns the client
//! IP); it rides a BIP-77 payjoin-directory (store-and-forward mailbox) accessed
//! through an OHTTP relay, which is its OWN transport crate (an oblivious mailbox
//! that is itself an [`AnonymousChannel`]). This crate treats the offer/answer/
//! candidate blobs purely as opaque INPUTS and OUTPUTS on its [`Signaling`] port
//! (see [`WebrtcRsConfig`]); it never dials a signaling server itself. Pairing /
//! introduction is decoupled and out of scope, exactly as a `DocTicket` is an
//! input to transport-iroh.
//!
//! # Feature gating (mirrors how `ptj` gates `iroh-sync`)
//!
//! ALL webrtc-rs / network usage sits behind the `webrtc-rs` cargo feature. With
//! the feature OFF (the default) this crate compiles as a **skeleton**: the public
//! type and its methods exist and satisfy [`AnonymousChannel`], but every
//! constructor and I/O call returns a clear
//! [`Error`]`("transport-webrtc-rs built without the 'webrtc-rs' feature; ...")`.
//! With the feature ON, the same surface performs real DTLS/SCTP data-channel I/O
//! via `webrtc` behind an owned tokio runtime at the edge (the channel seam is
//! async via `async_trait`; the authored backend drives webrtc-rs on its own
//! runtime and hands results back through the inbox).
//!
//! [str0m]: https://github.com/algesten/str0m

// A transport moves bytes; missing docs on public items are worth catching in a
// small crate, matching transport-core's own lint posture.
#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AnonymousChannel, Result};
// Only the feature-off skeleton constructs an `Error` at this level; the real
// backend builds its errors inside `imp`.
#[cfg(not(feature = "webrtc-rs"))]
use transport_core::Error;

// The real backend is compiled in only with the `webrtc-rs` feature. It owns the
// tokio runtime, the RTCPeerConnection, the data channel, and the inbox.
#[cfg(feature = "webrtc-rs")]
mod imp;

/// The role a peer plays in the WebRTC offer/answer handshake.
///
/// WebRTC is asymmetric at setup time: exactly one peer is the *offerer* (creates
/// the data channel and the SDP offer) and the other is the *answerer* (receives
/// the offer and produces the answer). Once the channel is open the two sides are
/// symmetric byte pipes. This is plain setup data — the transport never decides a
/// role on its own; it is handed one, exactly as a peer address is an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// This peer creates the data channel and emits the SDP offer first.
    Offerer,
    /// This peer waits for an SDP offer and emits the SDP answer.
    Answerer,
}

/// A single opaque signaling blob (an SDP offer/answer or a trickle-ICE
/// candidate), moved to/from the peer over the out-of-band oblivious mailbox.
///
/// It is transport-transparent bytes: `transport-webrtc-rs` neither mints these
/// nor parses them beyond handing them to webrtc-rs's own SDP/candidate parsers.
/// The mailbox that actually carries them (BIP-77 payjoin-directory over OHTTP) is
/// a SEPARATE transport crate; here they are just inputs and outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalBlob(pub Vec<u8>);

impl SignalBlob {
    /// Borrow the opaque signaling bytes. This crate never interprets them beyond
    /// passing them to webrtc-rs.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The out-of-band signaling port: how the SDP offer/answer + ICE candidate
/// blobs reach the peer before the data channel is up.
///
/// This is DELIBERATELY a trait, not a concrete client. Signaling MUST ride a
/// BIP-77 payjoin-directory over an OHTTP relay (its own transport crate), never
/// a direct signaling server — but that mechanism is out of scope for this crate,
/// so we depend only on this narrow port. `push` writes one opaque blob to the
/// peer's mailbox; `poll` returns whatever blobs have arrived since the last call
/// (a fresh snapshot, like `recv`). A real deployment supplies an impl backed by
/// the payjoin-directory-over-OHTTP oblivious mailbox; tests supply an in-memory
/// pair.
pub trait Signaling {
    /// Write one opaque signaling blob to the peer's oblivious mailbox.
    fn push(&mut self, blob: SignalBlob) -> Result<()>;

    /// Return every signaling blob that has arrived for us since the last poll.
    fn poll(&mut self) -> Result<Vec<SignalBlob>>;
}

/// Configuration for a [`WebrtcRsTransport`].
///
/// Everything here is plain data delivered out of band — the transport never
/// discovers or pairs peers itself (introduction is decoupled and out of scope).
pub struct WebrtcRsConfig<S: Signaling> {
    /// Which side of the offer/answer handshake this peer plays.
    pub role: Role,
    /// ICE server URLs (STUN/TURN) for NAT traversal, e.g.
    /// `"stun:stun.l.google.com:19302"`. Opaque strings handed to webrtc-rs; this
    /// crate never parses them. May be empty for host-only / same-LAN peers.
    pub ice_servers: Vec<String>,
    /// The out-of-band signaling port carrying SDP/ICE blobs to and from the
    /// peer (a payjoin-directory-over-OHTTP oblivious mailbox in production).
    pub signaling: S,
}

impl<S: Signaling> WebrtcRsConfig<S> {
    /// A config with the given role, ICE servers, and signaling port.
    pub fn new(role: Role, ice_servers: Vec<String>, signaling: S) -> Self {
        Self {
            role,
            ice_servers,
            signaling,
        }
    }
}

/// A WebRTC data-channel backed collaborative transport (webrtc-rs backend).
///
/// Implements [`AnonymousChannel`]: `send` writes one framed opaque record as a
/// binary data-channel message; `recv` returns a fresh snapshot of the framed
/// records the peer has delivered since the last poll, as bare bytes (no sender
/// identity). Bridges to the driver-facing [`Transport`](transport_core::Transport)
/// seam for free via transport-core's blanket impl for anonymous channels.
///
/// Construct it with [`WebrtcRsTransport::connect`], which brings up the
/// RTCPeerConnection, runs the offer/answer + trickle-ICE handshake over the
/// configured [`Signaling`] port, and waits for the SCTP data channel to open.
/// Requires the `webrtc-rs` feature; built without it, [`connect`](WebrtcRsTransport::connect)
/// and the channel methods return the [`BUILT_WITHOUT_WEBRTC_RS`] error and the
/// type is an empty skeleton that still satisfies the trait bound.
pub struct WebrtcRsTransport {
    // With the feature on, the real peer connection + runtime + inbox live here.
    // With it off, the struct is a zero-field skeleton so the type — and its
    // trait impls — still exist.
    #[cfg(feature = "webrtc-rs")]
    inner: imp::Inner,
    #[cfg(not(feature = "webrtc-rs"))]
    _skeleton: (),
}

/// Error text returned by every constructor / method when the crate was built
/// WITHOUT the `webrtc-rs` feature. Mirrors the ptj-style gating: the type still
/// exists and satisfies the channel trait, but you cannot bring a real peer
/// connection up.
pub const BUILT_WITHOUT_WEBRTC_RS: &str =
    "transport-webrtc-rs built without the 'webrtc-rs' feature; \
     rebuild with `--features webrtc-rs` to enable the webrtc-rs data-channel backend";

impl WebrtcRsTransport {
    /// Bring up the WebRTC data channel to the peer and return a ready transport.
    ///
    /// With the `webrtc-rs` feature ON this creates an RTCPeerConnection, runs the
    /// SDP offer/answer + trickle-ICE handshake over `config.signaling`, and
    /// blocks until the SCTP data channel is open (may take seconds while ICE
    /// completes — see the module docs). With the feature OFF this returns the
    /// skeleton error immediately.
    ///
    /// # Errors
    ///
    /// With the `webrtc-rs` feature OFF, always returns [`BUILT_WITHOUT_WEBRTC_RS`].
    /// With it ON, returns an error if the peer connection cannot be built, the
    /// handshake fails, or the data channel never opens.
    #[cfg(feature = "webrtc-rs")]
    pub fn connect<S: Signaling>(config: WebrtcRsConfig<S>) -> Result<Self> {
        Ok(Self {
            inner: imp::Inner::connect(config)?,
        })
    }

    /// Skeleton constructor: the crate was built without the `webrtc-rs` feature.
    /// `config` is accepted (and dropped) so the signature is identical across
    /// feature states and callers type-check either way.
    #[cfg(not(feature = "webrtc-rs"))]
    pub fn connect<S: Signaling>(_config: WebrtcRsConfig<S>) -> Result<Self> {
        Err(Error::new(BUILT_WITHOUT_WEBRTC_RS))
    }
}

/// `transport-webrtc-rs` offers the ANONYMOUS channel kind: a data channel
/// delivers received messages as bare bytes, with no sender identity, so `recv`
/// yields `Vec<Vec<u8>>` and there is no [`transport_core::SenderId`].
#[async_trait]
impl AnonymousChannel for WebrtcRsTransport {
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        #[cfg(feature = "webrtc-rs")]
        {
            self.inner.send(message)
        }
        #[cfg(not(feature = "webrtc-rs"))]
        {
            let _ = message;
            Err(Error::new(BUILT_WITHOUT_WEBRTC_RS))
        }
    }

    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        #[cfg(feature = "webrtc-rs")]
        {
            self.inner.recv()
        }
        #[cfg(not(feature = "webrtc-rs"))]
        {
            Err(Error::new(BUILT_WITHOUT_WEBRTC_RS))
        }
    }
}

/// Wrap opaque bytes as one framed record, ready to write as a binary
/// data-channel message. Byte-transparent: the transport does NOT interpret
/// `bytes` (on the ptj path they are a [`transport_core::Message`] envelope, but a
/// transport only moves bytes — envelope tagging is transport-core's job, not
/// ours). Shared by the real send path and the framing roundtrip test.
///
/// The payload is `frame(bytes)`: one length-prefixed [`transport_core::frame`]
/// record around the opaque bytes.
// Only the feature-on send path (and the tests) call this; with the feature off
// the crate is a skeleton, so allow it to be unused in that build.
#[cfg_attr(not(feature = "webrtc-rs"), allow(dead_code))]
pub(crate) fn wrap_outgoing(bytes: &[u8]) -> Vec<u8> {
    transport_core::frame(bytes)
}

/// Drain every complete framed record from a receive buffer, in order, leaving
/// any trailing partial record in `buf` for the next poll.
///
/// This is the push->pull adaptor's core: webrtc-rs's `on_message` callback
/// appends each received binary payload to `buf` (SCTP may fragment a large
/// record across several messages, and several small records may arrive in one),
/// and this loops [`transport_core::deframe`] to pull out every whole record. It
/// never interprets the bytes it yields. A length prefix exceeding
/// [`transport_core::MAX_FRAME_LEN`] surfaces as an error (the caller tears the
/// channel down). Shared by the real recv path and the framing tests.
// Only the feature-on recv path (and the tests) call this; allow unused in the
// feature-off skeleton build.
#[cfg_attr(not(feature = "webrtc-rs"), allow(dead_code))]
pub(crate) fn drain_frames(buf: &mut Vec<u8>) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    while let Some(record) = transport_core::deframe(buf)? {
        out.push(record);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::{Message, SenderId, Transport};

    // ---------------------------------------------------------------------
    // A trivial in-memory Signaling pair, so config/connect signatures and the
    // trait bounds type-check with no network. Not used to drive a real
    // handshake (that needs the feature + webrtc-rs); it exists so the
    // `WebrtcRsConfig<S>` / `connect<S>` generics are exercised at compile time
    // and the skeleton `connect` can be called in the feature-off test.
    #[derive(Default)]
    struct MemSignaling {
        inbox: Vec<SignalBlob>,
    }
    impl Signaling for MemSignaling {
        fn push(&mut self, blob: SignalBlob) -> Result<()> {
            self.inbox.push(blob);
            Ok(())
        }
        fn poll(&mut self) -> Result<Vec<SignalBlob>> {
            Ok(std::mem::take(&mut self.inbox))
        }
    }

    // ===== channel-trait-satisfaction test (no network) =====
    //
    // Static guarantee that `WebrtcRsTransport` satisfies the frozen channel
    // contract as an ANONYMOUS channel, and that (via transport-core's blanket
    // impl for anonymous channels) it is therefore also a driver-facing
    // `Transport`. Compiling this test IS the assertion; it needs no network and
    // runs identically with or without the `webrtc-rs` feature.
    #[test]
    fn webrtc_rs_transport_is_an_anonymous_channel_and_transport() {
        fn assert_anonymous<T: AnonymousChannel>() {}
        assert_anonymous::<WebrtcRsTransport>();

        // An AnonymousChannel is a Transport for free (send->publish,
        // recv->collect) via the blanket impl in transport-core.
        fn assert_transport<T: Transport>() {}
        assert_transport::<WebrtcRsTransport>();

        // The constructor has the expected (generic over Signaling) signature.
        let _ctor: fn(WebrtcRsConfig<MemSignaling>) -> Result<WebrtcRsTransport> =
            WebrtcRsTransport::connect::<MemSignaling>;

        // `&mut dyn Transport` is what the driver actually calls through.
        fn _drives(_t: &mut dyn Transport) {}
    }

    #[test]
    fn webrtc_rs_transport_is_not_attributable() {
        // A compile-time witness that this transport does NOT offer the
        // attributable kind: recv yields bare Vec<u8>, never (SenderId, Vec<u8>).
        // We can't `assert !impl`, so we pin the recv future's output type
        // instead (the seam is async; async_trait desugars recv to a boxed
        // future whose Output is the plain Result).
        fn _recv_yields_bare_bytes(
            t: &mut WebrtcRsTransport,
        ) -> impl std::future::Future<Output = Result<Vec<Vec<u8>>>> + '_ {
            t.recv()
        }
        // SenderId exists in the hub but this transport never yields it.
        let _ = |id: SenderId| id.as_bytes().len();
    }

    // ===== skeleton behavior (only when built WITHOUT the feature) =====
    //
    // With the default (feature-off) build, the skeleton constructor errors, and
    // if a caller somehow holds a skeleton value the channel methods error the
    // same way rather than silently no-op'ing. ptj-style gating; no network.
    #[cfg(not(feature = "webrtc-rs"))]
    #[test]
    fn skeleton_reports_built_without_webrtc_rs() {
        // `block_on` drives the (non-suspending) skeleton error paths of the
        // async channel methods, matching the other transports' skeleton tests.
        use futures::executor::block_on;

        let config = WebrtcRsConfig::new(
            Role::Offerer,
            vec!["stun:stun.l.google.com:19302".to_string()],
            MemSignaling::default(),
        );
        // (`.err()` rather than `.unwrap_err()`: the transport deliberately
        // has no Debug impl for the Ok arm to print.)
        let err = WebrtcRsTransport::connect(config)
            .err()
            .expect("skeleton connect must fail");
        assert_eq!(err.message(), BUILT_WITHOUT_WEBRTC_RS);

        // Build a skeleton value directly (ZST placeholder) to exercise send/recv.
        let mut skeleton = WebrtcRsTransport { _skeleton: () };
        assert_eq!(
            block_on(skeleton.send(b"psbt-bytes".to_vec()))
                .unwrap_err()
                .message(),
            BUILT_WITHOUT_WEBRTC_RS
        );
        assert_eq!(
            block_on(skeleton.recv()).unwrap_err().message(),
            BUILT_WITHOUT_WEBRTC_RS
        );
    }

    // ===== framing roundtrip test (no network) =====
    //
    // Exercises the exact wire wrap/drain the real send/recv path uses:
    // frame(Message::encode()) out, append-to-buffer + deframe-loop back in. This
    // is the "message send/recv + framing" implemented for real, verified with no
    // webrtc-rs involvement. Runs in both feature modes.

    #[test]
    fn framed_envelope_roundtrips_through_wrap_and_drain() {
        for message in [
            Message::Psbt(b"cHNidP8BAgQC".to_vec()),
            Message::Payment(vec![0x5A; 32]),
            Message::Confirmation(vec![0xC3; 64]),
            Message::Psbt(Vec::new()),
        ] {
            let envelope = message.encode();
            let payload = wrap_outgoing(&envelope);
            // Payload is one length-prefixed frame around the TLV envelope.
            assert_eq!(payload.len(), 4 /* frame len prefix */ + envelope.len());

            // Feed it into a receive buffer and drain: exactly one record.
            let mut buf = payload;
            let records = drain_frames(&mut buf).unwrap();
            assert!(buf.is_empty(), "no trailing bytes after one whole record");
            assert_eq!(records.len(), 1);
            assert_eq!(Message::decode(&records[0]).unwrap(), message);
        }
    }

    #[test]
    fn drain_yields_multiple_records_and_retains_partials() {
        // Two whole records plus a truncated third arrive concatenated (as SCTP
        // might coalesce them): drain both whole ones and keep the partial.
        let mut buf = Vec::new();
        buf.extend_from_slice(&wrap_outgoing(&Message::Payment(vec![1, 2]).encode()));
        buf.extend_from_slice(&wrap_outgoing(&Message::Confirmation(vec![3, 4, 5]).encode()));
        let third = wrap_outgoing(&Message::Psbt(vec![0xAB; 40]).encode());
        buf.extend_from_slice(&third[..third.len() - 3]); // truncated tail

        let records = drain_frames(&mut buf).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            Message::decode(&records[0]).unwrap(),
            Message::Payment(vec![1, 2])
        );
        assert_eq!(
            Message::decode(&records[1]).unwrap(),
            Message::Confirmation(vec![3, 4, 5])
        );
        // The partial third record is retained verbatim for the next poll.
        assert_eq!(buf, third[..third.len() - 3].to_vec());
    }

    #[test]
    fn drain_reassembles_a_record_split_across_arrivals() {
        // SCTP may fragment one big record across several on_message payloads.
        // Append them incrementally; drain yields nothing until the record is
        // whole, then yields exactly it.
        let whole = wrap_outgoing(&Message::Psbt(vec![0xEE; 5000]).encode());
        let (first, second) = whole.split_at(1500);

        let mut buf = first.to_vec();
        assert!(drain_frames(&mut buf).unwrap().is_empty(), "not whole yet");
        assert_eq!(buf, first.to_vec(), "partial retained untouched");

        buf.extend_from_slice(second);
        let records = drain_frames(&mut buf).unwrap();
        assert_eq!(records.len(), 1);
        assert!(buf.is_empty());
        assert_eq!(
            Message::decode(&records[0]).unwrap(),
            Message::Psbt(vec![0xEE; 5000])
        );
    }

    #[test]
    fn drain_rejects_oversize_length_prefix() {
        // A length prefix exceeding MAX_FRAME_LEN is an error (tear the channel
        // down), not an allocation. Craft the header without allocating the body.
        let big = (transport_core::MAX_FRAME_LEN as u32 + 1).to_be_bytes();
        let mut buf = big.to_vec();
        assert!(drain_frames(&mut buf).is_err());
    }

    // ===== plain-data invariants =====

    #[test]
    fn signal_blob_is_opaque() {
        let b = SignalBlob(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(b.as_bytes(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn roles_are_distinct() {
        assert_ne!(Role::Offerer, Role::Answerer);
    }

    // In-memory signaling round-trips as a plain push/poll queue (proves the port
    // shape; the real oblivious-mailbox impl lives in its own transport crate).
    #[test]
    fn signaling_port_pushes_and_polls() {
        let mut s = MemSignaling::default();
        s.push(SignalBlob(b"offer".to_vec())).unwrap();
        s.push(SignalBlob(b"candidate".to_vec())).unwrap();
        assert_eq!(
            s.poll().unwrap(),
            vec![SignalBlob(b"offer".to_vec()), SignalBlob(b"candidate".to_vec())]
        );
        // A second poll is empty: poll drains, matching the snapshot cadence.
        assert!(s.poll().unwrap().is_empty());
    }
}
