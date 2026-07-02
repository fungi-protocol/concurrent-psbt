use concurrent_psbt::fee::{FeeContribution, GlobalFeeExt as _};
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

    let input_sizes: Vec<_> = psbt.inputs.iter().map(input_size_estimate).collect();
    let output_sizes: Vec<_> = psbt.outputs.iter().map(output_size).collect();
    let inputs: Vec<_> = inputs
        .into_iter()
        .zip(&input_sizes)
        .map(|(mut entry, size)| {
            entry["size"] = size.to_json();
            entry
        })
        .collect();
    let outputs: Vec<_> = outputs
        .into_iter()
        .zip(&output_sizes)
        .map(|(mut entry, size)| {
            entry["size"] = size.to_json();
            entry
        })
        .collect();

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
            // Sum of the plaintext PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION
            // entries — the spec § Termination quantity, via the library's
            // read projection (`fee::total_declared_fee`). That projection
            // only reads contributions it can decode: encrypted or malformed
            // entries are skipped, so `declared_fee_undecoded_count` reports
            // how many entries the total could NOT count — a viewer can say
            // "N contributions unreadable" instead of presenting a partial
            // sum as the whole.
            "declared_fee_sats": concurrent_psbt::fee::total_declared_fee(&psbt.global),
            "declared_fee_undecoded_count": undecoded_fee_contribution_count(&psbt.global),
            "size": size_totals(psbt, &input_sizes, &output_sizes),
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

// --- size estimates ----------------------------------------------------------
//
// Weight units (WU) are the canonical integer; `vbytes = ceil(weight / 4)`.
// An entry is `exact` only when its serialized size is fully determined by
// the BIP 174 fields present: inputs with final scripts, and outputs always
// (their bytes are amount + script_pubkey, both known). Everything else is an
// estimate labelled by its `basis`, mirroring the demo GUI's conventions
// (contrib/demo-gui/src/model.ts itemSizeEstimate: 68 vB default input,
// taproot script-path floored at the same 68 vB) — but grounded in the real
// script kind when a spent scriptPubKey is available.

/// outpoint (36) + empty-scriptSig compact size (1) + sequence (4): the
/// non-witness bytes a scriptSig-less input contributes.
const INPUT_BASE_BYTES: u64 = 41;
/// Witness stack: item count (1) + DER signature push (1 + 72) + compressed
/// pubkey push (1 + 33).
const P2WPKH_WITNESS_BYTES: u64 = 108;
/// 272 WU = 68 vB, the classic single-sig segwit input estimate — also the
/// demo GUI's DEFAULT_INPUT_VBYTES fallback for unknown script kinds.
const P2WPKH_INPUT_WEIGHT: u64 = INPUT_BASE_BYTES * 4 + P2WPKH_WITNESS_BYTES;
/// Witness stack: item count (1) + 64-byte Schnorr signature push (1 + 64).
const P2TR_KEY_SPEND_WITNESS_BYTES: u64 = 66;
/// 230 WU = 57.5 vB (rounds up to 58 vB alone).
const P2TR_KEY_SPEND_INPUT_WEIGHT: u64 = INPUT_BASE_BYTES * 4 + P2TR_KEY_SPEND_WITNESS_BYTES;
/// scriptSig: DER signature push (1 + 72) + compressed pubkey push (1 + 33) =
/// 107 bytes; total (36 + 1 + 107 + 4) * 4 = 592 WU = 148 vB, no witness.
const P2PKH_INPUT_WEIGHT: u64 = (36 + 1 + 107 + 4) * 4;
/// Unknown script kind (or no UTXO data at all): assume the common P2WPKH
/// shape, exactly the demo's 68 vB default.
const FALLBACK_INPUT_WEIGHT: u64 = P2WPKH_INPUT_WEIGHT;

struct SizeEstimate {
    weight: u64,
    exact: bool,
    /// What the number was derived from — honest labelling for the UI
    /// ("final_scripts" is measured, everything else is an assumption).
    basis: &'static str,
    /// Whether this input is expected to carry a witness (decides the
    /// transaction-level segwit marker/flag overhead).
    witness: bool,
}

impl SizeEstimate {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "weight": self.weight,
            "vbytes": self.weight.div_ceil(4),
            "exact": self.exact,
            "basis": self.basis,
        })
    }
}

fn input_size_estimate(input: &psbt_v2::v2::Input) -> SizeEstimate {
    if input.final_script_sig.is_some() || input.final_script_witness.is_some() {
        let script_sig_len = input
            .final_script_sig
            .as_ref()
            .map_or(0, |script| script.len() as u64);
        let base = 36 + compact_size_len(script_sig_len) + script_sig_len + 4;
        let witness_bytes = input
            .final_script_witness
            .as_ref()
            .map_or(0, |witness| witness.size() as u64);
        return SizeEstimate {
            weight: base * 4 + witness_bytes,
            exact: true,
            basis: "final_scripts",
            witness: input.final_script_witness.is_some(),
        };
    }
    match spent_script_pubkey(input) {
        Some(spk) if spk.is_p2wpkh() => SizeEstimate {
            weight: P2WPKH_INPUT_WEIGHT,
            exact: false,
            basis: "p2wpkh",
            witness: true,
        },
        Some(spk) if spk.is_p2tr() && input.tap_scripts.is_empty() => SizeEstimate {
            weight: P2TR_KEY_SPEND_INPUT_WEIGHT,
            exact: false,
            basis: "p2tr_key_spend",
            witness: true,
        },
        Some(spk) if spk.is_p2tr() => SizeEstimate {
            // Script-path spends have no fixed size; floor at the demo's
            // 68 vB minimum instead of reporting the smaller key spend.
            weight: P2TR_KEY_SPEND_INPUT_WEIGHT.max(FALLBACK_INPUT_WEIGHT),
            exact: false,
            basis: "p2tr_script_path_floor",
            witness: true,
        },
        Some(spk) if spk.is_p2pkh() => SizeEstimate {
            weight: P2PKH_INPUT_WEIGHT,
            exact: false,
            basis: "p2pkh",
            witness: false,
        },
        _ => SizeEstimate {
            weight: FALLBACK_INPUT_WEIGHT,
            exact: false,
            basis: "fallback",
            witness: true,
        },
    }
}

/// Output bytes are fully known: amount (8) + compact size of the
/// scriptPubKey length + the scriptPubKey itself, all non-witness.
fn output_size(output: &psbt_v2::v2::Output) -> SizeEstimate {
    let spk_len = output.script_pubkey.len() as u64;
    SizeEstimate {
        weight: (8 + compact_size_len(spk_len) + spk_len) * 4,
        exact: true,
        basis: "script_pubkey",
        witness: false,
    }
}

/// The scriptPubKey this input spends, from PSBT_IN_WITNESS_UTXO or the
/// spent output of PSBT_IN_NON_WITNESS_UTXO.
fn spent_script_pubkey(input: &psbt_v2::v2::Input) -> Option<&bitcoin::Script> {
    input
        .witness_utxo
        .as_ref()
        .map(|utxo| utxo.script_pubkey.as_script())
        .or_else(|| {
            input
                .non_witness_utxo
                .as_ref()
                .and_then(|transaction| transaction.output.get(input.spent_output_index as usize))
                .map(|output| output.script_pubkey.as_script())
        })
}

/// Whole-transaction totals: element sums plus the fixed transaction bytes —
/// version (4) + locktime (4) + the input/output count compact sizes, and the
/// 2 WU segwit marker/flag when any input carries (or is assumed to carry) a
/// witness. `exact` iff every input is exact: outputs and counts always are,
/// and all-final inputs pin the segwit marker down too.
fn size_totals(
    psbt: &psbt_v2::v2::Psbt,
    input_sizes: &[SizeEstimate],
    output_sizes: &[SizeEstimate],
) -> serde_json::Value {
    let input_weight: u64 = input_sizes.iter().map(|size| size.weight).sum();
    let output_weight: u64 = output_sizes.iter().map(|size| size.weight).sum();
    let marker_weight = if input_sizes.iter().any(|size| size.witness) {
        2
    } else {
        0
    };
    let overhead_weight = (4
        + 4
        + compact_size_len(psbt.inputs.len() as u64)
        + compact_size_len(psbt.outputs.len() as u64))
        * 4
        + marker_weight;
    let weight = input_weight + output_weight + overhead_weight;
    json!({
        "input_weight": input_weight,
        "output_weight": output_weight,
        "overhead_weight": overhead_weight,
        "weight": weight,
        "vbytes": weight.div_ceil(4),
        "exact": input_sizes.iter().all(|size| size.exact),
    })
}

fn compact_size_len(value: u64) -> u64 {
    match value {
        0..=0xfc => 1,
        0xfd..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}

/// Fee-contribution entries that are present in the 0x22 band but do not
/// decode as plaintext [`FeeContribution`] records (encrypted blobs or
/// malformed values) — the entries `total_declared_fee` skipped.
fn undecoded_fee_contribution_count(global: &psbt_v2::v2::Global) -> usize {
    global
        .fee_contributions()
        .iter()
        .filter(|(_, blob)| FeeContribution::decode(blob).is_err())
        .count()
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
