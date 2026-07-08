// contrib/demo-gui/src/shared-frontend/core/types.ts
//
// Shared frontend core — DTOs and shared types.
//
// These are lifted VERBATIM from contrib/demo-gui/src/backend.ts so that the
// HttpBackend (webgui), WasmBackend (PWA), and TauriBackend (future) all speak
// the identical request/response shapes. The frontend logic (app/model) imports
// ONLY from this file and ./backend.ts — never from a concrete adapter.
//
// Provenance: request/response DTOs match the ptj webgui *_response_result JSON
// contract in crates/ptj/src/webgui.rs (POST /api/{inspect,create,join,sort,
// make-unordered,atomize,concatenate,export-bip174,import-bip174,assign-ids,
// edit,sync}) and the concurrent-psbt command set in
// crates/ptj/src/commands/*.rs.
// The ONE error type every backend throws. HttpBackend maps HTTP status +
// {error} body onto it; WasmBackend maps a caught JS/wasm error (status 0);
// TauriBackend maps a rejected invoke() (status 0). Frontend `instanceof`
// checks in app.ts keep working unchanged.
export class PtjBackendError extends Error {
    status;
    constructor(status, message) {
        super(message);
        this.name = "PtjBackendError";
        this.status = status;
        Object.setPrototypeOf(this, PtjBackendError.prototype);
    }
}
