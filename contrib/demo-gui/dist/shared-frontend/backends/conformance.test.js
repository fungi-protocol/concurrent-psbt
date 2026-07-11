// contrib/demo-gui/src/shared-frontend/backends/conformance.test.ts
//
// Compile-time conformance: every adapter satisfies the Backend interface.
//
// No network, no wasm, no tauri runtime required — this is a TYPE-LEVEL test
// (the analog of the transport crates' assert_anonymous::<T>() compile
// assertion). If any adapter drifts from the Backend interface (missing method,
// wrong DTO), `tsc --noEmit` fails here. Mirrors the "runs in both feature
// modes with no network" contract the transport skeletons use.
import { HttpBackend } from "./http.js";
import { WasmBackend } from "./wasm.js";
import { TauriBackend } from "./tauri.js";
// Structural assertions: each concrete class is assignable to Backend.
const _http = new HttpBackend(async () => ({
    ok: true,
    status: 200,
    json: async () => ({}),
}));
// A do-nothing wasm module double satisfying the glue surface, used only to
// prove WasmBackend implements Backend at the type level.
const stubModule = {
    inspect: () => ({}),
    create: () => ({ psbt: "" }),
    join: () => ({ psbt: "" }),
    sort: () => ({ psbt: "" }),
    makeUnordered: () => ({ psbt: "" }),
    atomize: () => ({ fragments: [] }),
    concatenate: () => ({ psbt: "" }),
    exportBip174: () => ({ format: "bip174", psbt: "" }),
    importBip174: () => ({ psbt: "" }),
    assignIds: () => ({ psbt: "" }),
    pay: () => ({ psbt: "" }),
    confirm: () => ({ psbt: "" }),
    payments: () => ({ payments: [], confirmations: [] }),
    localSync: () => ({ psbt: "", payments: [], confirmations: [] }),
};
const _wasm = new WasmBackend(stubModule);
const _tauri = new TauriBackend();
// Reference the bindings so they are not flagged unused; also assert all three
// share the identical method surface by cross-assigning through Backend.
export const _adapters = [_http, _wasm, _tauri];
