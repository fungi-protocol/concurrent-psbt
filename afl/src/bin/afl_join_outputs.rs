use arbitrary::{Arbitrary, Unstructured};

use lattice_psbt::_internal::*;
use lattice_psbt::fields::psbt_out_unique_id;

#[derive(Arbitrary, Debug)]
struct FuzzOutput {
    sats: u64,
    script_byte: u8,
    unique_id: [u8; 16],
}

#[derive(Arbitrary, Debug)]
struct FuzzData {
    a: Vec<FuzzOutput>,
    b: Vec<FuzzOutput>,
}

fn make_set(items: Vec<FuzzOutput>) -> OutputSet {
    items
        .into_iter()
        .map(|fo| {
            let mut output = Output::new(bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(fo.sats),
                script_pubkey: bitcoin::ScriptBuf::from(vec![fo.script_byte]),
            });
            output
                .proprietaries
                .insert(psbt_out_unique_id(), fo.unique_id.to_vec());
            output
        })
        .collect()
}

fn main() {
    afl::fuzz!(|data: &[u8]| {
        if let Ok(fd) = FuzzData::arbitrary(&mut Unstructured::new(data)) {
            let set_a = make_set(fd.a);
            let set_b = make_set(fd.b);
            let joined = Join::join(set_a.wrap(), set_b.wrap());
            let _ = joined.try_unwrap();
        }
    });
}
