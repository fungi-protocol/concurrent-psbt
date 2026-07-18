use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum ModifiabilityBit {
    Inputs = 0,
    Outputs = 1,
}

pub(super) type InputsModifiable = Modifiable<{ ModifiabilityBit::Inputs as u8 }>;
pub(super) type OutputsModifiable = Modifiable<{ ModifiabilityBit::Outputs as u8 }>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Modifiable<const BIT: u8>(pub(super) bool);

impl<const BIT: u8> Modifiable<BIT> {
    const FLAG: u8 = 1 << BIT;

    pub(super) fn from_flags(flags: u8) -> Self {
        Self(flags & Self::FLAG != 0)
    }

    pub(super) fn as_flag(self) -> u8 {
        if self.0 { Self::FLAG } else { 0 }
    }

    pub(super) fn validate_modification(self, set_modified: bool) -> JoinResult<Self> {
        if self.0 || !set_modified {
            Ok(self)
        } else {
            Err(Conflict::from_values([self]))
        }
    }
}

impl<const BIT: u8> Join for Modifiable<BIT> {
    fn join(self, other: Self) -> Self {
        Self(self.0 && other.0)
    }
}

impl<const BIT: u8> PartialJoin for Modifiable<BIT> {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        self.join(other).wrap()
    }
}

pub(super) trait ModifiabilityResult<const BIT: u8> {
    fn effective(&self) -> Modifiable<BIT>;
}

impl<const BIT: u8> ModifiabilityResult<BIT> for JoinResult<Modifiable<BIT>> {
    fn effective(&self) -> Modifiable<BIT> {
        match self {
            Ok(modifiable) => *modifiable,
            Err(conflict) => conflict
                .into_iter()
                .copied()
                .fold(Modifiable(true), Join::join),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn integral_bit_positions_match_the_psbt_flags() {
            assert_eq!(ModifiabilityBit::Inputs as u8, 0);
            assert_eq!(ModifiabilityBit::Outputs as u8, 1);
            assert_eq!(Modifiable::<0>::from_flags(0x01), Modifiable(true));
            assert_eq!(Modifiable::<1>::from_flags(0x01), Modifiable(false));
        }

        #[test]
        fn flag_roundtrip_uses_the_const_generic_bit() {
            assert_eq!(Modifiable::<0>(true).as_flag(), 0x01);
            assert_eq!(Modifiable::<1>(true).as_flag(), 0x02);
            assert_eq!(Modifiable::<0>(false).as_flag(), 0x00);
        }

        #[test]
        fn modification_requires_a_modifiable_bit() {
            assert_eq!(
                Modifiable::<0>(false).validate_modification(false),
                Ok(Modifiable(false)),
            );
            assert_eq!(
                Modifiable::<0>(false).validate_modification(true),
                Err(Conflict::from_values([Modifiable(false)])),
            );
            assert_eq!(
                Modifiable::<0>(true).validate_modification(true),
                Ok(Modifiable(true)),
            );
        }

        #[test]
        fn effective_modifiability_joins_conflicting_bits() {
            let conflict = Err(Conflict::from_values([
                Modifiable::<0>(false),
                Modifiable(true),
            ]));

            assert_eq!(conflict.effective(), Modifiable(false));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use proptest::prelude::*;

        use super::*;

        proptest! {
            #[test]
            fn join_is_idempotent(value in any::<bool>()) {
                let value = Modifiable::<0>(value);
                prop_assert_eq!(value.join(value), value);
            }

            #[test]
            fn join_is_commutative(left in any::<bool>(), right in any::<bool>()) {
                let left = Modifiable::<0>(left);
                let right = Modifiable::<0>(right);
                prop_assert_eq!(left.join(right), right.join(left));
            }

            #[test]
            fn join_is_associative(
                first in any::<bool>(),
                second in any::<bool>(),
                third in any::<bool>(),
            ) {
                let first = Modifiable::<0>(first);
                let second = Modifiable::<0>(second);
                let third = Modifiable::<0>(third);
                prop_assert_eq!(first.join(second).join(third), first.join(second.join(third)));
            }
        }
    }
}
