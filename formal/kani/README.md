# Kani

Kani is a bounded model checker for Rust: CBMC explores every execution of a
harness up to a loop-unwinding bound, over symbolic inputs. This PoC checks
the lattice laws of `JoinResult` (idempotent, commutative, associative)
against the crate's own code, not a transcription of it. The harnesses sit in
`crates/concurrent-psbt/src/lattice/proofs.rs` under `cfg(kani)`, and
`cargo kani` compiles them for CBMC.

They live in the crate because `Conflict::from_values` is `pub(crate)` and
`Conflict::singleton` is private at this point, so an external crate can't
build an `Err`.

## Results

| harness | scope | result | time |
| ---------------------- | ------------------------------------ | ------------------------------- | ----- |
| `flat_idempotent` | `Ok`, or `Err` of 0 to 2 values | proved | ~50s |
| `flat_commutative` | `Ok`, or `Err` of 0 or 1 value | proved | ~40s |
| `flat_associative` | `Ok` values only | proved | ~100s |
| `optional_associative` | `Ok` values only, `None ⊑ Some(_)` | **fails** | ~180s |
| `float_not_idempotent` | `Ok(f32)` | counterexample (`should_panic`) | ~1s |

Values range over a 3-element enum, which covers every equality case split.
Times are for each harness run alone; the check runs them in parallel
(`--jobs`), which takes about 6 minutes wall time in total.

- `optional_associative` found a bug in the crate. A `try_join` with a real
  least upper bound (here `None` below every `Some`) makes `join` satisfy the
  lattice laws only up to the congruence that `try_join` generates (#99
  proves this in Lean): `(None ⊔ 0) ⊔ 1` is `Err{0, 1}`, while
  `None ⊔ (0 ⊔ 1)` is `Err{None, 0, 1}`, and the two denote the same
  element. The crate compares results structurally, with `==` and in
  `Conflict`'s deduplication, so equivalent results compare unequal and
  replicas that have converged semantically look diverged. Kani finds this
  from `Ok` inputs alone. The harness asserts the law, so
  `checks.formal-kani` fails until the bug is fixed.
- `float_not_idempotent` documents a requirement on implementors, not a bug in
  the crate: a `try_join` that decides by `==` is only idempotent if `==` is
  reflexive. With `f32`, `NaN ⊔ NaN` gives `Err{NaN, NaN}` instead of
  `Ok(NaN)`. This concerns `==`-based impls like `IdempotentValue`, not
  `PartialJoin` itself: a float with a real lattice (e.g. max) would be a
  reasonable `PartialJoin`. No PSBT field is a float.

`checks.formal-kani-rejects-dropped-value` applies
`nix/formal/kani/dropped-value.patch` to the crate, which makes `Err ⊔ Ok`
forget the `Ok` value, and requires `flat_commutative` to fail. It shows the
harnesses constrain the real `impl Join for JoinResult`.

CBMC's cost here is driven by `Vec`, not by value width: comparing two
2-element conflicts took 18s at `unwind(8)` and 6s at `unwind(3)`. Switching
`u8` to a 3-valued enum barely helped, and neither did disabling the
memory-safety checks. Commutativity over conflicts of up to 2 values, and
associativity over any `Err` inputs, didn't finish within 8 minutes. That is
why the scopes above are narrow. The Lean proofs (#99) cover the general
case.

## Scrubber

`checks.formal-kani-scrubber` runs two harnesses over the real code of the
scrubber crate, in `crates/scrubber/src/scrub/proofs.rs`:

- `decode_encode_roundtrip`: `Pair::decode` never panics on up to 6 symbolic
  bytes, and whatever it accepts re-encodes to the bytes it consumed.
- `scrub_input_map`: on a concrete v2 global map (one input, no outputs)
  followed by up to 6 symbolic input-map bytes, `scrub` never panics, keeps
  no sensitive input field, and is idempotent.

No run has finished yet, so there are no results and no timing. CBMC has to
unwind bitcoin's `io::Read`-based decoders and `BTreeMap`. An ad hoc run did
not finish in 6 minutes, even with 4 symbolic bytes instead of 6, and a run
of the check as it is here did not finish within 4h37m on a loaded 24 GB
MacBook Air; it was stopped when the machine ran out of swap. The check
is separate from `formal-kani` so that it doesn't hold up the harnesses that
do finish. Making these tractable might mean stubbing (`#[kani::stub]`) the
consensus decoders, but then the parts checked would no longer be the real
code.

There is no seeded-bug fixture for it yet. The check isn't known to
terminate, so a fixture requiring it to fail could not tell a caught bug from
a timeout. One can be added once a run completes.

## Nix

`nix/formal/kani.nix` wraps the upstream Kani release bundle (CBMC, goto-cc,
kissat and kani-compiler, prebuilt) together with the pinned nightly that
kani-compiler links against, as `packages.<system>.kani`. The check,
`checks.<system>.formal-kani`, runs `cargo kani` over the crate through crane.
It runs on aarch64-darwin only: the module is guarded by `supported`, which
holds only for systems with a bundle hash, and elsewhere adds no package,
check or dev-shell entry. The closure is about 3.7 GiB, which a cold build has
to fetch; `formal-kani` takes about 6 minutes. `formal-kani-scrubber`
reuses the same check function for the scrubber crate.

Good for: panic freedom and properties of the real code over small,
heap-light inputs, with concrete counterexamples. Weak for: collection-heavy
code, where the bounds you can afford get small quickly.
