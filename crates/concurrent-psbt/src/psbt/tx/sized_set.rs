use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult};
use crate::{global::ResultGlobal, input::ResultInputSet, output::ResultOutputSet};

use super::super::SetLen;
use super::modifiability_bit::{ModifiabilityResult, Modifiable};
use super::tx_modifiability_flags::{TxModifiableFlags, effective_flags};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SizedSet<T, const BIT: u8> {
    modifiability: JoinResult<Modifiable<BIT>>,
    count: JoinResult<usize>,
    set: T,
}

impl<T: SetLen, const BIT: u8> SizedSet<T, BIT> {
    fn from_parts((modifiability, count): GlobalFields<BIT>, set: T) -> Self {
        let actual_len = set.len();
        let count = match count {
            Ok(declared_len) if declared_len != actual_len => {
                Err(Conflict::from_values([declared_len, actual_len]))
            }
            count => count,
        };

        Self {
            modifiability,
            count,
            set,
        }
    }

    fn len(&self) -> usize {
        self.set.len()
    }

    pub(super) fn joined_count(&self) -> JoinResult<usize> {
        if self.modifiability.is_err() {
            Err(Conflict::from_values([self.len()]))
        } else {
            self.count.clone()
        }
    }

    pub(super) fn into_parts(self) -> (JoinResult<usize>, T) {
        let count = self.joined_count();
        (count, self.set)
    }

    pub(super) fn modifiability(&self) -> &JoinResult<Modifiable<BIT>> {
        &self.modifiability
    }
}

impl<T, const BIT: u8> Join for SizedSet<T, BIT>
where
    T: Join + SetLen,
{
    fn join(self, other: Self) -> Self {
        let self_len = self.set.len();
        let other_len = other.set.len();

        let set = self.set.join(other.set);

        let self_changed = self_len != set.len();
        let other_changed = other_len != set.len();

        let self_modifiability = match self.modifiability {
            Ok(modifiable) => modifiable.validate_modification(self_changed),
            conflict @ Err(_) => conflict,
        };
        let other_modifiability = match other.modifiability {
            Ok(modifiable) => modifiable.validate_modification(other_changed),
            conflict @ Err(_) => conflict,
        };

        let modifiability = match (self_modifiability, other_modifiability) {
            (Ok(left), Ok(right)) => Ok(left.join(right)),
            (left, right) => Err(Conflict::from_values([left
                .effective()
                .join(right.effective())])),
        };

        let count = match (self.count, other.count) {
            (Ok(_), Ok(_)) => Ok(set.len()),
            (Err(conflict), Ok(_)) | (Ok(_), Err(conflict)) => Err(conflict),
            (Err(left), Err(right)) => Err(left.join(right)),
        };

        Self {
            modifiability,
            count,
            set,
        }
    }
}

impl<T, const BIT: u8> SizedSet<T, BIT>
where
    T: SizedSetFields<BIT>,
{
    pub(super) fn new(global: &ResultGlobal, set: T) -> Self {
        Self::from_parts(T::get_global_fields(global), set)
    }
}

type GlobalFields<const BIT: u8> = (JoinResult<Modifiable<BIT>>, JoinResult<usize>);

pub(super) trait SizedSetFields<const BIT: u8>: SetLen {
    fn get_global_count_field(global: &ResultGlobal) -> JoinResult<usize>;

    fn get_global_fields(global: &ResultGlobal) -> GlobalFields<BIT> {
        (
            Ok(Modifiable::from_flags(effective_flags(
                &global.tx_modifiable_flags,
            ))),
            Self::get_global_count_field(global),
        )
    }
}

impl SizedSetFields<{ TxModifiableFlags::INPUTS_BIT }> for ResultInputSet {
    fn get_global_count_field(global: &ResultGlobal) -> JoinResult<usize> {
        global.input_count.clone()
    }
}

impl SizedSetFields<{ TxModifiableFlags::OUTPUTS_BIT }> for ResultOutputSet {
    fn get_global_count_field(global: &ResultGlobal) -> JoinResult<usize> {
        global.output_count.clone()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::BTreeSet;

    use super::super::modifiability_bit::InputsModifiable;
    use super::*;

    fn inputs_modifiable(modifiable: bool) -> InputsModifiable {
        InputsModifiable::from_flags(u8::from(modifiable))
    }

    #[derive(Debug, Clone, PartialEq)]
    struct TestSet(BTreeSet<u8>);

    impl SetLen for TestSet {
        fn len(&self) -> usize {
            self.0.len()
        }
    }

    impl Join for TestSet {
        fn join(mut self, other: Self) -> Self {
            self.0.extend(other.0);
            self
        }
    }

    fn set(values: impl IntoIterator<Item = u8>) -> TestSet {
        TestSet(values.into_iter().collect())
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::input::{InputSet, ResultInputSet};
        use crate::output::{OutputSet, ResultOutputSet};

        #[test]
        fn set_len_is_shared_by_clean_and_result_sets() {
            assert_eq!(<InputSet as SetLen>::len(&InputSet::default()), 0);
            assert_eq!(
                <ResultInputSet as SetLen>::len(&ResultInputSet::default()),
                0,
            );
            assert_eq!(<OutputSet as SetLen>::len(&OutputSet::default()), 0);
            assert_eq!(
                <ResultOutputSet as SetLen>::len(&ResultOutputSet::default()),
                0,
            );
        }

        #[test]
        fn constructor_validates_the_declared_count() {
            let sized = SizedSet::from_parts((Ok(inputs_modifiable(true)), Ok(3)), set([]));

            assert_eq!(sized.count, Err(Conflict::from_values([3, 0])));
        }

        #[test]
        fn non_modifiable_operand_rejects_growth() {
            let frozen = SizedSet::from_parts((Ok(inputs_modifiable(false)), Ok(0)), set([]));
            let growing = SizedSet::from_parts((Ok(inputs_modifiable(true)), Ok(1)), set([1]));

            let joined = frozen.join(growing);

            assert_eq!(
                joined.modifiability,
                Err(Conflict::from_values([inputs_modifiable(false)])),
            );
            assert_eq!(joined.joined_count(), Err(Conflict::from_values([1])));
        }

        #[test]
        fn join_preserves_an_existing_count_conflict() {
            let conflict = Err(Conflict::from_values([3, 0]));
            let left =
                SizedSet::from_parts((Ok(inputs_modifiable(true)), conflict.clone()), set([]));
            let right = SizedSet::from_parts((Ok(inputs_modifiable(true)), Ok(1)), set([1]));

            let joined = left.join(right);

            assert_eq!(joined.modifiability, Ok(inputs_modifiable(true)));
            assert_eq!(joined.count, conflict);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use proptest::prelude::*;

        use super::*;

        fn arb_sized_set()
        -> impl Strategy<Value = SizedSet<TestSet, { TxModifiableFlags::INPUTS_BIT }>> {
            (
                proptest::collection::btree_set(any::<u8>(), 0..8),
                proptest::bool::ANY,
            )
                .prop_map(|(values, modifiable)| {
                    let set = TestSet(values);
                    let count = Ok(set.len());
                    SizedSet::from_parts((Ok(inputs_modifiable(modifiable)), count), set)
                })
        }

        proptest! {
            #[test]
            fn join_is_idempotent(a in arb_sized_set()) {
                let joined = a.clone().join(a.clone());
                prop_assert_eq!(joined, a);
            }

            #[test]
            fn join_is_commutative(a in arb_sized_set(), b in arb_sized_set()) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn join_is_associative(
                a in arb_sized_set(),
                b in arb_sized_set(),
                c in arb_sized_set(),
            ) {
                let ab_c = a.clone().join(b.clone()).join(c.clone());
                let a_bc = a.join(b.join(c));

                prop_assert_eq!(ab_c, a_bc);
            }
        }
    }
}
