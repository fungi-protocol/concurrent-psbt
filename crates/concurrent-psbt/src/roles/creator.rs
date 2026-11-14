use std::marker::PhantomData;

use bitcoin::locktime::absolute;
use bitcoin::transaction;
use psbt_v2::v2;

use crate::global::{Global, SortKeyDerivation, SortSeed};
use crate::input::InputSet;
use crate::output::OutputSet;
use crate::sorter::{BothModifiable, DerivedOnly, ExplicitOnly, Mixed, Seeded};
use crate::tx::UnorderedPsbt;

use super::constructor::Constructor;

/// Entry point for creating new unordered PSBTs: BIP 370's [`v2::Creator`]
/// with a sort mode.
///
/// The PSBT is marked unordered (`PSBT_GLOBAL_TX_UNORDERED`). Unlike
/// [`v2::Creator`] there is no `sighash_single`, since an unordered PSBT
/// cannot have the `SIGHASH_SINGLE` flag. The sort mode determines how the
/// [`Sorter`](crate::sorter::Sorter) orders it:
///
/// - [`Creator::new`] selects [`Mixed`]: explicit sort keys are used where
///   present, and the rest are derived from the sort seed.
/// - [`derived_only`](Creator::derived_only) selects [`DerivedOnly`]:
///   every sort key is derived from the sort seed.
/// - [`explicit_only`](Creator::explicit_only) selects [`ExplicitOnly`]: every input and
///   output needs an explicit sort key.
///
/// In [`Mixed`] and [`DerivedOnly`] mode, `set_seed` sets
/// `PSBT_GLOBAL_SORT_SEED` up front and refines the mode to [`Seeded`], so a
/// seeded [`DerivedOnly`] PSBT sorts without failing.
///
/// [`constructor`](Self::constructor) produces a [`Constructor`] with both
/// inputs and outputs modifiable. A PSBT cannot be a coinbase transaction,
/// so it always starts out needing inputs; a constructor that will add no
/// outputs comes from `constructor().no_more_outputs()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Creator<S = Mixed> {
    creator: v2::Creator,
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION`, if set.
    sort_key_derivation: Option<SortKeyDerivation>,
    seed: Option<SortSeed>,
    _sort: PhantomData<S>,
}

impl Creator<Mixed> {
    /// Create a [`Creator`] in [`Mixed`] mode, leaving
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` unset.
    pub fn new() -> Self {
        Creator {
            creator: v2::Creator::new(),
            sort_key_derivation: None,
            seed: None,
            _sort: PhantomData,
        }
    }

    /// Set `PSBT_GLOBAL_SORT_SEED`, from which missing sort keys are derived.
    ///
    /// The seed may instead be set later, on the constructor or sorter.
    pub fn set_seed(self, seed: SortSeed) -> Creator<Seeded<Mixed>> {
        self.with_seed(seed)
    }

    /// Switch to [`DerivedOnly`] mode (`PSBT_GLOBAL_SORT_KEY_DERIVATION = 0x01`).
    pub fn derived_only(self) -> Creator<DerivedOnly> {
        self.with_sort_key_derivation(SortKeyDerivation::Derived)
    }

    /// Switch to [`ExplicitOnly`] mode (`PSBT_GLOBAL_SORT_KEY_DERIVATION = 0x00`).
    pub fn explicit_only(self) -> Creator<ExplicitOnly> {
        self.with_sort_key_derivation(SortKeyDerivation::Explicit)
    }

    fn with_sort_key_derivation<T>(mut self, derivation: SortKeyDerivation) -> Creator<T> {
        self.sort_key_derivation = Some(derivation);
        self.into_mode()
    }
}

impl Creator<DerivedOnly> {
    /// Set `PSBT_GLOBAL_SORT_SEED`, from which every sort key is derived.
    ///
    /// The seed may instead be set later, on the constructor or sorter.
    pub fn set_seed(self, seed: SortSeed) -> Creator<Seeded<DerivedOnly>> {
        self.with_seed(seed)
    }
}

impl<S> Creator<S> {
    /// Set the fallback lock time, as [`v2::Creator::fallback_lock_time`] does.
    pub fn fallback_lock_time(mut self, fallback: absolute::LockTime) -> Self {
        self.creator = self.creator.fallback_lock_time(fallback);
        self
    }

    /// Set the transaction version, as [`v2::Creator::transaction_version`]
    /// does.
    pub fn transaction_version(mut self, version: transaction::Version) -> Self {
        self.creator = self.creator.transaction_version(version);
        self
    }

    fn into_mode<T>(self) -> Creator<T> {
        Creator {
            creator: self.creator,
            sort_key_derivation: self.sort_key_derivation,
            seed: self.seed,
            _sort: PhantomData,
        }
    }

    fn with_seed<T>(mut self, seed: SortSeed) -> Creator<Seeded<T>> {
        self.seed = Some(seed);
        self.into_mode()
    }

    /// Build a [`Constructor`] with both inputs and outputs modifiable
    /// (`tx_modifiable_flags = 0x03`).
    pub fn constructor(self) -> Constructor<BothModifiable, S> {
        let mut global =
            Global::try_from(self.creator.psbt().global).expect("a new PSBT has no sort fields");
        global.set_unordered();
        global.tx_modifiable_flags = 0x03;
        if let Some(derivation) = self.sort_key_derivation {
            global.set_sort_key_derivation(derivation);
        }
        if let Some(seed) = self.seed {
            global.set_sort_seed(seed);
        }
        Constructor::from_unordered(UnorderedPsbt {
            global,
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        })
    }
}

impl Default for Creator<Mixed> {
    fn default() -> Self {
        Creator::new()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn sort_seed(bytes: Vec<u8>) -> SortSeed {
        SortSeed::try_from(bytes).expect("test seeds are at least 16 bytes")
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::input::Input;
        use crate::output::{Output, UniqueId};
        use crate::sorter::SortError;
        use bitcoin::{Amount, OutPoint, ScriptBuf};

        fn input() -> Input {
            Input::new(&OutPoint::null())
        }

        fn output() -> Output {
            Output::new(UniqueId::new(vec![1]), Amount::ZERO, ScriptBuf::new())
        }

        #[test]
        fn each_sort_mode_sets_its_sort_fields() {
            let seed = vec![7; 16];

            // The annotations check the typestate each method returns.
            let mixed: Creator<Mixed> = Creator::default();
            let seeded_mixed: Creator<Seeded<Mixed>> =
                Creator::new().set_seed(sort_seed(seed.clone()));
            let derived: Creator<DerivedOnly> = Creator::new().derived_only();
            let seeded_derived: Creator<Seeded<DerivedOnly>> = Creator::new()
                .derived_only()
                .set_seed(sort_seed(seed.clone()));
            let explicit: Creator<ExplicitOnly> = Creator::new().explicit_only();

            // name, PSBT, expected sort key derivation, expected sort seed
            let cases = [
                ("mixed", mixed.constructor().into_inner(), None, None),
                (
                    "seeded mixed",
                    seeded_mixed.constructor().into_inner(),
                    None,
                    Some(seed.as_slice()),
                ),
                (
                    "derived only",
                    derived.constructor().into_inner(),
                    Some(SortKeyDerivation::Derived),
                    None,
                ),
                (
                    "seeded derived only",
                    seeded_derived.constructor().into_inner(),
                    Some(SortKeyDerivation::Derived),
                    Some(seed.as_slice()),
                ),
                (
                    "explicit only",
                    explicit.constructor().into_inner(),
                    Some(SortKeyDerivation::Explicit),
                    None,
                ),
            ];
            for (name, psbt, derivation, expected_seed) in cases {
                assert_eq!(psbt.global.sort_key_derivation(), derivation, "{name}");
                assert_eq!(psbt.global.sort_seed(), expected_seed, "{name}");
                assert!(psbt.global.is_unordered(), "{name}");
                assert_eq!(psbt.global.tx_modifiable_flags, 0x03, "{name}");
                assert_eq!(psbt.inputs, InputSet::default(), "{name}");
                assert_eq!(psbt.outputs, OutputSet::default(), "{name}");
            }
        }

        #[test]
        fn a_derived_only_psbt_without_a_seed_cannot_be_sorted() {
            let unseeded = Creator::new()
                .derived_only()
                .constructor()
                .input(input())
                .unwrap()
                .no_more_inputs()
                .no_more_outputs()
                .try_into_ordered_psbt();
            assert!(matches!(unseeded, Err(SortError::MissingSortSeed)));
        }

        #[test]
        fn a_created_psbt_is_constructed_then_sorted() {
            let seed = vec![7; 16];
            let sorter: crate::sorter::Sorter<_, Seeded<DerivedOnly>> = Creator::new()
                .derived_only()
                .set_seed(sort_seed(seed.clone()))
                .constructor()
                .input(input())
                .unwrap()
                .output(output())
                .unwrap()
                .no_more_inputs()
                .no_more_outputs();
            let ordered = sorter.into_ordered_psbt();

            let global = Global::try_from(ordered.global.clone()).expect("well-formed sort fields");
            assert!(!global.is_unordered());
            assert_eq!(ordered.global.tx_modifiable_flags, 0x00);
            assert_eq!(
                global.sort_key_derivation(),
                Some(SortKeyDerivation::Derived)
            );
            assert_eq!(global.sort_seed(), Some(seed.as_slice()));
            assert_eq!(ordered.inputs.len(), 1);
            assert_eq!(ordered.outputs.len(), 1);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        /// Check that `creator` builds a PSBT with `lock_time`, `version` and
        /// `expected_seed`.
        fn check_forwarded<S>(
            creator: Creator<S>,
            lock_time: absolute::LockTime,
            version: transaction::Version,
            expected_seed: Option<&[u8]>,
        ) -> Result<(), TestCaseError> {
            let psbt = creator.constructor().into_inner();
            prop_assert_eq!(psbt.global.fallback_lock_time, Some(lock_time));
            prop_assert_eq!(psbt.global.tx_version, version);
            prop_assert_eq!(psbt.global.sort_seed(), expected_seed);
            Ok(())
        }

        proptest! {
            /// The lock time and version are set before the sort mode, so
            /// changing the mode must keep them.
            #[test]
            fn the_creator_forwards_the_seed_lock_time_and_version_it_was_given(
                seed in proptest::collection::vec(any::<u8>(), 16..=32),
                lock_time in any::<u32>(),
                version in any::<i32>(),
            ) {
                let lock_time = absolute::LockTime::from_consensus(lock_time);
                let version = transaction::Version(version);
                let configured = || {
                    Creator::new()
                        .fallback_lock_time(lock_time)
                        .transaction_version(version)
                };
                let seeded = || sort_seed(seed.clone());
                let expected = Some(seed.as_slice());

                check_forwarded(configured(), lock_time, version, None)?;
                check_forwarded(configured().set_seed(seeded()), lock_time, version, expected)?;
                check_forwarded(configured().derived_only(), lock_time, version, None)?;
                check_forwarded(
                    configured().derived_only().set_seed(seeded()),
                    lock_time,
                    version,
                    expected,
                )?;
                check_forwarded(configured().explicit_only(), lock_time, version, None)?;
            }
        }
    }
}
