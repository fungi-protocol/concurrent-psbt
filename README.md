# concurrent-psbt

Concurrency friendly PSBT merging for collaborative transaction construction.

See [spec](https://github.com/nothingmuch/multiparty-protocol-docs/blob/psbt/psbt.md) for details.

## Contributing

### Development setup

```sh
nix develop  # enters the devshell with all tools
```

Or with direnv:

```sh
cp .envrc.local.example .envrc.local  # optional: customize
direnv allow
```

### Iteration loop

```sh
just check      # quick: tests + clippy (fast feedback)
just check-all  # all nix flake checks
just lint       # formatting + source invariants
just fmt        # auto-format
just test       # cargo-nextest
just clippy     # clippy -D warnings
just coverage   # llvm-cov report
just scrub      # history hygiene check
```

### Commit conventions

- One idea per commit, small and reviewable
- `[WIP]` prefix for work in progress
- `[EXPECT-FAIL: <check>]` for commits that intentionally break a check
- Tests live in the same file as the code they cover, feature-gated:
  ```rust
  #[cfg(feature = "unit-tests")]
  mod unit { ... }
  #[cfg(feature = "prop-tests")]
  mod laws { ... }
  ```
