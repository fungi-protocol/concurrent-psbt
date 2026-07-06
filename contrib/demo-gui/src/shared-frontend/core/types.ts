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
// make-unordered,atomize,concatenate,export-bip174,import-bip174,sync}) and the
// concurrent-psbt command set in crates/ptj/src/commands/*.rs.

export type OrderingMode = "unset" | "deterministic" | "explicit";

export interface InspectResponse {
  [key: string]: unknown;
}

export interface PsbtResponse {
  psbt: string;
  inspect?: InspectResponse;
}

export interface AtomizeResponse {
  fragments: PsbtResponse[];
}

export interface ExportBip174Response {
  format: "bip174";
  psbt: string;
}

export interface CreateInput {
  txid: string;
  vout: number;
}

export interface CreateOutput {
  address: string;
  amountBtc: string;
}

export interface CreatePsbtRequest {
  network: string;
  ordering: OrderingMode;
  seedHex?: string;
  inputs: CreateInput[];
  outputs: CreateOutput[];
}

export interface SyncRequest {
  psbts?: string[];
  // Transport selection. The webgui path only ever wires the iroh ticket
  // (feature-gated in ptj); the PWA path substitutes a browser-viable transport
  // handle (nostr / webrtc-over-payjoin-directory), still opaque to this seam.
  irohTicket?: string;
  irohWaitMs?: number;
}

export interface SyncResponse {
  psbt: string;
  inspect?: InspectResponse;
  payments: string[];
  confirmations: string[];
}

// Negotiation-band options/DTOs (Backend.pay / Backend.confirm / Backend.payments).
// The records are OPAQUE hex blobs — the frontend builds them, the backend only
// appends/decodes (mechanism-only, matching `ptj pay/confirm/payments`).
export interface PayOptions {
  // Opt-in deterministic AEAD encryption of the record (ptj `--encrypt`).
  secretHex?: string;
  // Number of random dummy records appended alongside (requires secretHex).
  dummy?: number;
}

export interface ConfirmOptions {
  secretHex?: string;
}

export interface PaymentsOptions {
  secretHex?: string;
}

export interface PaymentsResponse {
  payments: string[];
  confirmations: string[];
}

// The ONE error type every backend throws. HttpBackend maps HTTP status +
// {error} body onto it; WasmBackend maps a caught JS/wasm error (status 0);
// TauriBackend maps a rejected invoke() (status 0). Frontend `instanceof`
// checks in app.ts keep working unchanged.
export class PtjBackendError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "PtjBackendError";
    this.status = status;
    Object.setPrototypeOf(this, PtjBackendError.prototype);
  }
}
