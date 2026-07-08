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
// spendable outputs) is the Backend.classifyPaste seam (/api/classify): the
// shell mints the shallow node instantly, requests the deep classification
// asynchronously, and folds the details back into the node
// (enrichDescriptor / enrichPayment / applyTxOutputs in ./wiring.ts).
// Consensus data is still never half-parsed in the frontend; a remaining
// deep-parse gap, if a kind has one, is named in `needsBackend` so the UI
// stays honest.
//
// Recognized today:
//   bitcoin: URI            -> payment instruction (BIP 21 / BIP 321)
//   output descriptor       -> descriptor object (own/other attribution)
//   npub1...                -> peer (nostr identity)
//   iroh document ticket    -> peer (iroh transport)
//   base64 or hex PSBT      -> fragment (handled by the shell: backend inspect)
//   fully-signed tx hex     -> transaction object (outputs decoded by the
//                              classify enrichment; chain sources stay manual)
import { bytesToBase64, pastedPsbt, } from "./state.js";
import { hexToBytes, normalizeHexInput } from "./encoding.js";
import { descriptorLooksPrivate, looksLikeDescriptor, parseBitcoinUri, } from "../model.js";
import { mintDescriptor, mintPayment, mintPeer, mintUtxo, } from "./wiring.js";
// Real extended private keys are "xprv"/"tprv"/… immediately followed by
// base58 material, so the demo's word-boundary helper
// (model.js descriptorLooksPrivate, \bxprv\b) cannot match them — it only
// catches the prefixes as standalone words. Check the real shape first and
// keep the demo helper as a fallback for its own historical inputs.
const PRIVATE_KEY_MATERIAL = /\b[xtyzuv]prv[1-9A-HJ-NP-Za-km-z]{20,}/;
function descriptorIsPrivate(text) {
    return PRIVATE_KEY_MATERIAL.test(text) || descriptorLooksPrivate(text);
}
const NPUB_PATTERN = /\bnpub1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{6,}\b/i;
// iroh document tickets are base32ish blobs with a "doc" prefix (mirrors the
// demo's paste router).
const IROH_TICKET_PATTERN = /\bdoc[a-z2-7]{40,}\b/i;
function transactionVersioned(bytes) {
    if (bytes.length < 60)
        return false; // smaller than any real signed tx
    const version = bytes[0] | (bytes[1] << 8) | (bytes[2] << 16) | (bytes[3] << 24);
    return version >= 1 && version <= 3;
}
export function classifyPaste(text) {
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
            needsBackend: null,
        };
    }
    if (looksLikeDescriptor(trimmed)) {
        const isPrivate = descriptorIsPrivate(trimmed);
        return {
            kind: "descriptor",
            payload: trimmed,
            detail: `${isPrivate ? "private" : "public"} output descriptor`,
            needsBackend: null,
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
                needsBackend: null,
            };
        }
    }
    return {
        kind: "unknown",
        payload: trimmed,
        detail: "paste did not match a bitcoin: URI, output descriptor, npub, iroh ticket, PSBT (base64 or hex), or transaction hex",
        needsBackend: null,
    };
}
export function mintFromPaste(state, pasted) {
    switch (pasted.kind) {
        case "payment-uri": {
            const uri = parseBitcoinUri(pasted.payload);
            if (!uri)
                return { state, minted: null, log: "payment URI unexpectedly unparsable" };
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
                log: `minted ${minted.utxo.key} from a signed transaction (outputs decode via classifyPaste)`,
            };
        }
        default:
            return { state, minted: null, log: pasted.detail };
    }
}
