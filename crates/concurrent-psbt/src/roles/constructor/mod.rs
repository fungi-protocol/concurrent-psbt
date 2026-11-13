//! The BIP 370 Constructor role: adding inputs and outputs.
//!
//! [`Constructor`] tracks modifiability and sort mode in the type system.
#![allow(clippy::result_large_err)]

use std::marker::PhantomData;

use bitcoin::{Amount, ScriptBuf};
use psbt_v2::v2::{DetermineLockTimeError, Psbt};

use crate::global::SortSeed;
use crate::input::{Input, InputSet};
use crate::lattice::join::Join;
use crate::output::{Output, OutputSet, UniqueId};
use crate::sorter::{
    BothModifiable, DerivedOnly, InputsModifiable, Mixed, Modifiable, OutputsModifiable, Seeded,
    SortMode, Sorter, Unmodifiable, check_flags, check_sort_mode,
};
use crate::tx::UnorderedPsbt;

mod error;
mod result;

pub use error::ConstructorError;
pub use result::ResultConstructor;

/// Typed wrapper that tracks modifiability `M` and sort state `S`.
///
/// The type parameters enforce the BIP 370 constructor state machine at compile time:
/// `M` is one of `BothModifiable`, `InputsModifiable`, or `OutputsModifiable`.
/// `S` is the sort mode chosen when the constructor is created, refined to
/// [`Seeded`] by `set_seed`. Parsed PSBTs
/// enter as [`Mixed`].
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

    fn with_seed<T>(mut self, seed: SortSeed) -> Constructor<M, Seeded<T>> {
        self.0.global.set_sort_seed(seed);
        Constructor::from_unordered(self.0)
    }
}

impl<M> Constructor<M, Mixed> {
    /// Set `PSBT_GLOBAL_SORT_SEED`, replacing any existing seed.
    pub fn set_seed(self, seed: SortSeed) -> Constructor<M, Seeded<Mixed>> {
        self.with_seed(seed)
    }
}

impl<M> Constructor<M, DerivedOnly> {
    /// Set `PSBT_GLOBAL_SORT_SEED`, replacing any existing seed.
    pub fn set_seed(self, seed: SortSeed) -> Constructor<M, Seeded<DerivedOnly>> {
        self.with_seed(seed)
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

    /// Move on to the [`Sorter`] while inputs and outputs stay as modifiable
    /// as they are. Sorting then hands the PSBT to the BIP 370
    /// [`Constructor`](psbt_v2::v2::Constructor) of the same modifiability,
    /// which adds further inputs and outputs after the sorted ones.
    pub fn into_sorter(self) -> Sorter<M, S> {
        Sorter::from_unordered(self.0)
    }

    /// Join with a PSBT that shares this one's global fields and holds only
    /// the given inputs and outputs.
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
        check_sort_mode::<S>(&psbt)?;
        Ok(Constructor::from_unordered(psbt))
    }

    /// Join with a PSBT holding only `input`. Its outputs are empty and
    /// modifiable, so they join cleanly with these whether or not they are
    /// frozen.
    fn add_input(self, input: Input) -> Result<Self, ConstructorError> {
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
    /// Returns [`ConstructorError::Conflict`] if the two disagree, or
    /// [`ConstructorError::SortMode`] if the input has a sort key in
    /// [`DerivedOnly`] mode.
    pub fn input(self, input: Input) -> Result<Self, ConstructorError> {
        self.add_input(input)
    }

    /// Add an output to the PSBT.
    ///
    /// An output whose unique ID is already in the PSBT is joined with it.
    ///
    /// # Errors
    /// Returns [`ConstructorError::Conflict`] if it disagrees with the output
    /// already holding that ID, or
    /// [`ConstructorError::SortMode`] if it has a sort key in
    /// [`DerivedOnly`] mode.
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
    /// Returns [`ConstructorError::Conflict`] if the two disagree, or
    /// [`ConstructorError::SortMode`] if the input has a sort key in
    /// [`DerivedOnly`] mode.
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
    /// already holding that ID, or
    /// [`ConstructorError::SortMode`] if it has a sort key in
    /// [`DerivedOnly`] mode.
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
    use crate::global::{Global, SortKeyDerivation};

    type TestConstructor = Constructor<BothModifiable, Mixed>;

    /// A [`Global`] marked unordered, with these flags and transaction version 2.
    fn unordered_global(flags: u8) -> Global {
        let mut global = Global {
            tx_modifiable_flags: flags,
            tx_version: bitcoin::transaction::Version::TWO,
            ..Global::default()
        };
        global.set_unordered();
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

    /// The global map of an empty PSBT whose sort fields select mixed (0),
    /// seeded mixed (1), derived-only (2), seeded derived-only (3), or
    /// explicit-only (4) mode.
    fn global_with_mode(flags: u8, mode: usize) -> Global {
        let mut global = unordered_global(flags);
        match mode {
            2 | 3 => global.set_sort_key_derivation(SortKeyDerivation::Derived),
            4 => global.set_sort_key_derivation(SortKeyDerivation::Explicit),
            _ => {}
        }
        if mode == 1 || mode == 3 {
            global.set_sort_seed(SortSeed::try_from(vec![7; 16]).expect("16 bytes"));
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
        Output::new(UniqueId::new(vec![tag]), Amount::ZERO, ScriptBuf::new())
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::input::SortKey;
        use crate::sorter::{ExplicitOnly, SortModeError};
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
            assert_eq!(outputs.0.global.tx_modifiable_flags & 0x03, 0x02);
            assert_eq!(
                outputs.0.inputs,
                InputSet::try_from_inputs([test_input(1)]).unwrap().freeze()
            );

            let sorter: Sorter<Unmodifiable, Mixed> = outputs.no_more_outputs();
            let sorted = sorter.into_inner();
            assert_eq!(sorted.global.tx_modifiable_flags & 0x03, 0x00);
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
            assert_eq!(inputs.0.global.tx_modifiable_flags & 0x03, 0x01);
            assert_eq!(
                inputs.0.outputs,
                OutputSet::try_from_outputs([test_output(1)])
                    .unwrap()
                    .freeze()
            );

            let sorter: Sorter<Unmodifiable, Mixed> = inputs.no_more_inputs();
            let sorted = sorter.into_inner();
            assert_eq!(sorted.global.tx_modifiable_flags & 0x03, 0x00);
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
        fn setting_a_seed_lets_the_constructor_be_sorted() {
            let seed = || SortSeed::try_from(vec![7; 16]).expect("16 bytes");

            let mixed: Constructor<BothModifiable, Seeded<Mixed>> =
                empty_constructor().set_seed(seed());
            let ordered = mixed
                .output(test_output(1))
                .unwrap()
                .no_more_inputs()
                .no_more_outputs()
                .try_into_ordered_psbt()
                .expect("a seeded mixed PSBT without sort keys can be sorted");
            assert_eq!(ordered.outputs.len(), 1);

            let derived_only: Constructor<BothModifiable, Seeded<DerivedOnly>> =
                Constructor::<BothModifiable, DerivedOnly>::try_from_psbt(psbt_with_mode(0x03, 2))
                    .unwrap()
                    .set_seed(seed());
            let ordered = derived_only
                .no_more_outputs()
                .input(test_input(1))
                .unwrap()
                .no_more_inputs()
                .into_ordered_psbt();
            assert_eq!(ordered.inputs.len(), 1);
        }

        #[test]
        fn try_from_psbt_reports_why_a_psbt_is_rejected() {
            let mut miscounted = flags_psbt(0x03);
            miscounted.global.input_count = 1;
            let mut explicit_with_seed = global_with_mode(0x03, 1);
            explicit_with_seed.set_sort_key_derivation(SortKeyDerivation::Explicit);

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
                    ConstructorError::DuplicateUniqueId(UniqueId::new(vec![4])),
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
                    "derived-only mode needs derived key derivation",
                    Constructor::<BothModifiable, DerivedOnly>::try_from_psbt(psbt_with_mode(
                        0x03, 0,
                    ))
                    .err(),
                    ConstructorError::SortMode(SortModeError::SortKeyDerivation {
                        expected: Some(SortKeyDerivation::Derived),
                        actual: None,
                    }),
                ),
                (
                    "a seeded mode needs a seed",
                    Constructor::<BothModifiable, Seeded<Mixed>>::try_from_psbt(psbt_with_mode(
                        0x03, 0,
                    ))
                    .err(),
                    ConstructorError::SortMode(SortModeError::MissingSortSeed),
                ),
                (
                    "a seed under explicit keys is a parser error before the sort mode check",
                    Constructor::<BothModifiable, ExplicitOnly>::try_from_psbt(empty_psbt(
                        explicit_with_seed,
                    ))
                    .err(),
                    ConstructorError::Unordered(UnorderedPsbtError::UnexpectedSortSeed),
                ),
            ];
            for (name, got, want) in cases {
                assert_eq!(got, Some(want), "{name}");
            }
        }

        fn txids(psbt: &Psbt) -> Vec<bitcoin::Txid> {
            psbt.inputs
                .iter()
                .map(|input| input.previous_txid)
                .collect()
        }

        #[test]
        fn a_sorted_psbt_is_handed_to_a_bip370_constructor_that_appends_after_it() {
            let constructor = || {
                Constructor::<BothModifiable, Mixed>::try_from_psbt(psbt_with_mode(0x03, 1))
                    .unwrap()
                    .input(test_input(1))
                    .unwrap()
                    .input(test_input(2))
                    .unwrap()
                    .input(test_input(3))
                    .unwrap()
            };
            let mut expected = txids(&constructor().into_sorter().try_into_ordered_psbt().unwrap());
            expected.push(test_input(4).previous_txid);

            let appended = constructor()
                .into_sorter()
                .try_sort()
                .expect("a seeded PSBT can be sorted")
                .input(test_input(4).into())
                .psbt()
                .expect("inputs carry no lock time requirements");
            assert!(
                !Global::try_from(appended.global.clone())
                    .expect("well-formed sort fields")
                    .is_unordered()
            );
            assert_eq!(appended.global.tx_modifiable_flags & 0x03, 0x03);
            assert_eq!(txids(&appended), expected);
        }

        #[test]
        fn a_sorted_psbt_stays_as_modifiable_as_it_was() {
            let inputs =
                Constructor::<InputsModifiable, Mixed>::try_from_psbt(psbt_with_mode(0x01, 1))
                    .unwrap()
                    .input(test_input(1))
                    .unwrap()
                    .into_sorter()
                    .try_sort()
                    .unwrap()
                    .input(test_input(2).into())
                    .psbt()
                    .unwrap();
            assert_eq!(inputs.global.tx_modifiable_flags & 0x03, 0x01);
            assert_eq!(
                txids(&inputs),
                [1, 2].map(|tag| test_input(tag).previous_txid)
            );

            let outputs =
                Constructor::<OutputsModifiable, Mixed>::try_from_psbt(psbt_with_mode(0x02, 1))
                    .unwrap()
                    .output(test_output(1))
                    .unwrap()
                    .into_sorter()
                    .try_sort()
                    .unwrap()
                    .output(test_output(2).into())
                    .psbt()
                    .unwrap();
            assert_eq!(outputs.global.tx_modifiable_flags & 0x03, 0x02);
            assert_eq!(outputs.outputs.len(), 2);
        }

        #[test]
        fn an_explicit_only_psbt_needs_sort_keys_to_be_sorted() {
            let unsorted =
                Constructor::<BothModifiable, ExplicitOnly>::try_from_psbt(psbt_with_mode(0x03, 4))
                    .unwrap()
                    .input(test_input(1))
                    .unwrap()
                    .into_sorter()
                    .try_sort();
            assert!(matches!(
                unsorted,
                Err(crate::sorter::SortError::MissingInputSortKey(missing)) if missing == outpoint(1)
            ));
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
        fn a_sort_key_cannot_be_added_in_derived_only_mode() {
            let derived_only = || {
                Constructor::<BothModifiable, DerivedOnly>::try_from_psbt(psbt_with_mode(0x03, 2))
                    .expect("an empty derived-only PSBT")
            };
            let key = || SortKey::try_from(vec![1]).expect("one byte");
            let mut input = test_input(1);
            input.set_sort_key(key());
            let mut output = test_output(1);
            output.set_sort_key(key());
            let expected = Some(ConstructorError::SortMode(SortModeError::UnexpectedSortKey));
            assert_eq!(derived_only().input(input).err(), expected);
            assert_eq!(derived_only().output(output).err(), expected);
        }

        #[test]
        fn an_input_or_output_joins_the_one_with_its_identity_unless_they_disagree() {
            let mut input = test_input(5);
            input.sequence = Some(bitcoin::Sequence(1));
            let mut output = test_output(5);
            output.amount = Amount::from_sat(1);
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
            other_input.sequence = Some(bitcoin::Sequence(2));
            let mut other_output = output.clone();
            other_output.amount = Amount::from_sat(2);
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
            constructor.0.global.tx_version = bitcoin::transaction::Version(version);
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
            fn set_seed_replaces_the_seed_of_a_mixed_psbt(
                seed in proptest::collection::vec(any::<u8>(), 16..=32),
                mode in 0usize..2,
            ) {
                let sort_seed = SortSeed::try_from(seed.clone()).unwrap();
                let constructor =
                    Constructor::<BothModifiable, Mixed>::try_from_psbt(psbt_with_mode(0x03, mode))
                        .unwrap()
                        .set_seed(sort_seed);
                prop_assert_eq!(constructor.0.global.sort_seed(), Some(seed.as_slice()));
            }

            #[test]
            fn set_seed_replaces_the_seed_of_a_derived_only_psbt(
                seed in proptest::collection::vec(any::<u8>(), 16..=32),
                mode in 2usize..4,
            ) {
                let sort_seed = SortSeed::try_from(seed.clone()).unwrap();
                let constructor = Constructor::<BothModifiable, DerivedOnly>::try_from_psbt(
                    psbt_with_mode(0x03, mode),
                )
                .unwrap()
                .set_seed(sort_seed);
                prop_assert_eq!(constructor.0.global.sort_seed(), Some(seed.as_slice()));
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
                prop_assert_eq!(inner.global.tx_version, bitcoin::transaction::Version(version));

                let psbt = constructor_with_version(version).try_into_psbt().unwrap();
                prop_assert_eq!(psbt.global.tx_version, bitcoin::transaction::Version(version));

                let constructor = TestConstructor::from_unordered(inner);
                prop_assert_eq!(
                    constructor.into_inner().global.tx_version,
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
                    unwrapped.into_inner().global.tx_version,
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
                mode in 0usize..5,
            ) {
                use crate::sorter::ExplicitOnly;

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

                // A seeded PSBT also selects the unseeded mode it refines.
                fn modes<M: Modifiable>(psbt: &Psbt, mode: usize) -> Result<(), TestCaseError> {
                    let flags = psbt.global.tx_modifiable_flags == M::EXPECTED_FLAGS;
                    check::<M, Mixed>(psbt.clone(), flags && mode <= 1)?;
                    check::<M, Seeded<Mixed>>(psbt.clone(), flags && mode == 1)?;
                    check::<M, DerivedOnly>(psbt.clone(), flags && (mode == 2 || mode == 3))?;
                    check::<M, Seeded<DerivedOnly>>(psbt.clone(), flags && mode == 3)?;
                    check::<M, ExplicitOnly>(psbt.clone(), flags && mode == 4)
                }

                let psbt = psbt_with_mode(flags, mode);
                modes::<BothModifiable>(&psbt, mode)?;
                modes::<InputsModifiable>(&psbt, mode)?;
                modes::<OutputsModifiable>(&psbt, mode)?;
            }
        }
    }
}
