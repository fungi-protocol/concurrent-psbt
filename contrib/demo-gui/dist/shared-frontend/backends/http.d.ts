import type { Backend } from "../core/backend.js";
import { type ApplyEditsOptions, type ApplyEditsResponse, type AssignIdsOptions, type AtomizeResponse, type ClassifyResponse, type ConfirmationRecord, type ConfirmOptions, type CreatePsbtRequest, type ExportBip174Response, type FakeDescriptorKind, type FakeDescriptorResponse, type FakeUtxoRef, type FakeUtxosResponse, type FieldEdit, type InspectResponse, type PaymentRecord, type PayOptions, type PaymentsOptions, type PaymentsResponse, type PsbtResponse, type SyncRequest, type SyncResponse } from "../core/types.js";
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
export declare class HttpBackend implements Backend {
    private readonly fetchImpl;
    private readonly base;
    constructor(fetchImpl?: FetchLike, base?: string);
    private postJson;
    inspectPsbt(psbt: string): Promise<InspectResponse>;
    createPsbt(request: CreatePsbtRequest): Promise<PsbtResponse>;
    joinPsbts(psbts: string[]): Promise<PsbtResponse>;
    sortPsbt(psbt: string, seedHex?: string, allowShortSeed?: boolean): Promise<PsbtResponse>;
    makeUnordered(psbt: string): Promise<PsbtResponse>;
    atomizePsbt(psbt: string): Promise<AtomizeResponse>;
    concatenatePsbts(psbts: string[]): Promise<PsbtResponse>;
    exportBip174(psbt: string): Promise<ExportBip174Response>;
    importBip174(psbt: string, modifiable?: boolean): Promise<PsbtResponse>;
    assignIds(psbt: string, options?: AssignIdsOptions): Promise<PsbtResponse>;
    applyPsbtEdits(psbt: string, edits: FieldEdit[], options?: ApplyEditsOptions): Promise<ApplyEditsResponse>;
    classifyPaste(payload: string, network?: string): Promise<ClassifyResponse>;
    fakeDescriptor(network?: string, kind?: FakeDescriptorKind): Promise<FakeDescriptorResponse>;
    fakeUtxos(descriptor: string, network?: string, count?: number): Promise<FakeUtxosResponse>;
    fakePsbt(descriptor: string, utxos: FakeUtxoRef[], network?: string, recipients?: number): Promise<PsbtResponse>;
    pay(psbt: string, payment: PaymentRecord, options?: PayOptions): Promise<PsbtResponse>;
    confirm(psbt: string, confirmation: ConfirmationRecord, options?: ConfirmOptions): Promise<PsbtResponse>;
    payments(psbt: string, options?: PaymentsOptions): Promise<PaymentsResponse>;
    syncPsbts(request: SyncRequest): Promise<SyncResponse>;
}
