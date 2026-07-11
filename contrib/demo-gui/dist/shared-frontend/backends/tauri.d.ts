import type { Backend } from "../core/backend.js";
import { type ApplyEditsOptions, type ApplyEditsResponse, type AssignIdsOptions, type AtomizeResponse, type ClassifyResponse, type ConfirmationRecord, type ConfirmOptions, type CreatePsbtRequest, type ExportBip174Response, type FieldEdit, type InspectResponse, type PaymentRecord, type PayOptions, type PaymentsOptions, type PaymentsResponse, type PsbtResponse, type SyncRequest, type SyncResponse } from "../core/types.js";
export type TauriInvoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
export interface TauriBackendOptions {
    invoke?: TauriInvoke;
}
export declare class TauriBackend implements Backend {
    private readonly invoke;
    constructor(options?: TauriBackendOptions);
    private call;
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
    pay(psbt: string, payment: PaymentRecord, options?: PayOptions): Promise<PsbtResponse>;
    confirm(psbt: string, confirmation: ConfirmationRecord, options?: ConfirmOptions): Promise<PsbtResponse>;
    payments(psbt: string, options?: PaymentsOptions): Promise<PaymentsResponse>;
    syncPsbts(request: SyncRequest): Promise<SyncResponse>;
}
