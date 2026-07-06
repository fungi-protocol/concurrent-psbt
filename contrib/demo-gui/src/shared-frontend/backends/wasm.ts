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

import type { Backend } from "../core/backend.js";
import {
  type AtomizeResponse,
  type ConfirmOptions,
  type CreatePsbtRequest,
  type ExportBip174Response,
  type InspectResponse,
  type PayOptions,
  type PaymentsOptions,
  type PaymentsResponse,
  type PsbtResponse,
  PtjBackendError,
  type SyncRequest,
  type SyncResponse,
} from "../core/types.js";

// ---------------------------------------------------------------------------
// The wasm-bindgen glue surface emitted by the `concurrent-psbt-wasm` crate
// (the ONE wasm wrapper; the `ptj-wasm` duplicate was merged into it). Method
// names are the crate's #[wasm_bindgen(js_name = ...)] exports — camelCase,
// per the naming rule in ../core/backend.ts. Structured `request` params are
// snake_case JSON objects (the ptj webgui wire contract); this adapter builds
// them. Methods throw a JsError on failure; we normalize to
// PtjBackendError(0, msg).
// ---------------------------------------------------------------------------
export interface PtjWasmModule {
  // Layer-1 — pure concurrent-psbt calls.
  inspect(psbt: string): InspectResponse;
  // `request` is the snake_case JSON object (same shape ptj /api/create parses).
  create(request: unknown): PsbtResponse;
  join(psbts: string[]): PsbtResponse;
  sort(psbt: string, seedHex?: string): PsbtResponse;
  makeUnordered(psbt: string): PsbtResponse;
  atomize(psbt: string): AtomizeResponse;
  concatenate(psbts: string[]): PsbtResponse;
  exportBip174(psbt: string): ExportBip174Response;
  importBip174(psbt: string): PsbtResponse;
  // Negotiation band (opaque hex records; snake_case request objects).
  pay(request: unknown): PsbtResponse;
  confirm(request: unknown): PsbtResponse;
  payments(request: unknown): PaymentsResponse;
  // Layer-2 local fold: deterministic lattice join of many PSBTs into one,
  // returning the joined PSBT plus empty payments/confirmations (the same
  // SyncResponse shape the webgui local/no-ticket branch returns — transport
  // messages are what populate those arrays, and there is no transport here).
  localSync(psbts: string[]): SyncResponse;
}

// A browser transport that carries PSBT frames peer-to-peer. Minimal seam so
// the WasmBackend does not depend on any specific transport crate; the PWA
// shell constructs one (web-sys RTCPeerConnection, nostr-over-WebSocket, or the
// payjoin-directory-over-OHTTP oblivious mailbox) and injects it. Mirrors the
// pull-based collect/publish cadence of transport_core.
export interface BrowserTransport {
  // Publish our local PSBTs to peers (opaque bytes; the transport frames them).
  publish(psbts: string[]): Promise<void>;
  // Collect a fresh snapshot of all PSBTs seen so far (own + peers'), matching
  // the recv() snapshot-includes-own-sends contract in transport_core.
  collect(): Promise<string[]>;
}

export interface WasmBackendOptions {
  // Injected network transport for Layer-3 sync. Omit for a pure local PWA.
  transport?: BrowserTransport;
  // How long to gather peer PSBTs before folding, when a transport is present.
  // Mirrors iroh_wait_ms (SyncRequest default 5000 in webgui.rs:240).
  defaultWaitMs?: number;
}

function wrap<T>(fn: () => T): T {
  try {
    return fn();
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    // status 0 => "not an HTTP failure"; app.ts only checks instanceof.
    throw new PtjBackendError(0, msg);
  }
}

export class WasmBackend implements Backend {
  private readonly m: PtjWasmModule;
  // `| undefined` (not `?`): the constructor assigns a possibly-undefined
  // option, which exactOptionalPropertyTypes distinguishes from absence.
  private readonly transport: BrowserTransport | undefined;
  private readonly defaultWaitMs: number;

  constructor(module: PtjWasmModule, options: WasmBackendOptions = {}) {
    this.m = module;
    this.transport = options.transport;
    this.defaultWaitMs = options.defaultWaitMs ?? 5000;
  }

  async inspectPsbt(psbt: string): Promise<InspectResponse> {
    return wrap(() => this.m.inspect(psbt));
  }

  async createPsbt(request: CreatePsbtRequest): Promise<PsbtResponse> {
    // Map camelCase DTO -> the snake_case JSON the wrapper parses, identical to
    // HttpBackend.createPsbt so both backends feed concurrent-psbt the same shape.
    return wrap(() =>
      this.m.create({
        network: request.network,
        ordering: request.ordering,
        seed_hex: request.seedHex,
        inputs: request.inputs,
        outputs: request.outputs.map((output) => ({
          address: output.address,
          amount_btc: output.amountBtc,
        })),
      })
    );
  }

  async joinPsbts(psbts: string[]): Promise<PsbtResponse> {
    return wrap(() => this.m.join(psbts));
  }

  async sortPsbt(psbt: string, seedHex?: string): Promise<PsbtResponse> {
    return wrap(() => this.m.sort(psbt, seedHex));
  }

  async makeUnordered(psbt: string): Promise<PsbtResponse> {
    return wrap(() => this.m.makeUnordered(psbt));
  }

  async atomizePsbt(psbt: string): Promise<AtomizeResponse> {
    return wrap(() => this.m.atomize(psbt));
  }

  async concatenatePsbts(psbts: string[]): Promise<PsbtResponse> {
    return wrap(() => this.m.concatenate(psbts));
  }

  async exportBip174(psbt: string): Promise<ExportBip174Response> {
    return wrap(() => this.m.exportBip174(psbt));
  }

  async importBip174(psbt: string): Promise<PsbtResponse> {
    return wrap(() => this.m.importBip174(psbt));
  }

  // --- negotiation band (opaque hex records; snake_case wire fields) ---

  async pay(psbt: string, paymentHex: string, options?: PayOptions): Promise<PsbtResponse> {
    return wrap(() =>
      this.m.pay({
        psbt,
        payment_hex: paymentHex,
        secret_hex: options?.secretHex,
        dummy: options?.dummy ?? 0,
      })
    );
  }

  async confirm(
    psbt: string,
    confirmationHex: string,
    options?: ConfirmOptions
  ): Promise<PsbtResponse> {
    return wrap(() =>
      this.m.confirm({
        psbt,
        confirmation_hex: confirmationHex,
        secret_hex: options?.secretHex,
      })
    );
  }

  async payments(psbt: string, options?: PaymentsOptions): Promise<PaymentsResponse> {
    return wrap(() => this.m.payments({ psbt, secret_hex: options?.secretHex }));
  }

  async syncPsbts(request: SyncRequest): Promise<SyncResponse> {
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
