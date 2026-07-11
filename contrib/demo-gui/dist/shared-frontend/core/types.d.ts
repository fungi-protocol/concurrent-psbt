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
    allowShortSeed?: boolean;
    inputs: CreateInput[];
    outputs: CreateOutput[];
}
export interface IdAssignment {
    target: "in" | "out";
    index: number;
    id: string;
}
export interface AssignIdsOptions {
    ids?: IdAssignment[];
    auto?: boolean;
    overwrite?: boolean;
}
export interface FieldEdit {
    map: string;
    key: string;
    value: string | null;
}
export interface EditViolation {
    id: string;
    message: string;
    override_param: string;
    fix_id?: string;
    fix_label?: string;
    warning_text?: string;
}
export interface AppliedFix {
    fix_id: string;
    warning_text?: string;
}
export interface ApplyEditsOptions {
    applyFixes?: string[];
    overrides?: string[];
}
export interface ClassifyResponse {
    kind: string;
    [key: string]: unknown;
}
export interface ApplyEditsResponse {
    psbt?: string;
    inspect?: InspectResponse;
    violations: EditViolation[];
    overridden?: EditViolation[];
    applied_fixes?: AppliedFix[];
    error?: string;
}
export interface SyncRequest {
    psbts?: string[];
    transport?: string;
    sources?: string[];
    state?: string;
    irohTicket?: string;
    irohTicketOut?: boolean;
    irohWaitMs?: number;
    webrtcRole?: "offer" | "answer";
    signalOut?: string;
    signalIn?: string;
    webrtcBind?: string;
    iceServers?: string[];
    signalTimeoutMs?: number;
}
export interface SyncResponse {
    psbt?: string;
    inspect?: InspectResponse;
    payments: string[];
    confirmations: string[];
    irohTicketOut?: string;
}
export interface PayByAddress {
    address: string;
    amountBtc: string;
    network?: string;
    label?: string;
    payerHex?: string;
}
export type PaymentRecord = string | PayByAddress;
export interface DeriveConfirmation {
    derive: true;
    peerIdHex?: string;
}
export type ConfirmationRecord = string | DeriveConfirmation;
export interface PayOptions {
    secretHex?: string;
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
export declare class PtjBackendError extends Error {
    readonly status: number;
    constructor(status: number, message: string);
}
