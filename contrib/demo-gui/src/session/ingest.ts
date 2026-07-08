// contrib/demo-gui/src/session/ingest.ts
//
// Universal paste ingestion — SHALLOW classification of whatever the user
// pastes, by character set + structure, minting the right node kind in the
// session object graph with the raw payload retained. Mirrors the demo
// sandbox's paste router (src/app.ts handlePastedCandidate) but for REAL
// objects.
//
// Shallow means: enough syntax to pick a node kind and a card label. DEEP
// parsing (descriptor validation and script derivation via miniscript, BIP
// 321 validation via bitcoin-payment-instructions, transaction decode into
// spendable outputs) is a BACKEND seam — Backend.classifyPaste(payload) ->
// {kind, details} — which does not exist yet; every classification names
// what it is waiting on in `needsBackend` so the UI stays honest instead of
// half-parsing consensus data in the frontend.
//
// Recognized today:
//   bitcoin: URI            -> payment instruction (BIP 21 / BIP 321)
//   output descriptor       -> descriptor object (own/other attribution)
//   npub1...                -> peer (nostr identity)
//   iroh document ticket    -> peer (iroh transport)
//   base64 or hex PSBT      -> fragment (handled by the shell: backend inspect)
//   fully-signed tx hex     -> transaction object (spendable outputs pending
//                              backend decode; chain sources stay manual)

import {
  bytesToBase64,
  pastedPsbt,
} from "./state.js";
import { hexToBytes, normalizeHexInput } from "./encoding.js";
import {
  descriptorLooksPrivate,
  looksLikeDescriptor,
  parseBitcoinUri,
} from "../model.js";
import {
  mintDescriptor,
  mintPayment,
  mintPeer,
  mintUtxo,
  type NodeRef,
  type ObjectsState,
} from "./wiring.js";

export type PasteKind =
  | "psbt"
  | "payment-uri"
  | "descriptor"
  | "npub"
  | "iroh-ticket"
  | "transaction-hex"
  | "unknown";

export interface PasteClassification {
  kind: PasteKind;
  // The payload to retain: canonical base64 for PSBTs (hex pastes are
  // converted), the matched token for identifiers, trimmed text otherwise.
  payload: string;
  // One human-readable line for the event log / card subtitle.
  detail: string;
  // The missing deep-parse seam this object is waiting on, if any.
  needsBackend: string | null;
}

// Real extended private keys are "xprv"/"tprv"/… immediately followed by
// base58 material, so the demo's word-boundary helper
// (model.js descriptorLooksPrivate, \bxprv\b) cannot match them — it only
// catches the prefixes as standalone words. Check the real shape first and
// keep the demo helper as a fallback for its own historical inputs.
const PRIVATE_KEY_MATERIAL = /\b[xtyzuv]prv[1-9A-HJ-NP-Za-km-z]{20,}/;

function descriptorIsPrivate(text: string): boolean {
  return PRIVATE_KEY_MATERIAL.test(text) || descriptorLooksPrivate(text);
}

const NPUB_PATTERN = /\bnpub1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{6,}\b/i;
// iroh document tickets are base32ish blobs with a "doc" prefix (mirrors the
// demo's paste router).
const IROH_TICKET_PATTERN = /\bdoc[a-z2-7]{40,}\b/i;

function transactionVersioned(bytes: Uint8Array): boolean {
  if (bytes.length < 60) return false; // smaller than any real signed tx
  const version = bytes[0] | (bytes[1] << 8) | (bytes[2] << 16) | (bytes[3] << 24);
  return version >= 1 && version <= 3;
}

export function classifyPaste(text: string): PasteClassification {
  const trimmed = String(text || "").trim();
  if (!trimmed) {
    return { kind: "unknown", payload: "", detail: "empty paste", needsBackend: null };
  }

  const uri = parseBitcoinUri(trimmed);
  if (uri) {
    return {
      kind: "payment-uri",
      payload: uri.uri,
      detail: `payment instruction: ${uri.address} (${uri.valueSats} sats)`,
      needsBackend: "classifyPaste (bitcoin-payment-instructions) for full BIP 321 validation",
    };
  }

  if (looksLikeDescriptor(trimmed)) {
    const isPrivate = descriptorIsPrivate(trimmed);
    return {
      kind: "descriptor",
      payload: trimmed,
      detail: `${isPrivate ? "private" : "public"} output descriptor`,
      needsBackend: "classifyPaste (miniscript) for descriptor validation and script derivation",
    };
  }

  const npub = trimmed.match(NPUB_PATTERN)?.[0];
  if (npub) {
    return {
      kind: "npub",
      payload: npub.toLowerCase(),
      detail: "nostr peer identity (npub)",
      needsBackend: null,
    };
  }

  const ticket = trimmed.match(IROH_TICKET_PATTERN)?.[0];
  if (ticket) {
    return {
      kind: "iroh-ticket",
      payload: ticket,
      detail: "iroh document ticket",
      needsBackend: null,
    };
  }

  const psbt = pastedPsbt(trimmed);
  if (psbt) {
    return { kind: "psbt", payload: psbt, detail: "base64 PSBT", needsBackend: null };
  }

  const hex = normalizeHexInput(trimmed);
  const bytes = hex ? hexToBytes(hex) : null;
  if (bytes) {
    // BIP 174/370 magic "psbt\xff" — a hex PSBT paste converts to the
    // canonical base64 the whole seam speaks.
    if (hex.startsWith("70736274ff")) {
      return {
        kind: "psbt",
        payload: bytesToBase64(bytes),
        detail: "hex PSBT (converted to base64)",
        needsBackend: null,
      };
    }
    if (transactionVersioned(bytes)) {
      return {
        kind: "transaction-hex",
        payload: hex,
        detail: `transaction hex (${bytes.length} bytes)`,
        needsBackend: "classifyPaste (tx decode) to list this transaction's spendable outputs",
      };
    }
  }

  return {
    kind: "unknown",
    payload: trimmed,
    detail:
      "paste did not match a bitcoin: URI, output descriptor, npub, iroh ticket, PSBT (base64 or hex), or transaction hex",
    needsBackend: null,
  };
}

// Route a classification into the object graph. PSBTs are NOT minted here:
// fragments are owned by the fragment set (the shell inspects them through
// the backend first). Returns the minted node so the shell can focus/log it.
export interface MintResult {
  state: ObjectsState;
  minted: NodeRef | null;
  log: string;
}

export function mintFromPaste(state: ObjectsState, pasted: PasteClassification): MintResult {
  switch (pasted.kind) {
    case "payment-uri": {
      const uri = parseBitcoinUri(pasted.payload);
      if (!uri) return { state, minted: null, log: "payment URI unexpectedly unparsable" };
      const minted = mintPayment(state, uri.uri, uri.address, uri.valueSats, uri.label);
      return {
        state: minted.state,
        minted: { kind: "payment", key: minted.payment.key },
        log: `minted ${minted.payment.key} from a payment URI (${uri.address})`,
      };
    }
    case "descriptor": {
      const isPrivate = descriptorIsPrivate(pasted.payload);
      const minted = mintDescriptor(state, pasted.payload, isPrivate);
      return {
        state: minted.state,
        minted: { kind: "descriptor", key: minted.descriptor.key },
        log: `minted ${minted.descriptor.key} (${isPrivate ? "private" : "public"} descriptor)`,
      };
    }
    case "npub": {
      const minted = mintPeer(state, pasted.payload.slice(0, 12), "nostr", pasted.payload);
      return {
        state: minted.state,
        minted: { kind: "peer", key: minted.peer.key },
        log: `minted ${minted.peer.key} from an npub`,
      };
    }
    case "iroh-ticket": {
      const minted = mintPeer(state, pasted.payload.slice(0, 12), "iroh", pasted.payload);
      return {
        state: minted.state,
        minted: { kind: "peer", key: minted.peer.key },
        log: `minted ${minted.peer.key} from an iroh ticket`,
      };
    }
    case "transaction-hex": {
      const minted = mintUtxo(state, pasted.payload);
      return {
        state: minted.state,
        minted: { kind: "utxo", key: minted.utxo.key },
        log: `minted ${minted.utxo.key} from a signed transaction (outputs pending backend decode)`,
      };
    }
    default:
      return { state, minted: null, log: pasted.detail };
  }
}
