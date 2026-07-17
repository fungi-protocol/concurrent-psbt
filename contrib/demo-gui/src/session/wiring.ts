// contrib/demo-gui/src/session/wiring.ts
//
// Wiring presenter — the UNIVERSAL JOIN GESTURE over the session object
// graph, plus the contextual-enablement rules for the selection-scoped
// operations. Pure data-in/data-out (node --test covered by
// test/session-wiring.test.mjs); the DOM shell renders verdicts and reasons,
// it never re-derives them.
//
// The wiring metaphor: every card on the page is a NODE (PSBT fragment,
// session, peer, spendable output, descriptor), and connecting two nodes
// performs the join appropriate to the PAIR:
//
//   fragment ⋈ fragment  = PSBT lattice join            (/api/join, backed)
//   fragment → session   = incorporate into the session (UI membership, backed)
//   peer     → session   = participate: sync the session over the peer's
//                          transport                     (/api/sync, backed)
//   utxo     → create    = use the outpoint as a create-form input (backed)
//   session  ⋈ session   = MERGE: sessions are fragment-state carriers, so
//                          the merge joins their fragment states (via the
//                          join route) and UNIONS their peer connections —
//                          peers of both see the combined session. Client-
//                          orchestrated over the UI model + /api/join; a
//                          future backend session-state seam would own the
//                          server-side converging state.
//   peer     ⋈ peer      = BRIDGE: the group renders as one peer and every
//                          member receives every broadcast — equivalent to
//                          wiring the session to each member. UI grouping;
//                          broadcasts go through the existing per-member
//                          sync where a transport is configured.
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
  | "utxo"
  | "descriptor"
  | "create";

export interface NodeRef {
  kind: NodeKind;
  key: string;
}

// A session is a MONOTONE SHARED MEMORY REGISTER for PSBT fragments:
// fragments are values, the session holds ONE growing lattice value
// (contentKey, null while the register is empty). Writing a fragment in is
// content ⊔ fragment — always legal, absorbed when the register already
// contains it. The session also carries its authorized peer set. It has NO
// transport of its own: transports and their identity material belong to
// the PEERS, and reaching the session goes over each peer's own transport.
// With best-effort gossip over those peers the register is eventually
// consistent (which does not promise a VALID transaction — e.g. too many
// inputs/outputs is still a well-formed lattice value).
export interface SessionObject {
  key: string;
  name: string;
  contentKey: string | null;
  peerKeys: string[];
  // What each authorized peer's REPLICA is known to hold (peer key → the
  // register values delivered to or received from it). GROW-ONLY, like
  // everything else here: a delivery only ever adds to what a replica is
  // known to hold, so out-of-order settles commute and stale knowledge is
  // never overwritten by an older in-flight delivery. Distribution is
  // need-based, not a button: a replica not known to hold contentKey
  // cannot compute the same LUB, so the shell broadcasts to it as soon as
  // the register changes (and a freshly authorized peer, holding nothing,
  // receives the current value the same way).
  replicas: Record<string, string[]>;
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

// A peer-peer bridge edge (unordered pair of peer keys). The transitive
// closure of these edges partitions peers into BRIDGE GROUPS (the demo's
// peerBridgeComponents): a group renders as one peer, and broadcasts to any
// member address every member.
export interface PeerBridge {
  a: string;
  b: string;
}

export interface ObjectsState {
  sessions: SessionObject[];
  peers: PeerObject[];
  utxos: UtxoObject[];
  descriptors: DescriptorObject[];
  bridges: PeerBridge[];
  counter: number;
}

export function emptyObjects(): ObjectsState {
  return {
    sessions: [],
    peers: [],
    utxos: [],
    descriptors: [],
    bridges: [],
    counter: 0,
  };
}

function nextKey(state: ObjectsState, prefix: string): { state: ObjectsState; key: string } {
  const counter = state.counter + 1;
  return { state: { ...state, counter }, key: `${prefix}-${counter}` };
}

export function mintSession(
  state: ObjectsState,
  name: string,
): { state: ObjectsState; session: SessionObject } {
  const next = nextKey(state, "session");
  const session: SessionObject = {
    key: next.key,
    name: name.trim() || next.key,
    contentKey: null,
    peerKeys: [],
    replicas: {},
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
): { state: ObjectsState; peer: PeerObject; created: boolean } {
  const normalizedIdentity = identity.trim();
  const existing = normalizedIdentity
    ? state.peers.find(
        (peer) => peer.transport === transport && peer.identity === normalizedIdentity,
      )
    : undefined;
  if (existing) return { state, peer: existing, created: false };

  const next = nextKey(state, "peer");
  const peer: PeerObject = {
    key: next.key,
    name: name.trim() || next.key,
    transport,
    identity: normalizedIdentity,
  };
  return {
    state: { ...next.state, peers: [...next.state.peers, peer] },
    peer,
    created: true,
  };
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

// Set the register's value. The join itself (old content ⊔ fragment) is the
// shell's job — it owns the async backend seam; this is the pure write-back.
export function writeSessionContent(
  state: ObjectsState,
  sessionKey: string,
  fragmentKey: string,
): ObjectsState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.key === sessionKey ? { ...session, contentKey: fragmentKey } : session,
    ),
  };
}

// Peer↔session wiring is AUTHORIZATION: the peer may read/write the
// register. The peer set is a grow-only set-union, so re-authorizing is an
// idempotent no-op — callers report it, never error on it.
export function authorizePeerOnSession(
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

// The authorized peers whose replicas are not known to hold the register's
// current value — the peers a change must be broadcast to before every
// replica can compute the same LUB. Empty registers have nothing to
// distribute; a peer holding nothing (never delivered to) is stale.
export function staleReplicaPeers(session: SessionObject): string[] {
  const contentKey = session.contentKey;
  if (!contentKey) return [];
  return session.peerKeys.filter(
    (peerKey) => !(session.replicas[peerKey] ?? []).includes(contentKey),
  );
}

// Record a delivery (or a receipt): these peers' replicas now hold the
// fragment. Grow-only knowledge — a delivery only ever ADDS to what a
// replica is known to hold (settles of concurrent broadcasts commute; an
// older delivery landing late can never erase a newer one). Staleness is
// DERIVED against contentKey, so a register advance implicitly re-flags
// every peer.
export function markReplicas(
  state: ObjectsState,
  sessionKey: string,
  peerKeys: readonly string[],
  fragmentKey: string,
): ObjectsState {
  return {
    ...state,
    sessions: state.sessions.map((session) => {
      if (session.key !== sessionKey) return session;
      const replicas = { ...session.replicas };
      for (const peerKey of peerKeys) {
        const held = replicas[peerKey] ?? [];
        if (!held.includes(fragmentKey)) replicas[peerKey] = [...held, fragmentKey];
      }
      return { ...session, replicas };
    }),
  };
}

// Fragments removed from the fragment set must also leave the registers
// that reference them (a register whose value is dropped becomes empty).
export function dropFragmentKey(state: ObjectsState, fragmentKey: string): ObjectsState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.contentKey === fragmentKey ? { ...session, contentKey: null } : session,
    ),
  };
}

// ---------------------------------------------------------------------------
// MINE, the pseudo-peer (Q6): the container of all SESSIONLESS local
// fragments. Membership is DERIVED, never stored — a fragment lives in Mine
// exactly when no session carries it — so loaded/created fragments default
// there and wiring one into a session (publishing) moves it out with no
// extra bookkeeping. Local-only workflows (join, sort, edit, atomize) act
// on fragments wherever they live; Mine is where they happen before
// anything is published.
// ---------------------------------------------------------------------------

export function fragmentSessionKeys(state: ObjectsState, fragmentKey: string): string[] {
  return sessionsHolding(state, fragmentKey).map((session) => session.key);
}

export function mineFragmentKeys(
  fragmentKeys: readonly string[],
  state: ObjectsState,
): string[] {
  return fragmentKeys.filter(
    (fragmentKey) => !state.sessions.some((session) => session.contentKey === fragmentKey),
  );
}

// A derivation's result REPLACES its sources: fragments are value types, so
// once a join/sort/edit/assign-ids has minted the new value the stale source
// copies retire instead of piling grow-only clutter into Mine. These are the
// keys such a settlement drops: sources that still exist, are not themselves
// among the results (the op deduped onto an operand), and are not any
// register's content — registers change only through explicit write
// gestures, so a session's copy of the value always survives.
export function retiredByDerivation(
  sourceKeys: readonly string[],
  resultKeys: readonly string[],
  state: ObjectsState,
  fragmentKeys: readonly string[],
): string[] {
  return sourceKeys.filter(
    (key) =>
      !resultKeys.includes(key) &&
      fragmentKeys.includes(key) &&
      !state.sessions.some((session) => session.contentKey === key),
  );
}

// ---------------------------------------------------------------------------
// Monotonicity of sessions — ALL of them: a register only advances by ⊔.
// Its value cannot be withdrawn or rewritten in place; the one honest
// escape hatch for a non-monotone transform is a FORK: abort the session
// and mint a new one in its stead (same name, same peer connections,
// register seeded with the transformed value). Sharing raises the stakes
// (peers hold copies of a published value, and observe the abort plus the
// new session) but does not change the discipline.
// ---------------------------------------------------------------------------

export function sessionIsShared(session: SessionObject): boolean {
  return session.peerKeys.length > 0;
}

export function sessionsHolding(state: ObjectsState, fragmentKey: string): SessionObject[] {
  return state.sessions.filter((session) => session.contentKey === fragmentKey);
}

export function forkSession(
  state: ObjectsState,
  sessionKey: string,
  contentKey: string,
): { state: ObjectsState; forked: SessionObject | null } {
  const source = sessionByKey(state, sessionKey);
  if (!source) return { state, forked: null };
  const next = nextKey(state, "session");
  const forked: SessionObject = {
    key: next.key,
    name: source.name,
    contentKey,
    peerKeys: [...source.peerKeys],
    // Peers still hold the ABORTED session's value: the carried-over
    // holdings lack the forked content, so the shell re-broadcasts.
    replicas: { ...source.replicas },
  };
  return {
    state: {
      ...next.state,
      sessions: [
        ...next.state.sessions.filter((session) => session.key !== source.key),
        forked,
      ],
    },
    forked,
  };
}

// ---------------------------------------------------------------------------
// Session merge (session ⋈ session, per Q3): merging two registers JOINS
// their contents and takes the UNION of their peer sets as the merged
// session's peer set. This function is the UI-MODEL half: it mints the
// merged session (peer union, provisional content) and retires the two
// sources; the shell joins the two content values through the existing
// /api/join route and writes the result back — `contents` hands it both
// operand keys. What this does NOT merge — any server-side converging
// session state — is named in the notes so the shell logs it honestly.
// ---------------------------------------------------------------------------

export interface SessionMergeResult {
  state: ObjectsState;
  merged: SessionObject | null;
  // The two registers' values at merge time, for the shell to ⊔ and write
  // back (null entries are empty registers — nothing to join on that side).
  contents: { left: string | null; right: string | null };
  // Honest-logging notes: what the UI-model merge cannot do (server-side
  // converging state). Transports live on peers, and the peer union carries
  // every connection along — there is no transport to conflict.
  notes: string[];
}

// ⊔ of two replica maps: per-peer union of held values. Both inputs are
// grow-only knowledge, so the merge is their join — never a pick.
function unionReplicas(
  left: Record<string, string[]>,
  right: Record<string, string[]>,
): Record<string, string[]> {
  const union = { ...left };
  for (const [peerKey, held] of Object.entries(right)) {
    const existing = union[peerKey] ?? [];
    union[peerKey] = [...existing, ...held.filter((key) => !existing.includes(key))];
  }
  return union;
}

export function mergeSessions(
  state: ObjectsState,
  leftKey: string,
  rightKey: string,
): SessionMergeResult {
  const left = sessionByKey(state, leftKey);
  const right = sessionByKey(state, rightKey);
  if (!left || !right || left.key === right.key) {
    return { state, merged: null, contents: { left: null, right: null }, notes: [] };
  }
  const notes: string[] = [];
  notes.push(
    "server-side session state (if any) is NOT merged — the UI-model merge joins " +
      "the register contents via /api/join and unions peer connections; a backend " +
      "session-state merge seam would own the converging state itself",
  );
  const next = nextKey(state, "session");
  const merged: SessionObject = {
    key: next.key,
    name: `${left.name}+${right.name}`,
    // Provisional: the lone value when only one register holds one; when
    // both do, the shell writes the joined result over this.
    contentKey: left.contentKey ?? right.contentKey,
    peerKeys: [...left.peerKeys, ...right.peerKeys.filter((key) => !left.peerKeys.includes(key))],
    // Holdings union, per peer: whatever a peer held, it holds — a replica
    // not holding the merged content reads as stale, which is exactly
    // right after a merge.
    replicas: unionReplicas(left.replicas, right.replicas),
  };
  return {
    state: {
      ...next.state,
      sessions: [
        ...next.state.sessions.filter(
          (session) => session.key !== left.key && session.key !== right.key,
        ),
        merged,
      ],
    },
    merged,
    contents: { left: left.contentKey, right: right.contentKey },
    notes,
  };
}

// ---------------------------------------------------------------------------
// Peer bridges (peer ⋈ peer, per Q3): a bridge renders the group as ONE
// peer where every member receives every broadcast — equivalent to wiring
// the session to each member. Pure UI grouping over grow-only bridge edges;
// the shell broadcasts through the existing per-member sync where a member
// has a configured transport.
// ---------------------------------------------------------------------------

function bridgePairKey(a: string, b: string): string {
  return [a, b].sort().join("::");
}

export function addBridge(state: ObjectsState, aKey: string, bKey: string): ObjectsState {
  if (aKey === bKey) return state;
  const key = bridgePairKey(aKey, bKey);
  if (state.bridges.some((bridge) => bridgePairKey(bridge.a, bridge.b) === key)) {
    return state;
  }
  return { ...state, bridges: [...state.bridges, { a: aKey, b: bKey }] };
}

// Connected components over the bridge edges, in peer-list order (the
// demo's peerBridgeComponents). Singleton groups are included: every peer
// belongs to exactly one group.
export function peerBridgeGroups(state: ObjectsState): string[][] {
  const peerKeys = state.peers.map((peer) => peer.key);
  const order = new Map(peerKeys.map((key, index) => [key, index]));
  const adjacency = new Map<string, Set<string>>(peerKeys.map((key) => [key, new Set()]));
  for (const bridge of state.bridges) {
    if (!adjacency.has(bridge.a) || !adjacency.has(bridge.b)) continue;
    adjacency.get(bridge.a)!.add(bridge.b);
    adjacency.get(bridge.b)!.add(bridge.a);
  }
  const groups: string[][] = [];
  const seen = new Set<string>();
  for (const peerKey of peerKeys) {
    if (seen.has(peerKey)) continue;
    const stack = [peerKey];
    const group: string[] = [];
    while (stack.length) {
      const current = stack.pop()!;
      if (seen.has(current)) continue;
      seen.add(current);
      group.push(current);
      for (const next of adjacency.get(current)!) stack.push(next);
    }
    groups.push(group.sort((a, b) => Number(order.get(a)) - Number(order.get(b))));
  }
  return groups;
}

export function bridgeGroupContaining(state: ObjectsState, peerKey: string): string[] {
  return peerBridgeGroups(state).find((group) => group.includes(peerKey)) ?? [peerKey];
}

// After a bridge lands, every session already wired to ANY member of the
// group is wired to EVERY member (the Q3 equivalence: bridging = wiring the
// session to each member). Idempotent.
export function unionBridgedPeersIntoSessions(state: ObjectsState): ObjectsState {
  const groups = peerBridgeGroups(state).filter((group) => group.length > 1);
  if (!groups.length) return state;
  return {
    ...state,
    sessions: state.sessions.map((session) => {
      let peerKeys = session.peerKeys;
      for (const group of groups) {
        if (group.some((peerKey) => peerKeys.includes(peerKey))) {
          peerKeys = [...peerKeys, ...group.filter((peerKey) => !peerKeys.includes(peerKey))];
        }
      }
      return peerKeys === session.peerKeys ? session : { ...session, peerKeys };
    }),
  };
}

// A member with a transport /api/sync can drive today. Members without one
// (nostr, unconfigured) stay visible in the group; broadcasts to them are
// reported pending-backend by the shell.
export function peerUsableForSync(peer: PeerObject): boolean {
  return peer.transport !== "nostr" && peer.transport !== "unknown" && peer.identity !== "";
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
  | "add-create-input"
  | "session-merge"
  | "peer-bridge"
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
  // Human-readable ACTION label ("Join psbt-1 into psbt-2", "Publish psbt-1
  // to session lunch") for every DEFINED pair — the demo's wire-tooltip
  // vocabulary. null only when no join is defined at all (kind "none").
  // The label serves both the tap path (target titles, status bar) and any
  // future drag path (a live ribbon shows the same string).
  label: string | null;
}

function verdict(
  kind: WireKind,
  allowed: boolean,
  backed: boolean,
  reason: string | null = null,
  needs: string | null = null,
  label: string | null = null,
): WireVerdict {
  return { kind, allowed, backed, reason, needs, label };
}

// Wiring state tracks only keys; verdicts that depend on a fragment's VALUE
// (register compatibility below) read it through this shell-provided lookup.
// Optional everywhere it is threaded: without it the verdict falls back to
// identity-only rules, which is what the pure-model tests exercise too.
export type FragmentSummaryLookup = (key: string) => FragmentSummary | null;

// Register value compatibility: a register only advances by ⊔, so it can
// only hold values whose concurrent evolution IS a join. A BIP 370 PSBT
// that is MODIFIABLE but ORDERED admits index-sensitive adds with no
// lattice order over them — only non-modifiable values (updatable/signable:
// they still grow by signatures and field updates) and unordered-modifiable
// values (the concurrent constructor) are session-compatible.
export function registerIncompatibility(summary: FragmentSummary): string | null {
  const modifiable = summary.modifiableInputs === true || summary.modifiableOutputs === true;
  if (!modifiable || summary.ordering === "unordered") return null;
  return (
    "a modifiable but ORDERED PSBT cannot live in a register (concurrent adds " +
    "to an ordered list have no join) — make it unordered first, or clear its " +
    "modifiable flags"
  );
}

function unordered(a: NodeKind, b: NodeKind, x: NodeKind, y: NodeKind): boolean {
  return (a === x && b === y) || (a === y && b === x);
}

// Display name for a node in wire action labels: sessions and peers carry
// human names; fragments (and everything else) go by their key, which is
// already the visible card title.
export function nodeDisplayName(ref: NodeRef, state: ObjectsState): string {
  switch (ref.kind) {
    case "session":
      return sessionByKey(state, ref.key)?.name ?? ref.key;
    case "peer":
      return peerByKey(state, ref.key)?.name ?? ref.key;
    default:
      return ref.key;
  }
}

// Three-way target vocabulary (the demo's halo colors, ported to cards):
//   compatible — wire executes now (green outline);
//   blocked    — the pair is defined and backed, but a rule refuses it right
//                now (red-tinted outline + the reason);
//   unbacked   — no join is defined, or the seam that would implement it
//                does not exist yet (dimmed + reason/needs).
export type WireDisposition = "compatible" | "blocked" | "unbacked";

export function wireDisposition(v: WireVerdict): WireDisposition {
  if (v.allowed && v.backed) return "compatible";
  if (v.backed) return "blocked";
  return "unbacked";
}

// A fragment that IS a session's register content stands for its session in
// EVERY wire: the content card shows register state, not a Mine draft, so a
// gesture touching it means the session holding it. Fragment→content writes
// into the register (the session computes the LUB and absorbs the operand),
// content→content merges the sessions, content→peer authorizes the peer —
// and content→its-own-session collapses to a refused self-wire. Mine
// fragments are never register contents, so only in-session cards resolve.
export function resolveWireEndpoint(ref: NodeRef, state: ObjectsState): NodeRef {
  if (ref.kind !== "fragment") return ref;
  const holder = state.sessions.find((session) => session.contentKey === ref.key);
  return holder ? { kind: "session", key: holder.key } : ref;
}

export function wireVerdict(
  source: NodeRef,
  target: NodeRef,
  state: ObjectsState,
  summaryOf?: FragmentSummaryLookup,
): WireVerdict {
  const resolvedSource = resolveWireEndpoint(source, state);
  const resolvedTarget = resolveWireEndpoint(target, state);
  if (resolvedSource !== source || resolvedTarget !== target) {
    return wireVerdict(resolvedSource, resolvedTarget, state, summaryOf);
  }
  const a = source.kind;
  const b = target.kind;
  const sourceName = nodeDisplayName(source, state);
  const targetName = nodeDisplayName(target, state);
  if (a === b && source.key === target.key) {
    return verdict("none", false, false, `cannot wire a ${a} to itself`);
  }

  if (a === "fragment" && b === "fragment") {
    const label = `Join ${sourceName} into ${targetName}`;
    return verdict("fragment-join", true, true, null, null, label);
  }

  if (unordered(a, b, "fragment", "session")) {
    // Writing a value to a monotone register is ALWAYS legal — as long as
    // the VALUE is register-compatible: the register takes content ⊔
    // fragment, so the fragment must have a join-shaped future (see
    // registerIncompatibility). A compatible value it already contains is
    // an absorbed join the shell reports — never a refusal ("already in the
    // session" is nonsensical for a register).
    const fragmentKey = a === "fragment" ? source.key : target.key;
    const label = `Write ${fragmentKey} into session ${
      a === "session" ? sourceName : targetName
    } (⊔ into the register)`;
    const summary = summaryOf?.(fragmentKey) ?? null;
    const incompatibility = summary ? registerIncompatibility(summary) : null;
    if (incompatibility) {
      return verdict("fragment-into-session", false, true, incompatibility, null, label);
    }
    return verdict("fragment-into-session", true, true, null, null, label);
  }

  if (unordered(a, b, "peer", "session")) {
    // Peer↔session is AUTHORIZATION: connect the peer to the register so it
    // can read/write it. The UI model owns the peer set, so the wire is
    // executable; actually reaching the peer stays a sync-time transport
    // concern (surfaced there, not here). Re-authorizing is an idempotent
    // no-op the shell reports.
    const peerKey = a === "peer" ? source.key : target.key;
    const sessionName = a === "session" ? sourceName : targetName;
    const peerName = a === "peer" ? sourceName : targetName;
    const group = bridgeGroupContaining(state, peerKey)
      .map((memberKey) => peerByKey(state, memberKey))
      .filter((member): member is PeerObject => member !== null);
    const groupLabel =
      group.length > 1
        ? `bridge ${group.map((member) => member.name).join("+")}`
        : `peer ${peerName}`;
    const label = `Authorize ${groupLabel} on session ${sessionName}`;
    return verdict("peer-into-session", true, true, null, null, label);
  }

  if (a === "utxo" && b === "create") {
    return verdict(
      "add-create-input",
      true,
      true,
      null,
      null,
      `Use ${sourceName} as a create-form input`,
    );
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
    // Client-orchestrated merge: ⊔ the two registers' contents over the
    // existing join route, ∪ their peer sets as the merged session's peer
    // set. The server-side converging state (once it exists) stays with a
    // future backend session-state seam — the shell logs that honestly.
    const label = `Merge sessions ${sourceName} and ${targetName} (⊔ contents, ∪ peers)`;
    if (!sessionByKey(state, source.key) || !sessionByKey(state, target.key)) {
      return verdict(
        "session-merge",
        false,
        true,
        "session no longer exists (it may have been merged already)",
        null,
        label,
      );
    }
    return verdict("session-merge", true, true, null, null, label);
  }

  if (a === "peer" && b === "peer") {
    if (bridgeGroupContaining(state, source.key).includes(target.key)) {
      return verdict(
        "peer-bridge",
        false,
        true,
        "peers are already bridged",
        null,
        `Bridge peers ${sourceName}, ${targetName}`,
      );
    }
    return verdict(
      "peer-bridge",
      true,
      true,
      null,
      null,
      `Bridge peers ${sourceName}, ${targetName}`,
    );
  }

  if (a === "descriptor" && (b === "fragment" || b === "session")) {
    return verdict(
      "attribute-scripts",
      false,
      false,
      null,
      "descriptor derivation (Backend.classifyPaste) to match fragment scripts to the descriptor",
      `Attribute ${sourceName} scripts to ${targetName}`,
    );
  }

  if (unordered(a, b, "peer", "fragment")) {
    return verdict("none", false, false, "wire the peer to a session; fragments sync through sessions");
  }

  return verdict("none", false, false, `no join is defined for ${a} + ${b}`);
}

// ---------------------------------------------------------------------------
// Pending-wire queue (the demo's join-select graph, ported): completing a
// wire gesture QUEUES the edge instead of executing it, visualizing the
// flexibility of the join operation. Each queued edge carries its own Join;
// the toolbar Join applies whole connected components. Everything here is
// pure — the shell holds the PendingWire[] and calls the backend.
//
// Like the demo, local joins REPLACE their operands with the LUB (the
// shell retires sessionless operand copies after the join); a component's
// fragment-join cluster still executes as ONE n-ary /api/join call rather
// than successive pairwise LUBs, and the component's remaining wires run
// with consumed fragment endpoints remapped to the cluster's result.
// ---------------------------------------------------------------------------

export interface PendingWire {
  source: NodeRef;
  target: NodeRef;
}

function nodeId(ref: NodeRef): string {
  return `${ref.kind}:${ref.key}`;
}

// Direction-insensitive identity of a wire (the demo's joinWireKey).
export function wireKey(a: NodeRef, b: NodeRef): string {
  return [nodeId(a), nodeId(b)].sort().join("::");
}

export interface QueueWireResult {
  wires: PendingWire[];
  // The edge is newly queued (false for rejected verdicts AND duplicates).
  queued: boolean;
  // True when the pair was already in the queue.
  duplicate: boolean;
  // The verdict the queue decision was based on — the shell reports its
  // label (queued) or reason/needs (rejected).
  verdict: WireVerdict;
}

// Only compatible wires queue; blocked/unbacked verdicts come back for the
// shell's rejection feedback. Duplicates (either direction) are no-ops.
export function queueWire(
  wires: PendingWire[],
  source: NodeRef,
  target: NodeRef,
  state: ObjectsState,
  summaryOf?: FragmentSummaryLookup,
): QueueWireResult {
  // The queue stores CANONICAL endpoints (a content card resolves to its
  // session) so execution, edges, and duplicate detection all see the wire
  // the verdict was about.
  const canonicalSource = resolveWireEndpoint(source, state);
  const canonicalTarget = resolveWireEndpoint(target, state);
  const v = wireVerdict(canonicalSource, canonicalTarget, state, summaryOf);
  if (wireDisposition(v) !== "compatible") {
    return { wires, queued: false, duplicate: false, verdict: v };
  }
  const key = wireKey(canonicalSource, canonicalTarget);
  if (wires.some((wire) => wireKey(wire.source, wire.target) === key)) {
    return { wires, queued: false, duplicate: true, verdict: v };
  }
  return {
    wires: [...wires, { source: canonicalSource, target: canonicalTarget }],
    queued: true,
    duplicate: false,
    verdict: v,
  };
}

export function unqueueWire(wires: PendingWire[], key: string): PendingWire[] {
  return wires.filter((wire) => wireKey(wire.source, wire.target) !== key);
}

export function nodeExists(
  ref: NodeRef,
  state: ObjectsState,
  fragmentKeys: readonly string[],
): boolean {
  switch (ref.kind) {
    case "fragment":
      return fragmentKeys.includes(ref.key);
    case "session":
      return sessionByKey(state, ref.key) !== null;
    case "peer":
      return peerByKey(state, ref.key) !== null;
    case "utxo":
      return state.utxos.some((utxo) => utxo.key === ref.key);
    case "descriptor":
      return state.descriptors.some((descriptor) => descriptor.key === ref.key);
    case "create":
      return true;
  }
}

// Re-validate the queue against current state (the demo's validJoinWires):
// wires lose their place when an endpoint disappears or the pair's verdict
// is no longer compatible (e.g. the fragment was published to the session
// through another path). Endpoints re-canonicalize first — a fragment that
// became a register's content while its wire waited now stands for that
// session, so the wire executes as the write it now means (and wires that
// collapse onto an already-queued pair, or onto themselves, drop out).
export function pruneWires(
  wires: PendingWire[],
  state: ObjectsState,
  fragmentKeys: readonly string[],
  summaryOf?: FragmentSummaryLookup,
): PendingWire[] {
  const seen = new Set<string>();
  const live: PendingWire[] = [];
  for (const wire of wires) {
    const source = resolveWireEndpoint(wire.source, state);
    const target = resolveWireEndpoint(wire.target, state);
    if (!nodeExists(source, state, fragmentKeys) || !nodeExists(target, state, fragmentKeys)) {
      continue;
    }
    if (wireDisposition(wireVerdict(source, target, state, summaryOf)) !== "compatible") continue;
    const key = wireKey(source, target);
    if (seen.has(key)) continue;
    seen.add(key);
    live.push({ source, target });
  }
  return live;
}

export interface WireComponent {
  // Distinct endpoints in first-seen queue order.
  nodes: NodeRef[];
  // The component's wires in queue order.
  wires: PendingWire[];
}

// Connected components of the pending-wire graph (the demo's
// joinComponents): the toolbar Join applies each component as a unit.
export function wireComponents(wires: PendingWire[]): WireComponent[] {
  const adjacency = new Map<string, Set<string>>();
  const refs = new Map<string, NodeRef>();
  for (const wire of wires) {
    const a = nodeId(wire.source);
    const b = nodeId(wire.target);
    refs.set(a, wire.source);
    refs.set(b, wire.target);
    if (!adjacency.has(a)) adjacency.set(a, new Set());
    if (!adjacency.has(b)) adjacency.set(b, new Set());
    adjacency.get(a)!.add(b);
    adjacency.get(b)!.add(a);
  }
  const components: WireComponent[] = [];
  const seen = new Set<string>();
  for (const startId of adjacency.keys()) {
    if (seen.has(startId)) continue;
    const stack = [startId];
    const memberIds = new Set<string>();
    while (stack.length) {
      const current = stack.pop()!;
      if (seen.has(current)) continue;
      seen.add(current);
      memberIds.add(current);
      for (const next of adjacency.get(current) ?? []) stack.push(next);
    }
    // First-seen order over the queue keeps the report deterministic.
    const nodes: NodeRef[] = [];
    for (const wire of wires) {
      for (const ref of [wire.source, wire.target]) {
        const id = nodeId(ref);
        if (memberIds.has(id) && !nodes.some((candidate) => nodeId(candidate) === id)) {
          nodes.push(ref);
        }
      }
    }
    components.push({
      nodes,
      wires: wires.filter(
        (wire) => memberIds.has(nodeId(wire.source)) && memberIds.has(nodeId(wire.target)),
      ),
    });
  }
  return components;
}

export interface FragmentJoinGroup {
  // Distinct fragment keys connected by fragment-join wires (>= 2): one
  // n-ary /api/join call joins the whole group.
  fragments: string[];
  // The queued wires the group consumes when it joins.
  wires: PendingWire[];
}

export interface ComponentPlan {
  joinGroups: FragmentJoinGroup[];
  // The component's non-fragment-join wires in queue order; the shell
  // remaps fragment endpoints through the clusters' join results before
  // executing each one.
  rest: PendingWire[];
}

export function componentPlan(component: WireComponent): ComponentPlan {
  const joinWires = component.wires.filter(
    (wire) => wire.source.kind === "fragment" && wire.target.kind === "fragment",
  );
  const clusters = wireComponents(joinWires);
  return {
    joinGroups: clusters
      .map((cluster) => ({
        fragments: cluster.nodes.map((ref) => ref.key),
        wires: cluster.wires,
      }))
      .filter((group) => group.fragments.length >= 2),
    rest: component.wires.filter(
      (wire) => !(wire.source.kind === "fragment" && wire.target.kind === "fragment"),
    ),
  };
}

// Follow a consumed endpoint to its result (the session-side analog of the
// demo's remapNodeId): fragment clusters remap their members to the n-ary
// join result, merged sessions remap both sources to the merged session.
// Keys are kind-qualified ("fragment:psbt-1", "session:session-2") so the
// two namespaces cannot collide.
export function remapWireRef(ref: NodeRef, remap: ReadonlyMap<string, string>): NodeRef {
  const mapped = remap.get(`${ref.kind}:${ref.key}`);
  return mapped === undefined ? ref : { kind: ref.kind, key: mapped };
}

export interface WireQueueSummary {
  wireCount: number;
  componentCount: number;
  text: string;
}

export function wireQueueSummary(wires: PendingWire[]): WireQueueSummary {
  const wireCount = wires.length;
  const componentCount = wireComponents(wires).length;
  return {
    wireCount,
    componentCount,
    text:
      wireCount === 0
        ? "no pending wires"
        : `${wireCount} pending wire${wireCount === 1 ? "" : "s"} in ` +
          `${componentCount} component${componentCount === 1 ? "" : "s"}`,
  };
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
  summaryOf?: FragmentSummaryLookup,
): { gesture: WireGesture; verdict: WireVerdict | null } {
  if (!gesture.source) {
    return { gesture, verdict: null };
  }
  if (gesture.source.kind === target.kind && gesture.source.key === target.key) {
    return { gesture: idleWire(), verdict: null };
  }
  return { gesture: idleWire(), verdict: wireVerdict(gesture.source, target, state, summaryOf) };
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

// The repair an ARMED override applies before running the gated action.
// Overriding a gate the backend is KNOWN to reject would be a bypass in name
// only (a guaranteed 400), so those gates carry the fix as data: the shell
// executes it (minting fragments along the way — grow-only) and runs the
// action on the result. Gates without a fix keep send-as-is semantics.
export type GateOverrideFix =
  // Raw-edit PSBT_GLOBAL_TX_MODIFIABLE (raw global key 06) to 3 via
  // /api/edit, then act on the minted fragment.
  | { kind: "set-tx-modifiable" }
  // Run the sorter role (/api/sort) first, then act on the minted ordered
  // fragment.
  | { kind: "sort-first" };

export interface GateInfo {
  // Stable id the shell arms to override the gate (per-action, explicit,
  // never silent).
  id: string;
  label: string;
  warning: string;
  // Present when the override APPLIES this repair instead of sending the
  // blocked request as-is.
  fix: GateOverrideFix | null;
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
          fix: null,
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
          fix: null,
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
          fix: null,
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
            "the backend rejects unordered PSBTs for BIP 174 export — overriding runs the sorter role first (the sort-seed field applies; a random seed is generated when the fragment carries none), mints the ordered fragment, and exports THAT.",
          fix: { kind: "sort-first" },
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
            "atomize parses through the constructor role, which requires modifiable flags; overriding performs the raw edit — /api/edit sets the TX_MODIFIABLE flags on a NEW fragment — and atomizes that fragment.",
          fix: { kind: "set-tx-modifiable" },
        };
      }
      const elements = (summary.inputCount ?? 0) + (summary.outputCount ?? 0);
      if (summary.inputCount !== null && summary.outputCount !== null && elements <= 1) {
        return {
          id: "atomize-atomic",
          label: "fragment is already atomic (one element)",
          warning: "the backend reports 'PSBT is already atomic' for single-element fragments.",
          fix: null,
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

  // assign-ids stays available when every output already carries an id:
  // backend-minted fragments ALWAYS do (/api/create assigns ids), and the
  // panel this action opens owns the id-complete cases explicitly (manual
  // per-output ids, the overwrite-existing-ids checkbox). Arity is the only
  // pre-condition.

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
