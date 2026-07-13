import type { InspectResponse } from "../shared-frontend/core/backend.js";
import { type Network } from "./encoding.js";
import { type FragmentSummary } from "./state.js";
export interface InputView {
    index: number;
    outpointText: string | null;
    outpointTxid: string | null;
    outpointVout: number | null;
    sequence: string | null;
    knownUtxoSats: number | null;
    hasWitnessUtxo: boolean;
    hasNonWitnessUtxo: boolean;
    provenance: string | null;
    signatures: SignaturePresence;
}
export interface OutputView {
    index: number;
    amountSats: number | null;
    address: string | null;
    scriptHex: string | null;
    scriptKind: ScriptKind;
    scriptLabel: string;
    uniqueIdHex: string | null;
    provenance: string | null;
}
export type ScriptKind = "p2pkh" | "p2sh" | "p2wpkh" | "p2wsh" | "p2tr" | "witness" | "unknown" | "absent";
export declare function scriptTemplate(scriptHex: string | null): {
    kind: ScriptKind;
    label: string;
};
export declare const LIFEHASH_ROUTE = "/api/lifehash/";
export declare function lifehashSrc(digestHex: string): string;
export declare function addressChipDigestHex(address: string | null): string | null;
export interface ProvenanceMap {
    inputs: Record<string, string>;
    outputs: Record<string, string>;
}
export type SignaturePresence = "final" | "partial" | "unsigned";
export declare function signaturePresence(inspect: InspectResponse | null, index: number): SignaturePresence;
export declare function inputViews(inspect: InspectResponse | null, provenance?: ProvenanceMap): InputView[];
export declare function outputViews(inspect: InspectResponse | null, network: Network, provenance?: ProvenanceMap): OutputView[];
export interface AmountSpanPart {
    part: "symbol" | "scale" | "digits";
    className: "session-amount-symbol" | "session-amount-scale" | "session-amount-digits";
    text: string;
}
export declare function amountSpanParts(valueSats: number): AmountSpanPart[];
export declare function signedAmountSpanParts(valueSats: number): AmountSpanPart[];
export declare function amountBits(valueSats: number): string;
export interface CardGroup {
    key: string;
    label: string;
    kind: "provenance" | "script-template" | "unattributed";
    inputs: InputView[];
    outputs: OutputView[];
    inputSubtotalSats: number | null;
    outputSubtotalSats: number | null;
}
export type GroupingDimension = "provenance" | "provenance+script-template";
export declare function cardGroups(inputs: InputView[], outputs: OutputView[], dimension?: GroupingDimension): CardGroup[];
export declare function groupChipDigestHex(group: Pick<CardGroup, "outputs">): string | null;
export interface FeeLine {
    knownInputSats: number | null;
    outputSats: number | null;
    feeSats: number | null;
    text: string;
}
export declare function formatSignedSats(sats: number): string;
export declare function feeLine(summary: FragmentSummary): FeeLine;
export declare function declaredFeeSatsFromInspect(inspect: InspectResponse | null): number | null;
export declare function sizeEstimateVbytesFromInspect(inspect: InspectResponse | null): number | null;
export declare function formatFeeRate(rate: number): string;
export interface BalanceDelta {
    kind: "deficit" | "surplus";
    column: "input" | "output";
    sats: number;
}
export interface BalanceSheet {
    inputTotalSats: number | null;
    outputTotalSats: number | null;
    declaredFeeSats: number | null;
    outputAccountingTotalSats: number | null;
    outputTotalElidedByDeclaredFees: boolean;
    feeSats: number | null;
    implicitFeeSats: number | null;
    delta: BalanceDelta | null;
    sizeEstimateVbytes: number | null;
    feeRateText: string | null;
    showFeeRate: boolean;
    fallbackText: string | null;
}
export declare function balanceSheet(summary: FragmentSummary, inspect: InspectResponse | null): BalanceSheet;
export interface FragmentCardModel {
    summary: FragmentSummary;
    inputs: InputView[];
    outputs: OutputView[];
    groups: CardGroup[];
    uidPresent: number | null;
    uidTotal: number | null;
    fee: FeeLine;
    balance: BalanceSheet;
}
export declare function fragmentCardModel(inspect: InspectResponse | null, network: Network, provenance?: ProvenanceMap, dimension?: GroupingDimension): FragmentCardModel;
export declare function elisionLabel(shown: number, total: number): string | null;
export interface RowDetailPair {
    label: string;
    value: string;
}
export declare function rowDetailPairs(inspect: InspectResponse | null, side: "input" | "output", index: number, network: Network): RowDetailPair[];
export type DetailLevel = "collapsed" | "rows" | "detail";
export declare const DETAIL_LEVELS: readonly DetailLevel[];
export interface GroupAggregate {
    inputCount: number;
    outputCount: number;
    inputSubtotalSats: number | null;
    outputSubtotalSats: number | null;
    signedInputCount: number;
}
export declare function groupAggregate(group: CardGroup): GroupAggregate;
export declare function rowFacePairs(inspect: InspectResponse | null, side: "input" | "output", index: number, network: Network): RowDetailPair[];
export interface BadgeView {
    emoji: string | null;
    text: string;
    tone: "neutral" | "good" | "warn";
    title: string;
}
export declare function fragmentBadges(card: Pick<FragmentCardModel, "summary" | "uidPresent" | "uidTotal">): BadgeView[];
