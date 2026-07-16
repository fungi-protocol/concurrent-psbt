// contrib/demo-gui/src/session/editor.ts
//
// Low-level fragment editor presenter — the field-by-field view/edit model
// under a fragment card. Pure (node --test covered by
// test/session-editor.test.mjs): builds editable rows from `ptj inspect`
// JSON, applies liberally-parsed edits (every field accepts whatever
// encoding its context admits — see ./encoding.ts), and models SAVE as
// validate -> violations[], where a violation may OFFER an automatic fix
// carrying an informed warning. Applying a fix re-validates.
//
// HONESTY NOTES (updated as the seams landed):
// - Inspect now exposes the raw global/per-input/per-output keymaps
//   (raw.*[].key_hex — the full raw key, compact-size keytype prefix
//   included). Unknown and PROPRIETARY entries (concurrent-psbt's unique
//   ids, sort metadata, ...) render as editable raw hex rows; entries the
//   decoded fields above already parse stay collapsed into those fields.
// - The save seam landed: EDIT_SAVE_SEAM (Backend.applyPsbtEdits ->
//   POST /api/edit) takes raw-keymap edits, runs save-time validation, and
//   returns structured violations with fix offers and named overrides. The
//   shell sends the raw rows that CHANGED (rawEditsForSave below) — no
//   client-side byte re-encoding.
// - Decoded convenience fields whose values ARE the raw bytes travel on
//   save as raw-keymap edits under their constant keys: the tx-modifiable
//   bitfield (global 0x06) and per-output unique ids (proprietary
//   concurrent-psbt#1). Other decoded fields (amount, txid, script, ...)
//   still validate locally only — translating them into raw bytes is a
//   backend concern (a typed-edit request shape /api/edit does not take
//   yet), so their edits do NOT travel — the shell says so explicitly.
// - Edits NEVER mutate the source fragment: a saved edit mints a NEW
//   fragment (grow-only, like every other operation result).

import type { EditViolation, FieldEdit, InspectResponse } from "../shared-frontend/core/backend.js";
import { asArray, asNumber, asObject, asString } from "./state.js";
import { bytesToHex, parseFlexible, type Network } from "./encoding.js";

// The Backend seam method the save path drives (the field-edit route:
// psbt + raw-keymap edits -> validated, re-encoded psbt). Named here so the
// shell and the report speak the same name.
export const EDIT_SAVE_SEAM = "applyPsbtEdits";

export type FieldContext =
  | "bitfield" // a raw bitfield byte string (PSBT_GLOBAL_TX_MODIFIABLE): the
  // renderer offers one checkbox per DEFINED bit plus the hex value itself
  // as the escape hatch for bits this program does not (yet) understand
  | "sort-mode" // unset | deterministic | explicit
  | "hex" // free-length hex bytes (base64 accepted and converted)
  | "hex32" // exactly 32 bytes
  | "uid" // a unique id: free-length hex, empty allowed (not yet assigned).
  // 32 bytes is a recommendation (collision resistance for blindly minted
  // ids), not a rule — e.g. 8-byte interactive-tx serial_ids are fine verbatim
  | "integer" // non-negative decimal (0x hex accepted)
  | "u32" // sequence numbers
  | "script"; // scriptPubKey hex, or an address (converted)

// PSBT_GLOBAL_TX_MODIFIABLE (BIP 370 keytype 0x06): full raw key hex and the
// bits the spec defines. Unknown bits survive round-trips through the hex
// escape hatch untouched.
export const TX_MODIFIABLE_KEY_HEX = "06";
export const TX_MODIFIABLE_BITS = [
  { bit: 0, label: "inputs modifiable" },
  { bit: 1, label: "outputs modifiable" },
  { bit: 2, label: "has SIGHASH_SINGLE" },
] as const;

// Flip one bit of a bitfield value's first byte, preserving every other
// byte verbatim (unknown trailing bytes belong to specs this program
// doesn't understand yet). Returns null when the current value is not
// plain hex bytes — the checkboxes must stay inert rather than reinterpret
// garbage as a byte (parseInt would read "ba" out of "banana") and
// clobber whatever the operator was typing into the escape hatch.
export function toggledBitfieldValue(value: string, bit: number, checked: boolean): string | null {
  if (!/^([0-9a-fA-F]{2})*$/.test(value)) return null;
  const byte0 = value ? Number.parseInt(value.slice(0, 2), 16) : 0;
  const flipped = checked ? byte0 | (1 << bit) : byte0 & ~(1 << bit);
  return `${flipped.toString(16).padStart(2, "0")}${value.slice(2)}`;
}

// The psbt.md per-output unique id: proprietary keytype 0xFC, prefix
// "concurrent-psbt" (15 bytes), subtype 0x01, no subkeydata. The value IS
// the id bytes, so a decoded unique-id edit translates byte-for-byte into a
// raw-keymap edit — no client-side re-encoding beyond quoting the key.
export const OUTPUT_UNIQUE_ID_KEY_HEX = "fc0f636f6e63757272656e742d7073627401";

// The psbt.md sort metadata, same proprietary prefix: subtype 0x12 =
// PSBT_GLOBAL_SORT_DETERMINISTIC (0x01 = keys derived from the seed,
// 0x00 = explicit sort keys, absent = unset) and subtype 0x11 =
// PSBT_GLOBAL_SORT_SEED (the raw seed bytes). Both translate into raw
// edits like the unique id — the mode maps its enum to the value byte,
// the seed travels byte-verbatim.
export const SORT_DETERMINISTIC_KEY_HEX = "fc0f636f6e63757272656e742d7073627412";
export const SORT_SEED_KEY_HEX = "fc0f636f6e63757272656e742d7073627411";
export const SORT_MODES = [
  { value: "unset", label: "unset" },
  { value: "deterministic", label: "deterministic (keys derived from the seed)" },
  { value: "explicit", label: "explicit (entries carry their sort keys)" },
] as const;

export interface EditorField {
  // Stable row id: "global.flags", "input.0.txid", "output.1.unique_id", …
  path: string;
  label: string;
  value: string;
  context: FieldContext;
  // Liberal-parse failure for the CURRENT value (set by applyEdit).
  error: string | null;
  // Provenance note: "decoded base64", "converted from address", "generated".
  note: string | null;
}

export interface EditorSection {
  key: string;
  title: string;
  fields: EditorField[];
}

export interface EditorModel {
  fragmentKey: string;
  network: Network;
  // Interpretation, not a field: inspect's derived ordering verdict. The
  // editor exposes the underlying records (tx-modifiable bitfield, the
  // proprietary sort rows), never a stringly "ordering" pseudo-field.
  ordering: "ordered" | "unordered" | null;
  sections: EditorSection[];
}

function field(path: string, label: string, value: string, context: FieldContext): EditorField {
  return { path, label, value, context, error: null, note: null };
}

// Build the editable model from inspect JSON. Missing pieces degrade to
// empty editable values (the whole point is repairing incomplete fragments,
// e.g. an atomized BIP 174 import without output unique ids).
export function editorModel(
  fragmentKey: string,
  inspect: InspectResponse | null,
  network: Network,
): EditorModel {
  const root = asObject(inspect);
  const sort = asObject(root?.sort);
  const modifiability = asObject(root?.modifiability);
  const flags = asNumber(modifiability?.flags);

  // The REAL field, byte-faithful: raw global 0x06's value hex when present
  // (any length — a future spec may define more bytes), the interpreted
  // flags number only as a fallback for inspect payloads without raw maps.
  const rawGlobalEntries = asArray(asObject(root?.raw)?.global) ?? [];
  const txModifiableRaw = rawGlobalEntries
    .map((entry) => asObject(entry))
    .find((entry) => asString(entry?.key_hex) === TX_MODIFIABLE_KEY_HEX);
  const txModifiableHex =
    asString(txModifiableRaw?.value_hex) ??
    (flags === null ? "" : flags.toString(16).padStart(2, "0"));

  const globalSection: EditorSection = {
    key: "global",
    title: "Global",
    fields: [
      field(
        "global.tx_modifiable",
        "tx modifiable (PSBT_GLOBAL_TX_MODIFIABLE, hex)",
        txModifiableHex,
        "bitfield",
      ),
      field(
        "global.sort_mode",
        "sort mode (PSBT_GLOBAL_SORT_DETERMINISTIC)",
        asString(sort?.mode) ?? "unset",
        "sort-mode",
      ),
      field("global.sort_seed", "sort seed (PSBT_GLOBAL_SORT_SEED, hex)", asString(sort?.seed_hex) ?? "", "hex"),
    ],
  };

  const inputs = asArray(root?.inputs) ?? [];
  const inputSections = inputs.map((raw, index): EditorSection => {
    const input = asObject(raw);
    const outpoint = asString(input?.outpoint) ?? "";
    const colon = outpoint.lastIndexOf(":");
    const txid = colon > 0 ? outpoint.slice(0, colon) : "";
    const vout = colon > 0 ? outpoint.slice(colon + 1) : "";
    return {
      key: `input.${index}`,
      title: `Input ${index}`,
      fields: [
        field(`input.${index}.txid`, "previous txid", txid, "hex32"),
        field(`input.${index}.vout`, "output index", vout, "integer"),
        field(`input.${index}.sequence`, "sequence", asString(input?.sequence) ?? "", "u32"),
      ],
    };
  });

  const outputs = asArray(root?.outputs) ?? [];
  const outputSections = outputs.map((raw, index): EditorSection => {
    const output = asObject(raw);
    const amount = asNumber(output?.amount_sats);
    return {
      key: `output.${index}`,
      title: `Output ${index}`,
      fields: [
        field(
          `output.${index}.amount`,
          "amount (sats)",
          amount === null ? "" : String(amount),
          "integer",
        ),
        field(`output.${index}.script`, "scriptPubKey", asString(output?.script_pubkey_hex) ?? "", "script"),
        field(
          `output.${index}.unique_id`,
          "unique id",
          asString(output?.unique_id_hex) ?? "",
          "uid",
        ),
      ],
    };
  });

  return {
    fragmentKey,
    network,
    ordering:
      asString(root?.ordering) === "unordered"
        ? "unordered"
        : asString(root?.ordering) === "ordered"
          ? "ordered"
          : null,
    sections: [globalSection, ...inputSections, ...outputSections, ...rawSections(root)],
  };
}

// ---------------------------------------------------------------------------
// Raw keymap rows — the /api/edit handles. Inspect's raw projection lists
// every map entry with its full raw key (key_hex); entries the decoded
// fields above already parse (kind "known") stay collapsed into those
// fields, while unknown and proprietary entries render here as hex rows.
// Editing needs no client-side re-encoding: the key bytes come from inspect
// verbatim and the value is the row's canonical hex (empty = delete).
// ---------------------------------------------------------------------------

const RAW_SECTION_PREFIX = "raw.";

export function isRawPath(path: string): boolean {
  return path.startsWith(RAW_SECTION_PREFIX);
}

function rawSections(root: Record<string, unknown> | null): EditorSection[] {
  const raw = asObject(root?.raw);
  if (!raw) return [];
  const sections: EditorSection[] = [];
  const global = rawSection("raw.global", "Global — raw keymap", asArray(raw.global));
  if (global) sections.push(global);
  (asArray(raw.inputs) ?? []).forEach((entries, index) => {
    const section = rawSection(
      `raw.input.${index}`,
      `Input ${index} — raw keymap`,
      asArray(entries),
    );
    if (section) sections.push(section);
  });
  (asArray(raw.outputs) ?? []).forEach((entries, index) => {
    const section = rawSection(
      `raw.output.${index}`,
      `Output ${index} — raw keymap`,
      asArray(entries),
      // The unique id already renders (and now saves) as the decoded
      // per-output field; a second editable copy would race it.
      [OUTPUT_UNIQUE_ID_KEY_HEX],
    );
    if (section) sections.push(section);
  });
  return sections;
}

function rawSection(
  key: string,
  title: string,
  entries: unknown[] | null,
  collapsedKeys: readonly string[] = [],
): EditorSection | null {
  const fields: EditorField[] = [];
  for (const raw of entries ?? []) {
    const entry = asObject(raw);
    const keyHex = asString(entry?.key_hex);
    if (!keyHex) continue;
    if (collapsedKeys.includes(keyHex)) continue;
    const kind = asString(entry?.kind) ?? "unknown";
    if (kind === "known") continue; // already shown as a decoded field
    const row = field(`${key}.${keyHex}`, rawLabel(entry), asString(entry?.value_hex) ?? "", "hex");
    row.note = "clearing the value deletes the entry on save";
    fields.push(row);
  }
  if (!fields.length) return null;
  return { key, title, fields };
}

function rawLabel(entry: Record<string, unknown> | null): string {
  const kind = asString(entry?.kind) ?? "unknown";
  const keyType = asNumber(entry?.key_type);
  if (kind === "proprietary") {
    const proprietary = asObject(entry?.proprietary);
    const prefix = asString(proprietary?.prefix_utf8) ?? asString(proprietary?.prefix_hex);
    const subtype = asNumber(proprietary?.subtype);
    if (prefix !== null && subtype !== null) {
      return `proprietary ${prefix} #${subtype}`;
    }
    return "proprietary entry";
  }
  return keyType === null ? "raw entry (unparsed key)" : `unknown key type ${keyType}`;
}

// The raw-keymap edits a save must send: every raw row whose canonical value
// differs from the pristine model's, as {map, key, value|null}. Rows with a
// liberal-parse error are NEVER sent (the shell blocks the save on them).
//
// Some decoded fields ALSO travel, because they translate into raw entries
// without client-side re-encoding: global.tx_modifiable -> global 0x06,
// output.N.unique_id -> the proprietary concurrent-psbt#1 entry of output N
// (both byte-verbatim; empty deletes the entry), global.sort_mode -> the
// concurrent-psbt#0x12 entry (its enum IS the value byte; unset deletes),
// and global.sort_seed -> the concurrent-psbt#0x11 entry (byte-verbatim).
export function rawEditsForSave(pristine: EditorModel, edited: EditorModel): FieldEdit[] {
  const edits: FieldEdit[] = [];
  for (const section of edited.sections) {
    for (const candidate of section.fields) {
      if (candidate.error) continue;
      const before = fieldAt(pristine, candidate.path);
      if (before && before.value === candidate.value) continue;
      if (isRawPath(section.key)) {
        edits.push({
          map: mapSelector(section.key),
          key: candidate.path.slice(section.key.length + 1),
          value: candidate.value ? candidate.value : null,
        });
        continue;
      }
      const translated = translatedRawEdit(candidate.path, candidate.value);
      if (translated) edits.push(translated);
    }
  }
  return edits;
}

// Decoded paths whose edits translate directly into raw-keymap edits.
export function translatedRawEdit(path: string, value: string): FieldEdit | null {
  if (path === "global.tx_modifiable") {
    return { map: "global", key: TX_MODIFIABLE_KEY_HEX, value: value ? value : null };
  }
  if (path === "global.sort_mode") {
    const byte = value === "deterministic" ? "01" : value === "explicit" ? "00" : null;
    return { map: "global", key: SORT_DETERMINISTIC_KEY_HEX, value: byte };
  }
  if (path === "global.sort_seed") {
    return { map: "global", key: SORT_SEED_KEY_HEX, value: value ? value : null };
  }
  const uid = path.match(/^output\.(\d+)\.unique_id$/);
  if (uid) {
    return {
      map: `output:${uid[1]}`,
      key: OUTPUT_UNIQUE_ID_KEY_HEX,
      value: value ? value : null,
    };
  }
  return null;
}

// "raw.global" -> "global"; "raw.input.3" -> "input:3"; "raw.output.0" -> "output:0".
function mapSelector(sectionKey: string): string {
  const rest = sectionKey.slice(RAW_SECTION_PREFIX.length);
  const dot = rest.indexOf(".");
  return dot === -1 ? rest : `${rest.slice(0, dot)}:${rest.slice(dot + 1)}`;
}

// Decoded (non-raw) fields whose value changed. These do NOT travel over the
// save seam — /api/edit takes raw-keymap edits only, and translating decoded
// values into raw bytes client-side would be exactly the re-encoding this
// frontend refuses to do. The shell names them so nothing is dropped
// silently.
export function decodedEditsLeftBehind(pristine: EditorModel, edited: EditorModel): string[] {
  const paths: string[] = [];
  for (const section of edited.sections) {
    if (isRawPath(section.key)) continue;
    for (const candidate of section.fields) {
      if (translatedRawEdit(candidate.path, candidate.value)) continue; // travels
      const before = fieldAt(pristine, candidate.path);
      if (before && before.value !== candidate.value) {
        paths.push(candidate.path);
      }
    }
  }
  return paths;
}

export function fieldAt(model: EditorModel, path: string): EditorField | null {
  for (const section of model.sections) {
    const found = section.fields.find((candidate) => candidate.path === path);
    if (found) return found;
  }
  return null;
}

interface Canonicalized {
  value: string;
  error: string | null;
  note: string | null;
}

function canonicalize(text: string, context: FieldContext, network: Network): Canonicalized {
  const trimmed = text.trim();
  switch (context) {
    case "bitfield": {
      // The value is the raw entry's bytes; empty deletes the entry on
      // save. Hex only — the checkbox UI covers the defined bits, and the
      // hex form is precisely the escape hatch for undefined ones.
      if (!trimmed) return { value: "", error: null, note: null };
      const parsed = parseFlexible(trimmed, "hex-bytes");
      if (!parsed.ok) return { value: text, error: parsed.error, note: null };
      return { value: parsed.canonical, error: null, note: parsed.note ?? null };
    }
    case "sort-mode": {
      const lower = trimmed.toLowerCase();
      if (lower === "unset" || lower === "deterministic" || lower === "explicit") {
        return { value: lower, error: null, note: null };
      }
      return { value: text, error: "sort mode is unset, deterministic, or explicit", note: null };
    }
    case "hex":
    case "hex32":
    case "uid": {
      if (!trimmed && context !== "hex32") return { value: "", error: null, note: null };
      const parsed = parseFlexible(trimmed, context === "hex32" ? "hex-bytes-32" : "hex-bytes");
      if (!parsed.ok) return { value: text, error: parsed.error, note: null };
      return { value: parsed.canonical, error: null, note: parsed.note ?? null };
    }
    case "integer": {
      const parsed = parseUnsigned(trimmed);
      if (parsed === null) return { value: text, error: "must be a non-negative integer", note: null };
      return { value: String(parsed), error: null, note: null };
    }
    case "u32": {
      if (!trimmed) return { value: "", error: null, note: null };
      const parsed = parseUnsigned(trimmed);
      if (parsed === null || parsed > 0xffffffff) {
        return { value: text, error: "sequence must fit in 32 bits (decimal or 0x hex)", note: null };
      }
      return { value: `0x${parsed.toString(16).padStart(8, "0")}`, error: null, note: null };
    }
    case "script": {
      const parsed = parseFlexible(trimmed, "script", network);
      if (!parsed.ok) return { value: text, error: parsed.error, note: null };
      return { value: parsed.canonical, error: null, note: parsed.note ?? null };
    }
  }
}

function parseUnsigned(text: string): number | null {
  if (/^\d+$/.test(text)) return Number(text);
  if (/^0x[0-9a-fA-F]+$/.test(text)) return Number.parseInt(text.slice(2), 16);
  return null;
}

// Apply one edit: liberal-parse the raw text for the field's context and
// either canonicalize (with a provenance note for conversions) or record the
// error inline. Unknown paths are a no-op.
export function applyEdit(model: EditorModel, path: string, text: string): EditorModel {
  return {
    ...model,
    sections: model.sections.map((section) => ({
      ...section,
      fields: section.fields.map((candidate) =>
        candidate.path === path
          ? { ...candidate, ...canonicalize(text, candidate.context, model.network) }
          : candidate,
      ),
    })),
  };
}

// ---------------------------------------------------------------------------
// Save-time validation: violations are DISPLAYED, never silently fixed. A
// violation may offer an automatic fix; the fix carries the informed warning
// the shell must show alongside it.
// ---------------------------------------------------------------------------

export interface ViolationFix {
  id: string;
  label: string;
  warning: string;
}

export interface Violation {
  path: string | null;
  message: string;
  fix: ViolationFix | null;
  // Provenance: the local pre-flight validation (default) or the server's
  // save-time validation (an /api/edit 400 body). Server fixes run
  // SERVER-side — the shell requests them via apply_fixes on the next save
  // instead of applying them to the model.
  source?: "local" | "server";
  // Server violations carry the named request param that waives the gate on
  // the next save (the allow_short_seed convention).
  overrideParam?: string;
}

export const ASSIGN_UIDS_FIX: ViolationFix = {
  id: "assign-uids",
  label: "Generate unique ids for outputs missing them",
  // The informed warning, verbatim requirement: regenerating ids for the
  // same logical outputs makes them distinct elements again.
  warning:
    "automatically generating unique IDs may result in duplicate txouts if done more than once",
};

export function validateEditor(model: EditorModel): Violation[] {
  const violations: Violation[] = [];

  for (const section of model.sections) {
    for (const candidate of section.fields) {
      if (candidate.error) {
        violations.push({ path: candidate.path, message: candidate.error, fix: null });
      }
    }
  }

  const missingUids = model.sections
    .flatMap((section) => section.fields)
    .filter((candidate) => candidate.context === "uid" && !candidate.value && !candidate.error);
  if (model.ordering === "unordered" && missingUids.length > 0) {
    violations.push({
      path: null,
      message: `unordered PSBTs identify outputs by unique id; ${missingUids.length} output(s) have none`,
      fix: ASSIGN_UIDS_FIX,
    });
  }

  const sortMode = fieldAt(model, "global.sort_mode");
  const sortSeed = fieldAt(model, "global.sort_seed");
  if (sortMode?.value === "deterministic" && !sortSeed?.value) {
    violations.push({
      path: "global.sort_seed",
      message: "deterministic ordering requires a sort seed",
      fix: null,
    });
  }

  return violations;
}

// Map the server's save-time violations (/api/edit 400 body) into the
// editor's violation shape so they flow through the SAME
// violation -> fix -> revalidate loop as the local ones. fix_id/fix_label/
// warning_text become the fix offer (the warning stays attached verbatim);
// override_param rides along for the explicit-override affordance.
export function violationsFromServer(violations: EditViolation[]): Violation[] {
  return violations.map((violation) => ({
    path: null,
    message: violation.message,
    fix:
      violation.fix_id !== undefined
        ? {
            id: violation.fix_id,
            label: violation.fix_label ?? violation.fix_id,
            warning: violation.warning_text ?? "",
          }
        : null,
    source: "server" as const,
    overrideParam: violation.override_param,
  }));
}

// Apply an offered fix. Randomness is injected so the outcome is testable;
// the shell passes crypto.getRandomValues. Fixed fields carry a "generated"
// note so the user sees exactly what the fix touched.
export function applyFix(
  model: EditorModel,
  fixId: string,
  randomBytes: (length: number) => Uint8Array,
): EditorModel {
  if (fixId !== ASSIGN_UIDS_FIX.id) return model;
  return {
    ...model,
    sections: model.sections.map((section) => ({
      ...section,
      fields: section.fields.map((candidate) =>
        candidate.context === "uid" && !candidate.value
          ? // 16 bytes: the size ptj's own assign-ids mints.
            { ...candidate, value: bytesToHex(randomBytes(16)), error: null, note: "generated" }
          : candidate,
      ),
    })),
  };
}
