// TARGET (integrated): contrib/pwa/src/backend/wasm-backend.ts
//
// PWA backend construction — a thin bootstrap over the SHARED WasmBackend.
//
// Reconciliation note (2026-07-06): this file previously defined a FetchLike
// path-dispatch ("/api/*" -> wasm export) as its own backend abstraction. That
// contract is RETIRED — the one seam is the `Backend` interface in
// shared-frontend/core/backend.ts, and the one WASM adapter is
// shared-frontend/backends/wasm.ts. This file only (1) lazily loads the
// concurrent-psbt-wasm module and (2) news up the shared WasmBackend with an
// optional BrowserTransport.
//
// LOCAL-FIRST: with no transport injected, every op (including syncPsbts, via
// the localSync export) runs fully in-browser with no server. Networked
// transports (payjoin-dir/OHTTP mailbox, WebRTC, nostr) are explicit opt-in —
// the shell passes one in only when the user enables it (see ../transport/).
import { WasmBackend, } from "../shared-frontend/backends/wasm.js";
import { loadWasm } from "./wasm-loader.js";
let wasmPromise = null;
/**
 * Build the PWA's Backend. The wasm module is lazily initialized on first call
 * (and cached) so the app shell paints before the (larger) wasm binary loads.
 */
export async function makeWasmBackend(options = {}) {
    if (wasmPromise === null) {
        wasmPromise = loadWasm(options.debug ?? false);
    }
    const module = await wasmPromise;
    // Conditional spread: with exactOptionalPropertyTypes, an explicit
    // `transport: undefined` is not the same as an absent property.
    return new WasmBackend(module, {
        ...(options.transport !== undefined ? { transport: options.transport } : {}),
        ...(options.defaultWaitMs !== undefined ? { defaultWaitMs: options.defaultWaitMs } : {}),
    });
}
