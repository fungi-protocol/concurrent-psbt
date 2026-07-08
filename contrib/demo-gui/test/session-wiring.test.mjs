import test from "node:test";
import assert from "node:assert/strict";

import {
  actionState,
  addFragmentToSession,
  addPeerToSession,
  applyTxOutputs,
  beginWire,
  completeWire,
  dropFragmentKey,
  emptyObjects,
  enrichDescriptor,
  enrichPayment,
  idleWire,
  mintDescriptor,
  mintPayment,
  mintPeer,
  mintSession,
  mintUtxo,
  overviewFocus,
  peerByKey,
  sessionByKey,
  sessionFocus,
  validateFocus,
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

test("peer-session wiring depends on a usable transport identity", () => {
  let state = emptyObjects();
  state = mintSession(state, "s", "iroh").state;
  state = mintPeer(state, "good", "iroh", "doc-abc").state;
  state = mintPeer(state, "blank", "unknown", "").state;
  state = mintPeer(state, "nostr", "nostr", "npub1xyz").state;

  const good = wireVerdict(ref("peer", "peer-2"), ref("session", "session-1"), state);
  assert.equal(good.kind, "peer-into-session");
  assert.equal(good.allowed, true);
  assert.equal(good.backed, true);

  const blank = wireVerdict(ref("peer", "peer-3"), ref("session", "session-1"), state);
  assert.equal(blank.allowed, false);
  assert.equal(blank.backed, true);
  assert.match(blank.reason, /transport identity/);

  // npub peers are minted from paste but /api/sync has no nostr transport:
  // visible, explicitly unwired, with the missing seam named.
  const nostr = wireVerdict(ref("session", "session-1"), ref("peer", "peer-4"), state);
  assert.equal(nostr.allowed, false);
  assert.equal(nostr.backed, false);
  assert.match(nostr.needs, /nostr transport/);
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

test("unbacked pairs are visible with the missing seam named", () => {
  const state = emptyObjects();
  const merge = wireVerdict(ref("session", "session-1"), ref("session", "session-2"), state);
  assert.equal(merge.kind, "session-merge");
  assert.equal(merge.allowed, false);
  assert.equal(merge.backed, false);
  assert.match(merge.needs, /session-state merge seam/);

  const channel = wireVerdict(ref("peer", "peer-1"), ref("peer", "peer-2"), state);
  assert.equal(channel.kind, "peer-channel");
  assert.equal(channel.backed, false);
  assert.match(channel.needs, /channel establishment seam/);

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
  assert.equal(actionState("sort", { ...ordered, overrides: new Set(["sort-ordered"]) }).enabled, true);

  const unordered = { selected: [summary()], overrides: new Set() };
  const shuffleGate = actionState("make-unordered", unordered);
  assert.equal(shuffleGate.enabled, false);
  assert.equal(shuffleGate.gate.id, "make-unordered-unordered");
  assert.match(shuffleGate.gate.warning, /re-randomizes/);
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
  assert.equal(
    actionState("atomize", { ...unmodifiable, overrides: new Set(["atomize-unmodifiable"]) }).enabled,
    true,
  );

  const atomic = { selected: [summary({ inputCount: 1, outputCount: 0 })], overrides: new Set() };
  assert.equal(actionState("atomize", atomic).gate.id, "atomize-atomic");

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
  assert.equal(
    actionState("export-bip174", { ...unordered, overrides: new Set(["export-bip174-unordered"]) })
      .enabled,
    true,
  );
  // Ordered fragments export without a gate; v2 export never gates.
  assert.equal(actionState("export-bip174", { selected: [summary({ ordering: "ordered" })], overrides: new Set() }).enabled, true);
  assert.equal(actionState("export-v2", unordered).enabled, true);
});

test("assign-ids: enabled through the Backend seam, arity-checked, uid-aware", () => {
  // The assignIds seam landed (/api/assign-ids): outputs missing ids enable
  // the action, nothing is waiting on a backend anymore.
  const missing = { selected: [summary({ outputUidPresent: 1, outputCount: 2 })], overrides: new Set() };
  const ready = actionState("assign-ids", missing);
  assert.equal(ready.enabled, true);
  assert.equal(ready.needsBackend, null);
  assert.equal(ready.reason, null);

  const complete = { selected: [summary({ outputUidPresent: 2, outputCount: 2 })], overrides: new Set() };
  const done = actionState("assign-ids", complete);
  assert.equal(done.enabled, false);
  assert.match(done.reason, /already carry unique ids/);

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
