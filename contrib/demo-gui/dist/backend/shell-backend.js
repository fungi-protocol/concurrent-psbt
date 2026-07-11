// shell-backend — resolves the Backend for the current shell.
//
// This is the single injection point that replaces app.ts's hard-wired
// `window.fetch.bind(window)` (the "leak that blocks reuse"). The shared
// frontend boot calls makeBackend() once and hands the ONE `Backend` instance
// to app.ts (see shared-frontend/app-wiring.md).
//
// Reconciliation note (2026-07-06): this used to hand out a FetchLike; that
// path-dispatch backend variant is retired. All three branches now return the
// canonical `Backend` (shared-frontend/core/backend.ts).
import { HttpBackend } from "../shared-frontend/backends/http.js";
import { TauriBackend } from "../shared-frontend/backends/tauri.js";
import { makeWasmBackend } from "./wasm-backend.js";
export async function makeBackend() {
    const build = typeof PTJ_BUILD === "string" ? PTJ_BUILD : "pwa";
    switch (build) {
        case "pwa":
            // No server: PSBT ops run locally in concurrent-psbt-wasm. LOCAL-FIRST —
            // no transport is injected here; opt-in transports are wired by the
            // shell UI later (see ../transport/registry.ts).
            return makeWasmBackend();
        case "webgui":
            // The existing HTTP path: POST to the ptj webgui /api/* routes.
            // HttpBackend defaults its fetch impl to globalThis.fetch.
            return new HttpBackend();
        case "tauri":
            // Stub until the tauri command handlers exist; every op rejects with a
            // clear PtjBackendError (see shared-frontend/backends/tauri.ts).
            return new TauriBackend();
        default:
            return makeWasmBackend();
    }
}
