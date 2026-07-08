# PWA shell — file manifest

> **Integrated layout (this repo).** `app/*` + `icons/` + the scaffolding live
> at `contrib/ptj-pwa/`; the design docs below are under `contrib/ptj-pwa/docs/`;
> `src/backend/` + `src/transport/` live in the shared frontend tree at
> `contrib/demo-gui/src/{backend,transport}/` (imports adjusted from the
> staging-relative `../../../shared-frontend/` to `../shared-frontend/`); the
> wasm wrapper crate is `crates/concurrent-psbt-wasm/`. Paths in the tables
> below are the original authoring layout.

Authoritative list of files authored under
`.../scratchpad/frontier-build/pwa/`. Every path is relative to that dir. This is
the AUTHORED PLAN + SKELETON; the main loop integrates it into the repo (crate
under `crates/`, static app under `contrib/`, TS into the shared frontend build).

## Design docs (read these to understand the plan)

| File | Purpose |
|---|---|
| `README.md` | Overview: what the PWA is, the seam-reuse insight, layering, where to read next. |
| `decisions.md` | D1–D10, the load-bearing choices with rationale + rejected alternatives. |
| `feature-flags.md` | Crate Cargo features, build-time JS defines, runtime toggles; grounded vs deferred. |
| `frontend-integration.md` | The wire contract + how the PWA consumes the shared `Backend` seam (FetchLike framing annotated as retired). |
| `wasm-packaging.md` | How `concurrent-psbt-wasm` is built (nix/toolchain/Cargo changes, wasm-bindgen/wasm-opt pipeline). |
| `offline-first.md` | Service worker + manifest + install + sneakernet (the offline story). |
| `transports.md` | Opt-in browser transports + the signaling handoff to signaling-ohttp. |
| `file-list.md` | This manifest. |

## App shell (static, installable, offline) — integrates under `contrib/ptj-pwa/`

| File | Purpose |
|---|---|
| `app/index.html` | PWA head (manifest/theme/apple-touch), SW registration, shared-frontend entry. Body markup is the shared frontend's (not forked). |
| `app/manifest.webmanifest` | Installable PWA manifest: standalone, portrait, maskable icons, dark theme. |
| `app/sw.js` | Service worker: versioned app-shell cache, cache-first shell, SPA offline fallback, NEVER caches transport traffic. |
| `icons/README.md` | Icon spec (192/512/maskable) for the design pipeline. |

## WasmBackend + boot (TS) — integrates into the shared frontend `src/backend/`

| File | Purpose |
|---|---|
| `src/backend/fetch-like.ts` | RETIRED breadcrumb: FetchLike is an HttpBackend implementation detail (shared-frontend/backends/http.ts). |
| `src/backend/wasm-backend.ts` | `makeWasmBackend()`: loads concurrent-psbt-wasm and news up the SHARED `WasmBackend` (the canonical `Backend`) with an optional BrowserTransport. |
| `src/backend/wasm-loader.ts` | Lazily `init()`s the wasm-bindgen module; returns the `PtjWasmModule` op surface (camelCase exports). |
| `src/backend/shell-backend.ts` | `makeBackend()`: resolves the `Backend` per `PTJ_BUILD` (pwa→WasmBackend, webgui→HttpBackend, tauri→TauriBackend). Replaces app.ts's hard-wired `window.fetch`. |

## Transports (TS) — integrate into the shared frontend `src/transport/`

| File | Purpose |
|---|---|
| `src/transport/channel.ts` | TS mirror of transport-core's `AnonymousChannel`/`AttributableChannel`/`SenderId` + a `PwaTransport` façade. |
| `src/transport/framing.ts` | TS mirror of `frame`/`deframe` (u32-BE length prefix, 16 MiB cap) for byte/stream transports. |
| `src/transport/sneakernet.ts` | GROUNDED, always-on, NO-NETWORK transport: paste/file/clipboard/QR. Works fully offline. |
| `src/transport/webrtc.ts` | Browser-native `RTCPeerConnection` data-channel transport (Anonymous); SDP/ICE only via signaling-ohttp. str0m alternate deferred. |
| `src/transport/nostr-ws.ts` | Nostr over browser WebSocket (Attributable, sender pubkey). ws grounded; NIP-44 via rust `nostr` crate deferred. |
| `src/transport/ohttp-mailbox.ts` | Thin adaptor over the signaling-ohttp component: async offline mailbox (Anonymous) + the `Signaling` handle webrtc.ts consumes. Deferred (ohttp/payjoin ungrounded). |
| `src/transport/registry.ts` | Enumerate/construct transports; sneakernet always on, network opt-in + default off; honors `PTJ_TRANSPORTS` + config. |

## wasm wrapper crate (Rust) — MERGED into `concurrent-psbt-wasm` (wasm-bindgen component)

| File | Purpose |
|---|---|
| `crate/README.md` | Breadcrumb: the `ptj-wasm` wrapper authored here was merged into `../wasm-bindgen/` (`concurrent-psbt-wasm`); its `sync` op became the `localSync` export and its tests were ported. |

## Project scaffolding

| File | Purpose |
|---|---|
| `package.json` | Authoring-only manifest (type: module). Build scripts wired by the shared frontend build. |
| `tsconfig.json` | Strict TS config for the PWA backend + transport modules. |

## Integration targets (for the main loop)

- The wasm wrapper crate integrates from the wasm-bindgen component →
  `crates/concurrent-psbt-wasm/` (workspace globs `crates/*`).
- `app/` + `icons/` → `contrib/ptj-pwa/` (served static bundle output).
- `src/backend/` + `src/transport/` → the shared frontend core (the component
  that also owns the one `app.ts` edit to inject `makeBackend()`).
- Toolchain/Cargo/devshell changes (wasm target, secp256k1-sys hardeningDisable,
  wasm-bindgen-cli + binaryen) are recorded in `wasm-packaging.md` and belong to
  the toolchain/crate/devshell components — NOT applied by this component.
