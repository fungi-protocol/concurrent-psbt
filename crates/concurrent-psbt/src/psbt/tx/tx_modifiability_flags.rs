use crate::global::ResultGlobal;
use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult};

use super::modifiability_bit::{
    InputsModifiable, ModifiabilityBit, ModifiabilityResult, OutputsModifiable,
};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TxModifiableFlags(JoinResult<u8>);

pub(super) fn effective_flags(flags: &JoinResult<u8>) -> u8 {
    match flags {
        Ok(flags) => *flags,
        Err(conflict) => conflict
            .into_iter()
            .copied()
            .fold(TxModifiableFlags::BOTH, |joined, flags| joined & flags),
    }
}

impl TxModifiableFlags {
    pub(super) const INPUTS_BIT: u8 = ModifiabilityBit::Inputs as u8;
    pub(super) const OUTPUTS_BIT: u8 = ModifiabilityBit::Outputs as u8;
    pub(super) const INPUTS: u8 = 1 << Self::INPUTS_BIT;
    pub(super) const OUTPUTS: u8 = 1 << Self::OUTPUTS_BIT;
    pub(super) const BOTH: u8 = Self::INPUTS | Self::OUTPUTS;

    pub(super) fn effective_flags(&self) -> u8 {
        effective_flags(&self.0)
    }

    fn from_joined_bits(inputs: InputsModifiable, outputs: OutputsModifiable) -> Self {
        Self(Ok(inputs.as_flag() | outputs.as_flag()))
    }

    fn absorb_existing_conflict(self, joined: JoinResult<u8>) -> JoinResult<u8> {
        match self.0 {
            Ok(_) => joined,
            Err(conflict) => Err(conflict).join(joined),
        }
    }

    pub(super) fn complete_join(
        self,
        other: Self,
        inputs: JoinResult<InputsModifiable>,
        outputs: JoinResult<OutputsModifiable>,
    ) -> JoinResult<u8> {
        let inherited_conflict = self.is_conflicted() || other.is_conflicted();
        let self_effective = self.effective_flags();
        let other_effective = other.effective_flags();
        let modifiability_conflicted = inputs.is_err() || outputs.is_err();
        let joined =
            Self::from_joined_bits(inputs.effective(), outputs.effective()).effective_flags();

        let joined = self.absorb_existing_conflict(Ok(joined));
        let joined = other.absorb_existing_conflict(joined);

        if modifiability_conflicted {
            let conflict = Err(Conflict::from_values([self_effective, other_effective]));
            if inherited_conflict {
                joined.join(conflict)
            } else {
                conflict
            }
        } else {
            joined
        }
    }

    fn is_conflicted(&self) -> bool {
        self.0.is_err()
    }
}

impl Join for TxModifiableFlags {
    fn join(self, other: Self) -> Self {
        Self(Ok(self.effective_flags() & other.effective_flags()))
    }
}

impl From<TxModifiableFlags> for JoinResult<u8> {
    fn from(flags: TxModifiableFlags) -> Self {
        flags.0
    }
}

impl From<&ResultGlobal> for TxModifiableFlags {
    fn from(global: &ResultGlobal) -> Self {
        Self(global.tx_modifiable_flags.clone())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    impl TxModifiableFlags {
        fn conflict_with(self, other: Self) -> JoinResult<u8> {
            Err(Conflict::from_values([
                self.effective_flags(),
                other.effective_flags(),
            ]))
        }

        pub(in crate::psbt::tx) fn equivalent(&self, other: &Self) -> bool {
            self.is_conflicted() == other.is_conflicted()
                && self.effective_flags() == other.effective_flags()
        }

        pub(in crate::psbt::tx) fn canonicalized(self) -> JoinResult<u8> {
            if self.is_conflicted() {
                Err(Conflict::from_values([self.effective_flags()]))
            } else {
                Ok(self.effective_flags())
            }
        }
    }

    fn inputs_modifiable(modifiable: bool) -> InputsModifiable {
        InputsModifiable::from_flags(u8::from(modifiable))
    }

    fn outputs_modifiable(modifiable: bool) -> OutputsModifiable {
        OutputsModifiable::from_flags(u8::from(modifiable) << TxModifiableFlags::OUTPUTS_BIT)
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn effective_flags_reduce_a_formal_conflict() {
            let flags = TxModifiableFlags(Err(Conflict::from_values([0xff, 0xfe])));

            assert_eq!(flags.effective_flags(), 0x02);
        }

        #[test]
        fn effective_flags_preserve_a_validated_clean_value() {
            let flags = TxModifiableFlags(Ok(0xff));

            assert_eq!(flags.effective_flags(), 0xff);
        }

        #[test]
        fn native_join_is_bitwise_and() {
            let joined: JoinResult<u8> = TxModifiableFlags(Ok(0x01))
                .join(TxModifiableFlags(Ok(0x02)))
                .into();

            assert_eq!(joined, Ok(0x00));
        }

        #[test]
        fn completion_preserves_prior_conflict_and_absorbs_the_lub() {
            let left = TxModifiableFlags(Err(Conflict::from_values([0x01, 0x03])));
            let right = TxModifiableFlags(Ok(0x02));

            let flags = left.complete_join(
                right,
                Ok(inputs_modifiable(false)),
                Ok(outputs_modifiable(false)),
            );

            assert_eq!(flags, Err(Conflict::from_values([0x01, 0x03, 0x00])));
        }

        #[test]
        fn failed_join_preserves_operand_flags() {
            let flags = TxModifiableFlags(Ok(0x01)).conflict_with(TxModifiableFlags(Ok(0x02)));

            assert_eq!(flags, Err(Conflict::from_values([0x01, 0x02])));
        }

        #[test]
        fn equivalent_conflicts_remain_structurally_unequal() {
            let operands = TxModifiableFlags(Err(Conflict::from_values([0x00, 0x01])));
            let effective = TxModifiableFlags(Err(Conflict::from_values([0x00])));

            assert_ne!(operands, effective);
            assert!(operands.equivalent(&effective));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use proptest::prelude::*;

        use super::*;

        fn arb_flags() -> impl Strategy<Value = TxModifiableFlags> {
            (
                0u8..=TxModifiableFlags::BOTH,
                0u8..=TxModifiableFlags::BOTH,
                proptest::bool::ANY,
            )
                .prop_map(|(left, right, conflicted)| {
                    if conflicted {
                        TxModifiableFlags(Err(Conflict::from_values([left, right])))
                    } else {
                        TxModifiableFlags(Ok(left))
                    }
                })
        }

        proptest! {
            #[test]
            fn join_is_idempotent(a in arb_flags()) {
                prop_assert_eq!(a.clone().join(a.clone()).effective_flags(), a.effective_flags());
            }

            #[test]
            fn join_is_commutative(a in arb_flags(), b in arb_flags()) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).effective_flags(),
                    b.join(a).effective_flags(),
                );
            }

            #[test]
            fn join_is_associative(a in arb_flags(), b in arb_flags(), c in arb_flags()) {
                let ab_c = a.clone().join(b.clone()).join(c.clone());
                let a_bc = a.join(b.join(c));
                prop_assert_eq!(ab_c.effective_flags(), a_bc.effective_flags());
            }

            #[test]
            fn conflict_completion_preserves_the_effective_join(
                a in arb_flags(),
                b in arb_flags(),
            ) {
                let completed = TxModifiableFlags(a.clone().conflict_with(b.clone()));
                prop_assert!(completed.is_conflicted());
                prop_assert_eq!(completed.effective_flags(), a.join(b).effective_flags());
            }
        }
    }
}
