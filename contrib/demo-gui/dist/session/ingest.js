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
export const SAMPLE_PASTES = [
    {
        // BIP 370 test vector: one signed-key input, two derived outputs.
        name: "BIP 370 PSBT (1 in / 2 out)",
        kind: "psbt",
        value: "cHNidP8BAgQCAAAAAQQBAQEFAQIB+wQCAAAAAAEAUgIAAAABwaolbiFLlqGCL5PeQr/ztfP/jQUZMG41FddRWl6AWxIAAAAAAP////8BGMaaOwAAAAAWABSwo68UQghBJpPKfRZoUrUtsK7wbgAAAAABAR8Yxpo7AAAAABYAFLCjrxRCCEEmk8p9FmhStS2wrvBuAQ4gCwrZIUGcHIcZc11y3HOfnqngY40f5MHu8PmUQISBX8gBDwQAAAAAARAE/v///wAiAgLWAfhIRqZ1X3dr4A49nej7EKzJNfuDxF+wFi1MrVq3khj2nYc+VAAAgAEAAIAAAACAAAAAACoAAAABAwgACK8vAAAAAAEEFgAUxDD2TEdW2jENvRoIVXLvKZkmJywAIgIC42+/9T3VNAcM+P05ZhRoDzV6m4Xbc0C/HPp0XSrXs0AY9p2HPlQAAIABAACAAAAAgAEAAABkAAAAAQMIi73rCwAAAAABBBYAFE3Rk6yWSlasG54cyoRU/i9HT4UTAA==",
    },
    {
        // Backend-minted (POST /api/create, ordering=unset, no inputs or
        // outputs): the smallest PSBT ptj itself considers unordered and
        // fully modifiable.
        name: "Minimal unordered PSBT",
        kind: "psbt",
        value: "cHNidP8B+wQCAAAAAQIEAgAAAAEEAQABBQEAAQYBAxL8D2NvbmN1cnJlbnQtcHNidBABAwA=",
    },
    {
        // Backend-minted (POST /api/create, ordering=unset) paying 0.001 BTC
        // to the BIP 370 vector's first output address, so joining it with
        // the vector above exercises a real merge.
        name: "Unordered payment PSBT (1 out)",
        kind: "psbt",
        value: "cHNidP8B+wQCAAAAAQIEAgAAAAEEAQABBQEBAQYBAxL8D2NvbmN1cnJlbnQtcHNidBABAwABAwighgEAAAAAAAEEFgAUxDD2TEdW2jENvRoIVXLvKZkmJywS/A9jb25jdXJyZW50LXBzYnQBENLh8ZqP4TAiSzRI7NmYVUAA",
    },
    {
        name: "Output descriptor (public)",
        kind: "descriptor",
        value: "wpkh(xpub6BosfCnifzxcFwrSzQiqu2DBVTshkCXacvNsWGYJVVhhawA7d4R5WSWGFNbi8Aw6ZRc1brxMyWMzG3DSSSSoekkudhUd9yLb6qx39T9nMdj/0/*)",
    },
    {
        name: "Payment URI (BIP 21)",
        kind: "payment-uri",
        // The address is the BIP 370 vector's first output script encoded
        // for regtest — a checksum-valid bech32 address the classifier's
        // payment-instructions parser accepts.
        value: "bitcoin:bcrt1qcsc0vnz82mdrzrdargy92uh09xvjvfev50zrk2?amount=0.001&label=lunch",
    },
    {
        name: "Peer identity (npub)",
        kind: "npub",
        value: "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg",
    },
    {
        name: "Iroh document ticket",
        kind: "iroh-ticket",
        value: `doc${"a".repeat(64)}`,
    },
    {
        // The funding transaction embedded in the BIP 370 vector above
        // (input 0's previous tx), so the two samples relate on screen.
        name: "Transaction hex",
        kind: "transaction-hex",
        value: "0200000001c1aa256e214b96a1822f93de42bff3b5f3ff8d0519306e3515d7515a5e805b120000000000ffffffff0118c69a3b00000000160014b0a3af144208412693ca7d166852b52db0aef06e00000000",
    },
];
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
