export type OrderingMode = "unset" | "deterministic" | "explicit";

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

export class PtjBackendError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "PtjBackendError";
    this.status = status;
    Object.setPrototypeOf(this, PtjBackendError.prototype);
  }
}

async function postJson<T>(fetchImpl: FetchLike, path: string, body: unknown): Promise<T> {
  const response = await fetchImpl(path, {
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

function errorMessage(status: number, payload: unknown): string {
  return isErrorPayload(payload) ? payload.error : `ptj backend request failed with HTTP ${status}`;
}

function isErrorPayload(payload: unknown): payload is { error: string } {
  return typeof payload === "object"
    && payload !== null
    && "error" in payload
    && typeof payload.error === "string";
}

export function inspectPsbt(fetchImpl: FetchLike, psbt: string): Promise<InspectResponse> {
  return postJson(fetchImpl, "/api/inspect", { psbt });
}

export function createPsbt(fetchImpl: FetchLike, request: CreatePsbtRequest): Promise<PsbtResponse> {
  return postJson(fetchImpl, "/api/create", {
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

export function joinPsbts(fetchImpl: FetchLike, psbts: string[]): Promise<PsbtResponse> {
  return postJson(fetchImpl, "/api/join", { psbts });
}

export function sortPsbt(fetchImpl: FetchLike, psbt: string, seedHex?: string): Promise<PsbtResponse> {
  return postJson(fetchImpl, "/api/sort", { psbt, seed_hex: seedHex });
}

export function makeUnordered(fetchImpl: FetchLike, psbt: string): Promise<PsbtResponse> {
  return postJson(fetchImpl, "/api/make-unordered", { psbt });
}

export function atomizePsbt(fetchImpl: FetchLike, psbt: string): Promise<AtomizeResponse> {
  return postJson(fetchImpl, "/api/atomize", { psbt });
}

export function concatenatePsbts(fetchImpl: FetchLike, psbts: string[]): Promise<PsbtResponse> {
  return postJson(fetchImpl, "/api/concatenate", { psbts });
}

export function exportBip174(fetchImpl: FetchLike, psbt: string): Promise<ExportBip174Response> {
  return postJson(fetchImpl, "/api/export-bip174", { psbt });
}

export function importBip174(fetchImpl: FetchLike, psbt: string): Promise<PsbtResponse> {
  return postJson(fetchImpl, "/api/import-bip174", { psbt });
}

export interface SyncRequest {
  psbts?: string[];
  irohTicket?: string;
  irohWaitMs?: number;
}

export interface SyncResponse {
  psbt: string;
  inspect?: InspectResponse;
  payments: string[];
  confirmations: string[];
}

export function syncPsbts(fetchImpl: FetchLike, request: SyncRequest): Promise<SyncResponse> {
  return postJson(fetchImpl, "/api/sync", {
    psbts: request.psbts,
    iroh_ticket: request.irohTicket,
    iroh_wait_ms: request.irohWaitMs,
  });
}
