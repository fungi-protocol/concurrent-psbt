//! Field-level PSBT editing with save-time validation and structured fix
//! offers (the backend seam of the low-level fragment viewer/editor).
//!
//! Edits address raw keymap entries by the same handle `inspect` exposes
//! (`raw.*[].key_hex`: the full raw key, compact-size keytype prefix
//! included) and are GROW-ONLY: applying edits mints a NEW fragment, the
//! submitted PSBT is never mutated in place. The edited byte stream is
//! re-parsed before anything is returned, so a malformed edit can never mint
//! an unparseable fragment (that re-parse is constitutive, not an overridable
//! gate: unparseable bytes are not a fragment).
//!
//! Save-time validation returns structured [`Violation`]s instead of bare
//! errors. Every gate is strict by default and individually overridable
//! (`override_param`, the same explicit-override convention as
//! `allow_short_seed`); a violation MAY carry a [`Fix`] the caller can apply
//! server-side by naming its `fix_id` — the canonical case being
//! missing output unique ids on an unordered PSBT, fixed by the `assign-ids`
//! machinery while warning that repeating the auto-generation can mint
//! duplicate txouts.

use concurrent_psbt::global::GlobalSortExt as _;
use concurrent_psbt::output::{OutputUniqueIdExt as _, UniqueId};
use psbt_v2::v2::Psbt;

use crate::rawmap::{self, RawPair};
use crate::{Error, Result};

/// Which raw map an edit addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapTarget {
    Global,
    Input(usize),
    Output(usize),
}

impl MapTarget {
    /// Parse a map selector: `global` (or `g`), `input:<i>` / `in:<i>` /
    /// `i:<i>`, `output:<i>` / `out:<i>` / `o:<i>`.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("global") || value.eq_ignore_ascii_case("g") {
            return Ok(Self::Global);
        }
        let Some((kind, index)) = value.split_once(':') else {
            return Err(Error::new(format!(
                "invalid map selector {value}: expected `global`, `input:<i>`, or `output:<i>`"
            )));
        };
        let index: usize = index.trim().parse().map_err(|_| {
            Error::new(format!(
                "invalid map selector {value}: `{index}` is not a map index"
            ))
        })?;
        match kind.trim().to_ascii_lowercase().as_str() {
            "input" | "in" | "i" => Ok(Self::Input(index)),
            "output" | "out" | "o" => Ok(Self::Output(index)),
            other => Err(Error::new(format!(
                "invalid map selector {value}: unknown map `{other}` (expected `global`, \
                 `input:<i>`, or `output:<i>`)"
            ))),
        }
    }
}

impl std::fmt::Display for MapTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Global => write!(f, "global"),
            Self::Input(index) => write!(f, "input:{index}"),
            Self::Output(index) => write!(f, "output:{index}"),
        }
    }
}

/// One field edit: set (`value: Some`) or delete (`value: None`) the raw
/// entry addressed by `key` (full raw key bytes) in `map`.
#[derive(Debug, Clone)]
pub(crate) struct FieldEdit {
    pub(crate) map: MapTarget,
    pub(crate) key: Vec<u8>,
    pub(crate) value: Option<Vec<u8>>,
}

/// Apply `edits` in order and mint the resulting fragment. Setting an absent
/// key inserts it; setting a present key replaces its value; deleting an
/// absent key is an error (the edit did not do what it said).
pub(crate) fn apply_edits(psbt: &Psbt, edits: &[FieldEdit]) -> Result<Psbt> {
    let mut maps = rawmap::raw_maps(psbt)?;
    for (position, edit) in edits.iter().enumerate() {
        let map = match edit.map {
            MapTarget::Global => &mut maps.global,
            MapTarget::Input(index) => maps.inputs.get_mut(index).ok_or_else(|| {
                Error::new(format!(
                    "edits[{position}]: input index {index} out of range ({} inputs)",
                    psbt.inputs.len()
                ))
            })?,
            MapTarget::Output(index) => maps.outputs.get_mut(index).ok_or_else(|| {
                Error::new(format!(
                    "edits[{position}]: output index {index} out of range ({} outputs)",
                    psbt.outputs.len()
                ))
            })?,
        };
        let existing = map.iter().position(|pair| pair.key == edit.key);
        match (&edit.value, existing) {
            (Some(value), Some(index)) => map[index].value.clone_from(value),
            (Some(value), None) => map.push(RawPair {
                key: edit.key.clone(),
                value: value.clone(),
            }),
            (None, Some(index)) => {
                map.remove(index);
            }
            (None, None) => {
                return Err(Error::new(format!(
                    "edits[{position}]: key {} is not present in the {} map, so there is \
                     nothing to delete",
                    hex_encode(&edit.key),
                    edit.map,
                )));
            }
        }
    }
    // The constitutive re-parse. io::parse_psbt_bytes is the crate's single
    // panic boundary for BIP 370 parsing: psbt_v2 0.3.0 panics (todo!())
    // while DISPLAYING some deserialize errors, so the boundary formats the
    // error inside its own catch_unwind. A malformed edit is therefore a
    // clean 400, never a server panic.
    let bytes = rawmap::serialize_maps(&maps);
    crate::io::parse_psbt_bytes("edited psbt", &bytes)
}

/// A save-time validation violation. `override_param` names the request
/// boolean that waives this gate; `fix` (when present) is a server-side
/// repair the caller can request by `fix_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Violation {
    pub(crate) id: &'static str,
    pub(crate) message: String,
    pub(crate) override_param: &'static str,
    pub(crate) fix: Option<Fix>,
}

/// A structured fix offer attached to a violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fix {
    pub(crate) fix_id: &'static str,
    pub(crate) fix_label: &'static str,
    pub(crate) warning_text: &'static str,
}

pub(crate) const FIX_ASSIGN_IDS: &str = "assign-ids";

/// The duplicate-txout caveat that accompanies auto-generated unique ids —
/// part of the route contract, surfaced both in the fix OFFER and when the
/// fix is APPLIED.
pub(crate) const ASSIGN_IDS_WARNING: &str = "Automatically generating unique IDs may result in \
     duplicate txouts if done more than once.";

/// Save-time validation of a fragment about to be returned.
pub(crate) fn validate(psbt: &Psbt) -> Vec<Violation> {
    let mut violations = Vec::new();

    if psbt.global.is_unordered() {
        let missing = psbt
            .outputs
            .iter()
            .filter(|output| !output.has_unique_id())
            .count();
        if missing > 0 {
            violations.push(Violation {
                id: "unordered-missing-output-ids",
                message: format!(
                    "the PSBT is unordered but {missing} output{} lack{} PSBT_OUT_UNIQUE_ID; \
                     unordered PSBTs identify outputs by unique id",
                    if missing == 1 { "" } else { "s" },
                    if missing == 1 { "s" } else { "" },
                ),
                override_param: "allow_missing_output_ids",
                fix: Some(Fix {
                    fix_id: FIX_ASSIGN_IDS,
                    fix_label: "Generate missing output unique IDs",
                    warning_text: ASSIGN_IDS_WARNING,
                }),
            });
        }
    }

    let ids: Vec<Option<UniqueId>> = psbt
        .outputs
        .iter()
        .map(|output| output.unique_id())
        .collect();
    for (index, id) in ids.iter().enumerate() {
        let Some(id) = id else { continue };
        if let Some(other) = ids[..index]
            .iter()
            .position(|earlier| earlier.as_ref() == Some(id))
        {
            violations.push(Violation {
                id: "duplicate-output-ids",
                message: format!(
                    "outputs {other} and {index} carry the same PSBT_OUT_UNIQUE_ID \
                     ({}); unique ids must be universally unique",
                    hex_encode(id.as_bytes()),
                ),
                override_param: "allow_duplicate_output_ids",
                fix: None,
            });
        }
    }

    violations
}

/// Apply a fix by id (the implementation behind a [`Fix`] offer). Unknown
/// ids are an error naming the fixes this build knows.
pub(crate) fn apply_fix(psbt: Psbt, fix_id: &str) -> Result<Psbt> {
    match fix_id {
        FIX_ASSIGN_IDS => super::assign_ids::assign_ids_psbt(psbt, &[], true, false),
        other => Err(Error::new(format!(
            "unknown fix id `{other}` (known fixes: {FIX_ASSIGN_IDS})"
        ))),
    }
}

/// The warning text attached to applying `fix_id` (None when the fix carries
/// no caveat).
pub(crate) fn fix_warning(fix_id: &str) -> Option<&'static str> {
    (fix_id == FIX_ASSIGN_IDS).then_some(ASSIGN_IDS_WARNING)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn fixture_psbt() -> Psbt {
        crate::commands::create::create_psbt(crate::cli::CreateConfig {
            inputs: vec![crate::cli::OutPointArg {
                txid: "0000000000000000000000000000000000000000000000000000000000000001"
                    .parse()
                    .unwrap(),
                vout: 7,
            }],
            outputs: vec![],
            seed: None,
            allow_short_seed: false,
            ordering: crate::cli::OrderingArg::Unset,
            network: crate::cli::NetworkArg(bitcoin::Network::Regtest),
        })
        .unwrap()
    }

    #[test]
    fn map_target_parses_liberal_selectors() {
        assert_eq!(MapTarget::parse("global").unwrap(), MapTarget::Global);
        assert_eq!(MapTarget::parse(" G ").unwrap(), MapTarget::Global);
        assert_eq!(MapTarget::parse("input:3").unwrap(), MapTarget::Input(3));
        assert_eq!(MapTarget::parse("in:0").unwrap(), MapTarget::Input(0));
        assert_eq!(MapTarget::parse("out:2").unwrap(), MapTarget::Output(2));
        assert_eq!(MapTarget::parse("output:2").unwrap(), MapTarget::Output(2));
        assert!(
            MapTarget::parse("sideways:1")
                .unwrap_err()
                .to_string()
                .contains("sideways")
        );
        assert!(
            MapTarget::parse("input:x")
                .unwrap_err()
                .to_string()
                .contains("map index")
        );
        assert!(
            MapTarget::parse("global-ish")
                .unwrap_err()
                .to_string()
                .contains("expected")
        );
    }

    #[test]
    fn set_then_delete_round_trips_the_fragment() {
        let psbt = fixture_psbt();
        // An unknown global key (keytype 0xEF is unassigned).
        let key = vec![0xEF, 0x01];
        let set = FieldEdit {
            map: MapTarget::Global,
            key: key.clone(),
            value: Some(vec![0xAA, 0xBB]),
        };
        let edited = apply_edits(&psbt, &[set]).unwrap();
        assert_ne!(Psbt::serialize(&edited), Psbt::serialize(&psbt));
        assert_eq!(
            edited.global.unknowns.values().next(),
            Some(&vec![0xAA, 0xBB])
        );

        let delete = FieldEdit {
            map: MapTarget::Global,
            key,
            value: None,
        };
        let restored = apply_edits(&edited, &[delete]).unwrap();
        assert_eq!(Psbt::serialize(&restored), Psbt::serialize(&psbt));
    }

    #[test]
    fn deleting_an_absent_key_names_map_and_key() {
        let psbt = fixture_psbt();
        let error = apply_edits(
            &psbt,
            &[FieldEdit {
                map: MapTarget::Input(0),
                key: vec![0xEF],
                value: None,
            }],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("input:0"), "{error}");
        assert!(error.contains("ef"), "{error}");
    }

    #[test]
    fn out_of_range_maps_are_named() {
        let psbt = fixture_psbt();
        let error = apply_edits(
            &psbt,
            &[FieldEdit {
                map: MapTarget::Output(4),
                key: vec![0xEF],
                value: Some(vec![0x01]),
            }],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("output index 4 out of range"), "{error}");
    }

    #[test]
    fn edits_that_break_the_psbt_are_rejected_at_reparse() {
        let psbt = fixture_psbt();
        // Deleting the global version field makes the stream unparseable —
        // the constitutive re-parse rejects it.
        let version_key = vec![0xFB];
        let error = apply_edits(
            &psbt,
            &[FieldEdit {
                map: MapTarget::Global,
                key: version_key,
                value: None,
            }],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("edited psbt"), "{error}");
    }

    #[test]
    fn validate_flags_unordered_without_output_ids_with_the_assign_ids_fix() {
        let mut psbt = crate::commands::create::create_psbt(crate::cli::CreateConfig {
            inputs: vec![],
            outputs: vec![crate::cli::OutputArg {
                address_text: address(),
                address: address().parse().unwrap(),
                amount: bitcoin::Amount::from_sat(1000),
            }],
            seed: None,
            allow_short_seed: false,
            ordering: crate::cli::OrderingArg::Unset,
            network: crate::cli::NetworkArg(bitcoin::Network::Regtest),
        })
        .unwrap();
        assert!(validate(&psbt).is_empty());

        for output in &mut psbt.outputs {
            output.proprietaries.clear();
        }
        let violations = validate(&psbt);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].id, "unordered-missing-output-ids");
        assert_eq!(violations[0].override_param, "allow_missing_output_ids");
        let fix = violations[0].fix.as_ref().unwrap();
        assert_eq!(fix.fix_id, FIX_ASSIGN_IDS);
        assert!(fix.warning_text.contains("duplicate txouts"));

        // The offered fix clears the violation.
        let fixed = apply_fix(psbt, FIX_ASSIGN_IDS).unwrap();
        assert!(validate(&fixed).is_empty());
    }

    #[test]
    fn validate_flags_duplicate_output_ids() {
        use concurrent_psbt::output::OutputUniqueIdExt as _;
        let mut psbt = crate::commands::create::create_psbt(crate::cli::CreateConfig {
            inputs: vec![],
            outputs: vec![
                crate::cli::OutputArg {
                    address_text: address(),
                    address: address().parse().unwrap(),
                    amount: bitcoin::Amount::from_sat(1000),
                },
                crate::cli::OutputArg {
                    address_text: address(),
                    address: address().parse().unwrap(),
                    amount: bitcoin::Amount::from_sat(2000),
                },
            ],
            seed: None,
            allow_short_seed: false,
            ordering: crate::cli::OrderingArg::Unset,
            network: crate::cli::NetworkArg(bitcoin::Network::Regtest),
        })
        .unwrap();
        let id = UniqueId::new(vec![0x11; 16]);
        for output in &mut psbt.outputs {
            output.set_unique_id(id.clone());
        }
        let violations = validate(&psbt);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].id, "duplicate-output-ids");
        assert_eq!(violations[0].override_param, "allow_duplicate_output_ids");
        assert!(violations[0].fix.is_none());
        assert!(violations[0].message.contains("outputs 0 and 1"));
    }

    #[test]
    fn unknown_fix_ids_are_named() {
        let error = apply_fix(fixture_psbt(), "reticulate-splines")
            .unwrap_err()
            .to_string();
        assert!(error.contains("reticulate-splines"), "{error}");
        assert!(error.contains(FIX_ASSIGN_IDS), "{error}");
    }

    fn address() -> String {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let secret = bitcoin::secp256k1::SecretKey::from_slice(&[7; 32]).unwrap();
        let public_key = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret);
        let public_key = bitcoin::CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        bitcoin::Address::p2wpkh(&public_key, bitcoin::Network::Regtest).to_string()
    }
}
