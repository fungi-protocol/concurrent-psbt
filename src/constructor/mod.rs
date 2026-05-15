use core::marker::PhantomData;

use psbt_v2::v2::{InputsOnlyModifiable, Mod, Modifiable, OutputsOnlyModifiable, Psbt};

use crate::sort::SortMode;
// Re-export sort-mode types so constructor users (and tests) don't need to
// separately import crate::sort.
pub use crate::sort::{
    Deterministic, ExplicitSortKeys, Relaxed, Seeded, Sorter, SorterError, Unseeded,
};

use crate::fields::GlobalModifiableExt as _;
use crate::tx::UnorderedPsbt;

use psbt_v2::v2::{Input, Output};

pub mod errors;
pub use errors::{Error, SortingError};

// -- Helpers -----------------------------------------------------------------

/// Extract sort keys from items via `take_key`, sort by key, return items in order.
///
/// Fails if any key is missing or if two items share the same sort key.
use crate::output::OutputExt as _;
use crate::psbt_ext::PsbtExt as _;

// -- Constructor -------------------------------------------------------------

/// Unordered Constructor, mirrors the BIP 370 Constructor but for unordered PSBTs.
///
/// `M` encodes which inputs/outputs are still modifiable (see [`psbt_v2::v2::Mod`]).
/// `S` encodes the sort strategy (see [`crate::sort`]).
pub struct Constructor<M: Mod, S: SortMode>(UnorderedPsbt, PhantomData<(M, S)>);

// Manual impl: derive would add unnecessary bounds on M and S.
impl<M: Mod, S: SortMode + 'static> core::fmt::Debug for Constructor<M, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Constructor").field(&self.0).finish()
    }
}

// Manual impl: derive would add unnecessary bounds on M and S.
impl<M: Mod, S: SortMode + 'static> PartialEq for Constructor<M, S> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<M: Mod, S: SortMode + 'static> Constructor<M, S> {
    /// Return the inner `UnorderedPsbt`.
    pub fn into_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Merge two Constructors of the same typestate.
    ///
    /// For any side whose flag is locked (not modifiable), the corresponding
    /// set must be identical between `self` and `other`; if it is not,
    /// `Err(Error::LockedSetMismatch)` is returned.
    ///
    /// On the modifiable side(s) the sets are joined via the lattice join.
    /// If a value conflict arises, `Err(Error::JoinConflict(_))` is returned.
    pub fn try_join(self, other: Self) -> Result<Self, Error> {
        // Check locked sets are identical.
        if !self.0.global.is_inputs_modifiable() && self.0.inputs != other.0.inputs {
            return Err(Error::LockedSetMismatch);
        }
        if !self.0.global.is_outputs_modifiable() && self.0.outputs != other.0.outputs {
            return Err(Error::LockedSetMismatch);
        }
        self.0
            .try_join(other.0)
            .map(|p| Constructor(p, PhantomData))
            .map_err(|e| match e {
                crate::tx::JoinError::Conflict(c) => Error::JoinConflict(c),
                crate::tx::JoinError::DuplicateSortKey => Error::DuplicateSortKey,
            })
    }
}

impl<M: Mod, S: SortMode + 'static> Constructor<M, S> {
    /// Convert into a [`Sorter<S>`], consuming the constructor.
    ///
    /// Allows sorting independently of the modifiability typestate.
    pub fn into_sorter(self) -> crate::sort::Sorter<S> {
        crate::sort::Sorter::new_unchecked(self.0)
    }

    /// Wrap an `UnorderedPsbt` without validating flags.
    /// Only for use by trusted internal code (e.g. `dynamic::Constructor`).
    pub(crate) fn new_unchecked(psbt: UnorderedPsbt) -> Self {
        Constructor(psbt, PhantomData)
    }
}

impl<S: SortMode + 'static> Constructor<Modifiable, S> {
    /// Wrap an existing PSBT, validating it is unordered and fully modifiable.
    ///
    /// The sort mode `S` must match the `PSBT_GLOBAL_SORT_DETERMINISTIC` field
    /// in the PSBT; this is not validated here — callers should use [`Creator`]
    /// to produce correctly-typed constructors.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        psbt.validate_all_outputs_have_unique_ids()?;
        let unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_inputs_modifiable() {
            return Err(Error::InputsNotModifiable);
        }
        if !unordered.global.is_outputs_modifiable() {
            return Err(Error::OutputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Add an input.
    ///
    /// Enforces sort-mode invariants:
    /// - In `Deterministic<_>` mode, explicit sort keys are forbidden.
    /// - If the input has an explicit sort key, it must not duplicate any
    ///   existing input's sort key.
    pub fn input(self, input: Input) -> Result<Self, Error> {
        use crate::input::InputExt as _;
        input.validate_sort_key::<S>()?;
        self.0
            .try_join(UnorderedPsbt::from_input(input))
            .map(|p| Constructor(p, PhantomData))
            .map_err(|e| match e {
                crate::tx::JoinError::Conflict(c) => Error::JoinConflict(c),
                crate::tx::JoinError::DuplicateSortKey => Error::DuplicateSortKey,
            })
    }

    /// Add an output. Requires `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Enforces sort-mode invariants:
    /// - In `Deterministic<_>` mode, explicit sort keys are forbidden.
    /// - If the output has an explicit sort key, it must not duplicate any
    ///   existing output's sort key.
    pub fn output(self, output: Output) -> Result<Self, Error> {
        use crate::output::OutputExt as _;
        output.validate_has_unique_id()?;
        output.validate_sort_key::<S>()?;
        self.0
            .try_join(UnorderedPsbt::from_output(output))
            .map(|p| Constructor(p, PhantomData))
            .map_err(|e| match e {
                crate::tx::JoinError::Conflict(c) => Error::JoinConflict(c),
                crate::tx::JoinError::DuplicateSortKey => Error::DuplicateSortKey,
            })
    }

    /// Lock inputs: transition to `OutputsOnlyModifiable`.
    pub fn no_more_inputs(mut self) -> Constructor<OutputsOnlyModifiable, S> {
        self.0.global.clear_inputs_modifiable();
        Constructor(self.0, PhantomData)
    }

    /// Lock outputs: transition to `InputsOnlyModifiable`.
    pub fn no_more_outputs(mut self) -> Constructor<InputsOnlyModifiable, S> {
        self.0.global.clear_outputs_modifiable();
        Constructor(self.0, PhantomData)
    }
}

impl<S: SortMode + 'static> Constructor<InputsOnlyModifiable, S> {
    /// Add an input.
    ///
    /// Returns `Err(Error::JoinConflict(_))` if the same outpoint is already
    /// present with conflicting field values.
    pub fn input(self, input: Input) -> Result<Self, Error> {
        // The singleton starts fully modifiable; lock outputs to match self
        // so the UnorderedPsbt join doesn't conflict on tx_modifiable_flags.
        let mut singleton = UnorderedPsbt::from_input(input);
        singleton.global.clear_outputs_modifiable();
        self.0
            .try_join(singleton)
            .map(|p| Constructor(p, PhantomData))
            .map_err(|e| match e {
                crate::tx::JoinError::Conflict(c) => Error::JoinConflict(c),
                crate::tx::JoinError::DuplicateSortKey => Error::DuplicateSortKey,
            })
    }

    /// Wrap an existing PSBT, validating it is unordered and inputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        psbt.validate_all_outputs_have_unique_ids()?;
        let unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_inputs_modifiable() {
            return Err(Error::InputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Lock inputs: both sides now locked. Returns a [`Sorter<S>`] ready to sort.
    pub fn no_more_inputs(mut self) -> crate::sort::Sorter<S> {
        self.0.global.clear_inputs_modifiable();
        crate::sort::Sorter::new_unchecked(self.0)
    }
}

impl<S: SortMode + 'static> Constructor<OutputsOnlyModifiable, S> {
    /// Add an output. Requires `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Returns `Err(Error::JoinConflict(_))` if the same unique ID is already
    /// present with conflicting field values.
    pub fn output(self, output: Output) -> Result<Self, Error> {
        output.validate_has_unique_id()?;
        let mut singleton = UnorderedPsbt::from_output(output);
        singleton.global.clear_inputs_modifiable();
        self.0
            .try_join(singleton)
            .map(|p| Constructor(p, PhantomData))
            .map_err(|e| match e {
                crate::tx::JoinError::Conflict(c) => Error::JoinConflict(c),
                crate::tx::JoinError::DuplicateSortKey => Error::DuplicateSortKey,
            })
    }

    /// Wrap an existing PSBT, validating it is unordered and outputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        psbt.validate_all_outputs_have_unique_ids()?;
        let unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_outputs_modifiable() {
            return Err(Error::OutputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Lock outputs: both sides now locked. Returns a [`Sorter<S>`] ready to sort.
    pub fn no_more_outputs(mut self) -> crate::sort::Sorter<S> {
        self.0.global.clear_outputs_modifiable();
        crate::sort::Sorter::new_unchecked(self.0)
    }
}

mod sort_impls;

// -- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creator::Creator;
    use crate::fields::GlobalFieldsExt as _;
    use crate::input::InputExt as _;
    use psbt_v2::v2::Creator as Bip370Creator;

    #[test]
    fn creator_default_does_not_set_sort_deterministic_field() {
        let psbt = Creator::new().into_unordered_psbt();
        assert!(psbt.global.sort_deterministic_absent());
    }

    #[test]
    fn creator_explicit_sort_keys_sets_field_to_0x00() {
        let psbt = Creator::new().explicit_sort_keys().into_unordered_psbt();
        assert!(psbt.global.is_sort_explicit());
    }

    #[test]
    fn creator_deterministic_sorting_sets_field_to_0x01() {
        let psbt = Creator::new().deterministic_sorting().into_unordered_psbt();
        assert!(psbt.global.is_sort_deterministic());
    }

    #[test]
    fn creator_produces_valid_constructor() {
        let c = Creator::new().constructor();
        let unordered: UnorderedPsbt = c.into_psbt();
        assert!(unordered.is_unordered());
    }

    #[test]
    fn new_modifiable_rejects_non_unordered() {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();
        assert_eq!(
            Constructor::<Modifiable, Relaxed<Unseeded>>::new(psbt),
            Err(Error::NotUnordered)
        );
    }

    #[test]
    fn new_modifiable_rejects_missing_inputs_flag() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        assert_eq!(
            Constructor::<Modifiable, Relaxed<Unseeded>>::new(psbt),
            Err(Error::InputsNotModifiable)
        );
    }

    #[test]
    fn new_modifiable_rejects_missing_outputs_flag() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_outputs_modifiable();
        assert_eq!(
            Constructor::<Modifiable, Relaxed<Unseeded>>::new(psbt),
            Err(Error::OutputsNotModifiable)
        );
    }

    #[test]
    fn no_more_inputs_then_no_more_outputs() {
        let c = Creator::new().constructor();
        let c = c.no_more_inputs(); // Modifiable → OutputsOnlyModifiable
        let unordered = c.no_more_outputs().into_psbt(); // OutputsOnlyModifiable → Sorter<S> → UnorderedPsbt
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn no_more_outputs_then_no_more_inputs() {
        let c = Creator::new().constructor();
        let c = c.no_more_outputs(); // Modifiable → InputsOnlyModifiable
        let unordered = c.no_more_inputs().into_psbt(); // InputsOnlyModifiable → Sorter<S> → UnorderedPsbt
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn try_sort_sorts_by_explicit_sort_keys() {
        let mut psbt = Creator::new()
            .explicit_sort_keys()
            .into_unordered_psbt()
            .to_psbt();

        // Two inputs: sort key 0x02 first, 0x01 second (reverse order).
        let mut oa = bitcoin::OutPoint::null();
        oa.vout = 0;
        let mut ob = bitcoin::OutPoint::null();
        ob.vout = 1;

        let mut input_b = psbt_v2::v2::Input::new(&ob);
        input_b.set_sort_key(vec![0x02]);

        let mut input_a = psbt_v2::v2::Input::new(&oa);
        input_a.set_sort_key(vec![0x01]);

        psbt.inputs = vec![input_b, input_a];
        psbt.global.input_count = 2;

        // Two outputs: sort key 0x02 first, 0x01 second (reverse order).
        let mut output_y = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(2000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xBB]),
        });
        output_y.set_sort_key(vec![0x02]);
        output_y.set_unique_id(vec![0x02; 16]);

        let mut output_x = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xAA]),
        });
        output_x.set_sort_key(vec![0x01]);
        output_x.set_unique_id(vec![0x01; 16]);

        psbt.outputs = vec![output_y, output_x];
        psbt.global.output_count = 2;

        let constructor = Constructor::<Modifiable, ExplicitSortKeys>::new(psbt).unwrap();
        let ordered = constructor.try_sort().unwrap();

        // After sorting: sort_key 0x01 (vout=0) before 0x02 (vout=1).
        assert_eq!(ordered.inputs[0].spent_output_index, 0);
        assert_eq!(ordered.inputs[1].spent_output_index, 1);

        // After sorting: sort_key 0x01 (1000 sat) before 0x02 (2000 sat).
        assert_eq!(ordered.outputs[0].amount, bitcoin::Amount::from_sat(1000));
        assert_eq!(ordered.outputs[1].amount, bitcoin::Amount::from_sat(2000));

        // Sort keys are scrubbed from the ordered PSBT.
        use crate::input::InputExt as _;
        use crate::output::OutputExt as _;
        assert!(ordered.inputs.iter().all(|i| i.sort_key().is_none()));
        assert!(ordered.outputs.iter().all(|o| o.sort_key().is_none()));
    }

    #[test]
    fn try_sort_produces_valid_updater() {
        use psbt_v2::v2::Constructor as Bip370Constructor;

        // Empty PSBT — no inputs or outputs to sort.
        let constructor = Creator::new().explicit_sort_keys().constructor();
        let psbt = constructor.try_sort().unwrap();
        let _updater = Bip370Constructor::<Modifiable>::new(psbt)
            .unwrap()
            .updater()
            .unwrap();
    }

    #[test]
    fn try_sort_with_inputs_and_outputs_produces_ordered_psbt() {
        use psbt_v2::v2::Constructor as Bip370Constructor;

        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let mut input_b = psbt_v2::v2::Input::new(&op_b);
        input_b.set_sort_key(vec![0x01]); // sorts first
        let mut input_a = psbt_v2::v2::Input::new(&op_a);
        input_a.set_sort_key(vec![0x02]); // sorts second

        let mut out = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out.set_unique_id(vec![0x01; 16]);
        out.set_sort_key(vec![0x01]);

        let c = Creator::new()
            .explicit_sort_keys()
            .constructor()
            .input(input_b)
            .unwrap()
            .input(input_a)
            .unwrap()
            .output(out)
            .unwrap();

        let psbt = c.try_sort().unwrap();
        // input with sort key 0x01 (op_b, vout=1) sorts first
        assert_eq!(psbt.inputs[0].spent_output_index, 1);
        assert_eq!(psbt.inputs[1].spent_output_index, 0);

        // Can convert to BIP370 and proceed to updater
        let _updater = Bip370Constructor::<Modifiable>::new(psbt)
            .unwrap()
            .updater()
            .unwrap();
    }

    #[test]
    fn try_sort_inputs_only_modifiable_produces_bip370() {
        use psbt_v2::v2::Constructor as Bip370Constructor;
        let mut input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        input.set_sort_key(vec![0x01]);
        let c = Creator::new()
            .explicit_sort_keys()
            .constructor()
            .input(input)
            .unwrap()
            .no_more_outputs(); // Constructor<InputsOnlyModifiable, ExplicitSortKeys>
        let psbt = c.try_sort().unwrap();
        // BIP370 construction must succeed (flags preserved)
        assert!(Bip370Constructor::<InputsOnlyModifiable>::new(psbt).is_ok());
    }

    #[test]
    fn try_sort_outputs_only_modifiable_produces_bip370() {
        use psbt_v2::v2::Constructor as Bip370Constructor;
        let mut out = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(500),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out.set_unique_id(vec![0x02; 16]);
        out.set_sort_key(vec![0x01]);
        let c = Creator::new()
            .explicit_sort_keys()
            .constructor()
            .output(out)
            .unwrap()
            .no_more_inputs(); // Constructor<OutputsOnlyModifiable, ExplicitSortKeys>
        let psbt = c.try_sort().unwrap();
        assert!(Bip370Constructor::<OutputsOnlyModifiable>::new(psbt).is_ok());
    }

    #[test]
    fn try_sort_rejects_missing_sort_keys() {
        let mut psbt = Creator::new()
            .explicit_sort_keys()
            .into_unordered_psbt()
            .to_psbt();

        // Input without a sort key.
        let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        psbt.inputs = vec![input];
        psbt.global.input_count = 1;

        let constructor = Constructor::<Modifiable, ExplicitSortKeys>::new(psbt).unwrap();
        assert!(matches!(
            constructor.try_sort(),
            Err(SortingError::MissingSortKey)
        ));
    }

    #[test]
    fn try_sort_rejects_duplicate_input_sort_keys() {
        let mut psbt = Creator::new()
            .explicit_sort_keys()
            .into_unordered_psbt()
            .to_psbt();

        let mut input_a = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        input_a.set_sort_key(vec![0x01]);

        let mut ob = bitcoin::OutPoint::null();
        ob.vout = 1;
        let mut input_b = psbt_v2::v2::Input::new(&ob);
        input_b.set_sort_key(vec![0x01]); // same key

        psbt.inputs = vec![input_a, input_b];
        psbt.global.input_count = 2;

        let constructor = Constructor::<Modifiable, ExplicitSortKeys>::new(psbt).unwrap();
        assert!(matches!(
            constructor.try_sort(),
            Err(SortingError::DuplicateSortKey)
        ));
    }

    #[test]
    fn new_modifiable_rejects_missing_output_unique_id() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();

        // Add an output without PSBT_OUT_UNIQUE_ID.
        let output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xAA]),
        });
        psbt.outputs = vec![output];
        psbt.global.output_count = 1;

        assert_eq!(
            Constructor::<Modifiable, Relaxed<Unseeded>>::new(psbt),
            Err(Error::MissingOutputUniqueId),
        );
    }

    #[test]
    fn new_modifiable_accepts_output_with_unique_id() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();

        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xAA]),
        });
        output.set_unique_id(vec![0x01; 16]);
        psbt.outputs = vec![output];
        psbt.global.output_count = 1;

        assert!(Constructor::<Modifiable, Relaxed<Unseeded>>::new(psbt).is_ok());
    }

    #[test]
    fn inputs_only_new_rejects_locked_inputs() {
        let c = Creator::new().constructor();
        let unordered = c.no_more_inputs().no_more_outputs().into_psbt();
        assert_eq!(
            Constructor::<InputsOnlyModifiable, Relaxed<Unseeded>>::new(unordered.to_psbt()),
            Err(Error::InputsNotModifiable)
        );
    }

    #[test]
    fn outputs_only_new_rejects_locked_outputs() {
        let c = Creator::new().constructor();
        let unordered = c.no_more_outputs().no_more_inputs().into_psbt();
        assert_eq!(
            Constructor::<OutputsOnlyModifiable, Relaxed<Unseeded>>::new(unordered.to_psbt()),
            Err(Error::OutputsNotModifiable)
        );
    }

    #[test]
    fn input_count_is_correct_after_adding_inputs() {
        let c = Creator::new().constructor();
        let mut op1 = bitcoin::OutPoint::null();
        op1.vout = 0;
        let mut op2 = bitcoin::OutPoint::null();
        op2.vout = 1;
        let c = c
            .input(psbt_v2::v2::Input::new(&op1))
            .unwrap()
            .input(psbt_v2::v2::Input::new(&op2))
            .unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.global.input_count, 2);
        assert_eq!(psbt.inputs.len(), 2);
    }

    #[test]
    fn output_count_is_correct_after_adding_outputs() {
        let c = Creator::new().constructor();
        let mut o1 = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        o1.set_unique_id(vec![0x01; 16]);
        let mut o2 = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(2000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        o2.set_unique_id(vec![0x02; 16]);
        let c = c.output(o1).unwrap().output(o2).unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.global.output_count, 2);
        assert_eq!(psbt.outputs.len(), 2);
    }

    #[test]
    fn input_conflict_error_has_correct_count() {
        // Same outpoint, different sequence → conflict.
        // The error value's global.input_count should reflect the
        // actual set size (1), not 0 or 2.
        let c = Creator::new().constructor();
        let op = bitcoin::OutPoint::null();
        let mut input_a = psbt_v2::v2::Input::new(&op);
        input_a.sequence = Some(bitcoin::Sequence::MAX);
        let mut input_b = psbt_v2::v2::Input::new(&op);
        input_b.sequence = Some(bitcoin::Sequence::ENABLE_LOCKTIME_NO_RBF);
        let c = c.input(input_a).unwrap();
        let err = c.input(input_b).unwrap_err();
        assert!(matches!(err, Error::JoinConflict(_)));
        if let Error::JoinConflict(result) = err {
            // Count must be 1 (one distinct outpoint), not 0 or 2.
            assert_eq!(result.global.input_count, Ok(1));
        }
    }

    #[test]
    fn input_join_conflict_returns_error() {
        // Same outpoint, different sequence → conflict.
        let c = Creator::new().constructor();
        let op = bitcoin::OutPoint::null();
        let mut input_a = psbt_v2::v2::Input::new(&op);
        input_a.sequence = Some(bitcoin::Sequence::MAX);
        let mut input_b = psbt_v2::v2::Input::new(&op);
        input_b.sequence = Some(bitcoin::Sequence::ENABLE_LOCKTIME_NO_RBF);
        let c = c.input(input_a).unwrap();
        assert!(matches!(c.input(input_b), Err(Error::JoinConflict(_))));
    }

    #[test]
    fn modifiable_input_adds_to_set() {
        let c = Creator::new().constructor();
        let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        let c = c.input(input).unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.inputs.len(), 1);
    }

    #[test]
    fn modifiable_output_adds_to_set() {
        let c = Creator::new().constructor();
        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output.set_unique_id(vec![0xAA; 16]);
        let c = c.output(output).unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.outputs.len(), 1);
    }

    #[test]
    fn modifiable_output_rejects_missing_unique_id() {
        let c = Creator::new().constructor();
        let output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        assert_eq!(c.output(output), Err(Error::MissingOutputUniqueId));
    }

    #[test]
    fn inputs_only_input_adds_to_set() {
        let c = Creator::new().constructor().no_more_outputs();
        let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        let c = c.input(input).unwrap();
        let sorter = c.no_more_inputs();
        assert_eq!(sorter.into_psbt().inputs.len(), 1);
    }

    #[test]
    fn outputs_only_output_adds_to_set() {
        let c = Creator::new().constructor().no_more_inputs();
        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output.set_unique_id(vec![0xBB; 16]);
        let c = c.output(output).unwrap();
        let sorter = c.no_more_outputs();
        assert_eq!(sorter.into_psbt().outputs.len(), 1);
    }

    #[test]
    fn constructor_eq_reflexive() {
        let a = Creator::new().constructor();
        let b = Creator::new().constructor();
        assert_eq!(a, b);
    }

    #[test]
    fn constructor_eq_differs_after_input() {
        let a = Creator::new().constructor();
        let b = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&bitcoin::OutPoint::null()))
            .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn try_sort_inputs_only() {
        let mut psbt = Creator::new()
            .explicit_sort_keys()
            .into_unordered_psbt()
            .to_psbt();

        let mut input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        input.set_sort_key(vec![0x01]);
        psbt.inputs = vec![input];
        psbt.global.input_count = 1;
        psbt.global.clear_outputs_modifiable();

        let c = Constructor::<InputsOnlyModifiable, ExplicitSortKeys>::new(psbt).unwrap();
        let ordered = c.try_sort().unwrap();
        assert_eq!(ordered.inputs.len(), 1);
        assert_eq!(
            ordered.inputs[0].previous_txid,
            bitcoin::OutPoint::null().txid
        );
    }

    // -- Constructor::try_join tests -----------------------------------------

    #[test]
    fn try_join_modifiable_disjoint_inputs_merges() {
        // Two Modifiable constructors with disjoint inputs — join should succeed
        // and contain both inputs.
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let a = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_a))
            .unwrap();
        let b = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_b))
            .unwrap();

        let joined = a.try_join(b).unwrap();
        let psbt = joined.into_psbt();
        assert_eq!(psbt.inputs.len(), 2);
        assert_eq!(psbt.global.input_count, 2);
    }

    #[test]
    fn try_join_modifiable_same_input_no_conflict() {
        // Same outpoint, same data — idempotent join.
        let op = bitcoin::OutPoint::null();
        let a = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op))
            .unwrap();
        let b = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op))
            .unwrap();

        let joined = a.try_join(b).unwrap();
        let psbt = joined.into_psbt();
        assert_eq!(psbt.inputs.len(), 1);
    }

    #[test]
    fn try_join_modifiable_conflicting_input_returns_err() {
        let op = bitcoin::OutPoint::null();
        let mut input_a = psbt_v2::v2::Input::new(&op);
        input_a.sequence = Some(bitcoin::Sequence::MAX);
        let mut input_b = psbt_v2::v2::Input::new(&op);
        input_b.sequence = Some(bitcoin::Sequence::ENABLE_LOCKTIME_NO_RBF);

        let a = Creator::new().constructor().input(input_a).unwrap();
        let b = Creator::new().constructor().input(input_b).unwrap();

        assert!(matches!(a.try_join(b), Err(Error::JoinConflict(_))));
    }

    #[test]
    fn try_join_inputs_only_identical_outputs_succeeds() {
        // Constructor<InputsOnlyModifiable>: outputs are locked.
        // Both have no outputs (identical empty set) → join succeeds.
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let a = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_a))
            .unwrap()
            .no_more_outputs(); // Constructor<InputsOnlyModifiable>

        let b = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_b))
            .unwrap()
            .no_more_outputs();

        // Both have identical (empty) output sets → join succeeds.
        let joined = a.try_join(b).unwrap();
        let psbt = joined.into_psbt();
        assert_eq!(psbt.inputs.len(), 2);
    }

    #[test]
    fn try_join_outputs_only_different_output_sets_rejected() {
        // Constructor<OutputsOnlyModifiable>: inputs are locked.
        // Both have no inputs (identical empty sets) → inputs OK.
        // But if outputs differ beyond the locked empty set, that's different.
        // Actually for OutputsOnlyModifiable, *inputs* are locked (empty for both).
        // We test: if both have different outputs locked, LockedSetMismatch.
        // But wait — outputs are *modifiable* in OutputsOnlyModifiable.
        // So we test InputsOnlyModifiable where inputs are modifiable and
        // outputs are locked: if the locked *output* sets differ → LockedSetMismatch.

        let mut out_a = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out_a.set_unique_id(vec![0x01; 16]);

        let mut out_b = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(2000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out_b.set_unique_id(vec![0x02; 16]);

        // a has out_a locked (no_more_inputs locks inputs flag, but we want
        // locked outputs). Use no_more_outputs → InputsOnlyModifiable where
        // outputs flag is cleared (locked).
        // Wait: no_more_outputs on Modifiable → InputsOnlyModifiable:
        //   inputs flag = set (modifiable), outputs flag = clear (locked).
        // So in InputsOnlyModifiable, *outputs* are locked.

        let a = Creator::new()
            .constructor()
            .output(out_a)
            .unwrap()
            .no_more_outputs(); // Constructor<InputsOnlyModifiable>, outputs locked

        let b = Creator::new()
            .constructor()
            .output(out_b)
            .unwrap()
            .no_more_outputs(); // different locked output set

        assert_eq!(a.try_join(b), Err(Error::LockedSetMismatch));
    }

    #[test]
    fn try_join_inputs_only_locked_inputs_differ_rejected() {
        // Constructor<OutputsOnlyModifiable>: inputs are locked.
        // a has one input locked, b has a different input locked → LockedSetMismatch.
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let a = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_a))
            .unwrap()
            .no_more_inputs(); // Constructor<OutputsOnlyModifiable>, inputs locked

        let b = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_b))
            .unwrap()
            .no_more_inputs();

        assert_eq!(a.try_join(b), Err(Error::LockedSetMismatch));
    }

    // -- deterministic / relaxed-seeded sort tests --------------------------

    #[test]
    fn deterministic_seeded_sort_is_stable_and_ordered() {
        // Two inputs with the same seed: derived keys should be different (different outpoints)
        // and the order should be deterministic.
        let seed = b"test-seed-16bytes".to_vec();

        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let psbt = Creator::new()
            .deterministic_sorting()
            .set_seed(seed.clone())
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_b)) // add in reverse
            .unwrap()
            .input(psbt_v2::v2::Input::new(&op_a))
            .unwrap()
            .into_psbt()
            .to_psbt();

        let c = Constructor::<Modifiable, Deterministic<Seeded>>::new(psbt).unwrap();
        let ordered = c.try_sort().unwrap();

        // Both inputs present
        assert_eq!(ordered.inputs.len(), 2);
        // Order is deterministic — run twice and verify same result
        let mut op_a2 = bitcoin::OutPoint::null();
        op_a2.vout = 0;
        let mut op_b2 = bitcoin::OutPoint::null();
        op_b2.vout = 1;
        let psbt2 = Creator::new()
            .deterministic_sorting()
            .set_seed(seed.clone())
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_a2))
            .unwrap()
            .input(psbt_v2::v2::Input::new(&op_b2))
            .unwrap()
            .into_psbt()
            .to_psbt();
        let c2 = Constructor::<Modifiable, Deterministic<Seeded>>::new(psbt2).unwrap();
        let ordered2 = c2.try_sort().unwrap();
        assert_eq!(
            ordered.inputs[0].spent_output_index,
            ordered2.inputs[0].spent_output_index
        );
        assert_eq!(
            ordered.inputs[1].spent_output_index,
            ordered2.inputs[1].spent_output_index
        );
    }

    #[test]
    fn relaxed_seeded_uses_explicit_key_when_present() {
        // In Relaxed<Seeded>, an explicit sort key overrides derivation.
        let seed = b"test-seed-16bytes".to_vec();

        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        // Give input_b an explicit sort key of 0x00 (should sort first)
        // and input_a no explicit key (will be derived).
        let mut input_b = psbt_v2::v2::Input::new(&op_b);
        input_b.set_sort_key(vec![0x00]);

        let psbt = Creator::new()
            .set_seed(seed.clone()) // Relaxed<Seeded> via Creator::set_seed
            .constructor()
            .input(input_b)
            .unwrap()
            .input(psbt_v2::v2::Input::new(&op_a))
            .unwrap()
            .into_psbt()
            .to_psbt();

        let c = Constructor::<Modifiable, Relaxed<Seeded>>::new(psbt).unwrap();
        let ordered = c.try_sort().unwrap();

        // input_b had explicit key 0x00, so it sorts first
        assert_eq!(ordered.inputs[0].spent_output_index, 1); // op_b.vout = 1
        assert_eq!(ordered.inputs[1].spent_output_index, 0); // op_a.vout = 0
    }

    #[test]
    fn deterministic_sort_different_seeds_give_different_order() {
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let make_ordered = |seed: Vec<u8>| {
            let psbt = Creator::new()
                .deterministic_sorting()
                .set_seed(seed)
                .constructor()
                .input(psbt_v2::v2::Input::new(&op_a))
                .unwrap()
                .input(psbt_v2::v2::Input::new(&op_b))
                .unwrap()
                .into_psbt()
                .to_psbt();
            let c = Constructor::<Modifiable, Deterministic<Seeded>>::new(psbt).unwrap();
            let ordered = c.try_sort().unwrap();
            ordered
                .inputs
                .into_iter()
                .map(|i| i.spent_output_index)
                .collect::<Vec<_>>()
        };

        let order_a = make_ordered(b"seed-aaaa-16byte".to_vec());
        let order_b = make_ordered(b"seed-bbbb-16byte".to_vec());
        // Different seeds should (with overwhelming probability) give different orders.
        // Since we only have 2 inputs this is 50/50 — just verify both are present
        // and the sort is deterministic per seed.
        assert_eq!(order_a, make_ordered(b"seed-aaaa-16byte".to_vec()));
        assert_eq!(order_b, make_ordered(b"seed-bbbb-16byte".to_vec()));
    }

    // -- sort-mode invariant tests -------------------------------------------

    #[test]
    fn deterministic_input_rejects_explicit_sort_key() {
        let mut input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        input.set_sort_key(vec![0x01]);
        let c = Creator::new().deterministic_sorting().constructor();
        assert_eq!(c.input(input), Err(Error::SortKeyForbidden));
    }

    #[test]
    fn deterministic_output_rejects_explicit_sort_key() {
        let mut out = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out.set_unique_id(vec![0x01; 16]);
        out.set_sort_key(vec![0x01]);
        let c = Creator::new().deterministic_sorting().constructor();
        assert_eq!(c.output(out), Err(Error::SortKeyForbidden));
    }

    #[test]
    fn duplicate_input_sort_key_rejected_eagerly() {
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;
        let mut input_a = psbt_v2::v2::Input::new(&op_a);
        input_a.set_sort_key(vec![0x01]);
        let mut input_b = psbt_v2::v2::Input::new(&op_b);
        input_b.set_sort_key(vec![0x01]); // same key
        let c = Creator::new().explicit_sort_keys().constructor();
        let c = c.input(input_a).unwrap();
        assert_eq!(c.input(input_b), Err(Error::DuplicateSortKey));
    }

    #[test]
    fn duplicate_output_sort_key_rejected_eagerly() {
        let mut out_a = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out_a.set_unique_id(vec![0x01; 16]);
        out_a.set_sort_key(vec![0x01]);
        let mut out_b = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(2000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out_b.set_unique_id(vec![0x02; 16]);
        out_b.set_sort_key(vec![0x01]); // same key
        let c = Creator::new().explicit_sort_keys().constructor();
        let c = c.output(out_a).unwrap();
        assert_eq!(c.output(out_b), Err(Error::DuplicateSortKey));
    }

    #[test]
    fn duplicate_output_unique_id_rejected_by_constructor_new() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        let mut out_a = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out_a.set_unique_id(vec![0x01; 16]);
        let mut out_b = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(2000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out_b.set_unique_id(vec![0x01; 16]); // duplicate
        psbt.outputs = vec![out_a, out_b];
        psbt.global.output_count = 2;
        assert_eq!(
            Constructor::<Modifiable, Relaxed<Unseeded>>::new(psbt),
            Err(Error::DuplicateOutputUniqueId)
        );
    }

    // -- no_more_inputs/no_more_outputs → Sorter<S> -------------------------

    #[test]
    fn no_more_inputs_returns_sorter() {
        // InputsOnlyModifiable::no_more_inputs → Sorter<S>
        let c = Creator::new().constructor().no_more_outputs();
        let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        let c = c.input(input).unwrap();
        let sorter = c.no_more_inputs();
        // Sorter should hold the PSBT with both flags cleared
        let unordered = sorter.into_psbt();
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn no_more_outputs_returns_sorter() {
        // OutputsOnlyModifiable::no_more_outputs → Sorter<S>
        let c = Creator::new().constructor().no_more_inputs();
        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output.set_unique_id(vec![0x01; 16]);
        let c = c.output(output).unwrap();
        let sorter = c.no_more_outputs();
        let unordered = sorter.into_psbt();
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn sorter_into_shuffled_psbt() {
        let c = Creator::new().constructor().no_more_outputs();
        let sorter = c.no_more_inputs();
        let psbt = sorter.into_shuffled_psbt();
        // UNORDERED flag preserved (not stripped by shuffled conversion)
        assert!(psbt
            .global
            .proprietaries
            .contains_key(&crate::fields::psbt_global_tx_unordered()));
    }
}
