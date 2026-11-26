#![cfg(any(feature = "unit-tests", feature = "prop-tests"))]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg_attr(coverage_nightly, coverage(off))]
mod cases {
    use std::collections::{HashMap, HashSet};

    use bitcoin::hashes::Hash;
    use bitcoin::{OutPoint, Sequence, Txid};
    use concurrent_psbt::conflicts::SetConflict;
    use concurrent_psbt::input::{Input, InputConflict, InputSet};
    use concurrent_psbt::{Conflicts, Join, Lift, ResultWrapped};

    fn input(outpoint: &OutPoint, sequence: u32) -> Input {
        let mut input = Input::new(outpoint);
        input.set_sequence(Some(Sequence(sequence)));
        input
    }

    // Exercise clean joins, field conflicts, membership conflicts, and both
    // together. Each case also runs through the public generic trait surface.
    fn check_input_set_contract(tag: u8, sequence: u32, outer_key: u8) {
        let common = OutPoint {
            txid: Txid::from_byte_array([tag; 32]),
            vout: 0,
        };
        let extra = OutPoint { vout: 1, ..common };
        let another = OutPoint { vout: 2, ..common };
        for frozen in [false, true] {
            for field_conflict in [false, true] {
                let other_sequence = if field_conflict {
                    sequence.wrapping_add(1)
                } else {
                    sequence
                };
                let left = InputSet::try_from_inputs([input(&common, sequence)]).unwrap();
                let left = if frozen { left.freeze() } else { left };
                let right = InputSet::try_from_inputs([
                    input(&common, other_sequence),
                    input(&extra, sequence),
                    input(&another, sequence),
                ])
                .unwrap();
                let joined = Lift::wrap(left.clone()).join(Lift::wrap(right.clone()));
                let expected_ok = !frozen && !field_conflict;
                assert_eq!(ResultWrapped::is_ok(&joined), expected_ok);
                let expected = if expected_ok {
                    Ok(right.clone())
                } else {
                    Err(joined.clone())
                };
                assert_eq!(ResultWrapped::try_unwrap(joined.clone()), expected);

                let nested = HashMap::from([(outer_key, Some(left))])
                    .wrap()
                    .join(HashMap::from([(outer_key, Some(right.clone()))]).wrap());
                let direct_conflicts = Conflicts::conflicts(&joined).collect::<Vec<_>>();
                let nested_conflicts = Conflicts::conflicts(&nested)
                    .map(|(key, conflict)| {
                        assert_eq!(key, &outer_key);
                        conflict
                    })
                    .collect::<Vec<_>>();
                for conflicts in [direct_conflicts, nested_conflicts] {
                    let mut membership_count = 0;
                    let mut field_count = 0;
                    for conflict in conflicts {
                        match conflict {
                            SetConflict::Membership(evidence) => {
                                membership_count += 1;
                                assert_eq!(evidence.len(), 2);
                                assert!(evidence.contains(&HashSet::from([&common])));
                                assert!(
                                    evidence.contains(&HashSet::from([&common, &extra, &another]))
                                );
                            }
                            SetConflict::Entry {
                                key,
                                conflict: InputConflict::Sequence { values },
                            } => {
                                field_count += 1;
                                assert_eq!(key, &common);
                                assert_eq!(
                                    values.into_iter().copied().collect::<Vec<_>>(),
                                    [Sequence(sequence), Sequence(other_sequence)]
                                );
                            }
                            other => panic!("unexpected input-set conflict: {other:?}"),
                        }
                    }
                    assert_eq!(membership_count, usize::from(frozen));
                    assert_eq!(field_count, usize::from(field_conflict));
                }

                assert_eq!(ResultWrapped::is_ok(&nested), expected_ok);
                let expected = if expected_ok {
                    Ok(HashMap::from([(outer_key, Some(right))]))
                } else {
                    Err(nested.clone())
                };
                assert_eq!(ResultWrapped::try_unwrap(nested), expected);
            }
        }
    }

    #[cfg(feature = "unit-tests")]
    #[test]
    fn input_set_traits_preserve_membership_and_field_conflicts() {
        check_input_set_contract(7, 42, 9);
    }

    #[cfg(feature = "prop-tests")]
    mod properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn input_set_traits_preserve_every_conflict(
                tag in any::<u8>(), sequence in any::<u32>(), outer_key in any::<u8>(),
            ) {
                check_input_set_contract(tag, sequence, outer_key);
            }
        }
    }
}
