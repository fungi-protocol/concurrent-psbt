// contrib/demo-gui/src/session/app.ts
//
// Session UI shell — the REAL webgui page (served at "/"). A thin, strictly
// typed DOM layer: every decision lives in ./state.js (pure, node --test
// covered) and every operation drives the ONE Backend seam (HttpBackend
// against this server's own /api/* routes). No fixtures, no fake chain data:
// the fragment list is exactly the set of real PSBTs pasted, uploaded,
// imported, created, or produced by backend operations.
//
// No query strings on the seam imports: http.js itself imports
// ../core/types.js without one, and both URLs must resolve to the SAME module
// instance for `instanceof PtjBackendError` to work (responses are served
// Cache-Control: no-store; cache busting rides the ?v on dist/session/app.js
// in session.html alone).
import { HttpBackend } from "../shared-frontend/backends/http.js";
import { PtjBackendError } from "../shared-frontend/core/types.js";
import { seedFromRandomBytes } from "../model.js";
import { addFragment, buildConfirmArgs, buildCreateRequest, buildPayArgs, buildSyncRequest, bytesToBase64, emptySession, fragmentLabel, fragmentSummary, negotiationView, pastedPsbt, removeFragment, selectedFragments, setSelected, } from "./state.js";
const backend = new HttpBackend();
let session = emptySession();
const expanded = new Set();
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
function showOutput(title, body) {
    el("outputTitle").textContent = title;
    el("outputBody").value = body;
    el("outputPanel").hidden = false;
}
// --- fragment list rendering -------------------------------------------------
function shortHex(value) {
    if (!value)
        return "";
    return value.length > 16 ? `${value.slice(0, 16)}…` : value;
}
function addAndRender(psbt, inspect, origin) {
    const added = addFragment(session, psbt, inspect, origin);
    session = added.state;
    if (added.duplicate) {
        logEvent(`${added.fragment.key} already loaded; selected it (${origin})`);
    }
    else {
        logEvent(`added ${added.fragment.key} (${origin})`);
    }
    render();
}
async function addResponse(response, origin) {
    // Every mutating route returns {psbt, inspect}; fall back to /api/inspect
    // if a backend ever omits the inspection.
    const inspect = response.inspect ?? (await backend.inspectPsbt(response.psbt));
    addAndRender(response.psbt, inspect, origin);
}
function render() {
    const list = el("fragmentList");
    list.textContent = "";
    for (const fragment of session.fragments) {
        list.append(renderFragment(fragment));
    }
    el("fragmentEmpty").hidden = session.fragments.length > 0;
    const selected = selectedFragments(session);
    el("selectionCount").textContent = selected.length
        ? `${selected.length} selected`
        : "none selected";
}
function renderFragment(fragment) {
    const item = document.createElement("li");
    item.className = "list-item session-fragment";
    const row = document.createElement("div");
    row.className = "session-fragment-row";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = fragment.selected;
    checkbox.setAttribute("aria-label", `select ${fragment.key}`);
    checkbox.addEventListener("change", () => {
        session = setSelected(session, fragment.key, checkbox.checked);
        render();
    });
    row.append(checkbox);
    const title = document.createElement("span");
    title.className = "item-title";
    title.textContent = fragmentLabel(fragment);
    row.append(title);
    const summary = fragmentSummary(fragment.inspect);
    const meta = document.createElement("span");
    meta.className = "item-meta";
    meta.textContent = summary.uniqueIdHex ? `id ${shortHex(summary.uniqueIdHex)}` : "";
    row.append(meta);
    const details = document.createElement("button");
    details.type = "button";
    details.textContent = expanded.has(fragment.key) ? "Hide" : "Details";
    details.addEventListener("click", () => {
        if (expanded.has(fragment.key)) {
            expanded.delete(fragment.key);
        }
        else {
            expanded.add(fragment.key);
        }
        render();
    });
    row.append(details);
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Remove";
    remove.addEventListener("click", () => {
        session = removeFragment(session, fragment.key);
        expanded.delete(fragment.key);
        logEvent(`removed ${fragment.key}`);
        render();
    });
    row.append(remove);
    item.append(row);
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
function requireSelection(exactly) {
    const selected = selectedFragments(session);
    if (exactly !== undefined && selected.length !== exactly) {
        showStatus(`this action needs exactly ${exactly} selected fragment${exactly === 1 ? "" : "s"}`, true);
        return null;
    }
    if (exactly === undefined && selected.length < 2) {
        showStatus("this action needs at least 2 selected fragments", true);
        return null;
    }
    return selected;
}
// --- session screen: load + set operations -----------------------------------
async function addPasted(kind) {
    const raw = textareaValue("pasteInput");
    const psbt = pastedPsbt(raw);
    if (!psbt) {
        showStatus("paste a base64 PSBT first (v2 or BIP 174)", true);
        return;
    }
    try {
        if (kind === "v2") {
            const inspect = await backend.inspectPsbt(psbt);
            addAndRender(psbt, inspect, "paste");
        }
        else {
            await addResponse(await backend.importBip174(psbt), "import-bip174");
        }
        el("pasteInput").value = "";
        showStatus("", false);
    }
    catch (error) {
        reportError(kind === "v2" ? "inspect" : "import BIP 174", error);
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
    // base64 in the paste box, decoded by the button the user picks.
    el("pasteInput").value =
        pastedPsbt(text) ?? bytesToBase64(bytes);
    logEvent(`loaded ${file.name} into the paste box`);
    input.value = "";
}
async function joinSelected() {
    const selected = requireSelection();
    if (!selected)
        return;
    try {
        await addResponse(await backend.joinPsbts(selected.map((f) => f.psbt)), "join");
        showStatus("", false);
    }
    catch (error) {
        reportError("join", error);
    }
}
async function concatenateSelected() {
    const selected = requireSelection();
    if (!selected)
        return;
    try {
        await addResponse(await backend.concatenatePsbts(selected.map((f) => f.psbt)), "concatenate");
        showStatus("", false);
    }
    catch (error) {
        reportError("concatenate", error);
    }
}
async function sortSelected() {
    const selected = requireSelection(1);
    if (!selected)
        return;
    const seed = inputValue("sortSeed").trim();
    try {
        await addResponse(await backend.sortPsbt(selected[0].psbt, seed || undefined), "sort");
        showStatus("", false);
    }
    catch (error) {
        reportError("sort", error);
    }
}
async function makeUnorderedSelected() {
    const selected = requireSelection(1);
    if (!selected)
        return;
    try {
        await addResponse(await backend.makeUnordered(selected[0].psbt), "make-unordered");
        showStatus("", false);
    }
    catch (error) {
        reportError("make unordered", error);
    }
}
async function atomizeSelected() {
    const selected = requireSelection(1);
    if (!selected)
        return;
    try {
        const response = await backend.atomizePsbt(selected[0].psbt);
        for (const piece of response.fragments) {
            await addResponse(piece, "atomize");
        }
        logEvent(`atomize produced ${response.fragments.length} fragments`);
        showStatus("", false);
    }
    catch (error) {
        reportError("atomize", error);
    }
}
function exportSelectedV2() {
    const selected = requireSelection(1);
    if (!selected)
        return;
    showOutput(`${selected[0].key} — PSBT v2 (BIP 370) base64`, selected[0].psbt);
}
async function exportSelectedBip174() {
    const selected = requireSelection(1);
    if (!selected)
        return;
    try {
        const exported = await backend.exportBip174(selected[0].psbt);
        showOutput(`${selected[0].key} — BIP 174 base64`, exported.psbt);
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
                '<input data-role="vout" type="number" min="0" value="0"></label>';
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
function syncTransport() {
    return selectValue("syncTransport");
}
function renderSyncFields() {
    const transport = syncTransport();
    for (const section of Array.from(document.querySelectorAll("[data-transport]"))) {
        const kinds = (section.dataset.transport ?? "").split(" ");
        section.hidden = !kinds.includes(transport);
    }
}
async function runSync(event) {
    event.preventDefault();
    const psbts = selectedFragments(session).map((fragment) => fragment.psbt);
    const built = buildSyncRequest({
        transport: syncTransport(),
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
    }, psbts);
    if (built.ok === false) {
        showStatus(built.error, true);
        return;
    }
    const button = el("syncRun");
    button.disabled = true;
    showStatus("syncing…", false);
    try {
        const response = await backend.syncPsbts(built.value);
        await addResponse(response, "sync");
        const view = negotiationView(response);
        let summary = `sync converged: ${view.paymentCount} payment record(s), ` +
            `${view.confirmationCount} confirmation record(s) out of band`;
        if (response.irohTicketOut) {
            showOutput("iroh document ticket (share with peers)", response.irohTicketOut);
            summary += "; created a new iroh document";
        }
        logEvent(summary);
        showStatus("", false);
    }
    catch (error) {
        reportError("sync", error);
    }
    finally {
        button.disabled = false;
    }
}
// --- negotiation panel -----------------------------------------------------------
function negotiationTarget() {
    const selected = requireSelection(1);
    return selected ? selected[0] : null;
}
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
    const target = negotiationTarget();
    if (!target)
        return;
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
        await addResponse(await backend.pay(target.psbt, built.value.payment, built.value.options), "pay");
        logEvent(`payment record attached to ${target.key} (result added)`);
        showStatus("", false);
    }
    catch (error) {
        reportError("pay", error);
    }
}
async function runConfirm(event) {
    event.preventDefault();
    const target = negotiationTarget();
    if (!target)
        return;
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
        await addResponse(await backend.confirm(target.psbt, built.value.confirmation, built.value.options), "confirm");
        logEvent(`confirmation attached to ${target.key} (result added)`);
        showStatus("", false);
    }
    catch (error) {
        reportError("confirm", error);
    }
}
async function listPayments(event) {
    event.preventDefault();
    const target = negotiationTarget();
    if (!target)
        return;
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
// --- wiring -------------------------------------------------------------------
function wire() {
    el("addV2").addEventListener("click", () => void addPasted("v2"));
    el("addBip174").addEventListener("click", () => void addPasted("bip174"));
    el("uploadInput").addEventListener("change", () => void loadUpload());
    el("opJoin").addEventListener("click", () => void joinSelected());
    el("opConcatenate").addEventListener("click", () => void concatenateSelected());
    el("opSort").addEventListener("click", () => void sortSelected());
    el("opMakeUnordered").addEventListener("click", () => void makeUnorderedSelected());
    el("opAtomize").addEventListener("click", () => void atomizeSelected());
    el("opExportV2").addEventListener("click", exportSelectedV2);
    el("opExportBip174").addEventListener("click", () => void exportSelectedBip174());
    el("createForm").addEventListener("submit", (event) => void createPsbt(event));
    el("createAddInput").addEventListener("click", () => addCreateRow("input"));
    el("createAddOutput").addEventListener("click", () => addCreateRow("output"));
    el("createGenerateSeed").addEventListener("click", () => {
        const bytes = new Uint8Array(8);
        crypto.getRandomValues(bytes);
        el("createSeed").value = seedFromRandomBytes(bytes);
    });
    el("syncTransport").addEventListener("change", renderSyncFields);
    el("syncForm").addEventListener("submit", (event) => void runSync(event));
    for (const id of ["payModeAddress", "payModeHex", "confirmModeDerive", "confirmModeHex"]) {
        el(id).addEventListener("change", renderNegotiationModes);
    }
    el("payForm").addEventListener("submit", (event) => void runPay(event));
    el("confirmForm").addEventListener("submit", (event) => void runConfirm(event));
    el("paymentsForm").addEventListener("submit", (event) => void listPayments(event));
    el("outputClose").addEventListener("click", () => {
        el("outputPanel").hidden = true;
    });
    addCreateRow("input");
    addCreateRow("output");
    renderSyncFields();
    renderNegotiationModes();
    render();
}
wire();
