//! `Conflict<V>` from crates/concurrent-psbt/src/lattice/partial.rs, transcribed into Verus
//! with `V = u8`.
//!
//! The TLA+ and Lean models treat `Conflict` as a mathematical set. This file discharges that
//! assumption against the actual `Vec`-based algorithms:
//!
//! * `join_mut` keeps the no-duplicates invariant, computes set union, and preserves the
//!   existing entries as a prefix (the documented "first value is the left operand").
//! * `eq` (length check plus one-directional containment) decides set equality, but only
//!   *because of* the no-duplicates invariant; it is a `requires`, not a free fact.
use vstd::prelude::*;

verus! {

use vstd::seq_lib::lemma_seq_contains_after_push;

broadcast use vstd::seq_lib::group_seq_properties;

/// `<[V]>::contains`.
fn contains(v: &Vec<u8>, x: u8) -> (r: bool)
    ensures
        r == v@.contains(x),
{
    let mut i = 0;
    while i < v.len()
        invariant
            0 <= i <= v.len(),
            forall|k: int| 0 <= k < i ==> v@[k] != x,
        decreases v.len() - i,
    {
        if v[i] == x {
            return true;
        }
        i += 1;
    }
    false
}

/// `impl JoinMut for Conflict<V>`.
fn join_mut(this: &mut Vec<u8>, other: Vec<u8>)
    requires
        old(this)@.no_duplicates(),
    ensures
        final(this)@.no_duplicates(),
        final(this)@.to_set() == old(this)@.to_set() + other@.to_set(),
        final(this)@.subrange(0, old(this)@.len() as int) == old(this)@,
{
    let ghost start = this@;
    let mut i = 0;
    while i < other.len()
        invariant
            0 <= i <= other.len(),
            this@.no_duplicates(),
            start.len() <= this@.len(),
            this@.subrange(0, start.len() as int) == start,
            forall|x: u8| #[trigger]
                this@.contains(x) <==> (start.contains(x) || other@.subrange(0, i as int).contains(x)),
        decreases other.len() - i,
    {
        let x = other[i];
        let ghost prefix = other@.subrange(0, i as int);
        let ghost before = this@;
        if !contains(this, x) {
            this.push(x);
        }
        proof {
            assert(other@.subrange(0, i + 1) =~= prefix.push(x));
            assert forall|y: u8|
                this@.contains(y) <==> (start.contains(y) || prefix.push(x).contains(y)) by {
                lemma_seq_contains_after_push(prefix, x, y);
                lemma_seq_contains_after_push(before, x, y);
                assert(before.contains(y) <==> (start.contains(y) || prefix.contains(y)));
            }
        }
        i += 1;
    }
    assert(other@.subrange(0, other.len() as int) == other@);
    assert(this@.to_set() =~= start.to_set() + other@.to_set());
}

/// A duplicate-free sequence has as many distinct elements as entries.
proof fn lemma_unique_len(s: Seq<u8>)
    requires
        s.no_duplicates(),
    ensures
        s.to_set().len() == s.len(),
{
    s.unique_seq_to_set();
}

/// `impl PartialEq for Conflict<V>`.
fn eq(a: &Vec<u8>, b: &Vec<u8>) -> (r: bool)
    requires
        a@.no_duplicates(),
        b@.no_duplicates(),
    ensures
        r == (a@.to_set() == b@.to_set()),
{
    if a.len() != b.len() {
        proof {
            lemma_unique_len(a@);
            lemma_unique_len(b@);
        }
        return false;
    }
    let mut i = 0;
    while i < a.len()
        invariant
            0 <= i <= a.len(),
            a.len() == b.len(),
            forall|k: int| 0 <= k < i ==> b@.contains(a@[k]),
        decreases a.len() - i,
    {
        if !contains(b, a[i]) {
            assert(a@.to_set().contains(a@[i as int]));
            return false;
        }
        i += 1;
    }
    proof {
        // `a ⊆ b` and both have `len` distinct elements, so `b ⊆ a` as well. Without
        // `no_duplicates`, `a = [0, 0]` and `b = [0, 1]` would compare equal.
        lemma_unique_len(a@);
        lemma_unique_len(b@);
        let (sa, sb) = (a@.to_set(), b@.to_set());
        assert(sa.subset_of(sb));
        if !(sa =~= sb) {
            let x = choose|x: u8| sa.contains(x) != sb.contains(x);
            sa.lemma_subset_not_in_lt(sb, x);
        }
    }
    true
}

/// `Conflict::from_values`: deduplicate on construction.
fn from_values(vals: Vec<u8>) -> (r: Vec<u8>)
    ensures
        r@.no_duplicates(),
        r@.to_set() == vals@.to_set(),
{
    let mut r = Vec::new();
    join_mut(&mut r, vals);
    assert(r@.to_set() =~= Seq::<u8>::empty().to_set() + vals@.to_set());
    r
}

} // verus!
