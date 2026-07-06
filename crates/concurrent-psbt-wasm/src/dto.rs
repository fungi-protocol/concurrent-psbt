//! Request/response DTOs, field-for-field identical to the ptj webgui JSON.
//!
//! These mirror the request bodies parsed in `crates/ptj/src/webgui.rs` and the
//! DTOs declared in `contrib/demo-gui/src/backend.ts`. Keeping the field names
//! (snake_case) and shapes byte-identical is what lets the shared frontend
//! backend module target either the HTTP `fetch` backend or this WASM backend.
//!
//! Responses are emitted as `serde_json::Value` in `ops.rs` (so the inspect
//! JSON produced by `inspect_psbt` drops straight in); these structs only cover
//! the request side plus the small typed response bodies where a struct is
//! clearer than an ad-hoc `json!`.

use serde::Deserialize;

/// `POST /api/create` body. Mirrors webgui `create_config_from_request` +
/// backend.ts `CreatePsbtRequest` (which maps camelCase -> snake_case before
/// POST). `network`/`ordering` default exactly as the webgui does when absent.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateRequest {
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub ordering: Option<String>,
    #[serde(default)]
    pub seed_hex: Option<String>,
    #[serde(default)]
    pub inputs: Vec<CreateInput>,
    #[serde(default)]
    pub outputs: Vec<CreateOutput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateInput {
    pub txid: String,
    pub vout: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateOutput {
    pub address: String,
    pub amount_btc: String,
}

// NOTE: sort takes positional args (psbt, seed_hex?) matching the canonical
// Backend arity — no SortRequest DTO.

/// Negotiation `pay` body. `payment_hex` is the opaque record bytes (hex); the
/// frontend encodes the address/amount/label record the same way `ptj pay`
/// would build it, keeping the wasm core mechanism-only. `secret_hex` opt-in
/// enables deterministic AEAD encryption (see concurrent-psbt::negotiation).
#[derive(Debug, Clone, Deserialize)]
pub struct PayRequest {
    pub psbt: String,
    pub payment_hex: String,
    #[serde(default)]
    pub secret_hex: Option<String>,
    #[serde(default)]
    pub dummy: u32,
}

/// Negotiation `confirm` body.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfirmRequest {
    pub psbt: String,
    pub confirmation_hex: String,
    #[serde(default)]
    pub secret_hex: Option<String>,
}

/// Negotiation `payments` (decode) body.
#[derive(Debug, Clone, Deserialize)]
pub struct PaymentsRequest {
    pub psbt: String,
    #[serde(default)]
    pub secret_hex: Option<String>,
}
