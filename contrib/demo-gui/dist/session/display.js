// contrib/demo-gui/src/session/display.ts
//
// Fragment card presenter — the structured, MOSTLY-ELIDED high-level view of
// a real PSBT fragment (the default card; the field editor in ./editor.ts is
// the low-level view underneath). Pure projections of `ptj inspect` JSON
// (node --test covered by test/session-display.test.mjs); the DOM shell only
// lays out what this module computes.
//
// Modeled on the demo sandbox's fragment presentation (src/model.ts
// unorderedPsbtDisplay / displaySections / displaySubtransactions): inputs
// and outputs GROUPED with per-group subtotals and a fee-balance line, BTC
// amounts through the same amountParts/formatSatAmount rendering. Grouping
// precedence:
//   (a) pseudo-descriptor PROVENANCE metadata (which peer this txin/txout
//       came from) where present — honest-setting only, see
//       contrib/design/pseudo-descriptors.md;
//   (b) script TEMPLATE derived from the output scriptPubKey (the real-
//       descriptor stand-in derivable from inspect data today);
//   (c) an explicit "unattributed" group otherwise.
// GAPS (inspect extensions these projections are waiting on): inspect JSON
// carries no per-input script/descriptor data and no provenance metadata, so
// (a) only activates when the caller supplies a provenance map from some
// future seam, and inputs can only group by provenance or fall through to
// unattributed.
//
// Digest-like values (txids, unique ids, scripts) are NOT shown as hex on
// the card: the shell renders them as LifeHash fingerprints (lazy <img>s on
// GET /api/lifehash/<hex>, see lifehashBadge in ./app.ts) with full
// bitvomit on expand — this module exposes the digest strings.
import { amountParts, formatSatAmount } from "../model.js";
import { addressFromScript, bytesToHex, hexToBytes, normalizeHexInput } from "./encoding.js";
import { asArray, asBoolean, asNumber, asObject, asString, fragmentSummary, } from "./state.js";
import { scriptFromAddress } from "./encoding.js";
const SCRIPT_LABEL = {
    p2pkh: "legacy (P2PKH)",
    p2sh: "script hash (P2SH)",
    p2wpkh: "segwit v0 (P2WPKH)",
    p2wsh: "segwit v0 (P2WSH)",
    p2tr: "taproot (P2TR)",
    witness: "witness program",
    unknown: "nonstandard script",
    absent: "no script",
};
export function scriptTemplate(scriptHex) {
    const kind = classifyScript(scriptHex);
    return { kind, label: SCRIPT_LABEL[kind] };
}
function classifyScript(scriptHex) {
    if (!scriptHex)
        return "absent";
    const script = scriptHex.toLowerCase();
    if (!/^(?:[0-9a-f]{2})+$/.test(script))
        return "unknown";
    if (/^76a914[0-9a-f]{40}88ac$/.test(script))
        return "p2pkh";
    if (/^a914[0-9a-f]{40}87$/.test(script))
        return "p2sh";
    if (/^0014[0-9a-f]{40}$/.test(script))
        return "p2wpkh";
    if (/^0020[0-9a-f]{64}$/.test(script))
        return "p2wsh";
    if (/^5120[0-9a-f]{64}$/.test(script))
        return "p2tr";
    // OP_1..OP_16 (0x51..0x60) followed by a single 2..40-byte push.
    const version = Number.parseInt(script.slice(0, 2), 16);
    if (version >= 0x51 && version <= 0x60 && script.length >= 8) {
        const pushLength = Number.parseInt(script.slice(2, 4), 16);
        if (pushLength >= 2 && pushLength <= 40 && script.length === 4 + pushLength * 2) {
            return "witness";
        }
    }
    return "unknown";
}
// Opcode names for the standard-script vocabulary; anything else renders as
// OP_0x<byte> — honest about the gap without pretending to be a complete
// disassembler. Pushes render as bare hex (bitcoind's "asm" convention).
const OPCODE_NAMES = {
    0x00: "OP_0",
    0x4f: "OP_1NEGATE",
    0x61: "OP_NOP",
    0x63: "OP_IF",
    0x64: "OP_NOTIF",
    0x67: "OP_ELSE",
    0x68: "OP_ENDIF",
    0x69: "OP_VERIFY",
    0x6a: "OP_RETURN",
    0x75: "OP_DROP",
    0x76: "OP_DUP",
    0x7c: "OP_SWAP",
    0x82: "OP_SIZE",
    0x87: "OP_EQUAL",
    0x88: "OP_EQUALVERIFY",
    0xa8: "OP_SHA256",
    0xa9: "OP_HASH160",
    0xaa: "OP_HASH256",
    0xac: "OP_CHECKSIG",
    0xad: "OP_CHECKSIGVERIFY",
    0xae: "OP_CHECKMULTISIG",
    0xaf: "OP_CHECKMULTISIGVERIFY",
    0xb1: "OP_CHECKLOCKTIMEVERIFY",
    0xb2: "OP_CHECKSEQUENCEVERIFY",
    0xba: "OP_CHECKSIGADD",
};
for (let n = 1; n <= 16; n++)
    OPCODE_NAMES[0x50 + n] = `OP_${n}`;
// Disassemble a scriptPubKey: opcode names and bare-hex pushes. A truncated
// push means the bytes are not a script — null, never a guess.
export function decodeScript(scriptHex) {
    const bytes = hexToBytes(normalizeHexInput(scriptHex));
    if (bytes === null || bytes.length === 0)
        return null;
    const parts = [];
    let at = 0;
    const take = (count) => {
        if (at + count > bytes.length)
            return null;
        const chunk = bytesToHex(bytes.slice(at, at + count));
        at += count;
        return chunk;
    };
    while (at < bytes.length) {
        const opcode = bytes[at++];
        if (opcode >= 0x01 && opcode <= 0x4b) {
            const chunk = take(opcode);
            if (chunk === null)
                return null;
            parts.push(chunk);
            continue;
        }
        if (opcode >= 0x4c && opcode <= 0x4e) {
            // OP_PUSHDATA1/2/4: little-endian length, then the payload.
            const width = opcode === 0x4c ? 1 : opcode === 0x4d ? 2 : 4;
            if (at + width > bytes.length)
                return null;
            let length = 0;
            for (let i = width - 1; i >= 0; i--)
                length = length * 256 + bytes[at + i];
            at += width;
            const chunk = take(length);
            if (chunk === null)
                return null;
            parts.push(chunk);
            continue;
        }
        parts.push(OPCODE_NAMES[opcode] ?? `OP_0x${opcode.toString(16).padStart(2, "0")}`);
    }
    return parts.join(" ");
}
// ---------------------------------------------------------------------------
// LifeHash chips for addresses/scripts — the CARD face never shows an
// address or script as text: a LifeHash fingerprint chip stands in, and the
// textual form survives only in the chip's title/aria-label, the expanded
// detail view, and the field editor.
// ---------------------------------------------------------------------------
//
// Chip contract: the digest input is the script_pubkey HEX (never the
// address string), so the same script fingerprints identically however it
// reached the card; GET /api/lifehash/<hex> accepts arbitrary-length data.
export const LIFEHASH_ROUTE = "/api/lifehash/";
export function lifehashSrc(digestHex) {
    return `${LIFEHASH_ROUTE}${digestHex}`;
}
// Digest for an address-carrying object (payment cards, utxo nodes): the
// script the address encodes. Strings that decode to no script (a lightning
// invoice or BOLT 12 offer riding a payment's address slot) return null —
// those keep their textual rendering, there is no script to fingerprint.
export function addressChipDigestHex(address) {
    if (!address)
        return null;
    return scriptFromAddress(address)?.scriptHex ?? null;
}
const FINAL_SIG_KEYTYPES = new Set(["07", "08"]);
const PARTIAL_SIG_KEYTYPES = new Set(["02", "13", "14"]);
export function signaturePresence(inspect, index) {
    const raw = asObject(asObject(inspect)?.raw);
    const entries = asArray(asArray(raw?.inputs)?.[index]) ?? [];
    let partial = false;
    for (const rawEntry of entries) {
        const keyHex = asString(asObject(rawEntry)?.key_hex);
        if (!keyHex || keyHex.length < 2)
            continue;
        const keytype = keyHex.slice(0, 2).toLowerCase();
        if (FINAL_SIG_KEYTYPES.has(keytype))
            return "final";
        if (PARTIAL_SIG_KEYTYPES.has(keytype))
            partial = true;
    }
    return partial ? "partial" : "unsigned";
}
// The prevout scriptPubKey from the raw per-input keymap. PSBT_IN_WITNESS_UTXO
// (keytype 0x01) is a serialized TxOut: 8-byte LE amount, compact-size script
// length, script bytes — decodable right here. A non-witness utxo would need
// the whole previous transaction parsed (a backend concern), so it yields null.
const WITNESS_UTXO_KEY_HEX = "01";
export function prevoutScriptHex(inspect, index) {
    const raw = asObject(asObject(inspect)?.raw);
    const entries = asArray(asArray(raw?.inputs)?.[index]) ?? [];
    for (const rawEntry of entries) {
        const object = asObject(rawEntry);
        if (asString(object?.key_hex)?.toLowerCase() !== WITNESS_UTXO_KEY_HEX)
            continue;
        const value = asString(object?.value_hex)?.toLowerCase();
        if (!value || !/^(?:[0-9a-f]{2})+$/.test(value) || value.length < 18)
            return null;
        // Skip the 8 amount bytes, read the compact-size script length.
        let cursor = 16;
        const marker = Number.parseInt(value.slice(cursor, cursor + 2), 16);
        cursor += 2;
        let length;
        if (marker < 0xfd) {
            length = marker;
        }
        else if (marker === 0xfd || marker === 0xfe) {
            const lengthBytes = marker === 0xfd ? 2 : 4;
            if (value.length < cursor + lengthBytes * 2)
                return null;
            // Little-endian length field.
            let parsed = 0;
            for (let i = lengthBytes - 1; i >= 0; i--) {
                parsed = parsed * 256 + Number.parseInt(value.slice(cursor + i * 2, cursor + i * 2 + 2), 16);
            }
            length = parsed;
            cursor += lengthBytes * 2;
        }
        else {
            return null; // 0xff (8-byte length) cannot be a real script
        }
        // The script must fill the TxOut exactly — anything else is malformed.
        if (value.length !== cursor + length * 2)
            return null;
        return length === 0 ? null : value.slice(cursor);
    }
    return null;
}
export function inputViews(inspect, provenance) {
    const inputs = asArray(asObject(inspect)?.inputs) ?? [];
    return inputs.map((raw, index) => {
        const input = asObject(raw);
        const outpointText = asString(input?.outpoint);
        const colon = outpointText?.lastIndexOf(":") ?? -1;
        const vout = outpointText && colon > 0 ? Number(outpointText.slice(colon + 1)) : null;
        return {
            index,
            outpointText,
            outpointTxid: outpointText && colon > 0 ? outpointText.slice(0, colon) : null,
            outpointVout: vout !== null && Number.isFinite(vout) ? vout : null,
            sequence: asString(input?.sequence),
            knownUtxoSats: asNumber(input?.known_utxo_sats),
            hasWitnessUtxo: asBoolean(input?.has_witness_utxo) ?? false,
            hasNonWitnessUtxo: asBoolean(input?.has_non_witness_utxo) ?? false,
            prevoutScriptHex: prevoutScriptHex(inspect, index),
            provenance: (outpointText && provenance?.inputs[outpointText]) || null,
            signatures: signaturePresence(inspect, index),
        };
    });
}
export function outputViews(inspect, network, provenance) {
    const outputs = asArray(asObject(inspect)?.outputs) ?? [];
    return outputs.map((raw, index) => {
        const output = asObject(raw);
        const scriptHex = asString(output?.script_pubkey_hex);
        const template = scriptTemplate(scriptHex);
        const uniqueIdHex = asString(output?.unique_id_hex);
        return {
            index,
            amountSats: asNumber(output?.amount_sats),
            address: scriptHex ? addressFromScript(scriptHex, network) : null,
            scriptHex,
            scriptKind: template.kind,
            scriptLabel: template.label,
            uniqueIdHex,
            provenance: (uniqueIdHex && provenance?.outputs[uniqueIdHex]) || null,
        };
    });
}
function amountSpanPart(part, text) {
    return { part, className: `session-amount-${part}`, text };
}
// Split a leading ₿ off into the symbol part (the analog of the demo's
// appendAmountPartTspans): amountParts rides the currency symbol inside
// `prefix` (≥ 1 BTC) or `muted` (< 1 BTC).
function pushAmountText(parts, text, part) {
    if (!text)
        return;
    if (text.startsWith("₿")) {
        parts.push(amountSpanPart("symbol", "₿"));
        text = text.slice(1);
    }
    if (text)
        parts.push(amountSpanPart(part, text));
}
// A thin space (U+2009) splits the eight fraction digits 4+4 — 1.2345 6789
// — enough of a seam to count by, too narrow to read as a separator
// character. Inserted AFTER the emphasis split, so the scaffold/significant
// classification never sees it; it lands inside whichever part holds the
// fourth fraction digit.
const FRACTION_SEAM = "\u2009";
function withFractionSeam(parts) {
    let fraction = -1; // fraction digits seen; -1 = still before the decimal point
    return parts.map((part) => {
        let text = "";
        for (const ch of part.text) {
            text += ch;
            if (ch === ".")
                fraction = 0;
            else if (fraction >= 0 && ch >= "0" && ch <= "9" && ++fraction === 4) {
                text += FRACTION_SEAM;
            }
        }
        return text === part.text ? part : { ...part, text };
    });
}
export function amountSpanParts(valueSats) {
    const raw = amountParts(valueSats);
    const parts = [];
    pushAmountText(parts, raw.prefix, "digits");
    if (raw.prefix) {
        // Nonzero whole-BTC part: every fraction digit is a sat digit
        // (8.00000000 IS 800,000,000 sats — the zeros ARE the sat integer).
        // amountParts rides the fraction's leading-zero run inside `muted`;
        // reclassify everything after the decimal point as significant.
        pushAmountText(parts, ".", "scale");
        pushAmountText(parts, raw.muted.slice(1) + raw.sats, "digits");
    }
    else {
        // Below 1 BTC: the "0." and the zeros before the first significant
        // digit stay scaffold; trailing zeros after it already ride in
        // `sats` (0.05000000 keeps its seven significant digits).
        pushAmountText(parts, raw.muted, "scale");
        pushAmountText(parts, raw.sats, "digits");
    }
    return withFractionSeam(parts);
}
// Signed variant for balance deltas: the sign is a significant digit (it
// inherits the surrounding color — deficit contexts render it red).
export function signedAmountSpanParts(valueSats) {
    const parts = amountSpanParts(Math.abs(valueSats));
    if (valueSats < 0)
        parts.unshift(amountSpanPart("digits", "−"));
    return parts;
}
// Binary fingerprint: the sat value in base 2, rendered as a thin barcode
// row directly under the decimal amount — LSB right-aligned under the last
// digit, natural bit length (no padding: the row length doubles as a log2
// magnitude cue). 1-bits draw as crisp marks and 0-bits as nearly invisible
// slots, so values with low Hamming weight in base two (round binary
// numbers) are recognizable at a glance by people who do not spot them in
// decimal. BigInt keeps every sat-range value exact (21M BTC ≈ 2^51 sats).
export function amountBits(valueSats) {
    const sats = Math.trunc(Math.abs(Number(valueSats || 0)));
    return BigInt(sats).toString(2);
}
function groupSlot(key, label, kind) {
    return {
        group: { key, label, kind, inputs: [], outputs: [], inputSubtotalSats: 0, outputSubtotalSats: 0 },
        inputComplete: true,
        outputComplete: true,
    };
}
export function cardGroups(inputs, outputs, dimension = "provenance") {
    const provenance = new Map();
    const templates = new Map();
    let unattributed = null;
    const slotFor = (view, templateKind, templateLabel) => {
        if (view.provenance) {
            const key = `peer:${view.provenance}`;
            let slot = provenance.get(key);
            if (!slot) {
                slot = groupSlot(key, `from ${view.provenance}`, "provenance");
                provenance.set(key, slot);
            }
            return slot;
        }
        if (dimension === "provenance+script-template" && templateKind && templateKind !== "unknown" && templateKind !== "absent") {
            const key = `template:${templateKind}`;
            let slot = templates.get(key);
            if (!slot) {
                slot = groupSlot(key, templateLabel ?? templateKind, "script-template");
                templates.set(key, slot);
            }
            return slot;
        }
        if (!unattributed) {
            unattributed = groupSlot("unattributed", "unattributed", "unattributed");
        }
        return unattributed;
    };
    for (const input of inputs) {
        // Inputs carry no script data in inspect JSON (documented gap), so they
        // group by provenance or fall through to unattributed.
        const slot = slotFor(input);
        slot.group.inputs.push(input);
        if (input.knownUtxoSats === null) {
            slot.inputComplete = false;
        }
        else if (slot.group.inputSubtotalSats !== null) {
            slot.group.inputSubtotalSats += input.knownUtxoSats;
        }
    }
    for (const output of outputs) {
        const slot = slotFor(output, output.scriptKind, output.scriptLabel);
        slot.group.outputs.push(output);
        if (output.amountSats === null) {
            slot.outputComplete = false;
        }
        else if (slot.group.outputSubtotalSats !== null) {
            slot.group.outputSubtotalSats += output.amountSats;
        }
    }
    const slots = [...provenance.values(), ...templates.values(), ...(unattributed ? [unattributed] : [])];
    for (const slot of slots) {
        if (!slot.inputComplete)
            slot.group.inputSubtotalSats = null;
        if (!slot.outputComplete)
            slot.group.outputSubtotalSats = null;
        if (slot.group.inputs.length === 0)
            slot.group.inputSubtotalSats = null;
        if (slot.group.outputs.length === 0)
            slot.group.outputSubtotalSats = null;
    }
    return slots.map((slot) => slot.group);
}
// A group header wears the group's script fingerprint when the group has ONE
// concrete script identity (every output shares a script_pubkey — the
// pseudo-descriptor/template resolves to a single script). Mixed-script
// groups (a template bucket over different addresses) show no chip: one
// fingerprint would misattribute the rows under it.
export function groupChipDigestHex(group) {
    let digest = null;
    for (const output of group.outputs) {
        if (!output.scriptHex)
            return null;
        if (digest === null)
            digest = output.scriptHex;
        else if (digest !== output.scriptHex)
            return null;
    }
    return digest;
}
// The demo's formatSatAmount renders non-negative amounts; the fee delta is
// the one place a NEGATIVE number appears (outputs exceeding known inputs).
export function formatSignedSats(sats) {
    return sats < 0 ? `−${formatSatAmount(-sats)}` : formatSatAmount(sats);
}
export function feeLine(summary) {
    const { knownInputSats, outputSats, feeSats } = summary;
    let text;
    if (feeSats !== null && knownInputSats !== null && outputSats !== null) {
        // Same accounting the demo's fee-balance presentation shows: inputs in,
        // outputs out, the difference is the (implicit) fee.
        text = `${formatSatAmount(knownInputSats)} in − ${formatSatAmount(outputSats)} out = ${formatSignedSats(feeSats)} fee`;
        if (feeSats < 0) {
            text += " (deficit: outputs exceed known inputs)";
        }
    }
    else if (outputSats !== null) {
        text = `${formatSatAmount(outputSats)} out; fee unknown (input amounts incomplete)`;
    }
    else {
        text = "amounts unknown (not decoded)";
    }
    return { knownInputSats, outputSats, feeSats, text };
}
// ---------------------------------------------------------------------------
// Balance sheet — the card's balance-report footer: per-group subtotals and
// whole-transaction totals at the BOTTOM of the input/output columns, the
// demo's drawSectionSubtotal structure (sum line, accounting totals below
// it, the `balance:` delta on the shortfall side, declared fees above the
// line on the output side, the fee-rate signal).
// ---------------------------------------------------------------------------
// Seam readers: the inspect extension (ptj commands/inspect.rs) emits
// declared-fee and size data. Consume the fields when present, return null
// otherwise — the shell renders an honest "n/a" for null instead of
// inventing a number.
//   totals.declared_fee_sats — the summed PSBT_GLOBAL_EXPLICIT_FEE_-
//     CONTRIBUTION records (fee.rs total_declared_fee); null when none
//     decode (totals.declared_fee_undecoded_count reports how many entries
//     the total could not count);
//   totals.size — the whole-transaction size_totals object; its `vbytes`
//     (= ceil(weight / 4)) is the estimate consumed here. The earlier
//     guessed carriers stay tolerated: totals.size_estimate or top-level
//     size_estimate, as a bare number or an object carrying a vbytes field.
export function declaredFeeSatsFromInspect(inspect) {
    return asNumber(asObject(asObject(inspect)?.totals)?.declared_fee_sats);
}
export function sizeEstimateVbytesFromInspect(inspect) {
    const root = asObject(inspect);
    const totals = asObject(root?.totals);
    for (const carrier of [totals?.size, totals?.size_estimate, root?.size_estimate]) {
        const vbytes = asNumber(carrier) ?? asNumber(asObject(carrier)?.vbytes);
        if (vbytes !== null)
            return vbytes;
    }
    return null;
}
// The demo's formatFeeRate: two decimals below 10 sat/vB, one above.
export function formatFeeRate(rate) {
    const value = Number(rate || 0);
    return value >= 10 ? value.toFixed(1) : value.toFixed(2);
}
export function balanceSheet(summary, inspect) {
    const declaredFeeSats = declaredFeeSatsFromInspect(inspect);
    const sizeEstimateVbytes = sizeEstimateVbytesFromInspect(inspect);
    const feeSats = summary.feeSats;
    const delta = feeSats === null || feeSats === 0
        ? null
        : feeSats < 0
            ? { kind: "deficit", column: "input", sats: feeSats }
            : { kind: "surplus", column: "output", sats: feeSats };
    const showFeeRate = feeSats !== null && feeSats !== 0;
    return {
        inputTotalSats: summary.knownInputSats,
        outputTotalSats: summary.outputSats,
        declaredFeeSats,
        outputAccountingTotalSats: summary.outputSats === null ? null : summary.outputSats + (declaredFeeSats ?? 0),
        outputTotalElidedByDeclaredFees: summary.outputSats === 0 && declaredFeeSats !== null && declaredFeeSats > 0,
        feeSats,
        implicitFeeSats: feeSats !== null && declaredFeeSats !== null ? feeSats - declaredFeeSats : null,
        delta,
        sizeEstimateVbytes,
        feeRateText: showFeeRate && sizeEstimateVbytes !== null && sizeEstimateVbytes > 0
            ? `~${formatFeeRate(feeSats / sizeEstimateVbytes)} sat/vB`
            : null,
        showFeeRate,
        fallbackText: feeSats === null ? feeLine(summary).text : null,
        // Elide only when the totals are provably a repeat: the counts must be
        // KNOWN to be at most one line per side (unknown counts elide nothing),
        // and no declared fee may be folded in — with one the output accounting
        // total (outputs + fees) says more than the single output row.
        totalsRedundant: summary.inputCount !== null &&
            summary.inputCount <= 1 &&
            summary.outputCount !== null &&
            summary.outputCount <= 1 &&
            (declaredFeeSats ?? 0) === 0,
    };
}
export function fragmentCardModel(inspect, network, provenance, dimension = "provenance") {
    const summary = fragmentSummary(inspect);
    const inputs = inputViews(inspect, provenance);
    const outputs = outputViews(inspect, network, provenance);
    return {
        summary,
        inputs,
        outputs,
        groups: cardGroups(inputs, outputs, dimension),
        uidPresent: summary.outputUidPresent,
        uidTotal: outputs.length > 0 || summary.outputUidPresent !== null ? outputs.length : summary.outputCount,
        fee: feeLine(summary),
        balance: balanceSheet(summary, inspect),
    };
}
// Elision helper for the shell: show `shown` rows, elide the rest by count.
export function elisionLabel(shown, total) {
    return total > shown ? `+${total - shown} more` : null;
}
// An address-bearing fact is ONE fact with up to three representations:
// the address (base58/bech32, when the script encodes one), the raw script
// hex, and the decoded opcodes. First entry = what the fact shows by
// default; the chip fingerprints the script hex in every representation.
export function scriptCycle(scriptHex, network, prefix) {
    const entries = [];
    const address = addressFromScript(scriptHex, network);
    if (address)
        entries.push({ label: `${prefix}address`, value: address });
    entries.push({ label: `${prefix}script hex`, value: scriptHex });
    const asm = decodeScript(scriptHex);
    if (asm)
        entries.push({ label: `${prefix}script asm`, value: asm });
    return entries;
}
function detailValue(value) {
    if (value === null || value === undefined)
        return "—";
    if (typeof value === "string")
        return value;
    if (typeof value === "number" || typeof value === "boolean")
        return String(value);
    return JSON.stringify(value);
}
export function rowDetailPairs(inspect, side, index, network) {
    const root = asObject(inspect);
    const entries = asArray(side === "input" ? root?.inputs : root?.outputs) ?? [];
    const entry = asObject(entries[index]);
    const pairs = [];
    // The textual address the LifeHash chip stands for (outputs only: inspect
    // carries no per-input script data — the documented gap).
    if (side === "output") {
        const scriptHex = asString(entry?.script_pubkey_hex);
        const address = scriptHex ? addressFromScript(scriptHex, network) : null;
        if (address)
            pairs.push({ label: "address", value: address });
    }
    for (const [key, value] of Object.entries(entry ?? {})) {
        pairs.push({ label: key, value: detailValue(value) });
    }
    const raw = asObject(root?.raw);
    const maps = asArray(side === "input" ? raw?.inputs : raw?.outputs);
    for (const rawEntry of asArray(maps?.[index]) ?? []) {
        const object = asObject(rawEntry);
        const keyHex = asString(object?.key_hex);
        if (keyHex === null)
            continue;
        const kind = asString(object?.kind) ?? "unknown";
        pairs.push({
            label: `raw ${kind} ${keyHex}`,
            value: asString(object?.value_hex) ?? "",
        });
    }
    return pairs;
}
// Keytype → name tables per map kind (BIP 174 plus the BIP 370 additions).
// Keyed by the first byte of key_hex: every assigned keytype fits one byte
// of the compact-size varint, so a multi-byte keytype simply gets no name.
const GLOBAL_KEYTYPE_NAMES = {
    "00": "PSBT_GLOBAL_UNSIGNED_TX",
    "01": "PSBT_GLOBAL_XPUB",
    "02": "PSBT_GLOBAL_TX_VERSION",
    "03": "PSBT_GLOBAL_FALLBACK_LOCKTIME",
    "04": "PSBT_GLOBAL_INPUT_COUNT",
    "05": "PSBT_GLOBAL_OUTPUT_COUNT",
    "06": "PSBT_GLOBAL_TX_MODIFIABLE",
    fb: "PSBT_GLOBAL_VERSION",
    fc: "PSBT_GLOBAL_PROPRIETARY",
};
const INPUT_KEYTYPE_NAMES = {
    "00": "PSBT_IN_NON_WITNESS_UTXO",
    "01": "PSBT_IN_WITNESS_UTXO",
    "02": "PSBT_IN_PARTIAL_SIG",
    "03": "PSBT_IN_SIGHASH_TYPE",
    "04": "PSBT_IN_REDEEM_SCRIPT",
    "05": "PSBT_IN_WITNESS_SCRIPT",
    "06": "PSBT_IN_BIP32_DERIVATION",
    "07": "PSBT_IN_FINAL_SCRIPTSIG",
    "08": "PSBT_IN_FINAL_SCRIPTWITNESS",
    "09": "PSBT_IN_POR_COMMITMENT",
    "0a": "PSBT_IN_RIPEMD160",
    "0b": "PSBT_IN_SHA256",
    "0c": "PSBT_IN_HASH160",
    "0d": "PSBT_IN_HASH256",
    "0e": "PSBT_IN_PREVIOUS_TXID",
    "0f": "PSBT_IN_OUTPUT_INDEX",
    "10": "PSBT_IN_SEQUENCE",
    "11": "PSBT_IN_REQUIRED_TIME_LOCKTIME",
    "12": "PSBT_IN_REQUIRED_HEIGHT_LOCKTIME",
    "13": "PSBT_IN_TAP_KEY_SIG",
    "14": "PSBT_IN_TAP_SCRIPT_SIG",
    "15": "PSBT_IN_TAP_LEAF_SCRIPT",
    "16": "PSBT_IN_TAP_BIP32_DERIVATION",
    "17": "PSBT_IN_TAP_INTERNAL_KEY",
    "18": "PSBT_IN_TAP_MERKLE_ROOT",
    fc: "PSBT_IN_PROPRIETARY",
};
const OUTPUT_KEYTYPE_NAMES = {
    "00": "PSBT_OUT_REDEEM_SCRIPT",
    "01": "PSBT_OUT_WITNESS_SCRIPT",
    "02": "PSBT_OUT_BIP32_DERIVATION",
    "03": "PSBT_OUT_AMOUNT",
    "04": "PSBT_OUT_SCRIPT",
    "05": "PSBT_OUT_TAP_INTERNAL_KEY",
    "06": "PSBT_OUT_TAP_TREE",
    "07": "PSBT_OUT_TAP_BIP32_DERIVATION",
    fc: "PSBT_OUT_PROPRIETARY",
};
function rawKeymapEntries(map, names) {
    const entries = [];
    for (const rawEntry of asArray(map) ?? []) {
        const object = asObject(rawEntry);
        const keyHex = asString(object?.key_hex);
        if (keyHex === null)
            continue;
        const kind = asString(object?.kind) ?? "unknown";
        const proprietary = asObject(object?.proprietary);
        const prefix = asString(proprietary?.prefix_utf8) ?? asString(proprietary?.prefix_hex);
        const subtype = asNumber(proprietary?.subtype);
        entries.push({
            keyHex,
            valueHex: asString(object?.value_hex) ?? "",
            kind,
            // The name annotation follows the backend's own classification: an
            // entry the backend calls "unknown" gets no name even when its first
            // byte collides with a defined keytype (unexpected keydata, say) —
            // the annotation must never contradict the kind.
            name: kind === "proprietary" && prefix !== null
                ? `${prefix}#${subtype ?? "?"}`
                : kind === "known"
                    ? (names[keyHex.slice(0, 2).toLowerCase()] ?? null)
                    : null,
        });
    }
    return entries;
}
export function rawKeymapSections(inspect) {
    const raw = asObject(asObject(inspect)?.raw);
    if (!raw)
        return [];
    const sections = [
        { title: "global map", entries: rawKeymapEntries(raw.global, GLOBAL_KEYTYPE_NAMES) },
    ];
    (asArray(raw.inputs) ?? []).forEach((map, index) => {
        sections.push({ title: `input map ${index}`, entries: rawKeymapEntries(map, INPUT_KEYTYPE_NAMES) });
    });
    (asArray(raw.outputs) ?? []).forEach((map, index) => {
        sections.push({ title: `output map ${index}`, entries: rawKeymapEntries(map, OUTPUT_KEYTYPE_NAMES) });
    });
    return sections;
}
export const DETAIL_LEVELS = ["collapsed", "grouped", "expanded"];
export function groupAggregate(group) {
    return {
        inputCount: group.inputs.length,
        outputCount: group.outputs.length,
        inputSubtotalSats: group.inputSubtotalSats,
        outputSubtotalSats: group.outputSubtotalSats,
        signedInputCount: group.inputs.filter((input) => input.signatures !== "unsigned").length,
    };
}
// nSequence, read per BIP 68 and its neighbors: bit 31 disables the
// relative locktime; bit 22 picks 512-second granularity over blocks; the
// low 16 bits are the value. 0xffffffff additionally makes the input final
// (nLockTime loses force), and any value below 0xfffffffe signals BIP 125
// replaceability. The reading rides NEXT TO the hex, never instead of it —
// the raw field stays the authoritative fact.
export function sequenceReading(sequence) {
    if (!sequence)
        return null;
    const value = Number.parseInt(sequence, 16);
    if (!Number.isFinite(value) || value < 0 || value > 0xffff_ffff)
        return null;
    if (value === 0xffff_ffff)
        return "final — relative and absolute locktimes disabled";
    const rbf = value < 0xffff_fffe ? "; signals RBF (BIP 125)" : "";
    if (value >= 0x8000_0000)
        return `no relative locktime (BIP 68 disable bit)${rbf}`;
    const units = value & 0xffff;
    if (value & 0x0040_0000) {
        const seconds = units * 512;
        return `relative locktime ≥ ${units} × 512s (${approxDuration(seconds)})${rbf}`;
    }
    return `relative locktime ≥ ${units} block${units === 1 ? "" : "s"}${rbf}`;
}
function approxDuration(seconds) {
    if (seconds < 3600)
        return `≈${Math.round(seconds / 60)} min`;
    if (seconds < 48 * 3600)
        return `≈${(seconds / 3600).toFixed(1)} h`;
    return `≈${(seconds / 86400).toFixed(1)} days`;
}
// The level-3 facts for one row: a curated subset of rowDetailPairs — the
// textual identity behind the chips plus the row's structural facts. The
// exhaustive field-by-field projection stays in rowDetailPairs (the modal).
export function rowFacePairs(inspect, side, index, network) {
    const pairs = [];
    if (side === "input") {
        const [input] = inputViews(inspect).slice(index, index + 1);
        if (!input)
            return pairs;
        if (input.outpointText) {
            pairs.push({ label: "outpoint", value: input.outpointText, chipHex: input.outpointTxid });
        }
        // The prevout the input spends, when the PSBT carries it (witness utxo
        // today): who is paying, in the same address/type vocabulary as the
        // output facts. The chip rides NEXT TO the address here — the row face
        // hands the identity over when expanded — and clicking the value cycles
        // address | script hex | decoded opcodes. The amount stays on the row
        // face — no duplicate here.
        if (input.prevoutScriptHex) {
            const cycle = scriptCycle(input.prevoutScriptHex, network, "prevout ");
            pairs.push({ ...cycle[0], chipHex: input.prevoutScriptHex, cycle });
            pairs.push({ label: "prevout type", value: scriptTemplate(input.prevoutScriptHex).label });
        }
        if (input.sequence) {
            const reading = sequenceReading(input.sequence);
            pairs.push({
                label: "sequence",
                value: reading ? `${input.sequence} — ${reading}` : input.sequence,
            });
        }
        pairs.push({
            label: "utxo data",
            value: input.hasWitnessUtxo
                ? "witness utxo"
                : input.hasNonWitnessUtxo
                    ? "non-witness utxo"
                    : "none",
        });
        pairs.push({ label: "signatures", value: input.signatures });
        return pairs;
    }
    const [output] = outputViews(inspect, network).slice(index, index + 1);
    if (!output)
        return pairs;
    // Where the money goes, chip beside it, cycling address | script hex |
    // decoded opcodes — a non-encodable script simply starts at the hex.
    if (output.scriptHex) {
        const cycle = scriptCycle(output.scriptHex, network, "");
        pairs.push({ ...cycle[0], chipHex: output.scriptHex, cycle });
    }
    // scriptLabel is the TEMPLATE KIND (taproot, segwit v0…), not the script
    // bytes — "type" says what the value is; the bytes live in the modal.
    if (output.scriptKind !== "absent")
        pairs.push({ label: "type", value: output.scriptLabel });
    if (output.uniqueIdHex) {
        pairs.push({ label: "unique id", value: output.uniqueIdHex, chipHex: output.uniqueIdHex });
    }
    return pairs;
}
// The serialization format wears its BIP number on the card; inspect's
// internal names stay as the seam vocabulary.
const FORMAT_LABEL = {
    bip370: "BIP 370",
    bip174: "BIP 174",
};
export function fragmentBadges(card) {
    const { summary, uidPresent, uidTotal } = card;
    const badges = [];
    badges.push({
        emoji: null,
        text: summary.format === null ? "not decoded" : (FORMAT_LABEL[summary.format] ?? summary.format),
        tone: "neutral",
        title: "PSBT serialization format",
    });
    badges.push(summary.ordering === "unordered"
        ? {
            emoji: "🔀",
            text: "unordered",
            tone: "good",
            title: "unordered PSBT fragment: joinable before sorting",
        }
        : {
            emoji: null,
            text: summary.ordering ?? "ordering unknown",
            tone: "neutral",
            title: "ordering discipline",
        });
    if (summary.seedHex) {
        badges.push({
            emoji: "🌱",
            text: "seeded",
            tone: "neutral",
            title: "global deterministic sort seed set",
        });
    }
    if (summary.modifiableInputs === true || summary.modifiableOutputs === true) {
        const which = summary.modifiableInputs === true && summary.modifiableOutputs === true
            ? "both"
            : summary.modifiableInputs === true
                ? "inputs"
                : "outputs";
        badges.push({
            emoji: "✏️",
            text: `modifiable ${which}`,
            tone: "neutral",
            title: `BIP 370 modifiable ${which}`,
        });
    }
    if (uidTotal !== null) {
        const complete = uidPresent !== null && uidPresent >= uidTotal;
        badges.push({
            emoji: null,
            text: `ids ${uidPresent ?? "?"}/${uidTotal}`,
            tone: complete ? "good" : "warn",
            title: "outputs carrying PSBT_OUT_UNIQUE_ID",
        });
    }
    return badges;
}
