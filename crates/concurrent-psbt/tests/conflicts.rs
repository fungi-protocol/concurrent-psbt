#![cfg(feature = "unit-tests")]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::fmt::Debug;

use bitcoin::{Amount, OutPoint, ScriptBuf, transaction};
use concurrent_psbt::input::{Input, InputSet};
use concurrent_psbt::output::{Output, OutputSet, UniqueId};
use concurrent_psbt::tx::UnorderedPsbt;
use concurrent_psbt::{Lift, ResultWrapped};

#[cfg_attr(coverage_nightly, coverage(off))]
fn psbt(version: transaction::Version) -> UnorderedPsbt {
    UnorderedPsbt::try_from_psbt(psbt_v2::v2::Psbt {
        global: psbt_v2::v2::Global {
            tx_version: version,
            ..Default::default()
        },
        inputs: vec![],
        outputs: vec![],
    })
    .unwrap()
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
