import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  amountBits,
  amountSpanParts,
  balanceSheet,
  cardGroups,
  declaredFeeSatsFromInspect,
  decodeScript,
  elisionLabel,
  feeLine,
  formatFeeRate,
  fragmentBadges,
  fragmentCardModel,
  groupAggregate,
  inputViews,
  outputViews,
  prevoutScriptHex,
  rawKeymapSections,
  rowDetailPairs,
  rowFacePairs,
  scriptCycle,
  scriptTemplate,
  sequenceReading,
  signaturePresence,
  signedAmountSpanParts,
  sizeEstimateVbytesFromInspect,
} from "../dist/session/display.js";
import { formatSatAmount } from "../dist/model.js";
import { fragmentSummary } from "../dist/session/state.js";
import {
  addressChipDigestHex,
  groupChipDigestHex,
  lifehashSrc,
} from "../dist/session/display.js";

// BIP-350 reference script: P2WPKH for the shared test vector address.
const P2WPKH = "0014751e76e8199196d454941c45d1b3a323f1433bd6";
const P2TR = "512079be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

const INSPECT = {
  format: "bip370",
  ordering: "unordered",
  input_count: 2,
  output_count: 3,
  modifiability: { flags: 3, inputs: true, outputs: true },
  sort: { mode: "unset", seed_hex: null },
  unordered_unique_id_hex: "ee".repeat(32),
  inputs: [
    {
      outpoint: `${"aa".repeat(32)}:0`,
      sequence: "0xfffffffe",
      known_utxo_sats: 150000,
      has_witness_utxo: true,
      has_non_witness_utxo: false,
    },
    {
      outpoint: `${"bb".repeat(32)}:7`,
      sequence: null,
      known_utxo_sats: null,
      has_witness_utxo: false,
      has_non_witness_utxo: false,
    },
  ],
  outputs: [
    { amount_sats: 60000, script_pubkey_hex: P2WPKH, unique_id_hex: "11".repeat(32) },
    { amount_sats: 40000, script_pubkey_hex: P2TR, unique_id_hex: null },
    { amount_sats: 25000, script_pubkey_hex: "6a0b68656c6c6f20776f726c64", unique_id_hex: "22".repeat(32) },
  ],
  totals: { known_input_sats: null, output_sats: 125000, fee_sats_if_inputs_known: null },
};

test("scriptTemplate classifies the standard templates", () => {
  assert.equal(scriptTemplate(P2WPKH).kind, "p2wpkh");
  assert.equal(scriptTemplate(P2TR).kind, "p2tr");
  assert.equal(scriptTemplate("76a914" + "00".repeat(20) + "88ac").kind, "p2pkh");
  assert.equal(scriptTemplate("a914" + "00".repeat(20) + "87").kind, "p2sh");
  assert.equal(scriptTemplate("0020" + "00".repeat(32)).kind, "p2wsh");
  // Future witness version (v2, 20-byte program).
  assert.equal(scriptTemplate("5214" + "00".repeat(20)).kind, "witness");
  assert.equal(scriptTemplate("6a0b68656c6c6f20776f726c64").kind, "unknown"); // OP_RETURN
  assert.equal(scriptTemplate(null).kind, "absent");
  assert.equal(scriptTemplate("zz").kind, "unknown");
  assert.match(scriptTemplate(P2WPKH).label, /P2WPKH/);
});

// --- LifeHash chips (addresses/scripts fingerprint, never card text) --------

test("lifehashSrc: the chip src is the lifehash route over the digest hex", () => {
  assert.equal(lifehashSrc(P2WPKH), `/api/lifehash/${P2WPKH}`);
  assert.equal(lifehashSrc("ee".repeat(32)), `/api/lifehash/${"ee".repeat(32)}`);
});

test("addressChipDigestHex: an address chips as its script_pubkey hex", () => {
  // BIP-350 shared test vector: this address IS the P2WPKH script above, so
  // pasting the address and decoding the script fingerprint identically.
  assert.equal(addressChipDigestHex("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"), P2WPKH);
  // Non-script payment strings (a lightning invoice riding the address
  // slot) yield no digest: the caller keeps the textual rendering.
  assert.equal(addressChipDigestHex("lnbc10n1pexample"), null);
  assert.equal(addressChipDigestHex(""), null);
  assert.equal(addressChipDigestHex(null), null);
});

test("groupChipDigestHex: one shared script fingerprints the group", () => {
  assert.equal(groupChipDigestHex({ outputs: [{ scriptHex: P2WPKH }, { scriptHex: P2WPKH }] }), P2WPKH);
  // Mixed or unknown scripts: no chip (one fingerprint would misattribute).
  assert.equal(groupChipDigestHex({ outputs: [{ scriptHex: P2WPKH }, { scriptHex: P2TR }] }), null);
  assert.equal(groupChipDigestHex({ outputs: [{ scriptHex: P2WPKH }, { scriptHex: null }] }), null);
  // Input-only groups carry no output script identity.
  assert.equal(groupChipDigestHex({ outputs: [] }), null);
});

test("inputViews and outputViews project inspect JSON defensively", () => {
  const inputs = inputViews(INSPECT);
  assert.equal(inputs.length, 2);
  assert.equal(inputs[0].outpointTxid, "aa".repeat(32));
  assert.equal(inputs[0].outpointVout, 0);
  assert.equal(inputs[0].knownUtxoSats, 150000);
  assert.equal(inputs[0].hasWitnessUtxo, true);
  assert.equal(inputs[1].outpointVout, 7);
  assert.equal(inputs[1].knownUtxoSats, null);
  assert.equal(inputs[1].sequence, null);

  const outputs = outputViews(INSPECT, "regtest");
  assert.equal(outputs.length, 3);
  // Address rendered FROM the scriptPubKey for the session's network.
  assert.equal(outputs[0].address, "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080");
  assert.equal(outputs[0].uniqueIdHex, "11".repeat(32));
  assert.equal(outputs[1].scriptKind, "p2tr");
  assert.equal(outputs[1].uniqueIdHex, null);
  assert.equal(outputs[2].address, null); // OP_RETURN has no address form
  assert.equal(outputs[2].scriptKind, "unknown");

  assert.deepEqual(inputViews(null), []);
  assert.deepEqual(outputViews({ outputs: "mangled" }, "bitcoin"), []);
});

test("cardGroups: the default dimension leaves script templates ungrouped", () => {
  const inputs = inputViews(INSPECT);
  const outputs = outputViews(INSPECT, "regtest");
  // Script kind is not attribution: without provenance every row lands in
  // the single implicit unattributed group.
  const groups = cardGroups(inputs, outputs);
  assert.deepEqual(
    groups.map((group) => group.key),
    ["unattributed"],
  );
  assert.equal(groups[0].inputs.length, 2);
  assert.equal(groups[0].outputs.length, 3);
  assert.equal(groups[0].inputSubtotalSats, null); // one input amount unknown
  assert.equal(groups[0].outputSubtotalSats, 125000);
});

test("cardGroups: script templates group outputs under the extended dimension", () => {
  const inputs = inputViews(INSPECT);
  const outputs = outputViews(INSPECT, "regtest");
  const groups = cardGroups(inputs, outputs, "provenance+script-template");

  assert.deepEqual(
    groups.map((group) => group.key),
    ["template:p2wpkh", "template:p2tr", "unattributed"],
  );
  const [wpkh, tr, rest] = groups;
  assert.equal(wpkh.kind, "script-template");
  assert.equal(wpkh.outputSubtotalSats, 60000);
  assert.equal(wpkh.inputSubtotalSats, null); // no inputs in the group
  assert.equal(tr.outputSubtotalSats, 40000);
  // The unattributed group holds both inputs (inspect exposes no input
  // script/provenance data — subtotal null because one amount is unknown)
  // plus the nonstandard output.
  assert.equal(rest.inputs.length, 2);
  assert.equal(rest.inputSubtotalSats, null);
  assert.deepEqual(rest.outputs.map((output) => output.index), [2]);
  assert.equal(rest.outputSubtotalSats, 25000);
});

test("cardGroups: provenance metadata takes precedence when supplied", () => {
  const provenance = {
    inputs: { [`${"aa".repeat(32)}:0`]: "peer-alice" },
    outputs: { ["11".repeat(32)]: "peer-alice" },
  };
  const inputs = inputViews(INSPECT, provenance);
  const outputs = outputViews(INSPECT, "regtest", provenance);
  const groups = cardGroups(inputs, outputs, "provenance+script-template");

  assert.equal(groups[0].key, "peer:peer-alice");
  assert.equal(groups[0].kind, "provenance");
  assert.equal(groups[0].label, "from peer-alice");
  assert.equal(groups[0].inputs.length, 1);
  assert.equal(groups[0].inputSubtotalSats, 150000);
  assert.deepEqual(groups[0].outputs.map((output) => output.index), [0]);
  assert.equal(groups[0].outputSubtotalSats, 60000);
  // Provenance groups come first, then templates, then unattributed.
  assert.deepEqual(
    groups.map((group) => group.kind),
    ["provenance", "script-template", "unattributed"],
  );
});

test("feeLine mirrors the demo's input/output/fee accounting", () => {
  const complete = feeLine(
    fragmentSummary({
      totals: { known_input_sats: 200000, output_sats: 150000, fee_sats_if_inputs_known: 50000 },
    }),
  );
  assert.equal(complete.feeSats, 50000);
  assert.match(complete.text, /in − /);
  assert.match(complete.text, /fee$/);

  const deficit = feeLine(
    fragmentSummary({
      totals: { known_input_sats: 100000, output_sats: 150000, fee_sats_if_inputs_known: -50000 },
    }),
  );
  assert.match(deficit.text, /deficit/);
  // Negative fees render sign-aware (the demo formatter only handles >= 0).
  assert.match(deficit.text, /−₿0\.00050000 fee/);
  assert.doesNotMatch(deficit.text, /0\.0-/);

  const partial = feeLine(fragmentSummary({ totals: { output_sats: 125000 } }));
  assert.equal(partial.feeSats, null);
  assert.match(partial.text, /fee unknown \(input amounts incomplete\)/);

  const unknown = feeLine(fragmentSummary(null));
  assert.match(unknown.text, /not decoded/);
});

test("fragmentCardModel assembles summary, groups, uid indicator, fee", () => {
  const card = fragmentCardModel(INSPECT, "regtest");
  assert.equal(card.summary.ordering, "unordered");
  assert.equal(card.inputs.length, 2);
  assert.equal(card.outputs.length, 3);
  assert.equal(card.groups.length, 1); // default dimension: one implicit unattributed group
  assert.equal(card.uidPresent, 2);
  assert.equal(card.uidTotal, 3);
  assert.match(card.fee.text, /fee unknown/);

  const empty = fragmentCardModel(null, "bitcoin");
  assert.deepEqual(empty.inputs, []);
  assert.deepEqual(empty.groups, []);
  assert.equal(empty.uidPresent, null);
  assert.equal(empty.uidTotal, null);
});

// --- status badges (Q10: emoji + text pills) --------------------------------

test("fragmentBadges: the demo emoji ride the matching pills", () => {
  const card = fragmentCardModel(INSPECT, "regtest");
  const badges = fragmentBadges(card);
  // The format pill wears its BIP number (inspect's internal "bip370" stays
  // seam vocabulary).
  assert.deepEqual(
    badges.map((badgeView) => `${badgeView.emoji ?? "-"} ${badgeView.text}`),
    ["- BIP 370", "🔀 unordered", "✏️ modifiable both", "- ids 2/3"],
  );
  // Pills whose text IS the content carry no emoji (they never collapse).
  assert.equal(badges[0].emoji, null);
  assert.equal(badges[3].emoji, null);
  assert.equal(badges[3].tone, "warn"); // one output id missing
  assert.equal(badges[1].tone, "good");
  for (const badgeView of badges) assert.ok(badgeView.title.length > 0);
});

test("fragmentBadges: seed, partial modifiability, complete ids", () => {
  const inspect = {
    ...INSPECT,
    sort: { mode: "det", seed_hex: "aa".repeat(16) },
    modifiability: { flags: 1, inputs: true, outputs: false },
    outputs: INSPECT.outputs.map((output) => ({ ...output, unique_id_hex: "33".repeat(32) })),
  };
  const badges = fragmentBadges(fragmentCardModel(inspect, "regtest"));
  const seeded = badges.find((badgeView) => badgeView.emoji === "🌱");
  assert.ok(seeded, "seeded pill present");
  assert.equal(seeded.text, "seeded");
  assert.match(seeded.title, /sort seed/);
  const modifiable = badges.find((badgeView) => badgeView.emoji === "✏️");
  assert.equal(modifiable.text, "modifiable inputs");
  const ids = badges.find((badgeView) => badgeView.text.startsWith("ids"));
  assert.equal(ids.tone, "good");
});

test("fragmentBadges: undecoded fragments degrade honestly", () => {
  const badges = fragmentBadges(fragmentCardModel(null, "regtest"));
  assert.deepEqual(
    badges.map((badgeView) => badgeView.text),
    ["not decoded", "ordering unknown"],
  );
  assert.deepEqual(
    badges.map((badgeView) => badgeView.emoji),
    [null, null],
  );
});

test("elisionLabel counts what the card hides", () => {
  assert.equal(elisionLabel(3, 10), "+7 more");
  assert.equal(elisionLabel(3, 3), null);
  assert.equal(elisionLabel(5, 2), null);
});

// --- expanded row detail (rowDetailPairs) -----------------------------------

test("rowDetailPairs: every decoded field plus the raw keymap entries", () => {
  // Extend the shared fixture with a raw projection so the raw entries (the
  // kind=known ones INCLUDED — this view is the complete one) surface.
  const withRaw = {
    ...INSPECT,
    raw: {
      global: [{ key_hex: "fb", value_hex: "02000000", kind: "known" }],
      inputs: [
        [
          { key_hex: "0e", value_hex: "aa".repeat(32), kind: "known" },
          { key_hex: "ef01", value_hex: "beef", kind: "unknown" },
        ],
        [],
      ],
      outputs: [
        [{ key_hex: "fc0f636f6e63757272656e742d70736274aa", value_hex: "11".repeat(32), kind: "proprietary" }],
        [],
        [],
      ],
    },
  };

  // Output 0: the textual address FIRST (the LifeHash chip's counterpart),
  // then every decoded field by its inspect key, then the raw entries.
  const output = rowDetailPairs(withRaw, "output", 0, "bitcoin");
  assert.equal(output[0].label, "address");
  assert.equal(output[0].value, "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4");
  const outputLabels = output.map((pair) => pair.label);
  assert.ok(outputLabels.includes("amount_sats"));
  assert.ok(outputLabels.includes("script_pubkey_hex"));
  assert.ok(outputLabels.includes("unique_id_hex"));
  assert.deepEqual(output[output.length - 1], {
    label: "raw proprietary fc0f636f6e63757272656e742d70736274aa",
    value: "11".repeat(32),
  });

  // Input 0: no address (inspect carries no per-input script data), every
  // decoded field including the booleans the card row elides, raw entries
  // with kind=known included.
  const input = rowDetailPairs(withRaw, "input", 0, "bitcoin");
  const inputLabels = input.map((pair) => pair.label);
  assert.ok(inputLabels.includes("outpoint"));
  assert.ok(inputLabels.includes("sequence"));
  assert.ok(inputLabels.includes("has_witness_utxo"));
  assert.ok(inputLabels.includes("raw known 0e"));
  assert.ok(inputLabels.includes("raw unknown ef01"));
  assert.equal(input.find((pair) => pair.label === "has_witness_utxo").value, "true");

  // Null decoded values render as an honest dash, not "null" noise.
  const second = rowDetailPairs(withRaw, "input", 1, "bitcoin");
  assert.equal(second.find((pair) => pair.label === "sequence").value, "\u2014");

  // Defensive: no inspect, out-of-range index, missing raw section.
  assert.deepEqual(rowDetailPairs(null, "input", 0, "bitcoin"), []);
  assert.deepEqual(rowDetailPairs(withRaw, "output", 9, "bitcoin"), []);
  const bare = rowDetailPairs(INSPECT, "output", 1, "bitcoin");
  assert.ok(bare.length > 0);
  assert.ok(bare.every((pair) => !pair.label.startsWith("raw ")));
});

// --- prevout scriptPubKey (the input chip's identity) ------------------------

test("prevoutScriptHex decodes the witness utxo's TxOut", () => {
  // Serialized TxOut: 8-byte LE amount (150000 sats), compact-size script
  // length (0x16 = 22), then the P2WPKH script.
  const witnessUtxo = "f04902000000000016" + P2WPKH;
  const withRaw = {
    ...INSPECT,
    raw: {
      inputs: [
        [{ key_hex: "01", value_hex: witnessUtxo, kind: "known" }],
        [{ key_hex: "0e", value_hex: "aa".repeat(32), kind: "known" }], // no witness utxo
      ],
      outputs: [[], [], []],
    },
  };
  assert.equal(prevoutScriptHex(withRaw, 0), P2WPKH);
  // inputViews carries it onto the row view.
  assert.equal(inputViews(withRaw)[0].prevoutScriptHex, P2WPKH);
  // No witness utxo (a non-witness utxo would need the whole previous
  // transaction parsed — backend concern): null.
  assert.equal(prevoutScriptHex(withRaw, 1), null);
  assert.equal(inputViews(withRaw)[1].prevoutScriptHex, null);
  assert.equal(prevoutScriptHex(INSPECT, 0), null);
  assert.equal(prevoutScriptHex(null, 0), null);
});

test("prevoutScriptHex handles compact-size markers and rejects malformed TxOuts", () => {
  const scriptOf = (length) => "51" + "00".repeat(length - 1); // parses as a script blob
  const fdUtxo = (length) =>
    "0000000000000000" + "fd" + length.toString(16).padStart(4, "0").match(/../g).reverse().join("") + scriptOf(length);
  const entry = (value) => ({
    raw: { inputs: [[{ key_hex: "01", value_hex: value, kind: "known" }]] },
  });
  // 0xfd two-byte little-endian length.
  assert.equal(prevoutScriptHex(entry(fdUtxo(300)), 0), scriptOf(300));
  // Truncated script (declared 22 bytes, provides 2): malformed, null.
  assert.equal(prevoutScriptHex(entry("f049020000000000160014"), 0), null);
  // Trailing garbage after the script: malformed, null.
  assert.equal(prevoutScriptHex(entry("f04902000000000016" + P2WPKH + "ff"), 0), null);
  // Empty script: no identity to fingerprint.
  assert.equal(prevoutScriptHex(entry("f04902000000000000"), 0), null);
  // Non-hex value: null.
  assert.equal(prevoutScriptHex(entry("banana"), 0), null);
});

// --- the raw view (rawKeymapSections) ----------------------------------------

test("rawKeymapSections: the three map kinds, faithful to serialization order", () => {
  const withRaw = {
    ...INSPECT,
    raw: {
      // Deliberately NOT sorted by keytype: the raw view must preserve the
      // order the bytes actually appear in, not impose an interpretation.
      global: [
        { key_hex: "06", value_hex: "03", kind: "known" },
        { key_hex: "fb", value_hex: "02000000", kind: "known" },
        { key_hex: "04", value_hex: "01", kind: "known" },
        {
          key_hex: "fc0f636f6e63757272656e742d7073627410",
          value_hex: "0100",
          kind: "proprietary",
          proprietary: { prefix_hex: "636f6e63757272656e742d70736274", prefix_utf8: "concurrent-psbt", subtype: 16, key_data_hex: "" },
        },
      ],
      inputs: [[
        { key_hex: "0e", value_hex: "aa".repeat(32), kind: "known" },
        { key_hex: "ef01", value_hex: "beef", kind: "unknown" },
        // A kind=unknown entry whose first byte collides with a defined
        // keytype (0x01 WITNESS_UTXO, unexpected keydata): the annotation
        // follows the backend's classification, never contradicts it.
        { key_hex: "01aa", value_hex: "cafe", kind: "unknown" },
      ]],
      outputs: [
        [{ key_hex: "03", value_hex: "1027000000000000", kind: "known" }],
        [],
      ],
    },
  };

  const sections = rawKeymapSections(withRaw);
  assert.deepEqual(
    sections.map((section) => section.title),
    ["global map", "input map 0", "output map 0", "output map 1"],
  );

  // Serialization order preserved verbatim; keytype names annotate the hex.
  const [global, input0, output0, output1] = sections;
  assert.deepEqual(
    global.entries.map((entry) => entry.keyHex),
    ["06", "fb", "04", "fc0f636f6e63757272656e742d7073627410"],
  );
  assert.deepEqual(
    global.entries.map((entry) => entry.name),
    [
      "PSBT_GLOBAL_TX_MODIFIABLE",
      "PSBT_GLOBAL_VERSION",
      "PSBT_GLOBAL_INPUT_COUNT",
      "concurrent-psbt#16",
    ],
  );

  // Per-map-kind name tables: keytype 03 means AMOUNT in an output map…
  assert.deepEqual(output0.entries, [
    { keyHex: "03", valueHex: "1027000000000000", kind: "known", name: "PSBT_OUT_AMOUNT" },
  ]);
  // …and unknown entries keep their hex with no invented name, even when
  // the first byte collides with a defined keytype.
  assert.deepEqual(
    input0.entries.map((entry) => [entry.name, entry.kind]),
    [["PSBT_IN_PREVIOUS_TXID", "known"], [null, "unknown"], [null, "unknown"]],
  );

  // Empty maps stay in the section list — the PSBT genuinely contains them.
  assert.deepEqual(output1.entries, []);

  // Defensive: no inspect / no raw projection yields no sections.
  assert.deepEqual(rawKeymapSections(null), []);
  assert.deepEqual(rawKeymapSections(INSPECT), []);
});

// --- detail ladder (signaturePresence, groupAggregate, rowFacePairs) --------

test("signaturePresence reads the raw keymap keytypes", () => {
  const withSigs = {
    ...INSPECT,
    raw: {
      inputs: [
        [
          { key_hex: "0e", value_hex: "aa".repeat(32), kind: "known" },
          { key_hex: "02" + "03".repeat(33), value_hex: "30".repeat(70), kind: "known" },
        ],
        [{ key_hex: "08", value_hex: "beef", kind: "known" }],
      ],
      outputs: [[], [], []],
    },
  };
  assert.equal(signaturePresence(withSigs, 0), "partial"); // PARTIAL_SIG (0x02)
  assert.equal(signaturePresence(withSigs, 1), "final"); // FINAL_SCRIPTWITNESS (0x08)
  // Taproot key-path signature counts as a (non-final) signature.
  const taproot = { raw: { inputs: [[{ key_hex: "13", value_hex: "40".repeat(64), kind: "known" }]] } };
  assert.equal(signaturePresence(taproot, 0), "partial");
  // No signature keytypes, missing raw section, out of range: unsigned.
  assert.equal(signaturePresence(withSigs, 9), "unsigned");
  assert.equal(signaturePresence(INSPECT, 0), "unsigned");
  assert.equal(signaturePresence(null, 0), "unsigned");
  // inputViews carries the presence onto every row.
  assert.deepEqual(
    inputViews(withSigs).map((input) => input.signatures),
    ["partial", "final"],
  );
});

test("groupAggregate summarizes a group into one line of facts", () => {
  const inputs = inputViews(INSPECT);
  const outputs = outputViews(INSPECT, "regtest");
  const [group] = cardGroups(inputs, outputs);
  assert.deepEqual(groupAggregate(group), {
    inputCount: 2,
    outputCount: 3,
    inputSubtotalSats: null, // one input amount unknown — a partial sum would lie
    outputSubtotalSats: 125000,
    signedInputCount: 0,
  });

  const signed = { ...inputs[0], signatures: "partial" };
  const [signedGroup] = cardGroups([signed, inputs[1]], []);
  assert.equal(groupAggregate(signedGroup).signedInputCount, 1);
  assert.equal(groupAggregate(signedGroup).outputCount, 0);
  assert.equal(groupAggregate(signedGroup).outputSubtotalSats, null);
});

test("rowFacePairs is the curated level-3 subset, not the raw dump", () => {
  const input = rowFacePairs(INSPECT, "input", 0, "regtest");
  assert.deepEqual(
    input.map((pair) => pair.label),
    ["outpoint", "sequence", "utxo data", "signatures"],
  );
  assert.equal(input[0].value, `${"aa".repeat(32)}:0`);
  // Fingerprintable facts carry a chip digest so the LifeHash renders next
  // to the hex it identifies: the outpoint fact chips its txid.
  assert.equal(input[0].chipHex, "aa".repeat(32));
  // The sequence fact keeps the hex and appends its BIP 68 reading.
  assert.equal(input[1].value, "0xfffffffe — no relative locktime (BIP 68 disable bit)");
  assert.equal(input[2].value, "witness utxo");
  assert.equal(input[3].value, "unsigned");

  // With a raw witness utxo the input facts carry the prevout's identity —
  // address and type, the output-fact vocabulary; the amount stays on the
  // row face.
  const withPrevout = {
    ...INSPECT,
    raw: {
      inputs: [[{ key_hex: "01", value_hex: "f04902000000000016" + P2WPKH, kind: "known" }], []],
      outputs: [[], [], []],
    },
  };
  const prevout = rowFacePairs(withPrevout, "input", 0, "regtest");
  assert.deepEqual(
    prevout.map((pair) => pair.label),
    ["outpoint", "prevout address", "prevout type", "sequence", "utxo data", "signatures"],
  );
  assert.equal(prevout[1].value, "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080");
  // The prevout-address fact carries the script chip (the row face hands
  // the identity over when expanded) and cycles through the script's
  // representations, prefix threaded into every label.
  assert.equal(prevout[1].chipHex, P2WPKH);
  assert.deepEqual(
    prevout[1].cycle.map((entry) => entry.label),
    ["prevout address", "prevout script hex", "prevout script asm"],
  );
  assert.match(prevout[2].value, /P2WPKH/);

  // Sparse input: no sequence pair, honest "none" for utxo data.
  const sparse = rowFacePairs(INSPECT, "input", 1, "regtest");
  assert.deepEqual(
    sparse.map((pair) => pair.label),
    ["outpoint", "utxo data", "signatures"],
  );
  assert.equal(sparse.find((pair) => pair.label === "utxo data").value, "none");

  const output = rowFacePairs(INSPECT, "output", 0, "regtest");
  assert.deepEqual(
    output.map((pair) => pair.label),
    ["address", "type", "unique id"],
  );
  assert.equal(output[0].value, "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080");
  // The address fact carries the script chip and cycles address | script
  // hex | decoded opcodes.
  assert.equal(output[0].chipHex, P2WPKH);
  assert.deepEqual(
    output[0].cycle.map((entry) => entry.label),
    ["address", "script hex", "script asm"],
  );
  // The unique id is its own fingerprint: chip and hex read as one fact.
  assert.equal(output.find((pair) => pair.label === "unique id").chipHex, "11".repeat(32));

  // OP_RETURN output: no address form, so the script fact STARTS at the
  // hex representation; script label still present, uid present.
  const nonstandard = rowFacePairs(INSPECT, "output", 2, "regtest");
  assert.deepEqual(
    nonstandard.map((pair) => pair.label),
    ["script hex", "type", "unique id"],
  );
  assert.deepEqual(
    nonstandard[0].cycle.map((entry) => entry.label),
    ["script hex", "script asm"],
  );

  // Never the raw keymap: face pairs stay curated even when raw data exists.
  assert.ok(rowFacePairs(INSPECT, "output", 0, "regtest").every((pair) => !pair.label.startsWith("raw ")));
  assert.deepEqual(rowFacePairs(null, "input", 0, "regtest"), []);
  assert.deepEqual(rowFacePairs(INSPECT, "output", 9, "regtest"), []);
});

test("decodeScript disassembles standard scripts; truncation is null, never a guess", () => {
  // P2PKH: the canonical five-opcode template, pushes as bare hex.
  assert.equal(
    decodeScript("76a914751e76e8199196d454941c45d1b3a323f1433bd688ac"),
    "OP_DUP OP_HASH160 751e76e8199196d454941c45d1b3a323f1433bd6 OP_EQUALVERIFY OP_CHECKSIG",
  );
  // Witness programs: version opcode then the program push.
  assert.equal(decodeScript(P2WPKH), "OP_0 751e76e8199196d454941c45d1b3a323f1433bd6");
  assert.equal(
    decodeScript(P2TR),
    "OP_1 79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
  );
  assert.equal(decodeScript("6a0b68656c6c6f20776f726c64"), "OP_RETURN 68656c6c6f20776f726c64");
  // OP_PUSHDATA1: explicit little-endian length prefix.
  assert.equal(decodeScript("4c020102"), "0102");
  // An unnamed opcode renders as its byte — honest about the gap.
  assert.equal(decodeScript("50"), "OP_0x50");
  // A truncated push is not a script; neither is non-hex or empty input.
  assert.equal(decodeScript("0014ab"), null);
  assert.equal(decodeScript("4c02ff"), null);
  assert.equal(decodeScript(""), null);
  assert.equal(decodeScript("zz"), null);
});

test("scriptCycle: address first when encodable, hex otherwise, asm last", () => {
  const encodable = scriptCycle(P2WPKH, "regtest", "");
  assert.deepEqual(
    encodable.map((entry) => entry.label),
    ["address", "script hex", "script asm"],
  );
  assert.equal(encodable[0].value, "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080");
  assert.equal(encodable[1].value, P2WPKH);
  assert.equal(encodable[2].value, decodeScript(P2WPKH));
  // A non-encodable script starts at the hex; the prefix threads into
  // every label (the input facts say "prevout …").
  const opReturn = scriptCycle("6a0b68656c6c6f20776f726c64", "regtest", "prevout ");
  assert.deepEqual(
    opReturn.map((entry) => entry.label),
    ["prevout script hex", "prevout script asm"],
  );
});

test("sequenceReading decodes nSequence per BIP 68 (+finality, +BIP 125)", () => {
  // The two all-ones neighbors: final vs merely locktime-enabled.
  assert.equal(sequenceReading("0xffffffff"), "final — relative and absolute locktimes disabled");
  assert.equal(sequenceReading("0xfffffffe"), "no relative locktime (BIP 68 disable bit)");
  // Below 0xfffffffe the input signals BIP 125 replaceability — including
  // with the BIP 68 disable bit still set.
  assert.equal(
    sequenceReading("0xfffffffd"),
    "no relative locktime (BIP 68 disable bit); signals RBF (BIP 125)",
  );
  // Bit 22 clear: the low 16 bits count blocks.
  assert.equal(
    sequenceReading("0x00000090"),
    "relative locktime ≥ 144 blocks; signals RBF (BIP 125)",
  );
  assert.equal(
    sequenceReading("0x00000001"),
    "relative locktime ≥ 1 block; signals RBF (BIP 125)",
  );
  // Bit 22 set: the low 16 bits count 512-second units.
  assert.equal(
    sequenceReading("0x00400004"),
    "relative locktime ≥ 4 × 512s (≈34 min); signals RBF (BIP 125)",
  );
  assert.equal(
    sequenceReading("0x004000a8"),
    "relative locktime ≥ 168 × 512s (≈23.9 h); signals RBF (BIP 125)",
  );
  assert.equal(
    sequenceReading("0x0040ffff"),
    "relative locktime ≥ 65535 × 512s (≈388.4 days); signals RBF (BIP 125)",
  );
  // Reserved bits (16–21, 23–30) do not disturb the value read.
  assert.equal(
    sequenceReading("0x00230090"),
    "relative locktime ≥ 144 blocks; signals RBF (BIP 125)",
  );
  // Absent or unparseable sequences yield no reading.
  assert.equal(sequenceReading(null), null);
  assert.equal(sequenceReading("not-hex"), null);
  assert.equal(sequenceReading("0x1ffffffff"), null);
});

// --- amount emphasis (BIP 177 sat-first; the ead6ca05 convention) ----------

const flatten = (parts) => parts.map((part) => `${part.part}:${part.text}`);
// The thin space (U+2009) that splits the eight fraction digits 4+4; it
// lands inside whichever part holds the fourth fraction digit.
const SEAM = "\u2009";

test("amountSpanParts: zero is all scaffold", () => {
  const parts = amountSpanParts(0);
  assert.deepEqual(flatten(parts), ["symbol:₿", `scale:0.0000${SEAM}0000`]);
});

test("amountSpanParts: sub-BTC keeps the leading zeros as scale scaffold", () => {
  const parts = amountSpanParts(12_345);
  assert.deepEqual(flatten(parts), ["symbol:₿", "scale:0.000", `digits:1${SEAM}2345`]);
});

test("amountSpanParts: whole BTC digits are significant, separators kept", () => {
  // 2,500 BTC + 12,345 sats: the whole-BTC digits are high-order sat digits
  // (full emphasis). With a nonzero whole-BTC part ONLY the decimal point
  // scaffolds — the fraction's zero run belongs to the sat integer.
  const parts = amountSpanParts(250_000_012_345);
  assert.deepEqual(flatten(parts), [
    "symbol:₿",
    "digits:2,500",
    "scale:.",
    `digits:0001${SEAM}2345`,
  ]);
});

test("amountSpanParts: trailing zeros are significant (8.00000000 IS 800,000,000 sats)", () => {
  assert.deepEqual(flatten(amountSpanParts(800_000_000)), [
    "symbol:₿",
    "digits:8",
    "scale:.",
    `digits:0000${SEAM}0000`,
  ]);
});

test("amountSpanParts: 0.00000141 keeps its leading-zero scaffold", () => {
  // The seam falls inside the scaffold here — the fourth fraction digit is
  // a leading zero.
  assert.deepEqual(flatten(amountSpanParts(141)), [
    "symbol:₿",
    `scale:0.0000${SEAM}0`,
    "digits:141",
  ]);
});

test("amountSpanParts: zeros after a mid significant digit are significant (1.05000000)", () => {
  assert.deepEqual(flatten(amountSpanParts(105_000_000)), [
    "symbol:₿",
    "digits:1",
    "scale:.",
    `digits:0500${SEAM}0000`,
  ]);
});

test("amountSpanParts: concatenation is the flat string plus the 4+4 fraction seam", () => {
  for (const sats of [0, 1, 141, 549, 12_345, 99_999_999, 100_000_000, 105_000_000, 800_000_000, 250_000_012_345]) {
    const joined = amountSpanParts(sats).map((part) => part.text).join("");
    assert.equal(joined.replaceAll(SEAM, ""), formatSatAmount(sats));
    const fraction = joined.split(".")[1];
    assert.equal(fraction.length, 9, `8 sat digits + the seam must hold for ${sats}`);
    assert.equal(fraction[4], SEAM, `the seam splits 4+4 for ${sats}`);
  }
});

test("amountSpanParts: classes are the part names", () => {
  for (const part of amountSpanParts(250_000_012_345)) {
    assert.equal(part.className, `session-amount-${part.part}`);
  }
});

test("signedAmountSpanParts: the sign is a significant digit", () => {
  const negative = signedAmountSpanParts(-600);
  assert.deepEqual(flatten(negative), ["digits:−", "symbol:₿", `scale:0.0000${SEAM}0`, "digits:600"]);
  assert.deepEqual(signedAmountSpanParts(600), amountSpanParts(600));
  assert.deepEqual(signedAmountSpanParts(0), amountSpanParts(0));
});

// --- balance sheet (Q2: totals at the bottom, deficits red) -----------------

const summaryWithTotals = (totals) => fragmentSummary({ ...INSPECT, totals });

test("balanceSheet: a surplus is the accounted fee on the output side", () => {
  const sheet = balanceSheet(
    summaryWithTotals({ known_input_sats: 150000, output_sats: 125000, fee_sats_if_inputs_known: 25000 }),
    INSPECT,
  );
  assert.deepEqual(sheet.delta, { kind: "surplus", column: "output", sats: 25000 });
  assert.equal(sheet.inputTotalSats, 150000);
  assert.equal(sheet.outputAccountingTotalSats, 125000);
  assert.equal(sheet.fallbackText, null);
  assert.equal(sheet.showFeeRate, true);
  // Seams absent from inspect JSON today: honest n/a, never a guess.
  assert.equal(sheet.declaredFeeSats, null);
  assert.equal(sheet.sizeEstimateVbytes, null);
  assert.equal(sheet.feeRateText, null);
  assert.equal(sheet.implicitFeeSats, null);
});

test("balanceSheet: deficits sit on the input side (the red rule)", () => {
  const sheet = balanceSheet(
    summaryWithTotals({ known_input_sats: 100000, output_sats: 125000, fee_sats_if_inputs_known: -25000 }),
    INSPECT,
  );
  assert.deepEqual(sheet.delta, { kind: "deficit", column: "input", sats: -25000 });
  assert.equal(sheet.fallbackText, null);
});

test("balanceSheet: balanced and unknown cases", () => {
  const balanced = balanceSheet(
    summaryWithTotals({ known_input_sats: 125000, output_sats: 125000, fee_sats_if_inputs_known: 0 }),
    INSPECT,
  );
  assert.equal(balanced.delta, null);
  assert.equal(balanced.showFeeRate, false);
  assert.equal(balanced.fallbackText, null);

  // The stock fixture's inputs are incomplete: the delta cannot be computed
  // and the honest sentence comes back instead.
  const unknown = balanceSheet(fragmentSummary(INSPECT), INSPECT);
  assert.equal(unknown.delta, null);
  assert.equal(unknown.inputTotalSats, null);
  assert.match(unknown.fallbackText, /fee unknown \(input amounts incomplete\)/);

  const undecoded = balanceSheet(fragmentSummary(null), null);
  assert.match(undecoded.fallbackText, /not decoded/);
});

test("balanceSheet consumes the declared-fee and size seams when present", () => {
  const totals = {
    known_input_sats: 150000,
    output_sats: 125000,
    fee_sats_if_inputs_known: 25000,
    declared_fee_sats: 700,
    size_estimate: 140,
  };
  const inspect = { ...INSPECT, totals };
  const sheet = balanceSheet(summaryWithTotals(totals), inspect);
  assert.equal(sheet.declaredFeeSats, 700);
  // The demo's output accounting total: outputs + declared fees.
  assert.equal(sheet.outputAccountingTotalSats, 125700);
  assert.equal(sheet.implicitFeeSats, 24300);
  assert.equal(sheet.sizeEstimateVbytes, 140);
  assert.equal(sheet.feeRateText, `~${(25000 / 140).toFixed(1)} sat/vB`);
  assert.equal(sheet.outputTotalElidedByDeclaredFees, false);

  // An output total that is 100% declared fees is elided (declared fees are
  // never transaction outputs).
  const allDeclared = balanceSheet(
    summaryWithTotals({ ...totals, output_sats: 0, fee_sats_if_inputs_known: 150000 }),
    { ...INSPECT, totals: { ...totals, output_sats: 0 } },
  );
  assert.equal(allDeclared.outputTotalElidedByDeclaredFees, true);
});

test("balanceSheet: the grand total elides when each side is one line", () => {
  // 2 in / 3 out (the stock fixture): the totals genuinely sum something.
  assert.equal(balanceSheet(fragmentSummary(INSPECT), INSPECT).totalsRedundant, false);
  // 1 in / 1 out: a totals row would repeat the two rows right above it.
  const single = { ...INSPECT, input_count: 1, output_count: 1 };
  assert.equal(balanceSheet(fragmentSummary(single), single).totalsRedundant, true);
  // 0 in / 1 out (a txout intent): still nothing to sum on either side.
  const intent = { ...INSPECT, input_count: 0, output_count: 1 };
  assert.equal(balanceSheet(fragmentSummary(intent), intent).totalsRedundant, true);
  // Unknown counts prove nothing — the totals row stays.
  assert.equal(balanceSheet(fragmentSummary(null), null).totalsRedundant, false);
  // A declared fee folds into the output accounting total (outputs + fees),
  // which no single row repeats — the totals row stays.
  const declared = {
    ...INSPECT,
    input_count: 1,
    output_count: 1,
    totals: { ...INSPECT.totals, declared_fee_sats: 700 },
  };
  assert.equal(balanceSheet(fragmentSummary(declared), declared).totalsRedundant, false);
});

test("seam readers accept the emitted totals.size and tolerated size_estimate shapes", () => {
  assert.equal(declaredFeeSatsFromInspect(INSPECT), null);
  assert.equal(declaredFeeSatsFromInspect(null), null);
  assert.equal(declaredFeeSatsFromInspect({ totals: { declared_fee_sats: 700 } }), 700);
  assert.equal(sizeEstimateVbytesFromInspect(null), null);
  // The REAL emitter shape (ptj commands/inspect.rs size_totals): totals.size
  // is an object whose vbytes = ceil(weight / 4).
  assert.equal(
    sizeEstimateVbytesFromInspect({ totals: { size: { weight: 560, vbytes: 140, exact: false } } }),
    140,
  );
  assert.equal(sizeEstimateVbytesFromInspect({ totals: { size_estimate: 140 } }), 140);
  assert.equal(sizeEstimateVbytesFromInspect({ totals: { size_estimate: { vbytes: 141 } } }), 141);
  assert.equal(sizeEstimateVbytesFromInspect({ size_estimate: 142 }), 142);
  assert.equal(sizeEstimateVbytesFromInspect({ size_estimate: { vbytes: 143 } }), 143);
});

test("formatFeeRate: two decimals below 10 sat/vB, one above", () => {
  assert.equal(formatFeeRate(3.14159), "3.14");
  assert.equal(formatFeeRate(9.999), "10.00");
  assert.equal(formatFeeRate(10), "10.0");
  assert.equal(formatFeeRate(178.571), "178.6");
  assert.equal(formatFeeRate(0), "0.00");
});

test("fragmentCardModel carries the balance sheet", () => {
  const card = fragmentCardModel(INSPECT, "regtest");
  assert.equal(card.balance.delta, null);
  assert.match(card.balance.fallbackText, /fee unknown/);
});

// Deficit-red is a row-level color the amount parts INHERIT (ead6ca05):
// the stylesheet must set the red on .session-balance-deficit and never on
// an amount part.
test("styles.css: the deficit row is red, amounts inherit it", () => {
  const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
  const match = css.match(/^\.session-balance-deficit\s*\{([^}]*)\}/m);
  assert.ok(match, "expected a .session-balance-deficit rule");
  assert.match(match[1], /color:\s*var\(--red\)/);
});

test("amountBits: base-2 fingerprint at natural bit length", () => {
  assert.equal(amountBits(0), "0");
  assert.equal(amountBits(1), "1");
  // 600 sats: 1001011000 — Hamming weight 4.
  assert.equal(amountBits(600), "1001011000");
  assert.equal([...amountBits(600)].filter((bit) => bit === "1").length, 4);
});

test("amountBits: low-Hamming-weight values read as a single mark", () => {
  for (const value of [2 ** 20, 0x100000]) {
    const bits = amountBits(value);
    assert.equal(bits, "1" + "0".repeat(20));
    assert.equal(bits.length, 21, "natural length doubles as a log2 magnitude cue");
    assert.equal([...bits].filter((bit) => bit === "1").length, 1);
  }
});

test("amountBits: exact across the full sat range (BigInt)", () => {
  // All 21M BTC: 2,100,000,000,000,000 sats ≈ 2^51.
  const max = 2_100_000_000_000_000;
  const bits = amountBits(max);
  assert.equal(bits, BigInt(max).toString(2));
  assert.equal(bits.length, 51);
  assert.ok(bits.startsWith("1"));
});

test("amountBits: sign and junk fold to the magnitude fingerprint", () => {
  assert.equal(amountBits(-600), amountBits(600));
  assert.equal(amountBits(NaN), "0");
  assert.equal(amountBits(600.9), amountBits(600));
});

// The ead6ca05 regression, mirrored at the stylesheet level: the scaffold
// parts dim by OPACITY and inherit the surrounding color — no session
// amount rule may force a color (the old grey `.session-amount-muted` and
// invented green `.session-amount-sats` classes must stay gone).
test("styles.css: amount parts dim by opacity only and inherit color", () => {
  const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
  const rule = (selector) => {
    const match = css.match(new RegExp(`^\\${selector}\\s*\\{([^}]*)\\}`, "m"));
    assert.ok(match, `expected a ${selector} rule`);
    return match[1];
  };
  const scale = rule(".session-amount-scale");
  const symbol = rule(".session-amount-symbol");
  for (const [name, body] of [["scale", scale], ["symbol", symbol]]) {
    assert.match(body, /opacity:/, `${name} dims by opacity`);
    assert.doesNotMatch(body, /(?<!-)color:/, `${name} must inherit the surrounding color`);
  }
  // The ₿ symbol is less transparent than the scale scaffold.
  const opacity = (body) => Number(body.match(/opacity:\s*([0-9.]+)/)[1]);
  assert.ok(opacity(symbol) > opacity(scale), "symbol sits above the scaffold in emphasis");
  assert.ok(opacity(symbol) < 1, "symbol stays dimmer than the significant digits");
  // The retired forced-color classes must not come back.
  assert.doesNotMatch(css, /\.session-amount-muted/);
  assert.doesNotMatch(css, /\.session-amount-sats/);
  // Significant digits carry no rule at all: full inheritance.
  assert.doesNotMatch(css, /\.session-amount-digits\s*\{/);
});
