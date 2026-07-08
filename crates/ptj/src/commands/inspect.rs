use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::output::OutputUniqueIdExt;
use serde_json::json;

use crate::Result;
use crate::cli::InspectConfig;
use crate::io;

pub(super) fn run(config: InspectConfig, stdin: Option<&[u8]>) -> Result<String> {
    let psbt = io::read_psbt_source(&config.file, stdin)?;
    Ok(inspect_psbt(&psbt).to_string())
}

pub(crate) fn inspect_psbt(psbt: &psbt_v2::v2::Psbt) -> serde_json::Value {
    let flags = psbt.global.tx_modifiable_flags & 0x03;
    let inputs: Vec<_> = psbt.inputs.iter().map(inspect_input).collect();
    let outputs: Vec<_> = psbt.outputs.iter().map(inspect_output).collect();
    let known_input_sats = known_input_total_sats(&psbt.inputs);
    let output_sats: u64 = psbt
        .outputs
        .iter()
        .map(|output| output.amount.to_sat())
        .sum();

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
        // The psbt.md unordered PSBT unique id (canonical-sort-then-hash over
        // the live element sets) — the identity `ptj confirm` records, so a
        // GUI can show which state a confirmation refers to.
        "unordered_unique_id_hex": hex_encode(
            &concurrent_psbt::payments::negotiation::unordered_unique_id(psbt),
        ),
        "inputs": inputs,
        "outputs": outputs,
        "totals": {
            "known_input_sats": known_input_sats,
            "output_sats": output_sats,
            "fee_sats_if_inputs_known": known_input_sats.map(|input_sats| {
                i128::from(input_sats) - i128::from(output_sats)
            }),
        },
        // The RAW keymap entries (global / per-input / per-output), including
        // the pairs the typed fields above parse — the fragment viewer/editor
        // operates on these. `null` only if the serialized form failed to
        // re-derive (should not happen for a PSBT that just parsed).
        "raw": raw_entries(psbt),
    })
}

/// The raw `<key> -> <value>` entries of every map, each classified as
/// `known` (parsed into a typed field), `unknown`, or `proprietary` (with the
/// BIP 174 envelope broken out when it parses). `key_hex` is the full raw key
/// (compact-size `keytype` prefix plus `keydata`) — the handle `/api/edit`
/// edits address entries by.
fn raw_entries(psbt: &psbt_v2::v2::Psbt) -> serde_json::Value {
    let Ok(maps) = crate::rawmap::raw_maps(psbt) else {
        return serde_json::Value::Null;
    };
    let global = map_entries(&maps.global, &psbt.global.unknowns);
    let inputs: Vec<_> = maps
        .inputs
        .iter()
        .zip(&psbt.inputs)
        .map(|(map, input)| map_entries(map, &input.unknowns))
        .collect();
    let outputs: Vec<_> = maps
        .outputs
        .iter()
        .zip(&psbt.outputs)
        .map(|(map, output)| map_entries(map, &output.unknowns))
        .collect();
    json!({
        "global": global,
        "inputs": inputs,
        "outputs": outputs,
    })
}

fn map_entries(
    map: &[crate::rawmap::RawPair],
    unknowns: &std::collections::BTreeMap<psbt_v2::raw::Key, Vec<u8>>,
) -> Vec<serde_json::Value> {
    map.iter()
        .map(|pair| {
            let mut entry = json!({
                "key_hex": hex_encode(&pair.key),
                "value_hex": hex_encode(&pair.value),
            });
            if let Ok((key_type, key_data)) = crate::rawmap::split_key_type(&pair.key) {
                entry["key_type"] = json!(key_type);
                entry["key_data_hex"] = json!(hex_encode(key_data));
                if key_type == 0xFC {
                    entry["kind"] = json!("proprietary");
                    if let Some((prefix, subtype, sub_key)) =
                        crate::rawmap::split_proprietary(key_data)
                    {
                        entry["proprietary"] = json!({
                            "prefix_hex": hex_encode(&prefix),
                            "prefix_utf8": String::from_utf8(prefix.clone()).ok(),
                            "subtype": subtype,
                            "key_data_hex": hex_encode(&sub_key),
                        });
                    }
                } else if u8::try_from(key_type).is_ok_and(|type_value| {
                    unknowns.contains_key(&psbt_v2::raw::Key {
                        type_value,
                        key: key_data.to_vec(),
                    })
                }) {
                    entry["kind"] = json!("unknown");
                } else {
                    entry["kind"] = json!("known");
                }
            }
            entry
        })
        .collect()
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
