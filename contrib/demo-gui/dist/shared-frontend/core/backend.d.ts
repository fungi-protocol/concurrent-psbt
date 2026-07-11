import type { ApplyEditsOptions, ApplyEditsResponse, AssignIdsOptions, AtomizeResponse, ClassifyResponse, ConfirmOptions, ConfirmationRecord, CreatePsbtRequest, ExportBip174Response, FieldEdit, InspectResponse, PayOptions, PaymentRecord, PaymentsOptions, PaymentsResponse, PsbtResponse, SyncRequest, SyncResponse } from "./types.js";
export interface Backend {
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
export type { AppliedFix, ApplyEditsOptions, ApplyEditsResponse, AssignIdsOptions, AtomizeResponse, ClassifyResponse, ConfirmOptions, ConfirmationRecord, CreateInput, CreateOutput, CreatePsbtRequest, DeriveConfirmation, EditViolation, ExportBip174Response, FieldEdit, IdAssignment, InspectResponse, OrderingMode, PayByAddress, PaymentRecord, PayOptions, PaymentsOptions, PaymentsResponse, PsbtResponse, SyncRequest, SyncResponse, } from "./types.js";
export { PtjBackendError } from "./types.js";
