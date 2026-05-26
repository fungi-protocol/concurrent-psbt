use std::marker::PhantomData;

use bitcoin::locktime::absolute;
use bitcoin::transaction;
use psbt_v2::v2;

use crate::global::{Global, SortKeyDerivation, SortSeed, TxUuid};
use crate::input::InputSet;
use crate::output::OutputSet;
use crate::sorter::{BothModifiable, ExplicitKeysForbidden, ExplicitKeysRequired, Mixed, Seeded};
use crate::tx::UnorderedPsbt;

use super::constructor::Constructor;

/// Entry point for creating new unordered PSBTs: BIP 370's [`v2::Creator`]
/// with a sort mode.
///
/// The PSBT is marked unordered (`PSBT_GLOBAL_TX_UNORDERED`) and gets a fresh
/// random `PSBT_GLOBAL_TX_UUID`, from which the
/// [`Sorter`](crate::sorter::Sorter) derives sort keys, so its sort mode is
/// always [`Seeded`]. Unlike
/// [`v2::Creator`] there is no `sighash_single`, since an unordered PSBT
/// cannot have the `SIGHASH_SINGLE` flag. The sort mode determines how the
/// sorter orders it:
///
/// - [`Creator::new`] selects [`Mixed`]: inputs and outputs are ordered by
///   their explicit sort keys, a missing key counting as empty, then by their
///   derived keys.
/// - [`explicit_keys_forbidden`](Creator::explicit_keys_forbidden) selects
///   [`ExplicitKeysForbidden`]: every sort key is derived.
/// - [`explicit_keys_required`](Creator::explicit_keys_required) selects
///   [`ExplicitKeysRequired`]: every input and output needs a non-empty
///   explicit sort key, and derived keys break ties.
///
/// In every mode, [`set_seed`](Creator::set_seed) sets the auxiliary
/// `PSBT_GLOBAL_SORT_SEED` up front.
///
/// [`constructor`](Self::constructor) produces a [`Constructor`] at the
/// bottom of the modifiability lattice, with both flags set. Freezing
/// inputs or outputs clears the corresponding flag.
#[derive(Debug, Clone, PartialEq)]
pub struct Creator<S = Seeded<Mixed>> {
    creator: v2::Creator,
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION`, if set.
    sort_key_derivation: Option<SortKeyDerivation>,
    seed: Option<SortSeed>,
    uuid: TxUuid,
    _sort: PhantomData<S>,
}

impl Creator<Seeded<Mixed>> {
    /// Create a [`Creator`] in [`Mixed`] mode, leaving
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` unset, with a fresh
    /// [`TxUuid::generate`].
    ///
    /// # Panics
    /// Panics if the operating system's RNG is unavailable.
    pub fn new() -> Self {
        Creator {
            creator: v2::Creator::new(),
            sort_key_derivation: None,
            seed: None,
            uuid: TxUuid::generate(),
            _sort: PhantomData,
        }
    }

    /// Switch to [`ExplicitKeysForbidden`] mode
    /// (`PSBT_GLOBAL_SORT_KEY_DERIVATION = 0x01`).
    pub fn explicit_keys_forbidden(self) -> Creator<Seeded<ExplicitKeysForbidden>> {
        self.with_sort_key_derivation(SortKeyDerivation::ExplicitKeysForbidden)
    }

    /// Switch to [`ExplicitKeysRequired`] mode
    /// (`PSBT_GLOBAL_SORT_KEY_DERIVATION = 0x00`).
    pub fn explicit_keys_required(self) -> Creator<Seeded<ExplicitKeysRequired>> {
        self.with_sort_key_derivation(SortKeyDerivation::ExplicitKeysRequired)
    }

    fn with_sort_key_derivation<T>(self, derivation: SortKeyDerivation) -> Creator<T> {
        Creator {
            creator: self.creator,
            sort_key_derivation: Some(derivation),
            seed: self.seed,
            uuid: self.uuid,
            _sort: PhantomData,
        }
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

    /// Set the auxiliary `PSBT_GLOBAL_SORT_SEED`, which the sort seed hashes
    /// together with the UUID.
    ///
    /// The seed may instead be set later, on the constructor or sorter.
    pub fn set_seed(mut self, seed: SortSeed) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Build a [`Constructor`] with both inputs and outputs modifiable
    /// (`tx_modifiable_flags = 0x03`).
    pub fn constructor(self) -> Constructor<BothModifiable, S> {
        let mut global =
            Global::try_from(self.creator.psbt().global).expect("a new PSBT has no sort fields");
        global.set_unordered();
        global.set_tx_modifiable_flags(0x03);
        global.set_uuid(self.uuid);
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

impl Default for Creator<Seeded<Mixed>> {
    fn default() -> Self {
        Creator::new()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::input::Input;
        use crate::output::{Output, UniqueId};
        use bitcoin::{Amount, OutPoint, ScriptBuf};

        fn input() -> Input {
            Input::new(&OutPoint::null())
        }

        fn output() -> Output {
            Output::new(
                UniqueId::try_from(vec![1; 16]).unwrap(),
                Amount::ZERO,
                ScriptBuf::new(),
            )
        }

        #[test]
        fn each_sort_mode_sets_its_sort_fields() {
            let seed = vec![7; 16];
            let aux = || SortSeed::from(seed.clone());

            // The annotations check the typestate each method returns: a
            // created PSBT has a UUID, so every mode is seeded.
            let mixed: Creator<Seeded<Mixed>> = Creator::default();
            let aux_mixed: Creator<Seeded<Mixed>> = Creator::new().set_seed(aux());
            let derived: Creator<Seeded<ExplicitKeysForbidden>> =
                Creator::new().explicit_keys_forbidden();
            let aux_derived: Creator<Seeded<ExplicitKeysForbidden>> =
                Creator::new().explicit_keys_forbidden().set_seed(aux());
            let explicit: Creator<Seeded<ExplicitKeysRequired>> =
                Creator::new().explicit_keys_required();
            let aux_explicit: Creator<Seeded<ExplicitKeysRequired>> =
                Creator::new().set_seed(aux()).explicit_keys_required();

            let (derived_mode, explicit_mode) = (
                Some(SortKeyDerivation::ExplicitKeysForbidden),
                Some(SortKeyDerivation::ExplicitKeysRequired),
            );
            let seed = Some(seed.as_slice());
            // name, PSBT, expected sort key derivation, expected sort seed
            let cases = [
                ("mixed", mixed.constructor().into_inner(), None, None),
                (
                    "mixed with aux seed",
                    aux_mixed.constructor().into_inner(),
                    None,
                    seed,
                ),
                (
                    "explicit keys forbidden",
                    derived.constructor().into_inner(),
                    derived_mode,
                    None,
                ),
                (
                    "explicit keys forbidden with aux seed",
                    aux_derived.constructor().into_inner(),
                    derived_mode,
                    seed,
                ),
                (
                    "explicit keys required",
                    explicit.constructor().into_inner(),
                    explicit_mode,
                    None,
                ),
                (
                    "explicit keys required with aux seed",
                    aux_explicit.constructor().into_inner(),
                    explicit_mode,
                    seed,
                ),
            ];
            let mut uuids = std::collections::HashSet::new();
            for (name, psbt, derivation, expected_seed) in cases {
                assert_eq!(psbt.global.sort_key_derivation(), derivation, "{name}");
                assert_eq!(psbt.global.sort_seed(), expected_seed, "{name}");
                assert!(psbt.global.is_unordered(), "{name}");
                assert_eq!(*psbt.global.tx_modifiable_flags(), 0x03, "{name}");
                assert_eq!(psbt.inputs, InputSet::default(), "{name}");
                assert_eq!(psbt.outputs, OutputSet::default(), "{name}");
                let uuid = psbt.global.uuid().expect("a created PSBT has a UUID");
                assert_eq!(uuid.len(), 16, "{name}");
                assert!(uuids.insert(uuid.to_vec()), "{name} reuses a UUID");
            }
        }

        #[test]
        fn a_created_psbt_is_constructed_then_sorted() {
            let creator = Creator::new().explicit_keys_forbidden();
            let uuid = creator.uuid.clone();
            let sorter: crate::sorter::Sorter<_, Seeded<ExplicitKeysForbidden>> = creator
                .constructor()
                .input(input())
                .unwrap()
                .output(output())
                .unwrap()
                .no_more_inputs()
                .no_more_outputs();
            // A created PSBT is seeded, so it sorts infallibly, without an
            // aux seed.
            let ordered = sorter.into_ordered_psbt();

            let global = Global::try_from(ordered.global.clone()).expect("well-formed sort fields");
            assert!(!global.is_unordered());
            assert_eq!(ordered.global.tx_modifiable_flags, 0x00);
            assert_eq!(
                global.sort_key_derivation(),
                Some(SortKeyDerivation::ExplicitKeysForbidden)
            );
            assert_eq!(global.uuid(), Some(uuid.as_bytes()));
            assert_eq!(global.sort_seed(), None);
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
            prop_assert_eq!(*psbt.global.fallback_lock_time(), Some(lock_time));
            prop_assert_eq!(*psbt.global.tx_version(), version);
            prop_assert_eq!(psbt.global.sort_seed(), expected_seed);
            Ok(())
        }

        proptest! {
            /// The lock time and version are set before the sort mode, so
            /// changing the mode must keep them.
            #[test]
            fn the_creator_forwards_the_seed_lock_time_and_version_it_was_given(
                seed in proptest::collection::vec(any::<u8>(), 0..=32),
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
                let seeded = || SortSeed::from(seed.clone());
                let expected = Some(seed.as_slice());

                check_forwarded(configured(), lock_time, version, None)?;
                check_forwarded(configured().set_seed(seeded()), lock_time, version, expected)?;
                check_forwarded(configured().explicit_keys_forbidden(), lock_time, version, None)?;
                check_forwarded(
                    configured().explicit_keys_forbidden().set_seed(seeded()),
                    lock_time,
                    version,
                    expected,
                )?;
                check_forwarded(configured().explicit_keys_required(), lock_time, version, None)?;
                check_forwarded(
                    configured().set_seed(seeded()).explicit_keys_required(),
                    lock_time,
                    version,
                    expected,
                )?;
            }
        }
    }
}
