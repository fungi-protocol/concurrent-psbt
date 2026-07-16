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
import { formatSatAmount, parseBitcoinUri, seedFromRandomBytes } from "../model.js";
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
  authorizePeerOnSession,
  applyTxOutputs,
  beginWire,
  bridgeGroupContaining,
  completeWire,
  componentPlan,
  dropFragmentKey,
  emptyObjects,
  enrichDescriptor,
  idleWire,
  mergeSessions,
  mineFragmentKeys,
  writeSessionContent,
  mintPeer,
  mintSession,
  overviewFocus,
  peerBridgeGroups,
  peerByKey,
  peerUsableForSync,
  pruneWires,
  queueWire,
  retiredByDerivation,
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
  type WireComponent,
  type WireGesture,
} from "./wiring.js";
import {
  curveBetween,
  curveMidpoint,
  laneLayout,
  type LaneLayout,
  type LayoutNode,
  type LayoutRect,
} from "./layout.js";
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
// The shared key also lands as a data attribute, so hovering a descriptor
// card cross-references everything of the same identity (the delegated
// hover in wireDom dims the rest).
function colorizeIdentity(node: HTMLElement, colorKey: string | null): void {
  if (!colorKey) return;
  node.classList.add("session-colorized");
  node.dataset.identityKey = colorKey;
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

// --- bottom drawers ----------------------------------------------------------
// Every utility docks at the bottom of the page as a drawer behind the
// drawer bar. ONE drawer opens at a time; Esc, the backdrop, a [×], or
// re-pressing the bar button closes it.
const DRAWER_IDS = [
  "addDrawer",
  "createDrawer",
  "syncDrawer",
  "negotiateDrawer",
  "editorDrawer",
  "assignIdsDrawer",
  "exportDrawer",
  "logDrawer",
] as const;
type DrawerId = (typeof DRAWER_IDS)[number];

function openDrawerId(): DrawerId | null {
  return DRAWER_IDS.find((id) => !el<HTMLElement>(id).hidden) ?? null;
}

function setDrawer(id: DrawerId | null): void {
  for (const drawerId of DRAWER_IDS) {
    el<HTMLElement>(drawerId).hidden = drawerId !== id;
  }
  for (const toggle of Array.from(
    document.querySelectorAll<HTMLButtonElement>("[data-drawer]"),
  )) {
    toggle.setAttribute("aria-expanded", String(toggle.dataset.drawer === id));
  }
}

function closeDrawer(id: DrawerId): void {
  if (openDrawerId() === id) setDrawer(null);
}

// Opening a panel must be VISIBLE: unhiding something below the fold reads
// as a dead button (the live-review symptom on Edit). Every utility panel
// is a bottom drawer now, so reveal = open its drawer (which floats above
// the fold by construction) + move focus to the panel so keyboard/AT users
// land where the action went.
const PANEL_DRAWERS = {
  editorPanel: "editorDrawer",
  assignIdsPanel: "assignIdsDrawer",
  outputPanel: "exportDrawer",
  createForm: "createDrawer",
  syncResults: "syncDrawer",
} as const;

function revealPanel(id: keyof typeof PANEL_DRAWERS): void {
  setDrawer(PANEL_DRAWERS[id]);
  const panel = el<HTMLElement>(id);
  panel.tabIndex = -1;
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
// that decode to no script stay textual — there is no script to fingerprint.
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

// ⊥ ⊔ x = x: a join whose result dedupes onto one of its operands is
// mathematically a success and visually a no-op — every other operand was
// already contained. Say so where the user looks (status bar + a chip on
// the surviving card) instead of leaving "already loaded" buried in the
// event log; a fresh result keeps the silent status clear.
function reportJoinOutcome(joined: SessionFragment, sources: SessionFragment[]): void {
  if (!sources.some((source) => source.key === joined.key)) {
    showStatus("", false);
    return;
  }
  const contained = sources
    .filter((source) => source.key !== joined.key)
    .map((source) => source.key);
  const text =
    `join: ${contained.join(", ")} ⊑ ${joined.key} — the result IS ${joined.key}, nothing new to add`;
  showStatus(text, false);
  logEvent(text);
  flashWireNotice({ kind: "fragment", key: joined.key }, "join absorbed — nothing new", "absorbed");
}

// Post-derivation settlement. Fragments are VALUE TYPES: once an op has
// produced its result, stale sessionless source copies are retired — local
// derivations REPLACE their sources instead of piling grow-only clutter
// into Mine (retiredByDerivation carries the full rule: results and
// register contents survive; registers change only through an explicit
// write gesture, never here).
function settleDerivation(
  sourceKeys: readonly string[],
  resultKeys: readonly string[],
  livesOn: string,
): void {
  const fragmentKeys = session.fragments.map((fragment) => fragment.key);
  for (const key of retiredByDerivation(sourceKeys, resultKeys, objects, fragmentKeys)) {
    session = removeFragment(session, key);
    objects = dropFragmentKey(objects, key);
    detailLevels.delete(key);
    lineage.delete(key);
    logEvent(`retired ${key} — its value lives on in ${livesOn}`);
  }
}

function settleJoin(operandKeys: readonly string[], resultKey: string): void {
  settleDerivation(operandKeys, [resultKey], resultKey);
}

// A minting op replaces its source by default; the surface's "keep the
// original" checkbox opts the gesture out. The toolbar box governs the
// one-click ops, each saving drawer carries its own.
function settleMint(
  keepBoxId: string,
  sourceKeys: readonly string[],
  resultKeys: readonly string[],
): void {
  if (el<HTMLInputElement>(keepBoxId).checked) return;
  settleDerivation(sourceKeys, resultKeys, resultKeys.join(", "));
  render();
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

// Transient card feedback (the demo's failure pulse, card-shaped, plus an
// ink-toned success cousin): the pulse pins a short chip to the card for a
// moment; the status bar carries the full text persistently. "rejected" is
// the red refusal pulse; "absorbed" marks a join whose result deduped onto
// this card — correct, but visually a no-op without the cue.
type WireFlashTone = "rejected" | "absorbed";
let wireFlash: { ref: NodeRef; text: string; tone: WireFlashTone } | null = null;

function flashWireNotice(ref: NodeRef, text: string, tone: WireFlashTone): void {
  const flash = { ref, text, tone };
  wireFlash = flash;
  window.setTimeout(() => {
    if (wireFlash === flash) {
      wireFlash = null;
      render();
    }
  }, 1800);
}

function flashWireRejection(ref: NodeRef, text: string): void {
  flashWireNotice(ref, text, "rejected");
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

// Near-miss magnet: a pointer close to a compatible card still lands the
// wire — canvas cards are small islands and a pixel-exact release is a
// touch-hostile ask. Only compatible targets attract; blocked cards still
// explain themselves on a direct hit but never pull the pointer.
const WIRE_SNAP_RADIUS_PX = 32;

function snapWireTarget(x: number, y: number, source: NodeRef): HTMLElement | null {
  const direct = wireTargetAt(x, y);
  if (direct) return direct;
  let best: HTMLElement | null = null;
  let bestDistance = WIRE_SNAP_RADIUS_PX;
  for (const node of Array.from(document.querySelectorAll<HTMLElement>("[data-wire-kind]"))) {
    const ref = wireRefOf(node);
    if (!ref || sameRef(ref, source)) continue;
    const rect = node.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) continue; // hidden / in a closed drawer
    const dx = Math.max(rect.left - x, 0, x - rect.right);
    const dy = Math.max(rect.top - y, 0, y - rect.bottom);
    const distance = Math.hypot(dx, dy);
    if (distance >= bestDistance) continue;
    if (wireDisposition(wireVerdict(source, ref, objects)) !== "compatible") continue;
    best = node;
    bestDistance = distance;
  }
  return best;
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

// Only the pointerdown that ARMS a gesture lives on the card — cards are
// rebuilt every render, so the move/finish logic listens on the document
// (wireDom) and reads the off-DOM wireDrag state. A render replacing the
// source card mid-gesture loses that card's pointer capture, but the
// bubbled events still reach the document and the gesture completes.
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
}

function wireDragMove(event: PointerEvent): void {
  if (!wireDrag || wireDrag.pointerId !== event.pointerId) return;
  if (!wireDrag.active) {
    if (
      Math.hypot(event.clientX - wireDrag.startX, event.clientY - wireDrag.startY) <
      WIRE_DRAG_THRESHOLD_PX
    ) {
      return;
    }
    wireDrag.active = true;
    wire = beginWire(wireDrag.ref.kind, wireDrag.ref.key);
    paintWireTargets();
  }
  updateWireDragLine(wireDrag.startX, wireDrag.startY, event.clientX, event.clientY);
  // The hover preview uses the same magnet as the drop, so what lights up
  // is exactly what a release would hit.
  const hover = snapWireTarget(event.clientX, event.clientY, wireDrag.ref);
  for (const painted of Array.from(document.querySelectorAll<HTMLElement>(".session-wire-hover"))) {
    if (painted !== hover) painted.classList.remove("session-wire-hover");
  }
  if (hover && !sameRef(wireRefOf(hover), wireDrag.ref)) hover.classList.add("session-wire-hover");
}

function finishWireDrag(event: PointerEvent, completed: boolean): void {
  if (!wireDrag || wireDrag.pointerId !== event.pointerId) return;
  const { ref, node, active } = wireDrag;
  if (node.isConnected && node.hasPointerCapture?.(event.pointerId)) {
    node.releasePointerCapture(event.pointerId);
  }
  wireDrag = null;
  if (!active) return; // a plain click: selection/detail handlers take it
  hideWireDragLine();
  clearWirePaint();
  suppressNextClick = true;
  const target = completed ? snapWireTarget(event.clientX, event.clientY, ref) : null;
  const targetRef = target ? wireRefOf(target) : null;
  if (targetRef && !sameRef(targetRef, ref)) {
    wireTo(targetRef); // queues (or explains) and re-renders
  } else {
    wire = idleWire();
    render();
  }
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
        // The absorbed-join outcome message must survive this function's
        // generic status clear, so this case reports and returns itself.
        reportJoinOutcome(joined, [left, right]);
        settleJoin([left.key, right.key], joined.key);
        return true;
      }
      case "fragment-into-session": {
        // Write the value into the register: content ⊔ fragment, written
        // back. Publishing MOVES the value — settleJoin retires the stale
        // sessionless copy (unless another register still references it).
        const sessionKey = source.kind === "session" ? source.key : target.key;
        const fragmentKey = source.kind === "fragment" ? source.key : target.key;
        const sessionObject = sessionByKey(objects, sessionKey);
        const fragment = fragmentByKey(fragmentKey);
        if (!sessionObject || !fragment) return false;
        const content = sessionObject.contentKey
          ? fragmentByKey(sessionObject.contentKey)
          : null;
        let result = fragment;
        if (content && content.key !== fragment.key) {
          result = await addResponse(
            await backend.joinPsbts([content.psbt, fragment.psbt]),
            "join",
            `⊔ write of ${fragment.key} into ${sessionObject.name}`,
          );
          reportJoinOutcome(result, [content, fragment]);
        }
        objects = writeSessionContent(objects, sessionKey, result.key);
        settleJoin(
          content ? [fragment.key, content.key] : [fragment.key],
          result.key,
        );
        logEvent(`wrote ${fragmentKey} into ${sessionObject.name} — register now ${result.key}`);
        // Keep the absorbed-join status (when one was reported) alive past
        // the generic clear below.
        return true;
      }
      case "peer-into-session": {
        // Authorization: connect the peer (and its bridge group) to the
        // register. The peer set is grow-only — re-authorizing is an
        // idempotent no-op, reported, never an error.
        const sessionKey = source.kind === "session" ? source.key : target.key;
        const authPeerKey = source.kind === "peer" ? source.key : target.key;
        const sessionObject = sessionByKey(objects, sessionKey);
        const authPeer = peerByKey(objects, authPeerKey);
        if (!sessionObject || !authPeer) return false;
        if (sessionObject.peerKeys.includes(authPeerKey)) {
          const text = `${authPeer.name} is already authorized on ${sessionObject.name} — nothing to add`;
          showStatus(text, false);
          logEvent(text);
          return true;
        }
        objects = authorizePeerOnSession(objects, sessionKey, authPeerKey);
        objects = unionBridgedPeersIntoSessions(objects);
        logEvent(
          `authorized ${authPeer.name} on ${sessionObject.name} — the peer can now read/write the register` +
            ` (bridged peers ride along)`,
        );
        break;
      }
      case "add-create-input": {
        const utxoKey = source.kind === "utxo" ? source.key : target.key;
        const utxo = objects.utxos.find((candidate) => candidate.key === utxoKey);
        // The prefilled row lives in the create drawer — open it so the
        // wire's effect is visible, not buried in a closed sheet.
        revealPanel("createForm");
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
        // Client-orchestrated merge (Q3): merging registers ⊔s their
        // contents and ∪s their peer sets. The UI model unions the peers
        // and retires the sources; the contents join through the existing
        // /api/join route and the result is written into the merged
        // register. Every decision and every limit is logged honestly.
        const leftName = sessionByKey(objects, source.key)?.name ?? source.key;
        const rightName = sessionByKey(objects, target.key)?.name ?? target.key;
        const merge = mergeSessions(objects, source.key, target.key);
        if (!merge.merged) return false;
        objects = merge.state;
        remaps?.set(`session:${source.key}`, merge.merged.key);
        remaps?.set(`session:${target.key}`, merge.merged.key);
        logEvent(
          `merged sessions ${leftName} ⋈ ${rightName} → ${merge.merged.name} ` +
            `(${merge.merged.peerKeys.length} peer(s) unioned)`,
        );
        for (const note of merge.notes) {
          logEvent(`session merge: ${note}`);
        }
        const leftContent = merge.contents.left ? fragmentByKey(merge.contents.left) : null;
        const rightContent = merge.contents.right ? fragmentByKey(merge.contents.right) : null;
        if (leftContent && rightContent && leftContent.key !== rightContent.key) {
          const joined = await addResponse(
            await backend.joinPsbts([leftContent.psbt, rightContent.psbt]),
            "join",
            `⊔ register merge of ${leftName}, ${rightName}`,
          );
          objects = writeSessionContent(objects, merge.merged.key, joined.key);
          settleJoin([leftContent.key, rightContent.key], joined.key);
          logEvent(
            `register contents joined: ${leftContent.key} ⊔ ${rightContent.key} → ${joined.key} ` +
              `(written into ${merge.merged.name})`,
          );
        } else {
          const lone = leftContent ?? rightContent;
          logEvent(
            lone
              ? `merged register carries ${lone.key} (only one register held a value)`
              : "both registers were empty — the merged register starts empty",
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
    reportJoinOutcome(joined, members);
    settleJoin(
      members.map((fragment) => fragment.key),
      joined.key,
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

// --- join probes ------------------------------------------------------------------
// A queued wire is a PROMISE of a join, and joins are partial: the ⊔ can
// fail (conflicting fields). Rather than let the user discover that at
// commit time, every queued PSBT join is COMPUTED as soon as it is queued
// (the backend join is pure — committing just re-runs it and keeps the
// result). A conflicted wire trades its Join pill for an explanation, and
// a conflicted component blocks the toolbar Join the same way — including
// the case where every adjacent pair joins cleanly but the component's
// least upper bound does not.

type JoinProbe =
  | { state: "pending" }
  | { state: "ok" }
  | { state: "conflict"; detail: string };

// Probes are keyed by wire/component and re-run when the PSBTs under the
// endpoints change (a register can advance while its wire sits queued).
interface ProbeEntry {
  signature: string;
  probe: JoinProbe;
}

const wireProbes = new Map<string, ProbeEntry>();
const componentProbes = new Map<string, ProbeEntry>();

function wireLabel(entry: PendingWire): string {
  return (
    wireVerdict(entry.source, entry.target, objects).label ??
    `${nodeName(entry.source)} ⋈ ${nodeName(entry.target)}`
  );
}

// The fragments a wire endpoint contributes to its join: the fragment
// itself, or the register's current content. Peers and the other
// non-PSBT endpoints contribute nothing — their wires have effects, not
// joins, and never conflict.
function probeFragments(ref: NodeRef): SessionFragment[] {
  if (ref.kind === "fragment") {
    const fragment = fragmentByKey(ref.key);
    return fragment ? [fragment] : [];
  }
  if (ref.kind === "session") {
    const contentKey = sessionByKey(objects, ref.key)?.contentKey;
    const content = contentKey ? fragmentByKey(contentKey) : null;
    return content ? [content] : [];
  }
  return [];
}

function dedupeFragments(fragments: SessionFragment[]): SessionFragment[] {
  const byKey = new Map(fragments.map((fragment) => [fragment.key, fragment]));
  return [...byKey.values()];
}

function componentKey(component: WireComponent): string {
  return component.wires
    .map((entry) => wireKey(entry.source, entry.target))
    .sort()
    .join(" | ");
}

function probeInto(map: Map<string, ProbeEntry>, key: string, fragments: SessionFragment[]): void {
  const signature = fragments.map((fragment) => fragment.key).join("+");
  if (map.get(key)?.signature === signature) return;
  // Fewer than two PSBTs (or none at all): there is no join to compute.
  if (fragments.length < 2) {
    map.set(key, { signature, probe: { state: "ok" } });
    return;
  }
  map.set(key, { signature, probe: { state: "pending" } });
  const settle = (probe: JoinProbe): void => {
    const current = map.get(key);
    // A newer probe (changed signature) supersedes this one; stand down.
    if (!current || current.signature !== signature) return;
    map.set(key, { signature, probe });
    render();
  };
  backend
    .joinPsbts(fragments.map((fragment) => fragment.psbt))
    .then(() => settle({ state: "ok" }))
    .catch((error: unknown) =>
      settle({
        state: "conflict",
        detail: error instanceof Error ? error.message : String(error),
      }),
    );
}

// Keep the probe maps in step with the live queue: probe what is new or
// changed, drop what is gone. Called on every queue render; completed
// probes re-render, which arrives back here as a no-op (same signatures).
function refreshJoinProbes(wires: PendingWire[]): void {
  const liveWires = new Set<string>();
  for (const entry of wires) {
    const key = wireKey(entry.source, entry.target);
    liveWires.add(key);
    probeInto(
      wireProbes,
      key,
      dedupeFragments([...probeFragments(entry.source), ...probeFragments(entry.target)]),
    );
  }
  for (const key of [...wireProbes.keys()]) {
    if (!liveWires.has(key)) wireProbes.delete(key);
  }
  const liveComponents = new Set<string>();
  for (const component of wireComponents(wires)) {
    const key = componentKey(component);
    liveComponents.add(key);
    // A single-wire component IS its wire; only multi-wire components can
    // hide a LUB conflict behind clean pairwise joins.
    probeInto(
      componentProbes,
      key,
      component.wires.length > 1
        ? dedupeFragments(component.nodes.flatMap((ref) => probeFragments(ref)))
        : [],
    );
  }
  for (const key of [...componentProbes.keys()]) {
    if (!liveComponents.has(key)) componentProbes.delete(key);
  }
}

function wireConflict(entry: PendingWire): string | null {
  const probe = wireProbes.get(wireKey(entry.source, entry.target))?.probe;
  return probe?.state === "conflict" ? probe.detail : null;
}

// Everything conflicted in the queue right now: conflicted wires, plus
// components whose LUB fails even though their wires look fine.
function queueConflicts(wires: PendingWire[]): { label: string; detail: string }[] {
  const conflicts: { label: string; detail: string }[] = [];
  for (const entry of wires) {
    const detail = wireConflict(entry);
    if (detail !== null) conflicts.push({ label: wireLabel(entry), detail });
  }
  for (const component of wireComponents(wires)) {
    const probe = componentProbes.get(componentKey(component))?.probe;
    if (probe?.state !== "conflict") continue;
    const names = component.nodes.map((ref) => nodeName(ref)).join(", ");
    conflicts.push({
      label: `⊔ of the whole component (${names}) — its wires may join pairwise, but the least upper bound conflicts`,
      detail: probe.detail,
    });
  }
  return conflicts;
}

function openConflictModal(title: string, conflicts: { label: string; detail: string }[]): void {
  const dialog = el<HTMLDialogElement>("rawDialog");
  el<HTMLElement>("rawDialogTitle").textContent = title;
  const body = el<HTMLElement>("rawDialogBody");
  body.textContent = "";
  for (const conflict of conflicts) {
    const block = document.createElement("section");
    block.className = "session-conflict-block";
    const heading = document.createElement("h4");
    heading.textContent = conflict.label;
    const detail = document.createElement("pre");
    detail.className = "session-fragment-detail";
    detail.textContent = conflict.detail;
    block.append(heading, detail);
    body.append(block);
  }
  dialog.showModal();
}

// --- in-flight state --------------------------------------------------------------
// Honest busy feedback: a token is set exactly while its real backend call
// is pending — no timers, no fake counters. Cards wear session-busy (and
// aria-busy) while their token is set; a joining wire's edge marches.
// Tokens are node ids ("fragment:psbt-1", "session:s-1", "peer:p-1") plus
// "edge:<wireKey>" for the wire being joined.

const inflight = new Set<string>();

function busyToken(ref: NodeRef): string {
  return `${ref.kind}:${ref.key}`;
}

async function withBusy<T>(tokens: string[], run: () => Promise<T>): Promise<T> {
  for (const token of tokens) inflight.add(token);
  render();
  try {
    return await run();
  } finally {
    for (const token of tokens) inflight.delete(token);
    render();
  }
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
  const applied = await withBusy(
    [busyToken(entry.source), busyToken(entry.target), `edge:${key}`],
    () => executeWire(entry.source, entry.target),
  );
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
//
// The drain spans many awaits, so a second toolbar press (or a pill click
// routed here) mid-drain would re-plan and double-execute wires the first
// pass is still applying — one drain runs at a time.
let joinAllRunning = false;

async function joinAllWires(): Promise<void> {
  if (joinAllRunning) return;
  const wires = livePendingWires();
  const components = wireComponents(wires);
  if (!components.length) {
    showStatus("queue one or more wires before joining", true);
    render();
    return;
  }
  // Blocked, not failed: a known-conflicted queue explains itself instead
  // of running joins that were already computed to fail.
  const conflicts = queueConflicts(wires);
  if (conflicts.length) {
    openConflictModal("the queue cannot join — conflicts", conflicts);
    return;
  }
  const consumed = new Set<string>();
  let applied = 0;
  let failed = 0;
  joinAllRunning = true;
  try {
    for (const component of components) {
      const plan = componentPlan(component);
      const remap = new Map<string, string>();
      for (const group of plan.joinGroups) {
        const resultKey = await withBusy(
          [
            ...group.fragments.map((memberKey) => `fragment:${memberKey}`),
            ...group.wires.map((wireEntry) => `edge:${wireKey(wireEntry.source, wireEntry.target)}`),
          ],
          () => executeJoinGroup(group),
        );
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
        const source = remapWireRef(wireEntry.source, remap);
        const target = remapWireRef(wireEntry.target, remap);
        const ok = await withBusy(
          [
            busyToken(source),
            busyToken(target),
            `edge:${wireKey(wireEntry.source, wireEntry.target)}`,
          ],
          () => executeWire(source, target, remap),
        );
        if (ok) {
          applied += 1;
          consumed.add(wireKey(wireEntry.source, wireEntry.target));
        } else {
          failed += 1;
        }
      }
    }
  } finally {
    joinAllRunning = false;
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

// --- spatial canvas --------------------------------------------------------------
// The workbench is a scrolling viewport over #canvasWorld: an SVG edge
// layer under a layer of absolutely-positioned HTML cards. Positions come
// from the pure laneLayout (measured sizes in, rects out); the wrappers
// are KEYED and survive re-renders, so a position change is a transform
// transition — cards glide instead of teleporting — while their contents
// are rebuilt each render exactly like the old list items.

const canvasNodes = new Map<string, HTMLElement>();
let canvasLayout: LaneLayout | null = null;

function canvasWrapper(key: string, className: string): HTMLElement {
  let wrapper = canvasNodes.get(key);
  if (!wrapper) {
    wrapper = document.createElement("div");
    wrapper.className = className;
    canvasNodes.set(key, wrapper);
    el<HTMLElement>("nodeLayer").append(wrapper);
  }
  return wrapper;
}

function placeWrapper(wrapper: HTMLElement, x: number, y: number): void {
  wrapper.style.transform = `translate(${x}px, ${y}px)`;
}

// Content pass + measure/place pass. Renders every canvas node (peers,
// session containers, Mine fragments, lane furniture), measures the
// wrappers, runs the pure layout, and paints the rects.
function renderCanvas(): void {
  const live = new Set<string>();
  const node = (key: string, className: string, card: HTMLElement): string => {
    const wrapper = canvasWrapper(key, className);
    wrapper.replaceChildren(card);
    live.add(key);
    return key;
  };

  const peerKeys: string[] = [];
  for (const group of peerBridgeGroups(objects)) {
    const members = group
      .map((key) => peerByKey(objects, key))
      .filter((member): member is PeerObject => member !== null);
    if (!members.length) continue;
    // A bridge group renders as ONE card keyed by its first member.
    const card = members.length === 1 ? renderPeerCard(members[0]) : renderBridgeGroupCard(members);
    peerKeys.push(node(`peer:${members[0].key}`, "session-canvas-node session-node-peer", card));
  }

  const sessionKeys: string[] = [];
  for (const sessionObject of objects.sessions) {
    sessionKeys.push(
      node(
        `session:${sessionObject.key}`,
        "session-canvas-node session-node-session",
        renderSessionContainer(sessionObject),
      ),
    );
  }

  // Mine: the local-only fragments no register references. Publishing
  // (wiring Mine → session) is a visible MOVE out of the frame.
  const mineKeys: string[] = [];
  const mine = mineFragmentKeys(
    session.fragments.map((fragment) => fragment.key),
    objects,
  );
  for (const fragment of session.fragments) {
    if (!mine.includes(fragment.key)) continue;
    mineKeys.push(
      node(
        `fragment:${fragment.key}`,
        "session-canvas-node session-node-fragment",
        renderFragmentCard(fragment),
      ),
    );
  }

  // Lane furniture: labels above each lane, the frame behind Mine.
  const label = (key: string, text: string, title: string): HTMLElement => {
    const wrapper = canvasWrapper(key, "session-lane-label");
    if (wrapper.textContent !== text) wrapper.textContent = text;
    wrapper.title = title;
    live.add(key);
    return wrapper;
  };
  const peersLabel = label(
    "label:peers",
    peerKeys.length ? "peers" : "peers — none yet",
    "Ephemeral transport addresses for this page load. Adding does not pair, connect, or publish.",
  );
  const sessionsLabel = label(
    "label:sessions",
    sessionKeys.length ? "sessions" : "sessions — none yet",
    "Monotone shared registers for PSBT fragments. Wire a fragment in to write it (⊔); wire a peer in to authorize it.",
  );
  // Three-way: unpublished drafts, everything published, or nothing loaded
  // at all — "every loaded fragment is published" would be a lie on an
  // empty page.
  const mineLabel = label(
    "label:mine",
    mineKeys.length
      ? "mine — not published to any session; wiring a card to a session publishes it"
      : session.fragments.length
        ? "mine — every loaded fragment is published"
        : "mine — nothing loaded yet; paste or create a fragment to begin",
    "Local-only drafts. Wiring a card to a session writes it into that register (a visible move).",
  );
  const frame = canvasWrapper("frame:mine", "session-mine-frame");
  live.add("frame:mine");

  for (const [key, wrapper] of canvasNodes) {
    if (!live.has(key)) {
      wrapper.remove();
      canvasNodes.delete(key);
    }
  }

  // Measure, lay out, place. Wrapper widths are fixed per lane (CSS);
  // heights are whatever the cards need.
  const workbench = el<HTMLElement>("spatialWorkbench");
  const world = el<HTMLElement>("canvasWorld");
  const measure = (key: string): LayoutNode => {
    const wrapper = canvasNodes.get(key);
    return {
      key,
      width: wrapper?.offsetWidth ?? 0,
      height: wrapper?.offsetHeight ?? 0,
    };
  };
  const layout = laneLayout({
    peerGroups: peerKeys.map((key) => [measure(key)]),
    sessions: sessionKeys.map(measure),
    mine: mineKeys.map(measure),
    minWidth: workbench.clientWidth,
  });
  canvasLayout = layout;
  world.style.width = `${layout.world.width}px`;
  world.style.height = `${layout.world.height}px`;
  for (const [key, rect] of layout.positions) {
    const wrapper = canvasNodes.get(key);
    if (wrapper) placeWrapper(wrapper, rect.x, rect.y);
  }
  placeWrapper(peersLabel, layout.mineFrame.x, Math.max(0, layout.lanes.peersY - 22));
  placeWrapper(sessionsLabel, layout.mineFrame.x, layout.lanes.sessionsY - 22);
  placeWrapper(mineLabel, layout.mineFrame.x + 14, layout.lanes.mineY + 8);
  placeWrapper(frame, layout.mineFrame.x, layout.mineFrame.y);
  frame.style.width = `${layout.mineFrame.width}px`;
  frame.style.height = `${layout.mineFrame.height}px`;

  el<HTMLElement>("fragmentEmpty").hidden = session.fragments.length > 0;
}

// --- fragment cards --------------------------------------------------------------

const INPUT_ROWS_SHOWN = 3;
const OUTPUT_ROWS_SHOWN = 3;

// Single-session focus: the register's value as a flat card list (the
// canvas is hidden; this list is the whole view).
function renderFocusFragments(): void {
  const list = el<HTMLUListElement>("fragmentList");
  list.textContent = "";
  const focused =
    focus.mode === "session" && focus.sessionKey ? sessionByKey(objects, focus.sessionKey) : null;
  if (!focused) return;
  const visible = session.fragments.filter((fragment) => focused.contentKey === fragment.key);
  for (const fragment of visible) {
    const item = document.createElement("li");
    item.append(renderFragmentCard(fragment));
    list.append(item);
  }
  if (!visible.length) {
    // An empty REGISTER, not an empty workspace — the generic "No PSBTs
    // loaded yet" hint would be misleading (Mine may well hold fragments).
    const hint = document.createElement("li");
    hint.append(span("item-meta session-area-hint", "empty register — wire a fragment in"));
    list.append(hint);
  }
  el<HTMLElement>("fragmentEmpty").hidden = true;
}


// Cards are <article>s: they land in the canvas's div node layer and in
// session containers, neither of which is a list. True lists (the focus
// register list, the objects panel) wrap them in their own <li>.
function renderFragmentCard(fragment: SessionFragment): HTMLElement {
  const card = fragmentCardModel(fragment.inspect, displayNetwork());
  const item = document.createElement("article");
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
  // A subtotal earns its place only when its side is actually split
  // across groups — a lone group's side subtotal would just repeat the
  // grand total directly below it.
  const inputGroupCount = card.groups.filter((group) => group.inputs.length > 0).length;
  const outputGroupCount = card.groups.filter((group) => group.outputs.length > 0).length;
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

    // Per-group subtotals at the BOTTOM of the columns, per SIDE: a side
    // shows a subtotal only when that side is split across groups (the
    // demo's grand-total elision rule, inverted for the card layout); at
    // the collapsed mode the aggregate line IS the subtotal.
    const showInputSubtotal = group.inputs.length > 0 && inputGroupCount > 1;
    const showOutputSubtotal = group.outputs.length > 0 && outputGroupCount > 1;
    if (level !== "collapsed" && (showInputSubtotal || showOutputSubtotal)) {
      groupNode.append(groupBalanceFooter(group, showInputSubtotal, showOutputSubtotal));
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
    button("Edit", "Field-by-field editor (liberal parsing; saving replaces this fragment unless 'keep' is checked)", () => {
      editor = editorModel(fragment.key, fragment.inspect, displayNetwork());
      pendingEditorFixes.clear();
      editorOverrides.clear();
      el<HTMLInputElement>("editorKeep").checked = false;
      renderEditor([]);
      revealPanel("editorPanel");
    }),
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

function groupBalanceFooter(group: CardGroup, showInputs: boolean, showOutputs: boolean): HTMLElement {
  const footer = span("session-balance session-balance-group", "");
  footer.append(span("session-balance-sumline", ""));
  const totals = span("session-balance-row session-balance-totals", "");
  if (showInputs) {
    totals.append(balanceCell("input", "in", group.inputSubtotalSats, PARTIAL_SUBTOTAL_WHY, "subtotal"));
  }
  if (showOutputs) {
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

  // A total over one line per side would repeat the row right above it —
  // it elides (the demo's grand-total rule); the delta block below keeps
  // its own sum line, so the fee story stays readable.
  if (!sheet.totalsRedundant) {
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
  }

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
      // A fingerprintable fact renders chip-then-hex: the LifeHash sits
      // NEXT TO the bitvomit it identifies, one visual unit.
      if (pair.chipHex) value.append(lifehashBadge(pair.chipHex, `${pair.label} fingerprint`));
      value.append(pair.value);
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
    // No prevout script known: the row face stays textual — a txid chip
    // here would read as a payer identity, which the txid is not. The
    // txid's own fingerprint lives in the expanded facts, next to the
    // full outpoint (rowFacePairs chipHex).
    const short = span("item-meta", `${input.outpointTxid.slice(0, 8)}…:${input.outpointVout ?? "?"}`);
    short.title = `outpoint ${input.outpointText ?? input.outpointTxid} — prevout script unknown`;
    row.append(short);
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
  // The row face carries ONE fingerprint: the scriptPubKey — where the
  // money goes. The unique id's chip is bookkeeping identity; it sits next
  // to its hex in the expanded facts (rowFacePairs chipHex), not here.
  if (!output.uniqueIdHex && level === "expanded") {
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

// Mark a card as a wire endpoint and arm the always-on drag gesture. The
// verdict vocabulary (compatible green / blocked red / unbacked dim) is
// painted imperatively DURING a drag (paintWireTargets); at render time the
// card only wears any recent rejection pulse — queue participation is the
// pending EDGE (and its Join pill) on the canvas, not a card costume.
function decorateWireTarget(node: HTMLElement, ref: NodeRef): void {
  node.dataset.wireKind = ref.kind;
  node.dataset.wireKey = ref.key;
  armWireDrag(node, ref);
  if (wireFlash && sameRef(wireFlash.ref, ref)) {
    node.classList.add(`session-wire-${wireFlash.tone}`);
    node.append(span(`session-wire-reason session-wire-reason-${wireFlash.tone}`, wireFlash.text));
  }
  // A backend call is in flight against this node: pulse, announce, and
  // hold interaction until the promise settles (withBusy re-renders then).
  // inert makes the hold real — no clicks, drags, or focus land on a card
  // whose state is about to be replaced.
  if (inflight.has(busyToken(ref))) {
    node.classList.add("session-busy");
    node.setAttribute("aria-busy", "true");
    node.inert = true;
  }
}

// --- spatial shelves and remaining objects -----------------------------------------

// The ONE place a session renders: a container in the sessions region
// holding the register's value as a full fragment card (or an empty-register
// hint), plus the session's own actions. There is no second, published-area
// copy in the work area.
function renderSessionContainer(sessionObject: SessionObject): HTMLElement {
  const item = document.createElement("article");
  item.className = "list-item session-card session-container";
  const ref: NodeRef = { kind: "session", key: sessionObject.key };
  decorateWireTarget(item, ref);
  const head = document.createElement("div");
  head.className = "session-fragment-row";
  head.append(
    span("item-title", sessionObject.name),
    badge("session", "session-badge"),
    span(
      "item-meta",
      `${sessionObject.contentKey ? `register: ${sessionObject.contentKey}` : "empty register"} · ` +
        `${sessionObject.peerKeys.length} peer(s)`,
    ),
  );
  item.append(head);
  // The register's one growing value, as the card itself.
  const content = sessionObject.contentKey
    ? (session.fragments.find((fragment) => fragment.key === sessionObject.contentKey) ?? null)
    : null;
  if (content) {
    const inner = document.createElement("div");
    inner.className = "item-list session-card-list";
    inner.append(renderFragmentCard(content));
    item.append(inner);
  } else {
    item.append(span("item-meta session-area-hint", "empty register — wire a fragment in"));
  }
  const actions = document.createElement("div");
  actions.className = "session-card-actions";
  actions.append(
    button("Focus", "Fill the viewport with this session", () => {
      focus = sessionFocus(sessionObject.key);
      render();
    }),
    button("Sync now", "Sync this session's register value over its peers' transports", () => {
      void syncSessionOverPeer(sessionObject.key, null);
    }),
  );
  item.append(actions);
  return item;
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

function renderObjects(): void {
  const list = el<HTMLUListElement>("objectList");
  list.textContent = "";

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
      button("Copy hex", "Copy the raw transaction hex", () => copyText(utxo.rawTxHex, `${utxo.key} hex`)),
    );
    item.append(actions);
    list.append(item);
  }

  for (const descriptor of objects.descriptors) {
    const item = document.createElement("li");
    item.className = "list-item session-card session-descriptor-card";
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

// A local "peer" is a storage location on disk — same card, same wire
// gestures, but honestly badged: there is no peer identity behind it.
function peerKindBadge(peer: PeerObject): HTMLElement {
  return peer.transport === "local"
    ? badge("disk location", "session-badge")
    : badge(`peer · ${peer.transport}`, "session-badge");
}

// The peer card's reach, the mockup's "sees N session(s)": how many
// registers this peer (or any member of its bridge group) can read/write.
function sessionCountMeta(peerKey: string): HTMLElement {
  const group = bridgeGroupContaining(objects, peerKey);
  const count = objects.sessions.filter((sessionObject) =>
    sessionObject.peerKeys.some((key) => group.includes(key)),
  ).length;
  return span("item-meta", `sees ${count} session(s)`);
}

function renderPeerCard(peer: PeerObject): HTMLElement {
  const item = document.createElement("article");
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
    peerKindBadge(peer),
  );
  const identity = span("item-meta session-identity", peer.identity.slice(0, 24) + (peer.identity.length > 24 ? "…" : ""));
  identity.title = peer.identity;
  head.append(identity, sessionCountMeta(peer.key));
  item.append(head);
  if (peer.transport === "local") {
    item.append(
      span("item-meta", "storage on this machine — no peer identity is associated with this location"),
    );
  }
  const actions = document.createElement("div");
  actions.className = "session-card-actions";
  actions.append(
    button(
      peer.transport === "local" ? "Copy path" : "Copy id",
      peer.transport === "local"
        ? "Copy the server-side storage path"
        : "Copy the full transport identity",
      () => copyText(peer.identity, `${peer.key} identity`),
    ),
    unavailablePairButton(),
  );
  item.append(actions);
  return item;
}

// A bridged peer group renders as ONE peer node (the demo's green bridge
// block): one card, member chips inside, wired as a unit through its first
// member (the presenter expands any member ref to the whole group).
function renderBridgeGroupCard(members: PeerObject[]): HTMLElement {
  const item = document.createElement("article");
  item.className = "list-item session-card session-bridge-group";
  decorateWireTarget(item, { kind: "peer", key: members[0].key });
  const head = document.createElement("div");
  head.className = "session-fragment-row";
  head.append(
    span("item-title", members.map((member) => member.name).join(" + ")),
    badge(`bridge · ${members.length} peers`, "session-badge session-badge-good"),
    span("item-meta", "one peer to the session: every member receives every broadcast"),
    sessionCountMeta(members[0].key),
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
      peerKindBadge(member),
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
      button(
        member.transport === "local" ? "Copy path" : "Copy id",
        member.transport === "local"
          ? "Copy the server-side storage path"
          : "Copy the full transport identity",
        () => copyText(member.identity, `${member.key} identity`),
      ),
    );
    item.append(row);
  }
  const actions = document.createElement("div");
  actions.className = "session-card-actions";
  actions.append(unavailablePairButton());
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
  // Standing wire nodes that aren't draggable cards — the drawer-bar Create
  // button and the hidden in-drawer create target — should not advertise
  // the gesture on their own (nothing draggable exists yet).
  host.hidden =
    document.querySelector("[data-wire-kind]:not([data-drawer]):not([hidden])") === null;
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
  refreshJoinProbes(wires);
  const host = el<HTMLElement>("wireQueue");
  const list = el<HTMLUListElement>("wireQueueList");
  list.textContent = "";
  if (!wires.length) {
    host.hidden = true;
    return;
  }
  host.hidden = false;
  el<HTMLElement>("wireQueueSummary").textContent = wireQueueSummary(wires).text;
  // A conflicted queue blocks the toolbar Join — the button stays pressable
  // but opens the conflict explanation instead of running known failures.
  const conflicts = queueConflicts(wires);
  const joinAll = el<HTMLButtonElement>("wireJoinAll");
  joinAll.classList.toggle("session-join-blocked", conflicts.length > 0);
  joinAll.title = conflicts.length
    ? "The queue has computed conflicts — press to see why it cannot join"
    : "Apply every pending wire, whole connected components at a time (fragment clusters join in one n-ary call)";
  for (const wireEntry of wires) {
    const key = wireKey(wireEntry.source, wireEntry.target);
    const detail = wireConflict(wireEntry);
    const item = document.createElement("li");
    item.className = "session-wire-queue-row";
    item.append(
      span("session-wire-queue-label", wireLabel(wireEntry)),
      detail !== null
        ? button("⚠ why?", "This join was computed and it conflicts — see why", () =>
            openConflictModal(`${wireLabel(wireEntry)} — conflict`, [
              { label: wireLabel(wireEntry), detail },
            ]),
          )
        : button("Join", "Apply this wire alone", () => void joinPendingWire(key)),
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
        `${focused.name} · ${focused.contentKey ? `register: ${focused.contentKey}` : "empty register"}` +
        (peers ? ` · peers: ${peers}` : "");
    }
  }
  for (const panel of Array.from(document.querySelectorAll<HTMLElement>("[data-focus-hide]"))) {
    panel.hidden = inFocus;
  }
  // The focused-session panel is the inverse: overview never shows it.
  for (const panel of Array.from(document.querySelectorAll<HTMLElement>("[data-focus-show]"))) {
    panel.hidden = !inFocus;
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
    settleMint("editorKeep", [fragment.key], [added.key]);
    pendingEditorFixes.clear();
    editorOverrides.clear();
    editor = null;
    closeDrawer("editorDrawer");
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

// A bitcoin: URI (BIP 21/321) is a txout CREATION INTENT — one output the
// payee wants to exist. It mints a real one-output fragment via /api/create
// (the creator role assigns the output a random PSBT_OUT_UNIQUE_ID), so the
// intent joins, wires, and publishes like any other fragment — no separate
// payment node kind. PSBT_OUT_AMOUNT is not optional in PSBTv2, so a URI
// that names no amount prompts for one (mirrors the sort-seed prompt).
async function addPaymentUri(text: string): Promise<boolean> {
  const uri = parseBitcoinUri(text);
  if (!uri) {
    showStatus("bitcoin: URI unexpectedly unparsable", true);
    return false;
  }
  const amountSats = uri.valueSats > 0 ? uri.valueSats : await promptPaymentAmount(uri.address);
  if (amountSats === null) return false; // prompt cancelled — keep the paste
  try {
    const fragment = await addResponse(
      await backend.createPsbt({
        network: displayNetwork(),
        ordering: "unset",
        inputs: [],
        outputs: [
          { address: uri.address, amountBtc: (amountSats / 100_000_000).toFixed(8) },
        ],
      }),
      "payment-uri",
      uri.label,
    );
    logEvent(
      `minted ${fragment.key} from a bitcoin: URI — a txout intent paying ` +
        `${uri.address} ${formatSatAmount(amountSats)}${uri.label ? ` (${uri.label})` : ""}`,
    );
    showStatus("", false);
    return true;
  } catch (error) {
    reportError("payment URI", error);
    return true; // it WAS a payment URI; the error is already reported
  }
}

let payAmountResolve: ((sats: number | null) => void) | null = null;

function promptPaymentAmount(address: string): Promise<number | null> {
  const dialog = el<HTMLDialogElement>("payAmountDialog");
  el<HTMLElement>("payAmountDialogWhy").textContent =
    `The payment request for ${address} names no amount, and a PSBTv2 output ` +
    `cannot be serialized without one (PSBT_OUT_AMOUNT is required).`;
  const input = el<HTMLInputElement>("payAmountInput");
  input.value = "";
  return new Promise((resolve) => {
    // A re-prompt cancels any dangling prompt AND closes its dialog —
    // showModal() on an already-open dialog throws inside this executor.
    settlePayAmount(null);
    payAmountResolve = resolve;
    dialog.showModal();
    input.focus();
  });
}

function settlePayAmount(sats: number | null): void {
  const dialog = el<HTMLDialogElement>("payAmountDialog");
  if (dialog.open) dialog.close();
  payAmountResolve?.(sats);
  payAmountResolve = null;
}

function setAddDrawer(open: boolean, focusPeer = false): void {
  if (open) setDrawer("addDrawer");
  else closeDrawer("addDrawer");
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
  if (pasted.kind === "payment-uri") {
    if (await addPaymentUri(pasted.payload)) {
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
  if (pasted.kind !== "descriptor" && pasted.kind !== "transaction-hex") {
    return;
  }
  try {
    const classified = await backend.classifyPaste(pasted.payload, displayNetwork());
    switch (node.kind) {
      case "descriptor":
        objects = enrichDescriptor(objects, node.key, classified);
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
    const joined = await addResponse(
      await backend.joinPsbts(selected.map((f) => f.psbt)),
      "join",
      `⊔ join of ${selected.map((f) => f.key).join(", ")}`,
    );
    reportJoinOutcome(joined, selected);
    settleMint(
      "opsKeepOriginal",
      selected.map((f) => f.key),
      [joined.key],
    );
    render();
  } catch (error) {
    reportError("join", error);
  }
}

async function concatenateSelected(): Promise<void> {
  const selected = requireEnabled("concatenate");
  if (!selected) return;
  try {
    const minted = await addResponse(
      await backend.concatenatePsbts(selected.map((f) => f.psbt)),
      "concatenate",
      `concatenation of ${selected.map((f) => f.key).join(", ")}`,
    );
    settleMint(
      "opsKeepOriginal",
      selected.map((f) => f.key),
      [minted.key],
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
    // A re-prompt cancels any dangling prompt AND closes its dialog —
    // showModal() on an already-open dialog throws inside this executor.
    settleSortSeed(null);
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
    const sorted = await addResponse(
      await backend.sortPsbt(selected[0].psbt, seed),
      "sort",
      `sort of ${selected[0].key}`,
    );
    settleMint("opsKeepOriginal", [selected[0].key], [sorted.key]);
    showStatus("", false);
  } catch (error) {
    reportError("sort", error);
  }
}

async function makeUnorderedSelected(): Promise<void> {
  const selected = requireEnabled("make-unordered");
  if (!selected) return;
  try {
    const minted = await addResponse(
      await backend.makeUnordered(selected[0].psbt),
      "make-unordered",
      `make-unordered of ${selected[0].key}`,
    );
    settleMint("opsKeepOriginal", [selected[0].key], [minted.key]);
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
    const atomKeys: string[] = [];
    let index = 0;
    for (const piece of response.fragments) {
      index += 1;
      const atom = await addResponse(
        piece,
        "atomize",
        `atom ${index}/${response.fragments.length} of ${target.key}`,
      );
      atomKeys.push(atom.key);
    }
    logEvent(`atomize produced ${response.fragments.length} fragments`);
    // The fix intermediate (selected → modifiable target) retires with the
    // original: both values live on in the atoms.
    settleMint("opsKeepOriginal", [selected[0].key, target.key], atomKeys);
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
  el<HTMLInputElement>("assignIdsKeep").checked = false;
}

async function runAssignIds(): Promise<void> {
  const fragment = assignIdsTarget ? fragmentByKey(assignIdsTarget) : null;
  if (!fragment) {
    assignIdsTarget = null;
    closeDrawer("assignIdsDrawer");
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
    settleMint("assignIdsKeep", [fragment.key], [added.key]);
    assignIdsTarget = null;
    closeDrawer("assignIdsDrawer");
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
  // Zero-selection syncs that are legitimate: local and watched-dir run
  // from server-side sources (the register may already hold the frontier),
  // and iroh with ticket-out CREATES an empty shared document (peers
  // publish into it later). Every other shape syncs the selection.
  const serverSideSources =
    syncTransportValue() === "local" || syncTransportValue() === "watched-dir";
  const createsEmptyDoc =
    syncTransportValue() === "iroh" && el<HTMLInputElement>("syncIrohTicketOut").checked;
  if (!state.enabled && !serverSideSources && !createsEmptyDoc) {
    showStatus(`sync: ${state.reason ?? "not available"}`, true);
    return;
  }
  const selected = selectedFragments(session);
  await withBusy(
    selected.map((fragment) => `fragment:${fragment.key}`),
    () =>
      runSyncRequest(
        selected.map((fragment) => fragment.psbt),
        `sync of ${selected.length} selected fragment(s)`,
      ),
  );
}

// Peer→session wiring: sync the session's register value over the peer's
// transport. Sessions have no transport of their own, so with no peer given
// the fallback is the session's first member peer with a usable transport,
// and "local" (the disk) only as the last resort. The transport parameters
// ride the sync form so the manual-signaling transports stay configurable;
// iroh peers bring their ticket along — whichever peer supplied the
// transport, explicit or member fallback, supplies its ticket too.
function usablePeerForSync(peer: PeerObject | null): PeerObject | null {
  return peer && peerUsableForSync(peer) ? peer : null;
}

async function syncSessionOverPeer(sessionKey: string, peerKey: string | null): Promise<void> {
  const sessionObject = sessionByKey(objects, sessionKey);
  if (!sessionObject) return;
  const peer = peerKey ? peerByKey(objects, peerKey) : null;
  const memberPeer = sessionObject.peerKeys
    .map((key) => usablePeerForSync(peerByKey(objects, key)))
    .find((candidate): candidate is PeerObject => candidate !== null);
  const carrier = usablePeerForSync(peer) ?? memberPeer ?? null;
  // A disk-location peer is a storage place, not an endpoint: syncing over
  // it means driving the watched-dir register rooted at its path. Bare
  // "local" survives only as the no-carrier fallback (form-configured
  // server-side sources/state).
  const transport = carrier?.transport === "local" ? "watched-dir" : (carrier?.transport ?? "local");
  el<HTMLSelectElement>("syncTransport").value = transport;
  renderSyncFields();
  if (carrier && transport === "watched-dir" && carrier.identity) {
    el<HTMLTextAreaElement>("syncSources").value = carrier.identity;
  }
  if (carrier && carrier.transport === "iroh" && carrier.identity) {
    el<HTMLTextAreaElement>("syncIrohTicket").value = carrier.identity;
    el<HTMLInputElement>("syncIrohTicketOut").checked = false;
  }
  const content = sessionObject.contentKey ? fragmentByKey(sessionObject.contentKey) : null;
  // local reads the form's server-side paths; watched-dir can collect the
  // frontier from the register itself — both tolerate an empty session.
  if (!content && transport !== "local" && transport !== "watched-dir") {
    showStatus(`${sessionObject.name}: the register is empty — write a fragment in before syncing`, true);
    return;
  }
  // The state chip and results land in the sync drawer — open it, or the
  // container's Sync now reads as a dead button.
  revealPanel("syncResults");
  await withBusy(
    [`session:${sessionKey}`, ...(carrier ? [`peer:${carrier.key}`] : [])],
    () =>
      runSyncRequest(
        content ? [content.psbt] : [],
        `session ${sessionObject.name} (register ${content?.key ?? "empty"})`,
      ),
  );
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
    await withBusy([`fragment:${target.key}`], async () =>
      addResponse(
        await backend.pay(target.psbt, built.value.payment, built.value.options),
        "pay",
        `payment record attached to ${target.key}`,
      ),
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
    await withBusy([`fragment:${target.key}`], async () =>
      addResponse(
        await backend.confirm(target.psbt, built.value.confirmation, built.value.options),
        "confirm",
        `confirmation attached to ${target.key}`,
      ),
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

// --- persistent wire overlay --------------------------------------------------
// The standing edges of the wire metaphor: Mine (the local network peer)
// sees every session register, and each remote peer is wired to the
// sessions whose peer set contains it. Edges draw entirely from the SAME
// laneLayout rects that placed the cards — no DOM measurement — so the SVG
// (viewBox = world coordinates) scrolls and resizes with the world for
// free. The overlay is pointer-transparent; the transient drag line is a
// separate client-coordinate mechanism.

// The layout rect a wire endpoint occupies on the canvas, or null when the
// endpoint doesn't render there (utxos live in the objects panel).
function canvasRectFor(ref: NodeRef): LayoutRect | null {
  if (!canvasLayout) return null;
  // A bridged peer group renders as ONE card keyed by its first member.
  const key =
    ref.kind === "peer"
      ? `peer:${bridgeGroupContaining(objects, ref.key)[0] ?? ref.key}`
      : ref.kind === "session" || ref.kind === "fragment"
        ? `${ref.kind}:${ref.key}`
        : null;
  return key === null ? null : (canvasLayout.positions.get(key) ?? null);
}

function drawWireOverlay(): void {
  const overlay = document.getElementById("wireOverlay");
  if (!overlay) return;
  const pillLayer = el<HTMLElement>("pillLayer");
  overlay.textContent = "";
  pillLayer.textContent = "";
  // Focus mode swaps the canvas out; the standing edges stand down with it.
  if (focus.mode === "session" || !canvasLayout) return;
  overlay.setAttribute(
    "viewBox",
    `0 0 ${canvasLayout.world.width} ${canvasLayout.world.height}`,
  );
  const addEdge = (from: LayoutRect | null, to: LayoutRect | null, cls: string): void => {
    if (!from || !to) return;
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", curveBetween(from, to));
    path.setAttribute("class", cls);
    overlay.append(path);
  };
  const mineFrame = canvasLayout.mineFrame;
  const seen = new Set<string>();
  for (const sessionObject of objects.sessions) {
    const container = canvasRectFor({ kind: "session", key: sessionObject.key });
    // Mine sits BELOW the sessions; its edges rise from the frame's top.
    addEdge(mineFrame, container, "session-edge-mine");
    for (const peerKey of sessionObject.peerKeys) {
      const groupKey = bridgeGroupContaining(objects, peerKey)[0] ?? peerKey;
      const edgeKey = `${groupKey}→${sessionObject.key}`;
      if (seen.has(edgeKey)) continue;
      seen.add(edgeKey);
      addEdge(canvasRectFor({ kind: "peer", key: groupKey }), container, "session-edge-auth");
    }
  }
  // Pending wires are visible promises: an animated edge between the two
  // cards with a pill at its midpoint. The pill is the wire's own commit —
  // Join collapses exactly that edge — unless the probe already computed
  // the join to fail, in which case the pill explains the conflict
  // instead. Wires whose endpoints live off-canvas (utxos in the objects
  // panel) stay queue-panel-only.
  for (const entry of livePendingWires()) {
    const from = canvasRectFor(entry.source);
    const to = canvasRectFor(entry.target);
    if (!from || !to) continue;
    const key = wireKey(entry.source, entry.target);
    const probe = wireProbes.get(key)?.probe;
    const conflicted = probe?.state === "conflict";
    const joining = inflight.has(`edge:${key}`);
    addEdge(
      from,
      to,
      conflicted
        ? "session-edge-pending session-edge-conflict"
        : joining
          ? "session-edge-pending session-edge-busy"
          : "session-edge-pending",
    );
    const pill = conflicted
      ? button("⚠ why?", "This join was computed and it conflicts — see why", () => {
          const detail = wireConflict(entry);
          openConflictModal(`${wireLabel(entry)} — conflict`, [
            { label: wireLabel(entry), detail: detail ?? "conflict details unavailable" },
          ]);
        })
      : button("Join", `Apply this wire alone: ${wireLabel(entry)}`, () => void joinPendingWire(key));
    if (probe?.state === "pending" || joining) {
      pill.disabled = true;
      pill.textContent = "⋯";
      pill.title = joining ? "joining…" : "computing the join…";
    }
    pill.className = conflicted ? "session-wire-pill session-wire-pill-conflict" : "session-wire-pill";
    const mid = curveMidpoint(from, to);
    pill.style.left = `${mid.x}px`;
    pill.style.top = `${mid.y}px`;
    pillLayer.append(pill);
  }
}

function render(): void {
  renderFocus();
  // Focus hides the canvas (display:none measures as zero), so exactly one
  // of the two fragment surfaces renders per pass.
  if (focus.mode === "session" && focus.sessionKey !== null) {
    renderFocusFragments();
  } else {
    renderCanvas();
  }
  renderObjects();
  // After the card passes: the idle wire hint asks the DOM whether any
  // wireable card exists.
  renderWireStatus();
  renderOps();
  el<HTMLElement>("createWireTarget").hidden = !(wire.source && wire.source.kind === "utxo");
  // A live drag survives the render: the gesture state lives off-DOM and
  // the document-level handlers finish it, so a probe or sync settling
  // mid-gesture only needs the target paint re-applied to the fresh cards.
  if (wireDrag?.active) paintWireTargets();
  // The standing edges reflect the freshly-rendered card geometry.
  drawWireOverlay();
}

// --- wiring (DOM event hookup) -----------------------------------------------------

function wireDom(): void {
  for (const [id] of ACTION_BUTTONS) {
    BASE_TITLE.set(id, el<HTMLButtonElement>(id).title);
  }

  // Wire-drag move/finish live on the document, NOT on the cards: cards are
  // rebuilt every render, and a gesture must outlive the card it started on
  // (armWireDrag only arms the pointerdown).
  document.addEventListener("pointermove", wireDragMove);
  document.addEventListener("pointerup", (event) => finishWireDrag(event, true));
  document.addEventListener("pointercancel", (event) => finishWireDrag(event, false));

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
  // An empty or non-hex confirm is NOT a cancel — the dialog stays open and
  // the field says why, so the sort is never silently abandoned.
  const sortSeedDialog = el<HTMLDialogElement>("sortSeedDialog");
  const sortSeedInput = el<HTMLInputElement>("sortSeedInput");
  el<HTMLButtonElement>("sortSeedConfirm").addEventListener("click", () => {
    const seed = sortSeedInput.value.trim();
    if (!/^([0-9a-fA-F]{2})+$/.test(seed)) {
      sortSeedInput.setCustomValidity("enter a hex seed (whole bytes), or Generate one");
      sortSeedInput.reportValidity();
      return;
    }
    settleSortSeed(seed);
  });
  sortSeedInput.addEventListener("input", () => sortSeedInput.setCustomValidity(""));
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
  // The payment-amount prompt settles the same way: confirm resolves sats,
  // cancel/backdrop/Esc resolve null (the txout intent is abandoned). A
  // non-positive or non-numeric confirm keeps the dialog open and says why.
  const payAmountDialog = el<HTMLDialogElement>("payAmountDialog");
  const payAmountInput = el<HTMLInputElement>("payAmountInput");
  el<HTMLButtonElement>("payAmountConfirm").addEventListener("click", () => {
    const sats = Math.round(Number(payAmountInput.value.trim()) * 100_000_000);
    if (!Number.isFinite(sats) || sats <= 0) {
      payAmountInput.setCustomValidity("enter a positive BTC amount (e.g. 0.0005)");
      payAmountInput.reportValidity();
      return;
    }
    settlePayAmount(sats);
  });
  payAmountInput.addEventListener("input", () => payAmountInput.setCustomValidity(""));
  el<HTMLButtonElement>("payAmountCancel").addEventListener("click", () => settlePayAmount(null));
  payAmountDialog.addEventListener("click", (event) => {
    if (event.target === payAmountDialog) settlePayAmount(null);
  });
  payAmountDialog.addEventListener("cancel", () => settlePayAmount(null));
  // The drawer bar: each button toggles its drawer (one at a time). A bar
  // button that is ALSO a declared wire node (Create fragment ⋄ utxo) lands
  // an armed wire gesture instead of toggling.
  for (const toggle of Array.from(
    document.querySelectorAll<HTMLButtonElement>("[data-drawer]"),
  )) {
    toggle.addEventListener("click", () => {
      const id = toggle.dataset.drawer as DrawerId;
      if (toggle.dataset.wireKind === "create" && wire.source?.kind === "utxo") {
        wireTo({ kind: "create", key: "create" });
        return;
      }
      if (id === "addDrawer") {
        setAddDrawer(openDrawerId() !== "addDrawer");
        return;
      }
      // The assign-ids panel is parameterized by the selected fragment;
      // opening it from the bar takes the same gate as the ops-bar button,
      // so it never opens blank (or armed at a stale fragment). A wrong
      // selection reports its reason instead of showing a dead panel.
      if (id === "assignIdsDrawer" && openDrawerId() !== id) {
        openAssignIds();
        return;
      }
      setDrawer(openDrawerId() === id ? null : id);
    });
  }
  // Backdrop click and the shared [×] close whatever drawer is open.
  for (const drawerId of DRAWER_IDS) {
    el<HTMLElement>(drawerId).addEventListener("click", (event) => {
      if (event.target === el<HTMLElement>(drawerId)) setDrawer(null);
    });
  }
  for (const close of Array.from(
    document.querySelectorAll<HTMLElement>("[data-drawer-close]"),
  )) {
    close.addEventListener("click", () => setDrawer(null));
  }
  // One Esc, one surface: the topmost open layer wins. An open modal
  // dialog owns Esc natively (its `cancel` event), so it returns early;
  // then a live drag-to-wire, then the samples popover, then the drawer.
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    if (document.querySelector("dialog[open]")) return;
    if (wireDrag) {
      cancelWireDrag();
    } else if (!el<HTMLElement>("samplesPopover").hidden) {
      setSamplesPopover(false);
    } else if (openDrawerId()) {
      setDrawer(null);
    }
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
    closeDrawer("assignIdsDrawer");
  });

  el<HTMLSelectElement>("displayNetwork").addEventListener("change", render);
  // Edges live in world coordinates, so scrolling needs nothing; a resize
  // changes the layout's minWidth (the viewport), so the canvas re-lays out.
  // Latched to one render per animation frame — a live window drag fires
  // resize far faster than the full measure-and-place pass can run.
  let resizeRenderQueued = false;
  window.addEventListener("resize", () => {
    if (resizeRenderQueued) return;
    resizeRenderQueued = true;
    requestAnimationFrame(() => {
      resizeRenderQueued = false;
      render();
    });
  });
  // Descriptor-hover cross-referencing: hovering a descriptor card dims
  // every colorized node of a DIFFERENT identity, so everything the
  // descriptor touches (provenance groups, coins, peers) pops. Delegated,
  // so it survives every render; leaving the card lifts the dim.
  const applyIdentityDim = (key: string | null): void => {
    document.querySelectorAll<HTMLElement>("[data-identity-key]").forEach((node) => {
      node.classList.toggle(
        "session-identity-dim",
        key !== null && node.dataset.identityKey !== key,
      );
    });
  };
  const identityKeyAt = (target: EventTarget | null): string | null =>
    target instanceof Element
      ? (target.closest<HTMLElement>(".session-descriptor-card[data-identity-key]")?.dataset
          .identityKey ?? null)
      : null;
  document.addEventListener("pointerover", (event) => {
    applyIdentityDim(identityKeyAt(event.target));
  });
  // pointerover cannot lift the dim when the pointer exits the WINDOW
  // (nothing new is entered — relatedTarget null marks that), and a touch
  // tap elsewhere fires no pointerover at all — both clear explicitly.
  document.addEventListener("pointerout", (event) => {
    if (event.relatedTarget === null) applyIdentityDim(null);
  });
  document.addEventListener("pointerdown", (event) => {
    if (identityKeyAt(event.target) === null) applyIdentityDim(null);
  });
  el<HTMLButtonElement>("wireJoinAll").addEventListener("click", () => void joinAllWires());
  el<HTMLButtonElement>("wireClearAll").addEventListener("click", clearPendingWires);
  el<HTMLButtonElement>("focusBack").addEventListener("click", () => {
    focus = overviewFocus();
    render();
  });

  el<HTMLFormElement>("newSessionForm").addEventListener("submit", (event) => {
    event.preventDefault();
    const minted = mintSession(objects, inputValue("newSessionName"));
    objects = minted.state;
    el<HTMLInputElement>("newSessionName").value = "";
    logEvent(`created ${minted.session.key} (${minted.session.name}) — wire peers in to make it reachable`);
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
    closeDrawer("editorDrawer");
  });
  el<HTMLButtonElement>("editorValidate").addEventListener("click", () => {
    if (!editor) return;
    renderEditor(validateEditor(editor));
  });
  el<HTMLButtonElement>("editorSave").addEventListener("click", () => void saveEditor());

  el<HTMLButtonElement>("outputClose").addEventListener("click", () => {
    closeDrawer("exportDrawer");
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
