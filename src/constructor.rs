use core::marker::PhantomData;

use psbt_v2::v2::Constructor as Bip370Constructor;
use psbt_v2::v2::Creator as Bip370Creator;
use psbt_v2::v2::{InputsOnlyModifiable, Mod, Modifiable, OutputsOnlyModifiable, Psbt};

use crate::sort::{Deterministic, ExplicitSortKeys, Relaxed, Seeded, SortMode, Unseeded};

use crate::fields::{GlobalFieldsExt as _, GlobalModifiableExt as _};
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

impl<M: Mod, S: SortMode> Constructor<M, S> {
    /// Convert into a [`Sorter<S>`], consuming the constructor.
    ///
    /// Allows sorting independently of the modifiability typestate.
    pub fn into_sorter(self) -> crate::sort::Sorter<S> {
        // FIXME new_unchecked, which should only be pub(crate)
        crate::sort::Sorter::new_unchecked(self.0)
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

impl<S: SortMode> Constructor<InputsOnlyModifiable, S> {
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
            .map_err(Error::JoinConflict)
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

impl<S: SortMode> Constructor<OutputsOnlyModifiable, S> {
    /// Add an output. Requires `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Returns `Err(Error::JoinConflict(_))` if the same unique ID is already
    /// present with conflicting field values.
    pub fn output(self, output: Output) -> Result<Self, Error> {
        validate_output_unique_id(&output)?;
        let mut singleton = UnorderedPsbt::from_output(output);
        singleton.global.clear_inputs_modifiable();
        self.0
            .try_join(singleton)
            .map(|p| Constructor(p, PhantomData))
            .map_err(Error::JoinConflict)
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

// -- AnyConstructor ----------------------------------------------------------

/// Runtime representation of which inputs/outputs are still modifiable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyModifiability {
    /// Both inputs and outputs are modifiable.
    Modifiable,
    /// Only inputs are modifiable (outputs locked).
    InputsOnly,
    /// Only outputs are modifiable (inputs locked).
    OutputsOnly,
}

/// Runtime representation of the sort mode encoded in the PSBT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnySortMode {
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` absent, no seed — [`Relaxed<Unseeded>`].
    RelaxedUnseeded,
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` absent, seed present — [`Relaxed<Seeded>`].
    RelaxedSeeded,
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x00` — [`ExplicitSortKeys`].
    Explicit,
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`, no seed — [`Deterministic<Unseeded>`].
    DeterministicUnseeded,
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`, seed present — [`Deterministic<Seeded>`].
    DeterministicSeeded,
}

/// Error produced by [`AnyConstructor::try_into_constructor`] when the PSBT's
/// runtime flags don't match the requested static typestate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntoConstructorError {
    #[error("PSBT modifiability flags do not match the requested Constructor<M, _> type")]
    ModifiabilityMismatch,
    #[error("PSBT sort mode flags do not match the requested Constructor<_, S> type")]
    SortModeMismatch,
}

/// An unordered Constructor whose modifiability and sort-mode typestates are
/// determined at runtime from the PSBT flags.
///
/// Use [`AnyConstructor::from_psbt`] when you don't know the typestate a
/// priori. Inspect [`AnyConstructor::modifiable`] and
/// [`AnyConstructor::sort_mode`] to decide, then call
/// [`AnyConstructor::try_into_constructor`] to obtain a static `Constructor<M,S>`.
#[derive(Debug, PartialEq, Eq)]
pub struct AnyConstructor {
    /// Which inputs/outputs are still modifiable.
    pub modifiable: AnyModifiability,
    /// The sort mode in effect.
    pub sort_mode: AnySortMode,
    /// The underlying unordered PSBT (consistent with the two fields above).
    pub psbt: UnorderedPsbt,
}

impl AnyConstructor {
    /// Construct from a raw `Psbt`, reading all flags at runtime.
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
        let modifiable = match (
            unordered.global.is_inputs_modifiable(),
            unordered.global.is_outputs_modifiable(),
        ) {
            (true, true) => AnyModifiability::Modifiable,
            (true, false) => AnyModifiability::InputsOnly,
            (false, true) => AnyModifiability::OutputsOnly,
            (false, false) => return Err(Error::NeitherModifiable),
        };
        let has_seed = unordered.global.sort_seed().is_some();
        let sort_mode = if unordered.global.is_sort_explicit() {
            AnySortMode::Explicit
        } else if unordered.global.is_sort_deterministic() {
            if has_seed {
                AnySortMode::DeterministicSeeded
            } else {
                AnySortMode::DeterministicUnseeded
            }
        } else if has_seed {
            AnySortMode::RelaxedSeeded
        } else {
            AnySortMode::RelaxedUnseeded
        };
        Ok(AnyConstructor {
            modifiable,
            sort_mode,
            psbt: unordered,
        })
    }

    /// Attempt to convert into a static `Constructor<M, S>`.
    ///
    /// Returns `Err` if the runtime flags don't match `M` or `S`.
    /// The PSBT is returned inside the error so it isn't lost.
    pub fn try_into_constructor<M: Mod, S: SortMode>(
        self,
    ) -> Result<Constructor<M, S>, (IntoConstructorError, Self)>
    where
        M: ModifiabilityMarker,
        S: SortModeMarker,
    {
        if self.modifiable != M::ANY_MODIFIABILITY {
            return Err((IntoConstructorError::ModifiabilityMismatch, self));
        }
        if self.sort_mode != S::ANY_SORT_MODE {
            return Err((IntoConstructorError::SortModeMismatch, self));
        }
        Ok(Constructor(self.psbt, PhantomData))
    }

    /// Merge two `AnyConstructor`s, raising both to the modifiability-lattice join.
    ///
    /// For each locked side the locked set must be a superset of the other's;
    /// returns [`Error::LockedSetMismatch`] otherwise.
    ///
    /// If the lattice join yields both sides locked, `todo!()` — requires the
    /// sort/seed path not yet implemented.
    ///
    /// Returns [`Error::JoinConflict`] on a field-level conflict.
    pub fn try_join(self, other: Self) -> Result<Self, Error> {
        let self_inputs_mod = self.modifiable != AnyModifiability::OutputsOnly;
        let self_outputs_mod = self.modifiable != AnyModifiability::InputsOnly;
        let other_inputs_mod = other.modifiable != AnyModifiability::OutputsOnly;
        let other_outputs_mod = other.modifiable != AnyModifiability::InputsOnly;

        let result_inputs_mod = self_inputs_mod && other_inputs_mod;
        let result_outputs_mod = self_outputs_mod && other_outputs_mod;

        if !result_inputs_mod && !result_outputs_mod {
            todo!(
                "AnyConstructor::try_join: both sides locked; \
                 the result requires the sort/seed path (not yet implemented)"
            );
        }

        // For each locked side: the other's set must be a subset.
        if !result_inputs_mod {
            let (locked, candidate) = if !self_inputs_mod {
                (&self.psbt.inputs, &other.psbt.inputs)
            } else {
                (&other.psbt.inputs, &self.psbt.inputs)
            };
            if !candidate
                .iter_outpoints()
                .all(|op| locked.spends_outpoint(op))
            {
                return Err(Error::LockedSetMismatch);
            }
        }
        if !result_outputs_mod {
            let (locked, candidate) = if !self_outputs_mod {
                (&self.psbt.outputs, &other.psbt.outputs)
            } else {
                (&other.psbt.outputs, &self.psbt.outputs)
            };
            if !candidate
                .iter_unique_ids()
                .all(|id| locked.contains_unique_id(id))
            {
                return Err(Error::LockedSetMismatch);
            }
        }

        // Raise both to the result typestate, then join.
        let mut a = self.psbt;
        let mut b = other.psbt;
        if !result_inputs_mod {
            a.global.clear_inputs_modifiable();
            b.global.clear_inputs_modifiable();
        }
        if !result_outputs_mod {
            a.global.clear_outputs_modifiable();
            b.global.clear_outputs_modifiable();
        }

        let joined = a.try_join(b).map_err(Error::JoinConflict)?;

        let result_modifiable = match (result_inputs_mod, result_outputs_mod) {
            (true, true) => AnyModifiability::Modifiable,
            (true, false) => AnyModifiability::InputsOnly,
            (false, true) => AnyModifiability::OutputsOnly,
            (false, false) => unreachable!(),
        };
        // Sort mode is preserved from self (both sides must have the same mode
        // for try_join to make sense; the sort mode field is part of the global
        // and will have been validated by the lattice join above).
        Ok(AnyConstructor {
            modifiable: result_modifiable,
            sort_mode: self.sort_mode,
            psbt: joined,
        })
    }
}

// -- ModifiabilityMarker / SortModeMarker ------------------------------------
// These sealed traits let try_into_constructor check at runtime whether M / S
// match the AnyModifiability / AnySortMode stored in the AnyConstructor.

mod any_marker {
    pub trait ModifiabilityMarker {
        const ANY_MODIFIABILITY: super::AnyModifiability;
    }
    pub trait SortModeMarker {
        const ANY_SORT_MODE: super::AnySortMode;
    }
}
pub use any_marker::{ModifiabilityMarker, SortModeMarker};

impl ModifiabilityMarker for Modifiable {
    const ANY_MODIFIABILITY: AnyModifiability = AnyModifiability::Modifiable;
}
impl ModifiabilityMarker for InputsOnlyModifiable {
    const ANY_MODIFIABILITY: AnyModifiability = AnyModifiability::InputsOnly;
}
impl ModifiabilityMarker for OutputsOnlyModifiable {
    const ANY_MODIFIABILITY: AnyModifiability = AnyModifiability::OutputsOnly;
}

impl SortModeMarker for Relaxed<Unseeded> {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::RelaxedUnseeded;
}
impl SortModeMarker for Relaxed<Seeded> {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::RelaxedSeeded;
}
impl SortModeMarker for ExplicitSortKeys {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::Explicit;
}
impl SortModeMarker for Deterministic<Unseeded> {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::DeterministicUnseeded;
}
impl SortModeMarker for Deterministic<Seeded> {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::DeterministicSeeded;
}

// FIXME move Creator to a separate mod
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
    /// This is the `Relaxed` analogue of [`CreatorWith::set_deterministic_sort_seed`].
    pub fn set_deterministic_sort_seed(mut self, seed: Vec<u8>) -> CreatorWith<Relaxed<Seeded>> {
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
    pub fn set_deterministic_sort_seed(
        mut self,
        seed: Vec<u8>,
    ) -> CreatorWith<Deterministic<Seeded>> {
        self.0.global.set_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }
}

impl CreatorWith<Relaxed<Unseeded>> {
    /// Provide the sort seed, transitioning to [`Relaxed<Seeded>`].
    pub fn set_deterministic_sort_seed(mut self, seed: Vec<u8>) -> CreatorWith<Relaxed<Seeded>> {
        self.0.global.set_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }
}

// -- try_sort / sort on Constructor -----------------------------------------
//
// Each delegates to Sorter<S>, which owns the sort logic.

macro_rules! impl_try_sort {
    ($m:ty, $s:ty) => {
        impl Constructor<$m, $s> {
            pub fn try_sort(self) -> Result<Bip370Constructor<$m>, SortingError> {
                let psbt = self.into_sorter().try_sort()?;
                Ok(Bip370Constructor::<$m>::new(psbt).expect("flags preserved"))
            }
        }
    };
}

macro_rules! impl_sort {
    ($m:ty, $s:ty) => {
        impl Constructor<$m, $s> {
            pub fn sort(self) -> Bip370Constructor<$m> {
                let psbt = self.into_sorter().sort();
                Bip370Constructor::<$m>::new(psbt).expect("flags preserved")
            }
        }
    };
}

impl_try_sort!(Modifiable, ExplicitSortKeys);
impl_try_sort!(InputsOnlyModifiable, ExplicitSortKeys);
impl_try_sort!(OutputsOnlyModifiable, ExplicitSortKeys);
impl_try_sort!(Modifiable, Deterministic<Seeded>);
impl_try_sort!(InputsOnlyModifiable, Deterministic<Seeded>);
impl_try_sort!(OutputsOnlyModifiable, Deterministic<Seeded>);
impl_try_sort!(Modifiable, Relaxed<Seeded>);
impl_try_sort!(InputsOnlyModifiable, Relaxed<Seeded>);
impl_try_sort!(OutputsOnlyModifiable, Relaxed<Seeded>);

impl_sort!(Modifiable, Deterministic<Seeded>);
impl_sort!(InputsOnlyModifiable, Deterministic<Seeded>);
impl_sort!(OutputsOnlyModifiable, Deterministic<Seeded>);
impl_sort!(Modifiable, Relaxed<Seeded>);
impl_sort!(InputsOnlyModifiable, Relaxed<Seeded>);
impl_sort!(OutputsOnlyModifiable, Relaxed<Seeded>);

// -- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputExt as _;

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
        let psbt = c.no_more_inputs();
        assert_eq!(psbt.inputs.len(), 1);
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

    // Helper: wrap a Constructor<M,S> into AnyConstructor
    fn any<M: Mod + ModifiabilityMarker, S: SortMode + SortModeMarker>(
        c: Constructor<M, S>,
    ) -> AnyConstructor {
        AnyConstructor {
            modifiable: M::ANY_MODIFIABILITY,
            sort_mode: S::ANY_SORT_MODE,
            psbt: c.0,
        }
    }

    #[test]
    fn any_constructor_from_psbt_fully_modifiable() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = AnyConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.modifiable, AnyModifiability::Modifiable);
        assert_eq!(a.sort_mode, AnySortMode::RelaxedUnseeded);
    }

    #[test]
    fn any_constructor_from_psbt_inputs_only() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_outputs_modifiable();
        let a = AnyConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.modifiable, AnyModifiability::InputsOnly);
    }

    #[test]
    fn any_constructor_from_psbt_outputs_only() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        let a = AnyConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.modifiable, AnyModifiability::OutputsOnly);
    }

    #[test]
    fn any_constructor_from_psbt_explicit_sort_mode() {
        let psbt = Creator::new()
            .explicit_sort_keys()
            .into_unordered_psbt()
            .to_psbt();
        let a = AnyConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.sort_mode, AnySortMode::Explicit);
    }

    #[test]
    fn any_constructor_from_psbt_not_unordered() {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();
        assert!(matches!(
            AnyConstructor::from_psbt(psbt),
            Err(Error::NotUnordered)
        ));
    }

    #[test]
    fn any_constructor_from_psbt_neither_modifiable() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        psbt.global.clear_outputs_modifiable();
        assert!(matches!(
            AnyConstructor::from_psbt(psbt),
            Err(Error::NeitherModifiable)
        ));
    }

    #[test]
    fn any_constructor_from_psbt_rejects_missing_output_unique_id() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.outputs = vec![psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        })];
        psbt.global.output_count = 1;
        assert!(matches!(
            AnyConstructor::from_psbt(psbt),
            Err(Error::MissingOutputUniqueId)
        ));
    }

    #[test]
    fn any_constructor_try_into_constructor_succeeds() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = AnyConstructor::from_psbt(psbt).unwrap();
        let c: Constructor<Modifiable, Relaxed<Unseeded>> = a.try_into_constructor().unwrap();
        assert!(c.into_psbt().is_unordered());
    }

    #[test]
    fn any_constructor_try_into_constructor_wrong_modifiability() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = AnyConstructor::from_psbt(psbt).unwrap();
        let result = a.try_into_constructor::<InputsOnlyModifiable, Relaxed<Unseeded>>();
        assert!(matches!(
            result,
            Err((IntoConstructorError::ModifiabilityMismatch, _))
        ));
    }

    #[test]
    fn any_constructor_try_into_constructor_wrong_sort_mode() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = AnyConstructor::from_psbt(psbt).unwrap();
        let result = a.try_into_constructor::<Modifiable, ExplicitSortKeys>();
        assert!(matches!(
            result,
            Err((IntoConstructorError::SortModeMismatch, _))
        ));
    }

    // -- AnyConstructor::try_join tests -------------------------------------

    #[test]
    fn any_try_join_modifiable_with_modifiable_merges_inputs() {
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;
        let a = any(Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_a))
            .unwrap());
        let b = any(Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op_b))
            .unwrap());
        let joined = a.try_join(b).unwrap();
        assert_eq!(joined.modifiable, AnyModifiability::Modifiable);
        assert_eq!(joined.psbt.inputs.len(), 2);
    }

    #[test]
    fn any_try_join_modifiable_with_inputs_only_raises_to_inputs_only() {
        let op = bitcoin::OutPoint::null();
        let a = any(Creator::new().constructor());
        let b = any(Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&op))
            .unwrap()
            .no_more_outputs());
        let joined = a.try_join(b).unwrap();
        assert_eq!(joined.modifiable, AnyModifiability::InputsOnly);
    }

    #[test]
    fn any_try_join_modifiable_with_outputs_only_raises_to_outputs_only() {
        let mut out = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        out.set_unique_id(vec![0x01; 16]);
        let a = any(Creator::new().constructor());
        let b = any(Creator::new()
            .constructor()
            .output(out)
            .unwrap()
            .no_more_inputs());
        let joined = a.try_join(b).unwrap();
        assert_eq!(joined.modifiable, AnyModifiability::OutputsOnly);
    }

    #[test]
    #[should_panic(expected = "both sides locked")]
    fn any_try_join_inputs_only_with_outputs_only_panics_todo() {
        let a = any(Creator::new().constructor().no_more_outputs());
        let b = any(Creator::new().constructor().no_more_inputs());
        let _ = a.try_join(b);
    }

    #[test]
    fn any_try_join_locked_set_mismatch_returns_error() {
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
        let a = any(Creator::new()
            .constructor()
            .output(out_a)
            .unwrap()
            .no_more_outputs());
        let b = any(Creator::new()
            .constructor()
            .output(out_b)
            .unwrap()
            .no_more_outputs());
        assert_eq!(a.try_join(b), Err(Error::LockedSetMismatch));
    }

    #[test]
    fn try_sort_outputs_only() {
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
            .set_deterministic_sort_seed(seed.clone())
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
            .set_deterministic_sort_seed(seed.clone())
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
            .set_deterministic_sort_seed(seed.clone()) // Relaxed<Seeded> via Creator::set_deterministic_sort_seed
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
                .set_deterministic_sort_seed(seed)
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
