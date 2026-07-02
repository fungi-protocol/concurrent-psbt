//! Payment negotiation and confirmation subcommands.
//!
//! `pay` and `confirm` append grow-only global proprietary entries
//! (`concurrent_psbt::payments::negotiation`); `payments` decodes them back. Each
//! element is optionally encrypted with a group key derived from an
//! out-of-band shared secret (`--secret`), and `pay` can pad the visible
//! payment set with indistinguishable dummy entries (`--dummy N`, encrypted
//! only). Encryption is deterministic (nonce derived from the element id) so
//! re-emitting an identical element produces byte-identical field values and
//! the lattice join deduplicates instead of manufacturing a conflict.

use bitcoin::hashes::{Hash, HashEngine, sha256t_hash_newtype};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use concurrent_psbt::payments::negotiation::{
    Confirmation, FORMAT_ENCRYPTED, GlobalNegotiationExt, PAYMENT_KIND_DUMMY, PAYMENT_KIND_REAL,
    PSBT_GLOBAL_CONFIRMATION_SUBTYPE, PSBT_GLOBAL_PAYMENT_SUBTYPE, Payment, unordered_unique_id,
};
use psbt_v2::v2::Psbt;

use crate::cli::{ConfirmConfig, PayConfig, PaymentsConfig};
use crate::{Error, Result, io};

sha256t_hash_newtype! {
    /// Tag for the negotiation group encryption key.
    pub struct NegotiationKeyTag = hash_str("concurrent-psbt/negotiation-key");
    /// Derived 32-byte AEAD key.
    #[hash_newtype(forward)]
    pub struct NegotiationKeyHash(_);

    /// Tag for the per-element deterministic nonce.
    pub struct NegotiationNonceTag = hash_str("concurrent-psbt/negotiation-nonce");
    /// Derived nonce material (first 12 bytes used).
    #[hash_newtype(forward)]
    pub struct NegotiationNonceHash(_);

    /// Tag for the keyed confirmation-id derivation.
    pub struct ConfirmationIdTag = hash_str("concurrent-psbt/confirmation-id");
    /// Derived confirmation id (first 16 bytes used).
    #[hash_newtype(forward)]
    pub struct ConfirmationIdHash(_);
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

/// Encrypt a plaintext record into `FORMAT_ENCRYPTED || ciphertext`.
///
/// `pub(crate)` because the fee band shares this machinery: fee.rs documents
/// that fee-contribution encryption "reuses the negotiation group-key
/// machinery in the `ptj` CLI", and the helpers are already parameterized by
/// subtype (`commands::fee` passes the 0x22 fee subtype).
pub(crate) fn encrypt(
    secret: &[u8],
    subtype: u8,
    id: &[u8; 16],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(&derive_key(secret));
    let ct = cipher
        .encrypt(
            &derive_nonce(subtype, id),
            Payload {
                msg: plaintext,
                aad: &aad(subtype, id),
            },
        )
        .map_err(|_| Error::new("negotiation record encryption failed"))?;
    let mut out = Vec::with_capacity(1 + ct.len());
    out.push(FORMAT_ENCRYPTED);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a `FORMAT_ENCRYPTED` blob back to the plaintext record, or `None`
/// if the blob is not encrypted (leading byte is a plaintext format).
/// `pub(crate)` for the same fee-band sharing as [`encrypt`].
pub(crate) fn decrypt(
    secret: &[u8],
    subtype: u8,
    id: &[u8; 16],
    blob: &[u8],
) -> Result<Option<Vec<u8>>> {
    match blob.first() {
        Some(&FORMAT_ENCRYPTED) => {
            let cipher = ChaCha20Poly1305::new(&derive_key(secret));
            let pt = cipher
                .decrypt(
                    &derive_nonce(subtype, id),
                    Payload {
                        msg: &blob[1..],
                        aad: &aad(subtype, id),
                    },
                )
                .map_err(|_| {
                    Error::new("negotiation record failed to decrypt (wrong secret or tampered)")
                })?;
            Ok(Some(pt))
        }
        _ => Ok(None),
    }
}

pub(crate) fn random_id() -> [u8; 16] {
    rand::random::<[u8; 16]>()
}

pub(crate) fn require_secret(secret: Option<&[u8]>) -> Result<&[u8]> {
    secret.ok_or_else(|| Error::new("--encrypt requires --secret"))
}

pub(super) fn run_pay(config: PayConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    let mut psbt = io::read_psbt_source(&config.file, stdin)?;
    let recipient = config
        .to
        .address
        .require_network(config.network.0)
        .map_err(|error| {
            Error::new(format!("address not valid for {}: {error}", config.network))
        })?;
    let payer = config.payer.map(|p| p.into_array()).unwrap_or([0u8; 32]);

    let payment = Payment {
        kind: PAYMENT_KIND_REAL,
        payer,
        amount_sats: config.to.amount.to_sat(),
        script_pubkey: recipient.script_pubkey().into_bytes(),
        label: config.label.unwrap_or_default(),
    };

    if config.dummy > 0 && !config.encrypt {
        return Err(Error::new(
            "--dummy padding requires --encrypt; plaintext dummies are trivially distinguishable",
        ));
    }
    let secret = if config.encrypt {
        Some(require_secret(
            config.secret.as_ref().map(|s| s.as_bytes()),
        )?)
    } else {
        None
    };

    add_payment_entry(&mut psbt, &payment, secret)?;
    for _ in 0..config.dummy {
        add_payment_entry(&mut psbt, &random_dummy_payment(), secret)?;
    }
    Ok(psbt)
}

fn add_payment_entry(psbt: &mut Psbt, payment: &Payment, secret: Option<&[u8]>) -> Result<()> {
    let id = random_id();
    let blob = match secret {
        Some(secret) => encrypt(secret, PSBT_GLOBAL_PAYMENT_SUBTYPE, &id, &payment.encode())?,
        None => payment.encode(),
    };
    psbt.global.add_payment(id, blob);
    Ok(())
}

/// A randomly-generated dummy payment record: a fresh v0 witness program
/// script plus random payer/amount, indistinguishable from a real entry once
/// encrypted. Shared by `pay --dummy` and the webgui `/api/pay` dummy padding.
pub(crate) fn random_dummy_payment() -> Payment {
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
}

// --- webgui negotiation mechanism (opaque records) --------------------------
//
// The `/api/{pay,confirm,payments}` routes (webgui.rs) speak the shared
// frontend contract: the record bytes arrive OPAQUE (hex) — the frontend
// builds them — and the server only appends/decodes, using the exact same
// deterministic AEAD helpers as the CLI subcommands above. The surface mirrors
// concurrent-psbt-wasm's negotiation module (crates/concurrent-psbt-wasm/src/
// negotiation.rs) so HttpBackend and WasmBackend are indistinguishable to the
// shared frontend.

/// Build a real payment record from address/amount parts (the webgui
/// `/api/pay` address variant). The txout is constructed server-side with the
/// exact same validation as `ptj pay --to`: the address must parse and must be
/// valid for `network`, and the amount is parsed in BTC denomination. `payer`
/// is an OPAQUE optional 32-byte id copied into the record unchanged (payer
/// semantics live in the negotiation spec, not in this mechanism).
#[cfg(feature = "webgui")]
pub(crate) fn payment_from_parts(
    address: &str,
    amount_btc: &str,
    network: crate::cli::NetworkArg,
    label: Option<&str>,
    payer: Option<[u8; 32]>,
) -> Result<Payment> {
    let unchecked: bitcoin::Address<bitcoin::address::NetworkUnchecked> = address
        .parse()
        .map_err(|error| Error::new(format!("invalid address {address}: {error}")))?;
    let recipient = unchecked
        .require_network(network.0)
        .map_err(|error| Error::new(format!("address not valid for {network}: {error}")))?;
    let amount = bitcoin::Amount::from_str_in(amount_btc, bitcoin::Denomination::Bitcoin)
        .map_err(|error| Error::new(format!("invalid amount {amount_btc}: {error}")))?;
    Ok(Payment {
        kind: PAYMENT_KIND_REAL,
        payer: payer.unwrap_or([0u8; 32]),
        amount_sats: amount.to_sat(),
        script_pubkey: recipient.script_pubkey().into_bytes(),
        label: label.unwrap_or_default().to_owned(),
    })
}

/// Append an opaque payment record (webgui `/api/pay`). When `secret` is
/// present the record is encrypted; otherwise it is stored in the clear.
#[cfg(feature = "webgui")]
pub(crate) fn add_opaque_payment(
    psbt: &mut Psbt,
    record: &[u8],
    secret: Option<&[u8]>,
) -> Result<()> {
    let id = random_id();
    let blob = match secret {
        Some(secret) => encrypt(secret, PSBT_GLOBAL_PAYMENT_SUBTYPE, &id, record)?,
        None => record.to_vec(),
    };
    psbt.global.add_payment(id, blob);
    Ok(())
}

/// Append an opaque confirmation record (webgui `/api/confirm`).
#[cfg(feature = "webgui")]
pub(crate) fn add_opaque_confirmation(
    psbt: &mut Psbt,
    record: &[u8],
    secret: Option<&[u8]>,
) -> Result<()> {
    let id = random_id();
    let blob = match secret {
        Some(secret) => encrypt(secret, PSBT_GLOBAL_CONFIRMATION_SUBTYPE, &id, record)?,
        None => record.to_vec(),
    };
    psbt.global.add_confirmation(id, blob);
    Ok(())
}

/// Decode the negotiation band to opaque hex blobs (webgui `/api/payments`).
/// Encrypted entries are decrypted when `secret` is supplied (a wrong secret
/// is an error); encrypted entries with no secret given are returned as their
/// stored ciphertext bytes so the caller can decide.
#[cfg(feature = "webgui")]
pub(crate) fn decode_band(
    psbt: &Psbt,
    secret: Option<&[u8]>,
) -> Result<(Vec<String>, Vec<String>)> {
    let payments = psbt
        .global
        .payments()
        .into_iter()
        .map(|(id, blob)| decode_band_entry(PSBT_GLOBAL_PAYMENT_SUBTYPE, &id, &blob, secret))
        .collect::<Result<Vec<_>>>()?;
    let confirmations = psbt
        .global
        .confirmations()
        .into_iter()
        .map(|(id, blob)| decode_band_entry(PSBT_GLOBAL_CONFIRMATION_SUBTYPE, &id, &blob, secret))
        .collect::<Result<Vec<_>>>()?;
    Ok((payments, confirmations))
}

#[cfg(feature = "webgui")]
fn decode_band_entry(
    subtype: u8,
    id: &[u8; 16],
    blob: &[u8],
    secret: Option<&[u8]>,
) -> Result<String> {
    let bytes = match (blob.first(), secret) {
        (Some(&FORMAT_ENCRYPTED), Some(secret)) => {
            decrypt(secret, subtype, id, blob)?.unwrap_or_else(|| blob.to_vec())
        }
        _ => blob.to_vec(),
    };
    Ok(hex(&bytes))
}

pub(super) fn run_confirm(config: ConfirmConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    let mut psbt = io::read_psbt_source(&config.file, stdin)?;
    let peer_id = config.peer_id.map(|p| p.into_array()).unwrap_or([0u8; 32]);

    let secret = if config.encrypt {
        Some(require_secret(
            config.secret.as_ref().map(|s| s.as_bytes()),
        )?)
    } else {
        None
    };

    add_derived_confirmation(&mut psbt, peer_id, secret)?;
    Ok(psbt)
}

/// Derive a confirmation of the CURRENT state of `psbt` (its unordered unique
/// id) for `peer_id` and append it. This is the one confirmation builder: the
/// CLI (`ptj confirm`) and the webgui derive variant (`/api/confirm` with
/// `derive: true`) both land here, so the two surfaces produce byte-identical
/// records and ids.
pub(crate) fn add_derived_confirmation(
    psbt: &mut Psbt,
    peer_id: [u8; 32],
    secret: Option<&[u8]>,
) -> Result<()> {
    let unique_id = unordered_unique_id(psbt);
    let confirmation = Confirmation { peer_id, unique_id };

    // Derived id so a re-emitted identical confirmation deduplicates. Keyed
    // with the secret when encrypting, so observers cannot dictionary-test
    // guessable (peer_id, unique_id) pairs against the id.
    let mut engine = ConfirmationIdHash::engine();
    if let Some(secret) = secret {
        engine.input(&derive_key(secret));
    }
    engine.input(&peer_id);
    engine.input(&unique_id);
    let full = ConfirmationIdHash::from_engine(engine).to_byte_array();
    let mut id = [0u8; 16];
    id.copy_from_slice(&full[..16]);

    let blob = match secret {
        Some(secret) => encrypt(
            secret,
            PSBT_GLOBAL_CONFIRMATION_SUBTYPE,
            &id,
            &confirmation.encode(),
        )?,
        None => confirmation.encode(),
    };
    psbt.global.add_confirmation(id, blob);
    Ok(())
}

pub(super) fn run_payments(config: PaymentsConfig, stdin: Option<&[u8]>) -> Result<String> {
    let psbt = io::read_psbt_source(&config.file, stdin)?;
    let secret = config.secret.as_ref().map(|s| s.as_bytes());

    let mut payments = Vec::new();
    let mut real_output_total = 0u64;
    for (id, blob) in psbt.global.payments() {
        let (payment, encrypted, undecryptable) = decode_payment(&id, &blob, secret);
        if let Some(payment) = &payment
            && !payment.is_dummy()
        {
            real_output_total = real_output_total.saturating_add(payment.amount_sats);
        }
        payments.push(payment_json(&id, payment, encrypted, undecryptable));
    }

    let mut confirmations = Vec::new();
    for (id, blob) in psbt.global.confirmations() {
        let (confirmation, encrypted, undecryptable) = decode_confirmation(&id, &blob, secret);
        confirmations.push(confirmation_json(
            &id,
            confirmation,
            encrypted,
            undecryptable,
        ));
    }

    // Output coverage: does the transaction's real output value cover the
    // real (non-dummy, decrypted) payments requested?
    let output_total: u64 = psbt.outputs.iter().map(|o| o.amount.to_sat()).sum();
    let covered = output_total >= real_output_total;

    let report = serde_json::json!({
        "payments": payments,
        "confirmations": confirmations,
        "requested_real_sats": real_output_total,
        "output_total_sats": output_total,
        "outputs_cover_payments": covered,
    });
    if config.json {
        Ok(report.to_string())
    } else {
        Ok(render_human(&report))
    }
}

fn decode_payment(
    id: &[u8; 16],
    blob: &[u8],
    secret: Option<&[u8]>,
) -> (Option<Payment>, bool, bool) {
    let encrypted = blob.first() == Some(&FORMAT_ENCRYPTED);
    if encrypted {
        match secret.map(|s| decrypt(s, PSBT_GLOBAL_PAYMENT_SUBTYPE, id, blob)) {
            Some(Ok(Some(pt))) => (Payment::decode(&pt).ok(), true, false),
            _ => (None, true, true),
        }
    } else {
        (Payment::decode(blob).ok(), false, false)
    }
}

fn decode_confirmation(
    id: &[u8; 16],
    blob: &[u8],
    secret: Option<&[u8]>,
) -> (Option<Confirmation>, bool, bool) {
    let encrypted = blob.first() == Some(&FORMAT_ENCRYPTED);
    if encrypted {
        match secret.map(|s| decrypt(s, PSBT_GLOBAL_CONFIRMATION_SUBTYPE, id, blob)) {
            Some(Ok(Some(pt))) => (Confirmation::decode(&pt).ok(), true, false),
            _ => (None, true, true),
        }
    } else {
        (Confirmation::decode(blob).ok(), false, false)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn payment_json(
    id: &[u8; 16],
    payment: Option<Payment>,
    encrypted: bool,
    undecryptable: bool,
) -> serde_json::Value {
    match payment {
        Some(p) => serde_json::json!({
            "id": hex(id),
            "encrypted": encrypted,
            "dummy": p.is_dummy(),
            "payer": hex(&p.payer),
            "amount_sats": p.amount_sats,
            "script_pubkey": hex(&p.script_pubkey),
            "label": p.label,
        }),
        None => serde_json::json!({
            "id": hex(id),
            "encrypted": encrypted,
            "undecryptable": undecryptable,
        }),
    }
}

fn confirmation_json(
    id: &[u8; 16],
    confirmation: Option<Confirmation>,
    encrypted: bool,
    undecryptable: bool,
) -> serde_json::Value {
    match confirmation {
        Some(c) => serde_json::json!({
            "id": hex(id),
            "encrypted": encrypted,
            "peer_id": hex(&c.peer_id),
            "unique_id": hex(&c.unique_id),
        }),
        None => serde_json::json!({
            "id": hex(id),
            "encrypted": encrypted,
            "undecryptable": undecryptable,
        }),
    }
}

fn render_human(report: &serde_json::Value) -> String {
    let mut out = String::new();
    let payments = report["payments"].as_array().map_or(0, Vec::len);
    let confirmations = report["confirmations"].as_array().map_or(0, Vec::len);
    out.push_str(&format!("payments: {payments}\n"));
    for p in report["payments"].as_array().into_iter().flatten() {
        if p.get("undecryptable").and_then(serde_json::Value::as_bool) == Some(true) {
            out.push_str(&format!(
                "  {} (encrypted, not for us)\n",
                p["id"].as_str().unwrap_or("")
            ));
        } else {
            let dummy = if p["dummy"].as_bool() == Some(true) {
                " [dummy]"
            } else {
                ""
            };
            out.push_str(&format!(
                "  {} {} sats{}\n",
                p["id"].as_str().unwrap_or(""),
                p["amount_sats"].as_u64().unwrap_or(0),
                dummy
            ));
        }
    }
    out.push_str(&format!("confirmations: {confirmations}\n"));
    out.push_str(&format!(
        "requested (real): {} sats; outputs: {} sats; covered: {}\n",
        report["requested_real_sats"].as_u64().unwrap_or(0),
        report["output_total_sats"].as_u64().unwrap_or(0),
        report["outputs_cover_payments"].as_bool().unwrap_or(false),
    ));
    out
}
