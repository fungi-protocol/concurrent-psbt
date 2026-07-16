import type { ConfirmationRecord, ConfirmOptions, CreatePsbtRequest, InspectResponse, PaymentRecord, PaymentsResponse, PayOptions, SyncRequest } from "../shared-frontend/core/backend.js";
import { type SessionOrderingMode } from "../model.js";
export type FragmentOrigin = "paste" | "payment-uri" | "upload" | "import-bip174" | "create" | "join" | "sort" | "make-unordered" | "atomize" | "concatenate" | "assign-ids" | "edit" | "pay" | "confirm" | "sync";
export interface SessionFragment {
    key: string;
    psbt: string;
    inspect: InspectResponse | null;
    origin: FragmentOrigin;
    selected: boolean;
}
export interface SessionState {
    fragments: SessionFragment[];
    counter: number;
}
export interface AddFragmentResult {
    state: SessionState;
    fragment: SessionFragment;
    duplicate: boolean;
}
export declare function emptySession(): SessionState;
export declare function addFragment(state: SessionState, psbt: string, inspect: InspectResponse | null, origin: FragmentOrigin): AddFragmentResult;
export declare function removeFragment(state: SessionState, key: string): SessionState;
export declare function setSelected(state: SessionState, key: string, selected: boolean): SessionState;
export declare function selectedFragments(state: SessionState): SessionFragment[];
export declare function asObject(value: unknown): Record<string, unknown> | null;
export declare function asString(value: unknown): string | null;
export declare function asNumber(value: unknown): number | null;
export declare function asBoolean(value: unknown): boolean | null;
export declare function asArray(value: unknown): unknown[] | null;
export interface FragmentSummary {
    format: string | null;
    ordering: string | null;
    inputCount: number | null;
    outputCount: number | null;
    sortMode: string | null;
    seedHex: string | null;
    uniqueIdHex: string | null;
    knownInputSats: number | null;
    outputSats: number | null;
    feeSats: number | null;
    modifiableInputs: boolean | null;
    modifiableOutputs: boolean | null;
    outputUidPresent: number | null;
}
export declare function fragmentSummary(inspect: InspectResponse | null): FragmentSummary;
export declare function fragmentLabel(fragment: SessionFragment): string;
export interface NegotiationView {
    paymentCount: number;
    confirmationCount: number;
    payments: string[];
    confirmations: string[];
}
export declare function negotiationView(response: PaymentsResponse): NegotiationView;
export type FormResult<T> = {
    ok: true;
    value: T;
} | {
    ok: false;
    error: string;
};
export declare function isHexBytes(value: string, exactBytes?: number): boolean;
export interface CreateFormInput {
    txid: string;
    vout: string;
}
export interface CreateFormOutput {
    address: string;
    amountBtc: string;
}
export interface CreateForm {
    network: string;
    ordering: SessionOrderingMode;
    seed: string;
    inputs: CreateFormInput[];
    outputs: CreateFormOutput[];
}
export declare function buildCreateRequest(form: CreateForm): FormResult<CreatePsbtRequest>;
export type SyncTransport = "local" | "iroh" | "str0m" | "webrtc-rs";
export interface SyncForm {
    transport: SyncTransport;
    sources: string;
    state: string;
    irohTicket: string;
    irohTicketOut: boolean;
    irohWaitMs: string;
    webrtcRole: "" | "offer" | "answer";
    signalOut: string;
    signalIn: string;
    webrtcBind: string;
    iceServers: string;
    signalTimeoutMs: string;
}
export declare function parseLines(text: string): string[];
export declare function buildSyncRequest(form: SyncForm, psbts: string[]): FormResult<SyncRequest>;
export interface PayForm {
    mode: "address" | "hex";
    address: string;
    amountBtc: string;
    network: string;
    label: string;
    payerHex: string;
    paymentHex: string;
    secretHex: string;
    dummy: string;
}
export interface PayArgs {
    payment: PaymentRecord;
    options?: PayOptions;
}
export declare function buildPayArgs(form: PayForm): FormResult<PayArgs>;
export interface ConfirmForm {
    mode: "derive" | "hex";
    confirmationHex: string;
    peerIdHex: string;
    secretHex: string;
}
export interface ConfirmArgs {
    confirmation: ConfirmationRecord;
    options?: ConfirmOptions;
}
export declare function buildConfirmArgs(form: ConfirmForm): FormResult<ConfirmArgs>;
export declare function pastedPsbt(text: string): string | null;
export declare function bytesToBase64(bytes: Uint8Array): string;
