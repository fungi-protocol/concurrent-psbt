//! Bolero harnesses for the `JoinResult` lattice laws, run by `checks.formal-bolero` (randomized,
//! under `cargo test`).
//!
//! Each law is written once as a `bolero::check!` harness, which `cargo test` runs on random
//! inputs. The generators keep to small, fixed shapes, so the same functions can also run as
//! bounded proofs.
//!
//! A failing harness has found a bug, and `should_panic` is kept only for the `Float` harness,
//! which documents a requirement on `==`-based impls.

use bolero::{Driver, TypeGenerator};
use core::ops::Bound;

use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

/// An arbitrary `JoinResult`: `Ok(v)`, or an `Err` of up to `MAX` (at most 2) values, including
/// the empty conflict `Conflict::from_values` permits.
///
/// The conflict length is chosen by branching over fixed-size arrays, not by generating a `Vec`,
/// so the generator also suits a bounded model checker, whose cost grows steeply with `Vec`
/// length. Each harness picks the smallest `MAX` that covers its law.
#[derive(Debug, Clone)]
struct Res<V: PartialJoin, const MAX: usize>(JoinResult<V>);

impl<V: PartialJoin + TypeGenerator, const MAX: usize> TypeGenerator for Res<V, MAX> {
    fn generate<D: Driver>(driver: &mut D) -> Option<Self> {
        const { assert!(MAX <= 2) };
        let shape = driver.gen_u8(Bound::Included(&0), Bound::Included(&(MAX as u8 + 1)))?;
        let (x, y) = (V::generate(driver)?, V::generate(driver)?);
        Some(Self(match shape {
            0 => Ok(x),
            1 => Err(Conflict::from_values([] as [V; 0])),
            2 => Err(Conflict::from_values([x])),
            _ => Err(Conflict::from_values([x, y])),
        }))
    }
}

/// Three values suffice for every case split in the laws (equal, distinct, a third distinct
/// value), and a small domain keeps the formulas small and makes random collisions likely.
#[derive(Debug, Clone, Copy, PartialEq, TypeGenerator)]
enum Tri {
    A,
    B,
    C,
}

/// The flat `try_join` of the `partial.rs` tests: `Ok` iff equal.
#[derive(Debug, Clone, Copy, PartialEq, TypeGenerator)]
struct Flat(Tri);

impl PartialJoin for Flat {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        if self == other {
            Ok(self)
        } else {
            Err(Conflict::from_values([self, other]))
        }
    }
}

#[test]
fn flat_idempotent() {
    bolero::check!()
        .with_type::<Res<Flat, 2>>()
        .cloned()
        .for_each(|Res(a)| {
            assert_eq!(a.clone().join(a.clone()), a);
        });
}

#[test]
fn flat_commutative() {
    bolero::check!()
        .with_type::<(Res<Flat, 1>, Res<Flat, 1>)>()
        .cloned()
        .for_each(|(Res(a), Res(b))| {
            assert_eq!(a.clone().join(b.clone()), b.join(a));
        });
}

#[test]
fn flat_associative() {
    bolero::check!()
        .with_type::<(Flat, Flat, Flat)>()
        .cloned()
        .for_each(|(a, b, c)| {
            let (a, b, c): (JoinResult<Flat>, JoinResult<Flat>, JoinResult<Flat>) =
                (Ok(a), Ok(b), Ok(c));
            assert_eq!(a.clone().join(b.clone()).join(c.clone()), a.join(b.join(c)));
        });
}

/// Larger scopes than the harnesses above: conflicts of up to 2 values on every input.
#[test]
fn flat_commutative_over_larger_conflicts() {
    bolero::check!()
        .with_type::<(Res<Flat, 2>, Res<Flat, 2>)>()
        .cloned()
        .for_each(|(Res(a), Res(b))| {
            assert_eq!(a.clone().join(b.clone()), b.join(a));
        });
}

#[test]
fn flat_associative_over_any_result() {
    bolero::check!()
        .with_type::<(Res<Flat, 2>, Res<Flat, 2>, Res<Flat, 2>)>()
        .cloned()
        .for_each(|(Res(a), Res(b), Res(c))| {
            assert_eq!(a.clone().join(b.clone()).join(c.clone()), a.join(b.join(c)));
        });
}

/// A field that may be absent, where absence is below any concrete value: one of the orders
/// `PartialJoin::try_join` allows beyond equality.
#[derive(Debug, Clone, Copy, PartialEq, TypeGenerator)]
struct Optional(Option<Tri>);

impl PartialJoin for Optional {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        match (self.0, other.0) {
            (None, _) => Ok(other),
            (_, None) => Ok(self),
            (Some(a), Some(b)) if a == b => Ok(self),
            _ => Err(Conflict::from_values([self, other])),
        }
    }
}

/// Associativity beyond equality joins. Before conflicts kept only their maximal values, this
/// failed from `Ok` values alone: `(None ⊔ 0) ⊔ 1 = {0, 1}` but `None ⊔ (0 ⊔ 1) =
/// {None, 0, 1}`, and `None` is below both.
#[test]
fn optional_associative() {
    bolero::check!()
        .with_type::<(Optional, Optional, Optional)>()
        .cloned()
        .for_each(|(a, b, c)| {
            let (a, b, c): (
                JoinResult<Optional>,
                JoinResult<Optional>,
                JoinResult<Optional>,
            ) = (Ok(a), Ok(b), Ok(c));
            assert_eq!(a.clone().join(b.clone()).join(c.clone()), a.join(b.join(c)));
        });
}

/// A value whose `PartialEq` is not reflexive, as `f32::NAN` is, joined by `==` the way
/// `IdempotentValue`-style impls are.
#[derive(Debug, Clone, Copy, PartialEq, TypeGenerator)]
struct Float(f32);

impl PartialJoin for Float {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        if self == other {
            Ok(self)
        } else {
            Err(Conflict::from_values([self, other]))
        }
    }
}

/// Counterexample: an `==`-based join over a non-reflexive `==` is not idempotent
/// (`NaN ⊔ NaN = Err{NaN, NaN}`), and the conflict's dedup invariant is silently violated.
///
/// Random `f32` bits are NaN about once in 256 draws, so the randomized run needs many iterations
/// to find it reliably.
#[test]
#[should_panic]
fn float_not_idempotent() {
    bolero::check!()
        .with_type::<Float>()
        .with_iterations(10_000)
        .cloned()
        .for_each(|a| {
            let a: JoinResult<Float> = Ok(a);
            assert_eq!(a.clone().join(a.clone()), a);
        });
}
