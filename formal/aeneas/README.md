# Aeneas

Aeneas translates Rust to Lean, so the theorems are about the crate's own code
rather than a transcription of it. Charon compiles
`crates/concurrent-psbt/src/lib.rs` to LLBC (the compiler's mid-level IR),
Aeneas turns that into a Lean file, and `Proofs.lean` proves things about the
generated definitions. The sources are not changed, and cargo is not involved:
`charon rustc` compiles `lib.rs` directly, which is enough because the crate has
no dependencies outside its tests.

The generated `ConcurrentPsbt.lean` is never checked in; the check regenerates
it, so it cannot drift from the Rust. `JoinResult.lean` of `formal/lean` is
reused as the specification: it is the hand-written model of `JoinResult`, and
the theorems here relate the extracted code to it.

## Results

The whole crate extracts, including iterators, `Vec`, generics over
`V: PartialJoin` (a Lean record of the trait's operations) and the blanket
`Join` over `JoinMut`. Every function returns `Result`, which carries panics
and arithmetic overflow as well as the value. `&mut self` becomes a returned
value, and `debug_assert!` becomes an assertion that can fail.

Four external functions have no model in Aeneas' standard library and are left
as `axiom`s: `Iterator::all`, `slice::Iter::all`, `slice::split_first` and
`MaybeUninit`. Only `Conflict::eq` and `has_duplicates` use them, so those two
are not verified. The check audits `#print axioms` for every theorem: each
depends only on `propext`, `Classical.choice` and `Quot.sound`, never on the
extraction's axioms or `sorry`.

Proved in `Proofs.lean`, for every `V` with decidable equality:

- `join_mut_spec`: `Conflict::join_mut` is set union that keeps the first
  operand's order, keeps a duplicate-free `Conflict` duplicate-free, and cannot
  fail unless the `Vec` would outgrow `usize` (a hypothesis, because
  `Vec::push` can fail in the model). The proof is a loop invariant over the
  extracted `join_mut_loop`, using `contains_spec` for `Vec::contains` with the
  hypothesis that `V`'s `PartialEq` agrees with `=`.
- `join_refines`: the extracted `impl Join for JoinResult<V>` equals, arm for
  arm, the model `join` of `JoinResult.lean`, reading a `Conflict` as the set of
  its entries, for every `try_join` that the model describes. It also bounds the
  size of a conflict result by its operands. This is the connection that lets
  anything proved about the model transfer to the code.
- `join_idem_of_flat`, `join_comm_of_flat`: with a flat `try_join` (`Ok` iff the
  values are equal, the `Foo` impl of the tests of `partial.rs`), the crate's
  `join` is idempotent and commutative, by `flat_idem` and `flat_comm` of the
  model.
- `join_assoc_forces_flat`: if the crate's `join` is associative up to set
  equality wherever it is defined, then `try_join a b = Ok c` implies `a = b`.
  This repeats `assoc_forces_flat` of the model on the extracted code: a
  `PartialJoin` with a non-trivial least upper bound breaks the `Join` laws
  under the structural equality the crate uses. The proof uses a singleton
  conflict, so this is about every value a `JoinResult` can hold, not only
  the states reachable from `Ok` values. The extraction is of the crate
  before "keep only the maximal values in a conflict" (in #97, #102 and
  #105). No crate bug beyond the one `formal/lean` documents turned up.

## Fixture

`checks.formal-aeneas-rejects-dropped-value` applies
`nix/formal/aeneas/dropped-value.patch` to the crate, so that `err ⊔ ok`
forgets the `ok` value (`Err(a)` instead of
`Err(a.join(Conflict::singleton(b)))`), and requires that the check fails with
the tactic error "`simp` made no progress". The extraction changes with the
Rust, the arm of `join_refines` that proved the `singleton` join has nothing
left to rewrite, and the proof stops there. The failure is a tactic error rather
than a goal naming the lost value, because the proofs follow the shape of the
code; a wrong value in the same shape would fail a goal instead. It is the bug
of the `formal/lean` fixture, this time in the crate's code.

## Nix

`nix/formal/aeneas.nix` takes Aeneas from its flake, pinned in `flake.nix` to
the last revision on Lean v4.35.0 (it keeps its own nixpkgs, so the pinned
nightly Rust and Charon build as upstream does). Upstream provides the `aeneas`
binary, with Charon wrapped to find its toolchain, but not the Lean library.
That library (`backends/lean`) imports Mathlib, which `leanPackages` in nixpkgs
packages at the same Lean version, so it is built with `buildLakePackage` on top
of the cached Mathlib. `packages.aeneas` and `packages.aeneas-lean` expose both,
and `nix develop .#formal` has the `aeneas` binary.

`checks.formal-aeneas` extracts in one derivation and typechecks the proofs in
another, so editing `Proofs.lean` doesn't rerun Charon.

The check is defined for the systems upstream's flake covers (x86_64 and
aarch64, Linux and Darwin) and was only built on aarch64-darwin. A cold build
compiles the Rust toolchain closure, Charon, the OCaml `aeneas` and the 1769
modules of the Aeneas Lean library: 55 to 70 minutes on aarch64-darwin, measured
on a loaded machine. Mathlib comes from cache.nixos.org. Once those are cached
the check takes about a minute and a half, mostly Lean checking `Proofs.lean`.

## Good for

- Proofs about the real code: a change to `partial.rs` changes the extraction
  and the proofs must follow, and a bug in the code fails a proof instead of a
  transcription of it.
- Functional correctness for all `V` and all sizes, with panics and overflow
  part of the statement, which the bounded checkers can't give.
- Reusing the model: `join_refines` ties the code to `JoinResult.lean`, so the
  semilattice proofs there apply to it unchanged.

## Weak for

- Cost: a cold build takes about an hour, and the toolchain pins a nightly Rust
  and a specific Lean version, so upgrades come in lockstep.
- Proof effort: loop and monadic code needs invariants, and the proofs lean on
  Aeneas' `Result` and specification API, which changes between revisions
  (Aeneas was pinned to the revision that matches Lean v4.35.0).
- Coverage: functions that touch the iterator or `MaybeUninit` models Aeneas
  lacks (`Conflict::eq`, `has_duplicates`) are extracted as axioms, so they are
  unverified; the set-equality semantics of `eq` is assumed in the model, not
  proved from the Rust.
- Only the crate's pure Rust is in scope; `try_join` of a real `V` is a
  hypothesis (`hTry`, `hFlat`) because the instances live outside the crate.
- Capacity hypotheses (`size x + size y ≤ Usize.max`) are needed because
  `Vec::push` can fail, which the hand-written Lean model does not see at all.
