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

import type {
  ConfirmationRecord,
  ConfirmOptions,
  CreateInput,
  CreateOutput,
  CreatePsbtRequest,
  InspectResponse,
  OrderingMode,
  PaymentRecord,
  PaymentsResponse,
  PayOptions,
  SyncRequest,
} from "../shared-frontend/core/backend.js";
import {
  compactBase64,
  looksLikeBase64Psbt,
  normalizeSessionOrdering,
  type SessionOrderingMode,
} from "../model.js";

// ---------------------------------------------------------------------------
// Fragment set: the REAL loaded PSBTs (pasted, uploaded, imported, created,
// or produced by an operation). Monotone bookkeeping: adding an
// already-present PSBT re-selects it instead of duplicating, removals are
// explicit user actions, and operation results are ADDED as new fragments —
// the inputs stay untouched.
// ---------------------------------------------------------------------------

export type FragmentOrigin =
  | "paste"
  | "upload"
  | "import-bip174"
  | "create"
  | "join"
  | "sort"
  | "make-unordered"
  | "atomize"
  | "concatenate"
  | "sync";

export interface SessionFragment {
  key: string;
  // BIP 370 (PSBT v2) base64, whitespace-compacted.
  psbt: string;
  // `ptj inspect` JSON for this fragment (null until decoded).
  inspect: InspectResponse | null;
  origin: FragmentOrigin;
  selected: boolean;
}

export interface SessionState {
  fragments: SessionFragment[];
  counter: number;
}

export interface AddFragmentResult {
  state: SessionState;
  fragment: SessionFragment;
  duplicate: boolean;
}

export function emptySession(): SessionState {
  return { fragments: [], counter: 0 };
}

export function addFragment(
  state: SessionState,
  psbt: string,
  inspect: InspectResponse | null,
  origin: FragmentOrigin,
): AddFragmentResult {
  const compact = compactBase64(psbt);
  // Unordered PSBTs reserialize in a shuffled map order by design (psbt.md),
  // so byte equality is only a fast path. The canonical identity is the
  // unordered unique id: an id match is the same PSBT, possibly carrying more
  // data (the id commits to the input/output sets, not to their fields), so
  // the surviving card absorbs the incoming value instead of duplicating.
  const incomingId = fragmentSummary(inspect).uniqueIdHex;
  const existing = state.fragments.find(
    (fragment) =>
      fragment.psbt === compact ||
      (incomingId !== null && fragmentSummary(fragment.inspect).uniqueIdHex === incomingId),
  );
  if (existing) {
    const absorbed =
      existing.psbt === compact ? existing : { ...existing, psbt: compact, inspect };
    const fragments = state.fragments.map((fragment) =>
      fragment.key === existing.key ? { ...absorbed, selected: true } : fragment,
    );
    return {
      state: { fragments, counter: state.counter },
      fragment: fragments.find((fragment) => fragment.key === existing.key)!,
      duplicate: true,
    };
  }
  const counter = state.counter + 1;
  const fragment: SessionFragment = {
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

export function removeFragment(state: SessionState, key: string): SessionState {
  return {
    fragments: state.fragments.filter((fragment) => fragment.key !== key),
    counter: state.counter,
  };
}

export function setSelected(state: SessionState, key: string, selected: boolean): SessionState {
  return {
    fragments: state.fragments.map((fragment) =>
      fragment.key === key ? { ...fragment, selected } : fragment,
    ),
    counter: state.counter,
  };
}

export function selectedFragments(state: SessionState): SessionFragment[] {
  return state.fragments.filter((fragment) => fragment.selected);
}

// ---------------------------------------------------------------------------
// Inspect views: defensive projections of `ptj inspect` JSON (an open object
// on the seam) into what the fragment list and negotiation panel display.
// ---------------------------------------------------------------------------

function asObject(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export interface FragmentSummary {
  format: string | null;
  ordering: string | null;
  inputCount: number | null;
  outputCount: number | null;
  sortMode: string | null;
  seedHex: string | null;
  // The psbt.md unordered PSBT unique id (inspect `unordered_unique_id_hex`)
  // — the identity `ptj confirm` records.
  uniqueIdHex: string | null;
  knownInputSats: number | null;
  outputSats: number | null;
  feeSats: number | null;
}

export function fragmentSummary(inspect: InspectResponse | null): FragmentSummary {
  const root = asObject(inspect);
  const sort = asObject(root?.sort);
  const totals = asObject(root?.totals);
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
  };
}

export function fragmentLabel(fragment: SessionFragment): string {
  const summary = fragmentSummary(fragment.inspect);
  const shape =
    summary.inputCount === null || summary.outputCount === null
      ? "not decoded"
      : `${summary.inputCount} in / ${summary.outputCount} out`;
  const ordering = summary.ordering ?? "unknown";
  return `${fragment.key} · ${ordering} · ${shape} · ${fragment.origin}`;
}

// The raw-lists ceiling for the negotiation panel: counts plus the opaque
// record hex. Readiness/phase semantics are under active rework
// (wip/task-local-readiness) and deliberately NOT modeled here.
export interface NegotiationView {
  paymentCount: number;
  confirmationCount: number;
  payments: string[];
  confirmations: string[];
}

export function negotiationView(response: PaymentsResponse): NegotiationView {
  const payments = response.payments ?? [];
  const confirmations = response.confirmations ?? [];
  return {
    paymentCount: payments.length,
    confirmationCount: confirmations.length,
    payments: [...payments],
    confirmations: [...confirmations],
  };
}

// ---------------------------------------------------------------------------
// Request builders: validated form -> Backend seam DTO. Every builder returns
// FormResult instead of throwing, so the shell renders errors inline.
// Address/amount semantics stay SERVER-side (the create/pay routes validate
// against the selected network); the presenter only checks what must be
// well-formed before a request makes sense (hex fields, integers, presence).
// ---------------------------------------------------------------------------

export type FormResult<T> = { ok: true; value: T } | { ok: false; error: string };

function fail<T>(error: string): FormResult<T> {
  return { ok: false, error };
}

export function isHexBytes(value: string, exactBytes?: number): boolean {
  const trimmed = value.trim().toLowerCase();
  if (!/^(?:[0-9a-f]{2})+$/.test(trimmed)) return false;
  return exactBytes === undefined || trimmed.length === exactBytes * 2;
}

export interface CreateFormInput {
  txid: string;
  vout: string;
}

export interface CreateFormOutput {
  address: string;
  amountBtc: string;
}

export interface CreateForm {
  network: string;
  ordering: SessionOrderingMode;
  seed: string;
  inputs: CreateFormInput[];
  outputs: CreateFormOutput[];
}

const ORDERING_MODE: Record<SessionOrderingMode, OrderingMode> = {
  det: "deterministic",
  explicit: "explicit",
  unset: "unset",
};

export function buildCreateRequest(form: CreateForm): FormResult<CreatePsbtRequest> {
  const ordering = normalizeSessionOrdering(form.ordering, form.seed);
  if (!ordering.valid) {
    return fail(ordering.error ?? "invalid ordering");
  }
  const inputs: CreateInput[] = [];
  for (const [index, row] of form.inputs.entries()) {
    const txid = row.txid.trim().toLowerCase();
    const voutText = row.vout.trim();
    if (!txid && !voutText) continue; // blank row
    if (!isHexBytes(txid, 32)) {
      return fail(`input ${index + 1}: txid must be 32 hex bytes`);
    }
    if (!/^\d+$/.test(voutText)) {
      return fail(`input ${index + 1}: vout must be a non-negative integer`);
    }
    inputs.push({ txid, vout: Number(voutText) });
  }
  const outputs: CreateOutput[] = [];
  for (const [index, row] of form.outputs.entries()) {
    const address = row.address.trim();
    const amountBtc = row.amountBtc.trim();
    if (!address && !amountBtc) continue; // blank row
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

export type SyncTransport = "local" | "iroh" | "str0m" | "webrtc-rs";

export interface SyncForm {
  transport: SyncTransport;
  // Newline-separated server-side paths (files or directories of .psbt files).
  sources: string;
  state: string;
  irohTicket: string;
  irohTicketOut: boolean;
  irohWaitMs: string;
  webrtcRole: "" | "offer" | "answer";
  signalOut: string;
  signalIn: string;
  webrtcBind: string;
  // Newline-separated STUN/TURN URIs.
  iceServers: string;
  signalTimeoutMs: string;
}

export function parseLines(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

// Plain optional shape (not FormResult): both compile configs must accept
// forwarding the failure, and the lax emit config does not narrow generic
// discriminated unions across returns.
function optionalInteger(text: string, label: string): { value?: number; error?: string } {
  const trimmed = text.trim();
  if (!trimmed) return {};
  if (!/^\d+$/.test(trimmed)) return { error: `${label} must be a non-negative integer` };
  return { value: Number(trimmed) };
}

export function buildSyncRequest(form: SyncForm, psbts: string[]): FormResult<SyncRequest> {
  const request: SyncRequest = { transport: form.transport };
  if (psbts.length) request.psbts = [...psbts];

  const waitMs = optionalInteger(form.irohWaitMs, "wait ms");
  if (waitMs.error) return fail(waitMs.error);
  if (waitMs.value !== undefined) request.irohWaitMs = waitMs.value;

  if (form.transport === "local") {
    const sources = parseLines(form.sources);
    const state = form.state.trim();
    if (sources.length) request.sources = sources;
    if (state) request.state = state;
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
    if (ticket) request.irohTicket = ticket;
    if (form.irohTicketOut) request.irohTicketOut = true;
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
  if (bind) request.webrtcBind = bind;
  const iceServers = parseLines(form.iceServers);
  if (iceServers.length) request.iceServers = iceServers;
  const timeoutMs = optionalInteger(form.signalTimeoutMs, "signal timeout ms");
  if (timeoutMs.error) return fail(timeoutMs.error);
  if (timeoutMs.value !== undefined) request.signalTimeoutMs = timeoutMs.value;
  return { ok: true, value: request };
}

export interface PayForm {
  mode: "address" | "hex";
  address: string;
  amountBtc: string;
  network: string;
  label: string;
  // OPAQUE optional 32-byte hex payer id (semantics live in the negotiation
  // spec; the UI never interprets it).
  payerHex: string;
  paymentHex: string;
  secretHex: string;
  dummy: string;
}

export interface PayArgs {
  payment: PaymentRecord;
  options?: PayOptions;
}

export function buildPayArgs(form: PayForm): FormResult<PayArgs> {
  const options: PayOptions = {};
  const secret = form.secretHex.trim();
  if (secret) {
    if (!isHexBytes(secret)) return fail("secret must be hex bytes");
    options.secretHex = secret.toLowerCase();
  }
  const dummy = optionalInteger(form.dummy, "dummy count");
  if (dummy.error) return fail(dummy.error);
  if (dummy.value) {
    if (!options.secretHex) {
      return fail("dummy padding requires a secret (plaintext dummies are distinguishable)");
    }
    options.dummy = dummy.value;
  }

  let payment: PaymentRecord;
  if (form.mode === "hex") {
    const record = form.paymentHex.trim();
    if (!isHexBytes(record)) return fail("payment record must be hex bytes");
    payment = record.toLowerCase();
  } else {
    const address = form.address.trim();
    const amountBtc = form.amountBtc.trim();
    if (!address || !amountBtc) return fail("address and amount are both required");
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

export interface ConfirmForm {
  mode: "derive" | "hex";
  confirmationHex: string;
  // OPAQUE optional 32-byte hex peer id (the CLI --peer-id).
  peerIdHex: string;
  secretHex: string;
}

export interface ConfirmArgs {
  confirmation: ConfirmationRecord;
  options?: ConfirmOptions;
}

export function buildConfirmArgs(form: ConfirmForm): FormResult<ConfirmArgs> {
  let options: ConfirmOptions | undefined;
  const secret = form.secretHex.trim();
  if (secret) {
    if (!isHexBytes(secret)) return fail("secret must be hex bytes");
    options = { secretHex: secret.toLowerCase() };
  }
  if (form.mode === "hex") {
    const record = form.confirmationHex.trim();
    if (!isHexBytes(record)) return fail("confirmation record must be hex bytes");
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
export function pastedPsbt(text: string): string | null {
  const compact = compactBase64(text);
  return looksLikeBase64Psbt(compact) ? compact : null;
}

const BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// Pure base64 for uploaded binary PSBT files (browser `atob`/`btoa` are not
// byte-safe and node lacks them in older LTS; this is dependency-free and
// node --test coverable).
export function bytesToBase64(bytes: Uint8Array): string {
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
