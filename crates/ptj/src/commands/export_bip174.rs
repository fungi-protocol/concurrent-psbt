use std::collections::BTreeMap;

use bitcoin::{OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use concurrent_psbt::global::GlobalSortExt;
use psbt_v2::v0::bitcoin as bip174;
use psbt_v2::v2::Psbt;

use crate::cli::ExportBip174Config;
use crate::{Error, Result, io};

pub(super) fn run(config: ExportBip174Config, stdin: Option<&[u8]>) -> Result<String> {
    let psbt = io::read_psbt_source(&config.file, stdin)?;
    export_bip174_psbt(psbt)
}

pub(crate) fn export_bip174_psbt(psbt: Psbt) -> Result<String> {
    Ok(to_bip174(psbt)?.to_string())
}

fn to_bip174(psbt: Psbt) -> Result<bip174::Psbt> {
    if psbt.global.is_unordered() {
        return Err(Error::new(
            "export-bip174 expects an ordered PSBT; run `ptj sort` first",
        ));
    }

    let lock_time = psbt
        .determine_lock_time()
        .map_err(|_| Error::new("cannot determine BIP 370 transaction lock time"))?;
    let unsigned_tx = Transaction {
        version: psbt.global.tx_version,
        lock_time,
        input: psbt.inputs.iter().map(unsigned_tx_in).collect(),
        output: psbt.outputs.iter().map(tx_out).collect(),
    };

    let mut bip174 = bip174::Psbt::from_unsigned_tx(unsigned_tx)
        .map_err(|error| Error::new(format!("building BIP 174 PSBT: {error}")))?;
    bip174.xpub = psbt.global.xpubs;
    bip174.proprietary = map_proprietaries(psbt.global.proprietaries);
    bip174.unknown = map_unknowns(psbt.global.unknowns);
    bip174.inputs = psbt.inputs.into_iter().map(input_to_bip174).collect();
    bip174.outputs = psbt.outputs.into_iter().map(output_to_bip174).collect();
    Ok(bip174)
}

fn unsigned_tx_in(input: &psbt_v2::v2::Input) -> TxIn {
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

fn tx_out(output: &psbt_v2::v2::Output) -> TxOut {
    TxOut {
        value: output.amount,
        script_pubkey: output.script_pubkey.clone(),
    }
}

fn input_to_bip174(input: psbt_v2::v2::Input) -> bip174::Input {
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
        proprietary: map_proprietaries(input.proprietaries),
        unknown: map_unknowns(input.unknowns),
    }
}

fn output_to_bip174(output: psbt_v2::v2::Output) -> bip174::Output {
    bip174::Output {
        redeem_script: output.redeem_script,
        witness_script: output.witness_script,
        bip32_derivation: output.bip32_derivations,
        tap_internal_key: output.tap_internal_key,
        tap_tree: output.tap_tree,
        tap_key_origins: output.tap_key_origins,
        proprietary: map_proprietaries(output.proprietaries),
        unknown: map_unknowns(output.unknowns),
    }
}

fn map_proprietaries(
    keys: BTreeMap<psbt_v2::raw::ProprietaryKey, Vec<u8>>,
) -> BTreeMap<bip174::raw::ProprietaryKey, Vec<u8>> {
    keys.into_iter()
        // Belt-and-braces: strip the negotiation band (payments/confirmations)
        // on the BIP 174 handoff so it never reaches the Core-facing artifact.
        // The sorter already clears it before ordering; unique ids and sort
        // keys are preserved for round-trip recovery.
        .filter(|(key, _)| {
            !(key.prefix == concurrent_psbt::PROPRIETARY_PREFIX
                && matches!(
                    key.subtype,
                    concurrent_psbt::negotiation::PSBT_GLOBAL_PAYMENT_SUBTYPE
                        | concurrent_psbt::negotiation::PSBT_GLOBAL_CONFIRMATION_SUBTYPE
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

fn map_unknowns(keys: BTreeMap<psbt_v2::raw::Key, Vec<u8>>) -> BTreeMap<bip174::raw::Key, Vec<u8>> {
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
