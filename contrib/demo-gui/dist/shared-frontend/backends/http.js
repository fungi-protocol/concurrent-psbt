// contrib/demo-gui/src/shared-frontend/backends/http.ts
//
// HttpBackend — the webgui adapter.
//
// This is the drop-in replacement for the CURRENT behavior: it wraps the
// existing postJson-over-fetch client from contrib/demo-gui/src/backend.ts and
// implements the Backend interface. It POSTs JSON to the ptj webgui /api/*
// routes (crates/ptj/src/webgui.rs response_for @57). Behavior — path names,
// snake_case field mapping, {error} -> PtjBackendError(status,msg) — is
// preserved BYTE-FOR-BYTE from the original free functions so the webgui shell
// is a no-op refactor.
import { PtjBackendError, } from "../core/types.js";
function errorMessage(status, payload) {
    return isErrorPayload(payload) ? payload.error : `ptj backend request failed with HTTP ${status}`;
}
function isErrorPayload(payload) {
    return typeof payload === "object"
        && payload !== null
        && "error" in payload
        && typeof payload.error === "string";
}
export class HttpBackend {
    fetchImpl;
    // Base path lets the PWA/tauri point at a same-origin dev server if desired;
    // webgui uses "" (the served bundle's own origin), matching current behavior.
    base;
    constructor(fetchImpl, base = "") {
        // Default to window.fetch so the webgui shell needs no wiring; the frontend
        // no longer binds window.fetch itself (that leak is removed from app.ts).
        this.fetchImpl = fetchImpl ?? globalThis.fetch.bind(globalThis);
        this.base = base;
    }
    async postJson(path, body) {
        const response = await this.fetchImpl(`${this.base}${path}`, {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify(body),
        });
        const payload = await response.json();
        if (!response.ok) {
            throw new PtjBackendError(response.status, errorMessage(response.status, payload));
        }
        return payload;
    }
    inspectPsbt(psbt) {
        return this.postJson("/api/inspect", { psbt });
    }
    createPsbt(request) {
        return this.postJson("/api/create", {
            network: request.network,
            ordering: request.ordering,
            seed_hex: request.seedHex,
            inputs: request.inputs,
            outputs: request.outputs.map((output) => ({
                address: output.address,
                amount_btc: output.amountBtc,
            })),
        });
    }
    joinPsbts(psbts) {
        return this.postJson("/api/join", { psbts });
    }
    sortPsbt(psbt, seedHex) {
        return this.postJson("/api/sort", { psbt, seed_hex: seedHex });
    }
    makeUnordered(psbt) {
        return this.postJson("/api/make-unordered", { psbt });
    }
    atomizePsbt(psbt) {
        return this.postJson("/api/atomize", { psbt });
    }
    concatenatePsbts(psbts) {
        return this.postJson("/api/concatenate", { psbts });
    }
    exportBip174(psbt) {
        return this.postJson("/api/export-bip174", { psbt });
    }
    importBip174(psbt) {
        return this.postJson("/api/import-bip174", { psbt });
    }
    // Negotiation band: served by the webgui's /api/{pay,confirm,payments}
    // routes (crates/ptj/src/webgui.rs pay_response/confirm_response/
    // payments_response), which append/decode the same opaque records as the
    // wasm surface.
    pay(psbt, paymentHex, options) {
        return this.postJson("/api/pay", {
            psbt,
            payment_hex: paymentHex,
            secret_hex: options?.secretHex,
            dummy: options?.dummy ?? 0,
        });
    }
    confirm(psbt, confirmationHex, options) {
        return this.postJson("/api/confirm", {
            psbt,
            confirmation_hex: confirmationHex,
            secret_hex: options?.secretHex,
        });
    }
    payments(psbt, options) {
        return this.postJson("/api/payments", {
            psbt,
            secret_hex: options?.secretHex,
        });
    }
    syncPsbts(request) {
        return this.postJson("/api/sync", {
            psbts: request.psbts,
            iroh_ticket: request.irohTicket,
            iroh_wait_ms: request.irohWaitMs,
        });
    }
}
