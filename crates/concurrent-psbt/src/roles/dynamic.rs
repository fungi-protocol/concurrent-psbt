//! Roles chosen at runtime from an unordered PSBT.
//!
//! [`Constructor`] and [`Sorter`] carry modifiability and sort mode as type
//! parameters, so a caller holding a parsed PSBT would have to know them in
//! advance. A [`Role`] reads the sort mode from the PSBT's sort fields
//! and modifiability from its flags. It holds a [`Constructor`] while inputs
//! or outputs may be added and a [`Sorter`] once neither may.
#![allow(clippy::result_large_err)]

use psbt_v2::v2::Psbt;

use crate::global::SortKeyDerivation;
use crate::lattice::join::Join;
use crate::sorter::{
    BothModifiable, ExplicitKeysForbidden, ExplicitKeysRequired, InputsModifiable, Mixed,
    OutputsModifiable, Seeded, Sorter, Unmodifiable, has_uuid,
};
use crate::tx::{ResultUnorderedPsbt, UnorderedPsbt, UnorderedPsbtError};

use super::constructor::Constructor;

/// The typed role for sort mode `S`, chosen by `tx_modifiable_flags & 0x03`.
#[derive(Debug)]
pub enum Stage<S> {
    /// Inputs and outputs may be added (`0x03`).
    BothModifiable(Constructor<BothModifiable, S>),
    /// Only inputs may be added (`0x01`).
    InputsModifiable(Constructor<InputsModifiable, S>),
    /// Only outputs may be added (`0x02`).
    OutputsModifiable(Constructor<OutputsModifiable, S>),
    /// Neither may be added (`0x00`), so all that is left is to sort.
    Unmodifiable(Sorter<Unmodifiable, S>),
}

impl<S> Stage<S> {
    /// Wrap `psbt` in the role its flags select. `psbt`'s sort fields must
    /// select `S`.
    fn from_unordered(psbt: UnorderedPsbt) -> Self {
        match psbt.global.tx_modifiable_flags() & 0x03 {
            0x03 => Self::BothModifiable(Constructor::from_unordered(psbt)),
            0x01 => Self::InputsModifiable(Constructor::from_unordered(psbt)),
            0x02 => Self::OutputsModifiable(Constructor::from_unordered(psbt)),
            _ => Self::Unmodifiable(Sorter::from_unordered(psbt)),
        }
    }

    /// Consume and return the underlying [`UnorderedPsbt`].
    pub fn into_inner(self) -> UnorderedPsbt {
        match self {
            Self::BothModifiable(constructor) => constructor.into_inner(),
            Self::InputsModifiable(constructor) => constructor.into_inner(),
            Self::OutputsModifiable(constructor) => constructor.into_inner(),
            Self::Unmodifiable(sorter) => sorter.into_inner(),
        }
    }
}

/// The typed role of an [`UnorderedPsbt`], with its sort mode and
/// modifiability read from the PSBT.
///
/// The sort mode is [`Mixed`] if `PSBT_GLOBAL_SORT_KEY_DERIVATION` is unset,
/// [`ExplicitKeysForbidden`] if it is `0x01`, and [`ExplicitKeysRequired`] if
/// it is `0x00`. Each mode is [`Seeded`] if `PSBT_GLOBAL_TX_UUID` is set.
#[derive(Debug)]
pub enum Role {
    /// [`Mixed`] sort mode.
    Mixed(Stage<Mixed>),
    /// [`Seeded`] [`Mixed`] sort mode.
    SeededMixed(Stage<Seeded<Mixed>>),
    /// [`ExplicitKeysForbidden`] sort mode.
    ExplicitKeysForbidden(Stage<ExplicitKeysForbidden>),
    /// [`Seeded`] [`ExplicitKeysForbidden`] sort mode.
    SeededExplicitKeysForbidden(Stage<Seeded<ExplicitKeysForbidden>>),
    /// [`ExplicitKeysRequired`] sort mode.
    ExplicitKeysRequired(Stage<ExplicitKeysRequired>),
    /// [`Seeded`] [`ExplicitKeysRequired`] sort mode.
    SeededExplicitKeysRequired(Stage<Seeded<ExplicitKeysRequired>>),
}

impl From<UnorderedPsbt> for Role {
    fn from(psbt: UnorderedPsbt) -> Self {
        let seeded = has_uuid(&psbt.global);
        match psbt.global.sort_key_derivation() {
            None if seeded => Self::SeededMixed(Stage::from_unordered(psbt)),
            None => Self::Mixed(Stage::from_unordered(psbt)),
            Some(SortKeyDerivation::ExplicitKeysForbidden) if seeded => {
                Self::SeededExplicitKeysForbidden(Stage::from_unordered(psbt))
            }
            Some(SortKeyDerivation::ExplicitKeysForbidden) => {
                Self::ExplicitKeysForbidden(Stage::from_unordered(psbt))
            }
            Some(SortKeyDerivation::ExplicitKeysRequired) if seeded => {
                Self::SeededExplicitKeysRequired(Stage::from_unordered(psbt))
            }
            Some(SortKeyDerivation::ExplicitKeysRequired) => {
                Self::ExplicitKeysRequired(Stage::from_unordered(psbt))
            }
        }
    }
}

impl Role {
    /// Parse a v2 PSBT into the [`Role`] selected by its sort fields and
    /// modifiability flags.
    ///
    /// # Errors
    /// Returns the errors of [`UnorderedPsbt::try_from_psbt`].
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        Ok(UnorderedPsbt::try_from_psbt(psbt)?.into())
    }

    /// Consume and return the underlying [`UnorderedPsbt`].
    pub fn into_inner(self) -> UnorderedPsbt {
        match self {
            Self::Mixed(stage) => stage.into_inner(),
            Self::SeededMixed(stage) => stage.into_inner(),
            Self::ExplicitKeysForbidden(stage) => stage.into_inner(),
            Self::SeededExplicitKeysForbidden(stage) => stage.into_inner(),
            Self::ExplicitKeysRequired(stage) => stage.into_inner(),
            Self::SeededExplicitKeysRequired(stage) => stage.into_inner(),
        }
    }

    /// Lift into the result domain as a conflict-free [`ResultRole`].
    pub fn wrap(self) -> ResultRole {
        ResultRole(self.into_inner().wrap())
    }

    /// Join two roles, producing a [`ResultRole`] that may contain conflicts.
    /// The input roles may differ. When the join is conflict-free, the
    /// joined PSBT determines the role returned by [`ResultRole::try_unwrap`].
    pub fn try_join(self, other: Self) -> ResultRole {
        self.wrap().join(other.wrap())
    }
}

/// Result-domain version of [`Role`]. Implements [`Join`] for N-way merging.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultRole(ResultUnorderedPsbt);

impl Join for ResultRole {
    fn join(self, other: Self) -> Self {
        Self(self.0.join(other.0))
    }
}

impl ResultRole {
    /// Parse a v2 PSBT directly into the result domain.
    ///
    /// # Errors
    /// Returns the errors of [`ResultUnorderedPsbt::try_from_psbt`].
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        Ok(Self(ResultUnorderedPsbt::try_from_psbt(psbt)?))
    }

    /// Return `true` if there are no field or membership conflicts, which is
    /// when [`try_unwrap`](Self::try_unwrap) succeeds.
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    /// Return the [`Role`] selected by the joined sort fields and modifiability flags,
    /// or `Err(self)` on a field or membership conflict.
    pub fn try_unwrap(self) -> Result<Role, Self> {
        self.0.try_unwrap().map(Role::from).map_err(Self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use bitcoin::hashes::Hash;

    use super::*;
    use crate::global::{Global, TxUuid};
    use crate::input::{Input, SortKey};
    use crate::sorter::{SortMode, check_sort_mode};

    /// The sort modes, in the order of [`Role`]'s variants.
    const MODES: [&str; 6] = [
        "mixed",
        "seeded mixed",
        "explicit keys forbidden",
        "seeded explicit keys forbidden",
        "explicit keys required",
        "seeded explicit keys required",
    ];

    /// A global map with these flags whose sort fields select `MODES[mode]`.
    fn global(flags: u8, mode: usize) -> Global {
        let mut global = Global::default();
        global.set_tx_modifiable_flags(flags);
        global.set_tx_version(bitcoin::transaction::Version::TWO);
        global.set_unordered();
        match mode {
            2 | 3 => global.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysForbidden),
            4 | 5 => global.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysRequired),
            _ => {}
        }
        if mode % 2 == 1 {
            global.set_uuid(TxUuid::try_from(vec![9; 16]).unwrap());
        }
        global
    }

    /// A PSBT with `global` and one input spending `[tag; 32]:0` with
    /// `sequence`, sort key `[1]` unless sort keys are forbidden.
    fn psbt(global: Global, tag: u8, sequence: u32) -> Psbt {
        let mut input = Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([tag; 32]),
            vout: 0,
        });
        input.set_sequence(Some(bitcoin::Sequence(sequence)));
        if global.sort_key_derivation() != Some(SortKeyDerivation::ExplicitKeysForbidden) {
            input.set_sort_key(SortKey::try_from(vec![1]).expect("one byte"));
        }
        Psbt {
            global: global.into_v2(1, 0),
            inputs: vec![input.into()],
            outputs: vec![],
        }
    }

    /// `flags & 0x03` as the stage records it, after checking that the
    /// PSBT's sort fields select `S`.
    fn stage<S: SortMode>(stage: &Stage<S>) -> u8 {
        let (psbt, flags) = match stage {
            Stage::BothModifiable(constructor) => (&constructor.0, 0x03),
            Stage::InputsModifiable(constructor) => (&constructor.0, 0x01),
            Stage::OutputsModifiable(constructor) => (&constructor.0, 0x02),
            Stage::Unmodifiable(sorter) => (&sorter.0, 0x00),
        };
        assert_eq!(check_sort_mode::<S>(psbt), Ok(()));
        flags
    }

    /// The sort mode and flags that `role` was chosen by.
    fn describe(role: &Role) -> (&'static str, u8) {
        match role {
            Role::Mixed(s) => (MODES[0], stage(s)),
            Role::SeededMixed(s) => (MODES[1], stage(s)),
            Role::ExplicitKeysForbidden(s) => (MODES[2], stage(s)),
            Role::SeededExplicitKeysForbidden(s) => (MODES[3], stage(s)),
            Role::ExplicitKeysRequired(s) => (MODES[4], stage(s)),
            Role::SeededExplicitKeysRequired(s) => (MODES[5], stage(s)),
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn role_import_rejects_an_invalid_uuid() {
            use crate::global::{InvalidTxUuid, PSBT_GLOBAL_TX_UUID_SUBTYPE};

            let mut psbt = psbt(global(0x03, 0), 1, 0);
            let bytes = vec![0; 33];
            psbt.global.proprietaries.insert(
                crate::psbt::proprietary::key(PSBT_GLOBAL_TX_UUID_SUBTYPE),
                bytes.clone(),
            );
            let expected = UnorderedPsbtError::InvalidTxUuid(InvalidTxUuid(bytes));
            assert_eq!(Role::try_from_psbt(psbt.clone()).unwrap_err(), expected);
            assert_eq!(ResultRole::try_from_psbt(psbt).unwrap_err(), expected);
        }

        #[test]
        fn every_psbt_takes_the_role_its_flags_select() {
            for flags in 0..4 {
                for (mode, name) in MODES.into_iter().enumerate() {
                    let psbt = psbt(global(flags, mode), 1, 0);
                    let role = Role::try_from_psbt(psbt.clone()).expect("valid PSBT");
                    assert_eq!(describe(&role), (name, flags));
                    let role = role.wrap().try_unwrap().expect("no conflicts");
                    assert_eq!(describe(&role), (name, flags));
                    assert_eq!(
                        role.into_inner(),
                        UnorderedPsbt::try_from_psbt(psbt).expect("valid PSBT")
                    );
                }
            }
        }

        #[test]
        fn identical_roles_join_without_losing_fields() {
            for flags in 0..4 {
                for (mode, name) in MODES.into_iter().enumerate() {
                    let psbt = psbt(global(flags, mode), 1, 7);
                    let parsed = ResultRole::try_from_psbt(psbt.clone()).expect("valid PSBT");
                    assert!(parsed.is_ok());
                    let left = Role::try_from_psbt(psbt.clone()).expect("valid PSBT");
                    let right = Role::try_from_psbt(psbt.clone()).expect("valid PSBT");
                    let joined = left.try_join(right);
                    assert_eq!(joined, parsed);
                    assert!(joined.is_ok());
                    let role = joined.try_unwrap().expect("identical values join cleanly");
                    assert_eq!(describe(&role), (name, flags));
                    assert_eq!(
                        role.into_inner(),
                        UnorderedPsbt::try_from_psbt(psbt).unwrap()
                    );
                }
            }
        }

        #[test]
        fn conflicting_roles_retain_the_complete_result() {
            use crate::conflicts::ConflictPath;
            use crate::global::GlobalField;
            use crate::input::InputField;

            let outpoint = bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([1; 32]),
                vout: 0,
            };
            for flags in 0..4 {
                for mode in 0..MODES.len() {
                    for (other_flags, sequence, path) in [
                        (
                            flags ^ 1,
                            7,
                            ConflictPath::Global(GlobalField::TxModifiableFlags),
                        ),
                        (
                            flags,
                            8,
                            ConflictPath::Input {
                                outpoint: &outpoint,
                                field: InputField::Sequence,
                            },
                        ),
                    ] {
                        let left = Role::try_from_psbt(psbt(global(flags, mode), 1, 7)).unwrap();
                        let right =
                            Role::try_from_psbt(psbt(global(other_flags, mode), 1, sequence))
                                .unwrap();
                        let joined = left.try_join(right);
                        assert!(!joined.is_ok());
                        assert_eq!(
                            joined
                                .0
                                .conflicts()
                                .map(|conflict| conflict.path())
                                .collect::<Vec<_>>(),
                            [path]
                        );
                        let unchanged = joined.clone();
                        assert_eq!(joined.try_unwrap().unwrap_err(), unchanged);
                    }
                }
            }
        }

        #[test]
        fn parsing_reports_the_unordered_psbt_errors() {
            let mut ordered = global(0x03, 0);
            ordered.set_tx_modifiable_flags(0x07);
            assert_eq!(
                Role::try_from_psbt(psbt(ordered.clone(), 1, 0)).err(),
                Some(UnorderedPsbtError::UnorderedSighashSingle)
            );
            assert_eq!(
                ResultRole::try_from_psbt(psbt(ordered, 1, 0)).err(),
                Some(UnorderedPsbtError::UnorderedSighashSingle)
            );
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use proptest::prelude::*;

        use super::*;

        proptest! {
            #[test]
            fn joined_flags_select_the_role(
                flags in (0u8..4, 0u8..4),
                mode in 0usize..6,
                sequences in (0u32..2, 0u32..2),
            ) {
                let role = |flags, sequence| {
                    Role::try_from_psbt(psbt(global(flags, mode), 1, sequence))
                        .expect("valid PSBT")
                };
                let joined = role(flags.0, sequences.0).try_join(role(flags.1, sequences.1));
                let parsed = ResultRole::try_from_psbt(psbt(global(flags.0, mode), 1, sequences.0))
                    .expect("valid PSBT")
                    .join(
                        ResultRole::try_from_psbt(psbt(global(flags.1, mode), 1, sequences.1))
                            .expect("valid PSBT"),
                    );
                prop_assert_eq!(&joined, &parsed);
                let clean = flags.0 == flags.1 && sequences.0 == sequences.1;
                prop_assert_eq!(joined.is_ok(), clean);
                match joined.try_unwrap() {
                    Ok(role) => prop_assert_eq!(describe(&role), (MODES[mode], flags.0)),
                    Err(joined) => prop_assert!(!clean && !joined.is_ok()),
                }
            }
        }
    }
}
