//! transport-emissary — the I2P transport in the `transport-<name>` family.
//!
//! This crate moves OPAQUE bytes between collaborators over I2P, using the
//! upstream `emissary-core` I2P stack. It is ordinary messaging plumbing:
//! it sends and receives byte blobs and delimits them on the stream with
//! transport-core's length-prefixed framing. It implements the uniform channel
//! abstraction from `transport-core` and nothing more.
//!
//! # Channel kind: anonymous
//!
//! I2P hands us a byte stream with no sender identity attached, so this crate
//! implements exactly [`transport_core::AnonymousChannel`]: [`recv`] yields bare
//! bytes. There is no [`transport_core::SenderId`] to produce, so the
//! [`transport_core::AttributableChannel`] trait is deliberately not
//! implemented. That is the entire "kind" distinction — a *messaging* property
//! (what a received message carries about who sent it), not a security one.
//!
//! [`recv`]: transport_core::AnonymousChannel::recv
//!
//! Anonymity, when present, is a property of I2P (via emissary), not of this
//! crate. This crate contains zero security / threat-model reasoning; it just
//! uses the upstream stack to move bytes.
//!
//! # Embedded, in-process router (no external daemon)
//!
//! emissary-core is an *embeddable* I2P router: it runs inside our own process,
//! the same shape as transport-arti's in-process Tor client. There is no
//! separate i2pd/Java-router daemon to install —
//! [`EmissaryChannel::connect`] brings the router up itself. (Grounded against
//! the real emissary-core 0.4 API: the embedded router's client surface is the
//! SAMv3 listener it hosts in-process, so the backend speaks SAMv3 to OUR OWN
//! router on a loopback port — see `net.rs` — never to an external bridge.)
//! The router is async, and so is the [`AnonymousChannel`] seam, so the
//! feature-on backend runs the router on a dedicated actor thread and the
//! channel methods `.await` an `mpsc`/`oneshot` round-trip to it — no per-call
//! `block_on`.
//!
//! # Feature gating (mirrors how the ptj CLI gates iroh-sync)
//!
//! ALL external-network dependency usage lives behind the `emissary` cargo
//! feature:
//!
//!   * With `emissary` OFF (the default), the crate is a **skeleton**: it still
//!     compiles against `transport-core` + std only, [`EmissaryChannel::connect`]
//!     returns a clear "built without the `emissary` feature" error, and the
//!     channel-trait-satisfaction and framing-roundtrip tests run with no
//!     router and no SDK.
//!   * With `emissary` ON, [`EmissaryChannel::connect`] starts the embedded I2P
//!     router, opens a stream to the peer destination, and [`send`]/[`recv`]
//!     move framed envelopes over it.
//!
//! [`send`]: transport_core::AnonymousChannel::send
//!
//! # Wire format
//!
//! I2P streaming is a TCP-like byte stream with no inherent record boundaries,
//! so this crate uses transport-core's generic length-prefixed framing
//! ([`transport_core::write_frame`] / [`transport_core::read_frame`]): each
//! `send` writes one framed record; `recv` drains every complete record that
//! has arrived. What a record *is* (a [`transport_core::Message`] envelope) is
//! orthogonal and owned by transport-core; this crate never inspects payloads.
//!
//! No dedup / ordering / conflict logic lives here — the lattice join is
//! entirely outside transports. This crate only moves bytes.

#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AnonymousChannel, Error, Result};

#[cfg(feature = "emissary")]
mod net;

/// How to reach the I2P network and which peer stream to open.
///
/// The introduction/pairing step (how two collaborators exchange their I2P
/// destinations out of band) is decoupled from the transport and out of scope:
/// an `EmissaryChannel` receives the peer's destination as an input here.
#[derive(Debug, Clone)]
pub struct EmissaryConfig {
    /// Directory where the embedded router persists its state — our own I2P
    /// destination keys and the netdb. Reusing the same directory across runs
    /// reuses the same local destination (our stable `.b32.i2p` address), the
    /// I2P analogue of transport-arti's persisted onion-service nickname.
    pub state_dir: String,

    /// The peer's I2P destination to open a stream to — a base64 destination or
    /// a `.b32.i2p` address. This is the endpoint the pairing step handed us;
    /// the transport does not discover it.
    pub peer_destination: String,

    /// A locally-unique label for our streaming session on the embedded router.
    pub session_label: String,
}

impl EmissaryConfig {
    /// Build a config from the router state directory and a peer destination,
    /// using a default session label.
    pub fn new(state_dir: impl Into<String>, peer_destination: impl Into<String>) -> Self {
        EmissaryConfig {
            state_dir: state_dir.into(),
            peer_destination: peer_destination.into(),
            session_label: "transport-emissary".to_string(),
        }
    }

    /// Override the streaming-session label.
    pub fn with_session_label(mut self, session_label: impl Into<String>) -> Self {
        self.session_label = session_label.into();
        self
    }
}

/// An anonymous channel over I2P: a framed byte stream through an embedded,
/// in-process emissary-core router.
///
/// Implements [`AnonymousChannel`] only. Construct one with
/// [`EmissaryChannel::connect`]. With the `emissary` feature off, `connect`
/// returns a skeleton error and the type still satisfies the trait so the crate
/// (and everything that generically bounds `C: AnonymousChannel`) compiles.
pub struct EmissaryChannel {
    // The live I2P stream, present only when built WITH the feature AND a
    // connection succeeded. When the feature is off this field does not exist;
    // when it is on, an unconnected channel holds `None`.
    #[cfg(feature = "emissary")]
    stream: Option<net::I2pStream>,

    // A private field so the struct cannot be constructed except through
    // `connect` (whose skeleton form always errors). Keeps the skeleton honest:
    // no `EmissaryChannel` value can exist without the feature.
    #[cfg(not(feature = "emissary"))]
    _never: core::convert::Infallible,
}

impl EmissaryChannel {
    /// Open an anonymous I2P channel to the peer named in `config`.
    ///
    /// With the `emissary` feature ON this starts the embedded in-process I2P
    /// router (persisting to `config.state_dir`), waits for its tunnels to
    /// build, and opens a stream to the peer destination. The returned channel
    /// then frames envelopes over that stream.
    ///
    /// With the `emissary` feature OFF this is a skeleton: it returns a clear
    /// "built without the `emissary` feature" error and never constructs a
    /// channel.
    #[cfg(feature = "emissary")]
    pub fn connect(config: &EmissaryConfig) -> Result<Self> {
        let stream = net::I2pStream::connect(config)?;
        Ok(EmissaryChannel {
            stream: Some(stream),
        })
    }

    /// Skeleton form: the crate was built without the `emissary` feature, so no
    /// I2P stack is linked and no channel can be constructed.
    #[cfg(not(feature = "emissary"))]
    pub fn connect(_config: &EmissaryConfig) -> Result<Self> {
        Err(built_without_feature())
    }
}

/// The uniform error returned by every network entry point when the crate was
/// built without the `emissary` feature. Mirrors how ptj reports a
/// transport compiled out (a clear, actionable message, not a panic).
#[cfg(not(feature = "emissary"))]
fn built_without_feature() -> Error {
    Error::new(
        "transport-emissary built without the `emissary` feature: \
         rebuild with `--features emissary` to enable the I2P transport",
    )
}

#[async_trait]
impl AnonymousChannel for EmissaryChannel {
    /// Broadcast one opaque message: write it as a single framed record onto the
    /// I2P stream.
    #[cfg(feature = "emissary")]
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| Error::new("transport-emissary: channel is not connected"))?;
        stream.send_framed(&message).await
    }

    /// Skeleton form: no stream exists, so sending is impossible. Unreachable in
    /// practice because `connect` never yields a value without the feature, but
    /// the trait must still be satisfied for the crate to compile.
    #[cfg(not(feature = "emissary"))]
    async fn send(&mut self, _message: Vec<u8>) -> Result<()> {
        Err(built_without_feature())
    }

    /// Return a fresh snapshot of every complete framed record that has arrived
    /// on the stream since the last call (bare bytes — no sender identity).
    #[cfg(feature = "emissary")]
    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| Error::new("transport-emissary: channel is not connected"))?;
        stream.drain_framed().await
    }

    /// Skeleton form: no stream, nothing to receive.
    #[cfg(not(feature = "emissary"))]
    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        Err(built_without_feature())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `AnonymousChannel` is already in scope via `super::*`; pull in the extra
    // transport-core items the tests need.
    use transport_core::{Message, Transport, deframe, frame};

    // ----- channel-trait satisfaction (no network) -----
    //
    // The point of these tests is that `EmissaryChannel: AnonymousChannel`
    // holds, and — via transport-core's blanket impl — `EmissaryChannel:
    // Transport` too, so the ptj sync driver can hold a `dyn Transport` for
    // free. We verify the bounds statically without ever touching the network.

    /// Compile-time proof the crate advertises the ANONYMOUS channel kind: this
    /// only type-checks if `EmissaryChannel: AnonymousChannel`.
    fn _assert_is_anonymous_channel<C: AnonymousChannel>() {}

    /// Compile-time proof the blanket impl makes it a driver-facing `Transport`.
    fn _assert_is_transport<C: Transport>() {}

    #[test]
    fn satisfies_the_channel_and_transport_traits() {
        // If these bounds ever regressed, the crate would not compile — so
        // instantiating them here is the assertion.
        _assert_is_anonymous_channel::<EmissaryChannel>();
        _assert_is_transport::<EmissaryChannel>();
    }

    /// With the feature off, the skeleton `connect` returns the documented
    /// "built without the feature" error and constructs no channel — no network.
    #[cfg(not(feature = "emissary"))]
    #[test]
    fn skeleton_connect_reports_built_without_feature() {
        let config = EmissaryConfig::new("/tmp/transport-emissary-state", "peer.b32.i2p");
        // `EmissaryChannel` is not `Debug`, so match the error out rather than
        // `expect_err()`. In the skeleton build the `Ok` half is uninhabited (the
        // channel holds an `Infallible`), so this binding is irrefutable.
        #[allow(irrefutable_let_patterns)]
        let Err(err) = EmissaryChannel::connect(&config);
        assert!(
            err.message().contains("emissary"),
            "error should name the missing feature, got: {}",
            err.message()
        );
    }

    // ----- framing roundtrip (no network) -----
    //
    // The exact bytes this transport puts on the wire: a Message envelope
    // wrapped in transport-core's length-prefixed frame. We roundtrip it purely
    // in memory to prove the send-side framing and the recv-side deframing agree
    // on the wire format the I2P stream carries, with no SDK and no sockets.

    #[test]
    fn framing_roundtrip_over_an_in_memory_stream() {
        // Send side: three envelopes, each framed as one record, concatenated
        // exactly as `send` would write them onto the stream.
        let outgoing = [
            Message::Psbt(b"cHNidP8BAgQC".to_vec()),
            Message::Payment(vec![0xAB; 32]),
            Message::Confirmation(vec![0xCD; 65]),
        ];
        let mut wire = Vec::new();
        for msg in &outgoing {
            wire.extend_from_slice(&frame(&msg.encode()));
        }

        // Recv side: drain every complete framed record, exactly as `recv`
        // would, then decode each back into a Message.
        let mut buf = wire;
        let mut received = Vec::new();
        while let Some(record) = deframe(&mut buf).unwrap() {
            received.push(Message::decode(&record).unwrap());
        }
        assert!(buf.is_empty(), "all records consumed, no trailing bytes");
        assert_eq!(received, outgoing);
    }

    #[test]
    fn framing_handles_partial_records() {
        // A record split across two arrivals: deframe yields None until the
        // whole record is present, then returns it. This is the exact behavior
        // `recv` relies on when the I2P stream delivers a record in pieces.
        let full = frame(&Message::Payment(vec![0x11; 10]).encode());
        let split = full.len() - 3;

        let mut buf = full[..split].to_vec();
        assert_eq!(deframe(&mut buf).unwrap(), None, "record not yet complete");

        buf.extend_from_slice(&full[split..]);
        let record = deframe(&mut buf).unwrap().expect("now complete");
        assert_eq!(
            Message::decode(&record).unwrap(),
            Message::Payment(vec![0x11; 10])
        );
        assert!(buf.is_empty());
    }
}
