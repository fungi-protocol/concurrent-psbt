//! The BIP 370 Constructor role: adding inputs and outputs.
//!
//! [`Constructor`] tracks modifiability and sort mode in the type system.
#![allow(clippy::result_large_err)]

use std::marker::PhantomData;

use bitcoin::{Amount, ScriptBuf};
use psbt_v2::v2::{DetermineLockTimeError, Psbt};

use crate::global::{SortSeed, TxUuid};
use crate::input::{Input, InputSet};
use crate::lattice::join::Join;
use crate::output::{Output, OutputSet, UniqueId};
use crate::sorter::{
    BothModifiable, InputsModifiable, Modifiable, OutputsModifiable, SeedConflict, Seeded,
    SortMode, Sorter, Unmodifiable, UuidError, check_flags, check_sort_mode, join_seed, join_uuid,
};
use crate::tx::{UnorderedPsbt, check_input_sighash};

mod error;
mod result;

pub use error::ConstructorError;
pub use result::ResultConstructor;

#[cfg(doc)]
use crate::sorter::{ExplicitKeysForbidden, ExplicitKeysRequired};

/// Typed wrapper that tracks modifiability `M` and sort state `S`.
///
/// [`try_from_psbt`](Self::try_from_psbt) checks that the input and output
/// modifiability flags match `M` and the sort fields agree with `S`.
/// A [`Seeded`] mode also requires a transaction UUID.
/// [`try_set_uuid`](Self::try_set_uuid) refines it to a [`Seeded`] mode.
#[derive(Debug)]
pub struct Constructor<M, S>(pub(crate) UnorderedPsbt, pub(crate) PhantomData<(M, S)>);

impl<M, S> Constructor<M, S> {
    /// Consume the constructor and return the internal unordered PSBT representation.
    pub fn into_inner(self) -> UnorderedPsbt {
        self.0
    }

    /// Convert to a BIP 370 [`Psbt`].
    ///
    /// # Errors
    /// Returns [`DetermineLockTimeError`] if the inputs' lock time
    /// requirements conflict.
    pub fn try_into_psbt(self) -> Result<Psbt, DetermineLockTimeError> {
        let psbt = self.0.into_psbt();
        psbt.determine_lock_time()?;
        Ok(psbt)
    }

    /// Join two constructors, producing a [`ResultConstructor`] that may contain conflicts.
    pub fn try_join(self, other: Self) -> ResultConstructor<M, S> {
        let result = self.0.wrap().join(other.0.wrap());
        ResultConstructor(result, PhantomData)
    }

    /// Lift into the result domain as a conflict-free [`ResultConstructor`].
    pub fn wrap(self) -> ResultConstructor<M, S> {
        ResultConstructor(self.0.wrap(), PhantomData)
    }

    pub(crate) fn from_unordered(psbt: UnorderedPsbt) -> Self {
        Constructor(psbt, PhantomData)
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

impl<M, S> Constructor<M, S>
where
    Seeded<S>: SortMode,
{
    /// Join `uuid` into `PSBT_GLOBAL_TX_UUID`, setting it if unset, and
    /// refine the mode to [`Seeded`].
    ///
    /// # Errors
    /// Returns [`UuidError::Conflict`] if the PSBT already has a different UUID.
    pub fn try_set_uuid(mut self, uuid: TxUuid) -> Result<Constructor<M, Seeded<S>>, UuidError> {
        join_uuid(&mut self.0.global, uuid)?;
        Ok(Constructor::from_unordered(self.0))
    }
}

impl<M: Modifiable, S: SortMode> Constructor<M, S> {
    /// Parse a v2 PSBT into a constructor.
    ///
    /// # Errors
    /// Returns [`ConstructorError::FlagsMismatch`] if `tx_modifiable_flags`
    /// does not match `M`, propagates lower-level unordered PSBT parse
    /// errors, then returns [`ConstructorError::SortMode`] if the sort fields
    /// do not select `S`.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, ConstructorError> {
        check_flags::<M>(psbt.global.tx_modifiable_flags)?;
        let psbt = UnorderedPsbt::try_from_psbt(psbt)?;
        check_sort_mode::<S>(&psbt)?;
        Ok(Constructor::from_unordered(psbt))
    }

    /// Join with a PSBT that shares this one's global fields and holds only
    /// the given inputs and outputs.
    ///
    /// The global fields are this PSBT's, so the sort mode stays `S`, and the
    /// join marks a sort key that `S` rules out as a conflict.
    fn join_with(self, inputs: InputSet, outputs: OutputSet) -> Result<Self, ConstructorError> {
        let other = UnorderedPsbt {
            global: self.0.global.clone(),
            inputs,
            outputs,
        };
        let psbt = self
            .0
            .wrap()
            .join(other.wrap())
            .try_unwrap()
            .map_err(|result| ConstructorError::Conflict(Box::new(result)))?;
        Ok(Constructor::from_unordered(psbt))
    }

    /// Join with a PSBT holding only `input`. Its outputs are empty and
    /// modifiable, so they join cleanly with these whether or not they are
    /// frozen.
    fn add_input(self, input: Input) -> Result<Self, ConstructorError> {
        check_input_sighash(&input)?;
        let inputs = InputSet::try_from_inputs([input]).expect("a single input cannot repeat");
        self.join_with(inputs, OutputSet::default())
    }

    /// Join with a PSBT holding only `output`. Its inputs are empty and
    /// modifiable, so they join cleanly with these whether or not they are
    /// frozen.
    fn add_output(self, output: Output) -> Result<Self, ConstructorError> {
        let outputs = OutputSet::try_from_outputs([output]).expect("a single output cannot repeat");
        self.join_with(InputSet::default(), outputs)
    }
}

impl<S: SortMode> Constructor<BothModifiable, S> {
    /// Add an input to the PSBT.
    ///
    /// An input spending an outpoint already in the PSBT is joined with it.
    ///
    /// # Errors
    /// Returns [`ConstructorError::Unordered`] if an explicit sighash type differs
    /// from `SIGHASH_ALL`.
    /// Returns [`ConstructorError::Conflict`] if the two disagree, or if the
    /// input has a sort key in [`ExplicitKeysForbidden`] mode or an empty one
    /// in [`ExplicitKeysRequired`] mode, which makes the key a conflict.
    pub fn input(self, input: Input) -> Result<Self, ConstructorError> {
        self.add_input(input)
    }

    /// Add an output to the PSBT.
    ///
    /// An output whose unique ID is already in the PSBT is joined with it.
    ///
    /// # Errors
    /// Returns [`ConstructorError::Conflict`] if it disagrees with the output
    /// already holding that ID, or if it has a sort key in
    /// [`ExplicitKeysForbidden`] mode or an empty one in
    /// [`ExplicitKeysRequired`] mode, which makes the key a conflict.
    pub fn output(self, output: Output) -> Result<Self, ConstructorError> {
        self.add_output(output)
    }

    /// Add an output paying `amount` to `script_pubkey`, with a freshly
    /// generated [`UniqueId`].
    ///
    /// Each call generates a new random `PSBT_OUT_UNIQUE_ID`, so adding
    /// copies of the same txout yields distinct outputs.
    ///
    /// # Errors
    /// Returns [`ConstructorError::Conflict`] if the generated ID collides
    /// with an output that disagrees with this one.
    pub fn output_with_new_uid(
        self,
        amount: Amount,
        script_pubkey: ScriptBuf,
    ) -> Result<Self, ConstructorError> {
        self.output(Output::new(UniqueId::generate(), amount, script_pubkey))
    }

    /// Transition to [`OutputsModifiable`]: no more inputs will be added.
    ///
    /// Clears bit 0 of `tx_modifiable_flags` and freezes the inputs.
    pub fn no_more_inputs(self) -> Constructor<OutputsModifiable, S> {
        Constructor(self.0.no_more_inputs(), PhantomData)
    }

    /// Transition to [`InputsModifiable`]: no more outputs will be added.
    ///
    /// Clears bit 1 of `tx_modifiable_flags` and freezes the outputs.
    pub fn no_more_outputs(self) -> Constructor<InputsModifiable, S> {
        Constructor(self.0.no_more_outputs(), PhantomData)
    }
}

impl<S: SortMode> Constructor<InputsModifiable, S> {
    /// Add an input to the PSBT.
    ///
    /// An input spending an outpoint already in the PSBT is joined with it.
    ///
    /// # Errors
    /// Returns [`ConstructorError::Unordered`] if an explicit sighash type differs
    /// from `SIGHASH_ALL`.
    /// Returns [`ConstructorError::Conflict`] if the two disagree, or if the
    /// input has a sort key in [`ExplicitKeysForbidden`] mode or an empty one
    /// in [`ExplicitKeysRequired`] mode, which makes the key a conflict.
    pub fn input(self, input: Input) -> Result<Self, ConstructorError> {
        self.add_input(input)
    }

    /// Finalize inputs and transition to the [`Sorter`] for ordering.
    ///
    /// Clears bit 0 of `tx_modifiable_flags` and freezes the inputs.
    pub fn no_more_inputs(self) -> Sorter<Unmodifiable, S> {
        Sorter::from_unordered(self.0.no_more_inputs())
    }
}

impl<S: SortMode> Constructor<OutputsModifiable, S> {
    /// Add an output to the PSBT.
    ///
    /// An output whose unique ID is already in the PSBT is joined with it.
    ///
    /// # Errors
    /// Returns [`ConstructorError::Conflict`] if it disagrees with the output
    /// already holding that ID, or if it has a sort key in
    /// [`ExplicitKeysForbidden`] mode or an empty one in
    /// [`ExplicitKeysRequired`] mode, which makes the key a conflict.
    pub fn output(self, output: Output) -> Result<Self, ConstructorError> {
        self.add_output(output)
    }

    /// Add an output paying `amount` to `script_pubkey`, with a freshly
    /// generated [`UniqueId`].
    ///
    /// Each call generates a new random `PSBT_OUT_UNIQUE_ID`, so adding
    /// copies of the same txout yields distinct outputs.
    ///
    /// # Errors
    /// Returns [`ConstructorError::Conflict`] if the generated ID collides
    /// with an output that disagrees with this one.
    pub fn output_with_new_uid(
        self,
        amount: Amount,
        script_pubkey: ScriptBuf,
    ) -> Result<Self, ConstructorError> {
        self.output(Output::new(UniqueId::generate(), amount, script_pubkey))
    }

    /// Finalize outputs and transition to the [`Sorter`] for ordering.
    ///
    /// Clears bit 1 of `tx_modifiable_flags` and freezes the outputs.
    pub fn no_more_outputs(self) -> Sorter<Unmodifiable, S> {
        Sorter::from_unordered(self.0.no_more_outputs())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::global::{Global, SortKeyDerivation, TxUuid};
    use crate::sorter::{ExplicitKeysForbidden, ExplicitKeysRequired, Mixed};

    type TestConstructor = Constructor<BothModifiable, Mixed>;

    /// A [`Global`] marked unordered, with these flags, transaction version 2
    /// and `PSBT_GLOBAL_TX_UUID` `[9; 16]`.
    fn unordered_global(flags: u8) -> Global {
        let mut global = Global::default();
        global.set_tx_modifiable_flags(flags);
        global.set_tx_version(bitcoin::transaction::Version::TWO);
        global.set_unordered();
        global.set_uuid(TxUuid::try_from(vec![9; 16]).unwrap());
        global
    }

    /// An empty BIP 370 PSBT with this global map.
    fn empty_psbt(global: Global) -> Psbt {
        Psbt {
            global: global.into_v2(0, 0),
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn empty_constructor() -> TestConstructor {
        Constructor(
            UnorderedPsbt {
                global: unordered_global(0x03),
                inputs: InputSet::default(),
                outputs: OutputSet::default(),
            },
            PhantomData,
        )
    }

    /// The global map of an empty PSBT whose sort fields select mixed (0 and
    /// 1), explicit-keys-forbidden (2 and 3), or explicit-keys-required (4
    /// and 5) mode, with the sort seed `[7; 16]` if `mode` is odd.
    fn global_with_mode(flags: u8, mode: usize) -> Global {
        let mut global = unordered_global(flags);
        match mode {
            2 | 3 => global.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysForbidden),
            4 | 5 => global.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysRequired),
            _ => {}
        }
        if mode % 2 == 1 {
            global.set_sort_seed(SortSeed::from(vec![7; 16]));
        }
        global
    }

    fn psbt_with_mode(flags: u8, mode: usize) -> Psbt {
        empty_psbt(global_with_mode(flags, mode))
    }

    fn outpoint(tag: u8) -> bitcoin::OutPoint {
        use bitcoin::hashes::Hash;

        bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([tag; 32]),
            vout: u32::from(tag),
        }
    }

    fn test_input(tag: u8) -> Input {
        Input::new(&outpoint(tag))
    }

    fn test_output(tag: u8) -> Output {
        Output::new(
            UniqueId::try_from(vec![tag; 16]).unwrap(),
            Amount::ZERO,
            ScriptBuf::new(),
        )
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::input::SortKey;
        use crate::lattice::partial::Conflict;
        use crate::sorter::SortModeError;
        use crate::tx::UnorderedPsbtError;

        fn psbt_with(
            flags: u8,
            inputs: Vec<psbt_v2::v2::Input>,
            outputs: Vec<psbt_v2::v2::Output>,
        ) -> Psbt {
            Psbt {
                global: unordered_global(flags).into_v2(inputs.len(), outputs.len()),
                inputs,
                outputs,
            }
        }

        fn flags_psbt(flags: u8) -> Psbt {
            psbt_with(flags, vec![], vec![])
        }

        #[test]
        fn try_join_identical_is_ok() {
            let joined = empty_constructor().try_join(empty_constructor());
            assert!(joined.is_ok());
            assert!(joined.try_unwrap().is_ok());
        }

        #[test]
        fn clearing_input_modifiability_freezes_the_inputs_while_outputs_still_grow() {
            let outputs: Constructor<OutputsModifiable, Mixed> = empty_constructor()
                .input(test_input(1))
                .unwrap()
                .no_more_inputs();
            let outputs = outputs.output(test_output(1)).unwrap();
            assert_eq!(*outputs.0.global.tx_modifiable_flags() & 0x03, 0x02);
            assert_eq!(
                outputs.0.inputs,
                InputSet::try_from_inputs([test_input(1)]).unwrap().freeze()
            );

            let sorter: Sorter<Unmodifiable, Mixed> = outputs.no_more_outputs();
            let sorted = sorter.into_inner();
            assert_eq!(*sorted.global.tx_modifiable_flags() & 0x03, 0x00);
            assert_eq!(
                sorted.outputs,
                OutputSet::try_from_outputs([test_output(1)])
                    .unwrap()
                    .freeze()
            );
        }

        #[test]
        fn clearing_output_modifiability_freezes_the_outputs_while_inputs_still_grow() {
            let inputs: Constructor<InputsModifiable, Mixed> = empty_constructor()
                .output(test_output(1))
                .unwrap()
                .no_more_outputs();
            let inputs = inputs.input(test_input(1)).unwrap();
            assert_eq!(*inputs.0.global.tx_modifiable_flags() & 0x03, 0x01);
            assert_eq!(
                inputs.0.outputs,
                OutputSet::try_from_outputs([test_output(1)])
                    .unwrap()
                    .freeze()
            );

            let sorter: Sorter<Unmodifiable, Mixed> = inputs.no_more_inputs();
            let sorted = sorter.into_inner();
            assert_eq!(*sorted.global.tx_modifiable_flags() & 0x03, 0x00);
            assert_eq!(
                sorted.inputs,
                InputSet::try_from_inputs([test_input(1)]).unwrap().freeze()
            );
        }

        #[test]
        fn single_flag_psbts_parse_into_their_modifiability() {
            let inputs: Constructor<InputsModifiable, Mixed> =
                Constructor::try_from_psbt(flags_psbt(0x01)).unwrap();
            let sorter: Sorter<Unmodifiable, Mixed> =
                inputs.input(test_input(1)).unwrap().no_more_inputs();
            assert_eq!(sorter.into_inner().inputs.len(), 1);

            let outputs: Constructor<OutputsModifiable, Mixed> =
                Constructor::try_from_psbt(flags_psbt(0x02)).unwrap();
            let sorter: Sorter<Unmodifiable, Mixed> =
                outputs.output(test_output(1)).unwrap().no_more_outputs();
            assert_eq!(sorter.into_inner().outputs.len(), 1);
        }

        #[test]
        fn try_set_seed_keeps_the_sort_mode() {
            let seed = || SortSeed::from(vec![7; 16]);

            let mixed: Constructor<BothModifiable, Mixed> =
                empty_constructor().try_set_seed(seed()).unwrap();
            let ordered = mixed
                .output(test_output(1))
                .unwrap()
                .no_more_inputs()
                .no_more_outputs()
                .try_into_ordered_psbt()
                .expect("a mixed PSBT without sort keys can be sorted");
            assert_eq!(ordered.outputs.len(), 1);

            let explicit_keys_forbidden: Constructor<BothModifiable, ExplicitKeysForbidden> =
                Constructor::<BothModifiable, ExplicitKeysForbidden>::try_from_psbt(
                    psbt_with_mode(0x03, 2),
                )
                .unwrap()
                .try_set_seed(seed())
                .unwrap();
            let ordered = explicit_keys_forbidden
                .no_more_outputs()
                .input(test_input(1))
                .unwrap()
                .no_more_inputs()
                .try_into_ordered_psbt()
                .expect("a PSBT that forbids explicit keys can be sorted");
            assert_eq!(ordered.inputs.len(), 1);
        }

        #[test]
        fn try_from_psbt_reports_why_a_psbt_is_rejected() {
            let mut miscounted = flags_psbt(0x03);
            miscounted.global.input_count = 1;
            let mut empty_key = test_input(1);
            empty_key.set_sort_key(SortKey::try_from(vec![]).unwrap());
            let explicit_with_empty_key = Psbt {
                global: global_with_mode(0x03, 4).into_v2(1, 0),
                inputs: vec![empty_key.into()],
                outputs: vec![],
            };

            let cases = [
                (
                    "both-modifiable rejects inputs-only flags",
                    Constructor::<BothModifiable, Mixed>::try_from_psbt(flags_psbt(0x01)).err(),
                    ConstructorError::FlagsMismatch {
                        expected: 0x03,
                        actual: 0x01,
                    },
                ),
                (
                    "inputs-modifiable rejects outputs-only flags",
                    Constructor::<InputsModifiable, Mixed>::try_from_psbt(flags_psbt(0x02)).err(),
                    ConstructorError::FlagsMismatch {
                        expected: 0x01,
                        actual: 0x02,
                    },
                ),
                (
                    "outputs-modifiable rejects both-modifiable flags",
                    Constructor::<OutputsModifiable, Mixed>::try_from_psbt(flags_psbt(0x03)).err(),
                    ConstructorError::FlagsMismatch {
                        expected: 0x02,
                        actual: 0x03,
                    },
                ),
                (
                    "an output needs a unique ID",
                    Constructor::<BothModifiable, Mixed>::try_from_psbt(psbt_with(
                        0x03,
                        vec![],
                        vec![psbt_v2::v2::Output::default()],
                    ))
                    .err(),
                    ConstructorError::MissingUniqueId(0),
                ),
                (
                    "two inputs cannot spend one outpoint",
                    Constructor::<BothModifiable, Mixed>::try_from_psbt(psbt_with(
                        0x03,
                        vec![test_input(4).into(), test_input(4).into()],
                        vec![],
                    ))
                    .err(),
                    ConstructorError::DuplicateInput(outpoint(4)),
                ),
                (
                    "two outputs cannot share a unique ID",
                    Constructor::<BothModifiable, Mixed>::try_from_psbt(psbt_with(
                        0x03,
                        vec![],
                        vec![test_output(4).into(), test_output(4).into()],
                    ))
                    .err(),
                    ConstructorError::DuplicateUniqueId(UniqueId::try_from(vec![4; 16]).unwrap()),
                ),
                (
                    "parser errors are wrapped",
                    Constructor::<BothModifiable, Mixed>::try_from_psbt(miscounted).err(),
                    ConstructorError::Unordered(UnorderedPsbtError::InputCountMismatch {
                        declared: 1,
                        found: 0,
                    }),
                ),
                (
                    "ExplicitKeysForbidden needs derivation 0x01",
                    Constructor::<BothModifiable, ExplicitKeysForbidden>::try_from_psbt(
                        psbt_with_mode(0x03, 0),
                    )
                    .err(),
                    ConstructorError::SortMode(SortModeError::SortKeyDerivation {
                        expected: Some(SortKeyDerivation::ExplicitKeysForbidden),
                        actual: None,
                    }),
                ),
                (
                    "an empty key under explicit keys is a parser error before the sort mode check",
                    Constructor::<BothModifiable, Mixed>::try_from_psbt(explicit_with_empty_key)
                        .err(),
                    ConstructorError::Unordered(UnorderedPsbtError::EmptyInputSortKey(outpoint(1))),
                ),
            ];
            for (name, got, want) in cases {
                assert_eq!(got, Some(want), "{name}");
            }
        }

        #[test]
        fn try_into_psbt_rejects_conflicting_lock_times() {
            let [time, height] = crate::test_support::conflicting_lock_time_inputs();
            let conflicting = empty_constructor()
                .input(time)
                .unwrap()
                .input(height)
                .unwrap();
            assert!(conflicting.try_into_psbt().is_err());
        }

        #[test]
        fn the_result_domain_requires_output_unique_ids() {
            let psbt = psbt_with(0x03, vec![], vec![psbt_v2::v2::Output::default()]);
            assert_eq!(
                ResultConstructor::<BothModifiable, Mixed>::try_from_psbt(psbt).err(),
                Some(UnorderedPsbtError::MissingOutputUniqueId(0))
            );
        }

        #[test]
        fn a_sort_key_cannot_be_added_in_explicit_keys_forbidden_mode() {
            let explicit_keys_forbidden = || {
                Constructor::<BothModifiable, ExplicitKeysForbidden>::try_from_psbt(psbt_with_mode(
                    0x03, 2,
                ))
                .expect("an empty PSBT that forbids explicit keys")
            };
            let key = || SortKey::try_from(vec![1]).expect("one byte");
            let mut input = test_input(1);
            input.set_sort_key(key());
            let mut output = test_output(1);
            output.set_sort_key(key());
            // The join marks the key, and the derivation that rules it out.
            let marked = |result: Result<_, _>| match result {
                Err(ConstructorError::Conflict(psbt)) => {
                    psbt.global.sort_key_derivation
                        == Some(Err(Conflict::from_values([
                            SortKeyDerivation::ExplicitKeysForbidden,
                        ])))
                }
                _ => false,
            };
            assert!(marked(explicit_keys_forbidden().input(input)));
            assert!(marked(explicit_keys_forbidden().output(output)));
        }

        #[test]
        fn adding_an_input_checks_its_requested_sighash() {
            let spent = outpoint(2);
            for sighash in [None, Some(0x01), Some(0x00), Some(0x02), Some(0x101)] {
                let mut raw = psbt_v2::v2::Input::new(&spent);
                raw.sighash_type = sighash.map(psbt_v2::PsbtSighashType::from_u32);
                let input = Input::try_from(raw).unwrap();
                let expected = match sighash {
                    None | Some(0x01) => Ok(()),
                    _ => Err(ConstructorError::Unordered(
                        UnorderedPsbtError::UnorderedInputSighashNotAll(spent),
                    )),
                };
                assert_eq!(empty_constructor().input(input.clone()).map(drop), expected);
                assert_eq!(
                    empty_constructor().no_more_outputs().input(input).map(drop),
                    expected
                );
            }
        }

        #[test]
        fn an_input_or_output_joins_the_one_with_its_identity_unless_they_disagree() {
            let mut input = test_input(5);
            input.set_sequence(Some(bitcoin::Sequence(1)));
            let mut output = test_output(5);
            output.set_amount(Amount::from_sat(1));
            let with_both = || {
                empty_constructor()
                    .input(input.clone())
                    .unwrap()
                    .output(output.clone())
                    .unwrap()
            };

            let agreeing = with_both()
                .input(input.clone())
                .unwrap()
                .output(output.clone())
                .unwrap();
            assert_eq!(agreeing.into_inner(), with_both().into_inner());

            let mut other_input = input.clone();
            other_input.set_sequence(Some(bitcoin::Sequence(2)));
            let mut other_output = output.clone();
            other_output.set_amount(Amount::from_sat(2));
            for conflict in [
                with_both().input(other_input).err(),
                with_both().output(other_output).err(),
            ] {
                match conflict {
                    Some(ConstructorError::Conflict(result)) => assert!(!result.is_ok()),
                    other => panic!("expected a conflict, got {other:?}"),
                }
            }
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use std::collections::BTreeSet;

        use super::*;
        use proptest::prelude::*;

        fn constructor_with_version(version: i32) -> TestConstructor {
            let mut constructor = empty_constructor();
            constructor
                .0
                .global
                .set_tx_version(bitcoin::transaction::Version(version));
            constructor
        }

        fn input_set(tags: impl IntoIterator<Item = u8>) -> InputSet {
            let tags: BTreeSet<u8> = tags.into_iter().collect();
            InputSet::try_from_inputs(tags.into_iter().map(test_input)).unwrap()
        }

        fn output_set(tags: impl IntoIterator<Item = u8>) -> OutputSet {
            let tags: BTreeSet<u8> = tags.into_iter().collect();
            OutputSet::try_from_outputs(tags.into_iter().map(test_output)).unwrap()
        }

        proptest! {
            #[test]
            fn try_set_seed_joins_the_seed_of_the_psbt(
                seed in prop_oneof![Just(vec![7; 16]), proptest::collection::vec(any::<u8>(), 0..=32)],
                mode in 0usize..6,
            ) {
                // Odd modes preset the seed [7; 16].
                let preset = (mode % 2 == 1).then(|| vec![7; 16]);
                let sort_seed = |seed: &[u8]| SortSeed::from(seed.to_vec());
                fn joined<S: SortMode>(mode: usize, seed: SortSeed) -> Result<UnorderedPsbt, SeedConflict> {
                    Constructor::<BothModifiable, S>::try_from_psbt(psbt_with_mode(0x03, mode))
                        .unwrap()
                        .try_set_seed(seed)
                        .map(Constructor::into_inner)
                }
                let joined = match mode {
                    0 | 1 => joined::<Mixed>(mode, sort_seed(&seed)),
                    2 | 3 => joined::<ExplicitKeysForbidden>(mode, sort_seed(&seed)),
                    _ => joined::<ExplicitKeysRequired>(mode, sort_seed(&seed)),
                };
                match preset.filter(|preset| *preset != seed) {
                    None => prop_assert_eq!(
                        joined.map(|psbt| psbt.global.sort_seed().map(<[u8]>::to_vec)),
                        Ok(Some(seed))
                    ),
                    Some(preset) => prop_assert_eq!(
                        joined.err(),
                        Some(SeedConflict { existing: sort_seed(&preset), requested: sort_seed(&seed) })
                    ),
                }
            }

            /// Setting a UUID refines the mode to [`Seeded`]. A different
            /// preset UUID is rejected.
            #[test]
            fn try_set_uuid_joins_the_uuid_of_the_psbt(
                uuid in prop_oneof![Just(vec![9; 16]), proptest::collection::vec(any::<u8>(), 16..=32)],
                preset in any::<bool>(), // unset, or [9; 16]
                mode in 0usize..6,
            ) {
                fn joined<S: SortMode>(mode: usize, preset: bool, uuid: &[u8]) -> Result<UnorderedPsbt, UuidError>
                where
                    Seeded<S>: SortMode,
                {
                    let mut global = global_with_mode(0x03, mode);
                    if !preset {
                        global.uuid = None;
                    }
                    let seeded: Constructor<BothModifiable, Seeded<S>> =
                        Constructor::<BothModifiable, S>::try_from_psbt(empty_psbt(global))
                            .unwrap()
                            .try_set_uuid(TxUuid::try_from(uuid.to_vec()).unwrap())?;
                    Ok(seeded.into_inner())
                }
                let joined = match mode {
                    0 | 1 => joined::<Mixed>(mode, preset, &uuid),
                    2 | 3 => joined::<ExplicitKeysForbidden>(mode, preset, &uuid),
                    _ => joined::<ExplicitKeysRequired>(mode, preset, &uuid),
                };
                let preset = preset.then(|| vec![9; 16]);
                let expected = match preset.filter(|preset| *preset != uuid) {
                    None => Ok(Some(uuid)),
                    Some(preset) => Err(UuidError::Conflict {
                        existing: TxUuid::try_from(preset).unwrap(),
                        requested: TxUuid::try_from(uuid).unwrap(),
                    }),
                };
                prop_assert_eq!(
                    joined.map(|psbt| psbt.global.uuid().map(<[u8]>::to_vec)),
                    expected
                );
            }

            #[test]
            fn adding_inputs_and_outputs_keeps_one_of_each_identity(
                input_tags in proptest::collection::vec(0u8..4, 0..8),
                output_tags in proptest::collection::vec(0u8..4, 0..8),
            ) {
                let both = || {
                    let mut constructor = empty_constructor();
                    for &tag in &input_tags {
                        constructor = constructor.input(test_input(tag)).unwrap();
                    }
                    for &tag in &output_tags {
                        constructor = constructor.output(test_output(tag)).unwrap();
                    }
                    constructor
                };
                let inner = both().into_inner();
                prop_assert_eq!(&inner.inputs, &input_set(input_tags.iter().copied()));
                prop_assert_eq!(&inner.outputs, &output_set(output_tags.iter().copied()));

                // A frozen set keeps its members and the other set still grows.
                let sorted = both()
                    .no_more_inputs()
                    .output(test_output(9))
                    .unwrap()
                    .no_more_outputs()
                    .into_inner();
                prop_assert_eq!(&sorted.inputs, &input_set(input_tags.iter().copied()).freeze());
                let with_extra = output_tags.iter().copied().chain([9]);
                prop_assert_eq!(&sorted.outputs, &output_set(with_extra).freeze());

                let sorted = both()
                    .no_more_outputs()
                    .input(test_input(9))
                    .unwrap()
                    .no_more_inputs()
                    .into_inner();
                prop_assert_eq!(&sorted.outputs, &output_set(output_tags.iter().copied()).freeze());
                let with_extra = input_tags.iter().copied().chain([9]);
                prop_assert_eq!(&sorted.inputs, &input_set(with_extra).freeze());
            }

            #[test]
            fn output_with_new_uid_keeps_copies_of_one_txout_distinct(
                before in 0usize..4,
                after in 0usize..4,
            ) {
                let mut both = empty_constructor();
                for _ in 0..before {
                    both = both.output_with_new_uid(Amount::ZERO, ScriptBuf::new()).unwrap();
                }
                let mut outputs = both.no_more_inputs();
                for _ in 0..after {
                    outputs = outputs.output_with_new_uid(Amount::ZERO, ScriptBuf::new()).unwrap();
                }
                prop_assert_eq!(outputs.0.outputs.len(), before + after);
            }

            #[test]
            fn accessors_preserve_version(version in any::<i32>()) {
                let inner = constructor_with_version(version).into_inner();
                prop_assert_eq!(*inner.global.tx_version(), bitcoin::transaction::Version(version));

                let psbt = constructor_with_version(version).try_into_psbt().unwrap();
                prop_assert_eq!(psbt.global.tx_version, bitcoin::transaction::Version(version));

                let constructor = TestConstructor::from_unordered(inner);
                prop_assert_eq!(
                    *constructor.into_inner().global.tx_version(),
                    bitcoin::transaction::Version(version)
                );
            }

            #[test]
            fn clean_result_roundtrips_and_joins(version in any::<i32>()) {
                let constructor = constructor_with_version(version);
                let psbt = constructor.try_into_psbt().unwrap();
                let parsed = ResultConstructor::<BothModifiable, Mixed>::try_from_psbt(psbt)
                    .expect("an empty PSBT has no duplicate identities");
                prop_assert!(parsed.is_ok());

                let wrapped = constructor_with_version(version).wrap();
                let joined = parsed.join(wrapped);
                prop_assert!(joined.is_ok());
                let unwrapped = joined.try_unwrap().expect("identical values are compatible");
                prop_assert_eq!(
                    *unwrapped.into_inner().global.tx_version(),
                    bitcoin::transaction::Version(version)
                );
            }

            #[test]
            fn try_join_reports_different_versions(version in i32::MIN..i32::MAX) {
                let joined = constructor_with_version(version)
                    .try_join(constructor_with_version(version + 1));
                prop_assert!(!joined.is_ok());
                prop_assert!(joined.try_unwrap().is_err());
            }

            #[test]
            fn unwraps_only_the_selected_typestate(
                flags in 0u8..=0x03,
                mode in 0usize..6,
                uuid in any::<bool>(), // unset, or [9; 16]
            ) {
                fn check<M: Modifiable, S: SortMode>(
                    psbt: Psbt,
                    expected: bool,
                ) -> Result<(), TestCaseError> {
                    let result = ResultConstructor::<M, S>::try_from_psbt(psbt.clone())
                        .expect("an empty PSBT has no duplicate identities");
                    prop_assert_eq!(result.is_ok(), expected);
                    prop_assert_eq!(result.try_unwrap().is_ok(), expected);
                    prop_assert_eq!(Constructor::<M, S>::try_from_psbt(psbt).is_ok(), expected);
                    Ok(())
                }

                // A seed does not change the mode, and only a seeded mode
                // needs a UUID.
                fn modes<M: Modifiable>(psbt: &Psbt, mode: usize, uuid: bool) -> Result<(), TestCaseError> {
                    let flags = psbt.global.tx_modifiable_flags == M::EXPECTED_FLAGS;
                    check::<M, Mixed>(psbt.clone(), flags && mode / 2 == 0)?;
                    check::<M, ExplicitKeysForbidden>(psbt.clone(), flags && mode / 2 == 1)?;
                    check::<M, ExplicitKeysRequired>(psbt.clone(), flags && mode / 2 == 2)?;
                    check::<M, Seeded<Mixed>>(psbt.clone(), flags && uuid && mode / 2 == 0)?;
                    check::<M, Seeded<ExplicitKeysForbidden>>(psbt.clone(), flags && uuid && mode / 2 == 1)?;
                    check::<M, Seeded<ExplicitKeysRequired>>(psbt.clone(), flags && uuid && mode / 2 == 2)
                }

                let mut global = global_with_mode(flags, mode);
                if !uuid {
                    global.uuid = None;
                }
                let psbt = empty_psbt(global);
                modes::<BothModifiable>(&psbt, mode, uuid)?;
                modes::<InputsModifiable>(&psbt, mode, uuid)?;
                modes::<OutputsModifiable>(&psbt, mode, uuid)?;
            }
        }
    }
}
