#![cfg(feature = "prop-tests")]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg_attr(coverage_nightly, coverage(off))]
mod properties {
    use std::collections::{BTreeMap, HashMap};

    use bitcoin::transaction;
    use concurrent_psbt::global::{Global, GlobalField};
    use concurrent_psbt::{Conflicts, Join, Lift, ResultWrapped};
    use proptest::prelude::*;
    use psbt_v2::raw;

    fn convert<T>(global: psbt_v2::v2::Global) -> T
    where
        T: TryFrom<psbt_v2::v2::Global>,
        T::Error: std::fmt::Debug,
    {
        T::try_from(global).unwrap()
    }

    proptest! {
        #[test]
        fn nested_record_conflicts_keep_keys_and_field_paths(
            left_version in 1i32..4, right_version in 1i32..4,
            left_value in any::<u8>(), right_value in any::<u8>(),
            outer_key in any::<u8>(), keydata in proptest::collection::vec(any::<u8>(), 0..8),
        ) {
            let key = raw::Key { type_value: 0x80, key: keydata };
            let global = |version, value| convert::<Global>(psbt_v2::v2::Global {
                tx_version: transaction::Version(version),
                unknowns: BTreeMap::from([(key.clone(), vec![value])]),
                ..Default::default()
            });
            let left = HashMap::from([(outer_key, Some(global(left_version, left_value)))]);
            let right = HashMap::from([(outer_key, Some(global(right_version, right_value)))]);
            let joined = left.wrap().join(right.wrap());
            let mut paths = Vec::new();
            for (found_key, conflict) in Conflicts::conflicts(&joined) {
                prop_assert_eq!(found_key, &outer_key);
                paths.push(conflict.path());
            }
            let mut expected = Vec::new();
            if left_version != right_version { expected.push(GlobalField::TxVersion); }
            if left_value != right_value { expected.push(GlobalField::Unknown(&key)); }
            prop_assert_eq!(&paths, &expected);
            prop_assert_eq!(ResultWrapped::is_ok(&joined), expected.is_empty());
            prop_assert_eq!(ResultWrapped::try_unwrap(joined).is_ok(), expected.is_empty());
        }
    }
}
