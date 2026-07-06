import test from "node:test";
import assert from "node:assert/strict";

import * as model from "../dist/model.js";
import {
  amountParts,
  compactBase64,
  coinDetailLines,
  accountingDeltaPresentation,
  balanceSheetFeeSignal,
  descriptorLooksPrivate,
  descriptorDrawerItems,
  descriptorMenuState,
  finalizePayload,
  formatSatAmount,
  hashHex,
  joinSessionSeeds,
  looksLikeBase64Psbt,
  looksLikeDescriptor,
  mergePayloads,
  orderByStableId,
  orderedProjectionPayload,
  parseBitcoinUri,
  pendingPayloadRowKeys,
  peerAckPlan,
  peerBridgeComponents,
  peerEdgeTermination,
  peerGroupBounds,
  peerIsInteractive,
  psbtRole,
  peerLatencyProfile,
  psbtCompatibility,
  psbtProtocolIdentity,
  psbtsAreCompatible,
  psbtUnaryActions,
  samplePeerAckDelay,
  seedFromRandomBytes,
  sessionVisibleToPeerGroup,
  normalizeSessionOrdering,
  sneakernetFragmentStatus,
  transactionBalance,
  totalSats,
  shouldShowGrandTotal,
  unorderedBalanceSheetTotalRows,
  unorderedPsbtDisplay,
} from "../dist/model.js";

test("amount formatting mutes bitcoin scale and leading zero sat digits", () => {
  assert.deepEqual(amountParts(0), { prefix: "", muted: "₿0.00000000", sats: "" });
  assert.deepEqual(amountParts(1_200_000), { prefix: "", muted: "₿0.0", sats: "1200000" });
  assert.deepEqual(amountParts(9), { prefix: "", muted: "₿0.0000000", sats: "9" });
  assert.deepEqual(amountParts(100_000_009), { prefix: "₿1", muted: ".0000000", sats: "9" });
  assert.deepEqual(amountParts(100_000_000), { prefix: "₿1", muted: ".00000000", sats: "" });
  assert.equal(formatSatAmount(0), "₿0.00000000");
});

test("coinDetailLines keeps coin labels in expanded details and includes input proof fields", () => {
  assert.deepEqual(coinDetailLines("input", {
    id: "input-a",
    label: "Alice savings coin",
    outpoint: "a".repeat(64) + ":1",
    nSequence: "0xfffffffd",
    partialSignatures: ["30440220demo-signature"],
    finalScriptWitness: "024730440220demo-witness",
    finalScriptSig: "160014demo-scriptsig",
    signatureVerified: true,
    vbytes: 104,
  }), [
    "label Alice savings coin",
    `outpoint ${"a".repeat(64)}:1`,
    "nSequence 0xfffffffd",
    "authorized",
    "size 104 vB",
  ]);

  assert.deepEqual(coinDetailLines("input", {
    id: "input-b",
    outpoint: "b".repeat(64) + ":0",
    finalScriptWitness: "024730440220demo-witness",
    finalScriptSig: "160014demo-scriptsig",
    estimatedVbytes: 91,
  }), [
    `outpoint ${"b".repeat(64)}:0`,
    "nSequence 0xffffffff",
    "witness 024730440220demo-witness",
    "scriptSig 160014demo-scriptsig",
    "size estimate 91 vB",
  ]);
  assert.deepEqual(coinDetailLines("input", {
    id: "input-c",
    outpoint: "c".repeat(64) + ":2",
    partialSignatures: ["30440220sig-a", "30440220sig-b"],
  }), [
    `outpoint ${"c".repeat(64)}:2`,
    "nSequence 0xffffffff",
    "signature 30440220sig-a",
    "signature 30440220sig-b",
    "size estimate 68 vB",
  ]);

  assert.deepEqual(coinDetailLines("output", {
    id: "output-a",
    label: "Bob payment request",
    address: "bcrt1qexample",
    scriptHash: "d34db33f",
    vbytes: 43,
  }), [
    "label Bob payment request",
    "script d34db33f",
    "size 43 vB",
  ]);

  assert.deepEqual(coinDetailLines("input", {
    id: "fallback-input",
  }, 4), [
    "nSequence 0xffffffff",
    "size estimate 68 vB",
  ]);
  assert.deepEqual(coinDetailLines("input", {
    id: "",
    sequence: "0",
  }, 1), [
    "nSequence 0",
    "size estimate 68 vB",
  ]);
  assert.deepEqual(coinDetailLines("output", {
    id: "output-b",
  }), [
    "size 31 vB",
  ]);
});

test("PSBT size estimates support vbytes and weight-unit display", () => {
  assert.equal(typeof model.itemSizeEstimate, "function");
  assert.equal(typeof model.payloadSizeEstimate, "function");
  assert.equal(typeof model.formatSizeEstimate, "function");

  assert.deepEqual(model.itemSizeEstimate("input", { id: "legacy" }), {
    vbytes: 68,
    weightUnits: 272,
    exact: false,
  });
  assert.deepEqual(model.itemSizeEstimate("input", { id: "tap-script", estimatedVbytes: 50, scriptType: "p2tr-script-path" }), {
    vbytes: 68,
    weightUnits: 272,
    exact: false,
  });
  assert.deepEqual(model.itemSizeEstimate("input", { id: "tap-bool", estimatedVbytes: 50, scriptPath: true }), {
    vbytes: 68,
    weightUnits: 272,
    exact: false,
  });
  assert.deepEqual(model.itemSizeEstimate("input", { id: "tap-witness", estimatedVbytes: 50, witnessType: "p2tr script path" }), {
    vbytes: 68,
    weightUnits: 272,
    exact: false,
  });
  assert.deepEqual(model.itemSizeEstimate("output", { id: "out", vbytes: 43 }), {
    vbytes: 43,
    weightUnits: 172,
    exact: true,
  });
  const payloadEstimate = model.payloadSizeEstimate({
    inputs: [{ id: "a", estimatedVbytes: 80 }, { id: "b" }],
    outputs: [{ id: "out", vbytes: 43 }],
    conflicts: [],
  });
  assert.deepEqual(payloadEstimate, {
    inputVbytes: 148,
    outputVbytes: 43,
    totalVbytes: 191,
    totalWeightUnits: 764,
  });
  assert.deepEqual(model.payloadSizeEstimate({ conflicts: [] }), {
    inputVbytes: 0,
    outputVbytes: 0,
    totalVbytes: 0,
    totalWeightUnits: 0,
  });
  assert.equal(model.formatSizeEstimate(68, "vbytes"), "68 vB");
  assert.equal(model.formatSizeEstimate(12.5, "vbytes"), "12.5 vB");
  assert.equal(model.formatSizeEstimate(68, "weight-units"), "272 WU");
  assert.equal(model.formatSizeEstimate(payloadEstimate, "vbytes"), "191 vB");
  assert.equal(model.formatSizeEstimate(payloadEstimate, "weight-units"), "764 WU");
  assert.deepEqual(coinDetailLines("input", { id: "tap", taprootScriptPath: true }, 0, "weight-units"), [
    "nSequence 0xffffffff",
    "size estimate 272 WU",
  ]);
});

test("session ordering config requires det seeds and accepts optional unset seeds", () => {
  assert.deepEqual(normalizeSessionOrdering("det", " 0001FEff "), {
    mode: "det",
    seed: "0001feff",
    valid: true,
  });
  assert.deepEqual(normalizeSessionOrdering("det", "not-hex"), {
    mode: "det",
    seed: "",
    valid: false,
    error: "ordering seed must be hex bytes",
  });
  assert.deepEqual(normalizeSessionOrdering("det", ""), {
    mode: "det",
    seed: "",
    valid: false,
    error: "deterministic ordering requires a seed",
  });
  assert.deepEqual(normalizeSessionOrdering("explicit", "ignored"), {
    mode: "explicit",
    seed: "",
    valid: true,
  });
  assert.deepEqual(normalizeSessionOrdering("unset", ""), {
    mode: "unset",
    seed: "",
    valid: true,
  });
  assert.deepEqual(normalizeSessionOrdering("unset", " 0A0b "), {
    mode: "unset",
    seed: "0a0b",
    valid: true,
  });
  assert.deepEqual(normalizeSessionOrdering("unset", "not-hex"), {
    mode: "unset",
    seed: "",
    valid: false,
    error: "ordering seed must be hex bytes",
  });
  assert.equal(seedFromRandomBytes(Uint8Array.from([0, 1, 254, 255])), "0001feff");
  assert.equal(seedFromRandomBytes([]), "");
});

test("mergePayloads deduplicates, sorts, and reports domain conflicts", () => {
  const left = {
    inputs: [{ id: "b", valueSats: 2 }, { id: "a", valueSats: 1 }],
    outputs: [{ id: "out", valueSats: 3 }],
    descriptors: [{ id: "desc", privacy: "public", descriptor: "wpkh(xpub)" }],
    conflicts: ["prior"],
  };
  const right = {
    inputs: [{ id: "a", valueSats: 1 }, { id: "b", valueSats: 20 }],
    outputs: [{ id: "out", valueSats: 4 }],
    descriptors: [{ id: "desc", privacy: "private", descriptor: "wpkh(xprv)" }],
    conflicts: [],
  };
  assert.deepEqual(mergePayloads(left, right), {
    inputs: [{ id: "a", valueSats: 1 }, { id: "b", valueSats: 2 }],
    outputs: [{ id: "out", valueSats: 3 }],
    descriptors: [{ id: "desc", privacy: "public", descriptor: "wpkh(xpub)" }],
    conflicts: ["descriptor:desc", "input:b", "output:out", "prior"],
  });
  assert.deepEqual(mergePayloads({ conflicts: [] }), {
    inputs: [],
    outputs: [],
    descriptors: [],
    conflicts: [],
  });
  assert.deepEqual(mergePayloads({}), {
    inputs: [],
    outputs: [],
    descriptors: [],
    conflicts: [],
  });
});

test("PSBT compatibility requires unordered conflict-free payload joins", () => {
  const base = { id: "a", format: "unordered", inputs: [{ id: "in" }], outputs: [], conflicts: [] };
  const compatible = { id: "b", format: "unordered", inputs: [], outputs: [{ id: "out" }], conflicts: [] };
  const rightOrdered = { ...compatible, id: "c", format: "bip370" };
  const leftOrdered = { ...base, id: "e", format: "bip370" };
  const conflicting = { id: "d", format: "unordered", inputs: [{ id: "in", valueSats: 2 }], outputs: [], conflicts: [] };
  const det = { ...base, id: "det", sortMode: "det", seed: "0a" };
  const explicit = { ...compatible, id: "explicit", sortMode: "explicit" };
  const unset = { ...compatible, id: "unset", sortMode: "unset" };
  assert.equal(psbtsAreCompatible(base, compatible), true);
  assert.equal(psbtsAreCompatible(base, rightOrdered), false);
  assert.equal(psbtsAreCompatible(leftOrdered, compatible), false);
  assert.equal(psbtsAreCompatible(base, conflicting), false);
  assert.equal(psbtsAreCompatible(det, explicit), false);
  assert.equal(psbtsAreCompatible(det, unset), true);
  assert.deepEqual(psbtCompatibility(det, explicit), {
    ok: false,
    reason: "ordering policy conflict: deterministic cannot join explicit",
  });
  assert.deepEqual(psbtCompatibility(base, conflicting), {
    ok: false,
    reason: "payload conflict: input:in",
  });
});

test("psbtRole maps format and modifiability to protocol roles", () => {
  assert.deepEqual(psbtRole({ id: "s", format: "unordered", inputs: [], outputs: [], conflicts: [] }, "session"), {
    id: "unordered-register",
    label: "constructor<modifiable, unordered>",
    spec: "multiparty PSBT register",
    roles: ["Constructor", "Combiner", "Sync"],
  });
  assert.deepEqual(psbtRole({ id: "f", format: "unordered", inputs: [], outputs: [], conflicts: [] }, "fragment"), {
    id: "unordered-fragment",
    label: "constructor<modifiable, unordered>",
    spec: "multiparty PSBT fragment",
    roles: ["Constructor", "Combiner"],
  });
  assert.deepEqual(psbtRole({ id: "c", format: "bip370", modifiable: "inputs", inputs: [], outputs: [], conflicts: [] }, "fragment"), {
    id: "bip370-constructor",
    label: "constructor<modifiable inputs>",
    spec: "BIP 370",
    roles: ["Constructor", "Updater"],
  });
  assert.deepEqual(psbtRole({ id: "b", format: "bip370", modifiable: "both", inputs: [], outputs: [], conflicts: [] }, "fragment"), {
    id: "bip370-constructor",
    label: "constructor<modifiable inputs+outputs>",
    spec: "BIP 370",
    roles: ["Constructor", "Updater"],
  });
  assert.equal(
    psbtRole({ id: "default", format: "bip370", inputs: [], outputs: [], conflicts: [] }, "fragment").label,
    "constructor<modifiable inputs+outputs>",
  );
  assert.deepEqual(psbtRole({ id: "o", format: "bip370", modifiable: "none", inputs: [], outputs: [], conflicts: [] }, "fragment"), {
    id: "fixed-transaction",
    label: "Fixed transaction",
    spec: "BIP 174 compatible",
    roles: ["Updater", "Signer"],
  });
  assert.deepEqual(psbtRole({ id: "x", format: "bip174", inputs: [], outputs: [], conflicts: [] }, "fragment"), {
    id: "fixed-transaction",
    label: "Fixed transaction",
    spec: "BIP 174",
    roles: ["Updater", "Signer"],
  });
  assert.deepEqual(psbtRole({ id: "z", format: "bip370", kind: "sorter-output", inputs: [], outputs: [], conflicts: [] }, "fragment"), {
    id: "sorted-bip370",
    label: "fixed input/output set",
    spec: "Sorter output",
    roles: ["Updater", "Signer"],
  });
});

test("psbtProtocolIdentity distinguishes fixed segwit txids from PSBT unique ids", () => {
  const fixedSegwit = {
    id: "frag-04",
    format: "bip370",
    modifiable: "none",
    inputs: [{ id: "in-b", outpoint: "b:1" }, { id: "in-a", outpoint: "a:0" }],
    outputs: [{ id: "out", address: "bcrt1qrecipient", valueSats: 1234 }],
    conflicts: [],
  };
  const fixedIdentity = psbtProtocolIdentity(fixedSegwit, "fragment");
  assert.deepEqual({
    label: fixedIdentity.label,
    source: fixedIdentity.source,
    stableBeforeSigning: fixedIdentity.stableBeforeSigning,
    valueLength: fixedIdentity.value.length,
  }, {
    label: "txid",
    source: "ordered non-modifiable SegWit transaction",
    stableBeforeSigning: true,
    valueLength: 64,
  });
  assert.equal(psbtProtocolIdentity({ ...fixedSegwit, id: "frag-renamed" }, "fragment").value, fixedIdentity.value);

  const unorderedIdentity = psbtProtocolIdentity({
    id: "session-local",
    format: "unordered",
    sortMode: "det",
    seed: "0a0b",
    inputs: [{ id: "in-a" }],
    outputs: [],
    conflicts: [],
  }, "session");
  assert.deepEqual({
    label: unorderedIdentity.label,
    source: unorderedIdentity.source,
    stableBeforeSigning: unorderedIdentity.stableBeforeSigning,
  }, {
    label: "unique id",
    source: "psbt.md unordered PSBT unique id",
    stableBeforeSigning: false,
  });

  const modifiableIdentity = psbtProtocolIdentity({
    id: "candidate",
    format: "bip370",
    modifiable: "inputs",
    inputs: [],
    outputs: [{ id: "out" }],
    conflicts: [],
  }, "fragment");
  assert.deepEqual({
    label: modifiableIdentity.label,
    source: modifiableIdentity.source,
  }, {
    label: "unique id",
    source: "BIP 370 PSBT unique id",
  });

  const legacyFixedIdentity = psbtProtocolIdentity({
    id: "legacy",
    format: "bip174",
    segwit: false,
    inputs: [{ id: "in" }],
    outputs: [],
    conflicts: [],
  }, "fragment");
  assert.deepEqual({
    label: legacyFixedIdentity.label,
    source: legacyFixedIdentity.source,
    stableBeforeSigning: legacyFixedIdentity.stableBeforeSigning,
  }, {
    label: "unique id",
    source: "BIP 174 PSBT unique id",
    stableBeforeSigning: false,
  });

  const versionedFixedIdentity = psbtProtocolIdentity({
    ...fixedSegwit,
    version: 3,
    lockTime: 42,
    inputs: [{ txid: "a".repeat(64), vout: 0, sequence: 0xfffffffd }],
    outputs: [{ scriptPubKey: "51", valueSats: 1 }],
  }, "fragment");
  assert.equal(versionedFixedIdentity.label, "txid");
  assert.notEqual(versionedFixedIdentity.value, fixedIdentity.value);

  const legacyInputIdentity = psbtProtocolIdentity({
    ...fixedSegwit,
    inputs: [{ id: "legacy-input", legacy: true }],
  }, "fragment");
  assert.deepEqual({
    label: legacyInputIdentity.label,
    source: legacyInputIdentity.source,
    stableBeforeSigning: legacyInputIdentity.stableBeforeSigning,
  }, {
    label: "unique id",
    source: "BIP 370 PSBT unique id",
    stableBeforeSigning: false,
  });

  const nonSegwitInputIdentity = psbtProtocolIdentity({
    ...fixedSegwit,
    inputs: [{ id: "non-segwit-input", segwit: false }],
  }, "fragment");
  assert.equal(nonSegwitInputIdentity.label, "unique id");
});

test("psbtUnaryActions are derived from role state and vertex kind", () => {
  assert.deepEqual(psbtUnaryActions({ id: "u", format: "unordered", inputs: [], outputs: [], conflicts: [] }, "fragment"), [
    "sort",
    "promote",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "unmaterialized", format: "unordered", conflicts: [] }, "fragment"), [
    "sort",
    "promote",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "ua", format: "unordered", inputs: [{ id: "in" }], outputs: [], conflicts: [] }, "fragment"), [
    "sort",
    "promote",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "uo", format: "unordered", inputs: [], outputs: [{ id: "out" }], conflicts: [] }, "fragment"), [
    "sort",
    "promote",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "um", format: "unordered", inputs: [{ id: "in" }], outputs: [{ id: "out" }], conflicts: [] }, "fragment"), [
    "sort",
    "atomize",
    "promote",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "s", format: "unordered", inputs: [], outputs: [], conflicts: [] }, "session"), [
    "fix-sets",
    "abort-session",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "c", format: "bip370", modifiable: "both", inputs: [{ id: "in" }], outputs: [], conflicts: [] }, "fragment"), [
    "make-unordered",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "cm", format: "bip370", modifiable: "both", inputs: [{ id: "in" }], outputs: [{ id: "out" }], conflicts: [] }, "fragment"), [
    "make-unordered",
    "atomize",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "cs", format: "bip370", modifiable: "outputs", inputs: [], outputs: [], conflicts: [] }, "session"), [
    "make-unordered",
  ]);
  assert.deepEqual(psbtUnaryActions({ id: "f", format: "bip174", inputs: [], outputs: [], conflicts: [] }, "fragment"), []);
  assert.deepEqual(psbtUnaryActions({ id: "z", format: "bip370", kind: "sorter-output", inputs: [], outputs: [], conflicts: [] }, "fragment"), [
    "make-unordered",
  ]);
});

test("orderedProjectionPayload sorts coins and drops descriptor/session metadata", () => {
  const projection = orderedProjectionPayload({
    inputs: [{ id: "z" }, { id: "a" }],
    outputs: [{ id: "out-2" }, { id: "out-1" }],
    descriptors: [{ id: "desc", privacy: "public", descriptor: "wpkh(xpub)" }],
    conflicts: ["warning"],
  });
  assert.deepEqual(projection.inputs.map((input) => input.id), ["a", "z"]);
  assert.deepEqual(projection.outputs.map((output) => output.id), ["out-1", "out-2"]);
  assert.deepEqual(projection.descriptors, []);
  assert.deepEqual(projection.conflicts, ["warning"]);
  assert.equal(orderByStableId({ id: "", label: "b" }, { id: "", address: "a" }), 1);
  assert.equal(orderByStableId({}, {}), 0);
  assert.deepEqual(orderedProjectionPayload({}), {
    inputs: [],
    outputs: [],
    descriptors: [],
    conflicts: [],
  });
});

test("finalizePayload accounts for inputs, outputs, fee, and deficits", () => {
  assert.deepEqual(finalizePayload({
    inputs: [{ id: "in", valueSats: 10 }],
    outputs: [{ id: "out", valueSats: 7 }],
    conflicts: [],
  }), { inputTotal: 10, outputTotal: 7, fee: 3, status: "finalized" });
  assert.deepEqual(finalizePayload({
    inputs: [{ id: "in", valueSats: 1 }],
    outputs: [{ id: "out", valueSats: 2 }],
    conflicts: [],
  }), { inputTotal: 1, outputTotal: 2, fee: -1, status: "blocked" });
  assert.equal(totalSats([{ valueSats: 4 }, {}, { valueSats: 6 }]), 10);
});

test("transactionBalance separates total, explicit, implicit, mine, and other balances", () => {
  assert.deepEqual(transactionBalance({
    inputs: [
      { id: "mine-in", valueSats: 1200, descriptorMine: true, explicitFeeSats: 50 },
      { id: "other-in", valueSats: 900, descriptorMine: false, explicitFeeSats: 10 },
    ],
    outputs: [
      { id: "mine-change", valueSats: 1150, descriptorMine: true },
      { id: "other-change", valueSats: 800, descriptorMine: false },
    ],
    conflicts: [],
  }), {
    inputs: 2100,
    outputs: 1950,
    fee: { explicit: 60, implicit: 90, total: 150 },
    mine: {
      inputs: 1200,
      outputs: 1150,
      explicitFee: 50,
      implicitFee: 0,
      net: 0,
      balanced: true,
    },
    other: {
      inputs: 900,
      outputs: 800,
      explicitFee: 10,
      implicitFee: 90,
      net: 90,
      balanced: false,
    },
    mineBalanced: true,
    status: "mine-balanced",
  });
});

test("transactionBalance reports deficits and treats unrecognized coins as other", () => {
  assert.deepEqual(transactionBalance({
    inputs: [{ id: "unknown-in", valueSats: 500 }],
    outputs: [
      { id: "mine-out", valueSats: 200, descriptorMine: true },
      { id: "unknown-out", valueSats: 400 },
    ],
    conflicts: [],
  }), {
    inputs: 500,
    outputs: 600,
    fee: { explicit: 0, implicit: -100, total: -100 },
    mine: {
      inputs: 0,
      outputs: 200,
      explicitFee: 0,
      implicitFee: -200,
      net: -200,
      balanced: false,
    },
    other: {
      inputs: 500,
      outputs: 400,
      explicitFee: 0,
      implicitFee: 100,
      net: 100,
      balanced: false,
    },
    mineBalanced: false,
    status: "deficit",
  });
});

test("transactionBalance distinguishes fully balanced and mine-unbalanced states", () => {
  assert.equal(transactionBalance({
    inputs: [{ id: "mine-in", valueSats: 100, descriptorMine: true }],
    outputs: [{ id: "mine-out", valueSats: 100, descriptorMine: true }],
    conflicts: [],
  }).status, "balanced");
  assert.equal(transactionBalance({
    inputs: [{ id: "mine-in", valueSats: 100, descriptorMine: true }],
    outputs: [{ id: "mine-out", valueSats: 90, descriptorMine: true }],
    conflicts: [],
  }).status, "mine-unbalanced");
});

test("sneakernetFragmentStatus keeps peerless PSBT work on the normal join path", () => {
  assert.deepEqual(sneakernetFragmentStatus([], [], []), {
    peerless: true,
    peers: 0,
    sessions: 0,
    fragments: 0,
    ordered: 0,
    unordered: 0,
    psbts: 0,
    canExport: false,
    nextAction: "import",
  });
  assert.equal(sneakernetFragmentStatus([], [], [
    { id: "ordered", format: "bip370", inputs: [], outputs: [], conflicts: [] },
  ]).nextAction, "make-unordered");
  assert.equal(sneakernetFragmentStatus([], [], [
    { id: "a", format: "unordered", inputs: [], outputs: [], conflicts: [] },
    { id: "b", format: "unordered", inputs: [], outputs: [], conflicts: [] },
  ]).nextAction, "select-export");
  assert.equal(sneakernetFragmentStatus([{ id: "peer" }], [], [
    { id: "a", format: "unordered", inputs: [], outputs: [], conflicts: [] },
  ]).peerless, false);
  assert.equal(sneakernetFragmentStatus([], [
    { id: "session", format: "unordered", inputs: [], outputs: [], conflicts: [] },
  ], []).nextAction, "export");
});

test("peer ack plans sample independent delays from per-peer latency profiles", () => {
  const alice = peerLatencyProfile("alice");
  const bob = peerLatencyProfile("bob");
  assert.equal(samplePeerAckDelay("alice", () => 0), alice.minMs);
  assert.equal(samplePeerAckDelay("alice", () => 1), alice.minMs + alice.jitterMs);
  assert.equal(samplePeerAckDelay("alice", () => Number.NaN), alice.minMs);
  assert.equal(samplePeerAckDelay("alice", () => -1), alice.minMs);

  const samples = [0, 1];
  const plan = peerAckPlan(["alice", "bob", "alice"], () => samples.shift());
  assert.equal(plan.total, 2);
  assert.deepEqual(new Set(plan.peers), new Set(["alice", "bob"]));
  assert.deepEqual(plan.acks.map((ack) => ack.acked), [1, 2]);
  assert.deepEqual(plan.acks.map((ack) => ack.total), [2, 2]);
  assert.deepEqual(
    plan.acks.map((ack) => ack.delayMs),
    [...plan.acks.map((ack) => ack.delayMs)].sort((left, right) => left - right),
  );
  assert.equal(plan.completionDelayMs, Math.max(alice.minMs, bob.minMs + bob.jitterMs));

  assert.deepEqual(peerAckPlan(["", null, undefined], () => 0), {
    peers: [],
    total: 0,
    acks: [],
    completionDelayMs: 0,
  });
});

test("parseBitcoinUri accepts amount, sats, address parameter, and empty value", () => {
  assert.deepEqual(parseBitcoinUri("pay bitcoin:bcrt1qdemo?amount=0.00123456&label=Demo&message=Hi&ptj_descriptor=utxo-01"), {
    uri: "bitcoin:bcrt1qdemo?amount=0.00123456&label=Demo&message=Hi&ptj_descriptor=utxo-01",
    address: "bcrt1qdemo",
    valueSats: 123456,
    descriptorId: "utxo-01",
    label: "Demo",
    message: "Hi",
  });
  assert.equal(parseBitcoinUri("bitcoin:bcrt1qdemo?sats=42").valueSats, 42);
  assert.deepEqual(parseBitcoinUri("bitcoin:?address=bcrt1qparam"), {
    uri: "bitcoin:?address=bcrt1qparam",
    address: "bcrt1qparam",
    valueSats: 0,
    descriptorId: null,
    label: "BIP 321 request",
    message: "",
  });
  assert.equal(parseBitcoinUri("bitcoin:?amount=1"), null);
  assert.equal(parseBitcoinUri("not a uri"), null);
  assert.equal(parseBitcoinUri(""), null);
});

test("paste classifier primitives detect PSBTs and descriptors", () => {
  assert.equal(compactBase64("cHNidP\nAAAA"), "cHNidPAAAA");
  assert.equal(compactBase64(null), "");
  assert.equal(looksLikeBase64Psbt("cHNidPAAAA"), true);
  assert.equal(looksLikeBase64Psbt("cHNidP"), false);
  assert.equal(looksLikeBase64Psbt("cHNidP!!!!!!!!!"), false);
  assert.equal(looksLikeBase64Psbt("not psbt"), false);
  assert.equal(looksLikeDescriptor("wpkh([abcd]xpub/0/*)"), true);
  assert.equal(looksLikeDescriptor("not(desc)"), false);
  assert.equal(descriptorLooksPrivate("wpkh([abcd]xprv/0/*)"), true);
  assert.equal(descriptorLooksPrivate("wpkh([abcd]xpub/0/*)"), false);
});

test("descriptorMenuState exposes ownership, color, and payment request actions", () => {
  assert.deepEqual(descriptorMenuState({
    id: "utxo-01",
    privacy: "public",
    ownership: "other",
    color: "#1967d2",
  }, ["#1967d2", "#b65c00"]), {
    ownership: "other",
    ownershipActions: [
      { id: "tag-mine", label: "Tag mine", disabled: false },
      { id: "tag-other", label: "Tag other", disabled: true },
    ],
    colorChoices: [
      { color: "#1967d2", selected: true },
      { color: "#b65c00", selected: false },
    ],
    paymentRequestAction: { id: "payment-request", label: "Generate payment request URI" },
  });

  assert.deepEqual(descriptorMenuState({
    id: "utxo-02",
    privacy: "private",
    ownership: "other",
    color: "#b65c00",
  }, ["#1967d2", "#b65c00"]), {
    ownership: "mine",
    ownershipActions: [
      { id: "tag-mine", label: "Tag mine", disabled: true },
      { id: "tag-other", label: "Tag other", disabled: true },
    ],
    colorChoices: [
      { color: "#1967d2", selected: false },
      { color: "#b65c00", selected: true },
    ],
    paymentRequestAction: { id: "payment-request", label: "Generate payment request URI" },
  });

  assert.equal(descriptorMenuState({
    id: "utxo-03",
    privacy: "public",
    ownership: "mine",
    color: "#607d00",
  }, []).ownership, "mine");
});

test("descriptorDrawerItems groups spend and payment-request sources by descriptor", () => {
  const sources = [
    { kind: "utxo", id: "utxo-alice", descriptorId: "alice", label: "Alice coin", valueSats: 12_000, promotedTo: "frag-01" },
    { kind: "payment-request", id: "req-alice", descriptorId: "alice", label: "Alice request", valueSats: 34_000, uri: "bitcoin:bcrt1qalice" },
    { kind: "payment-request", id: "req-loose", descriptorId: null, label: "Loose request", valueSats: 56_000, uri: "bitcoin:bcrt1qloose", promotedTo: "frag-02" },
    { kind: "utxo", id: "utxo-loose", descriptorId: "" },
    { kind: "peer-provenance", id: "peer-bob", descriptorId: "alice", label: "Added by Bob", valueSats: 1 },
  ];

  assert.deepEqual(descriptorDrawerItems("alice", sources), [
    { kind: "utxo", id: "utxo-alice", label: "Alice coin", valueSats: 12_000, promotedTo: "frag-01", uri: null },
    { kind: "payment-request", id: "req-alice", label: "Alice request", valueSats: 34_000, promotedTo: null, uri: "bitcoin:bcrt1qalice" },
  ]);
  assert.deepEqual(descriptorDrawerItems(null, sources), [
    { kind: "payment-request", id: "req-loose", label: "Loose request", valueSats: 56_000, promotedTo: "frag-02", uri: "bitcoin:bcrt1qloose" },
    { kind: "utxo", id: "utxo-loose", label: "utxo-loose", valueSats: 0, promotedTo: null, uri: null },
  ]);
});

test("unorderedPsbtDisplay groups recognized rows by descriptor and keeps explicit fees summarized", () => {
  const display = unorderedPsbtDisplay({
    inputs: [
      { id: "alice-in", valueSats: 1000, descriptorId: "alice", descriptorLabel: "Alice vault", descriptorColor: "#1967d2", descriptorMine: true, explicitFeeSats: 20 },
      { id: "unknown-in", valueSats: 700, descriptorMine: false, explicitFeeSats: 30 },
      { id: "bob-in", valueSats: 500, descriptorId: "bob", descriptorLabel: "Bob request", descriptorColor: "#b65c00", descriptorMine: false },
    ],
    outputs: [
      { id: "bob-out", valueSats: 480, descriptorId: "bob", descriptorLabel: "Bob request", descriptorColor: "#b65c00", descriptorMine: false, explicitFeeSats: 5 },
      { id: "unknown-out", valueSats: 650, descriptorMine: false },
      { id: "alice-out", valueSats: 970, descriptorId: "alice", descriptorLabel: "Alice vault", descriptorColor: "#1967d2", descriptorMine: true },
    ],
    conflicts: [],
  });

  assert.equal(display.explicitFeeSats, 55);
  assert.deepEqual(display.inputs.map((section) => [section.kind, section.descriptorId, section.label]), [
    ["recognized", "alice", "Alice vault"],
    ["recognized", "bob", "Bob request"],
    ["unrecognized", undefined, "unrecognized"],
  ]);
  assert.deepEqual(display.inputs.map((section) => section.rows.map((row) => row.id)), [["alice-in"], ["bob-in"], ["unknown-in"]]);
  assert.deepEqual(display.outputs.map((section) => [section.kind, section.descriptorId, section.label]), [
    ["recognized", "bob", "Bob request"],
    ["recognized", "alice", "Alice vault"],
    ["unrecognized", undefined, "unrecognized"],
  ]);
  assert.deepEqual(display.outputs.map((section) => section.rows.map((row) => row.id)), [["bob-out"], ["alice-out"], ["unknown-out"]]);
  assert.equal(display.inputs[0].totalSats, 1000);
  assert.equal(display.inputs[1].totalSats, 500);
  assert.equal(display.inputs[2].totalSats, 700);
  assert.equal(display.outputs[0].totalSats, 480);
  assert.equal(display.outputs[1].totalSats, 970);
  assert.equal(display.outputs[2].totalSats, 650);
  assert.deepEqual(display.subtransactions.map((section) => [
    section.kind,
    section.descriptorId,
    section.label,
    section.inputTotalSats,
    section.outputTotalSats,
    section.feeSats,
    section.outputFeeSats,
    section.inputDeficitSats,
    section.explicitFeeSats,
    section.inputAccountingTotalSats,
    section.outputAccountingTotalSats,
    section.implicitFeeSats,
    section.inputs.rows.map((row) => row.id),
    section.outputs.rows.map((row) => row.id),
  ]), [
    ["recognized", "alice", "Alice vault", 1000, 970, 30, 30, 0, 20, 1000, 990, 10, ["alice-in"], ["alice-out"]],
    ["recognized", "bob", "Bob request", 500, 480, 20, 20, 0, 5, 500, 485, 15, ["bob-in"], ["bob-out"]],
    ["unrecognized", undefined, "unrecognized", 700, 650, 50, 50, 0, 30, 700, 680, 20, ["unknown-in"], ["unknown-out"]],
  ]);
  assert.equal(display.whole.fee.total, 100);
  assert.equal(display.whole.fee.explicit, 55);
  assert.deepEqual(unorderedBalanceSheetTotalRows(display).map((row) => [
    row.kind,
    row.label,
    row.inputAccountingTotalSats,
    row.outputAccountingTotalSats,
    row.explicitFeeSats,
    row.implicitFeeSats,
  ]), [
    ["recognized", "Alice vault", 1000, 990, 20, 10],
    ["recognized", "Bob request", 500, 485, 5, 15],
    ["unrecognized", "unrecognized", 700, 680, 30, 20],
    ["whole", "total", 2200, 2155, 55, 45],
  ]);
  assert.equal(display.outputs.flatMap((section) => section.rows).some((row) => row.displayKind === "explicit-fee"), false);

  assert.deepEqual(unorderedPsbtDisplay({
    inputs: [{ id: "mine-in", valueSats: 10, descriptorMine: true }],
    outputs: [{ id: "mine-out", valueSats: 10, descriptorMine: true }],
    conflicts: [],
  }).outputs.map((section) => section.rows.map((row) => row.id)), [["mine-out"]]);

  const fallback = unorderedPsbtDisplay({
    inputs: [
      { id: "legacy-in", valueSats: 100, descriptorId: "legacy" },
      { id: "legacy-change", valueSats: 25, descriptorId: "legacy" },
      { id: "legacy-meta", descriptorId: "legacy" },
      { id: "blank-id", valueSats: 5, descriptorId: "" },
    ],
    outputs: [],
    conflicts: [],
  });
  assert.deepEqual(fallback.inputs.map((section) => [section.kind, section.descriptorId, section.label, section.rows.map((row) => row.id), section.totalSats]), [
    ["recognized", "legacy", "legacy", ["legacy-in", "legacy-change", "legacy-meta"], 125],
    ["unrecognized", undefined, "unrecognized", ["blank-id"], 5],
  ]);
  assert.deepEqual(fallback.subtransactions.map((section) => [
    section.label,
    section.inputTotalSats,
    section.outputTotalSats,
    section.outputFeeSats,
    section.inputDeficitSats,
    section.inputs.rows.map((row) => row.id),
    section.outputs.rows.map((row) => row.id),
  ]), [
    ["legacy", 125, 0, 125, 0, ["legacy-in", "legacy-change", "legacy-meta"], []],
    ["unrecognized", 5, 0, 5, 0, ["blank-id"], []],
  ]);

  const deficit = unorderedPsbtDisplay({
    inputs: [],
    outputs: [{ id: "payment", valueSats: 25, descriptorId: "recipient", descriptorLabel: "Recipient", descriptorColor: "#607d00", descriptorMine: true }],
    conflicts: [],
  }).subtransactions[0];
  assert.deepEqual([
    deficit.descriptorColor,
    deficit.descriptorMine,
    deficit.inputTotalSats,
    deficit.outputTotalSats,
    deficit.feeSats,
    deficit.outputFeeSats,
    deficit.inputDeficitSats,
  ], ["#607d00", true, 0, 25, -25, 0, 25]);

  const grouped = unorderedPsbtDisplay({
    inputs: [
      { id: "external", valueSats: 60, descriptorId: "wallet-external", descriptorLabel: "external", descriptorGroupId: "wallet", descriptorGroupLabel: "Wallet", descriptorGroupColor: "#1967d2", descriptorGroupMine: true },
      { id: "internal", valueSats: 40, descriptorId: "wallet-internal", descriptorLabel: "internal", descriptorGroupId: "wallet", descriptorGroupLabel: "Wallet", descriptorGroupColor: "#1967d2", descriptorGroupMine: true },
    ],
    outputs: [{ id: "change", valueSats: 95, descriptorId: "wallet-internal", descriptorGroupId: "wallet", descriptorGroupLabel: "Wallet" }],
    conflicts: [],
  });
  assert.deepEqual(grouped.subtransactions.map((section) => [
    section.descriptorId,
    section.label,
    section.inputTotalSats,
    section.outputTotalSats,
    section.feeSats,
    section.inputs.rows.map((row) => row.id),
  ]), [["wallet", "Wallet", 100, 95, 5, ["external", "internal"]]]);
});

test("balance sheet delta presentation places deficits and surplus on opposite columns", () => {
  assert.equal(typeof accountingDeltaPresentation, "function");
  assert.equal(typeof shouldShowGrandTotal, "function");

  assert.deepEqual(accountingDeltaPresentation({
    kind: "recognized",
    label: "Alice",
    inputTotalSats: 1000,
    outputTotalSats: 970,
    feeSats: 30,
    outputFeeSats: 30,
    inputDeficitSats: 0,
    explicitFeeSats: 20,
    inputAccountingTotalSats: 1000,
    outputAccountingTotalSats: 990,
    implicitFeeSats: 10,
  }), {
    kind: "surplus",
    column: "output",
    oppositeColumn: "input",
    showTotals: true,
    totalSats: 30,
    explicitFeeSats: 20,
    implicitFeeSats: 10,
    label: "accounted / surplus",
    separator: " / ",
    amountA: 20,
    amountB: 30,
  });

  assert.deepEqual(accountingDeltaPresentation({
    kind: "recognized",
    label: "Recipient",
    inputTotalSats: 0,
    outputTotalSats: 25,
    feeSats: -25,
    outputFeeSats: 0,
    inputDeficitSats: 25,
    explicitFeeSats: 5,
    inputAccountingTotalSats: 0,
    outputAccountingTotalSats: 30,
    implicitFeeSats: -30,
  }), {
    kind: "deficit",
    column: "input",
    oppositeColumn: "output",
    showTotals: false,
    totalSats: 25,
    explicitFeeSats: 5,
    implicitFeeSats: -30,
    label: "deficit + accounted",
    separator: " + ",
    amountA: 25,
    amountB: 5,
  });

  assert.deepEqual(accountingDeltaPresentation({
    kind: "recognized",
    label: "Unannounced surplus",
    inputTotalSats: 50,
    outputTotalSats: 40,
    feeSats: 10,
    outputFeeSats: 10,
    inputDeficitSats: 0,
    explicitFeeSats: 0,
    inputAccountingTotalSats: 50,
    outputAccountingTotalSats: 40,
    implicitFeeSats: 10,
  }).label, "surplus");

  assert.deepEqual(accountingDeltaPresentation({
    kind: "recognized",
    label: "Unannounced deficit",
    inputTotalSats: 40,
    outputTotalSats: 50,
    feeSats: -10,
    outputFeeSats: 0,
    inputDeficitSats: 10,
    explicitFeeSats: 0,
    inputAccountingTotalSats: 40,
    outputAccountingTotalSats: 50,
    implicitFeeSats: -10,
  }).label, "deficit");

  assert.deepEqual(accountingDeltaPresentation({
    kind: "recognized",
    label: "Balanced",
    inputTotalSats: 10,
    outputTotalSats: 10,
    feeSats: 0,
    outputFeeSats: 0,
    inputDeficitSats: 0,
    explicitFeeSats: 0,
    inputAccountingTotalSats: 10,
    outputAccountingTotalSats: 10,
    implicitFeeSats: 0,
  }), {
    kind: "balanced",
    column: null,
    oppositeColumn: null,
    showTotals: true,
    totalSats: 0,
    explicitFeeSats: 0,
    implicitFeeSats: 0,
    label: "",
    separator: null,
    amountA: 0,
    amountB: null,
  });
  assert.equal(accountingDeltaPresentation({
    kind: "recognized",
    label: "Missing output accounting",
    feeSats: 0,
    outputFeeSats: 0,
    inputDeficitSats: 0,
    explicitFeeSats: 0,
    inputAccountingTotalSats: 1,
    implicitFeeSats: 0,
  }).showTotals, false);

  assert.equal(shouldShowGrandTotal(unorderedPsbtDisplay({
    inputs: [{ id: "alice-in", valueSats: 100, descriptorId: "alice" }],
    outputs: [{ id: "alice-out", valueSats: 90, descriptorId: "alice" }],
    conflicts: [],
  })), false);
  assert.equal(shouldShowGrandTotal(unorderedPsbtDisplay({
    inputs: [{ id: "alice-in", valueSats: 100, descriptorId: "alice" }],
    outputs: [{ id: "bob-out", valueSats: 90, descriptorId: "bob" }],
    conflicts: [],
  })), true);
});

test("descriptor fee signal can finalize mine surplus into explicit fee", () => {
  assert.equal(typeof balanceSheetFeeSignal, "function");
  assert.equal(typeof model.descriptorFeeSignal, "function");
  assert.equal(typeof model.descriptorFeeContributionPlan, "function");
  assert.equal(typeof model.finalizeDescriptorExplicitFee, "function");
  const payload = {
    inputs: [
      { id: "alice-in", valueSats: 1000, descriptorId: "alice", descriptorLabel: "Alice", descriptorColor: "#1967d2", descriptorMine: true, explicitFeeSats: 20, estimatedVbytes: 80 },
      { id: "other-in", valueSats: 500, vbytes: 100 },
      { id: "fallback-in", valueSats: 100, estimatedVbytes: 0 },
    ],
    outputs: [
      { id: "alice-change", valueSats: 970, descriptorId: "alice", descriptorLabel: "Alice", descriptorColor: "#1967d2", descriptorMine: true, estimatedVbytes: 40 },
      { id: "other-out", valueSats: 490, vbytes: 30 },
      { id: "fallback-out", valueSats: 90, estimatedVbytes: -1 },
    ],
    descriptors: [{ id: "alice", privacy: "public", descriptor: "wpkh(alice/*)" }],
    conflicts: [],
  };

  assert.deepEqual(model.descriptorFeeSignal(payload, "alice"), {
    descriptorId: "alice",
    descriptorLabel: "Alice",
    explicitFeeSats: 20,
    implicitFeeSats: 10,
    totalFeeSats: 30,
    estimatedVbytes: 120,
    feeRateSatsPerVbyte: 0.25,
    averageFeeRateSatsPerVbyte: 50 / 349,
    canFinalizeExplicitFee: true,
  });

  const display = unorderedPsbtDisplay({
    inputs: [
      { id: "alice-in", valueSats: 1000, descriptorId: "alice", descriptorLabel: "Alice", descriptorMine: true, explicitFeeSats: 20, estimatedVbytes: 80 },
      { id: "bob-in", valueSats: 500, descriptorId: "bob", descriptorLabel: "Bob", descriptorMine: false, estimatedVbytes: 100 },
    ],
    outputs: [
      { id: "alice-out", valueSats: 970, descriptorId: "alice", descriptorLabel: "Alice", descriptorMine: true, estimatedVbytes: 40 },
      { id: "bob-out", valueSats: 480, descriptorId: "bob", descriptorLabel: "Bob", descriptorMine: false, vbytes: 30 },
    ],
    conflicts: [],
  });
  const averageFeeRate = 50 / 250;
  const bobSection = display.subtransactions.find((section) => section.descriptorId === "bob");
  const totalRow = unorderedBalanceSheetTotalRows(display).at(-1);
  assert.deepEqual(balanceSheetFeeSignal(bobSection, averageFeeRate), {
    descriptorId: "bob",
    descriptorLabel: "Bob",
    explicitFeeSats: 0,
    implicitFeeSats: 20,
    totalFeeSats: 20,
    estimatedVbytes: 130,
    feeRateSatsPerVbyte: 20 / 130,
    averageFeeRateSatsPerVbyte: averageFeeRate,
    canFinalizeExplicitFee: false,
  });
  assert.deepEqual(balanceSheetFeeSignal(totalRow, averageFeeRate), {
    descriptorId: undefined,
    descriptorLabel: "total",
    explicitFeeSats: 20,
    implicitFeeSats: 30,
    totalFeeSats: 50,
    estimatedVbytes: 250,
    feeRateSatsPerVbyte: averageFeeRate,
    averageFeeRateSatsPerVbyte: averageFeeRate,
    canFinalizeExplicitFee: false,
  });

  const finalized = model.finalizeDescriptorExplicitFee(payload, "alice");
  assert.notEqual(finalized, payload);
  assert.equal(payload.inputs[0].explicitFeeSats, 20);
  assert.equal(finalized.inputs[0].explicitFeeSats, 30);
  assert.equal(model.descriptorFeeSignal(finalized, "alice").implicitFeeSats, 0);
  assert.equal(model.descriptorFeeSignal(finalized, "alice").canFinalizeExplicitFee, false);

  const partial = model.finalizeDescriptorExplicitFee(payload, "alice", 5);
  assert.equal(partial.inputs[0].explicitFeeSats, 25);
  assert.equal(model.descriptorFeeSignal(partial, "alice").implicitFeeSats, 5);
  const clamped = model.finalizeDescriptorExplicitFee(payload, "alice", 50);
  assert.equal(clamped.inputs[0].explicitFeeSats, 30);
  const unchanged = model.finalizeDescriptorExplicitFee(payload, "alice", -1);
  assert.equal(unchanged.inputs[0].explicitFeeSats, 20);

  assert.deepEqual(model.finalizeDescriptorExplicitFee({
    inputs: [{ id: "bare-in", valueSats: 100, descriptorId: "bare", descriptorMine: true }],
    outputs: [{ id: "bare-out", valueSats: 90, descriptorId: "bare", descriptorMine: true }],
  }, "bare"), {
    inputs: [{ id: "bare-in", valueSats: 100, descriptorId: "bare", descriptorMine: true, explicitFeeSats: 10 }],
    outputs: [{ id: "bare-out", valueSats: 90, descriptorId: "bare", descriptorMine: true }],
    descriptors: [],
    conflicts: [],
  });

  assert.equal(model.descriptorFeeSignal(payload, "missing"), null);
  assert.deepEqual(model.finalizeDescriptorExplicitFee(payload, "missing"), payload);
  assert.deepEqual(model.finalizeDescriptorExplicitFee({ inputs: [], outputs: [] }, "missing"), {
    inputs: [],
    outputs: [],
    descriptors: [],
    conflicts: [],
  });
  const otherPayload = {
    inputs: [{ id: "bob-in", valueSats: 100, descriptorId: "bob", descriptorMine: false }],
    outputs: [{ id: "bob-out", valueSats: 90, descriptorId: "bob", descriptorMine: false }],
    descriptors: [],
    conflicts: [],
  };
  assert.equal(model.descriptorFeeSignal(otherPayload, "bob").canFinalizeExplicitFee, false);
  assert.deepEqual(model.finalizeDescriptorExplicitFee(otherPayload, "bob"), otherPayload);

  const deficitPayload = {
    inputs: [],
    outputs: [{ id: "alice-payment", valueSats: 25, descriptorId: "alice", descriptorMine: true }],
    descriptors: [],
    conflicts: [],
  };
  const deficit = model.descriptorFeeSignal(deficitPayload, "alice");
  assert.equal(deficit.canFinalizeExplicitFee, false);
  assert.equal(deficit.implicitFeeSats, -25);
  assert.deepEqual(model.finalizeDescriptorExplicitFee(deficitPayload, "alice"), deficitPayload);

  const baseSignal = {
    descriptorId: "alice",
    descriptorLabel: "Alice",
    explicitFeeSats: 20,
    implicitFeeSats: 200000,
    totalFeeSats: 200020,
    estimatedVbytes: 100,
    feeRateSatsPerVbyte: 2000.2,
    averageFeeRateSatsPerVbyte: 9,
    canFinalizeExplicitFee: true,
  };
  assert.deepEqual(model.descriptorFeeContributionPlan(baseSignal, -7), {
    descriptorId: "alice",
    descriptorLabel: "Alice",
    availableSats: 200000,
    selectedSats: 0,
    finalExplicitFeeSats: 20,
    estimatedVbytes: 100,
    feeRateSatsPerVbyte: 0,
    averageFeeRateSatsPerVbyte: 9,
    relativeFeeRateRatio: 0,
    absoluteWarningLevel: "none",
    relativeWarningLevel: "none",
    warningLevel: "none",
    confirmationRequired: false,
  });
  const absoluteSignal = { ...baseSignal, averageFeeRateSatsPerVbyte: 10000 };
  assert.equal(model.descriptorFeeContributionPlan(absoluteSignal, 1001).warningLevel, "yellow");
  assert.equal(model.descriptorFeeContributionPlan(absoluteSignal, 10001).warningLevel, "red");
  assert.equal(model.descriptorFeeContributionPlan(absoluteSignal, 100001).warningLevel, "confirm");
  assert.equal(model.descriptorFeeContributionPlan({ ...baseSignal, averageFeeRateSatsPerVbyte: 9 }, 1000).relativeWarningLevel, "yellow");
  assert.equal(model.descriptorFeeContributionPlan({ ...baseSignal, averageFeeRateSatsPerVbyte: 30 }, 6100).relativeWarningLevel, "red");
  assert.equal(model.descriptorFeeContributionPlan({ ...baseSignal, averageFeeRateSatsPerVbyte: 20 }, 21000).relativeWarningLevel, "confirm");
  assert.equal(model.descriptorFeeContributionPlan({ ...baseSignal, implicitFeeSats: 5 }, 10).selectedSats, 5);
  assert.equal(model.descriptorFeeContributionPlan({ ...baseSignal, averageFeeRateSatsPerVbyte: 0 }, 1).relativeFeeRateRatio, Number.POSITIVE_INFINITY);
  assert.equal(model.descriptorFeeContributionPlan({ ...baseSignal, averageFeeRateSatsPerVbyte: 0 }, 0).relativeFeeRateRatio, 0);
  assert.equal(model.descriptorFeeContributionPlan(baseSignal, Number.NaN).selectedSats, 0);
  assert.equal(model.descriptorFeeContributionPlan(null, 1), null);
});

test("pendingPayloadRowKeys identifies fragment rows that should stay dashed while syncing", () => {
  assert.deepEqual(pendingPayloadRowKeys({
    inputs: [
      { id: "mine-in", valueSats: 1000, descriptorMine: true, explicitFeeSats: 20 },
      { id: "other-in", valueSats: 700, descriptorMine: false },
    ],
    outputs: [{ id: "recipient", valueSats: 1200 }],
    conflicts: [],
  }), [
    "input:mine-in",
    "input:other-in",
    "output:recipient",
  ]);

  assert.deepEqual(pendingPayloadRowKeys({
    inputs: [{ id: "input-without-fee", valueSats: 10 }],
    outputs: [],
    conflicts: [],
  }), ["input:input-without-fee"]);
});

test("peerBridgeComponents groups bridged peers in original peer order", () => {
  const peers = [{ id: "alice" }, { id: "bob" }, { id: "carol" }, { id: "dana" }];
  const edges = [
    { from: "bob", to: "carol", kind: "peer-bridge" },
    { from: "carol", to: "missing", kind: "peer-bridge" },
    { from: "dana", to: "alice", kind: "other" },
  ];
  assert.deepEqual(peerBridgeComponents(peers, edges), [["alice"], ["bob", "carol"], ["dana"]]);
});

test("local display peer is not selectable or wireable", () => {
  assert.equal(peerIsInteractive({ id: "me", local: true }), false);
  assert.equal(peerIsInteractive({ id: "alice", local: false }), true);
  assert.equal(peerIsInteractive({ id: "bob" }), true);
});

test("peer group bounds and edge termination use shared bridge ports", () => {
  const positions = new Map([
    ["alice", { x: 10, y: 5, width: 100, height: 20 }],
    ["bob", { x: 116, y: 5, width: 100, height: 20 }],
    ["carol", { x: 300, y: 5, width: 100, height: 20 }],
  ]);
  assert.deepEqual(peerGroupBounds(["alice", "bob"], positions), { x: 10, y: 5, width: 206, height: 20 });
  assert.deepEqual(peerEdgeTermination("alice", [["alice", "bob"], ["carol"]], positions), { x: 112, y: 5, width: 2, height: 28 });
  assert.deepEqual(peerEdgeTermination("carol", [["alice", "bob"], ["carol"]], positions), { x: 300, y: 5, width: 100, height: 20 });
  assert.equal(peerGroupBounds(["missing"], positions), null);
  assert.equal(peerEdgeTermination("missing", [["alice", "bob"]], positions), null);
  assert.equal(peerEdgeTermination("missing", [["missing", "also-missing"]], positions), null);
});

test("session visibility detects direct and bridged peer readers", () => {
  const session = { id: "session-1", peers: ["alice"] };
  const peers = [
    { id: "alice" },
    { id: "bob" },
    { id: "carol", views: { "session-1": {} } },
    { id: "dana" },
  ];

  assert.equal(sessionVisibleToPeerGroup(session, peers, ["alice"]), true);
  assert.equal(sessionVisibleToPeerGroup(session, peers, ["bob", "alice"]), true);
  assert.equal(sessionVisibleToPeerGroup(session, peers, ["carol"]), true);
  assert.equal(sessionVisibleToPeerGroup(session, peers, ["dana"]), false);
  assert.equal(sessionVisibleToPeerGroup(session, peers, ["missing"]), false);
  assert.equal(sessionVisibleToPeerGroup({ id: "session-2" }, peers, ["alice"]), false);
});

test("hashHex and joinSessionSeeds are deterministic", () => {
  assert.equal(hashHex("seed"), hashHex("seed"));
  assert.equal(hashHex(null), hashHex(""));
  assert.equal(joinSessionSeeds([]), "00000000");
  assert.equal(joinSessionSeeds([{ seed: "a" }, { seed: "b" }]), joinSessionSeeds([{ seed: "a" }, { seed: "b" }]));
  assert.match(joinSessionSeeds([{ seed: "0001" }, { seed: "0002" }]), /^[0-9a-f]{8}$/);
});
