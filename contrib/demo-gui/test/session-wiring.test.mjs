import test from "node:test";
import assert from "node:assert/strict";

import {
  actionState,
  addBridge,
  addFragmentToSession,
  addPeerToSession,
  applyTxOutputs,
  beginWire,
  bridgeGroupContaining,
  completeWire,
  dropFragmentKey,
  emptyObjects,
  enrichDescriptor,
  enrichPayment,
  idleWire,
  fragmentSessionKeys,
  mergeSessions,
  mineFragmentKeys,
  mintDescriptor,
  mintPayment,
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

test("minting sessions, peers, payments, utxos, and descriptors is grow-only", () => {
  let state = emptyObjects();
  const s1 = mintSession(state, "  alpha  ", "iroh");
  state = s1.state;
  assert.equal(s1.session.key, "session-1");
  assert.equal(s1.session.name, "alpha");
  assert.deepEqual(s1.session.fragmentKeys, []);

  const p1 = mintPeer(state, "", "iroh", " doc-ticket ");
  state = p1.state;
  assert.equal(p1.peer.key, "peer-2"); // one shared counter: keys never collide
  assert.equal(p1.peer.name, "peer-2"); // blank names fall back to the key
  assert.equal(p1.peer.identity, "doc-ticket");

  const pay = mintPayment(state, "bitcoin:bcrt1qx?amount=0.001", "bcrt1qx", 100000, "lunch");
  state = pay.state;
  assert.equal(pay.payment.key, "payment-3");

  const utxo = mintUtxo(state, "02000000...");
  state = utxo.state;
  assert.equal(utxo.utxo.key, "utxo-4");
  // Deep decode is a backend seam: minted spendable outputs stay pending.
  assert.equal(utxo.utxo.txid, null);
  assert.equal(utxo.utxo.vout, null);

  const desc = mintDescriptor(state, " wpkh(xpub6...) ", false);
  state = desc.state;
  assert.equal(desc.descriptor.key, "descriptor-5");
  assert.equal(desc.descriptor.descriptor, "wpkh(xpub6...)");

  assert.equal(sessionByKey(state, "session-1").name, "alpha");
  assert.equal(sessionByKey(state, "nope"), null);
  assert.equal(peerByKey(state, "peer-2").transport, "iroh");
  assert.equal(peerByKey(state, "nope"), null);
});

test("adding an ephemeral peer deduplicates the exact transport address without side effects", () => {
  let state = emptyObjects();
  state = mintSession(state, "lunch", "local").state;

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

test("session membership: add fragment/peer, deduplicate, drop removed fragments", () => {
  let state = emptyObjects();
  state = mintSession(state, "s", "local").state;
  state = addFragmentToSession(state, "session-1", "psbt-1");
  state = addFragmentToSession(state, "session-1", "psbt-1"); // no duplicate
  state = addFragmentToSession(state, "session-1", "psbt-2");
  assert.deepEqual(sessionByKey(state, "session-1").fragmentKeys, ["psbt-1", "psbt-2"]);

  state = addPeerToSession(state, "session-1", "peer-9");
  state = addPeerToSession(state, "session-1", "peer-9");
  assert.deepEqual(sessionByKey(state, "session-1").peerKeys, ["peer-9"]);

  state = dropFragmentKey(state, "psbt-1");
  assert.deepEqual(sessionByKey(state, "session-1").fragmentKeys, ["psbt-2"]);

  // Unknown session keys are a no-op, not a throw.
  const untouched = addFragmentToSession(state, "session-404", "psbt-3");
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

test("fragment-session wiring is symmetric and membership-aware", () => {
  let state = emptyObjects();
  state = mintSession(state, "s", "iroh").state;
  state = addFragmentToSession(state, "session-1", "psbt-1");

  const fresh = wireVerdict(ref("fragment", "psbt-2"), ref("session", "session-1"), state);
  assert.equal(fresh.kind, "fragment-into-session");
  assert.equal(fresh.allowed, true);
  assert.equal(fresh.backed, true);

  const reversed = wireVerdict(ref("session", "session-1"), ref("fragment", "psbt-2"), state);
  assert.equal(reversed.kind, "fragment-into-session");
  assert.equal(reversed.allowed, true);

  const member = wireVerdict(ref("fragment", "psbt-1"), ref("session", "session-1"), state);
  assert.equal(member.allowed, false);
  assert.match(member.reason, /already in the session/);
});

test("peer-session wiring is unbacked until the ptj pairing adapter exists", () => {
  let state = emptyObjects();
  state = mintSession(state, "s", "iroh").state;
  state = mintPeer(state, "good", "iroh", "doc-abc").state;
  state = mintPeer(state, "blank", "unknown", "").state;
  state = mintPeer(state, "nostr", "nostr", "npub1xyz").state;

  const good = wireVerdict(ref("peer", "peer-2"), ref("session", "session-1"), state);
  assert.equal(good.kind, "peer-into-session");
  assert.equal(good.allowed, false);
  assert.equal(good.backed, false);
  assert.equal(good.needs, "ptj session pairing adapter");

  const blank = wireVerdict(ref("peer", "peer-3"), ref("session", "session-1"), state);
  assert.equal(blank.allowed, false);
  assert.equal(blank.backed, false);
  assert.equal(blank.needs, "ptj session pairing adapter");

  // npub peers are minted from paste but /api/sync has no nostr transport:
  // visible, explicitly unwired, with the missing seam named.
  const nostr = wireVerdict(ref("session", "session-1"), ref("peer", "peer-4"), state);
  assert.equal(nostr.allowed, false);
  assert.equal(nostr.backed, false);
  assert.equal(nostr.needs, "ptj session pairing adapter");
});

test("payment and utxo wiring rows", () => {
  const state = emptyObjects();
  const attach = wireVerdict(ref("payment", "payment-1"), ref("fragment", "psbt-1"), state);
  assert.equal(attach.kind, "attach-payment");
  assert.equal(attach.allowed, true);
  assert.equal(attach.backed, true);
  // Symmetric.
  assert.equal(wireVerdict(ref("fragment", "psbt-1"), ref("payment", "payment-1"), state).kind, "attach-payment");

  const create = wireVerdict(ref("utxo", "utxo-1"), ref("create", "create"), state);
  assert.equal(create.kind, "add-create-input");
  assert.equal(create.allowed, true);
  assert.equal(create.backed, true);

  const toFragment = wireVerdict(ref("utxo", "utxo-1"), ref("fragment", "psbt-1"), state);
  assert.equal(toFragment.allowed, false);
  assert.match(toFragment.reason, /create form/);

  const toSession = wireVerdict(ref("payment", "payment-1"), ref("session", "session-1"), state);
  assert.equal(toSession.allowed, false);
  assert.match(toSession.reason, /to a fragment/);
});

test("session merge and peer bridge are wired; attribute-scripts still names its seam", () => {
  let state = emptyObjects();
  state = mintSession(state, "a", "iroh").state;
  state = mintSession(state, "b", "local").state;

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

// --- action labels + target vocabulary ----------------------------------------

test("wire verdicts carry concrete action labels built from display names", () => {
  let state = emptyObjects();
  state = mintSession(state, "lunch", "iroh").state;
  state = mintPeer(state, "alice", "iroh", "doc-abc").state;
  state = mintPayment(state, "bitcoin:bcrt1qx?amount=0.001", "bcrt1qx", 100000, "rent").state;
  state = mintUtxo(state, "020000dead").state;
  state = mintDescriptor(state, "wpkh(xpub6...)", false).state;

  assert.equal(nodeDisplayName(ref("fragment", "psbt-7"), state), "psbt-7");
  assert.equal(nodeDisplayName(ref("session", "session-1"), state), "lunch");
  assert.equal(nodeDisplayName(ref("peer", "peer-2"), state), "alice");
  assert.equal(nodeDisplayName(ref("payment", "payment-3"), state), "rent");
  // Unknown keys and label-less objects fall back to the key.
  assert.equal(nodeDisplayName(ref("session", "session-404"), state), "session-404");

  assert.equal(
    wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).label,
    "Join psbt-1 into psbt-2",
  );
  assert.equal(
    wireVerdict(ref("fragment", "psbt-1"), ref("session", "session-1"), state).label,
    "Publish psbt-1 to session lunch",
  );
  // Symmetric pairs label the same action regardless of direction.
  assert.equal(
    wireVerdict(ref("session", "session-1"), ref("fragment", "psbt-1"), state).label,
    "Publish psbt-1 to session lunch",
  );
  assert.equal(
    wireVerdict(ref("peer", "peer-2"), ref("session", "session-1"), state).label,
    "Sync session lunch over peer alice",
  );
  assert.equal(
    wireVerdict(ref("payment", "payment-3"), ref("fragment", "psbt-1"), state).label,
    "Attach payment rent to psbt-1",
  );
  assert.equal(
    wireVerdict(ref("utxo", "utxo-4"), ref("create", "create"), state).label,
    "Use utxo-4 as a create-form input",
  );
  assert.equal(
    wireVerdict(ref("session", "session-1"), ref("session", "session-9"), state).label,
    "Merge sessions lunch and session-9",
  );
  assert.equal(
    wireVerdict(ref("peer", "peer-2"), ref("peer", "peer-9"), state).label,
    "Bridge peers alice, peer-9",
  );
  assert.equal(
    wireVerdict(ref("descriptor", "descriptor-5"), ref("fragment", "psbt-1"), state).label,
    "Attribute descriptor-5 scripts to psbt-1",
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
  state = mintSession(state, "s", "iroh").state;
  state = addFragmentToSession(state, "session-1", "psbt-1");
  state = mintPeer(state, "npub", "nostr", "npub1xyz").state;

  // allowed && backed → compatible.
  const join = wireVerdict(ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state);
  assert.equal(wireDisposition(join), "compatible");

  // backed but refused right now → blocked (red vocabulary).
  const member = wireVerdict(ref("fragment", "psbt-1"), ref("session", "session-1"), state);
  assert.equal(wireDisposition(member), "blocked");

  // Defined pair waiting on a seam → unbacked (dim vocabulary)…
  const nostr = wireVerdict(ref("peer", "peer-2"), ref("session", "session-1"), state);
  assert.equal(wireDisposition(nostr), "unbacked");
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
  state = mintSession(state, "s", "iroh").state;
  state = addFragmentToSession(state, "session-1", "psbt-1");

  // Blocked (member already in the session): not queued, verdict says why.
  const blocked = queueWire([], ref("fragment", "psbt-1"), ref("session", "session-1"), state);
  assert.equal(blocked.queued, false);
  assert.equal(blocked.duplicate, false);
  assert.deepEqual(blocked.wires, []);
  assert.match(blocked.verdict.reason, /already in the session/);

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
  state = mintSession(state, "s", "iroh").state;
  const fragments = ["psbt-1", "psbt-2"];

  assert.equal(nodeExists(ref("fragment", "psbt-1"), state, fragments), true);
  assert.equal(nodeExists(ref("fragment", "psbt-9"), state, fragments), false);
  assert.equal(nodeExists(ref("session", "session-1"), state, fragments), true);
  assert.equal(nodeExists(ref("session", "session-9"), state, fragments), false);
  assert.equal(nodeExists(ref("create", "create"), state, []), true);

  let wires = queueWire([], ref("fragment", "psbt-1"), ref("fragment", "psbt-2"), state).wires;
  wires = queueWire(wires, ref("fragment", "psbt-1"), ref("session", "session-1"), state).wires;
  assert.equal(wires.length, 2);

  // Everything still valid: prune keeps both.
  assert.equal(pruneWires(wires, state, fragments).length, 2);

  // The fragment joined the session through another path: the queued
  // publish wire is no longer compatible and drops; the join stays.
  const joinedState = addFragmentToSession(state, "session-1", "psbt-1");
  const pruned = pruneWires(wires, joinedState, fragments);
  assert.equal(pruned.length, 1);
  assert.equal(pruned[0].target.key, "psbt-2");

  // A removed fragment takes its wires with it (psbt-2 gone drops the join;
  // the psbt-1 publish wire survives); removing both fragments empties the
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
  state = mintSession(state, "s", "iroh").state;

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

test("mine membership is derived: loaded fragments default there, publishing moves them out", () => {
  let state = emptyObjects();
  const fragments = ["psbt-1", "psbt-2", "psbt-3"];

  // No sessions: everything is Mine (loaded/created default there).
  assert.deepEqual(mineFragmentKeys(fragments, state), fragments);

  state = mintSession(state, "s", "iroh").state; // session-1
  assert.deepEqual(mineFragmentKeys(fragments, state), fragments);

  // Publishing (fragment → session wiring) moves the fragment out of Mine.
  state = addFragmentToSession(state, "session-1", "psbt-2");
  assert.deepEqual(mineFragmentKeys(fragments, state), ["psbt-1", "psbt-3"]);
  assert.deepEqual(fragmentSessionKeys(state, "psbt-2"), ["session-1"]);
  assert.deepEqual(fragmentSessionKeys(state, "psbt-1"), []);

  // Membership in ANY session keeps it published; multi-session membership
  // lists every carrier.
  state = mintSession(state, "t", "local").state; // session-2
  state = addFragmentToSession(state, "session-2", "psbt-2");
  assert.deepEqual(fragmentSessionKeys(state, "psbt-2"), ["session-1", "session-2"]);
  assert.deepEqual(mineFragmentKeys(fragments, state), ["psbt-1", "psbt-3"]);

  // Leaving every session returns the fragment to Mine.
  state = dropFragmentKey(state, "psbt-2");
  assert.deepEqual(mineFragmentKeys(fragments, state), fragments);
});

test("mine tracks session merges: members follow the merged session, not Mine", () => {
  let state = emptyObjects();
  state = mintSession(state, "a", "iroh").state; // session-1
  state = mintSession(state, "b", "iroh").state; // session-2
  state = addFragmentToSession(state, "session-1", "psbt-1");
  state = addFragmentToSession(state, "session-2", "psbt-2");

  const merge = mergeSessions(state, "session-1", "session-2");
  state = merge.state;
  // The sources are retired but the members ride the merged session — a
  // merge never dumps published fragments back into Mine.
  assert.deepEqual(mineFragmentKeys(["psbt-1", "psbt-2", "psbt-3"], state), ["psbt-3"]);
  assert.deepEqual(fragmentSessionKeys(state, "psbt-1"), [merge.merged.key]);
});

// --- session merge (Q3: join contents + union peer connections) -----------------

test("mergeSessions unions fragments and peers and retires the sources", () => {
  let state = emptyObjects();
  state = mintSession(state, "alpha", "iroh").state; // session-1
  state = mintSession(state, "beta", "local").state; // session-2
  state = addFragmentToSession(state, "session-1", "psbt-1");
  state = addFragmentToSession(state, "session-1", "psbt-2");
  state = addFragmentToSession(state, "session-2", "psbt-2");
  state = addFragmentToSession(state, "session-2", "psbt-3");
  state = addPeerToSession(state, "session-1", "peer-a");
  state = addPeerToSession(state, "session-2", "peer-b");

  const merge = mergeSessions(state, "session-1", "session-2");
  assert.notEqual(merge.merged, null);
  assert.equal(merge.merged.key, "session-3"); // fresh key from the shared counter
  assert.equal(merge.merged.name, "alpha+beta");
  // Content union, duplicates collapsed; peers of BOTH see the combined session.
  assert.deepEqual(merge.merged.fragmentKeys, ["psbt-1", "psbt-2", "psbt-3"]);
  assert.deepEqual(merge.merged.peerKeys, ["peer-a", "peer-b"]);
  // The left session's transport wins; the conflict is an honest note.
  assert.equal(merge.merged.transport, "iroh");
  assert.ok(merge.notes.some((note) => /transport conflict/.test(note)));
  // The UI-model merge always names what it cannot merge (the future
  // backend session-state seam).
  assert.ok(merge.notes.some((note) => /session-state merge seam|NOT merged/.test(note)));
  // The sources are retired; only the merged session remains.
  assert.deepEqual(
    merge.state.sessions.map((sessionObject) => sessionObject.key),
    ["session-3"],
  );

  // Missing keys and self-merges are no-ops.
  assert.equal(mergeSessions(state, "session-1", "session-9").merged, null);
  assert.equal(mergeSessions(state, "session-1", "session-1").merged, null);
  assert.deepEqual(mergeSessions(state, "session-1", "session-9").state, state);
});

test("mergeSessions keeps identity material and says which ticket survives", () => {
  let state = emptyObjects();
  const s1 = mintSession(state, "a", "iroh");
  state = s1.state;
  const s2 = mintSession(state, "b", "iroh");
  state = s2.state;
  state = {
    ...state,
    sessions: state.sessions.map((sessionObject) =>
      sessionObject.key === "session-1"
        ? { ...sessionObject, irohTicket: "doc-left" }
        : { ...sessionObject, irohTicket: "doc-right" },
    ),
  };
  const merge = mergeSessions(state, "session-1", "session-2");
  assert.equal(merge.merged.irohTicket, "doc-left");
  assert.ok(merge.notes.some((note) => /iroh ticket/.test(note)));

  // No conflict when only one side carries a ticket: it is simply kept.
  let oneSided = emptyObjects();
  oneSided = mintSession(oneSided, "a", "iroh").state;
  oneSided = mintSession(oneSided, "b", "iroh").state;
  oneSided = {
    ...oneSided,
    sessions: oneSided.sessions.map((sessionObject) =>
      sessionObject.key === "session-2"
        ? { ...sessionObject, irohTicket: "doc-only" }
        : sessionObject,
    ),
  };
  const kept = mergeSessions(oneSided, "session-1", "session-2");
  assert.equal(kept.merged.irohTicket, "doc-only");
  assert.ok(!kept.notes.some((note) => /iroh ticket/.test(note)));
  assert.ok(!kept.notes.some((note) => /transport conflict/.test(note)));
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
  state = mintSession(state, "s", "iroh").state; // session-1
  state = mintPeer(state, "alice", "iroh", "doc-a").state; // peer-2
  state = mintPeer(state, "bob", "iroh", "doc-b").state; // peer-3
  state = addPeerToSession(state, "session-1", "peer-2");

  state = addBridge(state, "peer-2", "peer-3");
  state = unionBridgedPeersIntoSessions(state);
  // The Q3 equivalence: a session wired to any member is wired to all.
  assert.deepEqual(sessionByKey(state, "session-1").peerKeys, ["peer-2", "peer-3"]);

  // Idempotent, and sessions with no member stay untouched.
  const again = unionBridgedPeersIntoSessions(state);
  assert.deepEqual(sessionByKey(again, "session-1").peerKeys, ["peer-2", "peer-3"]);
});

test("peer-bridge verdicts: bridgeable, already bridged, and group-aware session sync", () => {
  let state = emptyObjects();
  state = mintSession(state, "s", "iroh").state; // session-1
  state = mintPeer(state, "alice", "iroh", "doc-a").state; // peer-2
  state = mintPeer(state, "npub", "nostr", "npub1xyz").state; // peer-3
  state = mintPeer(state, "npub2", "nostr", "npub1abc").state; // peer-4

  const bridge = wireVerdict(ref("peer", "peer-2"), ref("peer", "peer-3"), state);
  assert.equal(bridge.kind, "peer-bridge");
  assert.equal(bridge.allowed, true);
  assert.equal(bridge.backed, true);

  state = addBridge(state, "peer-2", "peer-3");
  const again = wireVerdict(ref("peer", "peer-2"), ref("peer", "peer-3"), state);
  assert.equal(again.allowed, false);
  assert.equal(again.backed, true);
  assert.match(again.reason, /already bridged/);

  // A bridge remains a reachable peer-peer presentation operation, but it
  // cannot bypass the missing low-level session-pairing adapter.
  const throughNostr = wireVerdict(ref("peer", "peer-3"), ref("session", "session-1"), state);
  assert.equal(throughNostr.kind, "peer-into-session");
  assert.equal(throughNostr.allowed, false);
  assert.equal(throughNostr.backed, false);
  assert.equal(throughNostr.needs, "ptj session pairing adapter");
  assert.match(throughNostr.label, /bridge alice\+npub/);

  // An all-nostr BRIDGE GROUP stays honestly unbacked…
  state = mintPeer(state, "npub3", "nostr", "npub1def").state; // peer-5
  state = addBridge(state, "peer-4", "peer-5");
  const nostrGroup = wireVerdict(ref("peer", "peer-4"), ref("session", "session-1"), state);
  assert.equal(nostrGroup.allowed, false);
  assert.equal(nostrGroup.backed, false);
  assert.equal(nostrGroup.needs, "ptj session pairing adapter");
  assert.match(nostrGroup.label, /bridge npub2\+npub3/);

  // …and a group whose members exist but have no usable identity blocks.
  state = mintPeer(state, "blank1", "unknown", "").state; // peer-6
  state = mintPeer(state, "blank2", "unknown", "").state; // peer-7
  state = addBridge(state, "peer-6", "peer-7");
  const blankGroup = wireVerdict(ref("peer", "peer-6"), ref("session", "session-1"), state);
  assert.equal(blankGroup.allowed, false);
  assert.equal(blankGroup.backed, true);
  assert.match(blankGroup.reason, /no bridged peer has a usable transport identity/);

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

test("enrichPayment folds variant, methods, and description in", () => {
  let state = emptyObjects();
  state = mintPayment(state, "bitcoin:bcrt1qx?amount=0.001", "bcrt1qx", 100000, "lunch").state;

  state = enrichPayment(state, "payment-1", {
    kind: "payment",
    variant: "fixed_amount",
    description: "lunch money",
    methods: [
      { type: "onchain", address: "bcrt1qx" },
      { type: "bolt11", invoice: "lnbcrt1..." },
      { type: "cashu" },
      { no_type: true },
    ],
  });

  const enriched = state.payments[0];
  assert.equal(enriched.variant, "fixed_amount");
  assert.equal(enriched.description, "lunch money");
  assert.deepEqual(enriched.methods, [
    "onchain: bcrt1qx",
    "bolt11: lnbcrt1...",
    "cashu",
  ]);
  // The shallow-parsed URI fields stay authoritative for what they carried.
  assert.equal(enriched.address, "bcrt1qx");
  assert.equal(enriched.amountSats, 100000);

  assert.deepEqual(enrichPayment(state, "payment-1", { kind: "transaction" }), state);
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
