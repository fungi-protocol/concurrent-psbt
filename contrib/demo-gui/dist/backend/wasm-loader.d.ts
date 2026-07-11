import type { PtjWasmModule } from "../shared-frontend/backends/wasm.js";
/**
 * Initialize the wasm module once and return its op surface. The caller caches
 * the returned promise so this runs at most once per page.
 */
export declare function loadWasm(debug?: boolean): Promise<PtjWasmModule>;
