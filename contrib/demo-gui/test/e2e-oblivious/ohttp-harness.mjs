// contrib/demo-gui/test/e2e-oblivious/ohttp-harness.mjs
//
// Lives in e2e-oblivious/ (NOT e2e/): see assertions.mjs header.
//
// Orchestration layer ON TOP of the EXISTING harness.mjs. Stands up the GENUINE
// oblivious signaling path (a real payjoin directory + a real OHTTP relay),
// spawns the rust str0m peer, serves the WASM PWA to headless Chromium, and
// captures the logs the A1/A2/A3 assertions need.
//
// Reuses harness.mjs's `loadChromium` + `startStaticServer` VERBATIM (D9) — those
// two must be exported from harness.mjs (currently module-private; the
// integration adds `export` to both, non-behaviourally). No parallel Chromium /
// static-server plumbing is introduced.
//
// THE ENFORCEMENT RULE that makes obliviousness real (D1): neither peer is ever
// given the directory address as something to DIAL. Both get only:
//   - an OHTTP relay origin (the only host either peer's socket connects to),
//   - the directory's OHTTP gateway key (to HPKE-seal requests), and
//   - the room shared-secret (to derive mailbox slot IDs).
// The relay is the ONLY process that ever connects to the directory. There is no
// signaling-server URL in existence to accidentally use.

import { spawn } from "node:child_process";
import net from "node:net";
import { mkdtemp, readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { assert, loadChromium, startStaticServer } from "../e2e/harness.mjs";

/** Ask the OS for a free loopback TCP port, then release it. */
async function freePort() {
  const srv = net.createServer();
  await new Promise((resolve) => srv.listen(0, "127.0.0.1", resolve));
  const { port } = srv.address();
  await new Promise((resolve) => srv.close(resolve));
  return port;
}

/** Poll a TCP port until it accepts a connection (like regtest-lib.sh's bitcoind wait). */
async function waitReady(port, label, tries = 80, gapMs = 250) {
  for (let i = 0; i < tries; i += 1) {
    const ok = await new Promise((resolve) => {
      const s = net.connect(port, "127.0.0.1");
      s.once("connect", () => {
        s.destroy();
        resolve(true);
      });
      s.once("error", () => resolve(false));
    });
    if (ok) return;
    await new Promise((resolve) => setTimeout(resolve, gapMs));
  }
  throw new Error(`ASSERT FAILED: ${label} on 127.0.0.1:${port} never became ready`);
}

/** Spawn a child, teeing stdout/stderr into `sink` (array of strings). */
function spawnLogged(bin, args, env, sink) {
  assert(bin, `harness: missing binary path for ${JSON.stringify(args)}`);
  const child = spawn(bin, args, { env: { ...process.env, ...env } });
  child.stdout.on("data", (d) => sink.push(String(d)));
  child.stderr.on("data", (d) => sink.push(String(d)));
  return child;
}

/** Scan a process's captured log for a `PREFIX<value>` line and return <value>. */
function grabFromLog(sink, prefix, timeoutMs = 20_000) {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    const tick = () => {
      const hit = sink.find((line) => line.includes(prefix));
      if (hit) return resolve(hit.slice(hit.indexOf(prefix) + prefix.length).trim());
      if (Date.now() > deadline) return reject(new Error(`log never emitted ${prefix}`));
      setTimeout(tick, 100);
    };
    tick();
  });
}

// The directory test binary is asked to log, per request, the SOURCE socket it
// saw (A3.1: must ALWAYS be the relay) and to dump each raw stored slot body
// (A2 + A3.2: must be opaque HPKE ciphertext, never PSBT/SDP plaintext). Env
// DIR_REQUEST_LOG / DIR_SLOT_DUMP point at files/dirs the harness reads back.
export async function withObliviousSession(body) {
  const chromium = await loadChromium();

  const dirPort = await freePort();
  const relayPort = await freePort();
  const peerUdpPort = await freePort(); // rust peer's str0m UDP bind

  const dirLog = [];
  const relayLog = [];
  const peerLog = []; // carries the E2E_RUST_* lines

  const workDir = await mkdtemp(path.join(os.tmpdir(), "webrtc-e2e-"));
  const dirRequestLogPath = path.join(workDir, "dir-requests.log");
  const dirSlotDumpDir = path.join(workDir, "dir-slots");

  const roomSecretHex = process.env.E2E_ROOM_SECRET;
  assert(/^[0-9a-f]{64}$/.test(roomSecretHex || ""), "E2E_ROOM_SECRET must be 32-byte lowercase hex");

  const directoryUrl = `http://127.0.0.1:${dirPort}`;
  const relayOrigin = `http://127.0.0.1:${relayPort}`;

  let dir = null;
  let relay = null;
  let peer = null;
  let browser = null;
  let server = null;

  try {
    // 1) Real payjoin test directory. Logs request sources + dumps stored slots.
    dir = spawnLogged(
      process.env.PAYJOIN_DIRECTORY_BIN,
      ["--listen", `127.0.0.1:${dirPort}`],
      { DIR_REQUEST_LOG: dirRequestLogPath, DIR_SLOT_DUMP: dirSlotDumpDir },
      dirLog,
    );
    await waitReady(dirPort, "payjoin directory");
    // The directory publishes its OHTTP gateway key on startup; peers seal to it.
    const gatewayKeyHex = await grabFromLog(dirLog, "OHTTP_GATEWAY_KEY=");

    // 2) Real OHTTP relay -> directory. THE ONLY process that connects to the dir.
    relay = spawnLogged(
      process.env.OHTTP_RELAY_BIN,
      ["--listen", `127.0.0.1:${relayPort}`, "--gateway", directoryUrl],
      {},
      relayLog,
    );
    await waitReady(relayPort, "ohttp relay");

    // 3) Rust str0m peer (offerer). Given ONLY relay + gateway key + room secret
    //    (+ the dir URL the RELAY forwards to; the peer never dials it).
    peer = spawnLogged(
      process.env.PTJ_E2E_PEER_BIN,
      [
        "--role", "offerer",
        "--directory-url", directoryUrl,
        "--ohttp-relay", relayOrigin,
        "--ohttp-gateway-key", gatewayKeyHex,
        "--room", roomSecretHex,
        "--fragment", process.env.E2E_FRAGMENT_RUST,
        "--udp-bind", `127.0.0.1:${peerUdpPort}`,
      ],
      {},
      peerLog,
    );
    peer.on("exit", (code) => {
      if (code !== 0 && code !== null) peerLog.push(`ERR peer exited with code ${code}\n`);
    });

    // 4) Serve the built WASM PWA and drive it as the ANSWERER via Chromium.
    //    Same static server + Chromium plumbing as the demo-gui-playwright check.
    server = await startStaticServer(process.env.PWA_DIST);
    browser = await chromium.launch({
      executablePath: process.env.CHROMIUM_BIN,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    });
    const page = await browser.newPage({ viewport: { width: 390, height: 844 } }); // mobile-first
    const pageErrors = [];
    const consoleMessages = [];
    page.on("pageerror", (e) => pageErrors.push(String(e)));
    page.on("console", (m) => consoleMessages.push(`${m.type()}: ${m.text()}`));

    // Inject the SAME three signaling params the rust peer got — and NO signaling
    // URL. The PWA's WebRtcTransport refuses to run without a signaling-ohttp
    // channel, so there is no direct-server fallback to leak the client IP.
    const qs = new URLSearchParams({
      room: roomSecretHex,
      directoryUrl,
      ohttpRelay: relayOrigin,
      ohttpGatewayKey: gatewayKeyHex,
      role: "answerer",
      fragment: process.env.E2E_FRAGMENT_BROWSER_B64,
    });
    await page.goto(`${server.origin}/index.html?${qs}`, { waitUntil: "load" });

    // Hand the assertion body the page + all captured logs + artifact paths.
    await body(page, {
      pageErrors,
      consoleMessages,
      peerLog,
      dirLog,
      relayLog,
      dirRequestLogPath,
      dirSlotDumpDir,
      relayOrigin,
      relayPort,
      directoryUrl,
      roomSecretHex,
    });
  } finally {
    if (browser) await browser.close();
    if (server) await server.close();
    for (const child of [peer, relay, dir]) {
      try {
        child?.kill("SIGTERM");
      } catch {
        /* best-effort teardown */
      }
    }
  }
}

// Re-export for the spec's convenience (it imports the assert helper too).
export { assert, readFile };
