# hax

hax extracts Rust to proof assistants. Its Lean backend does not use hax's own
frontend or engine: `cargo hax extract` runs Charon (Rust to LLBC) and Aeneas
(LLBC to Lean) and scaffolds a Lean package around the result, built against
hax's own Lean models of `core` and `alloc` (`CoreModels`) and its tactics
library (`Hax`). So the theorems are about the crate's own code, not a
transcription of it. `[scenario.concurrent-psbt]` in `hax.toml` selects the
crate and the backend, `cargo hax extract` runs over the cargo workspace, and
`Proofs.lean` proves things about the generated definitions. The sources are
not changed.

The generated package is never checked in; the check regenerates it, so it
cannot drift from the Rust. `JoinResult.lean` of `formal/lean` is reused as the
specification: it is the hand-written model of `JoinResult`, and the theorems
here relate the extracted code to it. The F\* backend (the one hax's OCaml
engine exists for) is not used: the Lean backend extracted the whole crate
without trouble, so it was not tried.

## Results

The whole crate extracts, including iterators, `Vec`, generics over
`V: PartialJoin` (a Lean record of the trait's operations) and the blanket
`Join` over `JoinMut`. Every function returns `RustM`, hax's monad for
panics, overflow and divergence as well as the value. `&mut self` becomes a
returned value, and `debug_assert!` becomes an assertion that can fail.

The extraction does not typecheck as generated, which `Assumptions/FunsExternal.lean`
fixes. hax seeds that file for hand-written assumptions and never overwrites it,
and the check copies ours over the generated one. It declares two `axiom`s:

- `<[T]>::split_first`, which `CoreModels` has no model of, although the
  extracted code calls it, and which hax does not report as external. Only
  `has_duplicates` calls it.
- `Debug for Conflict<V>`: the declaration hax generates has an extra
  write-back component that `CoreModels`' `core.fmt.Debug` does not have, so it
  is an ill-typed `axiom`.

Only `Conflict::eq`, `has_duplicates` and `Debug` use them, so those are not
verified. The check audits `#print axioms` for every theorem: each depends only
on `propext`, `Classical.choice` and `Quot.sound`, never on those axioms or on
`sorry`.

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

The proofs follow those of the Aeneas PoC (#106), which proves the same
statements over the same extraction pipeline. They differ in the library the
extracted code is written against. Aeneas' standard library models `Vec` and
the iterators with `Result`; hax's `CoreModels` model them with `RustM`, an
interaction tree that tells divergence (`div`) from failure (`fail`).
`Vec::deref` and `Iterator::next` are monadic calls there, `Vec::push` is
`seq_push` and `Vec::contains` is `slice_contains_go`, which calls `eq` with the
list element first. The hypothesis on `V` is the same `EqSpec`, and the steps
are the same `WP.spec_bind`, `spec_mono` and `loop.spec_decr_nat` lemmas of
Aeneas' Lean library, which hax's package builds on. The gaps differ too:
Aeneas' library has no model of `Iterator::all`, `slice::Iter::all`,
`split_first` and `MaybeUninit`, which it declares as `axiom`s; `CoreModels`
models the first two and lacks `split_first`, as described above.

## Nix

`nix/formal/hax.nix` takes hax and the Aeneas fork commit it pins from their
flakes, pinned in `flake.nix` (the fork is the `aeneas-hax` input, since the
Aeneas PoC uses upstream Aeneas at another revision). Upstream provides the
`cargo-hax` driver, the OCaml engine (which the Lean backend never runs, but the
upstream package builds it anyway), Charon and the `aeneas` binary, but not the
Lean libraries. `Aeneas`'s Lean library and hax's (`Hax` and `CoreModels`,
whose lakefiles want to fetch dependencies with git) are built with
`buildLakePackage` on top of nixpkgs' cached Mathlib, which `leanPackages`
packages at the same Lean version (v4.35.0). `packages.hax` and
`packages.hax-lean` expose them, and `nix develop .#formal` has `cargo-hax`.

`checks.formal-hax` extracts in one derivation and typechecks the extraction,
the model and the proofs in another, so editing `Proofs.lean` doesn't rerun
Charon. The extraction runs offline: cargo resolves the whole workspace, so its
dependencies are vendored.

The check is defined for the systems both flakes cover and was only built on
aarch64-darwin. A cold build compiles the Rust toolchain closure, Charon, the
OCaml engine and `aeneas`, and the Lean libraries: `packages.hax-lean` took
93 minutes of wall time on aarch64-darwin, on a loaded machine and including
about 25 minutes the nix daemon spent in garbage collection. Mathlib comes from
cache.nixos.org. Once those are cached, the whole check takes about two minutes,
mostly Lean checking the extraction and `Proofs.lean`.

## Good for

- Proofs about the real code: a change to `partial.rs` changes the extraction
  and the proofs must follow, and a bug in the code fails a proof instead of a
  transcription of it.
- Functional correctness for all `V` and all sizes, with panics and overflow
  part of the statement, which the bounded checkers can't give.
- Reusing the model: `join_refines` ties the code to `JoinResult.lean`, so the
  semilattice proofs there apply to it unchanged.
- One command (`cargo hax extract`) from a scenario file to a Lean package, with
  a place for hand-written assumptions that regeneration never overwrites.

## Weak for

- Cost: a cold build takes more than an hour, and the toolchain pins a nightly
  Rust, a fork of Aeneas and a specific Lean version, so upgrades come in
  lockstep.
- Proof effort: loop and monadic code needs invariants, and the proofs lean on
  the `spec` API of Aeneas' Lean library, which changes between revisions.
- Coverage: the models are incomplete and the extraction does not say where. A
  missing function (`split_first`) and an ill-typed external declaration
  (`Debug::fmt`) both surface as type errors in the generated file, so the
  package does not build until the hand-written assumptions fill the holes.
  Functions that use them (`Conflict::eq`, `has_duplicates`) are unverified; the
  set-equality semantics of `eq` is assumed in the model, not proved from the
  Rust.
- Only the crate's pure Rust is in scope; `try_join` of a real `V` is a
  hypothesis (`hTry`, `hFlat`) because the instances live outside the crate.
- Capacity hypotheses (`size x + size y ≤ Usize.max`) are needed because
  `Vec::push` can fail, which the hand-written Lean model does not see at all.
