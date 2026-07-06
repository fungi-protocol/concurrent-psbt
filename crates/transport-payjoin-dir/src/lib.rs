//! `transport-payjoin-dir` — a BIP 77 payjoin-directory store-and-forward
//! mailbox reached through an OHTTP relay, in the `transport-<name>` family.
//!
//! This is ordinary messaging plumbing: it moves OPAQUE bytes between two peers
//! via a payjoin directory (a store-and-forward mailbox server), where every
//! request is OHTTP-encapsulated — HPKE-sealed to the directory's gateway key
//! and forwarded by a SEPARATE OHTTP relay. It is NOT security code: the
//! metadata privacy (relay sees the client IP but not the ciphertext/its
//! destination; directory sees the ciphertext-destination but never the client
//! IP) is a property of OHTTP + the directory. This crate reasons only about
//! mailbox addressing, record framing, and poll cadence.
//!
//! # One mechanism, two roles
//!
//! 1. **WebRTC signaling channel.** WebRTC/str0m need out-of-band exchange of
//!    the SDP offer/answer and trickled ICE candidates BEFORE the P2P data
//!    channel exists. That exchange MUST NOT touch a localhost/direct signaling
//!    server or the PWA origin (either would learn the client IP). Instead the
//!    SDP/ICE blobs are opaque HPKE-sealed bytes moved through this oblivious
//!    mailbox. `transport-str0m` / `transport-webrtc-rs` consume this crate as
//!    their introduction/pairing channel; once the data channel is up, PSBT
//!    frames flow peer-to-peer, NOT through the directory. The typed [`SignalingChannel`] wrapper
//!    frames offer/answer/ICE as [`SignalingMsg`] records over the raw channel.
//!
//! 2. **Async fallback transport.** The design docs' browser-viable "payjoin
//!    directory via OHTTP" row: the SAME mailbox is a plain
//!    [`AnonymousChannel`] carrying [`transport_core::Message`] PSBT envelopes,
//!    so if a peer is offline their PSBT waits in the directory.
//!
//! # Which channel kind, and why
//!
//! [`AnonymousChannel`] only. The directory delivers stored blobs as bare bytes
//! with no sender identity — it never learns who wrote a slot (OHTTP hides the
//! client IP; the blob is HPKE-sealed), so there is no
//! [`transport_core::SenderId`] to surface. Per the frozen channel contract a
//! transport advertises its kind purely by which trait it implements — here,
//! [`AnonymousChannel`]. Via transport-core's blanket impl it is therefore a
//! driver-facing [`transport_core::Transport`] for free, so the sync driver
//! keeps calling `publish`/`collect` unchanged.
//!
//! # Addressing
//!
//! See [`mailbox`]: two peers derive their read/write subdirectory slot IDs
//! from an out-of-band shared session secret as
//! `H(DOMAIN_TAG || secret || role_byte || index_be)`. A peer WRITES its own
//! [`mailbox::Role`] lane (walking the index forward on an HTTP 409 collision)
//! and READS the peer's lane (advancing the read index on each hit, stopping
//! when a slot is empty). Introduction — how the secret + role reach the peer —
//! is decoupled from the transport (a session ticket / room link), exactly as
//! arti receives a `.onion` and iroh receives a `DocTicket`.
//!
//! # Wire format
//!
//! A directory slot already has a record boundary (one POSTed blob = one slot),
//! so like the nym mailbox transport we still wrap each slot payload in ONE
//! length-prefixed [`transport_core::frame`] record. This keeps this transport
//! wire-compatible with the stream transports and lets a single slot payload
//! carry one clearly-delimited record. What a record *is* (a
//! [`transport_core::Message`] PSBT envelope on the fallback path, or a
//! [`SignalingMsg`] on the signaling path) is orthogonal type-tagging the
//! transport never inspects.
//!
//! # wasm-compatible, both sides
//!
//! The feature-on backend is authored so its deps (rust-payjoin v2 / BIP-77
//! directory client + `ohttp` + `bhttp` + a wasm-capable http client) resolve on
//! `wasm32-unknown-unknown` for the PWA client AND on native for the str0m peer.
//! The directory is reached with plain HTTP requests (grounded in the browser
//! via fetch), each OHTTP-encapsulated. See [`imp`] (feature-on) for how the
//! http client is abstracted so the PWA can inject a fetch-backed sender.
//!
//! # Feature gating (mirrors how the ptj CLI gates iroh-sync)
//!
//! ALL payjoin/ohttp/network usage lives behind the `payjoin-dir` cargo
//! feature. With the feature OFF (the default) the crate is a **skeleton**:
//! [`PayjoinDirChannel::open`] and the channel methods return a clear
//! "built without the `payjoin-dir` feature" error, and the mailbox-addressing,
//! channel-trait-satisfaction, signaling-record, and framing-roundtrip tests all
//! run with no network and no SDK. With the feature ON, the same surface
//! performs real OHTTP-encapsulated POST/GET-poll mailbox I/O via [`imp`].

#![warn(missing_docs)]

pub mod mailbox;
pub mod signaling;

pub use mailbox::{slot_id, Role, SlotId, MIN_SESSION_SECRET_LEN};
pub use signaling::{SignalingChannel, SignalingMsg};

use async_trait::async_trait;
use transport_core::{AnonymousChannel, Error, Result};

/// How to reach the payjoin directory through an OHTTP relay, plus the
/// out-of-band session parameters that address our mailbox lanes.
///
/// Every field is plain data delivered out of band — the transport never
/// discovers or pairs peers itself (introduction is decoupled and out of scope).
/// The directory / relay URLs and the gateway key config are opaque strings this
/// crate hands straight to the payjoin/ohttp SDKs; it never parses their
/// internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayjoinDirConfig {
    /// The payjoin directory base URL (the store-and-forward mailbox server),
    /// e.g. `https://payjo.in`. Slots live under it, keyed by [`SlotId`].
    pub directory_url: String,
    /// The OHTTP relay URL that forwards our encapsulated requests to the
    /// directory. A SEPARATE host from the directory: the relay sees our IP but
    /// not the ciphertext; the directory sees the ciphertext but not our IP.
    /// NEVER a localhost/direct signaling server and never the PWA origin.
    pub ohttp_relay_url: String,
    /// The directory's OHTTP gateway key material (ohttp-keys / key config),
    /// opaque to us; the `ohttp` crate parses it to seal requests to the
    /// directory's gateway. Delivered out of band with the session ticket (or
    /// fetched from the directory's key endpoint at open time).
    pub ohttp_keys: Vec<u8>,
    /// The out-of-band shared session secret both peers hold. Mailbox slot IDs
    /// derive from it (see [`mailbox`]). Must be at least
    /// [`MIN_SESSION_SECRET_LEN`] bytes.
    pub session_secret: Vec<u8>,
    /// Which lane THIS peer writes. The session creator (SDP offerer) is
    /// [`Role::Initiator`]; the joiner (SDP answerer) is [`Role::Responder`].
    pub role: Role,
}

impl PayjoinDirConfig {
    /// Assemble a config. Does not touch the network. Validate the secret with
    /// [`mailbox::validate_session_secret`] (also done inside
    /// [`PayjoinDirChannel::open`]) before relying on the derived slot IDs.
    pub fn new(
        directory_url: impl Into<String>,
        ohttp_relay_url: impl Into<String>,
        ohttp_keys: Vec<u8>,
        session_secret: Vec<u8>,
        role: Role,
    ) -> Self {
        Self {
            directory_url: directory_url.into(),
            ohttp_relay_url: ohttp_relay_url.into(),
            ohttp_keys,
            session_secret,
            role,
        }
    }
}

/// A payjoin-directory-over-OHTTP store-and-forward mailbox as an
/// [`AnonymousChannel`].
///
/// `send` POSTs one framed opaque record to our next write slot
/// (`slot_id(our_role, write_index)`), walking the index forward on an HTTP 409
/// collision; `recv` GET-polls the peer's lane (`slot_id(peer_role,
/// read_index)`), advancing on each hit and returning a fresh snapshot of the
/// newly-arrived framed records as bare bytes. Every request is
/// OHTTP-encapsulated through the relay. Bridges to the driver-facing
/// `Transport` seam for free via transport-core's blanket impl for anonymous
/// channels.
pub struct PayjoinDirChannel {
    inner: Inner,
}

impl PayjoinDirChannel {
    /// Open a mailbox channel against the directory described by `config`.
    ///
    /// With the `payjoin-dir` feature ON this validates the session secret and
    /// prepares the OHTTP-encapsulation context (it does not block on any peer;
    /// the mailbox is store-and-forward). With the feature OFF it returns the
    /// skeleton error immediately.
    pub fn open(config: PayjoinDirConfig) -> Result<Self> {
        Ok(Self {
            inner: Inner::open(config)?,
        })
    }
}

// The channel seam is async (async_trait desugars to the boxed-future shape
// the dyn-compatible seam needs); the mailbox bodies themselves are the
// backend's own (sync skeleton today).
#[async_trait]
impl AnonymousChannel for PayjoinDirChannel {
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        self.inner.send(message)
    }

    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        self.inner.recv()
    }
}

/// Wrap opaque bytes as the payload of a single framed slot record.
///
/// Byte-transparent: the transport does NOT interpret `bytes` (on the fallback
/// path they are a [`transport_core::Message`] envelope; on the signaling path a
/// [`SignalingMsg`]) — framing only delimits records. The payload is
/// `frame(bytes)`: one length-prefixed [`transport_core::frame`] record.
// Only the feature-on send path (and the tests) call this; with the feature off
// the crate is a skeleton, so allow it to be unused in that build.
#[cfg_attr(not(feature = "payjoin-dir"), allow(dead_code))]
pub(crate) fn wrap_outgoing(bytes: &[u8]) -> Vec<u8> {
    transport_core::frame(bytes)
}

/// Inverse of [`wrap_outgoing`]: pull the opaque bytes out of one framed slot
/// payload. Errors if the payload is not exactly one complete framed record
/// (incomplete, or with trailing bytes) — the directory stores one record per
/// slot, and this crate never interprets the bytes it unwraps.
#[cfg_attr(not(feature = "payjoin-dir"), allow(dead_code))]
pub(crate) fn unwrap_incoming(payload: &[u8]) -> Result<Vec<u8>> {
    let mut buf = payload.to_vec();
    match transport_core::deframe(&mut buf)? {
        Some(value) => {
            if !buf.is_empty() {
                return Err(Error::new(
                    "transport-payjoin-dir: slot payload carried trailing bytes after one framed record",
                ));
            }
            Ok(value)
        }
        None => Err(Error::new(
            "transport-payjoin-dir: slot payload was not a complete framed record",
        )),
    }
}

// ===========================================================================
// Real backend — compiled only with the `payjoin-dir` feature.
// ===========================================================================
#[cfg(feature = "payjoin-dir")]
mod imp;
#[cfg(feature = "payjoin-dir")]
use imp::Inner;

// ===========================================================================
// Skeleton backend — compiled when the `payjoin-dir` feature is OFF.
//
// The public surface is identical so the crate (and the channel-trait impl)
// still compiles without the payjoin/ohttp SDKs; every network operation reports
// that the crate was built without the feature. This mirrors ptj gating
// `iroh-sync` and the sibling deferred transports (arti/nym/emissary/mdk).
// ===========================================================================
#[cfg(not(feature = "payjoin-dir"))]
mod skeleton {
    use super::{Error, PayjoinDirConfig, Result};

    /// The clear, uniform error every skeleton network operation returns.
    fn not_built() -> Error {
        Error::new(
            "transport-payjoin-dir built without the 'payjoin-dir' feature; \
             rebuild with `--features payjoin-dir` to enable the BIP-77 \
             payjoin-directory-over-OHTTP mailbox backend",
        )
    }

    /// Placeholder backend: holds the config so the type is well-formed, but
    /// performs no network I/O. Every method returns [`not_built`].
    pub struct Inner {
        _config: PayjoinDirConfig,
    }

    impl Inner {
        pub fn open(config: PayjoinDirConfig) -> Result<Self> {
            // Constructing the value is fine (no SDK touched); the first real
            // network op reports the missing feature. We return the skeleton so
            // callers can still hold a `PayjoinDirChannel` and get the clear
            // error from send/recv.
            Ok(Self { _config: config })
        }

        pub fn send(&mut self, _message: Vec<u8>) -> Result<()> {
            Err(not_built())
        }

        pub fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
            Err(not_built())
        }
    }
}
#[cfg(not(feature = "payjoin-dir"))]
use skeleton::Inner;

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::{Message, SenderId, Transport};

    fn test_config() -> PayjoinDirConfig {
        PayjoinDirConfig::new(
            "https://payjo.in",
            "https://ohttp-relay.example",
            vec![0xAB; 32],
            b"an-out-of-band-session-secret-32-bytes!!".to_vec(),
            Role::Initiator,
        )
    }

    // ---- channel-trait-satisfaction test (no network) --------------------

    fn assert_anonymous<C: AnonymousChannel>() {}
    fn assert_transport<T: Transport>() {}

    #[test]
    fn payjoin_dir_channel_is_an_anonymous_channel_and_transport() {
        // Compiling this IS the assertion: PayjoinDirChannel: AnonymousChannel
        // (+ Transport via the blanket impl). No network required.
        assert_anonymous::<PayjoinDirChannel>();
        assert_transport::<PayjoinDirChannel>();
        let _ctor: fn(PayjoinDirConfig) -> Result<PayjoinDirChannel> = PayjoinDirChannel::open;
    }

    #[test]
    fn payjoin_dir_channel_is_not_attributable() {
        // A compile-time witness that this transport does NOT offer the
        // attributable kind: recv yields bare Vec<u8>, never (SenderId, Vec<u8>)
        // — the directory carries no sender identity. (The seam is async, so we
        // pin the recv future's output type.)
        fn _bare_recv(
            ch: &mut PayjoinDirChannel,
        ) -> impl std::future::Future<Output = Result<Vec<Vec<u8>>>> + '_ {
            ch.recv()
        }
        let _ = |id: SenderId| id.as_bytes().len(); // SenderId exists but is never yielded
    }

    // A minimal in-memory anonymous channel carrying the SAME framed payload the
    // real directory path carries, so the trait is driven end to end with no
    // network. `send` frames+buffers; `recv` snapshots as bare bytes.
    #[derive(Default)]
    struct MemDirectory {
        slots: Vec<Vec<u8>>,
    }
    #[async_trait]
    impl AnonymousChannel for MemDirectory {
        async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            self.slots.push(message);
            Ok(())
        }
        async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
            Ok(self.slots.clone())
        }
    }

    #[test]
    fn anonymous_channel_snapshot_semantics_hold() {
        // `block_on` drives the (non-suspending) async channel methods, matching
        // the sibling transports' tests.
        use futures::executor::block_on;

        let mut ch = MemDirectory::default();
        block_on(ch.send(b"offer".to_vec())).unwrap();
        block_on(ch.send(b"ice-1".to_vec())).unwrap();
        // recv is a fresh snapshot each call, includes our own prior sends, and
        // yields bare bytes with no sender identity.
        assert_eq!(
            block_on(ch.recv()).unwrap(),
            vec![b"offer".to_vec(), b"ice-1".to_vec()]
        );
        assert_eq!(
            block_on(ch.recv()).unwrap(),
            vec![b"offer".to_vec(), b"ice-1".to_vec()]
        );
        // Driven purely through the Transport seam (blanket impl) too.
        assert_eq!(
            block_on(Transport::collect(&mut ch)).unwrap(),
            vec![b"offer".to_vec(), b"ice-1".to_vec()]
        );
    }

    // ---- framing roundtrip test (no network) -----------------------------

    #[test]
    fn framed_message_roundtrips_through_wrap_unwrap() {
        // The fallback (PSBT) path: Message::encode() -> frame (wrap_outgoing) ->
        // deframe (unwrap_incoming) -> Message::decode. The exact wire path a
        // real slot POST/GET carries, verified with no network.
        for message in [
            Message::Psbt(b"cHNidP8BAgQC".to_vec()),
            Message::Payment(vec![0x5A; 32]),
            Message::Confirmation(vec![0xC3; 64]),
            Message::Psbt(Vec::new()),
        ] {
            let envelope = message.encode();
            let payload = wrap_outgoing(&envelope);
            assert_eq!(payload.len(), 4 /* frame len prefix */ + envelope.len());
            let recovered = unwrap_incoming(&payload).unwrap();
            assert_eq!(recovered, envelope);
            assert_eq!(Message::decode(&recovered).unwrap(), message);
        }
    }

    #[test]
    fn unwrap_rejects_incomplete_and_trailing() {
        let full = wrap_outgoing(&Message::Payment(vec![1, 2, 3, 4, 5]).encode());
        // Truncated (missing part of the value) is not a complete record.
        assert!(unwrap_incoming(&full[..full.len() - 2]).is_err());
        // Exactly one record per slot; extra bytes are a protocol error.
        let mut trailing = full.clone();
        trailing.push(0xFF);
        assert!(unwrap_incoming(&trailing).is_err());
    }

    // ---- skeleton behavior (only when built WITHOUT the feature) ---------

    #[cfg(not(feature = "payjoin-dir"))]
    #[test]
    fn skeleton_reports_missing_feature_clearly() {
        use futures::executor::block_on;

        let mut ch = PayjoinDirChannel::open(test_config())
            .expect("constructing the skeleton channel succeeds");
        for err in [
            block_on(ch.send(b"offer".to_vec())).err(),
            block_on(ch.recv()).err(),
        ] {
            let err = err.expect("skeleton op must be an error");
            assert!(
                err.message().contains("built without the 'payjoin-dir' feature"),
                "unexpected skeleton error text: {}",
                err.message()
            );
        }
    }

    #[test]
    fn config_is_constructible_without_network() {
        let cfg = test_config();
        assert_eq!(cfg.role, Role::Initiator);
        assert!(cfg.session_secret.len() >= MIN_SESSION_SECRET_LEN);
    }
}
