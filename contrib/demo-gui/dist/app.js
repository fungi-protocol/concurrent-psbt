// @ts-nocheck
// The graph shell is an incremental TypeScript migration from the preview JS.
// Pure PSBT/session model code is strictly checked in src/model.ts.
const { PtjBackendError, atomizePsbt: atomizeBackendPsbt, createPsbt: createBackendPsbt, joinPsbts: joinBackendPsbts, makeUnordered: makeBackendUnordered, sortPsbt: sortBackendPsbt, } = await import("./backend.js?v=20260629-backend-client-v1");
const { amountParts, accountingDeltaPresentation, balanceSheetFeeSignal, coinDetailLines, compactBase64, descriptorFeeContributionPlan, descriptorFeeSignal, descriptorDrawerItems, descriptorMenuState, descriptorLooksPrivate, finalizeDescriptorExplicitFee, formatSizeEstimate, formatSatAmount, hashHex, joinSessionSeeds, looksLikeBase64Psbt, looksLikeDescriptor, mergePayloads, orderedProjectionPayload, parseBitcoinUri, payloadSizeEstimate, payloadRowKey, pendingPayloadRowKeys, peerAckPlan, peerBridgeComponents: modelPeerBridgeComponents, peerEdgeTermination: modelPeerEdgeTermination, peerGroupBounds, peerIsInteractive, sessionVisibleToPeerGroup, psbtRole, psbtCompatibility, psbtProtocolIdentity, psbtUnaryActions, normalizeSessionOrdering, seedFromRandomBytes, shouldShowGrandTotal, transactionBalance, unorderedBalanceSheetTotalRows, unorderedPsbtDisplay, } = await import("./model.js?v=20260629-global-fields-v1");
const palette = ["#1967d2", "#0f7b4f", "#b65c00", "#7b3f98", "#607d00", "#b3261e"];
const descriptorPalette = ["#1967d2", "#b65c00", "#0f7b4f", "#7b3f98", "#b3261e", "#607d00", "#39434d", "#c2410c"];
const UNRECOGNIZED_DESCRIPTOR_ID = "__unrecognized__";
const emptyPayload = () => ({ inputs: [], outputs: [], conflicts: [], descriptors: [] });
const graphMetrics = {
    peer: { y: 56, width: 156, height: 70 },
    session: { labelY: 172, y: 196, width: 330, height: 512 },
    localPeer: { y: 770, height: 428, inset: 28 },
    fragment: { labelY: 874, y: 898, width: 330, height: 292, gap: 72 },
};
let counters = {};
let state;
function nextId(prefix) {
    counters[prefix] = (counters[prefix] || 0) + 1;
    return `${prefix}-${String(counters[prefix]).padStart(2, "0")}`;
}
function byId(collection, id) {
    return collection.find((item) => item.id === id);
}
function allNodes() {
    return [...state.peers, ...state.sessions, ...state.fragments];
}
function remotePeers() {
    return state.peers.filter((peer) => !peer.local);
}
function localPeers() {
    return state.peers.filter((peer) => peer.local);
}
function getNode(id) {
    return allNodes().find((node) => node.id === id);
}
function nodeKind(id) {
    if (byId(state.peers, id))
        return "peer";
    if (byId(state.sessions, id))
        return "session";
    if (byId(state.fragments, id))
        return "fragment";
    return "unknown";
}
function nodeDisplayLabel(node, kind = nodeKind(node?.id)) {
    if (!node)
        return "unselected";
    if (node.label)
        return node.label;
    if (kind === "peer")
        return "unlabeled peer";
    if (kind === "session")
        return "unlabeled session";
    if (kind === "fragment")
        return "unlabeled PSBT";
    return "unlabeled object";
}
function selectActionLabel(nodeId) {
    const node = getNode(nodeId);
    return `Select ${nodeDisplayLabel(node)}`;
}
function short(text, max = 24) {
    const value = String(text || "");
    return value.length > max ? `${value.slice(0, max - 1)}...` : value;
}
function concatLabel(...parts) {
    return parts
        .map((part) => String(part || "").trim())
        .filter(Boolean)
        .join(" ");
}
function coinScriptHash(coin) {
    return coin.scriptHash || hashHex(`${coin.id}|${coin.script || coin.address || coin.label}`);
}
function coinAmount(coin) {
    return Number(coin.valueSats || 0);
}
function descriptorColor(index) {
    return descriptorPalette[index % descriptorPalette.length];
}
function descriptorAddress(descriptorId) {
    return `bcrt1q${hashHex(descriptorId)}${hashHex(`${descriptorId}:addr`).slice(0, 12)}`;
}
function descriptorIsMine(record) {
    return record.privacy === "private" || record.ownership === "mine";
}
function dummyFingerprintColors(coin) {
    const hex = coinScriptHash(coin);
    const colors = ["#1967d2", "#0f7b4f", "#b65c00", "#7b3f98", "#607d00", "#b3261e", "#39434d", "#c2410c"];
    return [0, 2, 4, 6].map((offset) => colors[parseInt(hex.slice(offset, offset + 2), 16) % colors.length]);
}
function applyPayload(target, payload) {
    target.inputs = payload.inputs;
    target.outputs = payload.outputs;
    target.descriptors = payload.descriptors || [];
    target.conflicts = payload.conflicts;
}
function payloadSummary(payload) {
    const inputCount = payload.inputs.length;
    const outputCount = payload.outputs.length;
    const balance = transactionBalance(payload);
    const conflictText = payload.conflicts.length ? `${payload.conflicts.length} conflicts` : "ok";
    return `${inputCount} txin, ${outputCount} txout, ${feeSumText(balance)}, ${conflictText}`;
}
function signedSatAmount(valueSats) {
    const value = Number(valueSats || 0);
    if (value === 0)
        return formatSatAmount(0);
    return `${value > 0 ? "+" : "-"}${formatSatAmount(Math.abs(value))}`;
}
function feeTotalText(valueSats) {
    const value = Number(valueSats || 0);
    return value < 0 ? `deficit ${formatSatAmount(Math.abs(value))}` : `fee ${formatSatAmount(value)}`;
}
function feeSumText(balance) {
    return `${feeTotalText(balance.fee.total)}; ${formatSatAmount(balance.fee.explicit)} explicit; ${signedSatAmount(balance.fee.implicit)} implicit`;
}
function bucketBalanceText(label, bucket) {
    return `${label} ${bucket.balanced ? "balanced" : `Δ ${signedSatAmount(bucket.net)}`}`;
}
function mineBalanceText(balance) {
    return bucketBalanceText("mine", balance.mine);
}
function psbtFormatLabel(node) {
    if (node.format === "bip174")
        return "BIP 174";
    return node.format === "bip370" ? "BIP 370" : "unordered";
}
function psbtRoleFor(node, kind = nodeKind(node.id)) {
    return psbtRole(node, kind === "session" ? "session" : "fragment");
}
function psbtStatusBadges(node, kind, role) {
    const seedBadges = node.seed ? [{
            icon: "🌱",
            className: "sort-seed",
            title: "global deterministic sort seed set",
        }] : [];
    if (node.format === "unordered") {
        return [...seedBadges, {
                icon: "🔀",
                className: "unordered",
                title: kind === "session"
                    ? "unordered PSBT session register: join fragments before sorting"
                    : "unordered PSBT fragment: joinable before sorting",
            }];
    }
    if (role.id === "bip370-constructor") {
        const modifiable = node.modifiable || "both";
        return [...seedBadges, {
                icon: "✏️",
                className: "modifiable",
                title: `BIP 370 modifiable ${modifiable}`,
            }];
    }
    return [
        ...seedBadges,
        {
            icon: "✍",
            className: "sign",
            title: "signatures and witness data can be added",
        },
        {
            icon: "✓",
            className: "finalize",
            title: "finalizable and extractable after signature checks",
        },
    ];
}
function descriptorSummary(node) {
    const descriptors = node.descriptors || [];
    if (!descriptors.length)
        return "0 known descriptors";
    const publicCount = descriptors.filter((descriptor) => descriptor.privacy === "public").length;
    const privateCount = descriptors.filter((descriptor) => descriptor.privacy === "private").length;
    return `${descriptors.length} known descriptor${descriptors.length === 1 ? "" : "s"} · ${publicCount} public · ${privateCount} private`;
}
function descriptorPayload(id, privacy, descriptor) {
    if (!descriptor.trim() || privacy === "none")
        return [];
    return [{
            id,
            privacy,
            descriptor: descriptor.trim(),
        }];
}
function syntheticInputs(prefix, count, valueSats, owner = "import") {
    return Array.from({ length: Number(count || 0) }, (_, index) => ({
        id: `${prefix}:in:${index}`,
        label: `${prefix}-input-${index + 1}`,
        outpoint: `${prefix}:in:${index}`,
        nSequence: "0xffffffff",
        owner,
        valueSats,
        scriptHash: hashHex(`${prefix}:input:${index}:${owner}`),
    }));
}
function syntheticOutputs(prefix, count, totalValueSats, address = "bcrt1qimport") {
    const outputCount = Number(count || 0);
    const total = Number(totalValueSats || 0);
    const value = outputCount > 0 ? Math.floor(total / outputCount) : 0;
    return Array.from({ length: outputCount }, (_, index) => ({
        id: `${prefix}:out:${index}`,
        address,
        valueSats: index === outputCount - 1 ? total - value * (outputCount - 1) : value,
        label: `${prefix}-output-${index + 1}`,
        scriptHash: hashHex(`${prefix}:output:${index}:${address}`),
    }));
}
function ensureUnordered(...nodes) {
    const ordered = nodes.filter((node) => node?.format === "bip370");
    if (!ordered.length)
        return true;
    state.log.unshift(`Convert ${ordered.map((node) => node.id).join(", ")} to unordered before merging into a session CRDT register.`);
    return false;
}
function resetStateContainers() {
    counters = {};
    state = {
        peers: [],
        descriptors: [],
        paymentRequests: [],
        fragments: [],
        sessions: [],
        edges: [],
        selected: [],
        joinWires: [],
        hoveredDescriptorId: null,
        wireDraft: null,
        wireFailure: null,
        convergence: {},
        convergenceRun: 0,
        expandedCoinRows: [],
        feeDraft: null,
        sizeUnit: "vbytes",
        suppressNodeClick: false,
        log: [],
    };
}
function createInitialState() {
    resetStateContainers();
    addPeer("Me", false, { local: true, color: "#39434d" });
    const alice = addPeer("Alice", false);
    const bob = addPeer("Bob", false);
    const carol = addPeer("Carol", false);
    const aliceSpend = createSpendIntent(addDescriptorUtxo("Alice", "wpkh(alice-demo/0/*)", "a".repeat(64), 0, 1200000, false, "public", "mine").id, false);
    const bobSpend = createSpendIntent(addDescriptorUtxo("Bob", "wpkh(bob-demo/0/*)", "b".repeat(64), 1, 900000, false, "public", "other").id, false);
    aliceSpend.inputs[0].explicitFeeSats = 25000;
    bobSpend.inputs[0].explicitFeeSats = 12000;
    const payment = addPaymentIntent("venue output", "bcrt1qvenue", 1750000, false);
    addPaymentIntent("change reserve", "bcrt1qchange", 250000, false);
    addPaymentIntent("facilitator output", "bcrt1qfacilitator", 85000, false);
    addPaymentIntent("rounding buffer", "bcrt1qrounding", 42000, false);
    const aliceSession = promoteFragment(aliceSpend.id, hashHex("alice-seed"), false);
    const bobSession = promoteFragment(bobSpend.id, hashHex("bob-seed"), false);
    absorbFragmentIntoSession(payment.id, aliceSession.id, false);
    shareSessionWithPeer(alice.id, aliceSession.id, false);
    shareSessionWithPeer(bob.id, bobSession.id, false);
    shareSessionWithPeer(carol.id, bobSession.id, false);
    state.selected = [];
    state.joinWires = [];
    state.log.unshift("Demo initialized: demo chain data source lists descriptor UTXOs; free PSBT fragments remain at the bottom.");
}
function addPeer(name, shouldRender = true, options = {}) {
    const id = nextId("peer");
    const peer = {
        id,
        type: "peer",
        label: name.trim() || `Peer ${counters.peer}`,
        color: options.color || palette[(counters.peer - 1) % palette.length],
        local: options.local === true,
        views: {},
    };
    state.peers.push(peer);
    state.log.unshift(`Added peer ${peer.label}.`);
    if (shouldRender)
        render();
    return peer;
}
function addDescriptorUtxo(owner, descriptor, txid, vout, valueSats, shouldRender = true, privacy = "public", ownership = "mine") {
    const id = nextId("utxo");
    const normalizedPrivacy = privacy === "private" ? "private" : "public";
    const record = {
        id,
        hasUtxo: true,
        owner: owner.trim() || "Unassigned",
        descriptor: descriptor.trim() || "wpkh(demo/*)",
        privacy: normalizedPrivacy,
        ownership: normalizedPrivacy === "private" ? "mine" : ownership,
        color: descriptorColor(counters.utxo - 1),
        address: descriptorAddress(id),
        txid: txid.trim() || `${id}${"0".repeat(60)}`.slice(0, 64),
        vout: Number(vout || 0),
        valueSats: Number(valueSats || 1),
        fragmentId: null,
        absorbedInto: null,
    };
    state.descriptors.push(record);
    state.log.unshift(`Added descriptor UTXO ${record.id} for ${record.owner}.`);
    if (shouldRender)
        render();
    return record;
}
function addOutputDescriptor(owner, descriptor, shouldRender = true, privacy = "public", ownership = "mine") {
    const id = nextId("utxo");
    const normalizedPrivacy = privacy === "private" ? "private" : "public";
    const record = {
        id,
        hasUtxo: false,
        owner: owner.trim() || "Pasted descriptor",
        descriptor: descriptor.trim() || "wpkh(demo/*)",
        privacy: normalizedPrivacy,
        ownership: normalizedPrivacy === "private" ? "mine" : ownership,
        color: descriptorColor(counters.utxo - 1),
        address: descriptorAddress(id),
        txid: "",
        vout: 0,
        valueSats: 0,
        fragmentId: null,
        absorbedInto: null,
    };
    state.descriptors.push(record);
    state.log.unshift(`Added output descriptor ${record.id} for ${record.owner}.`);
    if (shouldRender)
        render();
    return record;
}
function createSpendIntent(utxoId, shouldRender = true) {
    const utxo = byId(state.descriptors, utxoId);
    if (!utxo)
        return null;
    if (!utxo.hasUtxo) {
        state.log.unshift(`${utxo.id} is an output descriptor; paste a payment URI or add a UTXO before making a spend intent.`);
        if (shouldRender)
            render();
        return null;
    }
    if (utxo.absorbedInto) {
        selectOnly(utxo.absorbedInto);
        return getNode(utxo.absorbedInto);
    }
    if (utxo.fragmentId && byId(state.fragments, utxo.fragmentId)) {
        selectOnly(utxo.fragmentId);
        return byId(state.fragments, utxo.fragmentId);
    }
    const fragment = {
        id: nextId("frag"),
        type: "fragment",
        kind: "spend",
        label: concatLabel(utxo.owner, "spend"),
        inputs: [{
                id: `${utxo.txid}:${utxo.vout}`,
                outpoint: `${utxo.txid}:${utxo.vout}`,
                nSequence: "0xffffffff",
                owner: utxo.owner,
                valueSats: utxo.valueSats,
                scriptHash: hashHex(`${utxo.descriptor}:${utxo.txid}:${utxo.vout}`),
                descriptorId: utxo.id,
                descriptorLabel: utxo.owner,
                descriptorColor: utxo.color,
                descriptorMine: descriptorIsMine(utxo),
            }],
        outputs: [],
        descriptors: descriptorPayload(`descriptor:${utxo.id}`, utxo.privacy, utxo.descriptor),
        conflicts: [],
        format: "unordered",
        raw: "",
        sourceDescription: `Unsigned one-input PSBT fragment from descriptor UTXO ${utxo.id}. It is not signed, broadcast, or confirmed.`,
    };
    utxo.fragmentId = fragment.id;
    state.fragments.push(fragment);
    state.log.unshift(`Converted descriptor UTXO ${utxo.id} into one-input PSBT ${fragment.id}.`);
    if (shouldRender)
        render();
    return fragment;
}
function addPaymentIntent(label, address, valueSats, shouldRender = true) {
    const id = nextId("frag");
    const outputId = nextId("out");
    const fragment = {
        id,
        type: "fragment",
        kind: "fund",
        label: label.trim() || concatLabel("payment", id),
        inputs: [],
        outputs: [{
                id: outputId,
                address: address.trim() || "bcrt1qrecipient",
                valueSats: Number(valueSats || 1),
                ...(label.trim() ? { label: label.trim() } : {}),
                scriptHash: hashHex(`${address.trim() || "bcrt1qrecipient"}:${valueSats}:${outputId}`),
            }],
        descriptors: [],
        conflicts: [],
        format: "unordered",
        raw: "",
        sourceDescription: "Unsigned one-output PSBT fragment from a manually entered payment intent. It is not signed, broadcast, or confirmed.",
    };
    state.fragments.push(fragment);
    state.log.unshift(`Added one-output PSBT ${fragment.id}.`);
    if (shouldRender)
        render();
    return fragment;
}
function formatBip321AmountParam(valueSats) {
    return (Number(valueSats || 0) / 100_000_000).toFixed(8);
}
function generateBip321Uri(record) {
    const params = new URLSearchParams({
        amount: formatBip321AmountParam(250_000),
        label: `${record.owner} PTJ request`,
        message: "Partial Transaction Joiner payment request",
        ptj_descriptor: record.id,
    });
    return `bitcoin:${record.address}?${params.toString()}`;
}
function paymentRequestId(uri) {
    return `request-${hashHex(uri)}`;
}
function rememberPaymentRequest(parsed, fragmentId = null) {
    if (!parsed)
        return null;
    const existing = state.paymentRequests.find((request) => request.uri === parsed.uri);
    const request = {
        id: paymentRequestId(parsed.uri),
        descriptorId: parsed.descriptor?.id || parsed.descriptorId || null,
        label: parsed.label || "BIP 321 request",
        address: parsed.address,
        valueSats: parsed.valueSats,
        uri: parsed.uri,
        message: parsed.message || "",
        fragmentId: fragmentId || existing?.fragmentId || null,
    };
    if (existing) {
        Object.assign(existing, request);
        return existing;
    }
    state.paymentRequests.push(request);
    return request;
}
function defaultPaymentRequestForDescriptor(record) {
    const parsed = parseBip321Uri(generateBip321Uri(record));
    if (!parsed)
        return null;
    return {
        id: paymentRequestId(parsed.uri),
        descriptorId: record.id,
        label: parsed.label,
        address: parsed.address,
        valueSats: parsed.valueSats,
        uri: parsed.uri,
        message: parsed.message,
        fragmentId: null,
    };
}
function allPaymentRequests() {
    const byUri = new Map();
    for (const descriptor of state.descriptors) {
        const request = defaultPaymentRequestForDescriptor(descriptor);
        if (request)
            byUri.set(request.uri, request);
    }
    for (const request of state.paymentRequests) {
        byUri.set(request.uri, { ...(byUri.get(request.uri) || {}), ...request });
    }
    return [...byUri.values()];
}
function copyDescriptorUri(descriptorId) {
    const record = byId(state.descriptors, descriptorId);
    if (!record)
        return;
    const uri = generateBip321Uri(record);
    rememberPaymentRequest(parseBip321Uri(uri));
    navigator.clipboard?.writeText(uri).catch(() => { });
    state.log.unshift(`Generated payment request URI for ${record.owner}: ${uri}`);
    render();
}
function setDescriptorOwnership(descriptorId, ownership) {
    const record = byId(state.descriptors, descriptorId);
    if (!record)
        return;
    record.ownership = record.privacy === "private" ? "mine" : ownership;
    updateDescriptorLinkedCoins(record);
    state.log.unshift(`${record.owner} descriptor tagged ${descriptorIsMine(record) ? "mine" : "other"}.`);
    render();
}
function setDescriptorColor(descriptorId, color) {
    const record = byId(state.descriptors, descriptorId);
    if (!record)
        return;
    record.color = color;
    updateDescriptorLinkedCoins(record);
    state.log.unshift(`${record.owner} descriptor color updated.`);
    render();
}
function updateDescriptorLinkedCoins(record) {
    for (const node of workspacePsbtNodes()) {
        for (const coin of [...node.inputs, ...node.outputs]) {
            if (coin.descriptorId !== record.id)
                continue;
            coin.descriptorColor = record.color;
            coin.descriptorMine = descriptorIsMine(record);
        }
    }
}
function openDescriptorFeeDialog(nodeId, descriptorId) {
    const node = getNode(nodeId);
    if (!node || !descriptorId)
        return;
    const signal = descriptorFeeSignal(node, descriptorId);
    if (!signal?.canFinalizeExplicitFee) {
        state.log.unshift(`No positive mine surplus to finalize for ${signal?.descriptorLabel || "that descriptor"}.`);
        render();
        return;
    }
    const plan = descriptorFeeContributionPlan(signal, signal.implicitFeeSats);
    state.feeDraft = {
        nodeId,
        descriptorId,
        selectedSats: plan?.selectedSats || 0,
        confirmed: false,
    };
    renderFeeContributionPanel();
}
function updateFeeContributionAmount(value) {
    if (!state.feeDraft)
        return;
    const plan = feeContributionPlan();
    if (!plan) {
        closeFeeContributionPanel();
        return;
    }
    const updated = descriptorFeeContributionPlan(feeContributionSignal(), value);
    state.feeDraft.selectedSats = updated?.selectedSats || 0;
    state.feeDraft.confirmed = false;
    renderFeeContributionPanel();
}
function setFeeContributionConfirmed(confirmed) {
    if (!state.feeDraft)
        return;
    state.feeDraft.confirmed = Boolean(confirmed);
    renderFeeContributionPanel();
}
function closeFeeContributionPanel() {
    state.feeDraft = null;
    renderFeeContributionPanel();
}
function applyFeeContribution() {
    if (!state.feeDraft)
        return;
    const draft = state.feeDraft;
    const node = getNode(draft.nodeId);
    const plan = feeContributionPlan();
    if (!node || !plan) {
        closeFeeContributionPanel();
        return;
    }
    if (plan.confirmationRequired && !draft.confirmed) {
        state.log.unshift("Confirm the high fee contribution before applying it.");
        renderFeeContributionPanel();
        return;
    }
    const updated = finalizeDescriptorExplicitFee(node, draft.descriptorId, plan.selectedSats);
    applyPayload(node, updated);
    if (nodeKind(node.id) === "session") {
        syncPeerViews(node);
        startConvergence(node, `explicit fee updated for ${plan.descriptorLabel}`, node.peers, pendingPayloadRowKeys(updated));
    }
    state.feeDraft = null;
    state.log.unshift(`Set ${formatSatAmount(plan.selectedSats)} additional explicit fee for ${plan.descriptorLabel}.`);
    render();
}
function feeContributionSignal() {
    if (!state.feeDraft)
        return null;
    const node = getNode(state.feeDraft.nodeId);
    return node ? descriptorFeeSignal(node, state.feeDraft.descriptorId) : null;
}
function feeContributionPlan() {
    return descriptorFeeContributionPlan(feeContributionSignal(), state.feeDraft?.selectedSats || 0);
}
function satsToBtcAmount(valueSats) {
    const sats = BigInt(Math.max(0, Math.trunc(Number(valueSats || 0))));
    const whole = sats / 100000000n;
    const fraction = String(sats % 100000000n).padStart(8, "0");
    return `${whole}.${fraction}`;
}
function networkForAddress(address) {
    const text = String(address || "").toLowerCase();
    if (text.startsWith("bcrt"))
        return "regtest";
    if (text.startsWith("tb") || text.startsWith("m") || text.startsWith("n") || text.startsWith("2"))
        return "testnet";
    return "bitcoin";
}
async function hydratePaymentRequestFragment(fragment, parsed) {
    try {
        const response = await createBackendPsbt(window.fetch.bind(window), {
            network: networkForAddress(parsed.address),
            ordering: "unset",
            inputs: [],
            outputs: [{
                    address: parsed.address,
                    amountBtc: satsToBtcAmount(parsed.valueSats),
                }],
        });
        if (byId(state.fragments, fragment.id) !== fragment)
            return;
        fragment.raw = response.psbt;
        fragment.inspect = response.inspect || null;
        fragment.sourceDescription = parsed.message
            ? `Raw one-output PSBT from ptj webgui /api/create for BIP 21/321 URI: ${parsed.message}`
            : "Raw one-output PSBT from ptj webgui /api/create for a BIP 21/321 URI; origin metadata is not serialized into the PSBT.";
        state.log.unshift(`ptj backend created raw PSBT for ${fragment.id}.`);
        render();
    }
    catch (error) {
        if (error instanceof PtjBackendError) {
            state.log.unshift(`ptj backend rejected payment request PSBT: ${error.message}`);
            render();
        }
    }
}
async function hydrateSortedFragment(fragment, source) {
    if (!source.raw)
        return;
    try {
        const response = await sortBackendPsbt(window.fetch.bind(window), source.raw);
        if (byId(state.fragments, fragment.id) !== fragment)
            return;
        fragment.raw = response.psbt;
        fragment.inspect = response.inspect || null;
        fragment.sourceDescription = `Raw sorter output from ptj webgui /api/sort for ${nodeDisplayLabel(source, nodeKind(source.id))}.`;
        state.log.unshift(`ptj backend sorted raw PSBT for ${fragment.id}.`);
        render();
    }
    catch (error) {
        if (error instanceof PtjBackendError) {
            state.log.unshift(`ptj backend rejected sort request: ${error.message}`);
            render();
        }
    }
}
async function hydrateUnorderedPsbt(node, previousRaw) {
    if (!previousRaw)
        return;
    try {
        const response = await makeBackendUnordered(window.fetch.bind(window), previousRaw);
        if (getNode(node.id) !== node)
            return;
        node.raw = response.psbt;
        node.inspect = response.inspect || null;
        node.sourceDescription = `Raw unordered PSBT from ptj webgui /api/make-unordered for ${nodeDisplayLabel(node, nodeKind(node.id))}.`;
        if (nodeKind(node.id) === "session")
            syncPeerViews(node);
        state.log.unshift(`ptj backend made raw PSBT unordered for ${node.id}.`);
        render();
    }
    catch (error) {
        if (error instanceof PtjBackendError) {
            state.log.unshift(`ptj backend rejected make-unordered request: ${error.message}`);
            render();
        }
    }
}
async function hydrateAtomizedFragments(atoms, previousRaw, sourceId) {
    if (!previousRaw)
        return;
    try {
        const response = await atomizeBackendPsbt(window.fetch.bind(window), previousRaw);
        const rawFragments = Array.isArray(response.fragments) ? response.fragments : [];
        for (const [index, atom] of atoms.entries()) {
            const rawFragment = rawFragments[index];
            if (!rawFragment || byId(state.fragments, atom.id) !== atom)
                continue;
            atom.raw = rawFragment.psbt;
            atom.inspect = rawFragment.inspect || null;
            atom.sourceDescription = `Raw atomic PSBT ${index + 1}/${rawFragments.length} from ptj webgui /api/atomize for ${sourceId}.`;
        }
        state.log.unshift(`ptj backend atomized raw PSBT for ${sourceId} into ${rawFragments.length} fragments.`);
        render();
    }
    catch (error) {
        if (error instanceof PtjBackendError) {
            state.log.unshift(`ptj backend rejected atomize request: ${error.message}`);
            render();
        }
    }
}
async function hydrateJoinedPsbt(target, sources) {
    const psbts = sources.map((source) => source.raw).filter(Boolean);
    if (psbts.length !== sources.length)
        return;
    try {
        const response = await joinBackendPsbts(window.fetch.bind(window), psbts);
        if (getNode(target.id) !== target)
            return;
        target.raw = response.psbt;
        target.inspect = response.inspect || null;
        target.sourceDescription = `Raw PSBT LUB from ptj webgui /api/join for ${sources.map((source) => source.id).join(" + ")}.`;
        if (nodeKind(target.id) === "session")
            syncPeerViews(target);
        state.log.unshift(`ptj backend joined raw PSBTs into ${target.id}.`);
        render();
    }
    catch (error) {
        if (error instanceof PtjBackendError) {
            state.log.unshift(`ptj backend rejected join request: ${error.message}`);
            render();
        }
    }
}
function parseBip321Uri(text) {
    const parsed = parseBitcoinUri(text);
    if (!parsed)
        return null;
    return {
        ...parsed,
        descriptor: parsed.descriptorId ? byId(state.descriptors, parsed.descriptorId) : null,
    };
}
function addBip321FragmentFromText(text, shouldRender = true) {
    const parsed = parseBip321Uri(text);
    if (!parsed) {
        state.log.unshift("Paste a bitcoin: BIP 21/321 URI to create a one-output PSBT.");
        if (shouldRender)
            render();
        return null;
    }
    const request = rememberPaymentRequest(parsed);
    const existing = (request?.fragmentId && byId(state.fragments, request.fragmentId)) ||
        state.fragments.find((fragment) => fragment.sourceUri === parsed.uri);
    if (existing) {
        state.selected = [existing.id];
        rememberPaymentRequest(parsed, existing.id);
        state.log.unshift(`Selected existing URI fragment ${existing.id}.`);
        if (shouldRender)
            render();
        return existing;
    }
    const id = nextId("frag");
    const outputId = nextId("out");
    const descriptor = parsed.descriptor;
    const fragment = {
        id,
        type: "fragment",
        kind: "uri",
        label: concatLabel("URI", parsed.label),
        inputs: [],
        outputs: [{
                id: outputId,
                address: parsed.address,
                valueSats: parsed.valueSats,
                label: parsed.label,
                scriptHash: hashHex(`${parsed.address}:${parsed.valueSats}:${outputId}`),
                descriptorId: descriptor?.id || null,
                descriptorLabel: descriptor?.owner || null,
                descriptorColor: descriptor?.color || null,
                descriptorMine: descriptor ? descriptorIsMine(descriptor) : false,
            }],
        descriptors: [],
        conflicts: [],
        format: "unordered",
        raw: "",
        sourceUri: parsed.uri,
        sourceDescription: parsed.message
            ? `Unsigned one-output PSBT fragment from BIP 21/321 URI: ${parsed.message}`
            : "Unsigned one-output PSBT fragment from BIP 21/321 URI; origin metadata is not serialized into the PSBT.",
    };
    state.fragments.push(fragment);
    rememberPaymentRequest(parsed, fragment.id);
    state.selected = [fragment.id];
    state.log.unshift(`Converted BIP 21/321 payment request into one-output PSBT ${fragment.id}.`);
    void hydratePaymentRequestFragment(fragment, parsed);
    if (shouldRender)
        render();
    return fragment;
}
function addPsbtFragmentFromRaw(label, raw, sourceDescription, shouldRender = true) {
    const fragment = importPsbt({
        label,
        raw,
        format: "bip370",
        target: "fragment",
        inputCount: 1,
        outputCount: 1,
        totalValueSats: 1_000_000,
        descriptorPrivacy: "none",
        descriptor: "",
    }, false);
    fragment.kind = "import";
    fragment.sourceDescription = sourceDescription;
    state.log.unshift(`Imported ${fragment.id} from pasted or dropped PSBT bytes.`);
    if (shouldRender)
        render();
    return fragment;
}
function handlePastedText(text, source = "paste") {
    const value = String(text || "").trim();
    if (!value)
        return 0;
    if (handlePastedCandidate(value, source))
        return 1;
    let count = 0;
    for (const line of value.split(/\r?\n/).map((part) => part.trim()).filter(Boolean)) {
        if (handlePastedCandidate(line, source, false))
            count += 1;
    }
    if (count) {
        render();
        return count;
    }
    state.log.unshift("Paste did not match npub, relay URI, wormhole code, bitcoin URI, base64 PSBT, or descriptor.");
    render();
    return 0;
}
function handlePastedCandidate(value, source, shouldRender = true) {
    const npub = value.match(/\bnpub1[023456789acdefghjklmnpqrstuvwxyz]+\b/i)?.[0];
    if (npub) {
        addConnectivityPeer(contactPeerLabel("npub", npub), "npub", npub, shouldRender);
        return true;
    }
    const relay = value.match(/\b(?:wss?:\/\/|nostr\+relay:\/\/)[^\s<>"']+/i)?.[0];
    if (relay) {
        addConnectivityPeer(contactPeerLabel("relay", relay), "relay", relay, shouldRender);
        return true;
    }
    const wormhole = value.match(/\b\d{1,4}-[a-z0-9]+(?:-[a-z0-9]+){1,}\b/i)?.[0];
    if (wormhole) {
        addConnectivityPeer(contactPeerLabel("wormhole", wormhole), "wormhole", wormhole, shouldRender);
        return true;
    }
    if (parseBip321Uri(value)) {
        addBip321FragmentFromText(value, shouldRender);
        return true;
    }
    const demoPsbt = parseDemoPsbtExport(value);
    if (demoPsbt) {
        addDemoPsbtFragmentFromExport(demoPsbt, value, shouldRender);
        return true;
    }
    if (looksLikeBase64Psbt(value)) {
        addPsbtFragmentFromRaw(`${source} PSBT`, compactBase64(value), "PSBT bytes imported from paste; this mock keeps only elided counts.", shouldRender);
        return true;
    }
    if (looksLikeDescriptor(value)) {
        const privacy = descriptorLooksPrivate(value) ? "private" : "public";
        addOutputDescriptor(`${source} descriptor`, value, shouldRender, privacy, privacy === "private" ? "mine" : "other");
        return true;
    }
    return false;
}
function parseDemoPsbtExport(value) {
    const compact = String(value || "").trim();
    if (!compact.startsWith("ptj-demo:"))
        return null;
    try {
        const parsed = JSON.parse(atob(compact.slice("ptj-demo:".length)));
        if (!["unordered", "bip370", "bip174"].includes(parsed.format))
            return null;
        return {
            label: parsed.label || "Sneakernet import",
            format: parsed.format,
            modifiable: parsed.modifiable || "both",
            inputs: Array.isArray(parsed.inputs) ? parsed.inputs : [],
            outputs: Array.isArray(parsed.outputs) ? parsed.outputs : [],
            descriptors: Array.isArray(parsed.descriptors) ? parsed.descriptors : [],
            conflicts: Array.isArray(parsed.conflicts) ? parsed.conflicts : [],
        };
    }
    catch {
        return null;
    }
}
function addDemoPsbtFragmentFromExport(parsed, raw, shouldRender = true) {
    const fragment = {
        id: nextId("frag"),
        type: "fragment",
        kind: "sneakernet",
        label: parsed.label,
        inputs: parsed.inputs,
        outputs: parsed.outputs,
        descriptors: parsed.descriptors,
        conflicts: parsed.conflicts,
        format: parsed.format,
        modifiable: parsed.modifiable,
        raw,
        sourceDescription: "Imported from a ptj demo sneakernet PSBT payload.",
    };
    state.fragments.push(fragment);
    state.selected = [fragment.id];
    state.log.unshift(`Imported sneakernet PSBT payload as ${fragment.id}.`);
    if (shouldRender)
        render();
    return fragment;
}
function addConnectivityPeer(label, sourceKind, sourceValue, shouldRender = true, verb = "Pasted") {
    const existing = state.peers.find((peer) => peer.sourceValue === sourceValue);
    if (existing) {
        state.selected = [existing.id];
        state.log.unshift(`Selected existing ${sourceKind} contact ${existing.label}.`);
        if (shouldRender)
            render();
        return existing;
    }
    const peer = addPeer(label, false);
    peer.sourceKind = sourceKind;
    peer.sourceValue = sourceValue;
    state.selected = [peer.id];
    state.log.unshift(`${verb} ${sourceKind} contact as ${peer.label}.`);
    if (shouldRender)
        render();
    return peer;
}
function relayHost(uri) {
    try {
        const normalized = uri.replace(/^nostr\+relay:/i, "wss:");
        return new URL(normalized).host || uri;
    }
    catch {
        return uri;
    }
}
function bytesToBase64(bytes) {
    let binary = "";
    for (let index = 0; index < bytes.length; index += 0x8000) {
        binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
    }
    return btoa(binary);
}
function secureRandomSessionSeed() {
    const bytes = new Uint8Array(16);
    crypto.getRandomValues(bytes);
    return seedFromRandomBytes(bytes);
}
function internalId(prefix, ordinal) {
    if (crypto.randomUUID)
        return `${prefix}-${crypto.randomUUID()}`;
    return `${prefix}-${String(ordinal).padStart(2, "0")}`;
}
function dummyPeerContact(kind) {
    const token = secureRandomSessionSeed();
    if (kind === "npub")
        return `npub1${token.slice(0, 48)}`;
    if (kind === "wormhole")
        return `7-${token.slice(0, 6)}-${token.slice(6, 12)}`;
    return `iroh:node-${token}`;
}
function addPeerContact(name, sourceKind, sourceValue, shouldRender = true) {
    const kind = sourceKind || "iroh";
    const value = String(sourceValue || "").trim() || dummyPeerContact(kind);
    const label = String(name || "").trim() || contactPeerLabel(kind, value);
    return addConnectivityPeer(label, kind, value, shouldRender, "Added");
}
function contactPeerLabel(kind, value) {
    if (kind === "npub")
        return `npub ${short(value, 14)}`;
    if (kind === "wormhole")
        return `wormhole ${short(value, 18)}`;
    if (kind === "relay")
        return `relay ${short(relayHost(value), 18)}`;
    return `iroh ${short(value, 18)}`;
}
function importDroppedFiles(files) {
    for (const file of files) {
        const reader = new FileReader();
        reader.addEventListener("load", () => {
            const bytes = new Uint8Array(reader.result || []);
            addPsbtFragmentFromRaw(file.name || "dropped PSBT", bytesToBase64(bytes), `Binary PSBT dropped from ${file.name || "a file"}; origin metadata is not serialized into the PSBT.`);
        });
        reader.readAsArrayBuffer(file);
    }
}
function addSession(orderingInput, shouldRender = true, payload = emptyPayload(), label = null) {
    counters.session = (counters.session || 0) + 1;
    const id = internalId("session", counters.session);
    const fallbackSeed = hashHex(`session:${counters.session}`);
    const ordering = typeof orderingInput === "string"
        ? normalizeSessionOrdering("det", orderingInput.trim() || fallbackSeed)
        : orderingInput;
    const session = {
        id,
        type: "session",
        label: label || `Session ${counters.session}`,
        sortMode: ordering.mode,
        seed: ordering.seed,
        inputs: [],
        outputs: [],
        descriptors: [],
        conflicts: [],
        peers: [],
        format: "unordered",
        raw: "",
    };
    applyPayload(session, mergePayloads(payload));
    state.sessions.push(session);
    connectLocalPeersToSession(session);
    state.log.unshift(`Created session PSBT ${session.label} with ${session.sortMode}${session.seed ? ` seed ${session.seed}` : ""}.`);
    if (shouldRender)
        render();
    return session;
}
function connectLocalPeersToSession(session) {
    for (const peer of localPeers()) {
        if (!session.peers.includes(peer.id)) {
            session.peers.push(peer.id);
            session.peers.sort();
        }
        addEdge(session.id, peer.id, "session-peer");
    }
    syncPeerViews(session);
}
function importPsbt({ label, raw, format, target, inputCount, outputCount, totalValueSats, descriptorPrivacy, descriptor, }, shouldRender = true) {
    const idPrefix = nextId("import");
    const importLabel = (label || "").trim();
    const importRaw = (raw || "").trim();
    const payload = {
        inputs: syntheticInputs(idPrefix, inputCount, Number(totalValueSats || 0), importLabel || "import"),
        outputs: syntheticOutputs(idPrefix, outputCount, Number(totalValueSats || 0)),
        descriptors: descriptorPayload(`descriptor:${idPrefix}`, descriptorPrivacy, descriptor || ""),
        conflicts: [],
    };
    if (target === "session" && format === "unordered") {
        const session = addSession(hashHex(idPrefix), false, payload, importLabel || `Imported ${idPrefix}`);
        session.raw = importRaw;
        state.selected = [session.id];
        state.log.unshift(`Imported unordered seed PSBT as ${session.label}.`);
        if (shouldRender)
            render();
        return session;
    }
    const fragment = {
        id: nextId("frag"),
        type: "fragment",
        kind: "import",
        label: importLabel || concatLabel("Imported", idPrefix),
        inputs: payload.inputs,
        outputs: payload.outputs,
        descriptors: payload.descriptors,
        conflicts: [],
        format,
        modifiable: format === "bip370" ? "both" : format === "bip174" ? "none" : undefined,
        raw: importRaw,
    };
    state.fragments.push(fragment);
    state.selected = [fragment.id];
    state.log.unshift(format === "bip370" && target === "session"
        ? `Imported BIP 370 seed as fragment ${fragment.id}; make it unordered before promoting to a session.`
        : `Imported ${psbtFormatLabel(fragment)} seed PSBT as fragment ${fragment.id}.`);
    if (shouldRender)
        render();
    return fragment;
}
function removeFragment(fragmentId, replacementId = null) {
    state.fragments = state.fragments.filter((fragment) => fragment.id !== fragmentId);
    for (const descriptor of state.descriptors) {
        if (descriptor.fragmentId === fragmentId) {
            descriptor.fragmentId = null;
            descriptor.absorbedInto = replacementId;
        }
    }
}
function removeSession(sessionId) {
    state.sessions = state.sessions.filter((session) => session.id !== sessionId);
    state.edges = state.edges.filter((edge) => edge.from !== sessionId && edge.to !== sessionId);
    clearSessionConvergence(sessionId);
    for (const peer of state.peers) {
        delete peer.views[sessionId];
    }
}
function syncPeerViews(session) {
    for (const peerId of session.peers) {
        const peer = byId(state.peers, peerId);
        if (peer)
            peer.views[session.id] = mergePayloads(session);
    }
}
function clearSessionConvergence(sessionId) {
    for (const [key, convergence] of Object.entries(state.convergence)) {
        if (convergence.sessionId === sessionId)
            delete state.convergence[key];
    }
}
function convergenceWireKey(sessionId, peerIds) {
    const peers = [...new Set(peerIds)].sort();
    return `${sessionId}->${peers.join("+")}`;
}
function convergencePeerGroups(peerIds) {
    const wanted = new Set(peerIds);
    return peerBridgeComponents()
        .map((group) => group.filter((peerId) => wanted.has(peerId)))
        .filter((group) => group.length);
}
function convergenceFor(nodeId) {
    if (nodeKind(nodeId) !== "session")
        return null;
    const entries = Object.values(state.convergence).filter((convergence) => convergence.sessionId === nodeId);
    const total = entries.reduce((sum, convergence) => sum + convergence.total, 0);
    if (!total)
        return null;
    return {
        acked: entries.reduce((sum, convergence) => sum + convergence.acked, 0),
        total,
        reason: entries.map((convergence) => convergence.reason).join("; "),
    };
}
function pendingPayloadRowsForNode(nodeId) {
    if (nodeKind(nodeId) !== "session")
        return new Set();
    return new Set(Object.values(state.convergence)
        .filter((convergence) => convergence.sessionId === nodeId)
        .flatMap((convergence) => convergence.pendingRows || []));
}
function wireConvergenceFor(sessionId, peerIds) {
    return state.convergence[convergenceWireKey(sessionId, peerIds)] || null;
}
function startConvergence(node, reason, peerIds = node?.peers || [], pendingRows = []) {
    if (!node || nodeKind(node.id) !== "session")
        return;
    const networkPeerIds = peerIds.filter((peerId) => !byId(state.peers, peerId)?.local);
    for (const group of convergencePeerGroups(networkPeerIds)) {
        const plan = peerAckPlan(group);
        if (!plan.total)
            continue;
        const key = convergenceWireKey(node.id, plan.peers);
        const runId = ++state.convergenceRun;
        state.convergence[key] = {
            key,
            sessionId: node.id,
            peerIds: plan.peers,
            ackedPeerIds: [],
            acked: 0,
            total: plan.total,
            reason,
            pendingRows,
            runId,
            completionDelayMs: plan.completionDelayMs,
        };
        for (const ack of plan.acks) {
            window.setTimeout(() => acknowledgeConvergence(key, runId, ack.peerId), ack.delayMs);
        }
    }
}
function acknowledgeConvergence(key, runId, peerId) {
    const convergence = state.convergence[key];
    if (!convergence || convergence.runId !== runId || !getNode(convergence.sessionId))
        return;
    if (!convergence.ackedPeerIds.includes(peerId)) {
        convergence.ackedPeerIds.push(peerId);
        convergence.acked = Math.min(convergence.ackedPeerIds.length, convergence.total);
    }
    render();
    if (convergence.acked >= convergence.total) {
        window.setTimeout(() => {
            const current = state.convergence[key];
            if (current?.runId === runId && current.acked >= current.total) {
                delete state.convergence[key];
                render();
            }
        }, 900);
    }
}
function promoteFragment(fragmentId, seed = "", shouldRender = true) {
    const fragment = byId(state.fragments, fragmentId);
    if (!fragment)
        return null;
    if (!ensureUnordered(fragment)) {
        if (shouldRender)
            render();
        return null;
    }
    const session = addSession(seed || hashHex(fragment.id), false, fragment, concatLabel(fragment.label, "session"));
    removeFragment(fragment.id, session.id);
    state.log.unshift(`Promoted fragment ${fragment.id} into ${session.label}.`);
    state.selected = [session.id];
    if (shouldRender)
        render();
    return session;
}
function abortSession(sessionId, shouldRender = true) {
    const session = byId(state.sessions, sessionId);
    if (!session)
        return null;
    const payload = mergePayloads(session);
    const fragment = {
        id: nextId("frag"),
        type: "fragment",
        kind: "aborted-session",
        label: concatLabel(session.label, "state"),
        inputs: payload.inputs,
        outputs: payload.outputs,
        descriptors: payload.descriptors,
        conflicts: payload.conflicts,
        format: session.format,
        raw: session.raw || "",
        sourceNodeId: session.id,
        sourceDescription: `${session.label} was aborted; peer read links were broken and this fragment preserves its current PSBT state.`,
    };
    removeSession(session.id);
    state.joinWires = state.joinWires.filter((wire) => wire.sourceId !== session.id && wire.targetId !== session.id);
    for (const descriptor of state.descriptors) {
        if (descriptor.absorbedInto === session.id)
            descriptor.absorbedInto = fragment.id;
    }
    state.fragments.push(fragment);
    state.selected = [fragment.id];
    state.log.unshift(`Aborted ${session.label}; preserved its state as fragment ${fragment.id}.`);
    if (shouldRender)
        render();
    return fragment;
}
function mergeFragments(leftId, rightId, shouldRender = true) {
    const left = byId(state.fragments, leftId);
    const right = byId(state.fragments, rightId);
    if (!left || !right || left.id === right.id)
        return null;
    if (!ensureUnordered(left, right)) {
        if (shouldRender)
            render();
        return null;
    }
    const payload = mergePayloads(left, right);
    const fragment = {
        id: nextId("frag"),
        type: "fragment",
        kind: "lub",
        label: concatLabel(left.label || left.id, right.label || right.id),
        inputs: payload.inputs,
        outputs: payload.outputs,
        descriptors: payload.descriptors,
        conflicts: payload.conflicts,
        format: "unordered",
        raw: "",
    };
    state.fragments.push(fragment);
    removeFragment(left.id, fragment.id);
    removeFragment(right.id, fragment.id);
    state.log.unshift(`Merged fragments ${left.id} and ${right.id}; replaced them with ${fragment.id}.`);
    void hydrateJoinedPsbt(fragment, [left, right]);
    state.selected = [fragment.id];
    if (shouldRender)
        render();
    return fragment;
}
function absorbFragmentIntoSession(fragmentId, sessionId, shouldRender = true) {
    const fragment = byId(state.fragments, fragmentId);
    const session = byId(state.sessions, sessionId);
    if (!fragment || !session)
        return null;
    if (!ensureUnordered(fragment, session)) {
        if (shouldRender)
            render();
        return null;
    }
    applyPayload(session, mergePayloads(session, fragment));
    removeFragment(fragment.id, session.id);
    syncPeerViews(session);
    startConvergence(session, `absorbing ${fragment.id}`, session.peers, pendingPayloadRowKeys(fragment));
    state.log.unshift(`Absorbed fragment ${fragment.id} into ${session.label}; fragment vertex was removed.`);
    void hydrateJoinedPsbt(session, [session, fragment]);
    state.selected = [session.id];
    if (shouldRender)
        render();
    return session;
}
function mergeSessions(leftId, rightId, shouldRender = true) {
    const left = byId(state.sessions, leftId);
    const right = byId(state.sessions, rightId);
    if (!left || !right || left.id === right.id)
        return null;
    if (!ensureUnordered(left, right)) {
        if (shouldRender)
            render();
        return null;
    }
    const peers = [...new Set([...left.peers, ...right.peers])].sort();
    const payload = mergePayloads(left, right);
    const session = addSession(joinSessionSeeds([left, right]), false, payload, `LUB ${left.id} ${right.id}`);
    session.peers = peers;
    removeSession(left.id);
    removeSession(right.id);
    for (const peerId of peers) {
        addEdge(session.id, peerId, "session-peer");
    }
    syncPeerViews(session);
    startConvergence(session, `merging ${left.id} and ${right.id}`);
    state.log.unshift(`Merged ${left.label} and ${right.label}; replaced them with ${session.label}.`);
    void hydrateJoinedPsbt(session, [left, right]);
    state.selected = [session.id];
    if (shouldRender)
        render();
    return session;
}
function bridgePeers(leftId, rightId, shouldRender = true) {
    const left = byId(state.peers, leftId);
    const right = byId(state.peers, rightId);
    if (!left || !right || left.id === right.id)
        return null;
    const bridgeGroup = peerBridgeGroupFor(left.id, right.id);
    const sessions = visibleSessionsForPeers(...bridgeGroup);
    if (sessions.length && !ensureUnordered(...sessions)) {
        if (shouldRender)
            render();
        return null;
    }
    addEdge(left.id, right.id, "peer-bridge");
    if (!sessions.length) {
        state.log.unshift(`Bridged ${left.label} and ${right.label}; no session data is visible yet.`);
        state.selected = [left.id];
        if (shouldRender)
            render();
        return left;
    }
    for (const session of sessions) {
        for (const peerId of bridgeGroup) {
            if (!session.peers.includes(peerId)) {
                session.peers.push(peerId);
                session.peers.sort();
            }
            addEdge(session.id, peerId, "session-peer");
        }
        syncPeerViews(session);
        startConvergence(session, `bridging ${left.label} and ${right.label}`, bridgeGroup);
    }
    state.log.unshift(`Bridged ${left.label} and ${right.label}; shared ${sessions.length} session${sessions.length === 1 ? "" : "s"} across ${bridgeGroup.map((peerId) => byId(state.peers, peerId)?.label || peerId).join(", ")} without merging them.`);
    state.selected = [left.id];
    if (shouldRender)
        render();
    return left;
}
function visibleSessionsForPeers(...peerIds) {
    return state.sessions.filter((session) => peerIds.some((peerId) => session.peers.includes(peerId) || Boolean(byId(state.peers, peerId)?.views[session.id])));
}
function peerBridgeGroupFor(leftId, rightId) {
    const key = joinWireKey(leftId, rightId);
    const edges = state.edges.some((edge) => edge.kind === "peer-bridge" && joinWireKey(edge.from, edge.to) === key)
        ? state.edges
        : [...state.edges, { from: leftId, to: rightId, kind: "peer-bridge" }];
    return modelPeerBridgeComponents(state.peers, edges).find((group) => group.includes(leftId)) || [leftId, rightId];
}
function peerBridgeGroupContaining(peerId) {
    return peerBridgeComponents().find((group) => group.includes(peerId)) || [peerId];
}
function convertSelectedToUnordered() {
    if (state.selected.length !== 1) {
        state.log.unshift("Select one BIP 370 PSBT to shuffle into unordered form.");
        render();
        return;
    }
    const node = getNode(state.selected[0]);
    if (!node || !["fragment", "session"].includes(nodeKind(node.id))) {
        state.log.unshift("Select a fragment or session PSBT to convert.");
        render();
        return;
    }
    if (node.format === "unordered") {
        state.log.unshift(`${node.id} is already unordered.`);
    }
    else {
        const previousRaw = node.raw;
        node.format = "unordered";
        node.convertedFrom = "bip370";
        node.modifiable = undefined;
        if (nodeKind(node.id) === "session")
            syncPeerViews(node);
        state.log.unshift(`Shuffled ${node.id} from BIP 370 order into unordered form for session sharing.`);
        void hydrateUnorderedPsbt(node, previousRaw);
    }
    render();
}
function sortSelectedPsbt() {
    if (state.selected.length !== 1) {
        state.log.unshift("Select one unordered PSBT to sort, or one session to fix its input/output sets.");
        render();
        return;
    }
    sortPsbtToBip370(state.selected[0]);
}
function sortPsbtToBip370(nodeId, shouldRender = true) {
    const node = getNode(nodeId);
    const kind = nodeKind(nodeId);
    if (!node || !["fragment", "session"].includes(kind))
        return null;
    if (node.format === "bip370") {
        state.log.unshift(`${node.id} is already a BIP 370 sorted PSBT.`);
        if (shouldRender)
            render();
        return node;
    }
    const payload = orderedProjectionPayload(node);
    const fragment = {
        id: nextId("frag"),
        type: "fragment",
        kind: "sorter-output",
        label: concatLabel("Sorted", node.label || node.id),
        inputs: payload.inputs,
        outputs: payload.outputs,
        descriptors: payload.descriptors,
        conflicts: payload.conflicts,
        format: "bip370",
        modifiable: "none",
        raw: "",
        sourceNodeId: node.id,
        sourceDescription: "Sorter output: BIP 370 transaction candidate with ordered inputs and outputs. The unordered session seed, read-capabilities, merge history, and CRDT register identity are not part of this artifact.",
    };
    state.fragments.push(fragment);
    state.selected = [fragment.id];
    state.log.unshift(kind === "session"
        ? `Fixed ${nodeDisplayLabel(node, kind)} input/output sets into an updater/signer PSBT candidate; the live session remains available.`
        : `Sorted ${nodeDisplayLabel(node, kind)} into a BIP 370 updater/signer PSBT candidate.`);
    void hydrateSortedFragment(fragment, node);
    if (shouldRender)
        render();
    return fragment;
}
function atomizeSelectedPsbt() {
    if (state.selected.length !== 1) {
        state.log.unshift("Select one PSBT to atomize.");
        render();
        return;
    }
    const node = getNode(state.selected[0]);
    const kind = nodeKind(state.selected[0]);
    if (!node || !["fragment", "session"].includes(kind)) {
        state.log.unshift("Select a fragment or session PSBT to atomize.");
        render();
        return;
    }
    if (kind === "session") {
        state.log.unshift(`Abort session ${node.id} before atomizing; atomizing a live register is not monotone.`);
        render();
        return;
    }
    const atoms = [];
    for (const input of node.inputs) {
        atoms.push({
            id: nextId("frag"),
            type: "fragment",
            kind: "spend",
            label: concatLabel(node.label || node.id, "input"),
            inputs: [input],
            outputs: [],
            descriptors: node.descriptors || [],
            conflicts: [],
            format: "unordered",
            raw: "",
        });
    }
    for (const output of node.outputs) {
        atoms.push({
            id: nextId("frag"),
            type: "fragment",
            kind: "fund",
            label: concatLabel(node.label || node.id, "output"),
            inputs: [],
            outputs: [output],
            descriptors: node.descriptors || [],
            conflicts: [],
            format: "unordered",
            raw: "",
        });
    }
    if (!atoms.length) {
        state.log.unshift(`${node.id} has no inputs or outputs to atomize.`);
        render();
        return;
    }
    if (atoms.length === 1) {
        state.log.unshift(`${node.id} is already an atomic PSBT.`);
        render();
        return;
    }
    const previousRaw = node.raw;
    removeFragment(node.id);
    state.fragments.push(...atoms);
    state.selected = atoms.slice(0, 1).map((atom) => atom.id);
    state.log.unshift(`Atomized ${node.id} into ${atoms.length} unordered atomic fragment PSBTs.`);
    void hydrateAtomizedFragments(atoms, previousRaw, node.id);
    render();
}
function abortSelectedSession() {
    if (state.selected.length !== 1 || nodeKind(state.selected[0]) !== "session") {
        state.log.unshift("Select one live session to abort.");
        render();
        return;
    }
    abortSession(state.selected[0]);
}
function shareSessionWithPeer(peerId, sessionId, shouldRender = true) {
    const peer = byId(state.peers, peerId);
    const session = byId(state.sessions, sessionId);
    if (!peer || !session)
        return null;
    if (!ensureUnordered(session)) {
        if (shouldRender)
            render();
        return null;
    }
    const readers = peerBridgeGroupContaining(peer.id);
    for (const readerId of readers) {
        if (!session.peers.includes(readerId)) {
            session.peers.push(readerId);
            session.peers.sort();
        }
        addEdge(session.id, readerId, "session-peer");
    }
    syncPeerViews(session);
    startConvergence(session, `sharing with ${peer.label}`, readers);
    state.log.unshift(`${session.label} is now readable by ${readers.map((readerId) => byId(state.peers, readerId)?.label || readerId).join(", ")}.`);
    if (shouldRender)
        render();
    return session;
}
function addEdge(from, to, kind) {
    if (kind === "peer-bridge" && from > to) {
        [from, to] = [to, from];
    }
    if (!state.edges.some((edge) => edge.from === from && edge.to === to && edge.kind === kind)) {
        state.edges.push({ from, to, kind });
    }
}
function nodeIsSelectable(id) {
    const node = getNode(id);
    return Boolean(node) && (nodeKind(id) !== "peer" || peerIsInteractive(node));
}
function selectOnly(id) {
    if (!nodeIsSelectable(id))
        return;
    state.selected = [id];
    render();
}
function toggleSelect(id) {
    if (!nodeIsSelectable(id))
        return;
    state.selected = state.selected[0] === id ? [] : [id];
    render();
}
function cancelJoinSelect() {
    state.joinWires = [];
    render();
}
function beginWireDraft(event, sourceId) {
    if (event.button !== 0 || !nodeIsSelectable(sourceId))
        return;
    const svg = document.querySelector("#graph");
    const pointer = svgPointFromEvent(svg, event);
    state.wireDraft = {
        sourceId,
        pointer,
        start: pointer,
        targetId: null,
        active: false,
    };
    try {
        event.currentTarget.setPointerCapture?.(event.pointerId);
    }
    catch {
        // Synthetic test events may not have an active browser pointer capture slot.
    }
}
function updateWireDraft(event) {
    if (!state.wireDraft)
        return;
    const svg = document.querySelector("#graph");
    const pointer = svgPointFromEvent(svg, event);
    const dx = pointer.x - state.wireDraft.start.x;
    const dy = pointer.y - state.wireDraft.start.y;
    const active = state.wireDraft.active || Math.hypot(dx, dy) > 6;
    state.wireDraft = {
        ...state.wireDraft,
        pointer,
        active,
        targetId: active ? nearestWireTargetNode(pointer, state.wireDraft.sourceId) : null,
    };
    if (active)
        renderGraph();
}
function finishWireDraft() {
    const draft = state.wireDraft;
    if (!draft)
        return;
    state.wireDraft = null;
    if (!draft.active) {
        return;
    }
    state.suppressNodeClick = true;
    const attempt = draft.targetId ? wireAttempt(draft.sourceId, draft.targetId) : null;
    if (attempt?.ok) {
        addJoinWire(draft.sourceId, draft.targetId);
        state.log.unshift(`${attempt.label} added to join-select; use Join on the edge or toolbar to apply.`);
    }
    else if (draft.targetId && attempt) {
        showWireFailure(draft.sourceId, draft.targetId, attempt.reason);
    }
    else {
        state.log.unshift(`Preview wire from ${draft.sourceId} cancelled.`);
    }
    render();
}
function addJoinWire(sourceId, targetId) {
    if (!hasCompatibleWireAction(sourceId, targetId))
        return false;
    const key = joinWireKey(sourceId, targetId);
    if (!state.joinWires.some((wire) => joinWireKey(wire.sourceId, wire.targetId) === key)) {
        state.joinWires.push({ sourceId, targetId });
    }
    return true;
}
function joinWireKey(leftId, rightId) {
    return [leftId, rightId].sort().join("::");
}
function cancelWireDraft() {
    if (!state.wireDraft)
        return;
    const wasActive = state.wireDraft.active;
    state.wireDraft = null;
    if (wasActive) {
        state.suppressNodeClick = true;
        renderGraph();
    }
}
function showWireFailure(sourceId, targetId, reason) {
    const failure = { sourceId, targetId, reason };
    state.wireFailure = failure;
    state.log.unshift(`Join failed: ${reason}.`);
    window.setTimeout(() => {
        if (state.wireFailure === failure) {
            state.wireFailure = null;
            renderGraph();
        }
    }, 1800);
}
function nearestWireTargetNode(pointer, sourceId) {
    const positions = layout(Number(document.querySelector("#graph").getAttribute("viewBox").split(" ")[2]));
    let nearest = null;
    let nearestDistance = Infinity;
    for (const [id, box] of positions.entries()) {
        if (!wireAttempt(sourceId, id))
            continue;
        const centerX = box.x + box.width / 2;
        const centerY = box.y + box.height / 2;
        const inside = pointer.x >= box.x - 12 &&
            pointer.x <= box.x + box.width + 12 &&
            pointer.y >= box.y - 12 &&
            pointer.y <= box.y + box.height + 12;
        const distance = Math.hypot(pointer.x - centerX, pointer.y - centerY);
        if ((inside || distance < 115) && distance < nearestDistance) {
            nearest = id;
            nearestDistance = distance;
        }
    }
    return nearest;
}
function hasCompatibleWireAction(sourceId, targetId) {
    return wireAttempt(sourceId, targetId)?.ok === true;
}
function wireActionLabel(sourceId, targetId) {
    const attempt = wireAttempt(sourceId, targetId);
    return attempt?.ok ? attempt.label : null;
}
function wireAttempt(sourceId, targetId) {
    if (!targetId || sourceId === targetId)
        return null;
    const sourceKind = nodeKind(sourceId);
    const targetKind = nodeKind(targetId);
    const source = getNode(sourceId);
    const target = getNode(targetId);
    if (!source || !target)
        return null;
    if ((sourceKind === "peer" && !peerIsInteractive(source)) || (targetKind === "peer" && !peerIsInteractive(target))) {
        return null;
    }
    if (sourceKind === "peer" && targetKind === "session") {
        if (sessionVisibleToPeerGroup(target, state.peers, peerBridgeGroupContaining(source.id)))
            return null;
        return { ok: true, label: `Share ${target.label} with ${source.label}` };
    }
    if (sourceKind === "session" && targetKind === "peer") {
        if (sessionVisibleToPeerGroup(source, state.peers, peerBridgeGroupContaining(target.id)))
            return null;
        return { ok: true, label: `Share ${source.label} with ${target.label}` };
    }
    if (sourceKind === "peer" && targetKind === "peer") {
        if (!peersCanBridge(source.id, target.id))
            return null;
        return { ok: true, label: `Bridge ${source.label} with ${target.label}` };
    }
    if (sourceKind === "fragment" && targetKind === "session") {
        return psbtWireAttempt(source, target, `Absorb ${source.label} into ${target.label}`);
    }
    if (sourceKind === "session" && targetKind === "fragment") {
        return psbtWireAttempt(source, target, `Absorb ${target.label} into ${source.label}`);
    }
    if (sourceKind === "session" && targetKind === "session") {
        return psbtWireAttempt(source, target, "merge sessions");
    }
    if (sourceKind === "fragment" && targetKind === "fragment") {
        return psbtWireAttempt(source, target, "Merge fragments");
    }
    return null;
}
function psbtWireAttempt(source, target, label) {
    const compatibility = psbtCompatibility(source, target);
    return compatibility.ok
        ? { ok: true, label }
        : { ok: false, label: `Cannot join: ${compatibility.reason}`, reason: compatibility.reason };
}
function peersCanBridge(leftId, rightId) {
    if (leftId === rightId)
        return false;
    const sessions = visibleSessionsForPeers(...peerBridgeGroupFor(leftId, rightId));
    return sessions.every((session) => session.format === "unordered");
}
function svgPointFromEvent(svg, event) {
    const rect = svg.getBoundingClientRect();
    const [minX, minY, width, height] = svg.getAttribute("viewBox").split(" ").map(Number);
    return {
        x: minX + ((event.clientX - rect.left) / rect.width) * width,
        y: minY + ((event.clientY - rect.top) / rect.height) * height,
    };
}
function connectSelected() {
    const wires = validJoinWires();
    if (!wires.length) {
        state.log.unshift("Draw one or more wires before joining.");
        render();
        return;
    }
    const remap = new Map();
    let applied = 0;
    for (const wire of wires) {
        const leftId = remapNodeId(wire.sourceId, remap);
        const rightId = remapNodeId(wire.targetId, remap);
        const result = applyWire(leftId, rightId);
        if (!result)
            continue;
        applied += 1;
        if (!getNode(leftId) && getNode(result.id))
            remap.set(leftId, result.id);
        if (!getNode(rightId) && getNode(result.id))
            remap.set(rightId, result.id);
    }
    state.joinWires = [];
    pruneSelection();
    state.log.unshift(applied
        ? `Joined ${applied} explicit wire${applied === 1 ? "" : "s"} across ${joinComponents(wires).length} join-select component${joinComponents(wires).length === 1 ? "" : "s"}.`
        : "No pending wires could be applied.");
    render();
}
function joinQueuedWire(sourceId, targetId) {
    const key = joinWireKey(sourceId, targetId);
    const wire = validJoinWires().find((candidate) => joinWireKey(candidate.sourceId, candidate.targetId) === key);
    if (!wire) {
        state.log.unshift("That queued wire is no longer joinable.");
        render();
        return;
    }
    const result = applyWire(wire.sourceId, wire.targetId);
    remapJoinWiresAfterJoin(wire, result);
    pruneSelection();
    state.log.unshift(result ? `Joined queued edge ${wire.sourceId} -> ${wire.targetId}.` : `Queued edge ${wire.sourceId} -> ${wire.targetId} could not be joined.`);
    render();
}
function remapJoinWiresAfterJoin(consumedWire, result) {
    const consumedKey = joinWireKey(consumedWire.sourceId, consumedWire.targetId);
    const remapped = [];
    for (const wire of state.joinWires) {
        if (joinWireKey(wire.sourceId, wire.targetId) === consumedKey)
            continue;
        let sourceId = wire.sourceId;
        let targetId = wire.targetId;
        if (result?.id && !getNode(sourceId) && (sourceId === consumedWire.sourceId || sourceId === consumedWire.targetId)) {
            sourceId = result.id;
        }
        if (result?.id && !getNode(targetId) && (targetId === consumedWire.sourceId || targetId === consumedWire.targetId)) {
            targetId = result.id;
        }
        if (!getNode(sourceId) || !getNode(targetId) || sourceId === targetId || !hasCompatibleWireAction(sourceId, targetId))
            continue;
        if (!remapped.some((candidate) => joinWireKey(candidate.sourceId, candidate.targetId) === joinWireKey(sourceId, targetId))) {
            remapped.push({ sourceId, targetId });
        }
    }
    state.joinWires = remapped;
}
function applyWire(leftId, rightId) {
    const kinds = [nodeKind(leftId), nodeKind(rightId)].sort().join("+");
    if (kinds === "fragment+fragment") {
        return mergeFragments(leftId, rightId, false);
    }
    else if (kinds === "fragment+session") {
        const fragmentId = nodeKind(leftId) === "fragment" ? leftId : rightId;
        const sessionId = nodeKind(leftId) === "session" ? leftId : rightId;
        return absorbFragmentIntoSession(fragmentId, sessionId, false);
    }
    else if (kinds === "peer+session") {
        const peerId = nodeKind(leftId) === "peer" ? leftId : rightId;
        const sessionId = nodeKind(leftId) === "session" ? leftId : rightId;
        return shareSessionWithPeer(peerId, sessionId, false);
    }
    else if (kinds === "peer+peer") {
        return bridgePeers(leftId, rightId, false) || { id: leftId };
    }
    else if (kinds === "session+session") {
        return mergeSessions(leftId, rightId, false);
    }
    state.log.unshift(`No LUB operation for ${nodeKind(leftId)} + ${nodeKind(rightId)}.`);
    return null;
}
function workspacePsbtNodes() {
    return [...state.sessions, ...state.fragments];
}
function makeAllWorkspaceUnordered() {
    const ordered = workspacePsbtNodes().filter((node) => node.format === "bip370");
    if (!ordered.length) {
        state.log.unshift("All workspace PSBTs are already unordered.");
        render();
        return;
    }
    for (const node of ordered) {
        node.format = "unordered";
        node.convertedFrom = "bip370";
        if (nodeKind(node.id) === "session")
            syncPeerViews(node);
    }
    state.log.unshift(`Shuffled ${ordered.length} workspace PSBT${ordered.length === 1 ? "" : "s"} into unordered form.`);
    render();
}
function remapNodeId(id, remap) {
    let current = id;
    const seen = new Set();
    while (remap.has(current) && !seen.has(current)) {
        seen.add(current);
        current = remap.get(current);
    }
    return current;
}
function validJoinWires() {
    state.joinWires = state.joinWires.filter((wire) => hasCompatibleWireAction(wire.sourceId, wire.targetId));
    return [...state.joinWires];
}
function joinComponents(wires = validJoinWires()) {
    const adjacency = new Map();
    for (const wire of wires) {
        if (!adjacency.has(wire.sourceId))
            adjacency.set(wire.sourceId, new Set());
        if (!adjacency.has(wire.targetId))
            adjacency.set(wire.targetId, new Set());
        adjacency.get(wire.sourceId).add(wire.targetId);
        adjacency.get(wire.targetId).add(wire.sourceId);
    }
    const components = [];
    const seen = new Set();
    for (const nodeId of adjacency.keys()) {
        if (seen.has(nodeId))
            continue;
        const stack = [nodeId];
        const nodes = new Set();
        while (stack.length) {
            const current = stack.pop();
            if (seen.has(current))
                continue;
            seen.add(current);
            nodes.add(current);
            for (const next of adjacency.get(current) || [])
                stack.push(next);
        }
        components.push({
            nodes,
            wires: wires.filter((wire) => nodes.has(wire.sourceId) && nodes.has(wire.targetId)),
        });
    }
    return components;
}
function pruneSelection() {
    state.selected = state.selected.filter((id) => nodeIsSelectable(id)).slice(0, 1);
}
function promoteSelected() {
    if (state.selected.length !== 1 || nodeKind(state.selected[0]) !== "fragment") {
        state.log.unshift("Select one fragment PSBT to promote.");
        render();
        return;
    }
    promoteFragment(state.selected[0]);
}
function renderPeerRail() {
    const rail = document.querySelector("#peerRail");
    if (!rail)
        return;
    rail.innerHTML = "";
    for (const peer of state.peers) {
        const pill = document.createElement("button");
        pill.className = "peer-pill";
        pill.type = "button";
        pill.addEventListener("click", () => toggleSelect(peer.id));
        pill.innerHTML = `
      <span class="peer-dot" style="background:${peer.color}"></span>
      <span class="peer-name">${escapeHtml(peer.label)}</span>
      <span class="peer-count">${Object.keys(peer.views).length} views</span>
    `;
        rail.appendChild(pill);
    }
}
function appendDescriptorMenu(parent, item) {
    const menu = document.createElement("div");
    menu.className = "descriptor-menu";
    const trigger = document.createElement("button");
    trigger.type = "button";
    trigger.className = "descriptor-menu-trigger";
    trigger.setAttribute("aria-label", `Descriptor actions for ${item.owner}`);
    trigger.setAttribute("aria-expanded", "false");
    trigger.title = "Descriptor actions";
    trigger.textContent = "☰";
    trigger.addEventListener("click", (event) => {
        event.preventDefault();
        const open = !menu.classList.contains("open");
        menu.classList.toggle("open", open);
        trigger.setAttribute("aria-expanded", String(open));
        if (open) {
            requestAnimationFrame(() => positionDescriptorMenu(trigger, panel));
        }
    });
    menu.appendChild(trigger);
    const panel = document.createElement("div");
    panel.className = "descriptor-menu-popover";
    const menuState = descriptorMenuState(item, descriptorPalette);
    const heading = document.createElement("div");
    heading.className = "descriptor-menu-heading";
    heading.textContent = "Descriptor";
    panel.appendChild(heading);
    for (const action of menuState.ownershipActions) {
        const button = document.createElement("button");
        button.type = "button";
        button.disabled = action.disabled;
        button.textContent = action.label;
        button.addEventListener("click", (event) => {
            event.preventDefault();
            menu.classList.remove("open");
            trigger.setAttribute("aria-expanded", "false");
            setDescriptorOwnership(item.id, action.id === "tag-mine" ? "mine" : "other");
        });
        panel.appendChild(button);
    }
    const colors = document.createElement("div");
    colors.className = "descriptor-color-menu";
    for (const choice of menuState.colorChoices) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = `descriptor-color-choice${choice.selected ? " selected" : ""}`;
        button.style.background = choice.color;
        button.title = `Use ${choice.color}`;
        button.setAttribute("aria-label", `Use descriptor color ${choice.color}`);
        button.addEventListener("click", (event) => {
            event.preventDefault();
            menu.classList.remove("open");
            trigger.setAttribute("aria-expanded", "false");
            setDescriptorColor(item.id, choice.color);
        });
        colors.appendChild(button);
    }
    panel.appendChild(colors);
    const uri = document.createElement("button");
    uri.type = "button";
    uri.textContent = menuState.paymentRequestAction.label;
    uri.title = "Copy a BIP 321 payment request URI generated from this descriptor.";
    uri.addEventListener("click", (event) => {
        event.preventDefault();
        menu.classList.remove("open");
        trigger.setAttribute("aria-expanded", "false");
        copyDescriptorUri(item.id);
    });
    panel.appendChild(uri);
    menu.appendChild(panel);
    parent.appendChild(menu);
}
function positionDescriptorMenu(summary, panel) {
    const anchor = summary.getBoundingClientRect();
    const panelWidth = panel.offsetWidth || 220;
    const panelHeight = panel.offsetHeight || 180;
    const left = Math.min(Math.max(8, anchor.right - panelWidth), Math.max(8, window.innerWidth - panelWidth - 8));
    let top = anchor.top - panelHeight - 6;
    if (top < 8)
        top = anchor.bottom + 6;
    panel.style.setProperty("--descriptor-menu-left", `${left}px`);
    panel.style.setProperty("--descriptor-menu-top", `${top}px`);
}
function descriptorDrawerSources() {
    const sources = [];
    for (const descriptor of state.descriptors) {
        if (!descriptor.hasUtxo)
            continue;
        sources.push({
            kind: "utxo",
            id: descriptor.id,
            descriptorId: descriptor.id,
            label: `${descriptor.owner} UTXO`,
            valueSats: descriptor.valueSats,
            promotedTo: descriptor.absorbedInto || descriptor.fragmentId || null,
        });
    }
    for (const request of allPaymentRequests()) {
        sources.push({
            kind: "payment-request",
            id: request.id,
            descriptorId: request.descriptorId || null,
            label: request.label,
            valueSats: request.valueSats,
            promotedTo: request.fragmentId || null,
            uri: request.uri,
        });
    }
    return sources;
}
function renderDescriptorDrawer(parent, descriptorId) {
    const items = descriptorDrawerItems(descriptorId, descriptorDrawerSources());
    const details = document.createElement("details");
    details.className = "descriptor-drawer";
    const summary = document.createElement("summary");
    const sourceCount = items.length;
    summary.textContent = `${sourceCount} source${sourceCount === 1 ? "" : "s"}`;
    details.appendChild(summary);
    const body = document.createElement("div");
    body.className = "descriptor-drawer-body";
    summary.addEventListener("click", (event) => {
        event.stopPropagation();
        positionDescriptorDrawer(summary, body);
    });
    if (!items.length) {
        const empty = document.createElement("div");
        empty.className = "descriptor-drawer-empty";
        empty.textContent = "No UTXOs or payment requests";
        body.appendChild(empty);
    }
    for (const item of items) {
        const row = document.createElement("div");
        row.className = `descriptor-source ${item.kind}`;
        const copy = document.createElement("div");
        copy.className = "descriptor-source-copy";
        const title = document.createElement("strong");
        title.textContent = item.label;
        const meta = document.createElement("span");
        meta.textContent = `${item.kind === "utxo" ? "UTXO" : "BIP 321"} · ${formatSatAmount(item.valueSats)}`;
        copy.appendChild(title);
        copy.appendChild(meta);
        row.appendChild(copy);
        const action = document.createElement("button");
        action.type = "button";
        action.textContent = item.promotedTo
            ? selectActionLabel(item.promotedTo)
            : item.kind === "utxo" ? "Create input" : "Create output";
        action.addEventListener("click", (event) => {
            event.preventDefault();
            event.stopPropagation();
            if (item.promotedTo) {
                selectOnly(item.promotedTo);
            }
            else if (item.kind === "utxo") {
                createSpendIntent(item.id);
            }
            else if (item.uri) {
                addBip321FragmentFromText(item.uri);
            }
        });
        row.appendChild(action);
        body.appendChild(row);
    }
    details.appendChild(body);
    details.addEventListener("toggle", () => {
        if (details.open) {
            requestAnimationFrame(() => positionDescriptorDrawer(summary, body));
        }
    });
    parent.appendChild(details);
}
function positionDescriptorDrawer(summary, body) {
    const anchor = summary.getBoundingClientRect();
    const width = Math.min(360, window.innerWidth - 16);
    const left = Math.min(Math.max(8, anchor.left), window.innerWidth - width - 8);
    const bottom = Math.max(8, window.innerHeight - anchor.top + 6);
    body.style.setProperty("--descriptor-drawer-left", `${left}px`);
    body.style.setProperty("--descriptor-drawer-bottom", `${bottom}px`);
    body.style.setProperty("--descriptor-drawer-width", `${width}px`);
}
function renderDescriptorChip(rail, item, options = {}) {
    const descriptorId = options.unrecognized ? null : item.id;
    const chip = document.createElement("article");
    chip.className = `descriptor-chip ${options.unrecognized ? "unrecognized" : descriptorIsMine(item) ? "mine" : "other"}${state.hoveredDescriptorId === item.id ? " active" : ""}`;
    chip.setAttribute("style", `border-color:${item.color}`);
    chip.addEventListener("mouseenter", () => setHoveredDescriptor(item.id));
    chip.addEventListener("mouseleave", () => setHoveredDescriptor(null));
    chip.innerHTML = `
    <span class="descriptor-color" style="background:${item.color}"></span>
    <span class="descriptor-copy">
      <strong>${escapeHtml(item.owner)}</strong>
      <span>${escapeHtml(short(item.descriptor, 34))}</span>
    </span>
    <span class="ownership-tag">${options.unrecognized ? "unrecognized" : descriptorIsMine(item) ? "mine" : "other"}</span>
  `;
    if (!options.unrecognized)
        appendDescriptorMenu(chip, item);
    renderDescriptorDrawer(chip, descriptorId);
    rail.appendChild(chip);
}
function renderDescriptorDock() {
    const rail = document.querySelector("#descriptorDockRail");
    if (!rail)
        return;
    rail.innerHTML = "";
    for (const item of state.descriptors) {
        renderDescriptorChip(rail, item);
    }
    renderDescriptorChip(rail, {
        id: UNRECOGNIZED_DESCRIPTOR_ID,
        owner: "Unrecognized",
        descriptor: "no descriptor data",
        color: "#39434d",
        privacy: "public",
        ownership: "other",
    }, { unrecognized: true });
}
function setHoveredDescriptor(descriptorId) {
    if (state.hoveredDescriptorId === descriptorId)
        return;
    state.hoveredDescriptorId = descriptorId;
    renderGraph();
}
function coinMatchesHoveredDescriptor(coin) {
    if (!state.hoveredDescriptorId)
        return false;
    if (state.hoveredDescriptorId === UNRECOGNIZED_DESCRIPTOR_ID)
        return !coin.descriptorId;
    return coin.descriptorId === state.hoveredDescriptorId;
}
function coinDimmedByHoveredDescriptor(coin) {
    if (!state.hoveredDescriptorId)
        return false;
    if (state.hoveredDescriptorId === UNRECOGNIZED_DESCRIPTOR_ID)
        return Boolean(coin.descriptorId);
    return coin.descriptorId !== state.hoveredDescriptorId;
}
function renderFragmentList() {
    const list = document.querySelector("#fragmentList");
    if (!list)
        return;
    list.innerHTML = "";
    for (const fragment of state.fragments) {
        const row = document.createElement("article");
        row.className = "list-item draft-fragment-item";
        row.innerHTML = `
      <div class="item-title">${escapeHtml(fragment.label)}</div>
      <div class="item-meta">${psbtFormatLabel(fragment)} fragment ${fragment.id}: ${payloadSummary(fragment)}</div>
      <div class="item-meta">${descriptorSummary(fragment)}</div>
      ${fragment.sourceDescription ? `<div class="item-meta">${escapeHtml(fragment.sourceDescription)}</div>` : ""}
      <span class="intent-state free">not signed or broadcast</span>
    `;
        const select = document.createElement("button");
        select.type = "button";
        select.textContent = "Select";
        select.addEventListener("click", () => selectOnly(fragment.id));
        const promote = document.createElement("button");
        promote.type = "button";
        promote.textContent = "Promote";
        promote.addEventListener("click", () => promoteFragment(fragment.id));
        row.appendChild(select);
        row.appendChild(promote);
        list.appendChild(row);
    }
}
function renderSessionList() {
    const list = document.querySelector("#sessionList");
    if (!list)
        return;
    list.innerHTML = "";
    for (const session of state.sessions) {
        const row = document.createElement("button");
        row.type = "button";
        row.className = "list-item";
        row.addEventListener("click", () => toggleSelect(session.id));
        row.innerHTML = `
      <span class="item-title">${escapeHtml(session.label)}</span>
      <span class="item-meta">${psbtFormatLabel(session)} CRDT register: ${payloadSummary(session)}</span>
      <span class="item-meta">${escapeHtml(sessionOrderingSummary(session))} · ${session.peers.length} readers</span>
      <span class="item-meta">${descriptorSummary(session)}</span>
    `;
        list.appendChild(row);
    }
}
function sessionOrderingSummary(session) {
    return session.seed ? `${session.sortMode || "det"} ${session.seed}` : `${session.sortMode || "unset"} ordering`;
}
function renderSelectionSummary() {
    const summary = document.querySelector("#selectionSummary");
    const wires = validJoinWires();
    const components = joinComponents(wires);
    if (!state.selected.length && !wires.length) {
        summary.textContent = state.peers.length
            ? "Click one vertex for unary PSBT actions. Drag wires to build an explicit join-select graph."
            : "Peerless workspace: import PSBT fragments, click one for unary actions, or wire PSBTs and press Join.";
        return;
    }
    if (wires.length) {
        const selected = getNode(state.selected[0]);
        const selectedText = selected ? ` Unary: ${nodeDisplayLabel(selected)} (${nodeKind(selected.id)}).` : "";
        summary.textContent = `${wires.length} pending wire${wires.length === 1 ? "" : "s"} in ${components.length} component${components.length === 1 ? "" : "s"}; press Join on an edge or in the toolbar to apply.${selectedText}`;
        return;
    }
    summary.textContent = state.selected
        .map((id) => {
        const selected = getNode(id);
        return `${nodeDisplayLabel(selected)} (${nodeKind(id)})`;
    })
        .join(" + ");
}
function renderFeeContributionPanel() {
    const panel = document.querySelector("#feeContributionPanel");
    if (!panel)
        return;
    const plan = feeContributionPlan();
    if (!state.feeDraft || !plan) {
        panel.hidden = true;
        panel.className = "fee-panel warning-none";
        return;
    }
    const context = document.querySelector("#feeContributionContext");
    const slider = document.querySelector("#feeContributionSlider");
    const amount = document.querySelector("#feeContributionAmount");
    const rate = document.querySelector("#feeContributionRate");
    const comparison = document.querySelector("#feeContributionComparison");
    const warning = document.querySelector("#feeContributionWarning");
    const confirmRow = document.querySelector("#feeContributionConfirmRow");
    const confirm = document.querySelector("#feeContributionConfirm");
    const apply = document.querySelector("#feeContributionApply");
    panel.hidden = false;
    panel.className = `fee-panel warning-${plan.warningLevel}`;
    context.textContent = `${plan.descriptorLabel}: choose additional explicit fee from ${formatSatAmount(0)} to ${formatSatAmount(plan.availableSats)}.`;
    slider.min = "0";
    slider.max = String(plan.availableSats);
    slider.step = "1";
    slider.value = String(plan.selectedSats);
    amount.min = "0";
    amount.max = String(plan.availableSats);
    amount.step = "1";
    amount.value = String(plan.selectedSats);
    rate.textContent = `${formatFeeRate(plan.feeRateSatsPerVbyte)} sat/vB`;
    comparison.textContent = `overall ${formatFeeRate(plan.averageFeeRateSatsPerVbyte)} sat/vB · ${formatFeeRatio(plan.relativeFeeRateRatio)} overall`;
    warning.textContent = feeContributionWarningText(plan);
    confirmRow.hidden = !plan.confirmationRequired;
    confirm.checked = Boolean(state.feeDraft.confirmed);
    apply.disabled = plan.confirmationRequired && !state.feeDraft.confirmed;
}
function formatFeeRatio(value) {
    return Number.isFinite(value) ? `${value.toFixed(value >= 10 ? 1 : 2)}x` : "∞x";
}
function feeContributionWarningText(plan) {
    if (plan.warningLevel === "confirm") {
        return "Mandatory confirmation: this fee is above 1000 sat/vB or 10x the overall transaction feerate.";
    }
    if (plan.warningLevel === "red") {
        return "High fee warning: this fee is above 100 sat/vB or 2x the overall transaction feerate.";
    }
    if (plan.warningLevel === "yellow") {
        return "Elevated fee warning: this fee is above 10 sat/vB or 1.1x the overall transaction feerate.";
    }
    return "No elevated fee warning for this explicit contribution.";
}
function renderInspector() {
    const inspector = document.querySelector("#inspector");
    if (!state.selected.length) {
        inspector.innerHTML = "<p class=\"item-meta\">No vertex selected.</p>";
        return;
    }
    const node = getNode(state.selected[state.selected.length - 1]);
    if (!node) {
        inspector.innerHTML = "<p class=\"item-meta\">Selected vertex was replaced by a LUB.</p>";
        return;
    }
    const kind = nodeKind(node.id);
    const rows = [["type", kind], ["label", node.label || "unlabeled"]];
    if (kind === "peer") {
        rows.push(["read caps", labelsForIds(Object.keys(node.views), "session").join(", ") || "none"]);
    }
    else {
        const balance = transactionBalance(node);
        const role = psbtRoleFor(node, kind);
        const identity = psbtProtocolIdentity(node, kind === "session" ? "session" : "fragment");
        rows.push(["format", psbtFormatLabel(node)]);
        rows.push([identity.label, identity.value]);
        rows.push(["identity source", identity.source]);
        rows.push(["typestate", role.label]);
        rows.push(["protocol", role.spec]);
        rows.push(["roles", role.roles.join(", ")]);
        rows.push(["summary", payloadSummary(node)]);
        rows.push(["fee", feeSumText(balance)]);
        rows.push(["mine", `${bucketBalanceText("mine", balance.mine)}; ${formatSatAmount(balance.mine.inputs)} in / ${formatSatAmount(balance.mine.outputs)} out`]);
        rows.push(["other", `${bucketBalanceText("other", balance.other)}; ${formatSatAmount(balance.other.inputs)} in / ${formatSatAmount(balance.other.outputs)} out`]);
        if (convergenceFor(node.id)) {
            const convergence = convergenceFor(node.id);
            rows.push(["replicas", `${convergence.acked}/${convergence.total} acked; waiting for matching LUB`]);
        }
        rows.push(["known descriptors", descriptorSummary(node)]);
        rows.push(["status", node.conflicts.length ? "conflict" : "ok"]);
        if (node.sourceDescription)
            rows.push(["origin", node.sourceDescription]);
    }
    if (kind === "session") {
        rows.push(["sort mode", node.sortMode || "det"]);
        if (node.seed)
            rows.push(["seed", node.seed]);
        rows.push(["register", "unordered PSBT CRDT"]);
        rows.push(["readers", labelsForIds(node.peers, "peer").join(", ") || "none"]);
    }
    inspector.innerHTML = `<dl>${rows.map(([key, value]) => `
    <dt>${escapeHtml(key)}</dt><dd>${escapeHtml(value)}</dd>
  `).join("")}</dl>`;
}
function labelsForIds(ids, kind) {
    return ids
        .map((id) => kind === "peer" ? byId(state.peers, id) : byId(state.sessions, id))
        .filter(Boolean)
        .map((node) => nodeDisplayLabel(node, kind));
}
function renderLog() {
    const log = document.querySelector("#eventLog");
    log.innerHTML = "";
    for (const entry of state.log.slice(0, 14)) {
        const item = document.createElement("li");
        item.textContent = entry;
        log.appendChild(item);
    }
}
function renderActionVisibility() {
    const selectedId = state.selected[0];
    const selectedKind = selectedId ? nodeKind(selectedId) : "unknown";
    const selectedNode = selectedId ? getNode(selectedId) : null;
    const unaryPsbt = ["fragment", "session"].includes(selectedKind);
    const unaryActions = unaryPsbt && selectedNode
        ? psbtUnaryActions(selectedNode, selectedKind === "session" ? "session" : "fragment")
        : [];
    document.querySelector("#connectSelected").hidden = validJoinWires().length === 0;
    document.querySelector("#convertSelected").hidden = !unaryActions.includes("make-unordered");
    const orderButton = document.querySelector("#orderSelected");
    const fixesSessionSets = unaryActions.includes("fix-sets");
    orderButton.hidden = !(unaryActions.includes("sort") || fixesSessionSets);
    orderButton.textContent = fixesSessionSets ? "Fix sets" : "Sort";
    orderButton.title = fixesSessionSets
        ? "Fix the session input/output sets and ordering into an updater/signer PSBT candidate."
        : "Run the sorter role and emit a BIP 370 transaction candidate.";
    document.querySelector("#atomizeSelected").hidden = !unaryActions.includes("atomize");
    document.querySelector("#abortSession").hidden = !unaryActions.includes("abort-session");
    document.querySelector("#promoteSelected").hidden = !unaryActions.includes("promote");
    document.querySelector("#cancelJoinSelect").hidden = validJoinWires().length === 0;
}
function renderGraph() {
    const svg = document.querySelector("#graph");
    const metrics = graphLayoutMetrics();
    const width = Math.max(760, svg.clientWidth || 960, requiredGraphWidth(metrics));
    const height = Math.max(640, svg.clientHeight || 660, metrics.graphHeight);
    const positions = layout(width, metrics);
    svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
    svg.innerHTML = "";
    drawRowLabel(svg, 24, "Remote peers");
    drawRowLabel(svg, metrics.sessionLabelY, "Sessions");
    drawPeerBridgeBlocks(svg, positions);
    for (const peer of localPeers()) {
        drawPeerNode(svg, positions.get(peer.id), peer);
    }
    const drawnSessionPeerWires = new Set();
    for (const edge of state.edges) {
        if (edge.kind !== "session-peer")
            continue;
        const peerIds = peerBridgeGroupContaining(edge.to)
            .filter((peerId) => state.edges.some((candidate) => candidate.kind === "session-peer" &&
            candidate.from === edge.from &&
            candidate.to === peerId));
        const wireKey = convergenceWireKey(edge.from, peerIds);
        if (drawnSessionPeerWires.has(wireKey))
            continue;
        drawnSessionPeerWires.add(wireKey);
        const from = positions.get(edge.from);
        const to = peerEdgeTermination(edge.to, positions);
        if (!from || !to)
            continue;
        const convergence = wireConvergenceFor(edge.from, peerIds);
        svg.appendChild(svgEl("path", {
            class: `edge session-peer${convergence ? " convergence-edge" : ""}`,
            d: curvePath(from, to),
        }));
        if (convergence)
            drawEdgeSpinner(svg, from, to, convergence);
    }
    drawJoinWires(svg, positions);
    drawFailedWire(svg, positions);
    drawWireDraft(svg, positions);
    for (const peer of remotePeers()) {
        drawPeerNode(svg, positions.get(peer.id), peer);
    }
    for (const session of state.sessions) {
        drawPsbtNode(svg, positions.get(session.id), session, "session");
    }
    for (const fragment of state.fragments) {
        drawPsbtNode(svg, positions.get(fragment.id), fragment, "fragment");
    }
}
function drawJoinWires(svg, positions) {
    for (const wire of validJoinWires()) {
        const source = positions.get(wire.sourceId);
        const target = positions.get(wire.targetId);
        if (!source || !target)
            continue;
        svg.appendChild(svgEl("path", {
            class: "pending-wire",
            d: snappedCurvePath(source, target, wire.sourceId, wire.targetId),
        }));
        drawQueuedWireJoinButton(svg, source, target, wire);
    }
}
function drawFailedWire(svg, positions) {
    const failure = state.wireFailure;
    if (!failure)
        return;
    const source = positions.get(failure.sourceId);
    const target = positions.get(failure.targetId);
    if (!source || !target)
        return;
    svg.appendChild(svgEl("path", {
        class: "wire-failed",
        d: snappedCurvePath(source, target, failure.sourceId, failure.targetId),
    }));
    drawWireTooltip(svg, target, `Cannot join: ${failure.reason}`, "invalid");
}
function requiredGraphWidth(metrics = graphLayoutMetrics()) {
    return Math.max(peerBlocksWidth(peerBridgeComponents(remotePeers())), sizedRowWidth(state.sessions, "session"), sizedRowWidth(state.fragments, "fragment", graphMetrics.fragment.gap), metrics.localPeerMinWidth);
}
function peerBlocksWidth(groups) {
    return 72 + peerGroupWidths(groups).reduce((sum, width) => sum + width, 0) + Math.max(2, groups.length + 1) * 36;
}
function peerBridgeComponents(peers = state.peers) {
    return modelPeerBridgeComponents(peers, state.edges);
}
function rowWidth(count, nodeWidth, minGap = 36) {
    return 72 + Math.max(1, count) * nodeWidth + Math.max(2, count + 1) * minGap;
}
function sizedRowWidth(items, kind, minGap = 36) {
    if (!items.length)
        return rowWidth(0, graphMetrics[kind].width, minGap);
    const nodeWidths = items.map((item) => psbtNodeSize(item, kind).width);
    return 72 + nodeWidths.reduce((sum, nodeWidth) => sum + nodeWidth, 0) + Math.max(2, items.length + 1) * minGap;
}
function graphLayoutMetrics() {
    const sessionMaxHeight = maxPsbtNodeHeight(state.sessions, "session");
    const fragmentMaxHeight = maxPsbtNodeHeight(state.fragments, "fragment");
    const localPeerY = graphMetrics.session.y + sessionMaxHeight + 62;
    const fragmentLabelY = localPeerY + 104;
    const fragmentY = localPeerY + 128;
    const localPeerHeight = Math.max(graphMetrics.localPeer.height, fragmentY + fragmentMaxHeight + 28 - localPeerY);
    return {
        sessionLabelY: graphMetrics.session.labelY,
        sessionY: graphMetrics.session.y,
        localPeerY,
        localPeerHeight,
        fragmentLabelY,
        fragmentY,
        graphHeight: localPeerY + localPeerHeight + 36,
        localPeerMinWidth: graphMetrics.localPeer.inset * 2 + Math.max(320, sizedRowWidth(state.fragments, "fragment", graphMetrics.fragment.gap) - 72),
    };
}
function maxPsbtNodeHeight(items, kind) {
    return Math.max(graphMetrics[kind].height, ...items.map((item) => psbtNodeSize(item, kind).height));
}
function psbtNodeSize(node, kind) {
    const base = graphMetrics[kind];
    const expandedRows = expandedCoinRowsForNode(node);
    if (!expandedRows.length)
        return { width: base.width, height: base.height };
    const detailHeight = expandedRows.reduce((sum, row) => sum + Math.max(0, coinRowHeight(row.side, row.item, row.index) - 34), 0);
    return {
        width: base.width + 220,
        height: base.height + Math.max(96, detailHeight + 64),
    };
}
function expandedCoinRowsForNode(node) {
    const rows = [];
    (node.inputs || []).forEach((item, index) => {
        if (isCoinRowExpanded("input", item))
            rows.push({ side: "input", item, index });
    });
    (node.outputs || []).forEach((item, index) => {
        if (isCoinRowExpanded("output", item))
            rows.push({ side: "output", item, index });
    });
    return rows;
}
function layout(width, metrics = graphLayoutMetrics()) {
    const positions = new Map();
    spreadPeerGroups(peerBridgeComponents(remotePeers()), positions, width, graphMetrics.peer.y);
    spreadSized(state.sessions, metrics.sessionY, "session", positions, width);
    positionLocalPeers(localPeers(), positions, width, metrics.localPeerY, metrics.localPeerHeight);
    spreadSized(state.fragments, metrics.fragmentY, "fragment", positions, width, graphMetrics.fragment.gap);
    return positions;
}
function positionLocalPeers(peers, positions, width, y, height) {
    if (!peers.length)
        return;
    const frameWidth = Math.max(320, width - graphMetrics.localPeer.inset * 2);
    peers.forEach((peer) => {
        positions.set(peer.id, {
            x: graphMetrics.localPeer.inset,
            y,
            width: frameWidth,
            height,
        });
    });
}
function spreadSized(items, y, kind, positions, width, minGap = 36) {
    const sizes = items.map((item) => psbtNodeSize(item, kind));
    const totalWidth = sizes.reduce((sum, size) => sum + size.width, 0);
    const gap = Math.max(minGap, (width - 72 - totalWidth) / Math.max(1, items.length + 1));
    let x = 36 + gap;
    items.forEach((item, index) => {
        const size = sizes[index];
        positions.set(item.id, {
            x,
            y,
            width: size.width,
            height: size.height,
        });
        x += size.width + gap;
    });
}
function spreadPeerGroups(groups, positions, width, y) {
    const widths = peerGroupWidths(groups);
    const totalWidth = widths.reduce((sum, groupWidth) => sum + groupWidth, 0);
    const gap = Math.max(36, (width - 72 - totalWidth) / Math.max(1, groups.length + 1));
    let x = 36 + gap;
    groups.forEach((group, groupIndex) => {
        group.forEach((peerId, peerIndex) => {
            positions.set(peerId, {
                x: x + peerIndex * (graphMetrics.peer.width + peerInnerGap(group)),
                y,
                width: graphMetrics.peer.width,
                height: graphMetrics.peer.height,
            });
        });
        x += widths[groupIndex] + gap;
    });
}
function peerGroupWidths(groups) {
    return groups.map((group) => group.length * graphMetrics.peer.width + Math.max(0, group.length - 1) * peerInnerGap(group));
}
function peerInnerGap(group) {
    return group.length > 1 ? 6 : 0;
}
function drawPeerBridgeBlocks(svg, positions) {
    for (const group of peerBridgeComponents()) {
        if (group.length < 2)
            continue;
        const bounds = peerGroupBounds(group, positions);
        if (!bounds)
            continue;
        svg.appendChild(svgEl("rect", {
            class: "peer-bridge-block",
            x: bounds.x - 8,
            y: bounds.y - 8,
            width: bounds.width + 16,
            height: bounds.height + 16,
        }));
        svg.appendChild(svgEl("circle", {
            class: "peer-bridge-port",
            cx: bounds.x + bounds.width / 2,
            cy: bounds.y + bounds.height + 8,
            r: 5,
        }));
    }
}
function peerEdgeTermination(peerId, positions) {
    return modelPeerEdgeTermination(peerId, peerBridgeComponents(), positions);
}
function spread(items, y, nodeWidth, nodeHeight, positions, width, minGap = 36) {
    const gap = Math.max(minGap, (width - 72 - items.length * nodeWidth) / Math.max(1, items.length + 1));
    items.forEach((item, index) => {
        positions.set(item.id, {
            x: 36 + gap + index * (nodeWidth + gap),
            y,
            width: nodeWidth,
            height: nodeHeight,
        });
    });
}
function curvePath(from, to) {
    const fromIsPeer = from.y < to.y;
    const startX = from.x + from.width / 2;
    const startY = fromIsPeer ? from.y + from.height : from.y;
    const endX = to.x + to.width / 2;
    const endY = fromIsPeer ? to.y : to.y + to.height;
    const midY = (startY + endY) / 2;
    return `M ${startX} ${startY} C ${startX} ${midY}, ${endX} ${midY}, ${endX} ${endY}`;
}
function edgeMidpoint(from, to) {
    const fromIsPeer = from.y < to.y;
    const startX = from.x + from.width / 2;
    const startY = fromIsPeer ? from.y + from.height : from.y;
    const endX = to.x + to.width / 2;
    const endY = fromIsPeer ? to.y : to.y + to.height;
    return {
        x: (startX + endX) / 2,
        y: (startY + endY) / 2,
    };
}
function drawEdgeSpinner(svg, from, to, convergence) {
    const point = edgeMidpoint(from, to);
    const group = svgEl("g", {
        class: "edge-spinner",
        transform: `translate(${point.x} ${point.y})`,
    });
    group.appendChild(svgEl("circle", { class: "edge-spinner-track", r: 8 }));
    const head = svgEl("g", { class: "edge-spinner-head-orbit" });
    head.appendChild(svgEl("animateTransform", {
        attributeName: "transform",
        type: "rotate",
        from: "0 0 0",
        to: "360 0 0",
        dur: "0.9s",
        repeatCount: "indefinite",
    }));
    head.appendChild(svgEl("path", {
        class: "edge-spinner-head",
        d: "M 0 -8 A 8 8 0 0 1 8 0",
    }));
    group.appendChild(head);
    group.appendChild(svgText(12, 4, `${convergence.acked}/${convergence.total}`, "edge-spinner-count"));
    svg.appendChild(group);
}
function wireEndpoint(box, toward) {
    const centerX = box.x + box.width / 2;
    const centerY = box.y + box.height / 2;
    if (!toward)
        return { x: centerX, y: centerY };
    const dx = toward.x - centerX;
    const dy = toward.y - centerY;
    if (Math.abs(dy) > Math.abs(dx)) {
        return {
            x: centerX,
            y: dy >= 0 ? box.y + box.height : box.y,
        };
    }
    return {
        x: dx >= 0 ? box.x + box.width : box.x,
        y: centerY,
    };
}
function verticalWireEndpoint(box, toward) {
    const centerX = box.x + box.width / 2;
    const centerY = box.y + box.height / 2;
    if (!toward)
        return { x: centerX, y: centerY };
    return {
        x: centerX,
        y: toward.y < centerY ? box.y : box.y + box.height,
    };
}
function draftCurvePath(sourceBox, pointer) {
    const start = wireEndpoint(sourceBox, pointer);
    const dx = Math.abs(pointer.x - start.x);
    const bend = Math.max(70, dx * 0.45);
    return `M ${start.x} ${start.y} C ${start.x} ${start.y + bend}, ${pointer.x} ${pointer.y - bend}, ${pointer.x} ${pointer.y}`;
}
function isPeerSessionWire(sourceId, targetId) {
    const kinds = [nodeKind(sourceId), nodeKind(targetId)].sort().join("+");
    return kinds === "peer+session";
}
function snappedWirePoints(sourceBox, targetBox, sourceId = null, targetId = null) {
    const targetCenter = boxCenter(targetBox);
    const sourceCenter = boxCenter(sourceBox);
    if (sourceId && targetId && isPeerSessionWire(sourceId, targetId)) {
        return {
            start: verticalWireEndpoint(sourceBox, targetCenter),
            end: verticalWireEndpoint(targetBox, sourceCenter),
        };
    }
    const start = wireEndpoint(sourceBox, targetCenter);
    const end = wireEndpoint(targetBox, start);
    return { start, end };
}
function snappedCurvePath(sourceBox, targetBox, sourceId = null, targetId = null) {
    const { start, end } = snappedWirePoints(sourceBox, targetBox, sourceId, targetId);
    const midY = (start.y + end.y) / 2;
    return `M ${start.x} ${start.y} C ${start.x} ${midY}, ${end.x} ${midY}, ${end.x} ${end.y}`;
}
function boxCenter(box) {
    return {
        x: box.x + box.width / 2,
        y: box.y + box.height / 2,
    };
}
function drawWireDraft(svg, positions) {
    const draft = state.wireDraft;
    if (!draft)
        return;
    const source = positions.get(draft.sourceId);
    if (!source)
        return;
    for (const [id, box] of positions.entries()) {
        if (id === draft.sourceId)
            continue;
        const attempt = wireAttempt(draft.sourceId, id);
        if (!attempt)
            continue;
        svg.appendChild(svgEl("rect", {
            class: `wire-halo${attempt.ok ? "" : " invalid"}${draft.targetId === id ? " target" : ""}`,
            x: box.x - 8,
            y: box.y - 8,
            width: box.width + 16,
            height: box.height + 16,
        }));
    }
    const target = draft.targetId ? positions.get(draft.targetId) : null;
    const targetAttempt = draft.targetId ? wireAttempt(draft.sourceId, draft.targetId) : null;
    const endpoint = target ? snappedWirePoints(source, target, draft.sourceId, draft.targetId).end : draft.pointer;
    svg.appendChild(svgEl("path", {
        class: `wire-draft${draft.targetId ? " target" : ""}${targetAttempt && !targetAttempt.ok ? " invalid" : ""}`,
        d: target ? snappedCurvePath(source, target, draft.sourceId, draft.targetId) : draftCurvePath(source, draft.pointer),
    }));
    svg.appendChild(svgEl("circle", {
        class: `wire-port${targetAttempt && !targetAttempt.ok ? " invalid" : ""}`,
        cx: endpoint.x,
        cy: endpoint.y,
        r: 5,
    }));
    if (target && targetAttempt)
        drawWireTooltip(svg, target, targetAttempt.label, targetAttempt.ok ? "" : "invalid");
}
function drawQueuedWireJoinButton(svg, sourceBox, targetBox, wire) {
    const { start, end } = snappedWirePoints(sourceBox, targetBox, wire.sourceId, wire.targetId);
    const x = (start.x + end.x) / 2 - 24;
    const y = (start.y + end.y) / 2 - 13;
    const group = svgEl("g", {
        class: "wire-join-button",
        transform: `translate(${x} ${y})`,
        tabindex: "0",
        role: "button",
        "aria-label": `Join ${wire.sourceId} to ${wire.targetId}`,
    });
    group.appendChild(svgEl("rect", { width: 48, height: 26, rx: 13 }));
    const text = svgText(24, 17, "Join");
    text.setAttribute("text-anchor", "middle");
    group.appendChild(text);
    group.addEventListener("pointerdown", (event) => event.stopPropagation());
    group.addEventListener("click", (event) => {
        event.stopPropagation();
        joinQueuedWire(wire.sourceId, wire.targetId);
    });
    group.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            event.stopPropagation();
            joinQueuedWire(wire.sourceId, wire.targetId);
        }
    });
    svg.appendChild(group);
}
function drawWireTooltip(svg, targetBox, label, variant = "") {
    if (!label)
        return;
    const width = Math.min(420, Math.max(118, label.length * 6.4 + 22));
    const x = Math.max(12, targetBox.x + targetBox.width / 2 - width / 2);
    const y = Math.max(34, targetBox.y - 42);
    const group = svgEl("g", {
        class: `wire-tooltip${variant ? ` ${variant}` : ""}`,
        transform: `translate(${x} ${y})`,
    });
    group.appendChild(svgEl("rect", { width, height: 28 }));
    group.appendChild(svgText(12, 18, label));
    svg.appendChild(group);
}
function drawRowLabel(svg, y, label) {
    const text = svgText(24, y, label, "row-label");
    svg.appendChild(text);
}
function drawPeerNode(svg, box, peer) {
    if (!box)
        return;
    const selected = state.selected.includes(peer.id);
    const group = nodeGroup(box, peer, nodeClass(peer.id, "peer", selected));
    if (peer.local)
        group.classList.add("local-peer");
    group.appendChild(svgEl("rect", { class: "node-body", width: box.width, height: box.height, rx: peer.local ? 14 : 24 }));
    group.appendChild(svgEl("circle", { class: "peer-core", cx: peer.local ? 26 : 24, cy: 28, r: peer.local ? 9 : 8, style: `fill:${peer.color}` }));
    group.appendChild(svgText(peer.local ? 44 : 42, 25, short(nodeDisplayLabel(peer, "peer"), peer.local ? 28 : 16)));
    group.appendChild(svgText(peer.local ? 44 : 42, 44, peer.local
        ? `${state.fragments.length} fragments · ${Object.keys(peer.views).length} session views`
        : `${Object.keys(peer.views).length} readable sessions`, "subtitle"));
    if (peer.local) {
        group.appendChild(svgEl("line", { class: "local-peer-divider", x1: 16, y1: 62, x2: box.width - 16, y2: 62 }));
        group.appendChild(svgText(18, 84, "Fragments", "local-peer-section-label"));
    }
    svg.appendChild(group);
}
function drawPsbtNode(svg, box, node, kind) {
    if (!box)
        return;
    const selected = state.selected.includes(node.id);
    const group = nodeGroup(box, node, nodeClass(node.id, kind, selected));
    const balance = transactionBalance(node);
    const convergence = convergenceFor(node.id);
    const role = psbtRoleFor(node, kind);
    const unordered = node.format === "unordered";
    const showPsbtTag = node.kind === "import" || Boolean(node.raw);
    const padding = kind === "session" ? 18 : 12;
    group.appendChild(svgEl("rect", { class: "node-body", width: box.width, height: box.height }));
    if (kind === "session") {
        drawConstructionFrame(group, box.width, box.height, "session-construction-marker", Boolean(convergence));
    }
    else if (convergence) {
        drawConstructionFrame(group, box.width, box.height, "node-construction-marker", true);
    }
    if (showPsbtTag)
        group.appendChild(svgText(12, 22, "PSBT", "psbt-tag"));
    group.appendChild(svgText(showPsbtTag ? 54 : padding, 22, short(nodeDisplayLabel(node, kind), showPsbtTag ? 16 : 25)));
    drawPsbtStatusBadges(group, box.width - padding, 20, psbtStatusBadges(node, kind, role));
    const detail = kind === "session"
        ? `${node.peers.length} readers`
        : psbtRoleFor(node, kind).label;
    group.appendChild(svgText(padding, 40, short(detail, 32), "subtitle"));
    const globalBottom = drawPsbtGlobalFields(group, node, padding, 57, box.width - padding * 2);
    let bodyTop = Math.max(90, globalBottom + 14);
    if (!unordered) {
        group.appendChild(svgText(padding, bodyTop, short(mineBalanceText(balance), 34), balance.mineBalanced ? "balance-ok" : "balance-warn"));
        group.appendChild(svgText(padding, bodyTop + 14, short(feeSumText(balance), 40), balance.fee.total < 0 ? "balance-warn" : "balance-fee"));
        bodyTop += 34;
    }
    if (convergence)
        drawConvergenceBadge(group, box.width - 78, 12, convergence);
    group.appendChild(svgEl("line", {
        class: "psbt-divider",
        x1: box.width / 2,
        y1: bodyTop - 10,
        x2: box.width / 2,
        y2: box.height - 30,
    }));
    if (unordered) {
        const display = unorderedPsbtDisplay(node);
        const pendingRows = pendingPayloadRowsForNode(node.id);
        drawUnorderedPsbtBalanceSheet(group, node, display, pendingRows, box, padding, bodyTop);
    }
    else {
        drawPsbtColumn(group, node, "input", padding, bodyTop, box.width / 2 - padding - 8, box.height - 30, "inputs", node.inputs, formatInputLine);
        drawPsbtColumn(group, node, "output", box.width / 2 + 10, bodyTop, box.width / 2 - 22, box.height - 30, "outputs", node.outputs, formatOutputLine);
    }
    drawPayloadSizeTotal(group, node, padding, box.height - 12, box.width - padding);
    svg.appendChild(group);
}
function drawPsbtGlobalFields(group, node, x, y, width) {
    const rows = psbtGlobalFieldRows(node);
    const lineHeight = 12;
    const valueX = x + 62;
    rows.forEach((row, index) => {
        const rowY = y + index * lineHeight;
        const field = svgEl("g", { class: "psbt-global-field" });
        field.appendChild(svgText(x, rowY, row.label, "psbt-global-label"));
        field.appendChild(svgText(valueX, rowY, short(row.value, Math.max(20, Math.floor((width - 62) / 5.4))), "psbt-global-value"));
        group.appendChild(field);
    });
    return y + rows.length * lineHeight;
}
function psbtGlobalFieldRows(node) {
    const version = node.version ?? 2;
    const lockTime = node.lockTime ?? 0;
    const rows = [
        { label: "format", value: psbtFormatLabel(node) },
        { label: "tx", value: `v${version} · lock ${lockTime}` },
    ];
    if (node.format === "unordered") {
        const sortMode = node.sortMode || "unset";
        rows.push({
            label: "sort",
            value: node.seed ? `${sortMode} · seed ${short(node.seed, 10)}` : sortMode,
        });
    }
    else if (node.format === "bip370") {
        rows.push({ label: "modifiable", value: node.modifiable || "both" });
    }
    else {
        rows.push({ label: "roles", value: "updater · signer" });
    }
    return rows;
}
function drawPsbtStatusBadges(group, rightX, y, badges) {
    const badgeSize = 18;
    const gap = 5;
    const totalWidth = badges.length * badgeSize + Math.max(0, badges.length - 1) * gap;
    badges.forEach((badge, index) => {
        const badgeGroup = svgEl("g", {
            class: `psbt-status-badge ${badge.className}`,
            transform: `translate(${rightX - totalWidth + index * (badgeSize + gap)} ${y - badgeSize / 2})`,
            role: "img",
            "aria-label": badge.title,
        });
        badgeGroup.appendChild(svgEl("title", {}));
        badgeGroup.lastChild.textContent = badge.title;
        const hitTarget = svgEl("rect", { class: "psbt-status-hit", width: badgeSize, height: badgeSize, rx: 4 });
        hitTarget.appendChild(svgEl("title", {}));
        hitTarget.lastChild.textContent = badge.title;
        badgeGroup.appendChild(hitTarget);
        const text = svgText(badgeSize / 2, 13, badge.icon);
        text.setAttribute("text-anchor", "middle");
        badgeGroup.appendChild(text);
        group.appendChild(badgeGroup);
    });
}
function drawConvergenceBadge(group, x, y, convergence) {
    const badge = svgEl("g", {
        class: "convergence-badge",
        transform: `translate(${x} ${y})`,
    });
    badge.appendChild(svgEl("rect", { width: 66, height: 24, rx: 12 }));
    badge.appendChild(svgText(10, 16, `${convergence.acked}/${convergence.total}`));
    badge.appendChild(svgEl("circle", { cx: 52, cy: 12, r: 5 }));
    group.appendChild(badge);
}
function drawPsbtColumn(group, node, side, x, y, width, maxY, heading, items, formatter) {
    let cursor = y + 10;
    let rendered = 0;
    group.appendChild(svgText(x, y, heading, "psbt-column-heading"));
    if (!items.length) {
        group.appendChild(svgText(x, y + 18, "empty", "psbt-empty"));
        return;
    }
    for (const [index, item] of items.entries()) {
        const rowHeight = coinRowHeight(side, item, index);
        if (cursor + rowHeight > maxY && rendered > 0)
            break;
        drawCoinRow(group, x, cursor, width, rowHeight, side, item, index);
        cursor += rowHeight + 5;
        rendered += 1;
    }
    if (rendered < items.length && cursor + 12 <= maxY) {
        group.appendChild(svgText(x, cursor + 10, `+${items.length - rendered} more`, "psbt-empty"));
    }
}
function drawUnorderedPsbtBalanceSheet(group, node, display, pendingRows, box, padding = 12, startY = 72) {
    const leftX = padding;
    const rightX = box.width / 2 + 10;
    const columnWidth = box.width - rightX - padding;
    const maxY = box.height - 30;
    const totalRows = unorderedBalanceSheetTotalRows(display);
    const sectionTotals = totalRows.slice(0, -1);
    const grandTotal = shouldShowGrandTotal(display) ? totalRows.at(-1) : null;
    const averageFeeRate = display.whole.fee.total / Math.max(1, display.estimatedVbytes);
    const grandTotalHeight = grandTotal ? sectionSubtotalHeight(grandTotal) : 0;
    const grandTotalY = grandTotal ? Math.max(96, maxY - grandTotalHeight) : maxY;
    const sectionMaxY = grandTotal ? grandTotalY - 4 : maxY;
    let cursor = startY;
    let rendered = 0;
    group.appendChild(svgText(leftX, cursor, "inputs", "psbt-column-heading"));
    group.appendChild(svgText(rightX, cursor, "outputs", "psbt-column-heading"));
    cursor += 15;
    for (const [sectionIndex, section] of display.subtransactions.entries()) {
        const remainingMinY = remainingBalanceSectionMinimum(display.subtransactions, sectionTotals, sectionIndex + 1);
        const sectionLimitY = Math.max(cursor, sectionMaxY - remainingMinY);
        if (cursor + sectionSubtotalHeight(section) > sectionLimitY && rendered > 0)
            break;
        if (sectionIndex > 0 && cursor + 8 <= sectionLimitY) {
            group.appendChild(svgEl("line", {
                class: "psbt-section-divider",
                x1: leftX,
                y1: cursor,
                x2: box.width - 12,
                y2: cursor,
            }));
            cursor += 8;
        }
        const sectionGroup = svgEl("g", {
            class: `psbt-subtxn ${section.kind}${section.descriptorMine ? " mine" : " other"}`,
        });
        const sectionStartY = cursor - 4;
        const hideHeading = display.subtransactions.length === 1 && section.kind === "unrecognized";
        if (!hideHeading && cursor + 14 <= sectionLimitY) {
            const label = svgText(leftX, cursor + 9, short(section.label, 22), `psbt-section-label ${section.kind}${section.descriptorMine ? " mine" : " other"}`);
            if (section.descriptorColor)
                label.setAttribute("style", `fill:${section.descriptorColor}`);
            sectionGroup.appendChild(label);
            cursor += 16;
        }
        const rowCount = Math.max(section.inputs.rows.length, section.outputs.rows.length);
        const subtotalHeight = sectionSubtotalHeight(section);
        const rowLimitY = Math.max(cursor, sectionLimitY - subtotalHeight - 4);
        let renderedRows = 0;
        for (let index = 0; index < rowCount; index += 1) {
            const input = section.inputs.rows[index];
            const output = section.outputs.rows[index];
            const rowHeight = Math.max(input ? coinRowHeight("input", input, index) : 0, output ? coinRowHeight("output", output, index) : 0);
            if (cursor + rowHeight > rowLimitY && renderedRows > 0)
                break;
            if (input) {
                drawCoinRow(sectionGroup, leftX, cursor, columnWidth, rowHeight, "input", input, index, pendingRows.has(payloadRowKey("input", input)));
            }
            if (output) {
                drawCoinRow(sectionGroup, rightX, cursor, columnWidth, rowHeight, "output", output, index, pendingRows.has(payloadRowKey("output", output)));
            }
            cursor += rowHeight + 5;
            renderedRows += 1;
        }
        if (renderedRows < rowCount && cursor + 12 <= rowLimitY) {
            sectionGroup.appendChild(svgText(leftX, cursor + 10, `+${rowCount - renderedRows} row${rowCount - renderedRows === 1 ? "" : "s"}`, "psbt-empty"));
            cursor += 14;
        }
        if (cursor + subtotalHeight > sectionLimitY)
            cursor = Math.max(cursor, sectionLimitY - subtotalHeight);
        drawSectionSubtotal(sectionGroup, section, leftX, rightX, cursor, columnWidth, node, averageFeeRate);
        cursor += subtotalHeight + 4;
        if (section.descriptorMine) {
            sectionGroup.insertBefore(svgEl("rect", {
                class: "psbt-subtxn-background mine",
                x: 4,
                y: sectionStartY,
                width: box.width - 8,
                height: Math.max(18, cursor - sectionStartY - 1),
                rx: 6,
            }), sectionGroup.firstChild);
        }
        group.appendChild(sectionGroup);
        rendered += 1;
    }
    if (!rendered) {
        group.appendChild(svgText(leftX, cursor + 12, "empty", "psbt-empty"));
    }
    if (grandTotal) {
        if (rendered) {
            group.appendChild(svgEl("line", {
                class: "psbt-grand-divider",
                x1: 4,
                y1: grandTotalY - 7,
                x2: box.width - 4,
                y2: grandTotalY - 7,
            }));
        }
        drawSectionSubtotal(group, grandTotal, leftX, rightX, grandTotalY, columnWidth, node, averageFeeRate);
    }
}
function sectionSubtotalHeight(section) {
    const presentation = accountingDeltaPresentation(section);
    return (presentation.showTotals ? 18 : 0) +
        (presentation.kind !== "balanced" ? 28 : 0) +
        (hasBalanceFeeSignal(section) ? 20 : 0);
}
function hasBalanceFeeSignal(section) {
    return Number(section.estimatedVbytes || 0) > 0 && Number(section.feeSats || 0) !== 0;
}
function minimumBalanceSectionHeight(section, total, includeDivider = true) {
    return (includeDivider ? 8 : 0) + 16 + sectionSubtotalHeight(total || section) + 4;
}
function remainingBalanceSectionMinimum(sections, totals, startIndex) {
    let total = 0;
    for (let index = startIndex; index < sections.length; index += 1) {
        total += minimumBalanceSectionHeight(sections[index], totals[index], true);
    }
    return total;
}
function drawSectionSubtotal(group, section, leftX, rightX, y, columnWidth, node = null, averageFeeRate = 0) {
    const sectionClass = "psbt-section-total";
    const presentation = accountingDeltaPresentation(section);
    let cursor = y;
    let lastFigureY = y;
    if (presentation.showTotals) {
        const lineY = cursor + 1;
        const totalY = cursor + 13;
        group.appendChild(svgEl("line", {
            class: `psbt-sum-line ${section.kind}`,
            x1: leftX,
            y1: lineY,
            x2: rightX + columnWidth,
            y2: lineY,
            ...(section.descriptorColor ? { style: `stroke:${section.descriptorColor}` } : {}),
        }));
        group.appendChild(svgAmountText(leftX + columnWidth, totalY, section.inputAccountingTotalSats ?? section.inputTotalSats, sectionClass, "end"));
        group.appendChild(svgOutputSubtotalText(rightX + columnWidth, totalY, section, sectionClass, "end"));
        cursor += 18;
        lastFigureY = totalY;
    }
    if (presentation.kind !== "balanced") {
        const deltaLineY = cursor + 1;
        const deltaLabelY = cursor + 10;
        const deltaY = cursor + 22;
        const deltaX = presentation.column === "input" ? leftX : rightX;
        const deltaEndX = deltaX + columnWidth;
        group.appendChild(svgEl("line", {
            class: `psbt-sum-line psbt-balance-delta-line ${presentation.kind}`,
            x1: deltaX,
            y1: deltaLineY,
            x2: deltaEndX,
            y2: deltaLineY,
        }));
        group.appendChild(svgText(deltaX, deltaLabelY, presentation.label, `psbt-balance-delta-label ${presentation.kind}`));
        const deltaClass = `${sectionClass} psbt-balance-delta ${presentation.kind}`;
        if (presentation.amountB === null) {
            group.appendChild(svgAmountText(deltaEndX, deltaY, presentation.amountA, deltaClass, "end"));
        }
        else {
            group.appendChild(svgAmountPairText(deltaEndX, deltaY, presentation.amountA, presentation.amountB, presentation.separator, deltaClass, "end"));
        }
        cursor += 28;
        lastFigureY = deltaY;
    }
    if (hasBalanceFeeSignal(section)) {
        const feeX = presentation.kind !== "balanced" && presentation.oppositeColumn === "output" ? rightX : leftX;
        drawBalanceSheetFeeSignal(group, node, section, feeX, lastFigureY + 19, columnWidth, averageFeeRate);
    }
}
function drawBalanceSheetFeeSignal(group, node, section, x, y, columnWidth, averageFeeRate) {
    const signal = balanceSheetFeeSignal(section, averageFeeRate);
    const label = `~${formatFeeRate(signal.feeRateSatsPerVbyte)} sat/vB · avg ${formatFeeRate(signal.averageFeeRateSatsPerVbyte)}`;
    if (node && signal.canFinalizeExplicitFee) {
        const button = svgEl("g", {
            class: "fee-finalize-button",
            transform: `translate(${x} ${y - 14})`,
            tabindex: "0",
            role: "button",
            "aria-label": `Finalize explicit fee for ${signal.descriptorLabel}`,
        });
        button.appendChild(svgEl("rect", { width: 86, height: 18, rx: 9 }));
        const text = svgText(43, 13, "Finalize fee");
        text.setAttribute("text-anchor", "middle");
        button.appendChild(text);
        button.addEventListener("pointerdown", (event) => event.stopPropagation());
        button.addEventListener("click", (event) => {
            event.stopPropagation();
            openDescriptorFeeDialog(node.id, section.descriptorId);
        });
        button.addEventListener("keydown", (event) => {
            if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                event.stopPropagation();
                openDescriptorFeeDialog(node.id, section.descriptorId);
            }
        });
        group.appendChild(button);
        group.appendChild(feeRateSignalText(section, x + 92, y, short(label, Math.max(16, Math.floor((columnWidth - 96) / 5.5)))));
        return;
    }
    group.appendChild(feeRateSignalText(section, x, y, short(label, Math.max(18, Math.floor(columnWidth / 5.5)))));
}
function feeRateSignalText(section, x, y, label) {
    const text = svgText(x, y, label, "fee-rate-signal");
    text.setAttribute("data-section-kind", section.kind);
    if (section.descriptorId)
        text.setAttribute("data-descriptor-id", section.descriptorId);
    return text;
}
function drawPayloadSizeTotal(group, node, leftX, y, rightX) {
    const size = payloadSizeEstimate(node);
    const text = svgText(rightX, y, `size estimate ${formatSizeEstimate(size, state.sizeUnit)}`, "psbt-size-total");
    text.setAttribute("text-anchor", "end");
    text.setAttribute("role", "button");
    text.setAttribute("tabindex", "0");
    text.setAttribute("aria-label", `Cycle size units; currently ${state.sizeUnit === "weight-units" ? "weight units" : "virtual bytes"}`);
    text.appendChild(svgEl("title", {}));
    text.lastChild.textContent = "Cycle size units";
    text.addEventListener("pointerdown", (event) => event.stopPropagation());
    text.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        cycleSizeUnit();
    });
    text.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            event.stopPropagation();
            cycleSizeUnit();
        }
    });
    group.appendChild(svgEl("line", {
        class: "psbt-size-divider",
        x1: leftX,
        y1: y - 12,
        x2: rightX,
        y2: y - 12,
    }));
    group.appendChild(text);
}
function cycleSizeUnit() {
    state.sizeUnit = state.sizeUnit === "weight-units" ? "vbytes" : "weight-units";
    renderGraph();
}
function formatFeeRate(value) {
    const rate = Number(value || 0);
    return rate >= 10 ? rate.toFixed(1) : rate.toFixed(2);
}
function drawCoinRow(group, x, y, width, height, side, coin, index, pending = false) {
    const highlighted = coinMatchesHoveredDescriptor(coin);
    const dimmed = coinDimmedByHoveredDescriptor(coin);
    const expanded = isCoinRowExpanded(side, coin);
    const detailLines = expanded ? coinDetailLines(side, coin, index, state.sizeUnit) : [];
    const row = svgEl("g", {
        class: `coin-row${expanded ? " expanded" : ""}${pending ? " pending-sync" : ""}${coin.descriptorId ? " has-descriptor" : ""}${coin.descriptorMine ? " mine" : " other"}${highlighted ? " descriptor-highlight" : ""}${dimmed ? " descriptor-dim" : ""}`,
        transform: `translate(${x} ${y})`,
        tabindex: "0",
        role: "button",
        "aria-expanded": String(expanded),
        "aria-label": `${side} ${coin.label || coin.id || index + 1}`,
    });
    row.addEventListener("pointerdown", (event) => event.stopPropagation());
    row.addEventListener("click", (event) => toggleCoinDetails(event, side, coin));
    row.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
            toggleCoinDetails(event, side, coin);
        }
    });
    const rowRect = svgEl("rect", { class: "coin-body", width, height });
    if (coin.descriptorColor) {
        rowRect.setAttribute("style", `stroke:${coin.descriptorColor};stroke-width:${coin.descriptorMine ? 3 : 1.1}`);
    }
    row.appendChild(rowRect);
    if (pending)
        drawConstructionFrame(row, width, height, "coin-construction-marker", true);
    if (coin.descriptorColor) {
        row.appendChild(svgEl("rect", {
            class: "descriptor-stripe",
            x: 4,
            y: 4,
            width: 5,
            height: Math.max(0, height - 8),
            rx: 2,
            style: `fill:${coin.descriptorColor}`,
        }));
    }
    const fingerprintX = coin.descriptorColor ? 16 : 7;
    const textX = fingerprintX + 23;
    const amountX = width - 8;
    drawDummyFingerprint(row, coin, fingerprintX, 7);
    row.appendChild(svgAmountText(amountX, 16, coinAmount(coin), "coin-main", "end"));
    const signedStatus = inputSignedStatus(side, coin);
    if (signedStatus) {
        row.appendChild(svgText(textX, 27, signedStatus, `coin-signed-status ${signedStatus}`));
    }
    detailLines.forEach((line, lineIndex) => {
        row.appendChild(svgText(textX, 32 + lineIndex * 12, short(line, Math.max(18, Math.floor((width - textX - 6) / 5.5))), "coin-detail"));
    });
    group.appendChild(row);
}
function inputSignedStatus(side, coin) {
    if (side !== "input")
        return "";
    if (coin.signatureVerified === true)
        return "authorized";
    return hasSignatureMaterial(coin) ? "signed" : "";
}
function hasSignatureMaterial(coin) {
    return [
        "finalScriptWitness",
        "finalWitness",
        "finalScriptSig",
        "scriptSig",
        "partialSignatures",
        "signatures",
        "signatureData",
        "tapKeySig",
    ].some((key) => detailValueIsPresent(coin[key]));
}
function detailValueIsPresent(value) {
    return Array.isArray(value)
        ? value.some(detailValueIsPresent)
        : typeof value === "string" && value.length > 0;
}
function coinRowHeight(side, coin, index) {
    if (!isCoinRowExpanded(side, coin))
        return 34;
    return Math.max(46, 32 + coinDetailLines(side, coin, index, state.sizeUnit).length * 12 + 8);
}
function isCoinRowExpanded(side, coin) {
    return state.expandedCoinRows.includes(payloadRowKey(side, coin));
}
function toggleCoinDetails(event, side, coin) {
    event.preventDefault();
    event.stopPropagation();
    const key = payloadRowKey(side, coin);
    if (state.expandedCoinRows.includes(key)) {
        state.expandedCoinRows = state.expandedCoinRows.filter((entry) => entry !== key);
    }
    else {
        state.expandedCoinRows = [...state.expandedCoinRows, key];
    }
    render();
}
function drawConstructionFrame(group, width, height, className, animated = false) {
    const isSessionFrame = className.includes("session-construction-marker");
    const inset = isSessionFrame ? 0 : 2.5;
    const attrs = {
        x: inset,
        y: inset,
        width: Math.max(0, width - inset * 2),
        height: Math.max(0, height - inset * 2),
        rx: Math.min(7, Math.max(2, Math.min(width, height) / 12)),
    };
    group.appendChild(svgEl("rect", {
        ...attrs,
        class: `${className} construction-frame-base`,
    }));
    group.appendChild(svgEl("rect", {
        ...attrs,
        class: `${className} construction-frame-stripe${animated ? " syncing" : ""}`,
    }));
}
function drawDummyFingerprint(group, coin, x, y) {
    const swatch = svgEl("g", {
        class: "dummy-fingerprint",
        transform: `translate(${x} ${y})`,
    });
    swatch.appendChild(svgEl("title", {}));
    swatch.lastChild.textContent = "Dummy script fingerprint placeholder; replace with LifeHash later.";
    dummyFingerprintColors(coin).forEach((color, index) => {
        swatch.appendChild(svgEl("rect", {
            x: (index % 2) * 8,
            y: Math.floor(index / 2) * 8,
            width: 8,
            height: 8,
            style: `fill:${color}`,
        }));
    });
    group.appendChild(swatch);
}
function formatInputLine(input, index) {
    const label = input.label || input.id || `input ${index + 1}`;
    const owner = input.owner ? ` ${input.owner}` : "";
    return `${label}${owner} · ${formatSatAmount(input.valueSats)}`;
}
function formatOutputLine(output, index) {
    const label = output.label || output.address || output.id || `output ${index + 1}`;
    return `${label} · ${formatSatAmount(output.valueSats)}`;
}
function nodeClass(nodeId, kind, selected) {
    const classes = ["node", kind];
    if (selected)
        classes.push("selected");
    if (state.joinWires.some((wire) => wire.sourceId === nodeId || wire.targetId === nodeId))
        classes.push("join-selected");
    if (convergenceFor(nodeId))
        classes.push("converging");
    if (state.wireDraft?.sourceId === nodeId)
        classes.push("wire-source");
    if (state.wireDraft) {
        const attempt = wireAttempt(state.wireDraft.sourceId, nodeId);
        if (attempt?.ok)
            classes.push("wire-compatible");
        if (attempt && !attempt.ok)
            classes.push("wire-incompatible");
    }
    if (state.wireDraft?.targetId === nodeId)
        classes.push("wire-target");
    return classes.join(" ");
}
function nodeGroup(box, node, className) {
    const kind = nodeKind(node.id);
    const interactive = kind !== "peer" || peerIsInteractive(node);
    const attrs = {
        class: `${className}${interactive ? "" : " passive"}`,
        transform: `translate(${box.x} ${box.y})`,
        role: interactive ? "button" : "group",
        "aria-label": `${kind} ${nodeDisplayLabel(node, kind)}`,
    };
    if (interactive)
        attrs.tabindex = "0";
    const group = svgEl("g", attrs);
    if (!interactive)
        return group;
    group.addEventListener("pointerdown", (event) => beginWireDraft(event, node.id));
    group.addEventListener("click", (event) => {
        if (state.suppressNodeClick) {
            state.suppressNodeClick = false;
            event.preventDefault();
            return;
        }
        toggleSelect(node.id);
    });
    group.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            toggleSelect(node.id);
        }
    });
    return group;
}
function svgEl(name, attrs) {
    const element = document.createElementNS("http://www.w3.org/2000/svg", name);
    for (const [key, value] of Object.entries(attrs || {})) {
        element.setAttribute(key, value);
    }
    return element;
}
function svgText(x, y, text, className) {
    const element = svgEl("text", { x, y });
    if (className)
        element.setAttribute("class", className);
    element.textContent = text;
    return element;
}
function svgAmountText(x, y, valueSats, className, anchor = "start") {
    const element = svgEl("text", { x, y, "text-anchor": anchor });
    if (className)
        element.setAttribute("class", className);
    appendAmountTspans(element, valueSats);
    return element;
}
function svgAmountPairText(x, y, leftSats, rightSats, separator, className, anchor = "start") {
    const element = svgEl("text", { x, y, "text-anchor": anchor });
    if (className)
        element.setAttribute("class", className);
    appendAmountTspans(element, leftSats);
    element.appendChild(svgTspan(separator || " / ", "amount-separator"));
    appendAmountTspans(element, rightSats);
    return element;
}
function svgOutputSubtotalText(x, y, section, className, anchor = "start") {
    const element = svgEl("text", { x, y, "text-anchor": anchor });
    if (className)
        element.setAttribute("class", className);
    if (subtotalIncludesExplicitFees(section)) {
        element.appendChild(svgTspan("(incl. fees)", "psbt-explicit-fee-subtotal-note"));
        element.appendChild(svgTspan(" "));
    }
    appendAmountTspans(element, section.outputAccountingTotalSats ?? section.outputTotalSats);
    return element;
}
function subtotalIncludesExplicitFees(section) {
    return section.kind !== "whole" && Number(section.explicitFeeSats || 0) !== 0;
}
function svgSignedAmountText(x, y, valueSats, className, anchor = "start") {
    const value = Number(valueSats || 0);
    const element = svgAmountText(x, y, Math.abs(value), className, anchor);
    if (value < 0)
        element.insertBefore(svgTspan("-"), element.firstChild);
    return element;
}
function appendAmountTspans(element, valueSats) {
    const parts = amountParts(valueSats);
    appendAmountPartTspans(element, parts.prefix, false);
    appendAmountPartTspans(element, parts.muted, true);
    element.appendChild(svgTspan(parts.sats));
}
function appendAmountPartTspans(element, text, muted) {
    if (!text)
        return;
    if (text.startsWith("₿")) {
        element.appendChild(svgTspan("₿", "amount-symbol"));
        text = text.slice(1);
    }
    if (text)
        element.appendChild(svgTspan(text, muted ? "amount-scale" : null));
}
function svgTspan(text, className = null) {
    const element = svgEl("tspan", {});
    if (className)
        element.setAttribute("class", className);
    element.textContent = text;
    return element;
}
function escapeHtml(value) {
    return String(value)
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll("\"", "&quot;")
        .replaceAll("'", "&#039;");
}
function render() {
    pruneSelection();
    renderPeerRail();
    renderDescriptorDock();
    renderFragmentList();
    renderSessionList();
    renderSelectionSummary();
    renderInspector();
    renderLog();
    renderActionVisibility();
    renderFeeContributionPanel();
    renderGraph();
}
function bindForms() {
    const onSubmit = (selector, handler) => {
        const form = document.querySelector(selector);
        if (!form)
            return;
        form.addEventListener("submit", handler);
    };
    const onClick = (selector, handler) => {
        const button = document.querySelector(selector);
        if (!button)
            return;
        button.addEventListener("click", handler);
    };
    onSubmit("#peerForm", (event) => {
        event.preventDefault();
        const name = document.querySelector("#peerName");
        const kind = document.querySelector("#peerContactKind");
        const value = document.querySelector("#peerContactValue");
        addPeerContact(name.value, kind.value, value.value);
        name.value = "";
        value.value = "";
    });
    onSubmit("#importForm", (event) => {
        event.preventDefault();
        importPsbt({
            label: document.querySelector("#importLabel").value,
            raw: document.querySelector("#importBytes").value,
            format: document.querySelector("#importFormat").value,
            target: document.querySelector("#importTarget").value,
            inputCount: document.querySelector("#importInputs").value,
            outputCount: document.querySelector("#importOutputs").value,
            totalValueSats: document.querySelector("#importValue").value,
            descriptorPrivacy: document.querySelector("#importDescriptorPrivacy").value,
            descriptor: document.querySelector("#importDescriptor").value,
        });
    });
    onSubmit("#descriptorForm", (event) => {
        event.preventDefault();
        addDescriptorUtxo(document.querySelector("#descriptorOwner").value, document.querySelector("#descriptorText").value, document.querySelector("#utxoTxid").value, document.querySelector("#utxoVout").value, document.querySelector("#utxoValue").value, true, document.querySelector("#descriptorPrivacy").value, document.querySelector("#descriptorOwnership").value);
    });
    onSubmit("#paymentForm", (event) => {
        event.preventDefault();
        addPaymentIntent(document.querySelector("#paymentLabel").value, document.querySelector("#paymentAddress").value, document.querySelector("#paymentValue").value);
    });
    const sessionOrderingMode = document.querySelector("#sessionOrderingMode");
    const sessionSeedField = document.querySelector("#sessionSeedField");
    const sessionSeed = document.querySelector("#sessionSeed");
    const generateSessionSeed = document.querySelector("#generateSessionSeed");
    const syncSessionSeedControls = () => {
        const deterministic = sessionOrderingMode.value === "det";
        const supportsSeed = deterministic || sessionOrderingMode.value === "unset";
        sessionSeedField.hidden = !supportsSeed;
        sessionSeed.disabled = !supportsSeed;
        sessionSeed.required = deterministic;
        sessionSeed.readOnly = false;
        sessionSeed.placeholder = deterministic ? "required hex seed" : "optional hex seed";
        generateSessionSeed.disabled = !supportsSeed;
        generateSessionSeed.title = deterministic ? "Generate deterministic ordering seed" : "Generate optional ordering seed";
        generateSessionSeed.setAttribute("aria-label", generateSessionSeed.title);
        sessionSeed.setCustomValidity("");
        if (!supportsSeed) {
            sessionSeed.value = "";
            return;
        }
        if (deterministic && !sessionSeed.value.trim()) {
            sessionSeed.value = secureRandomSessionSeed();
        }
    };
    sessionOrderingMode.addEventListener("change", syncSessionSeedControls);
    generateSessionSeed.addEventListener("click", () => {
        if (!["det", "unset"].includes(sessionOrderingMode.value))
            return;
        sessionSeed.value = secureRandomSessionSeed();
    });
    syncSessionSeedControls();
    onSubmit("#sessionForm", (event) => {
        event.preventDefault();
        const ordering = normalizeSessionOrdering(sessionOrderingMode.value, sessionSeed.value);
        if (!ordering.valid) {
            sessionSeed.setCustomValidity(ordering.error);
            sessionSeed.reportValidity();
            return;
        }
        sessionSeed.setCustomValidity("");
        const label = document.querySelector("#sessionLabel");
        addSession(ordering, true, emptyPayload(), label?.value.trim() || null);
        if (label)
            label.value = "";
        if (ordering.mode === "det") {
            sessionSeed.value = secureRandomSessionSeed();
        }
        else if (ordering.mode === "unset") {
            sessionSeed.value = "";
        }
    });
    onSubmit("#pasteForm", (event) => {
        event.preventDefault();
        const input = document.querySelector("#pasteInbox");
        handlePastedText(input.value, "paste");
        input.value = "";
    });
    onSubmit("#sneakernetPasteForm", (event) => {
        event.preventDefault();
        const input = document.querySelector("#sneakernetPaste");
        handlePastedText(input.value, "sneakernet");
        input.value = "";
    });
    onClick("#connectSelected", connectSelected);
    onClick("#convertSelected", convertSelectedToUnordered);
    onClick("#orderSelected", sortSelectedPsbt);
    onClick("#atomizeSelected", atomizeSelectedPsbt);
    onClick("#abortSession", abortSelectedSession);
    onClick("#promoteSelected", promoteSelected);
    onClick("#cancelJoinSelect", cancelJoinSelect);
    onClick("#sneakernetMakeUnordered", makeAllWorkspaceUnordered);
    onClick("#feeContributionCancel", closeFeeContributionPanel);
    onClick("#feeContributionApply", applyFeeContribution);
    onClick("#resetDemo", () => {
        createInitialState();
        render();
    });
    const feeSlider = document.querySelector("#feeContributionSlider");
    const feeAmount = document.querySelector("#feeContributionAmount");
    const feeConfirm = document.querySelector("#feeContributionConfirm");
    feeSlider?.addEventListener("input", () => updateFeeContributionAmount(feeSlider.value));
    feeAmount?.addEventListener("input", () => updateFeeContributionAmount(feeAmount.value));
    feeConfirm?.addEventListener("change", () => setFeeContributionConfirmed(feeConfirm.checked));
    const graph = document.querySelector("#graph");
    graph.addEventListener("pointermove", updateWireDraft);
    graph.addEventListener("pointerup", finishWireDraft);
    graph.addEventListener("pointerleave", cancelWireDraft);
    graph.addEventListener("pointercancel", cancelWireDraft);
    const dropzone = document.querySelector("#pasteDropzone");
    dropzone.addEventListener("paste", (event) => {
        event.preventDefault();
        handlePastedText(event.clipboardData?.getData("text/plain") || "", "paste");
    });
    dropzone.addEventListener("dragover", (event) => {
        event.preventDefault();
        dropzone.classList.add("drag-over");
    });
    dropzone.addEventListener("dragleave", () => dropzone.classList.remove("drag-over"));
    dropzone.addEventListener("drop", (event) => {
        event.preventDefault();
        dropzone.classList.remove("drag-over");
        const files = [...(event.dataTransfer?.files || [])];
        if (files.length) {
            importDroppedFiles(files);
        }
        else {
            handlePastedText(event.dataTransfer?.getData("text/plain") || "", "drop");
        }
    });
    window.addEventListener("resize", renderGraph);
}
bindForms();
createInitialState();
render();
export {};
