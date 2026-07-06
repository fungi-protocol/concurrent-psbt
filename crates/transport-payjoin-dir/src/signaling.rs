//! Typed WebRTC-signaling records carried over the payjoin-directory mailbox.
//!
//! This module is PURE and generic (no network, no feature gate): it type-tags
//! the WebRTC handshake blobs and drives them over ANY
//! [`AnonymousChannel`] — normally a [`crate::PayjoinDirChannel`], but the
//! generic bound lets the whole signaling handshake be unit-tested over an
//! in-memory channel with no directory.
//!
//! The WebRTC transports (`transport-str0m` / `transport-webrtc-rs` native; web-sys
//! RTCPeerConnection in the PWA) use
//! [`SignalingChannel`] to exchange the SDP offer/answer and trickled ICE
//! candidates BEFORE the DTLS/SCTP data channel is up. To this crate the SDP and
//! ICE payloads are OPAQUE bytes (str0m/web-sys produce and consume them); we
//! only tag WHICH handshake step a record is so the peer can route it.
//!
//! # Record shape
//!
//! Each signaling record is a tiny TLV — one kind byte then the opaque payload —
//! wrapped (by the underlying channel) in one [`transport_core::frame`] slot
//! record. This is deliberately the SAME structure as
//! [`transport_core::Message`] but a DISTINCT type: signaling records ride the
//! signaling lane, PSBT [`transport_core::Message`] envelopes ride the fallback
//! lane; keeping them separate types prevents mixing offer bytes into the join.
//!
//! ```text
//!   kind_byte || opaque_payload
//!     0x10  SDP offer        (from the Initiator)
//!     0x11  SDP answer       (from the Responder)
//!     0x12  ICE candidate    (trickled, either peer)
//! ```

use transport_core::{AnonymousChannel, Error, Result};

const KIND_OFFER: u8 = 0x10;
const KIND_ANSWER: u8 = 0x11;
const KIND_ICE: u8 = 0x12;

/// One WebRTC signaling record. The payloads are OPAQUE to this crate —
/// str0m/web-sys produce (offer/answer/candidate) and consume them; we only tag
/// which handshake step a record is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalingMsg {
    /// An SDP offer (produced by the [`crate::Role::Initiator`]). Opaque SDP
    /// bytes.
    Offer(Vec<u8>),
    /// An SDP answer (produced by the [`crate::Role::Responder`]). Opaque SDP
    /// bytes.
    Answer(Vec<u8>),
    /// A trickled ICE candidate (either peer). Opaque candidate bytes.
    IceCandidate(Vec<u8>),
}

impl SignalingMsg {
    fn kind_byte(&self) -> u8 {
        match self {
            SignalingMsg::Offer(_) => KIND_OFFER,
            SignalingMsg::Answer(_) => KIND_ANSWER,
            SignalingMsg::IceCandidate(_) => KIND_ICE,
        }
    }

    fn payload(&self) -> &[u8] {
        match self {
            SignalingMsg::Offer(p) | SignalingMsg::Answer(p) | SignalingMsg::IceCandidate(p) => p,
        }
    }

    /// Encode as `kind_byte || payload`. This is the byte-transparent value the
    /// underlying channel then wraps in one framed slot record.
    pub fn encode(&self) -> Vec<u8> {
        let payload = self.payload();
        let mut out = Vec::with_capacity(1 + payload.len());
        out.push(self.kind_byte());
        out.extend_from_slice(payload);
        out
    }

    /// Decode a `kind_byte || payload` record. Errors on an empty record or an
    /// unknown kind byte (so a stray non-signaling blob can't be mistaken for a
    /// handshake step).
    pub fn decode(bytes: &[u8]) -> Result<SignalingMsg> {
        match bytes.split_first() {
            Some((&KIND_OFFER, p)) => Ok(SignalingMsg::Offer(p.to_vec())),
            Some((&KIND_ANSWER, p)) => Ok(SignalingMsg::Answer(p.to_vec())),
            Some((&KIND_ICE, p)) => Ok(SignalingMsg::IceCandidate(p.to_vec())),
            Some((&other, _)) => Err(Error::new(format!(
                "transport-payjoin-dir signaling: unknown record kind byte 0x{other:02x}"
            ))),
            None => Err(Error::new(
                "transport-payjoin-dir signaling: empty signaling record",
            )),
        }
    }
}

/// A typed WebRTC-signaling view over any [`AnonymousChannel`] (in production a
/// [`crate::PayjoinDirChannel`]).
///
/// It is the introduction/pairing channel the WebRTC transports
/// (`transport-str0m` / `transport-webrtc-rs` / the PWA web-sys path) drive:
/// [`send`](SignalingChannel::send) type-tags and publishes one handshake
/// record; [`poll`](SignalingChannel::poll) drains the channel and decodes every
/// available handshake record. Because the underlying channel is anonymous and
/// broadcast-shaped (recv returns our OWN sends too, per the channel contract),
/// [`poll`] filters out records this peer itself just sent for the offer/answer
/// steps would otherwise echo; ICE candidates are idempotent so echoes are
/// harmless. We keep it simple: the caller decides which decoded records are for
/// it (an Initiator ignores `Offer`, applies `Answer`+`IceCandidate`; a
/// Responder ignores `Answer`, applies `Offer`+`IceCandidate`).
pub struct SignalingChannel<C: AnonymousChannel> {
    channel: C,
}

impl<C: AnonymousChannel> SignalingChannel<C> {
    /// Wrap a raw anonymous channel as a typed signaling channel.
    pub fn new(channel: C) -> Self {
        Self { channel }
    }

    /// Recover the underlying channel (e.g. after the handshake completes, to
    /// reuse the same mailbox as the async PSBT fallback).
    pub fn into_inner(self) -> C {
        self.channel
    }

    /// Publish one signaling record (offer / answer / ICE candidate) into our
    /// mailbox lane. Async because the underlying channel seam is async; over
    /// the directory this is one OHTTP-encapsulated POST.
    pub async fn send(&mut self, msg: &SignalingMsg) -> Result<()> {
        self.channel.send(msg.encode()).await
    }

    /// Drain the channel and decode every available signaling record.
    ///
    /// Returns a fresh snapshot per call (polling), mirroring the underlying
    /// [`AnonymousChannel::recv`] cadence. A record that fails to decode as a
    /// known signaling kind is an error (the signaling lane should carry only
    /// signaling records).
    pub async fn poll(&mut self) -> Result<Vec<SignalingMsg>> {
        self.channel
            .recv()
            .await?
            .iter()
            .map(|bytes| SignalingMsg::decode(bytes))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    #[test]
    fn signaling_msg_roundtrips_all_kinds() {
        for msg in [
            SignalingMsg::Offer(b"v=0\r\no=- ... (opaque SDP offer)".to_vec()),
            SignalingMsg::Answer(b"v=0\r\no=- ... (opaque SDP answer)".to_vec()),
            SignalingMsg::IceCandidate(b"candidate:... (opaque ICE)".to_vec()),
            SignalingMsg::Offer(Vec::new()),
        ] {
            assert_eq!(SignalingMsg::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn decode_rejects_empty_and_unknown_kind() {
        assert!(SignalingMsg::decode(&[]).is_err());
        assert!(SignalingMsg::decode(&[0xFF, 1, 2, 3]).is_err());
    }

    // Minimal in-memory anonymous channel to drive the handshake without a
    // directory. It is broadcast-shaped: recv returns every prior send (both
    // peers' records), matching the AnonymousChannel contract. Arc<Mutex<..>>
    // (not Rc<RefCell<..>>) because the channel trait requires Send.
    #[derive(Default, Clone)]
    struct MemChannel {
        wire: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    }
    #[async_trait]
    impl AnonymousChannel for MemChannel {
        async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            self.wire.lock().expect("test wire lock").push(message);
            Ok(())
        }
        async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
            Ok(self.wire.lock().expect("test wire lock").clone())
        }
    }

    #[test]
    fn offer_answer_ice_handshake_over_a_shared_mailbox() {
        // `block_on` drives the (non-suspending) async signaling methods.
        use futures::executor::block_on;

        // One shared in-memory mailbox stands in for the directory; both peers'
        // SignalingChannels wrap clones of it (same underlying wire).
        let shared = MemChannel::default();
        let mut initiator = SignalingChannel::new(shared.clone());
        let mut responder = SignalingChannel::new(shared.clone());

        // Initiator publishes the offer; Responder polls, applies it, answers.
        block_on(initiator.send(&SignalingMsg::Offer(b"OFFER-SDP".to_vec()))).unwrap();
        let seen_by_responder = block_on(responder.poll()).unwrap();
        assert!(seen_by_responder.contains(&SignalingMsg::Offer(b"OFFER-SDP".to_vec())));
        block_on(responder.send(&SignalingMsg::Answer(b"ANSWER-SDP".to_vec()))).unwrap();

        // Both trickle an ICE candidate.
        block_on(initiator.send(&SignalingMsg::IceCandidate(b"ICE-A".to_vec()))).unwrap();
        block_on(responder.send(&SignalingMsg::IceCandidate(b"ICE-B".to_vec()))).unwrap();

        // Initiator polls: sees the answer and both ICE candidates (its own
        // offer/ICE echo back too, per the broadcast contract).
        let seen_by_initiator = block_on(initiator.poll()).unwrap();
        assert!(seen_by_initiator.contains(&SignalingMsg::Answer(b"ANSWER-SDP".to_vec())));
        assert!(seen_by_initiator.contains(&SignalingMsg::IceCandidate(b"ICE-A".to_vec())));
        assert!(seen_by_initiator.contains(&SignalingMsg::IceCandidate(b"ICE-B".to_vec())));
    }

    #[test]
    fn poll_errors_on_a_non_signaling_record() {
        use futures::executor::block_on;

        let mut ch = SignalingChannel::new(MemChannel::default());
        // Inject a raw non-signaling blob directly onto the wire.
        block_on(ch.channel.send(vec![0x00, 0x01, 0x02])).unwrap();
        assert!(block_on(ch.poll()).is_err());
    }

    #[test]
    fn into_inner_recovers_the_channel_for_fallback_reuse() {
        let ch = SignalingChannel::new(MemChannel::default());
        // After the handshake the same mailbox is reused as the PSBT fallback.
        let _raw: MemChannel = ch.into_inner();
    }
}
