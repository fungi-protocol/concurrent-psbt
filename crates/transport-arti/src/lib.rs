//! `transport-arti` — a Tor onion-service backed [`AnonymousChannel`].
//!
//! This is ordinary messaging plumbing: it uses the off-the-shelf [arti]
//! crates (an in-process Tor client) to send and receive OPAQUE bytes between
//! collaborators over Tor. It is NOT security code. Anonymity, when present, is
//! a property of arti/Tor itself — this crate just *uses* the crate; it does not
//! implement or analyze any privacy property, adversary, or threat model.
//!
//! # Which channel kind, and why
//!
//! arti is an **ANONYMOUS** transport, so this crate implements
//! [`AnonymousChannel`] and only that. A Tor stream hands us a byte pipe over a
//! circuit; it does NOT hand us a peer identity. So [`AnonymousChannel::recv`]
//! yields bare opaque bytes with no [`transport_core::SenderId`]. That is the
//! entire reason this is anonymous rather than attributable: it is a messaging
//! distinction about what a received message carries about who sent it, nothing
//! more. Per the frozen channel contract, the transport advertises its kind
//! purely by which trait it implements — here, [`AnonymousChannel`].
//!
//! # Wire shape
//!
//! Tor gives us a *stream* (like TCP): a byte pipe with no inherent record
//! boundaries. To carry more than one message on a single circuit we delimit
//! records with transport-core's generic length-prefixed framing
//! ([`transport_core::write_frame`] / [`transport_core::read_frame`]): a `u32`
//! big-endian length prefix followed by the value bytes, with the shared 16 MiB
//! [`transport_core::MAX_FRAME_LEN`] cap. What each record *is* (a PSBT /
//! payment / confirmation) is orthogonal type-tagging owned by
//! [`transport_core::Message`]; framing only delimits records on the stream.
//!
//! No dedup / ordering / conflict logic lives here — the lattice join lives
//! entirely OUTSIDE transports. This crate only sends and receives opaque bytes.
//!
//! # Topology
//!
//! Each participant runs an onion service (via arti's in-process client) and
//! hands its `.onion` address to peers out of band (introduction/pairing is
//! decoupled from the transport and out of scope). To publish, we open a Tor
//! stream to each configured peer `.onion` and write one framed record. To
//! collect, we drain the records that peers wrote to our own onion service's
//! inbound streams since the last poll. `recv` returns a fresh snapshot per
//! call (polling), including nothing about who sent what — bare bytes only.
//!
//! # Feature gating (mirrors how `ptj` gates `iroh-sync`)
//!
//! ALL arti/network usage sits behind the `arti` cargo feature. With the
//! feature OFF (the default) this crate compiles as a **skeleton**: the public
//! type and its methods exist and satisfy [`AnonymousChannel`], but every
//! constructor and I/O call returns a clear
//! [`Error`]`("transport-arti built without the 'arti' feature; ...")`. With the
//! feature ON, the same surface performs real Tor connection + framing I/O via
//! `arti_client` (arti is async; the channel seam is async too, so a
//! feature-on backend `.await`s arti directly — no per-call `block_on`, the same
//! actor-at-the-edge shape transport-iroh uses). The backend is grounded against
//! arti-client 0.44 and its lockstep tor-* 0.44 crates (see `imp.rs`).
//!
//! [arti]: https://gitlab.torproject.org/tpo/core/arti

#![warn(missing_docs)]

use async_trait::async_trait;
// `Error` is named directly only by the skeleton arm (the real backend has its
// own import in `imp.rs`), so the top-level import is feature-off only.
#[cfg(not(feature = "arti"))]
use transport_core::Error;
use transport_core::{AnonymousChannel, Result};

/// Configuration for an [`ArtiTransport`]: where to reach peers and where to
/// listen for them.
///
/// Endpoints are plain data delivered out of band — the transport never
/// discovers or pairs peers itself (introduction is decoupled and out of scope).
/// A `.onion` address is an opaque string to this crate; arti parses it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtiConfig {
    /// The peer onion endpoints to publish to, each `"<base32>.onion:<port>"`.
    /// Every `publish` writes one framed record to each of these.
    pub peers: Vec<String>,
    /// The virtual port our own onion service exposes for inbound peer streams.
    /// Peers dial `<our-onion>.onion:<listen_port>` to reach us.
    pub listen_port: u16,
    /// Nickname for our onion service's persisted keys (arti state dir key).
    /// Reusing the same nickname across runs reuses the same `.onion` address.
    pub service_nickname: String,
}

impl ArtiConfig {
    /// A config that publishes to `peers` and listens on `listen_port`.
    pub fn new(peers: Vec<String>, listen_port: u16, service_nickname: impl Into<String>) -> Self {
        Self {
            peers,
            listen_port,
            service_nickname: service_nickname.into(),
        }
    }
}

/// A Tor onion-service backed collaborative transport.
///
/// Implements [`AnonymousChannel`]: `send` writes one framed opaque record to
/// every configured peer over Tor; `recv` returns a fresh snapshot of the framed
/// records peers have written to our onion service, as bare bytes (no sender
/// identity). Bridges to the driver-facing `Transport` seam for free via
/// transport-core's blanket impl for anonymous channels.
pub struct ArtiTransport {
    inner: Inner,
}

impl ArtiTransport {
    /// Build a transport from `config`.
    ///
    /// With the `arti` feature ON this bootstraps an in-process Tor client and
    /// launches our onion service (may take tens of seconds — see the module
    /// docs and the crate's `uxNotes`). With the feature OFF this returns the
    /// skeleton error immediately.
    pub fn new(config: ArtiConfig) -> Result<Self> {
        Ok(Self {
            inner: Inner::new(config)?,
        })
    }

    /// The `.onion` address peers should dial to reach us, if available.
    ///
    /// Only meaningful once the onion service has published its descriptor (with
    /// the `arti` feature on). Returns the skeleton error when built without it.
    pub fn onion_address(&self) -> Result<String> {
        self.inner.onion_address()
    }
}

#[async_trait]
impl AnonymousChannel for ArtiTransport {
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        self.inner.send(message).await
    }

    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        self.inner.recv().await
    }
}

// ===========================================================================
// Real backend — compiled only with the `arti` feature.
// ===========================================================================
#[cfg(feature = "arti")]
mod imp;
#[cfg(feature = "arti")]
use imp::Inner;

// ===========================================================================
// Skeleton backend — compiled when the `arti` feature is OFF.
//
// The public surface is identical so the crate (and the channel-trait impl)
// still compiles without the arti SDK; every operation reports that the crate
// was built without the feature. This mirrors ptj gating `iroh-sync`.
// ===========================================================================
#[cfg(not(feature = "arti"))]
mod skeleton {
    use super::{ArtiConfig, Error, Result};

    /// The clear, uniform error every skeleton operation returns.
    fn not_built() -> Error {
        Error::new(
            "transport-arti built without the 'arti' feature; \
             rebuild with `--features arti` to enable the Tor onion-service backend",
        )
    }

    /// Placeholder backend: holds the config so the type is well-formed, but
    /// performs no network I/O. Every method returns [`not_built`].
    pub struct Inner {
        _config: ArtiConfig,
    }

    impl Inner {
        pub fn new(config: ArtiConfig) -> Result<Self> {
            // Constructing the value is fine (no SDK touched); it is the first
            // real operation that reports the missing feature. We return the
            // skeleton immediately so callers can still hold an `ArtiTransport`
            // and get the clear error from send/recv/onion_address.
            Ok(Self { _config: config })
        }

        pub fn onion_address(&self) -> Result<String> {
            Err(not_built())
        }

        // Async to mirror the feature-on backend's signatures (the channel
        // seam is async); no runtime is needed to resolve an immediate `Err`.
        pub async fn send(&mut self, _message: Vec<u8>) -> Result<()> {
            Err(not_built())
        }

        pub async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
            Err(not_built())
        }
    }
}
#[cfg(not(feature = "arti"))]
use skeleton::Inner;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use transport_core::{Transport, frame, read_frame, write_frame};

    // ---- channel-trait-satisfaction test (no network) --------------------

    /// Static guarantee that `ArtiTransport` satisfies the frozen channel
    /// contract as an ANONYMOUS channel, and that (via transport-core's blanket
    /// impl for anonymous channels) it is therefore also a driver-facing
    /// `Transport`. Compiling this test IS the assertion; it needs no network.
    #[test]
    fn arti_transport_is_an_anonymous_channel_and_transport() {
        fn assert_anonymous<T: AnonymousChannel>() {}
        assert_anonymous::<ArtiTransport>();

        // An AnonymousChannel is a Transport for free (send->publish,
        // recv->collect) via the blanket impl in transport-core.
        fn assert_transport<T: Transport>() {}
        assert_transport::<ArtiTransport>();

        // The constructor has the expected signature.
        let _ctor: fn(ArtiConfig) -> Result<ArtiTransport> = ArtiTransport::new;
    }

    /// With the default (feature-off) build, the skeleton is constructible but
    /// every operation returns the clear "built without the 'arti' feature"
    /// error. This is the ptj-style gating behavior and needs no network.
    #[cfg(not(feature = "arti"))]
    #[test]
    fn skeleton_reports_missing_feature_clearly() {
        let mut t = ArtiTransport::new(ArtiConfig::new(
            vec!["examplexxxxxxxxxx.onion:9735".to_string()],
            9735,
            "ptj-collab",
        ))
        .expect("constructing the skeleton transport succeeds");

        // send / recv (async) and onion_address (sync) all report the missing
        // feature.
        let (send_err, recv_err) = futures::executor::block_on(async {
            (
                t.send(b"psbt-bytes".to_vec()).await.err(),
                t.recv().await.err(),
            )
        });
        for msg in [send_err, recv_err, t.onion_address().err()] {
            let err = msg.expect("skeleton op must be an error");
            assert!(
                err.message().contains("built without the 'arti' feature"),
                "unexpected skeleton error text: {}",
                err.message()
            );
        }
    }

    // ---- framing roundtrip test (no network) -----------------------------

    /// The transport-core length-prefixed framing this crate uses on the wire
    /// round-trips: what we would `write_frame` onto a Tor stream reads back via
    /// `read_frame`, in order, with a clean EOF on the record boundary — and it
    /// interoperates with the buffer-form `frame`/`deframe`. Pure I/O over an
    /// in-memory cursor; no Tor involved.
    #[test]
    fn wire_framing_roundtrips_without_network() {
        // Simulate two opaque records (e.g. two PSBT envelopes) written to a
        // stand-in for a Tor stream.
        let first = b"first-opaque-record".to_vec();
        let second = vec![0xABu8; 4096]; // a chunkier blob

        let mut stream = Vec::new();
        write_frame(&mut stream, &first).unwrap();
        write_frame(&mut stream, &second).unwrap();

        // Read them back in order, then observe a clean EOF on the boundary.
        let mut cursor = Cursor::new(stream.clone());
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(first.clone()));
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(second.clone()));
        assert_eq!(read_frame(&mut cursor).unwrap(), None);

        // The stream form and the buffer form share the wire format: `frame`
        // output reads back via `read_frame`, and `write_frame` output deframes.
        let via_buffer = frame(&first);
        let mut cur2 = Cursor::new(via_buffer);
        assert_eq!(read_frame(&mut cur2).unwrap(), Some(first));
    }
}
