// contrib/demo-gui/src/session/wiring.ts
//
// Wiring presenter — the UNIVERSAL JOIN GESTURE over the session object
// graph, plus the contextual-enablement rules for the selection-scoped
// operations. Pure data-in/data-out (node --test covered by
// test/session-wiring.test.mjs); the DOM shell renders verdicts and reasons,
// it never re-derives them.
//
// The wiring metaphor: every card on the page is a NODE (PSBT fragment,
// session, peer, payment instruction, spendable output, descriptor), and
// connecting two nodes performs the join appropriate to the PAIR:
//
//   fragment ⋈ fragment  = PSBT lattice join            (/api/join, backed)
//   fragment → session   = incorporate into the session (UI membership, backed)
//   peer     → session   = participate: sync the session over the peer's
//                          transport                     (/api/sync, backed)
//   payment  → fragment  = attach the payment record    (/api/pay, backed)
//   utxo     → create    = use the outpoint as a create-form input (backed)
//   session  ⋈ session   = merge converging states      (needs backend)
//   peer     ⋈ peer      = standalone channel           (needs backend)
//   descriptor → fragment = attribute matching scripts  (needs backend)
//
// Pairs with no backend seam yet stay VISIBLE but explicitly unwired
// (allowed=false + a precise `needs` string) — never silently hidden.
//
// Enablement doctrine (override affordances): "impossible" (wrong selection
// arity) is plainly disabled with a reason; "blocked by a correctness gate"
// (e.g. joining ordered fragments — a real spec gate, but pre-BIP-370
// interop needs escape hatches) is OVERRIDABLE: the gate carries a stable id
// the shell arms explicitly, with a warning, and the backend stays the final
// authority. Nothing is bypassed silently.

import type { ClassifyResponse } from "../shared-frontend/core/backend.js";
import { asArray, asNumber, asObject, asString, type FragmentSummary, type SyncTransport } from "./state.js";

// ---------------------------------------------------------------------------
// Object graph: node kinds and the session/peer object models layered over
// the fragment set. Objects are grow-only and immutable-updated, mirroring
// SessionState in ./state.js. "create" is the create-form pseudo-target: it
// participates in wiring (utxo → create) without being a mintable object.
// ---------------------------------------------------------------------------

export type NodeKind =
  | "fragment"
  | "session"
  | "peer"
  | "payment"
  | "utxo"
  | "descriptor"
  | "create";

export interface NodeRef {
  kind: NodeKind;
  key: string;
}

// A session binds a fragment subset to a transport/sync configuration. The
// converging state itself lives server-side (or on peers); this object is
// the UI-model handle the wire gesture manipulates.
export interface SessionObject {
  key: string;
  name: string;
  fragmentKeys: string[];
  transport: SyncTransport;
  // Transport identity material, kept raw: an iroh document ticket, or the
  // manual signaling params for the WebRTC transports.
  irohTicket: string;
  stateFile: string;
  peerKeys: string[];
}

// A peer is the other end of a configured transport: an iroh ticket, an
// npub, or a manual-signaling identity. `identity` stays RAW (pseudonymous
// transport material is opaque to the UI; see
// contrib/design/pseudo-descriptors.md for why delivery identity must not be
// conflated with authorship).
export interface PeerObject {
  key: string;
  name: string;
  transport: SyncTransport | "nostr" | "unknown";
  identity: string;
}

// Payment instruction minted from a BIP 21 / BIP 321 URI paste. The deep
// fields arrive from Backend.classifyPaste (bitcoin-payment-instructions):
// null/[] until the enrichment lands (or when it fails — the shallow parse
// stays authoritative for the fields it produced).
export interface PaymentObject {
  key: string;
  uri: string;
  address: string;
  amountSats: number;
  label: string;
  // "fixed_amount" | "configurable_amount" (deep).
  variant: string | null;
  // Payment methods the instruction carries, one display line each (deep).
  methods: string[];
  description: string | null;
}

// Spendable output minted from a pasted fully-signed transaction. The
// txid/vout/amount fields are pending until Backend.classifyPaste decodes
// the transaction (deep decode is NOT done in the frontend); the raw hex is
// retained either way.
export interface UtxoObject {
  key: string;
  rawTxHex: string;
  txid: string | null;
  vout: number | null;
  amountSats: number | null;
  // Deep decode extras (null until enriched).
  address: string | null;
  // The classify heuristic: every input carries a witness/scriptSig.
  fullySigned: boolean | null;
}

export interface DescriptorObject {
  key: string;
  descriptor: string;
  isPrivate: boolean;
  // Deep fields from Backend.classifyPaste (miniscript): the normalized
  // PUBLIC form (private material never echoes back from the backend), the
  // descriptor type, the authoritative private-key-material flag, and the
  // first derived scripts/addresses. null/[] until enriched.
  normalized: string | null;
  descriptorType: string | null;
  hasPrivateKeys: boolean | null;
  isRanged: boolean | null;
  derived: DerivedScript[];
}

// One derived script from a descriptor (classify's derived[] entries).
export interface DerivedScript {
  index: number;
  scriptPubkeyHex: string;
  address: string | null;
}

export interface ObjectsState {
  sessions: SessionObject[];
  peers: PeerObject[];
  payments: PaymentObject[];
  utxos: UtxoObject[];
  descriptors: DescriptorObject[];
  counter: number;
}

export function emptyObjects(): ObjectsState {
  return { sessions: [], peers: [], payments: [], utxos: [], descriptors: [], counter: 0 };
}

function nextKey(state: ObjectsState, prefix: string): { state: ObjectsState; key: string } {
  const counter = state.counter + 1;
  return { state: { ...state, counter }, key: `${prefix}-${counter}` };
}

export function mintSession(
  state: ObjectsState,
  name: string,
  transport: SyncTransport,
): { state: ObjectsState; session: SessionObject } {
  const next = nextKey(state, "session");
  const session: SessionObject = {
    key: next.key,
    name: name.trim() || next.key,
    fragmentKeys: [],
    transport,
    irohTicket: "",
    stateFile: "",
    peerKeys: [],
  };
  return {
    state: { ...next.state, sessions: [...next.state.sessions, session] },
    session,
  };
}

export function mintPeer(
  state: ObjectsState,
  name: string,
  transport: PeerObject["transport"],
  identity: string,
): { state: ObjectsState; peer: PeerObject } {
  const next = nextKey(state, "peer");
  const peer: PeerObject = {
    key: next.key,
    name: name.trim() || next.key,
    transport,
    identity: identity.trim(),
  };
  return { state: { ...next.state, peers: [...next.state.peers, peer] }, peer };
}

export function mintPayment(
  state: ObjectsState,
  uri: string,
  address: string,
  amountSats: number,
  label: string,
): { state: ObjectsState; payment: PaymentObject } {
  const next = nextKey(state, "payment");
  const payment: PaymentObject = {
    key: next.key,
    uri,
    address,
    amountSats,
    label,
    variant: null,
    methods: [],
    description: null,
  };
  return { state: { ...next.state, payments: [...next.state.payments, payment] }, payment };
}

export function mintUtxo(
  state: ObjectsState,
  rawTxHex: string,
): { state: ObjectsState; utxo: UtxoObject } {
  const next = nextKey(state, "utxo");
  const utxo: UtxoObject = {
    key: next.key,
    rawTxHex,
    txid: null,
    vout: null,
    amountSats: null,
    address: null,
    fullySigned: null,
  };
  return { state: { ...next.state, utxos: [...next.state.utxos, utxo] }, utxo };
}

export function mintDescriptor(
  state: ObjectsState,
  descriptor: string,
  isPrivate: boolean,
): { state: ObjectsState; descriptor: DescriptorObject } {
  const next = nextKey(state, "descriptor");
  const minted: DescriptorObject = {
    key: next.key,
    descriptor: descriptor.trim(),
    isPrivate,
    normalized: null,
    descriptorType: null,
    hasPrivateKeys: null,
    isRanged: null,
    derived: [],
  };
  return {
    state: { ...next.state, descriptors: [...next.state.descriptors, minted] },
    descriptor: minted,
  };
}

// ---------------------------------------------------------------------------
// Deep-classification enrichment: fold a Backend.classifyPaste response into
// the shallow-minted node. Pure and defensive (the details are read like
// inspect JSON); a response of the wrong kind leaves the state untouched, so
// a failed/misrouted enrichment can never damage the shallow card.
// ---------------------------------------------------------------------------

export function enrichDescriptor(
  state: ObjectsState,
  key: string,
  classified: ClassifyResponse,
): ObjectsState {
  if (classified.kind !== "descriptor") return state;
  const derived = (asArray(classified.derived) ?? []).flatMap((raw): DerivedScript[] => {
    const entry = asObject(raw);
    const index = asNumber(entry?.index);
    const scriptPubkeyHex = asString(entry?.script_pubkey_hex);
    if (index === null || scriptPubkeyHex === null) return [];
    return [{ index, scriptPubkeyHex, address: asString(entry?.address) }];
  });
  const hasPrivateKeys = asObject(classified)?.has_private_keys === true;
  return {
    ...state,
    descriptors: state.descriptors.map((descriptor) =>
      descriptor.key === key
        ? {
            ...descriptor,
            normalized: asString(classified.descriptor),
            descriptorType: asString(classified.descriptor_type),
            hasPrivateKeys,
            // The deep flag is authoritative: the shallow regex heuristic
            // only guessed.
            isPrivate: hasPrivateKeys,
            isRanged: asObject(classified)?.is_ranged === true,
            derived,
          }
        : descriptor,
    ),
  };
}

export function enrichPayment(
  state: ObjectsState,
  key: string,
  classified: ClassifyResponse,
): ObjectsState {
  if (classified.kind !== "payment") return state;
  const methods = (asArray(classified.methods) ?? []).flatMap((raw): string[] => {
    const entry = asObject(raw);
    const type = asString(entry?.type);
    if (type === null) return [];
    const detail =
      asString(entry?.address) ?? asString(entry?.invoice) ?? asString(entry?.offer);
    return [detail ? `${type}: ${detail}` : type];
  });
  return {
    ...state,
    payments: state.payments.map((payment) =>
      payment.key === key
        ? {
            ...payment,
            variant: asString(classified.variant),
            methods,
            description: asString(classified.description),
          }
        : payment,
    ),
  };
}

// Fold a transaction decode into the pending utxo node: the FIRST output
// updates the node in place (its key is what the paste flow logged/focused),
// every further output mints a sibling node carrying the same raw hex.
export function applyTxOutputs(
  state: ObjectsState,
  key: string,
  classified: ClassifyResponse,
): { state: ObjectsState; utxos: UtxoObject[] } {
  if (classified.kind !== "transaction") return { state, utxos: [] };
  const source = state.utxos.find((utxo) => utxo.key === key);
  if (!source) return { state, utxos: [] };
  const txid = asString(classified.txid);
  const fullySigned = asObject(classified)?.fully_signed === true;
  const outputs = (asArray(classified.outputs) ?? []).flatMap(
    (raw): { vout: number; amountSats: number | null; address: string | null }[] => {
      const entry = asObject(raw);
      const vout = asNumber(entry?.vout);
      if (vout === null) return [];
      return [{ vout, amountSats: asNumber(entry?.amount_sats), address: asString(entry?.address) }];
    },
  );
  if (txid === null || outputs.length === 0) return { state, utxos: [] };

  const enriched: UtxoObject[] = [];
  let next = state;
  outputs.forEach((output, position) => {
    const fields = {
      rawTxHex: source.rawTxHex,
      txid,
      vout: output.vout,
      amountSats: output.amountSats,
      address: output.address,
      fullySigned,
    };
    if (position === 0) {
      const updated: UtxoObject = { ...source, ...fields };
      next = {
        ...next,
        utxos: next.utxos.map((utxo) => (utxo.key === key ? updated : utxo)),
      };
      enriched.push(updated);
    } else {
      const minted = nextKey(next, "utxo");
      const sibling: UtxoObject = { key: minted.key, ...fields };
      next = { ...minted.state, utxos: [...minted.state.utxos, sibling] };
      enriched.push(sibling);
    }
  });
  return { state: next, utxos: enriched };
}

export function sessionByKey(state: ObjectsState, key: string): SessionObject | null {
  return state.sessions.find((session) => session.key === key) ?? null;
}

export function peerByKey(state: ObjectsState, key: string): PeerObject | null {
  return state.peers.find((peer) => peer.key === key) ?? null;
}

export function addFragmentToSession(
  state: ObjectsState,
  sessionKey: string,
  fragmentKey: string,
): ObjectsState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.key === sessionKey && !session.fragmentKeys.includes(fragmentKey)
        ? { ...session, fragmentKeys: [...session.fragmentKeys, fragmentKey] }
        : session,
    ),
  };
}

export function addPeerToSession(
  state: ObjectsState,
  sessionKey: string,
  peerKey: string,
): ObjectsState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.key === sessionKey && !session.peerKeys.includes(peerKey)
        ? { ...session, peerKeys: [...session.peerKeys, peerKey] }
        : session,
    ),
  };
}

// Fragments removed from the fragment set must also leave session
// memberships (sessions reference fragments by key).
export function dropFragmentKey(state: ObjectsState, fragmentKey: string): ObjectsState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.fragmentKeys.includes(fragmentKey)
        ? { ...session, fragmentKeys: session.fragmentKeys.filter((key) => key !== fragmentKey) }
        : session,
    ),
  };
}

// ---------------------------------------------------------------------------
// Join admissibility: the verdict for wiring `source` into `target`. This
// table IS the enablement rule set for the wire gesture — the shell's only
// job is to render it (highlight admissible targets, name why others are
// not, mark needs-backend pairs).
// ---------------------------------------------------------------------------

export type WireKind =
  | "fragment-join"
  | "fragment-into-session"
  | "peer-into-session"
  | "attach-payment"
  | "add-create-input"
  | "session-merge"
  | "peer-channel"
  | "attribute-scripts"
  | "none";

export interface WireVerdict {
  kind: WireKind;
  // Can the shell perform this wire right now?
  allowed: boolean;
  // Is there a backend/UI seam that implements it? allowed && backed drive
  // the action; !backed pairs render visibly unwired.
  backed: boolean;
  reason: string | null;
  // Precise missing-seam description for !backed pairs (these are the
  // backend tasks the wiring model is waiting on).
  needs: string | null;
}

function verdict(
  kind: WireKind,
  allowed: boolean,
  backed: boolean,
  reason: string | null = null,
  needs: string | null = null,
): WireVerdict {
  return { kind, allowed, backed, reason, needs };
}

function unordered(a: NodeKind, b: NodeKind, x: NodeKind, y: NodeKind): boolean {
  return (a === x && b === y) || (a === y && b === x);
}

export function wireVerdict(source: NodeRef, target: NodeRef, state: ObjectsState): WireVerdict {
  const a = source.kind;
  const b = target.kind;
  if (a === b && source.key === target.key) {
    return verdict("none", false, false, `cannot wire a ${a} to itself`);
  }

  if (a === "fragment" && b === "fragment") {
    return verdict("fragment-join", true, true);
  }

  if (unordered(a, b, "fragment", "session")) {
    const sessionKey = a === "session" ? source.key : target.key;
    const fragmentKey = a === "fragment" ? source.key : target.key;
    const session = sessionByKey(state, sessionKey);
    if (session && session.fragmentKeys.includes(fragmentKey)) {
      return verdict("fragment-into-session", false, true, "fragment is already in the session");
    }
    return verdict("fragment-into-session", true, true);
  }

  if (unordered(a, b, "peer", "session")) {
    const peerKey = a === "peer" ? source.key : target.key;
    const peer = peerByKey(state, peerKey);
    if (peer && peer.transport === "nostr") {
      // The nostr transport is not served by /api/sync yet; keep the pair
      // visible but honestly unwired.
      return verdict(
        "peer-into-session",
        false,
        false,
        null,
        "a nostr transport behind /api/sync (npub peers cannot sync yet)",
      );
    }
    if (peer && (peer.transport === "unknown" || !peer.identity)) {
      return verdict(
        "peer-into-session",
        false,
        true,
        "peer has no usable transport identity (configure a ticket or signaling files)",
      );
    }
    return verdict("peer-into-session", true, true);
  }

  if (unordered(a, b, "payment", "fragment")) {
    return verdict("attach-payment", true, true);
  }

  if (a === "utxo" && b === "create") {
    return verdict("add-create-input", true, true);
  }
  if (a === "utxo" || b === "utxo") {
    return verdict(
      "none",
      false,
      false,
      "spendable outputs feed the create form (chain sources stay manual for now)",
    );
  }

  if (a === "session" && b === "session") {
    return verdict(
      "session-merge",
      false,
      false,
      null,
      "a session-state merge seam (lattice join of two converging session states)",
    );
  }

  if (a === "peer" && b === "peer") {
    return verdict(
      "peer-channel",
      false,
      false,
      null,
      "a standalone peer-to-peer channel establishment seam",
    );
  }

  if (a === "descriptor" && (b === "fragment" || b === "session")) {
    return verdict(
      "attribute-scripts",
      false,
      false,
      null,
      "descriptor derivation (Backend.classifyPaste) to match fragment scripts to the descriptor",
    );
  }

  if (unordered(a, b, "peer", "fragment")) {
    return verdict("none", false, false, "wire the peer to a session; fragments sync through sessions");
  }
  if (a === "payment" && b === "session") {
    return verdict("none", false, false, "wire the payment instruction to a fragment");
  }

  return verdict("none", false, false, `no join is defined for ${a} + ${b}`);
}

// ---------------------------------------------------------------------------
// Wire gesture state machine (tap-first: works identically for click and
// touch — select a source, then pick a target; no drag required).
// ---------------------------------------------------------------------------

export interface WireGesture {
  source: NodeRef | null;
}

export function idleWire(): WireGesture {
  return { source: null };
}

export function beginWire(kind: NodeKind, key: string): WireGesture {
  return { source: { kind, key } };
}

// Tapping the armed source again cancels; tapping any other node yields the
// verdict for the pair (the shell acts on allowed+backed verdicts and
// reports the reason/needs text otherwise).
export function completeWire(
  gesture: WireGesture,
  target: NodeRef,
  state: ObjectsState,
): { gesture: WireGesture; verdict: WireVerdict | null } {
  if (!gesture.source) {
    return { gesture, verdict: null };
  }
  if (gesture.source.kind === target.kind && gesture.source.key === target.key) {
    return { gesture: idleWire(), verdict: null };
  }
  return { gesture: idleWire(), verdict: wireVerdict(gesture.source, target, state) };
}

// ---------------------------------------------------------------------------
// Contextual enablement for the selection-scoped operations.
// ---------------------------------------------------------------------------

export type SessionAction =
  | "join"
  | "concatenate"
  | "sort"
  | "make-unordered"
  | "atomize"
  | "export-v2"
  | "export-bip174"
  | "edit"
  | "pay"
  | "confirm"
  | "payments"
  | "sync"
  | "assign-ids";

export interface GateInfo {
  // Stable id the shell arms to override the gate (per-action, explicit,
  // never silent).
  id: string;
  label: string;
  warning: string;
}

export interface ActionState {
  enabled: boolean;
  // Why the action is disabled (arity or gate label); null when enabled.
  reason: string | null;
  // Present when the ONLY blocker is a correctness gate that may be
  // overridden (pre-BIP-370 interop escape hatch).
  gate: GateInfo | null;
  // True when the gate was armed and the action proceeds despite it — the
  // shell keeps the warning visible.
  overridden: boolean;
  // Names the missing backend seam for operations that are wired to a
  // not-yet-existing route (rendered as "needs backend: …").
  needsBackend: string | null;
}

export interface EnablementContext {
  selected: FragmentSummary[];
  overrides: ReadonlySet<string>;
}

interface ArityRule {
  min: number;
  exactly?: boolean;
}

const ARITY: Record<SessionAction, ArityRule> = {
  join: { min: 2 },
  concatenate: { min: 2 },
  sort: { min: 1, exactly: true },
  "make-unordered": { min: 1, exactly: true },
  atomize: { min: 1, exactly: true },
  "export-v2": { min: 1, exactly: true },
  "export-bip174": { min: 1, exactly: true },
  edit: { min: 1, exactly: true },
  pay: { min: 1, exactly: true },
  confirm: { min: 1, exactly: true },
  payments: { min: 1, exactly: true },
  sync: { min: 1 },
  "assign-ids": { min: 1, exactly: true },
};

function arityReason(action: SessionAction, count: number): string | null {
  const rule = ARITY[action];
  if (rule.exactly && count !== rule.min) {
    return `needs exactly ${rule.min} selected fragment${rule.min === 1 ? "" : "s"} (${count} selected)`;
  }
  if (!rule.exactly && count < rule.min) {
    return `needs at least ${rule.min} selected fragments (${count} selected)`;
  }
  return null;
}

// Correctness gates: what the UI KNOWS (from inspect data) the spec
// disallows, kept overridable because the backend re-validates and interop
// with pre-BIP-370 producers is a real need. Unknown (not-decoded) fragments
// never gate — the backend is the authority on them.
function gateFor(action: SessionAction, selected: FragmentSummary[]): GateInfo | null {
  switch (action) {
    case "join": {
      const ordered = selected.filter((summary) => summary.ordering === "ordered").length;
      if (ordered > 0) {
        return {
          id: "join-ordered",
          label: `${ordered} selected fragment(s) are ordered`,
          warning:
            "the lattice join is defined over unordered fragments; the backend may reject ordered ones. Overriding sends them as-is.",
        };
      }
      return null;
    }
    case "sort": {
      if (selected.length === 1 && selected[0].ordering === "ordered") {
        return {
          id: "sort-ordered",
          label: "fragment is already ordered",
          warning: "sorting an already-ordered PSBT asks the backend to re-run the sorter role on it.",
        };
      }
      return null;
    }
    case "make-unordered": {
      if (selected.length === 1 && selected[0].ordering === "unordered") {
        return {
          id: "make-unordered-unordered",
          label: "fragment is already unordered",
          warning: "re-shuffling an unordered PSBT re-randomizes its element order.",
        };
      }
      return null;
    }
    case "export-bip174": {
      // Observed route behavior: /api/export-bip174 rejects unordered PSBTs
      // ("expects an ordered PSBT; run `ptj sort` first").
      if (selected.length === 1 && selected[0].ordering === "unordered") {
        return {
          id: "export-bip174-unordered",
          label: "fragment is unordered (BIP 174 needs an ordered PSBT)",
          warning:
            "the backend rejects unordered PSBTs for BIP 174 export — run Sort first; overriding sends it anyway and surfaces the route's error.",
        };
      }
      return null;
    }
    case "atomize": {
      if (selected.length !== 1) return null;
      const summary = selected[0];
      if (summary.modifiableInputs === false && summary.modifiableOutputs === false) {
        return {
          id: "atomize-unmodifiable",
          label: "fragment is not modifiable (tx-modifiable flags are clear)",
          warning:
            "atomize parses through the constructor role, which requires modifiable flags; the backend will reject this unless the flags are edited. Overriding sends it as-is.",
        };
      }
      const elements = (summary.inputCount ?? 0) + (summary.outputCount ?? 0);
      if (summary.inputCount !== null && summary.outputCount !== null && elements <= 1) {
        return {
          id: "atomize-atomic",
          label: "fragment is already atomic (one element)",
          warning: "the backend reports 'PSBT is already atomic' for single-element fragments.",
        };
      }
      return null;
    }
    default:
      return null;
  }
}

export function actionState(action: SessionAction, ctx: EnablementContext): ActionState {
  // No selection-scoped action is waiting on a missing seam today
  // (Backend.assignIds landed with the /api/assign-ids route); the field
  // stays so future actions can name theirs.
  const needsBackend: string | null = null;
  const arity = arityReason(action, ctx.selected.length);
  if (arity) {
    return { enabled: false, reason: arity, gate: null, overridden: false, needsBackend };
  }

  if (action === "assign-ids") {
    const summary = ctx.selected[0];
    if (
      summary.outputUidPresent !== null &&
      summary.outputCount !== null &&
      summary.outputUidPresent >= summary.outputCount
    ) {
      return {
        enabled: false,
        reason: "all outputs already carry unique ids",
        gate: null,
        overridden: false,
        needsBackend,
      };
    }
  }

  const gate = gateFor(action, ctx.selected);
  if (gate && !ctx.overrides.has(gate.id)) {
    return { enabled: false, reason: gate.label, gate, overridden: false, needsBackend };
  }
  return {
    enabled: true,
    reason: null,
    gate,
    overridden: gate !== null,
    needsBackend,
  };
}

// ---------------------------------------------------------------------------
// Focus navigation: the mobile-friendly single-session view. The presenter
// owns the navigation STATE (which session fills the viewport, and whether
// the focus is still valid); the breakpoint itself is CSS.
// ---------------------------------------------------------------------------

export interface FocusState {
  mode: "overview" | "session";
  sessionKey: string | null;
}

export function overviewFocus(): FocusState {
  return { mode: "overview", sessionKey: null };
}

export function sessionFocus(key: string): FocusState {
  return { mode: "session", sessionKey: key };
}

// Re-validate focus against the live session list: a focused session that
// disappeared falls back to overview; overview never captures a key.
export function validateFocus(focus: FocusState, sessionKeys: string[]): FocusState {
  if (focus.mode === "session" && focus.sessionKey !== null && sessionKeys.includes(focus.sessionKey)) {
    return focus;
  }
  return overviewFocus();
}
