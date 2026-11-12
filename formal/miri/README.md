# Miri

Miri is an interpreter for Rust's mid-level IR that checks every memory
access, pointer and integer operation made by a running program for undefined
behaviour: out-of-bounds and dangling accesses, reads of uninitialized memory,
invalid values, data races, and violations of the aliasing models (Stacked or
Tree Borrows).

## What is checked

`checks.formal-miri` runs the existing test suite of both crates, unchanged,
under `cargo miri nextest run --workspace`. It checks the crate's own code, not
a transcription, and nothing is excluded. Miri only checks the executions the
tests perform. It is not a verifier: a property that no test exercises, or an
input no test generates, is not covered.

Both crates are `#![forbid(unsafe_code)]`, so there is no `unsafe` of our own
for Miri to check. What it does cover is the `unsafe` in the dependencies the
tests run through: `std` (`Vec`, `BTreeMap`, slices), `proptest`, and the
`bitcoin`/`psbt-v2` decoders the scrubber calls. A bug there, or a misuse of
one of their APIs, would be reported. Miri can't run foreign code, so a test
that reaches `secp256k1-sys` (C) would stop with an unsupported-operation
error. No current test does.

Settings, all in `nix/formal/miri.nix`:

- `PROPTEST_CASES=4`. Proptest's default is 256. Miri is orders of magnitude
  slower than native code and the scrubber's property tests decode and
  re-encode PSBTs, which is the expensive part: at 4 cases the slowest tests
  take 150 to 220s each, and the unit tests that decode a PSBT take 20 to 35s
  each. Extrapolating linearly, the default of 256 cases would take hours for
  `scrub::tests::prop::idempotent` alone. The cost is coverage: 4 random inputs
  per property rather than 256, so each property is a smoke test, not a
  search. Memory errors that only a rare input reaches won't be found.
- `PROPTEST_DISABLE_FAILURE_PERSISTENCE=true`. Proptest looks up the working
  directory to find its regression files, and Miri's isolation (kept enabled)
  doesn't allow `getcwd`. The regression files under
  `crates/scrubber/proptest-regressions` are not replayed.
- `MIRIFLAGS="-Zmiri-env-forward=..."`. Miri starts the program with an empty
  environment, so the two variables above have to be forwarded explicitly.
  Without that, proptest would not see them and would run its default 256
  cases.
- Isolation stays on. Miri then seeds its randomness deterministically
  (`-Zmiri-seed`, default 0), so runs are repeatable but always draw the same
  few cases from proptest. Set another seed to vary them.

## Results

All 66 tests pass, and Miri reports nothing. With no `unsafe` in the crates
this is the expected outcome, so the value is mostly in the future: the day an
`unsafe` block, an FFI call, or a `forbid` downgrade lands in the crate, the
check is already in place and exercises it with every test.

Two full runs on an Apple silicon laptop that was also running other builds:
6 minutes, and 23 to 25 minutes (the machine was busier). The 6-minute run was
roughly 50s to compile, 4.5 minutes in Miri (the scrubber property tests
dominate and run in parallel under nextest), and the rest sysroot setup and
startup. Expect the lower figure on an idle machine. The 35 `concurrent-psbt`
tests take 5 to 24s each; all of that crate's tests together finish well
inside the scrubber's slowest.

## Seeded bug

`checks.formal-miri-rejects-read-past-end` applies
`nix/formal/miri/read-past-end.patch`. It relaxes `forbid(unsafe_code)` and
replaces the `contains` in `Conflict`'s `PartialEq` with an `unsafe` loop whose
bound is off by one, so it reads one element past the end of the slice. That
element is spare capacity in the same allocation, so a native run reads a
stale value instead of crashing. The check runs only the one test that
compares two unequal conflicts (under 10s in Miri, plus compiling), and
requires Miri to report `Undefined Behavior: trying to retag` in
`contains_unchecked`. Miri objects as soon as the reference to the extra
element is created, since it lies outside the borrow of the slice, before
anything reads it.

`checks.formal-miri-read-past-end-native` runs that same test natively on the
same patched source (a plain `cargo nextest` in the dev profile), and it
passes in about a minute. This is the point of the fixture: the test suite
alone does not see this bug, and Miri does.

## Nix

`nix/formal/miri.nix` defines the check as a function of its source, and adds
the nightly toolchain and `cargo-nextest` to `nix develop .#formal`. The
toolchain is `selectLatestNightlyWith` with the `miri` and `rust-src`
components (minimal profile), separate from `toolchains.nightly`, which has no
`miri`.

`cargo miri` needs a sysroot: a copy of `std` built with Miri's
instrumentation. It builds it on first use with `cargo`, which downloads `std`'s
own crates.io dependencies (`hashbrown`, `libc`, `memchr`, ...). The sandbox has
no network, so they have to be vendored. The module does that with crane's
`vendorMultipleCargoDeps`, merging the workspace's `Cargo.lock` with the
`library/Cargo.lock` shipped in the `rust-src` component. The merged vendor
directory is set as `cargoVendorDir`, and `cargo miri` finds it through
`$CARGO_HOME/config.toml` like any other build, then builds the sysroot inside
the check. No prebuilt `MIRI_SYSROOT` derivation was needed.

Reading `library/Cargo.lock` from the toolchain at evaluation time is
import-from-derivation (IFD): `vendorMultipleCargoDeps` parses the lockfile
during evaluation, so Nix has to build the toolchain derivation (fetch and
unpack the nightly's components) before evaluation can finish. Whatever
evaluates this check pays that, serially and before any build starts. That
includes `nix flake check`, which evaluates every check, `formal-miri` or not,
on each system it evaluates; a build of another check, such as
`nix build .#checks.<system>.lint`, doesn't force it. Evaluation also fails
where IFD is disabled (`--no-allow-import-from-derivation`), and needs a
builder for each system evaluated. A
new nightly changes `std`'s lockfile and thus the vendored set, which rebuilds
automatically.

The module adds the check on every system of the flake, but it has only been
run on aarch64-darwin. A cold build fetches the nightly with Miri and about 30
crates for the sysroot, each a small derivation, and then pays the run time
above. The sysroot is not cached between runs: it is rebuilt inside every
check, so the check does not reuse `cargoArtifacts` either.

Good for: catching UB in `unsafe` code, or in dependencies' `unsafe`, as the
existing tests drive it, with a precise message (the access, the allocation,
the borrow that was invalidated), with no harnesses to write and the same
tests that run everywhere else. Reads of uninitialized or out-of-bounds memory
that a native run happily survives are the typical catch. It has nothing to
assert about logic, so it complements the lattice-law tests rather than
replacing them.

Weak for: proptest-heavy suites, where the slowdown forces the case count down
so far that the properties lose most of their power; code that calls C
(`secp256k1-sys`) or uses most of the OS; and crates with no `unsafe`, like
this one today, where it adds minutes of CI time and finds nothing until
`unsafe` appears.
