import test from "node:test";
import assert from "node:assert/strict";

import {
  TABLEAU10,
  descriptorColorKey,
  groupColorKey,
  paletteColor,
  paletteRegistry,
  peerColorKey,
} from "../dist/session/palette.js";

test("TABLEAU10 is ten distinct hex colors", () => {
  assert.equal(TABLEAU10.length, 10);
  assert.equal(new Set(TABLEAU10).size, 10);
  for (const color of TABLEAU10) {
    assert.match(color, /^#[0-9a-f]{6}$/);
  }
});

test("palette assignment is first-seen stable", () => {
  const registry = paletteRegistry();
  const first = paletteColor(registry, "descriptor:wpkh(xpub...)");
  // Later arrivals never move an assigned color.
  paletteColor(registry, "peer:alice");
  paletteColor(registry, "peer:bob");
  assert.equal(paletteColor(registry, "descriptor:wpkh(xpub...)"), first);
  // Re-reading in any order is stable too.
  const alice = paletteColor(registry, "peer:alice");
  paletteColor(registry, "template:p2tr");
  assert.equal(paletteColor(registry, "peer:alice"), alice);
});

test("distinct identities get distinct colors until the palette wraps", () => {
  const registry = paletteRegistry();
  const colors = Array.from({ length: 10 }, (_, index) => paletteColor(registry, `key-${index}`));
  assert.equal(new Set(colors).size, 10);
  // The eleventh identity wraps to the first color; the first key keeps it.
  assert.equal(paletteColor(registry, "key-10"), colors[0]);
  assert.equal(paletteColor(registry, "key-0"), colors[0]);
});

test("groupColorKey: descriptor identity where derivable, else neutral", () => {
  assert.equal(groupColorKey({ kind: "provenance", key: "peer:alice" }), "peer:alice");
  assert.equal(groupColorKey({ kind: "script-template", key: "template:p2wpkh" }), "template:p2wpkh");
  assert.equal(groupColorKey({ kind: "unattributed", key: "unattributed" }), null);
});

test("descriptor and peer keys follow immutable identity rather than mutable labels", () => {
  // The normalized public form wins once enrichment lands, so re-pasting
  // the same descriptor (or its private form) keeps the color.
  assert.equal(
    descriptorColorKey({ descriptor: "wpkh(xprv...)", normalized: "wpkh(xpub...)" }),
    "descriptor:wpkh(xpub...)",
  );
  assert.equal(
    descriptorColorKey({ descriptor: "wpkh(xpub...)", normalized: null }),
    "descriptor:wpkh(xpub...)",
  );
  const original = peerColorKey({ name: "alice", transport: "nostr", identity: "npub1alice" });
  const relabeled = peerColorKey({ name: "lunch coordinator", transport: "nostr", identity: "npub1alice" });
  assert.equal(original, "peer:nostr:npub1alice");
  assert.equal(relabeled, original);
  assert.notEqual(
    peerColorKey({ name: "alice", transport: "iroh", identity: "npub1alice" }),
    original,
  );
});
