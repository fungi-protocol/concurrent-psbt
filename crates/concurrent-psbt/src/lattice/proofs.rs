//! Kani harnesses for the `JoinResult` lattice laws, run by `checks.formal-kani`.
//!
//! These check the real implementation over symbolic values, unlike the proptests which sample
//! it. A failing harness has found a bug. `should_panic` is kept only for the `Float` harness,
//! which documents a requirement on `==`-based impls rather than a bug in the crate.

use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

/// An arbitrary `JoinResult`: `Ok(v)`, or an `Err` of up to `max_conflict` (at most 2) values,
/// including the empty conflict `Conflict::from_values` permits. Lengths are chosen by
/// branching over fixed-size arrays rather than symbolically.
///
/// CBMC cost grows steeply with `Vec` length and with the unwind bound, so each harness uses
/// the smallest conflicts and the tightest unwind that cover its law: the unwind must exceed the
/// longest conflict the harness can build.
fn any_result<V: PartialJoin + kani::Arbitrary>(max_conflict: u8) -> JoinResult<V> {
    match kani::any_where(|shape: &u8| *shape <= max_conflict + 1) {
        0 => Ok(kani::any()),
        1 => Err(Conflict::from_values([] as [V; 0])),
        2 => Err(Conflict::from_values([kani::any::<V>()])),
        _ => Err(Conflict::from_values([kani::any::<V>(), kani::any()])),
    }
}

/// Three values suffice for every case split in the laws (equal, distinct, a third distinct
/// value), and a small domain keeps the formulas small.
#[derive(Debug, Clone, Copy, PartialEq, kani::Arbitrary)]
enum Tri {
    A,
    B,
    C,
}

/// The flat `try_join` of the `partial.rs` tests: `Ok` iff equal.
#[derive(Debug, Clone, Copy, PartialEq, kani::Arbitrary)]
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

#[kani::proof]
#[kani::unwind(3)]
#[kani::solver(kissat)]
fn flat_idempotent() {
    let a: JoinResult<Flat> = any_result(2);
    assert!(a.clone().join(a.clone()) == a);
}

#[kani::proof]
#[kani::unwind(3)]
#[kani::solver(kissat)]
fn flat_commutative() {
    let a: JoinResult<Flat> = any_result(1);
    let b: JoinResult<Flat> = any_result(1);
    assert!(a.clone().join(b.clone()) == b.join(a));
}

#[kani::proof]
#[kani::unwind(4)]
#[kani::solver(kissat)]
fn flat_associative() {
    let a: JoinResult<Flat> = Ok(kani::any());
    let b: JoinResult<Flat> = Ok(kani::any());
    let c: JoinResult<Flat> = Ok(kani::any());
    assert!(a.clone().join(b.clone()).join(c.clone()) == a.join(b.join(c)));
}

/// A field that may be absent, where absence is below any concrete value. This is a lawful
/// partial order with real least upper bounds, as `PartialJoin::try_join` documents.
#[derive(Debug, Clone, Copy, PartialEq, kani::Arbitrary)]
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

/// Fails: over a non-flat `try_join`, `join` is associative only up to the congruence
/// `try_join` generates, but results are compared structurally. Even from `Ok` values only,
/// `(None ⊔ 0) ⊔ 1 = {0, 1}` while `None ⊔ (0 ⊔ 1) = {None, 0, 1}`: equivalent results that
/// compare unequal, so semantically converged replicas look diverged.
#[kani::proof]
#[kani::unwind(4)]
#[kani::solver(kissat)]
fn optional_associative() {
    let a: JoinResult<Optional> = Ok(kani::any());
    let b: JoinResult<Optional> = Ok(kani::any());
    let c: JoinResult<Optional> = Ok(kani::any());
    assert!(a.clone().join(b.clone()).join(c.clone()) == a.join(b.join(c)));
}

/// A value whose `PartialEq` is not reflexive, as `f32::NAN` is, joined by `==` the way
/// `IdempotentValue`-style impls are.
#[derive(Debug, Clone, Copy, PartialEq, kani::Arbitrary)]
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
#[kani::proof]
#[kani::unwind(3)]
#[kani::solver(kissat)]
#[kani::should_panic]
fn float_not_idempotent() {
    let a: JoinResult<Float> = Ok(kani::any());
    assert!(a.clone().join(a.clone()) == a);
}
