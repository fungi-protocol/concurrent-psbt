//! transport-plugin-api — the Cap'n Proto wire contract between ptj (the
//! host) and out-of-process transport plugins.
//!
//! # Why plugins exist
//!
//! Some transport stacks cannot share one Cargo.lock. The concrete case:
//! transport-arti's `arti-client` (static-sqlite) and transport-nym's
//! `nym-sdk` pin incompatible `libsqlite3-sys` majors, and `links =
//! "sqlite3"` permits exactly one version per lockfile — optional
//! dependencies still resolve into the lock, so feature-gating does not
//! help. A plugin is a SEPARATE binary with its OWN lockfile: the host
//! spawns it and speaks Cap'n Proto twoparty RPC over the child's
//! stdin/stdout. See `contrib/design/transport-plugins.md` for the full
//! architecture (lifecycle, supervision, security posture).
//!
//! # What this crate is
//!
//! The schema (`schema/transport.capnp`) plus its generated Rust bindings —
//! nothing else. Both sides depend on it: the host (ptj's `plugin-transports`
//! feature) drives the generated *clients*; a plugin binary implements the
//! generated *servers*. The crate re-exports [`capnp`] and [`capnp_rpc`] so
//! host and plugins build against the single grounded version of the RPC
//! stack instead of each grounding their own.
//!
//! # The contract in one paragraph
//!
//! The plugin's vat exports a bootstrap [`transport_capnp::plugin`]
//! capability. The host calls `handshake` first (exact
//! [`PROTOCOL_VERSION`] match, channel-kind negotiation, opaque config
//! passthrough), then requests the transport capability matching the
//! negotiated kind, then drives `publish`/`collect` — the same opaque-bytes,
//! snapshot-per-collect contract as transport-core's channel traits.
//! `handshake` reports refusal through an explicit result union;
//! `publish`/`collect` failures travel as RPC exceptions.

pub use capnp;
pub use capnp_rpc;

/// The wire protocol revision spoken by this build of the schema.
///
/// Sent in both handshake directions; the host refuses to drive a plugin
/// whose answer does not match exactly. Bump on ANY schema change — the
/// scaffold has no compatibility window (a versioning policy is deliberately
/// deferred until there is a second version to be compatible with).
pub const PROTOCOL_VERSION: u32 = 1;

/// The generated bindings for `schema/transport.capnp` (capnpc output,
/// included from OUT_DIR). Generated code carries its own style; lint it as
/// such rather than to the workspace's hand-written standards.
#[allow(missing_docs, clippy::all, clippy::pedantic)]
pub mod transport_capnp {
    include!(concat!(env!("OUT_DIR"), "/transport_capnp.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_capnp::{handshake, handshake_result};

    /// Build a handshake through the generated writers and read every field
    /// back through the generated readers: the codegen pipeline (capnp tool
    /// -> capnpc -> include!) works and the schema means what we think.
    #[test]
    fn handshake_roundtrips_through_generated_bindings() {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut hello = message.init_root::<handshake::Builder<'_>>();
            hello.set_protocol_version(PROTOCOL_VERSION);
            hello.set_channel_kind(handshake::ChannelKind::Anonymous);
            let mut config = hello.init_config(1);
            let mut entry = config.reborrow().get(0);
            entry.set_key("nym-address");
            entry.set_value("peer.gateway");
        }

        let hello = message
            .get_root_as_reader::<handshake::Reader<'_>>()
            .expect("re-reading the message we just built");
        assert_eq!(hello.get_protocol_version(), PROTOCOL_VERSION);
        assert_eq!(
            hello.get_channel_kind().expect("known enumerant"),
            handshake::ChannelKind::Anonymous
        );
        let config = hello.get_config().expect("config list");
        assert_eq!(config.len(), 1);
        assert_eq!(
            config.get(0).get_key().expect("key").to_str().unwrap(),
            "nym-address"
        );
        assert_eq!(
            config.get(0).get_value().expect("value").to_str().unwrap(),
            "peer.gateway"
        );
    }

    /// The handshake result union distinguishes acceptance from structured
    /// refusal — the explicit error-result path of the contract.
    #[test]
    fn handshake_result_union_carries_err() {
        let mut message = capnp::message::Builder::new_default();
        {
            let result = message.init_root::<handshake_result::Builder<'_>>();
            let mut err = result.init_err();
            err.set_message("unsupported protocol version: want 1, got 2");
        }
        let result = message
            .get_root_as_reader::<handshake_result::Reader<'_>>()
            .expect("re-reading the message we just built");
        match result.which().expect("known union variant") {
            handshake_result::Err(err) => {
                let text = err
                    .expect("err reader")
                    .get_message()
                    .expect("message field")
                    .to_string()
                    .unwrap();
                assert!(text.contains("unsupported protocol version"));
            }
            handshake_result::Ok(_) => panic!("built err, read ok"),
        }
    }
}
