// contrib/demo-gui/src/shared-frontend/index.ts
//
// Shared frontend core — public entry point.
//
// The three shells import from here. The frontend logic (app/model) imports the
// Backend interface + DTOs only; each shell's bootstrap picks ONE concrete
// backend and hands it to the app.
//
//   webgui  -> new HttpBackend()                       (fetch /api/*)
//   PWA     -> new WasmBackend(await loadWasm(), {transport?})  (see pwa/src/backend/)
//   tauri   -> new TauriBackend({ invoke })            (stub today)
//
// This is the single wiring point that replaces app.ts's 7 hard-bound
// window.fetch.bind(window) call sites.
export { PtjBackendError, } from "./core/types.js";
export { HttpBackend } from "./backends/http.js";
export { WasmBackend, } from "./backends/wasm.js";
export { TauriBackend } from "./backends/tauri.js";
