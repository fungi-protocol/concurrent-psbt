export type PsbtFormat = "unordered" | "bip370" | "bip174";
export type PsbtModifiability = "both" | "inputs" | "outputs" | "none";
export type SessionOrderingMode = "det" | "explicit" | "unset";
export type PsbtRoleId = "unordered-register" | "unordered-fragment" | "bip370-constructor" | "fixed-transaction" | "sorted-bip370";
export type PsbtUnaryAction = "make-unordered" | "sort" | "fix-sets" | "atomize" | "promote" | "abort-session";
export interface PayloadItem {
    id: string;
    label?: string;
    address?: string;
    valueSats?: number;
    explicitFeeSats?: number;
    [key: string]: unknown;
}
export interface DescriptorPayload {
    id: string;
    privacy: "public" | "private";
    descriptor: string;
    [key: string]: unknown;
}
export interface Payload {
    inputs: PayloadItem[];
    outputs: PayloadItem[];
    descriptors?: DescriptorPayload[];
    conflicts: string[];
}
export interface PsbtNode extends Payload {
    id: string;
    format: PsbtFormat;
    seed?: string;
    sortMode?: SessionOrderingMode;
    kind?: string;
    modifiable?: PsbtModifiability;
}
export interface PsbtRole {
    id: PsbtRoleId;
    label: string;
    spec: string;
    roles: string[];
}
export interface PsbtProtocolIdentity {
    label: "txid" | "unique id";
    value: string;
    source: string;
    stableBeforeSigning: boolean;
}
export interface PeerNode {
    id: string;
    local?: boolean;
    views?: Record<string, unknown>;
}
export interface GraphEdge {
    from: string;
    to: string;
    kind: string;
}
export interface Box {
    x: number;
    y: number;
    width: number;
    height: number;
}
export interface AmountParts {
    prefix: string;
    muted: string;
    sats: string;
}
export interface ParsedBitcoinUri {
    uri: string;
    address: string;
    valueSats: number;
    descriptorId: string | null;
    label: string;
    message: string;
}
export interface Finalization {
    inputTotal: number;
    outputTotal: number;
    fee: number;
    status: "finalized" | "blocked";
}
export interface BalanceBucket {
    inputs: number;
    outputs: number;
    explicitFee: number;
    implicitFee: number;
    net: number;
    balanced: boolean;
}
export interface TransactionBalance {
    inputs: number;
    outputs: number;
    fee: {
        explicit: number;
        implicit: number;
        total: number;
    };
    mine: BalanceBucket;
    other: BalanceBucket;
    mineBalanced: boolean;
    status: "balanced" | "mine-balanced" | "mine-unbalanced" | "deficit";
}
export interface SneakernetFragmentStatus {
    peerless: boolean;
    peers: number;
    sessions: number;
    fragments: number;
    ordered: number;
    unordered: number;
    psbts: number;
    canExport: boolean;
    nextAction: "import" | "make-unordered" | "select-export" | "export";
}
export interface SessionOrderingConfig {
    mode: SessionOrderingMode;
    seed: string;
    valid: boolean;
    error?: string;
}
export interface PsbtCompatibility {
    ok: boolean;
    reason: string;
}
export type PayloadSide = "input" | "output";
export type SizeUnit = "vbytes" | "weight-units";
export interface ItemSizeEstimate {
    vbytes: number;
    weightUnits: number;
    exact: boolean;
}
export interface PayloadSizeEstimate {
    inputVbytes: number;
    outputVbytes: number;
    totalVbytes: number;
    totalWeightUnits: number;
}
export interface PeerLatencyProfile {
    peerId: string;
    minMs: number;
    jitterMs: number;
}
export interface PeerAck {
    peerId: string;
    delayMs: number;
    acked: number;
    total: number;
}
export interface PeerAckPlan {
    peers: string[];
    total: number;
    acks: PeerAck[];
    completionDelayMs: number;
}
export type DescriptorOwnership = "mine" | "other";
export type DescriptorMenuActionId = "tag-mine" | "tag-other";
export interface DescriptorMenuDescriptor {
    id: string;
    privacy: "public" | "private";
    ownership?: DescriptorOwnership;
    color: string;
}
export interface DescriptorMenuAction {
    id: DescriptorMenuActionId;
    label: string;
    disabled: boolean;
}
export interface DescriptorColorChoice {
    color: string;
    selected: boolean;
}
export interface DescriptorMenuState {
    ownership: DescriptorOwnership;
    ownershipActions: DescriptorMenuAction[];
    colorChoices: DescriptorColorChoice[];
    paymentRequestAction: {
        id: "payment-request";
        label: string;
    };
}
export type DescriptorDrawerSourceKind = "utxo" | "payment-request" | "peer-provenance";
export interface DescriptorDrawerSource {
    kind: DescriptorDrawerSourceKind;
    id: string;
    descriptorId?: string | null;
    label?: string;
    valueSats?: number;
    promotedTo?: string | null;
    uri?: string | null;
}
export interface DescriptorDrawerItem {
    kind: "utxo" | "payment-request";
    id: string;
    label: string;
    valueSats: number;
    promotedTo: string | null;
    uri: string | null;
}
export type DisplaySectionKind = "recognized" | "unrecognized";
export type DisplayKind = "coin";
export interface DisplayRow extends PayloadItem {
    displayKind: DisplayKind;
}
export interface DisplaySection {
    kind: DisplaySectionKind;
    label: string;
    descriptorId?: string;
    descriptorColor?: string;
    descriptorMine?: boolean;
    rows: DisplayRow[];
    totalSats: number;
}
export interface DisplaySubtransaction {
    kind: DisplaySectionKind;
    label: string;
    descriptorId?: string;
    descriptorColor?: string;
    descriptorMine?: boolean;
    inputs: DisplaySection;
    outputs: DisplaySection;
    inputTotalSats: number;
    outputTotalSats: number;
    feeSats: number;
    outputFeeSats: number;
    inputDeficitSats: number;
    explicitFeeSats: number;
    inputAccountingTotalSats: number;
    outputAccountingTotalSats: number;
    implicitFeeSats: number;
    estimatedVbytes: number;
}
export interface BalanceSheetTotalRow {
    kind: DisplaySectionKind | "whole";
    label: string;
    descriptorId?: string;
    descriptorColor?: string;
    descriptorMine?: boolean;
    inputTotalSats: number;
    outputTotalSats: number;
    feeSats: number;
    outputFeeSats: number;
    inputDeficitSats: number;
    explicitFeeSats: number;
    inputAccountingTotalSats: number;
    outputAccountingTotalSats: number;
    implicitFeeSats: number;
    estimatedVbytes: number;
}
export interface AccountingDeltaPresentation {
    kind: "balanced" | "surplus" | "deficit";
    column: "input" | "output" | null;
    oppositeColumn: "input" | "output" | null;
    showTotals: boolean;
    totalSats: number;
    explicitFeeSats: number;
    implicitFeeSats: number;
    label: string;
    separator: " / " | " + " | null;
    amountA: number;
    amountB: number | null;
}
export interface DescriptorFeeSignal {
    descriptorId?: string;
    descriptorLabel: string;
    explicitFeeSats: number;
    implicitFeeSats: number;
    totalFeeSats: number;
    estimatedVbytes: number;
    feeRateSatsPerVbyte: number;
    averageFeeRateSatsPerVbyte: number;
    canFinalizeExplicitFee: boolean;
}
export interface DescriptorFeeFinalizeOptions {
    feeFinalized?: boolean;
}
export type FeeWarningLevel = "none" | "yellow" | "red" | "confirm";
export interface DescriptorFeeContributionPlan {
    descriptorId?: string;
    descriptorLabel: string;
    availableSats: number;
    selectedSats: number;
    finalExplicitFeeSats: number;
    estimatedVbytes: number;
    feeRateSatsPerVbyte: number;
    averageFeeRateSatsPerVbyte: number;
    relativeFeeRateRatio: number;
    absoluteWarningLevel: FeeWarningLevel;
    relativeWarningLevel: FeeWarningLevel;
    warningLevel: FeeWarningLevel;
    confirmationRequired: boolean;
}
export interface UnorderedPsbtDisplay {
    inputs: DisplaySection[];
    outputs: DisplaySection[];
    subtransactions: DisplaySubtransaction[];
    explicitFeeSats: number;
    estimatedVbytes: number;
    whole: TransactionBalance;
}
export declare function amountParts(valueSats: number): AmountParts;
export declare function formatSatAmount(valueSats: number): string;
export declare function coinDetailLines(side: PayloadSide, item: PayloadItem, index?: number, unit?: SizeUnit): string[];
export declare function normalizeSessionOrdering(mode: SessionOrderingMode, seed: string): SessionOrderingConfig;
export declare function seedFromRandomBytes(bytes: ArrayLike<number>): string;
export declare function hashHex(value: unknown): string;
export declare function peerLatencyProfile(peerId: string): PeerLatencyProfile;
export declare function samplePeerAckDelay(peerId: string, random01?: () => number): number;
export declare function peerIsInteractive(peer: PeerNode): boolean;
export declare function peerAckPlan(peerIds: string[], random01?: () => number): PeerAckPlan;
export declare function mergePayloads(...payloads: Payload[]): Payload;
export declare function psbtCompatibility(left: PsbtNode, right: PsbtNode): PsbtCompatibility;
export declare function psbtsAreCompatible(left: PsbtNode, right: PsbtNode): boolean;
export declare function psbtProtocolIdentity(node: PsbtNode, vertexKind?: "fragment" | "session"): PsbtProtocolIdentity;
export declare function psbtRole(node: PsbtNode, vertexKind?: "fragment" | "session"): PsbtRole;
export declare function psbtUnaryActions(node: PsbtNode, vertexKind?: "fragment" | "session"): PsbtUnaryAction[];
export declare function joinSessionSeeds(sessions: Array<{
    seed: string;
}>): string;
export declare function orderedProjectionPayload(node: Payload): Payload;
export declare function orderByStableId(left: PayloadItem, right: PayloadItem): number;
export declare function totalSats(items: Array<{
    valueSats?: number;
}>): number;
export declare function itemSizeEstimate(side: PayloadSide, item: PayloadItem): ItemSizeEstimate;
export declare function payloadSizeEstimate(payload: Payload): PayloadSizeEstimate;
export declare function formatSizeEstimate(size: number | ItemSizeEstimate | PayloadSizeEstimate, unit?: SizeUnit): string;
export declare function transactionBalance(payload: Payload): TransactionBalance;
export declare function descriptorMenuState(record: DescriptorMenuDescriptor, palette: string[]): DescriptorMenuState;
export declare function descriptorDrawerItems(descriptorId: string | null, sources: DescriptorDrawerSource[]): DescriptorDrawerItem[];
export declare function unorderedPsbtDisplay(payload: Payload): UnorderedPsbtDisplay;
export declare function payloadRowKey(side: "input" | "output", item: PayloadItem): string;
export declare function pendingPayloadRowKeys(payload: Payload): string[];
export declare function sneakernetFragmentStatus(peers: PeerNode[], sessions: PsbtNode[], fragments: PsbtNode[]): SneakernetFragmentStatus;
export declare function unorderedBalanceSheetTotalRows(display: UnorderedPsbtDisplay): BalanceSheetTotalRow[];
export declare function accountingDeltaPresentation(section: BalanceSheetTotalRow | DisplaySubtransaction): AccountingDeltaPresentation;
export declare function shouldShowGrandTotal(display: UnorderedPsbtDisplay): boolean;
export declare function balanceSheetFeeSignal(section: BalanceSheetTotalRow | DisplaySubtransaction, averageFeeRateSatsPerVbyte: number): DescriptorFeeSignal;
export declare function descriptorFeeSignal(payload: Payload, descriptorId: string): DescriptorFeeSignal | null;
export declare function defaultFeeContributionSats(signal: DescriptorFeeSignal | null): number;
export declare function descriptorFeeContributionPlan(signal: DescriptorFeeSignal | null, selectedSats?: number): DescriptorFeeContributionPlan | null;
export declare function finalizeDescriptorExplicitFee(payload: Payload, descriptorId: string, amountSats?: number, options?: DescriptorFeeFinalizeOptions): Payload;
export declare function finalizePayload(payload: Payload): Finalization;
export declare function parseBitcoinUri(text: string): ParsedBitcoinUri | null;
export declare function compactBase64(value: string): string;
export declare function looksLikeBase64Psbt(value: string): boolean;
export declare function looksLikeDescriptor(value: string): boolean;
export declare function descriptorLooksPrivate(value: string): boolean;
export declare function peerBridgeComponents(peers: PeerNode[], edges: GraphEdge[]): string[][];
export declare function sessionVisibleToPeerGroup(session: {
    id: string;
    peers?: string[];
}, peers: PeerNode[], peerIds: string[]): boolean;
export declare function peerGroupBounds(group: string[], positions: Map<string, Box>): Box | null;
export declare function peerEdgeTermination(peerId: string, groups: string[][], positions: Map<string, Box>): Box | null;
