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
// the card: the shell renders them as LifeHash fingerprints (./lifehash.ts)
// with full bitvomit on expand — this module exposes the digest strings.

import type { InspectResponse } from "../shared-frontend/core/backend.js";
import { formatSatAmount } from "../model.js";
import { addressFromScript, type Network } from "./encoding.js";
import {
  asArray,
  asBoolean,
  asNumber,
  asObject,
  asString,
  fragmentSummary,
  type FragmentSummary,
} from "./state.js";
import { scriptFromAddress } from "./encoding.js";

export interface InputView {
  index: number;
  outpointText: string | null;
  outpointTxid: string | null;
  outpointVout: number | null;
  sequence: string | null;
  knownUtxoSats: number | null;
  hasWitnessUtxo: boolean;
  hasNonWitnessUtxo: boolean;
  provenance: string | null;
}

export interface OutputView {
  index: number;
  amountSats: number | null;
  address: string | null;
  scriptHex: string | null;
  scriptKind: ScriptKind;
  scriptLabel: string;
  uniqueIdHex: string | null;
  provenance: string | null;
}

export type ScriptKind =
  | "p2pkh"
  | "p2sh"
  | "p2wpkh"
  | "p2wsh"
  | "p2tr"
  | "witness"
  | "unknown"
  | "absent";

const SCRIPT_LABEL: Record<ScriptKind, string> = {
  p2pkh: "legacy (P2PKH)",
  p2sh: "script hash (P2SH)",
  p2wpkh: "segwit v0 (P2WPKH)",
  p2wsh: "segwit v0 (P2WSH)",
  p2tr: "taproot (P2TR)",
  witness: "witness program",
  unknown: "nonstandard script",
  absent: "no script",
};

export function scriptTemplate(scriptHex: string | null): { kind: ScriptKind; label: string } {
  const kind = classifyScript(scriptHex);
  return { kind, label: SCRIPT_LABEL[kind] };
}

function classifyScript(scriptHex: string | null): ScriptKind {
  if (!scriptHex) return "absent";
  const script = scriptHex.toLowerCase();
  if (!/^(?:[0-9a-f]{2})+$/.test(script)) return "unknown";
  if (/^76a914[0-9a-f]{40}88ac$/.test(script)) return "p2pkh";
  if (/^a914[0-9a-f]{40}87$/.test(script)) return "p2sh";
  if (/^0014[0-9a-f]{40}$/.test(script)) return "p2wpkh";
  if (/^0020[0-9a-f]{64}$/.test(script)) return "p2wsh";
  if (/^5120[0-9a-f]{64}$/.test(script)) return "p2tr";
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

export function lifehashSrc(digestHex: string): string {
  return `${LIFEHASH_ROUTE}${digestHex}`;
}

// Digest for an address-carrying object (payment cards, utxo nodes): the
// script the address encodes. Strings that decode to no script (a lightning
// invoice or BOLT 12 offer riding a payment's address slot) return null —
// those keep their textual rendering, there is no script to fingerprint.
export function addressChipDigestHex(address: string | null): string | null {
  if (!address) return null;
  return scriptFromAddress(address)?.scriptHex ?? null;
}

// Provenance from pseudo-descriptor metadata (not in inspect JSON yet):
// inputs keyed by outpoint text, outputs keyed by output unique id hex.
export interface ProvenanceMap {
  inputs: Record<string, string>;
  outputs: Record<string, string>;
}

export function inputViews(inspect: InspectResponse | null, provenance?: ProvenanceMap): InputView[] {
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
      provenance: (outpointText && provenance?.inputs[outpointText]) || null,
    };
  });
}

export function outputViews(
  inspect: InspectResponse | null,
  network: Network,
  provenance?: ProvenanceMap,
): OutputView[] {
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

// ---------------------------------------------------------------------------
// Grouping with per-group subtotals (BTC amounts) — the card's body.
// ---------------------------------------------------------------------------

export interface CardGroup {
  key: string;
  label: string;
  kind: "provenance" | "script-template" | "unattributed";
  inputs: InputView[];
  outputs: OutputView[];
  // Subtotals go null as soon as one member amount is unknown — a partial
  // sum rendered as a total would be a lie.
  inputSubtotalSats: number | null;
  outputSubtotalSats: number | null;
}

interface GroupSlot {
  group: CardGroup;
  inputComplete: boolean;
  outputComplete: boolean;
}

function groupSlot(key: string, label: string, kind: CardGroup["kind"]): GroupSlot {
  return {
    group: { key, label, kind, inputs: [], outputs: [], inputSubtotalSats: 0, outputSubtotalSats: 0 },
    inputComplete: true,
    outputComplete: true,
  };
}

export function cardGroups(inputs: InputView[], outputs: OutputView[]): CardGroup[] {
  const provenance = new Map<string, GroupSlot>();
  const templates = new Map<string, GroupSlot>();
  let unattributed: GroupSlot | null = null;

  const slotFor = (view: { provenance: string | null }, templateKind?: ScriptKind, templateLabel?: string): GroupSlot => {
    if (view.provenance) {
      const key = `peer:${view.provenance}`;
      let slot = provenance.get(key);
      if (!slot) {
        slot = groupSlot(key, `from ${view.provenance}`, "provenance");
        provenance.set(key, slot);
      }
      return slot;
    }
    if (templateKind && templateKind !== "unknown" && templateKind !== "absent") {
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
    } else if (slot.group.inputSubtotalSats !== null) {
      slot.group.inputSubtotalSats += input.knownUtxoSats;
    }
  }
  for (const output of outputs) {
    const slot = slotFor(output, output.scriptKind, output.scriptLabel);
    slot.group.outputs.push(output);
    if (output.amountSats === null) {
      slot.outputComplete = false;
    } else if (slot.group.outputSubtotalSats !== null) {
      slot.group.outputSubtotalSats += output.amountSats;
    }
  }

  const slots = [...provenance.values(), ...templates.values(), ...(unattributed ? [unattributed] : [])];
  for (const slot of slots) {
    if (!slot.inputComplete) slot.group.inputSubtotalSats = null;
    if (!slot.outputComplete) slot.group.outputSubtotalSats = null;
    if (slot.group.inputs.length === 0) slot.group.inputSubtotalSats = null;
    if (slot.group.outputs.length === 0) slot.group.outputSubtotalSats = null;
  }
  return slots.map((slot) => slot.group);
}

// A group header wears the group's script fingerprint when the group has ONE
// concrete script identity (every output shares a script_pubkey — the
// pseudo-descriptor/template resolves to a single script). Mixed-script
// groups (a template bucket over different addresses) show no chip: one
// fingerprint would misattribute the rows under it.
export function groupChipDigestHex(group: Pick<CardGroup, "outputs">): string | null {
  let digest: string | null = null;
  for (const output of group.outputs) {
    if (!output.scriptHex) return null;
    if (digest === null) digest = output.scriptHex;
    else if (digest !== output.scriptHex) return null;
  }
  return digest;
}

// ---------------------------------------------------------------------------
// The whole card model.
// ---------------------------------------------------------------------------

export interface FeeLine {
  knownInputSats: number | null;
  outputSats: number | null;
  feeSats: number | null;
  text: string;
}

// The demo's formatSatAmount renders non-negative amounts; the fee delta is
// the one place a NEGATIVE number appears (outputs exceeding known inputs).
export function formatSignedSats(sats: number): string {
  return sats < 0 ? `−${formatSatAmount(-sats)}` : formatSatAmount(sats);
}

export function feeLine(summary: FragmentSummary): FeeLine {
  const { knownInputSats, outputSats, feeSats } = summary;
  let text: string;
  if (feeSats !== null && knownInputSats !== null && outputSats !== null) {
    // Same accounting the demo's fee-balance presentation shows: inputs in,
    // outputs out, the difference is the (implicit) fee.
    text = `${formatSatAmount(knownInputSats)} in − ${formatSatAmount(outputSats)} out = ${formatSignedSats(feeSats)} fee`;
    if (feeSats < 0) {
      text += " (deficit: outputs exceed known inputs)";
    }
  } else if (outputSats !== null) {
    text = `${formatSatAmount(outputSats)} out; fee unknown (input amounts incomplete)`;
  } else {
    text = "amounts unknown (not decoded)";
  }
  return { knownInputSats, outputSats, feeSats, text };
}

export interface FragmentCardModel {
  summary: FragmentSummary;
  inputs: InputView[];
  outputs: OutputView[];
  groups: CardGroup[];
  uidPresent: number | null;
  uidTotal: number | null;
  fee: FeeLine;
}

export function fragmentCardModel(
  inspect: InspectResponse | null,
  network: Network,
  provenance?: ProvenanceMap,
): FragmentCardModel {
  const summary = fragmentSummary(inspect);
  const inputs = inputViews(inspect, provenance);
  const outputs = outputViews(inspect, network, provenance);
  return {
    summary,
    inputs,
    outputs,
    groups: cardGroups(inputs, outputs),
    uidPresent: summary.outputUidPresent,
    uidTotal: outputs.length > 0 || summary.outputUidPresent !== null ? outputs.length : summary.outputCount,
    fee: feeLine(summary),
  };
}

// Elision helper for the shell: show `shown` rows, elide the rest by count.
export function elisionLabel(shown: number, total: number): string | null {
  return total > shown ? `+${total - shown} more` : null;
}
