use super::*;

use bitcoin::{
    consensus::Encodable,
    hashes::{sha256::Hash as Sha256, Hash, HashEngine},
};
use psbt_v2::v2::Psbt;
use std::{collections::HashSet, ops::Deref};

#[test]
fn full_flow() {
    let mut tx = Transaction::<UnOrderedInputs>::new();
    let my_vin = Vin::from_input(&bitcoin::transaction::TxIn::default());
    tx.state.inputs.insert(PartialVin::from(my_vin.clone()));

    let mut tx = tx.try_resolve_outpoints().unwrap();
    let my_vout = Vout::from_output(&bitcoin::TxOut {
        value: bitcoin::Amount::from_sat(1000),
        script_pubkey: bitcoin::ScriptBuf::new(),
    });
    tx.state
        .outputs
        .insert(PartialOutput::from(my_vout.clone()));
    let tx = tx.apply_ordering_with_salt(&[0; 32]);
    let tx = tx.try_resolve_outputs().unwrap();
    let tx = tx.apply_ordering_with_salt(&[0; 32]);
    let tx = tx.finalize();
    let psbt = Psbt::from(tx);
    assert_eq!(psbt.global.tx_version, bitcoin::transaction::Version::TWO);
    assert_eq!(psbt.global.input_count, 1);
    assert_eq!(psbt.global.output_count, 1);
    assert_eq!(psbt.global.xpubs, BTreeMap::new());
    assert_eq!(psbt.global.proprietaries, BTreeMap::new());
    assert_eq!(psbt.global.unknowns, BTreeMap::new());
    assert_eq!(psbt.global.fallback_lock_time, None);
    assert_eq!(psbt.global.tx_modifiable_flags, 0);
    assert_eq!(psbt.global.version, psbt_v2::Version::TWO);

    assert_eq!(psbt.inputs[0].previous_txid, my_vin.previous_output);
    assert_eq!(psbt.inputs[0].spent_output_index, my_vin.spent_output_index);

    assert_eq!(psbt.inputs[0].final_script_sig, None);
    assert_eq!(psbt.inputs[0].final_script_witness, None);

    assert_eq!(psbt.inputs[0].sequence, None);
    assert_eq!(psbt.inputs[0].min_time, None);
    assert_eq!(psbt.inputs[0].min_height, None);
    assert_eq!(psbt.inputs[0].non_witness_utxo, None);
    assert_eq!(psbt.inputs[0].witness_utxo, None);
    assert_eq!(psbt.inputs[0].partial_sigs, BTreeMap::new());
    assert_eq!(psbt.inputs[0].sighash_type, None);
    assert_eq!(psbt.inputs[0].redeem_script, None);
    assert_eq!(psbt.inputs[0].witness_script, None);
    assert_eq!(psbt.inputs[0].bip32_derivations, BTreeMap::new());
    assert_eq!(psbt.inputs[0].tap_key_sig, None);
    assert_eq!(psbt.inputs[0].tap_script_sigs, BTreeMap::new());
    assert_eq!(psbt.inputs[0].tap_scripts, BTreeMap::new());
    assert_eq!(psbt.inputs[0].tap_key_origins, BTreeMap::new());
    assert_eq!(psbt.inputs[0].tap_internal_key, None);
    assert_eq!(psbt.inputs[0].tap_merkle_root, None);
    assert_eq!(psbt.inputs[0].proprietaries, BTreeMap::new());
    assert_eq!(psbt.inputs[0].unknowns, BTreeMap::new());

    assert_eq!(psbt.outputs[0].amount, my_vout.value);
    assert_eq!(psbt.outputs[0].script_pubkey, my_vout.script_pubkey);
    assert_eq!(psbt.outputs[0].redeem_script, None);
    assert_eq!(psbt.outputs[0].witness_script, None);
    assert_eq!(psbt.outputs[0].bip32_derivations, BTreeMap::new());
    assert_eq!(psbt.outputs[0].tap_internal_key, None);
    assert_eq!(psbt.outputs[0].tap_tree, None);
    assert_eq!(psbt.outputs[0].tap_key_origins, BTreeMap::new());
    assert_eq!(psbt.outputs[0].proprietaries, BTreeMap::new());
    assert_eq!(psbt.outputs[0].unknowns, BTreeMap::new());
}

#[test]
fn test_join_outputs() {
    let output_amount = bitcoin::Amount::from_sat(1000);
    let output_script_pubkey = bitcoin::ScriptBuf::new();

    let p1 = PartialOutput {
        value: Some(output_amount),
        ..Default::default()
    };
    let p1_again = PartialOutput {
        value: Some(output_amount),
        ..Default::default()
    };
    // Joining two PartialVouts with the same value should succeed
    let p1_joined = p1.join(&p1_again).unwrap();
    assert_eq!(p1_joined.value, Some(output_amount));

    let p1_with_different_value = PartialOutput {
        value: Some(bitcoin::Amount::from_sat(2000)),
        ..Default::default()
    };
    let p1_joined_with_different_value = p1.join(&p1_with_different_value).err();
    assert_eq!(
        p1_joined_with_different_value,
        Some(JoinError::ScalarDisagree)
    );

    let p1_with_script_pubkey = PartialOutput {
        script_pubkey: Some(bitcoin::ScriptBuf::new()),
        ..Default::default()
    };

    let p1_joined_with_script_pubkey = p1.join(&p1_with_script_pubkey).unwrap();
    assert_eq!(
        p1_joined_with_script_pubkey.script_pubkey,
        Some(output_script_pubkey)
    );
    assert_eq!(p1_joined.value, Some(output_amount));
}

fn make_vin(txid_byte: u8, vout: u32) -> Vin {
    let mut txid_bytes = [0u8; 32];
    txid_bytes[0] = txid_byte;
    Vin {
        previous_output: bitcoin::Txid::from_byte_array(txid_bytes),
        spent_output_index: vout,
        data: input::VinData::default(),
    }
}

fn make_vout(sats: u64) -> Vout {
    Vout {
        value: bitcoin::Amount::from_sat(sats),
        script_pubkey: bitcoin::ScriptBuf::new(),
        data: output::VoutData::default(),
    }
}

#[test]
fn ordered_inputs_rejects_new_input_on_join() {
    let vin_a = make_vin(0x01, 0);
    let vin_b = make_vin(0x02, 0);

    let base = OrderedInputs {
        inputs: vec![vin_a.clone()],
        outputs: HashSet::new(),
        global: Global::default(),
    };

    // Same input: join succeeds
    let same = OrderedInputs {
        inputs: vec![vin_a.clone()],
        outputs: HashSet::new(),
        global: Global::default(),
    };
    assert!(base.join(&same).is_ok());

    // Extra input not in base: join must fail
    let with_extra = OrderedInputs {
        inputs: vec![vin_a.clone(), vin_b.clone()],
        outputs: HashSet::new(),
        global: Global::default(),
    };
    assert_eq!(
        base.join(&with_extra).unwrap_err(),
        JoinError::InputsAlreadyOrdered
    );

    // Completely different input: join must fail
    let different = OrderedInputs {
        inputs: vec![vin_b.clone()],
        outputs: HashSet::new(),
        global: Global::default(),
    };
    assert_eq!(
        base.join(&different).unwrap_err(),
        JoinError::InputsAlreadyOrdered
    );
}

#[test]
fn partial_outputs_rejects_new_input_on_join() {
    let vin_a = make_vin(0x01, 0);
    let vin_b = make_vin(0x02, 0);
    let vout_a = make_vout(1000);

    let base = PartialOutputs {
        inputs: vec![vin_a.clone()],
        outputs: HashSet::from([vout_a.clone()]),
        global: Global::default(),
    };

    // New output is fine: outputs not yet ordered
    let extra_output = PartialOutputs {
        inputs: vec![vin_a.clone()],
        outputs: HashSet::from([make_vout(2000)]),
        global: Global::default(),
    };
    assert!(base.join(&extra_output).is_ok());

    // New input is not fine
    let extra_input = PartialOutputs {
        inputs: vec![vin_a.clone(), vin_b.clone()],
        outputs: HashSet::from([vout_a.clone()]),
        global: Global::default(),
    };
    assert_eq!(
        base.join(&extra_input).unwrap_err(),
        JoinError::InputsAlreadyOrdered
    );
}

#[test]
fn ordered_outputs_rejects_new_input_or_output_on_join() {
    let vin_a = make_vin(0x01, 0);
    let vin_b = make_vin(0x02, 0);
    let vout_a = make_vout(1000);
    let vout_b = make_vout(2000);

    let base = OrderedOutputs {
        inputs: vec![vin_a.clone()],
        outputs: vec![vout_a.clone()],
        global: Global::default(),
    };

    // Identical: succeeds
    let same = OrderedOutputs {
        inputs: vec![vin_a.clone()],
        outputs: vec![vout_a.clone()],
        global: Global::default(),
    };
    assert!(base.join(&same).is_ok());

    // New input: fails
    let extra_input = OrderedOutputs {
        inputs: vec![vin_a.clone(), vin_b.clone()],
        outputs: vec![vout_a.clone()],
        global: Global::default(),
    };
    assert_eq!(
        base.join(&extra_input).unwrap_err(),
        JoinError::InputsAlreadyOrdered
    );

    // New output: fails
    let extra_output = OrderedOutputs {
        inputs: vec![vin_a.clone()],
        outputs: vec![vout_a.clone(), vout_b.clone()],
        global: Global::default(),
    };
    assert_eq!(
        base.join(&extra_output).unwrap_err(),
        JoinError::OutputsAlreadyOrdered
    );
}
