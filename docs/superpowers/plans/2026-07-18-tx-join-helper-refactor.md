# Transaction JOIN Helper Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor unordered transaction JOIN around private `Modifiability`, `TxModifiableFlags`, and `SizedSet<Result{Input,Output}Set>` helpers without changing behavior or the public API.

**Architecture:** `TxModifiableFlags` retains prior whole-field conflict state while unpacking its effective input/output bits into typed `Modifiability` values. Each `SizedSet<T>` joins one typed bit with its corresponding set and count, recording dimension-local prohibited growth. The joined bits are repacked into the original `JoinResult<u8>` representation, preserving the two immediate effective operand bytes on conflict. The existing `ResultGlobal` field types and `joinable_struct!` macro remain unchanged.

**Tech Stack:** Rust, `Join`/`JoinResult` lattice primitives, proptest, Nix development shell.

______________________________________________________________________

### Task 1: Specify the private helper behavior

**Files:**

- Modify: `crates/concurrent-psbt/src/psbt/tx.rs`

- Tests: in-file `tests::unit` and `tests::prop`

- [ ] **Step 1: Write failing unit tests for `SizedSet`**

Cover constructor validation, typed modifiability JOIN, clean count transport through a disjoint set JOIN, preservation of an existing count conflict, and canonical joined-count conflict replacement for prohibited growth.

- [ ] **Step 2: Write failing unit tests for `TxModifiableFlags`**

Cover effective flags for `Ok` and multi-value `Conflict`, native bitwise-AND JOIN, preservation of the failed JOIN's effective operand values, and deduplication when both failed operands have the same effective flags.

- [ ] **Step 3: Run the focused unit tests and verify RED**

Run:

```bash
nix develop -c cargo test -p concurrent-psbt --lib tx_join_helpers
```

Expected: compilation failure because the private helper types do not exist.

- [ ] **Step 4: Write failing property tests**

For `SizedSet<ResultInputSet>`, test constructor invariant preservation plus idempotence, commutativity, and associativity of JOIN over typed modifiability, count, and set values. For `TxModifiableFlags`, test effective-value idempotence, commutativity, and associativity under bitwise AND, and test that repacking preserves failed operand bytes and the effective value.

### Task 2: Implement the helpers

**Files:**

- Modify: `crates/concurrent-psbt/src/psbt/tx.rs`

- [ ] **Step 1: Add the private set-size abstraction**

Define a crate-private trait implemented by the clean and result-domain input/output set types:

```rust
trait SetLen {
    fn len(&self) -> usize;
}
```

- [ ] **Step 2: Add `SizedSet<T>`**

Define:

```rust
struct SizedSet<T> {
    modifiability: Modifiability,
    modifiability_conflicted: bool,
    count: JoinResult<usize>,
    set: T,
}
```

`SizedSet::new` converts an `Ok(declared)` mismatch into `Err(Conflict([declared, actual]))`. Its `Join` implementation joins the typed modifiability bit, set, and stored count while recording prohibited growth separately. Keeping the stored count JOIN associative avoids adding grouping-dependent cardinalities to malformed-count evidence; `joined_count()` applies the canonical singleton override only when exporting a dimension that violated modifiability.

- [ ] **Step 3: Add `TxModifiableFlags`**

Define:

```rust
struct TxModifiableFlags(JoinResult<u8>);
```

Provide effective-flag projection by passing validated clean values through unchanged and folding conflict values with bitwise AND from `BOTH`. Unpack the two effective bits as `Modifiability`, and repack their joined values through `From<TxModifiableFlags> for JoinResult<u8>`. When either dimension or an inherited flag is conflicted, repacking constructs a conflict containing the two immediate effective operand bytes.

- [ ] **Step 4: Run the focused helper tests and verify GREEN**

Run:

```bash
nix develop -c cargo test -p concurrent-psbt --lib tx_join_helpers
```

Expected: all helper unit and property tests pass.

### Task 3: Refactor `ResultUnorderedPsbt::join`

**Files:**

- Modify: `crates/concurrent-psbt/src/psbt/tx.rs`

- [ ] **Step 1: Replace local count/flag functions with the helpers**

Distribute each operand's effective input/output modifiability bits into its two `SizedSet`s, join those wrappers independently, and join ordinary globals independently.

- [ ] **Step 2: Enforce the whole-PSBT frozen-set invariant**

Treat an inherited formal flag conflict or prohibited growth in either dimension as a global flag conflict. Repack clean joined bits when possible; otherwise preserve the two immediate effective operand bytes. Override only the count of each dimension that actually violated modifiability.

- [ ] **Step 3: Override the three specialized global fields**

After generic global JOIN, replace `tx_modifiable_flags`, `input_count`, and `output_count` with outputs from the specialized helpers, then construct `ResultUnorderedPsbt` from the joined sets.

- [ ] **Step 4: Run focused transaction tests**

Run:

```bash
nix develop -c cargo test -p concurrent-psbt --lib psbt::tx::tests
```

Expected: existing behavioral regressions and new helper tests pass.

### Task 4: Verify the refactor

**Files:**

- Verify: `crates/concurrent-psbt/src/psbt/tx.rs`

- [ ] **Step 1: Run the crate test suite**

```bash
nix develop -c cargo test -p concurrent-psbt
```

- [ ] **Step 2: Run the dependent CLI suite**

```bash
nix develop -c cargo test -p ptj
```

- [ ] **Step 3: Run formatting and lint gates**

```bash
nix develop -c rustfmt --edition 2024 --check crates/concurrent-psbt/src/psbt/tx.rs
nix develop -c cargo clippy -p concurrent-psbt -p ptj --all-targets -- -D warnings
git diff --check -- crates/concurrent-psbt/src/psbt/tx.rs
```

### Deferred structural follow-up

Completed: `crates/concurrent-psbt/src/psbt/tx.rs` is now `tx/mod.rs`, with `ResultUnorderedPsbt` in `result.rs`, `UnorderedPsbt` in `unordered.rs`, and the internal JOIN helpers plus their unit/property tests in `sized_set.rs` and `tx_modifiability_flags.rs`.
