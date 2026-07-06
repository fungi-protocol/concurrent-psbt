export type PsbtFormat = "unordered" | "bip370" | "bip174";
export type PsbtModifiability = "both" | "inputs" | "outputs" | "none";
export type SessionOrderingMode = "det" | "explicit" | "unset";
export type PsbtRoleId =
  | "unordered-register"
  | "unordered-fragment"
  | "bip370-constructor"
  | "fixed-transaction"
  | "sorted-bip370";
export type PsbtUnaryAction = "make-unordered" | "sort" | "fix-sets" | "atomize" | "promote" | "abort-session";

export interface PayloadItem {
  id: string;
  label?: string;
  address?: string;
  valueSats?: number;
  explicitFeeSats?: number;
  [key: string]: unknown;
}

export interface DescriptorPayload {
  id: string;
  privacy: "public" | "private";
  descriptor: string;
  [key: string]: unknown;
}

export interface Payload {
  inputs: PayloadItem[];
  outputs: PayloadItem[];
  descriptors?: DescriptorPayload[];
  conflicts: string[];
}

export interface PsbtNode extends Payload {
  id: string;
  format: PsbtFormat;
  seed?: string;
  sortMode?: SessionOrderingMode;
  kind?: string;
  modifiable?: PsbtModifiability;
}

export interface PsbtRole {
  id: PsbtRoleId;
  label: string;
  spec: string;
  roles: string[];
}

export interface PsbtProtocolIdentity {
  label: "txid" | "unique id";
  value: string;
  source: string;
  stableBeforeSigning: boolean;
}

export interface PeerNode {
  id: string;
  local?: boolean;
  views?: Record<string, unknown>;
}

export interface GraphEdge {
  from: string;
  to: string;
  kind: string;
}

export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface AmountParts {
  prefix: string;
  muted: string;
  sats: string;
}

export interface ParsedBitcoinUri {
  uri: string;
  address: string;
  valueSats: number;
  descriptorId: string | null;
  label: string;
  message: string;
}

export interface Finalization {
  inputTotal: number;
  outputTotal: number;
  fee: number;
  status: "finalized" | "blocked";
}

export interface BalanceBucket {
  inputs: number;
  outputs: number;
  explicitFee: number;
  implicitFee: number;
  net: number;
  balanced: boolean;
}

export interface TransactionBalance {
  inputs: number;
  outputs: number;
  fee: {
    explicit: number;
    implicit: number;
    total: number;
  };
  mine: BalanceBucket;
  other: BalanceBucket;
  mineBalanced: boolean;
  status: "balanced" | "mine-balanced" | "mine-unbalanced" | "deficit";
}

export interface SneakernetFragmentStatus {
  peerless: boolean;
  peers: number;
  sessions: number;
  fragments: number;
  ordered: number;
  unordered: number;
  psbts: number;
  canExport: boolean;
  nextAction: "import" | "make-unordered" | "select-export" | "export";
}

export interface SessionOrderingConfig {
  mode: SessionOrderingMode;
  seed: string;
  valid: boolean;
  error?: string;
}

export interface PsbtCompatibility {
  ok: boolean;
  reason: string;
}

export type PayloadSide = "input" | "output";
export type SizeUnit = "vbytes" | "weight-units";

export interface ItemSizeEstimate {
  vbytes: number;
  weightUnits: number;
  exact: boolean;
}

export interface PayloadSizeEstimate {
  inputVbytes: number;
  outputVbytes: number;
  totalVbytes: number;
  totalWeightUnits: number;
}

export interface PeerLatencyProfile {
  peerId: string;
  minMs: number;
  jitterMs: number;
}

export interface PeerAck {
  peerId: string;
  delayMs: number;
  acked: number;
  total: number;
}

export interface PeerAckPlan {
  peers: string[];
  total: number;
  acks: PeerAck[];
  completionDelayMs: number;
}

export type DescriptorOwnership = "mine" | "other";
export type DescriptorMenuActionId = "tag-mine" | "tag-other";

export interface DescriptorMenuDescriptor {
  id: string;
  privacy: "public" | "private";
  ownership?: DescriptorOwnership;
  color: string;
}

export interface DescriptorMenuAction {
  id: DescriptorMenuActionId;
  label: string;
  disabled: boolean;
}

export interface DescriptorColorChoice {
  color: string;
  selected: boolean;
}

export interface DescriptorMenuState {
  ownership: DescriptorOwnership;
  ownershipActions: DescriptorMenuAction[];
  colorChoices: DescriptorColorChoice[];
  paymentRequestAction: {
    id: "payment-request";
    label: string;
  };
}

export type DescriptorDrawerSourceKind = "utxo" | "payment-request" | "peer-provenance";

export interface DescriptorDrawerSource {
  kind: DescriptorDrawerSourceKind;
  id: string;
  descriptorId?: string | null;
  label?: string;
  valueSats?: number;
  promotedTo?: string | null;
  uri?: string | null;
}

export interface DescriptorDrawerItem {
  kind: "utxo" | "payment-request";
  id: string;
  label: string;
  valueSats: number;
  promotedTo: string | null;
  uri: string | null;
}

export type DisplaySectionKind = "recognized" | "unrecognized";
export type DisplayKind = "coin";

export interface DisplayRow extends PayloadItem {
  displayKind: DisplayKind;
}

export interface DisplaySection {
  kind: DisplaySectionKind;
  label: string;
  descriptorId?: string;
  descriptorColor?: string;
  descriptorMine?: boolean;
  rows: DisplayRow[];
  totalSats: number;
}

export interface DisplaySubtransaction {
  kind: DisplaySectionKind;
  label: string;
  descriptorId?: string;
  descriptorColor?: string;
  descriptorMine?: boolean;
  inputs: DisplaySection;
  outputs: DisplaySection;
  inputTotalSats: number;
  outputTotalSats: number;
  feeSats: number;
  outputFeeSats: number;
  inputDeficitSats: number;
  explicitFeeSats: number;
  inputAccountingTotalSats: number;
  outputAccountingTotalSats: number;
  implicitFeeSats: number;
  estimatedVbytes: number;
}

export interface BalanceSheetTotalRow {
  kind: DisplaySectionKind | "whole";
  label: string;
  descriptorId?: string;
  descriptorColor?: string;
  descriptorMine?: boolean;
  inputTotalSats: number;
  outputTotalSats: number;
  feeSats: number;
  outputFeeSats: number;
  inputDeficitSats: number;
  explicitFeeSats: number;
  inputAccountingTotalSats: number;
  outputAccountingTotalSats: number;
  implicitFeeSats: number;
  estimatedVbytes: number;
}

export interface AccountingDeltaPresentation {
  kind: "balanced" | "surplus" | "deficit";
  column: "input" | "output" | null;
  oppositeColumn: "input" | "output" | null;
  showTotals: boolean;
  totalSats: number;
  explicitFeeSats: number;
  implicitFeeSats: number;
  label: string;
  separator: " / " | " + " | null;
  amountA: number;
  amountB: number | null;
}

export interface DescriptorFeeSignal {
  descriptorId?: string;
  descriptorLabel: string;
  explicitFeeSats: number;
  implicitFeeSats: number;
  totalFeeSats: number;
  estimatedVbytes: number;
  feeRateSatsPerVbyte: number;
  averageFeeRateSatsPerVbyte: number;
  canFinalizeExplicitFee: boolean;
}

export type FeeWarningLevel = "none" | "yellow" | "red" | "confirm";

export interface DescriptorFeeContributionPlan {
  descriptorId?: string;
  descriptorLabel: string;
  availableSats: number;
  selectedSats: number;
  finalExplicitFeeSats: number;
  estimatedVbytes: number;
  feeRateSatsPerVbyte: number;
  averageFeeRateSatsPerVbyte: number;
  relativeFeeRateRatio: number;
  absoluteWarningLevel: FeeWarningLevel;
  relativeWarningLevel: FeeWarningLevel;
  warningLevel: FeeWarningLevel;
  confirmationRequired: boolean;
}

export interface UnorderedPsbtDisplay {
  inputs: DisplaySection[];
  outputs: DisplaySection[];
  subtransactions: DisplaySubtransaction[];
  explicitFeeSats: number;
  estimatedVbytes: number;
  whole: TransactionBalance;
}

export function amountParts(valueSats: number): AmountParts {
  const sats = Math.trunc(Number(valueSats || 0));
  const whole = Math.floor(sats / 100_000_000);
  const fraction = String(sats % 100_000_000).padStart(8, "0");
  const firstSatDigit = fraction.search(/[1-9]/);
  if (whole > 0) {
    const leadingZeros = firstSatDigit === -1 ? fraction : fraction.slice(0, firstSatDigit);
    return {
      prefix: `₿${whole.toLocaleString("en-US")}`,
      muted: `.${leadingZeros}`,
      sats: firstSatDigit === -1 ? "" : fraction.slice(firstSatDigit),
    };
  }
  return {
    prefix: "",
    muted: firstSatDigit === -1 ? "₿0.00000000" : `₿0.${fraction.slice(0, firstSatDigit)}`,
    sats: firstSatDigit === -1 ? "" : fraction.slice(firstSatDigit),
  };
}

export function formatSatAmount(valueSats: number): string {
  const parts = amountParts(valueSats);
  return `${parts.prefix}${parts.muted}${parts.sats}`;
}

export function coinDetailLines(side: PayloadSide, item: PayloadItem, index = 0, unit: SizeUnit = "vbytes"): string[] {
  const lines = prefixedDetailLines("label", [item.label]);
  if (side === "input") {
    const proofLines = item.signatureVerified === true
      ? ["authorized"]
      : [
          ...prefixedDetailLines("witness", detailValues(item, ["finalScriptWitness", "finalWitness"])),
          ...prefixedDetailLines("scriptSig", detailValues(item, ["finalScriptSig", "scriptSig"])),
          ...prefixedDetailLines("signature", detailValues(item, ["partialSignatures", "signatures", "signatureData", "tapKeySig"])),
        ];
    return [
      ...lines,
      ...prefixedDetailLines("outpoint", [inputOutpoint(item, index)]),
      `nSequence ${detailText(item.nSequence) || detailText(item.sequence) || "0xffffffff"}`,
      ...proofLines,
      `${inputSizeLabel(item)} ${formatSizeEstimate(itemSizeEstimate("input", item), unit)}`,
    ];
  }
  return [
    ...lines,
    ...prefixedDetailLines("script", [item.scriptHash, item.script]),
    `size ${formatSizeEstimate(itemSizeEstimate("output", item), unit)}`,
  ];
}

export function normalizeSessionOrdering(mode: SessionOrderingMode, seed: string): SessionOrderingConfig {
  const trimmedSeed = String(seed || "").trim().toLowerCase();
  const validSeed = (value: string) => /^(?:[0-9a-f]{2})+$/.test(value);
  if (mode === "det") {
    if (!trimmedSeed) {
      return { mode, seed: "", valid: false, error: "deterministic ordering requires a seed" };
    }
    if (!validSeed(trimmedSeed)) {
      return { mode, seed: "", valid: false, error: "ordering seed must be hex bytes" };
    }
    return { mode, seed: trimmedSeed, valid: true };
  }
  if (mode === "unset" && trimmedSeed) {
    if (!validSeed(trimmedSeed)) {
      return { mode, seed: "", valid: false, error: "ordering seed must be hex bytes" };
    }
    return { mode, seed: trimmedSeed, valid: true };
  }
  return { mode, seed: "", valid: true };
}

export function seedFromRandomBytes(bytes: ArrayLike<number>): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function inputOutpoint(item: PayloadItem, _index: number): string | null {
  return detailText(item.outpoint);
}

function inputSizeLabel(item: PayloadItem): "size" | "size estimate" {
  return item.vbytes === undefined ? "size estimate" : "size";
}

function detailValues(item: PayloadItem, keys: string[]): unknown[] {
  const values: unknown[] = [];
  for (const key of keys) {
    const value = item[key];
    if (Array.isArray(value)) {
      values.push(...value);
    } else {
      values.push(value);
    }
  }
  return values;
}

function prefixedDetailLines(prefix: string, values: unknown[]): string[] {
  return values
    .map(detailText)
    .filter((value): value is string => Boolean(value))
    .map((value) => `${prefix} ${value}`);
}

function detailText(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

export function hashHex(value: unknown): string {
  let hash = 0x811c9dc5;
  for (const char of String(value || "")) {
    hash ^= char.charCodeAt(0);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

export function peerLatencyProfile(peerId: string): PeerLatencyProfile {
  const hash = Number.parseInt(hashHex(peerId).slice(0, 8), 16);
  return {
    peerId,
    minMs: 450 + (hash % 550),
    jitterMs: 700 + ((hash >>> 8) % 1500),
  };
}

export function samplePeerAckDelay(peerId: string, random01: () => number = Math.random): number {
  const profile = peerLatencyProfile(peerId);
  const sample = Number(random01());
  const finiteSample = Number.isFinite(sample) ? sample : 0;
  const clamped = Math.max(0, Math.min(1, finiteSample));
  return profile.minMs + Math.round(clamped * profile.jitterMs);
}

export function peerIsInteractive(peer: PeerNode): boolean {
  return peer.local !== true;
}

export function peerAckPlan(peerIds: string[], random01: () => number = Math.random): PeerAckPlan {
  const peers = Array.from(new Set(peerIds.map((peerId) => String(peerId || "")).filter(Boolean)));
  const total = peers.length;
  const acks = peers
    .map((peerId) => ({
      peerId,
      delayMs: samplePeerAckDelay(peerId, random01),
      acked: 0,
      total,
    }))
    .sort((left, right) => left.delayMs - right.delayMs);
  for (const [index, ack] of acks.entries()) {
    ack.acked = index + 1;
  }
  return {
    peers,
    total,
    acks,
    completionDelayMs: acks.at(-1)?.delayMs ?? 0,
  };
}

function canonical<T extends { id: string }>(items: T[]): { items: T[]; conflicts: string[] } {
  const seen = new Map<string, T>();
  const conflicts: string[] = [];
  for (const item of items) {
    const prior = seen.get(item.id);
    if (!prior) {
      seen.set(item.id, { ...item });
      continue;
    }
    if (JSON.stringify(prior) !== JSON.stringify(item)) {
      conflicts.push(item.id);
    }
  }
  return {
    items: [...seen.values()].sort((a, b) => a.id.localeCompare(b.id)),
    conflicts,
  };
}

export function mergePayloads(...payloads: Payload[]): Payload {
  const inputResult = canonical(payloads.flatMap((payload) => payload.inputs || []));
  const outputResult = canonical(payloads.flatMap((payload) => payload.outputs || []));
  const descriptorResult = canonical(payloads.flatMap((payload) => payload.descriptors || []));
  const conflicts = [
    ...payloads.flatMap((payload) => payload.conflicts || []),
    ...inputResult.conflicts.map((id) => `input:${id}`),
    ...outputResult.conflicts.map((id) => `output:${id}`),
    ...descriptorResult.conflicts.map((id) => `descriptor:${id}`),
  ];
  return {
    inputs: inputResult.items,
    outputs: outputResult.items,
    descriptors: descriptorResult.items,
    conflicts: [...new Set(conflicts)].sort(),
  };
}

export function psbtCompatibility(left: PsbtNode, right: PsbtNode): PsbtCompatibility {
  if (left.format !== "unordered" || right.format !== "unordered") {
    return { ok: false, reason: "only unordered PSBTs can join" };
  }
  if (hasOrderingPolicyConflict(left, right)) {
    return { ok: false, reason: "ordering policy conflict: deterministic cannot join explicit" };
  }
  const conflicts = mergePayloads(left, right).conflicts;
  if (conflicts.length) {
    return { ok: false, reason: `payload conflict: ${conflicts.join(", ")}` };
  }
  return { ok: true, reason: "compatible" };
}

export function psbtsAreCompatible(left: PsbtNode, right: PsbtNode): boolean {
  return psbtCompatibility(left, right).ok;
}

function hasOrderingPolicyConflict(left: PsbtNode, right: PsbtNode): boolean {
  const modes = [psbtOrderingMode(left), psbtOrderingMode(right)];
  return modes.includes("det") && modes.includes("explicit");
}

function psbtOrderingMode(node: PsbtNode): SessionOrderingMode {
  return node.sortMode || "unset";
}

export function psbtProtocolIdentity(node: PsbtNode, vertexKind: "fragment" | "session" = "fragment"): PsbtProtocolIdentity {
  if (isOrderedNonmodifiableSegwit(node)) {
    return {
      label: "txid",
      value: longHashHex({ kind: "txid", transaction: txidMaterial(node) }),
      source: "ordered non-modifiable SegWit transaction",
      stableBeforeSigning: true,
    };
  }
  return {
    label: "unique id",
    value: longHashHex({ kind: "psbt-unique-id", psbt: psbtIdentityMaterial(node, vertexKind) }),
    source: psbtUniqueIdSource(node),
    stableBeforeSigning: false,
  };
}

function isOrderedNonmodifiableSegwit(node: PsbtNode): boolean {
  if (node.format === "unordered") return false;
  const nonmodifiable = node.format === "bip174" || node.modifiable === "none" || node.kind === "sorter-output";
  if (!nonmodifiable) return false;
  if (unknownField(node, "segwit") === false) return false;
  return node.inputs.every((input) => unknownField(input, "segwit") !== false && unknownField(input, "legacy") !== true);
}

function psbtUniqueIdSource(node: PsbtNode): string {
  if (node.format === "unordered") return "psbt.md unordered PSBT unique id";
  if (node.format === "bip370") return "BIP 370 PSBT unique id";
  return "BIP 174 PSBT unique id";
}

function txidMaterial(node: PsbtNode): unknown {
  return {
    version: unknownField(node, "version") ?? 2,
    lockTime: unknownField(node, "lockTime") ?? 0,
    inputs: node.inputs.map((input) => pickFields(input, ["outpoint", "txid", "vout", "id", "nSequence", "sequence"])),
    outputs: node.outputs.map((output) => pickFields(output, ["address", "scriptPubKey", "script", "scriptHash", "valueSats", "id"])),
  };
}

function psbtIdentityMaterial(node: PsbtNode, vertexKind: "fragment" | "session"): unknown {
  const canonicalPayload = mergePayloads(node, { inputs: [], outputs: [], descriptors: [], conflicts: [] });
  return {
    format: node.format,
    vertexKind,
    role: psbtRole(node, vertexKind).id,
    modifiable: node.modifiable || null,
    sortMode: node.sortMode || null,
    seed: node.seed || null,
    inputs: canonicalPayload.inputs,
    outputs: canonicalPayload.outputs,
    descriptors: canonicalPayload.descriptors,
    conflicts: canonicalPayload.conflicts,
  };
}

function pickFields(value: unknown, keys: string[]): Record<string, unknown> {
  const record = value as Record<string, unknown>;
  return Object.fromEntries(keys.flatMap((key) => (
    Object.prototype.hasOwnProperty.call(record, key) ? [[key, record[key]]] : []
  )));
}

function unknownField(value: unknown, key: string): unknown {
  return (value as Record<string, unknown>)[key];
}

function longHashHex(value: unknown): string {
  const material = stableStringify(value);
  return Array.from({ length: 8 }, (_, index) => hashHex(`${index}:${material}`)).join("");
}

function stableStringify(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.entries(value as Record<string, unknown>)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, entry]) => `${JSON.stringify(key)}:${stableStringify(entry)}`)
    .join(",")}}`;
}

export function psbtRole(node: PsbtNode, vertexKind: "fragment" | "session" = "fragment"): PsbtRole {
  if (node.format === "unordered") {
    return vertexKind === "session"
      ? {
          id: "unordered-register",
          label: "constructor<modifiable, unordered>",
          spec: "multiparty PSBT register",
          roles: ["Constructor", "Combiner", "Sync"],
        }
      : {
          id: "unordered-fragment",
          label: "constructor<modifiable, unordered>",
          spec: "multiparty PSBT fragment",
          roles: ["Constructor", "Combiner"],
        };
  }
  if (node.format === "bip174") {
    return fixedTransactionRole("BIP 174");
  }
  if (node.kind === "sorter-output") {
    return {
      id: "sorted-bip370",
      label: "fixed input/output set",
      spec: "Sorter output",
      roles: ["Updater", "Signer"],
    };
  }
  const modifiable = node.modifiable || "both";
  if (modifiable === "none") {
    return fixedTransactionRole("BIP 174 compatible");
  }
  return {
    id: "bip370-constructor",
    label: `constructor<modifiable ${modifiabilityLabel(modifiable)}>`,
    spec: "BIP 370",
    roles: ["Constructor", "Updater"],
  };
}

export function psbtUnaryActions(node: PsbtNode, vertexKind: "fragment" | "session" = "fragment"): PsbtUnaryAction[] {
  const role = psbtRole(node, vertexKind);
  const atomizeActions: PsbtUnaryAction[] = psbtCanAtomize(node) ? ["atomize"] : [];
  if (role.id === "unordered-register") return ["fix-sets", "abort-session"];
  if (role.id === "unordered-fragment") return ["sort", ...atomizeActions, "promote"];
  if (role.id === "bip370-constructor") {
    return vertexKind === "fragment" ? ["make-unordered", ...atomizeActions] : ["make-unordered"];
  }
  if (role.id === "sorted-bip370") return ["make-unordered"];
  return [];
}

function psbtCanAtomize(node: Payload): boolean {
  return (node.inputs || []).length + (node.outputs || []).length > 1;
}

function fixedTransactionRole(spec: string): PsbtRole {
  return {
    id: "fixed-transaction",
    label: "Fixed transaction",
    spec,
    roles: ["Updater", "Signer"],
  };
}

function modifiabilityLabel(value: Exclude<PsbtModifiability, "none">): string {
  if (value === "both") return "inputs+outputs";
  return value;
}

export function joinSessionSeeds(sessions: Array<{ seed: string }>): string {
  const sum = sessions.reduce((accumulator, session) => (accumulator + parseInt(hashHex(session.seed), 16)) >>> 0, 0);
  return sum.toString(16).padStart(8, "0");
}

export function orderedProjectionPayload(node: Payload): Payload {
  const clone = <T extends PayloadItem | DescriptorPayload>(item: T): T => ({ ...item });
  return {
    inputs: [...(node.inputs || [])].map(clone).sort(orderByStableId),
    outputs: [...(node.outputs || [])].map(clone).sort(orderByStableId),
    descriptors: [],
    conflicts: [...(node.conflicts || [])],
  };
}

export function orderByStableId(left: PayloadItem, right: PayloadItem): number {
  return String(left.id || left.label || left.address || "").localeCompare(String(right.id || right.label || right.address || ""));
}

export function totalSats(items: Array<{ valueSats?: number }>): number {
  return items.reduce((sum, item) => sum + Number(item.valueSats || 0), 0);
}

function explicitFeeSats(items: Array<{ explicitFeeSats?: number }>): number {
  return items.reduce((sum, item) => sum + Number(item.explicitFeeSats || 0), 0);
}

function estimatedRowsVbytes(rows: PayloadItem[], side: PayloadSide): number {
  return rows.reduce((sum, row) => sum + itemSizeEstimate(side, row).vbytes, 0);
}

const DEFAULT_INPUT_VBYTES = 68;
const DEFAULT_OUTPUT_VBYTES = 31;
const MIN_TAPROOT_SCRIPT_PATH_INPUT_VBYTES = 68;

export function itemSizeEstimate(side: PayloadSide, item: PayloadItem): ItemSizeEstimate {
  const fallback = side === "input" ? inputFallbackVbytes(item) : DEFAULT_OUTPUT_VBYTES;
  const candidate = Number(item.estimatedVbytes ?? item.vbytes ?? fallback);
  const positiveCandidate = Number.isFinite(candidate) && candidate > 0 ? candidate : fallback;
  const vbytes = side === "input"
    ? Math.max(positiveCandidate, inputFallbackVbytes(item))
    : positiveCandidate;
  return {
    vbytes,
    weightUnits: Math.ceil(vbytes * 4),
    exact: side === "output" || item.vbytes !== undefined,
  };
}

export function payloadSizeEstimate(payload: Payload): PayloadSizeEstimate {
  const inputVbytes = estimatedRowsVbytes(payload.inputs || [], "input");
  const outputVbytes = estimatedRowsVbytes(payload.outputs || [], "output");
  return {
    inputVbytes,
    outputVbytes,
    totalVbytes: inputVbytes + outputVbytes,
    totalWeightUnits: Math.ceil((inputVbytes + outputVbytes) * 4),
  };
}

export function formatSizeEstimate(size: number | ItemSizeEstimate | PayloadSizeEstimate, unit: SizeUnit = "vbytes"): string {
  const vbytes = typeof size === "number"
    ? size
    : "totalVbytes" in size
      ? size.totalVbytes
      : size.vbytes;
  if (unit === "weight-units") {
    return `${Math.ceil(vbytes * 4)} WU`;
  }
  return `${formatSizeNumber(vbytes)} vB`;
}

function formatSizeNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function inputFallbackVbytes(item: PayloadItem): number {
  return inputMayUseTaprootScriptPath(item)
    ? Math.max(DEFAULT_INPUT_VBYTES, MIN_TAPROOT_SCRIPT_PATH_INPUT_VBYTES)
    : DEFAULT_INPUT_VBYTES;
}

function inputMayUseTaprootScriptPath(item: PayloadItem): boolean {
  if (item.taprootScriptPath === true || item.scriptPath === true) return true;
  const scriptType = stringField(item, "scriptType") || stringField(item, "inputType") || stringField(item, "witnessType");
  return Boolean(scriptType?.toLowerCase().includes("p2tr") && scriptType.toLowerCase().includes("script"));
}

function isMine(item: PayloadItem): boolean {
  return item.descriptorMine === true;
}

function balanceBucket(inputs: PayloadItem[], outputs: PayloadItem[]): BalanceBucket {
  const inputTotal = totalSats(inputs);
  const outputTotal = totalSats(outputs);
  const explicitFee = explicitFeeSats([...inputs, ...outputs]);
  const rawNet = inputTotal - outputTotal;
  const implicitFee = rawNet - explicitFee;
  return {
    inputs: inputTotal,
    outputs: outputTotal,
    explicitFee,
    implicitFee,
    net: implicitFee,
    balanced: implicitFee === 0,
  };
}

export function transactionBalance(payload: Payload): TransactionBalance {
  const inputs = payload.inputs;
  const outputs = payload.outputs;
  const inputTotal = totalSats(inputs);
  const outputTotal = totalSats(outputs);
  const totalFee = inputTotal - outputTotal;
  const explicit = explicitFeeSats([...inputs, ...outputs]);
  const mine = balanceBucket(inputs.filter(isMine), outputs.filter(isMine));
  const other = balanceBucket(inputs.filter((item) => !isMine(item)), outputs.filter((item) => !isMine(item)));
  const implicit = totalFee - explicit;
  const status = totalFee < 0
    ? "deficit"
    : mine.balanced && other.balanced
      ? "balanced"
      : mine.balanced
        ? "mine-balanced"
        : "mine-unbalanced";
  return {
    inputs: inputTotal,
    outputs: outputTotal,
    fee: {
      explicit,
      implicit,
      total: totalFee,
    },
    mine,
    other,
    mineBalanced: mine.balanced,
    status,
  };
}

export function descriptorMenuState(record: DescriptorMenuDescriptor, palette: string[]): DescriptorMenuState {
  const ownership = record.privacy === "private"
    ? "mine"
    : record.ownership === "mine" ? "mine" : "other";
  return {
    ownership,
    ownershipActions: [
      { id: "tag-mine", label: "Tag mine", disabled: ownership === "mine" },
      { id: "tag-other", label: "Tag other", disabled: record.privacy === "private" || ownership === "other" },
    ],
    colorChoices: palette.map((color) => ({
      color,
      selected: color === record.color,
    })),
    paymentRequestAction: {
      id: "payment-request",
      label: "Generate payment request URI",
    },
  };
}

export function descriptorDrawerItems(
  descriptorId: string | null,
  sources: DescriptorDrawerSource[],
): DescriptorDrawerItem[] {
  const wanted = descriptorId || null;
  return sources
    .filter((source) => source.kind !== "peer-provenance" && (source.descriptorId || null) === wanted)
    .map((source) => ({
      kind: source.kind === "utxo" ? "utxo" : "payment-request",
      id: source.id,
      label: source.label || source.id,
      valueSats: Number(source.valueSats || 0),
      promotedTo: source.promotedTo || null,
      uri: source.uri || null,
    }));
}

export function unorderedPsbtDisplay(payload: Payload): UnorderedPsbtDisplay {
  const whole = transactionBalance(payload);
  const inputs = displaySections(payload.inputs.map(displayCoinRow));
  const outputs = displaySections(payload.outputs.map(displayCoinRow));
  return {
    inputs,
    outputs,
    subtransactions: displaySubtransactions(inputs, outputs),
    explicitFeeSats: whole.fee.explicit,
    estimatedVbytes: payloadSizeEstimate(payload).totalVbytes,
    whole,
  };
}

export function payloadRowKey(side: "input" | "output", item: PayloadItem): string {
  return `${side}:${item.id}`;
}

export function pendingPayloadRowKeys(payload: Payload): string[] {
  return [
    ...payload.inputs.map((input) => payloadRowKey("input", input)),
    ...payload.outputs.map((output) => payloadRowKey("output", output)),
  ];
}

function displaySections(rows: DisplayRow[]): DisplaySection[] {
  const recognized = new Map<string, DisplaySection>();
  const unrecognizedRows: DisplayRow[] = [];
  for (const row of rows) {
    const descriptorId = descriptorGroupKey(row);
    if (!descriptorId) {
      unrecognizedRows.push(row);
      continue;
    }
    let section = recognized.get(descriptorId);
    if (!section) {
      section = {
        kind: "recognized",
        descriptorId,
        label: descriptorGroupLabel(row, descriptorId),
        descriptorColor: stringField(row, "descriptorGroupColor") || stringField(row, "descriptorColor"),
        descriptorMine: booleanField(row, "descriptorGroupMine") ?? row.descriptorMine === true,
        rows: [],
        totalSats: 0,
      };
      recognized.set(descriptorId, section);
    }
    section.rows.push(row);
    section.totalSats += Number(row.valueSats || 0);
  }
  const sections = [...recognized.values()];
  if (unrecognizedRows.length) {
    sections.push({
      kind: "unrecognized",
      label: "unrecognized",
      rows: unrecognizedRows,
      totalSats: totalSats(unrecognizedRows),
    });
  }
  return sections;
}

function displaySubtransactions(inputs: DisplaySection[], outputs: DisplaySection[]): DisplaySubtransaction[] {
  const order = sectionOrder(inputs, outputs);
  return order.map((key) => {
    const input = inputs.find((section) => sectionKey(section) === key);
    const output = outputs.find((section) => sectionKey(section) === key);
    const template = (input || output) as DisplaySection;
    const inputSection = input || emptyPeerSection(template);
    const outputSection = output || emptyPeerSection(template);
    const explicitFee = explicitFeeSats([...inputSection.rows, ...outputSection.rows]);
    const feeSats = inputSection.totalSats - outputSection.totalSats;
    const inputAccountingTotal = inputSection.totalSats;
    const outputAccountingTotal = outputSection.totalSats + explicitFee;
    const implicitFee = inputAccountingTotal - outputAccountingTotal;
    const estimatedVbytes =
      estimatedRowsVbytes(inputSection.rows, "input") + estimatedRowsVbytes(outputSection.rows, "output");
    return {
      kind: template.kind,
      label: template.label,
      descriptorId: template.descriptorId,
      descriptorColor: template.descriptorColor,
      descriptorMine: template.descriptorMine,
      inputs: inputSection,
      outputs: outputSection,
      inputTotalSats: inputSection.totalSats,
      outputTotalSats: outputSection.totalSats,
      feeSats,
      outputFeeSats: Math.max(0, feeSats),
      inputDeficitSats: Math.max(0, -feeSats),
      explicitFeeSats: explicitFee,
      inputAccountingTotalSats: inputAccountingTotal,
      outputAccountingTotalSats: outputAccountingTotal,
      implicitFeeSats: implicitFee,
      estimatedVbytes,
    };
  });
}

function sectionOrder(inputs: DisplaySection[], outputs: DisplaySection[]): string[] {
  const recognized = new Set<string>();
  const order: string[] = [];
  for (const section of [...inputs, ...outputs]) {
    if (section.kind === "unrecognized") continue;
    const key = sectionKey(section);
    if (!recognized.has(key)) {
      recognized.add(key);
      order.push(key);
    }
  }
  if ([...inputs, ...outputs].some((section) => section.kind === "unrecognized")) {
    order.push("unrecognized");
  }
  return order;
}

function sectionKey(section: DisplaySection): string {
  return section.kind === "unrecognized" ? "unrecognized" : String(section.descriptorId);
}

function emptyPeerSection(template: DisplaySection): DisplaySection {
  return {
    kind: template.kind,
    label: template.label,
    descriptorId: template.descriptorId,
    descriptorColor: template.descriptorColor,
    descriptorMine: template.descriptorMine,
    rows: [],
    totalSats: 0,
  };
}

function displayCoinRow(item: PayloadItem): DisplayRow {
  return {
    ...item,
    displayKind: "coin",
  };
}

function descriptorGroupKey(item: PayloadItem): string | undefined {
  return stringField(item, "descriptorGroupId") || stringField(item, "descriptorId");
}

function descriptorGroupLabel(item: PayloadItem, fallback: string): string {
  return stringField(item, "descriptorGroupLabel") || stringField(item, "descriptorLabel") || fallback;
}

function stringField(item: PayloadItem, key: string): string | undefined {
  const value = item[key];
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function booleanField(item: PayloadItem, key: string): boolean | undefined {
  const value = item[key];
  return typeof value === "boolean" ? value : undefined;
}

export function sneakernetFragmentStatus(
  peers: PeerNode[],
  sessions: PsbtNode[],
  fragments: PsbtNode[],
): SneakernetFragmentStatus {
  const psbts = [...sessions, ...fragments];
  const unordered = psbts.filter((node) => node.format === "unordered").length;
  const ordered = psbts.length - unordered;
  const nextAction = psbts.length === 0
    ? "import"
    : ordered > 0
      ? "make-unordered"
      : psbts.length > 1
        ? "select-export"
        : "export";
  return {
    peerless: peers.length === 0,
    peers: peers.length,
    sessions: sessions.length,
    fragments: fragments.length,
    ordered,
    unordered,
    psbts: psbts.length,
    canExport: psbts.length > 0,
    nextAction,
  };
}

export function unorderedBalanceSheetTotalRows(display: UnorderedPsbtDisplay): BalanceSheetTotalRow[] {
  return [
    ...display.subtransactions.map((section) => ({
      kind: section.kind,
      label: section.label,
      descriptorId: section.descriptorId,
      descriptorColor: section.descriptorColor,
      descriptorMine: section.descriptorMine,
      inputTotalSats: section.inputTotalSats,
      outputTotalSats: section.outputTotalSats,
      feeSats: section.feeSats,
      outputFeeSats: section.outputFeeSats,
      inputDeficitSats: section.inputDeficitSats,
      explicitFeeSats: section.explicitFeeSats,
      inputAccountingTotalSats: section.inputAccountingTotalSats,
      outputAccountingTotalSats: section.outputAccountingTotalSats,
      implicitFeeSats: section.implicitFeeSats,
      estimatedVbytes: section.estimatedVbytes,
    })),
    {
      kind: "whole",
      label: "total",
      inputTotalSats: display.whole.inputs,
      outputTotalSats: display.whole.outputs,
      feeSats: display.whole.fee.total,
      outputFeeSats: Math.max(0, display.whole.fee.total),
      inputDeficitSats: Math.max(0, -display.whole.fee.total),
      explicitFeeSats: display.whole.fee.explicit,
      inputAccountingTotalSats: display.whole.inputs,
      outputAccountingTotalSats: display.whole.outputs + display.whole.fee.explicit,
      implicitFeeSats: display.whole.fee.implicit,
      estimatedVbytes: display.estimatedVbytes,
    },
  ];
}

export function accountingDeltaPresentation(section: BalanceSheetTotalRow | DisplaySubtransaction): AccountingDeltaPresentation {
  const feeSats = Number(section.feeSats || 0);
  const explicitFee = Number(section.explicitFeeSats || 0);
  const implicitFee = Number(section.implicitFeeSats || 0);
  const totalSats = Math.abs(feeSats);
  const kind = feeSats < 0 ? "deficit" : feeSats > 0 ? "surplus" : "balanced";
  const column = kind === "deficit" ? "input" : kind === "surplus" ? "output" : null;
  const oppositeColumn = column === "input" ? "output" : column === "output" ? "input" : null;
  const showTotals = Number(section.inputAccountingTotalSats || 0) !== 0 && Number(section.outputAccountingTotalSats || 0) !== 0;
  if (kind === "surplus") {
    return {
      kind,
      column,
      oppositeColumn,
      showTotals,
      totalSats,
      explicitFeeSats: explicitFee,
      implicitFeeSats: implicitFee,
      label: explicitFee > 0 ? "accounted / surplus" : "surplus",
      separator: explicitFee > 0 ? " / " : null,
      amountA: explicitFee > 0 ? explicitFee : totalSats,
      amountB: explicitFee > 0 ? totalSats : null,
    };
  }
  if (kind === "deficit") {
    return {
      kind,
      column,
      oppositeColumn,
      showTotals,
      totalSats,
      explicitFeeSats: explicitFee,
      implicitFeeSats: implicitFee,
      label: explicitFee > 0 ? "deficit + accounted" : "deficit",
      separator: explicitFee > 0 ? " + " : null,
      amountA: totalSats,
      amountB: explicitFee > 0 ? explicitFee : null,
    };
  }
  return {
    kind,
    column,
    oppositeColumn,
    showTotals,
    totalSats: 0,
    explicitFeeSats: explicitFee,
    implicitFeeSats: implicitFee,
    label: "",
    separator: null,
    amountA: 0,
    amountB: null,
  };
}

export function shouldShowGrandTotal(display: UnorderedPsbtDisplay): boolean {
  return display.subtransactions.length > 1;
}

export function balanceSheetFeeSignal(
  section: BalanceSheetTotalRow | DisplaySubtransaction,
  averageFeeRateSatsPerVbyte: number,
): DescriptorFeeSignal {
  const estimatedVbytes = section.estimatedVbytes;
  return {
    descriptorId: section.descriptorId,
    descriptorLabel: section.label,
    explicitFeeSats: section.explicitFeeSats,
    implicitFeeSats: section.implicitFeeSats,
    totalFeeSats: section.feeSats,
    estimatedVbytes,
    feeRateSatsPerVbyte: section.feeSats / Math.max(1, estimatedVbytes),
    averageFeeRateSatsPerVbyte,
    canFinalizeExplicitFee: Boolean(section.descriptorId) &&
      section.descriptorMine === true &&
      section.implicitFeeSats > 0 &&
      "inputs" in section &&
      section.inputs.rows.length > 0,
  };
}

export function descriptorFeeSignal(payload: Payload, descriptorId: string): DescriptorFeeSignal | null {
  const display = unorderedPsbtDisplay(payload);
  const section = display.subtransactions.find((candidate) => candidate.descriptorId === descriptorId);
  if (!section) return null;
  return balanceSheetFeeSignal(section, display.whole.fee.total / Math.max(1, display.estimatedVbytes));
}

export function descriptorFeeContributionPlan(signal: DescriptorFeeSignal | null, selectedSats: number): DescriptorFeeContributionPlan | null {
  if (!signal) return null;
  const availableSats = Math.max(0, signal.implicitFeeSats);
  const selected = clampFeeContribution(selectedSats, availableSats);
  const feeRateSatsPerVbyte = selected / Math.max(1, signal.estimatedVbytes);
  const relativeFeeRateRatio = relativeFeeRate(feeRateSatsPerVbyte, signal.averageFeeRateSatsPerVbyte);
  const absoluteWarningLevel = feeRateWarningLevel(feeRateSatsPerVbyte);
  const relativeWarningLevel = relativeFeeWarningLevel(relativeFeeRateRatio);
  const warningLevel = maxFeeWarningLevel(absoluteWarningLevel, relativeWarningLevel);
  return {
    descriptorId: signal.descriptorId,
    descriptorLabel: signal.descriptorLabel,
    availableSats,
    selectedSats: selected,
    finalExplicitFeeSats: signal.explicitFeeSats + selected,
    estimatedVbytes: signal.estimatedVbytes,
    feeRateSatsPerVbyte,
    averageFeeRateSatsPerVbyte: signal.averageFeeRateSatsPerVbyte,
    relativeFeeRateRatio,
    absoluteWarningLevel,
    relativeWarningLevel,
    warningLevel,
    confirmationRequired: warningLevel === "confirm",
  };
}

function clampFeeContribution(value: number, maxValue: number): number {
  const candidate = Math.floor(Number(value));
  if (!Number.isFinite(candidate)) return 0;
  return Math.min(Math.max(0, candidate), maxValue);
}

function relativeFeeRate(feeRate: number, averageFeeRate: number): number {
  if (averageFeeRate > 0) return feeRate / averageFeeRate;
  return feeRate > 0 ? Number.POSITIVE_INFINITY : 0;
}

function feeRateWarningLevel(feeRate: number): FeeWarningLevel {
  if (feeRate > 1000) return "confirm";
  if (feeRate > 100) return "red";
  if (feeRate > 10) return "yellow";
  return "none";
}

function relativeFeeWarningLevel(ratio: number): FeeWarningLevel {
  if (ratio > 10) return "confirm";
  if (ratio > 2) return "red";
  if (ratio > 1.1) return "yellow";
  return "none";
}

const FEE_WARNING_ORDER: FeeWarningLevel[] = ["none", "yellow", "red", "confirm"];

function maxFeeWarningLevel(left: FeeWarningLevel, right: FeeWarningLevel): FeeWarningLevel {
  return FEE_WARNING_ORDER.indexOf(left) >= FEE_WARNING_ORDER.indexOf(right) ? left : right;
}

export function finalizeDescriptorExplicitFee(payload: Payload, descriptorId: string, amountSats?: number): Payload {
  const display = unorderedPsbtDisplay(payload);
  const section = display.subtransactions.find((candidate) => candidate.descriptorId === descriptorId);
  const signal = section ? descriptorFeeSignal(payload, descriptorId) : null;
  if (!section || !signal?.canFinalizeExplicitFee) return clonePayload(payload);
  const plan = descriptorFeeContributionPlan(signal, amountSats ?? signal.implicitFeeSats);
  const targetInputId = section.inputs.rows[0].id;
  return {
    inputs: payload.inputs.map((input) => input.id === targetInputId
      ? { ...input, explicitFeeSats: Number(input.explicitFeeSats || 0) + plan!.selectedSats }
      : { ...input }),
    outputs: payload.outputs.map((output) => ({ ...output })),
    descriptors: (payload.descriptors || []).map((descriptor) => ({ ...descriptor })),
    conflicts: [...(payload.conflicts || [])],
  };
}

function clonePayload(payload: Payload): Payload {
  return {
    inputs: payload.inputs.map((input) => ({ ...input })),
    outputs: payload.outputs.map((output) => ({ ...output })),
    descriptors: (payload.descriptors || []).map((descriptor) => ({ ...descriptor })),
    conflicts: [...(payload.conflicts || [])],
  };
}

export function finalizePayload(payload: Payload): Finalization {
  const inputTotal = totalSats(payload.inputs);
  const outputTotal = totalSats(payload.outputs);
  const fee = inputTotal - outputTotal;
  return {
    inputTotal,
    outputTotal,
    fee,
    status: fee >= 0 ? "finalized" : "blocked",
  };
}

export function parseBitcoinUri(text: string): ParsedBitcoinUri | null {
  const match = String(text || "").match(/\bbitcoin:[^\s<>"']+/i);
  if (!match) return null;
  const uri = match[0];
  const body = uri.slice("bitcoin:".length);
  const [addressPart, query = ""] = body.split("?");
  const params = new URLSearchParams(query);
  const address = decodeURIComponent(addressPart || params.get("address") || "");
  if (!address) return null;
  const satsParam = params.get("sats");
  const amountParam = params.get("amount");
  const valueSats = satsParam
    ? Math.max(0, Math.trunc(Number(satsParam)))
    : Math.max(0, Math.round(Number(amountParam || 0) * 100_000_000));
  return {
    uri,
    address,
    valueSats,
    descriptorId: params.get("ptj_descriptor"),
    label: params.get("label") || "BIP 321 request",
    message: params.get("message") || "",
  };
}

export function compactBase64(value: string): string {
  return String(value || "").replace(/\s+/g, "");
}

export function looksLikeBase64Psbt(value: string): boolean {
  const compact = compactBase64(value);
  return /^cHNidP/i.test(compact) && /^[A-Za-z0-9+/=]+$/.test(compact) && compact.length >= 10;
}

export function looksLikeDescriptor(value: string): boolean {
  return /^\s*(?:addr|combo|multi|pk|pkh|raw|sh|sortedmulti|tr|wpkh|wsh)\s*\(/i.test(value);
}

export function descriptorLooksPrivate(value: string): boolean {
  return /\b(?:xprv|tprv|yprv|zprv|uprv|vprv|prv)\b/i.test(value);
}

export function peerBridgeComponents(peers: PeerNode[], edges: GraphEdge[]): string[][] {
  const peerIds = peers.map((peer) => peer.id);
  const order = new Map(peerIds.map((id, index) => [id, index]));
  const adjacency = new Map(peerIds.map((id) => [id, new Set<string>()]));
  for (const edge of edges) {
    if (edge.kind !== "peer-bridge" || !adjacency.has(edge.from) || !adjacency.has(edge.to)) continue;
    adjacency.get(edge.from)?.add(edge.to);
    adjacency.get(edge.to)?.add(edge.from);
  }
  const groups: string[][] = [];
  const seen = new Set<string>();
  for (const peerId of peerIds) {
    if (seen.has(peerId)) continue;
    const stack = [peerId];
    const group: string[] = [];
    while (stack.length) {
      const current = stack.pop();
      if (!current || seen.has(current)) continue;
      seen.add(current);
      group.push(current);
      for (const next of adjacency.get(current)!) stack.push(next);
    }
    groups.push(group.sort((left, right) => Number(order.get(left)) - Number(order.get(right))));
  }
  return groups.sort((left, right) => Number(order.get(left[0])) - Number(order.get(right[0])));
}

export function sessionVisibleToPeerGroup(
  session: { id: string; peers?: string[] },
  peers: PeerNode[],
  peerIds: string[],
): boolean {
  const sessionReaders = new Set(session.peers ?? []);
  const peerViews = new Map(peers.map((peer) => [peer.id, peer.views ?? {}]));
  return peerIds.some((peerId) =>
    sessionReaders.has(peerId) ||
    Object.prototype.hasOwnProperty.call(peerViews.get(peerId) ?? {}, session.id),
  );
}

export function peerGroupBounds(group: string[], positions: Map<string, Box>): Box | null {
  const boxes = group.map((peerId) => positions.get(peerId)).filter((box): box is Box => Boolean(box));
  if (!boxes.length) return null;
  const minX = Math.min(...boxes.map((box) => box.x));
  const minY = Math.min(...boxes.map((box) => box.y));
  const maxX = Math.max(...boxes.map((box) => box.x + box.width));
  const maxY = Math.max(...boxes.map((box) => box.y + box.height));
  return {
    x: minX,
    y: minY,
    width: maxX - minX,
    height: maxY - minY,
  };
}

export function peerEdgeTermination(peerId: string, groups: string[][], positions: Map<string, Box>): Box | null {
  const group = groups.find((component) => component.includes(peerId));
  if (!group || group.length < 2) return positions.get(peerId) || null;
  const bounds = peerGroupBounds(group, positions);
  if (!bounds) return positions.get(peerId) || null;
  return {
    x: bounds.x + bounds.width / 2 - 1,
    y: bounds.y,
    width: 2,
    height: bounds.height + 8,
  };
}
