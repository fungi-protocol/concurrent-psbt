#![allow(clippy::result_large_err)]

//! Input/output removal as grow-only tombstone sets riding the global
//! proprietary map in the negotiation band.
//!
//! Mirror of `payments/negotiation.rs` (subtypes `0x20`/`0x21`), extending the same
//! grow-only global-proprietary-set idiom into two new tombstone subtypes:
//!
//! - `PSBT_GLOBAL_REMOVED_INPUT` (subtype `0x23`): a **tombstone set**. Each
//!   entry's *keydata* is the removed input's identity (its outpoint bytes,
//!   optionally suffixed by `PSBT_IN_UNIQUE_ID`); the *valuedata* is empty.
//! - `PSBT_GLOBAL_REMOVED_OUTPUT` (subtype `0x24`): the output analogue. Each
//!   entry's keydata is the removed output's `PSBT_OUT_UNIQUE_ID` bytes; the
//!   valuedata is empty.
//!
//! (The explicit-fee-contribution field `0x22` is a sibling of these in the same
//! negotiation band, but lives in [`crate::fee`]; the removal band strips it
//! alongside its own tombstones — see [`GlobalRemovalExt::clear_removal_and_fee`].)
//!
//! The removal sets together form a two-phase set (2P-Set): the *presence* set
//! is the existing [`InputSet`](crate::input::InputSet) /
//! [`OutputSet`](crate::output::OutputSet), and the *tombstone* set is the
//! subtype-`0x23`/`0x24` proprietary keys. A **live** element is one that is
//! present-and-not-tombstoned. Removal is **never** a destructive delete: the
//! element map is left untouched during join, and the live set is a
//! *projection* computed at read/serialize time (see [`retain_live_inputs`],
//! [`retain_live_outputs`], and the sorter/`into_psbt` projection).
//!
//! # CRDT laws
//!
//! Every field here rides `Global.proprietaries: BTreeMap<ProprietaryKey,
//! Vec<u8>>`, whose [`JoinMut`](crate::JoinMut) unions disjoint keys and joins
//! matching keys through the crate-private `IdempotentValue` on `Vec<u8>`
//! (equal → merge, unequal → conflict). Because each tombstone is keyed by a
//! distinct removed-element id and carries an empty (hence always-equal) value,
//! the tombstone set is a **grow-only set (G-Set)** under that union:
//! idempotent, commutative, associative, and monotone. The 2P-Set is the pair
//! (G-Set presence, G-Set tombstones); its join is the componentwise join of
//! two join-semilattices, so it inherits all three laws. See
//! `removal_laws.rs` for the machine-checked proptests and `LAWS.md` for the
//! written argument.
//!
//! Removal is monotone in the *data* (tombstones only accumulate) but, per the
//! spec, **not** monotone in *transaction effects* — a removed element changes
//! what is signed. This is safe only under `SIGHASH_ALL`, which every signer in
//! this protocol is required to use.

use bitcoin::OutPoint;
use psbt_v2::raw;
use psbt_v2::v2::{Global, Input, Output};

use crate::input::out_point;
use crate::output::{OutputUniqueIdExt, UniqueId};

/// Subtype for `PSBT_GLOBAL_REMOVED_INPUT`.
///
/// Tombstone set: keydata = the removed input id (outpoint bytes, optionally
/// suffixed by `PSBT_IN_UNIQUE_ID`), valuedata = empty.
pub const PSBT_GLOBAL_REMOVED_INPUT_SUBTYPE: u8 = 0x23;

/// Subtype for `PSBT_GLOBAL_REMOVED_OUTPUT`.
///
/// Tombstone set: keydata = the removed output's `PSBT_OUT_UNIQUE_ID` bytes,
/// valuedata = empty.
pub const PSBT_GLOBAL_REMOVED_OUTPUT_SUBTYPE: u8 = 0x24;

/// Subtype for `PSBT_IN_UNIQUE_ID` (per-input).
///
/// Optional suffix appended to an input's outpoint to distinguish a
/// removed-then-re-added input from its predecessor. `0x10` is already
/// `PSBT_IN_SORT_KEY`, so this takes the next free per-input subtype.
pub const PSBT_IN_UNIQUE_ID_SUBTYPE: u8 = 0x11;

/// Empty tombstone value. Removal fields carry no value data (spec), so the
/// blob is always this constant, guaranteeing the [`IdempotentValue`] join of
/// two tombstones for the same id is trivially `Ok` (equal).
///
/// [`IdempotentValue`]: crate::values
const TOMBSTONE_VALUE: &[u8] = &[];

// ── key builders ────────────────────────────────────────────────────────────

/// A proprietary key with arbitrary keydata bytes in the negotiation band.
fn keyed(subtype: u8, keydata: Vec<u8>) -> raw::ProprietaryKey {
    raw::ProprietaryKey {
        prefix: crate::PROPRIETARY_PREFIX.to_vec(),
        subtype,
        key: keydata,
    }
}

// ── input identity for removal ──────────────────────────────────────────────

/// The removal-comparison identity of an input.
///
/// Because inputs are keyed by outpoint alone in [`InputSet`], but the spec
/// allows a removed input to be re-added with different data, the identity used
/// for the tombstone set is `outpoint_bytes || PSBT_IN_UNIQUE_ID?` — the
/// 36-byte outpoint (txid ‖ vout LE) optionally suffixed by the input's
/// `PSBT_IN_UNIQUE_ID` value. An input with no unique id has the bare outpoint
/// as its id.
///
/// [`InputSet`]: crate::input::InputSet
pub fn input_removal_id(input: &Input) -> Vec<u8> {
    let op = out_point(input);
    let mut id = Vec::with_capacity(36 + 16);
    id.extend_from_slice(op.txid.as_ref());
    id.extend_from_slice(&op.vout.to_le_bytes());
    if let Some(uid) = input.unique_id() {
        id.extend_from_slice(&uid);
    }
    id
}

/// The removal-comparison identity of a bare [`OutPoint`] (no unique-id suffix).
///
/// Used by [`GlobalRemovalExt::remove_outpoint`] callers who only have an
/// outpoint to hand.
pub fn outpoint_removal_id(op: &OutPoint) -> Vec<u8> {
    let mut id = Vec::with_capacity(36);
    id.extend_from_slice(op.txid.as_ref());
    id.extend_from_slice(&op.vout.to_le_bytes());
    id
}

/// Extension trait on [`Input`] for the optional `PSBT_IN_UNIQUE_ID` field.
pub trait InputUniqueIdExt {
    /// Return the input's unique-id suffix, if set.
    fn unique_id(&self) -> Option<Vec<u8>>;
    /// Set the unique-id suffix.
    fn set_unique_id(&mut self, id: Vec<u8>);
}

impl InputUniqueIdExt for Input {
    fn unique_id(&self) -> Option<Vec<u8>> {
        self.proprietaries
            .get(&keyed(PSBT_IN_UNIQUE_ID_SUBTYPE, vec![]))
            .cloned()
    }

    fn set_unique_id(&mut self, id: Vec<u8>) {
        self.proprietaries
            .insert(keyed(PSBT_IN_UNIQUE_ID_SUBTYPE, vec![]), id);
    }
}

// ── the removal extension trait ─────────────────────────────────────────────

/// Extension trait on [`Global`] for the removal tombstone sets.
///
/// All methods are pure reads or grow-only inserts on `Global.proprietaries`;
/// no method ever removes a presence element or a tombstone. Removal is
/// realized as a *projection* (`retain_live_*`, `is_*_removed`) rather than a
/// delete.
pub trait GlobalRemovalExt {
    // ── removed-input tombstone set (0x23) ──
    /// Every removed-input tombstone as raw id bytes (keydata).
    fn removed_inputs(&self) -> Vec<Vec<u8>>;
    /// Tombstone an input by its removal id (see [`input_removal_id`]).
    /// Idempotent: re-tombstoning the same id is a no-op merge.
    fn remove_input_id(&mut self, id: Vec<u8>);
    /// Tombstone an input by its (unique-id-suffixed) identity.
    fn remove_input(&mut self, input: &Input);
    /// Tombstone an input by bare outpoint (no unique-id suffix).
    fn remove_outpoint(&mut self, op: &OutPoint);
    /// `true` if the given removal id is tombstoned.
    fn is_input_id_removed(&self, id: &[u8]) -> bool;
    /// `true` if this input's identity is tombstoned.
    fn is_input_removed(&self, input: &Input) -> bool;

    // ── removed-output tombstone set (0x24) ──
    /// Every removed-output tombstone as a [`UniqueId`].
    fn removed_outputs(&self) -> Vec<UniqueId>;
    /// Tombstone an output by its unique id. Idempotent.
    fn remove_output_id(&mut self, id: &UniqueId);
    /// Tombstone an output (must carry a `PSBT_OUT_UNIQUE_ID`); no-op if absent.
    fn remove_output(&mut self, output: &Output);
    /// `true` if the given output unique id is tombstoned.
    fn is_output_removed(&self, id: &UniqueId) -> bool;

    /// `true` if any removal tombstone (input or output) is present. Used by
    /// the fail-safe gate when removal support is compiled out.
    fn signals_removal(&self) -> bool;

    /// Strip the removal band and the sibling fee band (`0x22`) before signing,
    /// mirroring `clear_negotiation`. Called alongside it in the sorter.
    ///
    /// Tombstones and fee declarations are negotiation metadata; once the live
    /// set is materialized they must not leak into the signed PSBT.
    fn clear_removal_and_fee(&mut self);
}

impl GlobalRemovalExt for Global {
    fn removed_inputs(&self) -> Vec<Vec<u8>> {
        self.proprietaries
            .keys()
            .filter(|k| {
                k.prefix == crate::PROPRIETARY_PREFIX
                    && k.subtype == PSBT_GLOBAL_REMOVED_INPUT_SUBTYPE
            })
            .map(|k| k.key.clone())
            .collect()
    }

    fn remove_input_id(&mut self, id: Vec<u8>) {
        self.proprietaries.insert(
            keyed(PSBT_GLOBAL_REMOVED_INPUT_SUBTYPE, id),
            TOMBSTONE_VALUE.to_vec(),
        );
    }

    fn remove_input(&mut self, input: &Input) {
        self.remove_input_id(input_removal_id(input));
    }

    fn remove_outpoint(&mut self, op: &OutPoint) {
        self.remove_input_id(outpoint_removal_id(op));
    }

    fn is_input_id_removed(&self, id: &[u8]) -> bool {
        self.proprietaries
            .contains_key(&keyed(PSBT_GLOBAL_REMOVED_INPUT_SUBTYPE, id.to_vec()))
    }

    fn is_input_removed(&self, input: &Input) -> bool {
        self.is_input_id_removed(&input_removal_id(input))
    }

    fn removed_outputs(&self) -> Vec<UniqueId> {
        self.proprietaries
            .keys()
            .filter(|k| {
                k.prefix == crate::PROPRIETARY_PREFIX
                    && k.subtype == PSBT_GLOBAL_REMOVED_OUTPUT_SUBTYPE
            })
            .map(|k| UniqueId::new(k.key.clone()))
            .collect()
    }

    fn remove_output_id(&mut self, id: &UniqueId) {
        self.proprietaries.insert(
            keyed(PSBT_GLOBAL_REMOVED_OUTPUT_SUBTYPE, id.as_bytes().to_vec()),
            TOMBSTONE_VALUE.to_vec(),
        );
    }

    fn remove_output(&mut self, output: &Output) {
        if let Some(id) = OutputUniqueIdExt::unique_id(output) {
            self.remove_output_id(&id);
        }
    }

    fn is_output_removed(&self, id: &UniqueId) -> bool {
        self.proprietaries.contains_key(&keyed(
            PSBT_GLOBAL_REMOVED_OUTPUT_SUBTYPE,
            id.as_bytes().to_vec(),
        ))
    }

    fn signals_removal(&self) -> bool {
        self.proprietaries.keys().any(|k| {
            k.prefix == crate::PROPRIETARY_PREFIX
                && matches!(
                    k.subtype,
                    PSBT_GLOBAL_REMOVED_INPUT_SUBTYPE | PSBT_GLOBAL_REMOVED_OUTPUT_SUBTYPE
                )
        })
    }

    fn clear_removal_and_fee(&mut self) {
        self.proprietaries.retain(|k, _| {
            !(k.prefix == crate::PROPRIETARY_PREFIX
                && matches!(
                    k.subtype,
                    crate::fee::PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE
                        | PSBT_GLOBAL_REMOVED_INPUT_SUBTYPE
                        | PSBT_GLOBAL_REMOVED_OUTPUT_SUBTYPE
                ))
        });
    }
}

// ── live-set projection helpers ─────────────────────────────────────────────

/// Whether removal handling is compiled in. Removal support is optional and
/// fail-safe: an implementation that ignores these fields silently is safe, one
/// that fails is safer. When the `removal` feature is off, the projection
/// helpers below are the identity (they retain every element) and the join-time
/// gate ([`reject_if_removal_disabled`]) is the sole place that reacts to the
/// presence of tombstones.
#[cfg(feature = "removal")]
pub const REMOVAL_ENABLED: bool = true;
#[cfg(not(feature = "removal"))]
pub const REMOVAL_ENABLED: bool = false;

/// Project the live inputs out of a joined `(global, inputs)` pair: drop any
/// input whose removal id is tombstoned. When `removal` is disabled this is the
/// identity (tombstones are ignored, which is fail-safe).
pub fn retain_live_inputs(global: &Global, inputs: &mut Vec<Input>) {
    if !REMOVAL_ENABLED {
        return;
    }
    inputs.retain(|input| !global.is_input_removed(input));
}

/// Project the live outputs: drop any output whose `PSBT_OUT_UNIQUE_ID` is
/// tombstoned. Outputs with no unique id are never tombstoned (they can't be
/// named in `PSBT_GLOBAL_REMOVED_OUTPUT`) and are always retained.
pub fn retain_live_outputs(global: &Global, outputs: &mut Vec<Output>) {
    if !REMOVAL_ENABLED {
        return;
    }
    outputs.retain(|output| match OutputUniqueIdExt::unique_id(output) {
        Some(id) => !global.is_output_removed(&id),
        None => true,
    });
}

/// Fail-safe gate. When removal support is compiled out, a peer that signals
/// removal must be rejected (or, per spec, at minimum warned). Callers that
/// join a foreign `Global` should call this and propagate the error.
///
/// # Errors
/// Returns `Err` when `removal` is disabled and `global` carries any removal
/// tombstone.
#[cfg(not(feature = "removal"))]
pub fn reject_if_removal_disabled(global: &Global) -> Result<(), RemovalUnsupported> {
    if global.signals_removal() {
        Err(RemovalUnsupported)
    } else {
        Ok(())
    }
}

/// With `removal` enabled the gate is a no-op (always `Ok`).
#[cfg(feature = "removal")]
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn reject_if_removal_disabled(_global: &Global) -> Result<(), RemovalUnsupported> {
    Ok(())
}

/// Error returned by [`reject_if_removal_disabled`] when a removal-signaling
/// PSBT is combined by an implementation that omits removal support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalUnsupported;

impl std::fmt::Display for RemovalUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PSBT signals input/output removal but this build omits removal support \
             (rebuild with --features removal, or combining is refused fail-safe)"
        )
    }
}

impl std::error::Error for RemovalUnsupported {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::fee::GlobalFeeExt;
    use crate::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;
    use bitcoin::hashes::Hash as _;

    fn make_input(txid_byte: u8, vout: u32) -> Input {
        Input::new(&OutPoint {
            txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
            vout,
        })
    }

    fn output_with_uid(uid: &[u8]) -> Output {
        let mut output = Output::default();
        output.proprietaries.insert(
            raw::ProprietaryKey {
                prefix: crate::PROPRIETARY_PREFIX.to_vec(),
                subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                key: vec![],
            },
            uid.to_vec(),
        );
        output
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn input_removal_id_includes_unique_id_suffix() {
            let base = make_input(1, 0);
            let mut suffixed = make_input(1, 0);
            suffixed.set_unique_id(vec![0xaa, 0xbb]);
            // Same outpoint, different unique-id suffix → distinct removal ids.
            assert_ne!(input_removal_id(&base), input_removal_id(&suffixed));
            // Bare-outpoint id equals the no-suffix input's id.
            assert_eq!(
                input_removal_id(&base),
                outpoint_removal_id(&out_point(&base))
            );
        }

        #[test]
        fn tombstone_input_is_detected() {
            let mut g = Global::default();
            let input = make_input(2, 1);
            assert!(!g.is_input_removed(&input));
            g.remove_input(&input);
            assert!(g.is_input_removed(&input));
            assert_eq!(g.removed_inputs().len(), 1);
        }

        #[test]
        fn tombstone_input_is_idempotent() {
            let mut g = Global::default();
            let input = make_input(2, 1);
            g.remove_input(&input);
            g.remove_input(&input);
            assert_eq!(g.removed_inputs().len(), 1);
        }

        #[test]
        fn remove_outpoint_tombstones_bare_id() {
            let mut g = Global::default();
            let input = make_input(3, 2);
            g.remove_outpoint(&out_point(&input));
            assert!(g.is_input_removed(&input));
        }

        #[test]
        fn tombstone_output_is_detected_and_idempotent() {
            let mut g = Global::default();
            let out = output_with_uid(&[7; 16]);
            let id = OutputUniqueIdExt::unique_id(&out).unwrap();
            assert!(!g.is_output_removed(&id));
            g.remove_output(&out);
            g.remove_output(&out);
            assert!(g.is_output_removed(&id));
            assert_eq!(g.removed_outputs().len(), 1);
        }

        #[test]
        fn remove_output_without_uid_is_noop() {
            let mut g = Global::default();
            g.remove_output(&Output::default());
            assert!(g.removed_outputs().is_empty());
        }

        #[test]
        fn signals_removal_reflects_tombstones() {
            let mut g = Global::default();
            assert!(!g.signals_removal());
            g.add_fee_contribution([0u8; 16], vec![0u8; 8]); // fee alone does NOT signal removal
            assert!(!g.signals_removal());
            g.remove_output(&output_with_uid(&[3; 16]));
            assert!(g.signals_removal());
        }

        #[test]
        fn clear_removal_and_fee_strips_band() {
            let mut g = Global::default();
            g.add_fee_contribution([0u8; 16], vec![1u8; 8]);
            g.remove_input(&make_input(1, 0));
            g.remove_output(&output_with_uid(&[3; 16]));
            g.clear_removal_and_fee();
            assert!(g.fee_contributions().is_empty());
            assert!(g.removed_inputs().is_empty());
            assert!(g.removed_outputs().is_empty());
        }

        #[test]
        fn clear_removal_and_fee_leaves_negotiation_band() {
            use crate::payments::negotiation::GlobalNegotiationExt;
            let mut g = Global::default();
            g.add_payment([9u8; 16], vec![0u8]);
            g.remove_input(&make_input(1, 0));
            g.clear_removal_and_fee();
            // Payment band (0x20) is untouched by removal/fee stripping.
            assert_eq!(g.payments().len(), 1);
            assert!(g.removed_inputs().is_empty());
        }

        #[test]
        fn input_unique_id_roundtrip() {
            let mut input = make_input(1, 0);
            assert!(input.unique_id().is_none());
            input.set_unique_id(vec![0xde, 0xad]);
            assert_eq!(input.unique_id(), Some(vec![0xde, 0xad]));
        }

        #[test]
        fn projection_drops_tombstoned_input() {
            let mut g = Global::default();
            let live = make_input(1, 0);
            let dead = make_input(2, 0);
            g.remove_input(&dead);
            let mut inputs = vec![live.clone(), dead];
            retain_live_inputs(&g, &mut inputs);
            if REMOVAL_ENABLED {
                assert_eq!(inputs.len(), 1);
                assert_eq!(out_point(&inputs[0]), out_point(&live));
            } else {
                assert_eq!(inputs.len(), 2);
            }
        }

        #[test]
        fn projection_drops_tombstoned_output() {
            let mut g = Global::default();
            let live = output_with_uid(&[1; 16]);
            let dead = output_with_uid(&[2; 16]);
            g.remove_output(&dead);
            let mut outputs = vec![live, dead];
            retain_live_outputs(&g, &mut outputs);
            if REMOVAL_ENABLED {
                assert_eq!(outputs.len(), 1);
            } else {
                assert_eq!(outputs.len(), 2);
            }
        }

        #[test]
        fn projection_keeps_output_without_uid() {
            let g = Global::default();
            let mut outputs = vec![Output::default()];
            retain_live_outputs(&g, &mut outputs);
            assert_eq!(outputs.len(), 1);
        }

        #[cfg(not(feature = "removal"))]
        #[test]
        fn gate_rejects_removal_when_disabled() {
            let mut g = Global::default();
            g.remove_input(&make_input(1, 0));
            assert_eq!(reject_if_removal_disabled(&g), Err(RemovalUnsupported));
            assert!(!RemovalUnsupported.to_string().is_empty());
        }

        #[cfg(feature = "removal")]
        #[test]
        fn gate_allows_removal_when_enabled() {
            let mut g = Global::default();
            g.remove_input(&make_input(1, 0));
            assert!(reject_if_removal_disabled(&g).is_ok());
        }
    }
}
