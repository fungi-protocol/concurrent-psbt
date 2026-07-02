//! PSBT inspection JSON, ported verbatim from ptj.
//!
//! Source: crates/ptj/src/commands/inspect.rs::inspect_psbt. Produces the exact
//! same JSON the webgui `/api/inspect` returns and that the shared frontend
//! renders, so the two backends are indistinguishable.
//!
//! RECOMMENDATION (real repo): promote `inspect_psbt` to a shared crate (see
//! bip174_convert.rs note) and delete this module.

use concurrent_psbt::global::GlobalSortExt as _;
use concurrent_psbt::output::OutputUniqueIdExt as _;
use serde_json::{Value, json};

pub fn inspect_psbt(psbt: &psbt_v2::v2::Psbt) -> Value {
    let flags = psbt.global.tx_modifiable_flags & 0x03;
    let inputs: Vec<_> = psbt.inputs.iter().map(inspect_input).collect();
    let outputs: Vec<_> = psbt.outputs.iter().map(inspect_output).collect();
    let known_input_sats = known_input_total_sats(&psbt.inputs);
    let output_sats: u64 = psbt.outputs.iter().map(|o| o.amount.to_sat()).sum();

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

fn inspect_input(input: &psbt_v2::v2::Input) -> Value {
    let non_witness_utxo_sats = non_witness_utxo_sats(input);
    let witness_utxo_sats = input.witness_utxo.as_ref().map(|u| u.value.to_sat());
    json!({
        "outpoint": format!("{}:{}", input.previous_txid, input.spent_output_index),
        "sequence": input.sequence.map(|s| format!("0x{:08x}", s.0)),
        "witness_utxo_sats": witness_utxo_sats,
        "non_witness_utxo_sats": non_witness_utxo_sats,
        "known_utxo_sats": witness_utxo_sats.or(non_witness_utxo_sats),
        "has_witness_utxo": input.witness_utxo.is_some(),
        "has_non_witness_utxo": input.non_witness_utxo.is_some(),
    })
}

fn inspect_output(output: &psbt_v2::v2::Output) -> Value {
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
        .map(|u| u.value.to_sat())
        .or_else(|| non_witness_utxo_sats(input))
}

fn non_witness_utxo_sats(input: &psbt_v2::v2::Input) -> Option<u64> {
    input
        .non_witness_utxo
        .as_ref()
        .and_then(|tx| tx.output.get(input.spent_output_index as usize))
        .map(|o| o.value.to_sat())
}

fn sort_mode(mode: Option<u8>) -> &'static str {
    match mode {
        Some(0x00) => "explicit",
        Some(0x01) => "deterministic",
        _ => "unset",
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
