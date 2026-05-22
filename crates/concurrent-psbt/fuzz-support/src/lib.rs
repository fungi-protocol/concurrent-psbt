use arbitrary::{Arbitrary, Unstructured};
use bitcoin::hashes::Hash;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Txid};
use concurrent_psbt::output::{PSBT_OUT_UNIQUE_ID_SUBTYPE, ResultOutputSet};
use concurrent_psbt::{Join, PROPRIETARY_PREFIX, input::ResultInputSet};
use psbt_v2::raw::ProprietaryKey;
use psbt_v2::v2::{Input, Output};

const MAX_ITEMS: usize = 32;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    txid_tag: u8,
    vout: u8,
    sequence: Option<u32>,
    redeem_script: Option<Vec<u8>>,
}

#[derive(Arbitrary, Debug)]
struct FuzzOutput {
    sats: u64,
    script_byte: u8,
    unique_id: [u8; 4],
    redeem_script: Option<Vec<u8>>,
    witness_script: Option<Vec<u8>>,
}

#[derive(Arbitrary, Debug)]
struct FuzzJoinInputs {
    a: Vec<FuzzInput>,
    b: Vec<FuzzInput>,
}

#[derive(Arbitrary, Debug)]
struct FuzzJoinOutputs {
    a: Vec<FuzzOutput>,
    b: Vec<FuzzOutput>,
}

pub fn fuzz_join_inputs(data: &[u8]) {
    let Ok(case) = FuzzJoinInputs::arbitrary(&mut Unstructured::new(data)) else {
        return;
    };

    let a = ResultInputSet::from_inputs(case.a.into_iter().take(MAX_ITEMS).map(make_input));
    let b = ResultInputSet::from_inputs(case.b.into_iter().take(MAX_ITEMS).map(make_input));
    let _ = a.join(b).try_unwrap();
}

pub fn fuzz_join_outputs(data: &[u8]) {
    let Ok(case) = FuzzJoinOutputs::arbitrary(&mut Unstructured::new(data)) else {
        return;
    };

    let Ok(a) =
        ResultOutputSet::try_from_outputs(case.a.into_iter().take(MAX_ITEMS).map(make_output))
    else {
        return;
    };
    let Ok(b) =
        ResultOutputSet::try_from_outputs(case.b.into_iter().take(MAX_ITEMS).map(make_output))
    else {
        return;
    };
    let _ = a.join(b).try_unwrap();
}

pub fn fuzzer_verify_crash(data: &[u8]) {
    #[cfg(fuzzer_verify_crash)]
    {
        let mut state = 0u8;
        for &byte in data {
            state = match (state, byte) {
                (0, 0x7a) => 1,
                (1, 0x39) => 2,
                (2, 0xf1) => 3,
                (3, 0x42) => 4,
                (4, 0xce) => 5,
                (5, 0x8d) => 6,
                (6, 0xa7) => 7,
                (7, 0x05) => panic!("fuzzer verification crash"),
                _ => 0,
            };
        }
    }

    let _ = data;
}

fn make_input(input: FuzzInput) -> Input {
    let outpoint = OutPoint {
        txid: Txid::from_byte_array([input.txid_tag; 32]),
        vout: u32::from(input.vout),
    };
    let mut psbt_input = Input::new(&outpoint);
    psbt_input.sequence = input.sequence.map(Sequence);
    psbt_input.redeem_script = input.redeem_script.map(ScriptBuf::from_bytes);
    psbt_input
}

fn make_output(output: FuzzOutput) -> Output {
    let mut psbt_output = Output {
        amount: Amount::from_sat(output.sats),
        script_pubkey: ScriptBuf::from_bytes(vec![output.script_byte]),
        ..Output::default()
    };
    psbt_output.redeem_script = output.redeem_script.map(ScriptBuf::from_bytes);
    psbt_output.witness_script = output.witness_script.map(ScriptBuf::from_bytes);
    psbt_output
        .proprietaries
        .insert(unique_id_key(), output.unique_id.to_vec());
    psbt_output
}

fn unique_id_key() -> ProprietaryKey {
    ProprietaryKey {
        prefix: PROPRIETARY_PREFIX.to_vec(),
        subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
        key: vec![],
    }
}
