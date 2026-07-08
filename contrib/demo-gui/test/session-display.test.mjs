import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  amountBits,
  amountSpanParts,
  cardGroups,
  elisionLabel,
  feeLine,
  fragmentCardModel,
  inputViews,
  outputViews,
  scriptTemplate,
  signedAmountSpanParts,
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

test("cardGroups: script templates group outputs, inputs stay unattributed", () => {
  const inputs = inputViews(INSPECT);
  const outputs = outputViews(INSPECT, "regtest");
  const groups = cardGroups(inputs, outputs);

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
  const groups = cardGroups(inputs, outputs);

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
  assert.equal(card.groups.length, 3);
  assert.equal(card.uidPresent, 2);
  assert.equal(card.uidTotal, 3);
  assert.match(card.fee.text, /fee unknown/);

  const empty = fragmentCardModel(null, "bitcoin");
  assert.deepEqual(empty.inputs, []);
  assert.deepEqual(empty.groups, []);
  assert.equal(empty.uidPresent, null);
  assert.equal(empty.uidTotal, null);
});

test("elisionLabel counts what the card hides", () => {
  assert.equal(elisionLabel(3, 10), "+7 more");
  assert.equal(elisionLabel(3, 3), null);
  assert.equal(elisionLabel(5, 2), null);
});

// --- amount emphasis (BIP 177 sat-first; the ead6ca05 convention) ----------

const flatten = (parts) => parts.map((part) => `${part.part}:${part.text}`);

test("amountSpanParts: zero is all scaffold", () => {
  const parts = amountSpanParts(0);
  assert.deepEqual(flatten(parts), ["symbol:₿", "scale:0.00000000"]);
});

test("amountSpanParts: sub-BTC keeps the leading zeros as scale scaffold", () => {
  const parts = amountSpanParts(12_345);
  assert.deepEqual(flatten(parts), ["symbol:₿", "scale:0.000", "digits:12345"]);
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
    "digits:00012345",
  ]);
});

test("amountSpanParts: trailing zeros are significant (8.00000000 IS 800,000,000 sats)", () => {
  assert.deepEqual(flatten(amountSpanParts(800_000_000)), [
    "symbol:₿",
    "digits:8",
    "scale:.",
    "digits:00000000",
  ]);
});

test("amountSpanParts: 0.00000141 keeps its leading-zero scaffold", () => {
  assert.deepEqual(flatten(amountSpanParts(141)), [
    "symbol:₿",
    "scale:0.00000",
    "digits:141",
  ]);
});

test("amountSpanParts: zeros after a mid significant digit are significant (1.05000000)", () => {
  assert.deepEqual(flatten(amountSpanParts(105_000_000)), [
    "symbol:₿",
    "digits:1",
    "scale:.",
    "digits:05000000",
  ]);
});

test("amountSpanParts: concatenation is the flat string with all 8 fraction digits", () => {
  for (const sats of [0, 1, 141, 549, 12_345, 99_999_999, 100_000_000, 105_000_000, 800_000_000, 250_000_012_345]) {
    const joined = amountSpanParts(sats).map((part) => part.text).join("");
    assert.equal(joined, formatSatAmount(sats));
    const fraction = joined.split(".")[1];
    assert.equal(fraction.length, 8, `8th-sat-digit position must hold for ${sats}`);
  }
});

test("amountSpanParts: classes are the part names", () => {
  for (const part of amountSpanParts(250_000_012_345)) {
    assert.equal(part.className, `session-amount-${part.part}`);
  }
});

test("signedAmountSpanParts: the sign is a significant digit", () => {
  const negative = signedAmountSpanParts(-600);
  assert.deepEqual(flatten(negative), ["digits:−", "symbol:₿", "scale:0.00000", "digits:600"]);
  assert.deepEqual(signedAmountSpanParts(600), amountSpanParts(600));
  assert.deepEqual(signedAmountSpanParts(0), amountSpanParts(0));
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
