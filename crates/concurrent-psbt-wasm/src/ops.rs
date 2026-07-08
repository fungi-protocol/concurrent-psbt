//! Ported PSBT operations — one function per ptj webgui `/api/*` route.
//!
//! Each function is a faithful port of the corresponding `crate::commands::*`
//! wrapper in ptj, but linked against `concurrent-psbt` + `psbt-v2` + `bitcoin`
//! only (no CLI/file-IO types). The response JSON shape is byte-identical to the
//! webgui so the shared frontend cannot tell the HTTP and WASM backends apart.
//!
//! Errors are `String` (the webgui's `{ "error": <string> }` text); `lib.rs`
//! maps them to a thrown `JsError`.
//!
//! Provenance of each port (all paths under
//! /tmp/cpsbt-test-prune-bookkeeping/crates/ptj/src/):
//!   inspect        <- commands/inspect.rs::inspect_psbt (+ webgui inspect_response_result)
//!   create         <- commands/create.rs::create_psbt (+ webgui create_config_from_request)
//!   join           <- commands/join.rs::join_psbts
//!   sort           <- commands/sort.rs::sort_psbt
//!   make_unordered <- commands/make_unordered.rs::make_unordered_psbt
//!   atomize        <- commands/atomize.rs::atomize_psbt (+ webgui fragment shaping)
//!   concatenate    <- commands/concatenate.rs::concatenate_psbts
//!   export_bip174  <- commands/export_bip174.rs::export_bip174_psbt
//!   import_bip174  <- commands/import_bip174.rs::import_bip174_psbt
//!   pay/confirm/payments <- commands/negotiation.rs (mechanism-only, opaque blobs)

use bitcoin::{Amount, Denomination, Network};
use concurrent_psbt::Join as _;
use concurrent_psbt::global::GlobalSortExt as _;
use concurrent_psbt::roles::Creator;
use concurrent_psbt::roles::constructor::dynamic;
use concurrent_psbt::sorter::{Deterministic, ExplicitSortKeys, SeedPolicy, Sorter, Unset};
use psbt_v2::v2::{Global, Input, Output, Psbt};
use serde_json::{Value, json};

use crate::dto;
use crate::psbt_io::{encode_psbt, parse_bip174, parse_psbt_str};

// Re-implemented locally (ptj keeps these private in commands/{export,import}_bip174
// and commands/inspect); ported verbatim below to avoid a ptj dependency.
pub(crate) mod bip174_convert;
pub(crate) mod inspect_json;

// Ops return `serde_json::Value` (not `JsValue`) so the exact same code path is
// exercised by native `cargo test` AND on wasm. `lib.rs` does the final
// `Value -> JsValue` conversion at the `#[wasm_bindgen]` boundary.

/// Standard `{ psbt, inspect }` response body used by most ops.
fn psbt_response(psbt: &Psbt) -> Value {
    json!({
        "psbt": encode_psbt(psbt),
        "inspect": inspect_json::inspect_psbt(psbt),
    })
}

// --- inspect -------------------------------------------------------------

pub fn inspect(psbt: &str) -> Result<Value, String> {
    let psbt = parse_psbt_str("request psbt", psbt)?;
    // webgui returns the inspect object directly (not wrapped).
    Ok(inspect_json::inspect_psbt(&psbt))
}

// --- create --------------------------------------------------------------

pub fn create(req: dto::CreateRequest) -> Result<Value, String> {
    let network = parse_network(req.network.as_deref())?;
    let ordering = parse_ordering(req.ordering.as_deref())?;
    let seed = req
        .seed_hex
        .as_deref()
        .map(crate::bytes_arg::parse_bytes_arg)
        .transpose()?;
    let has_items = !req.inputs.is_empty() || !req.outputs.is_empty();

    let mut constructor = Creator::new().build();
    for input in &req.inputs {
        let txid: bitcoin::Txid = input
            .txid
            .parse()
            .map_err(|e| format!("invalid txid {}: {e}", input.txid))?;
        let outpoint = bitcoin::OutPoint { txid, vout: input.vout };
        constructor = constructor.input(Input::new(&outpoint));
    }
    for output in &req.outputs {
        let address: bitcoin::Address<bitcoin::address::NetworkUnchecked> = output
            .address
            .parse()
            .map_err(|e| format!("invalid address {}: {e}", output.address))?;
        let address = address
            .require_network(network)
            .map_err(|e| format!("address {} not valid for {network}: {e}", output.address))?;
        let amount = Amount::from_str_in(&output.amount_btc, Denomination::Bitcoin)
            .map_err(|e| format!("invalid amount {}: {e}", output.amount_btc))?;
        let psbt_output = Output {
            amount,
            script_pubkey: address.script_pubkey(),
            ..Output::default()
        };
        constructor = constructor.output_with_new_uid(psbt_output);
    }

    let mut psbt = constructor.into_inner();
    psbt.global.set_unordered();
    // Mirror commands/create.rs ordering/seed matrix exactly.
    match (ordering, seed) {
        (Ordering::Unset, Some(seed)) => {
            require_spec_minimum_seed(&seed, req.allow_short_seed)?;
            psbt.global.set_sort_seed(seed);
        }
        (Ordering::Unset, None) => {}
        (Ordering::Deterministic, Some(seed)) => {
            require_spec_minimum_seed(&seed, req.allow_short_seed)?;
            psbt.global.set_sort_seed(seed);
            psbt.global.set_sort_deterministic(0x01);
        }
        (Ordering::Deterministic, None) => {
            return Err("deterministic ordering requires seed_hex".to_string());
        }
        (Ordering::Explicit, Some(_)) => {
            return Err("explicit ordering does not use seed_hex".to_string());
        }
        (Ordering::Explicit, None) if has_items => {
            return Err("explicit ordering requires sort keys for every input and output; non-empty explicit create is not implemented yet".to_string());
        }
        (Ordering::Explicit, None) => psbt.global.set_sort_deterministic(0x00),
    }
    psbt.global.tx_modifiable_flags = 0x03;

    Ok(psbt_response(&psbt.into_psbt()))
}

// --- join ----------------------------------------------------------------

pub fn join(psbts: Vec<String>) -> Result<Value, String> {
    if psbts.is_empty() {
        return Err("request JSON field `psbts` must not be empty".to_string());
    }
    let parsed = psbts
        .iter()
        .enumerate()
        .map(|(i, p)| parse_psbt_str(&format!("request psbts[{i}]"), p))
        .collect::<Result<Vec<_>, _>>()?;
    let joined = join_psbts(parsed)?;
    Ok(psbt_response(&joined))
}

/// Port of commands/join.rs::join_psbts (+ join_wrapped conflict reporting).
fn join_psbts(psbts: Vec<Psbt>) -> Result<Psbt, String> {
    let constructors = psbts
        .into_iter()
        .map(|psbt| {
            dynamic::Constructor::try_from_psbt(psbt)
                .map(dynamic::ResultConstructor::wrap)
                .map_err(|e| format!("PSBT is not joinable: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = constructors
        .into_iter()
        .reduce(|left, right| left.join(right))
        .ok_or_else(|| "join expects at least one PSBT".to_string())?;
    if !result.is_ok() {
        let mut details = vec!["join produced conflicting fields".to_string(), String::new()];
        result.for_each_conflict(|section, field, conflict| {
            details.push(format!("  {section}.{field}: {conflict:?}"));
        });
        return Err(details.join("\n"));
    }
    match result.try_unwrap() {
        Ok(constructor) => Ok(constructor.into_psbt()),
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    }
}

// --- sort ----------------------------------------------------------------

/// Canonical arity: `(psbt, seed_hex?, allow_short_seed?)` — mirrors
/// `Backend.sortPsbt(psbt, seedHex?, allowShortSeed?)`.
pub fn sort(psbt: &str, seed_hex: Option<&str>, allow_short_seed: bool) -> Result<Value, String> {
    let seed = seed_hex
        .map(crate::bytes_arg::parse_bytes_arg)
        .transpose()?;
    let psbt = parse_psbt_str("request psbt", psbt)?;
    let constructor = dynamic::Constructor::try_from_psbt(psbt)
        .map_err(|e| format!("request psbt: {e}"))?;
    let mut unordered = constructor.into_inner();
    if let Some(seed) = seed {
        require_spec_minimum_seed(&seed, allow_short_seed)?;
        unordered.global.set_sort_seed(seed);
    }
    let policy = if allow_short_seed {
        SeedPolicy::AllowBelowSpecMinimum
    } else {
        SeedPolicy::RequireSpecMinimum
    };
    // Port of commands/sort.rs::sort_psbt (embedded deterministic-mode seeds
    // are gated by the library under the same override).
    let sorted = match unordered.global.sort_deterministic() {
        Some(0x01) => {
            Sorter::<Deterministic>::from_unordered_psbt(unordered).into_ordered_psbt_with(policy)
        }
        Some(0x00) => Sorter::<ExplicitSortKeys>::from_unordered_psbt(unordered).into_ordered_psbt(),
        _ => Sorter::<Unset>::from_unordered_psbt(unordered).into_ordered_psbt(),
    }
    .map_err(|e| e.to_string())?;
    Ok(psbt_response(&sorted))
}

/// Port of ptj commands::require_spec_minimum_seed — identical error text so
/// the two backends stay indistinguishable through the seam.
fn require_spec_minimum_seed(seed: &[u8], allow_short_seed: bool) -> Result<(), String> {
    let len = seed.len();
    if allow_short_seed || len >= concurrent_psbt::sorter::SPEC_MIN_SEED_BYTES {
        return Ok(());
    }
    Err(format!(
        "ordering seed is {len} byte{}; the spec requires at least 128 bits (16 bytes) of \
         randomness; pass --allow-short-seed (allow_short_seed on the web API) to accept it anyway",
        if len == 1 { "" } else { "s" },
    ))
}

// --- make-unordered ------------------------------------------------------

pub fn make_unordered(psbt: &str) -> Result<Value, String> {
    let mut psbt = parse_psbt_str("request psbt", psbt)?;
    psbt.global.set_unordered();
    let unordered = dynamic::Constructor::try_from_psbt(psbt)
        .map(dynamic::Constructor::into_psbt)
        .map_err(|e| e.to_string())?;
    Ok(psbt_response(&unordered))
}

// --- atomize -------------------------------------------------------------

pub fn atomize(psbt: &str) -> Result<Value, String> {
    let mut psbt = parse_psbt_str("request psbt", psbt)?;
    psbt.global.set_unordered();
    let psbt = dynamic::Constructor::try_from_psbt(psbt)
        .map(dynamic::Constructor::into_psbt)
        .map_err(|e| e.to_string())?;
    let atoms = atomize_psbt(psbt)?;
    let fragments = atoms
        .iter()
        .map(psbt_response)
        .collect::<Vec<_>>();
    Ok(json!({ "fragments": fragments }))
}

/// Port of the private `atomize` in commands/atomize.rs.
fn atomize_psbt(psbt: Psbt) -> Result<Vec<Psbt>, String> {
    if psbt.inputs.len() + psbt.outputs.len() <= 1 {
        return Err("PSBT is already atomic".to_string());
    }
    let global = psbt.global;
    let mut atoms = Vec::with_capacity(psbt.inputs.len() + psbt.outputs.len());
    atoms.extend(psbt.inputs.into_iter().map(|input| Psbt {
        global: atom_global(&global, 1, 0),
        inputs: vec![input],
        outputs: vec![],
    }));
    atoms.extend(psbt.outputs.into_iter().map(|output| Psbt {
        global: atom_global(&global, 0, 1),
        inputs: vec![],
        outputs: vec![output],
    }));
    Ok(atoms)
}

fn atom_global(global: &Global, input_count: usize, output_count: usize) -> Global {
    let mut global = global.clone();
    global.input_count = input_count;
    global.output_count = output_count;
    global
}

// --- concatenate ---------------------------------------------------------

pub fn concatenate(psbts: Vec<String>) -> Result<Value, String> {
    let parsed = psbts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            parse_psbt_str(&format!("request psbts[{i}]"), p)
                .map(|psbt| (format!("request psbts[{i}]"), psbt))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let concatenated = concatenate_psbts(parsed)?;
    Ok(psbt_response(&concatenated))
}

/// Port of commands/concatenate.rs::concatenate_psbts.
fn concatenate_psbts(psbts: Vec<(String, Psbt)>) -> Result<Psbt, String> {
    let mut iter = psbts.into_iter();
    let (first_label, mut result) = iter
        .next()
        .ok_or_else(|| "concatenate expects at least two ordered PSBTs".to_string())?;
    validate_ordered(&first_label, &result)?;

    let mut count = 1;
    for (label, psbt) in iter {
        validate_ordered(&label, &psbt)?;
        if !same_global_context(&result.global, &psbt.global) {
            return Err(format!(
                "{label} has global fields that differ from the first PSBT; concatenate would discard or reorder global information"
            ));
        }
        result.inputs.extend(psbt.inputs);
        result.outputs.extend(psbt.outputs);
        count += 1;
    }
    if count < 2 {
        return Err("concatenate expects at least two ordered PSBTs".to_string());
    }
    result.global.input_count = result.inputs.len();
    result.global.output_count = result.outputs.len();
    Ok(result)
}

fn validate_ordered(label: &str, psbt: &Psbt) -> Result<(), String> {
    if psbt.global.is_unordered() {
        return Err(format!(
            "{label} is unordered; concatenate only appends ordered PSBTs. Use join for unordered lattice merges."
        ));
    }
    Ok(())
}

fn same_global_context(left: &Global, right: &Global) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.input_count = 0;
    left.output_count = 0;
    right.input_count = 0;
    right.output_count = 0;
    left == right
}

// --- export/import bip174 ------------------------------------------------

pub fn export_bip174(psbt: &str) -> Result<Value, String> {
    let psbt = parse_psbt_str("request psbt", psbt)?;
    let exported = bip174_convert::export_bip174_psbt(psbt)?;
    Ok(json!({ "format": "bip174", "psbt": exported }))
}

pub fn import_bip174(psbt: &str, modifiable: bool) -> Result<Value, String> {
    let psbt = parse_bip174("request psbt", psbt.as_bytes())?;
    let imported = bip174_convert::import_bip174_psbt(psbt, modifiable)?;
    Ok(psbt_response(&imported))
}

// --- assign-ids ------------------------------------------------------------

/// Port of ptj commands/assign_ids.rs (+ webgui parse_id_assignments) — the
/// same semantics and error text as `/api/assign-ids`.
pub fn assign_ids(req: dto::AssignIdsRequest) -> Result<Value, String> {
    use concurrent_psbt::output::{OutputUniqueIdExt as _, UniqueId};
    use concurrent_psbt::removal::InputUniqueIdExt;

    let mut psbt = parse_psbt_str("request psbt", &req.psbt)?;
    let ids = req
        .ids
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let target = match item.target.as_str() {
                "in" | "input" => IdTarget::Input,
                "out" | "output" => IdTarget::Output,
                other => {
                    return Err(format!(
                        "request JSON ids[{position}].target must be `in` or `out`, got {other}"
                    ));
                }
            };
            let id = crate::bytes_arg::parse_bytes_arg(&item.id)?;
            Ok((target, item.index, id))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let auto = req.auto || ids.is_empty();

    for (target, index, id) in &ids {
        match target {
            IdTarget::Input => {
                let count = psbt.inputs.len();
                let input = psbt.inputs.get_mut(*index).ok_or_else(|| {
                    format!(
                        "--id in:{index}: input index out of range ({count} input{})",
                        if count == 1 { "" } else { "s" },
                    )
                })?;
                match InputUniqueIdExt::unique_id(input) {
                    Some(existing) if existing == *id => {}
                    Some(_) if !req.overwrite => {
                        return Err(format!(
                            "--id in:{index}: input already has a different PSBT_IN_UNIQUE_ID; \
                             pass --overwrite to replace it"
                        ));
                    }
                    _ => input.set_unique_id(id.clone()),
                }
            }
            IdTarget::Output => {
                let count = psbt.outputs.len();
                let output = psbt.outputs.get_mut(*index).ok_or_else(|| {
                    format!(
                        "--id out:{index}: output index out of range ({count} output{})",
                        if count == 1 { "" } else { "s" },
                    )
                })?;
                match output.unique_id() {
                    Some(existing) if existing.as_bytes() == id => {}
                    Some(_) if !req.overwrite => {
                        return Err(format!(
                            "--id out:{index}: output already has a different PSBT_OUT_UNIQUE_ID; \
                             pass --overwrite to replace it"
                        ));
                    }
                    _ => output.set_unique_id(UniqueId::new(id.clone())),
                }
            }
        }
    }

    if auto {
        for output in &mut psbt.outputs {
            if !output.has_unique_id() {
                output.set_unique_id(UniqueId::generate());
            }
        }
    }

    // Manual output ids colliding with any other output defeat uniqueness.
    for (target, index, id) in &ids {
        if *target != IdTarget::Output {
            continue;
        }
        for (other, output) in psbt.outputs.iter().enumerate() {
            if other == *index {
                continue;
            }
            if output.unique_id().map(UniqueId::into_bytes).as_deref() == Some(id.as_slice()) {
                return Err(format!(
                    "--id out:{index}: the id is already used by output {other}; \
                     PSBT_OUT_UNIQUE_ID must be unique"
                ));
            }
        }
    }

    Ok(psbt_response(&psbt))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdTarget {
    Input,
    Output,
}

// --- sync (local branch) ---------------------------------------------------

/// The Layer-2 lattice fold with no network — `POST /api/sync`'s local branch
/// (webgui-layer23 `sync_config_from_request.rs`: `TransportKind::Local` =>
/// `sync_json(&joined, &[])`). Ported here (originally authored in the merged
/// `ptj-wasm` duplicate as `sync_local`) so the PWA is LOCAL-FIRST: sync works
/// fully in-browser; transports only ever ADD input PSBTs before this fold.
///
/// `payments`/`confirmations` are empty because in the webgui contract they are
/// decoded from transport *messages*, of which the local branch has none. The
/// PSBT's own negotiation band is read via the `payments` op instead.
pub fn local_sync(psbts: Vec<String>) -> Result<Value, String> {
    if psbts.is_empty() {
        // Same 400 text as the webgui local branch with nothing to fold.
        return Err("request must contain `psbts` or a network transport".to_string());
    }
    let parsed = psbts
        .iter()
        .enumerate()
        .map(|(i, p)| parse_psbt_str(&format!("request psbts[{i}]"), p))
        .collect::<Result<Vec<_>, _>>()?;
    let joined = join_psbts(parsed)?;
    Ok(json!({
        "psbt": encode_psbt(&joined),
        "inspect": inspect_json::inspect_psbt(&joined),
        "payments": Vec::<String>::new(),
        "confirmations": Vec::<String>::new(),
    }))
}

// --- negotiation: pay / confirm / payments -------------------------------
//
// Mechanism-only: the wasm core takes an OPAQUE payment/confirmation record as
// hex (the frontend builds the record; the core never invents policy) and
// appends it to the grow-only negotiation band via the low-level ext methods.
// Optional deterministic AEAD encryption mirrors `ptj pay --encrypt`; when no
// secret is given, records are stored in the clear. `payments` decodes the
// band back to opaque hex blobs (decrypting when a secret is supplied).

pub fn pay(req: dto::PayRequest) -> Result<Value, String> {
    let mut psbt = parse_psbt_str("request psbt", &req.psbt)?;
    let record = crate::bytes_arg::parse_bytes_arg(&req.payment_hex)?;
    let secret = req
        .secret_hex
        .as_deref()
        .map(crate::bytes_arg::parse_bytes_arg)
        .transpose()?;
    if req.dummy > 0 && secret.is_none() {
        return Err(
            "dummy padding requires secret_hex; plaintext dummies are trivially distinguishable"
                .to_string(),
        );
    }
    crate::negotiation::add_payment(&mut psbt, &record, secret.as_deref())?;
    for _ in 0..req.dummy {
        let dummy = crate::negotiation::random_dummy_payment();
        crate::negotiation::add_payment(&mut psbt, &dummy, secret.as_deref())?;
    }
    Ok(psbt_response(&psbt))
}

pub fn confirm(req: dto::ConfirmRequest) -> Result<Value, String> {
    let mut psbt = parse_psbt_str("request psbt", &req.psbt)?;
    let record = crate::bytes_arg::parse_bytes_arg(&req.confirmation_hex)?;
    let secret = req
        .secret_hex
        .as_deref()
        .map(crate::bytes_arg::parse_bytes_arg)
        .transpose()?;
    crate::negotiation::add_confirmation(&mut psbt, &record, secret.as_deref())?;
    Ok(psbt_response(&psbt))
}

pub fn payments(req: dto::PaymentsRequest) -> Result<Value, String> {
    let psbt = parse_psbt_str("request psbt", &req.psbt)?;
    let secret = req
        .secret_hex
        .as_deref()
        .map(crate::bytes_arg::parse_bytes_arg)
        .transpose()?;
    let (payments, confirmations) = crate::negotiation::decode_band(&psbt, secret.as_deref())?;
    Ok(json!({ "payments": payments, "confirmations": confirmations }))
}

// --- shared helpers ------------------------------------------------------

#[derive(Clone, Copy)]
enum Ordering {
    Unset,
    Deterministic,
    Explicit,
}

/// Port of cli.rs OrderingArg::from_str. Defaults to Unset (webgui default).
fn parse_ordering(value: Option<&str>) -> Result<Ordering, String> {
    match value {
        None | Some("unset") => Ok(Ordering::Unset),
        Some("deterministic") | Some("det") => Ok(Ordering::Deterministic),
        Some("explicit") => Ok(Ordering::Explicit),
        Some(other) => Err(format!(
            "unknown ordering '{other}' (expected: unset, deterministic, explicit)"
        )),
    }
}

/// Port of cli.rs NetworkArg::from_str. Defaults to Bitcoin (webgui default).
fn parse_network(value: Option<&str>) -> Result<Network, String> {
    match value {
        None | Some("bitcoin") | Some("mainnet") => Ok(Network::Bitcoin),
        Some("testnet") | Some("testnet3") => Ok(Network::Testnet),
        Some("signet") => Ok(Network::Signet),
        Some("regtest") => Ok(Network::Regtest),
        Some(other) => Err(format!(
            "unknown network '{other}' (expected: bitcoin, testnet, signet, regtest)"
        )),
    }
}


#[cfg(test)]
mod tests {
    //! Native (off-target) tests over the pure op logic. These run under a
    //! normal `cargo test` (host target) — they exercise everything except the
    //! `#[wasm_bindgen]` JsValue marshaling in lib.rs, which is covered by the
    //! wasm-bindgen-test in tests/wasm.rs. They assert the response JSON shape
    //! is byte-identical to the ptj webgui (compare against webgui.rs tests).

    use super::*;

    /// A 16-byte (spec-minimum) ordering seed, matching the webgui tests.
    const SEED: &str = "abcdabcdabcdabcdabcdabcdabcdabcd";

    // A minimal empty regtest PSBT (base64), built the same way create() does,
    // to round-trip through the ops without needing ptj fixtures.
    fn empty_regtest_psbt() -> String {
        let v = create(dto::CreateRequest {
            network: Some("regtest".to_string()),
            ordering: None,
            seed_hex: None,
            allow_short_seed: false,
            inputs: vec![],
            outputs: vec![],
        })
        .expect("create empty");
        v["psbt"].as_str().unwrap().to_string()
    }

    #[test]
    fn create_matches_webgui_shape() {
        let v = create(dto::CreateRequest {
            network: Some("regtest".to_string()),
            ordering: Some("deterministic".to_string()),
            seed_hex: Some(SEED.to_string()),
            allow_short_seed: false,
            inputs: vec![dto::CreateInput {
                txid: "0000000000000000000000000000000000000000000000000000000000000001"
                    .to_string(),
                vout: 7,
            }],
            outputs: vec![],
        })
        .expect("create");
        assert!(v["psbt"].as_str().is_some());
        assert_eq!(v["inspect"]["ordering"], "unordered");
        assert_eq!(v["inspect"]["input_count"], 1);
        assert_eq!(v["inspect"]["sort"]["mode"], "deterministic");
        assert_eq!(v["inspect"]["sort"]["seed_hex"], SEED);
    }

    #[test]
    fn create_deterministic_without_seed_errors_like_cli() {
        let err = create(dto::CreateRequest {
            network: Some("regtest".to_string()),
            ordering: Some("deterministic".to_string()),
            seed_hex: None,
            allow_short_seed: false,
            inputs: vec![],
            outputs: vec![],
        })
        .unwrap_err();
        assert!(err.contains("deterministic ordering requires seed_hex"), "{err}");
    }

    #[test]
    fn create_rejects_short_seed_unless_overridden() {
        let request = |allow: bool| dto::CreateRequest {
            network: Some("regtest".to_string()),
            ordering: Some("deterministic".to_string()),
            seed_hex: Some("abcd".to_string()),
            allow_short_seed: allow,
            inputs: vec![],
            outputs: vec![],
        };
        let err = create(request(false)).unwrap_err();
        assert!(err.contains("128 bits"), "{err}");
        assert!(err.contains("allow_short_seed"), "{err}");

        let v = create(request(true)).expect("override accepts the short seed");
        assert_eq!(v["inspect"]["sort"]["seed_hex"], "abcd");
    }

    #[test]
    fn sort_rejects_short_seed_unless_overridden() {
        let created = created_with_output(3);
        let err = sort(&created, Some("deadbeef"), false).unwrap_err();
        assert!(err.contains("128 bits"), "{err}");

        let sorted = sort(&created, Some("deadbeef"), true).expect("override sorts");
        assert_eq!(sorted["inspect"]["ordering"], "ordered");
    }

    #[test]
    fn seed_accepts_base58_and_bech32_like_the_cli() {
        // Liberal parsing parity: a bech32 seed decodes to its 16-byte data
        // part and satisfies the spec minimum.
        use bitcoin::bech32::{self, Hrp};
        let seed = bech32::encode::<bech32::Bech32m>(Hrp::parse("seed").unwrap(), &[0xab; 16])
            .unwrap();
        let created = created_with_output(4);
        let sorted = sort(&created, Some(&seed), false).expect("bech32 seed sorts");
        assert_eq!(sorted["inspect"]["sort"]["seed_hex"], "ab".repeat(16));
    }

    #[test]
    fn inspect_roundtrips_created_psbt() {
        let psbt = empty_regtest_psbt();
        let v = inspect(&psbt).expect("inspect");
        assert_eq!(v["format"], "bip370");
        assert_eq!(v["ordering"], "unordered");
    }

    #[test]
    fn join_folds_two_copies_idempotently() {
        let psbt = empty_regtest_psbt();
        let v = join(vec![psbt.clone(), psbt]).expect("join");
        assert!(v["psbt"].as_str().is_some());
    }

    #[test]
    fn join_empty_errors() {
        assert!(join(vec![]).unwrap_err().contains("must not be empty"));
    }

    #[test]
    fn parse_psbt_rejects_garbage_like_webgui() {
        let err = inspect("not a psbt").unwrap_err();
        assert!(err.contains("decoding base64"), "{err}");
    }

    #[test]
    fn decode_hex_odd_length_matches_cli() {
        // Via the liberal parser (hex charset -> hex), same "odd length" text.
        let err = crate::bytes_arg::parse_bytes_arg("abc").unwrap_err();
        assert!(err.contains("hex string has odd length: abc"), "{err}");
    }

    // --- ports of the richer op tests from the merged ptj-wasm duplicate ---

    fn regtest_address(seed: u8) -> String {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let secret = bitcoin::secp256k1::SecretKey::from_slice(&[seed.max(1); 32]).unwrap();
        let public_key = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret);
        let public_key = bitcoin::CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        bitcoin::Address::p2wpkh(&public_key, Network::Regtest).to_string()
    }

    fn created_psbt(seed_byte: u8, vout: u32) -> String {
        let v = create(dto::CreateRequest {
            network: Some("regtest".to_string()),
            ordering: None,
            seed_hex: Some(SEED.to_string()),
            allow_short_seed: false,
            inputs: vec![dto::CreateInput { txid: format!("{seed_byte:064x}"), vout }],
            outputs: vec![],
        })
        .expect("create");
        v["psbt"].as_str().unwrap().to_string()
    }

    fn created_with_output(seed_byte: u8) -> String {
        let v = create(dto::CreateRequest {
            network: Some("regtest".to_string()),
            ordering: None,
            seed_hex: Some(SEED.to_string()),
            allow_short_seed: false,
            inputs: vec![dto::CreateInput { txid: format!("{seed_byte:064x}"), vout: 7 }],
            outputs: vec![dto::CreateOutput {
                address: regtest_address(seed_byte),
                amount_btc: "0.00050000".to_string(),
            }],
        })
        .expect("create");
        v["psbt"].as_str().unwrap().to_string()
    }

    fn ordered_with_output(seed_byte: u8) -> String {
        let unordered = created_with_output(seed_byte);
        let sorted = sort(&unordered, Some(SEED), false).expect("sort");
        sorted["psbt"].as_str().unwrap().to_string()
    }

    #[test]
    fn sort_then_make_unordered_toggles_ordering() {
        let created = created_with_output(5);
        let sorted = sort(&created, Some("deadbeefdeadbeefdeadbeefdeadbeef"), false).expect("sort ok");
        assert_eq!(sorted["inspect"]["ordering"], "ordered");

        let unordered =
            make_unordered(sorted["psbt"].as_str().unwrap()).expect("make_unordered ok");
        assert_eq!(unordered["inspect"]["ordering"], "unordered");
    }

    #[test]
    fn sort_without_seed_uses_unset_mode() {
        // Canonical two-arg sort: the seed is optional (Backend.sortPsbt(psbt)).
        let created = created_with_output(9);
        let sorted = sort(&created, None, false).expect("sort ok");
        assert_eq!(sorted["inspect"]["ordering"], "ordered");
    }

    #[test]
    fn atomize_splits_multi_element_psbt() {
        let created = created_with_output(6);
        let v = atomize(&created).expect("atomize ok");
        let fragments = v["fragments"].as_array().unwrap();
        assert_eq!(fragments.len(), 2);
        for fragment in fragments {
            assert_eq!(fragment["inspect"]["ordering"], "unordered");
        }
    }

    #[test]
    fn atomize_rejects_atomic_psbt() {
        let created = created_psbt(6, 0); // single input, no output = already atomic
        assert!(atomize(&created).unwrap_err().contains("already atomic"));
    }

    #[test]
    fn export_then_import_bip174_roundtrips() {
        let ordered = ordered_with_output(1);
        let exported = export_bip174(&ordered).expect("export ok");
        assert_eq!(exported["format"], "bip174");

        let imported =
            import_bip174(exported["psbt"].as_str().unwrap(), false).expect("import ok");
        assert_eq!(imported["inspect"]["format"], "bip370");
        assert_eq!(imported["inspect"]["ordering"], "ordered");
        assert_eq!(imported["inspect"]["input_count"], 1);
        assert_eq!(imported["inspect"]["output_count"], 1);
        // Strict default: BIP 174 has no TX_MODIFIABLE, so nothing is modifiable.
        assert_eq!(imported["inspect"]["modifiability"]["inputs"], false);
        assert_eq!(imported["inspect"]["modifiability"]["outputs"], false);

        let modifiable =
            import_bip174(exported["psbt"].as_str().unwrap(), true).expect("import ok");
        assert_eq!(modifiable["inspect"]["modifiability"]["inputs"], true);
        assert_eq!(modifiable["inspect"]["modifiability"]["outputs"], true);
    }

    #[test]
    fn assign_ids_completes_the_import_round_trip_like_the_cli() {
        use concurrent_psbt::output::OutputUniqueIdExt as _;

        // Strip the unique ids so the export looks like a foreign BIP 174.
        let ordered = ordered_with_output(1);
        let mut bare = parse_psbt_str("fixture", &ordered).unwrap();
        for output in &mut bare.outputs {
            output.proprietaries.clear();
        }
        let exported = export_bip174(&encode_psbt(&bare)).expect("export ok");
        let imported =
            import_bip174(exported["psbt"].as_str().unwrap(), true).expect("import ok");
        let imported_psbt = imported["psbt"].as_str().unwrap();

        // Previously failed here: no PSBT_OUT_UNIQUE_ID.
        let err = make_unordered(imported_psbt).unwrap_err();
        assert!(err.contains("PSBT_OUT_UNIQUE_ID"), "{err}");

        let assigned = assign_ids(dto::AssignIdsRequest {
            psbt: imported_psbt.to_string(),
            ids: vec![],
            auto: false,
            overwrite: false,
        })
        .expect("assign ids");
        let assigned_psbt = assigned["psbt"].as_str().unwrap();
        let parsed = parse_psbt_str("assigned", assigned_psbt).unwrap();
        assert!(parsed.outputs.iter().all(|output| output.has_unique_id()));

        let unordered = make_unordered(assigned_psbt).expect("make unordered");
        assert_eq!(unordered["inspect"]["ordering"], "unordered");

        // Idempotent second pass returns the identical PSBT.
        let again = assign_ids(dto::AssignIdsRequest {
            psbt: assigned_psbt.to_string(),
            ids: vec![],
            auto: false,
            overwrite: false,
        })
        .expect("assign ids again");
        assert_eq!(again["psbt"], assigned["psbt"]);
    }

    #[test]
    fn assign_ids_manual_directives_match_the_route_contract() {
        use concurrent_psbt::output::OutputUniqueIdExt as _;

        let ordered = ordered_with_output(2);
        let mut bare = parse_psbt_str("fixture", &ordered).unwrap();
        for output in &mut bare.outputs {
            output.proprietaries.clear();
        }
        let assigned = assign_ids(dto::AssignIdsRequest {
            psbt: encode_psbt(&bare),
            ids: vec![
                dto::IdAssignment {
                    target: "out".to_string(),
                    index: 0,
                    id: "0102030405060708090a0b0c0d0e0f10".to_string(),
                },
                dto::IdAssignment {
                    target: "in".to_string(),
                    index: 0,
                    id: "aa11".to_string(),
                },
            ],
            auto: false,
            overwrite: false,
        })
        .expect("assign ids");
        let parsed = parse_psbt_str("assigned", assigned["psbt"].as_str().unwrap()).unwrap();
        assert_eq!(
            parsed.outputs[0].unique_id().unwrap().into_bytes(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );
        assert_eq!(
            concurrent_psbt::removal::InputUniqueIdExt::unique_id(&parsed.inputs[0]),
            Some(vec![0xaa, 0x11]),
        );

        let err = assign_ids(dto::AssignIdsRequest {
            psbt: assigned["psbt"].as_str().unwrap().to_string(),
            ids: vec![dto::IdAssignment {
                target: "out".to_string(),
                index: 0,
                id: "ffff".to_string(),
            }],
            auto: false,
            overwrite: false,
        })
        .unwrap_err();
        assert!(err.contains("--overwrite"), "{err}");
    }

    #[test]
    fn export_bip174_rejects_unordered() {
        let created = created_with_output(2);
        assert!(export_bip174(&created).is_err());
    }

    #[test]
    fn concatenate_appends_two_ordered_psbts() {
        let a = ordered_with_output(1);
        let b = ordered_with_output(2);
        let v = concatenate(vec![a, b]).expect("concat ok");
        assert_eq!(v["inspect"]["ordering"], "ordered");
        assert_eq!(v["inspect"]["output_count"], 2);
    }

    #[test]
    fn concatenate_rejects_unordered() {
        let a = created_with_output(3);
        let b = created_with_output(4);
        assert!(concatenate(vec![a, b]).unwrap_err().contains("unordered"));
    }

    #[test]
    fn local_sync_folds_and_reports_empty_bands() {
        let a = created_psbt(4, 1);
        let v = local_sync(vec![a.clone(), a]).expect("sync ok");
        assert!(v["psbt"].as_str().is_some());
        assert_eq!(v["payments"], json!([]));
        assert_eq!(v["confirmations"], json!([]));
    }

    #[test]
    fn local_sync_requires_input() {
        let err = local_sync(vec![]).unwrap_err();
        assert!(err.contains("`psbts` or a network transport"), "{err}");
    }

    #[test]
    fn local_sync_join_is_idempotent() {
        let x = created_psbt(3, 1);
        let once = local_sync(vec![x.clone()]).unwrap();
        let twice = local_sync(vec![x.clone(), x]).unwrap();
        assert_eq!(once["inspect"]["input_count"], twice["inspect"]["input_count"]);
    }

    #[test]
    fn pay_then_payments_roundtrips_plaintext() {
        let psbt = empty_regtest_psbt();
        // Opaque 4-byte record; the core stores it verbatim (no secret).
        let paid = pay(dto::PayRequest {
            psbt,
            payment_hex: "deadbeef".to_string(),
            secret_hex: None,
            dummy: 0,
        })
        .expect("pay");
        let out = paid["psbt"].as_str().unwrap().to_string();
        let decoded = payments(dto::PaymentsRequest { psbt: out, secret_hex: None })
            .expect("payments");
        let entries = decoded["payments"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "deadbeef");
    }
}
