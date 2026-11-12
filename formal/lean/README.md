# Lean 4

Lean proves theorems for all types and sizes, which the bounded checkers can't.
`JoinResult.lean` is a transcription, not the crate's code: it models
`crates/concurrent-psbt/src/lattice/partial.rs` in core Lean 4 (no Mathlib, no
lake). `Conflict` is modelled as a set (`V → Bool`), as its `PartialEq` ignores
order and duplicates, and `JoinResult::join` is transcribed arm for arm,
parameterised by the `try_join` in use. Proved for every `V` with decidable
equality:

- `flat_idem`, `flat_comm`, `flat_assoc`: with a flat `try_join` (`Ok` iff
  equal), `JoinResult` is a join-semilattice.
- `assoc_forces_flat`: if `join` is associative up to structural equality, then
  `try_join a b = Ok c` implies `a = b`. The proof uses a singleton conflict
  `Err{c}`, so the result is about every value `JoinResult` can hold: a
  non-flat `try_join` breaks the laws on some of them, though not necessarily
  on the states reachable from `Ok` values (with `Ok(max)` over a chain, those
  stay `Ok`).
- `option_not_assoc`: a concrete counterexample built only from `Ok` values.

`Axioms.lean` is the audit the check runs: none of the theorems may depend on
`sorryAx`, and no module may report a `sorry` warning.

## Nix

`checks.formal-lean` typechecks the modules with a plain `lean` from nixpkgs
(`pkgs.lean4`, also in `nix develop .#formal`), then runs the audit. It needs
no network and no Mathlib, and it checks in seconds on top of the Lean
toolchain.

Good for: general theorems over all types and sizes, which bounded checkers
can't give, and for pinning down exactly which trait contract is needed.

Weak for: tying the result to the code. The model is a transcription, so it
can drift from `partial.rs` unnoticed, and a proof says nothing about a real
`try_join` implementation. Without Mathlib the proofs are by hand, and each
change to the model needs its proofs reworked.
