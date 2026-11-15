#![cfg(feature = "unit-tests")]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::collections::HashMap;
use std::fmt::Debug;

use bitcoin::{Amount, OutPoint, ScriptBuf, transaction};
use concurrent_psbt::conflicts::{ConflictPath, PsbtConflict};
use concurrent_psbt::global::{GlobalConflict, GlobalField};
use concurrent_psbt::input::{Input, InputSet};
use concurrent_psbt::output::{Output, OutputSet, UniqueId};
use concurrent_psbt::tx::UnorderedPsbt;
use concurrent_psbt::{Conflicts, Join, Lift, ResultWrapped};

#[cfg_attr(coverage_nightly, coverage(off))]
fn psbt(version: transaction::Version) -> UnorderedPsbt {
    let mut psbt = psbt_v2::v2::Psbt {
        global: psbt_v2::v2::Global {
            tx_version: version,
            ..Default::default()
        },
        inputs: vec![],
        outputs: vec![],
    };
    psbt.global.proprietaries.insert(
        psbt_v2::raw::ProprietaryKey {
            prefix: concurrent_psbt::PROPRIETARY_PREFIX.to_vec(),
            subtype: concurrent_psbt::global::PSBT_GLOBAL_TX_UNORDERED_SUBTYPE,
            key: vec![],
        },
        vec![0x03],
    );
    UnorderedPsbt::try_from_psbt(psbt).unwrap()
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn roundtrip<V: Lift + Clone + PartialEq + Debug>(value: V)
where
    V::Result: Debug,
{
    assert_eq!(value.clone().wrap().try_unwrap().unwrap(), value);
}

#[test]
#[cfg_attr(coverage_nightly, coverage(off))]
fn psbt_types_share_the_wrap_unwrap_contract() {
    let inputs = InputSet::try_from_inputs([Input::new(&OutPoint::null())]).unwrap();
    let outputs = OutputSet::try_from_outputs([Output::new(
        UniqueId::try_from(vec![1; 16]).unwrap(),
        Amount::from_sat(100),
        ScriptBuf::new(),
    )])
    .unwrap();
    roundtrip(inputs);
    roundtrip(outputs);
    roundtrip(psbt(transaction::Version::TWO));
}

#[test]
#[cfg_attr(coverage_nightly, coverage(off))]
fn public_iterators_compose_through_nested_containers() {
    let a = HashMap::from([(7u8, Some(psbt(transaction::Version::ONE)))]);
    let b = HashMap::from([(7u8, Some(psbt(transaction::Version::TWO)))]);
    let joined = a.wrap().join(b.wrap());
    let mut conflicts = joined.conflicts();
    let (key, conflict) = conflicts.next().unwrap();
    assert_eq!(key, &7);
    assert_eq!(
        conflict.path(),
        ConflictPath::Global(GlobalField::TxVersion)
    );
    let PsbtConflict::Global(GlobalConflict::TxVersion { values }) = conflict else {
        panic!("transaction version conflict");
    };
    assert_eq!(
        values.into_iter().copied().collect::<Vec<_>>(),
        [transaction::Version::ONE, transaction::Version::TWO]
    );
    assert!(conflicts.next().is_none());
}

#[test]
#[cfg_attr(coverage_nightly, coverage(off))]
fn proprietary_sort_fields_keep_their_typed_paths() {
    use concurrent_psbt::global::{Global, SortKeyDerivation, SortSeed};
    use concurrent_psbt::input::{InputConflict, InputField, SortKey};
    use concurrent_psbt::output::OutputField;

    let mut global = Global::default();
    global.clear_unordered();
    global.set_sort_seed(SortSeed::try_from(vec![1; 16]).unwrap());
    global.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysRequired);
    let mut other = global.clone();
    other.set_unordered();
    other.set_sort_seed(SortSeed::try_from(vec![2; 16]).unwrap());
    other.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysForbidden);
    let joined = global.wrap().join(other.wrap());
    assert_eq!(
        joined.conflicts().map(|c| c.path()).collect::<Vec<_>>(),
        [
            GlobalField::Unordered,
            GlobalField::SortKeyDerivation,
            GlobalField::SortSeed
        ]
    );

    let outpoint = OutPoint::null();
    let mut input = Input::new(&outpoint);
    input.set_sort_key(SortKey::try_from(vec![1]).unwrap());
    let mut other = input.clone();
    other.set_sort_key(SortKey::try_from(vec![2]).unwrap());
    let joined = input.wrap().join(other.wrap());
    let conflict = joined.conflicts().next().unwrap();
    assert_eq!(conflict.path(), InputField::SortKey);
    let InputConflict::SortKey { values } = conflict else {
        panic!("input sort key conflict");
    };
    assert_eq!(
        values
            .into_iter()
            .map(SortKey::as_bytes)
            .collect::<Vec<_>>(),
        [vec![1], vec![2]]
    );

    let id = UniqueId::try_from(vec![3; 16]).unwrap();
    let mut output = Output::new(id, Amount::from_sat(100), ScriptBuf::new());
    output.set_sort_key(SortKey::try_from(vec![4]).unwrap());
    let mut other = output.clone();
    other.set_sort_key(SortKey::try_from(vec![5]).unwrap());
    let joined = output.wrap().join(other.wrap());
    assert_eq!(
        joined.conflicts().map(|c| c.path()).collect::<Vec<_>>(),
        [OutputField::SortKey]
    );
}
