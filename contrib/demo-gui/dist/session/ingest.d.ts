import { type NodeRef, type ObjectsState } from "./wiring.js";
export type PasteKind = "psbt" | "payment-uri" | "descriptor" | "npub" | "iroh-ticket" | "transaction-hex" | "unknown";
export interface PasteClassification {
    kind: PasteKind;
    payload: string;
    detail: string;
    needsBackend: string | null;
}
export declare function classifyPaste(text: string): PasteClassification;
export interface MintResult {
    state: ObjectsState;
    minted: NodeRef | null;
    log: string;
}
export declare function mintFromPaste(state: ObjectsState, pasted: PasteClassification): MintResult;
