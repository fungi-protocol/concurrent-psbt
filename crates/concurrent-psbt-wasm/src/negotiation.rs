//! Negotiation band helpers (pay / confirm / payments), mechanism-only.
//!
//! The wasm core stores/reads OPAQUE payment and confirmation records; the
//! frontend builds the record bytes (matching `concurrent_psbt::payments::negotiation::
//! Payment::encode` / `Confirmation::encode`) and the core never invents
//! policy. This keeps the browser core to the transport-agnostic mechanism.
//!
//! Encryption mirrors `ptj pay --encrypt` / `ptj confirm --encrypt`
//! (crates/ptj/src/commands/negotiation.rs): deterministic ChaCha20-Poly1305
//! with a group key = tagged-hash(secret) and a per-element nonce =
//! tagged-hash(subtype || id). Deterministic so a re-emitted identical record
//! produces byte-identical field values and the lattice join deduplicates
//! instead of manufacturing a conflict.
//!
//! RECOMMENDATION (real repo): the encrypt/decrypt/id-derivation helpers are
//! duplicated from ptj. Promote them (with `Payment`/`Confirmation`) into
//! `concurrent-psbt` so ptj and this crate share one implementation. Note that
//! chacha20poly1305 + bitcoin::hashes are wasm-clean (pure Rust), so this
//! compiles to wasm32 unchanged.

use bitcoin::hashes::{Hash as _, HashEngine as _, sha256t_hash_newtype};
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use concurrent_psbt::payments::negotiation::{
    FORMAT_ENCRYPTED, GlobalNegotiationExt as _, PSBT_GLOBAL_CONFIRMATION_SUBTYPE,
    PSBT_GLOBAL_PAYMENT_SUBTYPE,
};
use psbt_v2::v2::Psbt;

sha256t_hash_newtype! {
    pub struct NegotiationKeyTag = hash_str("concurrent-psbt/negotiation-key");
    #[hash_newtype(forward)]
    pub struct NegotiationKeyHash(_);

    pub struct NegotiationNonceTag = hash_str("concurrent-psbt/negotiation-nonce");
    #[hash_newtype(forward)]
    pub struct NegotiationNonceHash(_);
}

fn derive_key(secret: &[u8]) -> Key {
    let mut engine = NegotiationKeyHash::engine();
    engine.input(secret);
    Key::clone_from_slice(&NegotiationKeyHash::from_engine(engine).to_byte_array())
}

fn derive_nonce(subtype: u8, id: &[u8; 16]) -> Nonce {
    let mut engine = NegotiationNonceHash::engine();
    engine.input(&[subtype]);
    engine.input(id);
    let bytes = NegotiationNonceHash::from_engine(engine).to_byte_array();
    *Nonce::from_slice(&bytes[..12])
}

fn aad(subtype: u8, id: &[u8; 16]) -> Vec<u8> {
    let mut a = concurrent_psbt::PROPRIETARY_PREFIX.to_vec();
    a.push(subtype);
    a.extend_from_slice(id);
    a
}

fn encrypt(secret: &[u8], subtype: u8, id: &[u8; 16], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = ChaCha20Poly1305::new(&derive_key(secret));
    let ct = cipher
        .encrypt(&derive_nonce(subtype, id), Payload { msg: plaintext, aad: &aad(subtype, id) })
        .map_err(|_| "negotiation record encryption failed".to_string())?;
    let mut out = Vec::with_capacity(1 + ct.len());
    out.push(FORMAT_ENCRYPTED);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn decrypt(
    secret: &[u8],
    subtype: u8,
    id: &[u8; 16],
    blob: &[u8],
) -> Result<Option<Vec<u8>>, String> {
    match blob.first() {
        Some(&FORMAT_ENCRYPTED) => {
            let cipher = ChaCha20Poly1305::new(&derive_key(secret));
            let pt = cipher
                .decrypt(&derive_nonce(subtype, id), Payload { msg: &blob[1..], aad: &aad(subtype, id) })
                .map_err(|_| {
                    "negotiation record failed to decrypt (wrong secret or tampered)".to_string()
                })?;
            Ok(Some(pt))
        }
        _ => Ok(None),
    }
}

/// Random 16-byte element id (browser crypto.getRandomValues via getrandom).
fn random_id() -> [u8; 16] {
    rand::random::<[u8; 16]>()
}

/// Append an opaque payment record. When `secret` is present the record is
/// encrypted; otherwise it is stored in the clear (== `ptj pay` w/o --encrypt).
pub fn add_payment(psbt: &mut Psbt, record: &[u8], secret: Option<&[u8]>) -> Result<(), String> {
    let id = random_id();
    let blob = match secret {
        Some(secret) => encrypt(secret, PSBT_GLOBAL_PAYMENT_SUBTYPE, &id, record)?,
        None => record.to_vec(),
    };
    psbt.global.add_payment(id, blob);
    Ok(())
}

/// Append an opaque confirmation record.
pub fn add_confirmation(
    psbt: &mut Psbt,
    record: &[u8],
    secret: Option<&[u8]>,
) -> Result<(), String> {
    let id = random_id();
    let blob = match secret {
        Some(secret) => encrypt(secret, PSBT_GLOBAL_CONFIRMATION_SUBTYPE, &id, record)?,
        None => record.to_vec(),
    };
    psbt.global.add_confirmation(id, blob);
    Ok(())
}

/// A randomly-generated dummy payment record (opaque bytes), only meaningful
/// when subsequently encrypted (see PayRequest.dummy guard in ops.rs). Mirrors
/// the dummy shape ptj builds: a 22-byte v0 witness program script + random
/// payer/amount, encoded via the same `Payment::encode`.
pub fn random_dummy_payment() -> Vec<u8> {
    use concurrent_psbt::payments::negotiation::{PAYMENT_KIND_DUMMY, Payment};
    let mut spk = vec![0u8; 22];
    spk[0] = 0x00;
    spk[1] = 0x14;
    rand::fill(&mut spk[2..]);
    Payment {
        kind: PAYMENT_KIND_DUMMY,
        payer: rand::random(),
        amount_sats: u64::from(rand::random::<u32>()),
        script_pubkey: spk,
        label: String::new(),
    }
    .encode()
}

/// Decode the negotiation band to opaque hex blobs. Encrypted entries are
/// decrypted when `secret` is supplied; undecryptable/opaque entries are
/// returned as their stored bytes (hex) so the frontend can decide.
#[allow(clippy::type_complexity)]
pub fn decode_band(
    psbt: &Psbt,
    secret: Option<&[u8]>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let payments = psbt
        .global
        .payments()
        .into_iter()
        .map(|(id, blob)| decode_entry(PSBT_GLOBAL_PAYMENT_SUBTYPE, &id, &blob, secret))
        .collect::<Result<Vec<_>, _>>()?;
    let confirmations = psbt
        .global
        .confirmations()
        .into_iter()
        .map(|(id, blob)| decode_entry(PSBT_GLOBAL_CONFIRMATION_SUBTYPE, &id, &blob, secret))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((payments, confirmations))
}

fn decode_entry(
    subtype: u8,
    id: &[u8; 16],
    blob: &[u8],
    secret: Option<&[u8]>,
) -> Result<String, String> {
    let bytes = match (blob.first(), secret) {
        (Some(&FORMAT_ENCRYPTED), Some(secret)) => {
            decrypt(secret, subtype, id, blob)?.unwrap_or_else(|| blob.to_vec())
        }
        // Encrypted but no secret: return the stored ciphertext blob as-is.
        _ => blob.to_vec(),
    };
    Ok(hex_encode(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
