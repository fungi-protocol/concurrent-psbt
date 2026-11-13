# Bolero

Bolero is a front end for property tests and fuzzers. One `bolero::check!`
function runs as a randomized test under `cargo test`, as a fuzz target
(libfuzzer, afl, honggfuzz) through `cargo bolero`, and as a Kani proof, because
bolero's Kani engine fills the inputs from `kani::any`. The question for this
PoC is whether that simplifies the shared basis: stating a law once, instead of
once as a proptest and once as a Kani harness.

This PoC checks the lattice laws of `JoinResult` against the crate's own code,
not a transcription of it. The harnesses are in
`crates/concurrent-psbt/src/lattice/bolero_laws.rs`. They state the flat laws,
`optional_associative` and `float_not_idempotent` with the scopes of the Kani
harnesses (#95), and add two harnesses with larger scopes. Here they run only
on random inputs. Running the same functions under Kani needs the Kani
package, so that is a separate commit on top of both branches. The harnesses
live in the crate because `Conflict::from_values` is `pub(crate)`, and sit
behind the off-by-default `bolero-tests` feature, so the regular test, coverage
and mutants checks don't see them.

## What a harness looks like

One function holds the law, the generator and the scope:

```rust
#[test]
fn flat_commutative() {
    bolero::check!()
        .with_type::<(Res<Flat, 1>, Res<Flat, 1>)>()
        .cloned()
        .for_each(|(Res(a), Res(b))| {
            assert_eq!(a.clone().join(b.clone()), b.join(a));
        });
}
```

`cargo test` runs it on 1000 random inputs. The generator keeps to small,
fixed shapes so that a bounded model checker can take the same function.
`Conflict` has no public constructor and CBMC is slow on `Vec`, so `Res<V, MAX>`
builds an `Err` of up to `MAX` values by branching over fixed arrays. That is
narrower than the proptest strategy (up to 5 values).

Fuzzing is not wired up. `cargo bolero test flat_commutative --engine libfuzzer`
should run the same function, but it needs `cargo-bolero`, clang and a nightly
with sanitizers, which the Nix checks don't provide, and it was not tried.

## Results

| harness | scope | `formal-bolero` |
| --- | --- | --- |
| `flat_idempotent` | `Ok`, or `Err` of 0 to 2 values | passes |
| `flat_commutative` | `Ok`, or `Err` of 0 or 1 value | passes |
| `flat_associative` | `Ok` values only | passes |
| `optional_associative` | `Ok` values only, `None ⊑ Some(_)` | passes |
| `float_not_idempotent` | `Ok(f32)` | counterexample (`should_panic`), 10000 iterations |
| `flat_commutative_over_larger_conflicts` | `Err` of 0 to 2 values | passes |
| `flat_associative_over_any_result` | any result, `Err` of 0 to 2 values | passes |

Values range over a 3-element enum, as in the Kani harnesses.

- `optional_associative` found a crate bug, before it was fixed earlier in this
  series ("keep only the maximal values in a conflict"). With `None` below every
  `Some`, `(None ⊔ 0) ⊔ 1` was `Err{0, 1}` while `None ⊔ (0 ⊔ 1)` was
  `Err{None, 0, 1}`. The random run reached it in 4 inputs (74ms) and printed
  the input and a `BOLERO_RANDOM_SEED` to replay it.
- `float_not_idempotent` documents a requirement on implementors, not a bug in
  the crate: a `try_join` that decides by `==` is only idempotent if `==` is
  reflexive, and `NaN ⊔ NaN` gives `Err{NaN, NaN}` instead of `Ok(NaN)`. It keeps
  `should_panic`. Random `f32` bits are NaN about once in 256 draws, so the
  harness asks for 10000 iterations to find it reliably.
- `flat_commutative_over_larger_conflicts` and `flat_associative_over_any_result`
  have larger scopes than the others, with conflicts of up to 2 values on every
  input. They pass, but only as samples.

## Compared to the proptest basis

- **What each found.** Before the fix, the proptests ran only the flat `Foo`,
  which is lawful, so they couldn't see the bug in `JoinResult::join`. The fix
  added a prefix order on bit strings, whose laws catch it. The Bolero
  `optional_associative` found it.
- **Generators.** Proptest's strategies are richer: `prop_oneof`, `vec(1..=5)`,
  `prop_map`. Bolero derives `TypeGenerator` for enums and structs (`Tri`,
  `Flat`, `Optional` and `Float` needed nothing else), and has `.filter`, `.map`
  and `one_of`, but a type with an invariant or a crate-private constructor needs
  a hand-written `TypeGenerator`, as `Res` does. For types we don't own, such as
  the `psbt-v2` types, a derive is not possible.
- **Shrinking.** Proptest shrinks structurally. Bolero's random engine shrinks
  the recorded random stream within a time budget and replays by seed, and the
  fuzz engines minimize with their own tools. The counterexample here was
  already small, so this PoC can't say which shrinks better on a large input.
- **Speed.** The randomized run of all 7 harnesses took 2.0s, after 10s of
  incremental compilation.

## Could it replace the proptest basis?

Partly. For the lattice laws, `assert_join_laws!` and
`assert_partial_join_laws!` could expand to Bolero harnesses. Two things stand
in the way.

- **The dependency.** Bolero is a dev-dependency that adds 21 packages to
  `Cargo.lock` (8 of them `bolero-*`; the rest are `indexmap`, `toml_edit` and
  friends for the derive macro). The coverage, mutants and nextest checks should
  run it as ordinary tests, but that was not tried.
- **Proptest features.** Regression files map to Bolero's `corpus` and `crashes`
  directories, which `cargo test` also replays. Filtering is `.filter` on the
  generator rather than `prop_assume!`.

## Seeded bug

`checks.formal-bolero-rejects-dropped-value` applies
`nix/formal/bolero/dropped-value.patch` to the crate, which makes `Err ⊔ Ok`
forget the `Ok` value, and requires `flat_commutative` to fail. It uses the
random engine, which finds the counterexample at once, and runs that harness
only. It expects `cargo test`'s exit code 101 and the line
`bolero_laws::flat_commutative ... FAILED`. It shows the harnesses constrain the
real `impl Join for JoinResult`.

## Nix

`nix/formal/bolero.nix` adds two checks, with no new tool to package: Bolero is
a dev-dependency of the crate, so Nix only vendors it through `Cargo.lock`.

| check | what it runs |
| --- | --- |
| `formal-bolero` | `cargo test` over `bolero_laws::`, with random inputs; defined for every system, run here on aarch64-darwin |
| `formal-bolero-rejects-dropped-value` | the seeded bug above; `flat_commutative` must fail |

Adding the dependency changes `Cargo.lock`, so every check's dependency build
is redone once: the first run of `formal-bolero` took 7.5 minutes on a busy
machine, almost all of it that. With the dependencies cached it is about 12
seconds (10s of compilation and 2s of tests).

To run the harnesses by hand, in `nix develop .#formal`:
`cargo test -p concurrent-psbt --lib --features bolero-tests bolero_laws::`.

Good for: stating a property with a derivable generator (enums, small structs)
and a seed to replay a failure, in a form that a fuzzer or a bounded model
checker can also run. Weak for: types we can't derive a generator for, and
inputs larger than the fixed shapes a model checker can afford.
