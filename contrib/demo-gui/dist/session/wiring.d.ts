import type { ClassifyResponse } from "../shared-frontend/core/backend.js";
import { type FragmentSummary, type SyncTransport } from "./state.js";
export type NodeKind = "fragment" | "session" | "peer" | "payment" | "utxo" | "descriptor" | "create";
export interface NodeRef {
    kind: NodeKind;
    key: string;
}
export interface SessionObject {
    key: string;
    name: string;
    fragmentKeys: string[];
    peerKeys: string[];
}
export interface PeerObject {
    key: string;
    name: string;
    transport: SyncTransport | "nostr" | "unknown";
    identity: string;
}
export interface PaymentObject {
    key: string;
    uri: string;
    address: string;
    amountSats: number;
    label: string;
    variant: string | null;
    methods: string[];
    description: string | null;
}
export interface UtxoObject {
    key: string;
    rawTxHex: string;
    txid: string | null;
    vout: number | null;
    amountSats: number | null;
    address: string | null;
    fullySigned: boolean | null;
}
export interface DescriptorObject {
    key: string;
    descriptor: string;
    isPrivate: boolean;
    normalized: string | null;
    descriptorType: string | null;
    hasPrivateKeys: boolean | null;
    isRanged: boolean | null;
    derived: DerivedScript[];
}
export interface DerivedScript {
    index: number;
    scriptPubkeyHex: string;
    address: string | null;
}
export interface PeerBridge {
    a: string;
    b: string;
}
export interface ObjectsState {
    sessions: SessionObject[];
    peers: PeerObject[];
    payments: PaymentObject[];
    utxos: UtxoObject[];
    descriptors: DescriptorObject[];
    bridges: PeerBridge[];
    counter: number;
}
export declare function emptyObjects(): ObjectsState;
export declare function mintSession(state: ObjectsState, name: string): {
    state: ObjectsState;
    session: SessionObject;
};
export declare function mintPeer(state: ObjectsState, name: string, transport: PeerObject["transport"], identity: string): {
    state: ObjectsState;
    peer: PeerObject;
    created: boolean;
};
export declare function mintPayment(state: ObjectsState, uri: string, address: string, amountSats: number, label: string): {
    state: ObjectsState;
    payment: PaymentObject;
};
export declare function mintUtxo(state: ObjectsState, rawTxHex: string): {
    state: ObjectsState;
    utxo: UtxoObject;
};
export declare function mintDescriptor(state: ObjectsState, descriptor: string, isPrivate: boolean): {
    state: ObjectsState;
    descriptor: DescriptorObject;
};
export declare function enrichDescriptor(state: ObjectsState, key: string, classified: ClassifyResponse): ObjectsState;
export declare function enrichPayment(state: ObjectsState, key: string, classified: ClassifyResponse): ObjectsState;
export declare function applyTxOutputs(state: ObjectsState, key: string, classified: ClassifyResponse): {
    state: ObjectsState;
    utxos: UtxoObject[];
};
export declare function sessionByKey(state: ObjectsState, key: string): SessionObject | null;
export declare function peerByKey(state: ObjectsState, key: string): PeerObject | null;
export declare function addFragmentToSession(state: ObjectsState, sessionKey: string, fragmentKey: string): ObjectsState;
export declare function addPeerToSession(state: ObjectsState, sessionKey: string, peerKey: string): ObjectsState;
export declare function dropFragmentKey(state: ObjectsState, fragmentKey: string): ObjectsState;
export declare function fragmentSessionKeys(state: ObjectsState, fragmentKey: string): string[];
export declare function mineFragmentKeys(fragmentKeys: readonly string[], state: ObjectsState): string[];
export interface SessionMergeResult {
    state: ObjectsState;
    merged: SessionObject | null;
    notes: string[];
}
export declare function mergeSessions(state: ObjectsState, leftKey: string, rightKey: string): SessionMergeResult;
export declare function addBridge(state: ObjectsState, aKey: string, bKey: string): ObjectsState;
export declare function peerBridgeGroups(state: ObjectsState): string[][];
export declare function bridgeGroupContaining(state: ObjectsState, peerKey: string): string[];
export declare function unionBridgedPeersIntoSessions(state: ObjectsState): ObjectsState;
export declare function peerUsableForSync(peer: PeerObject): boolean;
export type WireKind = "fragment-join" | "fragment-into-session" | "peer-into-session" | "attach-payment" | "add-create-input" | "session-merge" | "peer-bridge" | "attribute-scripts" | "none";
export interface WireVerdict {
    kind: WireKind;
    allowed: boolean;
    backed: boolean;
    reason: string | null;
    needs: string | null;
    label: string | null;
}
export declare function nodeDisplayName(ref: NodeRef, state: ObjectsState): string;
export type WireDisposition = "compatible" | "blocked" | "unbacked";
export declare function wireDisposition(v: WireVerdict): WireDisposition;
export declare function wireVerdict(source: NodeRef, target: NodeRef, state: ObjectsState): WireVerdict;
export interface PendingWire {
    source: NodeRef;
    target: NodeRef;
}
export declare function wireKey(a: NodeRef, b: NodeRef): string;
export interface QueueWireResult {
    wires: PendingWire[];
    queued: boolean;
    duplicate: boolean;
    verdict: WireVerdict;
}
export declare function queueWire(wires: PendingWire[], source: NodeRef, target: NodeRef, state: ObjectsState): QueueWireResult;
export declare function unqueueWire(wires: PendingWire[], key: string): PendingWire[];
export declare function nodeExists(ref: NodeRef, state: ObjectsState, fragmentKeys: readonly string[]): boolean;
export declare function pruneWires(wires: PendingWire[], state: ObjectsState, fragmentKeys: readonly string[]): PendingWire[];
export interface WireComponent {
    nodes: NodeRef[];
    wires: PendingWire[];
}
export declare function wireComponents(wires: PendingWire[]): WireComponent[];
export interface FragmentJoinGroup {
    fragments: string[];
    wires: PendingWire[];
}
export interface ComponentPlan {
    joinGroups: FragmentJoinGroup[];
    rest: PendingWire[];
}
export declare function componentPlan(component: WireComponent): ComponentPlan;
export declare function remapWireRef(ref: NodeRef, remap: ReadonlyMap<string, string>): NodeRef;
export interface WireQueueSummary {
    wireCount: number;
    componentCount: number;
    text: string;
}
export declare function wireQueueSummary(wires: PendingWire[]): WireQueueSummary;
export interface WireGesture {
    source: NodeRef | null;
}
export declare function idleWire(): WireGesture;
export declare function beginWire(kind: NodeKind, key: string): WireGesture;
export declare function completeWire(gesture: WireGesture, target: NodeRef, state: ObjectsState): {
    gesture: WireGesture;
    verdict: WireVerdict | null;
};
export type SessionAction = "join" | "concatenate" | "sort" | "make-unordered" | "atomize" | "export-v2" | "export-bip174" | "edit" | "pay" | "confirm" | "payments" | "sync" | "assign-ids";
export type GateOverrideFix = {
    kind: "set-tx-modifiable";
} | {
    kind: "sort-first";
};
export interface GateInfo {
    id: string;
    label: string;
    warning: string;
    fix: GateOverrideFix | null;
}
export interface ActionState {
    enabled: boolean;
    reason: string | null;
    gate: GateInfo | null;
    overridden: boolean;
    needsBackend: string | null;
}
export interface EnablementContext {
    selected: FragmentSummary[];
    overrides: ReadonlySet<string>;
}
export declare function actionState(action: SessionAction, ctx: EnablementContext): ActionState;
export interface FocusState {
    mode: "overview" | "session";
    sessionKey: string | null;
}
export declare function overviewFocus(): FocusState;
export declare function sessionFocus(key: string): FocusState;
export declare function validateFocus(focus: FocusState, sessionKeys: string[]): FocusState;
