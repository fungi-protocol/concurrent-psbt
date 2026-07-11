import type { Backend } from "../shared-frontend/core/backend.js";
import { type BrowserTransport } from "../shared-frontend/backends/wasm.js";
export interface PwaBackendOptions {
    transport?: BrowserTransport;
    defaultWaitMs?: number;
    debug?: boolean;
}
/**
 * Build the PWA's Backend. The wasm module is lazily initialized on first call
 * (and cached) so the app shell paints before the (larger) wasm binary loads.
 */
export declare function makeWasmBackend(options?: PwaBackendOptions): Promise<Backend>;
