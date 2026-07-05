//! transport-nym — ordinary messaging plumbing that moves OPAQUE bytes between
//! collaborators over the [nym](https://nym.com) mixnet, using the off-the-shelf
//! `nym-sdk` crate.
//!
//! This crate is NOT security code. It sends and receives opaque byte blobs; it
//! does not reason about unlinkability, adversaries, or threat models. Anonymity,
//! when present, is a property of the upstream `nym-sdk` mixnet — this crate just
//! *uses* the crate. We neither implement nor analyze any privacy property.
//!
//! # Which channel kind it offers
//!
//! [`AnonymousChannel`] only. The mixnet delivers received messages as bare
//! bytes with no sender identity attached, which is exactly the anonymous
//! channel's contract: `recv` yields `Vec<Vec<u8>>`, no [`SenderId`](transport_core::SenderId). (A nym
//! client *can* attach a single-use reply block for replies, but that is a
//! reply-routing token, not a sender identity we surface — so this transport
//! advertises the anonymous kind, and only that kind.)
//!
//! # How it satisfies the frozen channel contract
//!
//! `nym-sdk` is async (tokio) and push-based; the [`AnonymousChannel`] trait is
//! now async and pull-based. A feature-on backend would `.await` the SDK
//! directly (the same actor-at-the-edge shape transport-iroh uses), with an
//! internal buffer turning the push stream into a polling `recv` — no per-call
//! `block_on`. transport-core's blanket impl then makes every
//! [`AnonymousChannel`] a [`transport_core::Transport`] for free, so the driver
//! keeps calling `publish` / `collect` unchanged. The backend is still deferred;
//! only the skeleton is built today.
//!
//! # Feature gating (skeleton when off)
//!
//! All `nym-sdk` / network usage lives behind the `nym` cargo feature, named
//! after the transport (mirrors how ptj gates iroh behind `iroh-sync`). With the
//! feature OFF the crate still compiles: [`NymTransport`] exists, but its
//! constructor and every channel operation return a clear "built without `nym`"
//! error. With the feature ON the real mixnet send/receive is compiled in.
//!
//! The wire payload of each mixnet message is one length-prefixed
//! [`transport_core::frame`] record wrapping a [`transport_core::Message`]
//! envelope. Framing here is belt-and-suspenders record delimiting: the mixnet
//! reconstructs whole messages, but framing lets a single reconstructed payload
//! carry one clearly-delimited record and keeps this transport wire-compatible
//! with the stream transports.

// A transport moves bytes; missing docs on public items are worth catching in a
// small crate, matching transport-core's own lint posture.
#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AnonymousChannel, Error, Result};

#[cfg(feature = "nym")]
mod imp;

/// Error message returned by every operation when the crate was compiled
/// WITHOUT the `nym` feature. Shared by the skeleton paths so the text is
/// identical everywhere a caller might hit it.
#[cfg(not(feature = "nym"))]
const BUILT_WITHOUT_NYM: &str =
    "transport-nym was built without nym support; rebuild with feature `nym`";

/// A mixnet-backed anonymous transport: it moves opaque byte blobs between
/// collaborators over the nym mixnet.
///
/// Construct it with [`NymTransport::connect`], handing it the peer mixnet
/// address(es) to broadcast to (introduction / pairing — how you learned the
/// peer's `nym_address` — is out of scope; the address is an input here, exactly
/// as the iroh transport receives a `DocTicket`). It implements
/// [`AnonymousChannel`]: `send` broadcasts one opaque message to the configured
/// recipients; `recv` returns every message the mixnet has delivered since the
/// last poll (a fresh snapshot per call), each as bare bytes.
///
/// With the `nym` feature OFF this is a skeleton: [`connect`](NymTransport::connect)
/// and the channel methods return a "built without `nym`" error, so the crate
/// still builds network-free.
pub struct NymTransport {
    #[cfg(feature = "nym")]
    inner: imp::Inner,
    // A zero-sized placeholder keeps the type inhabited (and the skeleton
    // constructor buildable) when the feature is off.
    #[cfg(not(feature = "nym"))]
    _skeleton: (),
}

/// A peer's mixnet address, as the opaque string `nym-sdk` prints for a
/// `Recipient` (`<pub-keys>.<gateway>`). We never parse it — it is an input we
/// hand straight back to the SDK's address parser. Introduction / pairing (how
/// this string reaches you) is out of scope for the transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NymAddress(pub String);

impl NymAddress {
    /// Borrow the raw address string. transport-nym never interprets it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl NymTransport {
    /// Connect an ephemeral mixnet client and target the given peer addresses as
    /// broadcast recipients.
    ///
    /// `recipients` are the peer mixnet addresses to send each `send` to; they
    /// are delivered to you out of band (introduction / pairing is out of scope).
    /// An empty list is allowed — the client still receives, it just has no one
    /// to broadcast to yet.
    ///
    /// # Errors
    ///
    /// With the `nym` feature OFF, always returns the "built without `nym`"
    /// error. With it ON, returns an error if the mixnet client cannot be built
    /// or connected, or if any recipient address fails to parse.
    #[cfg(feature = "nym")]
    pub fn connect(recipients: Vec<NymAddress>) -> Result<Self> {
        Ok(NymTransport {
            inner: imp::Inner::connect(recipients)?,
        })
    }

    /// Skeleton constructor: the crate was built without the `nym` feature.
    #[cfg(not(feature = "nym"))]
    pub fn connect(_recipients: Vec<NymAddress>) -> Result<Self> {
        Err(Error::new(BUILT_WITHOUT_NYM))
    }

    /// Our own mixnet address, to hand to peers out of band so they can send to
    /// us (introduction / pairing is out of scope — this is just the string they
    /// need). `None` before a real connection exists.
    ///
    /// With the `nym` feature OFF this always returns `None`.
    pub fn our_address(&self) -> Option<NymAddress> {
        #[cfg(feature = "nym")]
        {
            Some(self.inner.our_address())
        }
        #[cfg(not(feature = "nym"))]
        {
            None
        }
    }
}

/// transport-nym offers the ANONYMOUS channel kind: the mixnet delivers received
/// messages as bare bytes, with no sender identity, so `recv` yields
/// `Vec<Vec<u8>>` and there is no [`transport_core::SenderId`].
#[async_trait]
impl AnonymousChannel for NymTransport {
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        #[cfg(feature = "nym")]
        {
            self.inner.send(message)
        }
        #[cfg(not(feature = "nym"))]
        {
            let _ = message;
            Err(Error::new(BUILT_WITHOUT_NYM))
        }
    }

    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        #[cfg(feature = "nym")]
        {
            self.inner.recv()
        }
        #[cfg(not(feature = "nym"))]
        {
            Err(Error::new(BUILT_WITHOUT_NYM))
        }
    }
}

/// Wrap opaque bytes as the payload of a single framed record, ready to hand to
/// the mixnet as the message body. Byte-transparent: the transport does NOT
/// interpret `bytes` (on the ptj path they are a [`transport_core::Message`]
/// envelope, but a
/// transport only moves bytes — envelope tagging is transport-core's job, not
/// ours). Shared by the real send path and the framing roundtrip test.
///
/// The payload is `frame(bytes)`: one length-prefixed [`transport_core::frame`]
/// record around the opaque bytes.
// Only the feature-on send path (and the tests) call this; with the feature off
// the crate is a skeleton, so allow it to be unused in that build.
#[cfg_attr(not(feature = "nym"), allow(dead_code))]
pub(crate) fn wrap_outgoing(bytes: &[u8]) -> Vec<u8> {
    transport_core::frame(bytes)
}

/// Inverse of [`wrap_outgoing`]: pull the opaque bytes back out of one framed
/// mixnet payload. Returns an error if the payload is not exactly one complete
/// framed record (incomplete, or with trailing bytes) — the transport delivers
/// only whole records, and it never interprets the bytes it unwraps.
// Only the feature-on recv path (and the tests) call this; allow unused in the
// feature-off skeleton build.
#[cfg_attr(not(feature = "nym"), allow(dead_code))]
pub(crate) fn unwrap_incoming(payload: &[u8]) -> Result<Vec<u8>> {
    let mut buf = payload.to_vec();
    match transport_core::deframe(&mut buf)? {
        Some(value) => {
            if !buf.is_empty() {
                return Err(Error::new(
                    "transport-nym: mixnet payload carried trailing bytes after one framed record",
                ));
            }
            Ok(value)
        }
        None => Err(Error::new(
            "transport-nym: mixnet payload was not a complete framed record",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::{Message, SenderId, Transport};

    // ===== channel-trait-satisfaction test (no network) =====
    //
    // Proves transport-nym advertises exactly the ANONYMOUS kind: it satisfies
    // AnonymousChannel, and via transport-core's blanket impl it is therefore a
    // Transport too. We assert this at the type level (a generic fn that only
    // accepts an AnonymousChannel / a Transport) plus a runtime drive through a
    // hand-rolled in-memory anonymous channel using the SAME wrap/unwrap the real
    // transport uses. No mixnet, no tokio, no feature required.

    fn assert_is_anonymous_channel<C: AnonymousChannel>() {}
    fn assert_is_transport<T: Transport>() {}

    #[test]
    fn nym_transport_advertises_the_anonymous_kind() {
        // Compiles only if NymTransport: AnonymousChannel (+ Transport via the
        // blanket impl). This is the advertisement: which trait(s) it implements.
        assert_is_anonymous_channel::<NymTransport>();
        assert_is_transport::<NymTransport>();
    }

    #[test]
    fn nym_transport_is_not_attributable() {
        // A compile-time witness that transport-nym does NOT offer the
        // attributable kind: an anonymous `recv` resolves to bare `Vec<Vec<u8>>`,
        // never `Vec<(SenderId, Vec<u8>)>`. The seam is async, so pin the awaited
        // OUTPUT type via a generic helper rather than the desugared fn pointer.
        // (SenderId is imported only to name it here, documenting the distinction
        // we are NOT on.)
        async fn _recv_yields_bare_bytes(t: &mut NymTransport) {
            let _snapshot: Result<Vec<Vec<u8>>> = t.recv().await;
        }
        let _ = |id: SenderId| id.as_bytes().len(); // SenderId exists but we never yield it
    }

    // A minimal in-memory anonymous channel that carries the SAME framed payload
    // the real mixnet path carries, so the trait is driven end to end without a
    // network. `send` frames+buffers; `recv` snapshots the buffer as bare bytes.
    #[derive(Default)]
    struct MemMixnet {
        wire: Vec<Vec<u8>>,
    }
    #[async_trait]
    impl AnonymousChannel for MemMixnet {
        async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            // The message reaching this trait is already the caller's opaque
            // blob; on the real path it is a Message envelope. Store it verbatim,
            // mirroring the anonymous contract (bare bytes, no sender identity).
            self.wire.push(message);
            Ok(())
        }
        async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
            Ok(self.wire.clone())
        }
    }

    #[test]
    fn anonymous_channel_snapshot_semantics_hold() {
        futures::executor::block_on(async {
            let mut ch = MemMixnet::default();
            ch.send(b"alpha".to_vec()).await.unwrap();
            ch.send(b"beta".to_vec()).await.unwrap();
            // recv is a fresh snapshot each call, includes our own prior sends,
            // and yields bare bytes with no sender identity.
            assert_eq!(
                ch.recv().await.unwrap(),
                vec![b"alpha".to_vec(), b"beta".to_vec()]
            );
            assert_eq!(
                ch.recv().await.unwrap(),
                vec![b"alpha".to_vec(), b"beta".to_vec()]
            );
            // Driven purely through the Transport seam (blanket impl) too.
            assert_eq!(
                Transport::collect(&mut ch).await.unwrap(),
                vec![b"alpha".to_vec(), b"beta".to_vec()]
            );
        });
    }

    // ===== framing roundtrip test (no network) =====
    //
    // Exercises the exact wire wrap/unwrap the real send/recv path uses:
    // frame(Message::encode()) out, deframe + Message::decode back in. This is
    // the "message send/recv + framing" implemented for real, verified without
    // any mixnet.

    #[test]
    fn framed_envelope_roundtrips_through_wrap_unwrap() {
        // Compose the Message TLV envelope on TOP of the byte-transparent framing
        // the transport applies: Message::encode() -> frame (wrap_outgoing) ->
        // deframe (unwrap_incoming) -> Message::decode. This is the exact wire
        // path the real send/recv carries, verified with no network.
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
            // Unframe back to the envelope bytes, then decode to the envelope.
            let recovered = unwrap_incoming(&payload).unwrap();
            assert_eq!(recovered, envelope);
            assert_eq!(Message::decode(&recovered).unwrap(), message);
        }
    }

    #[test]
    fn wrap_unwrap_end_to_end_over_a_channel() {
        // Send several framed envelopes through the in-memory anonymous channel,
        // then poll them back and unwrap+decode — the full send/recv shape with
        // framing, no network.
        let sent = [
            Message::Psbt(b"first".to_vec()),
            Message::Payment(b"second".to_vec()),
        ];
        let received: Vec<Message> = futures::executor::block_on(async {
            let mut ch = MemMixnet::default();
            for m in &sent {
                ch.send(wrap_outgoing(&m.encode())).await.unwrap();
            }
            ch.recv()
                .await
                .unwrap()
                .iter()
                .map(|payload| Message::decode(&unwrap_incoming(payload).unwrap()).unwrap())
                .collect()
        });
        assert_eq!(received, sent.to_vec());
    }

    #[test]
    fn unwrap_rejects_incomplete_frame() {
        // A truncated payload (missing part of the value) is not a complete
        // framed record -> a clear error, never a partial/garbled record.
        let full = wrap_outgoing(&Message::Payment(vec![1, 2, 3, 4, 5]).encode());
        let truncated = &full[..full.len() - 2];
        assert!(unwrap_incoming(truncated).is_err());
    }

    #[test]
    fn unwrap_rejects_trailing_bytes() {
        // Exactly one record per mixnet payload; extra bytes are a protocol error.
        let mut payload = wrap_outgoing(&Message::Confirmation(vec![9; 3]).encode());
        payload.push(0xFF);
        assert!(unwrap_incoming(&payload).is_err());
    }

    // ===== skeleton behavior (only when built WITHOUT the feature) =====

    #[cfg(not(feature = "nym"))]
    #[test]
    fn skeleton_operations_report_built_without_nym() {
        // connect() errors clearly... (`NymTransport` is not `Debug`, so match
        // the error out rather than `unwrap_err()`).
        let Err(err) = NymTransport::connect(vec![NymAddress("peer.gateway".into())]) else {
            panic!("skeleton connect must be an error");
        };
        assert!(err.message().contains("built without `nym`"), "{err}");

        // ...and if a caller somehow holds a skeleton value, the channel methods
        // error the same way rather than silently no-op'ing. Build one directly
        // (the skeleton is a ZST placeholder) to exercise send/recv.
        let mut skeleton = NymTransport { _skeleton: () };
        futures::executor::block_on(async {
            assert!(skeleton.send(b"x".to_vec()).await.is_err());
            assert!(skeleton.recv().await.is_err());
        });
        assert_eq!(skeleton.our_address(), None);
    }

    #[test]
    fn nym_address_is_opaque() {
        let a = NymAddress("Ab12...xyz.gateway-id".into());
        assert_eq!(a.as_str(), "Ab12...xyz.gateway-id");
    }
}
