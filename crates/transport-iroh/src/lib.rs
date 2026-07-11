//! An iroh-docs-backed [`AttributableChannel`].
//!
//! Each author replaces its record under [`FRONTIER_PREFIX`], producing a
//! bounded per-author frontier. Set reconciliation converges replicas on those
//! records. Received records retain their author as a [`SenderId`].

#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AttributableChannel, Result, SenderId};

/// Key prefix under which every node publishes its maximal tip.
///
/// The complete key appends the author's bytes, so each author replaces only
/// its own frontier record.
pub const FRONTIER_PREFIX: &[u8] = b"ptj/frontier/";

mod backend;

/// An iroh-docs-backed collaborative channel.
///
/// Implements [`AttributableChannel`]: each received blob is paired with the
/// [`SenderId`] derived from the `AuthorId` iroh-docs stamped on that record.
///
/// Construct one with [`IrohChannel::create`] or join an existing document
/// with [`IrohChannel::join`].
pub struct IrohChannel {
    inner: backend::Node,
}

/// A ticket for joining an iroh document.
pub use iroh_docs::DocTicket;

impl IrohChannel {
    /// Create a collaboration document and a write ticket for joining it.
    pub fn create() -> Result<(Self, DocTicket)> {
        let (node, ticket) = backend::Node::create()?;
        Ok((Self { inner: node }, ticket))
    }

    /// Join the collaboration document described by `ticket`.
    pub fn join(ticket: DocTicket) -> Result<Self> {
        Ok(Self {
            inner: backend::Node::join(ticket)?,
        })
    }
}

#[async_trait]
impl AttributableChannel for IrohChannel {
    /// Replace this author's frontier record with `message`.
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        self.inner.send(message).await
    }

    /// Return the latest attributable record from each author.
    async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
        self.inner.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::{Attributed, Message, Transport, deframe, frame};

    #[test]
    fn backend_is_compiled_unconditionally() {
        let manifest = include_str!("../Cargo.toml");
        let library = include_str!("lib.rs");

        assert!(
            !manifest.contains("iroh = ["),
            "crate must not define an iroh feature"
        );
        for dependency in [
            "iroh =",
            "iroh-blobs =",
            "iroh-docs =",
            "iroh-gossip =",
            "n0-future =",
            "tokio =",
        ] {
            let line = manifest
                .lines()
                .find(|line| line.starts_with(dependency))
                .unwrap_or_else(|| panic!("missing dependency: {dependency}"));
            assert!(
                !line.contains("optional = true"),
                "dependency must be unconditional: {line}"
            );
        }
        assert!(
            !library.contains("feature = \"iroh\""),
            "library must not compile a feature-off skeleton"
        );
    }

    // Keep both public transport contracts available without constructing a node.
    #[test]
    fn iroh_channel_satisfies_the_channel_contract() {
        fn assert_attributable<C: AttributableChannel>() {}
        assert_attributable::<IrohChannel>();

        fn assert_transport<T: Transport>() {}
        assert_transport::<Attributed<IrohChannel>>();

        assert_eq!(FRONTIER_PREFIX, b"ptj/frontier/");

        // The transport contract remains object-safe.
        fn _drives(_t: &mut dyn Transport) {}
    }

    // Verify the framing utility re-exported by transport-core independently of
    // Iroh's document records.
    #[test]
    fn transport_core_framing_roundtrips() {
        let payload = vec![0xABu8; 137];
        let envelope = Message::Psbt(payload.clone()).encode();
        let framed = frame(&envelope);

        let mut buf = framed.clone();
        let record = deframe(&mut buf).unwrap().expect("one complete record");
        assert!(buf.is_empty(), "no trailing bytes after one record");
        assert_eq!(Message::decode(&record).unwrap(), Message::Psbt(payload));

        let mut partial = framed[..framed.len() - 1].to_vec();
        let before = partial.clone();
        assert!(deframe(&mut partial).unwrap().is_none());
        assert_eq!(
            partial, before,
            "incomplete deframe leaves the buffer intact"
        );
    }

    #[test]
    fn transport_core_framing_delimits_multiple_records() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&frame(&Message::Payment(vec![1, 2]).encode()));
        buf.extend_from_slice(&frame(&Message::Confirmation(vec![3, 4, 5]).encode()));

        let first = deframe(&mut buf).unwrap().expect("first record");
        assert_eq!(
            Message::decode(&first).unwrap(),
            Message::Payment(vec![1, 2])
        );
        let second = deframe(&mut buf).unwrap().expect("second record");
        assert_eq!(
            Message::decode(&second).unwrap(),
            Message::Confirmation(vec![3, 4, 5])
        );
        assert!(deframe(&mut buf).unwrap().is_none());
        assert!(buf.is_empty());
    }
}
