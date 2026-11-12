# Stateright

[Stateright](https://github.com/stateright/stateright) is an explicit-state
model checker where the model is a Rust program. `src/lib.rs` models replicas
that each start with one `Ok` value and gossip by pulling a peer's state and
joining it into their own, as in the TLA+ model (#97). Unlike that model, this
one is not a transcription: the replica state is the crate's real
`JoinResult<V>` and each step calls the crate's real
`impl Join for JoinResult<V>`. Each replica's contribution is kept next to
the states, and two properties are checked:

- `preservation`: each replica's state is the maximal values among the
  contributions it has seen, `Ok` if there is exactly one. The order comes
  from a `below` method that each value type states separately from its
  `try_join`, so the property doesn't trust the join it checks.
- `convergence`: strong eventual consistency. Replicas that have seen the same
  updates hold equal states, compared structurally as the crate's `==` does.
  It follows from `preservation`, but on its own it would accept a join that
  loses updates, such as one that always returns `Ok(absent)`.

Three `PartialJoin` value types provide the configurations (`Flat`,
`Optional` and `Tree`). The checker is a breadth-first search over 3 replicas.

| config | values | `try_join` | result |
|----------|-------------------------|-------------------------------------|-----------------------------------|
| `flat` | `absent`, `x`, `y` | `Ok` iff equal | no violation (1674 unique states) |
| `option` | `absent`, `x`, `y` | `absent` below `x`, `y`; else flat | no violation (1674 unique states) |
| `tree` | `absent`, `x`, `x1`, `y` | `absent` below all; `x` below `x1` | no violation (3968 unique states) |

The `flat` and `option` state counts equal TLC's on the same model. Each search
takes under 30 milliseconds. `tree` covers refinement inside a conflict:
`Err{x, y} ⊔ Ok(x1)` must be `Err{x1, y}`.

Before conflicts kept only their maximal values, `option` violated
`convergence` in 3 steps. The check printed:

```
init
  r1 = Ok(Y) seen {r1} | r2 = Ok(X) seen {r2} | r3 = Ok(Absent) seen {r3}
step 1: r1 := r1 join r3
  r1 = Ok(Y) seen {r1,r3} | r2 = Ok(X) seen {r2} | r3 = Ok(Absent) seen {r3}
step 2: r1 := r1 join r2
  r1 = Err{Y, X} seen {r1,r2,r3} | r2 = Ok(X) seen {r2} | r3 = Ok(Absent) seen {r3}
step 3: r3 := r3 join r1
  r1 = Err{Y, X} seen {r1,r2,r3} | r2 = Ok(X) seen {r2} | r3 = Err{Absent, Y, X} seen {r1,r2,r3}
```

`r1` absorbed `absent` into `Ok(Y)` before the conflict arose, but joining
`Ok(absent)` into an `Err` only deduplicated it, so `r3` added `absent` to the
conflict and the two replicas disagreed. That was a bug in the crate, which the
TLA+ model found by a different path. It is fixed earlier in this series by
"keep only the maximal values in a conflict".

## Compared with the TLA+ model (#97)

- **Shared code, not a transcription.** The `JoinResult` and `Join` code is the
  crate's, so changing a match arm changes the model with no further edit. Only
  the gossip protocol, the properties and the three value types are written by
  hand. The model sees only the crate's public API, which is
  why `Conflict::from_values` is now `pub`.
- **Speed and state count.** The state spaces are the same (1674 for `flat` and
  `option`) and
  the check takes milliseconds. Wall time in nix is dominated by compiling the
  dependencies in release mode (minutes from cold); TLC has no compile step but
  needs a JVM.
- **Counterexample readability.** The trace prints the real `Err{Y, X}` values
  and replica names, from the model's `Debug` impls. TLC prints raw TLA+
  records, and states each invariant as a short formula that a non-Rust
  reader can skim.

## Nix

```bash
nix build .#checks.aarch64-darwin.formal-stateright
```

`nix/formal/stateright.nix` builds `formal/stateright` with crane (stable
toolchain) and runs the binary. The check runs on every system the flake
supports (`aarch64-darwin`, `aarch64-linux`, `x86_64-linux`). The crate is
deliberately not a workspace member: it has its own `[workspace]` table and
`Cargo.lock`, and depends on `concurrent-psbt` by path, so the workspace's
coverage, mutation and lint checks do not see it. Outside nix,
`cargo run --release -- flat option tree` in this directory runs the same thing,
and `--replicas N` changes the bound.

Good for: protocol-level and concurrency properties checked against the real
Rust merge code, with no drift and no new language to learn; fast small-scope
exhaustive search with readable, replayable traces; moving on to actors,
network faults and linearizability once there is real protocol code.

Weak for: explicit-state only, so the bounds stay small and there is no
symbolic checking or proof for unbounded replicas or values; liveness and
fairness are limited next to TLA+; the invariant and the state encoding
(`Hash`, `Eq`) are plain Rust, not a short spec; it only sees what the crate
makes public, and says nothing about anything inside a function body beyond
the inputs the model chooses.
