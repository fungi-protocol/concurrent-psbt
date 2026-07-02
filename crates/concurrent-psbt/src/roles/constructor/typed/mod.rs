//! Compile-time modifiability: the constructor typestate machine.

mod both_modifiable;
mod inputs_modifiable;
mod outputs_modifiable;

use std::marker::PhantomData;

use psbt_v2::v2::Psbt;

use crate::lattice::join::Join;
use crate::sorter::Unset;
use crate::tx::{ResultUnorderedPsbt, UnorderedPsbt, UnorderedPsbtError};

use super::Modifiability;

/// Typed wrapper over [`UnorderedPsbt`] that tracks modifiability `M` and sort state `S`.
///
/// The type parameters enforce the BIP 370 constructor state machine at compile time:
/// `M` is one of `BothModifiable`, `InputsModifiable`, or `OutputsModifiable`.
/// `S` is the sort mode chosen when the constructor is created. Parsed PSBTs
/// enter as [`crate::sorter::Unset`] until a parser validates their sort fields.
#[derive(Debug)]
pub struct Constructor<M, S>(pub(crate) UnorderedPsbt, pub(crate) PhantomData<(M, S)>);

impl<M, S> Constructor<M, S> {
    /// Consume the constructor and return the internal unordered PSBT representation.
    pub fn into_inner(self) -> UnorderedPsbt {
        self.0
    }

    /// Convert to a BIP 370 [`Psbt`].
    pub fn into_psbt(self) -> Psbt {
        self.0.into_psbt()
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
}

/// Result-domain version of [`Constructor`]. Implements [`Join`] for N-way merging.
///
/// Obtain via [`Constructor::wrap`], [`Constructor::try_join`], or
/// [`ResultConstructor::try_from_psbt`]. Use [`is_ok`](Self::is_ok) to check
/// for conflicts and [`try_unwrap`](Self::try_unwrap) to recover a clean [`Constructor`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResultConstructor<M, S>(ResultUnorderedPsbt, PhantomData<(M, S)>);

impl<M> ResultConstructor<M, Unset> {
    /// Parse a v2 PSBT directly into the result domain.
    ///
    /// Duplicate input outpoints and output UIDs produce conflicts. This is
    /// the entry point for N-way joins: parse each PSBT into
    /// `ResultConstructor`, then fold with `Join`.
    ///
    /// Unlike `Constructor::try_from_psbt`, this does **not** validate
    /// `tx_modifiable_flags`. In the result domain, PSBTs with differing
    /// flags are loaded as-is; flag mismatches surface as conflicts
    /// when joined, which is the correct behavior for N-way merges.
    ///
    /// # Errors
    /// Returns the first output missing a `PSBT_OUT_UNIQUE_ID` field.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        Ok(Self(ResultUnorderedPsbt::try_from_psbt(psbt)?, PhantomData))
    }
}

impl<M, S> ResultConstructor<M, S> {
    /// Return `true` if every field is conflict-free.
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    /// Extract a clean [`Constructor`] if no conflicts remain.
    ///
    /// Re-validates that `tx_modifiable_flags` matches the expected value for `M`.
    /// This prevents a typestate soundness hole where `try_from_psbt` (which
    /// skips flag validation) could produce a `Constructor<BothModifiable>` from
    /// a PSBT with `flags=0x00`.
    ///
    /// # Errors
    /// Returns `Err(self)` if any field contains a conflict, or if the resolved
    /// `tx_modifiable_flags` don't match `M::EXPECTED_FLAGS`.
    pub fn try_unwrap(self) -> Result<Constructor<M, S>, Self>
    where
        M: Modifiability,
    {
        match self.0.try_unwrap() {
            Ok(psbt) => {
                if psbt.global.tx_modifiable_flags & 0x03 != M::EXPECTED_FLAGS {
                    // Flags don't match the type parameter. Wrap back and return Err.
                    Err(ResultConstructor(psbt.wrap(), PhantomData))
                } else {
                    Ok(Constructor(psbt, PhantomData))
                }
            }
            Err(result) => Err(ResultConstructor(result, PhantomData)),
        }
    }
}

impl<M, S> Join for ResultConstructor<M, S> {
    fn join(self, other: Self) -> Self {
        ResultConstructor(self.0.join(other.0), PhantomData)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    use crate::roles::constructor::{BothModifiable, InputsModifiable, OutputsModifiable};

    type TestConstructor = Constructor<BothModifiable, crate::sort::Unset>;

    fn empty_constructor() -> TestConstructor {
        Constructor(
            UnorderedPsbt {
                global: psbt_v2::v2::Global {
                    tx_modifiable_flags: 0x03,
                    ..psbt_v2::v2::Global::default()
                },
                inputs: crate::input::InputSet::default(),
                outputs: crate::output::OutputSet::default(),
            },
            PhantomData,
        )
    }

    fn constructor_with_version(version: i32) -> TestConstructor {
        let mut constructor = empty_constructor();
        constructor.0.global.tx_version = bitcoin::transaction::Version(version);
        constructor
    }

    fn psbt_with_flags(flags: u8, version: i32) -> Psbt {
        Psbt {
            global: psbt_v2::v2::Global {
                tx_modifiable_flags: flags,
                tx_version: bitcoin::transaction::Version(version),
                ..psbt_v2::v2::Global::default()
            },
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn test_input(tag: u8) -> psbt_v2::v2::Input {
        use bitcoin::hashes::Hash;

        psbt_v2::v2::Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([tag; 32]),
            vout: u32::from(tag),
        })
    }

    fn test_output(tag: u8) -> psbt_v2::v2::Output {
        use crate::output::{OutputUniqueIdExt, UniqueId};

        let mut output = psbt_v2::v2::Output::default();
        output.set_unique_id(UniqueId::new(vec![tag]));
        output
    }

    fn exercise_modifiability_transitions(version: i32) {
        let both =
            Constructor::<BothModifiable, Unset>::try_from_psbt(psbt_with_flags(0x03, version))
                .expect("both-modifiable flags are valid")
                .input(test_input(1))
                .output(test_output(1));
        assert_eq!(both.0.inputs.len(), 1);
        assert_eq!(both.0.outputs.len(), 1);

        let outputs = both.no_more_inputs().output(test_output(2));
        assert_eq!(outputs.0.global.tx_modifiable_flags & 0x03, 0x02);
        assert_eq!(outputs.no_more_outputs().into_inner().outputs.len(), 2);

        let inputs =
            Constructor::<BothModifiable, Unset>::try_from_psbt(psbt_with_flags(0x03, version))
                .expect("both-modifiable flags are valid")
                .no_more_outputs()
                .input(test_input(2));
        assert_eq!(inputs.0.global.tx_modifiable_flags & 0x03, 0x01);
        assert_eq!(inputs.no_more_inputs().into_inner().inputs.len(), 1);

        let inputs =
            Constructor::<InputsModifiable, Unset>::try_from_psbt(psbt_with_flags(0x01, version))
                .expect("inputs-modifiable flags are valid")
                .input(test_input(3));
        assert_eq!(inputs.no_more_inputs().into_inner().inputs.len(), 1);

        let outputs =
            Constructor::<OutputsModifiable, Unset>::try_from_psbt(psbt_with_flags(0x02, version))
                .expect("outputs-modifiable flags are valid")
                .output(test_output(3));
        assert_eq!(outputs.no_more_outputs().into_inner().outputs.len(), 1);
    }

    fn exercise_constructor_errors(version: i32) {
        use crate::roles::constructor::ConstructorError;

        let wrong_typestate = ResultConstructor::<BothModifiable, Unset>::try_from_psbt(
            psbt_with_flags(0x00, version),
        )
        .expect("the result domain accepts flags before typestate validation");
        assert!(wrong_typestate.try_unwrap().is_err());

        let both_error =
            Constructor::<BothModifiable, Unset>::try_from_psbt(psbt_with_flags(0x01, version))
                .expect_err("both-modifiable rejects inputs-only flags");
        assert!(matches!(
            both_error,
            ConstructorError::FlagsMismatch {
                expected: 0x03,
                actual: 0x01
            }
        ));
        assert!(both_error.to_string().contains("expected 0x03"));

        assert!(matches!(
            Constructor::<InputsModifiable, Unset>::try_from_psbt(psbt_with_flags(0x02, version)),
            Err(ConstructorError::FlagsMismatch {
                expected: 0x01,
                actual: 0x02
            })
        ));
        assert!(matches!(
            Constructor::<OutputsModifiable, Unset>::try_from_psbt(psbt_with_flags(0x03, version)),
            Err(ConstructorError::FlagsMismatch {
                expected: 0x02,
                actual: 0x03
            })
        ));

        for error in [
            Constructor::<BothModifiable, Unset>::try_from_psbt({
                let mut psbt = psbt_with_flags(0x03, version);
                psbt.outputs.push(psbt_v2::v2::Output::default());
                psbt
            })
            .expect_err("output identity is required"),
            Constructor::<InputsModifiable, Unset>::try_from_psbt({
                let mut psbt = psbt_with_flags(0x01, version);
                psbt.outputs.push(psbt_v2::v2::Output::default());
                psbt
            })
            .expect_err("output identity is required"),
            Constructor::<OutputsModifiable, Unset>::try_from_psbt({
                let mut psbt = psbt_with_flags(0x02, version);
                psbt.outputs.push(psbt_v2::v2::Output::default());
                psbt
            })
            .expect_err("output identity is required"),
        ] {
            assert!(matches!(error, ConstructorError::MissingUniqueId(_)));
            assert!(error.to_string().contains("PSBT_OUT_UNIQUE_ID"));
        }

        let mut duplicate_inputs = psbt_with_flags(0x03, version);
        duplicate_inputs.inputs = vec![test_input(4), test_input(4)];
        let conflict = Constructor::<BothModifiable, Unset>::try_from_psbt(duplicate_inputs)
            .expect_err("duplicate outpoints conflict");
        assert!(matches!(conflict, ConstructorError::Conflict(_)));
        assert!(conflict.to_string().contains("conflicting fields"));

        let converted = ConstructorError::from(psbt_v2::v2::Output::default());
        assert!(matches!(converted, ConstructorError::MissingUniqueId(_)));
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn wrap_is_ok() {
            assert!(empty_constructor().wrap().is_ok());
        }

        #[test]
        fn accessors_return_the_wrapped_psbt() {
            let inner = constructor_with_version(7).into_inner();
            assert_eq!(inner.global.tx_version, bitcoin::transaction::Version(7));

            let psbt = constructor_with_version(8).into_psbt();
            assert_eq!(psbt.global.tx_version, bitcoin::transaction::Version(8));

            let constructor = TestConstructor::from_unordered(inner);
            assert_eq!(
                constructor.into_inner().global.tx_version,
                bitcoin::transaction::Version(7)
            );
        }

        #[test]
        fn wrap_try_unwrap_roundtrip() {
            assert!(empty_constructor().wrap().try_unwrap().is_ok());
        }

        #[test]
        fn join_identical_is_ok() {
            let a = empty_constructor().wrap();
            let b = empty_constructor().wrap();
            let joined = a.join(b);
            assert!(joined.is_ok());
        }

        #[test]
        fn try_join_identical_is_ok() {
            let joined = empty_constructor().try_join(empty_constructor());
            assert!(joined.is_ok());
            assert!(joined.try_unwrap().is_ok());
        }

        #[test]
        fn result_constructor_parses_empty_psbt() {
            let parsed = ResultConstructor::<BothModifiable, Unset>::try_from_psbt(
                empty_constructor().into_psbt(),
            )
            .expect("an empty PSBT has no duplicate identities");
            assert!(parsed.is_ok());
            assert!(parsed.try_unwrap().is_ok());
        }

        #[test]
        fn result_constructor_rejects_output_without_unique_id() {
            let mut psbt = empty_constructor().into_psbt();
            psbt.outputs.push(psbt_v2::v2::Output::default());
            assert!(ResultConstructor::<BothModifiable, Unset>::try_from_psbt(psbt).is_err());
        }

        #[test]
        fn conflicting_is_not_ok() {
            use bitcoin::transaction;
            let mut a = empty_constructor();
            a.0.global.tx_version = transaction::Version::ONE;
            let mut b = empty_constructor();
            b.0.global.tx_version = transaction::Version::TWO;
            let joined = a.wrap().join(b.wrap());
            assert!(!joined.is_ok());
            assert!(joined.try_unwrap().is_err());
        }

        #[test]
        fn try_unwrap_rejects_wrong_flags() {
            // A PSBT with flags=0x00 loaded into ResultConstructor<BothModifiable>
            // must fail try_unwrap even though all fields are Ok.
            use crate::roles::constructor::ResultConstructor;
            let psbt = psbt_v2::v2::Psbt {
                global: psbt_v2::v2::Global {
                    tx_modifiable_flags: 0x00,
                    ..psbt_v2::v2::Global::default()
                },
                inputs: vec![],
                outputs: vec![],
            };
            let rc = ResultConstructor::<BothModifiable, crate::sort::Unset>::try_from_psbt(psbt);
            assert!(
                rc.is_ok(),
                "try_from_psbt should succeed (no flag validation)"
            );
            let rc = rc.unwrap();
            assert!(rc.is_ok(), "all fields should be conflict-free");
            assert!(
                rc.try_unwrap().is_err(),
                "try_unwrap must reject: flags=0x00 but type says BothModifiable"
            );
        }

        #[test]
        fn modifiability_state_machine() {
            exercise_modifiability_transitions(7);
        }

        #[test]
        fn constructor_error_paths() {
            exercise_constructor_errors(7);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn accessors_preserve_version(version in any::<i32>()) {
                let inner = constructor_with_version(version).into_inner();
                prop_assert_eq!(inner.global.tx_version, bitcoin::transaction::Version(version));

                let psbt = constructor_with_version(version).into_psbt();
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
                let psbt = constructor.into_psbt();
                let parsed = ResultConstructor::<BothModifiable, Unset>::try_from_psbt(psbt)
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
            fn output_without_unique_id_is_rejected(version in any::<i32>()) {
                let mut psbt = constructor_with_version(version).into_psbt();
                psbt.outputs.push(psbt_v2::v2::Output::default());
                prop_assert!(ResultConstructor::<BothModifiable, Unset>::try_from_psbt(psbt).is_err());
            }

            #[test]
            fn modifiability_state_machine(version in any::<i32>()) {
                exercise_modifiability_transitions(version);
            }

            #[test]
            fn constructor_error_paths(version in any::<i32>()) {
                exercise_constructor_errors(version);
            }
        }
    }
}
