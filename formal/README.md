# Formal methods

Proof-of-concept harnesses for several verifiers, one directory each, so we can
judge which are worth adopting before the codebase grows.

Every verifier is a nix flake check with its tooling pinned through nix
(`nix/formal/<tool>.nix`, picked up automatically by `nix/formal/default.nix`).
Not every tool supports every system; a module that doesn't adds no checks
there.

```bash
nix build .#checks.<system>.formal-<tool>
```

```bash
nix develop .#formal
```

A check that finds a bug in the crate fails until the bug is fixed.

To show that a check can fail at all, a verifier also gets a
`checks.formal-<tool>-rejects-<bug>` fixture. It applies
`nix/formal/<tool>/<bug>.patch` and requires, via
`pkgs.testers.testBuildFailure'`, that the check fails with that specific
error.

Each `formal/<tool>/README.md` covers, in this order:

- what is checked, and whether against the crate's own code or a
  transcription of it;
- the results, including any bug found;
- the seeded-bug fixture;
- Nix: how the tool is packaged, which systems it runs on, and what a cold
  build costs;
- "Good for" and "Weak for".
