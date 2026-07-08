//! `concurrent-psbt-wasm` — the PWA no-backend core.
//!
//! Exposes concurrent-psbt's PSBT operations to JavaScript via
//! `#[wasm_bindgen]`. Each exported function mirrors ONE ptj webgui `/api/*`
//! route so the shared frontend backend module (contrib/demo-gui/src/
//! backend.ts) can dispatch to either an HTTP `fetch` (webgui / tauri) or this
//! WASM module (PWA) behind the same operation names + DTOs.
//!
//! ## The seam
//!
//! ptj's webgui exposes each op as `fn <op>_response_result(body: &[u8]) ->
//! Result<Vec<u8>>` that (1) parses a JSON request, (2) parses base64 PSBT(s),
//! (3) runs a `crate::commands::*` op on concurrent-psbt, (4) re-serializes to
//! `{ "psbt": <base64>, "inspect": <json> }` (or `{ "error": <string> }`).
//! Those command/io/DTO helpers are `pub(crate)` in ptj and pull in ptj's
//! CLI/file-IO types, so they cannot be linked from here. Instead this crate
//! PORTS the thin operation wrappers (they only need concurrent-psbt +
//! psbt-v2 + bitcoin), keeping the request/response JSON shape byte-identical
//! to the webgui so the frontend cannot tell the two backends apart.
//!
//! ## Data contract (identical to webgui)
//!
//! - PSBTs cross the boundary as base64 strings (BIP-370 v2 wire form), same as
//!   the HTTP body. `import-bip174` takes base64 BIP-174; `export-bip174`
//!   returns base64 BIP-174.
//! - Structured requests/responses are JSON with the SAME field names the
//!   webgui uses (snake_case: `seed_hex`, `amount_btc`, `iroh_ticket`, ...).
//! - Errors surface as a thrown `JsError` whose message equals the webgui's
//!   `{ "error": <string> }` text, so `PtjBackendError`-style handling in the
//!   frontend still works after the adapter maps a throw to `{status, message}`.
//!
//! ## Randomness
//!
//! concurrent-psbt's only runtime randomness is the 16-byte `UniqueId`
//! (`rand::random` in src/psbt/output.rs). On wasm the getrandom `wasm_js`
//! backend (see Cargo.toml) routes that to the browser `crypto.getRandomValues`.

#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

mod bytes_arg;
mod dto;
mod negotiation;
mod ops;
mod psbt_io;

/// Optional: install a panic hook that logs Rust panics to the browser console.
///
/// The PWA shell should call this once at startup in debug builds. No-op unless
/// the crate is built with the `debug-panic-hook` feature.
#[wasm_bindgen(js_name = initPanicHook)]
pub fn init_panic_hook() {
    #[cfg(feature = "debug-panic-hook")]
    console_error_panic_hook::set_once();
}

/// Semantic version of the wasm core, so the frontend can assert compatibility
/// with the bundle it loaded (mirrors nothing in webgui; PWA-only affordance).
#[wasm_bindgen(js_name = version)]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------------------------------------------------------------------------
// Operation surface. One export per ptj webgui `/api/*` route.
//
// Convention:
//   * js_name is camelCase for every export (the JS-facing naming rule; see
//     shared-frontend/core/backend.ts). JSON *field* names stay snake_case —
//     they are the ptj webgui wire contract.
//   * `psbt` / `psbts` params are base64 strings (as in the HTTP body).
//   * structured params come in as a single `JsValue` request object and are
//     deserialized with serde-wasm-bindgen (no JSON string hop).
//   * the return is a `JsValue` response object matching the webgui JSON.
//   * every fallible op returns `Result<JsValue, JsError>`; a JsError is thrown
//     to JS with the same message the webgui would put in `{ "error": ... }`.
//
// The `_` arg names on `JsValue` requests are typed loosely on purpose: the
// adapter (backend.wasm.ts) is the single typed caller and it constructs these
// objects to match the webgui request bodies exactly.
// ---------------------------------------------------------------------------

/// `POST /api/inspect` — returns the inspection JSON for a base64 PSBT.
///
/// Response: the raw inspect object (NOT wrapped in `{psbt, inspect}`), same as
/// the webgui, whose `/api/inspect` returns `inspect_psbt(&psbt)` directly.
#[wasm_bindgen(js_name = inspect)]
pub fn inspect(psbt: String) -> Result<JsValue, JsError> {
    finish(ops::inspect(&psbt))
}

/// `POST /api/create` — build a PSBT from a `CreatePsbtRequest`-shaped object.
///
/// Request object mirrors the webgui body: `{ network, ordering?, seed_hex?,
/// inputs: [{txid, vout}], outputs: [{address, amount_btc}] }`.
/// Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = create)]
pub fn create(request: JsValue) -> Result<JsValue, JsError> {
    let req: dto::CreateRequest = serde_wasm_bindgen::from_value(request)
        .map_err(|e| JsError::new(&format!("parsing create request: {e}")))?;
    finish(ops::create(req))
}

/// `POST /api/join` — lattice-fold a non-empty array of base64 PSBTs.
/// Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = join)]
pub fn join(psbts: JsValue) -> Result<JsValue, JsError> {
    let psbts: Vec<String> = serde_wasm_bindgen::from_value(psbts)
        .map_err(|e| JsError::new(&format!("parsing join psbts: {e}")))?;
    finish(ops::join(psbts))
}

/// `POST /api/sort` — order an unordered PSBT; optional seed.
///
/// Canonical Backend arity: `sort(psbt, seedHex?)` — two positional args, NOT a
/// request object (matches `Backend.sortPsbt(psbt, seedHex?, allowShortSeed?)`
/// in the shared frontend). Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = sort)]
pub fn sort(
    psbt: String,
    seed_hex: Option<String>,
    allow_short_seed: Option<bool>,
) -> Result<JsValue, JsError> {
    finish(ops::sort(
        &psbt,
        seed_hex.as_deref(),
        allow_short_seed.unwrap_or(false),
    ))
}

/// `POST /api/make-unordered` — clear ordering, returning an unordered PSBT.
/// Request: `{ psbt }`. Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = makeUnordered)]
pub fn make_unordered(psbt: String) -> Result<JsValue, JsError> {
    finish(ops::make_unordered(&psbt))
}

/// `POST /api/atomize` — split a multi-element PSBT into single-element atoms.
/// Request: `{ psbt }`. Response: `{ fragments: [{psbt, inspect}, ...] }`.
#[wasm_bindgen(js_name = atomize)]
pub fn atomize(psbt: String) -> Result<JsValue, JsError> {
    finish(ops::atomize(&psbt))
}

/// `POST /api/concatenate` — append ordered PSBTs sharing a global context.
/// Request: `{ psbts }`. Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = concatenate)]
pub fn concatenate(psbts: JsValue) -> Result<JsValue, JsError> {
    let psbts: Vec<String> = serde_wasm_bindgen::from_value(psbts)
        .map_err(|e| JsError::new(&format!("parsing concatenate psbts: {e}")))?;
    finish(ops::concatenate(psbts))
}

/// `POST /api/export-bip174` — convert an ordered BIP-370 PSBT to base64 BIP-174.
/// Request: `{ psbt }`. Response: `{ format: "bip174", psbt }`.
#[wasm_bindgen(js_name = exportBip174)]
pub fn export_bip174(psbt: String) -> Result<JsValue, JsError> {
    finish(ops::export_bip174(&psbt))
}

/// `POST /api/import-bip174` — convert base64 BIP-174 to a BIP-370 PSBT.
/// Request: `{ psbt, modifiable? }` (positional here: `importBip174(psbt,
/// modifiable?)`). BIP 174 has no TX_MODIFIABLE field; passing `true` is the
/// caller's explicit assertion that inputs/outputs may still be added.
/// Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = importBip174)]
pub fn import_bip174(psbt: String, modifiable: Option<bool>) -> Result<JsValue, JsError> {
    finish(ops::import_bip174(&psbt, modifiable.unwrap_or(false)))
}

/// `POST /api/assign-ids` — assign spec identity fields (PSBT_OUT_UNIQUE_ID,
/// optional PSBT_IN_UNIQUE_ID) to entries that lack them. Request mirrors the
/// webgui body: `{ psbt, ids?: [{target, index, id}], auto?, overwrite? }`.
/// Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = assignIds)]
pub fn assign_ids(request: JsValue) -> Result<JsValue, JsError> {
    let req: dto::AssignIdsRequest = serde_wasm_bindgen::from_value(request)
        .map_err(|e| JsError::new(&format!("parsing assign-ids request: {e}")))?;
    finish(ops::assign_ids(req))
}

/// `POST /api/sync`, local branch — the Layer-2 lattice fold with NO network.
///
/// This is the LOCAL-FIRST sync core: the PWA works fully in-browser with no
/// server. `psbts` is a non-empty array of base64 PSBTs; the response is the
/// webgui local-branch `SyncResponse` shape `{ psbt, inspect, payments: [],
/// confirmations: [] }` (payments/confirmations are populated from transport
/// MESSAGES, and there is no transport here — the negotiation band is read via
/// the `payments` export instead). Networked transports (payjoin-dir/OHTTP,
/// webrtc, nostr) are explicit opt-in in the JS layer (BrowserTransport
/// injected into WasmBackend); they gather more PSBTs and call this same fold.
#[wasm_bindgen(js_name = localSync)]
pub fn local_sync(psbts: JsValue) -> Result<JsValue, JsError> {
    let psbts: Vec<String> = serde_wasm_bindgen::from_value(psbts)
        .map_err(|e| JsError::new(&format!("parsing sync psbts: {e}")))?;
    finish(ops::local_sync(psbts))
}

// ---------------------------------------------------------------------------
// Negotiation surface (ptj `pay` / `confirm` / `payments`). The webgui does NOT
// currently expose these as /api routes, but the task requires pay/confirm and
// the shared backend contract benefits from a superset. These reuse
// concurrent-psbt::payments::negotiation directly. Signatures match the CLI semantics:
// `pay` appends a payment record; `confirm` appends a confirmation; `payments`
// decodes both back. Encryption (`secret`) is optional; when absent, records
// are stored in the clear exactly like `ptj pay` with no `--secret`.
// ---------------------------------------------------------------------------

/// Append a payment record to a PSBT's grow-only negotiation set.
/// Request: `{ psbt, payment_hex, secret_hex?, dummy? }`.
/// Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = pay)]
pub fn pay(request: JsValue) -> Result<JsValue, JsError> {
    let req: dto::PayRequest = serde_wasm_bindgen::from_value(request)
        .map_err(|e| JsError::new(&format!("parsing pay request: {e}")))?;
    finish(ops::pay(req))
}

/// Append a confirmation record. Request: `{ psbt, confirmation_hex, secret_hex? }`.
/// Response: `{ psbt, inspect }`.
#[wasm_bindgen(js_name = confirm)]
pub fn confirm(request: JsValue) -> Result<JsValue, JsError> {
    let req: dto::ConfirmRequest = serde_wasm_bindgen::from_value(request)
        .map_err(|e| JsError::new(&format!("parsing confirm request: {e}")))?;
    finish(ops::confirm(req))
}

/// Decode the negotiation set. Request: `{ psbt, secret_hex? }`.
/// Response: `{ payments: [hex...], confirmations: [hex...] }`.
#[wasm_bindgen(js_name = payments)]
pub fn payments(request: JsValue) -> Result<JsValue, JsError> {
    let req: dto::PaymentsRequest = serde_wasm_bindgen::from_value(request)
        .map_err(|e| JsError::new(&format!("parsing payments request: {e}")))?;
    finish(ops::payments(req))
}

/// Convert an op's `Result<serde_json::Value, String>` into the `#[wasm_bindgen]`
/// boundary type `Result<JsValue, JsError>`:
///   * `Ok(value)`  -> a real JS object (serde-wasm-bindgen, no JSON string hop);
///   * `Err(msg)`   -> a thrown `JsError` whose message is the SAME text the
///     webgui would place in `{ "error": <string> }`, so the frontend adapter
///     can rebuild a `PtjBackendError`-equivalent from `err.message`.
fn finish(result: Result<serde_json::Value, String>) -> Result<JsValue, JsError> {
    let value = result.map_err(|message| JsError::new(&message))?;
    serde_wasm_bindgen::to_value(&value)
        .map_err(|e| JsError::new(&format!("serializing response: {e}")))
}
