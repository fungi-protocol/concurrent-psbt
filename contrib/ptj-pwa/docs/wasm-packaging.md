# WASM packaging — `concurrent-psbt-wasm`

> **Reconciliation (2026-07-06).** This doc described the `ptj-wasm` wrapper
> authored under `crate/`; that crate was MERGED into `concurrent-psbt-wasm`
> (the wasm-bindgen component; integrates as `crates/concurrent-psbt-wasm/`).
> The `{status,body}` export shape described here was superseded by the merged
> crate's JsValue/JsError style with camelCase js_name exports — see
> `wasm-bindgen/README.md` for the authoritative export table. The build/nix
> findings below remain valid (they are about the toolchain, not the wrapper).

How `concurrent-psbt` reaches the browser. Grounded by the Feasibility agent's
probe (23s build, real WebAssembly secp256k1-sys object, `crypto.getRandomValues`
resolved). The wrapper crate skeleton lives in the wasm-bindgen component; the
main loop compiles it, and the toolchain/crate components own the nix + Cargo
changes.

## Crate: `concurrent-psbt-wasm`

A thin wrapper that (a) depends on `concurrent-psbt`, (b) holds ALL
`wasm-bindgen` usage, (c) exposes one export per PSBT op (ported ptj op logic —
see D2/D3 as amended). `concurrent-psbt` stays wasm-bindgen-free.

```
wasm-bindgen/            (the staged component; -> crates/concurrent-psbt-wasm/)
  Cargo.toml             # cdylib+rlib; wasm-bindgen + serde-wasm-bindgen; debug-panic-hook feature
  build-wasm.sh          # cargo build -> wasm-bindgen --target web -> wasm-opt
  src/lib.rs             # #[wasm_bindgen] exports (camelCase js_name) + finish()
  src/ops.rs             # op bodies ported from ptj commands::* (+ native tests)
  src/dto.rs             # serde request DTOs (snake_case wire fields)
  src/psbt_io.rs         # base64 <-> PSBT
  src/negotiation.rs     # pay/confirm/payments band helpers
```

### Export shape

Each export takes base64-PSBT string(s) / a snake_case request object (JsValue)
and returns the response as a real JS object (serde-wasm-bindgen); failures are
THROWN as a `JsError` whose message equals the webgui's `{error}` text. The
shared-frontend `WasmBackend` rewraps a throw into `PtjBackendError`. `sort` is
positional — `sort(psbt, seedHex?)` — matching the canonical
`Backend.sortPsbt` arity; `localSync(psbts[])` is the Layer-2 local fold.

> Ideal follow-up (not required for the PWA to work): factor the op bodies out
> of `webgui.rs` into a shared module both `webgui.rs` and
> `concurrent-psbt-wasm/src/ops.rs` call, so the JSON contract has ONE source.
> Until then the ops are a faithful port. Recorded as a `TODO(share-ops)`.

## Cargo.toml essentials

See `wasm-bindgen/Cargo.toml` (authoritative). Key points: `crate-type =
["cdylib", "rlib"]`; deps `concurrent-psbt` + `psbt-v2/base64` + `bitcoin` +
`chacha20poly1305` + `serde`/`serde_json` + `wasm-bindgen` +
`serde-wasm-bindgen`; feature `debug-panic-hook` (optional
console_error_panic_hook); wasm-target-gated `getrandom = { version = "0.3",
features = ["wasm_js"] }`.

## Build (owned by toolchain/crate components; recorded here for completeness)

The Feasibility agent verified these are the ONLY changes needed:

1. **`nix/toolchain.nix`** — add the wasm target to the rust-overlay override:
   `targets = [ "wasm32-unknown-unknown" ];` (the current toolchain ships std
   only for aarch64-apple-darwin; the compiler knows the triple but has no wasm
   rustlib). Verified: sysroot then contains `wasm32-unknown-unknown` and the
   build succeeds.

2. **`concurrent-psbt/Cargo.toml`** — already has the wasm getrandom backend:
   ```toml
   [target.'cfg(target_arch = "wasm32")'.dependencies]
   getrandom = { version = "0.3", features = ["wasm_js"] }
   ```
   Belt-and-suspenders: `RUSTFLAGS='--cfg getrandom_backend="wasm_js"'` (both
   variants built in the probe).

3. **Nix wasm C-build hygiene for secp256k1-sys** (the ONLY hard blocker, and it
   is purely build-env, not code): the wasm derivation must NOT apply host
   hardening to cc-rs's clang. Set
   `hardeningDisable = [ "zerocallusedregs" "stackprotector" "stackclashprotection" ]`
   (or export `NIX_HARDENING_ENABLE` without `zerocallusedregs`, or point
   `CC_wasm32_unknown_unknown` at an unwrapped clang). Without this, clang rejects
   `-fzero-call-used-regs=used-gpr` for wasm32 and secp256k1-sys fails to compile
   its C.

4. **PWA build tooling (nix/devshell.nix packages)** — add `wasm-bindgen-cli`
   (or `wasm-pack`) + `binaryen` (`wasm-opt`) for the JS glue + size opt. NOT on
   the repo's default check path. `concurrent-psbt` needs no wasm-bindgen; only
   `concurrent-psbt-wasm` does.

## Build pipeline (what the PWA build script runs)

```
cargo build -p concurrent-psbt-wasm --lib --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/concurrent_psbt_wasm.wasm \
    --out-dir app/pkg --target web
wasm-opt -Oz app/pkg/concurrent_psbt_wasm_bg.wasm -o app/pkg/concurrent_psbt_wasm_bg.wasm
```

(`wasm-bindgen/build-wasm.sh` is the maintained form of this pipeline.)

Output loaded by `src/backend/wasm-loader.ts` via the wasm-bindgen `--target web`
ESM glue (`init()` returning the exports). `app-suite.md` estimates a ~50KB
bundle; the debug probe was 41MB, so `--release` + `wasm-opt -Oz` + feature
gating (dropping unused secp features per the Feasibility "optional/robustness"
note) is where the size comes from.

## What is grounded vs deferred here

- GROUNDED, buildable today: the default `concurrent-psbt-wasm` →
  wasm-bindgen glue → `WasmBackend`. This is the whole offline sneakernet PWA
  (including `localSync`).
- DEFERRED: the rust-side network transports (`transport-str0m`/`str0m`,
  `transport-webrtc-rs`/`webrtc-rs`, `transport-payjoin-dir`/`payjoin-dir`;
  TODO(transport-nostr): unauthored). The browser-native web-sys
  WebRTC/WebSocket TS transports are grounded and do not need the wasm core at
  all — they run in JS/TS and push bytes into the shared array.
