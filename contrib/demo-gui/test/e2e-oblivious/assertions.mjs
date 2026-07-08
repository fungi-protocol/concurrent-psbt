// contrib/demo-gui/test/e2e-oblivious/assertions.mjs
//
// Lives in e2e-oblivious/ (NOT e2e/): the demo-gui-playwright check runs every
// test/e2e/*.spec.mjs, and this suite needs the rust peer + directory + relay
// binaries the mainline check must not require.
//
// A1/A2/A3 assertion helpers for the WebRTC-over-OHTTP e2e. Pure functions over
// the captured logs + page + dumped directory slots; they perform NO network of
// their own (only `bitcoin-cli decodepsbt`, a local process).

import { readFile, readdir } from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { assert } from "../e2e/harness.mjs";

const run = promisify(execFile);

// Plaintext markers that MUST NOT appear in any directory-visible byte stream.
const PSBT_MAGIC_HEX = "70736274ff"; // binary PSBT magic  "psbt\xff"
const PSBT_B64_PREFIX = "cHNidP8"; // base64("psbt\xff") — start of every ptj PSBT
// Canonical SDP / ICE plaintext tokens. If ANY appears in a stored slot, the
// payload was not HPKE-sealed (OHTTP was bypassed).
const SDP_ICE_MARKERS = ["v=0", "m=application", "a=candidate", "a=fingerprint", "a=ice-ufrag"];

/** Pull the value after `PREFIX` from an array of log lines (asserts it exists). */
function grabLine(log, prefix) {
  const hit = log.map(String).find((line) => line.includes(prefix));
  assert(hit, `expected a log line containing ${prefix}`);
  return hit.slice(hit.indexOf(prefix) + prefix.length).trim();
}

/** decodepsbt a base64 PSBT into a structural shape {inputs, outputs, total}. */
async function decodeShape(bitcoinCli, b64) {
  const { stdout } = await run(bitcoinCli.bin, [...bitcoinCli.args, "decodepsbt", b64]);
  const d = JSON.parse(stdout);
  return {
    inputs: d.tx.vin.length,
    outputs: d.tx.vout.length,
    total: d.tx.vout.reduce((sum, o) => sum + o.value, 0),
  };
}

// ---- A1: convergence on BOTH sides ------------------------------------------
export async function assertConvergence({ page, peerLog, bitcoinCli, expectedShape }) {
  // Browser side: the PWA exposes the converged PSBT on a stable DOM hook once
  // its local join_psbts folds in the peer's fragment.
  await page.waitForFunction(
    () => {
      const el = document.querySelector("#e2e-converged");
      return el && el.textContent && el.textContent.trim().length > 10;
    },
    null,
    { timeout: 45_000 },
  );
  const browserB64 = await page.evaluate(() =>
    document.querySelector("#e2e-converged").textContent.trim(),
  );

  // Rust side: the E2E_RUST_CONVERGED= stdout line.
  const rustB64 = grabLine(peerLog, "E2E_RUST_CONVERGED=");

  // (a) Byte-equal independent joins: the two peers folded the same fragments
  //     with the same engine, so the canonical base64 must match exactly.
  assert(
    browserB64 === rustB64,
    `converged PSBTs differ:\n browser=${browserB64}\n rust   =${rustB64}`,
  );

  // (b) Structural equality via bitcoind decodepsbt (guards against a degenerate
  //     empty join passing (a)). Both decode to the same input/output/total shape.
  const sb = await decodeShape(bitcoinCli, browserB64);
  const sr = await decodeShape(bitcoinCli, rustB64);
  assert(
    JSON.stringify(sb) === JSON.stringify(sr),
    `structural mismatch browser=${JSON.stringify(sb)} rust=${JSON.stringify(sr)}`,
  );

  // (c) Non-triviality (D6): the fold is over a genuine TWO-fragment fixture, so
  //     the converged shape must match the fixture's expected non-empty shape —
  //     not a degenerate 0-input/0-output PSBT that would satisfy (a)+(b).
  assert(
    JSON.stringify(sb) === JSON.stringify(expectedShape),
    `converged shape ${JSON.stringify(sb)} != fixture expected ${JSON.stringify(expectedShape)}`,
  );

  return { browserB64, rustB64, shape: sb };
}

// ---- A2: PSBT bytes flowed P2P, NOT through the directory --------------------
export async function assertDataPathIsP2P({ dirSlotDumpDir, convergedB64 }) {
  const files = await readdir(dirSlotDumpDir).catch(() => []);
  for (const f of files) {
    const buf = await readFile(`${dirSlotDumpDir}/${f}`);
    const hex = buf.toString("hex");
    const txt = buf.toString("latin1");
    assert(
      !hex.includes(PSBT_MAGIC_HEX),
      `PSBT binary magic found in directory slot ${f} — data path leaked through the directory`,
    );
    assert(
      !txt.includes(PSBT_B64_PREFIX),
      `PSBT base64 prefix found in directory slot ${f} — data path leaked through the directory`,
    );
    // The exact converged bytes must not appear anywhere the directory can see.
    // A prefix slice is enough: the whole thing is HPKE ciphertext, so even a
    // fragment of the plaintext PSBT showing up is a leak.
    assert(
      !txt.includes(convergedB64.slice(0, 24)),
      `converged-PSBT bytes appeared in directory slot ${f}`,
    );
  }
  // Only a handful of SIGNALING slots should exist (offer, answer, a few ICE).
  // A directory carrying the whole PSBT stream would have far more / bigger slots.
  assert(
    files.length > 0 && files.length <= 32,
    `unexpected directory slot count ${files.length} (expected a handful of signaling slots)`,
  );
}

// ---- A3: the directory is oblivious -----------------------------------------
export async function assertDirectoryOblivious({
  dirRequestLogPath,
  dirSlotDumpDir,
  relayPort,
  peerLog,
}) {
  // (1) Every request the directory saw came FROM THE RELAY, never a peer's
  //     client socket. On loopback all source IPs are 127.0.0.1, so we assert on
  //     the SOURCE PORT: it must always be the relay's, never a peer ephemeral.
  const reqLog = (await readFile(dirRequestLogPath, "utf8")).split("\n").filter(Boolean);
  assert(
    reqLog.length > 0,
    "directory saw no requests — signaling was bypassed (a localhost SDP relay?)",
  );
  for (const line of reqLog) {
    // The directory logs "src=<ip>:<port>" per request.
    const m = line.match(/src=([\d.]+):(\d+)/);
    assert(m, `directory request log line has no src=: ${line}`);
    assert(
      Number(m[2]) === Number(relayPort),
      `directory saw a NON-RELAY source ${m[1]}:${m[2]} — a peer's IP/port leaked (OHTTP bypassed)`,
    );
  }

  // (2) Stored slot bodies are opaque OHTTP/HPKE ciphertext: no SDP/ICE plaintext.
  //     This is the IP-INDEPENDENT strong claim (holds regardless of loopback):
  //     the directory literally cannot read what it relays.
  const files = await readdir(dirSlotDumpDir).catch(() => []);
  assert(files.length > 0, "no stored slots — signaling did not go through the directory");
  for (const f of files) {
    const txt = (await readFile(`${dirSlotDumpDir}/${f}`)).toString("latin1");
    for (const marker of SDP_ICE_MARKERS) {
      assert(
        !txt.includes(marker),
        `SDP/ICE plaintext marker "${marker}" in directory slot ${f} — payload was NOT HPKE-sealed`,
      );
    }
  }

  // (3) YET real SDP/ICE plaintext round-tripped end to end: the rust peer parsed
  //     a genuine SDP and >= 1 ICE candidate. Proves the oblivious path was
  //     EXERCISED, not skipped by a mock that emits nothing to the directory.
  assert(grabLine(peerLog, "E2E_RUST_SDP_PARSED=") === "ok", "rust peer never parsed a real SDP");
  const iceN = Number(grabLine(peerLog, "E2E_RUST_ICE_CANDIDATES="));
  assert(
    Number.isInteger(iceN) && iceN >= 1,
    `rust peer parsed ${iceN} ICE candidates (expected >= 1)`,
  );
}

// ---- honesty note surfaced in the check output ------------------------------
export const LOOPBACK_NOTE =
  "NOTE (loopback limitation): this is a single-host fixture. IP-hiding is asserted at " +
  "socket/port/process granularity (the directory's request source is ALWAYS the relay's " +
  "port; no peer ephemeral port ever appears — A3.1). The IP-INDEPENDENT strong claim is " +
  "ciphertext-opacity (A3.2): the directory literally cannot read the SDP/ICE. True " +
  "network-level IP separation would need a multi-host / network-namespace fixture (future work).";
