//! Runtime-typed Constructor: [`AnyConstructor`].
//!
//! `crate::constructor` provides static typestates; this module provides the
//! dynamic counterpart used when the PSBT's flags are not known a priori.

use psbt_v2::v2::{InputsOnlyModifiable, Mod, Modifiable, OutputsOnlyModifiable, Psbt};

use crate::constructor::{Constructor, Error};
use crate::fields::{GlobalFieldsExt as _, GlobalModifiableExt as _};
use crate::output::OutputExt as _;
use crate::sort::{
    Deterministic, ExplicitSortKeys, Relaxed, Seeded, SortMode, Unseeded,
};
use crate::tx::UnorderedPsbt;

// Silence unused-import warnings for sort-mode types used only as type params.
#[allow(unused_imports)]
use crate::sort::{Sortable as _, TrySortable as _};

// -- AnyModifiability --------------------------------------------------------

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

// -- AnySortMode -------------------------------------------------------------

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

// -- IntoConstructorError ----------------------------------------------------

/// Error produced by [`AnyConstructor::try_into_constructor`] when the PSBT's
/// runtime flags don't match the requested static typestate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntoConstructorError {
    #[error("PSBT modifiability flags do not match the requested Constructor<M, _> type")]
    ModifiabilityMismatch,
    #[error("PSBT sort mode flags do not match the requested Constructor<_, S> type")]
    SortModeMismatch,
}

// -- ModifiabilityMarker / SortModeMarker ------------------------------------

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

// -- AnyConstructor ----------------------------------------------------------

/// An unordered Constructor whose modifiability and sort-mode typestates are
/// determined at runtime from the PSBT flags.
///
/// Use [`AnyConstructor::from_psbt`] when you don't know the typestate a
/// priori. Inspect [`AnyConstructor::modifiable`] and
/// [`AnyConstructor::sort_mode`] to decide, then call
/// [`AnyConstructor::try_into_constructor`] to obtain a static
/// `crate::constructor::Constructor<M, S>`.
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
        for output in &psbt.outputs {
            if !output.has_unique_id() {
                return Err(Error::MissingOutputUniqueId);
            }
        }
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
            if has_seed { AnySortMode::DeterministicSeeded } else { AnySortMode::DeterministicUnseeded }
        } else if has_seed {
            AnySortMode::RelaxedSeeded
        } else {
            AnySortMode::RelaxedUnseeded
        };
        Ok(AnyConstructor { modifiable, sort_mode, psbt: unordered })
    }

    /// Attempt to convert into a static `Constructor<M, S>`.
    ///
    /// Returns `Err` if the runtime flags don't match `M` or `S`.
    /// The PSBT is returned inside the error so it isn't lost.
    pub fn try_into_constructor<M, S>(
        self,
    ) -> Result<Constructor<M, S>, (IntoConstructorError, Self)>
    where
        M: Mod,
        S: SortMode,
        M: ModifiabilityMarker,
        S: SortModeMarker,
    {
        if self.modifiable != M::ANY_MODIFIABILITY {
            return Err((IntoConstructorError::ModifiabilityMismatch, self));
        }
        if self.sort_mode != S::ANY_SORT_MODE {
            return Err((IntoConstructorError::SortModeMismatch, self));
        }
        Ok(Constructor::new_unchecked(self.psbt))
    }

    /// Merge two `AnyConstructor`s, raising both to the modifiability-lattice join.
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

        if !result_inputs_mod {
            let (locked, candidate) = if !self_inputs_mod {
                (&self.psbt.inputs, &other.psbt.inputs)
            } else {
                (&other.psbt.inputs, &self.psbt.inputs)
            };
            if !candidate.iter_outpoints().all(|op| locked.spends_outpoint(op)) {
                return Err(Error::LockedSetMismatch);
            }
        }
        if !result_outputs_mod {
            let (locked, candidate) = if !self_outputs_mod {
                (&self.psbt.outputs, &other.psbt.outputs)
            } else {
                (&other.psbt.outputs, &self.psbt.outputs)
            };
            if !candidate.iter_unique_ids().all(|id| locked.contains_unique_id(id)) {
                return Err(Error::LockedSetMismatch);
            }
        }

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
        Ok(AnyConstructor { modifiable: result_modifiable, sort_mode: self.sort_mode, psbt: joined })
    }
}
