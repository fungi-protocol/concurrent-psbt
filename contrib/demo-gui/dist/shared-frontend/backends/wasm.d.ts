import type { Backend } from "../core/backend.js";
import { type ApplyEditsOptions, type ApplyEditsResponse, type AssignIdsOptions, type AtomizeResponse, type ClassifyResponse, type ConfirmationRecord, type ConfirmOptions, type CreatePsbtRequest, type ExportBip174Response, type FakeDescriptorResponse, type FakeUtxosResponse, type FieldEdit, type InspectResponse, type PaymentRecord, type PayOptions, type PaymentsOptions, type PaymentsResponse, type PsbtResponse, type SyncRequest, type SyncResponse } from "../core/types.js";
export interface PtjWasmModule {
    inspect(psbt: string): InspectResponse;
    create(request: unknown): PsbtResponse;
    join(psbts: string[]): PsbtResponse;
    sort(psbt: string, seedHex?: string, allowShortSeed?: boolean): PsbtResponse;
    makeUnordered(psbt: string): PsbtResponse;
    atomize(psbt: string): AtomizeResponse;
    concatenate(psbts: string[]): PsbtResponse;
    exportBip174(psbt: string): ExportBip174Response;
    importBip174(psbt: string, modifiable?: boolean): PsbtResponse;
    assignIds(request: unknown): PsbtResponse;
    pay(request: unknown): PsbtResponse;
    confirm(request: unknown): PsbtResponse;
    payments(request: unknown): PaymentsResponse;
    localSync(psbts: string[]): SyncResponse;
}
export interface BrowserTransport {
    publish(psbts: string[]): Promise<void>;
    collect(): Promise<string[]>;
}
export interface WasmBackendOptions {
    transport?: BrowserTransport;
    defaultWaitMs?: number;
}
export declare class WasmBackend implements Backend {
    private readonly m;
    private readonly transport;
    private readonly defaultWaitMs;
    constructor(module: PtjWasmModule, options?: WasmBackendOptions);
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
    applyPsbtEdits(_psbt: string, _edits: FieldEdit[], _options?: ApplyEditsOptions): Promise<ApplyEditsResponse>;
    classifyPaste(_payload: string, _network?: string): Promise<ClassifyResponse>;
    fakeDescriptor(): Promise<FakeDescriptorResponse>;
    fakeUtxos(): Promise<FakeUtxosResponse>;
    fakePsbt(): Promise<PsbtResponse>;
    pay(psbt: string, payment: PaymentRecord, options?: PayOptions): Promise<PsbtResponse>;
    confirm(psbt: string, confirmation: ConfirmationRecord, options?: ConfirmOptions): Promise<PsbtResponse>;
    payments(psbt: string, options?: PaymentsOptions): Promise<PaymentsResponse>;
    syncPsbts(request: SyncRequest): Promise<SyncResponse>;
}
