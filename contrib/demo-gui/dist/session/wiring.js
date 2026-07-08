// contrib/demo-gui/src/session/wiring.ts
//
// Wiring presenter — the UNIVERSAL JOIN GESTURE over the session object
// graph, plus the contextual-enablement rules for the selection-scoped
// operations. Pure data-in/data-out (node --test covered by
// test/session-wiring.test.mjs); the DOM shell renders verdicts and reasons,
// it never re-derives them.
//
// The wiring metaphor: every card on the page is a NODE (PSBT fragment,
// session, peer, payment instruction, spendable output, descriptor), and
// connecting two nodes performs the join appropriate to the PAIR:
//
//   fragment ⋈ fragment  = PSBT lattice join            (/api/join, backed)
//   fragment → session   = incorporate into the session (UI membership, backed)
//   peer     → session   = participate: sync the session over the peer's
//                          transport                     (/api/sync, backed)
//   payment  → fragment  = attach the payment record    (/api/pay, backed)
//   utxo     → create    = use the outpoint as a create-form input (backed)
//   session  ⋈ session   = merge converging states      (needs backend)
//   peer     ⋈ peer      = standalone channel           (needs backend)
//   descriptor → fragment = attribute matching scripts  (needs backend)
//
// Pairs with no backend seam yet stay VISIBLE but explicitly unwired
// (allowed=false + a precise `needs` string) — never silently hidden.
//
// Enablement doctrine (override affordances): "impossible" (wrong selection
// arity) is plainly disabled with a reason; "blocked by a correctness gate"
// (e.g. joining ordered fragments — a real spec gate, but pre-BIP-370
// interop needs escape hatches) is OVERRIDABLE: the gate carries a stable id
// the shell arms explicitly, with a warning, and the backend stays the final
// authority. Nothing is bypassed silently.
export function emptyObjects() {
    return { sessions: [], peers: [], payments: [], utxos: [], descriptors: [], counter: 0 };
}
function nextKey(state, prefix) {
    const counter = state.counter + 1;
    return { state: { ...state, counter }, key: `${prefix}-${counter}` };
}
export function mintSession(state, name, transport) {
    const next = nextKey(state, "session");
    const session = {
        key: next.key,
        name: name.trim() || next.key,
        fragmentKeys: [],
        transport,
        irohTicket: "",
        stateFile: "",
        peerKeys: [],
    };
    return {
        state: { ...next.state, sessions: [...next.state.sessions, session] },
        session,
    };
}
export function mintPeer(state, name, transport, identity) {
    const next = nextKey(state, "peer");
    const peer = {
        key: next.key,
        name: name.trim() || next.key,
        transport,
        identity: identity.trim(),
    };
    return { state: { ...next.state, peers: [...next.state.peers, peer] }, peer };
}
export function mintPayment(state, uri, address, amountSats, label) {
    const next = nextKey(state, "payment");
    const payment = { key: next.key, uri, address, amountSats, label };
    return { state: { ...next.state, payments: [...next.state.payments, payment] }, payment };
}
export function mintUtxo(state, rawTxHex) {
    const next = nextKey(state, "utxo");
    const utxo = { key: next.key, rawTxHex, txid: null, vout: null, amountSats: null };
    return { state: { ...next.state, utxos: [...next.state.utxos, utxo] }, utxo };
}
export function mintDescriptor(state, descriptor, isPrivate) {
    const next = nextKey(state, "descriptor");
    const minted = { key: next.key, descriptor: descriptor.trim(), isPrivate };
    return {
        state: { ...next.state, descriptors: [...next.state.descriptors, minted] },
        descriptor: minted,
    };
}
export function sessionByKey(state, key) {
    return state.sessions.find((session) => session.key === key) ?? null;
}
export function peerByKey(state, key) {
    return state.peers.find((peer) => peer.key === key) ?? null;
}
export function addFragmentToSession(state, sessionKey, fragmentKey) {
    return {
        ...state,
        sessions: state.sessions.map((session) => session.key === sessionKey && !session.fragmentKeys.includes(fragmentKey)
            ? { ...session, fragmentKeys: [...session.fragmentKeys, fragmentKey] }
            : session),
    };
}
export function addPeerToSession(state, sessionKey, peerKey) {
    return {
        ...state,
        sessions: state.sessions.map((session) => session.key === sessionKey && !session.peerKeys.includes(peerKey)
            ? { ...session, peerKeys: [...session.peerKeys, peerKey] }
            : session),
    };
}
// Fragments removed from the fragment set must also leave session
// memberships (sessions reference fragments by key).
export function dropFragmentKey(state, fragmentKey) {
    return {
        ...state,
        sessions: state.sessions.map((session) => session.fragmentKeys.includes(fragmentKey)
            ? { ...session, fragmentKeys: session.fragmentKeys.filter((key) => key !== fragmentKey) }
            : session),
    };
}
function verdict(kind, allowed, backed, reason = null, needs = null) {
    return { kind, allowed, backed, reason, needs };
}
function unordered(a, b, x, y) {
    return (a === x && b === y) || (a === y && b === x);
}
export function wireVerdict(source, target, state) {
    const a = source.kind;
    const b = target.kind;
    if (a === b && source.key === target.key) {
        return verdict("none", false, false, `cannot wire a ${a} to itself`);
    }
    if (a === "fragment" && b === "fragment") {
        return verdict("fragment-join", true, true);
    }
    if (unordered(a, b, "fragment", "session")) {
        const sessionKey = a === "session" ? source.key : target.key;
        const fragmentKey = a === "fragment" ? source.key : target.key;
        const session = sessionByKey(state, sessionKey);
        if (session && session.fragmentKeys.includes(fragmentKey)) {
            return verdict("fragment-into-session", false, true, "fragment is already in the session");
        }
        return verdict("fragment-into-session", true, true);
    }
    if (unordered(a, b, "peer", "session")) {
        const peerKey = a === "peer" ? source.key : target.key;
        const peer = peerByKey(state, peerKey);
        if (peer && peer.transport === "nostr") {
            // The nostr transport is not served by /api/sync yet; keep the pair
            // visible but honestly unwired.
            return verdict("peer-into-session", false, false, null, "a nostr transport behind /api/sync (npub peers cannot sync yet)");
        }
        if (peer && (peer.transport === "unknown" || !peer.identity)) {
            return verdict("peer-into-session", false, true, "peer has no usable transport identity (configure a ticket or signaling files)");
        }
        return verdict("peer-into-session", true, true);
    }
    if (unordered(a, b, "payment", "fragment")) {
        return verdict("attach-payment", true, true);
    }
    if (a === "utxo" && b === "create") {
        return verdict("add-create-input", true, true);
    }
    if (a === "utxo" || b === "utxo") {
        return verdict("none", false, false, "spendable outputs feed the create form (chain sources stay manual for now)");
    }
    if (a === "session" && b === "session") {
        return verdict("session-merge", false, false, null, "a session-state merge seam (lattice join of two converging session states)");
    }
    if (a === "peer" && b === "peer") {
        return verdict("peer-channel", false, false, null, "a standalone peer-to-peer channel establishment seam");
    }
    if (a === "descriptor" && (b === "fragment" || b === "session")) {
        return verdict("attribute-scripts", false, false, null, "descriptor derivation (Backend.classifyPaste) to match fragment scripts to the descriptor");
    }
    if (unordered(a, b, "peer", "fragment")) {
        return verdict("none", false, false, "wire the peer to a session; fragments sync through sessions");
    }
    if (a === "payment" && b === "session") {
        return verdict("none", false, false, "wire the payment instruction to a fragment");
    }
    return verdict("none", false, false, `no join is defined for ${a} + ${b}`);
}
export function idleWire() {
    return { source: null };
}
export function beginWire(kind, key) {
    return { source: { kind, key } };
}
// Tapping the armed source again cancels; tapping any other node yields the
// verdict for the pair (the shell acts on allowed+backed verdicts and
// reports the reason/needs text otherwise).
export function completeWire(gesture, target, state) {
    if (!gesture.source) {
        return { gesture, verdict: null };
    }
    if (gesture.source.kind === target.kind && gesture.source.key === target.key) {
        return { gesture: idleWire(), verdict: null };
    }
    return { gesture: idleWire(), verdict: wireVerdict(gesture.source, target, state) };
}
const ARITY = {
    join: { min: 2 },
    concatenate: { min: 2 },
    sort: { min: 1, exactly: true },
    "make-unordered": { min: 1, exactly: true },
    atomize: { min: 1, exactly: true },
    "export-v2": { min: 1, exactly: true },
    "export-bip174": { min: 1, exactly: true },
    edit: { min: 1, exactly: true },
    pay: { min: 1, exactly: true },
    confirm: { min: 1, exactly: true },
    payments: { min: 1, exactly: true },
    sync: { min: 1 },
    "assign-ids": { min: 1, exactly: true },
};
function arityReason(action, count) {
    const rule = ARITY[action];
    if (rule.exactly && count !== rule.min) {
        return `needs exactly ${rule.min} selected fragment${rule.min === 1 ? "" : "s"} (${count} selected)`;
    }
    if (!rule.exactly && count < rule.min) {
        return `needs at least ${rule.min} selected fragments (${count} selected)`;
    }
    return null;
}
// Correctness gates: what the UI KNOWS (from inspect data) the spec
// disallows, kept overridable because the backend re-validates and interop
// with pre-BIP-370 producers is a real need. Unknown (not-decoded) fragments
// never gate — the backend is the authority on them.
function gateFor(action, selected) {
    switch (action) {
        case "join": {
            const ordered = selected.filter((summary) => summary.ordering === "ordered").length;
            if (ordered > 0) {
                return {
                    id: "join-ordered",
                    label: `${ordered} selected fragment(s) are ordered`,
                    warning: "the lattice join is defined over unordered fragments; the backend may reject ordered ones. Overriding sends them as-is.",
                };
            }
            return null;
        }
        case "sort": {
            if (selected.length === 1 && selected[0].ordering === "ordered") {
                return {
                    id: "sort-ordered",
                    label: "fragment is already ordered",
                    warning: "sorting an already-ordered PSBT asks the backend to re-run the sorter role on it.",
                };
            }
            return null;
        }
        case "make-unordered": {
            if (selected.length === 1 && selected[0].ordering === "unordered") {
                return {
                    id: "make-unordered-unordered",
                    label: "fragment is already unordered",
                    warning: "re-shuffling an unordered PSBT re-randomizes its element order.",
                };
            }
            return null;
        }
        case "export-bip174": {
            // Observed route behavior: /api/export-bip174 rejects unordered PSBTs
            // ("expects an ordered PSBT; run `ptj sort` first").
            if (selected.length === 1 && selected[0].ordering === "unordered") {
                return {
                    id: "export-bip174-unordered",
                    label: "fragment is unordered (BIP 174 needs an ordered PSBT)",
                    warning: "the backend rejects unordered PSBTs for BIP 174 export — run Sort first; overriding sends it anyway and surfaces the route's error.",
                };
            }
            return null;
        }
        case "atomize": {
            if (selected.length !== 1)
                return null;
            const summary = selected[0];
            if (summary.modifiableInputs === false && summary.modifiableOutputs === false) {
                return {
                    id: "atomize-unmodifiable",
                    label: "fragment is not modifiable (tx-modifiable flags are clear)",
                    warning: "atomize parses through the constructor role, which requires modifiable flags; the backend will reject this unless the flags are edited. Overriding sends it as-is.",
                };
            }
            const elements = (summary.inputCount ?? 0) + (summary.outputCount ?? 0);
            if (summary.inputCount !== null && summary.outputCount !== null && elements <= 1) {
                return {
                    id: "atomize-atomic",
                    label: "fragment is already atomic (one element)",
                    warning: "the backend reports 'PSBT is already atomic' for single-element fragments.",
                };
            }
            return null;
        }
        default:
            return null;
    }
}
export function actionState(action, ctx) {
    const needsBackend = action === "assign-ids" ? "assignIds" : null;
    const arity = arityReason(action, ctx.selected.length);
    if (arity) {
        return { enabled: false, reason: arity, gate: null, overridden: false, needsBackend };
    }
    if (action === "assign-ids") {
        const summary = ctx.selected[0];
        if (summary.outputUidPresent !== null &&
            summary.outputCount !== null &&
            summary.outputUidPresent >= summary.outputCount) {
            return {
                enabled: false,
                reason: "all outputs already carry unique ids",
                gate: null,
                overridden: false,
                needsBackend,
            };
        }
        // The seam itself is pending: visible, disabled, precisely labeled.
        return {
            enabled: false,
            reason: "waiting on the assignIds backend seam",
            gate: null,
            overridden: false,
            needsBackend,
        };
    }
    const gate = gateFor(action, ctx.selected);
    if (gate && !ctx.overrides.has(gate.id)) {
        return { enabled: false, reason: gate.label, gate, overridden: false, needsBackend };
    }
    return {
        enabled: true,
        reason: null,
        gate,
        overridden: gate !== null,
        needsBackend,
    };
}
export function overviewFocus() {
    return { mode: "overview", sessionKey: null };
}
export function sessionFocus(key) {
    return { mode: "session", sessionKey: key };
}
// Re-validate focus against the live session list: a focused session that
// disappeared falls back to overview; overview never captures a key.
export function validateFocus(focus, sessionKeys) {
    if (focus.mode === "session" && focus.sessionKey !== null && sessionKeys.includes(focus.sessionKey)) {
        return focus;
    }
    return overviewFocus();
}
