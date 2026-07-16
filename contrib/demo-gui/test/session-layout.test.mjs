import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../session.html", import.meta.url), "utf8");
const app = readFileSync(new URL("../src/session/app.ts", import.meta.url), "utf8");
const styles = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

test("the canvas is a scrolling viewport over a positioned world", () => {
  const workbenchStart = html.indexOf('id="spatialWorkbench"');
  const workbenchEnd = html.indexOf("</div>", html.indexOf('id="nodeLayer"'));
  const drawerBar = html.indexOf('id="drawerBar"');

  assert.ok(workbenchStart >= 0, "the spatial workbench is present");
  const workbench = html.slice(workbenchStart, workbenchEnd);
  // Edge SVG under the node layer, both inside the world.
  const world = workbench.indexOf('id="canvasWorld"');
  const overlay = workbench.indexOf('id="wireOverlay"');
  const nodes = workbench.indexOf('id="nodeLayer"');
  assert.ok(world >= 0, "the canvas world is present");
  assert.ok(world < overlay && overlay < nodes, "edges render under the node layer");
  assert.ok(drawerBar > workbenchStart, "the drawer bar follows the workbench");
  // Scroll, not zoom: the viewport pans natively (no transform scaling).
  assert.match(styles, /\.session-spatial-workbench\s*\{[\s\S]*?overflow:\s*auto;/);
  assert.match(styles, /\.session-canvas-world\s*\{[\s\S]*?position:\s*relative;/);
  // Keyed wrappers are absolutely positioned and GLIDE between positions.
  assert.match(styles, /\.session-canvas-node\s*\{[\s\S]*?position:\s*absolute;[\s\S]*?transition:\s*transform/);
  assert.match(styles, /prefers-reduced-motion/);
});

test("canvas nodes are keyed: wrappers survive, contents rebuild", () => {
  const canvas = app.slice(app.indexOf("const canvasNodes"), app.indexOf("function renderFocusFragments"));
  // Create-once wrappers, per-render replaceChildren — element identity
  // survives so transform transitions animate the moves.
  assert.match(canvas, /canvasNodes\.get\(key\)/);
  assert.match(canvas, /wrapper\.replaceChildren\(card\)/);
  // Vanished keys are pruned, not leaked.
  assert.match(canvas, /wrapper\.remove\(\);\s*\n\s*canvasNodes\.delete\(key\)/);
  // Positions come from the PURE layout over MEASURED sizes — no
  // hard-coded lane heights (the mockup's mistake).
  assert.match(canvas, /laneLayout\(\{/);
  assert.match(canvas, /offsetWidth/);
  assert.match(canvas, /offsetHeight/);
  assert.match(canvas, /minWidth: workbench\.clientWidth/);
});

test("Mine is a framed canvas lane of local-only drafts", () => {
  const canvas = app.slice(app.indexOf("function renderCanvas"), app.indexOf("function renderFocusFragments"));
  // Mine holds ONLY fragments no register references; sessions render
  // once, as containers — publishing reads as a move between nodes.
  assert.match(canvas, /mineFragmentKeys\(/);
  assert.doesNotMatch(app, /renderSessionArea/);
  assert.doesNotMatch(styles, /session-published-area/);
  // The frame is furniture BEHIND the cards (canvas floor z-order).
  assert.match(canvas, /"frame:mine", "session-mine-frame"/);
  assert.match(styles, /\.session-mine-frame\s*\{[\s\S]*?z-index:\s*0;/);
  // The label states the publishing rule instead of a pseudo-peer title.
  assert.match(canvas, /not published to any session/);
  assert.match(canvas, /every loaded fragment is published/);
});

test("the lanes stack peers over sessions over Mine", () => {
  const canvas = app.slice(app.indexOf("function renderCanvas"), app.indexOf("function renderFocusFragments"));
  // One content pass per lane, in lane order, all feeding one layout call.
  const peers = canvas.indexOf("peerBridgeGroups(objects)");
  const sessions = canvas.indexOf("renderSessionContainer(sessionObject)");
  const mine = canvas.indexOf("mineFragmentKeys(");
  assert.ok(peers >= 0 && peers < sessions && sessions < mine, "lane passes run top to bottom");
  assert.match(canvas, /peerGroups: peerKeys\.map/);
  assert.match(canvas, /sessions: sessionKeys\.map\(measure\)/);
  assert.match(canvas, /mine: mineKeys\.map\(measure\)/);
  // Fixed per-lane wrapper widths keep measurement stable.
  for (const lane of ["session-node-peer", "session-node-session", "session-node-fragment"]) {
    assert.match(styles, new RegExp(`\\.${lane}\\s*\\{[^}]*width:`), `${lane} has a fixed width`);
  }
  // Two-column coin reading is the desktop default: fragment and session
  // nodes leave the card's container inline-size clear of the 340px
  // column-collapse query (single column is the last resort, not the norm).
  const collapseAt = 340;
  for (const lane of ["session-node-session", "session-node-fragment"]) {
    const width = Number(styles.match(new RegExp(`\\.${lane}\\s*\\{[^}]*width:\\s*(\\d+)px`))?.[1]);
    assert.ok(width >= collapseAt + 60, `${lane} width ${width} clears the ${collapseAt}px collapse query`);
  }
});

test("single-session focus swaps the canvas for the flat register list", () => {
  // The canvas (and its bar) hide in focus; the focused panel is the
  // inverse — overview never shows it.
  const workbenchTag = html.match(/<div id="spatialWorkbench"[^>]*>/)?.[0] ?? "";
  assert.match(workbenchTag, /data-focus-hide/);
  const focusPanel = html.match(/<section[^>]*data-focus-show[^>]*>/)?.[0] ?? "";
  assert.match(focusPanel, /aria-label="Focused session"/);
  assert.match(app, /\[data-focus-hide\]/);
  assert.match(app, /\[data-focus-show\]/);
  assert.match(app, /panel\.hidden = !inFocus/);
});

test("peer cards preserve bridging while Pair stays visibly unavailable", () => {
  // Bridging rides the always-on drag gesture: decorateWireTarget arms the
  // drag on single peers AND bridge-group cards. Queue participation lives
  // on the pending EDGE (and its pill), not in a card chip.
  assert.match(app, /decorateWireTarget\(item, \{ kind: "peer", key: peer\.key \}\)/);
  assert.match(app, /decorateWireTarget\(item, \{ kind: "peer", key: members\[0\]\.key \}\)/);
  assert.match(app, /armWireDrag\(node, ref\)/);
  assert.doesNotMatch(app, /wireQueueChip/);
  assert.match(app, /Pair unavailable until the ptj adapter exposes session pairing/);
});

test("session containers hold the register card and explicit sync — never a transport", () => {
  const start = app.indexOf("function renderSessionContainer(");
  const end = app.indexOf("function unavailablePairButton", start);
  assert.ok(start >= 0 && end > start, "session-container renderer is present");
  const container = app.slice(start, end);
  // Sessions have no transport (peers do): the container must not claim one.
  assert.doesNotMatch(container, /sessionObject\.transport/);
  // A session is a single-value register, not a member list: the container
  // shows the register's value as a full fragment card, or an honest
  // empty-register hint.
  assert.match(container, /register: \$\{sessionObject\.contentKey\}/);
  assert.match(container, /renderFragmentCard\(content\)/);
  assert.match(container, /empty register — wire a fragment in/);
  assert.doesNotMatch(container, /fragmentKeys/);
  assert.match(container, /button\("Sync now"/);
});

test("new session is a one-field action in the canvas bar, not a utilities panel", () => {
  // The form lives in the lane-actions bar above the canvas…
  const barStart = html.indexOf('class="session-canvas-bar"');
  const barEnd = html.indexOf("</div>", html.indexOf("</form>", barStart));
  const bar = html.slice(barStart, barEnd);
  assert.match(bar, /id="newSessionForm"/);
  assert.match(bar, /id="newSessionName"/);
  assert.match(bar, /id="addPeerQuick"/);
  // …and the utilities "Create session" panel is gone (minting an empty
  // register needs no fragment/descriptor pickers).
  assert.doesNotMatch(html, /Create session/);
  assert.equal((html.match(/id="newSessionForm"/g) ?? []).length, 1);
});

test("one shared bottom Add drawer owns quick paste and manual peer creation", () => {
  assert.equal((html.match(/id="pasteInput"/g) ?? []).length, 1);
  assert.match(html, /id="addDrawer"[^>]*class="[^"]*session-drawer/);
  assert.match(html, /id="manualPeerForm"/);
  assert.match(html, /id="manualPeerTransport"/);
  for (const transport of ["nostr", "iroh", "str0m", "webrtc-rs"]) {
    assert.match(html, new RegExp(`<option value="${transport}">`));
  }
  assert.match(app, /Pair unavailable until the ptj adapter exposes session pairing/);
});

test("every utility docks in the bottom drawer bar, one drawer at a time", () => {
  // The old utilities column is gone; the bar exposes each drawer.
  assert.doesNotMatch(html, /id="sessionUtilities"/);
  for (const drawer of [
    "addDrawer",
    "createDrawer",
    "syncDrawer",
    "negotiateDrawer",
    "editorDrawer",
    "assignIdsDrawer",
    "exportDrawer",
    "logDrawer",
  ]) {
    assert.match(html, new RegExp(`data-drawer="${drawer}"`), `${drawer} has a bar toggle`);
    assert.match(html, new RegExp(`<section id="${drawer}" class="session-drawer"`), `${drawer} is a drawer`);
  }
  // One drawer at a time: the manager hides every drawer that is not the
  // requested one (a single setter owns all drawer visibility).
  assert.match(app, /el<HTMLElement>\(drawerId\)\.hidden = drawerId !== id;/);
  // The Create-fragment bar button doubles as the utxo drop target while
  // its drawer is closed.
  assert.match(html, /data-drawer="createDrawer" data-wire-kind="create" data-wire-key="create"/);
  // The ops toolbar stays attached to the work area, not a drawer.
  const main = html.slice(html.indexOf("<main"), html.indexOf("</main>"));
  assert.match(main, /session-ops-panel/);
});

test("standing wire edges: Mine sees every session, peers their authorized ones", () => {
  // The overlay lives inside the workbench and is pointer-transparent.
  const main = html.slice(html.indexOf("<main"), html.indexOf("</main>"));
  assert.match(main, /<svg id="wireOverlay" class="session-wire-overlay"/);
  assert.match(styles, /\.session-wire-overlay\s*\{[\s\S]*?pointer-events:\s*none;/);
  // Edges draw from the SAME layout rects that placed the cards — never
  // from DOM measurement — so the overlay scrolls with the world for free.
  const overlayStart = app.indexOf("function canvasRectFor");
  const overlayEnd = app.indexOf("function render(", overlayStart);
  assert.ok(overlayStart >= 0 && overlayEnd > overlayStart, "overlay slice is bounded");
  const overlay = app.slice(overlayStart, overlayEnd);
  assert.doesNotMatch(overlay, /getBoundingClientRect/);
  assert.match(overlay, /curveBetween\(from, to\)/);
  assert.match(overlay, /viewBox[\s\S]*?canvasLayout\.world\.width/);
  // Both edge builders exist: Mine → every session container…
  assert.match(overlay, /addEdge\(mineFrame, container, "session-edge-mine"\)/);
  // …and peer → each session whose peer set contains it (bridge groups
  // collapse to their first member's card, deduplicated).
  assert.match(overlay, /sessionObject\.peerKeys/);
  assert.match(overlay, /"session-edge-auth"/);
  // Redraw hooks: every render; a resize re-lays the canvas out (the SVG
  // lives in world coordinates, so scrolling needs no hook at all), latched
  // to one render per animation frame.
  assert.match(app, /drawWireOverlay\(\);\n\}/);
  assert.match(app, /if \(resizeRenderQueued\) return;/);
  assert.match(app, /requestAnimationFrame\(\(\) => \{\n\s*resizeRenderQueued = false;\n\s*render\(\);/);
  assert.doesNotMatch(app, /addEventListener\("scroll", drawWireOverlay/);
  assert.match(styles, /\.session-edge-mine\s*\{/);
  assert.match(styles, /\.session-edge-auth\s*\{/);
});

test("pending wires are canvas edges with midpoint Join pills", () => {
  // The pill layer is HTML above the cards; the SVG stays pointer-blind.
  const main = html.slice(html.indexOf("<main"), html.indexOf("</main>"));
  assert.match(main, /<div id="pillLayer" class="session-pill-layer"><\/div>/);
  assert.match(styles, /\.session-pill-layer\s*\{[\s\S]*?pointer-events:\s*none;[\s\S]*?z-index:\s*2;/);
  assert.match(styles, /\.session-wire-pill\s*\{[\s\S]*?pointer-events:\s*auto;/);
  // Each live pending wire draws an animated edge and one pill at the
  // curve's midpoint whose Join collapses exactly that edge.
  const overlayStart = app.indexOf("function drawWireOverlay");
  const overlayEnd = app.indexOf("function render(", overlayStart);
  const overlay = app.slice(overlayStart, overlayEnd);
  assert.match(overlay, /livePendingWires\(\)/);
  assert.match(overlay, /"session-edge-pending"/);
  assert.match(overlay, /curveMidpoint\(from, to\)/);
  assert.match(overlay, /joinPendingWire\(key\)/);
  assert.match(styles, /\.session-edge-pending\s*\{[\s\S]*?stroke-dasharray/);
  // The card costume and the "N queued" chip are gone — the edge IS the
  // queue participation (the panel keeps the textual list).
  assert.doesNotMatch(app, /session-wire-pending/);
  assert.doesNotMatch(styles, /session-wire-queued-chip/);
  assert.doesNotMatch(styles, /\.session-wire-pending/);
});

test("queued joins are computed before they can be committed", () => {
  // The probe runs the ACTUAL backend join (it is pure) the moment a wire
  // is queued, and re-runs when the PSBTs under the endpoints change.
  const probeStart = app.indexOf("--- join probes ---");
  const probeEnd = app.indexOf("async function joinPendingWire", probeStart);
  assert.ok(probeStart >= 0 && probeEnd > probeStart, "probe section is bounded");
  const probes = app.slice(probeStart, probeEnd);
  assert.match(probes, /backend\s*\.joinPsbts\(fragments\.map\(\(fragment\) => fragment\.psbt\)\)/);
  assert.match(probes, /signature/);
  // Probes for vanished wires and components are pruned.
  assert.match(probes, /wireProbes\.delete\(key\)/);
  assert.match(probes, /componentProbes\.delete\(key\)/);
  // A conflicted wire trades its Join for an explanation (pill and panel row).
  const conflictButtons = app.match(/button\("⚠ why\?"/g) ?? [];
  assert.equal(conflictButtons.length, 2, "pill and queue row both explain conflicts");
  assert.match(app, /openConflictModal/);
});

test("a conflicted component blocks the toolbar Join with an explanation", () => {
  // Even when every adjacent pair joins cleanly, the component's LUB can
  // conflict — the whole component is probed as one n-ary join.
  const probes = app.slice(app.indexOf("--- join probes ---"), app.indexOf("async function joinPendingWire"));
  assert.match(probes, /component\.wires\.length > 1/);
  assert.match(probes, /component\.nodes\.flatMap\(\(ref\) => probeFragments\(ref\)\)/);
  // The toolbar Join opens the conflict modal instead of running joins
  // already computed to fail; the button wears the blocked styling.
  const joinAllStart = app.indexOf("async function joinAllWires");
  const joinAll = app.slice(joinAllStart, app.indexOf("function clearPendingWires", joinAllStart));
  assert.match(joinAll, /queueConflicts\(wires\)/);
  assert.match(joinAll, /openConflictModal\("the queue cannot join — conflicts", conflicts\)/);
  assert.match(app, /classList\.toggle\("session-join-blocked", conflicts\.length > 0\)/);
  assert.match(styles, /\.session-join-blocked\s*\{/);
});

test("real backend calls wear honest in-flight state", () => {
  // withBusy is promise-scoped: tokens set before the call, cleared in
  // finally, a render on both edges — no timers, no fake counters.
  const busyStart = app.indexOf("--- in-flight state ---");
  const busyEnd = app.indexOf("async function joinPendingWire", busyStart);
  assert.ok(busyStart >= 0 && busyEnd > busyStart, "busy section is bounded");
  const busy = app.slice(busyStart, busyEnd);
  assert.match(busy, /for \(const token of tokens\) inflight\.add\(token\);/);
  assert.match(busy, /\} finally \{\n    for \(const token of tokens\) inflight\.delete\(token\);\n    render\(\);/);
  assert.doesNotMatch(busy, /setTimeout|setInterval/);
  // Cards wear the costume through the one decoration seam; the joining
  // wire's edge marches and its pill waits.
  assert.match(app, /inflight\.has\(busyToken\(ref\)\)/);
  assert.match(app, /node\.setAttribute\("aria-busy", "true"\)/);
  assert.match(app, /inflight\.has\(`edge:\$\{key\}`\)/);
  assert.match(app, /"session-edge-pending session-edge-busy"/);
  // Every real backend mutation is instrumented: per-wire join, component
  // joins and remapped wires, both sync paths, pay, and confirm.
  const wrapped = app.match(/withBusy(<[^>]+>)?\(/g) ?? [];
  assert.ok(wrapped.length >= 8, `all backend call sites wrapped (saw ${wrapped.length})`);
  assert.match(styles, /\.session-busy\s*\{[\s\S]*?pointer-events:\s*none;/);
  assert.match(styles, /\.session-edge-busy\s*\{/);
});

test("mockup parity: peer reach, breathing selection, identity hover", () => {
  // Every peer card states its reach — sessions its bridge group can
  // read/write — like the mockup's "sees N session(s)" meta.
  assert.match(app, /`sees \$\{count\} session\(s\)`/);
  assert.match(app, /sessionCountMeta\(peer\.key\)/);
  assert.match(app, /sessionCountMeta\(members\[0\]\.key\)/);
  // Selection breathes like the mockup's .node.selected halo (and holds a
  // static halo under prefers-reduced-motion).
  assert.match(styles, /\.session-card-selected\s*\{[\s\S]*?session-selection-glow/);
  assert.match(styles, /@keyframes session-selection-glow/);
  // Hovering a descriptor card dims every colorized node of a different
  // identity; the key rides colorizeIdentity so the wiring is delegated.
  assert.match(app, /node\.dataset\.identityKey = colorKey;/);
  assert.match(app, /\.session-descriptor-card\[data-identity-key\]/);
  assert.match(app, /"session-identity-dim",\n\s*key !== null && node\.dataset\.identityKey !== key/);
  assert.match(styles, /\.session-identity-dim\s*\{/);
  // The dim also LIFTS when pointerover can't do it: the pointer leaving
  // the window (pointerout with a null relatedTarget) and a touch tap
  // landing anywhere that is not a descriptor card.
  assert.match(app, /if \(event\.relatedTarget === null\) applyIdentityDim\(null\)/);
  assert.match(app, /if \(identityKeyAt\(event\.target\) === null\) applyIdentityDim\(null\)/);
});

test("row chips are scriptPubKeys only — secondary lifehashes chip their hex in the facts", () => {
  // Output rows: the one fingerprint is where the money goes. The unique
  // id's chip retreats to the expanded facts.
  const outputStart = app.indexOf("function outputRow");
  const outputSlice = app.slice(outputStart, app.indexOf("\n}", outputStart));
  assert.ok(outputStart >= 0, "outputRow slice is bounded");
  assert.doesNotMatch(outputSlice, /lifehashBadge\(output\.uniqueIdHex/);
  assert.match(outputSlice, /lifehashBadge\(output\.scriptHex/);
  // Input rows: no prevout script means a TEXTUAL outpoint, never a txid
  // chip masquerading as a payer identity.
  const inputStart = app.indexOf("function inputRow");
  const inputSlice = app.slice(inputStart, app.indexOf("\n}", inputStart));
  assert.doesNotMatch(inputSlice, /lifehashBadge\(\s*input\.outpointTxid/);
  assert.match(inputSlice, /lifehashBadge\(\s*input\.prevoutScriptHex/);
  // The expanded facts chip fingerprintable pairs beside their hex — the
  // LifeHash sits next to the bitvomit it identifies.
  const factsStart = app.indexOf("function coinRow");
  const factsSlice = app.slice(factsStart, app.indexOf("function signatureMark", factsStart));
  assert.match(factsSlice, /if \(pair\.chipHex\) value\.append\(lifehashBadge\(pair\.chipHex/);
});

test("disabled ops explain themselves on press, not only on hover", () => {
  const hint = html.indexOf('id="opsHint"');
  assert.ok(hint >= 0, "the ops hint line is present");
  assert.ok(hint > html.indexOf('id="gateOverrides"'), "the hint follows the gate rows");
  assert.match(html, /id="opsHint"[^>]*role="status"/, "the hint is a live status region");
  // The reason is stashed per button and surfaced by rect-hit-testing the
  // press on the toolbar: disabled buttons are pointer-events:none (Firefox
  // suppresses their pointer events entirely), so the section receives the
  // event and elementsFromPoint would skip the button.
  assert.match(app, /dataset\.why = why/);
  assert.match(app, /querySelectorAll<HTMLButtonElement>\("button:disabled"\)/);
  assert.match(app, /getBoundingClientRect\(\)/);
  assert.match(styles, /\.session-ops button:disabled \{[^}]*pointer-events: none;/);
});

test("fragment selection has a keyboard path", () => {
  // aria-pressed lives on a real toggle button (the <li> nests buttons and
  // cannot take a button role); the card-background click stays pointer-only.
  const cardStart = app.indexOf("function renderFragmentCard");
  const cardEnd = app.indexOf("function detailToggle", cardStart);
  assert.ok(cardStart >= 0 && cardEnd > cardStart, "renderFragmentCard slice is bounded");
  const card = app.slice(cardStart, cardEnd);
  assert.match(card, /session-select-toggle/);
  assert.match(card, /selectToggle\.setAttribute\("aria-pressed", String\(fragment\.selected\)\)/);
  assert.doesNotMatch(card, /item\.setAttribute\("aria-pressed"/);
});

test("the sort seed is PSBT state: no ops-bar field, a prompt only when absent", () => {
  // The standing "sort seed (hex, optional)" input is gone from the ops bar…
  assert.doesNotMatch(html, /id="sortSeed"/);
  // …replaced by a dialog that opens only when the PSBT carries neither
  // explicit sort keys nor a stored seed.
  assert.match(html, /<dialog id="sortSeedDialog"/);
  assert.match(html, /id="sortSeedGenerate"/);
  assert.match(app, /summary\.sortMode !== "explicit" && !summary\.seedHex/);
  // Cancel paths resolve the pending sort as abandoned, never half-armed.
  assert.match(app, /sortSeedDialog\.addEventListener\("cancel", \(\) => settleSortSeed\(null\)\)/);
  // …but an empty/non-hex confirm is NOT a cancel: it keeps the dialog
  // open with a field-level validity message instead of settling null.
  assert.match(app, /setCustomValidity\("enter a hex seed/);
  assert.match(app, /reportValidity\(\)/);
});

test("a bitcoin: URI mints a txout-intent fragment, prompting only for a missing amount", () => {
  // The paste path hands payment URIs to the shell (backend round-trip),
  // never to the node-graph mint.
  assert.match(app, /if \(pasted\.kind === "payment-uri"\) \{\s*\n\s*if \(await addPaymentUri\(pasted\.payload\)\)/);
  const start = app.indexOf("async function addPaymentUri");
  const end = app.indexOf("function settlePayAmount", start);
  assert.ok(start >= 0 && end > start, "addPaymentUri slice is bounded");
  const slice = app.slice(start, end);
  // The URI becomes a one-output PSBT via /api/create — the Creator role
  // assigns the output its random unique id; ordering stays unset.
  assert.match(slice, /backend\.createPsbt\(/);
  assert.match(slice, /ordering: "unset"/);
  assert.match(slice, /inputs: \[\]/);
  assert.match(slice, /"payment-uri"/);
  // PSBTv2 requires PSBT_OUT_AMOUNT, so an amountless URI prompts…
  assert.match(slice, /uri\.valueSats > 0 \? uri\.valueSats : await promptPaymentAmount\(uri\.address\)/);
  assert.match(html, /<dialog id="payAmountDialog"/);
  assert.match(html, /PSBT_OUT_AMOUNT|id="payAmountDialogWhy"/);
  // …and the cancel paths settle null, never half-armed (dialog cousin of
  // the sort-seed prompt).
  assert.match(app, /payAmountDialog\.addEventListener\("cancel", \(\) => settlePayAmount\(null\)\)/);
  assert.match(app, /setCustomValidity\("enter a positive BTC amount/);
});

test("a local 'peer' presents as a disk location, not an identity", () => {
  // Same card shape and wire gestures, but honestly badged: no peer
  // identity stands behind a local transport, only storage on disk.
  assert.match(app, /badge\("disk location", "session-badge"\)/);
  assert.match(app, /no peer identity/);
  // The Copy button names what it copies — a path, not an identity.
  assert.match(app, /"Copy path" : "Copy id"/);
  assert.match(app, /Copy the server-side storage path/);
  // The manual-peer form offers local alongside the real transports,
  // labelled as a storage location.
  assert.match(html, /<option value="local">local \(disk path/);
});

test("every path into the assign-ids panel parameterizes it first", () => {
  // The bar toggle must not unhide the drawer raw: openAssignIds gates on
  // the selection and renders the rows before revealing.
  assert.match(app, /if \(id === "assignIdsDrawer" && openDrawerId\(\) !== id\) \{\s*\n\s*openAssignIds\(\);/);
  assert.match(app, /el<HTMLButtonElement>\("opAssignIds"\)\.addEventListener\("click", openAssignIds\)/);
});

test("canvas cards are articles; the workbench viewport is focusable and named", () => {
  // The node layer is a div, not a list — cards must not be stray <li>.
  for (const fn of [
    "renderFragmentCard",
    "renderSessionContainer",
    "renderPeerCard",
    "renderBridgeGroupCard",
  ]) {
    const start = app.indexOf(`function ${fn}`);
    assert.ok(start >= 0, `${fn} exists`);
    assert.match(app.slice(start, start + 500), /createElement\("article"\)/, fn);
  }
  // True lists still wrap cards in their own list items.
  assert.match(app, /const item = document\.createElement\("li"\);\s*\n\s*item\.append\(renderFragmentCard\(fragment\)\);/);
  // The scrolling viewport is keyboard-reachable and announces itself.
  const workbenchTag = html.match(/<div id="spatialWorkbench"[^>]*>/s)?.[0] ?? "";
  assert.match(workbenchTag, /tabindex="0"/);
  assert.match(workbenchTag, /role="region"/);
  assert.match(workbenchTag, /aria-label="Spatial workbench/);
  // The Mine label is three-way: drafts present / all published / nothing
  // loaded at all.
  assert.match(app, /nothing loaded yet; paste or create a fragment to begin/);
});

test("one queue drain at a time; busy cards hold interaction for real", () => {
  // joinAllWires spans many awaits — a second press mid-drain must not
  // re-plan and double-execute the queue…
  assert.match(app, /if \(joinAllRunning\) return;/);
  assert.match(app, /joinAllRunning = true;\s*\n\s*try \{/);
  assert.match(app, /\} finally \{\s*\n\s*joinAllRunning = false;/);
  // …and a card with an in-flight backend call takes no clicks/drags/focus.
  assert.match(app, /node\.inert = true;/);
});

test("a join absorbed by its operand reports itself instead of looking broken", () => {
  // ⊥ ⊔ x = x: the result dedupes onto an operand's card, so every join
  // path routes its outcome through the reporter…
  assert.equal((app.match(/reportJoinOutcome\((?:joined|result),/g) ?? []).length, 4);
  // …which states the containment in the status bar and pulses the
  // surviving card with the ink-toned success cousin of the red pulse.
  assert.match(app, /nothing new to add/);
  assert.match(app, /"join absorbed — nothing new", "absorbed"/);
  assert.match(styles, /\.session-wire-absorbed\s*\{[\s\S]*?session-wire-absorbed-pulse/);
  assert.match(styles, /\.session-wire-reason-absorbed\s*\{/);
});

test("settleJoin retires operands but never advances a register", () => {
  const start = app.indexOf("function settleJoin");
  const end = app.indexOf("// --- contextual enablement", start);
  assert.ok(start >= 0 && end > start, "settleJoin slice is bounded");
  const settle = app.slice(start, end);
  // Registers change only through an explicit write gesture — the
  // fragment-into-session and session-merge paths call writeSessionContent
  // themselves. A plain fragment join must not promote a bystander register
  // that happens to hold an operand as its content…
  assert.doesNotMatch(settle, /writeSessionContent/);
  // …and the retire guard keeps such register-owned operands alive.
  assert.match(settle, /sessionObject\.contentKey === key/);
  assert.match(settle, /continue/);
});

test("a lone side subtotal is elided — it would repeat the grand total", () => {
  // Per-side rule: a group shows a side's subtotal only when that side is
  // split across groups; a footer with nothing to say is skipped.
  assert.match(app, /group\.inputs\.length > 0 && inputGroupCount > 1/);
  assert.match(app, /group\.outputs\.length > 0 && outputGroupCount > 1/);
  assert.match(app, /showInputSubtotal \|\| showOutputSubtotal/);
  // The old all-or-nothing gate (groups.length > 1) is gone.
  assert.doesNotMatch(app, /card\.groups\.length > 1/);
});

test("coins wear the mockup's boundary", () => {
  // The mockup draws every coin as rect.coin-body (thin ink stroke,
  // radius 5); the session coin items carry the CSS equivalent.
  assert.match(styles, /\.session-coin-item\s*\{[\s\S]*?border:\s*1px solid[\s\S]*?border-radius:\s*5px;/);
});

test("the wire drag advertises itself when idle", () => {
  // Grab cursor on every wireable card…
  assert.match(styles, /\[data-wire-kind\]\s*\{[^}]*cursor:\s*grab;/);
  assert.match(styles, /\[data-wire-kind\]:active\s*\{[^}]*cursor:\s*grabbing;/);
  // …and a standing hint in the status bar instead of a hidden one.
  assert.match(app, /session-wire-status-idle/);
  assert.match(app, /drag a card onto another to wire them/);
  // The hint only shows when something DRAGGABLE exists: standing wire
  // nodes that are hidden (the in-drawer create target) or live in the
  // drawer bar must not advertise the gesture on an empty page.
  assert.match(app, /\[data-wire-kind\]:not\(\[data-drawer\]\):not\(\[hidden\]\)/);
});

test("one Esc closes one surface, topmost first", () => {
  // A single unified handler: modal dialogs cancel natively, then a live
  // drag, then the samples popover, then the open drawer — never several
  // layers on one keystroke.
  assert.equal((app.match(/key === "Escape"/g) ?? []).length, 0);
  assert.equal((app.match(/key !== "Escape"/g) ?? []).length, 1);
  const esc = app.slice(app.indexOf('if (event.key !== "Escape") return;'));
  const handler = esc.slice(0, esc.indexOf("});"));
  const order = [
    handler.indexOf('querySelector("dialog[open]")'),
    handler.indexOf("cancelWireDrag()"),
    handler.indexOf("setSamplesPopover(false)"),
    handler.indexOf("setDrawer(null)"),
  ];
  assert.ok(order.every((at) => at >= 0), "all four layers are consulted");
  assert.deepEqual([...order].sort((a, b) => a - b), order, "consulted topmost-first");
});

test("the sync dropdown consumes the capability catalog, not a local mapping", () => {
  // Version gate: an unknown catalog version degrades to everything-enabled.
  assert.match(app, /CAPABILITY_CATALOG_VERSION = 1/);
  assert.match(app, /catalog\?\.version !== CAPABILITY_CATALOG_VERSION/);
  // Copy is branched on the typed reason codes, never parsed from prose.
  assert.match(app, /reasonCode === "feature-disabled"/);
  assert.match(app, /reasonCode === "unauthored"/);
  // The hand-maintained transport→feature table (and its webrtc_rs key
  // mismatch) is gone: kinds arrive under their select-value names.
  assert.doesNotMatch(app, /SYNC_TRANSPORT_CAPABILITY/);
  assert.doesNotMatch(app, /webrtc_rs/);
});

test("a live wire drag survives concurrent renders and finishes", () => {
  // The gesture must NOT die when a probe or sync settles mid-drag: cards
  // are rebuilt per render, so only the arming pointerdown lives on the
  // card — move/finish are document-level and read the off-DOM state.
  const arm = app.slice(app.indexOf("function armWireDrag"), app.indexOf("function wireDragMove"));
  assert.doesNotMatch(arm, /addEventListener\("pointer(?:move|up|cancel)"/);
  assert.match(app, /document\.addEventListener\("pointermove", wireDragMove\)/);
  assert.match(app, /document\.addEventListener\("pointerup", \(event\) => finishWireDrag\(event, true\)\)/);
  assert.match(app, /document\.addEventListener\("pointercancel", \(event\) => finishWireDrag\(event, false\)\)/);
  // render() repaints the targets onto the fresh cards instead of
  // cancelling the gesture.
  assert.doesNotMatch(app, /if \(wireDrag\) cancelWireDrag\(\);/);
  assert.match(app, /if \(wireDrag\?\.active\) paintWireTargets\(\);/);
  // A finished drag releases capture only if the source card survived, and
  // eats the trailing click so the release does not toggle selection.
  const finish = app.slice(app.indexOf("function finishWireDrag"), app.indexOf("// Completing a wire gesture"));
  assert.match(finish, /node\.isConnected && node\.hasPointerCapture/);
  assert.match(finish, /suppressNextClick = true/);
});

test("wire drops magnet to the nearest compatible target", () => {
  // Near-miss releases land: the drop and the hover preview share one
  // snap search, so what lights up is what a release hits.
  const snap = app.slice(app.indexOf("function snapWireTarget"), app.indexOf("function cancelWireDrag"));
  assert.match(snap, /WIRE_SNAP_RADIUS_PX/);
  // Direct hits win; the search only ever ATTRACTS compatible targets.
  assert.match(snap, /const direct = wireTargetAt\(x, y\);\n  if \(direct\) return direct;/);
  assert.match(snap, /wireDisposition\(wireVerdict\(source, ref, objects\)\) !== "compatible"/);
  // Hidden nodes (closed drawers) never attract.
  assert.match(snap, /rect\.width === 0 \|\| rect\.height === 0/);
  // Both the finish and the hover preview go through the magnet.
  const move = app.slice(app.indexOf("function wireDragMove"), app.indexOf("function finishWireDrag"));
  assert.match(move, /snapWireTarget\(event\.clientX, event\.clientY, wireDrag\.ref\)/);
  const finish = app.slice(app.indexOf("function finishWireDrag"), app.indexOf("// Completing a wire gesture"));
  assert.match(finish, /snapWireTarget\(event\.clientX, event\.clientY, ref\)/);
});

test("the create form is a reachable drop target for utxo drags", () => {
  // Declared wire node: drops resolve via closest("[data-wire-kind]").
  assert.match(html, /id="createWireTarget"[^>]*data-wire-kind="create"[^>]*data-wire-key="create"/);
  // Unhidden imperatively while a utxo drag is live (render cannot run
  // mid-drag), re-hidden when the paint clears.
  const paint = app.slice(app.indexOf("function paintWireTargets"), app.indexOf("function clearWirePaint"));
  assert.match(paint, /createWireTarget"\)\.hidden = wire\.source\.kind !== "utxo"/);
});
