//! `transport-str0m` — a WebRTC data-channel backed [`AnonymousChannel`], driven
//! by [str0m], a *sans-IO* WebRTC implementation.
//!
//! This is ordinary messaging plumbing: it uses str0m's WebRTC state machine to
//! move OPAQUE bytes between two collaborators over a DTLS/SCTP data channel. It
//! is NOT security code. It reasons about framing, the sans-IO poll loop, and
//! the connection lifecycle — never about privacy properties, adversaries, or
//! threat models.
//!
//! # Which channel kind, and why
//!
//! str0m is an **ANONYMOUS** transport, so this crate implements
//! [`AnonymousChannel`] and only that. A WebRTC data channel is a DTLS-encrypted
//! byte pipe; it does NOT hand us a verifiable peer identity we surface. (DTLS
//! carries a peer certificate fingerprint, but the transport does no security
//! reasoning and does not expose it.) So [`AnonymousChannel::recv`] yields bare
//! opaque bytes with no [`transport_core::SenderId`] — exactly like
//! transport-arti (a Tor circuit) and transport-nym (a mixnet delivery). Per the
//! frozen channel contract, a transport advertises its kind purely by which
//! trait it implements — here, [`AnonymousChannel`]. Do NOT reach for
//! [`transport_core::Attributed`]: that path is for iroh/mdk, which get an
//! identity from their SDK. str0m gives none.
//!
//! # Why str0m (sans-IO) is the PRIMARY rust WebRTC backend
//!
//! str0m is a *sans-IO* state machine: it owns no sockets and no runtime. You
//! own a [`std::net::UdpSocket`] and drive an `Rtc` value through a poll loop —
//! feed inbound datagrams and time in, drain "transmit these bytes" /
//! "wake me at this instant" / "here is an event" out. That poll model IS the
//! pull-based `publish`/`collect` cadence the driver expects, so str0m is the
//! ONLY transport in the family that needs **no tokio runtime**: the channel
//! seam is async, but this crate's `async fn`s never suspend — each call pumps
//! the sans-IO loop inline and completes (contrast transport-arti /
//! transport-nym / transport-iroh, which own a runtime and `.await` their
//! SDKs). That is why str0m is the
//! preferred CLI/tauri WebRTC backend; webrtc-rs (full-async, callback-driven)
//! is the heavier alternate, and the browser's native `RTCPeerConnection` (via
//! web-sys) is the PWA path — all three present this identical
//! [`AnonymousChannel`] seam.
//!
//! # Wire shape
//!
//! Open ONE reliable-ordered SCTP data channel. Although SCTP is
//! message-oriented, a large PSBT can exceed the ~16 KiB safe single-message
//! size and get fragmented, and the driver may put several
//! [`transport_core::Message`] envelopes on the channel, so we delimit records
//! with transport-core's generic length-prefixed framing
//! ([`transport_core::frame`] / [`transport_core::deframe`]): a `u32` big-endian
//! length prefix followed by the value bytes, with the shared 16 MiB
//! [`transport_core::MAX_FRAME_LEN`] cap. Each `send` writes one framed record
//! as a binary data-channel message; the inbound side appends every received
//! binary payload to a per-channel byte buffer and loops `deframe` to pull out
//! each complete record, retaining any trailing partial for the next poll. What
//! each record *is* (a PSBT / payment / confirmation) is orthogonal type-tagging
//! owned by [`transport_core::Message`]; framing only delimits records.
//!
//! No dedup / ordering / conflict logic lives here — the lattice join lives
//! entirely OUTSIDE transports. This crate only sends and receives opaque bytes.
//!
//! # Signaling is OUT OF BAND (and not in this crate)
//!
//! WebRTC needs the SDP offer/answer + trickle ICE candidates exchanged before
//! the data channel exists. That exchange MUST NOT go over a direct/localhost
//! signaling server or the PWA origin (either would learn the client IP). It
//! rides a BIP-77 payjoin-directory (store-and-forward mailbox) accessed through
//! an OHTTP relay — its own [`AnonymousChannel`] transport crate. This crate
//! consumes the offer/answer/candidate *blobs* as opaque bytes (see
//! [`Str0mTransport::local_handshake`] / [`Str0mTransport::accept_handshake`] /
//! [`Str0mTransport::add_remote_candidate`]); moving them is the signaling
//! transport's job, not ours. Once the channel is up, PSBT frames flow
//! peer-to-peer over the data channel, never through the directory.
//!
//! # Feature gating (mirrors how `ptj` gates `iroh-sync`)
//!
//! ALL str0m/network usage sits behind the `str0m` cargo feature. With the
//! feature OFF (the default) this crate compiles as a **skeleton**: the public
//! type and its methods exist and satisfy [`AnonymousChannel`], but every
//! constructor and I/O call returns a clear
//! [`Error`]`("transport-str0m built without the 'str0m' feature; ...")`. With
//! the feature ON, the same surface drives a real str0m `Rtc` over a
//! `std::net::UdpSocket` in `imp`.
//!
//! [str0m]: https://github.com/algesten/str0m

#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AnonymousChannel, Result};
// Only the feature-off skeleton constructs an `Error` at this level; the real
// backend builds its errors inside `imp`.
#[cfg(not(feature = "str0m"))]
use transport_core::Error;

/// Which end of the WebRTC handshake this peer plays.
///
/// WebRTC is asymmetric at setup: exactly one peer creates the SDP *offer* and
/// the other creates the *answer*. Introduction/pairing decides who is who and
/// is out of scope (a room link / session ticket names the roles). After the
/// data channel is open both ends are symmetric — send/recv are identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// This peer creates the SDP offer and opens the data channel.
    Offerer,
    /// This peer answers an offer supplied via [`Str0mTransport::accept_handshake`].
    Answerer,
}

/// Configuration for a [`Str0mTransport`].
///
/// All fields are plain data delivered out of band — the transport neither
/// discovers peers nor moves signaling itself (introduction/pairing and the
/// SDP/ICE exchange are decoupled; see the module docs). A large PSBT is carried
/// over ONE reliable-ordered data channel whose label is [`Self::channel_label`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Str0mConfig {
    /// Whether this peer offers or answers the WebRTC handshake.
    pub role: Role,
    /// The local UDP bind address for our ICE host candidate,
    /// e.g. `"0.0.0.0:0"` to let the OS pick a port. str0m parses/uses it; ICE
    /// discovers reflexive/relayed candidates as configured.
    pub bind_addr: String,
    /// The label of the single reliable-ordered data channel PSBT frames ride.
    /// Both peers must agree on it (part of the offer/answer). Defaults to
    /// `"ptj"` via [`Str0mConfig::new`].
    pub channel_label: String,
    /// Optional STUN/TURN server URIs for ICE (opaque strings; str0m parses
    /// them). Empty = host candidates only (LAN / already-reachable peers).
    pub ice_servers: Vec<String>,
}

impl Str0mConfig {
    /// A config for `role` binding UDP at `bind_addr`, using the default `"ptj"`
    /// channel label and no STUN/TURN servers.
    pub fn new(role: Role, bind_addr: impl Into<String>) -> Self {
        Self {
            role,
            bind_addr: bind_addr.into(),
            channel_label: "ptj".to_string(),
            ice_servers: Vec::new(),
        }
    }
}

/// A WebRTC data-channel backed collaborative transport, driven by str0m.
///
/// Implements [`AnonymousChannel`]: `send` writes one framed opaque record as a
/// binary data-channel message; `recv` pumps the sans-IO poll loop and returns a
/// fresh snapshot of every complete framed record received since the last poll,
/// as bare bytes (no sender identity). Bridges to the driver-facing
/// [`transport_core::Transport`] seam for free via transport-core's blanket impl
/// for anonymous channels.
///
/// # Lifecycle
///
/// 1. [`Str0mTransport::new`] — bind the UDP socket and create the str0m `Rtc`.
/// 2. Exchange signaling out of band (BIP-77 directory over OHTTP):
///    - offerer: [`local_handshake`](Self::local_handshake) -> send the SDP
///      offer blob; peer's answer -> [`accept_handshake`](Self::accept_handshake);
///    - answerer: peer's offer -> [`accept_handshake`](Self::accept_handshake)
///      returns the SDP answer blob to send back;
///    - both: exchange trickle ICE candidate blobs via
///      [`local_candidates`](Self::local_candidates) /
///      [`add_remote_candidate`](Self::add_remote_candidate).
/// 3. Once the channel is open, [`send`](AnonymousChannel::send) /
///    [`recv`](AnonymousChannel::recv) move PSBT frames peer-to-peer.
pub struct Str0mTransport {
    inner: Inner,
}

impl Str0mTransport {
    /// Bind the local UDP socket and create the str0m `Rtc` for `config`.
    ///
    /// With the `str0m` feature ON this opens a [`std::net::UdpSocket`] at
    /// `config.bind_addr` and seeds an ICE host candidate; the data channel is
    /// not up until signaling completes (see the lifecycle above). With the
    /// feature OFF this returns the skeleton error immediately.
    pub fn new(config: Str0mConfig) -> Result<Self> {
        Ok(Self {
            inner: Inner::new(config)?,
        })
    }

    /// Produce this peer's local SDP handshake blob to hand to the signaling
    /// channel: an OFFER for [`Role::Offerer`], meaningful only before
    /// [`accept_handshake`](Self::accept_handshake).
    ///
    /// The returned bytes are OPAQUE — the signaling transport (BIP-77 directory
    /// over OHTTP) moves them without interpretation. Answerers do not call this;
    /// they call [`accept_handshake`](Self::accept_handshake) with the offer and
    /// get the answer back from it.
    pub fn local_handshake(&mut self) -> Result<Vec<u8>> {
        self.inner.local_handshake()
    }

    /// Apply the remote SDP handshake blob received over the signaling channel.
    ///
    /// - Answerer: pass the remote OFFER; returns `Some(answer_blob)` to send
    ///   back over signaling.
    /// - Offerer: pass the remote ANSWER; returns `None` (nothing to send back).
    ///
    /// Blobs are opaque bytes produced by the peer's str0m (SDP text).
    pub fn accept_handshake(&mut self, remote: &[u8]) -> Result<Option<Vec<u8>>> {
        self.inner.accept_handshake(remote)
    }

    /// Drain the local trickle-ICE candidate blobs discovered since the last
    /// call, to forward to the peer over the signaling channel. Opaque bytes.
    pub fn local_candidates(&mut self) -> Result<Vec<Vec<u8>>> {
        self.inner.local_candidates()
    }

    /// Add a remote trickle-ICE candidate blob received over the signaling
    /// channel. Opaque bytes produced by the peer's str0m.
    pub fn add_remote_candidate(&mut self, candidate: &[u8]) -> Result<()> {
        self.inner.add_remote_candidate(candidate)
    }

    /// Whether the reliable-ordered data channel is open (ICE + DTLS + SCTP up).
    /// `send`/`recv` before this is ready buffer/no-op per the sans-IO loop.
    pub fn is_open(&self) -> bool {
        self.inner.is_open()
    }
}

// The channel seam is async; str0m needs no runtime, so these `async fn`s never
// suspend — each pumps the sans-IO loop inline and completes. `async_trait`
// only desugars them to the boxed-future shape the dyn-compatible seam needs.
#[async_trait]
impl AnonymousChannel for Str0mTransport {
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        self.inner.send(message)
    }

    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        self.inner.recv()
    }
}

// ===========================================================================
// Real backend — compiled only with the `str0m` feature.
// ===========================================================================
#[cfg(feature = "str0m")]
mod imp;
#[cfg(feature = "str0m")]
use imp::Inner;

// ===========================================================================
// Skeleton backend — compiled when the `str0m` feature is OFF.
//
// The public surface is identical so the crate (and the channel-trait impl)
// still compiles without the str0m SDK; every operation reports that the crate
// was built without the feature. This mirrors ptj gating `iroh-sync` and the
// transport-arti / transport-nym skeletons.
// ===========================================================================
#[cfg(not(feature = "str0m"))]
mod skeleton {
    use super::{Error, Result, Str0mConfig};

    /// The clear, uniform error every skeleton operation returns.
    fn not_built() -> Error {
        Error::new(
            "transport-str0m built without the 'str0m' feature; \
             rebuild with `--features str0m` to enable the WebRTC data-channel backend",
        )
    }

    /// Placeholder backend: holds the config so the type is well-formed, but
    /// performs no network I/O. Every method returns [`not_built`].
    pub struct Inner {
        _config: Str0mConfig,
    }

    impl Inner {
        pub fn new(config: Str0mConfig) -> Result<Self> {
            // Constructing the value is fine (no SDK/socket touched); the first
            // real operation reports the missing feature. We return the skeleton
            // immediately so callers can still hold a `Str0mTransport` and get
            // the clear error from send/recv/handshake.
            Ok(Self { _config: config })
        }

        pub fn local_handshake(&mut self) -> Result<Vec<u8>> {
            Err(not_built())
        }

        pub fn accept_handshake(&mut self, _remote: &[u8]) -> Result<Option<Vec<u8>>> {
            Err(not_built())
        }

        pub fn local_candidates(&mut self) -> Result<Vec<Vec<u8>>> {
            Err(not_built())
        }

        pub fn add_remote_candidate(&mut self, _candidate: &[u8]) -> Result<()> {
            Err(not_built())
        }

        pub fn is_open(&self) -> bool {
            false
        }

        pub fn send(&mut self, _message: Vec<u8>) -> Result<()> {
            Err(not_built())
        }

        pub fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
            Err(not_built())
        }
    }
}
#[cfg(not(feature = "str0m"))]
use skeleton::Inner;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use transport_core::{Message, Transport, deframe, frame, read_frame, write_frame};

    // ---- channel-trait-satisfaction test (no network) --------------------

    /// Static guarantee that `Str0mTransport` satisfies the frozen channel
    /// contract as an ANONYMOUS channel, and that (via transport-core's blanket
    /// impl for anonymous channels) it is therefore also a driver-facing
    /// `Transport`. Compiling this test IS the assertion; it needs no network.
    #[test]
    fn str0m_transport_is_an_anonymous_channel_and_transport() {
        fn assert_anonymous<T: AnonymousChannel>() {}
        assert_anonymous::<Str0mTransport>();

        // An AnonymousChannel is a Transport for free (send->publish,
        // recv->collect) via the blanket impl in transport-core. NOTE: it is
        // NOT wrapped in Attributed — str0m surfaces no SenderId.
        fn assert_transport<T: Transport>() {}
        assert_transport::<Str0mTransport>();

        // The constructor has the expected signature.
        let _ctor: fn(Str0mConfig) -> Result<Str0mTransport> = Str0mTransport::new;
    }

    /// With the default (feature-off) build, the skeleton is constructible but
    /// every operation returns the clear "built without the 'str0m' feature"
    /// error. This is the ptj-style gating behavior and needs no network.
    #[cfg(not(feature = "str0m"))]
    #[test]
    fn skeleton_reports_missing_feature_clearly() {
        // `block_on` drives the (non-suspending) async channel methods to
        // completion without any runtime, matching the other transport crates'
        // skeleton tests.
        use futures::executor::block_on;

        let mut t = Str0mTransport::new(Str0mConfig::new(Role::Offerer, "0.0.0.0:0"))
            .expect("constructing the skeleton transport succeeds");

        // is_open is a plain false in the skeleton (not an error path).
        assert!(!t.is_open());

        // Every I/O / handshake op reports the missing feature.
        let errs = [
            block_on(t.send(b"psbt-bytes".to_vec())).err(),
            block_on(t.recv()).err(),
            t.local_handshake().err(),
            t.accept_handshake(b"remote-sdp").err(),
            t.local_candidates().err(),
            t.add_remote_candidate(b"cand").err(),
        ];
        for slot in errs {
            let err = slot.expect("skeleton op must be an error");
            assert!(
                err.message().contains("built without the 'str0m' feature"),
                "unexpected skeleton error text: {}",
                err.message()
            );
        }
    }

    // ---- framing roundtrip test (no network) -----------------------------

    /// The transport-core length-prefixed framing this crate uses on the data
    /// channel round-trips. On the wire this transport uses the BUFFER form
    /// (`frame`/`deframe`) — it appends received binary payloads to a buffer and
    /// loops `deframe` — so we exercise that path AND its interop with the
    /// stream form, matching how a real data channel would deliver possibly
    /// coalesced/fragmented binary messages. Pure buffer ops; no WebRTC.
    #[test]
    fn wire_framing_roundtrips_without_network() {
        // Two opaque records (e.g. two Message envelopes), one small, one large
        // enough to have been fragmented across SCTP messages on a real channel.
        let first = Message::Psbt(b"cHNidP8BAgQC".to_vec()).encode();
        let second = frame_payload_of_size(20_000); // > ~16 KiB SCTP single-message

        // Sender side: each `send` writes one framed record. Simulate the peer
        // delivering them coalesced into one inbound buffer.
        let mut wire = frame(&first);
        wire.extend_from_slice(&frame(&second));
        // A trailing partial header (a fragment of the next record not yet
        // arrived) must be retained, not misparsed.
        wire.extend_from_slice(&[0x00, 0x00]);

        // Receiver side: loop deframe, retaining the trailing partial.
        let mut got = Vec::new();
        while let Some(record) = deframe(&mut wire).unwrap() {
            got.push(record);
        }
        assert_eq!(got, vec![first.clone(), second.clone()]);
        assert_eq!(wire, vec![0x00, 0x00]); // trailing partial retained

        // The first record is a real Message envelope and decodes as such.
        assert_eq!(
            Message::decode(&got[0]).unwrap(),
            Message::Psbt(b"cHNidP8BAgQC".to_vec())
        );

        // Buffer form and stream form share the wire format: `frame` output
        // reads back via `read_frame`, and `write_frame` output deframes.
        let via_buffer = frame(&first);
        let mut cur = Cursor::new(via_buffer);
        assert_eq!(read_frame(&mut cur).unwrap(), Some(first.clone()));

        let mut via_stream = Vec::new();
        write_frame(&mut via_stream, &first).unwrap();
        assert_eq!(deframe(&mut via_stream).unwrap(), Some(first));
    }

    fn frame_payload_of_size(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }
}
