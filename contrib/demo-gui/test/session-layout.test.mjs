import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../session.html", import.meta.url), "utf8");
const app = readFileSync(new URL("../src/session/app.ts", import.meta.url), "utf8");

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
  assert.match(app, /decorateWireTarget\(item, \{ kind: "peer", key: peer\.key \}\)/);
  assert.match(app, /wireButtonNodes\(\{ kind: "peer", key: peer\.key \}/);
  assert.match(app, /decorateWireTarget\(item, \{ kind: "peer", key: members\[0\]\.key \}\)/);
  assert.match(app, /wireButtonNodes\([\s\S]*members\[0\]\.key/);
  assert.match(app, /Pair unavailable until the ptj adapter exposes session pairing/);
});

test("session shelf cards retain transport, members, and explicit sync", () => {
  assert.match(app, /sessionObject\.transport/);
  assert.match(app, /sessionObject\.fragmentKeys\.join\(", "\)/);
  assert.match(app, /button\("Sync now"/);
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
