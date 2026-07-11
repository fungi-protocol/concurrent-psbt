export type OrderingMode = "unset" | "deterministic" | "explicit";
export interface FetchResponse {
    ok: boolean;
    status: number;
    json(): Promise<unknown>;
}
export type FetchLike = (path: string, init: {
    method: "POST";
    headers: {
        "content-type": "application/json";
    };
    body: string;
}) => Promise<FetchResponse>;
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
export declare class PtjBackendError extends Error {
    readonly status: number;
    constructor(status: number, message: string);
}
export declare function inspectPsbt(fetchImpl: FetchLike, psbt: string): Promise<InspectResponse>;
export declare function createPsbt(fetchImpl: FetchLike, request: CreatePsbtRequest): Promise<PsbtResponse>;
export declare function joinPsbts(fetchImpl: FetchLike, psbts: string[]): Promise<PsbtResponse>;
export declare function sortPsbt(fetchImpl: FetchLike, psbt: string, seedHex?: string): Promise<PsbtResponse>;
export declare function makeUnordered(fetchImpl: FetchLike, psbt: string): Promise<PsbtResponse>;
export declare function atomizePsbt(fetchImpl: FetchLike, psbt: string): Promise<AtomizeResponse>;
export declare function concatenatePsbts(fetchImpl: FetchLike, psbts: string[]): Promise<PsbtResponse>;
export declare function exportBip174(fetchImpl: FetchLike, psbt: string): Promise<ExportBip174Response>;
export declare function importBip174(fetchImpl: FetchLike, psbt: string): Promise<PsbtResponse>;
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
export declare function syncPsbts(fetchImpl: FetchLike, request: SyncRequest): Promise<SyncResponse>;
