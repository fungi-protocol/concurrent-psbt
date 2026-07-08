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
function isViolationPayload(payload) {
    return typeof payload === "object"
        && payload !== null
        && "violations" in payload
        && Array.isArray(payload.violations);
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
            allow_short_seed: request.allowShortSeed,
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
    sortPsbt(psbt, seedHex, allowShortSeed) {
        return this.postJson("/api/sort", { psbt, seed_hex: seedHex, allow_short_seed: allowShortSeed });
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
    importBip174(psbt, modifiable) {
        return this.postJson("/api/import-bip174", { psbt, modifiable });
    }
    assignIds(psbt, options) {
        return this.postJson("/api/assign-ids", {
            psbt,
            ids: options?.ids,
            auto: options?.auto,
            overwrite: options?.overwrite,
        });
    }
    async applyPsbtEdits(psbt, edits, options) {
        const body = {
            psbt,
            edits: edits.map((edit) => ({ map: edit.map, key: edit.key, value: edit.value })),
        };
        if (options?.applyFixes?.length) {
            body.apply_fixes = options.applyFixes;
        }
        // Overrides are TOP-LEVEL named boolean params (the route's
        // allow_short_seed convention): each violation names its own.
        for (const param of options?.overrides ?? []) {
            body[param] = true;
        }
        const response = await this.fetchImpl(`${this.base}/api/edit`, {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify(body),
        });
        const payload = await response.json();
        if (!response.ok) {
            // A 400 carrying violations[] is the seam's structured validation
            // outcome (violation -> fix -> revalidate), not a transport error.
            if (isViolationPayload(payload)) {
                return payload;
            }
            throw new PtjBackendError(response.status, errorMessage(response.status, payload));
        }
        return payload;
    }
    classifyPaste(payload, network) {
        return this.postJson("/api/classify", { payload, network });
    }
    // Negotiation band: served by the webgui's /api/{pay,confirm,payments}
    // routes (crates/ptj/src/webgui.rs pay_response/confirm_response/
    // payments_response). Opaque records pass through unchanged (wasm parity);
    // the PayByAddress / DeriveConfirmation variants map onto the routes'
    // build-it-server-side request shapes.
    pay(psbt, payment, options) {
        const record = typeof payment === "string"
            ? { payment_hex: payment }
            : {
                address: payment.address,
                amount_btc: payment.amountBtc,
                network: payment.network,
                label: payment.label,
                payer_hex: payment.payerHex,
            };
        return this.postJson("/api/pay", {
            psbt,
            ...record,
            secret_hex: options?.secretHex,
            dummy: options?.dummy ?? 0,
        });
    }
    confirm(psbt, confirmation, options) {
        const record = typeof confirmation === "string"
            ? { confirmation_hex: confirmation }
            : { derive: true, peer_id_hex: confirmation.peerIdHex };
        return this.postJson("/api/confirm", {
            psbt,
            ...record,
            secret_hex: options?.secretHex,
        });
    }
    payments(psbt, options) {
        return this.postJson("/api/payments", {
            psbt,
            secret_hex: options?.secretHex,
        });
    }
    async syncPsbts(request) {
        const raw = await this.postJson("/api/sync", {
            psbts: request.psbts,
            transport: request.transport,
            sources: request.sources,
            state: request.state,
            iroh_ticket: request.irohTicket,
            iroh_ticket_out: request.irohTicketOut,
            iroh_wait_ms: request.irohWaitMs,
            webrtc_role: request.webrtcRole,
            signal_out: request.signalOut,
            signal_in: request.signalIn,
            webrtc_bind: request.webrtcBind,
            ice_servers: request.iceServers,
            signal_timeout_ms: request.signalTimeoutMs,
        });
        // The one snake_case response field that needs camelCase surfacing.
        if (raw.iroh_ticket_out !== undefined && raw.irohTicketOut === undefined) {
            raw.irohTicketOut = raw.iroh_ticket_out;
        }
        return raw;
    }
}
