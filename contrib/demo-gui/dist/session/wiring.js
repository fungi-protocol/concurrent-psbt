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
import { asArray, asNumber, asObject, asString } from "./state.js";
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
    const payment = {
        key: next.key,
        uri,
        address,
        amountSats,
        label,
        variant: null,
        methods: [],
        description: null,
    };
    return { state: { ...next.state, payments: [...next.state.payments, payment] }, payment };
}
export function mintUtxo(state, rawTxHex) {
    const next = nextKey(state, "utxo");
    const utxo = {
        key: next.key,
        rawTxHex,
        txid: null,
        vout: null,
        amountSats: null,
        address: null,
        fullySigned: null,
    };
    return { state: { ...next.state, utxos: [...next.state.utxos, utxo] }, utxo };
}
export function mintDescriptor(state, descriptor, isPrivate) {
    const next = nextKey(state, "descriptor");
    const minted = {
        key: next.key,
        descriptor: descriptor.trim(),
        isPrivate,
        normalized: null,
        descriptorType: null,
        hasPrivateKeys: null,
        isRanged: null,
        derived: [],
    };
    return {
        state: { ...next.state, descriptors: [...next.state.descriptors, minted] },
        descriptor: minted,
    };
}
// ---------------------------------------------------------------------------
// Deep-classification enrichment: fold a Backend.classifyPaste response into
// the shallow-minted node. Pure and defensive (the details are read like
// inspect JSON); a response of the wrong kind leaves the state untouched, so
// a failed/misrouted enrichment can never damage the shallow card.
// ---------------------------------------------------------------------------
export function enrichDescriptor(state, key, classified) {
    if (classified.kind !== "descriptor")
        return state;
    const derived = (asArray(classified.derived) ?? []).flatMap((raw) => {
        const entry = asObject(raw);
        const index = asNumber(entry?.index);
        const scriptPubkeyHex = asString(entry?.script_pubkey_hex);
        if (index === null || scriptPubkeyHex === null)
            return [];
        return [{ index, scriptPubkeyHex, address: asString(entry?.address) }];
    });
    const hasPrivateKeys = asObject(classified)?.has_private_keys === true;
    return {
        ...state,
        descriptors: state.descriptors.map((descriptor) => descriptor.key === key
            ? {
                ...descriptor,
                normalized: asString(classified.descriptor),
                descriptorType: asString(classified.descriptor_type),
                hasPrivateKeys,
                // The deep flag is authoritative: the shallow regex heuristic
                // only guessed.
                isPrivate: hasPrivateKeys,
                isRanged: asObject(classified)?.is_ranged === true,
                derived,
            }
            : descriptor),
    };
}
export function enrichPayment(state, key, classified) {
    if (classified.kind !== "payment")
        return state;
    const methods = (asArray(classified.methods) ?? []).flatMap((raw) => {
        const entry = asObject(raw);
        const type = asString(entry?.type);
        if (type === null)
            return [];
        const detail = asString(entry?.address) ?? asString(entry?.invoice) ?? asString(entry?.offer);
        return [detail ? `${type}: ${detail}` : type];
    });
    return {
        ...state,
        payments: state.payments.map((payment) => payment.key === key
            ? {
                ...payment,
                variant: asString(classified.variant),
                methods,
                description: asString(classified.description),
            }
            : payment),
    };
}
// Fold a transaction decode into the pending utxo node: the FIRST output
// updates the node in place (its key is what the paste flow logged/focused),
// every further output mints a sibling node carrying the same raw hex.
export function applyTxOutputs(state, key, classified) {
    if (classified.kind !== "transaction")
        return { state, utxos: [] };
    const source = state.utxos.find((utxo) => utxo.key === key);
    if (!source)
        return { state, utxos: [] };
    const txid = asString(classified.txid);
    const fullySigned = asObject(classified)?.fully_signed === true;
    const outputs = (asArray(classified.outputs) ?? []).flatMap((raw) => {
        const entry = asObject(raw);
        const vout = asNumber(entry?.vout);
        if (vout === null)
            return [];
        return [{ vout, amountSats: asNumber(entry?.amount_sats), address: asString(entry?.address) }];
    });
    if (txid === null || outputs.length === 0)
        return { state, utxos: [] };
    const enriched = [];
    let next = state;
    outputs.forEach((output, position) => {
        const fields = {
            rawTxHex: source.rawTxHex,
            txid,
            vout: output.vout,
            amountSats: output.amountSats,
            address: output.address,
            fullySigned,
        };
        if (position === 0) {
            const updated = { ...source, ...fields };
            next = {
                ...next,
                utxos: next.utxos.map((utxo) => (utxo.key === key ? updated : utxo)),
            };
            enriched.push(updated);
        }
        else {
            const minted = nextKey(next, "utxo");
            const sibling = { key: minted.key, ...fields };
            next = { ...minted.state, utxos: [...minted.state.utxos, sibling] };
            enriched.push(sibling);
        }
    });
    return { state: next, utxos: enriched };
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
function verdict(kind, allowed, backed, reason = null, needs = null, label = null) {
    return { kind, allowed, backed, reason, needs, label };
}
function unordered(a, b, x, y) {
    return (a === x && b === y) || (a === y && b === x);
}
// Display name for a node in wire action labels: sessions and peers carry
// human names, payments a user label; fragments (and everything else) go by
// their key, which is already the visible card title.
export function nodeDisplayName(ref, state) {
    switch (ref.kind) {
        case "session":
            return sessionByKey(state, ref.key)?.name ?? ref.key;
        case "peer":
            return peerByKey(state, ref.key)?.name ?? ref.key;
        case "payment": {
            const payment = state.payments.find((candidate) => candidate.key === ref.key);
            return payment && payment.label ? payment.label : ref.key;
        }
        default:
            return ref.key;
    }
}
export function wireDisposition(v) {
    if (v.allowed && v.backed)
        return "compatible";
    if (v.backed)
        return "blocked";
    return "unbacked";
}
export function wireVerdict(source, target, state) {
    const a = source.kind;
    const b = target.kind;
    const sourceName = nodeDisplayName(source, state);
    const targetName = nodeDisplayName(target, state);
    if (a === b && source.key === target.key) {
        return verdict("none", false, false, `cannot wire a ${a} to itself`);
    }
    if (a === "fragment" && b === "fragment") {
        const label = `Join ${sourceName} into ${targetName}`;
        return verdict("fragment-join", true, true, null, null, label);
    }
    if (unordered(a, b, "fragment", "session")) {
        const sessionKey = a === "session" ? source.key : target.key;
        const fragmentKey = a === "fragment" ? source.key : target.key;
        const session = sessionByKey(state, sessionKey);
        const label = `Publish ${fragmentKey} to session ${a === "session" ? sourceName : targetName}`;
        if (session && session.fragmentKeys.includes(fragmentKey)) {
            return verdict("fragment-into-session", false, true, "fragment is already in the session", null, label);
        }
        return verdict("fragment-into-session", true, true, null, null, label);
    }
    if (unordered(a, b, "peer", "session")) {
        const peerKey = a === "peer" ? source.key : target.key;
        const peer = peerByKey(state, peerKey);
        const sessionName = a === "session" ? sourceName : targetName;
        const peerName = a === "peer" ? sourceName : targetName;
        const label = `Sync session ${sessionName} over peer ${peerName}`;
        if (peer && peer.transport === "nostr") {
            // The nostr transport is not served by /api/sync yet; keep the pair
            // visible but honestly unwired.
            return verdict("peer-into-session", false, false, null, "a nostr transport behind /api/sync (npub peers cannot sync yet)", label);
        }
        if (peer && (peer.transport === "unknown" || !peer.identity)) {
            return verdict("peer-into-session", false, true, "peer has no usable transport identity (configure a ticket or signaling files)", null, label);
        }
        return verdict("peer-into-session", true, true, null, null, label);
    }
    if (unordered(a, b, "payment", "fragment")) {
        const paymentRef = a === "payment" ? source : target;
        const fragmentKey = a === "fragment" ? source.key : target.key;
        const label = `Attach payment ${nodeDisplayName(paymentRef, state)} to ${fragmentKey}`;
        return verdict("attach-payment", true, true, null, null, label);
    }
    if (a === "utxo" && b === "create") {
        return verdict("add-create-input", true, true, null, null, `Use ${sourceName} as a create-form input`);
    }
    if (a === "utxo" || b === "utxo") {
        return verdict("none", false, false, "spendable outputs feed the create form (chain sources stay manual for now)");
    }
    if (a === "session" && b === "session") {
        return verdict("session-merge", false, false, null, "a session-state merge seam (lattice join of two converging session states)", `Merge sessions ${sourceName} and ${targetName}`);
    }
    if (a === "peer" && b === "peer") {
        return verdict("peer-channel", false, false, null, "a standalone peer-to-peer channel establishment seam", `Bridge peers ${sourceName}, ${targetName}`);
    }
    if (a === "descriptor" && (b === "fragment" || b === "session")) {
        return verdict("attribute-scripts", false, false, null, "descriptor derivation (Backend.classifyPaste) to match fragment scripts to the descriptor", `Attribute ${sourceName} scripts to ${targetName}`);
    }
    if (unordered(a, b, "peer", "fragment")) {
        return verdict("none", false, false, "wire the peer to a session; fragments sync through sessions");
    }
    if (a === "payment" && b === "session") {
        return verdict("none", false, false, "wire the payment instruction to a fragment");
    }
    return verdict("none", false, false, `no join is defined for ${a} + ${b}`);
}
function nodeId(ref) {
    return `${ref.kind}:${ref.key}`;
}
// Direction-insensitive identity of a wire (the demo's joinWireKey).
export function wireKey(a, b) {
    return [nodeId(a), nodeId(b)].sort().join("::");
}
// Only compatible wires queue; blocked/unbacked verdicts come back for the
// shell's rejection feedback. Duplicates (either direction) are no-ops.
export function queueWire(wires, source, target, state) {
    const v = wireVerdict(source, target, state);
    if (wireDisposition(v) !== "compatible") {
        return { wires, queued: false, duplicate: false, verdict: v };
    }
    const key = wireKey(source, target);
    if (wires.some((wire) => wireKey(wire.source, wire.target) === key)) {
        return { wires, queued: false, duplicate: true, verdict: v };
    }
    return { wires: [...wires, { source, target }], queued: true, duplicate: false, verdict: v };
}
export function unqueueWire(wires, key) {
    return wires.filter((wire) => wireKey(wire.source, wire.target) !== key);
}
export function nodeExists(ref, state, fragmentKeys) {
    switch (ref.kind) {
        case "fragment":
            return fragmentKeys.includes(ref.key);
        case "session":
            return sessionByKey(state, ref.key) !== null;
        case "peer":
            return peerByKey(state, ref.key) !== null;
        case "payment":
            return state.payments.some((payment) => payment.key === ref.key);
        case "utxo":
            return state.utxos.some((utxo) => utxo.key === ref.key);
        case "descriptor":
            return state.descriptors.some((descriptor) => descriptor.key === ref.key);
        case "create":
            return true;
    }
}
// Re-validate the queue against current state (the demo's validJoinWires):
// wires lose their place when an endpoint disappears or the pair's verdict
// is no longer compatible (e.g. the fragment was published to the session
// through another path).
export function pruneWires(wires, state, fragmentKeys) {
    return wires.filter((wire) => nodeExists(wire.source, state, fragmentKeys) &&
        nodeExists(wire.target, state, fragmentKeys) &&
        wireDisposition(wireVerdict(wire.source, wire.target, state)) === "compatible");
}
// Connected components of the pending-wire graph (the demo's
// joinComponents): the toolbar Join applies each component as a unit.
export function wireComponents(wires) {
    const adjacency = new Map();
    const refs = new Map();
    for (const wire of wires) {
        const a = nodeId(wire.source);
        const b = nodeId(wire.target);
        refs.set(a, wire.source);
        refs.set(b, wire.target);
        if (!adjacency.has(a))
            adjacency.set(a, new Set());
        if (!adjacency.has(b))
            adjacency.set(b, new Set());
        adjacency.get(a).add(b);
        adjacency.get(b).add(a);
    }
    const components = [];
    const seen = new Set();
    for (const startId of adjacency.keys()) {
        if (seen.has(startId))
            continue;
        const stack = [startId];
        const memberIds = new Set();
        while (stack.length) {
            const current = stack.pop();
            if (seen.has(current))
                continue;
            seen.add(current);
            memberIds.add(current);
            for (const next of adjacency.get(current) ?? [])
                stack.push(next);
        }
        // First-seen order over the queue keeps the report deterministic.
        const nodes = [];
        for (const wire of wires) {
            for (const ref of [wire.source, wire.target]) {
                const id = nodeId(ref);
                if (memberIds.has(id) && !nodes.some((candidate) => nodeId(candidate) === id)) {
                    nodes.push(ref);
                }
            }
        }
        components.push({
            nodes,
            wires: wires.filter((wire) => memberIds.has(nodeId(wire.source)) && memberIds.has(nodeId(wire.target))),
        });
    }
    return components;
}
export function componentPlan(component) {
    const joinWires = component.wires.filter((wire) => wire.source.kind === "fragment" && wire.target.kind === "fragment");
    const clusters = wireComponents(joinWires);
    return {
        joinGroups: clusters
            .map((cluster) => ({
            fragments: cluster.nodes.map((ref) => ref.key),
            wires: cluster.wires,
        }))
            .filter((group) => group.fragments.length >= 2),
        rest: component.wires.filter((wire) => !(wire.source.kind === "fragment" && wire.target.kind === "fragment")),
    };
}
// Follow a consumed fragment endpoint to its join result (the session-side
// analog of the demo's remapNodeId).
export function remapFragmentRef(ref, remap) {
    if (ref.kind !== "fragment")
        return ref;
    const mapped = remap.get(ref.key);
    return mapped === undefined ? ref : { kind: "fragment", key: mapped };
}
export function wireQueueSummary(wires) {
    const wireCount = wires.length;
    const componentCount = wireComponents(wires).length;
    return {
        wireCount,
        componentCount,
        text: wireCount === 0
            ? "no pending wires"
            : `${wireCount} pending wire${wireCount === 1 ? "" : "s"} in ` +
                `${componentCount} component${componentCount === 1 ? "" : "s"}`,
    };
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
    // No selection-scoped action is waiting on a missing seam today
    // (Backend.assignIds landed with the /api/assign-ids route); the field
    // stays so future actions can name theirs.
    const needsBackend = null;
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
