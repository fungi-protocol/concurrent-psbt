use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::output::OutputUniqueIdExt;
use serde_json::json;

use crate::Result;
use crate::cli::InspectConfig;
use crate::io;

pub(super) fn run(config: InspectConfig) -> Result<String> {
    let psbt = io::read_psbt(&config.file)?;
    Ok(inspect_psbt(&psbt).to_string())
}

pub(crate) fn inspect_psbt(psbt: &psbt_v2::v2::Psbt) -> serde_json::Value {
    let flags = psbt.global.tx_modifiable_flags & 0x03;
    let inputs: Vec<_> = psbt.inputs.iter().map(inspect_input).collect();
    let outputs: Vec<_> = psbt.outputs.iter().map(inspect_output).collect();
    let known_input_sats = known_input_total_sats(&psbt.inputs);
    let output_sats: u64 = psbt.outputs.iter().map(|output| output.amount.to_sat()).sum();

    json!({
        "format": "bip370",
        "ordering": if psbt.global.is_unordered() { "unordered" } else { "ordered" },
        "input_count": psbt.global.input_count,
        "output_count": psbt.global.output_count,
        "modifiability": {
            "flags": flags,
            "inputs": flags & 0x01 != 0,
            "outputs": flags & 0x02 != 0,
        },
        "sort": {
            "mode": sort_mode(psbt.global.sort_deterministic()),
            "seed_hex": psbt.global.sort_seed().map(hex_encode),
        },
        "inputs": inputs,
        "outputs": outputs,
        "totals": {
            "known_input_sats": known_input_sats,
            "output_sats": output_sats,
            "fee_sats_if_inputs_known": known_input_sats.map(|input_sats| {
                i128::from(input_sats) - i128::from(output_sats)
            }),
        },
    })
}

fn inspect_input(input: &psbt_v2::v2::Input) -> serde_json::Value {
    let non_witness_utxo_sats = non_witness_utxo_sats(input);
    let witness_utxo_sats = input.witness_utxo.as_ref().map(|utxo| utxo.value.to_sat());

    json!({
        "outpoint": format!("{}:{}", input.previous_txid, input.spent_output_index),
        "sequence": input.sequence.map(|sequence| format!("0x{:08x}", sequence.0)),
        "witness_utxo_sats": witness_utxo_sats,
        "non_witness_utxo_sats": non_witness_utxo_sats,
        "known_utxo_sats": witness_utxo_sats.or(non_witness_utxo_sats),
        "has_witness_utxo": input.witness_utxo.is_some(),
        "has_non_witness_utxo": input.non_witness_utxo.is_some(),
    })
}

fn inspect_output(output: &psbt_v2::v2::Output) -> serde_json::Value {
    json!({
        "amount_sats": output.amount.to_sat(),
        "script_pubkey_hex": hex_encode(output.script_pubkey.as_bytes()),
        "unique_id_hex": output.unique_id().map(|id| hex_encode(id.as_bytes())),
    })
}

fn known_input_total_sats(inputs: &[psbt_v2::v2::Input]) -> Option<u64> {
    inputs.iter().try_fold(0_u64, |sum, input| {
        sum.checked_add(input_amount_sats(input)?)
    })
}

fn input_amount_sats(input: &psbt_v2::v2::Input) -> Option<u64> {
    input
        .witness_utxo
        .as_ref()
        .map(|utxo| utxo.value.to_sat())
        .or_else(|| non_witness_utxo_sats(input))
}

fn non_witness_utxo_sats(input: &psbt_v2::v2::Input) -> Option<u64> {
    input
        .non_witness_utxo
        .as_ref()
        .and_then(|transaction| transaction.output.get(input.spent_output_index as usize))
        .map(|output| output.value.to_sat())
}

fn sort_mode(mode: Option<u8>) -> &'static str {
    match mode {
        Some(0x00) => "explicit",
        Some(0x01) => "deterministic",
        _ => "unset",
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
