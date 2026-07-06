//! Transport shim: ptj now consumes the standalone transport crates.
//!
//! The `Transport` seam and the `Message` TLV envelope live in `transport-core`
//! (`crates/transport-core`); each network backend lives in its own
//! `transport-<name>` crate behind a cargo feature. This module re-exports those
//! types so the existing ptj call sites — `use crate::transport::{LocalTransport,
//! Transport};` and `use crate::transport::message::Message;` — keep resolving.
//!
//! Convergence (the lattice join in `commands/join.rs`) is entirely outside the
//! transport. A transport only gathers (`collect`) and broadcasts (`publish`)
//! opaque bytes; it never parses, orders, or deduplicates them. The join is
//! idempotent/commutative/associative, so ordering and duplicates cost nothing.

pub(crate) mod local;

pub(crate) use local::LocalTransport;
// `Transport` is re-exported here so `use crate::transport::{LocalTransport,
// Transport};` call sites keep resolving; `Message` is served by the `message`
// submodule shim below (the only path any call site imports it from).
pub(crate) use transport_core::Transport;

/// Path shim so `use crate::transport::message::Message;` keeps resolving after
/// the envelope moved into `transport-core`.
pub(crate) mod message {
    pub(crate) use transport_core::Message;
}
