//! The Sorter role: orders the inputs and outputs of an unordered PSBT.

use std::marker::PhantomData;

use psbt_v2::v2::Psbt;

use crate::global::{SortKeyDerivation, SortSeed};
use crate::tx::UnorderedPsbt;

mod error;
mod key;
mod modifiability;
mod order;

pub use error::{FlagsMismatch, SortError, SortModeError, SorterError};
pub use key::{SortKeyHash, SortKeyTag};
use key::{
    derived_input_sort_key, derived_output_sort_key, explicit_input_sort_key,
    explicit_output_sort_key, mixed_input_sort_key, mixed_output_sort_key,
};
pub(crate) use modifiability::check_flags;
pub use modifiability::{
    BothModifiable, InputsModifiable, Modifiability, Modifiable, OutputsModifiable, Unmodifiable,
};
use order::ordered_psbt;

/// Hand an ordered PSBT to the role that follows sorting under `M`.
fn hand_off<M: Modifiability>(psbt: Psbt) -> Result<M::Sorted, M::Error> {
    <M as modifiability::sealed::Modifiability>::sorted(psbt)
}

/// Sort mode: deterministic ordering seeded by a high-entropy value.
///
/// NOT BIP 69. The ordering is derived from a seed (e.g. a shared secret
/// or transaction-specific nonce) so that all participants produce the
/// same order without revealing sort criteria to observers.
#[derive(Debug, Clone, PartialEq)]
pub enum DerivedOnly {}

/// Sort mode: ordering determined by explicit sort key fields on each input/output.
#[derive(Debug, Clone, PartialEq)]
pub enum ExplicitOnly {}

/// Sort mode: explicit sort keys where present, the rest derived from the
/// sort seed.
#[derive(Debug, Clone, PartialEq)]
pub enum Mixed {}

/// Sort mode refinement: [`DerivedOnly`] or [`Mixed`] with
/// `PSBT_GLOBAL_SORT_SEED` known to be set.
#[derive(Debug, Clone, PartialEq)]
pub struct Seeded<S>(PhantomData<S>, std::convert::Infallible);

/// Typestate wrapper that applies a sort order to an [`UnorderedPsbt`].
///
/// `M` is the PSBT's [`Modifiability`], which decides the role a sorted PSBT
/// is handed to: the matching BIP 370 constructor while inputs or outputs
/// may still be added, the [`Updater`](psbt_v2::v2::Updater) once neither
/// may.
///
/// `S` is the sort mode: [`Mixed`], [`DerivedOnly`], or [`ExplicitOnly`], or
/// [`Seeded`] `Mixed` or `DerivedOnly` once the sort seed is known to be set.
///
/// # Ordering
///
/// [`UnorderedPsbt::into_psbt`] uses `HashMap` iteration order, which is
/// intentionally non-deterministic, satisfying the spec's shuffle requirement
/// for unordered PSBTs. The [`Sorter`] applies a deterministic order when
/// transitioning to an ordered BIP 370 PSBT for signing.
#[derive(Debug)]
pub struct Sorter<M, S>(pub(crate) UnorderedPsbt, PhantomData<(M, S)>);

impl<M, S> Sorter<M, S> {
    /// Wrap `psbt` without checking it against `M` and `S`; its
    /// `tx_modifiable_flags` must match `M`, and a [`Seeded`] mode requires
    /// `PSBT_GLOBAL_SORT_SEED` to be set.
    pub(crate) fn from_unordered(psbt: UnorderedPsbt) -> Self {
        Sorter(psbt, PhantomData)
    }

    /// Access the underlying [`UnorderedPsbt`].
    pub fn inner(&self) -> &UnorderedPsbt {
        &self.0
    }

    /// Consume the sorter and return the underlying [`UnorderedPsbt`].
    pub fn into_inner(self) -> UnorderedPsbt {
        self.0
    }

    fn with_seed<T>(mut self, seed: SortSeed) -> Sorter<M, Seeded<T>> {
        self.0.global.set_sort_seed(seed);
        Sorter::from_unordered(self.0)
    }
}

impl<M: Modifiability, S: SortMode> Sorter<M, S> {
    /// Create a sorter from an [`UnorderedPsbt`] of any modifiability.
    ///
    /// # Errors
    /// Returns [`SorterError::Flags`] if `tx_modifiable_flags` don't match
    /// `M`, or [`SorterError::SortMode`] if the sort fields don't select `S`.
    pub fn try_from_unordered_psbt(psbt: UnorderedPsbt) -> Result<Self, SorterError> {
        check_flags::<M>(psbt.global.tx_modifiable_flags)?;
        check_sort_mode::<S>(&psbt)?;
        Ok(Sorter::from_unordered(psbt))
    }
}

impl<M: Modifiability> Sorter<M, Mixed> {
    /// Set `PSBT_GLOBAL_SORT_SEED`, replacing any existing seed.
    pub fn set_seed(self, seed: SortSeed) -> Sorter<M, Seeded<Mixed>> {
        self.with_seed(seed)
    }

    /// Apply mixed-mode ordering.
    ///
    /// Existing explicit sort keys are used as-is. Missing sort keys are
    /// replaced with deterministic stand-ins derived from `PSBT_GLOBAL_SORT_SEED`.
    ///
    /// # Errors
    /// Returns [`SortError::MissingSortSeed`] if any input or output lacks an
    /// explicit sort key and no sort seed is set, or
    /// [`SortError::DuplicateSortKey`] if two inputs (or two outputs) end up
    /// with the same sort key.
    pub fn try_into_ordered_psbt(self) -> Result<Psbt, SortError> {
        let seed = self.0.global.sort_seed().map(|seed| seed.to_vec());
        ordered_psbt(
            self.0,
            |input| mixed_input_sort_key(seed.as_deref(), input),
            |output| mixed_output_sort_key(seed.as_deref(), output),
        )
    }

    /// Order the PSBT as [`try_into_ordered_psbt`](Self::try_into_ordered_psbt)
    /// does and hand it to [`M::Sorted`](Modifiability::Sorted).
    ///
    /// # Errors
    /// Returns the errors of [`try_into_ordered_psbt`](Self::try_into_ordered_psbt),
    /// or [`SortError::UndeterminedLockTime`] if `M` is [`Unmodifiable`] and
    /// the inputs' lock time requirements conflict.
    pub fn try_sort(self) -> Result<M::Sorted, SortError> {
        hand_off::<M>(self.try_into_ordered_psbt()?).map_err(Into::into)
    }
}

impl<M: Modifiability> Sorter<M, Seeded<Mixed>> {
    /// Apply mixed-mode ordering with the sort seed set.
    ///
    /// # Errors
    /// Returns [`SortError::DuplicateSortKey`] if two inputs (or two outputs)
    /// end up with the same sort key.
    pub fn try_into_ordered_psbt(self) -> Result<Psbt, SortError> {
        Sorter::<M, Mixed>::from_unordered(self.0).try_into_ordered_psbt()
    }

    /// Order the PSBT as [`try_into_ordered_psbt`](Self::try_into_ordered_psbt)
    /// does and hand it to [`M::Sorted`](Modifiability::Sorted).
    ///
    /// # Errors
    /// Returns the errors of [`try_into_ordered_psbt`](Self::try_into_ordered_psbt),
    /// or [`SortError::UndeterminedLockTime`] if `M` is [`Unmodifiable`] and
    /// the inputs' lock time requirements conflict.
    pub fn try_sort(self) -> Result<M::Sorted, SortError> {
        hand_off::<M>(self.try_into_ordered_psbt()?).map_err(Into::into)
    }
}

impl<M: Modifiability> Sorter<M, DerivedOnly> {
    /// Set `PSBT_GLOBAL_SORT_SEED`, replacing any existing seed.
    pub fn set_seed(self, seed: SortSeed) -> Sorter<M, Seeded<DerivedOnly>> {
        self.with_seed(seed)
    }

    /// Apply deterministic ordering if the PSBT carries a sort seed.
    ///
    /// # Errors
    /// Returns [`SortError::MissingSortSeed`] if `PSBT_GLOBAL_SORT_SEED` is not
    /// set.
    pub fn try_into_ordered_psbt(self) -> Result<Psbt, SortError> {
        match self.0.global.sort_seed() {
            Some(_) => {
                Ok(Sorter::<M, Seeded<DerivedOnly>>::from_unordered(self.0).into_ordered_psbt())
            }
            None => Err(SortError::MissingSortSeed),
        }
    }

    /// Order the PSBT as [`try_into_ordered_psbt`](Self::try_into_ordered_psbt)
    /// does and hand it to [`M::Sorted`](Modifiability::Sorted).
    ///
    /// # Errors
    /// Returns the errors of [`try_into_ordered_psbt`](Self::try_into_ordered_psbt),
    /// or [`SortError::UndeterminedLockTime`] if `M` is [`Unmodifiable`] and
    /// the inputs' lock time requirements conflict.
    pub fn try_sort(self) -> Result<M::Sorted, SortError> {
        hand_off::<M>(self.try_into_ordered_psbt()?).map_err(Into::into)
    }
}

impl<M: Modifiability> Sorter<M, Seeded<DerivedOnly>> {
    /// Apply deterministic ordering: derive sort keys from the seed, sort,
    /// and produce an ordered BIP 370 [`Psbt`].
    ///
    /// Inputs are distinct by outpoint and outputs by unique ID, so their
    /// derived sort keys differ unless SHA-256 collides.
    pub fn into_ordered_psbt(self) -> Psbt {
        let seed = self
            .0
            .global
            .sort_seed()
            .expect("a seeded sorter carries a sort seed")
            .to_vec();

        ordered_psbt(
            self.0,
            |input| derived_input_sort_key(Some(&seed), input),
            |output| derived_output_sort_key(Some(&seed), output),
        )
        .expect("derived sort keys of distinct inputs and outputs do not collide")
    }

    /// Order the PSBT as [`into_ordered_psbt`](Self::into_ordered_psbt) does
    /// and hand it to [`M::Sorted`](Modifiability::Sorted).
    ///
    /// # Errors
    /// Returns [`M::Error`](Modifiability::Error): `Infallible` while `M` is
    /// modifiable, or [`DetermineLockTimeError`](psbt_v2::v2::DetermineLockTimeError)
    /// if `M` is [`Unmodifiable`] and the inputs' lock time requirements
    /// conflict.
    pub fn try_sort(self) -> Result<M::Sorted, M::Error> {
        hand_off::<M>(self.into_ordered_psbt())
    }
}

impl<M: Modifiability> Sorter<M, ExplicitOnly> {
    /// Apply explicit ordering: sort by the `PSBT_IN_SORT_KEY` and
    /// `PSBT_OUT_SORT_KEY` fields, and produce an ordered BIP 370 [`Psbt`].
    ///
    /// # Errors
    /// Returns [`SortError::MissingInputSortKey`] or [`SortError::MissingOutputSortKey`]
    /// if any input or output is missing a sort key, or
    /// [`SortError::DuplicateSortKey`] if two inputs (or two outputs) carry the
    /// same sort key.
    pub fn try_into_ordered_psbt(self) -> Result<Psbt, SortError> {
        ordered_psbt(self.0, explicit_input_sort_key, explicit_output_sort_key)
    }

    /// Order the PSBT as [`try_into_ordered_psbt`](Self::try_into_ordered_psbt)
    /// does and hand it to [`M::Sorted`](Modifiability::Sorted).
    ///
    /// # Errors
    /// Returns the errors of [`try_into_ordered_psbt`](Self::try_into_ordered_psbt),
    /// or [`SortError::UndeterminedLockTime`] if `M` is [`Unmodifiable`] and
    /// the inputs' lock time requirements conflict.
    pub fn try_sort(self) -> Result<M::Sorted, SortError> {
        hand_off::<M>(self.try_into_ordered_psbt()?).map_err(Into::into)
    }
}

pub(crate) mod sealed {
    use crate::global::SortKeyDerivation;

    pub trait SortMode {
        /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` in this mode.
        const SORT_KEY_DERIVATION: Option<SortKeyDerivation>;
        /// Whether this mode requires `PSBT_GLOBAL_SORT_SEED`.
        const SEEDED: bool;
    }
}

/// A sort mode that a PSBT's sort fields select: [`Mixed`] if
/// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is unset, [`DerivedOnly`] if it is
/// `0x01`, [`ExplicitOnly`] if it is `0x00`, and [`Seeded`] [`Mixed`] or
/// [`DerivedOnly`] if `PSBT_GLOBAL_SORT_SEED` is set too.
pub trait SortMode: sealed::SortMode {}

macro_rules! sort_mode {
    ($($mode:ty => $sort_key_derivation:expr, $seeded:expr;)*) => {$(
        impl SortMode for $mode {}

        impl sealed::SortMode for $mode {
            const SORT_KEY_DERIVATION: Option<SortKeyDerivation> = $sort_key_derivation;
            const SEEDED: bool = $seeded;
        }
    )*};
}

sort_mode! {
    Mixed => None, false;
    Seeded<Mixed> => None, true;
    DerivedOnly => Some(SortKeyDerivation::Derived), false;
    Seeded<DerivedOnly> => Some(SortKeyDerivation::Derived), true;
    ExplicitOnly => Some(SortKeyDerivation::Explicit), false;
}

/// Check that `psbt`'s sort fields select sort mode `S`. Besides matching
/// `PSBT_GLOBAL_SORT_KEY_DERIVATION` and requiring a seed in a [`Seeded`] mode,
/// an [`ExplicitOnly`] PSBT must have no sort seed and a [`DerivedOnly`] one no
/// sort keys, since neither would be used.
///
/// # Errors
/// Returns the first [`SortModeError`] in that order.
pub(crate) fn check_sort_mode<S: SortMode>(psbt: &UnorderedPsbt) -> Result<(), SortModeError> {
    let actual = psbt.global.sort_key_derivation();
    if actual != S::SORT_KEY_DERIVATION {
        return Err(SortModeError::SortKeyDerivation {
            expected: S::SORT_KEY_DERIVATION,
            actual,
        });
    }
    let seeded = psbt.global.sort_seed().is_some();
    let sort_keyed = || {
        psbt.inputs.values().any(|input| input.sort_key().is_some())
            || psbt
                .outputs
                .values()
                .any(|output| output.sort_key().is_some())
    };
    match S::SORT_KEY_DERIVATION {
        _ if S::SEEDED && !seeded => Err(SortModeError::MissingSortSeed),
        Some(SortKeyDerivation::Explicit) if seeded => Err(SortModeError::UnexpectedSortSeed),
        Some(SortKeyDerivation::Derived) if sort_keyed() => Err(SortModeError::UnexpectedSortKey),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::global::Global;
    use crate::input::{Input, InputSet, SortKey};
    use crate::output::{Output, OutputSet, UniqueId};

    fn sort_seed(bytes: Vec<u8>) -> SortSeed {
        SortSeed::try_from(bytes).expect("test seeds are at least 16 bytes")
    }

    fn sort_key(bytes: Vec<u8>) -> SortKey {
        SortKey::try_from(bytes).expect("test sort keys are at most 32 bytes")
    }

    fn make_input(txid_byte: u8, vout: u32) -> Input {
        use bitcoin::hashes::Hash;
        Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
            vout,
        })
    }

    fn make_output_with_uid(uid: &[u8]) -> Output {
        Output::new(
            UniqueId::new(uid.to_vec()),
            bitcoin::Amount::ZERO,
            bitcoin::ScriptBuf::new(),
        )
    }

    /// Read back an output of an ordered PSBT.
    fn read_output(output: &psbt_v2::v2::Output) -> Output {
        Output::try_from(output.clone()).expect("sorting keeps the unique IDs")
    }

    /// `psbt` with `PSBT_GLOBAL_SORT_KEY_DERIVATION` set to `derivation`.
    fn with_derivation(mut psbt: UnorderedPsbt, derivation: SortKeyDerivation) -> UnorderedPsbt {
        psbt.global.set_sort_key_derivation(derivation);
        psbt
    }

    /// The error of a [`Mixed`] sorter for `flags` under modifiability `M`.
    fn flags_error<M: Modifiability>(flags: u8) -> Option<SorterError> {
        let mut psbt = empty_psbt();
        psbt.global.tx_modifiable_flags = flags;
        Sorter::<M, Mixed>::try_from_unordered_psbt(psbt).err()
    }

    fn empty_psbt() -> UnorderedPsbt {
        UnorderedPsbt {
            global: Global::default(),
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use psbt_v2::v2::Updater;

        /// Each row is the PSBT's flags and, for `Unmodifiable`,
        /// `InputsModifiable`, `OutputsModifiable` and `BothModifiable`, the
        /// flags that sorter expects, or `None` if it admits the PSBT.
        #[test]
        fn only_the_matching_flags_are_admitted() {
            let rows = [
                (0x00, [None, Some(0x01), Some(0x02), Some(0x03)]),
                (0x01, [Some(0x00), None, Some(0x02), Some(0x03)]),
                (0x02, [Some(0x00), Some(0x01), None, Some(0x03)]),
                (0x03, [Some(0x00), Some(0x01), Some(0x02), None]),
            ];
            for (actual, expected) in rows {
                let mismatch = |expected: Option<u8>| {
                    expected.map(|expected| SorterError::from(FlagsMismatch { expected, actual }))
                };
                assert_eq!(flags_error::<Unmodifiable>(actual), mismatch(expected[0]));
                assert_eq!(
                    flags_error::<InputsModifiable>(actual),
                    mismatch(expected[1])
                );
                assert_eq!(
                    flags_error::<OutputsModifiable>(actual),
                    mismatch(expected[2])
                );
                assert_eq!(flags_error::<BothModifiable>(actual), mismatch(expected[3]));
            }
        }

        /// Which entry of a PSBT in [`the_sort_fields_select_the_sort_mode`]
        /// has a sort key.
        #[derive(Debug, Clone, Copy)]
        enum SortKeyOn {
            Neither,
            Input,
            Output,
        }

        /// What a sort mode does with a PSBT.
        #[derive(Clone, Copy)]
        enum Outcome {
            Admits,
            MissingSeed,
            SeedForbidden,
            KeyForbidden,
            /// The sort key derivation does not match the one in here.
            Wants(Option<SortKeyDerivation>),
        }

        /// Each row is a PSBT's sort key derivation, whether it has a seed and
        /// which entry has a sort key, then what `Mixed`, `Seeded<Mixed>`,
        /// `DerivedOnly`, `Seeded<DerivedOnly>` and `ExplicitOnly` do with it.
        #[test]
        fn the_sort_fields_select_the_sort_mode() {
            use Outcome::{Admits, KeyForbidden, MissingSeed, SeedForbidden, Wants};
            use SortKeyOn as K;
            const U: Option<SortKeyDerivation> = None;
            const D: Option<SortKeyDerivation> = Some(SortKeyDerivation::Derived);
            const E: Option<SortKeyDerivation> = Some(SortKeyDerivation::Explicit);

            #[rustfmt::skip]
            let rows = [
                (U, false, K::Neither, [Admits, MissingSeed, Wants(D), Wants(D), Wants(E)]),
                (U, false, K::Input,   [Admits, MissingSeed, Wants(D), Wants(D), Wants(E)]),
                (U, true,  K::Neither, [Admits, Admits, Wants(D), Wants(D), Wants(E)]),
                (U, true,  K::Output,  [Admits, Admits, Wants(D), Wants(D), Wants(E)]),
                (D, false, K::Neither, [Wants(U), Wants(U), Admits, MissingSeed, Wants(E)]),
                (D, false, K::Input,   [Wants(U), Wants(U), KeyForbidden, MissingSeed, Wants(E)]),
                (D, true,  K::Neither, [Wants(U), Wants(U), Admits, Admits, Wants(E)]),
                (D, true,  K::Input,   [Wants(U), Wants(U), KeyForbidden, KeyForbidden, Wants(E)]),
                (D, true,  K::Output,  [Wants(U), Wants(U), KeyForbidden, KeyForbidden, Wants(E)]),
                (E, false, K::Neither, [Wants(U), Wants(U), Wants(D), Wants(D), Admits]),
                (E, false, K::Input,   [Wants(U), Wants(U), Wants(D), Wants(D), Admits]),
                (E, false, K::Output,  [Wants(U), Wants(U), Wants(D), Wants(D), Admits]),
                (E, true,  K::Input,   [Wants(U), Wants(U), Wants(D), Wants(D), SeedForbidden]),
            ];
            for (derivation, seeded, sort_key_on, outcomes) in rows {
                let mut psbt = empty_psbt();
                if let Some(derivation) = derivation {
                    psbt.global.set_sort_key_derivation(derivation);
                }
                if seeded {
                    psbt.global.set_sort_seed(sort_seed(vec![0; 16]));
                }
                let mut input = make_input(1, 0);
                let mut output = make_output_with_uid(&[1]);
                match sort_key_on {
                    K::Neither => {}
                    K::Input => input.set_sort_key(sort_key(vec![1])),
                    K::Output => output.set_sort_key(sort_key(vec![1])),
                }
                psbt.inputs = InputSet::try_from_inputs([input]).unwrap();
                psbt.outputs = OutputSet::try_from_outputs([output]).unwrap();

                let errors = [
                    Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt.clone()).err(),
                    Sorter::<Unmodifiable, Seeded<Mixed>>::try_from_unordered_psbt(psbt.clone())
                        .err(),
                    Sorter::<Unmodifiable, DerivedOnly>::try_from_unordered_psbt(psbt.clone())
                        .err(),
                    Sorter::<Unmodifiable, Seeded<DerivedOnly>>::try_from_unordered_psbt(
                        psbt.clone(),
                    )
                    .err(),
                    Sorter::<Unmodifiable, ExplicitOnly>::try_from_unordered_psbt(psbt).err(),
                ];
                let expected = outcomes.map(|outcome| {
                    let error = match outcome {
                        Admits => return None,
                        MissingSeed => SortModeError::MissingSortSeed,
                        SeedForbidden => SortModeError::UnexpectedSortSeed,
                        KeyForbidden => SortModeError::UnexpectedSortKey,
                        Wants(expected) => SortModeError::SortKeyDerivation {
                            expected,
                            actual: derivation,
                        },
                    };
                    Some(SorterError::from(error))
                });
                assert_eq!(
                    errors, expected,
                    "{derivation:?}, seeded: {seeded}, {sort_key_on:?}"
                );
            }
        }

        #[test]
        fn invalid_flags_are_reported_before_an_invalid_sort_mode() {
            let mut psbt = with_derivation(empty_psbt(), SortKeyDerivation::Explicit);
            psbt.global.tx_modifiable_flags = 0x01;
            assert_eq!(
                Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt).err(),
                Some(SorterError::Flags(FlagsMismatch {
                    expected: 0x00,
                    actual: 0x01
                }))
            );
        }

        /// The orders are those of `SHA-256(tag || tag || seed || identifier)`
        /// for `tag = SHA-256("concurrent-psbt/deterministic-ordering")`,
        /// computed outside this crate.
        #[test]
        fn derived_order_follows_the_tagged_hash_of_seed_and_identifier() {
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(sort_seed(vec![42; 32]));
            psbt.inputs =
                InputSet::try_from_inputs((1..=5).map(|txid| make_input(txid, 0))).unwrap();
            psbt.outputs =
                OutputSet::try_from_outputs((0..=4).map(|uid| make_output_with_uid(&[uid])))
                    .unwrap();

            let ordered = Sorter::<Unmodifiable, DerivedOnly>::try_from_unordered_psbt(
                with_derivation(psbt, SortKeyDerivation::Derived),
            )
            .unwrap()
            .try_into_ordered_psbt()
            .unwrap();

            let txids: Vec<_> = ordered
                .inputs
                .iter()
                .map(|input| input.previous_txid)
                .collect();
            let expected_txids = [1, 5, 3, 4, 2].map(|txid| make_input(txid, 0).previous_txid);
            assert_eq!(txids, expected_txids);
            let uids: Vec<_> = ordered
                .outputs
                .iter()
                .map(|output| read_output(output).unique_id().as_bytes().to_vec())
                .collect();
            assert_eq!(uids, [0, 1, 4, 2, 3].map(|uid| vec![uid]));
        }

        /// Without explicit keys the derived order of these inputs is 1, 3, 2
        /// and of these outputs 1, 2.
        #[test]
        fn mixed_orders_explicit_keys_among_derived_ones() {
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(sort_seed(vec![42; 32]));
            let [mut high, derived, mut low] = [1, 2, 3].map(|txid| make_input(txid, 0));
            high.set_sort_key(sort_key(vec![0xff]));
            low.set_sort_key(sort_key(vec![0x00]));
            psbt.inputs = InputSet::try_from_inputs([high, derived, low]).unwrap();
            let mut explicit = make_output_with_uid(&[1]);
            explicit.set_sort_key(sort_key(vec![0xfe]));
            psbt.outputs =
                OutputSet::try_from_outputs([explicit, make_output_with_uid(&[2])]).unwrap();

            let ordered = Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt)
                .unwrap()
                .try_into_ordered_psbt()
                .unwrap();

            let txids: Vec<_> = ordered
                .inputs
                .iter()
                .map(|input| input.previous_txid)
                .collect();
            assert_eq!(
                txids,
                [3, 2, 1].map(|txid| make_input(txid, 0).previous_txid)
            );
            let uids: Vec<_> = ordered
                .outputs
                .iter()
                .map(|output| read_output(output).unique_id().as_bytes().to_vec())
                .collect();
            assert_eq!(uids, [vec![2], vec![1]]);
        }

        #[test]
        fn deriving_keys_requires_a_seed() {
            let missing_seed = |psbt: UnorderedPsbt| {
                Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt)
                    .unwrap()
                    .try_into_ordered_psbt()
            };

            let mut input = empty_psbt();
            input.inputs = InputSet::try_from_inputs([make_input(1, 0)]).unwrap();
            assert!(matches!(
                missing_seed(input),
                Err(SortError::MissingSortSeed)
            ));

            let mut output = empty_psbt();
            output.outputs = OutputSet::try_from_outputs([make_output_with_uid(&[1])]).unwrap();
            assert!(matches!(
                missing_seed(output),
                Err(SortError::MissingSortSeed)
            ));

            let derived_only = Sorter::<Unmodifiable, DerivedOnly>::try_from_unordered_psbt(
                with_derivation(empty_psbt(), SortKeyDerivation::Derived),
            )
            .unwrap();
            assert!(matches!(
                derived_only.try_into_ordered_psbt(),
                Err(SortError::MissingSortSeed)
            ));
        }

        /// Every `try_sort` of a sorter that is `Unmodifiable` fails exactly
        /// when the inputs' lock time requirements conflict, and otherwise
        /// hands the ordered PSBT to the updater.
        #[test]
        fn try_sort_fails_exactly_when_lock_times_conflict() {
            fn check<E: Into<SortError>>(
                conflicting: bool,
                result: Result<Updater, E>,
                ordered: Psbt,
            ) {
                assert_eq!(result.is_err(), conflicting);
                match result {
                    Ok(updater) => assert_eq!(updater.psbt(), ordered),
                    Err(error) => {
                        assert!(matches!(error.into(), SortError::UndeterminedLockTime(_)))
                    }
                }
            }

            for conflicting in [false, true] {
                let psbt = |seeded: bool, keyed: bool| {
                    let mut psbt = empty_psbt();
                    if seeded {
                        psbt.global.set_sort_seed(sort_seed(vec![9; 16]));
                    }
                    let mut inputs = crate::test_support::conflicting_lock_time_inputs();
                    if !conflicting {
                        inputs[0].min_time = None;
                    }
                    if keyed {
                        for (key, input) in inputs.iter_mut().enumerate() {
                            input.set_sort_key(sort_key(vec![key as u8]));
                        }
                    }
                    psbt.inputs = InputSet::try_from_inputs(inputs).unwrap();
                    psbt
                };
                let mixed =
                    |psbt| Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt).unwrap();
                let derived = |psbt| {
                    Sorter::<Unmodifiable, DerivedOnly>::try_from_unordered_psbt(with_derivation(
                        psbt,
                        SortKeyDerivation::Derived,
                    ))
                    .unwrap()
                };
                let explicit = |psbt| {
                    Sorter::<Unmodifiable, ExplicitOnly>::try_from_unordered_psbt(with_derivation(
                        psbt,
                        SortKeyDerivation::Explicit,
                    ))
                    .unwrap()
                };

                check(
                    conflicting,
                    mixed(psbt(true, true)).try_sort(),
                    mixed(psbt(true, true)).try_into_ordered_psbt().unwrap(),
                );
                check(
                    conflicting,
                    mixed(psbt(false, true))
                        .set_seed(sort_seed(vec![9; 16]))
                        .try_sort(),
                    mixed(psbt(true, true)).try_into_ordered_psbt().unwrap(),
                );
                check(
                    conflicting,
                    derived(psbt(true, false)).try_sort(),
                    derived(psbt(true, false)).try_into_ordered_psbt().unwrap(),
                );
                check(
                    conflicting,
                    derived(psbt(false, false))
                        .set_seed(sort_seed(vec![9; 16]))
                        .try_sort(),
                    derived(psbt(true, false)).try_into_ordered_psbt().unwrap(),
                );
                check(
                    conflicting,
                    explicit(psbt(false, true)).try_sort(),
                    explicit(psbt(false, true)).try_into_ordered_psbt().unwrap(),
                );
            }
        }

        #[test]
        fn the_sorter_keeps_the_psbt_it_wraps() {
            let mut psbt = empty_psbt();
            psbt.inputs = InputSet::try_from_inputs([make_input(1, 0), make_input(2, 0)]).unwrap();
            psbt.outputs = OutputSet::try_from_outputs([make_output_with_uid(&[1])]).unwrap();
            let sorter =
                Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt.clone()).unwrap();
            assert_eq!(sorter.inner(), &psbt);
            assert_eq!(sorter.into_inner(), psbt);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::super::key::{derive_sort_key, write_input_sort_identifier};
        use super::*;
        use bitcoin::hashes::HashEngine;
        use proptest::prelude::*;
        use psbt_v2::v2;

        fn seeds() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(any::<u8>(), 16..=32)
        }

        /// Which sorter, and so which sort keys, a test orders with.
        #[derive(Debug, Clone, Copy)]
        enum Mode {
            Mixed,
            DerivedOnly,
            ExplicitOnly,
        }

        fn modes() -> impl Strategy<Value = Mode> {
            prop_oneof![
                Just(Mode::Mixed),
                Just(Mode::DerivedOnly),
                Just(Mode::ExplicitOnly)
            ]
        }

        /// Up to five distinct sort keys, each with whether the item carries
        /// it. Some keys are prefixes of others. The first key is empty and
        /// the others end in their index, so no two are equal.
        fn keys() -> impl Strategy<Value = Vec<(Vec<u8>, bool)>> {
            proptest::collection::vec(
                (proptest::collection::vec(0u8..3, 0..=4), any::<bool>()),
                0..=5,
            )
            .prop_map(|keys| {
                keys.into_iter()
                    .enumerate()
                    .map(|(index, (mut key, carried))| {
                        if index == 0 {
                            key.clear();
                        } else {
                            key.push(index as u8);
                        }
                        (key, carried)
                    })
                    .collect()
            })
        }

        /// The sort key an item has in `mode`: all of them in the explicit
        /// mode, those that carry one in the mixed mode, none in the derived
        /// mode.
        fn explicit_key(mode: Mode, key: &[u8], carried: bool) -> Option<SortKey> {
            match mode {
                Mode::ExplicitOnly => true,
                Mode::Mixed => carried,
                Mode::DerivedOnly => false,
            }
            .then(|| sort_key(key.to_vec()))
        }

        fn inputs(mode: Mode, keys: &[(Vec<u8>, bool)]) -> Vec<Input> {
            keys.iter()
                .enumerate()
                .map(|(index, (key, carried))| {
                    let mut input = make_input(index as u8, 0);
                    if let Some(key) = explicit_key(mode, key, *carried) {
                        input.set_sort_key(key);
                    }
                    input
                })
                .collect()
        }

        fn outputs(mode: Mode, keys: &[(Vec<u8>, bool)]) -> Vec<Output> {
            keys.iter()
                .enumerate()
                .map(|(index, (key, carried))| {
                    let mut output = make_output_with_uid(&[index as u8]);
                    if let Some(key) = explicit_key(mode, key, *carried) {
                        output.set_sort_key(key);
                    }
                    output
                })
                .collect()
        }

        /// An unordered PSBT whose sort fields select `mode`.
        fn unordered(
            mode: Mode,
            seed: &[u8],
            inputs: Vec<Input>,
            outputs: Vec<Output>,
        ) -> UnorderedPsbt {
            let mut psbt = empty_psbt();
            psbt.global.set_unordered();
            match mode {
                Mode::Mixed => psbt.global.set_sort_seed(sort_seed(seed.to_vec())),
                Mode::DerivedOnly => {
                    psbt.global.set_sort_seed(sort_seed(seed.to_vec()));
                    psbt.global
                        .set_sort_key_derivation(SortKeyDerivation::Derived);
                }
                Mode::ExplicitOnly => psbt
                    .global
                    .set_sort_key_derivation(SortKeyDerivation::Explicit),
            }
            psbt.inputs = InputSet::try_from_inputs(inputs).unwrap();
            psbt.outputs = OutputSet::try_from_outputs(outputs).unwrap();
            psbt
        }

        fn order(mode: Mode, psbt: UnorderedPsbt) -> Psbt {
            match mode {
                Mode::Mixed => Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt)
                    .unwrap()
                    .try_into_ordered_psbt(),
                Mode::DerivedOnly => {
                    Sorter::<Unmodifiable, DerivedOnly>::try_from_unordered_psbt(psbt)
                        .unwrap()
                        .try_into_ordered_psbt()
                }
                Mode::ExplicitOnly => {
                    Sorter::<Unmodifiable, ExplicitOnly>::try_from_unordered_psbt(psbt)
                        .unwrap()
                        .try_into_ordered_psbt()
                }
            }
            .unwrap()
        }

        /// The updater that `try_sort` hands the ordered PSBT to.
        fn hand_off(mode: Mode, psbt: UnorderedPsbt) -> v2::Updater {
            match mode {
                Mode::Mixed => Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt)
                    .unwrap()
                    .try_sort(),
                Mode::DerivedOnly => {
                    Sorter::<Unmodifiable, DerivedOnly>::try_from_unordered_psbt(psbt)
                        .unwrap()
                        .try_sort()
                }
                Mode::ExplicitOnly => {
                    Sorter::<Unmodifiable, ExplicitOnly>::try_from_unordered_psbt(psbt)
                        .unwrap()
                        .try_sort()
                }
            }
            .unwrap()
        }

        /// `items` ordered by their keys.
        fn sorted_by_key<T>(mut items: Vec<(Vec<u8>, T)>) -> Vec<T> {
            items.sort_by(|(left, _), (right, _)| left.cmp(right));
            items.into_iter().map(|(_, item)| item).collect()
        }

        proptest! {
            /// Sorting keeps every input and output whole, orders them by
            /// their explicit keys, or by keys derived from the seed where
            /// they have none, and clears the unordered flag. Building the
            /// PSBT from the items in reverse gives the same ordered PSBT,
            /// and `try_sort` hands it to the updater.
            #[test]
            fn sorting_orders_inputs_and_outputs_by_their_sort_keys(
                mode in modes(),
                seed in seeds(),
                input_keys in keys(),
                output_keys in keys(),
            ) {
                let inputs = inputs(mode, &input_keys);
                let outputs = outputs(mode, &output_keys);
                let ordered = order(mode, unordered(mode, &seed, inputs.clone(), outputs.clone()));
                let handed_off = hand_off(mode, unordered(mode, &seed, inputs.clone(), outputs.clone()));
                prop_assert_eq!(handed_off.psbt(), ordered.clone());

                let expected_inputs = sorted_by_key(
                    inputs
                        .iter()
                        .map(|input| {
                            let key = input.sort_key().map(<[u8]>::to_vec).unwrap_or_else(|| {
                                let outpoint = bitcoin::OutPoint {
                                    txid: input.previous_txid,
                                    vout: input.spent_output_index,
                                };
                                derive_sort_key(&seed, |engine| {
                                    engine.input(&bitcoin::consensus::serialize(&outpoint))
                                })
                                .to_vec()
                            });
                            (key, input.clone())
                        })
                        .collect(),
                );
                let expected_outputs = sorted_by_key(
                    outputs
                        .iter()
                        .map(|output| {
                            let key = output.sort_key().map(<[u8]>::to_vec).unwrap_or_else(|| {
                                derive_sort_key(&seed, |engine| {
                                    engine.input(output.unique_id().as_bytes())
                                })
                                .to_vec()
                            });
                            (key, output.clone())
                        })
                        .collect(),
                );
                let ordered_inputs: Vec<_> = ordered
                    .inputs
                    .iter()
                    .map(|input| Input::try_from(input.clone()).unwrap())
                    .collect();
                let ordered_outputs: Vec<_> = ordered.outputs.iter().map(read_output).collect();
                prop_assert_eq!(ordered_inputs, expected_inputs);
                prop_assert_eq!(ordered_outputs, expected_outputs);

                let global = Global::try_from(ordered.global.clone()).unwrap();
                prop_assert!(!global.is_unordered());

                let reversed = order(
                    mode,
                    unordered(
                        mode,
                        &seed,
                        inputs.into_iter().rev().collect(),
                        outputs.into_iter().rev().collect(),
                    ),
                );
                prop_assert_eq!(reversed, ordered);
            }

            /// Only the lowest two flag bits decide admission.
            #[test]
            fn flags_are_admitted_by_their_lowest_two_bits(flags in any::<u8>()) {
                fn check<M: Modifiability>(flags: u8, expected: u8) -> Result<(), TestCaseError> {
                    let actual = flags & 0x03;
                    let mismatch = FlagsMismatch { expected, actual };
                    prop_assert_eq!(flags_error::<M>(flags), (actual != expected).then(|| mismatch.into()));
                    Ok(())
                }
                check::<Unmodifiable>(flags, 0x00)?;
                check::<InputsModifiable>(flags, 0x01)?;
                check::<OutputsModifiable>(flags, 0x02)?;
                check::<BothModifiable>(flags, 0x03)?;
            }

            /// Setting the seed, whether or not the PSBT had another one,
            /// orders like a PSBT that carries the seed.
            #[test]
            fn set_seed_matches_a_preset_seed(
                seed in seeds(),
                old_seed in proptest::option::of(seeds()),
            ) {
                let psbt = |seed: Option<&Vec<u8>>| {
                    let mut psbt = empty_psbt();
                    if let Some(seed) = seed {
                        psbt.global.set_sort_seed(sort_seed(seed.clone()));
                    }
                    psbt.inputs = InputSet::try_from_inputs((0..4).map(|txid| make_input(txid, 0))).unwrap();
                    psbt.outputs = OutputSet::try_from_outputs((0..4).map(|uid| make_output_with_uid(&[uid]))).unwrap();
                    psbt
                };
                let mixed = |seed| Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt(seed)).unwrap();
                let derived = |seed| {
                    Sorter::<Unmodifiable, DerivedOnly>::try_from_unordered_psbt(with_derivation(
                        psbt(seed),
                        SortKeyDerivation::Derived,
                    ))
                    .unwrap()
                };

                prop_assert_eq!(
                    mixed(old_seed.as_ref()).set_seed(sort_seed(seed.clone())).try_into_ordered_psbt().unwrap(),
                    mixed(Some(&seed)).try_into_ordered_psbt().unwrap()
                );
                prop_assert_eq!(
                    mixed(old_seed.as_ref()).set_seed(sort_seed(seed.clone())).try_sort().unwrap().psbt(),
                    mixed(Some(&seed)).try_into_ordered_psbt().unwrap()
                );
                prop_assert_eq!(
                    derived(old_seed.as_ref()).set_seed(sort_seed(seed.clone())).into_ordered_psbt(),
                    derived(Some(&seed)).try_into_ordered_psbt().unwrap()
                );
            }

            /// `try_sort` hands the ordered PSBT, with its flags, to the role
            /// that matches the modifiability.
            #[test]
            fn try_sort_hands_the_ordered_psbt_to_the_role_for_its_modifiability(
                seed in seeds(),
            ) {
                fn sorter<M: Modifiability>(flags: u8, seed: &[u8]) -> Sorter<M, Seeded<DerivedOnly>> {
                    let mut psbt = empty_psbt();
                    psbt.global.tx_modifiable_flags = flags;
                    psbt.global.set_sort_seed(sort_seed(seed.to_vec()));
                    psbt.inputs = InputSet::try_from_inputs([make_input(1, 0), make_input(2, 0)]).unwrap();
                    psbt.outputs = OutputSet::try_from_outputs([make_output_with_uid(&[1])]).unwrap();
                    Sorter::try_from_unordered_psbt(with_derivation(psbt, SortKeyDerivation::Derived)).unwrap()
                }

                let both: v2::Constructor<v2::Modifiable> = sorter::<BothModifiable>(0x03, &seed).try_sort().unwrap();
                let inputs: v2::Constructor<v2::InputsOnlyModifiable> = sorter::<InputsModifiable>(0x01, &seed).try_sort().unwrap();
                let outputs: v2::Constructor<v2::OutputsOnlyModifiable> = sorter::<OutputsModifiable>(0x02, &seed).try_sort().unwrap();
                let updater: v2::Updater = sorter::<Unmodifiable>(0x00, &seed).try_sort().unwrap();

                let ordered = sorter::<Unmodifiable>(0x00, &seed).into_ordered_psbt();
                let handed_off = [
                    (0x03, both.psbt().unwrap()),
                    (0x01, inputs.psbt().unwrap()),
                    (0x02, outputs.psbt().unwrap()),
                    (0x00, updater.psbt()),
                ];
                for (flags, psbt) in handed_off {
                    prop_assert_eq!(psbt.global.tx_modifiable_flags, flags);
                    prop_assert_eq!(psbt.inputs, ordered.inputs.clone());
                    prop_assert_eq!(psbt.outputs, ordered.outputs.clone());
                }
            }

            #[test]
            fn explicit_reports_missing_and_duplicate_keys(
                duplicate_key in proptest::collection::vec(any::<u8>(), 0..=8),
            ) {
                let mut missing_input = empty_psbt();
                missing_input.inputs = InputSet::try_from_inputs([make_input(1, 0)]).unwrap();
                prop_assert!(matches!(
                    Sorter::<Unmodifiable, ExplicitOnly>(missing_input, PhantomData).try_into_ordered_psbt(),
                    Err(SortError::MissingInputSortKey(_))
                ));

                let mut missing_output = empty_psbt();
                missing_output.outputs = OutputSet::try_from_outputs([make_output_with_uid(&[1])]).unwrap();
                prop_assert!(matches!(
                    Sorter::<Unmodifiable, ExplicitOnly>(missing_output, PhantomData).try_into_ordered_psbt(),
                    Err(SortError::MissingOutputSortKey)
                ));

                let mut duplicates = empty_psbt();
                duplicates.outputs = OutputSet::try_from_outputs([1, 2].map(|uid| {
                    let mut output = make_output_with_uid(&[uid]);
                    output.set_sort_key(sort_key(duplicate_key.clone()));
                    output
                }))
                .unwrap();
                prop_assert!(matches!(
                    Sorter::<Unmodifiable, ExplicitOnly>(duplicates, PhantomData).try_into_ordered_psbt(),
                    Err(SortError::DuplicateSortKey(_))
                ));
            }

            #[test]
            fn mixed_rejects_tied_sort_keys(
                seed in seeds(),
                key in proptest::collection::vec(any::<u8>(), 0..=8),
            ) {
                let mut explicit_tie = empty_psbt();
                explicit_tie.outputs = OutputSet::try_from_outputs([1, 2].map(|uid| {
                    let mut output = make_output_with_uid(&[uid]);
                    output.set_sort_key(sort_key(key.clone()));
                    output
                }))
                .unwrap();
                prop_assert!(matches!(
                    Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(explicit_tie).unwrap().try_into_ordered_psbt(),
                    Err(SortError::DuplicateSortKey(tied)) if tied == key
                ));

                let derived = make_input(2, 0);
                let derived_key = derive_sort_key(&seed, |engine| write_input_sort_identifier(&derived, engine)).to_vec();
                let mut explicit = make_input(1, 0);
                explicit.set_sort_key(sort_key(derived_key.clone()));
                let mut mixed_tie = empty_psbt();
                mixed_tie.global.set_sort_seed(sort_seed(seed));
                mixed_tie.inputs = InputSet::try_from_inputs([explicit, derived]).unwrap();
                prop_assert!(matches!(
                    Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(mixed_tie).unwrap().try_into_ordered_psbt(),
                    Err(SortError::DuplicateSortKey(tied)) if tied == derived_key
                ));
            }
        }
    }
}
