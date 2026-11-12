//! `Conflict<V>` from crates/concurrent-psbt/src/lattice/partial.rs, transcribed for Creusot.
//!
//! Same properties as the Verus PoC (#98), but generic over the value type:
//! `T: Eq + DeepModel` stands in for `V: PartialJoin`, and Creusot specifies `==` as equality of
//! deep models. Unlike Verus, this is plain Rust: the contracts are attributes that erase to
//! nothing under rustc.
//!
//! * `join_mut` keeps the no-duplicates invariant, computes set union, and preserves the existing
//!   entries as a prefix (the documented "first value is the left operand").
//! * `eq` (length check plus one-directional containment) decides set equality, but only *because
//!   of* the no-duplicates invariant; it is a `requires`, not a free fact.

use creusot_std::{logic::such_that, prelude::*};

/// No value occurs twice.
#[logic(open)]
pub fn unique<T>(s: Seq<T>) -> bool {
    pearlite! { forall<i, j> 0 <= i && i < j && j < s.len() ==> s[i] != s[j] }
}

/// Every element of `a` is in `b`.
#[logic(open)]
pub fn subset<T>(a: Seq<T>, b: Seq<T>) -> bool {
    pearlite! { forall<x> a.contains(x) ==> b.contains(x) }
}

/// `a` and `b` hold the same elements, regardless of order and multiplicity.
#[logic(open)]
pub fn same_set<T>(a: Seq<T>, b: Seq<T>) -> bool {
    subset(a, b) && subset(b, a)
}

/// `<[V]>::contains`.
#[ensures(result == v.deep_model().contains(x.deep_model()))]
pub fn contains<T: Eq + DeepModel>(v: &Vec<T>, x: &T) -> bool {
    #[invariant(forall<k> 0 <= k && k < produced.len() ==> v.deep_model()[k] != x.deep_model())]
    for i in 0..v.len() {
        if &v[i] == x {
            return true;
        }
    }
    false
}

/// `impl JoinMut for Conflict<V>`.
#[requires(unique(this.deep_model()))]
#[ensures(unique((^this).deep_model()))]
#[ensures(forall<x> (^this).deep_model().contains(x)
    == (this.deep_model().contains(x) || other.deep_model().contains(x)))]
#[ensures(this.deep_model().len() <= (^this).deep_model().len())]
#[ensures(forall<k> 0 <= k && k < this.deep_model().len()
    ==> (^this).deep_model()[k] == this.deep_model()[k])]
pub fn join_mut<T: Eq + DeepModel>(this: &mut Vec<T>, other: Vec<T>) {
    let start = snapshot! { this.deep_model() };
    let others = snapshot! { other.deep_model() };
    #[invariant(unique(this.deep_model()))]
    #[invariant(start.len() <= this.deep_model().len())]
    #[invariant(forall<k> 0 <= k && k < start.len() ==> this.deep_model()[k] == start[k])]
    #[invariant(forall<x> this.deep_model().contains(x)
        == (start.contains(x) || others.subsequence(0, produced.len()).contains(x)))]
    for value in other {
        proof_assert! { value.deep_model() == others[produced.len() - 1] };
        proof_assert! { others.subsequence(0, produced.len())
        .ext_eq(others.subsequence(0, produced.len() - 1).push_back(value.deep_model())) };
        snapshot! { lemma_push_contains::<T::DeepModelTy> };
        if !contains(this, &value) {
            this.push(value);
        }
    }
    proof_assert! { others.subsequence(0, others.len()).ext_eq(*others) };
}

/// Where `s.removed(i)` takes its elements from.
#[logic]
#[requires(0 <= i && i < s.len())]
#[ensures(s.removed(i).len() == s.len() - 1)]
#[ensures(forall<j> 0 <= j && j < i ==> s.removed(i)[j] == s[j])]
#[ensures(forall<j> i < j && j < s.len() ==> s.removed(i)[j - 1] == s[j])]
pub fn lemma_removed_index<T>(s: Seq<T>, i: Int) {}

/// Removing index `i` keeps every element other than `s[i]`.
#[logic]
#[requires(0 <= i && i < s.len())]
#[ensures(forall<x> s.contains(x) && x != s[i] ==> s.removed(i).contains(x))]
pub fn lemma_removed_contains<T>(s: Seq<T>, i: Int) {
    lemma_removed_index(s, i)
}

/// What `push_back` adds.
#[logic]
#[ensures(forall<x> s.push_back(v).contains(x) == (s.contains(x) || x == v))]
pub fn lemma_push_contains<T>(s: Seq<T>, v: T) {}

/// A duplicate-free sequence contained in another one is no longer than it.
#[logic]
#[variant(a.len())]
#[requires(unique(a))]
#[requires(subset(a, b))]
#[ensures(a.len() <= b.len())]
pub fn lemma_unique_subset_len<T>(a: Seq<T>, b: Seq<T>) {
    pearlite! {
        if a.len() > 0 {
            let x = a[a.len() - 1];
            let a2 = a.subsequence(0, a.len() - 1);
            let i = choose_index(b, x);
            let b2 = b.removed(i);
            lemma_removed_index(b, i);
            lemma_removed_contains(b, i);
            lemma_unique_subset_len(a2, b2)
        }
    }
}

/// An index of `x` in `s`.
#[logic]
#[requires(s.contains(x))]
#[ensures(0 <= result && result < s.len() && s[result] == x)]
pub fn choose_index<T>(s: Seq<T>, x: T) -> Int {
    pearlite! { such_that(|i: Int| 0 <= i && i < s.len() && s[i] == x) }
}

/// If `a ⊆ b` and `b` has no more elements than duplicate-free `a`, then `b ⊆ a`.
#[logic]
#[requires(unique(a))]
#[requires(unique(b))]
#[requires(subset(a, b))]
#[requires(a.len() == b.len())]
#[ensures(subset(b, a))]
pub fn lemma_subset_antisym<T>(a: Seq<T>, b: Seq<T>) {
    pearlite! {
        // Otherwise some `y ∈ b \ a`, and `a ⊆ b - {y}`, which has fewer elements than `a`.
        if exists<y> b.contains(y) && !a.contains(y) {
            let y = such_that(|y: T| b.contains(y) && !a.contains(y));
            let i = choose_index(b, y);
            let b2 = b.removed(i);
            lemma_removed_index(b, i);
            lemma_removed_contains(b, i);
            lemma_unique_subset_len(a, b2)
        }
    }
}

/// `impl PartialEq for Conflict<V>`.
#[requires(unique(a.deep_model()))]
#[requires(unique(b.deep_model()))]
#[ensures(result == same_set(a.deep_model(), b.deep_model()))]
pub fn eq<T: Eq + DeepModel>(a: &Vec<T>, b: &Vec<T>) -> bool {
    if a.len() != b.len() {
        snapshot! { lemma_unique_subset_len::<T::DeepModelTy> };
        return false;
    }
    #[invariant(forall<k> 0 <= k && k < produced.len() ==> b.deep_model().contains(a.deep_model()[k]))]
    for i in 0..a.len() {
        if !contains(b, &a[i]) {
            return false;
        }
    }
    // Without `unique`, `a = [0, 0]` and `b = [0, 1]` would compare equal.
    snapshot! { lemma_subset_antisym::<T::DeepModelTy> };
    true
}

/// `Conflict::from_values`: deduplicate on construction.
#[ensures(unique(result.deep_model()))]
#[ensures(same_set(result.deep_model(), vals.deep_model()))]
pub fn from_values<T: Eq + DeepModel>(vals: Vec<T>) -> Vec<T> {
    let mut r = Vec::new();
    join_mut(&mut r, vals);
    r
}
