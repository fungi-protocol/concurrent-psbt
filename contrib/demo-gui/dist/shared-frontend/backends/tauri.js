// contrib/demo-gui/src/shared-frontend/backends/tauri.ts
//
// TauriBackend — the future desktop adapter (STUB).
//
// The tauri shell will run the SAME frontend core with a backend that bridges
// to native ptj / concurrent-psbt over tauri's IPC (`invoke()`), giving the
// desktop app access to the heavy native transports (iroh/arti/nym/emissary/mdk)
// that are not browser-viable. This is intentionally a stub: it satisfies the
// Backend interface so the type-checker and the shell wiring are exercised, but
// every op rejects with a clear "not yet implemented" PtjBackendError until the
// tauri command handlers exist. This is the tauri analog of the webgui
// iroh-sync feature gate and the PWA wasm wrapper: authored seam, deferred impl.
import { PtjBackendError, } from "../core/types.js";
const UNIMPLEMENTED = "TauriBackend is a stub; the tauri shell command handlers are not implemented yet " +
    "(TODO(ground-tauri): wire invoke() to native ptj/concurrent-psbt commands).";
export class TauriBackend {
    // `| undefined` (not `?`): the constructor assigns a possibly-undefined
    // option, which exactOptionalPropertyTypes distinguishes from absence.
    invoke;
    constructor(options = {}) {
        this.invoke = options.invoke;
    }
    async call(cmd, args) {
        if (!this.invoke) {
            throw new PtjBackendError(0, UNIMPLEMENTED);
        }
        try {
            // Tauri command names mirror the ptj command set (snake_case). When the
            // handlers are authored they call crate::commands::* directly, returning
            // the same {psbt,inspect} DTOs the webgui and wasm paths return.
            return await this.invoke(cmd, args);
        }
        catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            throw new PtjBackendError(0, msg);
        }
    }
    inspectPsbt(psbt) {
        return this.call("ptj_inspect", { psbt });
    }
    createPsbt(request) {
        return this.call("ptj_create", {
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
        });
    }
    joinPsbts(psbts) {
        return this.call("ptj_join", { psbts });
    }
    sortPsbt(psbt, seedHex, allowShortSeed) {
        return this.call("ptj_sort", { psbt, seed_hex: seedHex, allow_short_seed: allowShortSeed });
    }
    makeUnordered(psbt) {
        return this.call("ptj_make_unordered", { psbt });
    }
    atomizePsbt(psbt) {
        return this.call("ptj_atomize", { psbt });
    }
    concatenatePsbts(psbts) {
        return this.call("ptj_concatenate", { psbts });
    }
    exportBip174(psbt) {
        return this.call("ptj_export_bip174", { psbt });
    }
    importBip174(psbt, modifiable) {
        return this.call("ptj_import_bip174", { psbt, modifiable });
    }
    assignIds(psbt, options) {
        return this.call("ptj_assign_ids", {
            psbt,
            ids: options?.ids,
            auto: options?.auto,
            overwrite: options?.overwrite,
        });
    }
    applyPsbtEdits(psbt, edits, options) {
        // Same request shape as the webgui /api/edit route: apply_fixes plus
        // top-level named override booleans. The future native handler shares
        // crate::commands::field_edit, so violations come back structured.
        const args = { psbt, edits };
        if (options?.applyFixes?.length) {
            args.apply_fixes = options.applyFixes;
        }
        for (const param of options?.overrides ?? []) {
            args[param] = true;
        }
        return this.call("ptj_edit", args);
    }
    classifyPaste(payload, network) {
        return this.call("ptj_classify", { payload, network });
    }
    fakeDescriptor(network, kind) {
        return this.call("ptj_fake_descriptor", { network, kind });
    }
    fakeUtxos(descriptor, network, count) {
        return this.call("ptj_fake_utxos", { descriptor, network, count });
    }
    fakePsbt(descriptor, utxos, network, recipients) {
        // Same wire mapping as HttpBackend: snake_case utxo refs, so the future
        // native handler parses the identical request shape as /api/fake/psbt.
        return this.call("ptj_fake_psbt", {
            descriptor,
            utxos: utxos.map((utxo) => ({
                txid: utxo.txid,
                vout: utxo.vout,
                amount_sats: utxo.amountSats,
            })),
            network,
            recipients,
        });
    }
    pay(psbt, payment, options) {
        // Same request-shape mapping as HttpBackend: the future native handler
        // shares the webgui route's builder (`ptj pay --to`).
        const record = typeof payment === "string"
            ? { payment_hex: payment }
            : {
                address: payment.address,
                amount_btc: payment.amountBtc,
                network: payment.network,
                label: payment.label,
                payer_hex: payment.payerHex,
            };
        return this.call("ptj_pay", {
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
        return this.call("ptj_confirm", {
            psbt,
            ...record,
            secret_hex: options?.secretHex,
        });
    }
    payments(psbt, options) {
        return this.call("ptj_payments", { psbt, secret_hex: options?.secretHex });
    }
    syncPsbts(request) {
        return this.call("ptj_sync", {
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
    }
}
