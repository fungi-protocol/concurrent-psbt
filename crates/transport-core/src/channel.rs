//! The two channel traits — the uniform abstraction across ALL transports.
//!
//! This is a MESSAGING distinction, not a security one. Both channels move
//! OPAQUE bytes and both are pull-based (poll on `recv`, matching the
//! `publish`/`collect` cadence the sync driver already expects; a push
//! transport converts push -> pull internally behind its own buffer). Both are
//! ASYNC — `send`/`recv` are `async fn` (via [`async_trait`](mod@async_trait)) so a channel can
//! `.await` its upstream SDK directly instead of bridging through a
//! per-call `block_on`. The ENTIRE difference between them is what `recv`
//! yields about the sender:
//!
//!   * [`AnonymousChannel::recv`] -> bare bytes. No sender identity.
//!   * [`AttributableChannel::recv`] -> `(SenderId, bytes)`. The sender identity
//!     the transport provides.
//!
//! Anonymity, when present, is a property of the upstream crate (nym mixnet,
//! arti Tor, emissary I2P) — a transport just *uses* the crate. transport-core
//! contains zero security / threat-model reasoning. A transport ADVERTISES its
//! kind purely by which trait(s) it implements (both, if its upstream SDK
//! exposes both shapes).

use async_trait::async_trait;

use crate::Result;

/// Opaque sender identity an [`AttributableChannel`] provides with each received
/// message.
///
/// It is transport-supplied bytes (iroh `NodeId`, nostr/mdk group-member
/// pubkey, ...). transport-core NEVER interprets it, and the lattice join
/// (which lives entirely outside transports) ignores it: provenance is
/// unauthenticated, and folding is fail-safe under `SIGHASH_ALL`. `SenderId`
/// exists only so an attributable transport can carry the metadata its upstream
/// crate already hands it — e.g. to display "who contributed this" in a GUI.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SenderId(pub Vec<u8>);

impl SenderId {
    /// Borrow the opaque identity bytes. transport-core never parses these.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// A channel that delivers received messages as BARE opaque bytes — no sender
/// identity.
///
/// `send` broadcasts one opaque message to all participants; `recv` returns
/// every message currently available (including our own prior sends), one
/// opaque blob each, as a fresh snapshot per call (polling).
///
/// No dedup / ordering / conflict logic here — the lattice join lives OUTSIDE
/// transports. A transport only moves bytes.
///
/// `send`/`recv` are async so a channel can `.await` its upstream SDK directly.
/// `#[async_trait]` desugars them to `Pin<Box<dyn Future>>` so the trait stays
/// object-safe; the bound is `Send` so a boxed channel can move across the
/// driver's runtime worker threads.
#[async_trait]
pub trait AnonymousChannel: Send {
    /// Broadcast one opaque message to all participants.
    async fn send(&mut self, message: Vec<u8>) -> Result<()>;

    /// Return a fresh snapshot of every opaque message currently available,
    /// including our own prior sends.
    async fn recv(&mut self) -> Result<Vec<Vec<u8>>>;
}

/// A channel that delivers each received message together with a [`SenderId`]
/// the transport provides.
///
/// `send` is identical to the anonymous case (broadcast one opaque message);
/// `recv` pairs each opaque blob with its transport-supplied sender identity,
/// as a fresh snapshot per call.
///
/// Same rule as [`AnonymousChannel`]: no dedup / ordering / conflict logic.
/// Bytes only (plus the opaque identity metadata the upstream crate gives us).
///
/// Async and `Send` for the same reasons as [`AnonymousChannel`].
#[async_trait]
pub trait AttributableChannel: Send {
    /// Broadcast one opaque message to all participants.
    async fn send(&mut self, message: Vec<u8>) -> Result<()>;

    /// Return a fresh snapshot of every available message paired with the
    /// transport-supplied identity of its sender.
    async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn sender_id_is_opaque_bytes() {
        let id = SenderId(vec![1, 2, 3, 4]);
        assert_eq!(id.as_bytes(), &[1, 2, 3, 4]);
    }

    #[test]
    fn sender_id_hashes_by_value() {
        let mut set = HashSet::new();
        set.insert(SenderId(vec![0xAB; 32]));
        assert!(set.contains(&SenderId(vec![0xAB; 32])));
        assert!(!set.contains(&SenderId(vec![0xCD; 32])));
    }

    // A minimal in-memory anonymous channel, proving the async trait is
    // object-safe enough to be implemented and driven with a snapshot-per-call
    // cadence. The bodies await nothing real; `block_on` runs them to completion.
    #[derive(Default)]
    struct MemAnon {
        buf: Vec<Vec<u8>>,
    }
    #[async_trait]
    impl AnonymousChannel for MemAnon {
        async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            self.buf.push(message);
            Ok(())
        }
        async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
            Ok(self.buf.clone())
        }
    }

    #[test]
    fn anonymous_channel_snapshots_include_own_sends() {
        futures::executor::block_on(async {
            let mut ch = MemAnon::default();
            ch.send(b"a".to_vec()).await.unwrap();
            ch.send(b"b".to_vec()).await.unwrap();
            // recv is idempotent: repeated polling returns the same fresh snapshot.
            assert_eq!(ch.recv().await.unwrap(), vec![b"a".to_vec(), b"b".to_vec()]);
            assert_eq!(ch.recv().await.unwrap(), vec![b"a".to_vec(), b"b".to_vec()]);
        });
    }

    // A minimal in-memory attributable channel: recv pairs each blob with the
    // SenderId the (pretend) transport supplied.
    #[derive(Default)]
    struct MemAttr {
        buf: Vec<(SenderId, Vec<u8>)>,
        me: Vec<u8>,
    }
    #[async_trait]
    impl AttributableChannel for MemAttr {
        async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            self.buf.push((SenderId(self.me.clone()), message));
            Ok(())
        }
        async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
            Ok(self.buf.clone())
        }
    }

    #[test]
    fn attributable_channel_carries_sender_id() {
        futures::executor::block_on(async {
            let mut ch = MemAttr {
                me: vec![0xEE; 4],
                ..Default::default()
            };
            ch.send(b"hello".to_vec()).await.unwrap();
            let got = ch.recv().await.unwrap();
            assert_eq!(got, vec![(SenderId(vec![0xEE; 4]), b"hello".to_vec())]);
        });
    }
}
