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
import { seedFromRandomBytes } from "../model.js";
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
  amountBits,
  amountSpanParts,
  DETAIL_LEVELS,
  elisionLabel,
  fragmentBadges,
  fragmentCardModel,
  groupAggregate,
  rawKeymapSections,
  rowDetailPairs,
  rowFacePairs,
  signedAmountSpanParts,
  type AmountSpanPart,
  type BalanceSheet,
  type CardGroup,
  type DetailLevel,
  type InputView,
  type OutputView,
} from "./display.js";
import { addressFromScript, type Network } from "./encoding.js";
import {
  classifyPaste,
  mintFromPaste,
  SAMPLE_PASTES,
  type PasteClassification,
} from "./ingest.js";
import {
  actionState,
  addBridge,
  addFragmentToSession,
  applyTxOutputs,
  beginWire,
  bridgeGroupContaining,
  completeWire,
  componentPlan,
  dropFragmentKey,
  emptyObjects,
  enrichDescriptor,
  enrichPayment,
  idleWire,
  mergeSessions,
  mineFragmentKeys,
  mintPeer,
  mintSession,
  overviewFocus,
  peerBridgeGroups,
  peerByKey,
  peerUsableForSync,
  pruneWires,
  queueWire,
  sessionByKey,
  sessionFocus,
  unionBridgedPeersIntoSessions,
  unqueueWire,
  validateFocus,
  wireComponents,
  wireDisposition,
  wireKey,
  wireQueueSummary,
  wireVerdict,
  remapWireRef,
  type FocusState,
  type FragmentJoinGroup,
  type NodeRef,
  type ObjectsState,
  type PeerObject,
  type PendingWire,
  type SessionAction,
  type SessionObject,
  type WireGesture,
} from "./wiring.js";
import {
  applyEdit,
  applyFix,
  decodedEditsLeftBehind,
  editorModel,
  rawEditsForSave,
  toggledBitfieldValue,
  TX_MODIFIABLE_BITS,
  validateEditor,
  violationsFromServer,
  type EditorField,
  type EditorModel,
} from "./editor.js";
import {
  descriptorColorKey,
  groupColorKey,
  paletteColor,
  paletteRegistry,
  peerColorKey,
} from "./palette.js";

const backend: Backend = new HttpBackend();

// --- shell state ------------------------------------------------------------

let session: SessionState = emptySession();
let objects: ObjectsState = emptyObjects();
let focus: FocusState = overviewFocus();
let wire: WireGesture = idleWire();
// The pending-wire queue: completed wire gestures accumulate here as
// visible edges (each with its own Join) instead of executing immediately;
// the toolbar Join applies whole connected components. Pruned against the
// live object graph on every render.
let pendingWires: PendingWire[] = [];
// A completed pointer gesture (drag-to-wire) sets this so the click event
// the browser fires afterward does not ALSO toggle selection or open a
// row dialog. Consumed by the first click handler that checks it.
let suppressNextClick = false;

function consumeSuppressedClick(): boolean {
  const suppressed = suppressNextClick;
  suppressNextClick = false;
  return suppressed;
}
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
// Per-card detail-ladder level (display.ts DetailLevel). Absent = the
// default "grouped" mode; the fourth mode (everything, raw) is the modal
// dialog, not a card state.
const detailLevels = new Map<string, DetailLevel>();

function detailLevel(key: string): DetailLevel {
  return detailLevels.get(key) ?? "grouped";
}
// Lineage notes for operation results ("join of psbt-1, psbt-2") — the
// lattice provenance the card shows under the title.
const lineage = new Map<string, string>();
// Tableau 10 color identities (palette.js): first-seen stable for the page
// session — descriptors and pseudo-descriptors keep their color across
// re-renders and later arrivals.
const identityColors = paletteRegistry();

// Paint a node with its identity color: the CSS custom property drives the
// group/card delineation (border, stripe, chip) in the descriptor's color.
function colorizeIdentity(node: HTMLElement, colorKey: string | null): void {
  if (!colorKey) return;
  node.classList.add("session-colorized");
  node.style.setProperty("--identity-color", paletteColor(identityColors, colorKey));
}

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

// Opening a panel must be VISIBLE: the wide panels live below the fold, and
// unhiding one without scrolling reads as a dead button (the live-review
// symptom on Edit). Reveal = unhide + scroll into view + move focus to the
// panel so keyboard/AT users land where the action went.
function revealPanel(id: string): void {
  const panel = el<HTMLElement>(id);
  panel.hidden = false;
  panel.tabIndex = -1;
  panel.scrollIntoView({ behavior: "smooth", block: "start" });
  panel.focus({ preventScroll: true });
}

function showOutput(title: string, body: string): void {
  el<HTMLElement>("outputTitle").textContent = title;
  el<HTMLTextAreaElement>("outputBody").value = body;
  revealPanel("outputPanel");
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

// BIP 177 sat-first emphasis (display.ts amountSpanParts): symbol/scale/
// digits spans whose classes carry only opacity and weight — every part
// inherits the surrounding color (the ead6ca05 rule). Underneath, the
// binary fingerprint (display.ts amountBits): a thin barcode of the value
// in base 2, LSB right-aligned under the last digit, for at-a-glance
// recognition of low-Hamming-weight values.
function amountSpanFrom(parts: AmountSpanPart[], sats: number): HTMLElement {
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

function amountSpan(sats: number): HTMLElement {
  return amountSpanFrom(amountSpanParts(sats), sats);
}

// Signed variant for balance deltas: the sign inherits the surrounding
// color like every other significant digit.
function signedAmountSpan(sats: number): HTMLElement {
  return amountSpanFrom(signedAmountSpanParts(sats), sats);
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
  // Enablement changed, so a previously surfaced disabled-reason is stale.
  el<HTMLElement>("opsHint").hidden = true;
  const gates: { action: SessionAction; id: string; label: string; warning: string }[] = [];
  for (const [id, action] of ACTION_BUTTONS) {
    const node = el<HTMLButtonElement>(id);
    const state = actionState(action, ctx);
    node.disabled = !state.enabled;
    node.dataset.why = "";
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
      node.dataset.why = why;
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

// --- drag-to-wire ------------------------------------------------------------
//
// Wiring is always on (the demo's gesture): press a card and drag to
// another card to queue the wire — no mode button. A press only becomes a
// drag past a small movement threshold, so plain clicks keep meaning
// selection (cards) and detail (rows). While a drag is live the DOM is NOT
// re-rendered (a rebuild would destroy the pointer-captured node): targets
// are painted imperatively via their data-wire-* attributes and a fixed
// overlay line follows the pointer.

interface WireDrag {
  ref: NodeRef;
  pointerId: number;
  startX: number;
  startY: number;
  active: boolean;
  node: HTMLElement;
}

let wireDrag: WireDrag | null = null;
const WIRE_DRAG_THRESHOLD_PX = 6;

function wireRefOf(node: HTMLElement): NodeRef | null {
  const kind = node.dataset.wireKind as NodeRef["kind"] | undefined;
  const key = node.dataset.wireKey;
  return kind && key ? { kind, key } : null;
}

function sameRef(a: NodeRef | null | undefined, b: NodeRef | null | undefined): boolean {
  return !!a && !!b && a.kind === b.kind && a.key === b.key;
}

// Imperative target painting: the render-time verdict pass cannot run
// mid-drag, so the same class vocabulary (source / compatible green /
// blocked red / unbacked dim) is applied directly to the live nodes.
function paintWireTargets(): void {
  if (!wire.source) return;
  // The create-form target only exists while a utxo drag is live; render()
  // cannot unhide it mid-drag, so it is shown (and painted, below) here.
  el<HTMLElement>("createWireTarget").hidden = wire.source.kind !== "utxo";
  for (const node of Array.from(document.querySelectorAll<HTMLElement>("[data-wire-kind]"))) {
    const ref = wireRefOf(node);
    if (!ref) continue;
    if (sameRef(wire.source, ref)) {
      node.classList.add("session-wire-source");
      continue;
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
  }
  const host = el<HTMLElement>("wireStatus");
  host.hidden = false;
  host.classList.remove("session-wire-status-idle");
  el<HTMLElement>("wireStatusText").textContent =
    `wiring from ${nodeName(wire.source)} — drop on a highlighted card to queue the wire ` +
    "(dimmed cards explain why not)";
}

function clearWirePaint(): void {
  for (const node of Array.from(document.querySelectorAll<HTMLElement>("[data-wire-kind]"))) {
    node.classList.remove(
      "session-wire-source",
      "session-wire-target",
      "session-wire-incompatible",
      "session-wire-blocked",
      "session-wire-hover",
    );
    node.removeAttribute("title");
  }
  el<HTMLElement>("createWireTarget").hidden = true;
  // Back from live-drag messaging to the idle advertisement.
  renderWireStatus();
}

// The drag line: one fixed-position element from the gesture's start to
// the pointer, created lazily and reused.
function wireDragLine(): HTMLElement {
  let line = document.getElementById("wireDragLine");
  if (!line) {
    line = document.createElement("div");
    line.id = "wireDragLine";
    line.className = "session-wire-drag-line";
    line.hidden = true;
    document.body.append(line);
  }
  return line;
}

function updateWireDragLine(x1: number, y1: number, x2: number, y2: number): void {
  const line = wireDragLine();
  const length = Math.hypot(x2 - x1, y2 - y1);
  const angle = Math.atan2(y2 - y1, x2 - x1);
  line.hidden = false;
  line.style.width = `${length}px`;
  line.style.transform = `translate(${x1}px, ${y1}px) rotate(${angle}rad)`;
}

function hideWireDragLine(): void {
  wireDragLine().hidden = true;
}

function wireTargetAt(x: number, y: number): HTMLElement | null {
  const hit = document.elementFromPoint(x, y);
  return hit ? (hit as HTMLElement).closest<HTMLElement>("[data-wire-kind]") : null;
}

function cancelWireDrag(): void {
  if (wireDrag) {
    if (wireDrag.node.hasPointerCapture?.(wireDrag.pointerId)) {
      wireDrag.node.releasePointerCapture(wireDrag.pointerId);
    }
    // The pointer release that follows a cancelled drag must not read as a
    // click on the source card (it would toggle selection or open a row).
    if (wireDrag.active) suppressNextClick = true;
  }
  wireDrag = null;
  wire = idleWire();
  hideWireDragLine();
  clearWirePaint();
}

function armWireDrag(node: HTMLElement, ref: NodeRef): void {
  node.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || wireDrag) return;
    // Form controls and buttons keep their own press semantics.
    if ((event.target as HTMLElement).closest("button, a, input, textarea, select")) return;
    wireDrag = {
      ref,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      active: false,
      node,
    };
    node.setPointerCapture(event.pointerId);
  });
  node.addEventListener("pointermove", (event) => {
    if (!wireDrag || wireDrag.node !== node || wireDrag.pointerId !== event.pointerId) return;
    if (!wireDrag.active) {
      if (
        Math.hypot(event.clientX - wireDrag.startX, event.clientY - wireDrag.startY) <
        WIRE_DRAG_THRESHOLD_PX
      ) {
        return;
      }
      wireDrag.active = true;
      wire = beginWire(ref.kind, ref.key);
      paintWireTargets();
    }
    updateWireDragLine(wireDrag.startX, wireDrag.startY, event.clientX, event.clientY);
    const hover = wireTargetAt(event.clientX, event.clientY);
    for (const painted of Array.from(document.querySelectorAll<HTMLElement>(".session-wire-hover"))) {
      if (painted !== hover) painted.classList.remove("session-wire-hover");
    }
    if (hover && !sameRef(wireRefOf(hover), ref)) hover.classList.add("session-wire-hover");
  });
  const finish = (event: PointerEvent, completed: boolean) => {
    if (!wireDrag || wireDrag.node !== node || wireDrag.pointerId !== event.pointerId) return;
    const wasActive = wireDrag.active;
    if (node.hasPointerCapture?.(event.pointerId)) node.releasePointerCapture(event.pointerId);
    wireDrag = null;
    if (!wasActive) return; // a plain click: selection/detail handlers take it
    hideWireDragLine();
    clearWirePaint();
    suppressNextClick = true;
    const target = completed ? wireTargetAt(event.clientX, event.clientY) : null;
    const targetRef = target ? wireRefOf(target) : null;
    if (targetRef && !sameRef(targetRef, ref)) {
      wireTo(targetRef); // queues (or explains) and re-renders
    } else {
      wire = idleWire();
      render();
    }
  };
  node.addEventListener("pointerup", (event) => finish(event, true));
  node.addEventListener("pointercancel", (event) => finish(event, false));
}

// Completing a wire gesture QUEUES the edge (compatible verdicts) or
// reports why it cannot wire (blocked/unbacked) — nothing executes on tap.
function wireTo(target: NodeRef): void {
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
  const queued = queueWire(pendingWires, source, target, objects);
  pendingWires = queued.wires;
  if (queued.queued) {
    logEvent(
      `queued: ${v.label ?? `${nodeName(source)} ⋈ ${nodeName(target)}`} — ` +
        "Join on the wire applies it alone; the toolbar Join applies whole components",
    );
    showStatus("", false);
  } else if (queued.duplicate) {
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
async function executeWire(
  source: NodeRef,
  target: NodeRef,
  remaps?: Map<string, string>,
): Promise<boolean> {
  const v = wireVerdict(source, target, objects);
  if (wireDisposition(v) !== "compatible") {
    const text =
      `${v.label ?? `${nodeName(source)} → ${nodeName(target)}`} is no longer applicable: ` +
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
        if (!left || !right) return false;
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
      case "attach-payment": {
        const paymentKey = source.kind === "payment" ? source.key : target.key;
        const fragmentKey = source.kind === "fragment" ? source.key : target.key;
        const payment = objects.payments.find((candidate) => candidate.key === paymentKey);
        const fragment = fragmentByKey(fragmentKey);
        if (!payment || !fragment) return false;
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
        const utxoKey = source.kind === "utxo" ? source.key : target.key;
        const utxo = objects.utxos.find((candidate) => candidate.key === utxoKey);
        addCreateRow("input");
        if (utxo?.txid && utxo.vout !== null) {
          const rows = el<HTMLElement>("createInputs");
          const txids = rows.querySelectorAll<HTMLInputElement>("input[data-role=txid]");
          const vouts = rows.querySelectorAll<HTMLInputElement>("input[data-role=vout]");
          txids[txids.length - 1].value = utxo.txid;
          vouts[vouts.length - 1].value = String(utxo.vout);
          logEvent(`wired ${utxoKey} → create: input row prefilled`);
        } else {
          logEvent(
            `wired ${utxoKey} → create: added an input row, but the transaction is not decoded ` +
              "(deep classify pending or unavailable) — enter txid:vout manually",
          );
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
        if (!merge.merged) return false;
        objects = merge.state;
        remaps?.set(`session:${source.key}`, merge.merged.key);
        remaps?.set(`session:${target.key}`, merge.merged.key);
        logEvent(
          `merged sessions ${leftName} ⋈ ${rightName} → ${merge.merged.name} ` +
            `(${merge.merged.fragmentKeys.length} fragment(s), ` +
            `${merge.merged.peerKeys.length} peer(s) unioned)`,
        );
        for (const note of merge.notes) {
          logEvent(`session merge: ${note}`);
        }
        const members = merge.merged.fragmentKeys
          .map((key) => fragmentByKey(key))
          .filter((fragment): fragment is SessionFragment => fragment !== null);
        if (members.length >= 2) {
          const joined = await addResponse(
            await backend.joinPsbts(members.map((fragment) => fragment.psbt)),
            "join",
            `⊔ session merge of ${leftName}, ${rightName}`,
          );
          objects = addFragmentToSession(objects, merge.merged.key, joined.key);
          logEvent(
            `session merge joined ${members.map((fragment) => fragment.key).join(" ⋈ ")} → ` +
              `${joined.key} via /api/join (added to ${merge.merged.name})`,
          );
        } else {
          logEvent(
            "session merge: fewer than two member fragment states loaded — nothing to join",
          );
        }
        break;
      }
      case "peer-bridge": {
        objects = addBridge(objects, source.key, target.key);
        objects = unionBridgedPeersIntoSessions(objects);
        const group = bridgeGroupContaining(objects, source.key);
        logEvent(
          `bridged ${source.key} and ${target.key}: group [${group.join(", ")}] now renders ` +
            "as one peer; broadcasts address every member (sessions wired to any member are " +
            "wired to all)",
        );
        break;
      }
      default:
        return false;
    }
    showStatus("", false);
    return true;
  } catch (error) {
    reportError(`wire ${v.kind}`, error);
    flashWireRejection(target, error instanceof Error ? error.message : String(error));
    return false;
  }
}

// One n-ary /api/join call for a component's fragment-join cluster (the
// grow-only analog of the demo's successive pairwise LUBs).
async function executeJoinGroup(group: FragmentJoinGroup): Promise<string | null> {
  const members = group.fragments
    .map((key) => fragmentByKey(key))
    .filter((fragment): fragment is SessionFragment => fragment !== null);
  if (members.length < 2) return null;
  try {
    const joined = await addResponse(
      await backend.joinPsbts(members.map((fragment) => fragment.psbt)),
      "join",
      `⊔ join of ${members.map((fragment) => fragment.key).join(", ")}`,
    );
    logEvent(
      `wired ${members.map((fragment) => fragment.key).join(" ⋈ ")} → ${joined.key} (lattice join)`,
    );
    return joined.key;
  } catch (error) {
    reportError("wire fragment-join", error);
    flashWireRejection(
      { kind: "fragment", key: members[0].key },
      error instanceof Error ? error.message : String(error),
    );
    return null;
  }
}

function livePendingWires(): PendingWire[] {
  pendingWires = pruneWires(
    pendingWires,
    objects,
    session.fragments.map((fragment) => fragment.key),
  );
  return pendingWires;
}

async function joinPendingWire(key: string): Promise<void> {
  const entry = livePendingWires().find(
    (candidate) => wireKey(candidate.source, candidate.target) === key,
  );
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
async function joinAllWires(): Promise<void> {
  const components = wireComponents(livePendingWires());
  if (!components.length) {
    showStatus("queue one or more wires before joining", true);
    render();
    return;
  }
  const consumed = new Set<string>();
  let applied = 0;
  let failed = 0;
  for (const component of components) {
    const plan = componentPlan(component);
    const remap = new Map<string, string>();
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
      } else {
        failed += group.wires.length;
      }
    }
    for (const wireEntry of plan.rest) {
      // Session merges in this component record their result into the
      // remap, so later wires follow the merged session.
      const ok = await executeWire(
        remapWireRef(wireEntry.source, remap),
        remapWireRef(wireEntry.target, remap),
        remap,
      );
      if (ok) {
        applied += 1;
        consumed.add(wireKey(wireEntry.source, wireEntry.target));
      } else {
        failed += 1;
      }
    }
  }
  pendingWires = pendingWires.filter(
    (wireEntry) => !consumed.has(wireKey(wireEntry.source, wireEntry.target)),
  );
  const summary =
    `Join applied ${applied} wire${applied === 1 ? "" : "s"} across ` +
    `${components.length} component${components.length === 1 ? "" : "s"}` +
    (failed ? `; ${failed} failed (kept queued)` : "");
  logEvent(summary);
  showStatus(summary, failed > 0);
  render();
}

function clearPendingWires(): void {
  if (pendingWires.length) {
    logEvent(`cancelled ${pendingWires.length} pending wire(s)`);
  }
  pendingWires = [];
  render();
}

// --- fragment cards --------------------------------------------------------------

const INPUT_ROWS_SHOWN = 3;
const OUTPUT_ROWS_SHOWN = 3;

function renderFragments(): void {
  const list = el<HTMLUListElement>("fragmentList");
  list.textContent = "";
  // Solo mode: with no peers and no sessions the spatial shelves collapse
  // to their headings and the Mine strip takes over the whole work area.
  el<HTMLElement>("spatialWorkbench").classList.toggle(
    "session-workbench-solo",
    objects.peers.length === 0 && objects.sessions.length === 0,
  );
  const focused = focus.mode === "session" && focus.sessionKey ? sessionByKey(objects, focus.sessionKey) : null;
  // Overview stacks full-width area strips (sessions above, Mine pinned to
  // the bottom); focus mode is a flat card grid. The list element is shared,
  // so the layout class flips with the mode.
  list.classList.toggle("session-area-list", !focused);
  if (focused) {
    // Single-session focus keeps the flat member list.
    const visible = session.fragments.filter((fragment) => focused.fragmentKeys.includes(fragment.key));
    for (const fragment of visible) {
      list.append(renderFragmentCard(fragment));
    }
    el<HTMLElement>("fragmentEmpty").hidden = visible.length > 0;
    return;
  }
  // Overview partitions the fragment set by WHERE each fragment lives (Q6):
  // the MINE pseudo-peer holds every sessionless local fragment (loaded and
  // created fragments default there), and each session with loaded members
  // gets its own container — so publishing (wiring Mine → session) is a
  // visible MOVE between areas.
  if (session.fragments.length) {
    const mineKeys = mineFragmentKeys(
      session.fragments.map((fragment) => fragment.key),
      objects,
    );
    for (const sessionObject of objects.sessions) {
      const members = session.fragments.filter((fragment) =>
        sessionObject.fragmentKeys.includes(fragment.key),
      );
      if (members.length) {
        list.append(renderSessionArea(sessionObject, members));
      }
    }
    // Mine renders LAST: it is the static full-width strip at the bottom
    // of the work area, beneath every published-session container.
    list.append(
      renderMineArea(session.fragments.filter((fragment) => mineKeys.includes(fragment.key))),
    );
  }
  el<HTMLElement>("fragmentEmpty").hidden = session.fragments.length > 0;
}

// The MINE pseudo-peer container: a peer-like large area holding the
// sessionless local fragments (Q6). Local-only workflows (join, sort,
// edit, atomize) happen here; wiring a fragment to a session publishes it
// and moves it out.
function renderMineArea(fragments: SessionFragment[]): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "session-mine-area";
  const head = document.createElement("div");
  head.className = "session-fragment-row";
  head.append(
    span("item-title", "Mine"),
    badge("local only", "session-badge"),
    span(
      "item-meta",
      `${fragments.length} local fragment(s), not published to any session`,
    ),
  );
  item.append(head);
  item.append(
    span(
      "item-meta session-area-hint",
      "Local-only workflows (join, sort, edit, atomize) happen here; wiring a fragment to a session publishes it.",
    ),
  );
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
function renderSessionArea(
  sessionObject: SessionObject,
  members: SessionFragment[],
): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "session-published-area";
  const head = document.createElement("div");
  head.className = "session-fragment-row";
  head.append(
    span("item-title", sessionObject.name),
    badge("session", "session-badge session-badge-good"),
    span(
      "item-meta",
      `${sessionObject.transport} · ${members.length} published fragment(s) · ` +
        `${sessionObject.peerKeys.length} peer(s)`,
    ),
    button("Focus", "Fill the viewport with this session (mobile view)", () => {
      focus = sessionFocus(sessionObject.key);
      render();
    }),
  );
  item.append(head);
  const inner = document.createElement("ul");
  inner.className = "item-list session-card-list";
  for (const fragment of members) {
    inner.append(renderFragmentCard(fragment));
  }
  item.append(inner);
  return item;
}

function renderFragmentCard(fragment: SessionFragment): HTMLLIElement {
  const card = fragmentCardModel(fragment.inspect, displayNetwork());
  const item = document.createElement("li");
  item.className = "list-item session-fragment session-card";
  const ref: NodeRef = { kind: "fragment", key: fragment.key };
  decorateWireTarget(item, ref);

  // Selection is the card itself (the demo's click-a-vertex semantics):
  // clicking the card background toggles it; rows, buttons, and form
  // controls keep their own click meanings, and a completed drag-wire
  // gesture suppresses the click it would otherwise leave behind.
  item.classList.toggle("session-card-selected", fragment.selected);
  item.addEventListener("click", (event) => {
    if (consumeSuppressedClick()) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, a, input, textarea, select, dialog, .session-coin-row, .session-detail-toggle")) {
      return;
    }
    session = setSelected(session, fragment.key, !fragment.selected);
    overrides.clear();
    render();
  });

  // Header: identity fingerprint, key, badges, fee.
  const head = document.createElement("div");
  head.className = "session-fragment-row";

  if (card.summary.uniqueIdHex) {
    head.append(lifehashBadge(card.summary.uniqueIdHex, `unordered unique id of ${fragment.key}`));
  }
  head.append(span("item-title", fragment.key));
  for (const view of fragmentBadges(card)) {
    head.append(badge(view.text, badgeToneClass(view.tone), view.emoji, view.title));
  }
  head.append(span("item-meta", fragment.origin));
  // The keyboard/AT path to selection: the card-background click is
  // pointer-only, and the <li> cannot own aria-pressed (that needs a button
  // role, which the card cannot take — it nests real buttons). A real
  // toggle button carries the pressed state instead.
  const selectToggle = document.createElement("button");
  selectToggle.type = "button";
  selectToggle.className = "session-select-toggle";
  selectToggle.textContent = fragment.selected ? "selected" : "select";
  selectToggle.setAttribute("aria-pressed", String(fragment.selected));
  selectToggle.title = `toggle selection of ${fragment.key}`;
  selectToggle.addEventListener("click", () => {
    session = setSelected(session, fragment.key, !fragment.selected);
    overrides.clear();
    render();
  });
  head.append(selectToggle);
  head.append(detailToggle(fragment.key));
  item.append(head);

  const note = lineage.get(fragment.key);
  if (note) item.append(span("item-meta session-lineage", note));

  // Body: groups with subtotals; details elided, structure shown.
  const body = document.createElement("div");
  body.className = "session-card-body";
  for (const group of card.groups) {
    // Attribution is the exception, not the default: only attributed groups
    // (descriptor / pseudo-descriptor provenance) earn a wrapper, title, and
    // identity color. Unattributed rows render flat — being unattributed is
    // implicit, so no "unattributed" label either.
    const attributed = group.kind !== "unattributed";
    let groupNode: HTMLElement = body;
    if (attributed) {
      groupNode = document.createElement("div");
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
        title.append(
          lifehashBadge(groupChip, `${groupAddress ?? group.label}\nshared script of every output in this group`),
        );
      }
      title.append(span("", group.label));
      groupNode.append(title);
    }

    const level = detailLevel(fragment.key);
    if (level === "collapsed") {
      // One aggregate line per group — in provenance mode this reads as
      // one line per peer's operations.
      groupNode.append(aggregateRow(group));
    } else {
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
        inputColumn.append(coinRow(fragment, "input", input.index, inputRow(input, level), level));
      }
      const inputsHidden = elisionLabel(INPUT_ROWS_SHOWN, group.inputs.length);
      if (inputsHidden) inputColumn.append(span("item-meta session-elided", `inputs ${inputsHidden}`));

      for (const output of group.outputs.slice(0, OUTPUT_ROWS_SHOWN)) {
        outputColumn.append(coinRow(fragment, "output", output.index, outputRow(output, level), level));
      }
      const outputsHidden = elisionLabel(OUTPUT_ROWS_SHOWN, group.outputs.length);
      if (outputsHidden) outputColumn.append(span("item-meta session-elided", `outputs ${outputsHidden}`));
      columns.append(inputColumn, outputColumn);
      groupNode.append(columns);
    }

    // Per-group subtotals at the BOTTOM of the columns. With a single
    // group the card-level report directly below would repeat them (the
    // demo's grand-total elision rule, inverted for the card layout); at
    // the collapsed mode the aggregate line IS the subtotal.
    if (level !== "collapsed" && card.groups.length > 1) {
      groupNode.append(groupBalanceFooter(group));
    }

    if (attributed) body.append(groupNode);
  }
  if (card.groups.length) {
    body.append(balanceReport(card.balance, card.fee.text));
  }
  item.append(body);

  // Footer: per-card actions.
  const foot = document.createElement("div");
  foot.className = "session-card-actions";
  foot.append(
    button("Raw", "The BIP 174/370 key-value maps in actual serialization order (the computed inspect JSON is tucked behind a fold)", () => {
      openRawModal(fragment, "card");
    }),
    button("Edit", "Field-by-field editor (liberal parsing; saving mints a new fragment)", () => {
      editor = editorModel(fragment.key, fragment.inspect, displayNetwork());
      pendingEditorFixes.clear();
      editorOverrides.clear();
      renderEditor([]);
      revealPanel("editorPanel");
    }),
    ...wireQueueChip(ref),
    button("Remove", "Drop the fragment from the set", () => {
      session = removeFragment(session, fragment.key);
      objects = dropFragmentKey(objects, fragment.key);
      detailLevels.delete(fragment.key);
      lineage.delete(fragment.key);
      logEvent(`removed ${fragment.key}`);
      render();
    }),
  );
  item.append(foot);
  return item;
}

// The detail-ladder control: a three-segment toggle cycling how much of the
// card body is visible (display.ts DetailLevel). The fourth level — every
// field, raw — is the dialog behind each row and the card's Raw button.
function detailToggle(key: string): HTMLElement {
  const current = detailLevel(key);
  const control = span("session-detail-toggle", "");
  control.setAttribute("role", "group");
  control.setAttribute("aria-label", `detail level for ${key}`);
  const titles: Record<DetailLevel, string> = {
    collapsed: "collapsed: one line item with a balance per group",
    grouped: "grouped: every input/output with chip, amount, signature state",
    expanded: "expanded: rows plus their low-level facts (address, outpoint, sequence…)",
  };
  const labels: Record<DetailLevel, string> = { collapsed: "Σ", grouped: "☰", expanded: "☷" };
  for (const level of DETAIL_LEVELS) {
    const segment = button(labels[level], titles[level], () => {
      detailLevels.set(key, level);
      render();
    });
    segment.classList.add("session-detail-segment");
    segment.setAttribute("aria-pressed", String(level === current));
    control.append(segment);
  }
  return control;
}

// The collapsed level's one-line group summary (display.ts groupAggregate).
function aggregateRow(group: CardGroup): HTMLElement {
  const aggregate = groupAggregate(group);
  const row = span("session-aggregate-row", "");
  const inCell = span("session-balance-cell session-balance-cell-input", "");
  inCell.append(span("session-coin-side", `${aggregate.inputCount} in`));
  if (aggregate.inputCount > 0) {
    inCell.append(
      aggregate.inputSubtotalSats !== null
        ? amountSpan(aggregate.inputSubtotalSats)
        : naSlot(PARTIAL_SUBTOTAL_WHY),
    );
  }
  if (aggregate.signedInputCount > 0) {
    inCell.append(
      span("item-meta", `${aggregate.signedInputCount}/${aggregate.inputCount} signed`),
    );
  }
  const outCell = span("session-balance-cell session-balance-cell-output", "");
  outCell.append(span("session-coin-side", `${aggregate.outputCount} out`));
  if (aggregate.outputCount > 0) {
    outCell.append(
      aggregate.outputSubtotalSats !== null
        ? amountSpan(aggregate.outputSubtotalSats)
        : naSlot(PARTIAL_SUBTOTAL_WHY),
    );
  }
  row.append(inCell, outCell);
  return row;
}

// Emoji + text pill (display.ts fragmentBadges): with an emoji the pill
// collapses to emoji-only in narrow cards (container query; the title
// carries the words); without one the text always shows.
function badge(text: string, className: string, emoji: string | null = null, title = ""): HTMLElement {
  const node = span(className, "");
  if (title) node.title = title;
  if (emoji) {
    node.classList.add("session-badge-emoji");
    node.append(span("session-badge-icon", emoji), span("session-badge-label", text));
  } else {
    node.textContent = text;
  }
  return node;
}

function badgeToneClass(tone: "neutral" | "good" | "warn"): string {
  if (tone === "good") return "session-badge session-badge-good";
  if (tone === "warn") return "session-badge session-badge-warn";
  return "session-badge";
}

// --- balance report footer (display.ts balanceSheet) --------------------------
//
// Per-group subtotals and whole-transaction totals at the BOTTOM of the
// input/output columns under a sum line — the demo's drawSectionSubtotal
// placement. Numbers that need backend data render as an honest "n/a"
// carrying the seam in the tooltip; deficits are red (via CSS, the amounts
// inherit the color).

function naSlot(why: string): HTMLElement {
  const node = span("session-balance-na", "n/a");
  node.title = why;
  return node;
}

function balanceCell(
  side: "input" | "output",
  label: string,
  sats: number | null,
  why: string,
  roleLabel?: string,
): HTMLElement {
  const cell = span(`session-balance-cell session-balance-cell-${side}`, "");
  // The ledger reading: an explicit subtotal/total word before the side
  // marker, the amount right-aligned under its column's amounts.
  if (roleLabel) cell.append(span("session-balance-label", roleLabel));
  cell.append(span("session-coin-side", label));
  cell.append(sats !== null ? amountSpan(sats) : naSlot(why));
  return cell;
}

const PARTIAL_SUBTOTAL_WHY = "member amounts unknown — a partial sum is not shown as a total";

function groupBalanceFooter(group: CardGroup): HTMLElement {
  const footer = span("session-balance session-balance-group", "");
  footer.append(span("session-balance-sumline", ""));
  const totals = span("session-balance-row session-balance-totals", "");
  if (group.inputs.length > 0) {
    totals.append(balanceCell("input", "in", group.inputSubtotalSats, PARTIAL_SUBTOTAL_WHY, "subtotal"));
  }
  if (group.outputs.length > 0) {
    totals.append(balanceCell("output", "out", group.outputSubtotalSats, PARTIAL_SUBTOTAL_WHY, "subtotal"));
  }
  footer.append(totals);
  return footer;
}

function balanceReport(sheet: BalanceSheet, feeText: string): HTMLElement {
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
    cell.append(
      sheet.declaredFeeSats !== null
        ? amountSpan(sheet.declaredFeeSats)
        : naSlot("needs backend: totals.declared_fee_sats (inspect extension)"),
    );
    row.append(cell);
    block.append(row);
  }

  block.append(span("session-balance-sumline", ""));

  const totals = span("session-balance-row session-balance-totals", "");
  totals.append(
    balanceCell("input", "in", sheet.inputTotalSats, "input amounts incomplete (missing UTXO data)", "total"),
  );
  if (!sheet.outputTotalElidedByDeclaredFees) {
    const outCell = balanceCell("output", "out", sheet.outputAccountingTotalSats, "outputs not decoded", "total");
    if (sheet.declaredFeeSats !== null && sheet.declaredFeeSats > 0) {
      outCell.title = "outputs + declared fees";
    }
    totals.append(outCell);
  }
  block.append(totals);

  if (sheet.delta) {
    // The demo's imbalance block: a second thinner sum line (red-tinted for
    // a deficit) and the `balance:` label on the shortfall side.
    block.append(
      span(
        "session-balance-sumline session-balance-deltaline" +
          (sheet.delta.kind === "deficit" ? " session-balance-deltaline-deficit" : ""),
        "",
      ),
    );
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
    row.append(
      sheet.feeRateText !== null
        ? span("item-meta", sheet.feeRateText)
        : naSlot("fee rate needs backend: totals.size (inspect extension)"),
    );
    if (sheet.feeRateText === null) row.prepend(span("item-meta", "fee rate "));
    block.append(row);
  }

  if (sheet.fallbackText) {
    block.append(span("item-meta session-fee-line", sheet.fallbackText));
  }
  return block;
}

// A coin row: clicking it opens the level-4 dialog — the textual address
// and EVERY field inspect carries for that index, all decoded entry fields
// plus the raw keymap entries (display.ts rowDetailPairs), the counterpart
// of the chips-instead-of-text card face. At the "expanded" mode the row
// also carries its curated facts inline (display.ts rowFacePairs).
// During a wire gesture the whole card is the tap target, so the row
// steps aside (the click bubbles to the card's wire handler).
function coinRow(
  fragment: SessionFragment,
  side: "input" | "output",
  index: number,
  row: HTMLElement,
  level: DetailLevel,
): HTMLElement {
  const host = document.createElement("div");
  host.className = "session-coin-item";
  row.classList.add("session-coin-row-expandable");
  row.setAttribute("role", "button");
  row.tabIndex = 0;
  row.title = `${side} ${index} — click for every field, raw (address, omitted fields, raw keymap entries)`;
  const open = () => openRawModal(fragment, { side, index });
  row.addEventListener("click", (event) => {
    if (wire.source) return; // wiring in progress: the card handles the tap
    if ((event.target as HTMLElement).closest("button, a, input")) return;
    event.stopPropagation();
    open();
  });
  row.addEventListener("keydown", (event) => {
    if (wire.source) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      open();
    }
  });
  host.append(row);
  if (level === "expanded") {
    const facts = document.createElement("dl");
    facts.className = "session-coin-detail session-coin-facts";
    for (const pair of rowFacePairs(fragment.inspect, side, index, displayNetwork())) {
      const term = document.createElement("dt");
      term.textContent = pair.label;
      const value = document.createElement("dd");
      value.textContent = pair.value;
      facts.append(term, value);
    }
    if (facts.childElementCount > 0) host.append(facts);
  }
  return host;
}

// The signature-presence indicator: ✓ finalized, ◐ signed but not final,
// ○ unsigned. Text lives in the title; the mark inherits the row color.
function signatureMark(presence: InputView["signatures"], index: number): HTMLElement {
  const marks: Record<InputView["signatures"], string> = { final: "✓", partial: "◐", unsigned: "○" };
  const titles: Record<InputView["signatures"], string> = {
    final: "finalized (final scriptSig/scriptWitness present)",
    partial: "signature present, not finalized",
    unsigned: "no signatures yet",
  };
  const mark = span(`session-sig-indicator session-sig-${presence}`, marks[presence]);
  mark.title = `input ${index}: ${titles[presence]}`;
  return mark;
}

// Row faces. The "grouped" mode is minimal identity — LifeHash chip, amount,
// signature state; the structural warnings (no utxo data, no id) join at
// the "expanded" mode, and everything else lives in the dialog.
function inputRow(input: InputView, level: DetailLevel): HTMLElement {
  const row = document.createElement("div");
  row.className = "session-coin-row";
  row.append(span("session-coin-side", "in"));
  // The chip is the prevout's scriptPubKey — who is paying — matching the
  // output rows. The outpoint stays textual (chip title); only when no
  // prevout script is known does the txid chip return, saying so.
  if (input.prevoutScriptHex) {
    const address = addressFromScript(input.prevoutScriptHex, displayNetwork());
    row.append(
      lifehashBadge(
        input.prevoutScriptHex,
        `${address ?? "prevout scriptPubKey"} (input ${input.index})` +
          (input.outpointText ? `\noutpoint ${input.outpointText}` : ""),
      ),
    );
  } else if (input.outpointTxid) {
    row.append(
      lifehashBadge(
        input.outpointTxid,
        `outpoint txid (input ${input.index}) — prevout script unknown, fingerprint is the txid`,
      ),
    );
    row.append(span("item-meta", `:${input.outpointVout ?? "?"}`));
  } else {
    row.append(span("item-meta", "outpoint unknown"));
  }
  if (input.knownUtxoSats !== null) {
    row.append(amountSpan(input.knownUtxoSats));
  } else {
    row.append(span("item-meta", "amount unknown"));
  }
  row.append(signatureMark(input.signatures, input.index));
  if (level === "expanded" && !input.hasWitnessUtxo && !input.hasNonWitnessUtxo) {
    row.append(span("session-badge session-badge-warn", "no utxo data"));
  }
  return row;
}

function outputRow(output: OutputView, level: DetailLevel): HTMLElement {
  const row = document.createElement("div");
  row.className = "session-coin-row";
  row.append(span("session-coin-side", "out"));
  if (output.uniqueIdHex) {
    row.append(lifehashBadge(output.uniqueIdHex, `output unique id (output ${output.index})`));
  } else if (level === "expanded") {
    row.append(span("session-badge session-badge-warn", "no id"));
  }
  if (output.scriptHex && output.address) {
    // Address as LifeHash chip of the script_pubkey hex — the textual
    // address rides the chip title/aria-label and stays available in the
    // dialog's raw view and the field editor.
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

// --- the level-4 dialog: everything, raw -------------------------------------
//
// One <dialog> serves every scope: a single row (rowDetailPairs — every
// decoded field plus the raw keymap entries) or the whole card. The card
// scope is the faithful raw representation: the BIP 174/370 key-value
// pairs of the global/input/output maps in actual serialization order
// (rawKeymapSections). The computed inspect JSON — totals and other
// derived fields — is demoted to a collapsed <details> below the maps so
// it can never be mistaken for the wire format. Native showModal gives
// Esc and focus trapping; clicking the backdrop closes.

function openRawModal(
  fragment: SessionFragment,
  scope: { side: "input" | "output"; index: number } | "card",
): void {
  const dialog = el<HTMLDialogElement>("rawDialog");
  const title = el<HTMLElement>("rawDialogTitle");
  const dialogBody = el<HTMLElement>("rawDialogBody");
  dialogBody.textContent = "";
  if (scope === "card") {
    title.textContent = `${fragment.key} — raw PSBT maps`;
    const sections = rawKeymapSections(fragment.inspect);
    if (!sections.length) {
      dialogBody.append(span("session-raw-empty", "(not decoded)"));
    }
    for (const section of sections) {
      const block = document.createElement("section");
      block.className = "session-raw-map";
      const heading = document.createElement("h4");
      heading.textContent = section.entries.length
        ? section.title
        : `${section.title} (empty)`;
      block.append(heading);
      for (const entry of section.entries) {
        const row = document.createElement("div");
        row.className = "session-raw-entry";
        const key = span("session-raw-key", entry.keyHex);
        if (entry.name) {
          key.append(span("session-raw-name", entry.name));
        } else if (entry.kind === "unknown") {
          key.append(span("session-raw-name", "(unknown keytype)"));
        }
        row.append(key, span("session-raw-value", entry.valueHex || "(empty value)"));
        block.append(row);
      }
      dialogBody.append(block);
    }
    if (fragment.inspect) {
      const computed = document.createElement("details");
      computed.className = "session-raw-computed";
      const summary = document.createElement("summary");
      summary.textContent = "computed view (inspect JSON — derived fields, not the wire format)";
      const detail = document.createElement("pre");
      detail.className = "session-fragment-detail";
      detail.textContent = JSON.stringify(fragment.inspect, null, 2);
      computed.append(summary, detail);
      dialogBody.append(computed);
    }
  } else {
    title.textContent = `${fragment.key} — ${scope.side} ${scope.index}`;
    const detail = document.createElement("dl");
    detail.className = "session-coin-detail";
    const pairs = rowDetailPairs(fragment.inspect, scope.side, scope.index, displayNetwork());
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
    dialogBody.append(detail);
  }
  dialog.showModal();
}

// The card's pending-wire participation: a "N queued" chip (tooltip: the
// queued action labels). The Wire button is gone — wiring is the always-on
// drag gesture (armWireDrag), the demo's semantics.
function wireQueueChip(ref: NodeRef): HTMLElement[] {
  const queued = pendingWires.filter(
    (wireEntry) => sameRef(wireEntry.source, ref) || sameRef(wireEntry.target, ref),
  );
  if (!queued.length) return [];
  const chip = span("session-badge session-wire-queued-chip", `${queued.length} queued`);
  chip.title = queued
    .map(
      (wireEntry) =>
        wireVerdict(wireEntry.source, wireEntry.target, objects).label ??
        `${nodeName(wireEntry.source)} ⋈ ${nodeName(wireEntry.target)}`,
    )
    .join("\n");
  return [chip];
}

// Mark a card as a wire endpoint and arm the always-on drag gesture. The
// verdict vocabulary (compatible green / blocked red / unbacked dim) is
// painted imperatively DURING a drag (paintWireTargets); at render time the
// card only wears its queue participation and any recent rejection pulse.
function decorateWireTarget(node: HTMLElement, ref: NodeRef): void {
  node.dataset.wireKind = ref.kind;
  node.dataset.wireKey = ref.key;
  armWireDrag(node, ref);
  if (wireRejection && sameRef(wireRejection.ref, ref)) {
    node.classList.add("session-wire-rejected");
    node.append(span("session-wire-reason", wireRejection.text));
  }
  // Cards with at least one queued wire wear the pending-edge vocabulary
  // (the demo's animated orange dashes, card-shaped).
  if (
    pendingWires.some(
      (wireEntry) => sameRef(wireEntry.source, ref) || sameRef(wireEntry.target, ref),
    )
  ) {
    node.classList.add("session-wire-pending");
  }
}

// --- spatial shelves and remaining objects -----------------------------------------

function renderSessionShelf(): void {
  const list = el<HTMLUListElement>("sessionShelfList");
  list.textContent = "";
  for (const sessionObject of objects.sessions) {
    const item = document.createElement("li");
    item.className = "list-item session-card session-shelf-card";
    const ref: NodeRef = { kind: "session", key: sessionObject.key };
    decorateWireTarget(item, ref);
    const head = document.createElement("div");
    head.className = "session-fragment-row";
    head.append(
      span("item-title", sessionObject.name),
      badge("session", "session-badge"),
      span(
        "item-meta",
        `${sessionObject.transport} · ${sessionObject.fragmentKeys.length} fragment(s) · ` +
          `${sessionObject.peerKeys.length} peer(s)`,
      ),
    );
    item.append(head);
    if (sessionObject.fragmentKeys.length) {
      item.append(span("item-meta", sessionObject.fragmentKeys.join(", ")));
    }
    const actions = document.createElement("div");
    actions.className = "session-card-actions";
    actions.append(
      button("Focus", "Fill the viewport with this session", () => {
        focus = sessionFocus(sessionObject.key);
        render();
      }),
      ...wireQueueChip(ref),
      button("Sync now", "Sync this session's fragments over its transport", () => {
        void syncSessionOverPeer(sessionObject.key, null);
      }),
    );
    item.append(actions);
    list.append(item);
  }
  el<HTMLElement>("sessionShelfEmpty").hidden = objects.sessions.length > 0;
}

function unavailablePairButton(): HTMLButtonElement {
  const pair = button(
    "Pair unavailable",
    "Pair unavailable until the ptj adapter exposes session pairing",
    () => {},
  );
  pair.disabled = true;
  return pair;
}

function renderPeerShelf(): void {
  const list = el<HTMLUListElement>("peerShelfList");
  list.textContent = "";
  for (const group of peerBridgeGroups(objects)) {
    const members = group
      .map((key) => peerByKey(objects, key))
      .filter((member): member is PeerObject => member !== null);
    if (!members.length) continue;
    list.append(members.length === 1 ? renderPeerCard(members[0]) : renderBridgeGroupCard(members));
  }
  el<HTMLElement>("peerShelfEmpty").hidden = objects.peers.length > 0;
}

function renderObjects(): void {
  const list = el<HTMLUListElement>("objectList");
  list.textContent = "";

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
      ...wireQueueChip({ kind: "payment", key: payment.key }),
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
      ...wireQueueChip({ kind: "utxo", key: utxo.key }),
      button("Copy hex", "Copy the raw transaction hex", () => copyText(utxo.rawTxHex, `${utxo.key} hex`)),
    );
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
    head.append(
      span("session-color-chip", ""),
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

function renderPeerCard(peer: PeerObject): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "list-item session-card session-peer-card";
  // The Tableau color follows the immutable transport address, never the
  // editable local label and never a fabricated group fingerprint.
  colorizeIdentity(item, peerColorKey(peer));
  decorateWireTarget(item, { kind: "peer", key: peer.key });
  const head = document.createElement("div");
  head.className = "session-fragment-row";
  head.append(
    span("session-color-chip", ""),
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
    ...wireQueueChip({ kind: "peer", key: peer.key }),
    unavailablePairButton(),
  );
  item.append(actions);
  return item;
}

// A bridged peer group renders as ONE peer node (the demo's green bridge
// block): one card, member chips inside, wired as a unit through its first
// member (the presenter expands any member ref to the whole group).
function renderBridgeGroupCard(members: PeerObject[]): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "list-item session-card session-bridge-group";
  decorateWireTarget(item, { kind: "peer", key: members[0].key });
  const head = document.createElement("div");
  head.className = "session-fragment-row";
  head.append(
    span("item-title", members.map((member) => member.name).join(" + ")),
    badge(`bridge · ${members.length} peers`, "session-badge session-badge-good"),
    span("item-meta", "one peer to the session: every member receives every broadcast"),
  );
  item.append(head);
  for (const member of members) {
    const row = document.createElement("div");
    row.className = "session-bridge-member";
    // Each member keeps its pseudo-descriptor identity color so the row
    // still matches the peer's contributed provenance groups.
    colorizeIdentity(row, peerColorKey(member));
    row.append(
      span("session-color-chip", ""),
      span("item-title", member.name),
      badge(`peer · ${member.transport}`, "session-badge"),
    );
    if (!peerUsableForSync(member)) {
      row.append(
        badge("broadcast pending-backend (no usable transport)", "session-badge session-badge-warn"),
      );
    }
    const identity = span(
      "item-meta session-identity",
      member.identity.slice(0, 24) + (member.identity.length > 24 ? "…" : ""),
    );
    identity.title = member.identity;
    row.append(identity);
    row.append(
      button("Copy id", "Copy the full transport identity", () =>
        copyText(member.identity, `${member.key} identity`),
      ),
    );
    item.append(row);
  }
  const actions = document.createElement("div");
  actions.className = "session-card-actions";
  actions.append(...wireQueueChip({ kind: "peer", key: members[0].key }), unavailablePairButton());
  item.append(actions);
  return item;
}

// --- wire status + focus bar ---------------------------------------------------------

function renderWireStatus(): void {
  // The live-drag hint is painted imperatively (paintWireTargets); at
  // render time the gesture is always over, so the bar idles as the
  // gesture's advertisement whenever anything on screen is wireable.
  // (render() calls this after the card passes so the DOM query sees them.)
  const host = el<HTMLElement>("wireStatus");
  host.classList.add("session-wire-status-idle");
  host.hidden = document.querySelector("[data-wire-kind]") === null;
  // Write the standing hint only on change: the bar is role="status", and
  // rewriting identical text every render can re-announce in screen readers.
  const hint = "drag a card onto another to wire them — Esc cancels a drag";
  const text = el<HTMLElement>("wireStatusText");
  if (text.textContent !== hint) text.textContent = hint;
  renderWireQueue();
}

// The pending-wire queue panel: one row per queued edge with its action
// label, an edge-local Join, and a discard; the header carries the
// wire/component summary next to the toolbar Join and Cancel wires.
function renderWireQueue(): void {
  const wires = livePendingWires();
  const host = el<HTMLElement>("wireQueue");
  const list = el<HTMLUListElement>("wireQueueList");
  list.textContent = "";
  if (!wires.length) {
    host.hidden = true;
    return;
  }
  host.hidden = false;
  el<HTMLElement>("wireQueueSummary").textContent = wireQueueSummary(wires).text;
  for (const wireEntry of wires) {
    const key = wireKey(wireEntry.source, wireEntry.target);
    const v = wireVerdict(wireEntry.source, wireEntry.target, objects);
    const item = document.createElement("li");
    item.className = "session-wire-queue-row";
    item.append(
      span(
        "session-wire-queue-label",
        v.label ?? `${nodeName(wireEntry.source)} ⋈ ${nodeName(wireEntry.target)}`,
      ),
      button("Join", "Apply this wire alone", () => void joinPendingWire(key)),
      button("✕", "Discard this wire without applying it", () => {
        pendingWires = unqueueWire(pendingWires, key);
        logEvent(`discarded pending wire ${nodeName(wireEntry.source)} ⋈ ${nodeName(wireEntry.target)}`);
        render();
      }),
    );
    list.append(item);
  }
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

// A bitfield row: one checkbox per spec-defined bit, plus the raw hex value
// itself as the escape hatch (unknown bits and future longer values survive
// untouched — editing a PSBT from a spec this program doesn't know yet).
function bitfieldEditorRow(field: EditorField): HTMLElement {
  const row = document.createElement("div");
  row.className = "field-label session-editor-field session-editor-bitfield";
  row.setAttribute("role", "group");
  row.setAttribute("aria-label", field.label);
  row.append(span("", field.label));
  // While the escape-hatch hex is invalid, the checkboxes go inert: flipping
  // a bit of unparseable text would clobber what the operator was typing.
  const byte0 = toggledBitfieldValue(field.value, 0, false) === null
    ? null
    : field.value
      ? Number.parseInt(field.value.slice(0, 2), 16)
      : 0;
  const setValue = (next: string): void => {
    if (!editor) return;
    editor = applyEdit(editor, field.path, next);
    renderEditor([]);
  };
  for (const { bit, label } of TX_MODIFIABLE_BITS) {
    const wrap = document.createElement("label");
    wrap.className = "session-editor-bit";
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = byte0 !== null && (byte0 & (1 << bit)) !== 0;
    box.disabled = byte0 === null;
    box.addEventListener("change", () => {
      const next = toggledBitfieldValue(field.value, bit, box.checked);
      if (next === null) return;
      setValue(next);
    });
    wrap.append(box, span("", label));
    row.append(wrap);
  }
  const hexWrap = document.createElement("label");
  hexWrap.className = "session-editor-bit session-editor-bitfield-hex";
  hexWrap.append(span("item-meta", "hex"));
  const hex = document.createElement("input");
  hex.value = field.value;
  hex.autocomplete = "off";
  hex.spellcheck = false;
  hex.placeholder = "raw value bytes (empty deletes the entry)";
  hex.addEventListener("change", () => setValue(hex.value));
  hexWrap.append(hex);
  row.append(hexWrap);
  if (field.error) row.append(span("session-status-error", field.error));
  if (field.note) row.append(span("item-meta", field.note));
  return row;
}

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
      if (field.context === "bitfield") {
        box.append(bitfieldEditorRow(field));
        continue;
      }
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

async function addPsbtText(raw: string): Promise<boolean> {
  const psbt = pastedPsbt(raw) ?? classifyPasteToPsbt(raw);
  if (!psbt) return false;
  try {
    // Which decoder applies is a CLASSIFICATION OUTCOME, not a button: try
    // BIP 370 first, fall back to a BIP 174 upgrade (mirrors the demo
    // sandbox's hydratePastedPsbtFragment). The formats share the `psbt`
    // magic.
    try {
      const inspect = await backend.inspectPsbt(psbt);
      addAndRender(psbt, inspect, "paste");
    } catch (error) {
      if (!(error instanceof PtjBackendError)) throw error;
      await addResponse(await backend.importBip174(psbt), "import-bip174");
      logEvent("paste decoded as BIP 174 and upgraded to BIP 370");
    }
    showStatus("", false);
    return true;
  } catch (error) {
    reportError("add PSBT", error);
    return true; // it WAS a PSBT; the error is already reported
  }
}

function classifyPasteToPsbt(raw: string): string | null {
  const pasted = classifyPaste(raw);
  return pasted.kind === "psbt" ? pasted.payload : null;
}

function setAddDrawer(open: boolean, focusPeer = false): void {
  el<HTMLElement>("addDrawer").hidden = !open;
  const toggle = el<HTMLButtonElement>("addDrawerToggle");
  toggle.setAttribute("aria-expanded", String(open));
  if (open) {
    if (focusPeer) el<HTMLInputElement>("manualPeerAddress").focus();
    else el<HTMLTextAreaElement>("pasteInput").focus();
  }
}

// The test-vector palette (header corner). A chip fills the paste box and
// focuses it — ingestion stays behind the operator's explicit Add, so a
// sample walks exactly the real universal-paste path.
function setSamplesPopover(open: boolean): void {
  el<HTMLElement>("samplesPopover").hidden = !open;
  el<HTMLButtonElement>("samplesToggle").setAttribute("aria-expanded", String(open));
}

function initSamplesPalette(): void {
  const list = el<HTMLElement>("samplesList");
  for (const sample of SAMPLE_PASTES) {
    const chip = button(sample.name, `${sample.kind}: fills the paste box`, () => {
      setSamplesPopover(false);
      setAddDrawer(true);
      el<HTMLTextAreaElement>("pasteInput").value = sample.value;
      el<HTMLTextAreaElement>("pasteInput").focus();
    });
    chip.classList.add("session-sample-chip");
    list.append(chip);
  }
  el<HTMLButtonElement>("samplesToggle").addEventListener("click", () => {
    setSamplesPopover(el<HTMLElement>("samplesPopover").hidden);
  });
  // Click-away and Escape both dismiss the popover.
  document.addEventListener("click", (event) => {
    if (el<HTMLElement>("samplesPopover").hidden) return;
    const target = event.target as HTMLElement | null;
    if (target && !target.closest(".session-samples")) setSamplesPopover(false);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !el<HTMLElement>("samplesPopover").hidden) {
      setSamplesPopover(false);
    }
  });
}

function addManualPeer(event: SubmitEvent): void {
  event.preventDefault();
  const identity = inputValue("manualPeerAddress").trim();
  if (!identity) {
    showStatus("A transport address is required.", true);
    return;
  }
  const minted = mintPeer(
    objects,
    inputValue("manualPeerLabel"),
    selectValue("manualPeerTransport") as PeerObject["transport"],
    identity,
  );
  objects = minted.state;
  logEvent(
    minted.created
      ? `added inert ${minted.peer.key} (${minted.peer.transport}); no session or transport changed`
      : `selected existing ${minted.peer.key}; exact transport address already present`,
  );
  el<HTMLInputElement>("manualPeerLabel").value = "";
  el<HTMLInputElement>("manualPeerAddress").value = "";
  showStatus("", false);
  setAddDrawer(false);
  render();
}

async function addObject(): Promise<void> {
  const raw = textareaValue("pasteInput");
  const pasted = classifyPaste(raw);
  if (pasted.kind === "psbt") {
    if (await addPsbtText(raw)) {
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

async function loadUpload(): Promise<void> {
  const input = el<HTMLInputElement>("uploadInput");
  const file = input.files?.[0];
  if (!file) return;
  const bytes = new Uint8Array(await file.arrayBuffer());
  const text = new TextDecoder().decode(bytes).trim();
  // A .psbt file is either raw binary or already-base64 text; both end up as
  // base64 in the paste box, decoded when the user hits Add (BIP 370 first,
  // BIP 174 upgrade as the fallback — the same auto-classification as a
  // direct paste).
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

// The sort seed is PSBT state (PSBT_GLOBAL_SORT_SEED), not a UI parameter:
// explicit sort keys or a stored seed mean the backend sorts with what the
// PSBT carries. Only a fragment with NEITHER prompts — a modal asking for
// the missing seed (resolves null on cancel).
function sortSeedNeeded(fragment: SessionFragment): boolean {
  const summary = fragmentSummary(fragment.inspect);
  return summary.sortMode !== "explicit" && !summary.seedHex;
}

let sortSeedResolve: ((seed: string | null) => void) | null = null;

function promptSortSeed(fragmentKey: string): Promise<string | null> {
  const dialog = el<HTMLDialogElement>("sortSeedDialog");
  el<HTMLElement>("sortSeedDialogWhy").textContent =
    `${fragmentKey} carries no explicit sort keys and no PSBT_GLOBAL_SORT_SEED — ` +
    `the sorter role needs a seed. It rides this one request; the sorted result stores it.`;
  const input = el<HTMLInputElement>("sortSeedInput");
  input.value = "";
  return new Promise((resolve) => {
    sortSeedResolve?.(null); // a re-prompt cancels any dangling prompt
    sortSeedResolve = resolve;
    dialog.showModal();
    input.focus();
  });
}

function settleSortSeed(seed: string | null): void {
  const dialog = el<HTMLDialogElement>("sortSeedDialog");
  if (dialog.open) dialog.close();
  sortSeedResolve?.(seed);
  sortSeedResolve = null;
}

// Resolve the seed to send for a fragment: undefined = the PSBT's own
// records suffice; a string = the prompted seed; null = the user cancelled.
async function sortSeedFor(fragment: SessionFragment): Promise<string | undefined | null> {
  if (!sortSeedNeeded(fragment)) return undefined;
  const seed = await promptSortSeed(fragment.key);
  return seed === null ? null : seed;
}

async function sortSelected(): Promise<void> {
  const selected = requireEnabled("sort");
  if (!selected) return;
  const seed = await sortSeedFor(selected[0]);
  if (seed === null) return; // prompt cancelled
  try {
    await addResponse(
      await backend.sortPsbt(selected[0].psbt, seed),
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

async function applySetTxModifiableFix(fragment: SessionFragment): Promise<SessionFragment> {
  const response = await backend.applyPsbtEdits(fragment.psbt, [
    { map: "global", key: TX_MODIFIABLE_KEY_HEX, value: TX_MODIFIABLE_BOTH_HEX },
  ]);
  if (response.psbt === undefined) {
    throw new Error(response.error ?? "the tx-modifiable raw edit failed save-time validation");
  }
  const minted = await addResponse(
    { psbt: response.psbt, inspect: response.inspect },
    "edit",
    `raw edit of ${fragment.key}: TX_MODIFIABLE set to both (override fix)`,
  );
  logEvent(`override fix: ${fragment.key} → ${minted.key} (TX_MODIFIABLE flags set via /api/edit)`);
  return minted;
}

async function applySortFirstFix(fragment: SessionFragment): Promise<SessionFragment> {
  // Same seed policy as the Sort op: the PSBT's own records when it has
  // them, the modal prompt when it doesn't (Generate lives in the dialog).
  const seed = await sortSeedFor(fragment);
  if (seed === null) {
    throw new Error("sort-first fix cancelled: no sort seed provided");
  }
  const sorted = await addResponse(
    await backend.sortPsbt(fragment.psbt, seed),
    "sort",
    `sort of ${fragment.key} (override fix)`,
  );
  logEvent(`override fix: ${fragment.key} → ${sorted.key} (sorted via /api/sort)`);
  return sorted;
}

// The gate's armed-override repair for this action, if any (null = run the
// action on the selection as-is, the send-as-is override semantics).
function armedOverrideFix(action: SessionAction) {
  const state = actionState(action, enablementContext());
  return state.enabled && state.overridden ? (state.gate?.fix ?? null) : null;
}

async function atomizeSelected(): Promise<void> {
  const selected = requireEnabled("atomize");
  if (!selected) return;
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
  revealPanel("assignIdsPanel");
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
  showOutput(`${selected[0].key} — BIP 370 base64`, selected[0].psbt);
}

async function exportSelectedBip174(): Promise<void> {
  const selected = requireEnabled("export-bip174");
  if (!selected) return;
  const fix = armedOverrideFix("export-bip174");
  try {
    let target = selected[0];
    if (fix?.kind === "sort-first") {
      target = await applySortFirstFix(target);
    }
    const exported = await backend.exportBip174(target.psbt);
    showOutput(`${target.key} — BIP 174 base64`, exported.psbt);
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
      // Placeholder, not value: a pristine row must count as BLANK so the
      // zero-row create path stays reachable; an omitted vout defaults to 0
      // when a txid is entered (buildCreateRequest).
      '<input data-role="vout" type="number" min="0" placeholder="0"></label>';
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

// --- compile-time capabilities (GET /api/capabilities) ----------------------
//
// Which transports THIS ptj binary can drive is a compile-time fact; the
// Sync dropdown reflects it up front (disabled option + a typed reason)
// instead of failing on use. The route serves the versioned capability
// catalog (crates/ptj/src/capabilities.rs): each recognized kind carries an
// availability bit and, when unusable, a reason CODE — copy is assembled
// here from the code, not parsed from server prose. Fetched directly rather
// than through the Backend seam: this is deployment metadata of the HTTP
// shell, not a PSBT operation. A missing route or an unknown catalog
// version degrades to everything-enabled with precise use-time errors.

const CAPABILITY_CATALOG_VERSION = 1;

type TransportCapability = {
  browserSelectable: boolean;
  reasonCode: string | null;
  feature: string | null;
};

let transportCapabilities: Map<string, TransportCapability> | null = null;

// The full-sentence refusal for the sync path (and the event log); null
// when the kind is selectable or the catalog never loaded.
function transportUnavailable(transport: string): string | null {
  const capability = transportCapabilities?.get(transport);
  if (!capability || capability.browserSelectable) return null;
  if (capability.reasonCode === "feature-disabled" && capability.feature) {
    return `${transport} is unavailable in this build — rebuild ptj with --features ${capability.feature}`;
  }
  if (capability.reasonCode === "unauthored") {
    return `${transport} is recognized but not implemented by any build yet`;
  }
  // Available to the host but not browser input (plugin executables are
  // host configuration), or a reason code this UI predates.
  return `${transport} is not selectable from the browser`;
}

function markSyncTransportOptions(): void {
  if (!transportCapabilities) return;
  const select = el<HTMLSelectElement>("syncTransport");
  for (const option of Array.from(select.options)) {
    const reason = transportUnavailable(option.value);
    if (reason === null) continue;
    option.disabled = true;
    if (!option.text.includes(" — ")) {
      // Strip the leading "<kind> is" — the option already names the kind.
      option.text = `${option.text} — ${reason.replace(`${option.value} is `, "")}`;
    }
    if (select.value === option.value) {
      select.value = "local";
      renderSyncFields();
    }
  }
}

async function loadCapabilities(): Promise<void> {
  try {
    const response = await fetch("/api/capabilities");
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const catalog = asObject((await response.json()) as unknown);
    const entries = asArray(catalog?.transports);
    if (catalog?.version !== CAPABILITY_CATALOG_VERSION || !entries) {
      // Degrading silently would leave no trace of why disabled-marking
      // vanished; availability still falls back to precise use-time errors.
      logEvent(
        `capability catalog not understood (version ${String(catalog?.version)}, ` +
          `want ${CAPABILITY_CATALOG_VERSION}) — transport availability unknown`,
      );
      return;
    }
    const capabilities = new Map<string, TransportCapability>();
    for (const raw of entries) {
      const entry = asObject(raw);
      const kind = asString(entry?.kind);
      if (!entry || kind === null) continue;
      const reason = asObject(entry.reason);
      capabilities.set(kind, {
        browserSelectable: entry.browserSelectable === true,
        reasonCode: asString(reason?.code) ?? null,
        feature: asString(reason?.feature) ?? null,
      });
    }
    transportCapabilities = capabilities;
    markSyncTransportOptions();
    const off = Array.from(el<HTMLSelectElement>("syncTransport").options)
      .map((option) => transportUnavailable(option.value))
      .filter((reason): reason is string => reason !== null);
    if (off.length) {
      // Each reason is a full sentence naming its kind; some (unauthored)
      // are not this build's fault, so the prefix stays neutral.
      logEvent(`sync transports unavailable: ${off.join("; ")}`);
    }
  } catch (error) {
    // Covers both a route that never answered and a 200 with an unusable
    // body — either way availability is unknown, not everything-off.
    logEvent(
      "transport availability unknown (/api/capabilities unusable) — " +
        (error instanceof Error ? error.message : String(error)),
    );
  }
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
  // buildSyncRequest always sets transport; the DTO type keeps it optional
  // for the legacy no-transport request shape.
  const unavailable = transportUnavailable(built.value.transport ?? "local");
  if (unavailable) {
    showStatus(unavailable, true);
    setSyncState("error", unavailable);
    return;
  }
  const runButton = el<HTMLButtonElement>("syncRun");
  runButton.disabled = true;
  setSyncState("syncing", `${sourceLabel} over ${built.value.transport}…`);
  showStatus("syncing…", false);
  try {
    const response = await backend.syncPsbts(built.value);
    let summary: string;
    if (response.psbt !== undefined) {
      const converged = await addResponse(
        { psbt: response.psbt, inspect: response.inspect },
        "sync",
        `sync convergence (${sourceLabel})`,
      );
      const view = negotiationView(response);
      summary =
        `${sourceLabel}: converged into ${converged.key}; ` +
        `${view.paymentCount} payment record(s), ${view.confirmationCount} confirmation record(s) out of band`;
    } else {
      // Ticket-only response: the request minted an EMPTY shared document —
      // nothing to converge, no fragment to add (a fabricated one would
      // misstate the document contents).
      summary =
        `${sourceLabel}: created an empty shared document — share the ticket so peers can publish into it`;
    }
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
  // Zero-selection syncs that are legitimate: local sync runs from
  // server-side sources, and iroh with ticket-out CREATES an empty shared
  // document (peers publish into it later). Every other shape syncs the
  // selection.
  const createsEmptyDoc =
    syncTransportValue() === "iroh" && el<HTMLInputElement>("syncIrohTicketOut").checked;
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
  // A render replaces the card nodes, so a live drag's captured node (and
  // its finish handlers) would be orphaned — wireDrag would never clear and
  // every future pointerdown would bail on it. Concurrent renders (an async
  // sync completing mid-gesture) cancel the gesture instead.
  if (wireDrag) cancelWireDrag();
  renderFocus();
  renderPeerShelf();
  renderSessionShelf();
  renderFragments();
  renderObjects();
  // After the card passes: the idle wire hint asks the DOM whether any
  // wireable card exists.
  renderWireStatus();
  renderOps();
  el<HTMLElement>("createWireTarget").hidden = !(wire.source && wire.source.kind === "utxo");
}

// --- wiring (DOM event hookup) -----------------------------------------------------

function wireDom(): void {
  for (const [id] of ACTION_BUTTONS) {
    BASE_TITLE.set(id, el<HTMLButtonElement>(id).title);
  }

  el<HTMLButtonElement>("addObject").addEventListener("click", () => void addObject());
  el<HTMLInputElement>("uploadInput").addEventListener("change", () => void loadUpload());
  const rawDialog = el<HTMLDialogElement>("rawDialog");
  el<HTMLButtonElement>("rawDialogClose").addEventListener("click", () => rawDialog.close());
  rawDialog.addEventListener("click", (event) => {
    // A click on the dialog element itself is the backdrop (the content
    // is fully covered by the dialog's children).
    if (event.target === rawDialog) rawDialog.close();
  });
  // The sort-seed prompt settles the pending promise: confirm resolves the
  // trimmed hex, cancel/backdrop/Esc resolve null (the sort is abandoned).
  const sortSeedDialog = el<HTMLDialogElement>("sortSeedDialog");
  el<HTMLButtonElement>("sortSeedConfirm").addEventListener("click", () => {
    settleSortSeed(el<HTMLInputElement>("sortSeedInput").value.trim() || null);
  });
  el<HTMLButtonElement>("sortSeedCancel").addEventListener("click", () => settleSortSeed(null));
  el<HTMLButtonElement>("sortSeedGenerate").addEventListener("click", () => {
    const bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    el<HTMLInputElement>("sortSeedInput").value = seedFromRandomBytes(bytes);
  });
  sortSeedDialog.addEventListener("click", (event) => {
    if (event.target === sortSeedDialog) settleSortSeed(null);
  });
  sortSeedDialog.addEventListener("cancel", () => settleSortSeed(null));
  el<HTMLButtonElement>("addDrawerToggle").addEventListener("click", () => {
    setAddDrawer(el<HTMLElement>("addDrawer").hidden);
  });
  el<HTMLButtonElement>("addDrawerClose").addEventListener("click", () => setAddDrawer(false));
  el<HTMLButtonElement>("addPeerQuick").addEventListener("click", () => setAddDrawer(true, true));
  el<HTMLFormElement>("manualPeerForm").addEventListener("submit", addManualPeer);
  initSamplesPalette();

  // Disabled controls swallow — and Firefox outright suppresses — their own
  // pointer events, so disabled op buttons are pointer-events:none (styles)
  // and the press/hover lands on the toolbar itself in every engine. The
  // point is hit-tested against the disabled buttons' rects, because
  // elementsFromPoint skips pointer-events:none nodes.
  const disabledOpAt = (x: number, y: number): HTMLButtonElement | undefined =>
    Array.from(
      el<HTMLElement>("sessionOps").querySelectorAll<HTMLButtonElement>("button:disabled"),
    ).find((node) => {
      if (node.dataset.action === undefined) return false;
      const rect = node.getBoundingClientRect();
      return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    });
  const surfaceDisabledOp = (event: PointerEvent): void => {
    const hit = disabledOpAt(event.clientX, event.clientY);
    if (!hit) return;
    const hint = el<HTMLElement>("opsHint");
    hint.textContent = `${hit.textContent}: ${hit.dataset.why || "unavailable"}`;
    hint.hidden = false;
  };
  // Hover surfaces the reason too: pointer-events:none also disabled the
  // native title tooltip, so the hint line takes over that duty.
  el<HTMLElement>("sessionOps").addEventListener("pointerdown", surfaceDisabledOp);
  el<HTMLElement>("sessionOps").addEventListener("pointermove", surfaceDisabledOp);

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
  // Escape cancels a live drag-to-wire gesture.
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && wireDrag) cancelWireDrag();
  });
  el<HTMLButtonElement>("wireJoinAll").addEventListener("click", () => void joinAllWires());
  el<HTMLButtonElement>("wireClearAll").addEventListener("click", clearPendingWires);
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
      wireTo({ kind: "create", key: "create" });
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
  void loadCapabilities();
  render();
}

wireDom();
