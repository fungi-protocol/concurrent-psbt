//! Compile-time modifiability: the constructor typestate machine.

use std::marker::PhantomData;

use psbt_v2::v2::Psbt;

use crate::lattice::join::Join;
use crate::sorter::Unset;
use crate::tx::{ResultUnorderedPsbt, UnorderedPsbt, UnorderedPsbtError};

/// Typed wrapper that tracks modifiability `M` and sort state `S`.
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

    #[allow(dead_code)]
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
    /// # Errors
    /// Returns `Err(self)` if any field contains a conflict.
    pub fn try_unwrap(self) -> Result<Constructor<M, S>, Self> {
        match self.0.try_unwrap() {
            Ok(psbt) => Ok(Constructor(psbt, PhantomData)),
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

    type TestConstructor = Constructor<(), Unset>;

    fn empty_constructor() -> TestConstructor {
        Constructor(
            UnorderedPsbt {
                global: psbt_v2::v2::Global::default(),
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
            let parsed = ResultConstructor::<(), Unset>::try_from_psbt(
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
            assert!(ResultConstructor::<(), Unset>::try_from_psbt(psbt).is_err());
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
                let parsed = ResultConstructor::<(), Unset>::try_from_psbt(psbt)
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
                prop_assert!(ResultConstructor::<(), Unset>::try_from_psbt(psbt).is_err());
            }
        }
    }
}
