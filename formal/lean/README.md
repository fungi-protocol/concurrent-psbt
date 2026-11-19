# Lean 4

Lean proves theorems for all types and sizes, which the bounded checkers can't.
`JoinResult.lean` and `Modulo.lean` are transcriptions, not the crate's code:
they model `crates/concurrent-psbt/src/lattice/partial.rs` in core Lean 4, with
`JoinResult::join` transcribed arm for arm and parameterised by the `try_join`
in use. `PaymentGraph.lean` and `PaymentConfirmation.lean` are separate protocol
models and use Mathlib. There is no lake project.

## Structural equality

`JoinResult.lean` models `Conflict` as a set (`V → Bool`), as its `PartialEq`
ignores order and duplicates. Proved for every `V` with decidable equality:

- `flat_idem`, `flat_comm`, `flat_assoc`: with a flat `try_join` (`Ok` iff
  equal), `JoinResult` is a join-semilattice.
- `assoc_forces_flat`: if `join` is associative up to structural equality, then
  `try_join a b = Ok c` implies `a = b`. The proof uses a singleton conflict
  `Err{c}`, so the result is about every value `JoinResult` can hold: a
  non-flat `try_join` breaks the laws on some of them, though not necessarily
  on the states reachable from `Ok` values (with `Ok(max)` over a chain, those
  stay `Ok`).
- `option_not_assoc`: a concrete counterexample built only from `Ok` values.

## Modulo the congruence

Structural equality is too fine when `try_join` merges values semantically, as
it may (e.g. MuSig2 partial signatures, or a list of tap leaf hashes that is
semantically a set). `Modulo.lean` proves the laws up to what results denote.
A *model* of a `try_join` is a join-semilattice `S` with `f : V → S` satisfying
every equation `try_join` asserts (`Ok c` means `f c = f a ⊔ f b`). Two results
are `Equiv` when every model denotes them the same; this is the congruence
`try_join` generates, and it quotients out whatever equivalence `try_join`
treats as equality.

`Equiv` takes `try_join`'s equations as given, so it can't tell a faithful
merge from a lossy one. A `try_join` returning `Ok(a)` for every pair forces
`f a = f b` for all `a` and `b`, so every result denotes the same thing and the
laws below hold trivially. They say `join` is lawful relative to `try_join`,
not that `try_join` keeps the information it should; that needs an
independently stated order, as the option example has (`option_nontrivial`).

`Modulo.lean` stands alone: it re-declares `JoinResult`, `join` and the option
example with `Conflict` as a `List`, as the Rust `Vec` is, rather than the set
above. Order and duplicates don't affect denotations, and a list lets the TLA+
states be written out literally. Both transcribe the same `partial.rs`.

- `denote_join`, `join_idem`, `join_comm`, `join_assoc`, `join_congr`: `join`
  is a semilattice modulo `Equiv` for *every* `try_join`, flat or not.
- `self_join_equiv`: a normalising join (sorting tap leaf hashes) is idempotent
  modulo `Equiv` even though `Ok(a) ⊔ Ok(a)` is structurally `Ok(sort(a))`.
- `option_converges`, `option_nontrivial`: the two diverging TLA+ states,
  `Err{x, y}` and `Err{absent, x, y}`, are equivalent, and `Equiv` is not
  trivial.
- `flat_equiv_members`: for a flat `try_join`, equivalent results have the same
  members, so `Equiv` is no coarser than the set equality `Conflict`'s
  `PartialEq` implements.
- `above_sup`, `above_injective`, `gset_sup`: every join-semilattice embeds
  into a powerset under union (`a ↦ {b | ¬ a ≤ b}`), so any join-semilattice
  can be represented by a grow-only set. The embedding re-encodes each value
  as a set, which can be infinite, so it says nothing about the crate's finite
  `Vec` of raw values: there `{a} ∪ {b}` stays `{a, b}` even when `a ⊔ b`
  exists.

Consequence for the crate: the join laws hold, but only up to `Equiv`. `==` on
`JoinResult` and `Conflict`'s deduplication are structural, which is finer than
`Equiv` whenever `try_join` is not flat. Equivalent results then compare
unequal, so replicas that have converged semantically look diverged, and a
conflict can list two entries that mean the same thing. That is a bug in the
crate, not in the laws. Comparing entries pairwise modulo `Equiv` would not
fix it, and `Equiv` has no decision procedure here anyway: `absent`, `x` and
`y` are pairwise distinct, and dropping `absent` from `Err{absent, x, y}` is
absorption by the other entries, not deduplication. A fix needs a normal form
for conflicts. "keep only the maximal values in a conflict" (in #97, #102 and
#105) gives one for a `try_join` that returns the greater operand; merging joins
still have none.

## Payment confirmation

`PaymentGraph.lean` models finite weighted payment graphs and pairwise netting
of GRANT totals. Internal incoming and outgoing totals count the same edges, so
every nonempty residual graph has a vertex whose incoming total is at least its
outstanding total. Strong induction on the remaining participant set turns that
fact into a complete peeling order. `validated_peeling` also records an explicit
validation predicate at every step; `peeling_order` and `netted_peeling_order`
are the unconditional arithmetic results, without output-validation claims.

`PaymentConfirmation.lean` separately models confirmation, receipt delivery, and
signing as a transition system. Its soundness theorem preserves validation and
receipt guards. At completion, `confirmed_protected` shows that a confirmed peer
validated and has receipts from all confirmed payees;
`confirmed_accounting_safe` states the corresponding conditional accounting
bound for an externally supplied realized-balance function.

The completion theorem is deliberately conditional: every participant with an
outgoing payment must be an input owner, every participant must pass direct
validation, and the trace must cooperatively execute confirmation, delivery, and
signing. Funded peers may confirm directly; unfunded peers conservatively wait
for all payees' receipts. Only input owners sign, and completion requires every
input owner in the session to sign.

These are fixed-context abstractions, not proofs about the Rust implementation.
Cryptographic/script enforcement, output/fee validation, receipt transport,
commitment/grant-ID binding, and deriving realized balances from actual
transaction effects remain outside the model. In particular, belonging to
`funded` must mean that the input really requires that peer's authorization, not
merely that the peer once owned an outpoint.

`Axioms.lean` is the audit the check runs: none of the theorems may depend on
`sorryAx`, and no module may report a `sorry` warning.

`checks.formal-lean-rejects-dropped-value` applies
`nix/formal/lean/dropped-value.patch`, which makes the `err, ok` arm of `join`
forget the `ok` value, and requires the build to fail with unsolved goals: the
`flat_comm` and `flat_assoc` proofs stop typechecking. It shows the theorems
constrain the transcribed `join`.

## Nix

`checks.formal-lean` typechecks the modules with a plain `lean` from nixpkgs
(`pkgs.leanPackages.lean4`, also in `nix develop .#formal`), putting nixpkgs'
pinned Mathlib and its dependencies on `LEAN_PATH`, then runs the audit. The
proofs need no network or lake. A cold setup normally downloads the cached Lean
and Mathlib closures from cache.nixos.org, substantially more data than the old
core-only check; a cold-build timing has not been measured.

Good for: general theorems over all types and sizes, which bounded checkers
can't give, for pinning down exactly which trait contract is needed, and for
making the payment-confirmation assumptions explicit. The congruence is how the
spec for a `try_join` that merges semantically is stated at all.

Weak for: tying the result to the code. The lattice models are transcriptions,
so they can drift from `partial.rs` unnoticed, and a proof says nothing about a
real `try_join` implementation. The payment models are more abstract still:
their premises and fixed-context boundaries must be justified separately. Each
change to a model needs its proofs reworked.
