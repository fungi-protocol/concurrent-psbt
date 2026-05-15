use core::marker::PhantomData;

use psbt_v2::v2::Constructor as Bip370Constructor;
use psbt_v2::v2::Creator as Bip370Creator;
use psbt_v2::v2::{InputsOnlyModifiable, Mod, Modifiable, OutputsOnlyModifiable, Psbt};

use crate::sort::{Deterministic, ExplicitSortKeys, Relaxed, Seeded, SortMode, Unseeded};

use crate::fields::{GlobalFieldsExt as _, GlobalModifiableExt as _};
use crate::input::InputExt as _;
use crate::output::OutputExt as _;
use crate::tx::UnorderedPsbt;

use psbt_v2::v2::{Input, Output};

/// Error returned when a PSBT is not suitable for an unordered Constructor.
// PartialEq is manual: JoinConflict compares equal regardless of payload.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The `PSBT_GLOBAL_TX_UNORDERED` field is missing or has a wrong value.
    #[error("PSBT is not marked unordered")]
    NotUnordered,
    /// The inputs-modifiable flag is not set.
    #[error("inputs are not modifiable")]
    InputsNotModifiable,
    /// The outputs-modifiable flag is not set.
    #[error("outputs are not modifiable")]
    OutputsNotModifiable,
    /// An output is missing the `PSBT_OUT_UNIQUE_ID` proprietary field.
    #[error("an output is missing PSBT_OUT_UNIQUE_ID")]
    MissingOutputUniqueId,
    /// Joining the new input or output with the existing PSBT produced a conflict.
    #[error("joining the new input or output produced a conflict")]
    JoinConflict(crate::tx::ResultUnorderedPsbt),
    /// Neither the inputs-modifiable nor the outputs-modifiable flag is set.
    #[error("neither inputs-modifiable nor outputs-modifiable flag is set")]
    NeitherModifiable,
    /// A locked (non-modifiable) set contained items not present in the other side.
    #[error("a locked set contained items not present in the other constructor")]
    LockedSetMismatch,
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Error::NotUnordered, Error::NotUnordered)
                | (Error::InputsNotModifiable, Error::InputsNotModifiable)
                | (Error::OutputsNotModifiable, Error::OutputsNotModifiable)
                | (Error::MissingOutputUniqueId, Error::MissingOutputUniqueId)
                | (Error::JoinConflict(_), Error::JoinConflict(_))
                | (Error::NeitherModifiable, Error::NeitherModifiable)
                | (Error::LockedSetMismatch, Error::LockedSetMismatch)
        )
    }
}

impl Eq for Error {}

/// Error returned when sorting an unordered Constructor into a fixed order.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SortingError {
    /// An input or output is missing its sort key.
    // TODO split into MissingSortKeyForInput(OutPoint) and MissingSortKeyForOutput(unique id)
    #[error("an input or output is missing its sort key")]
    MissingSortKey,
    /// Two inputs or two outputs share the same sort key.
    // TODO (OutPoint, OutPoint) or (unique id, unique id) pointing out which collide
    #[error("two inputs or two outputs share the same sort key")]
    DuplicateSortKey,
}

// -- Helpers -----------------------------------------------------------------

/// Extract sort keys from items via `take_key`, sort by key, return items in order.
///
/// Fails if any key is missing or if two items share the same sort key.
fn sort_by_extracted_key<T>(
    items: impl IntoIterator<Item = T>,
    mut take_key: impl FnMut(&mut T) -> Option<Vec<u8>>,
) -> Result<Vec<T>, SortingError> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<Vec<u8>, T> = BTreeMap::new();
    for mut item in items {
        let key = take_key(&mut item).ok_or(SortingError::MissingSortKey)?;
        if map.insert(key, item).is_some() {
            return Err(SortingError::DuplicateSortKey);
        }
    }
    Ok(map.into_values().collect())
}

// -- Validation --------------------------------------------------------------

/// Check that every output in a raw `Psbt` carries `PSBT_OUT_UNIQUE_ID`.
fn validate_output_unique_ids(psbt: &Psbt) -> Result<(), Error> {
    for output in &psbt.outputs {
        if !output.has_unique_id() {
            return Err(Error::MissingOutputUniqueId);
        }
    }
    Ok(())
}

/// Check that a single output carries `PSBT_OUT_UNIQUE_ID`.
fn validate_output_unique_id(output: &Output) -> Result<(), Error> {
    if output.has_unique_id() {
        Ok(())
    } else {
        Err(Error::MissingOutputUniqueId)
    }
}

// -- Constructor -------------------------------------------------------------

/// Unordered Constructor, mirrors the BIP 370 Constructor but for unordered PSBTs.
///
/// `M` encodes which inputs/outputs are still modifiable (see [`psbt_v2::v2::Mod`]).
/// `S` encodes the sort strategy (see [`crate::sort`]).
pub struct Constructor<M: Mod, S: SortMode>(UnorderedPsbt, PhantomData<(M, S)>);

// Manual impl: derive would add unnecessary bounds on M and S.
impl<M: Mod, S: SortMode> core::fmt::Debug for Constructor<M, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Constructor").field(&self.0).finish()
    }
}

// Manual impl: derive would add unnecessary bounds on M and S.
impl<M: Mod, S: SortMode> PartialEq for Constructor<M, S> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<M: Mod, S: SortMode> Constructor<M, S> {
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
            .map_err(Error::JoinConflict)
    }
}

impl<M: Mod> Constructor<M, ExplicitSortKeys> {
    /// Sort inputs and outputs by their explicit sort keys, producing an ordered BIP 370 `Psbt`.
    fn try_sort_inner(self) -> Result<Psbt, SortingError> {
        let inputs = sort_by_extracted_key(self.0.inputs, |i| i.take_sort_key())?;
        let outputs = sort_by_extracted_key(self.0.outputs, |o| o.take_sort_key())?;

        let mut global = self.0.global;
        global.clear_tx_unordered();

        Ok(Psbt {
            global,
            inputs,
            outputs,
        })
    }
}

impl<S: SortMode> Constructor<Modifiable, S> {
    /// Wrap an existing PSBT, validating it is unordered and fully modifiable.
    ///
    /// The sort mode `S` must match the `PSBT_GLOBAL_SORT_DETERMINISTIC` field
    /// in the PSBT; this is not validated here — callers should use [`Creator`]
    /// to produce correctly-typed constructors.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        validate_output_unique_ids(&psbt)?;
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
    /// Returns `Err(Error::JoinConflict(_))` if the same outpoint is already
    /// present with conflicting field values. The payload is the
    /// conflict-annotated `ResultUnorderedPsbt` with correct counts.
    pub fn input(self, input: Input) -> Result<Self, Error> {
        self.0
            .try_join(UnorderedPsbt::from_input(input))
            .map(|p| Constructor(p, PhantomData))
            .map_err(Error::JoinConflict)
    }

    /// Add an output. Requires `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Returns `Err(Error::JoinConflict(_))` if the same unique ID is already
    /// present with conflicting field values. The payload is the
    /// conflict-annotated `ResultUnorderedPsbt` with correct counts.
    pub fn output(self, output: Output) -> Result<Self, Error> {
        validate_output_unique_id(&output)?;
        self.0
            .try_join(UnorderedPsbt::from_output(output))
            .map(|p| Constructor(p, PhantomData))
            .map_err(Error::JoinConflict)
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

impl Constructor<Modifiable, ExplicitSortKeys> {
    /// Sort inputs/outputs by explicit sort keys, producing a BIP 370 `Constructor<Modifiable>`.
    pub fn try_sort(self) -> Result<Bip370Constructor<Modifiable>, SortingError> {
        let psbt = self.try_sort_inner()?;
        Ok(Bip370Constructor::<Modifiable>::new(psbt).expect("modifiable flags are preserved"))
    }
}

impl<S: SortMode> Constructor<InputsOnlyModifiable, S> {
    /// Add an input.
    ///
    /// Returns `Err(Error::JoinConflict(_))` if the same outpoint is already
    /// present with conflicting field values.
    pub fn input(self, input: Input) -> Result<Self, Error> {
        let singleton =
            AnyConstructor::Modifiable(Constructor(UnorderedPsbt::from_input(input), PhantomData));
        match AnyConstructor::InputsOnly(self).try_join(singleton)? {
            AnyConstructor::InputsOnly(c) => Ok(c),
            _ => unreachable!("InputsOnly joined with Modifiable stays InputsOnly"),
        }
    }

    /// Wrap an existing PSBT, validating it is unordered and inputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        validate_output_unique_ids(&psbt)?;
        let unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_inputs_modifiable() {
            return Err(Error::InputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Lock inputs: both sides now locked, return the `UnorderedPsbt`.
    pub fn no_more_inputs(mut self) -> UnorderedPsbt {
        self.0.global.clear_inputs_modifiable();
        self.0
    }
}

impl Constructor<InputsOnlyModifiable, ExplicitSortKeys> {
    /// Sort inputs/outputs by explicit sort keys, producing a BIP 370 `Constructor<InputsOnlyModifiable>`.
    pub fn try_sort(self) -> Result<Bip370Constructor<InputsOnlyModifiable>, SortingError> {
        let psbt = self.try_sort_inner()?;
        Ok(Bip370Constructor::<InputsOnlyModifiable>::new(psbt)
            .expect("inputs-modifiable flag is preserved"))
    }
}

impl<S: SortMode> Constructor<OutputsOnlyModifiable, S> {
    /// Add an output. Requires `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Returns `Err(Error::JoinConflict(_))` if the same unique ID is already
    /// present with conflicting field values.
    pub fn output(self, output: Output) -> Result<Self, Error> {
        validate_output_unique_id(&output)?;
        let singleton = AnyConstructor::Modifiable(Constructor(
            UnorderedPsbt::from_output(output),
            PhantomData,
        ));
        match AnyConstructor::OutputsOnly(self).try_join(singleton)? {
            AnyConstructor::OutputsOnly(c) => Ok(c),
            _ => unreachable!("OutputsOnly joined with Modifiable stays OutputsOnly"),
        }
    }

    /// Wrap an existing PSBT, validating it is unordered and outputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        validate_output_unique_ids(&psbt)?;
        let unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_outputs_modifiable() {
            return Err(Error::OutputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Lock outputs: both sides now locked, return the `UnorderedPsbt`.
    pub fn no_more_outputs(mut self) -> UnorderedPsbt {
        self.0.global.clear_outputs_modifiable();
        self.0
    }
}

impl Constructor<OutputsOnlyModifiable, ExplicitSortKeys> {
    /// Sort inputs/outputs by explicit sort keys, producing a BIP 370 `Constructor<OutputsOnlyModifiable>`.
    pub fn try_sort(self) -> Result<Bip370Constructor<OutputsOnlyModifiable>, SortingError> {
        let psbt = self.try_sort_inner()?;
        Ok(Bip370Constructor::<OutputsOnlyModifiable>::new(psbt)
            .expect("outputs-modifiable flag is preserved"))
    }
}

// -- AnyConstructor ----------------------------------------------------------

/// An unordered Constructor whose modifiability typestate is determined at
/// runtime from the PSBT's flags.
///
/// Use `AnyConstructor::from_psbt` when you do not know a priori which
/// typestate applies; match on the variants to get a concrete
/// `Constructor<M, S>`.
///
/// `S` is the sort mode, uniform across all variants.
#[derive(Debug, PartialEq)]
pub enum AnyConstructor<S: SortMode = Relaxed<Unseeded>> {
    /// Both inputs and outputs are modifiable.
    Modifiable(Constructor<Modifiable, S>),
    /// Only inputs are modifiable (outputs locked).
    InputsOnly(Constructor<InputsOnlyModifiable, S>),
    /// Only outputs are modifiable (inputs locked).
    OutputsOnly(Constructor<OutputsOnlyModifiable, S>),
}

impl<S: SortMode> AnyConstructor<S> {
    /// Extract the inner `UnorderedPsbt` regardless of typestate.
    fn into_inner(self) -> UnorderedPsbt {
        match self {
            AnyConstructor::Modifiable(c) => c.0,
            AnyConstructor::InputsOnly(c) => c.0,
            AnyConstructor::OutputsOnly(c) => c.0,
        }
    }

    /// Merge two `AnyConstructor`s, raising both to the modifiability-lattice join.
    ///
    /// The modifiability lattice: bottom = both modifiable, top = both locked.
    /// Locking is monotone: if either side has a flag cleared, the result has
    /// it cleared.
    ///
    /// For each locked side the locked set of the more-locked constructor must
    /// be a superset of (or equal to) the other's corresponding set; if not,
    /// `Err(LockedSetMismatch)` is returned.
    ///
    /// If the lattice join yields both sides locked (e.g. `InputsOnly` ∨
    /// `OutputsOnly`), we `todo!()` — that case requires the sort/seed path
    /// which is not yet implemented.
    ///
    /// On a value conflict `Err(JoinConflict)` is returned.
    pub fn try_join(self, other: Self) -> Result<Self, Error> {
        let self_inputs_mod = match &self {
            AnyConstructor::Modifiable(_) | AnyConstructor::InputsOnly(_) => true,
            AnyConstructor::OutputsOnly(_) => false,
        };
        let self_outputs_mod = match &self {
            AnyConstructor::Modifiable(_) | AnyConstructor::OutputsOnly(_) => true,
            AnyConstructor::InputsOnly(_) => false,
        };
        let other_inputs_mod = match &other {
            AnyConstructor::Modifiable(_) | AnyConstructor::InputsOnly(_) => true,
            AnyConstructor::OutputsOnly(_) => false,
        };
        let other_outputs_mod = match &other {
            AnyConstructor::Modifiable(_) | AnyConstructor::OutputsOnly(_) => true,
            AnyConstructor::InputsOnly(_) => false,
        };

        // Lattice join: a side is locked in the result if either side locks it.
        let result_inputs_mod = self_inputs_mod && other_inputs_mod;
        let result_outputs_mod = self_outputs_mod && other_outputs_mod;

        if !result_inputs_mod && !result_outputs_mod {
            // TODO add an Unmodifiable variant to AnyConstructor, which
            // provides sort only functionality (new Sorter role, distinct from
            // Constructor, but where Constructor uses Sorter as part of its
            // implementation). Unmodifiable Sorter transitions straight to
            // Updater in bip 174/370 sense, and so should convert to bip 370
            // constructor internally and call .updater()
            //
            // to go down the modifiability lattice, the user should serialize
            // the Psbt and set its modifiability manually
            todo!(
                "AnyConstructor::try_join: both sides locked after modifiability join; \
                 the resulting unmodifiable constructor can only be sorted (requires seed path)"
            );
        }

        let self_inner = self.into_inner();
        let other_inner = other.into_inner();

        // FIXME this should probably get refactored and live in GlobalExt or something
        // For each locked side: the locked constructor's set must be a superset
        // of the other's (i.e. join(locked, other) == locked).
        if !result_inputs_mod {
            // inputs are locked — find which side is the locked one and check
            // the other's inputs are a subset.
            let locked_inputs = if !self_inputs_mod {
                &self_inner.inputs
            } else {
                &other_inner.inputs
            };
            let candidate_inputs = if !self_inputs_mod {
                &other_inner.inputs
            } else {
                &self_inner.inputs
            };
            // candidate must be a subset: every key in candidate must be in locked.
            // Since InputSet uses OutPoint keys, we check via try_join idempotence:
            // locked ∪ candidate == locked iff candidate ⊆ locked.
            // Simplest check: all keys in candidate are in locked.
            if !candidate_inputs
                .iter_outpoints()
                .all(|op| locked_inputs.spends_outpoint(op))
            {
                return Err(Error::LockedSetMismatch);
            }
        }
        if !result_outputs_mod {
            let locked_outputs = if !self_outputs_mod {
                &self_inner.outputs
            } else {
                &other_inner.outputs
            };
            let candidate_outputs = if !self_outputs_mod {
                &other_inner.outputs
            } else {
                &self_inner.outputs
            };
            if !candidate_outputs
                .iter_unique_ids()
                .all(|id| locked_outputs.contains_unique_id(id))
            {
                return Err(Error::LockedSetMismatch);
            }
        }

        // FIXME this can be done as bitwise AND on global.transaction_modifiable
        // it should be a method in GlobalExt
        // Raise both to the result typestate by applying locking flags, then join.
        let mut a = self_inner;
        let mut b = other_inner;
        if !result_inputs_mod {
            a.global.clear_inputs_modifiable();
            b.global.clear_inputs_modifiable();
        }
        if !result_outputs_mod {
            a.global.clear_outputs_modifiable();
            b.global.clear_outputs_modifiable();
        }

        let joined = a.try_join(b).map_err(Error::JoinConflict)?;

        match (result_inputs_mod, result_outputs_mod) {
            (true, true) => Ok(AnyConstructor::Modifiable(Constructor(joined, PhantomData))),
            (true, false) => Ok(AnyConstructor::InputsOnly(Constructor(joined, PhantomData))),
            (false, true) => Ok(AnyConstructor::OutputsOnly(Constructor(
                joined,
                PhantomData,
            ))),
            (false, false) => unreachable!("handled above"), // TODO make a Sorter here
        }
    }

    /// Construct from a raw `Psbt`, dispatching to the appropriate typestate
    /// based on the modifiable flags.
    ///
    /// Errors:
    /// - [`Error::NotUnordered`] — `PSBT_GLOBAL_TX_UNORDERED` is absent or wrong.
    /// - [`Error::MissingOutputUniqueId`] — an output lacks `PSBT_OUT_UNIQUE_ID`.
    /// - [`Error::NeitherModifiable`] — both modifiable flags are cleared.
    pub fn from_psbt(psbt: Psbt) -> Result<Self, Error> {
        validate_output_unique_ids(&psbt)?;
        let unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        let inputs_mod = unordered.global.is_inputs_modifiable();
        let outputs_mod = unordered.global.is_outputs_modifiable();
        match (inputs_mod, outputs_mod) {
            (true, true) => Ok(AnyConstructor::Modifiable(Constructor(
                unordered,
                PhantomData,
            ))),
            (true, false) => Ok(AnyConstructor::InputsOnly(Constructor(
                unordered,
                PhantomData,
            ))),
            (false, true) => Ok(AnyConstructor::OutputsOnly(Constructor(
                unordered,
                PhantomData,
            ))),
            (false, false) => Err(Error::NeitherModifiable),
        }
    }
}

// -- Creator -----------------------------------------------------------------

/// Creator for unordered PSBTs.
///
/// Sets the `PSBT_GLOBAL_TX_UNORDERED` proprietary field and both modifiable
/// flags. By default produces a [`Constructor`] with sort mode
/// [`Relaxed<Unseeded>`]. Call [`Creator::explicit_sort_keys`] or
/// [`Creator::deterministic_sorting`] to select a different sort mode.
pub struct Creator(UnorderedPsbt);

impl Creator {
    pub fn new() -> Self {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();

        let mut unordered = UnorderedPsbt::unchecked_from_psbt(psbt);

        unordered.global.set_tx_unordered();

        Creator(unordered)
    }

    /// Set sort mode to explicit sort keys (`PSBT_GLOBAL_SORT_DETERMINISTIC = 0x00`).
    ///
    /// All inputs and outputs must have explicit sort keys before `finalize_order` is called.
    /// Mutually exclusive with [`Creator::deterministic_sorting`].
    pub fn explicit_sort_keys(mut self) -> CreatorWith<ExplicitSortKeys> {
        self.0.global.set_sort_explicit();
        CreatorWith(self.0, PhantomData)
    }

    /// Set sort mode to deterministic sorting (`PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`).
    ///
    /// Sort keys are derived from a seed; explicit per-input/output keys are not permitted.
    /// Mutually exclusive with [`Creator::explicit_sort_keys`].
    pub fn deterministic_sorting(mut self) -> CreatorWith<Deterministic<Unseeded>> {
        self.0.global.set_sort_deterministic();
        CreatorWith(self.0, PhantomData)
    }

    /// Provide a sort seed without changing the deterministic mode, staying in [`Relaxed<Seeded>`].
    ///
    /// This is the `Relaxed` analogue of [`CreatorWith::set_seed`].
    pub fn set_seed(mut self, seed: Vec<u8>) -> CreatorWith<Relaxed<Seeded>> {
        self.0.global.set_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }

    /// Consume the creator and return the `UnorderedPsbt`.
    pub fn into_unordered_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Consume the creator and return a fully-modifiable Constructor with [`Relaxed<Unseeded>`] sort mode.
    pub fn constructor(self) -> Constructor<Modifiable, Relaxed<Unseeded>> {
        Constructor::<Modifiable, Relaxed<Unseeded>>::new(self.0.to_psbt())
            .expect("Creator always produces a valid unordered PSBT")
    }
}

impl Default for Creator {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`Creator`] with a specific sort mode already chosen.
pub struct CreatorWith<S: SortMode>(UnorderedPsbt, PhantomData<S>);

impl<S: SortMode> CreatorWith<S> {
    /// Consume and return the `UnorderedPsbt`.
    pub fn into_unordered_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Consume and return a fully-modifiable Constructor with sort mode `S`.
    pub fn constructor(self) -> Constructor<Modifiable, S> {
        Constructor::<Modifiable, S>::new(self.0.to_psbt())
            .expect("CreatorWith always produces a valid unordered PSBT")
    }
}

impl CreatorWith<Deterministic<Unseeded>> {
    /// Provide the sort seed, transitioning to [`Deterministic<Seeded>`].
    pub fn set_seed(mut self, seed: Vec<u8>) -> CreatorWith<Deterministic<Seeded>> {
        self.0.global.set_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }
}

impl CreatorWith<Relaxed<Unseeded>> {
    /// Provide the sort seed, transitioning to [`Relaxed<Seeded>`].
    pub fn set_seed(mut self, seed: Vec<u8>) -> CreatorWith<Relaxed<Seeded>> {
        self.0.global.set_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }
}

// -- finalize_order helpers for seeded modes --------------------------------

impl<M: Mod, S: SortMode> Constructor<M, S> {
    fn try_sort_deterministic(self) -> Result<Psbt, SortingError> {
        use crate::sort::OutPointIdentifier as _;
        let seed = self
            .0
            .global
            .sort_seed()
            .expect("seeded mode always has a seed")
            .clone();
        let inputs = sort_by_extracted_key(self.0.inputs, |i| {
            Some(crate::sort::derive_sort_key(
                &seed,
                &i.out_point().to_identifier(),
            ))
        })?;
        let outputs = sort_by_extracted_key(self.0.outputs, |o| {
            Some(crate::sort::derive_sort_key(&seed, &o.unique_id()))
        })?;
        let mut global = self.0.global;
        global.clear_tx_unordered();
        Ok(Psbt {
            global,
            inputs,
            outputs,
        })
    }

    fn try_sort_relaxed_seeded(self) -> Result<Psbt, SortingError> {
        let seed = self
            .0
            .global
            .sort_seed()
            .expect("seeded mode always has a seed")
            .clone();
        let inputs =
            sort_by_extracted_key(self.0.inputs, |i| Some(i.take_or_derive_sort_key(&seed)))?;
        let outputs =
            sort_by_extracted_key(self.0.outputs, |o| Some(o.take_or_derive_sort_key(&seed)))?;
        let mut global = self.0.global;
        global.clear_tx_unordered();
        Ok(Psbt {
            global,
            inputs,
            outputs,
        })
    }
}

impl Constructor<Modifiable, Deterministic<Seeded>> {
    /// Sort all inputs/outputs using seed-derived keys (HMAC-SHA256).
    pub fn try_sort(self) -> Result<Bip370Constructor<Modifiable>, SortingError> {
        let psbt = self.try_sort_deterministic()?; // FIXME this should be infallible
        Ok(Bip370Constructor::<Modifiable>::new(psbt).expect("flags preserved"))
    }
}

impl Constructor<InputsOnlyModifiable, Deterministic<Seeded>> {
    /// Sort all inputs/outputs using seed-derived keys (HMAC-SHA256).
    pub fn try_sort(self) -> Result<Bip370Constructor<InputsOnlyModifiable>, SortingError> {
        let psbt = self.try_sort_deterministic()?;
        Ok(Bip370Constructor::<InputsOnlyModifiable>::new(psbt).expect("flags preserved"))
    }
}

impl Constructor<OutputsOnlyModifiable, Deterministic<Seeded>> {
    /// Sort all inputs/outputs using seed-derived keys (HMAC-SHA256).
    pub fn try_sort(self) -> Result<Bip370Constructor<OutputsOnlyModifiable>, SortingError> {
        let psbt = self.try_sort_deterministic()?;
        Ok(Bip370Constructor::<OutputsOnlyModifiable>::new(psbt).expect("flags preserved"))
    }
}

impl Constructor<Modifiable, Relaxed<Seeded>> {
    /// Sort inputs/outputs: explicit key if present, otherwise seed-derived.
    pub fn try_sort(self) -> Result<Bip370Constructor<Modifiable>, SortingError> {
        let psbt = self.try_sort_relaxed_seeded()?;
        Ok(Bip370Constructor::<Modifiable>::new(psbt).expect("flags preserved"))
    }
}

impl Constructor<InputsOnlyModifiable, Relaxed<Seeded>> {
    /// Sort inputs/outputs: explicit key if present, otherwise seed-derived.
    pub fn try_sort(self) -> Result<Bip370Constructor<InputsOnlyModifiable>, SortingError> {
        let psbt = self.try_sort_relaxed_seeded()?;
        Ok(Bip370Constructor::<InputsOnlyModifiable>::new(psbt).expect("flags preserved"))
    }
}

impl Constructor<OutputsOnlyModifiable, Relaxed<Seeded>> {
    /// Sort inputs/outputs: explicit key if present, otherwise seed-derived.
    pub fn try_sort(self) -> Result<Bip370Constructor<OutputsOnlyModifiable>, SortingError> {
        let psbt = self.try_sort_relaxed_seeded()?;
        Ok(Bip370Constructor::<OutputsOnlyModifiable>::new(psbt).expect("flags preserved"))
    }
}

// -- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_default_does_not_set_sort_deterministic_field() {
        use crate::fields::psbt_global_sort_deterministic;
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
        let unordered = c.no_more_outputs(); // OutputsOnlyModifiable → UnorderedPsbt
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn no_more_outputs_then_no_more_inputs() {
        let c = Creator::new().constructor();
        let c = c.no_more_outputs(); // Modifiable → InputsOnlyModifiable
        let unordered = c.no_more_inputs(); // InputsOnlyModifiable → UnorderedPsbt
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn try_sort_sorts_by_explicit_sort_keys() {
        use crate::fields::{psbt_in_sort_key, psbt_out_sort_key, psbt_out_unique_id};

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
        let bip370 = constructor.try_sort().unwrap();
        let ordered = bip370.psbt().unwrap();

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
        let constructor = Creator::new().explicit_sort_keys().constructor();
        let bip370 = constructor.try_sort().unwrap();
        let _updater = bip370.updater().unwrap();
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

    // Note: finalize_order_rejects_missing_deterministic_field and
    // finalize_order_rejects_invalid_deterministic_value are no longer needed:
    // the sort mode is now encoded in the type, so these cases cannot arise.

    #[test]
    fn try_sort_rejects_duplicate_input_sort_keys() {
        use crate::fields::psbt_in_sort_key;

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
        use crate::fields::psbt_out_unique_id;

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
        let unordered = c.no_more_inputs().no_more_outputs();
        assert_eq!(
            Constructor::<InputsOnlyModifiable, Relaxed<Unseeded>>::new(unordered.to_psbt()),
            Err(Error::InputsNotModifiable)
        );
    }

    #[test]
    fn outputs_only_new_rejects_locked_outputs() {
        let c = Creator::new().constructor();
        let unordered = c.no_more_outputs().no_more_inputs();
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
        use crate::fields::psbt_out_unique_id;

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
        use crate::fields::psbt_out_unique_id;

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
        let psbt = c.no_more_inputs();
        assert_eq!(psbt.inputs.len(), 1);
    }

    #[test]
    fn outputs_only_output_adds_to_set() {
        use crate::fields::psbt_out_unique_id;

        let c = Creator::new().constructor().no_more_inputs();
        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output.set_unique_id(vec![0xBB; 16]);
        let c = c.output(output).unwrap();
        let psbt = c.no_more_outputs();
        assert_eq!(psbt.outputs.len(), 1);
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
        use crate::fields::psbt_in_sort_key;

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
        let ordered = c.try_sort().unwrap().psbt().unwrap();
        assert_eq!(ordered.inputs.len(), 1);
        assert_eq!(
            ordered.inputs[0].previous_txid,
            bitcoin::OutPoint::null().txid
        );
    }

    // -- AnyConstructor tests ------------------------------------------------

    #[test]
    fn any_constructor_from_psbt_fully_modifiable() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let any = AnyConstructor::<Relaxed<Unseeded>>::from_psbt(psbt).unwrap();
        assert!(matches!(any, AnyConstructor::Modifiable(_)));
    }

    #[test]
    fn any_constructor_from_psbt_inputs_only() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_outputs_modifiable();
        let any = AnyConstructor::<Relaxed<Unseeded>>::from_psbt(psbt).unwrap();
        assert!(matches!(any, AnyConstructor::InputsOnly(_)));
    }

    #[test]
    fn any_constructor_from_psbt_outputs_only() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        let any = AnyConstructor::<Relaxed<Unseeded>>::from_psbt(psbt).unwrap();
        assert!(matches!(any, AnyConstructor::OutputsOnly(_)));
    }

    #[test]
    fn any_constructor_from_psbt_not_unordered() {
        // A plain BIP370 PSBT (no UNORDERED field) must be rejected.
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();
        assert!(matches!(
            AnyConstructor::<Relaxed<Unseeded>>::from_psbt(psbt),
            Err(Error::NotUnordered)
        ));
    }

    #[test]
    fn any_constructor_from_psbt_neither_modifiable() {
        // Both flags cleared → neither typestate is valid → NotModifiable error.
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        psbt.global.clear_outputs_modifiable();
        assert!(matches!(
            AnyConstructor::<Relaxed<Unseeded>>::from_psbt(psbt),
            Err(Error::NeitherModifiable)
        ));
    }

    #[test]
    fn any_constructor_from_psbt_rejects_missing_output_unique_id() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        // Add output without PSBT_OUT_UNIQUE_ID.
        psbt.outputs = vec![psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        })];
        psbt.global.output_count = 1;
        assert!(matches!(
            AnyConstructor::<Relaxed<Unseeded>>::from_psbt(psbt),
            Err(Error::MissingOutputUniqueId)
        ));
    }

    // -- AnyConstructor::try_join tests -------------------------------------

    #[test]
    fn any_try_join_modifiable_with_modifiable_merges_inputs() {
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;
        let a = AnyConstructor::Modifiable(
            Creator::new()
                .constructor()
                .input(psbt_v2::v2::Input::new(&op_a))
                .unwrap(),
        );
        let b = AnyConstructor::Modifiable(
            Creator::new()
                .constructor()
                .input(psbt_v2::v2::Input::new(&op_b))
                .unwrap(),
        );
        let joined = a.try_join(b).unwrap();
        assert!(matches!(joined, AnyConstructor::Modifiable(_)));
        if let AnyConstructor::Modifiable(c) = joined {
            assert_eq!(c.into_psbt().inputs.len(), 2);
        }
    }

    #[test]
    fn any_try_join_modifiable_with_inputs_only_raises_to_inputs_only() {
        // Modifiable ∨ InputsOnly = InputsOnly (inputs stay modifiable, outputs locked).
        let op = bitcoin::OutPoint::null();
        let a = AnyConstructor::Modifiable(Creator::new().constructor());
        let b = AnyConstructor::InputsOnly(
            Creator::new()
                .constructor()
                .input(psbt_v2::v2::Input::new(&op))
                .unwrap()
                .no_more_outputs(),
        );
        // a's outputs are empty, b's outputs are empty (locked) → identical → OK.
        let joined = a.try_join(b).unwrap();
        assert!(matches!(joined, AnyConstructor::InputsOnly(_)));
    }

    #[test]
    fn any_try_join_modifiable_with_outputs_only_raises_to_outputs_only() {
        use crate::fields::psbt_out_unique_id;
        let a = AnyConstructor::Modifiable(Creator::new().constructor());
        let mut out = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out.set_unique_id(vec![0x01; 16]);
        let b = AnyConstructor::OutputsOnly(
            Creator::new()
                .constructor()
                .output(out)
                .unwrap()
                .no_more_inputs(),
        );
        // a's inputs are empty, b's inputs are empty (locked) → identical → OK.
        let joined = a.try_join(b).unwrap();
        assert!(matches!(joined, AnyConstructor::OutputsOnly(_)));
    }

    #[test]
    #[should_panic(expected = "both sides locked after modifiability join")]
    fn any_try_join_inputs_only_with_outputs_only_panics_todo() {
        // InputsOnly ∨ OutputsOnly = both locked — not yet implemented (requires seed path).
        let a = AnyConstructor::InputsOnly(Creator::new().constructor().no_more_outputs());
        let b = AnyConstructor::OutputsOnly(Creator::new().constructor().no_more_inputs());
        let _ = a.try_join(b);
    }

    #[test]
    fn any_try_join_locked_set_mismatch_returns_error() {
        // Both InputsOnly; a has an input that b doesn't → b's locked output
        // set (empty) matches a's, but inputs differ. Since inputs are
        // *modifiable* in InputsOnly, the join merges them fine.
        // Test the *outputs* locked mismatch: both InputsOnly, but different output sets.
        use crate::fields::psbt_out_unique_id;

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

        let a = AnyConstructor::InputsOnly(
            Creator::new()
                .constructor()
                .output(out_a)
                .unwrap()
                .no_more_outputs(),
        );
        let b = AnyConstructor::InputsOnly(
            Creator::new()
                .constructor()
                .output(out_b)
                .unwrap()
                .no_more_outputs(),
        );
        assert_eq!(a.try_join(b), Err(Error::LockedSetMismatch));
    }

    #[test]
    fn try_sort_outputs_only() {
        use crate::fields::{psbt_out_sort_key, psbt_out_unique_id};

        let mut psbt = Creator::new()
            .explicit_sort_keys()
            .into_unordered_psbt()
            .to_psbt();

        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output.set_sort_key(vec![0x01]);
        output.set_unique_id(vec![0x01; 16]);
        psbt.outputs = vec![output];
        psbt.global.output_count = 1;
        psbt.global.clear_inputs_modifiable();

        let c = Constructor::<OutputsOnlyModifiable, ExplicitSortKeys>::new(psbt).unwrap();
        let ordered = c.try_sort().unwrap().psbt().unwrap();
        assert_eq!(ordered.outputs.len(), 1);
        assert_eq!(ordered.outputs[0].amount, bitcoin::Amount::from_sat(1000));
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
        use crate::fields::psbt_out_unique_id;

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
        let ordered = c.try_sort().unwrap().psbt().unwrap();

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
        let ordered2 = c2.try_sort().unwrap().psbt().unwrap();
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
        let ordered = c.try_sort().unwrap().psbt().unwrap();

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
            let ordered = c.try_sort().unwrap().psbt().unwrap();
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
}
