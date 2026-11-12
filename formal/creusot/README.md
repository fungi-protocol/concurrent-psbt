# Creusot

Creusot is a deductive verifier for Rust: contracts are attributes, the code
is translated to Coma (a Why3 dialect), and the goals go to SMT solvers.

`src/lib.rs` is a transcription of `Conflict`'s `Vec` algorithms (from
`crates/concurrent-psbt/src/lattice/partial.rs`), not the crate's own source,
proved against set semantics. These are the same properties as the Verus PoC
(#98):

- `join_mut` keeps the no-duplicates invariant, computes set union, and keeps
  the existing entries as a prefix.
- `eq` (length check plus one-directional containment) decides set equality
  given no duplicates on both sides; that is a `requires`, not a free fact.
- `from_values` deduplicates.

Unlike the Verus transcription, this one is generic: `T: Eq + DeepModel`
stands in for `V: PartialJoin`, and `creusot-std` specifies `==` as equality of
deep models. It is also plain Rust: the contracts are attributes that erase
under rustc, so the code could in principle sit next to the real
implementation.

The proof needed a few small lemmas about `Seq` (where `removed` and
`push_back` take their elements from) and two counting lemmas
(`lemma_unique_subset_len`, `lemma_subset_antisym`). A lemma is a `#[logic]`
function with a contract and a unit body. Program code brings its contract
into scope with `snapshot! { lemma::<T::DeepModelTy> }`.

No bug was found.

`checks.formal-creusot` runs `cargo creusot`, which translates to Coma and
discharges the goals with why3find. `why3find.json` is what
`cargo creusot init` generates, except that the per-goal time limit is raised
from 1s to 10s. Every goal proves in milliseconds, but on a loaded machine
the 1s limit made the check flaky. The prover set is alt-ergo-free, z3, cvc5
and cvc4.

`checks.formal-creusot-rejects-duplicates` applies
`nix/formal/creusot/duplicates.patch`, which drops the `contains` guard so
`join_mut` pushes every value, and requires a `join_mut` goal to fail, to
show that the proof can fail.

## Nix

Creusot is not in nixpkgs. The upstream flake (`inputs.creusot`, `#free`)
builds creusot-rustc, cargo-creusot, Why3, why3find and the solvers from
source against its own nixpkgs pin. Nothing is substituted from a binary
cache, so the first build takes a long time (it compiles cvc4, cvc5 and z3).
The cold build time was not measured.
The checks run wherever upstream provides `#free` (aarch64-darwin,
aarch64-linux and x86_64-linux; only aarch64-darwin has been run).

This is the one verifier that edits shared files: it adds an input to
`flake.nix` and about 140 lines to `flake.lock`, for the input's own nixpkgs,
crane, flake-parts and rust-overlay. They have no `follows`, because upstream
builds its toolchain and solvers against its own pin and overriding it would
mean an untested rebuild.

The wrapped `cargo` patches `creusot-std` to its own copy in the store. The
check vendors the registry crates from upstream's `Cargo.lock` so cargo
resolves this crate offline and the repo needs no lock file of its own. The
lock is read from the flake input, so evaluating the check does not build
anything. (Reading it from the built package, `${creusot}/share/Cargo.lock`,
is import-from-derivation: every `nix flake check` would then evaluate by
building Creusot.)

Good for: functional correctness of generic Rust code written in ordinary
Rust, with contracts as attributes.

Weak for: setup cost, a long first build and a large `flake.lock` footprint.
Why3find calibrates each prover against a reference problem with a fixed 5s
timeout, which fails ("can not calibrate prover") on a saturated machine, so
the check is load-sensitive. Proofs that need the solver to pick witnesses or
count have to be written out as lemmas by hand.
