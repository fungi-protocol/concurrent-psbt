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

// TARGET (integrated): import from the shared frontend package/path.
import type { Backend } from "../shared-frontend/core/backend.js";
import {
  WasmBackend,
  type BrowserTransport,
  type PtjWasmModule,
} from "../shared-frontend/backends/wasm.js";
import { loadWasm } from "./wasm-loader.js";

export interface PwaBackendOptions {
  // Opt-in Layer-3 transport; omit for the pure offline PWA (the default).
  transport?: BrowserTransport;
  // Gather window before folding when a transport is present (default 5000ms).
  defaultWaitMs?: number;
  // Wire Rust panics to the console (debug builds).
  debug?: boolean;
}

let wasmPromise: Promise<PtjWasmModule> | null = null;

/**
 * Build the PWA's Backend. The wasm module is lazily initialized on first call
 * (and cached) so the app shell paints before the (larger) wasm binary loads.
 */
export async function makeWasmBackend(options: PwaBackendOptions = {}): Promise<Backend> {
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
