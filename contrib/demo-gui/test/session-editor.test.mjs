import test from "node:test";
import assert from "node:assert/strict";

import {
  applyEdit,
  applyFix,
  ASSIGN_UIDS_FIX,
  EDIT_SAVE_SEAM,
  editorModel,
  fieldAt,
  validateEditor,
} from "../dist/session/editor.js";

const INSPECT = {
  format: "bip370",
  ordering: "unordered",
  input_count: 1,
  output_count: 2,
  modifiability: { flags: 0, inputs: false, outputs: false },
  sort: { mode: "deterministic", seed_hex: "abcd" },
  inputs: [
    {
      outpoint: `${"aa".repeat(32)}:7`,
      sequence: "0xfffffffe",
      known_utxo_sats: 200000,
    },
  ],
  outputs: [
    { amount_sats: 100000, script_pubkey_hex: "0014" + "22".repeat(20), unique_id_hex: "33".repeat(32) },
    { amount_sats: 50000, script_pubkey_hex: "0014" + "44".repeat(20), unique_id_hex: null },
  ],
  totals: { known_input_sats: 200000, output_sats: 150000, fee_sats_if_inputs_known: 50000 },
};

function model() {
  return editorModel("psbt-1", INSPECT, "regtest");
}

test("editorModel decodes global, per-input, and per-output sections", () => {
  const built = model();
  assert.equal(built.fragmentKey, "psbt-1");
  assert.deepEqual(
    built.sections.map((section) => section.key),
    ["global", "input.0", "output.0", "output.1"],
  );

  assert.equal(fieldAt(built, "global.flags").value, "0");
  assert.equal(fieldAt(built, "global.ordering").value, "unordered");
  assert.equal(fieldAt(built, "global.sort_mode").value, "deterministic");
  assert.equal(fieldAt(built, "global.sort_seed").value, "abcd");
  assert.equal(fieldAt(built, "input.0.txid").value, "aa".repeat(32));
  assert.equal(fieldAt(built, "input.0.vout").value, "7");
  assert.equal(fieldAt(built, "input.0.sequence").value, "0xfffffffe");
  assert.equal(fieldAt(built, "output.0.amount").value, "100000");
  assert.equal(fieldAt(built, "output.1.unique_id").value, "");
  assert.equal(fieldAt(built, "nope"), null);
});

test("editorModel degrades defensively on absent or mangled inspect JSON", () => {
  const empty = editorModel("psbt-9", null, "bitcoin");
  assert.equal(empty.sections.length, 1); // global only
  assert.equal(fieldAt(empty, "global.flags").value, "");
  const mangled = editorModel("psbt-9", { inputs: "no", outputs: [{ outpoint: 5 }] }, "bitcoin");
  assert.equal(fieldAt(mangled, "output.0.script").value, "");
});

test("applyEdit: modifiability flags accept decimal, hex, and named forms", () => {
  let built = model();
  // The motivating case: set the flags back to modifiable.
  built = applyEdit(built, "global.flags", "both");
  assert.equal(fieldAt(built, "global.flags").value, "3");
  assert.equal(fieldAt(built, "global.flags").error, null);

  built = applyEdit(built, "global.flags", "0x2");
  assert.equal(fieldAt(built, "global.flags").value, "2");

  built = applyEdit(built, "global.flags", "7");
  assert.match(fieldAt(built, "global.flags").error, /bits 0 .* and 1/);

  built = applyEdit(built, "global.flags", "banana");
  assert.match(fieldAt(built, "global.flags").error, /none\/inputs\/outputs\/both/);
});

test("applyEdit: liberal parsing per field context", () => {
  let built = model();

  built = applyEdit(built, "global.ordering", " Ordered ");
  assert.equal(fieldAt(built, "global.ordering").value, "ordered");
  built = applyEdit(built, "global.ordering", "sideways");
  assert.match(fieldAt(built, "global.ordering").error, /'ordered' or 'unordered'/);

  built = applyEdit(built, "global.sort_mode", "EXPLICIT");
  assert.equal(fieldAt(built, "global.sort_mode").value, "explicit");
  built = applyEdit(built, "global.sort_mode", "random");
  assert.match(fieldAt(built, "global.sort_mode").error, /unset, deterministic/);

  // hex32 via base64 (32 bytes of 0x11) — liberal input, canonical hex out.
  const b64 = Buffer.from("11".repeat(32), "hex").toString("base64");
  built = applyEdit(built, "input.0.txid", b64);
  assert.equal(fieldAt(built, "input.0.txid").value, "11".repeat(32));
  assert.match(fieldAt(built, "input.0.txid").note, /base64/);

  built = applyEdit(built, "input.0.sequence", "4294967294");
  assert.equal(fieldAt(built, "input.0.sequence").value, "0xfffffffe");
  built = applyEdit(built, "input.0.sequence", "5000000000");
  assert.match(fieldAt(built, "input.0.sequence").error, /32 bits/);
  built = applyEdit(built, "input.0.sequence", "");
  assert.equal(fieldAt(built, "input.0.sequence").value, "");

  built = applyEdit(built, "output.0.amount", "0x10");
  assert.equal(fieldAt(built, "output.0.amount").value, "16");
  built = applyEdit(built, "output.0.amount", "-3");
  assert.match(fieldAt(built, "output.0.amount").error, /non-negative/);

  // A script field accepts an ADDRESS and converts it (regtest network).
  built = applyEdit(built, "output.0.script", "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080");
  assert.equal(fieldAt(built, "output.0.script").value, "0014751e76e8199196d454941c45d1b3a323f1433bd6");
  assert.ok(fieldAt(built, "output.0.script").note !== null);

  // Unknown paths are a no-op.
  const untouched = applyEdit(built, "global.nonexistent", "x");
  assert.deepEqual(untouched, built);
});

test("validateEditor: field errors and cross-field rules become violations", () => {
  let built = model();
  assert.deepEqual(
    validateEditor(built).map((violation) => violation.message),
    [`unordered PSBTs identify outputs by unique id; 1 output(s) have none`],
  );

  built = applyEdit(built, "global.flags", "nope");
  const violations = validateEditor(built);
  assert.equal(violations.length, 2);
  assert.equal(violations[0].path, "global.flags");
  assert.equal(violations[0].fix, null);

  // Ordered PSBTs do not demand unique ids.
  const ordered = applyEdit(model(), "global.ordering", "ordered");
  assert.deepEqual(validateEditor(ordered), []);

  // Deterministic sort without a seed is a violation.
  const seedless = applyEdit(model(), "global.sort_seed", "");
  assert.ok(
    validateEditor(seedless).some((violation) => /requires a sort seed/.test(violation.message)),
  );
});

test("the missing-uid violation offers the generate fix with the informed warning", () => {
  const violations = validateEditor(model());
  assert.equal(violations.length, 1);
  const fix = violations[0].fix;
  assert.equal(fix.id, "assign-uids");
  assert.match(fix.warning, /may result in duplicate txouts if done more than once/);

  // Applying the fix fills ONLY the missing ids, marks them generated, and
  // re-validation passes.
  let counter = 0;
  const fixed = applyFix(model(), fix.id, (length) => new Uint8Array(length).fill(++counter));
  assert.equal(fieldAt(fixed, "output.0.unique_id").value, "33".repeat(32)); // untouched
  assert.equal(fieldAt(fixed, "output.1.unique_id").value, "01".repeat(32));
  assert.equal(fieldAt(fixed, "output.1.unique_id").note, "generated");
  assert.deepEqual(validateEditor(fixed), []);

  // Unknown fix ids are a no-op.
  const untouched = applyFix(model(), "not-a-fix", () => new Uint8Array(32));
  assert.deepEqual(untouched, model());
});

test("the save seam is named for the shell and the backend task list", () => {
  assert.equal(EDIT_SAVE_SEAM, "applyPsbtEdits");
  assert.equal(ASSIGN_UIDS_FIX.id, "assign-uids");
});
