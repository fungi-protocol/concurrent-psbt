// contrib/demo-gui/src/shared-frontend/core/types.ts
//
// Shared frontend core — DTOs and shared types.
//
// These are lifted VERBATIM from contrib/demo-gui/src/backend.ts so that the
// HttpBackend (webgui), WasmBackend (PWA), and TauriBackend (future) all speak
// the identical request/response shapes. The frontend logic (app/model) imports
// ONLY from this file and ./backend.ts — never from a concrete adapter.
//
// Provenance: request/response DTOs match the ptj webgui *_response_result JSON
// contract in crates/ptj/src/webgui.rs (POST /api/{inspect,create,join,sort,
// make-unordered,atomize,concatenate,export-bip174,import-bip174,assign-ids,
// edit,classify,sync}) and the concurrent-psbt command set in
// crates/ptj/src/commands/*.rs.

export type OrderingMode = "unset" | "deterministic" | "explicit";

export interface InspectResponse {
  [key: string]: unknown;
}

export interface PsbtResponse {
  psbt: string;
  inspect?: InspectResponse;
}

export interface AtomizeResponse {
  fragments: PsbtResponse[];
}

export interface ExportBip174Response {
  format: "bip174";
  psbt: string;
}

export interface CreateInput {
  txid: string;
  vout: number;
}

export interface CreateOutput {
  address: string;
  amountBtc: string;
}

export interface CreatePsbtRequest {
  network: string;
  ordering: OrderingMode;
  seedHex?: string;
  // Explicit override for ordering seeds below the spec minimum of 128 bits
  // (16 bytes); maps to the wire field `allow_short_seed`. Never silent: the
  // backend rejects short seeds unless this is set.
  allowShortSeed?: boolean;
  inputs: CreateInput[];
  outputs: CreateOutput[];
}

// One manual unique-id directive for assignIds: `out` sets
// PSBT_OUT_UNIQUE_ID, `in` sets the optional PSBT_IN_UNIQUE_ID outpoint
// suffix. `id` bytes accept hex/base58/bech32, detected by character set.
export interface IdAssignment {
  target: "in" | "out";
  index: number;
  id: string;
}

export interface AssignIdsOptions {
  // Manual directives; without any, the backend auto-assigns fresh random
  // 16-byte ids to every output missing one (idempotent).
  ids?: IdAssignment[];
  // Combine manual directives with auto-fill of the remaining outputs.
  auto?: boolean;
  // Replace an existing unique id that differs from the requested one.
  overwrite?: boolean;
}

// One raw-keymap field edit for applyPsbtEdits (/api/edit). Edits address
// entries by the same handle `inspect` exposes (raw.*[].key_hex — the full
// raw key, compact-size keytype prefix included) and are GROW-ONLY: a save
// mints a NEW fragment, the submitted PSBT is never mutated.
export interface FieldEdit {
  // Map selector: `global`, `input:<i>`, or `output:<i>`.
  map: string;
  // Full raw key bytes (hex/base58/bech32, detected by character set).
  key: string;
  // Bytes to set, or null to DELETE the entry.
  value: string | null;
}

// One save-time validation violation. Every gate is strict by default and
// individually waived by asserting its named `override_param` on the next
// save; a violation MAY offer a server-side fix (`fix_id`) whose caveat
// (`warning_text`) the shell must keep visible.
export interface EditViolation {
  id: string;
  message: string;
  override_param: string;
  fix_id?: string;
  fix_label?: string;
  warning_text?: string;
}

// Echo of a fix the server ran before validating (`apply_fixes` request
// array), with its informed warning repeated verbatim.
export interface AppliedFix {
  fix_id: string;
  warning_text?: string;
}

export interface ApplyEditsOptions {
  // Server-side fixes to run before validation (violation fix_ids).
  applyFixes?: string[];
  // Violation override_params to assert true on this save.
  overrides?: string[];
}

// The /api/classify response: universal paste classification,
// `{kind, ...details}` from ptj's classify command. Kinds today:
//   "descriptor"  — normalized public form (private material never echoes),
//                   descriptor_type, has_private_keys, is_ranged,
//                   is_multipath, paths?, derived[] {index,
//                   script_pubkey_hex, address?}
//   "transaction" — txid, input_count, output_count, fully_signed,
//                   outputs[] {outpoint, vout, amount_sats,
//                   script_pubkey_hex, address?}
//   "payment"     — variant (fixed_amount|configurable_amount), amount
//                   bounds, methods[] {type, ...}, description?, label?,
//                   message?
//   "peer_id"     — format ("npub"), id_hex
// The details stay loosely typed (the session presenter reads them
// defensively, like inspect JSON); PSBT pastes are REDIRECTED by the route
// (400 pointing at the PSBT flows) rather than half-classified.
export interface ClassifyResponse {
  kind: string;
  [key: string]: unknown;
}

// The /api/edit response. Success (HTTP 200) carries the NEW fragment
// (`psbt` + `inspect`) with `violations: []`; a save-time validation failure
// (HTTP 400) carries `error` + the remaining `violations` and NO psbt — it
// is a structured seam response (the violation -> fix -> revalidate loop),
// not a transport error, so adapters return it instead of throwing.
export interface ApplyEditsResponse {
  psbt?: string;
  inspect?: InspectResponse;
  violations: EditViolation[];
  // Violations acknowledged away by named overrides on this save.
  overridden?: EditViolation[];
  applied_fixes?: AppliedFix[];
  error?: string;
}

export interface SyncRequest {
  psbts?: string[];
  // Transport selection, mirroring the CLI's --transport ValueEnum ("local",
  // "iroh", "arti", "nym", "emissary", "mdk", "str0m", "webrtc-rs",
  // "payjoin-dir"). Absent, the webgui infers iroh from a pasted ticket and
  // local otherwise (back-compat); the PWA path substitutes a browser-viable
  // transport handle injected into the WasmBackend, still opaque to this seam.
  transport?: string;
  // Server-side local sources: PSBT files or directories of .psbt files (the
  // CLI's positional sources) plus the state PSBT file. Paths on the machine
  // running `ptj webgui` (an offline localhost GUI: the server IS the user's
  // machine); folded read-only with `psbts[]` in one lattice join.
  sources?: string[];
  state?: string;
  // Iroh document tickets: paste one in (`irohTicket`) to join an existing
  // document, or set `irohTicketOut` to have the server CREATE a document and
  // return its ticket in SyncResponse.irohTicketOut.
  irohTicket?: string;
  irohTicketOut?: boolean;
  irohWaitMs?: number;
  // Manual WebRTC signaling/session params for the str0m / webrtc-rs
  // transports, mirroring the CLI flags 1:1 (--webrtc-role, --signal-out,
  // --signal-in, --webrtc-bind, --ice-server, --signal-timeout-ms). The
  // signal files are server-side paths, exchanged out of band.
  webrtcRole?: "offer" | "answer";
  signalOut?: string;
  signalIn?: string;
  webrtcBind?: string;
  iceServers?: string[];
  signalTimeoutMs?: number;
}

export interface SyncResponse {
  psbt: string;
  inspect?: InspectResponse;
  payments: string[];
  confirmations: string[];
  // The ticket of the iroh document created for this request (set only when
  // the request asked for `irohTicketOut`); hand it to peers out of band.
  irohTicketOut?: string;
}

// Negotiation-band options/DTOs (Backend.pay / Backend.confirm / Backend.payments).
// The records are OPAQUE hex blobs — the frontend builds them, the backend only
// appends/decodes (mechanism-only, matching `ptj pay/confirm/payments`).

// Backend.pay's record argument: either a pre-built OPAQUE record (hex
// string), or the address variant, where the BACKEND builds the txout-shaped
// record with the same network validation as `ptj pay --to` — the frontend
// never parses addresses. `payerHex` is an OPAQUE optional 32-byte hex id
// copied into the record unchanged (payer semantics live in the negotiation
// spec, not in this seam).
export interface PayByAddress {
  address: string;
  amountBtc: string;
  // Same selector as CreatePsbtRequest.network; the backend defaults to
  // bitcoin, like `ptj pay`.
  network?: string;
  label?: string;
  payerHex?: string;
}
export type PaymentRecord = string | PayByAddress;

// Backend.confirm's record argument: either a pre-built OPAQUE record (hex
// string), or `derive: true`, where the BACKEND derives a confirmation of the
// submitted PSBT's current unordered unique id (the CLI's `ptj confirm`),
// with `peerIdHex` mirroring --peer-id as an OPAQUE optional 32-byte hex id.
export interface DeriveConfirmation {
  derive: true;
  peerIdHex?: string;
}
export type ConfirmationRecord = string | DeriveConfirmation;

export interface PayOptions {
  // Opt-in deterministic AEAD encryption of the record (ptj `--encrypt`).
  secretHex?: string;
  // Number of random dummy records appended alongside (requires secretHex).
  dummy?: number;
}

export interface ConfirmOptions {
  secretHex?: string;
}

export interface PaymentsOptions {
  secretHex?: string;
}

export interface PaymentsResponse {
  payments: string[];
  confirmations: string[];
}

// The ONE error type every backend throws. HttpBackend maps HTTP status +
// {error} body onto it; WasmBackend maps a caught JS/wasm error (status 0);
// TauriBackend maps a rejected invoke() (status 0). Frontend `instanceof`
// checks in app.ts keep working unchanged.
export class PtjBackendError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "PtjBackendError";
    this.status = status;
    Object.setPrototypeOf(this, PtjBackendError.prototype);
  }
}
