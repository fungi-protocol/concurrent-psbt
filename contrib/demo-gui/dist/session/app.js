// contrib/demo-gui/src/session/app.ts
//
// Session UI shell — the REAL webgui page (served at "/"). A thin, strictly
// typed DOM layer: every decision lives in the pure presenter modules
// (./state.js fragment set + form builders, ./display.js card projections,
// ./wiring.js object graph + join admissibility + contextual enablement,
// ./ingest.js universal paste classification, ./editor.js field editor,
// ./encoding.js liberal parsing, ./lifehash.js fingerprints) and every
// operation drives the ONE Backend seam (HttpBackend against this server's
// own /api/* routes). No fixtures: the fragment list is exactly the set of
// real PSBTs pasted, uploaded, imported, created, or produced by backend
// operations.
//
// No query strings on the seam imports: http.js itself imports
// ../core/types.js without one, and both URLs must resolve to the SAME
// module instance for `instanceof PtjBackendError` to work (responses are
// served Cache-Control: no-store; cache busting rides the ?v on
// dist/session/app.js in session.html alone).
import { addressChipDigestHex, groupChipDigestHex, lifehashSrc } from "./display.js";
import { HttpBackend } from "../shared-frontend/backends/http.js";
import { PtjBackendError } from "../shared-frontend/core/types.js";
import { seedFromRandomBytes } from "../model.js";
import { addFragment, asArray, asObject, asString, buildConfirmArgs, buildCreateRequest, buildPayArgs, buildSyncRequest, bytesToBase64, emptySession, fragmentSummary, negotiationView, pastedPsbt, removeFragment, selectedFragments, setSelected, } from "./state.js";
import { amountBits, amountSpanParts, elisionLabel, fragmentBadges, fragmentCardModel, rowDetailPairs, signedAmountSpanParts, } from "./display.js";
import { classifyPaste, mintFromPaste } from "./ingest.js";
import { actionState, addBridge, addFragmentToSession, addPeerToSession, applyTxOutputs, beginWire, bridgeGroupContaining, completeWire, componentPlan, dropFragmentKey, emptyObjects, enrichDescriptor, enrichPayment, idleWire, mergeSessions, mineFragmentKeys, mintPeer, mintSession, overviewFocus, peerBridgeGroups, peerByKey, peerUsableForSync, pruneWires, queueWire, sessionByKey, sessionFocus, unionBridgedPeersIntoSessions, unqueueWire, validateFocus, wireComponents, wireDisposition, wireKey, wireQueueSummary, wireVerdict, remapWireRef, } from "./wiring.js";
import { applyEdit, applyFix, decodedEditsLeftBehind, editorModel, rawEditsForSave, validateEditor, violationsFromServer, } from "./editor.js";
import { descriptorColorKey, groupColorKey, paletteColor, paletteRegistry, peerColorKey, } from "./palette.js";
const backend = new HttpBackend();
// --- shell state ------------------------------------------------------------
let session = emptySession();
let objects = emptyObjects();
let focus = overviewFocus();
let wire = idleWire();
// The pending-wire queue: completed wire gestures accumulate here as
// visible edges (each with its own Join) instead of executing immediately;
// the toolbar Join applies whole connected components. Pruned against the
// live object graph on every render.
let pendingWires = [];
let editor = null;
// Server-side fixes queued for the next editor save (violation fix_ids the
// user accepted) and gate overrides armed for it (violation override_params).
// Cleared whenever the editor opens on a fragment or closes: both are
// explicit per-save decisions, never sticky defaults.
const pendingEditorFixes = new Set();
const editorOverrides = new Set();
// The fragment the assign-ids panel is parameterizing (null = panel closed).
let assignIdsTarget = null;
// Armed correctness-gate overrides. Cleared whenever the selection changes:
// an override is an explicit, per-situation decision, never a sticky default.
const overrides = new Set();
const expanded = new Set();
// Expanded input/output rows, keyed "<fragment>:<side>:<index>" — clicking a
// row toggles its full-field detail (display.ts rowDetailPairs).
const expandedRows = new Set();
// Lineage notes for operation results ("join of psbt-1, psbt-2") — the
// lattice provenance the card shows under the title.
const lineage = new Map();
// Tableau 10 color identities (palette.js): first-seen stable for the page
// session — descriptors and pseudo-descriptors keep their color across
// re-renders and later arrivals.
const identityColors = paletteRegistry();
// Paint a node with its identity color: the CSS custom property drives the
// group/card delineation (border, stripe, chip) in the descriptor's color.
function colorizeIdentity(node, colorKey) {
    if (!colorKey)
        return;
    node.classList.add("session-colorized");
    node.style.setProperty("--identity-color", paletteColor(identityColors, colorKey));
}
// --- tiny DOM helpers -------------------------------------------------------
function el(id) {
    const node = document.getElementById(id);
    if (!node)
        throw new Error(`session UI is missing #${id}`);
    return node;
}
function inputValue(id) {
    return el(id).value;
}
function textareaValue(id) {
    return el(id).value;
}
function selectValue(id) {
    return el(id).value;
}
function button(label, title, onClick) {
    const node = document.createElement("button");
    node.type = "button";
    node.textContent = label;
    if (title)
        node.title = title;
    node.addEventListener("click", onClick);
    return node;
}
function span(className, text) {
    const node = document.createElement("span");
    node.className = className;
    node.textContent = text;
    return node;
}
function logEvent(message) {
    const log = el("sessionLog");
    const item = document.createElement("li");
    item.textContent = message;
    log.prepend(item);
    while (log.children.length > 40) {
        log.lastElementChild?.remove();
    }
}
function showStatus(message, isError) {
    const status = el("sessionStatus");
    status.textContent = message;
    status.classList.toggle("session-status-error", isError);
}
function reportError(context, error) {
    const detail = error instanceof PtjBackendError
        ? error.message
        : error instanceof Error
            ? error.message
            : String(error);
    showStatus(`${context}: ${detail}`, true);
    logEvent(`ERROR ${context}: ${detail}`);
}
// Opening a panel must be VISIBLE: the wide panels live below the fold, and
// unhiding one without scrolling reads as a dead button (the live-review
// symptom on Edit). Reveal = unhide + scroll into view + move focus to the
// panel so keyboard/AT users land where the action went.
function revealPanel(id) {
    const panel = el(id);
    panel.hidden = false;
    panel.tabIndex = -1;
    panel.scrollIntoView({ behavior: "smooth", block: "start" });
    panel.focus({ preventScroll: true });
}
function showOutput(title, body) {
    el("outputTitle").textContent = title;
    el("outputBody").value = body;
    revealPanel("outputPanel");
}
function copyText(text, what) {
    navigator.clipboard.writeText(text).then(() => showStatus(`${what} copied to the clipboard`, false), (error) => reportError(`copy ${what}`, error));
}
function displayNetwork() {
    return selectValue("displayNetwork");
}
// --- LifeHash fingerprints ---------------------------------------------------
//
// Digest-like values (txids, unique ids, scripts) render as LifeHash visual
// fingerprints so humans compare at a glance; the full bitvomit stays one
// click away (hover title, raw view, field editor).
//
// Fingerprints come from the server route GET /api/lifehash/<hex-digest> ->
// PNG (image/png, cacheable; the `lifehash` Rust crate backend-side — a
// concurrent-psbt-wasm export follows later for the PWA). The frontend
// stays trivial: a plain lazy-loaded <img> per fingerprint. If a fetch
// fails (a shell without the route, a digest the route rejects), the image
// swaps to a clearly marked placeholder chip carrying the truncated hex —
// graceful, never blocking.
// Hexes whose fetch already failed: skip re-requesting on every render (a
// reload retries).
const lifehashFailed = new Set();
function lifehashPlaceholder(hex, title) {
    const chip = span("session-fingerprint-pending", hex.slice(0, 8));
    chip.title = `${title}\n${hex}\n(LifeHash fingerprint unavailable — GET /api/lifehash/<hex> did not serve a PNG)`;
    return chip;
}
// An address slot on a card: the LifeHash chip of the script the address
// encodes (display.js addressChipDigestHex — identical scripts fingerprint
// identically), the address itself riding the chip title/aria-label. Strings
// that decode to no script (a lightning invoice or offer in a payment's
// address slot) stay textual — there is no script to fingerprint.
function addressNode(address, what, className = "session-address") {
    const digest = addressChipDigestHex(address);
    return digest ? lifehashBadge(digest, `${address}\n${what}`) : span(className, address);
}
function lifehashBadge(hex, title) {
    if (lifehashFailed.has(hex)) {
        return lifehashPlaceholder(hex, title);
    }
    const img = document.createElement("img");
    img.className = "session-lifehash";
    img.alt = `fingerprint ${hex.slice(0, 8)}`;
    // The full textual value (an address chip's address) reads out to AT.
    img.setAttribute("aria-label", title);
    img.title = `${title}\n${hex}`;
    // Cards render in bulk; fingerprints load as they scroll into view.
    img.loading = "lazy";
    img.src = lifehashSrc(hex);
    img.addEventListener("error", () => {
        lifehashFailed.add(hex);
        img.replaceWith(lifehashPlaceholder(hex, title));
    }, { once: true });
    return img;
}
// BIP 177 sat-first emphasis (display.ts amountSpanParts): symbol/scale/
// digits spans whose classes carry only opacity and weight — every part
// inherits the surrounding color (the ead6ca05 rule). Underneath, the
// binary fingerprint (display.ts amountBits): a thin barcode of the value
// in base 2, LSB right-aligned under the last digit, for at-a-glance
// recognition of low-Hamming-weight values.
function amountSpanFrom(parts, sats) {
    const node = span("session-amount", "");
    const text = span("session-amount-text", "");
    for (const part of parts) {
        text.append(span(part.className, part.text));
    }
    node.append(text);
    const bits = amountBits(sats);
    const row = span("session-amount-bits", "");
    row.title = `binary ${bits}`;
    row.setAttribute("aria-hidden", "true");
    for (const bit of bits) {
        row.append(span(`session-amount-bit session-amount-bit-${bit}`, ""));
    }
    node.append(row);
    return node;
}
function amountSpan(sats) {
    return amountSpanFrom(amountSpanParts(sats), sats);
}
// Signed variant for balance deltas: the sign inherits the surrounding
// color like every other significant digit.
function signedAmountSpan(sats) {
    return amountSpanFrom(signedAmountSpanParts(sats), sats);
}
// --- fragment set plumbing ----------------------------------------------------
function addAndRender(psbt, inspect, origin, note) {
    const added = addFragment(session, psbt, inspect, origin);
    session = added.state;
    if (added.duplicate) {
        logEvent(`${added.fragment.key} already loaded; selected it (${origin})`);
    }
    else {
        logEvent(`added ${added.fragment.key} (${origin})`);
        if (note)
            lineage.set(added.fragment.key, note);
    }
    render();
    return added.fragment;
}
async function addResponse(response, origin, note) {
    // Every mutating route returns {psbt, inspect}; fall back to /api/inspect
    // if a backend ever omits the inspection.
    const inspect = response.inspect ?? (await backend.inspectPsbt(response.psbt));
    return addAndRender(response.psbt, inspect, origin, note);
}
function fragmentByKey(key) {
    return session.fragments.find((fragment) => fragment.key === key) ?? null;
}
// --- contextual enablement -----------------------------------------------------
const ACTION_BUTTONS = [
    ["opJoin", "join"],
    ["opConcatenate", "concatenate"],
    ["opSort", "sort"],
    ["opMakeUnordered", "make-unordered"],
    ["opAtomize", "atomize"],
    ["opAssignIds", "assign-ids"],
    ["opExportV2", "export-v2"],
    ["opExportBip174", "export-bip174"],
    ["payRun", "pay"],
    ["confirmRun", "confirm"],
    ["paymentsRun", "payments"],
    ["syncRun", "sync"],
];
const BASE_TITLE = new Map();
function enablementContext() {
    return {
        selected: selectedFragments(session).map((fragment) => fragmentSummary(fragment.inspect)),
        overrides,
    };
}
function renderOps() {
    const ctx = enablementContext();
    const gates = [];
    for (const [id, action] of ACTION_BUTTONS) {
        const node = el(id);
        const state = actionState(action, ctx);
        node.disabled = !state.enabled;
        const base = BASE_TITLE.get(id) ?? "";
        if (state.enabled && state.overridden && state.gate) {
            node.title = `${base}\nOVERRIDDEN: ${state.gate.label} — ${state.gate.warning}`.trim();
            node.classList.add("session-overridden");
        }
        else {
            node.classList.remove("session-overridden");
            const why = state.reason
                ? state.needsBackend
                    ? `${state.reason} (needs backend: ${state.needsBackend})`
                    : state.reason
                : "";
            node.title = why ? `${base}\ndisabled: ${why}`.trim() : base;
        }
        if (state.gate) {
            gates.push({ action, ...state.gate });
        }
    }
    // Overridable correctness gates: explicit, visible, warning attached.
    const host = el("gateOverrides");
    host.textContent = "";
    for (const gate of gates) {
        const row = document.createElement("label");
        row.className = "session-gate-row";
        const box = document.createElement("input");
        box.type = "checkbox";
        box.checked = overrides.has(gate.id);
        box.addEventListener("change", () => {
            if (box.checked) {
                overrides.add(gate.id);
                logEvent(`override armed for ${gate.action}: ${gate.label}`);
            }
            else {
                overrides.delete(gate.id);
            }
            render();
        });
        row.append(box, span("", ` override for ${gate.action}: ${gate.label}`));
        if (overrides.has(gate.id)) {
            row.append(span("session-gate-warning", ` — ${gate.warning}`));
        }
        host.append(row);
    }
    const selected = selectedFragments(session);
    el("selectionCount").textContent = selected.length
        ? `${selected.length} selected`
        : "none selected";
    el("negotiationTargetLine").textContent =
        selected.length === 1
            ? `Target: ${selected[0].key}. Records are grow-only; results are added as new fragments.`
            : "Targets the one selected fragment (select exactly one). Records are grow-only; results are added as new fragments.";
}
// --- wire gesture ---------------------------------------------------------------
function nodeName(ref) {
    return `${ref.kind} ${ref.key}`;
}
// Transient rejection feedback (the demo's red failure pulse, card-shaped):
// tapping a blocked/unbacked target pulses the card and pins the reason chip
// to it for a moment; the status bar carries the same text persistently.
let wireRejection = null;
function flashWireRejection(ref, text) {
    const rejection = { ref, text };
    wireRejection = rejection;
    window.setTimeout(() => {
        if (wireRejection === rejection) {
            wireRejection = null;
            render();
        }
    }, 1800);
}
function startWire(kind, key) {
    wire = beginWire(kind, key);
    showStatus("", false);
    render();
}
function cancelWire() {
    wire = idleWire();
    render();
}
// Completing a wire gesture QUEUES the edge (compatible verdicts) or
// reports why it cannot wire (blocked/unbacked) — nothing executes on tap.
function wireTo(target) {
    const source = wire.source;
    const done = completeWire(wire, target, objects);
    wire = done.gesture;
    if (!done.verdict || !source) {
        render();
        return;
    }
    const v = done.verdict;
    if (wireDisposition(v) !== "compatible") {
        const action = v.label ?? `${nodeName(source)} → ${nodeName(target)}`;
        const text = wireDisposition(v) === "blocked"
            ? `${action} — blocked: ${v.reason}`
            : v.needs
                ? `${action} is not wired yet — needs backend: ${v.needs}`
                : `${action}: ${v.reason ?? "no join is defined"}`;
        showStatus(text, true);
        logEvent(text);
        flashWireRejection(target, v.reason ?? v.needs ?? "not wireable");
        render();
        return;
    }
    const queued = queueWire(pendingWires, source, target, objects);
    pendingWires = queued.wires;
    if (queued.queued) {
        logEvent(`queued: ${v.label ?? `${nodeName(source)} ⋈ ${nodeName(target)}`} — ` +
            "Join on the wire applies it alone; the toolbar Join applies whole components");
        showStatus("", false);
    }
    else if (queued.duplicate) {
        showStatus(`${v.label ?? "that wire"} is already queued`, false);
    }
    render();
}
// Execute ONE wire now (per-edge Join, or a component's non-join edge).
// The verdict is recomputed at execution time — queued wires can go stale —
// and a failure pulses the target card. Returns whether the wire applied.
// When the toolbar Join passes a remap, wires that consume nodes (session
// merges) record their result there so later wires in the same component
// follow the consumed endpoints.
async function executeWire(source, target, remaps) {
    const v = wireVerdict(source, target, objects);
    if (wireDisposition(v) !== "compatible") {
        const text = `${v.label ?? `${nodeName(source)} → ${nodeName(target)}`} is no longer applicable: ` +
            `${v.reason ?? v.needs ?? "the verdict changed"}`;
        showStatus(text, true);
        logEvent(text);
        flashWireRejection(target, v.reason ?? v.needs ?? "no longer applicable");
        return false;
    }
    try {
        switch (v.kind) {
            case "fragment-join": {
                const left = fragmentByKey(source.key);
                const right = fragmentByKey(target.key);
                if (!left || !right)
                    return false;
                const joined = await addResponse(await backend.joinPsbts([left.psbt, right.psbt]), "join", `⊔ join of ${left.key}, ${right.key}`);
                logEvent(`wired ${left.key} ⋈ ${right.key} → ${joined.key} (lattice join)`);
                break;
            }
            case "fragment-into-session": {
                const sessionKey = source.kind === "session" ? source.key : target.key;
                const fragmentKey = source.kind === "fragment" ? source.key : target.key;
                objects = addFragmentToSession(objects, sessionKey, fragmentKey);
                logEvent(`wired ${fragmentKey} into ${sessionKey}`);
                break;
            }
            case "peer-into-session": {
                const sessionKey = source.kind === "session" ? source.key : target.key;
                const peerKey = source.kind === "peer" ? source.key : target.key;
                // A bridged peer stands for its whole group: the session is wired
                // to EVERY member (the Q3 equivalence), and the broadcast goes
                // through the existing per-member sync where a transport exists.
                const group = bridgeGroupContaining(objects, peerKey);
                for (const memberKey of group) {
                    objects = addPeerToSession(objects, sessionKey, memberKey);
                }
                logEvent(group.length > 1
                    ? `wired bridge group [${group.join(", ")}] into ${sessionKey}; broadcasting`
                    : `wired ${peerKey} into ${sessionKey}; syncing`);
                await broadcastSessionToPeers(sessionKey, group);
                break;
            }
            case "attach-payment": {
                const paymentKey = source.kind === "payment" ? source.key : target.key;
                const fragmentKey = source.kind === "fragment" ? source.key : target.key;
                const payment = objects.payments.find((candidate) => candidate.key === paymentKey);
                const fragment = fragmentByKey(fragmentKey);
                if (!payment || !fragment)
                    return false;
                const paid = await addResponse(await backend.pay(fragment.psbt, {
                    address: payment.address,
                    amountBtc: (payment.amountSats / 100_000_000).toFixed(8),
                    network: displayNetwork(),
                    label: payment.label || undefined,
                    payerHex: undefined,
                }), "pay", `payment ${payment.key} attached to ${fragment.key}`);
                logEvent(`wired ${payment.key} → ${fragment.key}: payment attached, result ${paid.key}`);
                break;
            }
            case "add-create-input": {
                const utxoKey = source.kind === "utxo" ? source.key : target.key;
                const utxo = objects.utxos.find((candidate) => candidate.key === utxoKey);
                addCreateRow("input");
                if (utxo?.txid && utxo.vout !== null) {
                    const rows = el("createInputs");
                    const txids = rows.querySelectorAll("input[data-role=txid]");
                    const vouts = rows.querySelectorAll("input[data-role=vout]");
                    txids[txids.length - 1].value = utxo.txid;
                    vouts[vouts.length - 1].value = String(utxo.vout);
                    logEvent(`wired ${utxoKey} → create: input row prefilled`);
                }
                else {
                    logEvent(`wired ${utxoKey} → create: added an input row, but the transaction is not decoded ` +
                        "(deep classify pending or unavailable) — enter txid:vout manually");
                }
                break;
            }
            case "session-merge": {
                // Client-orchestrated merge (Q3): the UI model unions memberships
                // and retires the sources; the fragment states join through the
                // existing /api/join route. Every decision and every limit of the
                // merge is logged honestly.
                const leftName = sessionByKey(objects, source.key)?.name ?? source.key;
                const rightName = sessionByKey(objects, target.key)?.name ?? target.key;
                const merge = mergeSessions(objects, source.key, target.key);
                if (!merge.merged)
                    return false;
                objects = merge.state;
                remaps?.set(`session:${source.key}`, merge.merged.key);
                remaps?.set(`session:${target.key}`, merge.merged.key);
                logEvent(`merged sessions ${leftName} ⋈ ${rightName} → ${merge.merged.name} ` +
                    `(${merge.merged.fragmentKeys.length} fragment(s), ` +
                    `${merge.merged.peerKeys.length} peer(s) unioned)`);
                for (const note of merge.notes) {
                    logEvent(`session merge: ${note}`);
                }
                const members = merge.merged.fragmentKeys
                    .map((key) => fragmentByKey(key))
                    .filter((fragment) => fragment !== null);
                if (members.length >= 2) {
                    const joined = await addResponse(await backend.joinPsbts(members.map((fragment) => fragment.psbt)), "join", `⊔ session merge of ${leftName}, ${rightName}`);
                    objects = addFragmentToSession(objects, merge.merged.key, joined.key);
                    logEvent(`session merge joined ${members.map((fragment) => fragment.key).join(" ⋈ ")} → ` +
                        `${joined.key} via /api/join (added to ${merge.merged.name})`);
                }
                else {
                    logEvent("session merge: fewer than two member fragment states loaded — nothing to join");
                }
                break;
            }
            case "peer-bridge": {
                objects = addBridge(objects, source.key, target.key);
                objects = unionBridgedPeersIntoSessions(objects);
                const group = bridgeGroupContaining(objects, source.key);
                logEvent(`bridged ${source.key} and ${target.key}: group [${group.join(", ")}] now renders ` +
                    "as one peer; broadcasts address every member (sessions wired to any member are " +
                    "wired to all)");
                break;
            }
            default:
                return false;
        }
        showStatus("", false);
        return true;
    }
    catch (error) {
        reportError(`wire ${v.kind}`, error);
        flashWireRejection(target, error instanceof Error ? error.message : String(error));
        return false;
    }
}
// One n-ary /api/join call for a component's fragment-join cluster (the
// grow-only analog of the demo's successive pairwise LUBs).
async function executeJoinGroup(group) {
    const members = group.fragments
        .map((key) => fragmentByKey(key))
        .filter((fragment) => fragment !== null);
    if (members.length < 2)
        return null;
    try {
        const joined = await addResponse(await backend.joinPsbts(members.map((fragment) => fragment.psbt)), "join", `⊔ join of ${members.map((fragment) => fragment.key).join(", ")}`);
        logEvent(`wired ${members.map((fragment) => fragment.key).join(" ⋈ ")} → ${joined.key} (lattice join)`);
        return joined.key;
    }
    catch (error) {
        reportError("wire fragment-join", error);
        flashWireRejection({ kind: "fragment", key: members[0].key }, error instanceof Error ? error.message : String(error));
        return null;
    }
}
function livePendingWires() {
    pendingWires = pruneWires(pendingWires, objects, session.fragments.map((fragment) => fragment.key));
    return pendingWires;
}
async function joinPendingWire(key) {
    const entry = livePendingWires().find((candidate) => wireKey(candidate.source, candidate.target) === key);
    if (!entry) {
        showStatus("that queued wire is no longer joinable", true);
        render();
        return;
    }
    const applied = await executeWire(entry.source, entry.target);
    if (applied) {
        pendingWires = unqueueWire(pendingWires, key);
    }
    render();
}
// The toolbar Join: apply the whole queue, one connected component at a
// time. Fragment-join clusters collapse into single n-ary joins; the
// remaining wires run with consumed fragment endpoints remapped to their
// cluster's result. Applied wires leave the queue; failed ones stay queued
// (their cards pulse) so the user can retry or cancel.
async function joinAllWires() {
    const components = wireComponents(livePendingWires());
    if (!components.length) {
        showStatus("queue one or more wires before joining", true);
        render();
        return;
    }
    const consumed = new Set();
    let applied = 0;
    let failed = 0;
    for (const component of components) {
        const plan = componentPlan(component);
        const remap = new Map();
        for (const group of plan.joinGroups) {
            const resultKey = await executeJoinGroup(group);
            if (resultKey !== null) {
                applied += group.wires.length;
                for (const wireEntry of group.wires) {
                    consumed.add(wireKey(wireEntry.source, wireEntry.target));
                }
                for (const memberKey of group.fragments) {
                    remap.set(`fragment:${memberKey}`, resultKey);
                }
            }
            else {
                failed += group.wires.length;
            }
        }
        for (const wireEntry of plan.rest) {
            // Session merges in this component record their result into the
            // remap, so later wires follow the merged session.
            const ok = await executeWire(remapWireRef(wireEntry.source, remap), remapWireRef(wireEntry.target, remap), remap);
            if (ok) {
                applied += 1;
                consumed.add(wireKey(wireEntry.source, wireEntry.target));
            }
            else {
                failed += 1;
            }
        }
    }
    pendingWires = pendingWires.filter((wireEntry) => !consumed.has(wireKey(wireEntry.source, wireEntry.target)));
    const summary = `Join applied ${applied} wire${applied === 1 ? "" : "s"} across ` +
        `${components.length} component${components.length === 1 ? "" : "s"}` +
        (failed ? `; ${failed} failed (kept queued)` : "");
    logEvent(summary);
    showStatus(summary, failed > 0);
    render();
}
function clearPendingWires() {
    if (pendingWires.length) {
        logEvent(`cancelled ${pendingWires.length} pending wire(s)`);
    }
    pendingWires = [];
    render();
}
// --- fragment cards --------------------------------------------------------------
const INPUT_ROWS_SHOWN = 3;
const OUTPUT_ROWS_SHOWN = 3;
function renderFragments() {
    const list = el("fragmentList");
    list.textContent = "";
    const focused = focus.mode === "session" && focus.sessionKey ? sessionByKey(objects, focus.sessionKey) : null;
    if (focused) {
        // Single-session focus keeps the flat member list.
        const visible = session.fragments.filter((fragment) => focused.fragmentKeys.includes(fragment.key));
        for (const fragment of visible) {
            list.append(renderFragmentCard(fragment));
        }
        el("fragmentEmpty").hidden = visible.length > 0;
        return;
    }
    // Overview partitions the fragment set by WHERE each fragment lives (Q6):
    // the MINE pseudo-peer holds every sessionless local fragment (loaded and
    // created fragments default there), and each session with loaded members
    // gets its own container — so publishing (wiring Mine → session) is a
    // visible MOVE between areas.
    if (session.fragments.length) {
        const mineKeys = mineFragmentKeys(session.fragments.map((fragment) => fragment.key), objects);
        list.append(renderMineArea(session.fragments.filter((fragment) => mineKeys.includes(fragment.key))));
        for (const sessionObject of objects.sessions) {
            const members = session.fragments.filter((fragment) => sessionObject.fragmentKeys.includes(fragment.key));
            if (members.length) {
                list.append(renderSessionArea(sessionObject, members));
            }
        }
    }
    el("fragmentEmpty").hidden = session.fragments.length > 0;
}
// The MINE pseudo-peer container: a peer-like large area holding the
// sessionless local fragments (Q6). Local-only workflows (join, sort,
// edit, atomize) happen here; wiring a fragment to a session publishes it
// and moves it out.
function renderMineArea(fragments) {
    const item = document.createElement("li");
    item.className = "session-mine-area";
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(span("item-title", "Mine"), badge("pseudo-peer", "session-badge"), span("item-meta", `${fragments.length} local fragment(s), not published to any session`));
    item.append(head);
    item.append(span("item-meta session-area-hint", "Local-only workflows (join, sort, edit, atomize) happen here; wiring a fragment to a session publishes it."));
    const inner = document.createElement("ul");
    inner.className = "item-list session-card-list";
    for (const fragment of fragments) {
        inner.append(renderFragmentCard(fragment));
    }
    item.append(inner);
    if (!fragments.length) {
        item.append(span("item-meta session-area-hint", "empty — every loaded fragment is published"));
    }
    return item;
}
// One container per session with loaded member fragments: the published
// side of the Mine → session move.
function renderSessionArea(sessionObject, members) {
    const item = document.createElement("li");
    item.className = "session-published-area";
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(span("item-title", sessionObject.name), badge("session", "session-badge session-badge-good"), span("item-meta", `${sessionObject.transport} · ${members.length} published fragment(s) · ` +
        `${sessionObject.peerKeys.length} peer(s)`), button("Focus", "Fill the viewport with this session (mobile view)", () => {
        focus = sessionFocus(sessionObject.key);
        render();
    }));
    item.append(head);
    const inner = document.createElement("ul");
    inner.className = "item-list session-card-list";
    for (const fragment of members) {
        inner.append(renderFragmentCard(fragment));
    }
    item.append(inner);
    return item;
}
function renderFragmentCard(fragment) {
    const card = fragmentCardModel(fragment.inspect, displayNetwork());
    const item = document.createElement("li");
    item.className = "list-item session-fragment session-card";
    const ref = { kind: "fragment", key: fragment.key };
    decorateWireTarget(item, ref);
    // Header: select, identity fingerprint, key, badges, fee.
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = fragment.selected;
    checkbox.setAttribute("aria-label", `select ${fragment.key}`);
    checkbox.addEventListener("change", () => {
        session = setSelected(session, fragment.key, checkbox.checked);
        overrides.clear();
        render();
    });
    head.append(checkbox);
    if (card.summary.uniqueIdHex) {
        head.append(lifehashBadge(card.summary.uniqueIdHex, `unordered unique id of ${fragment.key}`));
    }
    head.append(span("item-title", fragment.key));
    for (const view of fragmentBadges(card)) {
        head.append(badge(view.text, badgeToneClass(view.tone), view.emoji, view.title));
    }
    head.append(span("item-meta", fragment.origin));
    item.append(head);
    const note = lineage.get(fragment.key);
    if (note)
        item.append(span("item-meta session-lineage", note));
    // Body: groups with subtotals; details elided, structure shown.
    const body = document.createElement("div");
    body.className = "session-card-body";
    for (const group of card.groups) {
        const groupNode = document.createElement("div");
        groupNode.className = `session-group session-group-${group.kind}`;
        // Group delineation in the descriptor's (or pseudo-descriptor's) color.
        colorizeIdentity(groupNode, groupColorKey(group));
        const title = document.createElement("div");
        title.className = "session-group-title";
        // The header wears the group's script fingerprint when every output
        // shares one script_pubkey (display.js groupChipDigestHex).
        const groupChip = groupChipDigestHex(group);
        if (groupChip) {
            const groupAddress = group.outputs.find((output) => output.address)?.address;
            title.append(lifehashBadge(groupChip, `${groupAddress ?? group.label}\nshared script of every output in this group`));
        }
        title.append(span("", group.label));
        groupNode.append(title);
        // Inputs LEFT, outputs RIGHT — the demo's section layout, card-shaped.
        // The columns collapse to one in narrow cards (container query); the
        // per-row in/out side markers keep the sides readable there.
        const columns = document.createElement("div");
        columns.className = "session-group-columns";
        const inputColumn = document.createElement("div");
        inputColumn.className = "session-group-column session-group-column-inputs";
        const outputColumn = document.createElement("div");
        outputColumn.className = "session-group-column session-group-column-outputs";
        if (group.inputs.length || group.outputs.length) {
            inputColumn.append(span("session-column-heading", "inputs"));
            outputColumn.append(span("session-column-heading", "outputs"));
        }
        for (const input of group.inputs.slice(0, INPUT_ROWS_SHOWN)) {
            inputColumn.append(expandableCoinRow(fragment, "input", input.index, inputRow(input)));
        }
        const inputsHidden = elisionLabel(INPUT_ROWS_SHOWN, group.inputs.length);
        if (inputsHidden)
            inputColumn.append(span("item-meta session-elided", `inputs ${inputsHidden}`));
        for (const output of group.outputs.slice(0, OUTPUT_ROWS_SHOWN)) {
            outputColumn.append(expandableCoinRow(fragment, "output", output.index, outputRow(output)));
        }
        const outputsHidden = elisionLabel(OUTPUT_ROWS_SHOWN, group.outputs.length);
        if (outputsHidden)
            outputColumn.append(span("item-meta session-elided", `outputs ${outputsHidden}`));
        columns.append(inputColumn, outputColumn);
        groupNode.append(columns);
        // Per-group subtotals at the BOTTOM of the columns. With a single
        // group the card-level report directly below would repeat them (the
        // demo's grand-total elision rule, inverted for the card layout).
        if (card.groups.length > 1) {
            groupNode.append(groupBalanceFooter(group));
        }
        body.append(groupNode);
    }
    if (card.groups.length) {
        body.append(balanceReport(card.balance, card.fee.text));
    }
    item.append(body);
    // Footer: per-card actions.
    const foot = document.createElement("div");
    foot.className = "session-card-actions";
    foot.append(button(expanded.has(fragment.key) ? "Hide JSON" : "JSON", "The full inspect JSON dump (not raw bytes — those live behind the export buttons)", () => {
        if (expanded.has(fragment.key)) {
            expanded.delete(fragment.key);
        }
        else {
            expanded.add(fragment.key);
        }
        render();
    }), button("Edit", "Field-by-field editor (liberal parsing; saving mints a new fragment)", () => {
        editor = editorModel(fragment.key, fragment.inspect, displayNetwork());
        pendingEditorFixes.clear();
        editorOverrides.clear();
        renderEditor([]);
        revealPanel("editorPanel");
    }), ...wireButtonNodes(ref, "Connect this fragment to another object (join, session, payment)."), button("Remove", "Drop the fragment from the set", () => {
        session = removeFragment(session, fragment.key);
        objects = dropFragmentKey(objects, fragment.key);
        expanded.delete(fragment.key);
        for (const rowKey of Array.from(expandedRows)) {
            if (rowKey.startsWith(`${fragment.key}:`))
                expandedRows.delete(rowKey);
        }
        lineage.delete(fragment.key);
        logEvent(`removed ${fragment.key}`);
        render();
    }));
    item.append(foot);
    if (expanded.has(fragment.key)) {
        const detail = document.createElement("pre");
        detail.className = "session-fragment-detail";
        detail.textContent = fragment.inspect
            ? JSON.stringify(fragment.inspect, null, 2)
            : "(not decoded)";
        item.append(detail);
    }
    return item;
}
// Emoji + text pill (display.ts fragmentBadges): with an emoji the pill
// collapses to emoji-only in narrow cards (container query; the title
// carries the words); without one the text always shows.
function badge(text, className, emoji = null, title = "") {
    const node = span(className, "");
    if (title)
        node.title = title;
    if (emoji) {
        node.classList.add("session-badge-emoji");
        node.append(span("session-badge-icon", emoji), span("session-badge-label", text));
    }
    else {
        node.textContent = text;
    }
    return node;
}
function badgeToneClass(tone) {
    if (tone === "good")
        return "session-badge session-badge-good";
    if (tone === "warn")
        return "session-badge session-badge-warn";
    return "session-badge";
}
// --- balance report footer (display.ts balanceSheet) --------------------------
//
// Per-group subtotals and whole-transaction totals at the BOTTOM of the
// input/output columns under a sum line — the demo's drawSectionSubtotal
// placement. Numbers that need backend data render as an honest "n/a"
// carrying the seam in the tooltip; deficits are red (via CSS, the amounts
// inherit the color).
function naSlot(why) {
    const node = span("session-balance-na", "n/a");
    node.title = why;
    return node;
}
function balanceCell(side, label, sats, why) {
    const cell = span(`session-balance-cell session-balance-cell-${side}`, "");
    cell.append(span("session-coin-side", label));
    cell.append(sats !== null ? amountSpan(sats) : naSlot(why));
    return cell;
}
const PARTIAL_SUBTOTAL_WHY = "member amounts unknown — a partial sum is not shown as a total";
function groupBalanceFooter(group) {
    const footer = span("session-balance session-balance-group", "");
    footer.append(span("session-balance-sumline", ""));
    const totals = span("session-balance-row session-balance-totals", "");
    if (group.inputs.length > 0) {
        totals.append(balanceCell("input", "in", group.inputSubtotalSats, PARTIAL_SUBTOTAL_WHY));
    }
    if (group.outputs.length > 0) {
        totals.append(balanceCell("output", "out", group.outputSubtotalSats, PARTIAL_SUBTOTAL_WHY));
    }
    footer.append(totals);
    return footer;
}
function balanceReport(sheet, feeText) {
    const block = span("session-balance session-balance-whole", "");
    // The flat fee sentence stays as tooltip/aria text.
    block.title = feeText;
    block.setAttribute("aria-label", feeText);
    // Declared fees sit ABOVE the sum line on the output side and are never
    // rendered as transaction outputs. Elided when known to be zero.
    if (sheet.declaredFeeSats === null || sheet.declaredFeeSats > 0) {
        const row = span("session-balance-row session-balance-declared", "");
        const cell = span("session-balance-cell session-balance-cell-output", "");
        cell.append(span("session-balance-label session-balance-label-muted", "declared fees:"));
        cell.append(sheet.declaredFeeSats !== null
            ? amountSpan(sheet.declaredFeeSats)
            : naSlot("needs backend: totals.declared_fee_sats (inspect extension)"));
        row.append(cell);
        block.append(row);
    }
    block.append(span("session-balance-sumline", ""));
    const totals = span("session-balance-row session-balance-totals", "");
    totals.append(balanceCell("input", "in", sheet.inputTotalSats, "input amounts incomplete (missing UTXO data)"));
    if (!sheet.outputTotalElidedByDeclaredFees) {
        const outCell = balanceCell("output", "out", sheet.outputAccountingTotalSats, "outputs not decoded");
        if (sheet.declaredFeeSats !== null && sheet.declaredFeeSats > 0) {
            outCell.title = "outputs + declared fees";
        }
        totals.append(outCell);
    }
    block.append(totals);
    if (sheet.delta) {
        // The demo's imbalance block: a second thinner sum line (red-tinted for
        // a deficit) and the `balance:` label on the shortfall side.
        block.append(span("session-balance-sumline session-balance-deltaline" +
            (sheet.delta.kind === "deficit" ? " session-balance-deltaline-deficit" : ""), ""));
        const row = span(`session-balance-row session-balance-delta session-balance-${sheet.delta.kind}`, "");
        const cell = span(`session-balance-cell session-balance-cell-${sheet.delta.column}`, "");
        cell.append(span("session-balance-label", "balance:"));
        cell.append(signedAmountSpan(sheet.delta.sats));
        if (sheet.implicitFeeSats !== null && sheet.declaredFeeSats !== null) {
            cell.title = `${sheet.declaredFeeSats} sat declared + ${sheet.implicitFeeSats} sat implicit`;
        }
        row.append(cell);
        block.append(row);
    }
    if (sheet.showFeeRate) {
        const row = span("session-balance-row session-balance-feerate", "");
        row.append(sheet.feeRateText !== null
            ? span("item-meta", sheet.feeRateText)
            : naSlot("fee rate needs backend: totals.size (inspect extension)"));
        if (sheet.feeRateText === null)
            row.prepend(span("item-meta", "fee rate "));
        block.append(row);
    }
    if (sheet.fallbackText) {
        block.append(span("item-meta session-fee-line", sheet.fallbackText));
    }
    return block;
}
// Row expansion: clicking an input/output row toggles a detail block with
// the textual address and EVERY field inspect carries for that index — all
// decoded entry fields plus the raw keymap entries (display.ts
// rowDetailPairs), the counterpart of the chips-instead-of-text card face.
// During a wire gesture the whole card is the tap target, so expansion
// steps aside (the click bubbles to the card's wire handler).
function expandableCoinRow(fragment, side, index, row) {
    const key = `${fragment.key}:${side}:${index}`;
    const host = document.createElement("div");
    host.className = "session-coin-item";
    const open = expandedRows.has(key);
    row.classList.add("session-coin-row-expandable");
    row.setAttribute("role", "button");
    row.setAttribute("aria-expanded", String(open));
    row.tabIndex = 0;
    row.title = `${side} ${index} — click for every field (address, omitted fields, raw keymap entries)`;
    const toggle = () => {
        if (expandedRows.has(key)) {
            expandedRows.delete(key);
        }
        else {
            expandedRows.add(key);
        }
        render();
    };
    row.addEventListener("click", (event) => {
        if (wire.source)
            return; // wiring in progress: the card handles the tap
        if (event.target.closest("button, a, input"))
            return;
        event.stopPropagation();
        toggle();
    });
    row.addEventListener("keydown", (event) => {
        if (wire.source)
            return;
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            toggle();
        }
    });
    host.append(row);
    if (open) {
        const detail = document.createElement("dl");
        detail.className = "session-coin-detail";
        const pairs = rowDetailPairs(fragment.inspect, side, index, displayNetwork());
        for (const pair of pairs) {
            const term = document.createElement("dt");
            term.textContent = pair.label;
            const value = document.createElement("dd");
            value.textContent = pair.value;
            detail.append(term, value);
        }
        if (!pairs.length) {
            const term = document.createElement("dt");
            term.textContent = "(not decoded)";
            detail.append(term);
        }
        host.append(detail);
    }
    return host;
}
function inputRow(input) {
    const row = document.createElement("div");
    row.className = "session-coin-row";
    row.append(span("session-coin-side", "in"));
    if (input.outpointTxid) {
        row.append(lifehashBadge(input.outpointTxid, `outpoint txid (input ${input.index})`));
        row.append(span("item-meta", `:${input.outpointVout ?? "?"}`));
    }
    else {
        row.append(span("item-meta", "outpoint unknown"));
    }
    if (input.knownUtxoSats !== null) {
        row.append(amountSpan(input.knownUtxoSats));
    }
    else {
        row.append(span("item-meta", "amount unknown"));
    }
    if (!input.hasWitnessUtxo && !input.hasNonWitnessUtxo) {
        row.append(span("session-badge session-badge-warn", "no utxo data"));
    }
    return row;
}
function outputRow(output) {
    const row = document.createElement("div");
    row.className = "session-coin-row";
    row.append(span("session-coin-side", "out"));
    if (output.uniqueIdHex) {
        row.append(lifehashBadge(output.uniqueIdHex, `output unique id (output ${output.index})`));
    }
    else {
        row.append(span("session-badge session-badge-warn", "no id"));
    }
    if (output.scriptHex && output.address) {
        // Address as LifeHash chip of the script_pubkey hex — the textual
        // address rides the chip title/aria-label and stays available in the
        // expanded raw view and the field editor.
        row.append(lifehashBadge(output.scriptHex, `${output.address}\n${output.scriptLabel} (output ${output.index})`));
    }
    else if (output.scriptHex) {
        const script = document.createElement("span");
        script.className = "item-meta";
        script.title = output.scriptHex;
        script.textContent = output.scriptLabel;
        row.append(script, lifehashBadge(output.scriptHex, `scriptPubKey (output ${output.index})`));
    }
    else {
        row.append(span("item-meta", "script unknown"));
    }
    if (output.amountSats !== null)
        row.append(amountSpan(output.amountSats));
    return row;
}
// The per-card Wire affordance, action made visible (the wiring verdict
// system's labels): idle it STARTS a gesture; while a gesture is active it
// becomes the completion affordance on other cards — labeled with the
// verdict's action ("Join psbt-1 into psbt-2") — and the cancel affordance
// on the source card. Blocked/unbacked pairs disable it with the reason.
// A "N queued" chip beside it shows the card's pending-wire participation
// (tooltip: the queued action labels).
function wireButtonNodes(ref, idleTitle) {
    const nodes = [];
    if (!wire.source) {
        nodes.push(button("Wire", `${idleTitle}\nCompatible targets light up and their buttons name the action ("Join X into Y").`, () => startWire(ref.kind, ref.key)));
    }
    else if (wire.source.kind === ref.kind && wire.source.key === ref.key) {
        nodes.push(button("Cancel wiring", "Stop the wire gesture without queueing anything", cancelWire));
    }
    else {
        const v = wireVerdict(wire.source, ref, objects);
        const label = v.label ?? "Wire";
        const node = button(label, "", () => wireTo(ref));
        switch (wireDisposition(v)) {
            case "compatible":
                node.title = `${label} — queue this wire`;
                break;
            case "blocked":
                node.disabled = true;
                node.title = `${label} — blocked: ${v.reason ?? ""}`;
                break;
            default:
                node.disabled = true;
                node.title = v.needs
                    ? `${label} — needs backend: ${v.needs}`
                    : (v.reason ?? "no join is defined");
                break;
        }
        nodes.push(node);
    }
    const queued = pendingWires.filter((wireEntry) => (wireEntry.source.kind === ref.kind && wireEntry.source.key === ref.key) ||
        (wireEntry.target.kind === ref.kind && wireEntry.target.key === ref.key));
    if (queued.length) {
        const chip = span("session-badge session-wire-queued-chip", `${queued.length} queued`);
        chip.title = queued
            .map((wireEntry) => wireVerdict(wireEntry.source, wireEntry.target, objects).label ??
            `${nodeName(wireEntry.source)} ⋈ ${nodeName(wireEntry.target)}`)
            .join("\n");
        nodes.push(chip);
    }
    return nodes;
}
// Highlight and arm wire targets while a wire is pending. The three-way
// vocabulary (compatible green / blocked red / unbacked dim) and the action
// label in the title come from the presenter's verdict; a recently rejected
// tap keeps its pulse + reason chip independent of wire mode.
function decorateWireTarget(node, ref) {
    if (wireRejection && wireRejection.ref.kind === ref.kind && wireRejection.ref.key === ref.key) {
        node.classList.add("session-wire-rejected");
        node.append(span("session-wire-reason", wireRejection.text));
    }
    // Cards with at least one queued wire wear the pending-edge vocabulary
    // (the demo's animated orange dashes, card-shaped).
    if (pendingWires.some((wireEntry) => (wireEntry.source.kind === ref.kind && wireEntry.source.key === ref.key) ||
        (wireEntry.target.kind === ref.kind && wireEntry.target.key === ref.key))) {
        node.classList.add("session-wire-pending");
    }
    if (!wire.source)
        return;
    if (wire.source.kind === ref.kind && wire.source.key === ref.key) {
        node.classList.add("session-wire-source");
        return;
    }
    const v = wireVerdict(wire.source, ref, objects);
    switch (wireDisposition(v)) {
        case "compatible":
            node.classList.add("session-wire-target");
            node.title = `wire here: ${v.label ?? v.kind}`;
            break;
        case "blocked":
            node.classList.add("session-wire-incompatible");
            node.title = `${v.label ?? "not wireable"} — blocked: ${v.reason ?? ""}`;
            break;
        default:
            node.classList.add("session-wire-blocked");
            node.title = v.needs
                ? `${v.label ?? "not wireable"} — needs backend: ${v.needs}`
                : (v.reason ?? "no join is defined");
            break;
    }
    node.addEventListener("click", (event) => {
        // The explicit per-card buttons keep working; a plain click on the card
        // body completes (or explains) the pending wire.
        if (event.target.closest("button, input, textarea, select, a"))
            return;
        event.preventDefault();
        wireTo(ref);
    });
}
// --- spatial shelves and remaining objects -----------------------------------------
function renderSessionShelf() {
    const list = el("sessionShelfList");
    list.textContent = "";
    for (const sessionObject of objects.sessions) {
        const item = document.createElement("li");
        item.className = "list-item session-card session-shelf-card";
        const ref = { kind: "session", key: sessionObject.key };
        decorateWireTarget(item, ref);
        const head = document.createElement("div");
        head.className = "session-fragment-row";
        head.append(span("item-title", sessionObject.name), badge("session", "session-badge"), span("item-meta", `${sessionObject.fragmentKeys.length} fragment(s) · ${sessionObject.peerKeys.length} peer(s)`));
        item.append(head);
        const actions = document.createElement("div");
        actions.className = "session-card-actions";
        actions.append(button("Focus", "Fill the viewport with this session", () => {
            focus = sessionFocus(sessionObject.key);
            render();
        }), ...wireButtonNodes(ref, "Publish a fragment to this session."));
        item.append(actions);
        list.append(item);
    }
    el("sessionShelfEmpty").hidden = objects.sessions.length > 0;
}
function unavailablePairButton() {
    const pair = button("Pair unavailable", "Pair unavailable until the ptj adapter exposes session pairing", () => { });
    pair.disabled = true;
    return pair;
}
function renderPeerShelf() {
    const list = el("peerShelfList");
    list.textContent = "";
    for (const group of peerBridgeGroups(objects)) {
        const members = group
            .map((key) => peerByKey(objects, key))
            .filter((member) => member !== null);
        if (!members.length)
            continue;
        list.append(members.length === 1 ? renderPeerCard(members[0]) : renderBridgeGroupCard(members));
    }
    el("peerShelfEmpty").hidden = objects.peers.length > 0;
}
function renderObjects() {
    const list = el("objectList");
    list.textContent = "";
    for (const payment of objects.payments) {
        const item = document.createElement("li");
        item.className = "list-item session-card";
        decorateWireTarget(item, { kind: "payment", key: payment.key });
        const head = document.createElement("div");
        head.className = "session-fragment-row";
        head.append(span("item-title", payment.label || payment.key), badge("payment", "session-badge"), addressNode(payment.address, "payment address"), amountSpan(payment.amountSats));
        item.append(head);
        // Deep classification details (bitcoin-payment-instructions): variant,
        // recipient description, and the instruction's payment methods.
        if (payment.variant || payment.description || payment.methods.length) {
            const parts = [
                payment.variant,
                payment.description,
                ...payment.methods,
            ].filter((part) => part !== null && part !== "");
            item.append(span("item-meta", parts.join(" · ")));
        }
        const actions = document.createElement("div");
        actions.className = "session-card-actions";
        actions.append(button("Prefill Pay", "Copy this instruction into the Pay form", () => {
            el("payAddress").value = payment.address;
            el("payAmount").value = (payment.amountSats / 100_000_000).toFixed(8);
            el("payLabel").value = payment.label;
            logEvent(`prefilled the Pay form from ${payment.key}`);
        }), ...wireButtonNodes({ kind: "payment", key: payment.key }, "Attach this payment to a fragment."));
        item.append(actions);
        list.append(item);
    }
    for (const utxo of objects.utxos) {
        const item = document.createElement("li");
        item.className = "list-item session-card";
        decorateWireTarget(item, { kind: "utxo", key: utxo.key });
        const head = document.createElement("div");
        head.className = "session-fragment-row";
        head.append(span("item-title", utxo.key), badge("signed tx", "session-badge"), span("item-meta", utxo.txid
            ? `${utxo.txid.slice(0, 16)}…:${utxo.vout ?? "?"}`
            : "outputs not decoded (deep classify pending or unavailable)"));
        if (utxo.amountSats !== null)
            head.append(amountSpan(utxo.amountSats));
        if (utxo.fullySigned === false) {
            head.append(badge("inputs not fully signed", "session-badge session-badge-warn"));
        }
        item.append(head);
        if (utxo.address)
            item.append(addressNode(utxo.address, `address of ${utxo.key}`, "item-meta session-address"));
        const actions = document.createElement("div");
        actions.className = "session-card-actions";
        actions.append(...wireButtonNodes({ kind: "utxo", key: utxo.key }, "Use as a create-form input."), button("Copy hex", "Copy the raw transaction hex", () => copyText(utxo.rawTxHex, `${utxo.key} hex`)));
        item.append(actions);
        list.append(item);
    }
    for (const descriptor of objects.descriptors) {
        const item = document.createElement("li");
        item.className = "list-item session-card";
        // Unique palette color per descriptor, keyed by textual identity.
        colorizeIdentity(item, descriptorColorKey(descriptor));
        decorateWireTarget(item, { kind: "descriptor", key: descriptor.key });
        const head = document.createElement("div");
        head.className = "session-fragment-row";
        const text = span("item-meta session-identity", descriptor.descriptor.slice(0, 40) + (descriptor.descriptor.length > 40 ? "…" : ""));
        text.title = descriptor.descriptor;
        head.append(span("session-color-chip", ""), span("item-title", descriptor.key), badge(descriptor.isPrivate ? "descriptor · PRIVATE" : "descriptor", descriptor.isPrivate ? "session-badge session-badge-warn" : "session-badge"), text);
        if (descriptor.descriptorType) {
            head.append(badge(descriptor.descriptorType, "session-badge"));
        }
        item.append(head);
        // Deep classification details (miniscript): the authoritative
        // private-key warning and the first derived addresses/scripts.
        if (descriptor.hasPrivateKeys === true) {
            item.append(span("item-meta session-gate-warning", "contains PRIVATE key material — anyone holding this descriptor can spend from it"));
        }
        if (descriptor.derived.length) {
            // Derived scripts render as LifeHash chips of their script_pubkey hex
            // (never address/script text on the card face; the textual form rides
            // each chip's title/aria-label — display.js chip contract).
            const derivedRow = span("item-meta session-derived-scripts", `derives${descriptor.isRanged ? " (ranged)" : ""}:`);
            for (const entry of descriptor.derived) {
                derivedRow.append(lifehashBadge(entry.scriptPubkeyHex, `${entry.address ?? "derived script"} (derivation index ${entry.index})`));
            }
            item.append(derivedRow);
            item.append(span("item-meta", "matching these scripts to fragments still needs backend (descriptor → fragment wiring)"));
        }
        else {
            item.append(span("item-meta", "deep classification pending — script derivation folds in when /api/classify answers"));
        }
        list.append(item);
    }
}
function renderPeerCard(peer) {
    const item = document.createElement("li");
    item.className = "list-item session-card session-peer-card";
    // The Tableau color follows the immutable transport address, never the
    // editable local label and never a fabricated group fingerprint.
    colorizeIdentity(item, peerColorKey(peer));
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(span("session-color-chip", ""), span("item-title", peer.name), badge(`peer · ${peer.transport}`, "session-badge"));
    const identity = span("item-meta session-identity", peer.identity.slice(0, 24) + (peer.identity.length > 24 ? "…" : ""));
    identity.title = peer.identity;
    head.append(identity);
    item.append(head);
    const actions = document.createElement("div");
    actions.className = "session-card-actions";
    actions.append(button("Copy id", "Copy the full transport identity", () => copyText(peer.identity, `${peer.key} identity`)), unavailablePairButton());
    item.append(actions);
    return item;
}
// A bridged peer group renders as ONE peer node (the demo's green bridge
// block): one card, member chips inside, wired as a unit through its first
// member (the presenter expands any member ref to the whole group).
function renderBridgeGroupCard(members) {
    const item = document.createElement("li");
    item.className = "list-item session-card session-bridge-group";
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(span("item-title", members.map((member) => member.name).join(" + ")), badge(`bridge · ${members.length} peers`, "session-badge session-badge-good"), span("item-meta", "one peer to the session: every member receives every broadcast"));
    item.append(head);
    for (const member of members) {
        const row = document.createElement("div");
        row.className = "session-bridge-member";
        // Each member keeps its pseudo-descriptor identity color so the row
        // still matches the peer's contributed provenance groups.
        colorizeIdentity(row, peerColorKey(member));
        row.append(span("session-color-chip", ""), span("item-title", member.name), badge(`peer · ${member.transport}`, "session-badge"));
        if (!peerUsableForSync(member)) {
            row.append(badge("broadcast pending-backend (no usable transport)", "session-badge session-badge-warn"));
        }
        const identity = span("item-meta session-identity", member.identity.slice(0, 24) + (member.identity.length > 24 ? "…" : ""));
        identity.title = member.identity;
        row.append(identity);
        row.append(button("Copy id", "Copy the full transport identity", () => copyText(member.identity, `${member.key} identity`)));
        item.append(row);
    }
    const actions = document.createElement("div");
    actions.className = "session-card-actions";
    actions.append(unavailablePairButton());
    item.append(actions);
    return item;
}
// --- wire status + focus bar ---------------------------------------------------------
function renderWireStatus() {
    const host = el("wireStatus");
    if (!wire.source) {
        host.hidden = true;
    }
    else {
        host.hidden = false;
        el("wireStatusText").textContent =
            `wiring from ${nodeName(wire.source)} — tap a highlighted card to queue the wire ` +
                "(dimmed cards explain why not)";
    }
    renderWireQueue();
}
// The pending-wire queue panel: one row per queued edge with its action
// label, an edge-local Join, and a discard; the header carries the
// wire/component summary next to the toolbar Join and Cancel wires.
function renderWireQueue() {
    const wires = livePendingWires();
    const host = el("wireQueue");
    const list = el("wireQueueList");
    list.textContent = "";
    if (!wires.length) {
        host.hidden = true;
        return;
    }
    host.hidden = false;
    el("wireQueueSummary").textContent = wireQueueSummary(wires).text;
    for (const wireEntry of wires) {
        const key = wireKey(wireEntry.source, wireEntry.target);
        const v = wireVerdict(wireEntry.source, wireEntry.target, objects);
        const item = document.createElement("li");
        item.className = "session-wire-queue-row";
        item.append(span("session-wire-queue-label", v.label ?? `${nodeName(wireEntry.source)} ⋈ ${nodeName(wireEntry.target)}`), button("Join", "Apply this wire alone", () => void joinPendingWire(key)), button("✕", "Discard this wire without applying it", () => {
            pendingWires = unqueueWire(pendingWires, key);
            logEvent(`discarded pending wire ${nodeName(wireEntry.source)} ⋈ ${nodeName(wireEntry.target)}`);
            render();
        }));
        list.append(item);
    }
}
function renderFocus() {
    focus = validateFocus(focus, objects.sessions.map((sessionObject) => sessionObject.key));
    const focusBar = el("focusBar");
    const inFocus = focus.mode === "session" && focus.sessionKey !== null;
    focusBar.hidden = !inFocus;
    document.body.classList.toggle("session-focused", inFocus);
    if (inFocus && focus.sessionKey) {
        const focused = sessionByKey(objects, focus.sessionKey);
        if (focused) {
            const peers = focused.peerKeys
                .map((key) => peerByKey(objects, key)?.name ?? key)
                .join(", ");
            el("focusTitle").textContent =
                `${focused.name} · ${focused.transport} · ${focused.fragmentKeys.length} fragment(s)` +
                    (peers ? ` · peers: ${peers}` : "");
        }
    }
    for (const panel of Array.from(document.querySelectorAll("[data-focus-hide]"))) {
        // The fragments panel stays: in focus mode it shows the session subset.
        const keep = panel.querySelector("#fragmentList") !== null;
        panel.hidden = inFocus && !keep;
    }
}
// --- editor panel -----------------------------------------------------------------
function renderEditor(violations) {
    const model = editor;
    const host = el("editorSections");
    host.textContent = "";
    if (!model)
        return;
    el("editorTitle").textContent = `Field editor — ${model.fragmentKey}`;
    for (const section of model.sections) {
        const box = document.createElement("fieldset");
        box.className = "session-editor-section";
        const legend = document.createElement("legend");
        legend.textContent = section.title;
        box.append(legend);
        for (const field of section.fields) {
            const row = document.createElement("label");
            row.className = "field-label session-editor-field";
            row.append(span("", field.label));
            const input = document.createElement("input");
            input.value = field.value;
            input.autocomplete = "off";
            input.spellcheck = false;
            input.addEventListener("change", () => {
                if (!editor)
                    return;
                editor = applyEdit(editor, field.path, input.value);
                renderEditor([]);
            });
            row.append(input);
            if (field.error)
                row.append(span("session-status-error", field.error));
            if (field.note)
                row.append(span("item-meta", field.note));
            box.append(row);
        }
        host.append(box);
    }
    const violationsHost = el("editorViolations");
    violationsHost.textContent = "";
    for (const violation of violations) {
        const row = document.createElement("div");
        row.className = "session-editor-violation";
        row.append(span("session-status-error", violation.path ? `${violation.path}: ${violation.message}` : violation.message));
        const fix = violation.fix;
        if (fix) {
            if (violation.source === "server") {
                // Server fix offers run SERVER-side: queue the fix_id and re-save
                // (apply_fixes on the request); the response echoes the applied
                // fix's warning verbatim in applied_fixes[].warning_text.
                row.append(button(fix.label, fix.warning, () => {
                    pendingEditorFixes.add(fix.id);
                    logEvent(`editor: server fix ${fix.id} requested for the next save — ${fix.warning}`);
                    void saveEditor();
                }), span("session-gate-warning", ` ${fix.warning}`));
            }
            else {
                row.append(button(fix.label, fix.warning, () => {
                    if (!editor)
                        return;
                    editor = applyFix(editor, fix.id, (length) => {
                        const bytes = new Uint8Array(length);
                        crypto.getRandomValues(bytes);
                        return bytes;
                    });
                    logEvent(`editor fix applied (${fix.id}) — ${fix.warning}`);
                    renderEditor(validateEditor(editor));
                }), span("session-gate-warning", ` ${fix.warning}`));
            }
        }
        if (violation.source === "server" && violation.overrideParam) {
            const param = violation.overrideParam;
            row.append(button(`Override (${param})`, "Waive this gate explicitly on the next save; the backend re-validates everything else.", () => {
                editorOverrides.add(param);
                logEvent(`editor: override ${param} armed for the next save`);
                void saveEditor();
            }));
        }
        violationsHost.append(row);
    }
    if (!violations.length) {
        violationsHost.append(span("item-meta", "no violations recorded on the last validation"));
    }
}
// Save the editor through the applyPsbtEdits seam (/api/edit): raw-keymap
// rows that changed travel as edits[]; accepted server fixes ride as
// apply_fixes; armed overrides ride as their named boolean params. Success
// mints a NEW fragment; a validation failure feeds the server's violations
// back into the editor's violation -> fix -> revalidate loop.
async function saveEditor() {
    const model = editor;
    if (!model)
        return;
    const fragment = fragmentByKey(model.fragmentKey);
    if (!fragment) {
        showStatus(`editor save: ${model.fragmentKey} is no longer loaded`, true);
        return;
    }
    // Liberal-parse errors block the save: sending a row the parser rejected
    // would silently drop the user's text.
    const localErrors = validateEditor(model).filter((violation) => violation.path !== null);
    if (localErrors.length) {
        renderEditor(localErrors);
        showStatus("editor save: fix the flagged fields first", true);
        return;
    }
    const pristine = editorModel(model.fragmentKey, fragment.inspect, model.network);
    const edits = rawEditsForSave(pristine, model);
    const leftBehind = decodedEditsLeftBehind(pristine, model);
    if (leftBehind.length) {
        // Decoded-field edits have no wire shape on /api/edit (raw keymap only);
        // never drop them silently.
        logEvent(`editor save: decoded-field edits do not travel over /api/edit (raw keymap rows only)` +
            ` — not sent: ${leftBehind.join(", ")}`);
    }
    try {
        const response = await backend.applyPsbtEdits(fragment.psbt, edits, {
            applyFixes: Array.from(pendingEditorFixes),
            overrides: Array.from(editorOverrides),
        });
        if (response.psbt === undefined) {
            // Save-time validation failed: the server's violations run the same
            // loop as local ones (fix offers queue apply_fixes, overrides arm
            // their named params).
            renderEditor(violationsFromServer(response.violations));
            showStatus(response.error ?? "editor save: save-time validation failed", true);
            return;
        }
        // The applied-fix caveats surface VERBATIM (applied_fixes[].warning_text).
        let lastWarning = null;
        for (const applied of response.applied_fixes ?? []) {
            if (applied.warning_text) {
                lastWarning = applied.warning_text;
                logEvent(`editor fix ${applied.fix_id} applied server-side — ${applied.warning_text}`);
            }
            else {
                logEvent(`editor fix ${applied.fix_id} applied server-side`);
            }
        }
        for (const waived of response.overridden ?? []) {
            logEvent(`editor save: gate overridden (${waived.override_param}) — ${waived.message}`);
        }
        const added = await addResponse({ psbt: response.psbt, inspect: response.inspect }, "edit", `edit of ${fragment.key}` +
            (edits.length ? ` (${edits.length} raw edit(s))` : " (validation/fixes only)"));
        logEvent(`editor save minted ${added.key} from ${fragment.key}`);
        pendingEditorFixes.clear();
        editorOverrides.clear();
        editor = null;
        el("editorPanel").hidden = true;
        showStatus(lastWarning ?? "", lastWarning !== null);
    }
    catch (error) {
        reportError("editor save", error);
    }
}
// --- session screen: load + set operations -----------------------------------
async function addPsbtText(raw) {
    const psbt = pastedPsbt(raw) ?? classifyPasteToPsbt(raw);
    if (!psbt)
        return false;
    try {
        // Which decoder applies is a CLASSIFICATION OUTCOME, not a button: try
        // BIP 370 first, fall back to a BIP 174 upgrade (mirrors the demo
        // sandbox's hydratePastedPsbtFragment). The formats share the `psbt`
        // magic.
        try {
            const inspect = await backend.inspectPsbt(psbt);
            addAndRender(psbt, inspect, "paste");
        }
        catch (error) {
            if (!(error instanceof PtjBackendError))
                throw error;
            await addResponse(await backend.importBip174(psbt), "import-bip174");
            logEvent("paste decoded as BIP 174 and upgraded to BIP 370");
        }
        showStatus("", false);
        return true;
    }
    catch (error) {
        reportError("add PSBT", error);
        return true; // it WAS a PSBT; the error is already reported
    }
}
function classifyPasteToPsbt(raw) {
    const pasted = classifyPaste(raw);
    return pasted.kind === "psbt" ? pasted.payload : null;
}
function setAddDrawer(open, focusPeer = false) {
    el("addDrawer").hidden = !open;
    const toggle = el("addDrawerToggle");
    toggle.setAttribute("aria-expanded", String(open));
    if (open) {
        if (focusPeer)
            el("manualPeerAddress").focus();
        else
            el("pasteInput").focus();
    }
}
function addManualPeer(event) {
    event.preventDefault();
    const identity = inputValue("manualPeerAddress").trim();
    if (!identity) {
        showStatus("A transport address is required.", true);
        return;
    }
    const minted = mintPeer(objects, inputValue("manualPeerLabel"), selectValue("manualPeerTransport"), identity);
    objects = minted.state;
    logEvent(minted.created
        ? `added inert ${minted.peer.key} (${minted.peer.transport}); no session or transport changed`
        : `selected existing ${minted.peer.key}; exact transport address already present`);
    el("manualPeerLabel").value = "";
    el("manualPeerAddress").value = "";
    showStatus("", false);
    setAddDrawer(false);
    render();
}
async function addObject() {
    const raw = textareaValue("pasteInput");
    const pasted = classifyPaste(raw);
    if (pasted.kind === "psbt") {
        if (await addPsbtText(raw)) {
            el("pasteInput").value = "";
        }
        return;
    }
    const minted = mintFromPaste(objects, pasted);
    objects = minted.state;
    logEvent(minted.log);
    if (minted.minted) {
        el("pasteInput").value = "";
        if (pasted.needsBackend) {
            logEvent(`${minted.minted.key}: deep parsing pending — needs backend: ${pasted.needsBackend}`);
        }
        showStatus("", false);
        void enrichFromClassify(minted.minted, pasted);
    }
    else {
        showStatus(pasted.detail, true);
    }
    render();
}
// Deep classification (Backend.classifyPaste -> /api/classify): the shallow
// node renders instantly and the deep details fold in when the backend
// answers — miniscript-validated descriptors (normalized public form,
// derived scripts, the authoritative private-key flag), payment-method
// details, and transaction decodes into per-output utxo nodes. Failure
// degrades to the shallow card with an event-log note (an adapter without
// the seam, e.g. wasm today, rejects with a clear error).
async function enrichFromClassify(node, pasted) {
    if (pasted.kind !== "descriptor" &&
        pasted.kind !== "payment-uri" &&
        pasted.kind !== "transaction-hex") {
        return;
    }
    try {
        const classified = await backend.classifyPaste(pasted.payload, displayNetwork());
        switch (node.kind) {
            case "descriptor":
                objects = enrichDescriptor(objects, node.key, classified);
                logEvent(`${node.key}: deep classification folded in (${classified.kind})`);
                break;
            case "payment":
                objects = enrichPayment(objects, node.key, classified);
                logEvent(`${node.key}: deep classification folded in (${classified.kind})`);
                break;
            case "utxo": {
                const applied = applyTxOutputs(objects, node.key, classified);
                objects = applied.state;
                logEvent(applied.utxos.length
                    ? `${node.key}: transaction decoded — ${applied.utxos.length} output(s) as spendable outpoints`
                    : `${node.key}: deep classification returned no decodable outputs`);
                break;
            }
            default:
                break;
        }
        render();
    }
    catch (error) {
        logEvent(`${node.key}: deep classification unavailable — ` +
            (error instanceof Error ? error.message : String(error)));
    }
}
async function loadUpload() {
    const input = el("uploadInput");
    const file = input.files?.[0];
    if (!file)
        return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    const text = new TextDecoder().decode(bytes).trim();
    // A .psbt file is either raw binary or already-base64 text; both end up as
    // base64 in the paste box, decoded when the user hits Add (BIP 370 first,
    // BIP 174 upgrade as the fallback — the same auto-classification as a
    // direct paste).
    el("pasteInput").value =
        pastedPsbt(text) ?? bytesToBase64(bytes);
    logEvent(`loaded ${file.name} into the paste box`);
    input.value = "";
}
function requireEnabled(action) {
    const state = actionState(action, enablementContext());
    if (!state.enabled) {
        showStatus(`${action}: ${state.reason ?? "not available"}`, true);
        return null;
    }
    return selectedFragments(session);
}
async function joinSelected() {
    const selected = requireEnabled("join");
    if (!selected)
        return;
    try {
        await addResponse(await backend.joinPsbts(selected.map((f) => f.psbt)), "join", `⊔ join of ${selected.map((f) => f.key).join(", ")}`);
        showStatus("", false);
    }
    catch (error) {
        reportError("join", error);
    }
}
async function concatenateSelected() {
    const selected = requireEnabled("concatenate");
    if (!selected)
        return;
    try {
        await addResponse(await backend.concatenatePsbts(selected.map((f) => f.psbt)), "concatenate", `concatenation of ${selected.map((f) => f.key).join(", ")}`);
        showStatus("", false);
    }
    catch (error) {
        reportError("concatenate", error);
    }
}
async function sortSelected() {
    const selected = requireEnabled("sort");
    if (!selected)
        return;
    const seed = inputValue("sortSeed").trim();
    try {
        await addResponse(await backend.sortPsbt(selected[0].psbt, seed || undefined), "sort", `sort of ${selected[0].key}`);
        showStatus("", false);
    }
    catch (error) {
        reportError("sort", error);
    }
}
async function makeUnorderedSelected() {
    const selected = requireEnabled("make-unordered");
    if (!selected)
        return;
    try {
        await addResponse(await backend.makeUnordered(selected[0].psbt), "make-unordered", `make-unordered of ${selected[0].key}`);
        showStatus("", false);
    }
    catch (error) {
        reportError("make unordered", error);
    }
}
// --- override fixes -----------------------------------------------------------
//
// An armed override on a gate that carries a fix APPLIES the repair instead
// of sending the blocked request as-is (the backend would reject it — an
// escape hatch into a guaranteed 400 is no escape hatch). Every repair mints
// a NEW fragment through the normal grow-only path, so the provenance stays
// visible in the fragment set.
// PSBT_GLOBAL_TX_MODIFIABLE: raw global key 0x06, value 0x03 = bits 0
// (inputs) + 1 (outputs) modifiable — the field-edit route's raw handles.
const TX_MODIFIABLE_KEY_HEX = "06";
const TX_MODIFIABLE_BOTH_HEX = "03";
async function applySetTxModifiableFix(fragment) {
    const response = await backend.applyPsbtEdits(fragment.psbt, [
        { map: "global", key: TX_MODIFIABLE_KEY_HEX, value: TX_MODIFIABLE_BOTH_HEX },
    ]);
    if (response.psbt === undefined) {
        throw new Error(response.error ?? "the tx-modifiable raw edit failed save-time validation");
    }
    const minted = await addResponse({ psbt: response.psbt, inspect: response.inspect }, "edit", `raw edit of ${fragment.key}: TX_MODIFIABLE set to both (override fix)`);
    logEvent(`override fix: ${fragment.key} → ${minted.key} (TX_MODIFIABLE flags set via /api/edit)`);
    return minted;
}
async function applySortFirstFix(fragment) {
    let seed = inputValue("sortSeed").trim() || undefined;
    if (!seed && !fragmentSummary(fragment.inspect).seedHex) {
        // The sorter role needs PSBT_GLOBAL_SORT_SEED; the fragment carries none
        // and the field is blank, so generate one (the create form's spec
        // minimum: 128 bits) and say so.
        const bytes = new Uint8Array(16);
        crypto.getRandomValues(bytes);
        seed = seedFromRandomBytes(bytes);
        logEvent(`override fix: generated a random sort seed (${seed}) — fill the sort-seed field to control it`);
    }
    const sorted = await addResponse(await backend.sortPsbt(fragment.psbt, seed), "sort", `sort of ${fragment.key} (override fix)`);
    logEvent(`override fix: ${fragment.key} → ${sorted.key} (sorted via /api/sort)`);
    return sorted;
}
// The gate's armed-override repair for this action, if any (null = run the
// action on the selection as-is, the send-as-is override semantics).
function armedOverrideFix(action) {
    const state = actionState(action, enablementContext());
    return state.enabled && state.overridden ? (state.gate?.fix ?? null) : null;
}
async function atomizeSelected() {
    const selected = requireEnabled("atomize");
    if (!selected)
        return;
    const fix = armedOverrideFix("atomize");
    try {
        let target = selected[0];
        if (fix?.kind === "set-tx-modifiable") {
            target = await applySetTxModifiableFix(target);
        }
        const response = await backend.atomizePsbt(target.psbt);
        let index = 0;
        for (const piece of response.fragments) {
            index += 1;
            await addResponse(piece, "atomize", `atom ${index}/${response.fragments.length} of ${target.key}`);
        }
        logEvent(`atomize produced ${response.fragments.length} fragments`);
        showStatus("", false);
    }
    catch (error) {
        reportError("atomize", error);
    }
}
// --- assign ids -------------------------------------------------------------
//
// Backend.assignIds carries the /api/assign-ids contract: manual per-output
// directives ({target: "out", index, id}) combine with auto-fill of the
// remainder. The id text goes to the backend VERBATIM — it is parsed
// liberally server-side (hex/base58/bech32 by character set), the UI adds no
// parsing of its own. Overwriting an existing id is the explicit
// per-invocation choice (strict by default, overridable — the backend
// re-validates either way). Input-map ids (PSBT_IN_UNIQUE_ID) have no
// inspect surface yet, so the panel lists outputs only.
function openAssignIds() {
    const selected = requireEnabled("assign-ids");
    if (!selected)
        return;
    const fragment = selected[0];
    assignIdsTarget = fragment.key;
    renderAssignIds(fragment);
    revealPanel("assignIdsPanel");
}
function renderAssignIds(fragment) {
    el("assignIdsTitle").textContent = `Assign unique ids — ${fragment.key}`;
    const host = el("assignIdsRows");
    host.textContent = "";
    const outputs = asArray(asObject(fragment.inspect)?.outputs) ?? [];
    outputs.forEach((raw, index) => {
        const current = asString(asObject(raw)?.unique_id_hex);
        const row = document.createElement("label");
        row.className = "field-label session-editor-field";
        row.append(span("", `output ${index}${current ? "" : " — id missing"}`));
        const input = document.createElement("input");
        input.dataset.index = String(index);
        input.autocomplete = "off";
        input.spellcheck = false;
        input.placeholder = current
            ? `${current.slice(0, 16)}… (blank keeps the current id)`
            : "blank auto-assigns (or type an id: hex/base58/bech32)";
        row.append(input);
        host.append(row);
    });
    el("assignIdsAuto").checked = true;
    el("assignIdsOverwrite").checked = false;
}
async function runAssignIds() {
    const fragment = assignIdsTarget ? fragmentByKey(assignIdsTarget) : null;
    if (!fragment) {
        assignIdsTarget = null;
        el("assignIdsPanel").hidden = true;
        return;
    }
    const ids = Array.from(el("assignIdsRows").querySelectorAll("input[data-index]"))
        .filter((input) => input.value.trim())
        .map((input) => ({
        target: "out",
        index: Number(input.dataset.index),
        id: input.value.trim(),
    }));
    const auto = el("assignIdsAuto").checked;
    const overwrite = el("assignIdsOverwrite").checked;
    try {
        const added = await addResponse(await backend.assignIds(fragment.psbt, {
            ids: ids.length ? ids : undefined,
            auto,
            overwrite,
        }), "assign-ids", `assign-ids of ${fragment.key}`);
        logEvent(`assign-ids minted ${added.key} from ${fragment.key}` +
            ` (${ids.length} manual id(s), auto=${auto}, overwrite=${overwrite})`);
        assignIdsTarget = null;
        el("assignIdsPanel").hidden = true;
        showStatus("", false);
    }
    catch (error) {
        reportError("assign ids", error);
    }
}
function exportSelectedV2() {
    const selected = requireEnabled("export-v2");
    if (!selected)
        return;
    showOutput(`${selected[0].key} — BIP 370 base64`, selected[0].psbt);
}
async function exportSelectedBip174() {
    const selected = requireEnabled("export-bip174");
    if (!selected)
        return;
    const fix = armedOverrideFix("export-bip174");
    try {
        let target = selected[0];
        if (fix?.kind === "sort-first") {
            target = await applySortFirstFix(target);
        }
        const exported = await backend.exportBip174(target.psbt);
        showOutput(`${target.key} — BIP 174 base64`, exported.psbt);
        showStatus("", false);
    }
    catch (error) {
        reportError("export BIP 174", error);
    }
}
// --- create screen ------------------------------------------------------------
function rowValues(container, selector) {
    return Array.from(container.querySelectorAll(selector)).map((input) => input.value);
}
function createFormInputs() {
    const rows = el("createInputs");
    const txids = rowValues(rows, "input[data-role=txid]");
    const vouts = rowValues(rows, "input[data-role=vout]");
    return txids.map((txid, index) => ({ txid, vout: vouts[index] ?? "" }));
}
function createFormOutputs() {
    const rows = el("createOutputs");
    const addresses = rowValues(rows, "input[data-role=address]");
    const amounts = rowValues(rows, "input[data-role=amount]");
    return addresses.map((address, index) => ({ address, amountBtc: amounts[index] ?? "" }));
}
function addCreateRow(kind) {
    const container = el(kind === "input" ? "createInputs" : "createOutputs");
    const row = document.createElement("div");
    row.className = "split-row";
    if (kind === "input") {
        row.innerHTML =
            '<label class="field-label">txid' +
                '<input data-role="txid" autocomplete="off" spellcheck="false" placeholder="64 hex chars"></label>' +
                '<label class="field-label compact">vout' +
                // Placeholder, not value: a pristine row must count as BLANK so the
                // zero-row create path stays reachable; an omitted vout defaults to 0
                // when a txid is entered (buildCreateRequest).
                '<input data-role="vout" type="number" min="0" placeholder="0"></label>';
    }
    else {
        row.innerHTML =
            '<label class="field-label">address' +
                '<input data-role="address" autocomplete="off" spellcheck="false" placeholder="bcrt1q…"></label>' +
                '<label class="field-label compact">amount (BTC)' +
                '<input data-role="amount" autocomplete="off" inputmode="decimal" placeholder="0.00050000"></label>';
    }
    container.append(row);
}
async function createPsbt(event) {
    event.preventDefault();
    const built = buildCreateRequest({
        network: selectValue("createNetwork"),
        ordering: selectValue("createOrdering"),
        seed: inputValue("createSeed"),
        inputs: createFormInputs(),
        outputs: createFormOutputs(),
    });
    if (built.ok === false) {
        showStatus(built.error, true);
        return;
    }
    try {
        await addResponse(await backend.createPsbt(built.value), "create");
        showStatus("", false);
    }
    catch (error) {
        reportError("create", error);
    }
}
// --- sync panel ----------------------------------------------------------------
function syncTransportValue() {
    return selectValue("syncTransport");
}
// --- compile-time capabilities (GET /api/capabilities) ----------------------
//
// Which transports THIS ptj binary can drive is a compile-time fact; the
// Sync dropdown reflects it up front (disabled option + the rebuild hint)
// instead of failing on use. Fetched directly rather than through the
// Backend seam: this is deployment metadata of the HTTP shell, not a PSBT
// operation. An older server without the route degrades to
// everything-enabled with precise use-time errors.
const SYNC_TRANSPORT_CAPABILITY = {
    iroh: { key: "iroh", feature: "iroh-sync" },
    str0m: { key: "str0m", feature: "str0m" },
    "webrtc-rs": { key: "webrtc_rs", feature: "webrtc-rs" },
};
let transportCapabilities = null;
function transportUnavailable(transport) {
    const mapping = SYNC_TRANSPORT_CAPABILITY[transport];
    if (!mapping || !transportCapabilities)
        return null;
    return transportCapabilities[mapping.key] === false
        ? `${transport} is unavailable in this build — rebuild ptj with --features ${mapping.feature}`
        : null;
}
function markSyncTransportOptions() {
    if (!transportCapabilities)
        return;
    const select = el("syncTransport");
    for (const option of Array.from(select.options)) {
        const mapping = SYNC_TRANSPORT_CAPABILITY[option.value];
        if (!mapping || transportCapabilities[mapping.key] !== false)
            continue;
        option.disabled = true;
        if (!option.text.includes("unavailable")) {
            option.text = `${option.text} — unavailable in this build (rebuild with --features ${mapping.feature})`;
        }
        if (select.value === option.value) {
            select.value = "local";
            renderSyncFields();
        }
    }
}
async function loadCapabilities() {
    try {
        const response = await fetch("/api/capabilities");
        if (!response.ok)
            throw new Error(`HTTP ${response.status}`);
        const transports = asObject(asObject((await response.json()))?.transports);
        if (!transports)
            return;
        const capabilities = {};
        for (const [key, value] of Object.entries(transports)) {
            capabilities[key] = value === true;
        }
        transportCapabilities = capabilities;
        markSyncTransportOptions();
        const off = Object.entries(SYNC_TRANSPORT_CAPABILITY)
            .filter(([, mapping]) => capabilities[mapping.key] === false)
            .map(([name, mapping]) => `${name} (rebuild with --features ${mapping.feature})`);
        if (off.length) {
            logEvent(`this build lacks sync transports: ${off.join(", ")}`);
        }
    }
    catch (error) {
        logEvent("transport availability unknown (/api/capabilities did not answer) — " +
            (error instanceof Error ? error.message : String(error)));
    }
}
function renderSyncFields() {
    const transport = syncTransportValue();
    for (const section of Array.from(document.querySelectorAll("[data-transport]"))) {
        const kinds = (section.dataset.transport ?? "").split(" ");
        section.hidden = !kinds.includes(transport);
    }
}
function setSyncState(state, detail) {
    const chip = el("syncStateChip");
    chip.textContent = state === "ok" ? "converged" : state;
    chip.className = `session-sync-chip session-sync-${state}`;
    el("syncStateDetail").textContent = detail;
}
function pushSyncResult(text) {
    const results = el("syncResults");
    const item = document.createElement("li");
    item.textContent = text;
    results.prepend(item);
    while (results.children.length > 8) {
        results.lastElementChild?.remove();
    }
}
function syncFormSnapshot() {
    return {
        transport: syncTransportValue(),
        sources: textareaValue("syncSources"),
        state: inputValue("syncState"),
        irohTicket: textareaValue("syncIrohTicket"),
        irohTicketOut: el("syncIrohTicketOut").checked,
        irohWaitMs: inputValue("syncIrohWaitMs"),
        webrtcRole: selectValue("syncWebrtcRole"),
        signalOut: inputValue("syncSignalOut"),
        signalIn: inputValue("syncSignalIn"),
        webrtcBind: inputValue("syncWebrtcBind"),
        iceServers: textareaValue("syncIceServers"),
        signalTimeoutMs: inputValue("syncSignalTimeoutMs"),
    };
}
async function runSyncRequest(psbts, sourceLabel) {
    const built = buildSyncRequest(syncFormSnapshot(), psbts);
    if (built.ok === false) {
        showStatus(built.error, true);
        setSyncState("error", built.error);
        return;
    }
    // buildSyncRequest always sets transport; the DTO type keeps it optional
    // for the legacy no-transport request shape.
    const unavailable = transportUnavailable(built.value.transport ?? "local");
    if (unavailable) {
        showStatus(unavailable, true);
        setSyncState("error", unavailable);
        return;
    }
    const runButton = el("syncRun");
    runButton.disabled = true;
    setSyncState("syncing", `${sourceLabel} over ${built.value.transport}…`);
    showStatus("syncing…", false);
    try {
        const response = await backend.syncPsbts(built.value);
        let summary;
        if (response.psbt !== undefined) {
            const converged = await addResponse({ psbt: response.psbt, inspect: response.inspect }, "sync", `sync convergence (${sourceLabel})`);
            const view = negotiationView(response);
            summary =
                `${sourceLabel}: converged into ${converged.key}; ` +
                    `${view.paymentCount} payment record(s), ${view.confirmationCount} confirmation record(s) out of band`;
        }
        else {
            // Ticket-only response: the request minted an EMPTY shared document —
            // nothing to converge, no fragment to add (a fabricated one would
            // misstate the document contents).
            summary =
                `${sourceLabel}: created an empty shared document — share the ticket so peers can publish into it`;
        }
        setSyncState("ok", summary);
        pushSyncResult(summary);
        if (response.irohTicketOut) {
            el("syncTicketBody").value = response.irohTicketOut;
            el("syncTicketPanel").hidden = false;
            pushSyncResult("created a new iroh document — ticket ready to share (Copy above)");
        }
        logEvent(summary);
        showStatus("", false);
    }
    catch (error) {
        setSyncState("error", error instanceof Error ? error.message : String(error));
        pushSyncResult(`sync failed: ${error instanceof Error ? error.message : String(error)}`);
        reportError("sync", error);
    }
    finally {
        runButton.disabled = false;
        render();
    }
}
async function runSync(event) {
    event.preventDefault();
    const state = actionState("sync", enablementContext());
    // Zero-selection syncs that are legitimate: local sync runs from
    // server-side sources, and iroh with ticket-out CREATES an empty shared
    // document (peers publish into it later). Every other shape syncs the
    // selection.
    const createsEmptyDoc = syncTransportValue() === "iroh" && el("syncIrohTicketOut").checked;
    if (!state.enabled && syncTransportValue() !== "local" && !createsEmptyDoc) {
        showStatus(`sync: ${state.reason ?? "not available"}`, true);
        return;
    }
    const psbts = selectedFragments(session).map((fragment) => fragment.psbt);
    await runSyncRequest(psbts, `sync of ${psbts.length} selected fragment(s)`);
}
// Peer→session wiring: sync the session's member fragments over the peer's
// transport (or the session's own transport when no peer is given). The
// transport parameters ride the sync form so the manual-signaling transports
// stay configurable; iroh peers bring their ticket along.
async function syncSessionOverPeer(sessionKey, peerKey) {
    const sessionObject = sessionByKey(objects, sessionKey);
    if (!sessionObject)
        return;
    const peer = peerKey ? peerByKey(objects, peerKey) : null;
    const transport = peer && peer.transport !== "nostr" && peer.transport !== "unknown"
        ? peer.transport
        : sessionObject.transport;
    el("syncTransport").value = transport;
    renderSyncFields();
    if (peer && peer.transport === "iroh" && peer.identity) {
        el("syncIrohTicket").value = peer.identity;
        el("syncIrohTicketOut").checked = false;
    }
    const members = sessionObject.fragmentKeys
        .map((key) => fragmentByKey(key))
        .filter((fragment) => fragment !== null)
        .map((fragment) => fragment.psbt);
    if (!members.length && transport !== "local") {
        showStatus(`${sessionObject.name}: wire fragments into the session before syncing`, true);
        return;
    }
    await runSyncRequest(members, `session ${sessionObject.name} (${members.length} fragment(s))`);
}
// Broadcast semantics for a bridged peer group (Q3): every member receives
// every broadcast. Today's transport reality: members with a configured
// transport sync one by one through the existing /api/sync (sequential —
// the sync form is shared state); members without one are honestly marked
// pending-backend instead of being silently skipped.
async function broadcastSessionToPeers(sessionKey, peerKeys) {
    for (const memberKey of peerKeys) {
        const member = peerByKey(objects, memberKey);
        if (!member)
            continue;
        if (peerUsableForSync(member)) {
            await syncSessionOverPeer(sessionKey, memberKey);
        }
        else {
            logEvent(`broadcast to ${member.name} (${member.transport}) is pending-backend: ` +
                "no usable transport behind /api/sync yet — the member stays wired and " +
                "receives the session when its transport lands");
        }
    }
}
// --- negotiation panel -----------------------------------------------------------
function payMode() {
    return el("payModeHex").checked ? "hex" : "address";
}
function confirmMode() {
    return el("confirmModeHex").checked ? "hex" : "derive";
}
function renderNegotiationModes() {
    const pay = payMode();
    el("payAddressFields").hidden = pay !== "address";
    el("payHexFields").hidden = pay !== "hex";
    const confirm = confirmMode();
    el("confirmDeriveFields").hidden = confirm !== "derive";
    el("confirmHexFields").hidden = confirm !== "hex";
}
async function runPay(event) {
    event.preventDefault();
    const selected = requireEnabled("pay");
    if (!selected)
        return;
    const target = selected[0];
    const built = buildPayArgs({
        mode: payMode(),
        address: inputValue("payAddress"),
        amountBtc: inputValue("payAmount"),
        network: selectValue("payNetwork"),
        label: inputValue("payLabel"),
        payerHex: inputValue("payPayerHex"),
        paymentHex: textareaValue("payPaymentHex"),
        secretHex: inputValue("paySecretHex"),
        dummy: inputValue("payDummy"),
    });
    if (built.ok === false) {
        showStatus(built.error, true);
        return;
    }
    try {
        await addResponse(await backend.pay(target.psbt, built.value.payment, built.value.options), "pay", `payment record attached to ${target.key}`);
        logEvent(`payment record attached to ${target.key} (result added)`);
        showStatus("", false);
    }
    catch (error) {
        reportError("pay", error);
    }
}
async function runConfirm(event) {
    event.preventDefault();
    const selected = requireEnabled("confirm");
    if (!selected)
        return;
    const target = selected[0];
    const built = buildConfirmArgs({
        mode: confirmMode(),
        confirmationHex: textareaValue("confirmHex"),
        peerIdHex: inputValue("confirmPeerIdHex"),
        secretHex: inputValue("confirmSecretHex"),
    });
    if (built.ok === false) {
        showStatus(built.error, true);
        return;
    }
    try {
        await addResponse(await backend.confirm(target.psbt, built.value.confirmation, built.value.options), "confirm", `confirmation attached to ${target.key}`);
        logEvent(`confirmation attached to ${target.key} (result added)`);
        showStatus("", false);
    }
    catch (error) {
        reportError("confirm", error);
    }
}
async function listPayments(event) {
    event.preventDefault();
    const selected = requireEnabled("payments");
    if (!selected)
        return;
    const target = selected[0];
    const secret = inputValue("paymentsSecretHex").trim();
    try {
        const response = await backend.payments(target.psbt, secret ? { secretHex: secret.toLowerCase() } : undefined);
        const view = negotiationView(response);
        const summary = fragmentSummary(target.inspect);
        const lines = [
            `fragment: ${target.key}`,
            `unordered unique id: ${summary.uniqueIdHex ?? "(not decoded)"}`,
            "",
            `payments (${view.paymentCount}):`,
            ...view.payments.map((record) => `  ${record}`),
            "",
            `confirmations (${view.confirmationCount}):`,
            ...view.confirmations.map((record) => `  ${record}`),
        ];
        showOutput(`negotiation band of ${target.key}`, lines.join("\n"));
        showStatus("", false);
    }
    catch (error) {
        reportError("payments", error);
    }
}
// --- render root -----------------------------------------------------------------
function render() {
    renderFocus();
    renderWireStatus();
    renderPeerShelf();
    renderSessionShelf();
    renderFragments();
    renderObjects();
    renderOps();
    el("createWireTarget").hidden = !(wire.source && wire.source.kind === "utxo");
}
// --- wiring (DOM event hookup) -----------------------------------------------------
function wireDom() {
    for (const [id] of ACTION_BUTTONS) {
        BASE_TITLE.set(id, el(id).title);
    }
    el("addObject").addEventListener("click", () => void addObject());
    el("uploadInput").addEventListener("change", () => void loadUpload());
    el("addDrawerToggle").addEventListener("click", () => {
        setAddDrawer(el("addDrawer").hidden);
    });
    el("addDrawerClose").addEventListener("click", () => setAddDrawer(false));
    el("addPeerQuick").addEventListener("click", () => setAddDrawer(true, true));
    el("manualPeerForm").addEventListener("submit", addManualPeer);
    el("opJoin").addEventListener("click", () => void joinSelected());
    el("opConcatenate").addEventListener("click", () => void concatenateSelected());
    el("opSort").addEventListener("click", () => void sortSelected());
    el("opMakeUnordered").addEventListener("click", () => void makeUnorderedSelected());
    el("opAtomize").addEventListener("click", () => void atomizeSelected());
    el("opExportV2").addEventListener("click", exportSelectedV2);
    el("opExportBip174").addEventListener("click", () => void exportSelectedBip174());
    el("opAssignIds").addEventListener("click", openAssignIds);
    el("assignIdsRun").addEventListener("click", () => void runAssignIds());
    el("assignIdsClose").addEventListener("click", () => {
        el("assignIdsPanel").hidden = true;
    });
    el("displayNetwork").addEventListener("change", render);
    el("wireCancel").addEventListener("click", cancelWire);
    el("wireJoinAll").addEventListener("click", () => void joinAllWires());
    el("wireClearAll").addEventListener("click", clearPendingWires);
    el("focusBack").addEventListener("click", () => {
        focus = overviewFocus();
        render();
    });
    el("newSessionForm").addEventListener("submit", (event) => {
        event.preventDefault();
        const minted = mintSession(objects, inputValue("newSessionName"), selectValue("newSessionTransport"));
        objects = minted.state;
        el("newSessionName").value = "";
        logEvent(`created ${minted.session.key} (${minted.session.name}, ${minted.session.transport})`);
        render();
    });
    el("createForm").addEventListener("submit", (event) => void createPsbt(event));
    el("createAddInput").addEventListener("click", () => addCreateRow("input"));
    el("createAddOutput").addEventListener("click", () => addCreateRow("output"));
    el("createGenerateSeed").addEventListener("click", () => {
        // Spec: PSBT_GLOBAL_SORT_SEED must carry at least 128 bits of randomness.
        const bytes = new Uint8Array(16);
        crypto.getRandomValues(bytes);
        el("createSeed").value = seedFromRandomBytes(bytes);
    });
    el("createWireTarget").addEventListener("click", () => {
        if (wire.source?.kind === "utxo") {
            wireTo({ kind: "create", key: "create" });
        }
    });
    el("syncTransport").addEventListener("change", renderSyncFields);
    el("syncForm").addEventListener("submit", (event) => void runSync(event));
    el("syncTicketCopy").addEventListener("click", () => {
        copyText(el("syncTicketBody").value, "iroh ticket");
    });
    for (const id of ["payModeAddress", "payModeHex", "confirmModeDerive", "confirmModeHex"]) {
        el(id).addEventListener("change", renderNegotiationModes);
    }
    el("payForm").addEventListener("submit", (event) => void runPay(event));
    el("confirmForm").addEventListener("submit", (event) => void runConfirm(event));
    el("paymentsForm").addEventListener("submit", (event) => void listPayments(event));
    el("editorClose").addEventListener("click", () => {
        editor = null;
        pendingEditorFixes.clear();
        editorOverrides.clear();
        el("editorPanel").hidden = true;
    });
    el("editorValidate").addEventListener("click", () => {
        if (!editor)
            return;
        renderEditor(validateEditor(editor));
    });
    el("editorSave").addEventListener("click", () => void saveEditor());
    el("outputClose").addEventListener("click", () => {
        el("outputPanel").hidden = true;
    });
    el("outputCopy").addEventListener("click", () => {
        copyText(el("outputBody").value, "output");
    });
    addCreateRow("input");
    addCreateRow("output");
    renderSyncFields();
    renderNegotiationModes();
    setSyncState("idle", "");
    void loadCapabilities();
    render();
}
wireDom();
