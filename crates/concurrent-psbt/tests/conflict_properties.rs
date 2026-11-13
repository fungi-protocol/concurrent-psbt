#![cfg(feature = "prop-tests")]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg_attr(coverage_nightly, coverage(off))]
mod properties {
    use std::fmt::Debug;

    use bitcoin::hashes::Hash;
    use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Txid, transaction};
    use concurrent_psbt::conflicts::ConflictPath;
    use concurrent_psbt::input::{Input, InputSet};
    use concurrent_psbt::output::{Output, OutputSet, UniqueId};
    use concurrent_psbt::tx::UnorderedPsbt;
    use concurrent_psbt::{Conflicts, Join, Lift, ResultWrapped};
    use proptest::prelude::*;

    fn roundtrip<T>(value: T)
    where
        T: Lift + Clone + PartialEq + Debug,
        T::Result: Conflicts + Debug,
    {
        let wrapped = Lift::wrap(value.clone());
        assert!(ResultWrapped::is_ok(&wrapped));
        assert!(Conflicts::conflicts(&wrapped).next().is_none());
        assert_eq!(ResultWrapped::try_unwrap(wrapped).unwrap(), value);
    }

    fn psbt(mask: u8, value: u8, flags: u8) -> UnorderedPsbt {
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for index in 0..3 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let outpoint = OutPoint {
                txid: Txid::from_byte_array([index; 32]),
                vout: 0,
            };
            let mut input = Input::new(&outpoint);
            input.set_sequence(Some(Sequence(u32::from(value))));
            roundtrip(input.clone());
            inputs.push(input);
            let output = Output::new(
                UniqueId::try_from(vec![index; 16]).unwrap(),
                Amount::from_sat(u64::from(value)),
                ScriptBuf::new(),
            );
            roundtrip(output.clone());
            outputs.push(output);
        }
        let input_set = InputSet::try_from_inputs(inputs.clone()).unwrap();
        assert_eq!(input_set.is_empty(), mask == 0);
        for index in 0..3 {
            assert_eq!(
                input_set.spends_outpoint(&OutPoint {
                    txid: Txid::from_byte_array([index; 32]),
                    vout: 0
                }),
                mask & (1 << index) != 0
            );
        }
        roundtrip(input_set);
        roundtrip(OutputSet::try_from_outputs(outputs.clone()).unwrap());
        let mut global = psbt_v2::v2::Global {
            tx_version: transaction::Version(i32::from(value) + 1),
            tx_modifiable_flags: flags,
            input_count: inputs.len(),
            output_count: outputs.len(),
            ..Default::default()
        };
        global.proprietaries.insert(
            psbt_v2::raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: 0x10,
                key: vec![],
            },
            vec![0x03],
        );
        UnorderedPsbt::try_from_psbt(psbt_v2::v2::Psbt {
            global,
            inputs: inputs.into_iter().map(Into::into).collect(),
            outputs: outputs.into_iter().map(Into::into).collect(),
        })
        .unwrap()
    }

    proptest! {
        #[test]
        fn wrapped_psbts_keep_paths_and_generic_roundtrips(
            a_mask in 0u8..8, b_mask in 0u8..8,
            a_value in 0u8..3, b_value in 0u8..3,
            a_flags in 0u8..4, b_flags in 0u8..4,
        ) {
            let a = psbt(a_mask, a_value, a_flags);
            let b = psbt(b_mask, b_value, b_flags);
            roundtrip(a.global().clone());
            roundtrip(a.inputs().clone());
            roundtrip(a.outputs().clone());
            roundtrip(a.clone());
            let joined = Lift::wrap(a).join(Lift::wrap(b));
            let paths: Vec<_> = Conflicts::conflicts(&joined).map(|c| c.path()).collect();
            prop_assert_eq!(paths.is_empty(), ResultWrapped::is_ok(&joined));
            for path in paths {
                match path {
                    ConflictPath::Global(_) => prop_assert!(a_value != b_value || a_flags != b_flags),
                    ConflictPath::Input { outpoint, .. } => {
                        let index = outpoint.txid.to_byte_array()[0];
                        prop_assert_ne!(a_mask & b_mask & (1 << index), 0);
                        prop_assert_ne!(a_value, b_value);
                    }
                    ConflictPath::Output { id, .. } => {
                        prop_assert_ne!(a_mask & b_mask & (1 << id.as_bytes()[0]), 0);
                        prop_assert_ne!(a_value, b_value);
                    }
                    ConflictPath::InputMembership | ConflictPath::OutputMembership => prop_assert_ne!(a_mask, b_mask),
                }
            }
            prop_assert_eq!(ResultWrapped::is_ok(&joined), ResultWrapped::try_unwrap(joined).is_ok());
        }
    }
}
