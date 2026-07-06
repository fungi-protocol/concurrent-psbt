//! Base64 <-> PSBT (de)serialization, wasm-safe.
//!
//! Ports the pure parts of `crates/ptj/src/io.rs`: `parse_psbt_bytes`,
//! `parse_bip174_bytes`, `encode_psbt`. Drops everything filesystem-bound
//! (read/write/lock/stdin) — the PWA has no filesystem. Error strings match the
//! webgui's so the frontend surfaces identical messages.
//!
//! NOTE on `catch_unwind`: ptj's `parse_psbt_bytes` wraps `Psbt::deserialize`
//! in `std::panic::catch_unwind` to turn a malformed-input panic into an error.
//! On `wasm32-unknown-unknown` panics abort by default (no unwinding), so
//! `catch_unwind` cannot recover them. We therefore rely on
//! `Psbt::deserialize` returning `Err` for malformed input (the normal path);
//! any residual panic is surfaced to the browser console via the optional
//! `console_error_panic_hook`. This is the one behavioral delta from the
//! webgui and is documented in README.md.

use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
use psbt_v2::v0::bitcoin as bip174;
use psbt_v2::v2::Psbt;

/// Decode a base64 string to raw PSBT bytes. Accepts a leading binary `psbt`
/// magic (returned untouched) exactly like ptj's `psbt_bytes`.
fn psbt_bytes(label: &str, raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.starts_with(b"psbt") {
        return Ok(raw.to_vec());
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| format!("{label} is neither binary PSBT nor valid UTF-8"))?;
    BASE64_STANDARD
        .decode(text.trim())
        .map_err(|error| format!("decoding base64 {label}: {error}"))
}

/// Parse a base64/binary BIP-370 (v2) PSBT. Rejects BIP-174 input with the same
/// guidance message ptj uses.
pub fn parse_psbt(label: &str, raw: &[u8]) -> Result<Psbt, String> {
    let bytes = psbt_bytes(label, raw)?;
    if bip174::Psbt::deserialize(&bytes).is_ok() {
        return Err(format!(
            "{label} is a BIP 174 PSBT; run import-bip174 before using BIP 370 operations"
        ));
    }
    Psbt::deserialize(&bytes).map_err(|error| format!("parsing {label}: {error}"))
}

/// Parse a base64/binary BIP-174 (v0) PSBT.
pub fn parse_bip174(label: &str, raw: &[u8]) -> Result<bip174::Psbt, String> {
    let bytes = psbt_bytes(label, raw)?;
    bip174::Psbt::deserialize(&bytes).map_err(|error| format!("parsing BIP 174 {label}: {error}"))
}

/// Serialize a BIP-370 PSBT to base64 (standard alphabet), matching ptj.
pub fn encode_psbt(psbt: &Psbt) -> String {
    BASE64_STANDARD.encode(Psbt::serialize(psbt))
}

/// Convenience: parse from a base64 `&str` request field with a stable label.
pub fn parse_psbt_str(label: &str, psbt: &str) -> Result<Psbt, String> {
    parse_psbt(label, psbt.as_bytes())
}
