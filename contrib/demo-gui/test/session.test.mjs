import test from "node:test";
import assert from "node:assert/strict";

import {
  addFragment,
  buildConfirmArgs,
  buildCreateRequest,
  buildPayArgs,
  buildSyncRequest,
  bytesToBase64,
  emptySession,
  fragmentLabel,
  fragmentSummary,
  isHexBytes,
  negotiationView,
  parseLines,
  pastedPsbt,
  removeFragment,
  selectedFragments,
  setSelected,
} from "../dist/session/state.js";

const PSBT = "cHNidP8BAgQCAAAAAQMEAAAAAAEEAQABBQEAAQb8BHBzYnQBAA==";

const INSPECT = {
  format: "bip370",
  ordering: "unordered",
  input_count: 1,
  output_count: 2,
  sort: { mode: "deterministic", seed_hex: "abcd" },
  unordered_unique_id_hex: "11".repeat(32),
  modifiability: { flags: 3, inputs: true, outputs: true },
  outputs: [
    { amount_sats: 100000, script_pubkey_hex: "0014" + "22".repeat(20), unique_id_hex: "33".repeat(32) },
    { amount_sats: 50000, script_pubkey_hex: "0014" + "44".repeat(20), unique_id_hex: null },
  ],
  totals: { known_input_sats: 200000, output_sats: 150000, fee_sats_if_inputs_known: 50000 },
};

test("fragment set adds, deduplicates, selects, and removes", () => {
  let state = emptySession();
  const first = addFragment(state, ` ${PSBT} `, INSPECT, "paste");
  state = first.state;
  assert.equal(first.duplicate, false);
  assert.equal(first.fragment.psbt, PSBT);
  assert.equal(first.fragment.origin, "paste");
  assert.equal(state.fragments.length, 1);

  // Same PSBT again (whitespace-mangled): re-selected, not duplicated.
  const again = addFragment(state, `${PSBT.slice(0, 10)}\n${PSBT.slice(10)}`, null, "upload");
  state = again.state;
  assert.equal(again.duplicate, true);
  assert.equal(state.fragments.length, 1);
  assert.equal(state.fragments[0].selected, true);
  // The original inspect data is retained on the deduplicated fragment.
  assert.equal(state.fragments[0].inspect, INSPECT);

  const second = addFragment(state, `${PSBT}AA==`, null, "create");
  state = second.state;
  assert.equal(state.fragments.length, 2);
  assert.notEqual(second.fragment.key, first.fragment.key);

  state = setSelected(state, second.fragment.key, true);
  assert.deepEqual(
    selectedFragments(state).map((fragment) => fragment.key),
    [first.fragment.key, second.fragment.key],
  );
  state = setSelected(state, first.fragment.key, false);
  assert.deepEqual(
    selectedFragments(state).map((fragment) => fragment.key),
    [second.fragment.key],
  );

  state = removeFragment(state, second.fragment.key);
  assert.equal(state.fragments.length, 1);
  // Keys stay unique after removals: the counter never rewinds.
  const third = addFragment(state, `${PSBT}BB==`, null, "join");
  assert.notEqual(third.fragment.key, second.fragment.key);
});

test("canonical unique id dedupes shuffled serializations and absorbs the incoming value", () => {
  // Different bytes, same identity: the unordered phase shuffles map order on
  // every serialization, so byte equality never defines identity (psbt.md).
  const reshuffled = `${PSBT.slice(4)}${PSBT.slice(0, 4)}`;
  const richer = { ...INSPECT, output_count: 3 };

  let state = emptySession();
  const first = addFragment(state, PSBT, INSPECT, "paste");
  state = first.state;

  // Id match: one card survives and absorbs the incoming value — the id
  // commits to the input/output sets, so the newcomer may carry more data.
  const echo = addFragment(state, reshuffled, richer, "paste");
  state = echo.state;
  assert.equal(echo.duplicate, true);
  assert.equal(state.fragments.length, 1);
  assert.equal(state.fragments[0].key, first.fragment.key);
  assert.equal(state.fragments[0].psbt, reshuffled);
  assert.equal(state.fragments[0].inspect, richer);
  assert.equal(state.fragments[0].selected, true);

  // Byte-identical duplicate: the fast path keeps the existing value — a
  // null inspect on the incoming copy must not erase the decoded one.
  const byteDupe = addFragment(state, ` ${reshuffled} `, null, "upload");
  state = byteDupe.state;
  assert.equal(byteDupe.duplicate, true);
  assert.equal(state.fragments.length, 1);
  assert.equal(state.fragments[0].inspect, richer);

  // A distinct unique id is a genuinely different PSBT: its own card.
  const other = addFragment(
    state,
    `${PSBT}AA==`,
    { ...INSPECT, unordered_unique_id_hex: "22".repeat(32) },
    "paste",
  );
  assert.equal(other.duplicate, false);
  assert.equal(other.state.fragments.length, 2);
});

test("fragmentSummary projects inspect JSON defensively", () => {
  const summary = fragmentSummary(INSPECT);
  assert.equal(summary.format, "bip370");
  assert.equal(summary.ordering, "unordered");
  assert.equal(summary.inputCount, 1);
  assert.equal(summary.outputCount, 2);
  assert.equal(summary.sortMode, "deterministic");
  assert.equal(summary.seedHex, "abcd");
  assert.equal(summary.uniqueIdHex, "11".repeat(32));
  assert.equal(summary.knownInputSats, 200000);
  assert.equal(summary.outputSats, 150000);
  assert.equal(summary.feeSats, 50000);
  assert.equal(summary.modifiableInputs, true);
  assert.equal(summary.modifiableOutputs, true);
  // One of the two outputs carries a unique id (the second's is null).
  assert.equal(summary.outputUidPresent, 1);

  const empty = fragmentSummary(null);
  assert.equal(empty.format, null);
  assert.equal(empty.uniqueIdHex, null);
  assert.equal(empty.feeSats, null);
  assert.equal(empty.modifiableInputs, null);
  assert.equal(empty.modifiableOutputs, null);
  assert.equal(empty.outputUidPresent, null);

  // Wrong shapes degrade to null instead of throwing.
  const mangled = fragmentSummary({ format: 7, sort: "nope", totals: [1, 2], modifiability: 3, outputs: "x" });
  assert.equal(mangled.format, null);
  assert.equal(mangled.sortMode, null);
  assert.equal(mangled.knownInputSats, null);
  assert.equal(mangled.modifiableInputs, null);
  assert.equal(mangled.outputUidPresent, null);
});

test("fragmentLabel renders decoded and undecoded fragments", () => {
  const { fragment } = addFragment(emptySession(), PSBT, INSPECT, "paste");
  assert.equal(fragmentLabel(fragment), "psbt-1 · unordered · 1 in / 2 out · paste");
  const { fragment: raw } = addFragment(emptySession(), PSBT, null, "upload");
  assert.equal(fragmentLabel(raw), "psbt-1 · unknown · not decoded · upload");
});

test("negotiationView is the counts-and-raw-lists ceiling", () => {
  const view = negotiationView({ payments: ["aa", "bb"], confirmations: ["cc"] });
  assert.equal(view.paymentCount, 2);
  assert.equal(view.confirmationCount, 1);
  assert.deepEqual(view.payments, ["aa", "bb"]);
  assert.deepEqual(view.confirmations, ["cc"]);
  const empty = negotiationView({ payments: [], confirmations: [] });
  assert.equal(empty.paymentCount, 0);
  assert.equal(empty.confirmationCount, 0);
});

test("buildCreateRequest validates rows and maps ordering", () => {
  const txid = "ab".repeat(32);
  const form = {
    network: "regtest",
    ordering: "det",
    seed: "AB CD".replace(" ", ""),
    inputs: [
      { txid, vout: "7" },
      { txid: "", vout: "" }, // blank row skipped
    ],
    outputs: [{ address: "bcrt1qexample", amountBtc: "0.0005" }],
  };
  const built = buildCreateRequest(form);
  assert.equal(built.ok, true);
  assert.equal(built.value.network, "regtest");
  assert.equal(built.value.ordering, "deterministic");
  assert.equal(built.value.seedHex, "abcd");
  assert.deepEqual(built.value.inputs, [{ txid, vout: 7 }]);
  assert.deepEqual(built.value.outputs, [{ address: "bcrt1qexample", amountBtc: "0.0005" }]);

  // Ordering validation rides model.ts normalizeSessionOrdering.
  const noSeed = buildCreateRequest({ ...form, seed: "" });
  assert.equal(noSeed.ok, false);
  assert.match(noSeed.error, /seed/);

  const badTxid = buildCreateRequest({
    ...form,
    inputs: [{ txid: "1234", vout: "0" }],
  });
  assert.equal(badTxid.ok, false);
  assert.match(badTxid.error, /txid/);

  const badVout = buildCreateRequest({
    ...form,
    inputs: [{ txid, vout: "-1" }],
  });
  assert.equal(badVout.ok, false);
  assert.match(badVout.error, /vout/);

  // An omitted vout defaults to 0 once a txid identifies the row (the row
  // template shows the 0 as a placeholder), so a pristine row stays blank
  // while a txid-only row still builds.
  const voutDefaulted = buildCreateRequest({
    ...form,
    inputs: [{ txid, vout: "" }],
  });
  assert.equal(voutDefaulted.ok, true);
  assert.deepEqual(voutDefaulted.value.inputs, [{ txid, vout: 0 }]);
  // A typed vout WITHOUT a txid is not blank: typed information is never
  // silently dropped.
  const voutOnly = buildCreateRequest({
    ...form,
    inputs: [{ txid: "", vout: "7" }],
  });
  assert.equal(voutOnly.ok, false);
  assert.match(voutOnly.error, /txid/);

  const halfOutput = buildCreateRequest({
    ...form,
    outputs: [{ address: "bcrt1qexample", amountBtc: "" }],
  });
  assert.equal(halfOutput.ok, false);
  assert.match(halfOutput.error, /address and amount/);

  // Zero rows is a valid request: /api/create returns an empty MODIFIABLE
  // unordered PSBT — the natural starting fragment for a session that grows
  // by joins.
  const nothing = buildCreateRequest({
    network: "regtest",
    ordering: "unset",
    seed: "",
    inputs: [],
    outputs: [],
  });
  assert.equal(nothing.ok, true);
  assert.deepEqual(nothing.value.inputs, []);
  assert.deepEqual(nothing.value.outputs, []);
});

test("buildSyncRequest local transport folds psbts, sources, and state", () => {
  const form = {
    transport: "local",
    sources: " /tmp/psbts \n\n/tmp/one.psbt\n",
    state: " /tmp/state.psbt ",
    irohTicket: "",
    irohTicketOut: false,
    irohWaitMs: "",
    webrtcRole: "",
    signalOut: "",
    signalIn: "",
    webrtcBind: "",
    iceServers: "",
    signalTimeoutMs: "",
  };
  const built = buildSyncRequest(form, [PSBT]);
  assert.equal(built.ok, true);
  assert.deepEqual(built.value, {
    transport: "local",
    psbts: [PSBT],
    sources: ["/tmp/psbts", "/tmp/one.psbt"],
    state: "/tmp/state.psbt",
  });

  const nothing = buildSyncRequest({ ...form, sources: "", state: "" }, []);
  assert.equal(nothing.ok, false);
  assert.match(nothing.error, /fragments or provide/);

  const badWait = buildSyncRequest({ ...form, irohWaitMs: "soon" }, [PSBT]);
  assert.equal(badWait.ok, false);
  assert.match(badWait.error, /wait ms/);
});

test("buildSyncRequest watched-dir needs the register directory, tolerates an empty selection", () => {
  const base = {
    transport: "watched-dir",
    sources: "/tmp/register\n/tmp/seed.psbt\n",
    state: "",
    irohTicket: "",
    irohTicketOut: false,
    irohWaitMs: "",
    webrtcRole: "",
    signalOut: "",
    signalIn: "",
    webrtcBind: "",
    iceServers: "",
    signalTimeoutMs: "",
  };
  // Zero fragments is fine: the register itself may hold the frontier. The
  // state field never rides along — the directory IS the register.
  const registerOnly = buildSyncRequest({ ...base, state: "/tmp/ignored.psbt" }, []);
  assert.equal(registerOnly.ok, true);
  assert.deepEqual(registerOnly.value, {
    transport: "watched-dir",
    sources: ["/tmp/register", "/tmp/seed.psbt"],
  });

  const withFragment = buildSyncRequest(base, [PSBT]);
  assert.equal(withFragment.ok, true);
  assert.deepEqual(withFragment.value, {
    transport: "watched-dir",
    psbts: [PSBT],
    sources: ["/tmp/register", "/tmp/seed.psbt"],
  });

  const noRegister = buildSyncRequest({ ...base, sources: " \n" }, [PSBT]);
  assert.equal(noRegister.ok, false);
  assert.match(noRegister.error, /register directory/);
});

test("buildSyncRequest iroh transport takes a ticket in xor out", () => {
  const base = {
    transport: "iroh",
    sources: "",
    state: "",
    irohTicket: "",
    irohTicketOut: false,
    irohWaitMs: "2500",
    webrtcRole: "",
    signalOut: "",
    signalIn: "",
    webrtcBind: "",
    iceServers: "",
    signalTimeoutMs: "",
  };
  const joinDoc = buildSyncRequest({ ...base, irohTicket: " doc-ticket " }, [PSBT]);
  assert.equal(joinDoc.ok, true);
  assert.deepEqual(joinDoc.value, {
    transport: "iroh",
    psbts: [PSBT],
    irohWaitMs: 2500,
    irohTicket: "doc-ticket",
  });

  const newDoc = buildSyncRequest({ ...base, irohTicketOut: true }, []);
  assert.equal(newDoc.ok, true);
  assert.deepEqual(newDoc.value, {
    transport: "iroh",
    irohWaitMs: 2500,
    irohTicketOut: true,
  });

  const both = buildSyncRequest({ ...base, irohTicket: "t", irohTicketOut: true }, []);
  assert.equal(both.ok, false);
  assert.match(both.error, /not both/);

  const neither = buildSyncRequest(base, [PSBT]);
  assert.equal(neither.ok, false);
  assert.match(neither.error, /ticket/);
});

test("buildSyncRequest webrtc transports carry the manual signaling params", () => {
  const base = {
    transport: "str0m",
    sources: "",
    state: "",
    irohTicket: "",
    irohTicketOut: false,
    irohWaitMs: "",
    webrtcRole: "offer",
    signalOut: "/tmp/us.sig",
    signalIn: "/tmp/peer.sig",
    webrtcBind: "127.0.0.1:0",
    iceServers: "stun:stun.example.org:3478\n",
    signalTimeoutMs: "1234",
  };
  const built = buildSyncRequest(base, [PSBT]);
  assert.equal(built.ok, true);
  assert.deepEqual(built.value, {
    transport: "str0m",
    psbts: [PSBT],
    webrtcRole: "offer",
    signalOut: "/tmp/us.sig",
    signalIn: "/tmp/peer.sig",
    webrtcBind: "127.0.0.1:0",
    iceServers: ["stun:stun.example.org:3478"],
    signalTimeoutMs: 1234,
  });

  const noRole = buildSyncRequest({ ...base, transport: "webrtc-rs", webrtcRole: "" }, []);
  assert.equal(noRole.ok, false);
  assert.match(noRole.error, /role/);

  const noFiles = buildSyncRequest({ ...base, signalIn: "" }, []);
  assert.equal(noFiles.ok, false);
  assert.match(noFiles.error, /signal-out and signal-in/);

  const defaults = buildSyncRequest(
    { ...base, webrtcBind: "", iceServers: "", signalTimeoutMs: "" },
    [],
  );
  assert.equal(defaults.ok, true);
  assert.deepEqual(defaults.value, {
    transport: "str0m",
    webrtcRole: "offer",
    signalOut: "/tmp/us.sig",
    signalIn: "/tmp/peer.sig",
  });

  const badTimeout = buildSyncRequest({ ...base, signalTimeoutMs: "later" }, []);
  assert.equal(badTimeout.ok, false);
  assert.match(badTimeout.error, /signal timeout/);
});

test("buildPayArgs address variant keeps the payer id opaque", () => {
  const base = {
    mode: "address",
    address: "bcrt1qexample",
    amountBtc: "0.0005",
    network: "regtest",
    label: " lunch ",
    payerHex: "AA".repeat(32),
    paymentHex: "",
    secretHex: "",
    dummy: "",
  };
  const built = buildPayArgs(base);
  assert.equal(built.ok, true);
  assert.deepEqual(built.value.payment, {
    address: "bcrt1qexample",
    amountBtc: "0.0005",
    network: "regtest",
    label: "lunch",
    payerHex: "aa".repeat(32),
  });
  assert.equal(built.value.options, undefined);

  const noPayer = buildPayArgs({ ...base, payerHex: "", label: "", network: "" });
  assert.equal(noPayer.ok, true);
  assert.deepEqual(noPayer.value.payment, {
    address: "bcrt1qexample",
    amountBtc: "0.0005",
    network: undefined,
    label: undefined,
    payerHex: undefined,
  });

  const shortPayer = buildPayArgs({ ...base, payerHex: "1234" });
  assert.equal(shortPayer.ok, false);
  assert.match(shortPayer.error, /payer id/);

  const missing = buildPayArgs({ ...base, address: "" });
  assert.equal(missing.ok, false);
  assert.match(missing.error, /address and amount/);
});

test("buildPayArgs opaque variant and encryption options", () => {
  const base = {
    mode: "hex",
    address: "",
    amountBtc: "",
    network: "",
    label: "",
    payerHex: "",
    paymentHex: " DEADBEEF ",
    secretHex: "0011",
    dummy: "2",
  };
  const built = buildPayArgs(base);
  assert.equal(built.ok, true);
  assert.equal(built.value.payment, "deadbeef");
  assert.deepEqual(built.value.options, { secretHex: "0011", dummy: 2 });

  const badRecord = buildPayArgs({ ...base, paymentHex: "xyz" });
  assert.equal(badRecord.ok, false);
  assert.match(badRecord.error, /payment record/);

  const dummyNoSecret = buildPayArgs({ ...base, secretHex: "" });
  assert.equal(dummyNoSecret.ok, false);
  assert.match(dummyNoSecret.error, /dummy padding requires a secret/);

  const badSecret = buildPayArgs({ ...base, secretHex: "abc" });
  assert.equal(badSecret.ok, false);
  assert.match(badSecret.error, /secret/);

  const badDummy = buildPayArgs({ ...base, dummy: "many" });
  assert.equal(badDummy.ok, false);
  assert.match(badDummy.error, /dummy count/);
});

test("buildConfirmArgs derives or passes an opaque record", () => {
  const base = {
    mode: "derive",
    confirmationHex: "",
    peerIdHex: "",
    secretHex: "",
  };
  const derived = buildConfirmArgs(base);
  assert.equal(derived.ok, true);
  assert.deepEqual(derived.value.confirmation, { derive: true, peerIdHex: undefined });
  assert.equal(derived.value.options, undefined);

  const withPeer = buildConfirmArgs({ ...base, peerIdHex: "BB".repeat(32), secretHex: "0011" });
  assert.equal(withPeer.ok, true);
  assert.deepEqual(withPeer.value.confirmation, { derive: true, peerIdHex: "bb".repeat(32) });
  assert.deepEqual(withPeer.value.options, { secretHex: "0011" });

  const badPeer = buildConfirmArgs({ ...base, peerIdHex: "12" });
  assert.equal(badPeer.ok, false);
  assert.match(badPeer.error, /peer id/);

  const opaque = buildConfirmArgs({ ...base, mode: "hex", confirmationHex: "C0FFEE00" });
  assert.equal(opaque.ok, true);
  assert.equal(opaque.value.confirmation, "c0ffee00");

  const badOpaque = buildConfirmArgs({ ...base, mode: "hex", confirmationHex: "" });
  assert.equal(badOpaque.ok, false);
  assert.match(badOpaque.error, /confirmation record/);

  const badSecret = buildConfirmArgs({ ...base, secretHex: "zz" });
  assert.equal(badSecret.ok, false);
  assert.match(badSecret.error, /secret/);
});

test("paste and upload helpers", () => {
  assert.equal(pastedPsbt(`  ${PSBT}\n`), PSBT);
  assert.equal(pastedPsbt("not a psbt"), null);
  assert.equal(pastedPsbt(""), null);

  assert.equal(isHexBytes("deadBEEF"), true);
  assert.equal(isHexBytes("abc"), false);
  assert.equal(isHexBytes("", 0), false);
  assert.equal(isHexBytes("aa".repeat(32), 32), true);
  assert.equal(isHexBytes("aa".repeat(31), 32), false);

  assert.deepEqual(parseLines(" a \n\n b\n"), ["a", "b"]);
  assert.deepEqual(parseLines(""), []);

  // bytesToBase64 agrees with node's own encoder, including padding shapes.
  for (const bytes of [
    new Uint8Array([]),
    new Uint8Array([0x70]),
    new Uint8Array([0x70, 0x73]),
    new Uint8Array([0x70, 0x73, 0x62]),
    new Uint8Array([0x70, 0x73, 0x62, 0x74, 0xff, 0x00, 0x01]),
  ]) {
    assert.equal(bytesToBase64(bytes), Buffer.from(bytes).toString("base64"));
  }
});
