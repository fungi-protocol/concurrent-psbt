import test from "node:test";
import assert from "node:assert/strict";

import {
  cardGroups,
  elisionLabel,
  feeLine,
  fragmentCardModel,
  inputViews,
  outputViews,
  scriptTemplate,
} from "../dist/session/display.js";
import { fragmentSummary } from "../dist/session/state.js";

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
