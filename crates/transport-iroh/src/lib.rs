//! transport-iroh — an iroh-docs-backed [`AttributableChannel`] for the
//! transport family.
//!
//! This is ordinary messaging plumbing. It uses the off-the-shelf iroh stack
//! (`iroh` + `iroh-docs` + `iroh-blobs` + `iroh-gossip`) to move OPAQUE bytes
//! between collaborators. It contains no security / threat-model reasoning: the
//! only thing it does with identity is carry, verbatim, the `AuthorId` that
//! iroh-docs already stamps on every record, exposing it as the
//! [`SenderId`] the attributable-channel contract
//! yields. That is the entire "attribution" — a piece of metadata the upstream
//! crate hands us, not something this crate interprets, verifies, or reasons
//! about.
//!
//! # Which channel kind(s) it offers
//!
//! iroh-docs stamps each record with its writer's `AuthorId`, so this
//! transport advertises the **attributable** kind: it implements
//! [`AttributableChannel`], pairing every
//! received blob with the `SenderId` derived from that record's `AuthorId`.
//!
//! A caller that only wants opaque bytes wraps the channel in
//! [`Attributed`](transport_core::Attributed), which drops the `SenderId` and
//! exposes the plain [`Transport`](transport_core::Transport) seam that
//! `commands::sync` drives. A caller that wants attribution (e.g. a GUI showing
//! "who contributed this fragment") holds the [`IrohChannel`] directly and reads
//! the `SenderId` off `recv`. See [`AttributableChannel`].
//!
//! # Set-reconciliation, not a history log
//!
//! iroh-docs is a range-based set-reconciliation store: two joined replicas
//! efficiently converge on the union of their records. This transport uses it
//! to hold a **bounded frontier of non-dominated states**, one record per
//! participant, keyed by [`FRONTIER_PREFIX`]` ++ author_bytes`. It is NOT a
//! growing CRDT log:
//!
//!   * [`send`](AttributableChannel::send) writes the caller's current state
//!     under THIS node's per-author frontier key. `set_bytes` supersedes our own
//!     prior, now-dominated record, so the doc retains at most one maximal tip
//!     per author.
//!   * [`recv`](AttributableChannel::recv) snapshots the doc's current frontier
//!     (the single latest record for each per-author key under the prefix),
//!     resolves each record's content hash to its blob bytes, and pairs it with
//!     the record author's `SenderId`. It includes our own prior write.
//!
//! Deduplication / ordering / conflict resolution (the lattice join) lives
//! entirely OUTSIDE this transport — it only sends and receives opaque bytes.
//!
//! # Feature gating (`iroh`)
//!
//! ALL external-network dependency usage sits behind the `iroh` cargo feature
//! (named after the transport), mirroring how `crates/ptj` gates `iroh-sync`.
//!
//!   * With `iroh` OFF, the crate compiles as a skeleton against transport-core
//!     only (bitcoin/std, no async runtime, no SDK). [`IrohChannel::create`] and
//!     [`IrohChannel::join`] return a clear ["built without iroh"](SETUP_WITHOUT_IROH)
//!     error, and the type still satisfies the channel traits so downstream code
//!     type-checks. The channel-trait-satisfaction test and the framing
//!     roundtrip test run in this mode with no network.
//!   * With `iroh` ON, the real iroh-docs backend is compiled in and the channel
//!     genuinely moves bytes between peers.
//!
//! # UX notes (honest; the comparison deliverable)
//!
//! Recorded on the type — see [`IrohChannel`] and the crate-level
//! `docs/UX.md`-style summary the build emails back. In brief: **attributable**
//! (NodeId/AuthorId); a relay is used for NAT traversal only (no data flows
//! through it); no offline delivery (both peers must be online to reconcile);
//! push-based reconciliation, ~seconds to connect once endpoints discover each
//! other; pairing is a long base32 `DocTicket` exchanged out of band; runs on
//! POSIX + mobile but **not** in a browser (needs a native endpoint / UDP).

// Deny the same cfg surface transport-core does; keep the public API documented.
#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AttributableChannel, Result, SenderId};
// `Error` is only constructed in the skeleton (no-`iroh`) arms; scope its import
// there so the real build carries no unused import under the workspace lints.
#[cfg(not(feature = "iroh"))]
use transport_core::Error;

/// Error text returned by every constructor when the crate was built WITHOUT
/// the `iroh` feature. Mirrors the ptj skeleton behavior: the type still exists
/// and satisfies the channel traits, but you cannot bring a real node up.
pub const SETUP_WITHOUT_IROH: &str = "transport-iroh: built without the `iroh` feature; rebuild with \
     --features iroh to use the iroh-docs transport";

/// Key prefix under which every node publishes its maximal tip.
///
/// The full key is `FRONTIER_PREFIX ++ author_id_bytes`, so each participant
/// writes a DISTINCT key. A prefix-scoped single-latest-per-key query therefore
/// returns exactly one record per participant — a bounded frontier, not a
/// growing log. (Ported verbatim from `crates/ptj/src/transport/iroh.rs`.)
pub const FRONTIER_PREFIX: &[u8] = b"ptj/frontier/";

// The real backend is compiled in only with the `iroh` feature. It runs the
// iroh doc/replica event loop in its own task (on a dedicated runtime spawned
// once — the ACTOR AT THE EDGE); the async channel methods talk to it over
// `tokio::mpsc`. No `block_on` lives in the channel methods.
#[cfg(feature = "iroh")]
mod backend;

/// An iroh-docs-backed collaborative channel.
///
/// Implements [`AttributableChannel`]: each received blob is paired with the
/// [`SenderId`] derived from the `AuthorId` iroh-docs stamped on that record.
///
/// Construct one with [`IrohChannel::create`] (mint a fresh doc + a write
/// `DocTicket` to hand peers out of band) or [`IrohChannel::join`]
/// (join an existing doc from a ticket). Both require the `iroh` feature; built
/// without it they return the [`SETUP_WITHOUT_IROH`] error and the type is an
/// empty skeleton that still satisfies the trait bound.
pub struct IrohChannel {
    // With the feature on, the real node lives here. With it off, the struct is
    // a zero-field skeleton so the type — and its trait impls — still exist.
    #[cfg(feature = "iroh")]
    inner: backend::Node,
}

/// Re-export the upstream ticket type when the backend is present, so callers
/// can name it without depending on iroh-docs directly. Introduction / pairing
/// is out of scope: a `DocTicket` is an INPUT delivered out of band (e.g. by
/// wormhole), not something this crate mints an introduction protocol for.
#[cfg(feature = "iroh")]
pub use iroh_docs::DocTicket;

impl IrohChannel {
    /// Create a NEW collaboration document, returning both the ready channel and
    /// a write [`DocTicket`] to hand to peers out of band.
    ///
    /// Requires the `iroh` feature. Without it, returns [`SETUP_WITHOUT_IROH`].
    #[cfg(feature = "iroh")]
    pub fn create() -> Result<(Self, DocTicket)> {
        let (node, ticket) = backend::Node::create()?;
        Ok((Self { inner: node }, ticket))
    }

    /// Skeleton form when built WITHOUT `iroh`: no node can be created.
    #[cfg(not(feature = "iroh"))]
    pub fn create() -> Result<(Self, ())> {
        Err(Error::new(SETUP_WITHOUT_IROH))
    }

    /// Join the collaboration document described by `ticket`.
    ///
    /// `ticket` is the out-of-band join credential; introduction / pairing is
    /// out of scope (see the module docs). Requires the `iroh` feature; without
    /// it, returns [`SETUP_WITHOUT_IROH`].
    #[cfg(feature = "iroh")]
    pub fn join(ticket: DocTicket) -> Result<Self> {
        Ok(Self {
            inner: backend::Node::join(ticket)?,
        })
    }

    /// Skeleton form when built WITHOUT `iroh`: `ticket` is opaque bytes and no
    /// node can be joined.
    #[cfg(not(feature = "iroh"))]
    pub fn join(_ticket: Vec<u8>) -> Result<Self> {
        Err(Error::new(SETUP_WITHOUT_IROH))
    }
}

#[async_trait]
impl AttributableChannel for IrohChannel {
    /// Broadcast one opaque message: (re)assert our maximal tip under our
    /// per-author frontier key. `set_bytes` supersedes our prior record, so the
    /// doc keeps at most one tip per author. Awaits the backend actor over its
    /// request channel — no `block_on`.
    #[cfg(feature = "iroh")]
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        self.inner.send(message).await
    }

    /// Snapshot the whole frontier: for each participant's per-author key, take
    /// the single latest record, resolve its content hash to the stored bytes,
    /// and pair it with the `SenderId` derived from the record's `AuthorId`.
    /// Includes our own prior write (lattice-idempotent self-absorption).
    /// Awaits the backend actor over its request channel — no `block_on`.
    #[cfg(feature = "iroh")]
    async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
        self.inner.recv().await
    }

    // Skeleton (no `iroh` feature): the type still satisfies the trait so
    // downstream generic code type-checks, but there is no node to move bytes.
    #[cfg(not(feature = "iroh"))]
    async fn send(&mut self, _message: Vec<u8>) -> Result<()> {
        Err(Error::new(SETUP_WITHOUT_IROH))
    }

    #[cfg(not(feature = "iroh"))]
    async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
        Err(Error::new(SETUP_WITHOUT_IROH))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::{Attributed, Message, Transport, deframe, frame};

    // ---- Channel-trait-satisfaction test (no network) -------------------------
    //
    // Statically proves `IrohChannel` implements the attributable-channel
    // contract and, via the `Attributed` wrapper, the driver-facing `Transport`
    // seam that `commands::sync` drives. Pure type-level checks: no node is
    // constructed, so this runs identically with or without the `iroh` feature.
    #[test]
    fn iroh_channel_satisfies_the_channel_contract() {
        fn assert_attributable<C: AttributableChannel>() {}
        assert_attributable::<IrohChannel>();

        // `Attributed<IrohChannel>` bridges to the `Transport` seam (drops the
        // SenderId on collect); this is the exact object the sync driver holds.
        fn assert_transport<T: Transport>() {}
        assert_transport::<Attributed<IrohChannel>>();

        // The per-author frontier key convention is part of the wire contract.
        assert_eq!(FRONTIER_PREFIX, b"ptj/frontier/");

        // `&mut dyn Transport` is what the driver actually calls through.
        fn _drives(_t: &mut dyn Transport) {}
    }

    // Without the `iroh` feature, the skeleton constructors and channel methods
    // report the clear "built without iroh" error rather than panicking.
    #[cfg(not(feature = "iroh"))]
    #[test]
    fn skeleton_reports_built_without_iroh() {
        // `create()`'s Ok half is `(IrohChannel, ())`, which is not `Debug`, so
        // match the error out rather than `unwrap_err()`.
        let Err(create_err) = IrohChannel::create() else {
            panic!("skeleton create() must be an error");
        };
        assert_eq!(create_err.message(), SETUP_WITHOUT_IROH);

        let mut ch = IrohChannel {};
        futures::executor::block_on(async {
            assert_eq!(
                ch.send(vec![1, 2, 3]).await.unwrap_err().message(),
                SETUP_WITHOUT_IROH
            );
            assert_eq!(ch.recv().await.unwrap_err().message(), SETUP_WITHOUT_IROH);
        });
    }

    // ---- Framing roundtrip test (no network) ----------------------------------
    //
    // A doc-entry transport has native record boundaries and does not itself
    // frame, but this crate depends on transport-core's framing, so we verify
    // the round trip end-to-end here (Message envelope -> frame -> deframe ->
    // decode) with no iroh involvement. Runs in both feature modes.
    #[test]
    fn framing_roundtrip_over_transport_core() {
        // Type-tag a PSBT record, delimit it on a notional stream, read it back.
        let payload = vec![0xABu8; 137];
        let envelope = Message::Psbt(payload.clone()).encode();
        let framed = frame(&envelope);

        let mut buf = framed.clone();
        let record = deframe(&mut buf).unwrap().expect("one complete record");
        assert!(buf.is_empty(), "no trailing bytes after one record");
        assert_eq!(Message::decode(&record).unwrap(), Message::Psbt(payload));

        // A partial buffer yields Ok(None) and is left untouched for a re-read.
        let mut partial = framed[..framed.len() - 1].to_vec();
        let before = partial.clone();
        assert!(deframe(&mut partial).unwrap().is_none());
        assert_eq!(
            partial, before,
            "incomplete deframe leaves the buffer intact"
        );
    }

    // Two framed records on one buffer deframe in order, exercising the record
    // boundaries a stream transport would rely on (iroh itself skips framing).
    #[test]
    fn framing_delimits_multiple_records() {
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
