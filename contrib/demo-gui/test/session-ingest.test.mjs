import test from "node:test";
import assert from "node:assert/strict";

import { classifyPaste, mintFromPaste, SAMPLE_PASTES } from "../dist/session/ingest.js";
import { emptyObjects } from "../dist/session/wiring.js";

const PSBT_B64 = "cHNidP8BAgQCAAAAAQMEAAAAAAEEAQABBQEAAQb8BHBzYnQBAA==";
// The same bytes as hex (psbt\xff magic).
const PSBT_HEX = Buffer.from(PSBT_B64, "base64").toString("hex");

test("classifyPaste: payment URIs win and carry parsed fields", () => {
  const paste = classifyPaste(" bitcoin:bcrt1qexample?amount=0.001&label=lunch ");
  assert.equal(paste.kind, "payment-uri");
  assert.equal(paste.payload, "bitcoin:bcrt1qexample?amount=0.001&label=lunch");
  assert.match(paste.detail, /txout intent/);
  assert.match(paste.detail, /bcrt1qexample/);
  assert.match(paste.detail, /100000 sats/);
  assert.equal(paste.needsBackend, null);

  // No amount named: the detail says so (PSBTv2 outputs need one, so the
  // shell prompts before minting the fragment).
  const bare = classifyPaste("bitcoin:bcrt1qexample");
  assert.equal(bare.kind, "payment-uri");
  assert.match(bare.detail, /no amount/);
});

test("classifyPaste: descriptors, private and public", () => {
  const pub = classifyPaste("wpkh(xpub6BosfCnifzxcFwrSzQiqu2DBVTshkCXacvNsWGYJVVhhawA7d4R5WSWGFNbi8Aw6ZRc1brxMyWMzG3DSSSSoekkudhUd9yLb6qx39T9nMdj/0/*)#checksum");
  assert.equal(pub.kind, "descriptor");
  assert.match(pub.detail, /public output descriptor/);
  // Miniscript validation/derivation now folds in via Backend.classifyPaste.
  assert.equal(pub.needsBackend, null);

  const priv = classifyPaste("tr(xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi/86h/0h/0h/0/*)");
  assert.equal(priv.kind, "descriptor");
  assert.match(priv.detail, /private output descriptor/);
});

test("classifyPaste: peer identities (npub, iroh ticket)", () => {
  const npub = classifyPaste("contact me at npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg thanks");
  assert.equal(npub.kind, "npub");
  assert.equal(npub.payload, "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg");
  assert.equal(npub.needsBackend, null);

  const ticket = classifyPaste(`doc${"a".repeat(64)}`);
  assert.equal(ticket.kind, "iroh-ticket");
  assert.match(ticket.detail, /iroh document ticket/);
});

test("classifyPaste: PSBTs in base64 and hex both canonicalize to base64", () => {
  const b64 = classifyPaste(`  ${PSBT_B64}\n`);
  assert.equal(b64.kind, "psbt");
  assert.equal(b64.payload, PSBT_B64);

  const hex = classifyPaste(PSBT_HEX.toUpperCase());
  assert.equal(hex.kind, "psbt");
  assert.equal(hex.payload, PSBT_B64);
  assert.match(hex.detail, /hex PSBT/);
});

test("classifyPaste: versioned tx hex is a transaction (deep decode via the seam)", () => {
  // A minimal-looking segwit tx skeleton: version 2 LE + filler to pass the
  // size floor. Shallow classification looks at charset + version only.
  const txHex = "02000000" + "00".repeat(96);
  const tx = classifyPaste(txHex);
  assert.equal(tx.kind, "transaction-hex");
  assert.equal(tx.payload, txHex);
  // The tx decode now folds in via Backend.classifyPaste.
  assert.equal(tx.needsBackend, null);

  // Version 0 is not a real tx version: falls through to unknown.
  assert.equal(classifyPaste("00000000" + "00".repeat(96)).kind, "unknown");
  // Too short to be a signed tx.
  assert.equal(classifyPaste("02000000ffff").kind, "unknown");
});

test("classifyPaste: unknown pastes name everything that was tried", () => {
  const unknown = classifyPaste("hello world");
  assert.equal(unknown.kind, "unknown");
  assert.match(unknown.detail, /bitcoin: URI/);
  assert.match(unknown.detail, /descriptor/);
  assert.match(unknown.detail, /npub/);
  assert.match(unknown.detail, /iroh ticket/);
  assert.match(unknown.detail, /PSBT/);
  assert.match(unknown.detail, /transaction hex/);

  assert.equal(classifyPaste("   ").kind, "unknown");
  assert.equal(classifyPaste("").detail, "empty paste");
});

test("every sample paste classifies as its declared kind", () => {
  // The palette promises the operator a specific object kind; a sample that
  // drifts away from classifyPaste would fill the box with a lie.
  assert.ok(SAMPLE_PASTES.length >= 6, "palette covers the recognized kinds");
  const names = new Set();
  const kinds = new Set();
  for (const sample of SAMPLE_PASTES) {
    const classified = classifyPaste(sample.value);
    assert.equal(classified.kind, sample.kind, `sample "${sample.name}"`);
    assert.notEqual(sample.kind, "unknown", `sample "${sample.name}" must be recognizable`);
    assert.ok(sample.name.trim(), "samples are named");
    names.add(sample.name);
    kinds.add(sample.kind);
  }
  assert.equal(names.size, SAMPLE_PASTES.length, "sample names are unique");
  // One of each recognizable kind, at minimum.
  for (const kind of ["psbt", "payment-uri", "descriptor", "npub", "iroh-ticket", "transaction-hex"]) {
    assert.ok(kinds.has(kind), `palette has a ${kind} sample`);
  }
});

test("mintFromPaste routes classifications into the object graph", () => {
  let state = emptyObjects();

  // Fragment-producing kinds are the shell's: a payment URI mints no graph
  // node here — the shell creates a one-output PSBT through the backend.
  const payment = mintFromPaste(state, classifyPaste("bitcoin:bcrt1qdest?sats=42000"));
  assert.equal(payment.minted, null);
  assert.deepEqual(payment.state, state);

  const descriptor = mintFromPaste(state, classifyPaste("wpkh(xpub6Bos/0/*)"));
  state = descriptor.state;
  assert.deepEqual(descriptor.minted, { kind: "descriptor", key: "descriptor-1" });
  assert.equal(state.descriptors[0].isPrivate, false);

  const peer = mintFromPaste(state, classifyPaste("npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg"));
  state = peer.state;
  assert.deepEqual(peer.minted, { kind: "peer", key: "peer-2" });
  assert.equal(state.peers[0].transport, "nostr");

  const iroh = mintFromPaste(state, classifyPaste(`doc${"b".repeat(60)}`));
  state = iroh.state;
  assert.equal(state.peers[1].transport, "iroh");

  const tx = mintFromPaste(state, classifyPaste("01000000" + "11".repeat(80)));
  state = tx.state;
  assert.deepEqual(tx.minted, { kind: "utxo", key: "utxo-4" });
  assert.match(tx.log, /outputs decode via classifyPaste/);

  // PSBTs and unknowns mint nothing (the fragment set owns PSBTs).
  const psbt = mintFromPaste(state, classifyPaste(PSBT_B64));
  assert.equal(psbt.minted, null);
  assert.deepEqual(psbt.state, state);
  const unknown = mintFromPaste(state, classifyPaste("garbage"));
  assert.equal(unknown.minted, null);
});
