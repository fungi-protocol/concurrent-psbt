import test from "node:test";
import assert from "node:assert/strict";

import {
  actionState,
  addBridge,
  applyTxOutputs,
  authorizePeerOnSession,
  beginWire,
  bridgeGroupContaining,
  completeWire,
  dropFragmentKey,
  emptyObjects,
  enrichDescriptor,
  idleWire,
  fragmentSessionKeys,
  mergeSessions,
  mineFragmentKeys,
  mintDescriptor,
  mintPeer,
  mintSession,
  componentPlan,
  mintUtxo,
  nodeDisplayName,
  nodeExists,
  overviewFocus,
  peerBridgeGroups,
  peerByKey,
  peerUsableForSync,
  pruneWires,
  queueWire,
  remapWireRef,
  sessionByKey,
  sessionFocus,
  unionBridgedPeersIntoSessions,
  unqueueWire,
  validateFocus,
  wireComponents,
  wireDisposition,
  wireKey,
  wireQueueSummary,
  wireVerdict,
  writeSessionContent,
} from "../dist/session/wiring.js";

function summary(overrides = {}) {
  return {
    format: "bip370",
    ordering: "unordered",
    inputCount: 1,
    outputCount: 2,
    sortMode: "unset",
    seedHex: null,
    uniqueIdHex: "11".repeat(32),
    knownInputSats: 200000,
    outputSats: 150000,
    feeSats: 50000,
    modifiableInputs: true,
    modifiableOutputs: true,
    outputUidPresent: 2,
    ...overrides,
  };
}

function ref(kind, key) {
  return { kind, key };
}

// --- object model -----------------------------------------------------------

test("minting sessions, peers, utxos, and descriptors is grow-only", () => {
  let state = emptyObjects();
  const s1 = mintSession(state, "  alpha  ");
  state = s1.state;
  assert.equal(s1.session.key, "session-1");
  assert.equal(s1.session.name, "alpha");
  assert.equal(s1.session.contentKey, null); // an empty register

  const p1 = mintPeer(state, "", "iroh", " doc-ticket ");
  state = p1.state;
  assert.equal(p1.peer.key, "peer-2"); // one shared counter: keys never collide
  assert.equal(p1.peer.name, "peer-2"); // blank names fall back to the key
  assert.equal(p1.peer.identity, "doc-ticket");

  const utxo = mintUtxo(state, "02000000...");
  state = utxo.state;
  assert.equal(utxo.utxo.key, "utxo-3");
  // Deep decode is a backend seam: minted spendable outputs stay pending.
  assert.equal(utxo.utxo.txid, null);
  assert.equal(utxo.utxo.vout, null);

  const desc = mintDescriptor(state, " wpkh(xpub6...) ", false);
  state = desc.state;
  assert.equal(desc.descriptor.key, "descriptor-4");
  assert.equal(desc.descriptor.descriptor, "wpkh(xpub6...)");

  assert.equal(sessionByKey(state, "session-1").name, "alpha");
  assert.equal(sessionByKey(state, "nope"), null);
  assert.equal(peerByKey(state, "peer-2").transport, "iroh");
  assert.equal(peerByKey(state, "nope"), null);
});

test("adding an ephemeral peer deduplicates the exact transport address without side effects", () => {
  let state = emptyObjects();
  state = mintSession(state, "lunch").state;

  const first = mintPeer(state, "Alice", "nostr", " npub1alice ");
  assert.equal(first.created, true);
  assert.equal(first.peer.identity, "npub1alice");

  const repeated = mintPeer(first.state, "renamed by a duplicate paste", "nostr", "npub1alice");
  assert.equal(repeated.created, false);
  assert.equal(repeated.peer.key, first.peer.key);
  assert.equal(repeated.peer.name, "Alice");
  assert.strictEqual(repeated.state, first.state);
  assert.deepEqual(repeated.state.sessions, state.sessions);
  assert.deepEqual(repeated.state.bridges, []);

  const distinctTransport = mintPeer(repeated.state, "Alice over iroh", "iroh", "npub1alice");
  assert.equal(distinctTransport.created, true);
  assert.equal(distinctTransport.state.peers.length, 2);
});

test("register writes: set the content, authorize peers idempotently, drop nulls", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state;
  // A session is a single-value register: each write replaces the content
  // key (the shell computes old ⊔ new and writes the RESULT in).
  state = writeSessionContent(state, "session-1", "psbt-1");
  assert.equal(sessionByKey(state, "session-1").contentKey, "psbt-1");
  state = writeSessionContent(state, "session-1", "psbt-2");
  assert.equal(sessionByKey(state, "session-1").contentKey, "psbt-2");

  // Peer authorization is a grow-only set union: re-auth is a no-op.
  state = authorizePeerOnSession(state, "session-1", "peer-9");
  state = authorizePeerOnSession(state, "session-1", "peer-9");
  assert.deepEqual(sessionByKey(state, "session-1").peerKeys, ["peer-9"]);

  // Dropping the fragment a register holds empties the register.
  state = dropFragmentKey(state, "psbt-2");
  assert.equal(sessionByKey(state, "session-1").contentKey, null);

  // Unknown session keys are a no-op, not a throw.
  const untouched = writeSessionContent(state, "session-404", "psbt-3");
  assert.deepEqual(untouched, state);
});

// --- join admissibility ------------------------------------------------------

test("fragment-fragment wires to the lattice join; self-wire is refused", () => {
  const state = emptyObjects();
  const join = wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state);
  assert.equal(join.kind, "fragment-join");
  assert.equal(join.allowed, true);
  assert.equal(join.backed, true);

  const self = wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-1"), state);
  assert.equal(self.allowed, false);
  assert.match(self.reason, /itself/);
});

test("fragment-session wiring is symmetric and ALWAYS legal (⊔ into the register)", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state;
  state = writeSessionContent(state, "session-1", "psbt-1");

  const fresh = wireVerdict(ref("fragment", "psbt-2"), ref("session", "session-1"), state);
  assert.equal(fresh.kind, "fragment-into-session");
  assert.equal(fresh.allowed, true);
  assert.equal(fresh.backed, true);

  const reversed = wireVerdict(ref("session", "session-1"), ref("fragment", "psbt-2"), state);
  assert.equal(reversed.kind, "fragment-into-session");
  assert.equal(reversed.allowed, true);

  // The verdict never peeks at PSBT values, so a fragment the register
  // already subsumes still wires in — the shell reports the ABSORBED join
  // ("already in the session" is nonsense for a monotone register). Only
  // the register's own content card refuses, as a self-wire (below).
  const contained = wireVerdict(ref("fragment", "psbt-3"), ref("session", "session-1"), state);
  assert.equal(contained.allowed, true);
  assert.equal(contained.backed, true);
  assert.equal(contained.reason, null);

  // Dragging the content card onto its own session collapses to a
  // self-wire: the card IS the register value, there is nothing to write.
  const self = wireVerdict(ref("fragment", "psbt-1"), ref("session", "session-1"), state);
  assert.equal(self.allowed, false);
  assert.match(self.reason, /itself/);
});

test("peer-session wiring is authorization: allowed, backed, transport-agnostic", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state;
  state = mintPeer(state, "good", "iroh", "doc-abc").state;
  state = mintPeer(state, "blank", "unknown", "").state;
  state = mintPeer(state, "nostr", "nostr", "npub1xyz").state;

  // Connecting a peer to a session is a UI-model authorization (grow the
  // peer set); actually REACHING the peer is a sync-time transport concern
  // surfaced there, not a verdict refusal here.
  const good = wireVerdict(ref("peer", "peer-2"), ref("session", "session-1"), state);
  assert.equal(good.kind, "peer-into-session");
  assert.equal(good.allowed, true);
  assert.equal(good.backed, true);
  assert.equal(good.needs, null);

  // Even peers with no usable transport identity (blank, nostr) can be
  // authorized — the register's peer set does not gate on reachability.
  const blank = wireVerdict(ref("peer", "peer-3"), ref("session", "session-1"), state);
  assert.equal(blank.allowed, true);

  const nostr = wireVerdict(ref("session", "session-1"), ref("peer", "peer-4"), state);
  assert.equal(nostr.allowed, true);
  assert.equal(nostr.backed, true);
});

test("utxo wiring rows", () => {
  const state = emptyObjects();
  const create = wireVerdict(ref("utxo", "utxo-1"), ref("create", "create"), state);
  assert.equal(create.kind, "add-create-input");
  assert.equal(create.allowed, true);
  assert.equal(create.backed, true);

  const toFragment = wireVerdict(ref("utxo", "utxo-1"), ref("fragment", "psbt-1"), state);
  assert.equal(toFragment.allowed, false);
  assert.match(toFragment.reason, /create form/);
});

test("session merge and peer bridge are wired; attribute-scripts still names its seam", () => {
  let state = emptyObjects();
  state = mintSession(state, "a").state;
  state = mintSession(state, "b").state;

  // Q3: session ⋈ session = MERGE, client-orchestrated (join the fragment
  // states via the join route + union the peer connections in UI state).
  const merge = wireVerdict(ref("session", "session-1"), ref("session", "session-2"), state);
  assert.equal(merge.kind, "session-merge");
  assert.equal(merge.allowed, true);
  assert.equal(merge.backed, true);

  // A vanished session (already merged away) blocks instead of queueing.
  const gone = wireVerdict(ref("session", "session-1"), ref("session", "session-9"), state);
  assert.equal(gone.kind, "session-merge");
  assert.equal(gone.allowed, false);
  assert.equal(gone.backed, true);
  assert.match(gone.reason, /no longer exists/);

  // Q3: peer ⋈ peer = BRIDGE (UI grouping; the group renders as one peer).
  const bridge = wireVerdict(ref("peer", "peer-1"), ref("peer", "peer-2"), state);
  assert.equal(bridge.kind, "peer-bridge");
  assert.equal(bridge.allowed, true);
  assert.equal(bridge.backed, true);

  const attribute = wireVerdict(ref("descriptor", "descriptor-1"), ref("fragment", "psbt-1"), state);
  assert.equal(attribute.kind, "attribute-scripts");
  assert.equal(attribute.backed, false);
  assert.match(attribute.needs, /classifyPaste/);
});

test("undefined pairs are refused with a reason", () => {
  const state = emptyObjects();
  const peerFragment = wireVerdict(ref("peer", "peer-1"), ref("fragment", "psbt-1"), state);
  assert.equal(peerFragment.allowed, false);
  assert.match(peerFragment.reason, /through sessions/);

  const descriptorPeer = wireVerdict(ref("descriptor", "descriptor-1"), ref("peer", "peer-1"), state);
  assert.equal(descriptorPeer.kind, "none");
  assert.match(descriptorPeer.reason, /no join is defined/);
});

test("a register's content card wires to peers as its session", () => {
  // Joining a fragment VALUE to a peer has no meaning, so a gesture on the
  // content card unambiguously refers to the session holding it: the wire
  // resolves to peer↔session authorization, in either direction.
  let state = emptyObjects();
  state = mintSession(state, "lunch").state;
  state = mintPeer(state, "alice", "iroh", "doc-abc").state;
  state = writeSessionContent(state, "session-1", "psbt-1");

  const fromContent = wireVerdict(ref("fragment", "psbt-1"), ref("peer", "peer-1"), state);
  assert.equal(fromContent.kind, "peer-into-session");
  assert.equal(wireDisposition(fromContent), "compatible");
  assert.match(fromContent.label, /Authorize peer .+ on session lunch/);
  const ontoContent = wireVerdict(ref("peer", "peer-1"), ref("fragment", "psbt-1"), state);
  assert.equal(ontoContent.kind, "peer-into-session");

  // The queue stores the CANONICAL endpoints: the same wire queued through
  // the content card and through the session card is one wire, and its
  // stored refs execute as an authorization, not a fragment join.
  const viaContent = queueWire([], ref("peer", "peer-1"), ref("fragment", "psbt-1"), state);
  assert.equal(viaContent.queued, true);
  assert.deepEqual(viaContent.wires[0].target, { kind: "session", key: "session-1" });
  const viaSession = queueWire(viaContent.wires, ref("peer", "peer-1"), ref("session", "session-1"), state);
  assert.equal(viaSession.duplicate, true);

  // A MINE fragment (no session holds it) keeps the honest refusal.
  const mine = wireVerdict(ref("fragment", "psbt-9"), ref("peer", "peer-1"), state);
  assert.equal(mine.allowed, false);
  assert.match(mine.reason, /through sessions/);
});

test("a register's content card stands for its session in every wire", () => {
  // The content card shows register STATE, not a Mine draft — a gesture
  // touching it means the session holding it. Wiring a Mine fragment onto
  // the content card is therefore a register write (the session computes
  // the LUB and absorbs the operand), NOT a fragment join minted into Mine.
  let state = emptyObjects();
  state = mintSession(state, "lunch").state;
  state = mintSession(state, "rent").state;
  state = writeSessionContent(state, "session-1", "psbt-1");
  state = writeSessionContent(state, "session-2", "psbt-2");

  const write = wireVerdict(ref("fragment", "psbt-9"), ref("fragment", "psbt-1"), state);
  assert.equal(write.kind, "fragment-into-session");
  assert.equal(wireDisposition(write), "compatible");
  assert.match(write.label, /Write psbt-9 into session lunch/);

  // Content card onto another session's content card = session merge.
  const merge = wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state);
  assert.equal(merge.kind, "session-merge");
  assert.equal(wireDisposition(merge), "compatible");

  // The queue canonicalizes: through the content card and through the
  // session card is the SAME wire.
  const viaContent = queueWire([], ref("fragment", "psbt-9"), ref("fragment", "psbt-1"), state);
  assert.equal(viaContent.queued, true);
  assert.deepEqual(viaContent.wires[0].target, { kind: "session", key: "session-1" });
  const viaSession = queueWire(
    viaContent.wires,
    ref("fragment", "psbt-9"),
    ref("session", "session-1"),
    state,
  );
  assert.equal(viaSession.duplicate, true);
});

test("pruneWires re-canonicalizes endpoints that became register contents", () => {
  // A fragment⋈fragment wire is queued while both are Mine drafts; one of
  // them is then written into a register while the wire waits. The queued
  // wire follows it: it now means "write the other fragment into that
  // session" and must execute as that write, not as a Mine-minting join.
  let state = emptyObjects();
  state = mintSession(state, "lunch").state;
  const queued = queueWire([], ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state);
  assert.equal(queued.queued, true);

  state = writeSessionContent(state, "session-1", "psbt-2");
  const live = pruneWires(queued.wires, state, ["psbt-1", "psbt-2"]);
  assert.deepEqual(live, [
    { source: { kind: "fragment", key: "psbt-1" }, target: { kind: "session", key: "session-1" } },
  ]);

  // Re-canonicalization that collapses a wire onto itself drops it: the
  // queued write's fragment became the register's OWN content.
  const selfWire = queueWire([], ref("fragment", "psbt-1"), ref("session", "session-1"), state);
  assert.equal(selfWire.queued, true);
  state = writeSessionContent(state, "session-1", "psbt-1");
  assert.deepEqual(pruneWires(selfWire.wires, state, ["psbt-1", "psbt-2"]), []);

  // ...and two wires that canonicalize to the same pair keep one copy.
  state = writeSessionContent(state, "session-1", "psbt-2");
  const viaSession = queueWire([], ref("fragment", "psbt-1"), ref("session", "session-1"), state);
  const doubled = [
    ...viaSession.wires,
    { source: { kind: "fragment", key: "psbt-1" }, target: { kind: "fragment", key: "psbt-2" } },
  ];
  assert.equal(pruneWires(doubled, state, ["psbt-1", "psbt-2"]).length, 1);
});

// --- action labels + target vocabulary ----------------------------------------

test("wire verdicts carry concrete action labels built from display names", () => {
  let state = emptyObjects();
  state = mintSession(state, "lunch").state;
  state = mintPeer(state, "alice", "iroh", "doc-abc").state;
  state = mintUtxo(state, "020000dead").state;
  state = mintDescriptor(state, "wpkh(xpub6...)", false).state;

  assert.equal(nodeDisplayName(ref("fragment", "psbt-7"), state), "psbt-7");
  assert.equal(nodeDisplayName(ref("session", "session-1"), state), "lunch");
  assert.equal(nodeDisplayName(ref("peer", "peer-2"), state), "alice");
  // Unknown keys and label-less objects fall back to the key.
  assert.equal(nodeDisplayName(ref("session", "session-404"), state), "session-404");

  assert.equal(
    wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).label,
    "Join psbt-1 into psbt-2",
  );
  assert.equal(
    wireVerdict(ref("fragment", "psbt-1"), ref("session", "session-1"), state).label,
    "Write psbt-1 into session lunch (⊔ into the register)",
  );
  // Symmetric pairs label the same action regardless of direction.
  assert.equal(
    wireVerdict(ref("session", "session-1"), ref("fragment", "psbt-1"), state).label,
    "Write psbt-1 into session lunch (⊔ into the register)",
  );
  assert.equal(
    wireVerdict(ref("peer", "peer-2"), ref("session", "session-1"), state).label,
    "Authorize peer alice on session lunch",
  );
  assert.equal(
    wireVerdict(ref("utxo", "utxo-3"), ref("create", "create"), state).label,
    "Use utxo-3 as a create-form input",
  );
  assert.equal(
    wireVerdict(ref("session", "session-1"), ref("session", "session-9"), state).label,
    "Merge sessions lunch and session-9 (⊔ contents, ∪ peers)",
  );
  assert.equal(
    wireVerdict(ref("peer", "peer-2"), ref("peer", "peer-9"), state).label,
    "Bridge peers alice, peer-9",
  );
  assert.equal(
    wireVerdict(ref("descriptor", "descriptor-4"), ref("fragment", "psbt-1"), state).label,
    "Attribute descriptor-4 scripts to psbt-1",
  );

  // Undefined pairs carry no action label.
  assert.equal(wireVerdict(ref("peer", "peer-2"), ref("fragment", "psbt-1"), state).label, null);
  assert.equal(
    wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-1"), state).label,
    null,
  );
});

test("wire disposition: compatible / blocked / unbacked three-way vocabulary", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state;
  state = mintPeer(state, "npub", "nostr", "npub1xyz").state;

  // allowed && backed → compatible.
  const join = wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state);
  assert.equal(wireDisposition(join), "compatible");

  // backed but refused right now → blocked (red vocabulary): merging with
  // a session that no longer exists.
  const gone = wireVerdict(ref("session", "session-1"), ref("session", "session-9"), state);
  assert.equal(wireDisposition(gone), "blocked");

  // Defined pair waiting on a seam → unbacked (dim vocabulary)…
  const attribute = wireVerdict(ref("descriptor", "descriptor-1"), ref("fragment", "psbt-1"), state);
  assert.equal(wireDisposition(attribute), "unbacked");
  // …and so are pairs with no join defined at all.
  const none = wireVerdict(ref("peer", "peer-2"), ref("fragment", "psbt-1"), state);
  assert.equal(wireDisposition(none), "unbacked");
});

// --- wire gesture ------------------------------------------------------------

test("wire gesture arms, cancels on re-tap, and yields verdicts", () => {
  const state = emptyObjects();
  assert.equal(idleWire().source, null);

  const armed = beginWire("fragment", "psbt-1");
  assert.deepEqual(armed.source, { kind: "fragment", key: "psbt-1" });

  // Re-tapping the source cancels without a verdict.
  const cancelled = completeWire(armed, { kind: "fragment", key: "psbt-1" }, state);
  assert.equal(cancelled.gesture.source, null);
  assert.equal(cancelled.verdict, null);

  const completed = completeWire(armed, { kind: "fragment", key: "psbt-2" }, state);
  assert.equal(completed.gesture.source, null);
  assert.equal(completed.verdict.kind, "fragment-join");

  // Completing from idle is a no-op.
  const idle = completeWire(idleWire(), { kind: "fragment", key: "psbt-2" }, state);
  assert.equal(idle.verdict, null);
});

// --- pending-wire queue --------------------------------------------------------

test("wireKey is direction-insensitive; queueWire dedupes both directions", () => {
  const state = emptyObjects();
  const a = ref("fragment", "psbt-1");
  const b = ref("fragment", "psbt-2");
  assert.equal(wireKey(a, b), wireKey(b, a));

  const first = queueWire([], a, b, state);
  assert.equal(first.queued, true);
  assert.equal(first.duplicate, false);
  assert.equal(first.wires.length, 1);
  assert.equal(first.verdict.label, "Join psbt-1 into psbt-2");

  const again = queueWire(first.wires, b, a, state);
  assert.equal(again.queued, false);
  assert.equal(again.duplicate, true);
  assert.equal(again.wires.length, 1);
});

test("queueWire refuses non-compatible verdicts and returns them for reporting", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state;

  // Blocked (merge with a vanished session): not queued, verdict says why.
  const blocked = queueWire([], ref("session", "session-1"), ref("session", "session-9"), state);
  assert.equal(blocked.queued, false);
  assert.equal(blocked.duplicate, false);
  assert.deepEqual(blocked.wires, []);
  assert.match(blocked.verdict.reason, /no longer exists/);

  // Unbacked (descriptor attribution before its seam): not queued either.
  const unbacked = queueWire([], ref("descriptor", "descriptor-1"), ref("fragment", "psbt-1"), state);
  assert.equal(unbacked.queued, false);
  assert.match(unbacked.verdict.needs, /classifyPaste/);
});

test("unqueueWire removes exactly the keyed wire", () => {
  const state = emptyObjects();
  let wires = queueWire([], ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).wires;
  wires = queueWire(wires, ref("fragment", "psbt-2"), ref("fragment", "psbt-3"), state).wires;
  assert.equal(wires.length, 2);

  const key = wireKey(ref("fragment", "psbt-2"), ref("fragment", "psbt-1"));
  const rest = unqueueWire(wires, key);
  assert.equal(rest.length, 1);
  assert.equal(rest[0].source.key, "psbt-2");
  assert.equal(rest[0].target.key, "psbt-3");
});

test("nodeExists and pruneWires: vanished endpoints and stale verdicts drop", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state;
  const fragments = ["psbt-1", "psbt-2"];

  assert.equal(nodeExists(ref("fragment", "psbt-1"), state, fragments), true);
  assert.equal(nodeExists(ref("fragment", "psbt-9"), state, fragments), false);
  assert.equal(nodeExists(ref("session", "session-1"), state, fragments), true);
  assert.equal(nodeExists(ref("session", "session-9"), state, fragments), false);
  assert.equal(nodeExists(ref("create", "create"), state, []), true);

  let wires = queueWire([], ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).wires;
  wires = queueWire(wires, ref("fragment", "psbt-1"), ref("session", "session-1"), state).wires;
  assert.equal(wires.length, 2);

  // Everything still valid: prune keeps both. Once psbt-1 becomes the
  // register's OWN content its write wire collapses to a self-wire and
  // drops (nothing left to write), while the join wire follows the
  // fragment into the session and lives on as a register write.
  assert.equal(pruneWires(wires, state, fragments).length, 2);
  const written = writeSessionContent(state, "session-1", "psbt-1");
  const rewritten = pruneWires(wires, written, fragments);
  assert.equal(rewritten.length, 1);
  assert.deepEqual(rewritten[0].source, { kind: "session", key: "session-1" });
  assert.deepEqual(rewritten[0].target, { kind: "fragment", key: "psbt-2" });

  // A vanished session takes its wires with it (the join survives).
  const sessionGone = pruneWires(wires, emptyObjects(), fragments);
  assert.equal(sessionGone.length, 1);
  assert.equal(sessionGone[0].target.key, "psbt-2");

  // A removed fragment takes its wires with it (psbt-2 gone drops the join;
  // the psbt-1 write wire survives); removing both fragments empties the
  // queue.
  const withoutPsbt2 = pruneWires(wires, state, ["psbt-1"]);
  assert.equal(withoutPsbt2.length, 1);
  assert.equal(withoutPsbt2[0].target.kind, "session");
  assert.deepEqual(pruneWires(wires, state, []), []);
});

test("wireComponents groups queued wires into connected components", () => {
  const state = emptyObjects();
  let wires = queueWire([], ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).wires;
  wires = queueWire(wires, ref("fragment", "psbt-2"), ref("fragment", "psbt-3"), state).wires;
  wires = queueWire(wires, ref("fragment", "psbt-8"), ref("fragment", "psbt-9"), state).wires;

  const components = wireComponents(wires);
  assert.equal(components.length, 2);
  const chain = components.find((component) => component.nodes.length === 3);
  assert.deepEqual(
    chain.nodes.map((node) => node.key),
    ["psbt-1", "psbt-2", "psbt-3"],
  );
  assert.equal(chain.wires.length, 2);
  const pair = components.find((component) => component.nodes.length === 2);
  assert.deepEqual(
    pair.nodes.map((node) => node.key),
    ["psbt-8", "psbt-9"],
  );

  assert.deepEqual(wireComponents([]), []);
});

test("componentPlan collapses fragment-join clusters into n-ary groups", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state;

  // Chain psbt-1 ⋈ psbt-2 ⋈ psbt-3 plus a publish wire into the session:
  // one component, one 3-fragment join group, one remaining wire.
  let wires = queueWire([], ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).wires;
  wires = queueWire(wires, ref("fragment", "psbt-2"), ref("fragment", "psbt-3"), state).wires;
  wires = queueWire(wires, ref("fragment", "psbt-2"), ref("session", "session-1"), state).wires;

  const components = wireComponents(wires);
  assert.equal(components.length, 1);
  const plan = componentPlan(components[0]);
  assert.equal(plan.joinGroups.length, 1);
  assert.deepEqual(plan.joinGroups[0].fragments, ["psbt-1", "psbt-2", "psbt-3"]);
  assert.equal(plan.joinGroups[0].wires.length, 2);
  assert.equal(plan.rest.length, 1);
  assert.equal(plan.rest[0].target.kind, "session");

  // The rest wire executes against the cluster's join result. Remap keys
  // are kind-qualified: fragment joins and session merges share one map.
  const remap = new Map([
    ["fragment:psbt-1", "psbt-4"],
    ["fragment:psbt-2", "psbt-4"],
    ["fragment:psbt-3", "psbt-4"],
    ["session:session-8", "session-10"],
  ]);
  assert.deepEqual(remapWireRef(plan.rest[0].source, remap), {
    kind: "fragment",
    key: "psbt-4",
  });
  assert.deepEqual(remapWireRef(ref("session", "session-8"), remap), {
    kind: "session",
    key: "session-10",
  });
  // Unmapped refs pass through untouched; the namespaces cannot collide.
  assert.deepEqual(remapWireRef(ref("session", "session-1"), remap), ref("session", "session-1"));
  assert.deepEqual(remapWireRef(ref("fragment", "psbt-7"), remap), ref("fragment", "psbt-7"));
  assert.deepEqual(remapWireRef(ref("session", "psbt-1"), remap), ref("session", "psbt-1"));

  // A component with no fragment-join edges plans no join groups.
  const publishOnly = wireComponents(
    queueWire([], ref("fragment", "psbt-7"), ref("session", "session-1"), state).wires,
  );
  const publishPlan = componentPlan(publishOnly[0]);
  assert.deepEqual(publishPlan.joinGroups, []);
  assert.equal(publishPlan.rest.length, 1);
});

test("wireQueueSummary counts wires and components", () => {
  const state = emptyObjects();
  assert.equal(wireQueueSummary([]).text, "no pending wires");

  let wires = queueWire([], ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).wires;
  const one = wireQueueSummary(wires);
  assert.equal(one.wireCount, 1);
  assert.equal(one.componentCount, 1);
  assert.equal(one.text, "1 pending wire in 1 component");

  wires = queueWire(wires, ref("fragment", "psbt-8"), ref("fragment", "psbt-9"), state).wires;
  const two = wireQueueSummary(wires);
  assert.equal(two.wireCount, 2);
  assert.equal(two.componentCount, 2);
  assert.equal(two.text, "2 pending wires in 2 components");
});

// --- MINE, the pseudo-peer (Q6: sessionless local fragments) ---------------------

test("mine membership is derived: fragments no register holds are local-only", () => {
  let state = emptyObjects();
  const fragments = ["psbt-1", "psbt-2", "psbt-3"];

  // No sessions: everything is Mine (loaded/created default there).
  assert.deepEqual(mineFragmentKeys(fragments, state), fragments);

  state = mintSession(state, "s").state; // session-1
  assert.deepEqual(mineFragmentKeys(fragments, state), fragments);

  // Writing a fragment into a register moves it out of Mine.
  state = writeSessionContent(state, "session-1", "psbt-2");
  assert.deepEqual(mineFragmentKeys(fragments, state), ["psbt-1", "psbt-3"]);
  assert.deepEqual(fragmentSessionKeys(state, "psbt-2"), ["session-1"]);
  assert.deepEqual(fragmentSessionKeys(state, "psbt-1"), []);

  // ANY register holding the value keeps it published; two registers can
  // hold the same value, and both are listed as carriers.
  state = mintSession(state, "t").state; // session-2
  state = writeSessionContent(state, "session-2", "psbt-2");
  assert.deepEqual(fragmentSessionKeys(state, "psbt-2"), ["session-1", "session-2"]);
  assert.deepEqual(mineFragmentKeys(fragments, state), ["psbt-1", "psbt-3"]);

  // When no register holds the value any more, it returns to Mine.
  state = dropFragmentKey(state, "psbt-2");
  assert.deepEqual(mineFragmentKeys(fragments, state), fragments);
});

test("mine tracks session merges: the provisional content rides the merged register", () => {
  let state = emptyObjects();
  state = mintSession(state, "a").state; // session-1
  state = mintSession(state, "b").state; // session-2
  state = writeSessionContent(state, "session-1", "psbt-1");
  state = writeSessionContent(state, "session-2", "psbt-2");

  const merge = mergeSessions(state, "session-1", "session-2");
  state = merge.state;
  // The merged register provisionally holds the LEFT value; the shell joins
  // both operand values (handed back in `contents`) and writes the result
  // over it. At the model layer only the provisional value stays published.
  assert.deepEqual(merge.contents, { left: "psbt-1", right: "psbt-2" });
  assert.equal(merge.merged.contentKey, "psbt-1");
  assert.deepEqual(fragmentSessionKeys(state, "psbt-1"), [merge.merged.key]);
  assert.deepEqual(mineFragmentKeys(["psbt-1", "psbt-2", "psbt-3"], state), ["psbt-2", "psbt-3"]);
});

// --- session merge (Q3: join contents + union peer connections) -----------------

test("mergeSessions unions peers, hands back both contents, retires the sources", () => {
  let state = emptyObjects();
  state = mintSession(state, "alpha").state; // session-1
  state = mintSession(state, "beta").state; // session-2
  state = writeSessionContent(state, "session-1", "psbt-1");
  state = writeSessionContent(state, "session-2", "psbt-2");
  state = authorizePeerOnSession(state, "session-1", "peer-a");
  state = authorizePeerOnSession(state, "session-1", "peer-shared");
  state = authorizePeerOnSession(state, "session-2", "peer-shared");
  state = authorizePeerOnSession(state, "session-2", "peer-b");

  const merge = mergeSessions(state, "session-1", "session-2");
  assert.notEqual(merge.merged, null);
  assert.equal(merge.merged.key, "session-3"); // fresh key from the shared counter
  assert.equal(merge.merged.name, "alpha+beta");
  // Peer-set UNION, duplicates collapsed: peers of BOTH see the merged session.
  assert.deepEqual(merge.merged.peerKeys, ["peer-a", "peer-shared", "peer-b"]);
  // Both register values come back for the shell to ⊔ and write over the
  // provisional content.
  assert.deepEqual(merge.contents, { left: "psbt-1", right: "psbt-2" });
  assert.equal(merge.merged.contentKey, "psbt-1");
  // Sessions carry no transport: the peer union brings every connection
  // along, so there is nothing to conflict and no transport note.
  assert.equal("transport" in merge.merged, false);
  assert.ok(!merge.notes.some((note) => /transport/.test(note)));
  // The UI-model merge always names what it cannot merge (the future
  // backend session-state seam).
  assert.ok(merge.notes.some((note) => /session-state merge seam|NOT merged/.test(note)));
  // The sources are retired; only the merged session remains.
  assert.deepEqual(
    merge.state.sessions.map((sessionObject) => sessionObject.key),
    ["session-3"],
  );

  // One empty register: the lone value stands, nothing to join.
  let lone = emptyObjects();
  lone = mintSession(lone, "a").state; // session-1
  lone = mintSession(lone, "b").state; // session-2
  lone = writeSessionContent(lone, "session-2", "psbt-9");
  const loneMerge = mergeSessions(lone, "session-1", "session-2");
  assert.deepEqual(loneMerge.contents, { left: null, right: "psbt-9" });
  assert.equal(loneMerge.merged.contentKey, "psbt-9");

  // Missing keys and self-merges are no-ops.
  assert.equal(mergeSessions(state, "session-1", "session-9").merged, null);
  assert.equal(mergeSessions(state, "session-1", "session-1").merged, null);
  assert.deepEqual(mergeSessions(state, "session-1", "session-9").state, state);
});

test("a session is only a register and peers — identity material lives on peers", () => {
  // Transports and their identity material (tickets, disk paths) belong to
  // PeerObject; SessionObject carries none of it, so a merge has no
  // transport or ticket to reconcile.
  let state = emptyObjects();
  state = mintSession(state, "a").state;
  state = mintSession(state, "b").state;
  const merge = mergeSessions(state, "session-1", "session-2");
  assert.deepEqual(Object.keys(merge.merged).sort(), ["contentKey", "key", "name", "peerKeys"]);
  assert.equal(merge.merged.contentKey, null); // ⊥ ⊔ ⊥ is still empty
  assert.ok(!merge.notes.some((note) => /transport|ticket/.test(note)));
});

// --- peer bridges (Q3: the group renders as one peer) ----------------------------

test("addBridge is grow-only and groups are transitive", () => {
  let state = emptyObjects();
  state = mintPeer(state, "alice", "iroh", "doc-a").state; // peer-1
  state = mintPeer(state, "bob", "iroh", "doc-b").state; // peer-2
  state = mintPeer(state, "carol", "iroh", "doc-c").state; // peer-3
  state = mintPeer(state, "dave", "iroh", "doc-d").state; // peer-4

  // Everyone starts in their own singleton group.
  assert.deepEqual(peerBridgeGroups(state), [["peer-1"], ["peer-2"], ["peer-3"], ["peer-4"]]);

  state = addBridge(state, "peer-1", "peer-2");
  state = addBridge(state, "peer-2", "peer-1"); // duplicate either direction
  state = addBridge(state, "peer-1", "peer-1"); // self is a no-op
  assert.equal(state.bridges.length, 1);

  state = addBridge(state, "peer-2", "peer-3"); // transitive: {1,2,3}
  assert.deepEqual(peerBridgeGroups(state), [["peer-1", "peer-2", "peer-3"], ["peer-4"]]);
  assert.deepEqual(bridgeGroupContaining(state, "peer-3"), ["peer-1", "peer-2", "peer-3"]);
  assert.deepEqual(bridgeGroupContaining(state, "peer-4"), ["peer-4"]);
  // Unknown peers fall back to a singleton of themselves.
  assert.deepEqual(bridgeGroupContaining(state, "peer-9"), ["peer-9"]);
});

test("bridging wires the sessions of any member to every member", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state; // session-1
  state = mintPeer(state, "alice", "iroh", "doc-a").state; // peer-2
  state = mintPeer(state, "bob", "iroh", "doc-b").state; // peer-3
  state = authorizePeerOnSession(state, "session-1", "peer-2");

  state = addBridge(state, "peer-2", "peer-3");
  state = unionBridgedPeersIntoSessions(state);
  // The Q3 equivalence: a session wired to any member is wired to all.
  assert.deepEqual(sessionByKey(state, "session-1").peerKeys, ["peer-2", "peer-3"]);

  // Idempotent, and sessions with no member stay untouched.
  const again = unionBridgedPeersIntoSessions(state);
  assert.deepEqual(sessionByKey(again, "session-1").peerKeys, ["peer-2", "peer-3"]);
});

test("bridged peers authorize as a group; reachability stays a sync-time concern", () => {
  let state = emptyObjects();
  state = mintSession(state, "s").state; // session-1
  state = mintPeer(state, "alice", "iroh", "doc-a").state; // peer-2
  state = mintPeer(state, "npub", "nostr", "npub1xyz").state; // peer-3

  const bridge = wireVerdict(ref("peer", "peer-2"), ref("peer", "peer-3"), state);
  assert.equal(bridge.kind, "peer-bridge");
  assert.equal(bridge.allowed, true);
  assert.equal(bridge.backed, true);

  state = addBridge(state, "peer-2", "peer-3");
  const again = wireVerdict(ref("peer", "peer-2"), ref("peer", "peer-3"), state);
  assert.equal(again.allowed, false);
  assert.equal(again.backed, true);
  assert.match(again.reason, /already bridged/);

  // Authorizing any bridged member authorizes the GROUP: the label names
  // the bridge, and the wire is executable regardless of member transport
  // (reaching npub-only members is surfaced at sync time, not refused here).
  const throughNostr = wireVerdict(ref("peer", "peer-3"), ref("session", "session-1"), state);
  assert.equal(throughNostr.kind, "peer-into-session");
  assert.equal(throughNostr.allowed, true);
  assert.equal(throughNostr.backed, true);
  assert.match(throughNostr.label, /bridge alice\+npub/);

  // Transport usability stays a per-peer sync-time predicate.
  assert.equal(peerUsableForSync(peerByKey(state, "peer-2")), true);
  assert.equal(peerUsableForSync(peerByKey(state, "peer-3")), false);
});

// --- contextual enablement ----------------------------------------------------

test("arity rules: exactly-one and at-least-N actions", () => {
  // ordering:null keeps every correctness gate quiet so this test observes
  // arity alone (gates get their own tests below).
  const none = { selected: [], overrides: new Set() };
  const one = { selected: [summary({ ordering: null })], overrides: new Set() };
  const two = { selected: [summary({ ordering: null }), summary({ ordering: null })], overrides: new Set() };

  for (const action of ["sort", "make-unordered", "atomize", "export-v2", "export-bip174", "edit", "pay", "confirm", "payments"]) {
    assert.equal(actionState(action, none).enabled, false, `${action} with none`);
    assert.match(actionState(action, none).reason, /exactly 1/);
    assert.equal(actionState(action, one).enabled, true, `${action} with one`);
    assert.equal(actionState(action, two).enabled, false, `${action} with two`);
    assert.match(actionState(action, two).reason, /exactly 1 selected fragment \(2 selected\)/);
  }

  for (const action of ["join", "concatenate"]) {
    assert.equal(actionState(action, one).enabled, false, `${action} with one`);
    assert.match(actionState(action, one).reason, /at least 2 selected fragments \(1 selected\)/);
    assert.equal(actionState(action, two).enabled, true, `${action} with two`);
  }

  assert.equal(actionState("sync", none).enabled, false);
  assert.match(actionState("sync", none).reason, /at least 1/);
  assert.equal(actionState("sync", one).enabled, true);
  assert.equal(actionState("sync", two).enabled, true);
});

test("join gate: ordered fragments block, override arms with warning kept", () => {
  const mixed = [summary(), summary({ ordering: "ordered" })];
  const blocked = actionState("join", { selected: mixed, overrides: new Set() });
  assert.equal(blocked.enabled, false);
  assert.equal(blocked.gate.id, "join-ordered");
  assert.match(blocked.reason, /1 selected fragment\(s\) are ordered/);
  assert.match(blocked.gate.warning, /Overriding sends them as-is/);
  // Audited: /api/join accepts ordered PSBTs (no systematic 400), so this
  // override keeps send-as-is semantics — no repair to apply.
  assert.equal(blocked.gate.fix, null);

  const overridden = actionState("join", { selected: mixed, overrides: new Set(["join-ordered"]) });
  assert.equal(overridden.enabled, true);
  assert.equal(overridden.overridden, true);
  assert.equal(overridden.gate.id, "join-ordered");

  // Not-decoded fragments never gate: the backend is the authority.
  const unknown = [summary({ ordering: null }), summary({ ordering: null })];
  assert.equal(actionState("join", { selected: unknown, overrides: new Set() }).enabled, true);
});

test("sort and make-unordered idempotence gates", () => {
  const ordered = { selected: [summary({ ordering: "ordered" })], overrides: new Set() };
  const sortGate = actionState("sort", ordered);
  assert.equal(sortGate.enabled, false);
  assert.equal(sortGate.gate.id, "sort-ordered");
  // Legitimate re-runs, not bypasses into a 400: no repair to apply.
  assert.equal(sortGate.gate.fix, null);
  assert.equal(actionState("sort", { ...ordered, overrides: new Set(["sort-ordered"]) }).enabled, true);

  const unordered = { selected: [summary()], overrides: new Set() };
  const shuffleGate = actionState("make-unordered", unordered);
  assert.equal(shuffleGate.enabled, false);
  assert.equal(shuffleGate.gate.id, "make-unordered-unordered");
  assert.match(shuffleGate.gate.warning, /re-randomizes/);
  assert.equal(shuffleGate.gate.fix, null);
  // An ordered fragment passes make-unordered without a gate.
  assert.equal(actionState("make-unordered", ordered).enabled, true);
});

test("atomize gates: unmodifiable flags and already-atomic", () => {
  const unmodifiable = {
    selected: [summary({ modifiableInputs: false, modifiableOutputs: false })],
    overrides: new Set(),
  };
  const gate = actionState("atomize", unmodifiable);
  assert.equal(gate.enabled, false);
  assert.equal(gate.gate.id, "atomize-unmodifiable");
  assert.match(gate.gate.warning, /constructor role/);
  // The backend is KNOWN to reject unmodifiable atomize (a guaranteed 400),
  // so the armed override APPLIES the repair: raw-edit the TX_MODIFIABLE
  // flags via /api/edit, then atomize the minted fragment.
  assert.deepEqual(gate.gate.fix, { kind: "set-tx-modifiable" });
  const armed = actionState("atomize", { ...unmodifiable, overrides: new Set(["atomize-unmodifiable"]) });
  assert.equal(armed.enabled, true);
  assert.deepEqual(armed.gate.fix, { kind: "set-tx-modifiable" });

  const atomic = { selected: [summary({ inputCount: 1, outputCount: 0 })], overrides: new Set() };
  assert.equal(actionState("atomize", atomic).gate.id, "atomize-atomic");
  // No repair exists for an already-atomic fragment: send-as-is override.
  assert.equal(actionState("atomize", atomic).gate.fix, null);

  // Unknown counts and partial modifiability do not gate.
  const unknown = { selected: [summary({ inputCount: null, outputCount: null })], overrides: new Set() };
  assert.equal(actionState("atomize", unknown).enabled, true);
  const partial = { selected: [summary({ modifiableInputs: false, modifiableOutputs: true })], overrides: new Set() };
  assert.equal(actionState("atomize", partial).enabled, true);
});

test("export-bip174 gate: unordered fragments need a sort first (observed route gate)", () => {
  const unordered = { selected: [summary()], overrides: new Set() };
  const gate = actionState("export-bip174", unordered);
  assert.equal(gate.enabled, false);
  assert.equal(gate.gate.id, "export-bip174-unordered");
  assert.match(gate.gate.warning, /ordered PSBTs for BIP 174/);
  // The route is KNOWN to reject unordered PSBTs, so the armed override
  // APPLIES the repair: run the sorter role first, export the result.
  assert.deepEqual(gate.gate.fix, { kind: "sort-first" });
  assert.equal(
    actionState("export-bip174", { ...unordered, overrides: new Set(["export-bip174-unordered"]) })
      .enabled,
    true,
  );
  // Ordered fragments export without a gate; v2 export never gates.
  assert.equal(actionState("export-bip174", { selected: [summary({ ordering: "ordered" })], overrides: new Set() }).enabled, true);
  assert.equal(actionState("export-v2", unordered).enabled, true);
});

test("assign-ids: enabled through the Backend seam, arity-checked", () => {
  // The assignIds seam landed (/api/assign-ids): outputs missing ids enable
  // the action, nothing is waiting on a backend anymore.
  const missing = { selected: [summary({ outputUidPresent: 1, outputCount: 2 })], overrides: new Set() };
  const ready = actionState("assign-ids", missing);
  assert.equal(ready.enabled, true);
  assert.equal(ready.needsBackend, null);
  assert.equal(ready.reason, null);

  // Id-complete fragments stay actionable: backend-minted fragments ALWAYS
  // carry ids (/api/create assigns them), and the panel this action opens
  // owns the id-complete cases explicitly (manual per-output ids, the
  // overwrite-existing-ids checkbox). A lockout here made the affordance
  // permanently dead.
  const complete = { selected: [summary({ outputUidPresent: 2, outputCount: 2 })], overrides: new Set() };
  const done = actionState("assign-ids", complete);
  assert.equal(done.enabled, true);
  assert.equal(done.reason, null);

  const wrongArity = actionState("assign-ids", { selected: [], overrides: new Set() });
  assert.match(wrongArity.reason, /exactly 1/);
  assert.equal(wrongArity.needsBackend, null);

  // Unknown uid/output counts (undecoded fragment): stay permissive, the
  // backend is the authority on whether ids are actually missing.
  const unknown = { selected: [summary({ outputUidPresent: null, outputCount: null })], overrides: new Set() };
  assert.equal(actionState("assign-ids", unknown).enabled, true);
});

// --- focus navigation ----------------------------------------------------------

test("focus state validates against the live session list", () => {
  assert.deepEqual(overviewFocus(), { mode: "overview", sessionKey: null });
  assert.deepEqual(sessionFocus("session-1"), { mode: "session", sessionKey: "session-1" });

  const kept = validateFocus(sessionFocus("session-1"), ["session-1", "session-2"]);
  assert.deepEqual(kept, { mode: "session", sessionKey: "session-1" });

  const dropped = validateFocus(sessionFocus("session-9"), ["session-1"]);
  assert.deepEqual(dropped, { mode: "overview", sessionKey: null });

  assert.deepEqual(validateFocus(overviewFocus(), []), { mode: "overview", sessionKey: null });
});

// --- deep-classification enrichment ---------------------------------------------

test("enrichDescriptor folds the miniscript details into the shallow node", () => {
  let state = emptyObjects();
  state = mintDescriptor(state, "wpkh(xprv.../0/*)", true).state;

  state = enrichDescriptor(state, "descriptor-1", {
    kind: "descriptor",
    descriptor: "wpkh(xpub.../0/*)#checksum",
    descriptor_type: "Wpkh",
    has_private_keys: true,
    is_ranged: true,
    is_multipath: false,
    derived: [
      { index: 0, script_pubkey_hex: "0014" + "11".repeat(20), address: "bcrt1qaaa" },
      { index: 1, script_pubkey_hex: "0014" + "22".repeat(20) },
      { script_pubkey_hex: "junk with no index" },
    ],
  });

  const enriched = state.descriptors[0];
  // The pasted text is retained; the normalized PUBLIC form rides alongside.
  assert.equal(enriched.descriptor, "wpkh(xprv.../0/*)");
  assert.equal(enriched.normalized, "wpkh(xpub.../0/*)#checksum");
  assert.equal(enriched.descriptorType, "Wpkh");
  assert.equal(enriched.hasPrivateKeys, true);
  assert.equal(enriched.isPrivate, true); // deep flag is authoritative
  assert.equal(enriched.isRanged, true);
  assert.deepEqual(enriched.derived, [
    { index: 0, scriptPubkeyHex: "0014" + "11".repeat(20), address: "bcrt1qaaa" },
    { index: 1, scriptPubkeyHex: "0014" + "22".repeat(20), address: null },
  ]);

  // Wrong kind or unknown key: untouched state, never a throw.
  assert.deepEqual(enrichDescriptor(state, "descriptor-1", { kind: "payment" }), state);
  assert.deepEqual(enrichDescriptor(state, "descriptor-404", { kind: "descriptor" }), state);
});


test("applyTxOutputs updates the pending node and mints per-output siblings", () => {
  let state = emptyObjects();
  state = mintUtxo(state, "020000dead").state;

  const txid = "ab".repeat(32);
  const applied = applyTxOutputs(state, "utxo-1", {
    kind: "transaction",
    txid,
    input_count: 1,
    output_count: 2,
    fully_signed: true,
    outputs: [
      { outpoint: `${txid}:0`, vout: 0, amount_sats: 70000, script_pubkey_hex: "0014aa", address: "bcrt1qfirst" },
      { outpoint: `${txid}:1`, vout: 1, amount_sats: 30000, script_pubkey_hex: "0014bb" },
    ],
  });
  state = applied.state;

  assert.equal(applied.utxos.length, 2);
  // First output updates the ORIGINAL node in place (its key was logged).
  assert.equal(state.utxos[0].key, "utxo-1");
  assert.equal(state.utxos[0].txid, txid);
  assert.equal(state.utxos[0].vout, 0);
  assert.equal(state.utxos[0].amountSats, 70000);
  assert.equal(state.utxos[0].address, "bcrt1qfirst");
  assert.equal(state.utxos[0].fullySigned, true);
  // Further outputs mint sibling nodes carrying the same raw hex.
  assert.equal(state.utxos[1].key, "utxo-2");
  assert.equal(state.utxos[1].vout, 1);
  assert.equal(state.utxos[1].amountSats, 30000);
  assert.equal(state.utxos[1].address, null);
  assert.equal(state.utxos[1].rawTxHex, "020000dead");

  // Wrong kind, unknown node, or an undecodable response: untouched.
  assert.deepEqual(applyTxOutputs(state, "utxo-1", { kind: "descriptor" }).utxos, []);
  assert.deepEqual(applyTxOutputs(state, "utxo-404", { kind: "transaction" }).utxos, []);
  assert.deepEqual(applyTxOutputs(state, "utxo-1", { kind: "transaction", outputs: [] }).utxos, []);
});
