use arbitrary::{Arbitrary, Unstructured};

use lattice_psbt::_internal::*;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    vout: u32,
    sequence: Option<u32>,
}

#[derive(Arbitrary, Debug)]
struct FuzzData {
    a: Vec<FuzzInput>,
    b: Vec<FuzzInput>,
}

fn make_set(items: Vec<FuzzInput>) -> InputSet {
    items
        .into_iter()
        .enumerate()
        .map(|(i, fi)| {
            let mut op = bitcoin::OutPoint::null();
            op.vout = fi.vout.wrapping_add(i as u32);
            let mut input = Input::new(&op);
            input.sequence = fi.sequence.map(bitcoin::Sequence);
            input
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
