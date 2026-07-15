import test from "node:test";
import assert from "node:assert/strict";

import {
  applyEdit,
  applyFix,
  ASSIGN_UIDS_FIX,
  decodedEditsLeftBehind,
  EDIT_SAVE_SEAM,
  editorModel,
  fieldAt,
  isRawPath,
  OUTPUT_UNIQUE_ID_KEY_HEX,
  rawEditsForSave,
  TX_MODIFIABLE_BITS,
  TX_MODIFIABLE_KEY_HEX,
  validateEditor,
  violationsFromServer,
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

  // The REAL global field: the tx-modifiable bitfield as hex bytes
  // (derived from the interpreted flags when no raw map travels).
  assert.equal(fieldAt(built, "global.tx_modifiable").value, "00");
  assert.equal(fieldAt(built, "global.tx_modifiable").context, "bitfield");
  // Ordering is an interpretation, not an editable pseudo-field.
  assert.equal(fieldAt(built, "global.ordering"), null);
  assert.equal(built.ordering, "unordered");
  assert.equal(fieldAt(built, "global.sort_mode").value, "deterministic");
  assert.equal(fieldAt(built, "global.sort_seed").value, "abcd");
  assert.equal(fieldAt(built, "input.0.txid").value, "aa".repeat(32));
  assert.equal(fieldAt(built, "input.0.vout").value, "7");
  assert.equal(fieldAt(built, "input.0.sequence").value, "0xfffffffe");
  assert.equal(fieldAt(built, "output.0.amount").value, "100000");
  assert.equal(fieldAt(built, "output.1.unique_id").value, "");
  assert.equal(fieldAt(built, "nope"), null);
});

test("editorModel prefers the raw tx-modifiable bytes over the interpretation", () => {
  const withRaw = editorModel(
    "psbt-raw",
    {
      ...INSPECT,
      raw: {
        // A future-spec value longer than one byte survives verbatim.
        global: [{ key_hex: "06", value_hex: "0380", key_type: 6, key_data_hex: "", kind: "known" }],
        inputs: [[]],
        outputs: [[], []],
      },
    },
    "regtest",
  );
  assert.equal(fieldAt(withRaw, "global.tx_modifiable").value, "0380");
});

test("editorModel degrades defensively on absent or mangled inspect JSON", () => {
  const empty = editorModel("psbt-9", null, "bitcoin");
  assert.equal(empty.sections.length, 1); // global only
  assert.equal(fieldAt(empty, "global.tx_modifiable").value, "");
  assert.equal(empty.ordering, null);
  const mangled = editorModel("psbt-9", { inputs: "no", outputs: [{ outpoint: 5 }] }, "bitcoin");
  assert.equal(fieldAt(mangled, "output.0.script").value, "");
});

test("applyEdit: the tx-modifiable bitfield takes hex bytes, empty deletes", () => {
  let built = model();
  built = applyEdit(built, "global.tx_modifiable", "03");
  assert.equal(fieldAt(built, "global.tx_modifiable").value, "03");
  assert.equal(fieldAt(built, "global.tx_modifiable").error, null);

  // Unknown bits and extra bytes pass through — the hex form IS the escape
  // hatch for specs this program does not understand yet.
  built = applyEdit(built, "global.tx_modifiable", "FF01");
  assert.equal(fieldAt(built, "global.tx_modifiable").value, "ff01");

  built = applyEdit(built, "global.tx_modifiable", "");
  assert.equal(fieldAt(built, "global.tx_modifiable").value, "");

  built = applyEdit(built, "global.tx_modifiable", "!!not bytes!!");
  assert.match(fieldAt(built, "global.tx_modifiable").error, /./);
});

test("applyEdit: liberal parsing per field context", () => {
  let built = model();

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

  built = applyEdit(built, "global.tx_modifiable", "!!not bytes!!");
  const violations = validateEditor(built);
  assert.equal(violations.length, 2);
  assert.equal(violations[0].path, "global.tx_modifiable");
  assert.equal(violations[0].fix, null);

  // Ordered PSBTs do not demand unique ids.
  const ordered = editorModel("psbt-1", { ...INSPECT, ordering: "ordered" }, "regtest");
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
  // Generated ids are 16 bytes — the size ptj's own assign-ids mints.
  assert.equal(fieldAt(fixed, "output.1.unique_id").value, "01".repeat(16));
  assert.equal(fieldAt(fixed, "output.1.unique_id").note, "generated");
  assert.deepEqual(validateEditor(fixed), []);

  // Unknown fix ids are a no-op.
  const untouched = applyFix(model(), "not-a-fix", () => new Uint8Array(16));
  assert.deepEqual(untouched, model());
});

test("the save seam is named for the shell and the backend task list", () => {
  assert.equal(EDIT_SAVE_SEAM, "applyPsbtEdits");
  assert.equal(ASSIGN_UIDS_FIX.id, "assign-uids");
});

// --- raw keymap rows + the applyPsbtEdits save path ------------------------

const PROPRIETARY_KEY = "fc0470736274ab";
const UNKNOWN_KEY = "09";

const RAW_INSPECT = {
  ...INSPECT,
  raw: {
    global: [
      // A pair the decoded fields already parse: stays collapsed.
      { key_hex: "02", value_hex: "02000000", key_type: 2, key_data_hex: "", kind: "known" },
      {
        key_hex: PROPRIETARY_KEY,
        value_hex: "beef",
        key_type: 252,
        key_data_hex: "0470736274ab",
        kind: "proprietary",
        proprietary: { prefix_hex: "70736274", prefix_utf8: "psbt", subtype: 171, key_data_hex: "" },
      },
    ],
    inputs: [[]],
    outputs: [
      [{ key_hex: UNKNOWN_KEY, value_hex: "1111", key_type: 9, key_data_hex: "", kind: "unknown" }],
      [],
    ],
  },
};

function rawModel() {
  return editorModel("psbt-2", RAW_INSPECT, "regtest");
}

test("editorModel renders unknown/proprietary raw entries as editable hex rows", () => {
  const built = rawModel();
  const rawKeys = built.sections.filter((section) => isRawPath(section.key)).map((s) => s.key);
  // Empty maps and all-known maps mint no raw section.
  assert.deepEqual(rawKeys, ["raw.global", "raw.output.0"]);

  const proprietary = fieldAt(built, `raw.global.${PROPRIETARY_KEY}`);
  assert.equal(proprietary.value, "beef");
  assert.equal(proprietary.context, "hex");
  assert.match(proprietary.label, /proprietary psbt #171/);
  assert.match(proprietary.note, /deletes the entry/);

  const unknown = fieldAt(built, `raw.output.0.${UNKNOWN_KEY}`);
  assert.equal(unknown.value, "1111");
  assert.match(unknown.label, /unknown key type 9/);

  // The known pair stays collapsed into the decoded fields.
  assert.equal(fieldAt(built, "raw.global.02"), null);
});

test("rawEditsForSave diffs raw rows into {map, key, value|null} edits", () => {
  const pristine = rawModel();
  let edited = applyEdit(rawModel(), `raw.global.${PROPRIETARY_KEY}`, "ABCD");
  edited = applyEdit(edited, `raw.output.0.${UNKNOWN_KEY}`, "");

  const edits = rawEditsForSave(pristine, edited);
  assert.deepEqual(edits, [
    { map: "global", key: PROPRIETARY_KEY, value: "abcd" },
    { map: "output:0", key: UNKNOWN_KEY, value: null },
  ]);

  // Untouched models produce no edits; errored rows are never sent.
  // (Note "!!" — the value context parses LIBERALLY, so a merely odd-looking
  // string like "zz not bytes zz" decodes as base58; only genuinely
  // undecodable text errors.)
  assert.deepEqual(rawEditsForSave(pristine, rawModel()), []);
  const errored = applyEdit(rawModel(), `raw.global.${PROPRIETARY_KEY}`, "!!not bytes!!");
  assert.deepEqual(rawEditsForSave(pristine, errored), []);
  assert.match(fieldAt(errored, `raw.global.${PROPRIETARY_KEY}`).error, /./);
});

test("decodedEditsLeftBehind names decoded changes that cannot travel", () => {
  const pristine = rawModel();
  let edited = applyEdit(rawModel(), "output.0.amount", "123456");
  edited = applyEdit(edited, `raw.global.${PROPRIETARY_KEY}`, "abcd");
  assert.deepEqual(decodedEditsLeftBehind(pristine, edited), ["output.0.amount"]);
  assert.deepEqual(decodedEditsLeftBehind(pristine, rawModel()), []);
});

// --- decoded fields that DO travel: unique ids and the tx-modifiable byte --

test("unique-id and tx-modifiable edits travel as raw-keymap edits", () => {
  const pristine = model();
  // Add an id to the output missing one, replace the other, flip a bit.
  let edited = applyEdit(model(), "output.1.unique_id", "aa".repeat(16));
  edited = applyEdit(edited, "output.0.unique_id", "");
  edited = applyEdit(edited, "global.tx_modifiable", "03");

  const edits = rawEditsForSave(pristine, edited);
  assert.deepEqual(edits, [
    { map: "global", key: TX_MODIFIABLE_KEY_HEX, value: "03" },
    { map: "output:0", key: OUTPUT_UNIQUE_ID_KEY_HEX, value: null }, // cleared = delete
    { map: "output:1", key: OUTPUT_UNIQUE_ID_KEY_HEX, value: "aa".repeat(16) },
  ]);

  // They are no longer "left behind" — nothing to warn about.
  assert.deepEqual(decodedEditsLeftBehind(pristine, edited), []);
});

test("the defined tx-modifiable bits are named for the checkbox UI", () => {
  assert.deepEqual(
    TX_MODIFIABLE_BITS.map((entry) => entry.bit),
    [0, 1, 2],
  );
  assert.match(TX_MODIFIABLE_BITS[0].label, /inputs/);
  assert.match(TX_MODIFIABLE_BITS[1].label, /outputs/);
  assert.match(TX_MODIFIABLE_BITS[2].label, /SIGHASH_SINGLE/);
});

test("the proprietary unique-id raw row collapses into the decoded field", () => {
  const built = editorModel(
    "psbt-uid",
    {
      ...INSPECT,
      raw: {
        global: [],
        inputs: [[]],
        outputs: [
          [
            {
              key_hex: OUTPUT_UNIQUE_ID_KEY_HEX,
              value_hex: "33".repeat(32),
              key_type: 252,
              key_data_hex: OUTPUT_UNIQUE_ID_KEY_HEX.slice(2),
              kind: "proprietary",
              proprietary: { prefix_hex: "636f6e63757272656e742d70736274", prefix_utf8: "concurrent-psbt", subtype: 1, key_data_hex: "" },
            },
          ],
          [],
        ],
      },
    },
    "regtest",
  );
  // No raw row for the uid — the decoded unique-id field is the editor.
  assert.equal(fieldAt(built, `raw.output.0.${OUTPUT_UNIQUE_ID_KEY_HEX}`), null);
  assert.equal(fieldAt(built, "output.0.unique_id").value, "33".repeat(32));
});

test("violationsFromServer maps /api/edit violations into the editor loop", () => {
  const mapped = violationsFromServer([
    {
      id: "unordered-missing-output-ids",
      message: "the PSBT is unordered but 1 output lacks PSBT_OUT_UNIQUE_ID",
      override_param: "allow_missing_output_ids",
      fix_id: "assign-ids",
      fix_label: "Generate missing output unique IDs",
      warning_text:
        "Automatically generating unique IDs may result in duplicate txouts if done more than once.",
    },
    {
      id: "duplicate-output-ids",
      message: "outputs 0 and 1 carry the same PSBT_OUT_UNIQUE_ID",
      override_param: "allow_duplicate_output_ids",
    },
  ]);

  assert.equal(mapped.length, 2);
  assert.equal(mapped[0].source, "server");
  assert.equal(mapped[0].overrideParam, "allow_missing_output_ids");
  assert.equal(mapped[0].fix.id, "assign-ids");
  assert.equal(mapped[0].fix.label, "Generate missing output unique IDs");
  // The canonical duplicate-txout caveat rides the fix offer VERBATIM.
  assert.equal(
    mapped[0].fix.warning,
    "Automatically generating unique IDs may result in duplicate txouts if done more than once.",
  );
  assert.equal(mapped[1].fix, null);
  assert.equal(mapped[1].overrideParam, "allow_duplicate_output_ids");
});
