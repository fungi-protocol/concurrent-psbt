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
import type {
  Backend,
  InspectResponse,
  PsbtResponse,
} from "../shared-frontend/core/backend.js";
import { amountParts, seedFromRandomBytes } from "../model.js";
import {
  addFragment,
  asArray,
  asObject,
  asString,
  buildConfirmArgs,
  buildCreateRequest,
  buildPayArgs,
  buildSyncRequest,
  bytesToBase64,
  emptySession,
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
import {
  elisionLabel,
  fragmentCardModel,
  type InputView,
  type OutputView,
} from "./display.js";
import type { Network } from "./encoding.js";
import { classifyPaste, mintFromPaste, type PasteClassification } from "./ingest.js";
import {
  actionState,
  addFragmentToSession,
  addPeerToSession,
  applyTxOutputs,
  beginWire,
  completeWire,
  dropFragmentKey,
  emptyObjects,
  enrichDescriptor,
  enrichPayment,
  idleWire,
  mintSession,
  overviewFocus,
  peerByKey,
  sessionByKey,
  sessionFocus,
  validateFocus,
  wireDisposition,
  wireVerdict,
  type FocusState,
  type NodeRef,
  type ObjectsState,
  type SessionAction,
  type WireGesture,
  type WireVerdict,
} from "./wiring.js";
import {
  applyEdit,
  applyFix,
  decodedEditsLeftBehind,
  editorModel,
  rawEditsForSave,
  validateEditor,
  violationsFromServer,
  type EditorModel,
} from "./editor.js";

const backend: Backend = new HttpBackend();

// --- shell state ------------------------------------------------------------

let session: SessionState = emptySession();
let objects: ObjectsState = emptyObjects();
let focus: FocusState = overviewFocus();
let wire: WireGesture = idleWire();
let editor: EditorModel | null = null;
// Server-side fixes queued for the next editor save (violation fix_ids the
// user accepted) and gate overrides armed for it (violation override_params).
// Cleared whenever the editor opens on a fragment or closes: both are
// explicit per-save decisions, never sticky defaults.
const pendingEditorFixes = new Set<string>();
const editorOverrides = new Set<string>();
// The fragment the assign-ids panel is parameterizing (null = panel closed).
let assignIdsTarget: string | null = null;
// Armed correctness-gate overrides. Cleared whenever the selection changes:
// an override is an explicit, per-situation decision, never a sticky default.
const overrides = new Set<string>();
const expanded = new Set<string>();
// Lineage notes for operation results ("join of psbt-1, psbt-2") — the
// lattice provenance the card shows under the title.
const lineage = new Map<string, string>();

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

function button(label: string, title: string, onClick: () => void): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.textContent = label;
  if (title) node.title = title;
  node.addEventListener("click", onClick);
  return node;
}

function span(className: string, text: string): HTMLSpanElement {
  const node = document.createElement("span");
  node.className = className;
  node.textContent = text;
  return node;
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

function copyText(text: string, what: string): void {
  navigator.clipboard.writeText(text).then(
    () => showStatus(`${what} copied to the clipboard`, false),
    (error: unknown) => reportError(`copy ${what}`, error),
  );
}

function displayNetwork(): Network {
  return selectValue("displayNetwork") as Network;
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
const lifehashFailed = new Set<string>();

function lifehashPlaceholder(hex: string, title: string): HTMLElement {
  const chip = span("session-fingerprint-pending", hex.slice(0, 8));
  chip.title = `${title}\n${hex}\n(LifeHash fingerprint unavailable — GET /api/lifehash/<hex> did not serve a PNG)`;
  return chip;
}

// An address slot on a card: the LifeHash chip of the script the address
// encodes (display.js addressChipDigestHex — identical scripts fingerprint
// identically), the address itself riding the chip title/aria-label. Strings
// that decode to no script (a lightning invoice or offer in a payment's
// address slot) stay textual — there is no script to fingerprint.
function addressNode(address: string, what: string, className = "session-address"): HTMLElement {
  const digest = addressChipDigestHex(address);
  return digest ? lifehashBadge(digest, `${address}\n${what}`) : span(className, address);
}

function lifehashBadge(hex: string, title: string): HTMLElement {
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
  img.addEventListener(
    "error",
    () => {
      lifehashFailed.add(hex);
      img.replaceWith(lifehashPlaceholder(hex, title));
    },
    { once: true },
  );
  return img;
}

function amountSpan(sats: number): HTMLElement {
  const parts = amountParts(sats);
  const node = span("session-amount", "");
  if (parts.prefix) node.append(span("session-amount-whole", parts.prefix));
  node.append(span("session-amount-muted", parts.muted));
  if (parts.sats) node.append(span("session-amount-sats", parts.sats));
  return node;
}

// --- fragment set plumbing ----------------------------------------------------

function addAndRender(
  psbt: string,
  inspect: InspectResponse | null,
  origin: FragmentOrigin,
  note?: string,
): SessionFragment {
  const added = addFragment(session, psbt, inspect, origin);
  session = added.state;
  if (added.duplicate) {
    logEvent(`${added.fragment.key} already loaded; selected it (${origin})`);
  } else {
    logEvent(`added ${added.fragment.key} (${origin})`);
    if (note) lineage.set(added.fragment.key, note);
  }
  render();
  return added.fragment;
}

async function addResponse(
  response: PsbtResponse,
  origin: FragmentOrigin,
  note?: string,
): Promise<SessionFragment> {
  // Every mutating route returns {psbt, inspect}; fall back to /api/inspect
  // if a backend ever omits the inspection.
  const inspect = response.inspect ?? (await backend.inspectPsbt(response.psbt));
  return addAndRender(response.psbt, inspect, origin, note);
}

function fragmentByKey(key: string): SessionFragment | null {
  return session.fragments.find((fragment) => fragment.key === key) ?? null;
}

// --- contextual enablement -----------------------------------------------------

const ACTION_BUTTONS: [string, SessionAction][] = [
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

const BASE_TITLE = new Map<string, string>();

function enablementContext() {
  return {
    selected: selectedFragments(session).map((fragment) => fragmentSummary(fragment.inspect)),
    overrides,
  };
}

function renderOps(): void {
  const ctx = enablementContext();
  const gates: { action: SessionAction; id: string; label: string; warning: string }[] = [];
  for (const [id, action] of ACTION_BUTTONS) {
    const node = el<HTMLButtonElement>(id);
    const state = actionState(action, ctx);
    node.disabled = !state.enabled;
    const base = BASE_TITLE.get(id) ?? "";
    if (state.enabled && state.overridden && state.gate) {
      node.title = `${base}\nOVERRIDDEN: ${state.gate.label} — ${state.gate.warning}`.trim();
      node.classList.add("session-overridden");
    } else {
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
  const host = el<HTMLElement>("gateOverrides");
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
      } else {
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
  el<HTMLElement>("selectionCount").textContent = selected.length
    ? `${selected.length} selected`
    : "none selected";
  el<HTMLElement>("negotiationTargetLine").textContent =
    selected.length === 1
      ? `Target: ${selected[0].key}. Records are grow-only; results are added as new fragments.`
      : "Targets the one selected fragment (select exactly one). Records are grow-only; results are added as new fragments.";
}

// --- wire gesture ---------------------------------------------------------------

function nodeName(ref: NodeRef): string {
  return `${ref.kind} ${ref.key}`;
}

// Transient rejection feedback (the demo's red failure pulse, card-shaped):
// tapping a blocked/unbacked target pulses the card and pins the reason chip
// to it for a moment; the status bar carries the same text persistently.
let wireRejection: { ref: NodeRef; text: string } | null = null;

function flashWireRejection(ref: NodeRef, text: string): void {
  const rejection = { ref, text };
  wireRejection = rejection;
  window.setTimeout(() => {
    if (wireRejection === rejection) {
      wireRejection = null;
      render();
    }
  }, 1800);
}

function startWire(kind: NodeRef["kind"], key: string): void {
  wire = beginWire(kind, key);
  showStatus("", false);
  render();
}

function cancelWire(): void {
  wire = idleWire();
  render();
}

function wireTo(target: NodeRef): void {
  const done = completeWire(wire, target, objects);
  wire = done.gesture;
  if (!done.verdict) {
    render();
    return;
  }
  void performWire(done.verdict, target);
}

async function performWire(v: WireVerdict, target: NodeRef): Promise<void> {
  const source = wireSource;
  wireSource = null;
  if (!source) {
    render();
    return;
  }
  if (!v.allowed) {
    const action = v.label ?? `${nodeName(source)} → ${nodeName(target)}`;
    const text =
      wireDisposition(v) === "blocked"
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
  try {
    switch (v.kind) {
      case "fragment-join": {
        const left = fragmentByKey(source.key);
        const right = fragmentByKey(target.key);
        if (!left || !right) break;
        const joined = await addResponse(
          await backend.joinPsbts([left.psbt, right.psbt]),
          "join",
          `⊔ join of ${left.key}, ${right.key}`,
        );
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
        objects = addPeerToSession(objects, sessionKey, peerKey);
        logEvent(`wired ${peerKey} into ${sessionKey}; syncing`);
        await syncSessionOverPeer(sessionKey, peerKey);
        break;
      }
      case "attach-payment": {
        const paymentKey = source.kind === "payment" ? source.key : target.key;
        const fragmentKey = source.kind === "fragment" ? source.key : target.key;
        const payment = objects.payments.find((candidate) => candidate.key === paymentKey);
        const fragment = fragmentByKey(fragmentKey);
        if (!payment || !fragment) break;
        const paid = await addResponse(
          await backend.pay(fragment.psbt, {
            address: payment.address,
            amountBtc: (payment.amountSats / 100_000_000).toFixed(8),
            network: displayNetwork(),
            label: payment.label || undefined,
            payerHex: undefined,
          }),
          "pay",
          `payment ${payment.key} attached to ${fragment.key}`,
        );
        logEvent(`wired ${payment.key} → ${fragment.key}: payment attached, result ${paid.key}`);
        break;
      }
      case "add-create-input": {
        const utxo = objects.utxos.find((candidate) => candidate.key === source.key);
        addCreateRow("input");
        if (utxo?.txid && utxo.vout !== null) {
          const rows = el<HTMLElement>("createInputs");
          const txids = rows.querySelectorAll<HTMLInputElement>("input[data-role=txid]");
          const vouts = rows.querySelectorAll<HTMLInputElement>("input[data-role=vout]");
          txids[txids.length - 1].value = utxo.txid;
          vouts[vouts.length - 1].value = String(utxo.vout);
          logEvent(`wired ${source.key} → create: input row prefilled`);
        } else {
          logEvent(
            `wired ${source.key} → create: added an input row, but the transaction is not decoded ` +
              "(deep classify pending or unavailable) — enter txid:vout manually",
          );
        }
        break;
      }
      default:
        break;
    }
    showStatus("", false);
  } catch (error) {
    reportError(`wire ${v.kind}`, error);
  }
  render();
}

// completeWire consumes the gesture before performWire runs, so the source
// ref is stashed here for the async continuation.
let wireSource: NodeRef | null = null;

function wireTargetRef(target: NodeRef): void {
  wireSource = wire.source;
  wireTo(target);
}

// --- fragment cards --------------------------------------------------------------

const INPUT_ROWS_SHOWN = 3;
const OUTPUT_ROWS_SHOWN = 3;

function renderFragments(): void {
  const list = el<HTMLUListElement>("fragmentList");
  list.textContent = "";
  const focused = focus.mode === "session" && focus.sessionKey ? sessionByKey(objects, focus.sessionKey) : null;
  const visible = focused
    ? session.fragments.filter((fragment) => focused.fragmentKeys.includes(fragment.key))
    : session.fragments;
  for (const fragment of visible) {
    list.append(renderFragmentCard(fragment));
  }
  el<HTMLElement>("fragmentEmpty").hidden = visible.length > 0;
}

function renderFragmentCard(fragment: SessionFragment): HTMLLIElement {
  const card = fragmentCardModel(fragment.inspect, displayNetwork());
  const item = document.createElement("li");
  item.className = "list-item session-fragment session-card";
  const ref: NodeRef = { kind: "fragment", key: fragment.key };
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
  head.append(badge(card.summary.format ?? "not decoded", "session-badge"));
  head.append(
    badge(
      card.summary.ordering ?? "ordering unknown",
      card.summary.ordering === "unordered" ? "session-badge session-badge-good" : "session-badge",
    ),
  );
  if (card.uidTotal !== null) {
    const complete = card.uidPresent !== null && card.uidPresent >= card.uidTotal;
    head.append(
      badge(
        `ids ${card.uidPresent ?? "?"}/${card.uidTotal}`,
        complete ? "session-badge session-badge-good" : "session-badge session-badge-warn",
      ),
    );
  }
  head.append(span("item-meta", fragment.origin));
  item.append(head);

  const note = lineage.get(fragment.key);
  if (note) item.append(span("item-meta session-lineage", note));

  // Body: groups with subtotals; details elided, structure shown.
  const body = document.createElement("div");
  body.className = "session-card-body";
  for (const group of card.groups) {
    const groupNode = document.createElement("div");
    groupNode.className = `session-group session-group-${group.kind}`;
    const title = document.createElement("div");
    title.className = "session-group-title";
    // The header wears the group's script fingerprint when every output
    // shares one script_pubkey (display.js groupChipDigestHex).
    const groupChip = groupChipDigestHex(group);
    if (groupChip) {
      const groupAddress = group.outputs.find((output) => output.address)?.address;
      title.append(
        lifehashBadge(groupChip, `${groupAddress ?? group.label}\nshared script of every output in this group`),
      );
    }
    title.append(span("", group.label));
    if (group.inputSubtotalSats !== null) {
      title.append(span("item-meta", " in "), amountSpan(group.inputSubtotalSats));
    }
    if (group.outputSubtotalSats !== null) {
      title.append(span("item-meta", " out "), amountSpan(group.outputSubtotalSats));
    }
    groupNode.append(title);

    for (const input of group.inputs.slice(0, INPUT_ROWS_SHOWN)) {
      groupNode.append(inputRow(input));
    }
    const inputsHidden = elisionLabel(INPUT_ROWS_SHOWN, group.inputs.length);
    if (inputsHidden) groupNode.append(span("item-meta session-elided", `inputs ${inputsHidden}`));

    for (const output of group.outputs.slice(0, OUTPUT_ROWS_SHOWN)) {
      groupNode.append(outputRow(output));
    }
    const outputsHidden = elisionLabel(OUTPUT_ROWS_SHOWN, group.outputs.length);
    if (outputsHidden) groupNode.append(span("item-meta session-elided", `outputs ${outputsHidden}`));

    body.append(groupNode);
  }
  if (card.groups.length) {
    body.append(span("item-meta session-fee-line", card.fee.text));
  }
  item.append(body);

  // Footer: per-card actions.
  const foot = document.createElement("div");
  foot.className = "session-card-actions";
  foot.append(
    button(expanded.has(fragment.key) ? "Hide raw" : "Raw", "Full inspect JSON (bitvomit view)", () => {
      if (expanded.has(fragment.key)) {
        expanded.delete(fragment.key);
      } else {
        expanded.add(fragment.key);
      }
      render();
    }),
    button("Edit", "Field-by-field editor (liberal parsing; saving mints a new fragment)", () => {
      editor = editorModel(fragment.key, fragment.inspect, displayNetwork());
      pendingEditorFixes.clear();
      editorOverrides.clear();
      renderEditor([]);
      el<HTMLElement>("editorPanel").hidden = false;
    }),
    button("Wire", "Connect this fragment to another object (join, session, payment)", () =>
      startWire("fragment", fragment.key),
    ),
    button("Remove", "Drop the fragment from the set", () => {
      session = removeFragment(session, fragment.key);
      objects = dropFragmentKey(objects, fragment.key);
      expanded.delete(fragment.key);
      lineage.delete(fragment.key);
      logEvent(`removed ${fragment.key}`);
      render();
    }),
  );
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

function badge(text: string, className: string): HTMLElement {
  return span(className, text);
}

function inputRow(input: InputView): HTMLElement {
  const row = document.createElement("div");
  row.className = "session-coin-row";
  row.append(span("session-coin-side", "in"));
  if (input.outpointTxid) {
    row.append(lifehashBadge(input.outpointTxid, `outpoint txid (input ${input.index})`));
    row.append(span("item-meta", `:${input.outpointVout ?? "?"}`));
  } else {
    row.append(span("item-meta", "outpoint unknown"));
  }
  if (input.knownUtxoSats !== null) {
    row.append(amountSpan(input.knownUtxoSats));
  } else {
    row.append(span("item-meta", "amount unknown"));
  }
  if (!input.hasWitnessUtxo && !input.hasNonWitnessUtxo) {
    row.append(span("session-badge session-badge-warn", "no utxo data"));
  }
  return row;
}

function outputRow(output: OutputView): HTMLElement {
  const row = document.createElement("div");
  row.className = "session-coin-row";
  row.append(span("session-coin-side", "out"));
  if (output.uniqueIdHex) {
    row.append(lifehashBadge(output.uniqueIdHex, `output unique id (output ${output.index})`));
  } else {
    row.append(span("session-badge session-badge-warn", "no id"));
  }
  if (output.scriptHex && output.address) {
    // Address as LifeHash chip of the script_pubkey hex — the textual
    // address rides the chip title/aria-label and stays available in the
    // expanded raw view and the field editor.
    row.append(
      lifehashBadge(output.scriptHex, `${output.address}\n${output.scriptLabel} (output ${output.index})`),
    );
  } else if (output.scriptHex) {
    const script = document.createElement("span");
    script.className = "item-meta";
    script.title = output.scriptHex;
    script.textContent = output.scriptLabel;
    row.append(script, lifehashBadge(output.scriptHex, `scriptPubKey (output ${output.index})`));
  } else {
    row.append(span("item-meta", "script unknown"));
  }
  if (output.amountSats !== null) row.append(amountSpan(output.amountSats));
  return row;
}

// Highlight and arm wire targets while a wire is pending. The three-way
// vocabulary (compatible green / blocked red / unbacked dim) and the action
// label in the title come from the presenter's verdict; a recently rejected
// tap keeps its pulse + reason chip independent of wire mode.
function decorateWireTarget(node: HTMLElement, ref: NodeRef): void {
  if (wireRejection && wireRejection.ref.kind === ref.kind && wireRejection.ref.key === ref.key) {
    node.classList.add("session-wire-rejected");
    node.append(span("session-wire-reason", wireRejection.text));
  }
  if (!wire.source) return;
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
    if ((event.target as HTMLElement).closest("button, input, textarea, select, a")) return;
    event.preventDefault();
    wireTargetRef(ref);
  });
}

// --- objects rail ------------------------------------------------------------------

function renderObjects(): void {
  const list = el<HTMLUListElement>("objectList");
  list.textContent = "";

  for (const sessionObject of objects.sessions) {
    const item = document.createElement("li");
    item.className = "list-item session-card";
    const ref: NodeRef = { kind: "session", key: sessionObject.key };
    decorateWireTarget(item, ref);
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(
      span("item-title", `${sessionObject.name}`),
      badge("session", "session-badge"),
      span("item-meta", `${sessionObject.transport} · ${sessionObject.fragmentKeys.length} fragment(s) · ${sessionObject.peerKeys.length} peer(s)`),
    );
    item.append(head);
    if (sessionObject.fragmentKeys.length) {
      item.append(span("item-meta", sessionObject.fragmentKeys.join(", ")));
    }
    const actions = document.createElement("div");
    actions.className = "session-card-actions";
    actions.append(
      button("Focus", "Fill the viewport with this session (mobile view)", () => {
        focus = sessionFocus(sessionObject.key);
        render();
      }),
      button("Wire", "Connect fragments or peers to this session", () =>
        startWire("session", sessionObject.key),
      ),
      button("Sync now", "Sync this session's fragments over its transport", () => {
        void syncSessionOverPeer(sessionObject.key, null);
      }),
    );
    item.append(actions);
    list.append(item);
  }

  for (const peer of objects.peers) {
    const item = document.createElement("li");
    item.className = "list-item session-card";
    decorateWireTarget(item, { kind: "peer", key: peer.key });
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(
      span("item-title", peer.name),
      badge(`peer · ${peer.transport}`, "session-badge"),
    );
    const identity = span("item-meta session-identity", peer.identity.slice(0, 24) + (peer.identity.length > 24 ? "…" : ""));
    identity.title = peer.identity;
    head.append(identity);
    item.append(head);
    const actions = document.createElement("div");
    actions.className = "session-card-actions";
    actions.append(
      button("Copy id", "Copy the full transport identity", () => copyText(peer.identity, `${peer.key} identity`)),
      button("Wire", "Connect this peer to a session", () => startWire("peer", peer.key)),
    );
    item.append(actions);
    list.append(item);
  }

  for (const payment of objects.payments) {
    const item = document.createElement("li");
    item.className = "list-item session-card";
    decorateWireTarget(item, { kind: "payment", key: payment.key });
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(
      span("item-title", payment.label || payment.key),
      badge("payment", "session-badge"),
      addressNode(payment.address, "payment address"),
      amountSpan(payment.amountSats),
    );
    item.append(head);
    // Deep classification details (bitcoin-payment-instructions): variant,
    // recipient description, and the instruction's payment methods.
    if (payment.variant || payment.description || payment.methods.length) {
      const parts = [
        payment.variant,
        payment.description,
        ...payment.methods,
      ].filter((part): part is string => part !== null && part !== "");
      item.append(span("item-meta", parts.join(" · ")));
    }
    const actions = document.createElement("div");
    actions.className = "session-card-actions";
    actions.append(
      button("Prefill Pay", "Copy this instruction into the Pay form", () => {
        el<HTMLInputElement>("payAddress").value = payment.address;
        el<HTMLInputElement>("payAmount").value = (payment.amountSats / 100_000_000).toFixed(8);
        el<HTMLInputElement>("payLabel").value = payment.label;
        logEvent(`prefilled the Pay form from ${payment.key}`);
      }),
      button("Wire", "Attach this payment to a fragment", () => startWire("payment", payment.key)),
    );
    item.append(actions);
    list.append(item);
  }

  for (const utxo of objects.utxos) {
    const item = document.createElement("li");
    item.className = "list-item session-card";
    decorateWireTarget(item, { kind: "utxo", key: utxo.key });
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(
      span("item-title", utxo.key),
      badge("signed tx", "session-badge"),
      span(
        "item-meta",
        utxo.txid
          ? `${utxo.txid.slice(0, 16)}…:${utxo.vout ?? "?"}`
          : "outputs not decoded (deep classify pending or unavailable)",
      ),
    );
    if (utxo.amountSats !== null) head.append(amountSpan(utxo.amountSats));
    if (utxo.fullySigned === false) {
      head.append(badge("inputs not fully signed", "session-badge session-badge-warn"));
    }
    item.append(head);
    if (utxo.address) item.append(addressNode(utxo.address, `address of ${utxo.key}`, "item-meta session-address"));
    const actions = document.createElement("div");
    actions.className = "session-card-actions";
    actions.append(
      button("Wire", "Use as a create-form input", () => startWire("utxo", utxo.key)),
      button("Copy hex", "Copy the raw transaction hex", () => copyText(utxo.rawTxHex, `${utxo.key} hex`)),
    );
    item.append(actions);
    list.append(item);
  }

  for (const descriptor of objects.descriptors) {
    const item = document.createElement("li");
    item.className = "list-item session-card";
    decorateWireTarget(item, { kind: "descriptor", key: descriptor.key });
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    const text = span("item-meta session-identity", descriptor.descriptor.slice(0, 40) + (descriptor.descriptor.length > 40 ? "…" : ""));
    text.title = descriptor.descriptor;
    head.append(
      span("item-title", descriptor.key),
      badge(descriptor.isPrivate ? "descriptor · PRIVATE" : "descriptor", descriptor.isPrivate ? "session-badge session-badge-warn" : "session-badge"),
      text,
    );
    if (descriptor.descriptorType) {
      head.append(badge(descriptor.descriptorType, "session-badge"));
    }
    item.append(head);
    // Deep classification details (miniscript): the authoritative
    // private-key warning and the first derived addresses/scripts.
    if (descriptor.hasPrivateKeys === true) {
      item.append(
        span(
          "item-meta session-gate-warning",
          "contains PRIVATE key material — anyone holding this descriptor can spend from it",
        ),
      );
    }
    if (descriptor.derived.length) {
      // Derived scripts render as LifeHash chips of their script_pubkey hex
      // (never address/script text on the card face; the textual form rides
      // each chip's title/aria-label — display.js chip contract).
      const derivedRow = span(
        "item-meta session-derived-scripts",
        `derives${descriptor.isRanged ? " (ranged)" : ""}:`,
      );
      for (const entry of descriptor.derived) {
        derivedRow.append(
          lifehashBadge(
            entry.scriptPubkeyHex,
            `${entry.address ?? "derived script"} (derivation index ${entry.index})`,
          ),
        );
      }
      item.append(derivedRow);
      item.append(
        span(
          "item-meta",
          "matching these scripts to fragments still needs backend (descriptor → fragment wiring)",
        ),
      );
    } else {
      item.append(
        span("item-meta", "deep classification pending — script derivation folds in when /api/classify answers"),
      );
    }
    list.append(item);
  }
}

// --- wire status + focus bar ---------------------------------------------------------

function renderWireStatus(): void {
  const host = el<HTMLElement>("wireStatus");
  if (!wire.source) {
    host.hidden = true;
    return;
  }
  host.hidden = false;
  el<HTMLElement>("wireStatusText").textContent =
    `wiring from ${nodeName(wire.source)} — tap a highlighted card to join (dimmed cards explain why not)`;
}

function renderFocus(): void {
  focus = validateFocus(
    focus,
    objects.sessions.map((sessionObject) => sessionObject.key),
  );
  const focusBar = el<HTMLElement>("focusBar");
  const inFocus = focus.mode === "session" && focus.sessionKey !== null;
  focusBar.hidden = !inFocus;
  document.body.classList.toggle("session-focused", inFocus);
  if (inFocus && focus.sessionKey) {
    const focused = sessionByKey(objects, focus.sessionKey);
    if (focused) {
      const peers = focused.peerKeys
        .map((key) => peerByKey(objects, key)?.name ?? key)
        .join(", ");
      el<HTMLElement>("focusTitle").textContent =
        `${focused.name} · ${focused.transport} · ${focused.fragmentKeys.length} fragment(s)` +
        (peers ? ` · peers: ${peers}` : "");
    }
  }
  for (const panel of Array.from(document.querySelectorAll<HTMLElement>("[data-focus-hide]"))) {
    // The fragments panel stays: in focus mode it shows the session subset.
    const keep = panel.querySelector("#fragmentList") !== null;
    panel.hidden = inFocus && !keep;
  }
}

// --- editor panel -----------------------------------------------------------------

function renderEditor(violations: ReturnType<typeof validateEditor>): void {
  const model = editor;
  const host = el<HTMLElement>("editorSections");
  host.textContent = "";
  if (!model) return;
  el<HTMLElement>("editorTitle").textContent = `Field editor — ${model.fragmentKey}`;

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
        if (!editor) return;
        editor = applyEdit(editor, field.path, input.value);
        renderEditor([]);
      });
      row.append(input);
      if (field.error) row.append(span("session-status-error", field.error));
      if (field.note) row.append(span("item-meta", field.note));
      box.append(row);
    }
    host.append(box);
  }

  const violationsHost = el<HTMLElement>("editorViolations");
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
        row.append(
          button(fix.label, fix.warning, () => {
            pendingEditorFixes.add(fix.id);
            logEvent(`editor: server fix ${fix.id} requested for the next save — ${fix.warning}`);
            void saveEditor();
          }),
          span("session-gate-warning", ` ${fix.warning}`),
        );
      } else {
        row.append(
          button(fix.label, fix.warning, () => {
            if (!editor) return;
            editor = applyFix(editor, fix.id, (length) => {
              const bytes = new Uint8Array(length);
              crypto.getRandomValues(bytes);
              return bytes;
            });
            logEvent(`editor fix applied (${fix.id}) — ${fix.warning}`);
            renderEditor(validateEditor(editor));
          }),
          span("session-gate-warning", ` ${fix.warning}`),
        );
      }
    }
    if (violation.source === "server" && violation.overrideParam) {
      const param = violation.overrideParam;
      row.append(
        button(
          `Override (${param})`,
          "Waive this gate explicitly on the next save; the backend re-validates everything else.",
          () => {
            editorOverrides.add(param);
            logEvent(`editor: override ${param} armed for the next save`);
            void saveEditor();
          },
        ),
      );
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
async function saveEditor(): Promise<void> {
  const model = editor;
  if (!model) return;
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
    logEvent(
      `editor save: decoded-field edits do not travel over /api/edit (raw keymap rows only)` +
        ` — not sent: ${leftBehind.join(", ")}`,
    );
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
    let lastWarning: string | null = null;
    for (const applied of response.applied_fixes ?? []) {
      if (applied.warning_text) {
        lastWarning = applied.warning_text;
        logEvent(`editor fix ${applied.fix_id} applied server-side — ${applied.warning_text}`);
      } else {
        logEvent(`editor fix ${applied.fix_id} applied server-side`);
      }
    }
    for (const waived of response.overridden ?? []) {
      logEvent(`editor save: gate overridden (${waived.override_param}) — ${waived.message}`);
    }
    const added = await addResponse(
      { psbt: response.psbt, inspect: response.inspect },
      "edit",
      `edit of ${fragment.key}` +
        (edits.length ? ` (${edits.length} raw edit(s))` : " (validation/fixes only)"),
    );
    logEvent(`editor save minted ${added.key} from ${fragment.key}`);
    pendingEditorFixes.clear();
    editorOverrides.clear();
    editor = null;
    el<HTMLElement>("editorPanel").hidden = true;
    showStatus(lastWarning ?? "", lastWarning !== null);
  } catch (error) {
    reportError("editor save", error);
  }
}

// --- session screen: load + set operations -----------------------------------

async function addPsbtText(raw: string, kind: "v2" | "bip174" | "auto"): Promise<boolean> {
  const psbt = pastedPsbt(raw) ?? classifyPasteToPsbt(raw);
  if (!psbt) {
    if (kind !== "auto") showStatus("paste a base64 PSBT first (v2 or BIP 174)", true);
    return false;
  }
  try {
    if (kind === "bip174") {
      await addResponse(await backend.importBip174(psbt), "import-bip174");
    } else if (kind === "v2") {
      const inspect = await backend.inspectPsbt(psbt);
      addAndRender(psbt, inspect, "paste");
    } else {
      // Auto: try v2 first, fall back to a BIP 174 upgrade (mirrors the demo
      // sandbox's hydratePastedPsbtFragment).
      try {
        const inspect = await backend.inspectPsbt(psbt);
        addAndRender(psbt, inspect, "paste");
      } catch (error) {
        if (!(error instanceof PtjBackendError)) throw error;
        await addResponse(await backend.importBip174(psbt), "import-bip174");
        logEvent("paste decoded as BIP 174 and upgraded to v2");
      }
    }
    showStatus("", false);
    return true;
  } catch (error) {
    reportError(kind === "bip174" ? "import BIP 174" : "inspect", error);
    return true; // it WAS a PSBT; the error is already reported
  }
}

function classifyPasteToPsbt(raw: string): string | null {
  const pasted = classifyPaste(raw);
  return pasted.kind === "psbt" ? pasted.payload : null;
}

async function addObject(): Promise<void> {
  const raw = textareaValue("pasteInput");
  const pasted = classifyPaste(raw);
  if (pasted.kind === "psbt") {
    if (await addPsbtText(raw, "auto")) {
      el<HTMLTextAreaElement>("pasteInput").value = "";
    }
    return;
  }
  const minted = mintFromPaste(objects, pasted);
  objects = minted.state;
  logEvent(minted.log);
  if (minted.minted) {
    el<HTMLTextAreaElement>("pasteInput").value = "";
    if (pasted.needsBackend) {
      logEvent(`${minted.minted.key}: deep parsing pending — needs backend: ${pasted.needsBackend}`);
    }
    showStatus("", false);
    void enrichFromClassify(minted.minted, pasted);
  } else {
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
async function enrichFromClassify(node: NodeRef, pasted: PasteClassification): Promise<void> {
  if (
    pasted.kind !== "descriptor" &&
    pasted.kind !== "payment-uri" &&
    pasted.kind !== "transaction-hex"
  ) {
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
        logEvent(
          applied.utxos.length
            ? `${node.key}: transaction decoded — ${applied.utxos.length} output(s) as spendable outpoints`
            : `${node.key}: deep classification returned no decodable outputs`,
        );
        break;
      }
      default:
        break;
    }
    render();
  } catch (error) {
    logEvent(
      `${node.key}: deep classification unavailable — ` +
        (error instanceof Error ? error.message : String(error)),
    );
  }
}

async function addPasted(kind: "v2" | "bip174"): Promise<void> {
  if (await addPsbtText(textareaValue("pasteInput"), kind)) {
    el<HTMLTextAreaElement>("pasteInput").value = "";
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

function requireEnabled(action: SessionAction): SessionFragment[] | null {
  const state = actionState(action, enablementContext());
  if (!state.enabled) {
    showStatus(`${action}: ${state.reason ?? "not available"}`, true);
    return null;
  }
  return selectedFragments(session);
}

async function joinSelected(): Promise<void> {
  const selected = requireEnabled("join");
  if (!selected) return;
  try {
    await addResponse(
      await backend.joinPsbts(selected.map((f) => f.psbt)),
      "join",
      `⊔ join of ${selected.map((f) => f.key).join(", ")}`,
    );
    showStatus("", false);
  } catch (error) {
    reportError("join", error);
  }
}

async function concatenateSelected(): Promise<void> {
  const selected = requireEnabled("concatenate");
  if (!selected) return;
  try {
    await addResponse(
      await backend.concatenatePsbts(selected.map((f) => f.psbt)),
      "concatenate",
      `concatenation of ${selected.map((f) => f.key).join(", ")}`,
    );
    showStatus("", false);
  } catch (error) {
    reportError("concatenate", error);
  }
}

async function sortSelected(): Promise<void> {
  const selected = requireEnabled("sort");
  if (!selected) return;
  const seed = inputValue("sortSeed").trim();
  try {
    await addResponse(
      await backend.sortPsbt(selected[0].psbt, seed || undefined),
      "sort",
      `sort of ${selected[0].key}`,
    );
    showStatus("", false);
  } catch (error) {
    reportError("sort", error);
  }
}

async function makeUnorderedSelected(): Promise<void> {
  const selected = requireEnabled("make-unordered");
  if (!selected) return;
  try {
    await addResponse(
      await backend.makeUnordered(selected[0].psbt),
      "make-unordered",
      `make-unordered of ${selected[0].key}`,
    );
    showStatus("", false);
  } catch (error) {
    reportError("make unordered", error);
  }
}

async function atomizeSelected(): Promise<void> {
  const selected = requireEnabled("atomize");
  if (!selected) return;
  try {
    const response = await backend.atomizePsbt(selected[0].psbt);
    let index = 0;
    for (const piece of response.fragments) {
      index += 1;
      await addResponse(piece, "atomize", `atom ${index}/${response.fragments.length} of ${selected[0].key}`);
    }
    logEvent(`atomize produced ${response.fragments.length} fragments`);
    showStatus("", false);
  } catch (error) {
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

function openAssignIds(): void {
  const selected = requireEnabled("assign-ids");
  if (!selected) return;
  const fragment = selected[0];
  assignIdsTarget = fragment.key;
  renderAssignIds(fragment);
  el<HTMLElement>("assignIdsPanel").hidden = false;
}

function renderAssignIds(fragment: SessionFragment): void {
  el<HTMLElement>("assignIdsTitle").textContent = `Assign unique ids — ${fragment.key}`;
  const host = el<HTMLElement>("assignIdsRows");
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
  el<HTMLInputElement>("assignIdsAuto").checked = true;
  el<HTMLInputElement>("assignIdsOverwrite").checked = false;
}

async function runAssignIds(): Promise<void> {
  const fragment = assignIdsTarget ? fragmentByKey(assignIdsTarget) : null;
  if (!fragment) {
    assignIdsTarget = null;
    el<HTMLElement>("assignIdsPanel").hidden = true;
    return;
  }
  const ids = Array.from(
    el<HTMLElement>("assignIdsRows").querySelectorAll<HTMLInputElement>("input[data-index]"),
  )
    .filter((input) => input.value.trim())
    .map((input) => ({
      target: "out" as const,
      index: Number(input.dataset.index),
      id: input.value.trim(),
    }));
  const auto = el<HTMLInputElement>("assignIdsAuto").checked;
  const overwrite = el<HTMLInputElement>("assignIdsOverwrite").checked;
  try {
    const added = await addResponse(
      await backend.assignIds(fragment.psbt, {
        ids: ids.length ? ids : undefined,
        auto,
        overwrite,
      }),
      "assign-ids",
      `assign-ids of ${fragment.key}`,
    );
    logEvent(
      `assign-ids minted ${added.key} from ${fragment.key}` +
        ` (${ids.length} manual id(s), auto=${auto}, overwrite=${overwrite})`,
    );
    assignIdsTarget = null;
    el<HTMLElement>("assignIdsPanel").hidden = true;
    showStatus("", false);
  } catch (error) {
    reportError("assign ids", error);
  }
}

function exportSelectedV2(): void {
  const selected = requireEnabled("export-v2");
  if (!selected) return;
  showOutput(`${selected[0].key} — PSBT v2 (BIP 370) base64`, selected[0].psbt);
}

async function exportSelectedBip174(): Promise<void> {
  const selected = requireEnabled("export-bip174");
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

function syncTransportValue(): SyncTransport {
  return selectValue("syncTransport") as SyncTransport;
}

function renderSyncFields(): void {
  const transport = syncTransportValue();
  for (const section of Array.from(document.querySelectorAll<HTMLElement>("[data-transport]"))) {
    const kinds = (section.dataset.transport ?? "").split(" ");
    section.hidden = !kinds.includes(transport);
  }
}

function setSyncState(state: "idle" | "syncing" | "ok" | "error", detail: string): void {
  const chip = el<HTMLElement>("syncStateChip");
  chip.textContent = state === "ok" ? "converged" : state;
  chip.className = `session-sync-chip session-sync-${state}`;
  el<HTMLElement>("syncStateDetail").textContent = detail;
}

function pushSyncResult(text: string): void {
  const results = el<HTMLOListElement>("syncResults");
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
    irohTicketOut: el<HTMLInputElement>("syncIrohTicketOut").checked,
    irohWaitMs: inputValue("syncIrohWaitMs"),
    webrtcRole: selectValue("syncWebrtcRole") as "" | "offer" | "answer",
    signalOut: inputValue("syncSignalOut"),
    signalIn: inputValue("syncSignalIn"),
    webrtcBind: inputValue("syncWebrtcBind"),
    iceServers: textareaValue("syncIceServers"),
    signalTimeoutMs: inputValue("syncSignalTimeoutMs"),
  };
}

async function runSyncRequest(psbts: string[], sourceLabel: string): Promise<void> {
  const built = buildSyncRequest(syncFormSnapshot(), psbts);
  if (built.ok === false) {
    showStatus(built.error, true);
    setSyncState("error", built.error);
    return;
  }
  const runButton = el<HTMLButtonElement>("syncRun");
  runButton.disabled = true;
  setSyncState("syncing", `${sourceLabel} over ${built.value.transport}…`);
  showStatus("syncing…", false);
  try {
    const response = await backend.syncPsbts(built.value);
    const converged = await addResponse(response, "sync", `sync convergence (${sourceLabel})`);
    const view = negotiationView(response);
    const summary =
      `${sourceLabel}: converged into ${converged.key}; ` +
      `${view.paymentCount} payment record(s), ${view.confirmationCount} confirmation record(s) out of band`;
    setSyncState("ok", summary);
    pushSyncResult(summary);
    if (response.irohTicketOut) {
      el<HTMLTextAreaElement>("syncTicketBody").value = response.irohTicketOut;
      el<HTMLElement>("syncTicketPanel").hidden = false;
      pushSyncResult("created a new iroh document — ticket ready to share (Copy above)");
    }
    logEvent(summary);
    showStatus("", false);
  } catch (error) {
    setSyncState("error", error instanceof Error ? error.message : String(error));
    pushSyncResult(`sync failed: ${error instanceof Error ? error.message : String(error)}`);
    reportError("sync", error);
  } finally {
    runButton.disabled = false;
    render();
  }
}

async function runSync(event: Event): Promise<void> {
  event.preventDefault();
  const state = actionState("sync", enablementContext());
  if (!state.enabled && syncTransportValue() !== "local") {
    // Local sync legitimately runs from server-side sources with nothing
    // selected; every other transport syncs the selection.
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
async function syncSessionOverPeer(sessionKey: string, peerKey: string | null): Promise<void> {
  const sessionObject = sessionByKey(objects, sessionKey);
  if (!sessionObject) return;
  const peer = peerKey ? peerByKey(objects, peerKey) : null;
  const transport = peer && peer.transport !== "nostr" && peer.transport !== "unknown"
    ? peer.transport
    : sessionObject.transport;
  el<HTMLSelectElement>("syncTransport").value = transport;
  renderSyncFields();
  if (peer && peer.transport === "iroh" && peer.identity) {
    el<HTMLTextAreaElement>("syncIrohTicket").value = peer.identity;
    el<HTMLInputElement>("syncIrohTicketOut").checked = false;
  }
  const members = sessionObject.fragmentKeys
    .map((key) => fragmentByKey(key))
    .filter((fragment): fragment is SessionFragment => fragment !== null)
    .map((fragment) => fragment.psbt);
  if (!members.length && transport !== "local") {
    showStatus(`${sessionObject.name}: wire fragments into the session before syncing`, true);
    return;
  }
  await runSyncRequest(members, `session ${sessionObject.name} (${members.length} fragment(s))`);
}

// --- negotiation panel -----------------------------------------------------------

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
  const selected = requireEnabled("pay");
  if (!selected) return;
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
    await addResponse(
      await backend.pay(target.psbt, built.value.payment, built.value.options),
      "pay",
      `payment record attached to ${target.key}`,
    );
    logEvent(`payment record attached to ${target.key} (result added)`);
    showStatus("", false);
  } catch (error) {
    reportError("pay", error);
  }
}

async function runConfirm(event: Event): Promise<void> {
  event.preventDefault();
  const selected = requireEnabled("confirm");
  if (!selected) return;
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
    await addResponse(
      await backend.confirm(target.psbt, built.value.confirmation, built.value.options),
      "confirm",
      `confirmation attached to ${target.key}`,
    );
    logEvent(`confirmation attached to ${target.key} (result added)`);
    showStatus("", false);
  } catch (error) {
    reportError("confirm", error);
  }
}

async function listPayments(event: Event): Promise<void> {
  event.preventDefault();
  const selected = requireEnabled("payments");
  if (!selected) return;
  const target = selected[0];
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

// --- render root -----------------------------------------------------------------

function render(): void {
  renderFocus();
  renderWireStatus();
  renderFragments();
  renderObjects();
  renderOps();
  el<HTMLElement>("createWireTarget").hidden = !(wire.source && wire.source.kind === "utxo");
}

// --- wiring (DOM event hookup) -----------------------------------------------------

function wireDom(): void {
  for (const [id] of ACTION_BUTTONS) {
    BASE_TITLE.set(id, el<HTMLButtonElement>(id).title);
  }

  el<HTMLButtonElement>("addObject").addEventListener("click", () => void addObject());
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
  el<HTMLButtonElement>("opAssignIds").addEventListener("click", openAssignIds);
  el<HTMLButtonElement>("assignIdsRun").addEventListener("click", () => void runAssignIds());
  el<HTMLButtonElement>("assignIdsClose").addEventListener("click", () => {
    el<HTMLElement>("assignIdsPanel").hidden = true;
  });

  el<HTMLSelectElement>("displayNetwork").addEventListener("change", render);
  el<HTMLButtonElement>("wireCancel").addEventListener("click", cancelWire);
  el<HTMLButtonElement>("focusBack").addEventListener("click", () => {
    focus = overviewFocus();
    render();
  });

  el<HTMLFormElement>("newSessionForm").addEventListener("submit", (event) => {
    event.preventDefault();
    const minted = mintSession(
      objects,
      inputValue("newSessionName"),
      selectValue("newSessionTransport") as SyncTransport,
    );
    objects = minted.state;
    el<HTMLInputElement>("newSessionName").value = "";
    logEvent(`created ${minted.session.key} (${minted.session.name}, ${minted.session.transport})`);
    render();
  });

  el<HTMLFormElement>("createForm").addEventListener("submit", (event) => void createPsbt(event));
  el<HTMLButtonElement>("createAddInput").addEventListener("click", () => addCreateRow("input"));
  el<HTMLButtonElement>("createAddOutput").addEventListener("click", () => addCreateRow("output"));
  el<HTMLButtonElement>("createGenerateSeed").addEventListener("click", () => {
    // Spec: PSBT_GLOBAL_SORT_SEED must carry at least 128 bits of randomness.
    const bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    el<HTMLInputElement>("createSeed").value = seedFromRandomBytes(bytes);
  });
  el<HTMLElement>("createWireTarget").addEventListener("click", () => {
    if (wire.source?.kind === "utxo") {
      wireTargetRef({ kind: "create", key: "create" });
    }
  });

  el<HTMLSelectElement>("syncTransport").addEventListener("change", renderSyncFields);
  el<HTMLFormElement>("syncForm").addEventListener("submit", (event) => void runSync(event));
  el<HTMLButtonElement>("syncTicketCopy").addEventListener("click", () => {
    copyText(el<HTMLTextAreaElement>("syncTicketBody").value, "iroh ticket");
  });

  for (const id of ["payModeAddress", "payModeHex", "confirmModeDerive", "confirmModeHex"]) {
    el<HTMLInputElement>(id).addEventListener("change", renderNegotiationModes);
  }
  el<HTMLFormElement>("payForm").addEventListener("submit", (event) => void runPay(event));
  el<HTMLFormElement>("confirmForm").addEventListener("submit", (event) => void runConfirm(event));
  el<HTMLFormElement>("paymentsForm").addEventListener("submit", (event) => void listPayments(event));

  el<HTMLButtonElement>("editorClose").addEventListener("click", () => {
    editor = null;
    pendingEditorFixes.clear();
    editorOverrides.clear();
    el<HTMLElement>("editorPanel").hidden = true;
  });
  el<HTMLButtonElement>("editorValidate").addEventListener("click", () => {
    if (!editor) return;
    renderEditor(validateEditor(editor));
  });
  el<HTMLButtonElement>("editorSave").addEventListener("click", () => void saveEditor());

  el<HTMLButtonElement>("outputClose").addEventListener("click", () => {
    el<HTMLElement>("outputPanel").hidden = true;
  });
  el<HTMLButtonElement>("outputCopy").addEventListener("click", () => {
    copyText(el<HTMLTextAreaElement>("outputBody").value, "output");
  });

  addCreateRow("input");
  addCreateRow("output");
  renderSyncFields();
  renderNegotiationModes();
  setSyncState("idle", "");
  render();
}

wireDom();
