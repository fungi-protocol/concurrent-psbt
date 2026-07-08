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

import type { Backend } from "../core/backend.js";
import {
  type ApplyEditsOptions,
  type ApplyEditsResponse,
  type AssignIdsOptions,
  type AtomizeResponse,
  type ConfirmationRecord,
  type ConfirmOptions,
  type CreatePsbtRequest,
  type ExportBip174Response,
  type FieldEdit,
  type InspectResponse,
  type PaymentRecord,
  type PayOptions,
  type PaymentsOptions,
  type PaymentsResponse,
  type PsbtResponse,
  PtjBackendError,
  type SyncRequest,
  type SyncResponse,
} from "../core/types.js";

// The tauri IPC bridge. At runtime this is `@tauri-apps/api/core`'s `invoke`;
// typed minimally here so the stub compiles without the tauri dep.
export type TauriInvoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

const UNIMPLEMENTED =
  "TauriBackend is a stub; the tauri shell command handlers are not implemented yet " +
  "(TODO(ground-tauri): wire invoke() to native ptj/concurrent-psbt commands).";

export interface TauriBackendOptions {
  // Inject the real `invoke` when the tauri shell is built. When absent, every
  // op rejects with UNIMPLEMENTED so the stub is safe to load in any shell.
  invoke?: TauriInvoke;
}

export class TauriBackend implements Backend {
  private readonly invoke?: TauriInvoke;

  constructor(options: TauriBackendOptions = {}) {
    this.invoke = options.invoke;
  }

  private async call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
    if (!this.invoke) {
      throw new PtjBackendError(0, UNIMPLEMENTED);
    }
    try {
      // Tauri command names mirror the ptj command set (snake_case). When the
      // handlers are authored they call crate::commands::* directly, returning
      // the same {psbt,inspect} DTOs the webgui and wasm paths return.
      return await this.invoke<T>(cmd, args);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      throw new PtjBackendError(0, msg);
    }
  }

  inspectPsbt(psbt: string): Promise<InspectResponse> {
    return this.call("ptj_inspect", { psbt });
  }

  createPsbt(request: CreatePsbtRequest): Promise<PsbtResponse> {
    return this.call("ptj_create", {
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

  joinPsbts(psbts: string[]): Promise<PsbtResponse> {
    return this.call("ptj_join", { psbts });
  }

  sortPsbt(psbt: string, seedHex?: string, allowShortSeed?: boolean): Promise<PsbtResponse> {
    return this.call("ptj_sort", { psbt, seed_hex: seedHex, allow_short_seed: allowShortSeed });
  }

  makeUnordered(psbt: string): Promise<PsbtResponse> {
    return this.call("ptj_make_unordered", { psbt });
  }

  atomizePsbt(psbt: string): Promise<AtomizeResponse> {
    return this.call("ptj_atomize", { psbt });
  }

  concatenatePsbts(psbts: string[]): Promise<PsbtResponse> {
    return this.call("ptj_concatenate", { psbts });
  }

  exportBip174(psbt: string): Promise<ExportBip174Response> {
    return this.call("ptj_export_bip174", { psbt });
  }

  importBip174(psbt: string, modifiable?: boolean): Promise<PsbtResponse> {
    return this.call("ptj_import_bip174", { psbt, modifiable });
  }

  assignIds(psbt: string, options?: AssignIdsOptions): Promise<PsbtResponse> {
    return this.call("ptj_assign_ids", {
      psbt,
      ids: options?.ids,
      auto: options?.auto,
      overwrite: options?.overwrite,
    });
  }

  applyPsbtEdits(
    psbt: string,
    edits: FieldEdit[],
    options?: ApplyEditsOptions,
  ): Promise<ApplyEditsResponse> {
    // Same request shape as the webgui /api/edit route: apply_fixes plus
    // top-level named override booleans. The future native handler shares
    // crate::commands::field_edit, so violations come back structured.
    const args: Record<string, unknown> = { psbt, edits };
    if (options?.applyFixes?.length) {
      args.apply_fixes = options.applyFixes;
    }
    for (const param of options?.overrides ?? []) {
      args[param] = true;
    }
    return this.call("ptj_edit", args);
  }

  pay(psbt: string, payment: PaymentRecord, options?: PayOptions): Promise<PsbtResponse> {
    // Same request-shape mapping as HttpBackend: the future native handler
    // shares the webgui route's builder (`ptj pay --to`).
    const record =
      typeof payment === "string"
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

  confirm(
    psbt: string,
    confirmation: ConfirmationRecord,
    options?: ConfirmOptions,
  ): Promise<PsbtResponse> {
    const record =
      typeof confirmation === "string"
        ? { confirmation_hex: confirmation }
        : { derive: true, peer_id_hex: confirmation.peerIdHex };
    return this.call("ptj_confirm", {
      psbt,
      ...record,
      secret_hex: options?.secretHex,
    });
  }

  payments(psbt: string, options?: PaymentsOptions): Promise<PaymentsResponse> {
    return this.call("ptj_payments", { psbt, secret_hex: options?.secretHex });
  }

  syncPsbts(request: SyncRequest): Promise<SyncResponse> {
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
