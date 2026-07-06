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

export type { Backend } from "./core/backend.js";
export {
  type AtomizeResponse,
  type ConfirmOptions,
  type CreateInput,
  type CreateOutput,
  type CreatePsbtRequest,
  type ExportBip174Response,
  type InspectResponse,
  type OrderingMode,
  type PayOptions,
  type PaymentsOptions,
  type PaymentsResponse,
  type PsbtResponse,
  PtjBackendError,
  type SyncRequest,
  type SyncResponse,
} from "./core/types.js";

export { HttpBackend, type FetchLike, type FetchResponse } from "./backends/http.js";
export {
  WasmBackend,
  type BrowserTransport,
  type PtjWasmModule,
  type WasmBackendOptions,
} from "./backends/wasm.js";
export { TauriBackend, type TauriBackendOptions, type TauriInvoke } from "./backends/tauri.js";
