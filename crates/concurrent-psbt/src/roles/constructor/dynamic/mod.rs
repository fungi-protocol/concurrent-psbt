//! Runtime modifiability: dispatch on `tx_modifiable_flags`.

use psbt_v2::v2::Psbt;

use crate::tx::UnorderedPsbtError;

use super::typed;
use super::{BothModifiable, ConstructorError, InputsModifiable, OutputsModifiable};

/// A [`typed::Constructor`] whose modifiability is determined at runtime from
/// `tx_modifiable_flags`.
///
/// Use [`Constructor::try_from_psbt`] to parse a PSBT and
/// recover the modifiability typestate dynamically.
#[derive(Debug)]
pub enum Constructor {
    Both(typed::Constructor<BothModifiable, crate::sort::Unset>),
    InputsOnly(typed::Constructor<InputsModifiable, crate::sort::Unset>),
    OutputsOnly(typed::Constructor<OutputsModifiable, crate::sort::Unset>),
}

impl Constructor {
    /// Parse a v2 PSBT, dispatching on `tx_modifiable_flags & 0x03`.
    ///
    /// # Errors
    /// Returns [`ConstructorError::NotModifiable`] if neither bit 0 nor bit 1
    /// is set, or [`ConstructorError::MissingUniqueId`] for the first output
    /// missing a `PSBT_OUT_UNIQUE_ID` field.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, ConstructorError> {
        let flags = psbt.global.tx_modifiable_flags & 0x03;
        match flags {
            0x03 => Ok(Self::Both(
                typed::Constructor::<BothModifiable, _>::try_from_psbt(psbt)?,
            )),
            0x01 => Ok(Self::InputsOnly(
                typed::Constructor::<InputsModifiable, _>::try_from_psbt(psbt)?,
            )),
            0x02 => Ok(Self::OutputsOnly(
                typed::Constructor::<OutputsModifiable, _>::try_from_psbt(psbt)?,
            )),
            _ => Err(ConstructorError::NotModifiable(flags)),
        }
    }

    /// Consume and return the underlying [`UnorderedPsbt`](crate::tx::UnorderedPsbt).
    pub fn into_inner(self) -> crate::tx::UnorderedPsbt {
        match self {
            Self::Both(c) => c.into_inner(),
            Self::InputsOnly(c) => c.into_inner(),
            Self::OutputsOnly(c) => c.into_inner(),
        }
    }

    /// Convert to a BIP 370 [`Psbt`].
    pub fn into_psbt(self) -> Psbt {
        match self {
            Self::Both(c) => c.into_psbt(),
            Self::InputsOnly(c) => c.into_psbt(),
            Self::OutputsOnly(c) => c.into_psbt(),
        }
    }
}

/// Result-domain counterpart of [`Constructor`].
///
/// Wraps [`ResultUnorderedPsbt`](crate::tx::ResultUnorderedPsbt) directly,
/// erasing the modifiability type parameter. Implements [`Join`](crate::Join):
/// if two PSBTs have different `tx_modifiable_flags`, the join produces a
/// [`Conflict`](crate::Conflict) in that global field, just like any other
/// field-level disagreement.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultConstructor(crate::tx::ResultUnorderedPsbt);

impl crate::Join for ResultConstructor {
    fn join(self, other: Self) -> Self {
        Self(self.0.join(other.0))
    }
}

impl ResultConstructor {
    /// Lift a [`Constructor`] into the result domain.
    pub fn wrap(ctor: Constructor) -> Self {
        match ctor {
            Constructor::Both(c) => Self(c.into_inner().wrap()),
            Constructor::InputsOnly(c) => Self(c.into_inner().wrap()),
            Constructor::OutputsOnly(c) => Self(c.into_inner().wrap()),
        }
    }

    /// Parse a v2 PSBT directly into the result domain.
    ///
    /// # Errors
    /// Returns the first output missing a `PSBT_OUT_UNIQUE_ID` field or any
    /// accumulated unordered PSBT conflict.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        Ok(Self(crate::tx::ResultUnorderedPsbt::try_from_psbt(psbt)?))
    }

    /// Return `true` if every field is conflict-free.
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    /// Visit each conflicted field across global, inputs, and outputs.
    ///
    /// The callback receives `(section, field_name, &dyn Debug)` where
    /// section is `"global"`, `"input:<outpoint>"`, or `"output:<uid>"`.
    pub fn for_each_conflict(&self, mut f: impl FnMut(&str, &str, &dyn std::fmt::Debug)) {
        // Global conflicts
        self.0.global.for_each_conflict(|field, conflict| {
            f("global", field, conflict);
        });

        // Input conflicts: iterate the result map
        for (outpoint, result_input) in &self.0.inputs.0 {
            result_input.for_each_conflict(|field, conflict| {
                f(&format!("input:{outpoint}"), field, conflict);
            });
        }

        // Output conflicts: iterate the result map
        for (uid, result_output) in &self.0.outputs.0 {
            result_output.for_each_conflict(|field, conflict| {
                let uid_hex: String = uid.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
                f(&format!("output:{uid_hex}"), field, conflict);
            });
        }
    }

    /// Extract a clean [`Constructor`] if no conflicts remain.
    ///
    /// # Errors
    /// Returns `Err(self)` if any field contains a conflict (including
    /// a `tx_modifiable_flags` mismatch from joining PSBTs with different
    /// modifiability).
    pub fn try_unwrap(self) -> Result<Constructor, Self> {
        match self.0.try_unwrap() {
            Ok(psbt) => {
                let flags = psbt.global.tx_modifiable_flags & 0x03;
                match flags {
                    0x03 => Ok(Constructor::Both(typed::Constructor::from_unordered(psbt))),
                    0x01 => Ok(Constructor::InputsOnly(typed::Constructor::from_unordered(
                        psbt,
                    ))),
                    0x02 => Ok(Constructor::OutputsOnly(
                        typed::Constructor::from_unordered(psbt),
                    )),
                    // flags 0x00 means neither is modifiable; still a valid
                    // state after joining (e.g. both sides cleared flags).
                    // Default to Both since construction is complete.
                    _ => Ok(Constructor::Both(typed::Constructor::from_unordered(psbt))),
                }
            }
            Err(result) => Err(Self(result)),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    use crate::Join;
    use crate::output::{OutputUniqueIdExt, UniqueId};

    fn psbt(flags: u8, version: i32) -> Psbt {
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

    fn input(sequence: u32) -> psbt_v2::v2::Input {
        use bitcoin::hashes::Hash;

        let mut input = psbt_v2::v2::Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([1; 32]),
            vout: 0,
        });
        input.sequence = Some(bitcoin::Sequence(sequence));
        input
    }

    fn output(amount: u64) -> psbt_v2::v2::Output {
        let mut output = psbt_v2::v2::Output {
            amount: bitcoin::Amount::from_sat(amount),
            ..psbt_v2::v2::Output::default()
        };
        output.set_unique_id(UniqueId::new(vec![1]));
        output
    }

    fn assert_variant(constructor: Constructor, flags: u8) {
        match (constructor, flags) {
            (Constructor::Both(_), 0x03 | 0x00)
            | (Constructor::InputsOnly(_), 0x01)
            | (Constructor::OutputsOnly(_), 0x02) => {}
            (constructor, flags) => panic!("unexpected constructor {constructor:?} for {flags:#04x}"),
        }
    }

    fn exercise_dispatch(version: i32) {
        for flags in [0x03, 0x01, 0x02] {
            assert_variant(
                Constructor::try_from_psbt(psbt(flags, version)).expect("known flags dispatch"),
                flags,
            );
            assert_eq!(
                Constructor::try_from_psbt(psbt(flags, version))
                    .expect("known flags dispatch")
                    .into_inner()
                    .global
                    .tx_modifiable_flags
                    & 0x03,
                flags
            );
            assert_eq!(
                Constructor::try_from_psbt(psbt(flags, version))
                    .expect("known flags dispatch")
                    .into_psbt()
                    .global
                    .tx_modifiable_flags
                    & 0x03,
                flags
            );

            let wrapped = ResultConstructor::wrap(
                Constructor::try_from_psbt(psbt(flags, version)).expect("known flags dispatch"),
            );
            assert!(wrapped.is_ok());
            assert_variant(wrapped.try_unwrap().expect("wrapped constructors are clean"), flags);

            let mut missing_id = psbt(flags, version);
            missing_id.outputs.push(psbt_v2::v2::Output::default());
            assert!(matches!(
                Constructor::try_from_psbt(missing_id),
                Err(ConstructorError::MissingUniqueId(_))
            ));
        }

        let not_modifiable = Constructor::try_from_psbt(psbt(0x00, version))
            .expect_err("zero flags do not describe a modifiable constructor");
        assert!(matches!(not_modifiable, ConstructorError::NotModifiable(0)));
        assert!(not_modifiable.to_string().contains("not modifiable"));

        let no_flags = ResultConstructor::try_from_psbt(psbt(0x00, version))
            .expect("the result domain accepts completed construction");
        assert_variant(no_flags.try_unwrap().expect("clean result unwraps"), 0x00);

        let clean = ResultConstructor::try_from_psbt(psbt(0x03, version))
            .expect("empty PSBT is valid");
        assert!(clean.is_ok());

        let mut missing_id = psbt(0x03, version);
        missing_id.outputs.push(psbt_v2::v2::Output::default());
        assert!(ResultConstructor::try_from_psbt(missing_id).is_err());
    }

    fn exercise_conflicts(version: i32) {
        let mut left = psbt(0x03, version);
        left.inputs.push(input(1));
        left.outputs.push(output(1));

        let mut right = psbt(0x03, version.wrapping_add(1));
        right.inputs.push(input(2));
        right.outputs.push(output(2));

        let joined = ResultConstructor::try_from_psbt(left)
            .expect("identified fields parse")
            .join(ResultConstructor::try_from_psbt(right).expect("identified fields parse"));
        assert!(!joined.is_ok());

        let mut sections = Vec::new();
        joined.for_each_conflict(|section, field, _| {
            sections.push((section.to_owned(), field.to_owned()));
        });
        assert!(sections.iter().any(|(section, _)| section == "global"));
        assert!(sections.iter().any(|(section, _)| section.starts_with("input:")));
        assert!(sections.iter().any(|(section, _)| section.starts_with("output:")));
        assert!(joined.try_unwrap().is_err());
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn runtime_dispatch_contract() {
            exercise_dispatch(7);
        }

        #[test]
        fn conflict_visitation_contract() {
            exercise_conflicts(7);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn runtime_dispatch_contract(version in any::<i32>()) {
                exercise_dispatch(version);
            }

            #[test]
            fn conflict_visitation_contract(version in any::<i32>()) {
                exercise_conflicts(version);
            }
        }
    }
}
