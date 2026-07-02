# PWA shell — design + skeleton (`ptj-pwa`)

The no-backend Progressive Web App shell for ptj. It is a **static site**
(HTML + service worker + a wasm-bindgen module) that runs the whole ptj
loop — `read PSBTs → join → show → repeat` — **entirely in the browser**,
with **no server**. Bytes are moved by opt-in browser-viable transports only.

This directory is the AUTHORED PLAN + SKELETON for that shell. It is written
under the frontier-build scratch area and is **read-only against the repo**:
nothing here is copied into `/tmp/cpsbt-test-prune-bookkeeping` or
`/Users/yuval/code`. The main loop integrates it later.

______________________________________________________________________

## What the PWA is (and is not)

**IS:** a mobile-first, offline-first static bundle that

- loads `concurrent-psbt` compiled to `wasm32-unknown-unknown` (via the thin
  `concurrent-psbt-wasm` wrapper crate + `wasm-bindgen`; staged in the
  wasm-bindgen component) and drives the SAME join/sort/inspect/create/atomize/
  etc. operations locally — no HTTP round-trips;
- reuses the EXISTING TypeScript frontend (`model.ts` + `app.ts`) unchanged in
  behavior by swapping ONLY the backend — the shared-frontend `WasmBackend`
  implementing the ONE canonical `Backend` interface
  (`shared-frontend/core/backend.ts`);
- works FULLY OFFLINE for **local sneakernet**: import / export / paste / QR of
  PSBTs, with no network of any kind (the service worker caches the whole app
  shell for airplane-mode / line-of-sight use);
- offers OPT-IN network transports that a user can turn on per-session: **nostr
  over WebSocket** and **WebRTC data channel**, whose SDP/ICE signaling is
  carried by the **signaling-ohttp** component (BIP 77 payjoin directory +
  OHTTP relay) — NEVER a signaling server that could learn the client IP.

**IS NOT:** a wallet (no key management, no signing — export to the user's real
wallet), and NOT a host for the heavy native transports. iroh (QUIC), arti
(Tor), nym, emissary (I2P) are **not browser-viable** and stay CLI/tauri-only.

______________________________________________________________________

## The single most important design fact: ONE Backend seam

> **Reconciliation (2026-07-06).** This component originally treated the old
> `FetchLike` free-function seam in `contrib/demo-gui/src/backend.ts` as the
> abstraction and implemented the PWA backend as a path-dispatch `FetchLike`.
> That fork is RETIRED. The canonical seam is the **`Backend` interface** in
> `shared-frontend/core/backend.ts`; `FetchLike` survives only as an
> implementation detail of the HTTP adapter (`shared-frontend/backends/http.ts`).

The PWA backend is NOT a new API. It is the shared-frontend `WasmBackend`
(`shared-frontend/backends/wasm.ts`) instantiated over the
`concurrent-psbt-wasm` module loaded in this page (`src/backend/wasm-loader.ts`

- `src/backend/wasm-backend.ts`). Each `Backend` method calls the matching
  wasm-bindgen export (camelCase js_name; the same ptj op logic compiled to
  wasm). The DTOs (`CreatePsbtRequest`, `PsbtResponse{psbt,inspect}`,
  `AtomizeResponse`, `SyncResponse`, `PtjBackendError{status,message}`) are
  identical across shells. This is why the three shells (webgui / PWA / tauri) do
  NOT fork the frontend: they differ only in which `Backend` instance `app.ts` is
  given at init (see `shared-frontend/app-wiring.md`).

______________________________________________________________________

## Layering

```
┌───────────────────────────────────────────────────────────────┐
│  index.html  +  manifest.webmanifest  +  sw.js (service worker) │  app shell (cached, offline)
├───────────────────────────────────────────────────────────────┤
│  shared frontend core  (model.ts + app.ts, UNCHANGED behavior)  │  ← the other component
│                      injected backend ↓                         │
├───────────────────────────────────────────────────────────────┤
│  WasmBackend  (shared Backend impl; op→wasm export, 1:1)        │  shared-frontend (bootstrapped in src/backend/)
│  concurrent-psbt-wasm  (wasm-bindgen wrapper over the library)  │  wasm-bindgen component (crate/ = merge breadcrumb)
├───────────────────────────────────────────────────────────────┤
│  transports (opt-in, all AnonymousChannel/Attributable seam):   │  this component (src/transport/)
│    • sneakernet: import/export/paste/QR   (NO network, always)  │
│    • nostr over WebSocket                 (opt-in)              │
│    • WebRTC data channel                  (opt-in)              │
│        └─ SDP/ICE signaling ─► signaling-ohttp (BIP77+OHTTP)    │  ← separate component
└───────────────────────────────────────────────────────────────┘
```

The transports are pushers into ONE shared PSBT array; `join()` runs on every
update (the shared directory model from `app-suite.md`). The join is
idempotent/commutative/associative, so duplicate deliveries and out-of-order
arrival are harmless — which is exactly why a broadcast data channel or a polled
mailbox is safe.

______________________________________________________________________

## Files in this component

See `file-list.md` for the authoritative list with one-line purposes. Highlights:

- `app/index.html`, `app/manifest.webmanifest`, `app/sw.js` — the installable
  offline shell.
- `crate/` — breadcrumb only: the `ptj-wasm` wrapper authored here was MERGED
  into `concurrent-psbt-wasm` (the wasm-bindgen component), which carries the
  real ports of ptj's op logic + the `localSync` local fold + native `cargo test` coverage.
- `src/backend/wasm-backend.ts` — `makeWasmBackend()`: loads the wasm module
  and news up the shared-frontend `WasmBackend` (the canonical `Backend`).
- `src/transport/` — TypeScript transport adaptors: sneakernet (grounded, no
  network), nostr-ws and webrtc (opt-in; nostr/str0m deferred exactly like the
  Rust transport-\* skeletons, browser-native web-sys paths grounded).
- `*.md` — the design decisions, feature flags, packaging, offline strategy,
  transport plans, and the frontend integration contract.

## Read next

1. `decisions.md` — the load-bearing choices and why.
1. `feature-flags.md` — every flag / build-mode toggle across crate + JS + SW.
1. `frontend-integration.md` — the wire contract + how the PWA consumes the
   shared `Backend` seam (historical FetchLike framing annotated).
1. `wasm-packaging.md` — how `concurrent-psbt-wasm` is built and loaded
   (grounded by the Feasibility probe).
1. `offline-first.md` — service worker + manifest + install + sneakernet.
1. `transports.md` — the opt-in browser transports and the signaling handoff.
1. `file-list.md` — the full manifest.

## Grounding & constraints honored

- STRICTLY read-only on the repo; everything authored under this scratch dir.
- No `jj`, no `cargo`/`nix` builds run by this component (authoring only).
- `concurrent-psbt → wasm32-unknown-unknown` is PROVEN by the Feasibility agent
  (23s build, real WebAssembly secp256k1-sys object, `crypto.getRandomValues`
  resolved). The required nix/toolchain + Cargo.toml changes are recorded in
  `wasm-packaging.md` and belong to the toolchain/crate components.
- Ungrounded deps (nostr, str0m, webrtc, ohttp, payjoin) are DEFERRED behind
  flags, mirroring the `transport-arti`/`transport-nym` skeleton pattern; the
  browser-native web-sys paths (RTCPeerConnection, WebSocket) are grounded and
  are the PWA's preferred implementations.
