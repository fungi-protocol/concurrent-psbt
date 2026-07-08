// contrib/demo-gui/src/shared-frontend/core/backend.ts
//
// Shared frontend core — the Backend interface (the abstraction point).
//
// This REPLACES the free-function + FetchLike client in
// contrib/demo-gui/src/backend.ts. Previously every op was a standalone
// function whose first arg was an injected `FetchLike`, and app.ts defeated the
// injection by hard-binding `window.fetch.bind(window)` at 7 call sites
// (app.ts:686,714,732,751,774,878,900). Here the seam is promoted to an
// interface: one method per op, same DTOs. app.ts receives a single Backend
// instance at init and calls `backend.<op>(...)` — no fetch threading, no shell
// coupling. The three shells swap ONLY the implementation.

import type {
  AssignIdsOptions,
  AtomizeResponse,
  ConfirmOptions,
  ConfirmationRecord,
  CreatePsbtRequest,
  ExportBip174Response,
  InspectResponse,
  PayOptions,
  PaymentRecord,
  PaymentsOptions,
  PaymentsResponse,
  PsbtResponse,
  SyncRequest,
  SyncResponse,
} from "./types.js";

// THE canonical Backend interface (reconciled 2026-07-06). This is the ONE seam
// all shells implement; the PtjBackend interface (wasm-bindgen/frontend/
// backend.iface.ts) and the FetchLike path-dispatch contract (pwa/src/backend/)
// are RETIRED as backend definitions. FetchLike survives only as an
// implementation detail of HttpBackend (backends/http.ts).
//
// Naming rule: JS-facing identifiers are camelCase — Backend methods here, and
// the concurrent-psbt-wasm #[wasm_bindgen(js_name = ...)] exports they call
// (makeUnordered, exportBip174, localSync, ...). Wire-format JSON field names
// stay snake_case (seed_hex, amount_btc, iroh_ticket, payment_hex, ...) because
// they are the ptj webgui HTTP contract.
export interface Backend {
  // Layer-1 PSBT byte manipulation — pure, no network. Every shell implements
  // all of these (HTTP -> ptj /api/*; WASM -> concurrent-psbt calls; tauri ->
  // invoke()). These are the exact ops in concurrent-psbt / ptj commands.
  inspectPsbt(psbt: string): Promise<InspectResponse>;
  createPsbt(request: CreatePsbtRequest): Promise<PsbtResponse>;
  joinPsbts(psbts: string[]): Promise<PsbtResponse>;
  // allowShortSeed is the explicit override for ordering seeds below the spec
  // minimum of 128 bits; without it the backend rejects short seeds.
  sortPsbt(psbt: string, seedHex?: string, allowShortSeed?: boolean): Promise<PsbtResponse>;
  makeUnordered(psbt: string): Promise<PsbtResponse>;
  atomizePsbt(psbt: string): Promise<AtomizeResponse>;
  concatenatePsbts(psbts: string[]): Promise<PsbtResponse>;
  exportBip174(psbt: string): Promise<ExportBip174Response>;
  // BIP 174 has no TX_MODIFIABLE field; `modifiable` is the caller's explicit
  // assertion that inputs/outputs may still be added to the import.
  importBip174(psbt: string, modifiable?: boolean): Promise<PsbtResponse>;
  // Assign spec identity fields (PSBT_OUT_UNIQUE_ID, optional
  // PSBT_IN_UNIQUE_ID) to entries that lack them — the practical path from
  // imported BIP 174/370 data to the unordered constructor. Default:
  // auto-assign missing output ids; see AssignIdsOptions for manual
  // directives (the atomized-import case).
  assignIds(psbt: string, options?: AssignIdsOptions): Promise<PsbtResponse>;

  // Negotiation band (ptj pay / confirm / payments). Mechanism-only: the
  // record bytes are opaque hex, appended to / decoded from the grow-only
  // negotiation set. Each record argument also admits a build-it-for-me
  // variant (PayByAddress / DeriveConfirmation) where the BACKEND constructs
  // the record — the webgui routes do this with the CLI's own builders;
  // adapters without a native builder (wasm today) reject the variant with a
  // clear PtjBackendError instead of guessing. WASM implements the opaque
  // forms via concurrent-psbt-wasm's pay/confirm/payments exports; the webgui
  // serves /api/{pay,confirm,payments}; tauri stubs them like every other op.
  pay(psbt: string, payment: PaymentRecord, options?: PayOptions): Promise<PsbtResponse>;
  confirm(
    psbt: string,
    confirmation: ConfirmationRecord,
    options?: ConfirmOptions,
  ): Promise<PsbtResponse>;
  payments(psbt: string, options?: PaymentsOptions): Promise<PaymentsResponse>;

  // Layer-2 (local lattice fold, always real) + Layer-3 (network, transport-
  // dependent). On HTTP this is POST /api/sync (local join_psbts always; iroh
  // only when ptj is built --features iroh-sync). On WASM the local fold runs
  // in-process (concurrent-psbt join via the localSync export) and the network
  // leg is a browser-viable transport injected into the WasmBackend (see
  // backends/wasm.ts) — LOCAL-FIRST: with no transport injected, syncPsbts is a
  // pure in-browser fold with zero network dependency. Networked transports
  // (payjoin-dir/OHTTP, webrtc, nostr) are explicit opt-in, never a default.
  syncPsbts(request: SyncRequest): Promise<SyncResponse>;
}

// Re-export the DTOs and error so the frontend imports everything it needs from
// this one module (matching the old backend.ts import surface in app.ts:4-14).
export type {
  AssignIdsOptions,
  AtomizeResponse,
  ConfirmOptions,
  ConfirmationRecord,
  CreateInput,
  CreateOutput,
  CreatePsbtRequest,
  DeriveConfirmation,
  ExportBip174Response,
  IdAssignment,
  InspectResponse,
  OrderingMode,
  PayByAddress,
  PaymentRecord,
  PayOptions,
  PaymentsOptions,
  PaymentsResponse,
  PsbtResponse,
  SyncRequest,
  SyncResponse,
} from "./types.js";
export { PtjBackendError } from "./types.js";
