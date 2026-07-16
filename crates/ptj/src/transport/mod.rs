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
// A shared directory as a write-once content-addressed register: the
// filesystem spelling of replace-by-LUB (new files + atomic link/unlink,
// never an overwrite).
pub(crate) mod watched_dir;
// Out-of-process plugin host: spawn a plugin binary (its OWN lockfile — for
// transport stacks that cannot share this workspace's lock) and drive it over
// child stdio via Cap'n Proto RPC (wire contract: transport-plugin-api). The
// module is `pub` (re-exported from lib.rs) because the fake-plugin loopback
// integration test drives `PluginTransport` from outside the crate.
#[cfg(feature = "plugin-transports")]
pub mod plugin;
// Manual file-based SDP/ICE signaling for the WebRTC transports; only those
// features have a caller, so the module is gated with them (keeping the
// default build free of dead code).
#[cfg(any(feature = "str0m", feature = "webrtc-rs"))]
pub(crate) mod signaling;

pub(crate) use local::LocalTransport;
pub(crate) use watched_dir::WatchedDirTransport;
// `Transport` is re-exported here so `use crate::transport::{LocalTransport,
// Transport};` call sites keep resolving; `Message` is served by the `message`
// submodule shim below (the only path any call site imports it from).
pub(crate) use transport_core::Transport;

/// Path shim so `use crate::transport::message::Message;` keeps resolving after
/// the envelope moved into `transport-core`.
pub(crate) mod message {
    pub(crate) use transport_core::Message;
}
