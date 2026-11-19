#![cfg(any(feature = "unit-tests", feature = "prop-tests"))]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg_attr(coverage_nightly, coverage(off))]
mod cases {
    use std::collections::{HashMap, HashSet};

    use bitcoin::{Amount, ScriptBuf};
    use concurrent_psbt::conflicts::SetConflict;
    use concurrent_psbt::output::{Output, OutputConflict, OutputSet, UniqueId};
    use concurrent_psbt::{Conflicts, Join, Lift, ResultWrapped};

    fn output(id: &UniqueId, sats: u64) -> Output {
        Output::new(id.clone(), Amount::from_sat(sats), ScriptBuf::new())
    }

    // A fixed ID joins field-by-field, while extra IDs test frozen membership
    // independently of the amount disagreement at the first ID.
    fn check_output_set_contract(tag: u8, sats: u64, outer_key: u8) {
        let unique_id = |index| {
            let mut bytes = vec![tag; 16];
            bytes[15] = index;
            UniqueId::try_from(bytes).unwrap()
        };
        let common = unique_id(0);
        let extra = unique_id(1);
        let another = unique_id(2);
        for frozen in [false, true] {
            for field_conflict in [false, true] {
                let other_sats = if field_conflict { sats + 1 } else { sats };
                let left = OutputSet::try_from_outputs([output(&common, sats)]).unwrap();
                let left = if frozen { left.freeze() } else { left };
                let right = OutputSet::try_from_outputs([
                    output(&common, other_sats),
                    output(&extra, sats),
                    output(&another, sats),
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
                                conflict: OutputConflict::Amount { values },
                            } => {
                                field_count += 1;
                                assert_eq!(key, &common);
                                assert_eq!(
                                    values.into_iter().copied().collect::<Vec<_>>(),
                                    [Amount::from_sat(sats), Amount::from_sat(other_sats)]
                                );
                            }
                            other => panic!("unexpected output-set conflict: {other:?}"),
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
    fn output_set_traits_preserve_membership_and_field_conflicts() {
        check_output_set_contract(7, 42, 9);
    }

    #[cfg(feature = "prop-tests")]
    mod properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn output_set_traits_preserve_every_conflict(
                tag in any::<u8>(), sats in 0u64..=2_100_000_000_000_000,
                outer_key in any::<u8>(),
            ) {
                check_output_set_contract(tag, sats, outer_key);
            }
        }
    }
}
