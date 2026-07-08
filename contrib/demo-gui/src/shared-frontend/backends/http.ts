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

import type { Backend } from "../core/backend.js";
import {
  type AssignIdsOptions,
  type AtomizeResponse,
  type ConfirmationRecord,
  type ConfirmOptions,
  type CreatePsbtRequest,
  type ExportBip174Response,
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

// FetchLike/FetchResponse are an IMPLEMENTATION DETAIL of this HTTP adapter
// (an injectable fetch for tests / non-window environments). They are NOT a
// backend abstraction — the seam every consumer programs against is the
// Backend interface in ../core/backend.ts. (The old FetchLike path-dispatch
// backend variant in pwa/src/backend/ is retired.)
export interface FetchResponse {
  ok: boolean;
  status: number;
  json(): Promise<unknown>;
}

export type FetchLike = (
  path: string,
  init: {
    method: "POST";
    headers: { "content-type": "application/json" };
    body: string;
  },
) => Promise<FetchResponse>;

function errorMessage(status: number, payload: unknown): string {
  return isErrorPayload(payload) ? payload.error : `ptj backend request failed with HTTP ${status}`;
}

function isErrorPayload(payload: unknown): payload is { error: string } {
  return typeof payload === "object"
    && payload !== null
    && "error" in payload
    && typeof payload.error === "string";
}

export class HttpBackend implements Backend {
  private readonly fetchImpl: FetchLike;

  // Base path lets the PWA/tauri point at a same-origin dev server if desired;
  // webgui uses "" (the served bundle's own origin), matching current behavior.
  private readonly base: string;

  constructor(fetchImpl?: FetchLike, base = "") {
    // Default to window.fetch so the webgui shell needs no wiring; the frontend
    // no longer binds window.fetch itself (that leak is removed from app.ts).
    this.fetchImpl = fetchImpl ?? (globalThis.fetch.bind(globalThis) as FetchLike);
    this.base = base;
  }

  private async postJson<T>(path: string, body: unknown): Promise<T> {
    const response = await this.fetchImpl(`${this.base}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    const payload = await response.json();
    if (!response.ok) {
      throw new PtjBackendError(response.status, errorMessage(response.status, payload));
    }
    return payload as T;
  }

  inspectPsbt(psbt: string): Promise<InspectResponse> {
    return this.postJson("/api/inspect", { psbt });
  }

  createPsbt(request: CreatePsbtRequest): Promise<PsbtResponse> {
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

  joinPsbts(psbts: string[]): Promise<PsbtResponse> {
    return this.postJson("/api/join", { psbts });
  }

  sortPsbt(psbt: string, seedHex?: string, allowShortSeed?: boolean): Promise<PsbtResponse> {
    return this.postJson("/api/sort", { psbt, seed_hex: seedHex, allow_short_seed: allowShortSeed });
  }

  makeUnordered(psbt: string): Promise<PsbtResponse> {
    return this.postJson("/api/make-unordered", { psbt });
  }

  atomizePsbt(psbt: string): Promise<AtomizeResponse> {
    return this.postJson("/api/atomize", { psbt });
  }

  concatenatePsbts(psbts: string[]): Promise<PsbtResponse> {
    return this.postJson("/api/concatenate", { psbts });
  }

  exportBip174(psbt: string): Promise<ExportBip174Response> {
    return this.postJson("/api/export-bip174", { psbt });
  }

  importBip174(psbt: string, modifiable?: boolean): Promise<PsbtResponse> {
    return this.postJson("/api/import-bip174", { psbt, modifiable });
  }

  assignIds(psbt: string, options?: AssignIdsOptions): Promise<PsbtResponse> {
    return this.postJson("/api/assign-ids", {
      psbt,
      ids: options?.ids,
      auto: options?.auto,
      overwrite: options?.overwrite,
    });
  }

  // Negotiation band: served by the webgui's /api/{pay,confirm,payments}
  // routes (crates/ptj/src/webgui.rs pay_response/confirm_response/
  // payments_response). Opaque records pass through unchanged (wasm parity);
  // the PayByAddress / DeriveConfirmation variants map onto the routes'
  // build-it-server-side request shapes.
  pay(psbt: string, payment: PaymentRecord, options?: PayOptions): Promise<PsbtResponse> {
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
    return this.postJson("/api/pay", {
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
    return this.postJson("/api/confirm", {
      psbt,
      ...record,
      secret_hex: options?.secretHex,
    });
  }

  payments(psbt: string, options?: PaymentsOptions): Promise<PaymentsResponse> {
    return this.postJson("/api/payments", {
      psbt,
      secret_hex: options?.secretHex,
    });
  }

  async syncPsbts(request: SyncRequest): Promise<SyncResponse> {
    const raw = await this.postJson<SyncResponse & { iroh_ticket_out?: string }>("/api/sync", {
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
