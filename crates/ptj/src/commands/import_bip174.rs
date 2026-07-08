use std::collections::BTreeMap;

use bitcoin::Sequence;
use psbt_v2::v0::bitcoin as bip174;
use psbt_v2::v2::{Global, Input, Output, Psbt};

use crate::cli::ImportBip174Config;
use crate::{Error, Result, io};

pub(super) fn run(config: ImportBip174Config, stdin: Option<&[u8]>) -> Result<Psbt> {
    let psbt = io::read_bip174_source(&config.file, stdin)?;
    import_bip174_psbt(psbt, config.modifiable)
}

pub(crate) fn import_bip174_psbt(psbt: bip174::Psbt, modifiable: bool) -> Result<Psbt> {
    from_bip174(psbt, modifiable)
}

fn from_bip174(psbt: bip174::Psbt, modifiable: bool) -> Result<Psbt> {
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
        return Err(Error::new(
            "BIP 174 input map count does not match unsigned transaction",
        ));
    }
    if outputs.len() != unsigned_tx.output.len() {
        return Err(Error::new(
            "BIP 174 output map count does not match unsigned transaction",
        ));
    }

    let input_count = inputs.len();
    let output_count = outputs.len();
    let mut global = Global {
        tx_version: unsigned_tx.version,
        fallback_lock_time: Some(unsigned_tx.lock_time),
        input_count,
        output_count,
        xpubs: xpub,
        proprietaries: map_proprietaries(proprietary),
        unknowns: map_unknowns(unknown),
        ..Global::default()
    };
    // BIP 174 has no TX_MODIFIABLE field, so import cannot know whether the
    // author still considers the transaction modifiable. Strict by default
    // (0x00: constructor operations refuse the PSBT), overridable always:
    // --modifiable / `modifiable: true` is the user's explicit assertion that
    // inputs and outputs may still be added, enabling make-unordered /
    // atomize / join on the import.
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

    Ok(Psbt {
        global,
        inputs,
        outputs,
    })
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
    result.proprietaries = map_proprietaries(input.proprietary);
    result.unknowns = map_unknowns(input.unknown);
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
    result.proprietaries = map_proprietaries(output.proprietary);
    result.unknowns = map_unknowns(output.unknown);
    result
}

fn map_proprietaries(
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

fn map_unknowns(keys: BTreeMap<bip174::raw::Key, Vec<u8>>) -> BTreeMap<psbt_v2::raw::Key, Vec<u8>> {
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
