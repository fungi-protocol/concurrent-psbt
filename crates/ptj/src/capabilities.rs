//! The capability catalog: a versioned, typed description of every transport
//! kind this ptj RECOGNIZES, and whether this particular build can USE it.
//!
//! One value, emitted identically by every shell (`ptj capabilities` on the
//! CLI, `GET /api/capabilities` over HTTP) so UIs stop inferring the
//! transport surface from scattered feature booleans. Two principles from the
//! capability-catalog-v1 spec are load-bearing:
//!
//! - **Recognized ≠ usable.** Every kind the endpoint classifier can parse is
//!   in the catalog, including kinds this build cannot drive (typed reason)
//!   and kinds no build drives yet (`unauthored` — the nostr entry). Parsing
//!   a valid endpoint must not imply the current shell can use it.
//! - **Plugins are host configuration.** The `plugin` kind spawns a local
//!   executable; a browser must never name an executable path, so its entry
//!   is marked `host-configuration` rather than browser-selectable.

/// The catalog schema revision. Consumers must check this before reading the
/// rest of the value; unknown versions degrade to "availability unknown".
pub const CATALOG_VERSION: u32 = 1;

/// What `collect` yields on this transport — transport-core's two channel
/// traits, plus the two shapes that live outside that seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelSemantics {
    /// File/dir state shared on the local machine; no network channel.
    Local,
    /// Bare opaque bytes; the transport supplies no sender identity.
    Anonymous,
    /// (sender, message) pairs; the transport attributes each message.
    Attributable,
    /// Decided per plugin at handshake time (anonymous or attributable).
    Negotiated,
}

impl ChannelSemantics {
    fn as_str(self) -> &'static str {
        match self {
            ChannelSemantics::Local => "local",
            ChannelSemantics::Anonymous => "anonymous",
            ChannelSemantics::Attributable => "attributable",
            ChannelSemantics::Negotiated => "negotiated",
        }
    }
}

/// What a user must exchange out-of-band before two peers can converge over
/// this transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pairing {
    /// Nothing: peers share the machine (local file/dir state).
    None,
    /// A single opaque ticket minted by one side (iroh docs ticket).
    Ticket,
    /// A peer address exchanged over any side channel (.onion, nym, i2p).
    Address,
    /// A manual offer/answer signal exchange (WebRTC SDP round-trip).
    ManualSignal,
    /// Group membership established by the messaging layer (MLS group).
    Group,
    /// A shared mailbox/session identifier (BIP-77 payjoin directory).
    Mailbox,
    /// Host-side configuration names the plugin binary; never browser input.
    HostConfiguration,
}

impl Pairing {
    fn as_str(self) -> &'static str {
        match self {
            Pairing::None => "none",
            Pairing::Ticket => "ticket",
            Pairing::Address => "address",
            Pairing::ManualSignal => "manual-signal",
            Pairing::Group => "group",
            Pairing::Mailbox => "mailbox",
            Pairing::HostConfiguration => "host-configuration",
        }
    }
}

/// Why a recognized kind cannot be used, as a typed code — UIs branch on the
/// code and render their own copy; the CLI feature name rides along so the
/// rebuild hint stays mechanical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// The provider crate exists but this binary was built without its
    /// cargo feature; rebuilding with the named feature lights it up.
    FeatureDisabled { feature: &'static str },
    /// No provider crate exists yet anywhere — the kind is recognized by
    /// the endpoint classifier only.
    Unauthored,
}

/// One recognized transport kind: identity, semantics, and this build's
/// ability to drive it.
#[derive(Debug, Clone, Copy)]
pub struct TransportCapability {
    /// The CLI value name (`ptj sync --transport <kind>`) and the browser
    /// select value — one vocabulary across shells.
    pub kind: &'static str,
    /// The crate that implements it; `None` for the built-in local state.
    pub provider: Option<&'static str>,
    pub channel: ChannelSemantics,
    pub pairing: Pairing,
    /// `Some(reason)` when this build cannot drive the kind.
    pub unavailable: Option<Unavailable>,
}

impl TransportCapability {
    /// Usable by THIS build (regardless of who may select it).
    pub fn available(&self) -> bool {
        self.unavailable.is_none()
    }

    /// Selectable from a browser. Plugins are host configuration: the
    /// executable path comes from the host's own CLI/config, never from
    /// browser input, so the plugin kind is available-but-not-selectable.
    pub fn browser_selectable(&self) -> bool {
        self.available() && self.pairing != Pairing::HostConfiguration
    }
}

/// The whole catalog for this build. `cfg!`-driven, so it is a compile-time
/// constant fact about the binary, not runtime state.
pub fn catalog() -> Vec<TransportCapability> {
    fn gate(enabled: bool, feature: &'static str) -> Option<Unavailable> {
        if enabled {
            None
        } else {
            Some(Unavailable::FeatureDisabled { feature })
        }
    }
    vec![
        TransportCapability {
            kind: "local",
            provider: None,
            channel: ChannelSemantics::Local,
            pairing: Pairing::None,
            unavailable: None,
        },
        TransportCapability {
            kind: "iroh",
            provider: Some("transport-iroh"),
            channel: ChannelSemantics::Attributable,
            pairing: Pairing::Ticket,
            unavailable: gate(cfg!(feature = "iroh-sync"), "iroh-sync"),
        },
        TransportCapability {
            kind: "arti",
            provider: Some("transport-arti"),
            channel: ChannelSemantics::Anonymous,
            pairing: Pairing::Address,
            unavailable: gate(cfg!(feature = "arti"), "arti"),
        },
        TransportCapability {
            kind: "nym",
            provider: Some("transport-nym"),
            channel: ChannelSemantics::Anonymous,
            pairing: Pairing::Address,
            unavailable: gate(cfg!(feature = "nym"), "nym"),
        },
        TransportCapability {
            kind: "emissary",
            provider: Some("transport-emissary"),
            channel: ChannelSemantics::Anonymous,
            pairing: Pairing::Address,
            unavailable: gate(cfg!(feature = "emissary"), "emissary"),
        },
        TransportCapability {
            kind: "mdk",
            provider: Some("transport-mdk"),
            channel: ChannelSemantics::Attributable,
            pairing: Pairing::Group,
            unavailable: gate(cfg!(feature = "mdk"), "mdk"),
        },
        TransportCapability {
            kind: "str0m",
            provider: Some("transport-str0m"),
            channel: ChannelSemantics::Anonymous,
            pairing: Pairing::ManualSignal,
            unavailable: gate(cfg!(feature = "str0m"), "str0m"),
        },
        TransportCapability {
            kind: "webrtc-rs",
            provider: Some("transport-webrtc-rs"),
            channel: ChannelSemantics::Anonymous,
            pairing: Pairing::ManualSignal,
            unavailable: gate(cfg!(feature = "webrtc-rs"), "webrtc-rs"),
        },
        TransportCapability {
            kind: "payjoin-dir",
            provider: Some("transport-payjoin-dir"),
            channel: ChannelSemantics::Anonymous,
            pairing: Pairing::Mailbox,
            unavailable: gate(cfg!(feature = "payjoin-dir"), "payjoin-dir"),
        },
        TransportCapability {
            kind: "plugin",
            provider: Some("transport-plugin-api"),
            channel: ChannelSemantics::Negotiated,
            pairing: Pairing::HostConfiguration,
            unavailable: gate(cfg!(feature = "plugin-transports"), "plugin-transports"),
        },
        // Recognized by the endpoint classifier (npub paste), driven by no
        // build yet: the spec's canonical recognized/inactive example.
        TransportCapability {
            kind: "nostr",
            provider: None,
            channel: ChannelSemantics::Attributable,
            pairing: Pairing::Address,
            unavailable: Some(Unavailable::Unauthored),
        },
    ]
}

/// The optional cargo features this binary was compiled with, by cargo name.
/// Deployment metadata alongside the catalog (which covers transports only).
pub fn enabled_features() -> Vec<&'static str> {
    const FEATURES: [(&str, bool); 11] = [
        ("webgui", cfg!(feature = "webgui")),
        ("tui", cfg!(feature = "tui")),
        ("iroh-sync", cfg!(feature = "iroh-sync")),
        ("arti", cfg!(feature = "arti")),
        ("nym", cfg!(feature = "nym")),
        ("emissary", cfg!(feature = "emissary")),
        ("mdk", cfg!(feature = "mdk")),
        ("str0m", cfg!(feature = "str0m")),
        ("webrtc-rs", cfg!(feature = "webrtc-rs")),
        ("payjoin-dir", cfg!(feature = "payjoin-dir")),
        ("plugin-transports", cfg!(feature = "plugin-transports")),
    ];
    FEATURES
        .iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(name, _)| *name)
        .collect()
}

/// The runtime refusal for a transport this build cannot drive, assembled
/// from the catalog so builder errors, route errors, and catalog reasons
/// never drift. Callers are the feature-off builder arms, so the kind is
/// always catalog-listed and feature-gated; the fallback arm keeps the
/// function total anyway.
pub fn rebuild_hint(kind: &str) -> String {
    let feature = catalog().iter().find(|c| c.kind == kind).and_then(|c| {
        match c.unavailable {
            Some(Unavailable::FeatureDisabled { feature }) => Some(feature),
            _ => None,
        }
    });
    match feature {
        Some(feature) => {
            format!("ptj was built without {kind} sync support; rebuild with --features {feature}")
        }
        None => format!("ptj was built without {kind} sync support"),
    }
}

/// The single wire shape every shell emits. Field names are the contract;
/// see the session UI's `loadCapabilities` and the webgui route test.
pub fn catalog_json() -> serde_json::Value {
    let transports: Vec<serde_json::Value> = catalog()
        .iter()
        .map(|capability| {
            let mut entry = serde_json::json!({
                "kind": capability.kind,
                "provider": capability.provider,
                "channel": capability.channel.as_str(),
                "pairing": capability.pairing.as_str(),
                "available": capability.available(),
                "browserSelectable": capability.browser_selectable(),
            });
            match capability.unavailable {
                None => {}
                Some(Unavailable::FeatureDisabled { feature }) => {
                    entry["reason"] = serde_json::json!({
                        "code": "feature-disabled",
                        "feature": feature,
                    });
                }
                Some(Unavailable::Unauthored) => {
                    entry["reason"] = serde_json::json!({ "code": "unauthored" });
                }
            }
            entry
        })
        .collect();
    serde_json::json!({
        "version": CATALOG_VERSION,
        "transports": transports,
        "features": enabled_features(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog names every kind exactly once, regardless of which
    /// features this test build has — recognition is feature-independent;
    /// only availability varies.
    #[test]
    fn every_kind_recognized_exactly_once() {
        let mut kinds: Vec<&str> = catalog().iter().map(|c| c.kind).collect();
        kinds.sort_unstable();
        let mut deduped = kinds.clone();
        deduped.dedup();
        assert_eq!(kinds, deduped, "duplicate kind in the catalog");
        for kind in [
            "local", "iroh", "arti", "nym", "emissary", "mdk", "str0m", "webrtc-rs",
            "payjoin-dir", "plugin", "nostr",
        ] {
            assert!(kinds.contains(&kind), "missing kind {kind}");
        }
    }

    /// Availability tracks this build's cfg! exactly — the feature-on and
    /// feature-off builds emit the same catalog modulo the reason entries.
    #[test]
    fn availability_matches_compile_time_features() {
        for capability in catalog() {
            let expected = match capability.kind {
                "local" => true,
                "iroh" => cfg!(feature = "iroh-sync"),
                "arti" => cfg!(feature = "arti"),
                "nym" => cfg!(feature = "nym"),
                "emissary" => cfg!(feature = "emissary"),
                "mdk" => cfg!(feature = "mdk"),
                "str0m" => cfg!(feature = "str0m"),
                "webrtc-rs" => cfg!(feature = "webrtc-rs"),
                "payjoin-dir" => cfg!(feature = "payjoin-dir"),
                "plugin" => cfg!(feature = "plugin-transports"),
                "nostr" => false,
                other => panic!("unmapped kind {other}"),
            };
            assert_eq!(
                capability.available(),
                expected,
                "availability mismatch for {}",
                capability.kind
            );
        }
    }

    /// A disabled kind's reason carries the exact cargo feature name, so the
    /// rebuild hint (`--features <name>`) can be assembled mechanically.
    #[test]
    fn feature_disabled_reasons_name_real_features() {
        for capability in catalog() {
            if let Some(Unavailable::FeatureDisabled { feature }) = capability.unavailable {
                assert!(
                    !enabled_features().contains(&feature),
                    "{}: reason names feature {feature} which IS enabled",
                    capability.kind
                );
            }
        }
    }

    /// nostr is the recognized-but-unauthored fixture: never available, and
    /// its reason is `unauthored`, not a feature hint (there is no feature).
    #[test]
    fn nostr_is_recognized_but_unauthored() {
        let nostr = catalog()
            .into_iter()
            .find(|c| c.kind == "nostr")
            .expect("nostr entry");
        assert!(!nostr.available());
        assert_eq!(nostr.unavailable, Some(Unavailable::Unauthored));
    }

    /// The plugin kind is host configuration: even when the feature is on
    /// (available), it must never be browser-selectable.
    #[test]
    fn plugin_is_never_browser_selectable() {
        let plugin = catalog()
            .into_iter()
            .find(|c| c.kind == "plugin")
            .expect("plugin entry");
        assert!(!plugin.browser_selectable());
    }

    /// The wire value: versioned, and each entry either is available or
    /// carries a typed reason with a code — never both, never neither.
    #[test]
    fn wire_shape_versioned_and_reasons_typed() {
        let value = catalog_json();
        assert_eq!(value["version"], serde_json::json!(CATALOG_VERSION));
        let transports = value["transports"].as_array().expect("transports array");
        assert_eq!(transports.len(), catalog().len());
        for entry in transports {
            let available = entry["available"].as_bool().expect("available bool");
            let reason = entry.get("reason");
            assert_eq!(
                available,
                reason.is_none(),
                "{}: available and reason must be mutually exclusive",
                entry["kind"]
            );
            if let Some(reason) = reason {
                let code = reason["code"].as_str().expect("reason code");
                assert!(
                    code == "feature-disabled" || code == "unauthored",
                    "unknown reason code {code}"
                );
                assert_eq!(
                    code == "feature-disabled",
                    reason.get("feature").is_some(),
                    "feature field rides exactly with feature-disabled"
                );
            }
        }
    }
}
