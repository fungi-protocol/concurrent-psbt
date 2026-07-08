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
// - Decoded convenience fields (amount, txid, ordering, ...) still validate
//   locally only: translating them into raw key/value bytes is a backend
//   concern (a typed-edit request shape /api/edit does not take yet), so
//   their edits do NOT travel on save — the shell says so explicitly.
// - Edits NEVER mutate the source fragment: a saved edit mints a NEW
//   fragment (grow-only, like every other operation result).
import { asArray, asNumber, asObject, asString } from "./state.js";
import { bytesToHex, parseFlexible } from "./encoding.js";
// The Backend seam method the save path drives (the field-edit route:
// psbt + raw-keymap edits -> validated, re-encoded psbt). Named here so the
// shell and the report speak the same name.
export const EDIT_SAVE_SEAM = "applyPsbtEdits";
function field(path, label, value, context) {
    return { path, label, value, context, error: null, note: null };
}
// Build the editable model from inspect JSON. Missing pieces degrade to
// empty editable values (the whole point is repairing incomplete fragments,
// e.g. an atomized BIP 174 import without output unique ids).
export function editorModel(fragmentKey, inspect, network) {
    const root = asObject(inspect);
    const sort = asObject(root?.sort);
    const modifiability = asObject(root?.modifiability);
    const flags = asNumber(modifiability?.flags);
    const globalSection = {
        key: "global",
        title: "Global",
        fields: [
            field("global.flags", "tx-modifiable flags", flags === null ? "" : String(flags), "flags"),
            field("global.ordering", "ordering", asString(root?.ordering) ?? "", "ordering"),
            field("global.sort_mode", "sort mode", asString(sort?.mode) ?? "", "sort-mode"),
            field("global.sort_seed", "sort seed", asString(sort?.seed_hex) ?? "", "hex"),
        ],
    };
    const inputs = asArray(root?.inputs) ?? [];
    const inputSections = inputs.map((raw, index) => {
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
    const outputSections = outputs.map((raw, index) => {
        const output = asObject(raw);
        const amount = asNumber(output?.amount_sats);
        return {
            key: `output.${index}`,
            title: `Output ${index}`,
            fields: [
                field(`output.${index}.amount`, "amount (sats)", amount === null ? "" : String(amount), "integer"),
                field(`output.${index}.script`, "scriptPubKey", asString(output?.script_pubkey_hex) ?? "", "script"),
                field(`output.${index}.unique_id`, "unique id", asString(output?.unique_id_hex) ?? "", "hex32-optional"),
            ],
        };
    });
    return {
        fragmentKey,
        network,
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
export function isRawPath(path) {
    return path.startsWith(RAW_SECTION_PREFIX);
}
function rawSections(root) {
    const raw = asObject(root?.raw);
    if (!raw)
        return [];
    const sections = [];
    const global = rawSection("raw.global", "Global — raw keymap", asArray(raw.global));
    if (global)
        sections.push(global);
    (asArray(raw.inputs) ?? []).forEach((entries, index) => {
        const section = rawSection(`raw.input.${index}`, `Input ${index} — raw keymap`, asArray(entries));
        if (section)
            sections.push(section);
    });
    (asArray(raw.outputs) ?? []).forEach((entries, index) => {
        const section = rawSection(`raw.output.${index}`, `Output ${index} — raw keymap`, asArray(entries));
        if (section)
            sections.push(section);
    });
    return sections;
}
function rawSection(key, title, entries) {
    const fields = [];
    for (const raw of entries ?? []) {
        const entry = asObject(raw);
        const keyHex = asString(entry?.key_hex);
        if (!keyHex)
            continue;
        const kind = asString(entry?.kind) ?? "unknown";
        if (kind === "known")
            continue; // already shown as a decoded field
        const row = field(`${key}.${keyHex}`, rawLabel(entry), asString(entry?.value_hex) ?? "", "hex");
        row.note = "clearing the value deletes the entry on save";
        fields.push(row);
    }
    if (!fields.length)
        return null;
    return { key, title, fields };
}
function rawLabel(entry) {
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
export function rawEditsForSave(pristine, edited) {
    const edits = [];
    for (const section of edited.sections) {
        if (!isRawPath(section.key))
            continue;
        for (const candidate of section.fields) {
            if (candidate.error)
                continue;
            const before = fieldAt(pristine, candidate.path);
            if (before && before.value === candidate.value)
                continue;
            edits.push({
                map: mapSelector(section.key),
                key: candidate.path.slice(section.key.length + 1),
                value: candidate.value ? candidate.value : null,
            });
        }
    }
    return edits;
}
// "raw.global" -> "global"; "raw.input.3" -> "input:3"; "raw.output.0" -> "output:0".
function mapSelector(sectionKey) {
    const rest = sectionKey.slice(RAW_SECTION_PREFIX.length);
    const dot = rest.indexOf(".");
    return dot === -1 ? rest : `${rest.slice(0, dot)}:${rest.slice(dot + 1)}`;
}
// Decoded (non-raw) fields whose value changed. These do NOT travel over the
// save seam — /api/edit takes raw-keymap edits only, and translating decoded
// values into raw bytes client-side would be exactly the re-encoding this
// frontend refuses to do. The shell names them so nothing is dropped
// silently.
export function decodedEditsLeftBehind(pristine, edited) {
    const paths = [];
    for (const section of edited.sections) {
        if (isRawPath(section.key))
            continue;
        for (const candidate of section.fields) {
            const before = fieldAt(pristine, candidate.path);
            if (before && before.value !== candidate.value) {
                paths.push(candidate.path);
            }
        }
    }
    return paths;
}
export function fieldAt(model, path) {
    for (const section of model.sections) {
        const found = section.fields.find((candidate) => candidate.path === path);
        if (found)
            return found;
    }
    return null;
}
const FLAG_NAMES = {
    none: "0",
    inputs: "1",
    outputs: "2",
    both: "3",
};
function canonicalize(text, context, network) {
    const trimmed = text.trim();
    switch (context) {
        case "flags": {
            const named = FLAG_NAMES[trimmed.toLowerCase()];
            if (named !== undefined)
                return { value: named, error: null, note: `named form of ${named}` };
            const parsed = parseUnsigned(trimmed);
            if (parsed === null) {
                return {
                    value: text,
                    error: "flags take 0-3, 0x0-0x3, or none/inputs/outputs/both",
                    note: null,
                };
            }
            if (parsed > 3) {
                return { value: text, error: "only bits 0 (inputs) and 1 (outputs) are defined", note: null };
            }
            return { value: String(parsed), error: null, note: null };
        }
        case "ordering": {
            const lower = trimmed.toLowerCase();
            if (lower === "ordered" || lower === "unordered")
                return { value: lower, error: null, note: null };
            return { value: text, error: "ordering is 'ordered' or 'unordered'", note: null };
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
        case "hex32-optional": {
            if (!trimmed && context !== "hex32")
                return { value: "", error: null, note: null };
            const parsed = parseFlexible(trimmed, context === "hex" ? "hex-bytes" : "hex-bytes-32");
            if (!parsed.ok)
                return { value: text, error: parsed.error, note: null };
            return { value: parsed.canonical, error: null, note: parsed.note ?? null };
        }
        case "integer": {
            const parsed = parseUnsigned(trimmed);
            if (parsed === null)
                return { value: text, error: "must be a non-negative integer", note: null };
            return { value: String(parsed), error: null, note: null };
        }
        case "u32": {
            if (!trimmed)
                return { value: "", error: null, note: null };
            const parsed = parseUnsigned(trimmed);
            if (parsed === null || parsed > 0xffffffff) {
                return { value: text, error: "sequence must fit in 32 bits (decimal or 0x hex)", note: null };
            }
            return { value: `0x${parsed.toString(16).padStart(8, "0")}`, error: null, note: null };
        }
        case "script": {
            const parsed = parseFlexible(trimmed, "script", network);
            if (!parsed.ok)
                return { value: text, error: parsed.error, note: null };
            return { value: parsed.canonical, error: null, note: parsed.note ?? null };
        }
    }
}
function parseUnsigned(text) {
    if (/^\d+$/.test(text))
        return Number(text);
    if (/^0x[0-9a-fA-F]+$/.test(text))
        return Number.parseInt(text.slice(2), 16);
    return null;
}
// Apply one edit: liberal-parse the raw text for the field's context and
// either canonicalize (with a provenance note for conversions) or record the
// error inline. Unknown paths are a no-op.
export function applyEdit(model, path, text) {
    return {
        ...model,
        sections: model.sections.map((section) => ({
            ...section,
            fields: section.fields.map((candidate) => candidate.path === path
                ? { ...candidate, ...canonicalize(text, candidate.context, model.network) }
                : candidate),
        })),
    };
}
export const ASSIGN_UIDS_FIX = {
    id: "assign-uids",
    label: "Generate unique ids for outputs missing them",
    // The informed warning, verbatim requirement: regenerating ids for the
    // same logical outputs makes them distinct elements again.
    warning: "automatically generating unique IDs may result in duplicate txouts if done more than once",
};
export function validateEditor(model) {
    const violations = [];
    for (const section of model.sections) {
        for (const candidate of section.fields) {
            if (candidate.error) {
                violations.push({ path: candidate.path, message: candidate.error, fix: null });
            }
        }
    }
    const ordering = fieldAt(model, "global.ordering");
    const missingUids = model.sections
        .flatMap((section) => section.fields)
        .filter((candidate) => candidate.context === "hex32-optional" && !candidate.value && !candidate.error);
    if (ordering?.value === "unordered" && missingUids.length > 0) {
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
export function violationsFromServer(violations) {
    return violations.map((violation) => ({
        path: null,
        message: violation.message,
        fix: violation.fix_id !== undefined
            ? {
                id: violation.fix_id,
                label: violation.fix_label ?? violation.fix_id,
                warning: violation.warning_text ?? "",
            }
            : null,
        source: "server",
        overrideParam: violation.override_param,
    }));
}
// Apply an offered fix. Randomness is injected so the outcome is testable;
// the shell passes crypto.getRandomValues. Fixed fields carry a "generated"
// note so the user sees exactly what the fix touched.
export function applyFix(model, fixId, randomBytes) {
    if (fixId !== ASSIGN_UIDS_FIX.id)
        return model;
    return {
        ...model,
        sections: model.sections.map((section) => ({
            ...section,
            fields: section.fields.map((candidate) => candidate.context === "hex32-optional" && !candidate.value
                ? { ...candidate, value: bytesToHex(randomBytes(32)), error: null, note: "generated" }
                : candidate),
        })),
    };
}
