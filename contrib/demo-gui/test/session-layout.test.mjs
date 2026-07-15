import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../session.html", import.meta.url), "utf8");
const app = readFileSync(new URL("../src/session/app.ts", import.meta.url), "utf8");
const styles = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

test("the primary objects share one bounded spatial workbench", () => {
  const workbenchStart = html.indexOf('id="spatialWorkbench"');
  const workbenchEnd = html.indexOf("</main>", workbenchStart);
  const utilities = html.indexOf('id="sessionUtilities"');

  assert.ok(workbenchStart >= 0, "the spatial workbench is present");
  assert.ok(workbenchEnd > workbenchStart, "the spatial workbench is bounded");
  const workbench = html.slice(workbenchStart, workbenchEnd);
  assert.ok(
    workbench.indexOf('data-spatial-region="peers"') <
      workbench.indexOf('data-spatial-region="sessions"'),
    "peers are above sessions inside the workbench",
  );
  assert.ok(
    workbench.indexOf('data-spatial-region="sessions"') <
      workbench.indexOf('data-spatial-region="me"'),
    "sessions are above the Me workspace inside the workbench",
  );
  assert.ok(utilities > workbenchEnd, "secondary utilities follow the workbench");
  assert.match(
    styles,
    /\.session-spatial-workbench\s*\{[\s\S]*?flex-direction:\s*column;/,
    "the workbench stacks its shelves as a column",
  );
  assert.match(
    styles,
    /\.session-spatial-workbench\s*>\s*\[data-spatial-region="me"\]\s*\{[\s\S]*?flex:\s*1 1 auto;/,
    "the Me region absorbs the workbench's remaining height",
  );
});

test("the Mine strip is the bottom band and owns an empty workbench", () => {
  // Overview renders published-session areas first and Mine last…
  const overview = app.slice(app.indexOf("function renderFragments"));
  const sessionsLoop = overview.indexOf("renderSessionArea(sessionObject, members)");
  const mineAppend = overview.indexOf("renderMineArea(");
  assert.ok(sessionsLoop >= 0 && mineAppend > sessionsLoop, "Mine renders after the session areas");
  // …as full-width strips with Mine pinned to the bottom of the Me region.
  assert.match(styles, /\.session-area-list\s*\{[\s\S]*?flex-direction:\s*column;/);
  assert.match(styles, /\.session-area-list\s*>\s*\.session-mine-area\s*\{\s*margin-top:\s*auto;/);
  // With no peers and no sessions the shelves collapse and Mine expands.
  assert.match(app, /"session-workbench-solo",\s*objects\.peers\.length === 0 && objects\.sessions\.length === 0/);
  assert.match(styles, /\.session-workbench-solo \.session-area-list\s*>\s*\.session-mine-area\s*\{[\s\S]*?flex:\s*1 0 auto;/);
  // The band is layout only — no nested pseudo-peer: local/unpublished is
  // the default, so it wears no title and no badge (published areas do).
  const mine = app.slice(app.indexOf("function renderMineArea"), app.indexOf("function renderSessionArea"));
  assert.doesNotMatch(mine, /item-title/);
  assert.doesNotMatch(mine, /badge\(/);
});

test("the real shell keeps peers above sessions and the Me workspace", () => {
  const peers = html.indexOf('data-spatial-region="peers"');
  const sessions = html.indexOf('data-spatial-region="sessions"');
  const me = html.indexOf('data-spatial-region="me"');

  assert.ok(peers >= 0, "peer shelf is present");
  assert.ok(sessions > peers, "sessions follow peers");
  assert.ok(me > sessions, "Me/local-only workspace follows sessions");
  assert.match(html, /id="peerShelfList"/);
  assert.match(app, /renderPeerShelf\(\)/);
});

test("single-session focus hides both overview shelves", () => {
  const peerShelf = html.match(/<section id="peerShelf"[^>]*>/)?.[0] ?? "";
  const sessionShelf = html.match(/<section id="sessionShelf"[^>]*>/)?.[0] ?? "";
  assert.match(peerShelf, /data-focus-hide/);
  assert.match(sessionShelf, /data-focus-hide/);
});

test("peer cards preserve bridging while Pair stays visibly unavailable", () => {
  // Bridging rides the always-on drag gesture: decorateWireTarget arms the
  // drag on single peers AND bridge-group cards, and the queue chip keeps
  // their pending-wire participation visible.
  assert.match(app, /decorateWireTarget\(item, \{ kind: "peer", key: peer\.key \}\)/);
  assert.match(app, /wireQueueChip\(\{ kind: "peer", key: peer\.key \}\)/);
  assert.match(app, /decorateWireTarget\(item, \{ kind: "peer", key: members\[0\]\.key \}\)/);
  assert.match(app, /wireQueueChip\(\{ kind: "peer", key: members\[0\]\.key \}\)/);
  assert.match(app, /armWireDrag\(node, ref\)/);
  assert.match(app, /Pair unavailable until the ptj adapter exposes session pairing/);
});

test("session shelf cards show members and explicit sync — never a transport", () => {
  const start = app.indexOf("function renderSessionShelf");
  const end = app.indexOf("function unavailablePairButton", start);
  assert.ok(start >= 0 && end > start, "session-shelf renderer is present");
  const sessionShelf = app.slice(start, end);
  // Sessions have no transport (peers do): the card must not claim one.
  assert.doesNotMatch(sessionShelf, /sessionObject\.transport/);
  assert.match(sessionShelf, /sessionObject\.fragmentKeys\.join\(", "\)/);
  assert.match(sessionShelf, /button\("Sync now"/);
});

test("one shared bottom Add drawer owns quick paste and manual peer creation", () => {
  assert.equal((html.match(/id="pasteInput"/g) ?? []).length, 1);
  assert.match(html, /id="addDrawer"[^>]*class="[^"]*session-add-drawer/);
  assert.match(html, /id="manualPeerForm"/);
  assert.match(html, /id="manualPeerTransport"/);
  for (const transport of ["nostr", "iroh", "str0m", "webrtc-rs"]) {
    assert.match(html, new RegExp(`<option value="${transport}">`));
  }
  assert.match(app, /Pair unavailable until the ptj adapter exposes session pairing/);
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
  const card = app.slice(cardStart, app.indexOf("function renderFragmentGroup", cardStart) > 0 ? app.indexOf("function renderFragmentGroup", cardStart) : cardStart + 4000);
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

test("a join absorbed by its operand reports itself instead of looking broken", () => {
  // ⊥ ⊔ x = x: the result dedupes onto an operand's card, so every join
  // path routes its outcome through the reporter…
  assert.equal((app.match(/reportJoinOutcome\(joined,/g) ?? []).length, 3);
  // …which states the containment in the status bar and pulses the
  // surviving card with the ink-toned success cousin of the red pulse.
  assert.match(app, /nothing new to add/);
  assert.match(app, /"join absorbed — nothing new", "absorbed"/);
  assert.match(styles, /\.session-wire-absorbed\s*\{[\s\S]*?session-wire-absorbed-pulse/);
  assert.match(styles, /\.session-wire-reason-absorbed\s*\{/);
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

test("a live wire drag survives concurrent renders by cancelling cleanly", () => {
  // render() must not orphan a captured drag node (wireDrag would never
  // clear and all future gestures would bail on it).
  assert.match(app, /function render\(\): void \{\n[^]*?if \(wireDrag\) cancelWireDrag\(\);/);
  // Cancelling releases the capture and eats the trailing click so the
  // release does not toggle the source card's selection.
  const cancel = app.slice(app.indexOf("function cancelWireDrag"), app.indexOf("function armWireDrag"));
  assert.match(cancel, /releasePointerCapture\(wireDrag\.pointerId\)/);
  assert.match(cancel, /suppressNextClick = true/);
});

test("the create form is a reachable drop target for utxo drags", () => {
  // Declared wire node: drops resolve via closest("[data-wire-kind]").
  assert.match(html, /id="createWireTarget"[^>]*data-wire-kind="create"[^>]*data-wire-key="create"/);
  // Unhidden imperatively while a utxo drag is live (render cannot run
  // mid-drag), re-hidden when the paint clears.
  const paint = app.slice(app.indexOf("function paintWireTargets"), app.indexOf("function clearWirePaint"));
  assert.match(paint, /createWireTarget"\)\.hidden = wire\.source\.kind !== "utxo"/);
});
