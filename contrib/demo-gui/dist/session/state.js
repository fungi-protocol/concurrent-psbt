// contrib/demo-gui/src/session/state.ts
//
// Session UI presenter — the PURE model behind the real webgui session page
// (src/session/app.ts is the thin DOM shell). Everything here is data-in /
// data-out: fragment-set bookkeeping over REAL loaded PSBTs, defensive views
// over `ptj inspect` JSON, and request builders for the Backend seam. No DOM,
// no fetch, no Backend calls — which is what makes it node --test coverable
// (test/session.test.mjs).
//
// Strictly typed (tsconfig.model.json), like src/model.ts; the reusable
// validation helpers (hex/base64/ordering) come from there instead of being
// reinvented.
import { compactBase64, looksLikeBase64Psbt, normalizeSessionOrdering, } from "../model.js";
export function emptySession() {
    return { fragments: [], counter: 0 };
}
export function addFragment(state, psbt, inspect, origin) {
    const compact = compactBase64(psbt);
    // Unordered PSBTs reserialize in a shuffled map order by design (psbt.md),
    // so byte equality is only a fast path. The canonical identity is the
    // unordered unique id: an id match is the same PSBT, possibly carrying more
    // data (the id commits to the input/output sets, not to their fields), so
    // the surviving card absorbs the incoming value instead of duplicating.
    const incomingId = fragmentSummary(inspect).uniqueIdHex;
    const existing = state.fragments.find((fragment) => fragment.psbt === compact ||
        (incomingId !== null && fragmentSummary(fragment.inspect).uniqueIdHex === incomingId));
    if (existing) {
        const absorbed = existing.psbt === compact ? existing : { ...existing, psbt: compact, inspect };
        const fragments = state.fragments.map((fragment) => fragment.key === existing.key ? { ...absorbed, selected: true } : fragment);
        return {
            state: { fragments, counter: state.counter },
            fragment: fragments.find((fragment) => fragment.key === existing.key),
            duplicate: true,
        };
    }
    const counter = state.counter + 1;
    const fragment = {
        key: `psbt-${counter}`,
        psbt: compact,
        inspect,
        origin,
        selected: false,
    };
    return {
        state: { fragments: [...state.fragments, fragment], counter },
        fragment,
        duplicate: false,
    };
}
export function removeFragment(state, key) {
    return {
        fragments: state.fragments.filter((fragment) => fragment.key !== key),
        counter: state.counter,
    };
}
export function setSelected(state, key, selected) {
    return {
        fragments: state.fragments.map((fragment) => fragment.key === key ? { ...fragment, selected } : fragment),
        counter: state.counter,
    };
}
export function selectedFragments(state) {
    return state.fragments.filter((fragment) => fragment.selected);
}
// ---------------------------------------------------------------------------
// Inspect views: defensive projections of `ptj inspect` JSON (an open object
// on the seam) into what the fragment list and negotiation panel display.
// ---------------------------------------------------------------------------
// Exported: the sibling presenter modules (display/editor/wiring) project the
// same open inspect JSON and must share ONE set of defensive readers instead
// of each reinventing subtly different ones.
export function asObject(value) {
    return typeof value === "object" && value !== null && !Array.isArray(value)
        ? value
        : null;
}
export function asString(value) {
    return typeof value === "string" ? value : null;
}
export function asNumber(value) {
    return typeof value === "number" && Number.isFinite(value) ? value : null;
}
export function asBoolean(value) {
    return typeof value === "boolean" ? value : null;
}
export function asArray(value) {
    return Array.isArray(value) ? value : null;
}
export function fragmentSummary(inspect) {
    const root = asObject(inspect);
    const sort = asObject(root?.sort);
    const totals = asObject(root?.totals);
    const modifiability = asObject(root?.modifiability);
    const outputs = asArray(root?.outputs);
    return {
        format: asString(root?.format),
        ordering: asString(root?.ordering),
        inputCount: asNumber(root?.input_count),
        outputCount: asNumber(root?.output_count),
        sortMode: asString(sort?.mode),
        seedHex: asString(sort?.seed_hex),
        uniqueIdHex: asString(root?.unordered_unique_id_hex),
        knownInputSats: asNumber(totals?.known_input_sats),
        outputSats: asNumber(totals?.output_sats),
        feeSats: asNumber(totals?.fee_sats_if_inputs_known),
        modifiableInputs: asBoolean(modifiability?.inputs),
        modifiableOutputs: asBoolean(modifiability?.outputs),
        outputUidPresent: outputs === null
            ? null
            : outputs.filter((output) => asString(asObject(output)?.unique_id_hex) !== null).length,
    };
}
export function fragmentLabel(fragment) {
    const summary = fragmentSummary(fragment.inspect);
    const shape = summary.inputCount === null || summary.outputCount === null
        ? "not decoded"
        : `${summary.inputCount} in / ${summary.outputCount} out`;
    const ordering = summary.ordering ?? "unknown";
    return `${fragment.key} · ${ordering} · ${shape} · ${fragment.origin}`;
}
export function negotiationView(response) {
    const payments = response.payments ?? [];
    const confirmations = response.confirmations ?? [];
    return {
        paymentCount: payments.length,
        confirmationCount: confirmations.length,
        payments: [...payments],
        confirmations: [...confirmations],
    };
}
function fail(error) {
    return { ok: false, error };
}
export function isHexBytes(value, exactBytes) {
    const trimmed = value.trim().toLowerCase();
    if (!/^(?:[0-9a-f]{2})+$/.test(trimmed))
        return false;
    return exactBytes === undefined || trimmed.length === exactBytes * 2;
}
const ORDERING_MODE = {
    det: "deterministic",
    explicit: "explicit",
    unset: "unset",
};
export function buildCreateRequest(form) {
    const ordering = normalizeSessionOrdering(form.ordering, form.seed);
    if (!ordering.valid) {
        return fail(ordering.error ?? "invalid ordering");
    }
    const inputs = [];
    for (const [index, row] of form.inputs.entries()) {
        const txid = row.txid.trim().toLowerCase();
        const voutText = row.vout.trim();
        if (!txid && !voutText)
            continue; // blank row
        if (!isHexBytes(txid, 32)) {
            return fail(`input ${index + 1}: txid must be 32 hex bytes`);
        }
        if (!/^\d+$/.test(voutText)) {
            return fail(`input ${index + 1}: vout must be a non-negative integer`);
        }
        inputs.push({ txid, vout: Number(voutText) });
    }
    const outputs = [];
    for (const [index, row] of form.outputs.entries()) {
        const address = row.address.trim();
        const amountBtc = row.amountBtc.trim();
        if (!address && !amountBtc)
            continue; // blank row
        if (!address || !amountBtc) {
            return fail(`output ${index + 1}: address and amount are both required`);
        }
        // Address + amount validity is the create route's job (real network
        // validation); nothing is second-guessed here.
        outputs.push({ address, amountBtc });
    }
    if (inputs.length === 0 && outputs.length === 0) {
        return fail("add at least one input or output");
    }
    return {
        ok: true,
        value: {
            network: form.network,
            ordering: ORDERING_MODE[ordering.mode],
            seedHex: ordering.seed || undefined,
            inputs,
            outputs,
        },
    };
}
export function parseLines(text) {
    return text
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean);
}
// Plain optional shape (not FormResult): both compile configs must accept
// forwarding the failure, and the lax emit config does not narrow generic
// discriminated unions across returns.
function optionalInteger(text, label) {
    const trimmed = text.trim();
    if (!trimmed)
        return {};
    if (!/^\d+$/.test(trimmed))
        return { error: `${label} must be a non-negative integer` };
    return { value: Number(trimmed) };
}
export function buildSyncRequest(form, psbts) {
    const request = { transport: form.transport };
    if (psbts.length)
        request.psbts = [...psbts];
    const waitMs = optionalInteger(form.irohWaitMs, "wait ms");
    if (waitMs.error)
        return fail(waitMs.error);
    if (waitMs.value !== undefined)
        request.irohWaitMs = waitMs.value;
    if (form.transport === "local") {
        const sources = parseLines(form.sources);
        const state = form.state.trim();
        if (sources.length)
            request.sources = sources;
        if (state)
            request.state = state;
        if (!psbts.length && !sources.length && !state) {
            return fail("select fragments or provide server-side sources/state paths");
        }
        return { ok: true, value: request };
    }
    if (form.transport === "iroh") {
        const ticket = form.irohTicket.trim();
        if (ticket && form.irohTicketOut) {
            return fail("paste a ticket to join OR request a new one, not both");
        }
        if (!ticket && !form.irohTicketOut) {
            return fail("paste an iroh ticket or request a new document ticket");
        }
        if (ticket)
            request.irohTicket = ticket;
        if (form.irohTicketOut)
            request.irohTicketOut = true;
        return { ok: true, value: request };
    }
    // str0m / webrtc-rs: manual file signaling, mirroring the CLI flags. The
    // shared selector re-validates and names any missing param; the presenter
    // only requires what cannot be defaulted.
    if (!form.webrtcRole) {
        return fail("webrtc transports need a role (offer or answer)");
    }
    request.webrtcRole = form.webrtcRole;
    const signalOut = form.signalOut.trim();
    const signalIn = form.signalIn.trim();
    if (!signalOut || !signalIn) {
        return fail("webrtc transports need signal-out and signal-in file paths");
    }
    request.signalOut = signalOut;
    request.signalIn = signalIn;
    const bind = form.webrtcBind.trim();
    if (bind)
        request.webrtcBind = bind;
    const iceServers = parseLines(form.iceServers);
    if (iceServers.length)
        request.iceServers = iceServers;
    const timeoutMs = optionalInteger(form.signalTimeoutMs, "signal timeout ms");
    if (timeoutMs.error)
        return fail(timeoutMs.error);
    if (timeoutMs.value !== undefined)
        request.signalTimeoutMs = timeoutMs.value;
    return { ok: true, value: request };
}
export function buildPayArgs(form) {
    const options = {};
    const secret = form.secretHex.trim();
    if (secret) {
        if (!isHexBytes(secret))
            return fail("secret must be hex bytes");
        options.secretHex = secret.toLowerCase();
    }
    const dummy = optionalInteger(form.dummy, "dummy count");
    if (dummy.error)
        return fail(dummy.error);
    if (dummy.value) {
        if (!options.secretHex) {
            return fail("dummy padding requires a secret (plaintext dummies are distinguishable)");
        }
        options.dummy = dummy.value;
    }
    let payment;
    if (form.mode === "hex") {
        const record = form.paymentHex.trim();
        if (!isHexBytes(record))
            return fail("payment record must be hex bytes");
        payment = record.toLowerCase();
    }
    else {
        const address = form.address.trim();
        const amountBtc = form.amountBtc.trim();
        if (!address || !amountBtc)
            return fail("address and amount are both required");
        const payer = form.payerHex.trim();
        if (payer && !isHexBytes(payer, 32)) {
            return fail("payer id must be 32 hex bytes (64 hex chars)");
        }
        payment = {
            address,
            amountBtc,
            network: form.network || undefined,
            label: form.label.trim() || undefined,
            payerHex: payer ? payer.toLowerCase() : undefined,
        };
    }
    return {
        ok: true,
        value: {
            payment,
            options: options.secretHex === undefined && options.dummy === undefined ? undefined : options,
        },
    };
}
export function buildConfirmArgs(form) {
    let options;
    const secret = form.secretHex.trim();
    if (secret) {
        if (!isHexBytes(secret))
            return fail("secret must be hex bytes");
        options = { secretHex: secret.toLowerCase() };
    }
    if (form.mode === "hex") {
        const record = form.confirmationHex.trim();
        if (!isHexBytes(record))
            return fail("confirmation record must be hex bytes");
        return { ok: true, value: { confirmation: record.toLowerCase(), options } };
    }
    const peer = form.peerIdHex.trim();
    if (peer && !isHexBytes(peer, 32)) {
        return fail("peer id must be 32 hex bytes (64 hex chars)");
    }
    return {
        ok: true,
        value: {
            confirmation: { derive: true, peerIdHex: peer ? peer.toLowerCase() : undefined },
            options,
        },
    };
}
// ---------------------------------------------------------------------------
// Paste/upload helpers.
// ---------------------------------------------------------------------------
// Classify pasted text: a base64 BIP 370 / BIP 174 blob is accepted as-is
// (the two share the `psbt` magic; which decoder applies is the user's
// explicit choice of button, exactly like `ptj import-bip174` vs stdin).
export function pastedPsbt(text) {
    const compact = compactBase64(text);
    return looksLikeBase64Psbt(compact) ? compact : null;
}
const BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
// Pure base64 for uploaded binary PSBT files (browser `atob`/`btoa` are not
// byte-safe and node lacks them in older LTS; this is dependency-free and
// node --test coverable).
export function bytesToBase64(bytes) {
    let out = "";
    for (let i = 0; i < bytes.length; i += 3) {
        const a = bytes[i];
        const b = i + 1 < bytes.length ? bytes[i + 1] : 0;
        const c = i + 2 < bytes.length ? bytes[i + 2] : 0;
        out += BASE64_ALPHABET[a >> 2];
        out += BASE64_ALPHABET[((a & 0x03) << 4) | (b >> 4)];
        out += i + 1 < bytes.length ? BASE64_ALPHABET[((b & 0x0f) << 2) | (c >> 6)] : "=";
        out += i + 2 < bytes.length ? BASE64_ALPHABET[c & 0x3f] : "=";
    }
    return out;
}
