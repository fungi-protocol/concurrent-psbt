//! The Sorter role: orders the inputs and outputs of an unordered PSBT.

use std::marker::PhantomData;

use psbt_v2::v2::Psbt;

use crate::global::{Global, SortKeyDerivation, SortSeed, TxUuid};
use crate::lattice::partial::PartialJoin;
use crate::tx::UnorderedPsbt;

mod error;
mod key;
mod modifiability;
mod order;

pub use error::{FlagsMismatch, SeedConflict, SortError, SortModeError, SorterError, UuidError};
pub use key::{
    AuxSortSeedHash, AuxSortSeedTag, SortKeyHash, SortKeyTag, SortSeedHash, SortSeedTag, UuidHash,
    UuidTag,
};
pub(crate) use modifiability::check_flags;
pub use modifiability::{
    BothModifiable, InputsModifiable, Modifiability, Modifiable, OutputsModifiable, Unmodifiable,
};
use order::ordered_psbt;

/// Join `seed` into `PSBT_GLOBAL_SORT_SEED`: set it if unset, and keep it if
/// it is already `seed`.
///
/// # Errors
/// Returns [`SeedConflict`] if the PSBT has a different seed.
pub(crate) fn join_seed(global: &mut Global, seed: SortSeed) -> Result<(), SeedConflict> {
    let seed = match global.sort_seed.clone() {
        Some(existing) => existing
            .try_join(seed)
            .map_err(SeedConflict::from_conflict)?,
        None => seed,
    };
    global.set_sort_seed(seed);
    Ok(())
}

/// Whether the PSBT has the UUID required by a [`Seeded`] mode.
pub(crate) fn has_uuid(global: &Global) -> bool {
    global.uuid().is_some()
}

/// Join `uuid` into `PSBT_GLOBAL_TX_UUID`: set it if unset, and keep it if it
/// is already `uuid`.
///
/// # Errors
/// Returns [`UuidError::Conflict`] if the PSBT already has a different UUID.
pub(crate) fn join_uuid(global: &mut Global, uuid: TxUuid) -> Result<(), UuidError> {
    let uuid = match global.uuid.clone() {
        Some(existing) => existing.try_join(uuid).map_err(UuidError::from_conflict)?,
        None => uuid,
    };
    global.set_uuid(uuid);
    Ok(())
}

/// Hand an ordered PSBT to the role that follows sorting under `M`.
fn hand_off<M: Modifiability>(psbt: Psbt) -> Result<M::Sorted, M::Error> {
    <M as modifiability::sealed::Modifiability>::sorted(psbt)
}

/// Sort mode: every input and output is ordered by its derived sort key
/// alone, and explicit sort keys are not allowed.
///
/// Participants derive the same order from the transaction UUID in the PSBT
/// before signing. Observers on Bitcoin's peer-to-peer network receive the
/// ordered transaction without the PSBT's sort fields.
#[derive(Debug, Clone, PartialEq)]
pub enum ExplicitKeysForbidden {}

/// Sort mode: every input and output needs a non-empty explicit sort key.
/// Inputs, or outputs, with the same key are ordered by their derived keys.
#[derive(Debug, Clone, PartialEq)]
pub enum ExplicitKeysRequired {}

/// Sort mode: inputs and outputs are ordered by their explicit sort keys, a
/// missing key counting as empty, and then by their derived keys.
#[derive(Debug, Clone, PartialEq)]
pub enum Mixed {}

/// Sort mode refinement: [`Mixed`], [`ExplicitKeysForbidden`] or
/// [`ExplicitKeysRequired`] with `PSBT_GLOBAL_TX_UUID` known to be set.
///
/// The [`Creator`](crate::roles::creator::Creator) always sets a UUID and
/// produces a seeded mode. A parsed PSBT may lack one: it enters an unseeded
/// mode and is refined by `try_set_uuid`.
#[derive(Debug, Clone, PartialEq)]
pub struct Seeded<S>(PhantomData<S>, std::convert::Infallible);

/// Typestate wrapper that applies a sort order to an [`UnorderedPsbt`].
///
/// `M` is the PSBT's [`Modifiability`], which decides the role a sorted PSBT
/// is handed to: the matching BIP 370 constructor while inputs or outputs
/// may still be added, the [`Updater`](psbt_v2::v2::Updater) once neither
/// may.
///
/// `S` is the sort mode: [`Mixed`], [`ExplicitKeysForbidden`], or
/// [`ExplicitKeysRequired`], or one of them [`Seeded`] once
/// `PSBT_GLOBAL_TX_UUID` is known to be set.
///
/// # Ordering
///
/// Each mode constrains which explicit keys participants may supply. Once
/// those constraints are met, the sorter compares explicit keys first,
/// treating an absent key as empty. Equal explicit keys are ordered by
/// their UUID-derived keys. Derived keys are never stored in the PSBT.
/// The derivation uses the following BIP 340-style tagged hashes:
///
/// - `h` = [`UuidHash`] of the required `PSBT_GLOBAL_TX_UUID`,
/// - `e` = [`AuxSortSeedHash`] of `PSBT_GLOBAL_SORT_SEED`, empty if unset,
/// - the sort seed = [`SortSeedHash`] of `h || e`,
/// - a derived sort key = [`SortKeyHash`] of the sort seed, then the input's
///   consensus-encoded outpoint or the output's unique ID.
///
/// Derived keys of distinct inputs, or outputs, differ, so explicit keys may
/// repeat.
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
    /// `PSBT_GLOBAL_TX_UUID` to be set.
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

    /// Join `seed` into `PSBT_GLOBAL_SORT_SEED`: set it if unset, and keep it
    /// if it is already `seed`.
    ///
    /// # Errors
    /// Returns [`SeedConflict`] if the PSBT has a different seed, which
    /// replacing would not be monotone.
    pub fn try_set_seed(mut self, seed: SortSeed) -> Result<Self, SeedConflict> {
        join_seed(&mut self.0.global, seed)?;
        Ok(self)
    }
}

impl<M: Modifiability, S: SortMode> Sorter<M, S> {
    /// Create a sorter from an [`UnorderedPsbt`] of any modifiability.
    ///
    /// # Errors
    /// Returns [`SorterError::Flags`] if `tx_modifiable_flags` don't match
    /// `M`, or [`SorterError::SortMode`] if the sort fields don't select `S`.
    pub fn try_from_unordered_psbt(psbt: UnorderedPsbt) -> Result<Self, SorterError> {
        check_flags::<M>(*psbt.global.tx_modifiable_flags())?;
        check_sort_mode::<S>(&psbt)?;
        Ok(Sorter::from_unordered(psbt))
    }
}

impl<M, S> Sorter<M, S>
where
    Seeded<S>: SortMode,
{
    /// Join `uuid` into `PSBT_GLOBAL_TX_UUID`, setting it if unset, and
    /// refine the mode to [`Seeded`].
    ///
    /// # Errors
    /// Returns [`UuidError::Conflict`] if the PSBT already has a different UUID.
    pub fn try_set_uuid(mut self, uuid: TxUuid) -> Result<Sorter<M, Seeded<S>>, UuidError> {
        join_uuid(&mut self.0.global, uuid)?;
        Ok(Sorter::from_unordered(self.0))
    }
}

impl<M: Modifiability, S: SortMode> Sorter<M, S>
where
    Seeded<S>: SortMode,
{
    /// Order the inputs and outputs as described [above](Sorter#ordering),
    /// and produce an ordered BIP 370 [`Psbt`].
    ///
    /// # Errors
    /// Returns [`SortError::MissingUuid`] if `PSBT_GLOBAL_TX_UUID` is unset.
    /// In [`ExplicitKeysRequired`] mode, then returns
    /// [`SortError::MissingInputSortKey`] or [`SortError::MissingOutputSortKey`]
    /// if an input or output has no sort key.
    pub fn try_into_ordered_psbt(self) -> Result<Psbt, SortError> {
        ordered_psbt(
            self.0,
            S::SORT_KEY_DERIVATION == Some(SortKeyDerivation::ExplicitKeysRequired),
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

/// The infallible sorting of a [`Seeded`] mode that needs no explicit keys.
macro_rules! infallible_sort {
    ($($mode:ty),*) => {$(
        impl<M: Modifiability> Sorter<M, Seeded<$mode>> {
            /// Order the inputs and outputs as described
            /// [above](Sorter#ordering), and produce an ordered BIP 370
            /// [`Psbt`].
            ///
            /// The UUID is set and a missing explicit key counts as empty,
            /// so this cannot fail.
            pub fn into_ordered_psbt(self) -> Psbt {
                ordered_psbt(self.0, false)
                    .expect("a seeded sorter has a UUID, and this mode needs no explicit key")
            }

            /// Order the PSBT as [`into_ordered_psbt`](Self::into_ordered_psbt)
            /// does and hand it to [`M::Sorted`](Modifiability::Sorted).
            ///
            /// # Errors
            /// Returns [`M::Error`](Modifiability::Error): `Infallible` while
            /// `M` is modifiable, or
            /// [`DetermineLockTimeError`](psbt_v2::v2::DetermineLockTimeError)
            /// if `M` is [`Unmodifiable`] and the inputs' lock time
            /// requirements conflict.
            pub fn try_sort(self) -> Result<M::Sorted, M::Error> {
                hand_off::<M>(self.into_ordered_psbt())
            }
        }
    )*};
}

infallible_sort!(Mixed, ExplicitKeysForbidden);

impl<M: Modifiability> Sorter<M, Seeded<ExplicitKeysRequired>> {
    /// Order the inputs and outputs as described [above](Sorter#ordering),
    /// and produce an ordered BIP 370 [`Psbt`].
    ///
    /// # Errors
    /// Returns [`SortError::MissingInputSortKey`] or
    /// [`SortError::MissingOutputSortKey`] if an input or output has no sort
    /// key.
    pub fn try_into_ordered_psbt(self) -> Result<Psbt, SortError> {
        ordered_psbt(self.0, true)
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
        /// Whether this mode requires `PSBT_GLOBAL_TX_UUID`.
        const SEEDED: bool;
    }
}

/// A sort mode that a PSBT's sort fields select: [`Mixed`] if
/// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is unset, [`ExplicitKeysForbidden`] if it
/// is `0x01`, and [`ExplicitKeysRequired`] if it is `0x00`, each [`Seeded`] if
/// `PSBT_GLOBAL_TX_UUID` is set.
pub trait SortMode: sealed::SortMode {}

macro_rules! sort_mode {
    ($($mode:ty => $sort_key_derivation:expr;)*) => {$(
        impl SortMode for $mode {}

        impl sealed::SortMode for $mode {
            const SORT_KEY_DERIVATION: Option<SortKeyDerivation> = $sort_key_derivation;
            const SEEDED: bool = false;
        }

        impl SortMode for Seeded<$mode> {}

        impl sealed::SortMode for Seeded<$mode> {
            const SORT_KEY_DERIVATION: Option<SortKeyDerivation> = $sort_key_derivation;
            const SEEDED: bool = true;
        }
    )*};
}

sort_mode! {
    Mixed => None;
    ExplicitKeysForbidden => Some(SortKeyDerivation::ExplicitKeysForbidden);
    ExplicitKeysRequired => Some(SortKeyDerivation::ExplicitKeysRequired);
}

/// Check that `psbt`'s sort fields select sort mode `S`: they match
/// `PSBT_GLOBAL_SORT_KEY_DERIVATION`, a [`Seeded`] mode has a
/// `PSBT_GLOBAL_TX_UUID`, and an [`ExplicitKeysForbidden`] PSBT has no sort
/// keys, since they would not be used.
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
    let sort_keyed = || {
        psbt.inputs.values().any(|input| input.sort_key().is_some())
            || psbt
                .outputs
                .values()
                .any(|output| output.sort_key().is_some())
    };
    match S::SORT_KEY_DERIVATION {
        _ if S::SEEDED && !has_uuid(&psbt.global) => Err(SortModeError::MissingUuid),
        Some(SortKeyDerivation::ExplicitKeysForbidden) if sort_keyed() => {
            Err(SortModeError::UnexpectedSortKey)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::global::{Global, TxUuid};
    use crate::input::{Input, InputSet, SortKey};
    use crate::output::{Output, OutputSet, UniqueId};

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

    fn make_output_with_uid(tag: u8) -> Output {
        Output::new(
            UniqueId::try_from(vec![tag; 16]).unwrap(),
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
        psbt.global.set_tx_modifiable_flags(flags);
        Sorter::<M, Mixed>::try_from_unordered_psbt(psbt).err()
    }

    /// A PSBT with no inputs or outputs and `PSBT_GLOBAL_TX_UUID` `[13; 16]`.
    fn empty_psbt() -> UnorderedPsbt {
        let mut global = Global::default();
        global.set_uuid(TxUuid::try_from(vec![13; 16]).unwrap());
        UnorderedPsbt {
            global,
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
            KeyForbidden,
            NoUuid,
            /// The sort key derivation does not match the one in here.
            Wants(Option<SortKeyDerivation>),
        }

        /// Each row is a PSBT's sort key derivation, its UUID (unset, empty or
        /// set) and which entry has a sort key, then what `Mixed`,
        /// `ExplicitKeysForbidden`, `ExplicitKeysRequired` and their [`Seeded`]
        /// refinements do with it. Only a seeded mode needs the UUID, and
        /// checks it after the derivation and before the sort keys.
        #[test]
        fn the_sort_fields_select_the_sort_mode() {
            use Outcome::{Admits, KeyForbidden, NoUuid, Wants};
            use SortKeyOn as K;
            const U: Option<SortKeyDerivation> = None;
            const D: Option<SortKeyDerivation> = Some(SortKeyDerivation::ExplicitKeysForbidden);
            const E: Option<SortKeyDerivation> = Some(SortKeyDerivation::ExplicitKeysRequired);
            const SET: Option<&[u8]> = Some(&[13; 16]);

            #[rustfmt::skip]
            let rows = [
                (U, SET,   K::Neither, [Admits,   Wants(D),     Wants(E), Admits,   Wants(D),     Wants(E)]),
                (U, None,  K::Input,   [Admits,   Wants(D),     Wants(E), NoUuid,   Wants(D),     Wants(E)]),
                (U, SET,   K::Output,  [Admits,   Wants(D),     Wants(E), Admits,   Wants(D),     Wants(E)]),
                (D, SET,   K::Neither, [Wants(U), Admits,       Wants(E), Wants(U), Admits,       Wants(E)]),
                (D, None,  K::Neither, [Wants(U), Admits,       Wants(E), Wants(U), NoUuid,       Wants(E)]),
                (D, SET,   K::Input,   [Wants(U), KeyForbidden, Wants(E), Wants(U), KeyForbidden, Wants(E)]),
                (D, None,  K::Output,  [Wants(U), KeyForbidden, Wants(E), Wants(U), NoUuid,       Wants(E)]),
                (E, SET,   K::Neither, [Wants(U), Wants(D),     Admits,   Wants(U), Wants(D),     Admits]),
                (E, None,  K::Output,  [Wants(U), Wants(D),     Admits,   Wants(U), Wants(D),     NoUuid]),
                (E, SET,   K::Input,   [Wants(U), Wants(D),     Admits,   Wants(U), Wants(D),     Admits]),
            ];
            for (derivation, uuid, sort_key_on, outcomes) in rows {
                let mut psbt = empty_psbt();
                if let Some(derivation) = derivation {
                    psbt.global.set_sort_key_derivation(derivation);
                }
                psbt.global.uuid = uuid.map(|uuid| TxUuid::try_from(uuid.to_vec()).unwrap());
                let mut input = make_input(1, 0);
                let mut output = make_output_with_uid(1);
                match sort_key_on {
                    K::Neither => {}
                    K::Input => input.set_sort_key(sort_key(vec![1])),
                    K::Output => output.set_sort_key(sort_key(vec![1])),
                }
                psbt.inputs = InputSet::try_from_inputs([input]).unwrap();
                psbt.outputs = OutputSet::try_from_outputs([output]).unwrap();

                fn error<S: SortMode>(psbt: &UnorderedPsbt) -> Option<SorterError> {
                    Sorter::<Unmodifiable, S>::try_from_unordered_psbt(psbt.clone()).err()
                }
                let errors = [
                    error::<Mixed>(&psbt),
                    error::<ExplicitKeysForbidden>(&psbt),
                    error::<ExplicitKeysRequired>(&psbt),
                    error::<Seeded<Mixed>>(&psbt),
                    error::<Seeded<ExplicitKeysForbidden>>(&psbt),
                    error::<Seeded<ExplicitKeysRequired>>(&psbt),
                ];
                let expected = outcomes.map(|outcome| {
                    let error = match outcome {
                        Admits => return None,
                        KeyForbidden => SortModeError::UnexpectedSortKey,
                        NoUuid => SortModeError::MissingUuid,
                        Wants(expected) => SortModeError::SortKeyDerivation {
                            expected,
                            actual: derivation,
                        },
                    };
                    Some(SorterError::from(error))
                });
                assert_eq!(
                    errors, expected,
                    "{derivation:?}, UUID {uuid:?}, {sort_key_on:?}"
                );
            }
        }

        #[test]
        fn invalid_flags_are_reported_before_an_invalid_sort_mode() {
            let mut psbt = with_derivation(empty_psbt(), SortKeyDerivation::ExplicitKeysRequired);
            psbt.global.set_tx_modifiable_flags(0x01);
            assert_eq!(
                Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt).err(),
                Some(SorterError::Flags(FlagsMismatch {
                    expected: 0x00,
                    actual: 0x01
                }))
            );
        }

        /// The orders are those of the derived keys for the UUID `[13; 16]`
        /// and the seed `[42; 32]`, computed outside this crate. With
        /// `T(tag, x) = SHA-256(SHA-256(tag) || SHA-256(tag) || x)` and tags
        /// under `concurrent-psbt/`, a key is `T(deterministic-ordering,
        /// T(sort-seed, T(uuid, uuid) || T(aux-sort-seed, seed)) ||
        /// identifier)`.
        #[test]
        fn derived_order_follows_the_tagged_hash_of_uuid_seed_and_identifier() {
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(SortSeed::from(vec![42; 32]));
            psbt.inputs =
                InputSet::try_from_inputs((1..=5).map(|txid| make_input(txid, 0))).unwrap();
            psbt.outputs = OutputSet::try_from_outputs((0..=4).map(make_output_with_uid)).unwrap();

            let ordered = Sorter::<Unmodifiable, ExplicitKeysForbidden>::try_from_unordered_psbt(
                with_derivation(psbt, SortKeyDerivation::ExplicitKeysForbidden),
            )
            .unwrap()
            .try_into_ordered_psbt()
            .unwrap();

            let txids: Vec<_> = ordered
                .inputs
                .iter()
                .map(|input| input.previous_txid)
                .collect();
            let expected_txids = [2, 5, 1, 4, 3].map(|txid| *make_input(txid, 0).previous_txid());
            assert_eq!(txids, expected_txids);
            let uids: Vec<_> = ordered
                .outputs
                .iter()
                .map(|output| read_output(output).unique_id().as_bytes().to_vec())
                .collect();
            assert_eq!(uids, [3, 2, 4, 1, 0].map(|uid| vec![uid; 16]));
        }

        /// With the hashes of
        /// [`derived_order_follows_the_tagged_hash_of_uuid_seed_and_identifier`],
        /// the derived order of inputs 1 to 5 is 2, 5, 1, 4, 3 and of outputs
        /// 1 to 3 is 3, 2, 1. A missing explicit key sorts as an empty one, and
        /// derived keys break ties, here against the order of the txids and
        /// unique IDs.
        #[test]
        fn mixed_orders_by_explicit_key_then_derived_key() {
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(SortSeed::from(vec![42; 32]));
            let [mut one, mut two, unkeyed, mut empty, mut zero] =
                [1, 2, 3, 4, 5].map(|txid| make_input(txid, 0));
            one.set_sort_key(sort_key(vec![0x01]));
            two.set_sort_key(sort_key(vec![0x01]));
            empty.set_sort_key(sort_key(vec![]));
            zero.set_sort_key(sort_key(vec![0x00]));
            psbt.inputs = InputSet::try_from_inputs([one, two, unkeyed, empty, zero]).unwrap();
            let [mut first, mut second, unkeyed] = [1, 2, 3].map(make_output_with_uid);
            first.set_sort_key(sort_key(vec![0xfe]));
            second.set_sort_key(sort_key(vec![0xfe]));
            psbt.outputs = OutputSet::try_from_outputs([first, second, unkeyed]).unwrap();

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
                [4, 3, 5, 2, 1].map(|txid| *make_input(txid, 0).previous_txid())
            );
            let uids: Vec<_> = ordered
                .outputs
                .iter()
                .map(|output| read_output(output).unique_id().as_bytes().to_vec())
                .collect();
            assert_eq!(uids, [vec![3; 16], vec![2; 16], vec![1; 16]]);
        }

        /// Every mode needs a UUID and reports its absence before a missing
        /// explicit key.
        #[test]
        fn sorting_requires_a_uuid() {
            for inputs in [vec![], vec![make_input(1, 0)]] {
                let mut psbt = empty_psbt();
                psbt.global.uuid = None;
                psbt.inputs = InputSet::try_from_inputs(inputs).unwrap();
                let errors = [
                    Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt.clone())
                        .unwrap()
                        .try_into_ordered_psbt()
                        .err(),
                    Sorter::<Unmodifiable, ExplicitKeysForbidden>::try_from_unordered_psbt(
                        with_derivation(psbt.clone(), SortKeyDerivation::ExplicitKeysForbidden),
                    )
                    .unwrap()
                    .try_sort()
                    .err(),
                    Sorter::<Unmodifiable, ExplicitKeysRequired>::try_from_unordered_psbt(
                        with_derivation(psbt, SortKeyDerivation::ExplicitKeysRequired),
                    )
                    .unwrap()
                    .try_into_ordered_psbt()
                    .err(),
                ];
                for error in errors {
                    assert!(matches!(error, Some(SortError::MissingUuid)), "{error:?}");
                }
            }
        }

        /// Missing explicit keys are reported whether or not the mode is
        /// seeded.
        #[test]
        fn explicit_reports_missing_keys() {
            let mut missing_input = empty_psbt();
            missing_input.inputs = InputSet::try_from_inputs([make_input(1, 0)]).unwrap();
            assert!(matches!(
                Sorter::<Unmodifiable, ExplicitKeysRequired>(missing_input, PhantomData)
                    .try_into_ordered_psbt(),
                Err(SortError::MissingInputSortKey(_))
            ));

            let mut missing_output = empty_psbt();
            missing_output.outputs =
                OutputSet::try_from_outputs([make_output_with_uid(1)]).unwrap();
            assert!(matches!(
                Sorter::<Unmodifiable, Seeded<ExplicitKeysRequired>>(missing_output, PhantomData)
                    .try_into_ordered_psbt(),
                Err(SortError::MissingOutputSortKey)
            ));
        }

        /// Every `try_sort` of a sorter that is `Unmodifiable`, seeded or not,
        /// fails exactly when the inputs' lock time requirements conflict,
        /// and otherwise hands the ordered PSBT to the updater.
        #[test]
        fn try_sort_fails_exactly_when_lock_times_conflict() {
            fn check(conflicting: bool, result: Result<Updater, SortError>, ordered: Psbt) {
                assert_eq!(result.is_err(), conflicting);
                match result {
                    Ok(updater) => assert_eq!(updater.psbt(), ordered),
                    Err(error) => assert!(matches!(error, SortError::UndeterminedLockTime(_))),
                }
            }

            for conflicting in [false, true] {
                let psbt = |keyed: bool| {
                    let mut psbt = empty_psbt();
                    let mut inputs = crate::test_support::conflicting_lock_time_inputs();
                    if !conflicting {
                        inputs[0].set_min_time(None);
                    }
                    if keyed {
                        for (key, input) in inputs.iter_mut().enumerate() {
                            input.set_sort_key(sort_key(vec![key as u8 + 1]));
                        }
                    }
                    psbt.inputs = InputSet::try_from_inputs(inputs).unwrap();
                    psbt
                };
                fn sorter<S: SortMode>(psbt: UnorderedPsbt) -> Sorter<Unmodifiable, S> {
                    let derivation = S::SORT_KEY_DERIVATION;
                    let psbt = match derivation {
                        Some(derivation) => with_derivation(psbt, derivation),
                        None => psbt,
                    };
                    Sorter::try_from_unordered_psbt(psbt).unwrap()
                }
                let mixed = sorter::<Mixed>;
                let derived = sorter::<ExplicitKeysForbidden>;
                let explicit = sorter::<ExplicitKeysRequired>;
                let seeded_mixed = sorter::<Seeded<Mixed>>;
                let seeded_derived = sorter::<Seeded<ExplicitKeysForbidden>>;
                let seeded_explicit = sorter::<Seeded<ExplicitKeysRequired>>;

                check(
                    conflicting,
                    mixed(psbt(true)).try_sort(),
                    mixed(psbt(true)).try_into_ordered_psbt().unwrap(),
                );
                check(
                    conflicting,
                    derived(psbt(false)).try_sort(),
                    derived(psbt(false)).try_into_ordered_psbt().unwrap(),
                );
                check(
                    conflicting,
                    explicit(psbt(true)).try_sort(),
                    explicit(psbt(true)).try_into_ordered_psbt().unwrap(),
                );
                check(
                    conflicting,
                    seeded_mixed(psbt(true)).try_sort().map_err(Into::into),
                    seeded_mixed(psbt(true)).into_ordered_psbt(),
                );
                check(
                    conflicting,
                    seeded_derived(psbt(false)).try_sort().map_err(Into::into),
                    seeded_derived(psbt(false)).into_ordered_psbt(),
                );
                check(
                    conflicting,
                    seeded_explicit(psbt(true)).try_sort(),
                    seeded_explicit(psbt(true)).try_into_ordered_psbt().unwrap(),
                );
            }
        }

        #[test]
        fn the_sorter_keeps_the_psbt_it_wraps() {
            let mut psbt = empty_psbt();
            psbt.inputs = InputSet::try_from_inputs([make_input(1, 0), make_input(2, 0)]).unwrap();
            psbt.outputs = OutputSet::try_from_outputs([make_output_with_uid(1)]).unwrap();
            let sorter =
                Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt.clone()).unwrap();
            assert_eq!(sorter.inner(), &psbt);
            assert_eq!(sorter.into_inner(), psbt);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use bitcoin::hashes::{Hash, HashEngine, sha256};
        use proptest::prelude::*;
        use psbt_v2::v2;

        fn seeds() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(any::<u8>(), 0..=32)
        }

        fn uuids() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(any::<u8>(), 16..=32)
        }

        /// Which sorter, and so which sort keys, a test orders with.
        #[derive(Debug, Clone, Copy)]
        enum Mode {
            Mixed,
            ExplicitKeysForbidden,
            ExplicitKeysRequired,
        }

        fn modes() -> impl Strategy<Value = Mode> {
            prop_oneof![
                Just(Mode::Mixed),
                Just(Mode::ExplicitKeysForbidden),
                Just(Mode::ExplicitKeysRequired)
            ]
        }

        /// Up to five short sort keys, each with whether the item carries it.
        /// Keys often repeat, and some are prefixes of others.
        fn keys() -> impl Strategy<Value = Vec<(Vec<u8>, bool)>> {
            proptest::collection::vec(
                (proptest::collection::vec(0u8..2, 0..=2), any::<bool>()),
                0..=5,
            )
        }

        /// The sort key an item has in `mode`: a non-empty one in the explicit
        /// mode, as derivation `0x00` requires, the key if the item carries it
        /// in the mixed mode, and none in the derived mode.
        fn explicit_key(mode: Mode, key: &[u8], carried: bool) -> Option<SortKey> {
            match mode {
                Mode::ExplicitKeysRequired => Some([&[1u8][..], key].concat()),
                Mode::Mixed => carried.then(|| key.to_vec()),
                Mode::ExplicitKeysForbidden => None,
            }
            .map(sort_key)
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
                    let mut output = make_output_with_uid(index as u8);
                    if let Some(key) = explicit_key(mode, key, *carried) {
                        output.set_sort_key(key);
                    }
                    output
                })
                .collect()
        }

        /// An unordered PSBT with `uuid` and `seed` whose sort fields select
        /// `mode`.
        fn unordered(
            mode: Mode,
            uuid: &[u8],
            seed: &[u8],
            inputs: Vec<Input>,
            outputs: Vec<Output>,
        ) -> UnorderedPsbt {
            let mut psbt = empty_psbt();
            psbt.global.set_unordered();
            psbt.global
                .set_uuid(TxUuid::try_from(uuid.to_vec()).unwrap());
            psbt.global.set_sort_seed(SortSeed::from(seed.to_vec()));
            match mode {
                Mode::Mixed => {}
                Mode::ExplicitKeysForbidden => psbt
                    .global
                    .set_sort_key_derivation(SortKeyDerivation::ExplicitKeysForbidden),
                Mode::ExplicitKeysRequired => psbt
                    .global
                    .set_sort_key_derivation(SortKeyDerivation::ExplicitKeysRequired),
            }
            psbt.inputs = InputSet::try_from_inputs(inputs).unwrap();
            psbt.outputs = OutputSet::try_from_outputs(outputs).unwrap();
            psbt
        }

        fn sorter<S: SortMode>(psbt: UnorderedPsbt) -> Sorter<Unmodifiable, S> {
            Sorter::try_from_unordered_psbt(psbt).unwrap()
        }

        /// The ordered PSBT of the sorter for `mode`, [`Seeded`] if `seeded`.
        fn order(mode: Mode, seeded: bool, psbt: UnorderedPsbt) -> Psbt {
            match (mode, seeded) {
                (Mode::Mixed, false) => sorter::<Mixed>(psbt).try_into_ordered_psbt().unwrap(),
                (Mode::Mixed, true) => sorter::<Seeded<Mixed>>(psbt).into_ordered_psbt(),
                (Mode::ExplicitKeysForbidden, false) => sorter::<ExplicitKeysForbidden>(psbt)
                    .try_into_ordered_psbt()
                    .unwrap(),
                (Mode::ExplicitKeysForbidden, true) => {
                    sorter::<Seeded<ExplicitKeysForbidden>>(psbt).into_ordered_psbt()
                }
                (Mode::ExplicitKeysRequired, false) => sorter::<ExplicitKeysRequired>(psbt)
                    .try_into_ordered_psbt()
                    .unwrap(),
                (Mode::ExplicitKeysRequired, true) => sorter::<Seeded<ExplicitKeysRequired>>(psbt)
                    .try_into_ordered_psbt()
                    .unwrap(),
            }
        }

        /// The updater that `try_sort` of the sorter for `mode`, [`Seeded`]
        /// if `seeded`, hands the ordered PSBT to.
        fn hand_off(mode: Mode, seeded: bool, psbt: UnorderedPsbt) -> v2::Updater {
            match (mode, seeded) {
                (Mode::Mixed, false) => sorter::<Mixed>(psbt).try_sort().unwrap(),
                (Mode::Mixed, true) => sorter::<Seeded<Mixed>>(psbt).try_sort().unwrap(),
                (Mode::ExplicitKeysForbidden, false) => {
                    sorter::<ExplicitKeysForbidden>(psbt).try_sort().unwrap()
                }
                (Mode::ExplicitKeysForbidden, true) => {
                    sorter::<Seeded<ExplicitKeysForbidden>>(psbt)
                        .try_sort()
                        .unwrap()
                }
                (Mode::ExplicitKeysRequired, false) => {
                    sorter::<ExplicitKeysRequired>(psbt).try_sort().unwrap()
                }
                (Mode::ExplicitKeysRequired, true) => sorter::<Seeded<ExplicitKeysRequired>>(psbt)
                    .try_sort()
                    .unwrap(),
            }
        }

        /// `SHA-256(SHA-256(tag) || SHA-256(tag) || data)` for the tag
        /// `concurrent-psbt/<tag>`.
        fn tagged(tag: &str, data: &[u8]) -> [u8; 32] {
            let tag = sha256::Hash::hash(format!("concurrent-psbt/{tag}").as_bytes());
            let mut engine = sha256::Hash::engine();
            engine.input(tag.as_byte_array());
            engine.input(tag.as_byte_array());
            engine.input(data);
            sha256::Hash::from_engine(engine).to_byte_array()
        }

        /// The derived sort key of the identifier `id` under `uuid` and the
        /// auxiliary `seed`.
        fn derived_key(uuid: &[u8], seed: &[u8], id: &[u8]) -> [u8; 32] {
            let entropy = [tagged("uuid", uuid), tagged("aux-sort-seed", seed)].concat();
            let sort_seed = tagged("sort-seed", &entropy);
            tagged("deterministic-ordering", &[&sort_seed[..], id].concat())
        }

        /// `items` ordered by their keys.
        fn sorted_by_key<K: Ord, T>(mut items: Vec<(K, T)>) -> Vec<T> {
            items.sort_by(|(left, _), (right, _)| left.cmp(right));
            items.into_iter().map(|(_, item)| item).collect()
        }

        proptest! {
            /// Sorting keeps every input and output whole, orders them by
            /// their explicit keys, empty where they have none, then by keys
            /// derived from the UUID, the seed and their identifiers, and
            /// clears the unordered flag. Building the PSBT from the items in
            /// reverse gives the same ordered PSBT, and `try_sort` hands it to
            /// the updater. A [`Seeded`] sorter orders the same way.
            #[test]
            fn sorting_orders_inputs_and_outputs_by_their_sort_keys(
                mode in modes(),
                seeded in any::<bool>(),
                uuid in uuids(),
                seed in seeds(),
                input_keys in keys(),
                output_keys in keys(),
            ) {
                let inputs = inputs(mode, &input_keys);
                let outputs = outputs(mode, &output_keys);
                let unordered = |inputs, outputs| unordered(mode, &uuid, &seed, inputs, outputs);
                let ordered = order(mode, seeded, unordered(inputs.clone(), outputs.clone()));
                let handed_off = hand_off(mode, seeded, unordered(inputs.clone(), outputs.clone()));
                prop_assert_eq!(handed_off.psbt(), ordered.clone());

                let explicit = |key: Option<&[u8]>| key.unwrap_or_default().to_vec();
                let expected_inputs = sorted_by_key(
                    inputs
                        .iter()
                        .map(|input| {
                            let outpoint = bitcoin::OutPoint {
                                txid: *input.previous_txid(),
                                vout: *input.spent_output_index(),
                            };
                            let id = bitcoin::consensus::serialize(&outpoint);
                            let key = (explicit(input.sort_key()), derived_key(&uuid, &seed, &id));
                            (key, input.clone())
                        })
                        .collect(),
                );
                let expected_outputs = sorted_by_key(
                    outputs
                        .iter()
                        .map(|output| {
                            let id = output.unique_id().as_bytes();
                            let key = (explicit(output.sort_key()), derived_key(&uuid, &seed, id));
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
                    seeded,
                    unordered(
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
            fn try_set_seed_joins_a_preset_seed(
                seed in seeds(),
                other in seeds(),
                preset in 0u8..3, // unset, the same seed, or another seed
            ) {
                let old_seed = match preset {
                    0 => None,
                    1 => Some(&seed),
                    _ => Some(&other),
                };
                let psbt = |seed: Option<&Vec<u8>>| {
                    let mut psbt = empty_psbt();
                    if let Some(seed) = seed {
                        psbt.global.set_sort_seed(SortSeed::from(seed.clone()));
                    }
                    psbt.inputs = InputSet::try_from_inputs((0..4).map(|txid| make_input(txid, 0))).unwrap();
                    psbt.outputs = OutputSet::try_from_outputs((0..4).map(make_output_with_uid)).unwrap();
                    psbt
                };
                let mixed = |seed| Sorter::<Unmodifiable, Mixed>::try_from_unordered_psbt(psbt(seed)).unwrap();
                let derived = |seed| {
                    Sorter::<Unmodifiable, ExplicitKeysForbidden>::try_from_unordered_psbt(with_derivation(
                        psbt(seed),
                        SortKeyDerivation::ExplicitKeysForbidden,
                    ))
                    .unwrap()
                };
                let set = || SortSeed::from(seed.clone());

                match old_seed.filter(|old| **old != seed) {
                    None => {
                        prop_assert_eq!(
                            mixed(old_seed).try_set_seed(set()).unwrap().try_sort().unwrap().psbt(),
                            mixed(Some(&seed)).try_into_ordered_psbt().unwrap()
                        );
                        prop_assert_eq!(
                            derived(old_seed).try_set_seed(set()).unwrap().try_into_ordered_psbt().unwrap(),
                            derived(Some(&seed)).try_into_ordered_psbt().unwrap()
                        );
                    }
                    Some(old) => {
                        let conflict = SeedConflict {
                            existing: SortSeed::from(old.clone()),
                            requested: set(),
                        };
                        prop_assert_eq!(
                            mixed(old_seed).try_set_seed(set()).err(),
                            Some(conflict.clone())
                        );
                        prop_assert_eq!(
                            derived(old_seed).try_set_seed(set()).err(),
                            Some(conflict)
                        );
                    }
                }
            }

            /// Setting a UUID refines the mode to [`Seeded`] and orders like a
            /// PSBT that already carries that UUID. A different preset UUID is
            /// rejected.
            #[test]
            fn try_set_uuid_joins_a_preset_uuid(
                uuid in uuids(),
                other in uuids(),
                preset in 0u8..3, // unset, the same UUID, or another UUID
                mode in modes(),
            ) {
                let old_uuid = match preset {
                    0 => None,
                    1 => Some(&uuid),
                    _ => Some(&other),
                };
                let keys = [(vec![0], true), (vec![1], false), (vec![0], false)];
                let psbt = |uuid: Option<&Vec<u8>>| {
                    let mut psbt =
                        unordered(mode, &[13; 16], &[], inputs(mode, &keys), outputs(mode, &keys));
                    psbt.global.uuid = uuid.cloned().map(|uuid| TxUuid::try_from(uuid).unwrap());
                    psbt
                };
                let joined = |psbt: UnorderedPsbt| -> Result<Psbt, UuidError> {
                    let set = TxUuid::try_from(uuid.clone()).unwrap();
                    match mode {
                        Mode::Mixed => sorter::<Mixed>(psbt).try_set_uuid(set).map(|s| s.into_ordered_psbt()),
                        Mode::ExplicitKeysForbidden => sorter::<ExplicitKeysForbidden>(psbt).try_set_uuid(set).map(|s| s.into_ordered_psbt()),
                        Mode::ExplicitKeysRequired => sorter::<ExplicitKeysRequired>(psbt)
                            .try_set_uuid(set)
                            .map(|s| s.try_into_ordered_psbt().unwrap()),
                    }
                };

                let expected = match old_uuid.filter(|old| **old != uuid) {
                    None => Ok(order(mode, false, psbt(Some(&uuid)))),
                    Some(old) => Err(UuidError::Conflict {
                        existing: TxUuid::try_from(old.clone()).unwrap(),
                        requested: TxUuid::try_from(uuid.clone()).unwrap(),
                    }),
                };
                prop_assert_eq!(joined(psbt(old_uuid)), expected);
            }

            /// `try_sort` hands the ordered PSBT, with its flags, to the role
            /// that matches the modifiability.
            #[test]
            fn try_sort_hands_the_ordered_psbt_to_the_role_for_its_modifiability(
                uuid in uuids(),
            ) {
                fn sorter<M: Modifiability>(flags: u8, uuid: &[u8]) -> Sorter<M, ExplicitKeysForbidden> {
                    let mut psbt = empty_psbt();
                    psbt.global.set_tx_modifiable_flags(flags);
                    psbt.global.set_uuid(TxUuid::try_from(uuid.to_vec()).unwrap());
                    psbt.inputs = InputSet::try_from_inputs([make_input(1, 0), make_input(2, 0)]).unwrap();
                    psbt.outputs = OutputSet::try_from_outputs([make_output_with_uid(1)]).unwrap();
                    Sorter::try_from_unordered_psbt(with_derivation(psbt, SortKeyDerivation::ExplicitKeysForbidden)).unwrap()
                }

                let both: v2::Constructor<v2::Modifiable> = sorter::<BothModifiable>(0x03, &uuid).try_sort().unwrap();
                let inputs: v2::Constructor<v2::InputsOnlyModifiable> = sorter::<InputsModifiable>(0x01, &uuid).try_sort().unwrap();
                let outputs: v2::Constructor<v2::OutputsOnlyModifiable> = sorter::<OutputsModifiable>(0x02, &uuid).try_sort().unwrap();
                let updater: v2::Updater = sorter::<Unmodifiable>(0x00, &uuid).try_sort().unwrap();

                let ordered = sorter::<Unmodifiable>(0x00, &uuid).try_into_ordered_psbt().unwrap();
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
        }
    }
}
