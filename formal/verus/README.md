# Verus

Verus is a deductive verifier for Rust: the code carries `requires` /
`ensures` / loop invariants, and the proof obligations go to z3.

`conflict.rs` is a transcription of `Conflict`'s `Vec` algorithms (from
`crates/concurrent-psbt/src/lattice/partial.rs`) with `V = u8`, not the crate's
own source. Verus needs its own dialect, and a generic `V: PartialEq` would
need an explicit spec for `==`. It proves, for all input sizes:

- `join_mut` keeps the no-duplicates invariant, computes set union, and keeps
  the existing entries as a prefix (the documented left-operand-first order).
- `eq` (length check plus one-directional containment) decides set equality
  *only given* no duplicates: without that invariant, `[0, 0]` and `[0, 1]`
  compare equal. The `debug_assert!` in `PartialEq` is load-bearing.
- `from_values` deduplicates.

No bug was found. This backs the treatment of `Conflict` as a set in the TLA+
(#97) and Lean (#99) models, which assume it.

## Nix

Verus is not in nixpkgs, so `nix/formal/verus.nix` wraps the upstream release
bundle (which ships its own z3) together with the exact rust release its
`rust_verify` links against, taken from rust-overlay; the launcher's rustup
check is skipped. Only `aarch64-darwin` has a bundle hash (`supported`); on
other systems the module adds no checks. Adding a system means adding its
bundle hash. The check is `checks.formal-verus`; the wrapped tools are in
`nix develop .#formal`. A cold build only fetches the bundle and the toolchain,
and the proofs themselves verify in seconds.

Good for: proving functional correctness of algorithms (loops, invariants) for
all input sizes, which no bounded checker can do.

Weak for: checking the crate's own code. The code has to be rewritten in
Verus' dialect, so the proof is only as good as the transcription, and the
proofs need hand-written invariants and lemmas to steer z3.
