# concurrent-psbt-wasm — the PWA no-backend core (design)

A WASM crate that exposes `concurrent-psbt`'s PSBT operations to JavaScript via
`#[wasm_bindgen]`, so the shared frontend can do **local PSBT manipulation with
no server** (the sneakernet PWA). Every export mirrors one ptj webgui `/api/*`
route over base64 PSBT strings + JSON DTOs, so the same TypeScript backend can
target either the HTTP server (webgui/tauri) or this WASM module (PWA).

> **Reconciliation (2026-07-06).** This is THE one wasm wrapper crate. A second
> wrapper (`ptj-wasm`, authored in parallel under `pwa/crate/`) was MERGED into
> this crate: its `sync` local-fold op was ported here as the `localSync`
> export, its richer op tests were ported into `src/ops.rs`, and its
> `{status,body}` byte-slice export style was dropped in favor of this crate's
> JsValue/JsError style (which the shared-frontend `WasmBackend` adapter maps to
> `PtjBackendError`). The name is `concurrent-psbt-wasm` because it wraps the
> concurrent-psbt LIBRARY, not the ptj CLI. JS-facing export names are
> camelCase (`js_name`); JSON wire fields stay snake_case.

It lives at `crates/concurrent-psbt-wasm/` (the workspace root globs
`crates/*`, so no members edit is needed). It is **NOT** in the default nix
flake check — the `#[wasm_bindgen]` surface is only meaningful on the wasm
target with the PWA toolchain (see Building); host `cargo test` covers the
pure op/DTO logic.

## Where it sits

```
                shared frontend core (model.ts pure  +  Backend interface)
                        (shared-frontend/core/backend.ts)
                                          |
        +---------------------------------+---------------------------------+
        |                                 |                                 |
   HttpBackend (webgui/tauri-http)   WasmBackend (PWA)                 TauriBackend (future)
   fetch -> ptj /api/*               concurrent-psbt-wasm              invoke("ptj_<op>")
        |                                 |
   ptj crate: commands::* + io + webgui   THIS CRATE: ops.rs ports the same
                                          commands::* logic onto concurrent-psbt
```

The ONE seam is the `Backend` interface in `shared-frontend/core/backend.ts`
(the `PtjBackend` variant that used to live in `frontend/backend.iface.ts` here
is retired; see `frontend/README.md`). The shared-frontend `WasmBackend`
adapter (`shared-frontend/backends/wasm.ts`) is the single typed caller of this
crate's exports, via its `PtjWasmModule` glue-surface type.

## Exported operations (JS names — camelCase js_name, the naming rule)

| Export | Backend method | webgui route | Request (positional args / snake_case object) | Response |
|-----------------|--------------------|-----------------------|--------------------------------------------------|---------------------------------|
| `inspect` | `inspectPsbt` | `/api/inspect` | `psbt: string` | inspect object (unwrapped) |
| `create` | `createPsbt` | `/api/create` | `{network,ordering?,seed_hex?,inputs,outputs}` | `{psbt,inspect}` |
| `join` | `joinPsbts` | `/api/join` | `string[]` | `{psbt,inspect}` |
| `sort` | `sortPsbt` | `/api/sort` | `(psbt: string, seedHex?: string)` — positional | `{psbt,inspect}` |
| `makeUnordered` | `makeUnordered` | `/api/make-unordered` | `psbt: string` | `{psbt,inspect}` |
| `atomize` | `atomizePsbt` | `/api/atomize` | `psbt: string` | `{fragments:[{psbt,inspect}]}` |
| `concatenate` | `concatenatePsbts` | `/api/concatenate` | `string[]` | `{psbt,inspect}` |
| `exportBip174` | `exportBip174` | `/api/export-bip174` | `psbt: string` | `{format:"bip174",psbt}` |
| `importBip174` | `importBip174` | `/api/import-bip174` | `psbt: string` | `{psbt,inspect}` |
| `pay` | `pay` | (new) | `{psbt,payment_hex,secret_hex?,dummy?}` | `{psbt,inspect}` |
| `confirm` | `confirm` | (new) | `{psbt,confirmation_hex,secret_hex?}` | `{psbt,inspect}` |
| `payments` | `payments` | (new) | `{psbt,secret_hex?}` | `{payments:[hex],confirmations:[hex]}` |
| `localSync` | `syncPsbts` (local leg) | `/api/sync` (local branch) | `string[]` (non-empty) | `{psbt,inspect,payments:[],confirmations:[]}` |
| `initPanicHook` | — | — | — | wires panics to console (debug) |
| `version` | — | — | — | crate semver string |

`localSync` is the LOCAL-FIRST sync core: `syncPsbts` works fully in-browser
with no server; a networked transport (payjoin-dir/OHTTP, webrtc, nostr) is
explicit opt-in in the JS layer and only ever ADDS input PSBTs before this fold.
Its `payments`/`confirmations` are empty because in the webgui contract those
come from transport messages (none here); the PSBT's own negotiation band is
read via the `payments` export.

**Field names are snake_case** on the request objects (`seed_hex`, `amount_btc`,
`payment_hex`), byte-identical to the webgui JSON, so the two backends are
indistinguishable to `app.ts`. Response JSON is produced by the SAME
`inspect_psbt` / `encode_psbt` code the webgui uses (ported), so rendering is
identical.

Errors are **thrown** as a `JsError` whose `.message` equals the webgui's
`{ "error": <string> }` text; the shared-frontend `WasmBackend` catches and
rewraps it into the same `PtjBackendError` the HTTP client throws.

## Why ops are ported, not called

The op logic (`create_psbt`, `join_psbts`, `sort_psbt`, `atomize`, the
bip174 converters, `inspect_psbt`, the DTO/base64 parsing) currently lives as
`pub(crate)` functions in the **ptj** crate and drags in ptj's CLI/file-IO
types. This crate must depend only on `concurrent-psbt` + `psbt-v2` + `bitcoin`
(+ serde), so `src/ops.rs` + `src/ops/{bip174_convert,inspect_json}.rs` +
`src/psbt_io.rs` **port the thin wrappers verbatim**, keeping the request/
response shapes identical. Each port is annotated with its ptj source path.

### Recommendation: promote to a shared crate (removes the duplication)

The clean end-state is to move the pure, IO-free op logic into a library both
ptj and this crate depend on — either into `concurrent-psbt` directly, or a new
small `ptj-core` crate — and delete the ports here. Candidates to promote (all
already pure): `create_psbt`, `join_psbts`, `sort_psbt`, `make_unordered_psbt`,
`atomize_psbt`, `concatenate_psbts`, `export_bip174_psbt`, `import_bip174_psbt`,
`inspect_psbt`, and the base64 `parse/encode`. ptj's `webgui.rs` would then call
the shared lib, and this crate would too, with zero divergence risk. The current
ports are the pragmatic first step; the shared crate is the follow-up.

## Randomness (Feasibility)

`concurrent-psbt`'s only runtime randomness is the 16-byte `UniqueId`
(`rand::random` in `src/psbt/output.rs`). On `wasm32-unknown-unknown` the
getrandom `wasm_js` feature (Cargo.toml, target-gated) routes this to the
browser `crypto.getRandomValues`. The wasm boundary test
(`tests/wasm.rs::create_empty_regtest_uses_browser_rng`) proves the whole RNG
chain works in-browser. The negotiation `pay --dummy` path and the element-id
generation also use this.

## Building (Feasibility findings baked into build-wasm.sh)

Two build-ENV facts (neither is a code change to concurrent-psbt):

1. **Toolchain needs the wasm target.** `nix/toolchain.nix` ships std only for
   `aarch64-apple-darwin`; add `targets = [ "wasm32-unknown-unknown" ];` to the
   rust-overlay override, else a bare wasm build fails "can't find crate for
   std".
1. **Nix cc-wrapper hardening breaks secp256k1-sys' C build.** cc-rs compiles
   secp256k1's C via the nix-wrapped clang, which injects the host-only flag
   `-fzero-call-used-regs=used-gpr` from `NIX_HARDENING_ENABLE`; clang rejects it
   for wasm32. Fix: strip host-only hardening for the wasm build —
   `NIX_HARDENING_ENABLE="fortify pic"` (or `hardeningDisable = ["zerocallusedregs" "stackprotector" "stackclashprotection"]`). This is the
   ONLY hard blocker and it is entirely in the nix env.

Downstream tooling (not a code blocker): producing the browser-loadable module
needs `wasm-bindgen`/`wasm-pack` to generate the JS glue that satisfies the
`__wbindgen_placeholder__` imports; add `wasm-pack` (or `wasm-bindgen-cli`) +
`binaryen` to `nix/devshell.nix` for the PWA build only.

```sh
# from crates/concurrent-psbt-wasm/
./build-wasm.sh pkg release     # -> pkg/ (ES module: default init + named ops)
./build-wasm.sh pkg dev         # debug + panic hook
```

## Behavioral delta vs webgui (one, documented)

ptj's `parse_psbt_bytes` wraps `Psbt::deserialize` in `std::panic::catch_unwind`
to turn a malformed-input panic into a clean error. On
`wasm32-unknown-unknown` panics abort (no unwinding), so `catch_unwind` cannot
recover them; `src/psbt_io.rs` relies on `Psbt::deserialize` returning `Err`
(the normal path) and surfaces any residual panic to the console via the
optional `console_error_panic_hook`. In practice the malformed inputs the webgui
tests exercise (`"not a psbt"`, bad base64) go through the `Err` path
identically.

## Not in this crate

- **Layer-3 sync.** The PWA's remote sync is NOT a wasm-core call. Browser
  transports (the payjoin-directory-over-OHTTP mailbox — `transport-payjoin-dir`
  rust-side; web-sys WebRTC / nostr-over-WebSocket in the TS layer) push remote
  PSBTs into the shared array and `localSync`/`join` (Layer-2, here) fold them.
  The wasm core is pure Layer-2 — exactly the boundary the frontend survey drew.
- **wasm-bindgen glue for transports.** This crate is PSBT ops only.

## File map

```
Cargo.toml                     crate manifest; wasm getrandom + wasm-bindgen deps
build-wasm.sh                  PWA build (hardening strip + getrandom + wasm-pack)
src/lib.rs                     #[wasm_bindgen] exports (one per /api route) + finish()
src/dto.rs                     serde request DTOs, snake_case = webgui JSON
src/psbt_io.rs                 base64 <-> PSBT (ported from ptj io.rs, no fs)
src/ops.rs                     op logic ported from ptj commands::* (+ native tests)
src/ops/bip174_convert.rs      v2<->v0 conversion (verbatim from ptj)
src/ops/inspect_json.rs        inspect JSON (verbatim from ptj)
src/negotiation.rs             pay/confirm/payments band helpers + AEAD (from ptj)
tests/wasm.rs                  wasm-bindgen boundary + browser-RNG tests
frontend/README.md             breadcrumb: the TS seam moved to shared-frontend/
```
