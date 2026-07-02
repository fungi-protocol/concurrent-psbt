//! The driver-facing `Transport` seam and its channel bridges.
//!
//! `commands::sync::sync_step` in the ptj CLI drives `&mut dyn Transport` with
//! `publish` / `collect`. transport-core keeps that trait as the single
//! integration boundary so the sync driver is unchanged, and bridges the
//! richer channel traits down onto it:
//!
//!   * every [`AnonymousChannel`] IS a [`Transport`] via a blanket impl
//!     (`send` -> `publish`, `recv` -> `collect`);
//!   * an [`AttributableChannel`] becomes a [`Transport`] by wrapping it in
//!     [`Attributed`], which drops the [`SenderId`](crate::SenderId) on `collect` (the lattice
//!     join ignores it anyway). It is a newtype wrapper rather than a second
//!     blanket impl because two blanket impls of `Transport` would overlap and
//!     fail to compile.
//!
//! Net: a transport implements ONE channel trait; the CLI/webgui gets a
//! `dyn Transport` for free. A caller that wants sender identities (GUI
//! attribution display) holds the [`AttributableChannel`] directly instead of
//! wrapping it, and reads the `SenderId` off `recv`.

use async_trait::async_trait;

use crate::Result;
use crate::channel::{AnonymousChannel, AttributableChannel};

/// A pluggable byte-mover between collaboration participants — the driver seam.
///
/// Contract (verbatim from the ptj CLI's `crate::transport`):
///   1. messages are opaque byte blobs — the payload is a serialized PSBT; the
///      transport never parses or orders them;
///   2. every message a participant `publish`es is eventually visible in some
///      peer's `collect()`, and `collect()` returns the participant's own
///      writes too (idempotent self-absorption is fine — the lattice join is
///      idempotent/commutative/associative, so duplicates and order cost
///      nothing).
///
/// No dedup, no conflict resolution, no ordering here. Convergence is entirely
/// the lattice join, which lives OUTSIDE transports; the transport only gathers
/// and publishes.
///
/// `publish`/`collect` are `async fn` (via [`async_trait`](mod@async_trait)). The trait stays
/// dyn-compatible — the sync driver holds `Box<dyn Transport>` — because
/// `#[async_trait]` desugars each method to a boxed future. `Send` lets a boxed
/// transport move across the driver runtime's worker threads.
#[async_trait]
pub trait Transport: Send {
    /// Publish our current local state to all participants.
    /// `message` is one serialized PSBT (the converged local result).
    async fn publish(&mut self, message: Vec<u8>) -> Result<()>;

    /// Return every message currently known to this transport, including our
    /// own prior `publish`es. Each element is one serialized PSBT to be folded.
    /// May be called repeatedly (polling); each call returns a fresh snapshot.
    async fn collect(&mut self) -> Result<Vec<Vec<u8>>>;
}

/// Every anonymous channel is a `Transport`: `send` -> `publish`, `recv` ->
/// `collect`. The driver never needs to know it got an anonymous channel.
#[async_trait]
impl<C: AnonymousChannel> Transport for C {
    async fn publish(&mut self, message: Vec<u8>) -> Result<()> {
        self.send(message).await
    }

    async fn collect(&mut self) -> Result<Vec<Vec<u8>>> {
        self.recv().await
    }
}

/// Adapts an [`AttributableChannel`] to the [`Transport`] seam by DROPPING the
/// [`SenderId`](crate::SenderId) on `collect` (the lattice join ignores provenance).
///
/// A newtype wrapper, not a blanket impl: a second `impl<C: AttributableChannel>
/// Transport for C` would overlap the anonymous blanket impl above and fail to
/// compile. A caller that wants the identities holds the underlying
/// [`AttributableChannel`] directly instead of wrapping it.
pub struct Attributed<C: AttributableChannel>(pub C);

impl<C: AttributableChannel> Attributed<C> {
    /// Wrap an attributable channel for the driver seam.
    pub fn new(channel: C) -> Self {
        Attributed(channel)
    }

    /// Recover the wrapped channel (e.g. to switch to identity-aware `recv`).
    pub fn into_inner(self) -> C {
        self.0
    }
}

#[async_trait]
impl<C: AttributableChannel> Transport for Attributed<C> {
    async fn publish(&mut self, message: Vec<u8>) -> Result<()> {
        self.0.send(message).await
    }

    async fn collect(&mut self) -> Result<Vec<Vec<u8>>> {
        // Drop the SenderId: the driver seam only moves opaque bytes.
        Ok(self
            .0
            .recv()
            .await?
            .into_iter()
            .map(|(_sender, message)| message)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::SenderId;

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

    #[derive(Default)]
    struct MemAttr {
        buf: Vec<(SenderId, Vec<u8>)>,
    }
    #[async_trait]
    impl AttributableChannel for MemAttr {
        async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            self.buf.push((SenderId(vec![0x01]), message));
            Ok(())
        }
        async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
            Ok(self.buf.clone())
        }
    }

    // Drive whatever we're given purely through the async Transport seam.
    async fn drive(t: &mut dyn Transport) -> Vec<Vec<u8>> {
        t.publish(b"x".to_vec()).await.unwrap();
        t.publish(b"y".to_vec()).await.unwrap();
        t.collect().await.unwrap()
    }

    #[test]
    fn anonymous_channel_is_a_transport() {
        futures::executor::block_on(async {
            let mut ch = MemAnon::default();
            assert_eq!(drive(&mut ch).await, vec![b"x".to_vec(), b"y".to_vec()]);
        });
    }

    #[test]
    fn attributed_wrapper_drops_sender_id() {
        futures::executor::block_on(async {
            let mut wrapped = Attributed::new(MemAttr::default());
            // collect() yields bare bytes; the SenderId has been dropped.
            assert_eq!(
                drive(&mut wrapped).await,
                vec![b"x".to_vec(), b"y".to_vec()]
            );
        });
    }

    #[test]
    fn attributed_into_inner_recovers_identity_aware_channel() {
        futures::executor::block_on(async {
            let mut wrapped = Attributed::new(MemAttr::default());
            wrapped.publish(b"z".to_vec()).await.unwrap();
            // A caller wanting attribution recovers the channel and reads SenderId.
            let mut inner = wrapped.into_inner();
            let got = inner.recv().await.unwrap();
            assert_eq!(got, vec![(SenderId(vec![0x01]), b"z".to_vec())]);
        });
    }
}
