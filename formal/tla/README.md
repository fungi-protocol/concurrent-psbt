# TLA+ (TLC)

`JoinResult.tla` is a hand transcription, not the crate's code. It models
replicas that each start with one `Ok` value and gossip by joining a peer's
state into their own, using the arms of `impl Join for JoinResult<V>`.
`Conflict` is a set because its `PartialEq` is set equality, and `Err` keeps
only the maximal values, as `Conflict::insert` does. Each replica's
contribution is kept, unchanged, next to the states, and TLC checks two
invariants:

- `Preservation`: each replica's state is the maximal values among the
  contributions it has seen, `Ok` if there is exactly one. This is defined
  without the model's `Join`.
- `Convergence`: strong eventual consistency. Replicas that have seen the same
  updates hold equal states, compared structurally as the crate's `==` does.
  It follows from `Preservation`, but on its own it would accept a `Join` that
  loses updates, such as one that always returns `Ok(absent)`.

| config | `try_join` | result |
|--------------|-------------------------------------|----------------------------|
| `flat.cfg` | `Ok` iff equal | no violation (1674 states) |
| `option.cfg` | `absent` below `x`, `y`; else flat | no violation (1674 states) |

Before conflicts kept only their maximal values, `option.cfg` violated
`Convergence` in 3 steps: `r2 := absent ⊔ x = Ok x`,
`r3 := y ⊔ Ok x = Err{x,y}`, `r1 := absent ⊔ Err{x,y} = Err{absent,x,y}`.
`r1` and `r3` had seen all three updates but disagreed. That was a bug in the
crate, fixed earlier in this series by "keep only the maximal values in a
conflict", and the model follows the fix.

The model checks safety only. There is no fairness or liveness property, so
nothing requires updates to reach every replica, and each replica contributes
once, at the start.

`checks.formal-tla-rejects-dropped-value` shows the check can fail at all. It
applies `nix/formal/tla/dropped-value.patch`, a wrong `Join` arm in the model
(`Err ⊔ Ok` forgets the `Ok` value), and requires TLC to report a
`Preservation` violation under `flat.cfg`. TLC stops at the first invariant
that fails. The patch changes the model, not the crate.

## Nix

TLC comes from nixpkgs (`tlaplus18`, a prebuilt jar plus a JVM), so nothing
is compiled; it is also in `nix develop .#formal`. The check runs `tlc` on
each config in turn and runs on every system the flake supports
(`aarch64-darwin`, `aarch64-linux`, `x86_64-linux`).

Good for: protocol-level and concurrency properties (message orderings,
interleavings) on a hand-written model, with counterexample traces that read
as a sequence of steps.

Weak for: tying the model to the code. It is a hand transcription, so
nothing checks that it still matches the Rust, and a green check says nothing
about the Rust itself. Explicit-state checking also needs small finite
constants.

## Keeping the model in step with the code

No tool checks that a TLA+ spec and a Rust codebase agree, or generates a spec
from Rust. These are the practical options, from loosest to tightest coupling:

- **Hand-maintained transcription (this PoC).** The operators mirror the match
  arms of `impl Join for JoinResult<V>`, and nothing enforces it. Cheap, but it
  drifts silently.
- **Trace validation.** Instrument the Rust to log each state transition, and
  have TLC check that the logged behaviour is allowed by the spec (Cirstea,
  Kuppe, Loillier and Merz, "Validating Traces of Distributed Programs Against
  TLA+ Specifications", 2024; applied to etcd's raft and Microsoft's CCF). It
  catches drift from the code side, but only on the behaviours the tests
  exercise.
- **Model-based testing.** Generate traces from the spec, with Apalache or
  Quint emitting ITF JSON, and replay them against the Rust, asserting that the
  states match at every step. The `quint-connect` crate drives this from Rust
  for Quint specs. It catches drift from the spec side, and the spec stays the
  source of truth.
- **Model in Rust ([Stateright](https://github.com/stateright/stateright)).**
  The model is a Rust program that calls the real `JoinResult::join`, so there
  is nothing to drift. The cost is explicit-state checking only (no symbolic
  checking, unlike Apalache) and no separate, readable spec. See #102 for a
  Stateright model of this crate and a comparison with this one.

Trace validation or model-based testing pay off once there is real protocol
code (transports, session state) whose spec should stay independent of the
implementation.
