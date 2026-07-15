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
    /\.session-spatial-workbench\s*\{[\s\S]*?grid-template-rows:\s*auto\s+minmax\([^;]+\)\s+auto;/,
  );
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

test("session shelf cards retain transport, members, and explicit sync", () => {
  const start = app.indexOf("function renderSessionShelf");
  const end = app.indexOf("function unavailablePairButton", start);
  assert.ok(start >= 0 && end > start, "session-shelf renderer is present");
  const sessionShelf = app.slice(start, end);
  assert.match(sessionShelf, /sessionObject\.transport/);
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
  // The reason is stashed per button and surfaced by hit-testing the press
  // (disabled buttons swallow their own pointer events).
  assert.match(app, /dataset\.why = why/);
  assert.match(app, /elementsFromPoint\(event\.clientX, event\.clientY\)/);
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
