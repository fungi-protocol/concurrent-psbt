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
import type {
  Backend,
  InspectResponse,
  PsbtResponse,
} from "../shared-frontend/core/backend.js";
import { seedFromRandomBytes } from "../model.js";
import {
  addFragment,
  buildConfirmArgs,
  buildCreateRequest,
  buildPayArgs,
  buildSyncRequest,
  bytesToBase64,
  emptySession,
  fragmentLabel,
  fragmentSummary,
  negotiationView,
  pastedPsbt,
  removeFragment,
  selectedFragments,
  setSelected,
  type CreateFormInput,
  type CreateFormOutput,
  type FragmentOrigin,
  type SessionFragment,
  type SessionState,
  type SyncTransport,
} from "./state.js";

const backend: Backend = new HttpBackend();

let session: SessionState = emptySession();
const expanded = new Set<string>();

// --- tiny DOM helpers -------------------------------------------------------

function el<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`session UI is missing #${id}`);
  return node as T;
}

function inputValue(id: string): string {
  return el<HTMLInputElement>(id).value;
}

function textareaValue(id: string): string {
  return el<HTMLTextAreaElement>(id).value;
}

function selectValue(id: string): string {
  return el<HTMLSelectElement>(id).value;
}

function logEvent(message: string): void {
  const log = el<HTMLOListElement>("sessionLog");
  const item = document.createElement("li");
  item.textContent = message;
  log.prepend(item);
  while (log.children.length > 40) {
    log.lastElementChild?.remove();
  }
}

function showStatus(message: string, isError: boolean): void {
  const status = el<HTMLElement>("sessionStatus");
  status.textContent = message;
  status.classList.toggle("session-status-error", isError);
}

function reportError(context: string, error: unknown): void {
  const detail =
    error instanceof PtjBackendError
      ? error.message
      : error instanceof Error
        ? error.message
        : String(error);
  showStatus(`${context}: ${detail}`, true);
  logEvent(`ERROR ${context}: ${detail}`);
}

function showOutput(title: string, body: string): void {
  el<HTMLElement>("outputTitle").textContent = title;
  el<HTMLTextAreaElement>("outputBody").value = body;
  el<HTMLElement>("outputPanel").hidden = false;
}

// --- fragment list rendering -------------------------------------------------

function shortHex(value: string | null): string {
  if (!value) return "";
  return value.length > 16 ? `${value.slice(0, 16)}…` : value;
}

function addAndRender(psbt: string, inspect: InspectResponse | null, origin: FragmentOrigin): void {
  const added = addFragment(session, psbt, inspect, origin);
  session = added.state;
  if (added.duplicate) {
    logEvent(`${added.fragment.key} already loaded; selected it (${origin})`);
  } else {
    logEvent(`added ${added.fragment.key} (${origin})`);
  }
  render();
}

async function addResponse(response: PsbtResponse, origin: FragmentOrigin): Promise<void> {
  // Every mutating route returns {psbt, inspect}; fall back to /api/inspect
  // if a backend ever omits the inspection.
  const inspect = response.inspect ?? (await backend.inspectPsbt(response.psbt));
  addAndRender(response.psbt, inspect, origin);
}

function render(): void {
  const list = el<HTMLUListElement>("fragmentList");
  list.textContent = "";
  for (const fragment of session.fragments) {
    list.append(renderFragment(fragment));
  }
  el<HTMLElement>("fragmentEmpty").hidden = session.fragments.length > 0;
  const selected = selectedFragments(session);
  el<HTMLElement>("selectionCount").textContent = selected.length
    ? `${selected.length} selected`
    : "none selected";
}

function renderFragment(fragment: SessionFragment): HTMLLIElement {
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
    } else {
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

function requireSelection(exactly?: number): SessionFragment[] | null {
  const selected = selectedFragments(session);
  if (exactly !== undefined && selected.length !== exactly) {
    showStatus(
      `this action needs exactly ${exactly} selected fragment${exactly === 1 ? "" : "s"}`,
      true,
    );
    return null;
  }
  if (exactly === undefined && selected.length < 2) {
    showStatus("this action needs at least 2 selected fragments", true);
    return null;
  }
  return selected;
}

// --- session screen: load + set operations -----------------------------------

async function addPasted(kind: "v2" | "bip174"): Promise<void> {
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
    } else {
      await addResponse(await backend.importBip174(psbt), "import-bip174");
    }
    el<HTMLTextAreaElement>("pasteInput").value = "";
    showStatus("", false);
  } catch (error) {
    reportError(kind === "v2" ? "inspect" : "import BIP 174", error);
  }
}

async function loadUpload(): Promise<void> {
  const input = el<HTMLInputElement>("uploadInput");
  const file = input.files?.[0];
  if (!file) return;
  const bytes = new Uint8Array(await file.arrayBuffer());
  const text = new TextDecoder().decode(bytes).trim();
  // A .psbt file is either raw binary or already-base64 text; both end up as
  // base64 in the paste box, decoded by the button the user picks.
  el<HTMLTextAreaElement>("pasteInput").value =
    pastedPsbt(text) ?? bytesToBase64(bytes);
  logEvent(`loaded ${file.name} into the paste box`);
  input.value = "";
}

async function joinSelected(): Promise<void> {
  const selected = requireSelection();
  if (!selected) return;
  try {
    await addResponse(await backend.joinPsbts(selected.map((f) => f.psbt)), "join");
    showStatus("", false);
  } catch (error) {
    reportError("join", error);
  }
}

async function concatenateSelected(): Promise<void> {
  const selected = requireSelection();
  if (!selected) return;
  try {
    await addResponse(
      await backend.concatenatePsbts(selected.map((f) => f.psbt)),
      "concatenate",
    );
    showStatus("", false);
  } catch (error) {
    reportError("concatenate", error);
  }
}

async function sortSelected(): Promise<void> {
  const selected = requireSelection(1);
  if (!selected) return;
  const seed = inputValue("sortSeed").trim();
  try {
    await addResponse(await backend.sortPsbt(selected[0].psbt, seed || undefined), "sort");
    showStatus("", false);
  } catch (error) {
    reportError("sort", error);
  }
}

async function makeUnorderedSelected(): Promise<void> {
  const selected = requireSelection(1);
  if (!selected) return;
  try {
    await addResponse(await backend.makeUnordered(selected[0].psbt), "make-unordered");
    showStatus("", false);
  } catch (error) {
    reportError("make unordered", error);
  }
}

async function atomizeSelected(): Promise<void> {
  const selected = requireSelection(1);
  if (!selected) return;
  try {
    const response = await backend.atomizePsbt(selected[0].psbt);
    for (const piece of response.fragments) {
      await addResponse(piece, "atomize");
    }
    logEvent(`atomize produced ${response.fragments.length} fragments`);
    showStatus("", false);
  } catch (error) {
    reportError("atomize", error);
  }
}

function exportSelectedV2(): void {
  const selected = requireSelection(1);
  if (!selected) return;
  showOutput(`${selected[0].key} — PSBT v2 (BIP 370) base64`, selected[0].psbt);
}

async function exportSelectedBip174(): Promise<void> {
  const selected = requireSelection(1);
  if (!selected) return;
  try {
    const exported = await backend.exportBip174(selected[0].psbt);
    showOutput(`${selected[0].key} — BIP 174 base64`, exported.psbt);
    showStatus("", false);
  } catch (error) {
    reportError("export BIP 174", error);
  }
}

// --- create screen ------------------------------------------------------------

function rowValues(container: HTMLElement, selector: string): string[] {
  return Array.from(container.querySelectorAll<HTMLInputElement>(selector)).map(
    (input) => input.value,
  );
}

function createFormInputs(): CreateFormInput[] {
  const rows = el<HTMLElement>("createInputs");
  const txids = rowValues(rows, "input[data-role=txid]");
  const vouts = rowValues(rows, "input[data-role=vout]");
  return txids.map((txid, index) => ({ txid, vout: vouts[index] ?? "" }));
}

function createFormOutputs(): CreateFormOutput[] {
  const rows = el<HTMLElement>("createOutputs");
  const addresses = rowValues(rows, "input[data-role=address]");
  const amounts = rowValues(rows, "input[data-role=amount]");
  return addresses.map((address, index) => ({ address, amountBtc: amounts[index] ?? "" }));
}

function addCreateRow(kind: "input" | "output"): void {
  const container = el<HTMLElement>(kind === "input" ? "createInputs" : "createOutputs");
  const row = document.createElement("div");
  row.className = "split-row";
  if (kind === "input") {
    row.innerHTML =
      '<label class="field-label">txid' +
      '<input data-role="txid" autocomplete="off" spellcheck="false" placeholder="64 hex chars"></label>' +
      '<label class="field-label compact">vout' +
      '<input data-role="vout" type="number" min="0" value="0"></label>';
  } else {
    row.innerHTML =
      '<label class="field-label">address' +
      '<input data-role="address" autocomplete="off" spellcheck="false" placeholder="bcrt1q…"></label>' +
      '<label class="field-label compact">amount (BTC)' +
      '<input data-role="amount" autocomplete="off" inputmode="decimal" placeholder="0.00050000"></label>';
  }
  container.append(row);
}

async function createPsbt(event: Event): Promise<void> {
  event.preventDefault();
  const built = buildCreateRequest({
    network: selectValue("createNetwork"),
    ordering: selectValue("createOrdering") as "det" | "explicit" | "unset",
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
  } catch (error) {
    reportError("create", error);
  }
}

// --- sync panel ----------------------------------------------------------------

function syncTransport(): SyncTransport {
  return selectValue("syncTransport") as SyncTransport;
}

function renderSyncFields(): void {
  const transport = syncTransport();
  for (const section of Array.from(document.querySelectorAll<HTMLElement>("[data-transport]"))) {
    const kinds = (section.dataset.transport ?? "").split(" ");
    section.hidden = !kinds.includes(transport);
  }
}

async function runSync(event: Event): Promise<void> {
  event.preventDefault();
  const psbts = selectedFragments(session).map((fragment) => fragment.psbt);
  const built = buildSyncRequest(
    {
      transport: syncTransport(),
      sources: textareaValue("syncSources"),
      state: inputValue("syncState"),
      irohTicket: textareaValue("syncIrohTicket"),
      irohTicketOut: el<HTMLInputElement>("syncIrohTicketOut").checked,
      irohWaitMs: inputValue("syncIrohWaitMs"),
      webrtcRole: selectValue("syncWebrtcRole") as "" | "offer" | "answer",
      signalOut: inputValue("syncSignalOut"),
      signalIn: inputValue("syncSignalIn"),
      webrtcBind: inputValue("syncWebrtcBind"),
      iceServers: textareaValue("syncIceServers"),
      signalTimeoutMs: inputValue("syncSignalTimeoutMs"),
    },
    psbts,
  );
  if (built.ok === false) {
    showStatus(built.error, true);
    return;
  }
  const button = el<HTMLButtonElement>("syncRun");
  button.disabled = true;
  showStatus("syncing…", false);
  try {
    const response = await backend.syncPsbts(built.value);
    await addResponse(response, "sync");
    const view = negotiationView(response);
    let summary =
      `sync converged: ${view.paymentCount} payment record(s), ` +
      `${view.confirmationCount} confirmation record(s) out of band`;
    if (response.irohTicketOut) {
      showOutput("iroh document ticket (share with peers)", response.irohTicketOut);
      summary += "; created a new iroh document";
    }
    logEvent(summary);
    showStatus("", false);
  } catch (error) {
    reportError("sync", error);
  } finally {
    button.disabled = false;
  }
}

// --- negotiation panel -----------------------------------------------------------

function negotiationTarget(): SessionFragment | null {
  const selected = requireSelection(1);
  return selected ? selected[0] : null;
}

function payMode(): "address" | "hex" {
  return el<HTMLInputElement>("payModeHex").checked ? "hex" : "address";
}

function confirmMode(): "derive" | "hex" {
  return el<HTMLInputElement>("confirmModeHex").checked ? "hex" : "derive";
}

function renderNegotiationModes(): void {
  const pay = payMode();
  el<HTMLElement>("payAddressFields").hidden = pay !== "address";
  el<HTMLElement>("payHexFields").hidden = pay !== "hex";
  const confirm = confirmMode();
  el<HTMLElement>("confirmDeriveFields").hidden = confirm !== "derive";
  el<HTMLElement>("confirmHexFields").hidden = confirm !== "hex";
}

async function runPay(event: Event): Promise<void> {
  event.preventDefault();
  const target = negotiationTarget();
  if (!target) return;
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
  } catch (error) {
    reportError("pay", error);
  }
}

async function runConfirm(event: Event): Promise<void> {
  event.preventDefault();
  const target = negotiationTarget();
  if (!target) return;
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
    await addResponse(
      await backend.confirm(target.psbt, built.value.confirmation, built.value.options),
      "confirm",
    );
    logEvent(`confirmation attached to ${target.key} (result added)`);
    showStatus("", false);
  } catch (error) {
    reportError("confirm", error);
  }
}

async function listPayments(event: Event): Promise<void> {
  event.preventDefault();
  const target = negotiationTarget();
  if (!target) return;
  const secret = inputValue("paymentsSecretHex").trim();
  try {
    const response = await backend.payments(
      target.psbt,
      secret ? { secretHex: secret.toLowerCase() } : undefined,
    );
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
  } catch (error) {
    reportError("payments", error);
  }
}

// --- wiring -------------------------------------------------------------------

function wire(): void {
  el<HTMLButtonElement>("addV2").addEventListener("click", () => void addPasted("v2"));
  el<HTMLButtonElement>("addBip174").addEventListener("click", () => void addPasted("bip174"));
  el<HTMLInputElement>("uploadInput").addEventListener("change", () => void loadUpload());

  el<HTMLButtonElement>("opJoin").addEventListener("click", () => void joinSelected());
  el<HTMLButtonElement>("opConcatenate").addEventListener("click", () => void concatenateSelected());
  el<HTMLButtonElement>("opSort").addEventListener("click", () => void sortSelected());
  el<HTMLButtonElement>("opMakeUnordered").addEventListener("click", () => void makeUnorderedSelected());
  el<HTMLButtonElement>("opAtomize").addEventListener("click", () => void atomizeSelected());
  el<HTMLButtonElement>("opExportV2").addEventListener("click", exportSelectedV2);
  el<HTMLButtonElement>("opExportBip174").addEventListener("click", () => void exportSelectedBip174());

  el<HTMLFormElement>("createForm").addEventListener("submit", (event) => void createPsbt(event));
  el<HTMLButtonElement>("createAddInput").addEventListener("click", () => addCreateRow("input"));
  el<HTMLButtonElement>("createAddOutput").addEventListener("click", () => addCreateRow("output"));
  el<HTMLButtonElement>("createGenerateSeed").addEventListener("click", () => {
    // Spec: PSBT_GLOBAL_SORT_SEED must carry at least 128 bits of randomness.
    const bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    el<HTMLInputElement>("createSeed").value = seedFromRandomBytes(bytes);
  });

  el<HTMLSelectElement>("syncTransport").addEventListener("change", renderSyncFields);
  el<HTMLFormElement>("syncForm").addEventListener("submit", (event) => void runSync(event));

  for (const id of ["payModeAddress", "payModeHex", "confirmModeDerive", "confirmModeHex"]) {
    el<HTMLInputElement>(id).addEventListener("change", renderNegotiationModes);
  }
  el<HTMLFormElement>("payForm").addEventListener("submit", (event) => void runPay(event));
  el<HTMLFormElement>("confirmForm").addEventListener("submit", (event) => void runConfirm(event));
  el<HTMLFormElement>("paymentsForm").addEventListener("submit", (event) => void listPayments(event));

  el<HTMLButtonElement>("outputClose").addEventListener("click", () => {
    el<HTMLElement>("outputPanel").hidden = true;
  });

  addCreateRow("input");
  addCreateRow("output");
  renderSyncFields();
  renderNegotiationModes();
  render();
}

wire();
