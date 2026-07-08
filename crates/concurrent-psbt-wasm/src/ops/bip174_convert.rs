//! BIP-370 (v2) <-> BIP-174 (v0) conversion, ported verbatim from ptj.
//!
//! Source: crates/ptj/src/commands/{export_bip174,import_bip174}.rs.
//!
//! RECOMMENDATION (real repo): these converters are pure and are duplicated
//! between ptj and this wasm crate only because they currently live as
//! `pub(crate)`/private fns in ptj. The clean fix is to PROMOTE
//! `export_bip174_psbt` / `import_bip174_psbt` (and the inspect JSON) into a
//! shared library both crates depend on — either `concurrent-psbt` itself or a
//! small `ptj-core` crate — and delete this module. See README.md "Sharing".

use std::collections::BTreeMap;

use bitcoin::{OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use concurrent_psbt::global::GlobalSortExt as _;
use psbt_v2::v0::bitcoin as bip174;
use psbt_v2::v2::{Global, Input, Output, Psbt};

// --- export (v2 -> v0 base64) --------------------------------------------

pub fn export_bip174_psbt(psbt: Psbt) -> Result<String, String> {
    Ok(to_bip174(psbt)?.to_string())
}

fn to_bip174(psbt: Psbt) -> Result<bip174::Psbt, String> {
    if psbt.global.is_unordered() {
        return Err("export-bip174 expects an ordered PSBT; run sort first".to_string());
    }
    let lock_time = psbt
        .determine_lock_time()
        .map_err(|_| "cannot determine BIP 370 transaction lock time".to_string())?;
    let unsigned_tx = Transaction {
        version: psbt.global.tx_version,
        lock_time,
        input: psbt.inputs.iter().map(unsigned_tx_in).collect(),
        output: psbt.outputs.iter().map(tx_out).collect(),
    };
    let mut out = bip174::Psbt::from_unsigned_tx(unsigned_tx)
        .map_err(|e| format!("building BIP 174 PSBT: {e}"))?;
    out.xpub = psbt.global.xpubs;
    out.proprietary = map_proprietaries_out(psbt.global.proprietaries);
    out.unknown = map_unknowns_out(psbt.global.unknowns);
    out.inputs = psbt.inputs.into_iter().map(input_to_bip174).collect();
    out.outputs = psbt.outputs.into_iter().map(output_to_bip174).collect();
    Ok(out)
}

fn unsigned_tx_in(input: &Input) -> TxIn {
    TxIn {
        previous_output: OutPoint {
            txid: input.previous_txid,
            vout: input.spent_output_index,
        },
        script_sig: ScriptBuf::new(),
        sequence: input.sequence.unwrap_or(Sequence::MAX),
        witness: Witness::new(),
    }
}

fn tx_out(output: &Output) -> TxOut {
    TxOut {
        value: output.amount,
        script_pubkey: output.script_pubkey.clone(),
    }
}

fn input_to_bip174(input: Input) -> bip174::Input {
    bip174::Input {
        non_witness_utxo: input.non_witness_utxo,
        witness_utxo: input.witness_utxo,
        partial_sigs: input.partial_sigs,
        sighash_type: input.sighash_type,
        redeem_script: input.redeem_script,
        witness_script: input.witness_script,
        bip32_derivation: input.bip32_derivations,
        final_script_sig: input.final_script_sig,
        final_script_witness: input.final_script_witness,
        ripemd160_preimages: input.ripemd160_preimages,
        sha256_preimages: input.sha256_preimages,
        hash160_preimages: input.hash160_preimages,
        hash256_preimages: input.hash256_preimages,
        tap_key_sig: input.tap_key_sig,
        tap_script_sigs: input.tap_script_sigs,
        tap_scripts: input.tap_scripts,
        tap_key_origins: input.tap_key_origins,
        tap_internal_key: input.tap_internal_key,
        tap_merkle_root: input.tap_merkle_root,
        proprietary: map_proprietaries_out(input.proprietaries),
        unknown: map_unknowns_out(input.unknowns),
    }
}

fn output_to_bip174(output: Output) -> bip174::Output {
    bip174::Output {
        redeem_script: output.redeem_script,
        witness_script: output.witness_script,
        bip32_derivation: output.bip32_derivations,
        tap_internal_key: output.tap_internal_key,
        tap_tree: output.tap_tree,
        tap_key_origins: output.tap_key_origins,
        proprietary: map_proprietaries_out(output.proprietaries),
        unknown: map_unknowns_out(output.unknowns),
    }
}

/// v2 -> v0 proprietary map, stripping the negotiation band so it never reaches
/// the Core-facing artifact (verbatim from ptj export_bip174).
fn map_proprietaries_out(
    keys: BTreeMap<psbt_v2::raw::ProprietaryKey, Vec<u8>>,
) -> BTreeMap<bip174::raw::ProprietaryKey, Vec<u8>> {
    keys.into_iter()
        .filter(|(key, _)| {
            !(key.prefix == concurrent_psbt::PROPRIETARY_PREFIX
                && matches!(
                    key.subtype,
                    concurrent_psbt::payments::negotiation::PSBT_GLOBAL_PAYMENT_SUBTYPE
                        | concurrent_psbt::payments::negotiation::PSBT_GLOBAL_CONFIRMATION_SUBTYPE
                ))
        })
        .map(|(key, value)| {
            (
                bip174::raw::ProprietaryKey {
                    prefix: key.prefix,
                    subtype: key.subtype,
                    key: key.key,
                },
                value,
            )
        })
        .collect()
}

fn map_unknowns_out(
    keys: BTreeMap<psbt_v2::raw::Key, Vec<u8>>,
) -> BTreeMap<bip174::raw::Key, Vec<u8>> {
    keys.into_iter()
        .map(|(key, value)| {
            (
                bip174::raw::Key {
                    type_value: key.type_value,
                    key: key.key,
                },
                value,
            )
        })
        .collect()
}

// --- import (v0 -> v2) ---------------------------------------------------

pub fn import_bip174_psbt(psbt: bip174::Psbt, modifiable: bool) -> Result<Psbt, String> {
    let bip174::Psbt {
        unsigned_tx,
        version: _,
        xpub,
        proprietary,
        unknown,
        inputs,
        outputs,
    } = psbt;

    if inputs.len() != unsigned_tx.input.len() {
        return Err("BIP 174 input map count does not match unsigned transaction".to_string());
    }
    if outputs.len() != unsigned_tx.output.len() {
        return Err("BIP 174 output map count does not match unsigned transaction".to_string());
    }

    let input_count = inputs.len();
    let output_count = outputs.len();
    let mut global = Global {
        tx_version: unsigned_tx.version,
        fallback_lock_time: Some(unsigned_tx.lock_time),
        input_count,
        output_count,
        xpubs: xpub,
        proprietaries: map_proprietaries_in(proprietary),
        unknowns: map_unknowns_in(unknown),
        ..Global::default()
    };
    // BIP 174 has no TX_MODIFIABLE field. Strict by default, overridable
    // always: `modifiable` is the caller's explicit assertion that inputs and
    // outputs may still be added (mirrors ptj import-bip174 --modifiable).
    global.tx_modifiable_flags = if modifiable { 0x03 } else { 0 };

    let inputs = unsigned_tx
        .input
        .into_iter()
        .zip(inputs)
        .map(|(txin, input)| input_from_bip174(txin, input))
        .collect();
    let outputs = unsigned_tx
        .output
        .into_iter()
        .zip(outputs)
        .map(|(txout, output)| output_from_bip174(txout, output))
        .collect();

    Ok(Psbt { global, inputs, outputs })
}

fn input_from_bip174(txin: bitcoin::TxIn, input: bip174::Input) -> Input {
    let mut result = Input::new(&txin.previous_output);
    if txin.sequence != Sequence::MAX {
        result.sequence = Some(txin.sequence);
    }
    result.non_witness_utxo = input.non_witness_utxo;
    result.witness_utxo = input.witness_utxo;
    result.partial_sigs = input.partial_sigs;
    result.sighash_type = input.sighash_type;
    result.redeem_script = input.redeem_script;
    result.witness_script = input.witness_script;
    result.bip32_derivations = input.bip32_derivation;
    result.final_script_sig = input.final_script_sig;
    result.final_script_witness = input.final_script_witness;
    result.ripemd160_preimages = input.ripemd160_preimages;
    result.sha256_preimages = input.sha256_preimages;
    result.hash160_preimages = input.hash160_preimages;
    result.hash256_preimages = input.hash256_preimages;
    result.tap_key_sig = input.tap_key_sig;
    result.tap_script_sigs = input.tap_script_sigs;
    result.tap_scripts = input.tap_scripts;
    result.tap_key_origins = input.tap_key_origins;
    result.tap_internal_key = input.tap_internal_key;
    result.tap_merkle_root = input.tap_merkle_root;
    result.proprietaries = map_proprietaries_in(input.proprietary);
    result.unknowns = map_unknowns_in(input.unknown);
    result
}

fn output_from_bip174(txout: bitcoin::TxOut, output: bip174::Output) -> Output {
    let mut result = Output::new(txout);
    result.redeem_script = output.redeem_script;
    result.witness_script = output.witness_script;
    result.bip32_derivations = output.bip32_derivation;
    result.tap_internal_key = output.tap_internal_key;
    result.tap_tree = output.tap_tree;
    result.tap_key_origins = output.tap_key_origins;
    result.proprietaries = map_proprietaries_in(output.proprietary);
    result.unknowns = map_unknowns_in(output.unknown);
    result
}

fn map_proprietaries_in(
    keys: BTreeMap<bip174::raw::ProprietaryKey, Vec<u8>>,
) -> BTreeMap<psbt_v2::raw::ProprietaryKey, Vec<u8>> {
    keys.into_iter()
        .map(|(key, value)| {
            (
                psbt_v2::raw::ProprietaryKey {
                    prefix: key.prefix,
                    subtype: key.subtype,
                    key: key.key,
                },
                value,
            )
        })
        .collect()
}

fn map_unknowns_in(
    keys: BTreeMap<bip174::raw::Key, Vec<u8>>,
) -> BTreeMap<psbt_v2::raw::Key, Vec<u8>> {
    keys.into_iter()
        .map(|(key, value)| {
            (
                psbt_v2::raw::Key {
                    type_value: key.type_value,
                    key: key.key,
                },
                value,
            )
        })
        .collect()
}
