#![cfg(any(feature = "unit-tests", feature = "prop-tests"))]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg_attr(coverage_nightly, coverage(off))]
mod cases {
    use bitcoin::hashes::Hash;
    use bitcoin::{Amount, OutPoint, ScriptBuf, Txid};
    use concurrent_psbt::Join;
    use concurrent_psbt::conflicts::{ConflictPath, PsbtConflict};
    use concurrent_psbt::global::{GlobalConflict, GlobalField};
    use concurrent_psbt::input::{Input, InputConflict, InputField};
    use concurrent_psbt::output::{Output, OutputConflict, OutputField, UniqueId};
    use concurrent_psbt::tx::UnorderedPsbt;
    use psbt_v2::raw;
    use psbt_v2::v2::{Global, Psbt};
    use std::collections::{BTreeMap, HashSet};

    fn check_locations(tag: u8, keydata: Vec<u8>) {
        let keys: Vec<_> = (0..3)
            .map(|scope| {
                let mut key = keydata.clone();
                key.push(scope);
                raw::Key {
                    type_value: 0x80,
                    key,
                }
            })
            .collect();
        let outpoint = OutPoint {
            txid: Txid::from_byte_array([tag; 32]),
            vout: 0,
        };
        let extra_outpoint = OutPoint {
            vout: 1,
            ..outpoint
        };
        let unique_id = |index| {
            let mut bytes = vec![tag; 16];
            bytes[15] = index;
            UniqueId::try_from(bytes).unwrap()
        };
        let id = unique_id(0);
        let extra_id = unique_id(1);
        let fields = |scope: usize, value| BTreeMap::from([(keys[scope].clone(), vec![value])]);
        let psbt = |value, add_members| {
            let mut input = Input::new(&outpoint);
            input.set_unknowns(fields(1, value));
            let mut output = Output::new(id.clone(), Amount::from_sat(42), ScriptBuf::new());
            output.set_unknowns(fields(2, value));
            let mut inputs = vec![input.into()];
            let mut outputs = vec![output.into()];
            if add_members {
                inputs.push(Input::new(&extra_outpoint).into());
                outputs.push(
                    Output::new(extra_id.clone(), Amount::from_sat(42), ScriptBuf::new()).into(),
                );
            }
            let mut global = Global {
                tx_modifiable_flags: 0,
                input_count: inputs.len(),
                output_count: outputs.len(),
                unknowns: fields(0, value),
                ..Default::default()
            };
            global.proprietaries.insert(
                raw::ProprietaryKey {
                    prefix: b"concurrent-psbt".to_vec(),
                    subtype: 0x10,
                    key: vec![],
                },
                vec![0x03],
            );
            UnorderedPsbt::try_from_psbt(Psbt {
                global,
                inputs,
                outputs,
            })
            .unwrap()
        };
        let other = tag.wrapping_add(1);
        let joined = psbt(tag, false).wrap().join(psbt(other, true).wrap());
        let mut paths = Vec::new();
        for diagnostic in joined.conflicts() {
            paths.push(diagnostic.path());
            let values = match diagnostic {
                PsbtConflict::Global(GlobalConflict::Unknown { values, .. })
                | PsbtConflict::Input {
                    conflict: InputConflict::Unknown { values, .. },
                    ..
                }
                | PsbtConflict::Output {
                    conflict: OutputConflict::Unknown { values, .. },
                    ..
                } => values,
                PsbtConflict::InputMembership(sets) => {
                    assert_eq!(sets.len(), 2);
                    assert!(sets.contains(&HashSet::from([&outpoint])));
                    assert!(sets.contains(&HashSet::from([&outpoint, &extra_outpoint])));
                    continue;
                }
                PsbtConflict::OutputMembership(sets) => {
                    assert_eq!(sets.len(), 2);
                    assert!(sets.contains(&HashSet::from([&id])));
                    assert!(sets.contains(&HashSet::from([&id, &extra_id])));
                    continue;
                }
                other => panic!("unexpected conflict: {other:?}"),
            };
            assert_eq!(
                values.into_iter().cloned().collect::<Vec<_>>(),
                [vec![tag], vec![other]]
            );
        }
        let expected = [
            ConflictPath::Global(GlobalField::Unknown(&keys[0])),
            ConflictPath::Input {
                outpoint: &outpoint,
                field: InputField::Unknown(&keys[1]),
            },
            ConflictPath::Output {
                id: &id,
                field: OutputField::Unknown(&keys[2]),
            },
            ConflictPath::InputMembership,
            ConflictPath::OutputMembership,
        ];
        assert_eq!(paths.len(), expected.len());
        assert!(expected.iter().all(|path| paths.contains(path)));
    }

    #[cfg(feature = "unit-tests")]
    #[test]
    fn diagnostics_locate_each_scope_and_its_keydata() {
        check_locations(7, vec![1, 2]);
    }

    #[cfg(feature = "prop-tests")]
    mod properties {
        use super::*;
        use proptest::prelude::*;
        proptest! {
            #[test]
            fn diagnostic_paths_keep_scope_identity_and_keydata(
                tag in any::<u8>(), keydata in proptest::collection::vec(any::<u8>(), 0..8),
            ) {
                check_locations(tag, keydata);
            }
        }
    }
}
