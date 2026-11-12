//! Fuzz targets shared by the libFuzzer, honggfuzz and AFL++ frontends.
//!
//! Each target takes the fuzzer's raw bytes, so every frontend is a one-line
//! wrapper and a crash found by one fuzzer reproduces under the others.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;

use arbitrary::{Result, Unstructured};
use concurrent_psbt::{Join, JoinResult, PartialJoin};

/// Check that [`Join`] is idempotent, commutative and associative on the
/// result-domain collections the crate provides joins for.
///
/// Operands are folds of freshly wrapped values, so they carry conflicts as
/// well as agreed values, and every arm of `JoinResult::join` is reachable.
pub fn lattice_laws(data: &[u8]) {
    // Running out of input mid-operand is not interesting; the fuzzer will
    // find longer inputs on its own.
    let _ = lattice_laws_structured(&mut Unstructured::new(data));
}

fn lattice_laws_structured(u: &mut Unstructured) -> Result<()> {
    match u.choose_index(4)? {
        0 => laws(u, |u| Ok(value(u)?.wrap())),
        1 => laws(u, |u| {
            Ok(if u.arbitrary()? {
                Some(value(u)?.wrap())
            } else {
                None
            })
        }),
        2 => laws(u, entries::<BTreeMap<_, _>>),
        _ => laws(u, entries::<HashMap<_, _>>),
    }
}

/// Values and keys come from small domains so that independently drawn
/// operands collide often: equal values exercise the `Ok` path, and
/// overlapping keys exercise joins of present entries rather than unions.
fn value(u: &mut Unstructured) -> Result<u8> {
    u.int_in_range(0..=3)
}

fn key(u: &mut Unstructured) -> Result<u8> {
    u.int_in_range(0..=7)
}

fn entries<M>(u: &mut Unstructured) -> Result<M>
where
    M: FromIterator<(u8, JoinResult<u8>)>,
{
    let len = u.int_in_range(0..=4)?;
    (0..len).map(|_| Ok((key(u)?, value(u)?.wrap()))).collect()
}

fn operand<T: Join>(
    u: &mut Unstructured,
    leaf: &impl Fn(&mut Unstructured) -> Result<T>,
) -> Result<T> {
    let mut acc = leaf(u)?;
    for _ in 0..u.int_in_range(0..=2)? {
        acc = acc.join(leaf(u)?);
    }
    Ok(acc)
}

fn laws<T>(u: &mut Unstructured, leaf: impl Fn(&mut Unstructured) -> Result<T>) -> Result<()>
where
    T: Join + Clone + PartialEq + Debug,
{
    let a = operand(u, &leaf)?;
    let b = operand(u, &leaf)?;
    let c = operand(u, &leaf)?;

    assert_eq!(a.clone().join(a.clone()), a, "join is not idempotent");
    assert_eq!(
        a.clone().join(b.clone()),
        b.clone().join(a.clone()),
        "join is not commutative"
    );
    assert_eq!(
        a.clone().join(b.clone()).join(c.clone()),
        a.join(b.join(c)),
        "join is not associative"
    );
    Ok(())
}

/// Panic on one 8-byte sequence, so that a sandboxed check can confirm a
/// fuzzer's coverage feedback works: each byte only advances the state when
/// the previous ones matched, which a guided fuzzer solves in seconds and
/// blind mutation essentially never does.
///
/// The panic only exists under `--cfg fuzzer_verify_crash`, which the
/// verification checks set, so running every target finds nothing here.
pub fn crash_test(data: &[u8]) {
    #[cfg(fuzzer_verify_crash)]
    {
        let mut state: u32 = 0;
        for &b in data {
            state = match (state, b) {
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
