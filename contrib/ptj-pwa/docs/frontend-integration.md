# Frontend integration — the PWA's use of the ONE `Backend` seam

> **Reconciliation (2026-07-06).** This doc originally specified the PWA
> backend as a `FetchLike` path-dispatch. That variant is RETIRED: the canonical
> seam is the `Backend` interface (`shared-frontend/core/backend.ts`), the PWA
> adapter is the shared `WasmBackend` (`shared-frontend/backends/wasm.ts`)
> over the merged `concurrent-psbt-wasm` module, and `FetchLike` survives only
> inside `HttpBackend`. The route/DTO table below is still normative — it is
> the HTTP WIRE contract the webgui speaks and the JSON-shape contract the wasm
> ops reproduce — but the dispatch mechanism description is superseded by
> `src/backend/{wasm-loader,wasm-backend,shell-backend}.ts`.

This is the exact seam between the shared frontend core (`model.ts` + `app.ts`)
and the PWA's local WASM backend. It is the whole reason the frontend is NOT
forked three ways.

## The existing seam (from `contrib/demo-gui/src/backend.ts`)

```ts
type FetchLike = (
  path: string,
  init: { method: "POST"; headers: { "content-type": "application/json" }; body: string }
) => Promise<{ ok: boolean; status: number; json(): Promise<unknown> }>;
```

Every backend op (`inspectPsbt`, `createPsbt`, `joinPsbts`, `sortPsbt`,
`makeUnordered`, `atomizePsbt`, `concatenatePsbts`, `exportBip174`,
`importBip174`, `syncPsbts`) takes `fetchImpl: FetchLike` as arg 1 and calls
`postJson(fetchImpl, "/api/<op>", body)`. `postJson` does `JSON.stringify → fetch → .json()` and throws `PtjBackendError(status, msg)` on `!ok`.

The routes and DTOs are FIXED and shared with webgui:

| path | request DTO | response DTO |
|---|---|---|
| `/api/inspect` | `{psbt}` | `InspectResponse` |
| `/api/create` | `CreatePsbtRequest` (`network,ordering,seed_hex,inputs,outputs`) | `PsbtResponse{psbt,inspect}` |
| `/api/join` | `{psbts:string[]}` | `PsbtResponse` |
| `/api/sort` | `{psbt,seed_hex}` | `PsbtResponse` |
| `/api/make-unordered` | `{psbt}` | `PsbtResponse` |
| `/api/atomize` | `{psbt}` | `AtomizeResponse{fragments[]}` |
| `/api/concatenate` | `{psbts:string[]}` | `PsbtResponse` |
| `/api/export-bip174` | `{psbt}` | `ExportBip174Response{format,psbt}` |
| `/api/import-bip174` | `{psbt}` | `PsbtResponse` |
| `/api/sync` | `SyncRequest{psbts,iroh_ticket?,iroh_wait_ms?}` | `SyncResponse{psbt,inspect,payments[],confirmations[]}` |
| errors | — | `{error:string}` + HTTP status |

## The one required frontend change (owned by the shared-frontend component)

`app.ts` currently passes `window.fetch.bind(window)` at ~7 call sites. Replace
with a single injected backend chosen at boot:

```ts
// shared frontend boot (pseudocode; the shared-frontend component owns app.ts)
import { makeBackend } from "./shell-backend.js"; // resolves per PTJ_BUILD
const backend: Backend = await makeBackend();     // pwa → shared WasmBackend
// ...every prior free-function + window.fetch call site becomes a method call
backend.inspectPsbt(psbt);
```

The PWA supplies `makeBackend()` = `makeWasmBackend()`
(`src/backend/wasm-backend.ts`). webgui gets `new HttpBackend()`. tauri gets
`new TauriBackend()`. NOTHING else in `app.ts`/`model.ts` changes beyond the
method-call rewrite documented in `shared-frontend/app-wiring.md`.

## `WasmBackend` behavior (shared-frontend/backends/wasm.ts)

`makeWasmBackend()` lazily initializes the `concurrent-psbt-wasm` module
(`src/backend/wasm-loader.ts`) and returns `new WasmBackend(module)`. Each
`Backend` method calls ONE wasm export 1:1 (camelCase js_name: `inspect`,
`create`, `join`, `sort`, `makeUnordered`, `atomize`, `concatenate`,
`exportBip174`, `importBip174`, `pay`, `confirm`, `payments`, `localSync`).
Structured requests are snake_case JSON objects (built by the adapter, same
mapping as HttpBackend's POST bodies); responses are the same DTO shapes the
webgui returns; a thrown `JsError` is rewrapped as `PtjBackendError`. The
frontend cannot tell it is talking to wasm instead of a server.

## `/api/sync` in the PWA

webgui's `/api/sync` does a REAL local `join_psbts` fold (Layer 2) and then, only
with an `iroh_ticket`, a feature-gated iroh sync (Layer 3). In the PWA:

- **Layer 2 (local fold)** is real: `concurrent-psbt-wasm`'s `localSync` export
  runs `join_psbts` over `psbts[]` and returns the merged `SyncResponse`
  (matching webgui's local/no-ticket branch). This is the always-available,
  offline path — `Backend.syncPsbts` with no transport injected is exactly this.
- **Layer 3 (network)** does NOT go through `/api/sync`'s `iroh_ticket` (iroh is
  not browser-viable). Instead, the PWA's network transports (WebRTC / nostr /
  OHTTP mailbox) push received PSBTs into the shared array, and the frontend calls
  the local `sync`/`join` on every update — the "shared array + join on every
  update" model. So the transport layer, not the sync endpoint, carries the
  network. `iroh_ticket`/`iroh_wait_ms` fields are simply absent/ignored in the
  PWA; the DTO is unchanged so no frontend edit is needed.

This keeps the DTO stable across shells while routing network egress through the
opt-in browser transports rather than a sync-endpoint feature gate.

## Data flow: a received PSBT

```
transport.recv()  →  bytes  →  shared PSBT array  →  frontend calls backend.syncPsbts/joinPsbts
        │                                                        │
   (WebRTC / nostr / OHTTP mailbox / QR / paste)   concurrent-psbt-wasm localSync/join  (WASM, local)
                                                                 │
                                                          merged PsbtResponse → render
```

No bytes for the JOIN ever leave the device; only the transport moves peers'
PSBTs in, and only if the user opted into a network transport. Sneakernet moves
them with no network at all.
