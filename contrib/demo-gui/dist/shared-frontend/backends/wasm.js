// contrib/demo-gui/src/shared-frontend/backends/wasm.ts
//
// WasmBackend — the PWA adapter (NO server).
//
// Implements the same Backend interface by calling concurrent-psbt compiled to
// wasm32-unknown-unknown via a thin wasm-bindgen wrapper crate (the PWA-facing
// crate the Feasibility agent identified: concurrent-psbt itself needs no
// wasm-bindgen; the wrapper does — it emits the JS glue that resolves the
// __wbindgen_placeholder__ imports and the getRandomValues backend). Layer-1
// ops run fully in-browser. Layer-2 (the lattice fold in sync) also runs
// in-browser. Layer-3 (network) is delegated to an injected browser transport
// (nostr / webrtc-over-payjoin-directory) — the transport is optional; with
// none injected, sync degenerates to the local join, exactly like the webgui
// no-ticket branch (webgui.rs:246-248).
//
// The wrapper crate is `concurrent-psbt-wasm` (staged in the wasm-bindgen
// component; integrates as crates/concurrent-psbt-wasm). PtjWasmModule below is
// the exact export surface that crate emits (js_name camelCase); the two are
// cross-checked by the export table in the crate's README.
import { PtjBackendError, } from "../core/types.js";
function wrap(fn) {
    try {
        return fn();
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        // status 0 => "not an HTTP failure"; app.ts only checks instanceof.
        throw new PtjBackendError(0, msg);
    }
}
const WASM_NO_FAKER = "WasmBackend does not support the fake test-data generators yet; use a " +
    "server backend (ptj webgui /api/fake/*)";
export class WasmBackend {
    m;
    // `| undefined` (not `?`): the constructor assigns a possibly-undefined
    // option, which exactOptionalPropertyTypes distinguishes from absence.
    transport;
    defaultWaitMs;
    constructor(module, options = {}) {
        this.m = module;
        this.transport = options.transport;
        this.defaultWaitMs = options.defaultWaitMs ?? 5000;
    }
    async inspectPsbt(psbt) {
        return wrap(() => this.m.inspect(psbt));
    }
    async createPsbt(request) {
        // Map camelCase DTO -> the snake_case JSON the wrapper parses, identical to
        // HttpBackend.createPsbt so both backends feed concurrent-psbt the same shape.
        return wrap(() => this.m.create({
            network: request.network,
            ordering: request.ordering,
            seed_hex: request.seedHex,
            allow_short_seed: request.allowShortSeed,
            inputs: request.inputs.map((input) => ({
                txid: input.txid,
                vout: input.vout,
                raw_tx: input.rawTxHex,
            })),
            outputs: request.outputs.map((output) => ({
                address: output.address,
                amount_btc: output.amountBtc,
            })),
        }));
    }
    async joinPsbts(psbts) {
        return wrap(() => this.m.join(psbts));
    }
    async sortPsbt(psbt, seedHex, allowShortSeed) {
        return wrap(() => this.m.sort(psbt, seedHex, allowShortSeed));
    }
    async makeUnordered(psbt) {
        return wrap(() => this.m.makeUnordered(psbt));
    }
    async atomizePsbt(psbt) {
        return wrap(() => this.m.atomize(psbt));
    }
    async concatenatePsbts(psbts) {
        return wrap(() => this.m.concatenate(psbts));
    }
    async exportBip174(psbt) {
        return wrap(() => this.m.exportBip174(psbt));
    }
    async importBip174(psbt, modifiable) {
        return wrap(() => this.m.importBip174(psbt, modifiable));
    }
    async assignIds(psbt, options) {
        return wrap(() => this.m.assignIds({
            psbt,
            ids: options?.ids,
            auto: options?.auto,
            overwrite: options?.overwrite,
        }));
    }
    async applyPsbtEdits(_psbt, _edits, _options) {
        // Field-level editing needs the server-side field_edit machinery
        // (raw-keymap surgery + save-time validation + fix offers); the wasm
        // wrapper exports no edit op yet. Reject clearly — the transport-skeleton
        // "built without support" pattern — rather than half-implement it here.
        throw new PtjBackendError(0, "WasmBackend does not support field edits yet; use a server backend " +
            "(ptj webgui /api/edit)");
    }
    async classifyPaste(_payload, _network) {
        // Deep classification leans on miniscript + bitcoin-payment-instructions
        // server-side; the wasm wrapper exports no classify op yet. The shallow
        // paste router still classifies locally — this seam only ENRICHES — so
        // rejecting degrades gracefully to the shallow card.
        throw new PtjBackendError(0, "WasmBackend does not support deep paste classification yet; use a " +
            "server backend (ptj webgui /api/classify)");
    }
    // The fake test-data generators lean on miniscript + bip32 key derivation
    // server-side (ptj commands::faker); the wasm wrapper exports none of it.
    // Reject clearly — the transport-skeleton "built without support" pattern.
    async fakeDescriptor() {
        throw new PtjBackendError(0, WASM_NO_FAKER);
    }
    async fakeUtxos() {
        throw new PtjBackendError(0, WASM_NO_FAKER);
    }
    async fakePsbt() {
        throw new PtjBackendError(0, WASM_NO_FAKER);
    }
    // --- negotiation band (opaque hex records; snake_case wire fields) ---
    async pay(psbt, payment, options) {
        // The wasm wrapper only appends OPAQUE records; the PayByAddress variant
        // needs a server-side builder (webgui /api/pay). Reject it clearly rather
        // than parse addresses here — the transport-skeleton "built without
        // support" pattern.
        if (typeof payment !== "string") {
            throw new PtjBackendError(0, "WasmBackend cannot build a payment record from an address; " +
                "pass an opaque payment hex or use a server backend");
        }
        return wrap(() => this.m.pay({
            psbt,
            payment_hex: payment,
            secret_hex: options?.secretHex,
            dummy: options?.dummy ?? 0,
        }));
    }
    async confirm(psbt, confirmation, options) {
        // Same shape as pay: DeriveConfirmation needs the server-side builder
        // (webgui /api/confirm with derive: true).
        if (typeof confirmation !== "string") {
            throw new PtjBackendError(0, "WasmBackend cannot derive a confirmation record; " +
                "pass an opaque confirmation hex or use a server backend");
        }
        return wrap(() => this.m.confirm({
            psbt,
            confirmation_hex: confirmation,
            secret_hex: options?.secretHex,
        }));
    }
    async payments(psbt, options) {
        return wrap(() => this.m.payments({ psbt, secret_hex: options?.secretHex }));
    }
    async syncPsbts(request) {
        const local = request.psbts ?? [];
        // No transport -> pure local fold (PWA offline / single-device). Same as
        // the webgui no-ticket branch: just return the locally-joined PSBT.
        if (!this.transport) {
            return wrap(() => this.m.localSync(local));
        }
        // Layer-3: publish ours, wait, collect peers', then fold everything locally.
        // The fold is the source of truth (idempotent/commutative/associative
        // lattice join); the transport only moves opaque frames.
        await this.transport.publish(local);
        const waitMs = request.irohWaitMs ?? this.defaultWaitMs;
        await new Promise((resolve) => setTimeout(resolve, waitMs));
        const seen = await this.transport.collect();
        // collect() includes our own sends per the snapshot contract; de-dup defensively.
        const all = Array.from(new Set([...local, ...seen]));
        return wrap(() => this.m.localSync(all));
    }
}
