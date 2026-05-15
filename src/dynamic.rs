//! Runtime-typed Constructor: `dynamic::Constructor`.
//!
//! `crate::constructor` provides static typestates; this module provides the
//! dynamic counterpart used when the PSBT's flags are not known a priori.

use psbt_v2::v2::{InputsOnlyModifiable, Mod, Modifiable, OutputsOnlyModifiable, Psbt};

use crate::constructor::{Constructor as StaticConstructor, Error};
use crate::fields::{GlobalFieldsExt as _, GlobalModifiableExt as _};

use crate::sort::{Deterministic, ExplicitSortKeys, Relaxed, Seeded, SortMode, Unseeded};
use crate::psbt::tx::UnorderedPsbt;

// Silence unused-import warnings for sort-mode types used only as type params.
#[allow(unused_imports)]
use crate::sort::{Sortable as _, TrySortable as _};

// -- AnyModifiability --------------------------------------------------------

/// Runtime representation of which inputs/outputs are still modifiable.
///
/// This is a lattice: `Modifiable` is the bottom (both flags set), `NotModifiable`
/// is the top (both cleared). `InputsOnly` and `OutputsOnly` are incomparable
/// intermediate elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyModifiability {
    /// Both inputs and outputs are modifiable (lattice bottom).
    Modifiable,
    /// Only inputs are modifiable (outputs locked).
    InputsOnly,
    /// Only outputs are modifiable (inputs locked).
    OutputsOnly,
    /// Neither inputs nor outputs are modifiable (lattice top).
    /// A constructor in this state can only be sorted, not modified.
    NotModifiable,
}

impl AnyModifiability {
    /// Read from the tx-modifiable flags on a [`crate::psbt::global::Global`].
    pub fn from_global(global: &psbt_v2::v2::Global) -> Self {
        use crate::fields::GlobalModifiableExt as _;
        match (global.is_inputs_modifiable(), global.is_outputs_modifiable()) {
            (true, true) => AnyModifiability::Modifiable,
            (true, false) => AnyModifiability::InputsOnly,
            (false, true) => AnyModifiability::OutputsOnly,
            (false, false) => AnyModifiability::NotModifiable,
        }
    }

    /// Lattice join: the least upper bound of `self` and `other`.
    ///
    /// Locking is monotone — a side once locked stays locked in the join.
    pub fn join(self, other: Self) -> Self {
        use AnyModifiability::*;
        match (self, other) {
            // Identical: identity element
            (Modifiable, Modifiable) => Modifiable,
            (InputsOnly, InputsOnly) => InputsOnly,
            (OutputsOnly, OutputsOnly) => OutputsOnly,
            (NotModifiable, NotModifiable) => NotModifiable,
            // Modifiable is the bottom: Modifiable ∨ x = x
            (Modifiable, x) | (x, Modifiable) => x,
            // NotModifiable is the top: NotModifiable ∨ x = NotModifiable
            (NotModifiable, _) | (_, NotModifiable) => NotModifiable,
            // Incomparable pair: InputsOnly ∨ OutputsOnly = NotModifiable
            (InputsOnly, OutputsOnly) | (OutputsOnly, InputsOnly) => NotModifiable,
        }
    }
}

// -- SeedMode ----------------------------------------------------------------

/// Whether a sort seed is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedMode {
    /// No seed set yet.
    Unseeded,
    /// Seed is present.
    Seeded,
}

// -- AnySortMode -------------------------------------------------------------

/// Runtime representation of the sort mode encoded in the PSBT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnySortMode {
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` absent — [`Relaxed<_>`].
    Relaxed(SeedMode),

    /// `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x00` — [`ExplicitSortKeys`].
    Explicit,

    /// `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01` — [`Deterministic<_>`].
    Deterministic(SeedMode),
}

// -- IntoConstructorError ----------------------------------------------------

/// Error produced by [`Constructor::try_into_constructor`] when the PSBT's
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
    const ANY_SORT_MODE: AnySortMode = AnySortMode::Relaxed(SeedMode::Unseeded);
}
impl SortModeMarker for Relaxed<Seeded> {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::Relaxed(SeedMode::Seeded);
}
impl SortModeMarker for ExplicitSortKeys {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::Explicit;
}
impl SortModeMarker for Deterministic<Unseeded> {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::Deterministic(SeedMode::Unseeded);
}
impl SortModeMarker for Deterministic<Seeded> {
    const ANY_SORT_MODE: AnySortMode = AnySortMode::Deterministic(SeedMode::Seeded);
}

// -- dynamic::Constructor ----------------------------------------------------------

/// An unordered Constructor whose modifiability and sort-mode typestates are
/// determined at runtime from the PSBT flags.
///
/// Use [`Constructor::from_psbt`] when you don't know the typestate a
/// priori. Inspect [`Constructor::modifiable`] and
/// [`Constructor::sort_mode`] to decide, then call
/// [`Constructor::try_into_constructor`] to obtain a static
/// `crate::constructor::Constructor<M, S>`.
#[derive(Debug, PartialEq, Eq)]
pub struct Constructor {
    /// Which inputs/outputs are still modifiable.
    pub modifiable: AnyModifiability,
    /// The sort mode in effect.
    pub sort_mode: AnySortMode,
    /// The underlying unordered PSBT (consistent with the two fields above).
    // TODO: becomes `pub` when UnorderedPsbt is published.
    pub(crate) psbt: UnorderedPsbt,
}

impl Constructor {
    /// Construct from a raw `Psbt`, reading all flags at runtime.
    ///
    /// Errors:
    /// - [`Error::NotUnordered`] — `PSBT_GLOBAL_TX_UNORDERED` is absent or wrong.
    /// - [`Error::MissingOutputUniqueId`] — an output lacks `PSBT_OUT_UNIQUE_ID`.
    ///
    /// When both modifiable flags are cleared, `modifiable` is set to
    /// [`AnyModifiability::NotModifiable`] and `Ok` is still returned.
    pub fn from_psbt(psbt: Psbt) -> Result<Self, Error> {
        use crate::psbt::psbt_ext::PsbtExt as _;
        psbt.validate_all_outputs_have_unique_ids()?;
        let unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        let modifiable = AnyModifiability::from_global(&unordered.global);
        let has_seed = unordered.global.deterministic_sort_seed().is_some();
        let sort_mode = if unordered.global.is_sort_explicit() {
            AnySortMode::Explicit
        } else if unordered.global.is_sort_deterministic() {
            if has_seed {
                AnySortMode::Deterministic(SeedMode::Seeded)
            } else {
                AnySortMode::Deterministic(SeedMode::Unseeded)
            }
        } else if has_seed {
            AnySortMode::Relaxed(SeedMode::Seeded)
        } else {
            AnySortMode::Relaxed(SeedMode::Unseeded)
        };
        Ok(Constructor {
            modifiable,
            sort_mode,
            psbt: unordered,
        })
    }

    /// Attempt to convert into a static `Constructor<M, S>`.
    ///
    /// Returns `Err` if the runtime flags don't match `M` or `S`.
    /// The PSBT is returned inside the error so it isn't lost.
    pub fn try_into_constructor<M, S>(
        self,
    ) -> Result<StaticConstructor<M, S>, (IntoConstructorError, Self)>
    where
        M: Mod,
        S: SortMode,
        M: ModifiabilityMarker,
        S: SortModeMarker + 'static,
    {
        if self.modifiable != M::ANY_MODIFIABILITY {
            return Err((IntoConstructorError::ModifiabilityMismatch, self));
        }
        if self.sort_mode != S::ANY_SORT_MODE {
            return Err((IntoConstructorError::SortModeMismatch, self));
        }
        Ok(StaticConstructor::new_unchecked(self.psbt))
    }

    /// Convert into a `Sorter<S>` when modifiability is
    /// [`AnyModifiability::NotModifiable`].
    ///
    /// Returns `Err(self)` if the constructor is still modifiable (use
    /// [`StaticConstructor::into_sorter`] after downcasting via
    /// [`Self::try_into_constructor`] instead).
    ///
    /// The sort mode `S` must match [`Self::sort_mode`]; this is not validated
    /// — use [`Self::sort_mode`] to inspect before calling.
    pub fn try_into_sorter<S: crate::sort::SortMode>(
        self,
    ) -> Result<crate::sort::Sorter<S>, Self> {
        if self.modifiable != AnyModifiability::NotModifiable {
            return Err(self);
        }
        Ok(crate::sort::Sorter::new_unchecked(self.psbt))
    }

    /// Merge two `dynamic::Constructor`s, raising both to the modifiability-lattice join.
    ///
    /// Uses [`AnyModifiability::join`] to compute the result modifiability.
    /// For each locked side the locked set must be a superset of the other's.
    /// Returns [`Error::JoinConflict`] on field-level conflicts.
    pub fn try_join(self, other: Self) -> Result<Self, Error> {
        let result_modifiable = self.modifiable.join(other.modifiable);

        let self_inputs_locked = matches!(self.modifiable,
            AnyModifiability::OutputsOnly | AnyModifiability::NotModifiable);
        let self_outputs_locked = matches!(self.modifiable,
            AnyModifiability::InputsOnly | AnyModifiability::NotModifiable);


        let result_inputs_locked = matches!(result_modifiable,
            AnyModifiability::OutputsOnly | AnyModifiability::NotModifiable);
        let result_outputs_locked = matches!(result_modifiable,
            AnyModifiability::InputsOnly | AnyModifiability::NotModifiable);

        // For each locked side: the locked constructor's set must be a superset
        // of the other's.
        if result_inputs_locked {
            let (locked, candidate) = if self_inputs_locked {
                (&self.psbt.inputs, &other.psbt.inputs)
            } else {
                (&other.psbt.inputs, &self.psbt.inputs)
            };
            if !candidate.iter_outpoints().all(|op| locked.spends_outpoint(op)) {
                return Err(Error::LockedSetMismatch);
            }
        }
        if result_outputs_locked {
            let (locked, candidate) = if self_outputs_locked {
                (&self.psbt.outputs, &other.psbt.outputs)
            } else {
                (&other.psbt.outputs, &self.psbt.outputs)
            };
            if !candidate.iter_unique_ids().all(|id| locked.contains_unique_id(id)) {
                return Err(Error::LockedSetMismatch);
            }
        }

        // Raise both to the result modifiability by syncing flags, then join.
        let mut a = self.psbt;
        let mut b = other.psbt;
        if result_inputs_locked {
            a.global.clear_inputs_modifiable();
            b.global.clear_inputs_modifiable();
        }
        if result_outputs_locked {
            a.global.clear_outputs_modifiable();
            b.global.clear_outputs_modifiable();
        }

        let joined = a.try_join(b).map_err(|crate::psbt::tx::JoinError(c)| Error::JoinConflict(c))?;
        Ok(Constructor { modifiable: result_modifiable, sort_mode: self.sort_mode, psbt: joined })
    }
}

#[cfg(test)]
mod tests {
    use super::Constructor as DynConstructor;
    use super::*;
    use crate::constructor::{Constructor, Error, ExplicitSortKeys, Relaxed, Unseeded};
    use crate::creator::Creator;
    use crate::psbt::output::OutputExt as _;

    use psbt_v2::v2::{
        Creator as Bip370Creator, InputsOnlyModifiable, Mod, Modifiable, OutputsOnlyModifiable,
    };

    fn any<M: Mod + ModifiabilityMarker, S: SortMode + SortModeMarker + 'static>(
        c: Constructor<M, S>,
    ) -> DynConstructor {
        DynConstructor {
            modifiable: M::ANY_MODIFIABILITY,
            sort_mode: S::ANY_SORT_MODE,
            psbt: c.into_psbt(),
        }
    }

    #[test]
    fn any_constructor_from_psbt_fully_modifiable() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = DynConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.modifiable, AnyModifiability::Modifiable);
        assert_eq!(a.sort_mode, AnySortMode::Relaxed(SeedMode::Unseeded));
    }

    #[test]
    fn any_constructor_from_psbt_inputs_only() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_outputs_modifiable();
        let a = DynConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.modifiable, AnyModifiability::InputsOnly);
    }

    #[test]
    fn any_constructor_from_psbt_outputs_only() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        let a = DynConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.modifiable, AnyModifiability::OutputsOnly);
    }

    #[test]
    fn any_constructor_from_psbt_explicit_sort_mode() {
        let psbt = Creator::new()
            .explicit_sort_keys()
            .into_unordered_psbt()
            .to_psbt();
        let a = DynConstructor::from_psbt(psbt).unwrap();
        assert_eq!(a.sort_mode, AnySortMode::Explicit);
    }

    #[test]
    fn any_constructor_from_psbt_not_unordered() {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();
        assert!(matches!(
            DynConstructor::from_psbt(psbt),
            Err(Error::NotUnordered)
        ));
    }

    #[test]
    fn any_constructor_from_psbt_neither_modifiable() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        psbt.global.clear_outputs_modifiable();
        let c = DynConstructor::from_psbt(psbt).unwrap();
        assert_eq!(c.modifiable, AnyModifiability::NotModifiable);
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
            DynConstructor::from_psbt(psbt),
            Err(Error::MissingOutputUniqueId)
        ));
    }

    #[test]
    fn any_constructor_try_into_constructor_succeeds() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = DynConstructor::from_psbt(psbt).unwrap();
        let c: Constructor<Modifiable, Relaxed<Unseeded>> = a.try_into_constructor().unwrap();
        assert!(c.into_psbt().is_unordered());
    }

    #[test]
    fn any_constructor_try_into_constructor_wrong_modifiability() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = DynConstructor::from_psbt(psbt).unwrap();
        let result = a.try_into_constructor::<InputsOnlyModifiable, Relaxed<Unseeded>>();
        assert!(matches!(
            result,
            Err((IntoConstructorError::ModifiabilityMismatch, _))
        ));
    }

    #[test]
    fn any_constructor_try_into_constructor_wrong_sort_mode() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let a = DynConstructor::from_psbt(psbt).unwrap();
        let result = a.try_into_constructor::<Modifiable, ExplicitSortKeys>();
        assert!(matches!(
            result,
            Err((IntoConstructorError::SortModeMismatch, _))
        ));
    }

    // -- Constructor::try_join tests -------------------------------------

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
    fn any_try_join_inputs_only_with_outputs_only_yields_not_modifiable() {
        // InputsOnly ∨ OutputsOnly = NotModifiable (both sides locked).
        let a = any(Creator::new().constructor().no_more_outputs());
        let b = any(Creator::new().constructor().no_more_inputs());
        let joined = a.try_join(b).unwrap();
        assert_eq!(joined.modifiable, AnyModifiability::NotModifiable);
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
        let ordered = c.try_sort().unwrap();
        assert_eq!(ordered.outputs.len(), 1);
        assert_eq!(ordered.outputs[0].amount, bitcoin::Amount::from_sat(1000));
    }

    #[test]
    fn try_into_sorter_succeeds_when_not_modifiable() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        psbt.global.clear_outputs_modifiable();
        let c = DynConstructor::from_psbt(psbt).unwrap();
        assert_eq!(c.modifiable, AnyModifiability::NotModifiable);
        let sorter = c
            .try_into_sorter::<crate::sort::Relaxed<crate::sort::Unseeded>>()
            .unwrap();
        // Both flags cleared in the recovered PSBT
        let unordered = sorter.into_psbt();
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn try_into_sorter_fails_when_still_modifiable() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let c = DynConstructor::from_psbt(psbt).unwrap();
        assert_eq!(c.modifiable, AnyModifiability::Modifiable);
        let result =
            c.try_into_sorter::<crate::sort::Relaxed<crate::sort::Unseeded>>();
        assert!(result.is_err());
    }
}
