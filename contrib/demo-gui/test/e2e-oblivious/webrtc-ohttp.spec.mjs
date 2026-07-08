// contrib/demo-gui/test/e2e-oblivious/webrtc-ohttp.spec.mjs
//
// The top-level e2e spec. Runs under `node webrtc-ohttp.spec.mjs`, exactly like
// the existing session-ordering.spec.mjs. Ties the harness + assertions
// together and asserts A1/A2/A3.
//
// It is DELIBERATELY NOT under contrib/demo-gui/test/e2e/*.spec.mjs: the
// mainline demo-gui-playwright check loops test/e2e/*.spec.mjs, and this spec
// needs the rust peer + payjoin directory + OHTTP relay binaries that check
// must not require. The Stage-2 `webrtc-e2e-live` nix check (frontier
// aggregate) is the intended runner; see README.md here for the manual run.

import { withObliviousSession } from "./ohttp-harness.mjs";
import {
  assertConvergence,
  assertDataPathIsP2P,
  assertDirectoryOblivious,
  LOOPBACK_NOTE,
} from "./assertions.mjs";

const bitcoinCli = {
  bin: process.env.BITCOIN_CLI_BIN || "bitcoin-cli",
  args: (process.env.BITCOIN_CLI_ARGS || "-regtest -rpcuser=test -rpcpassword=test")
    .split(" ")
    .filter(Boolean),
};

// The fixture's expected converged shape, exported by webrtc-e2e-fixtures.sh as
// JSON so A1's non-triviality check (D6) is anchored to the real two-fragment join.
const expectedShape = JSON.parse(process.env.E2E_EXPECTED_SHAPE);

await withObliviousSession(async (page, ctx) => {
  const { pageErrors, peerLog, dirRequestLogPath, dirSlotDumpDir, relayPort } = ctx;

  // A1 — convergence on both sides: byte-equal joins + structural decode equality
  //      + non-triviality against the fixture's expected shape.
  const { rustB64 } = await assertConvergence({ page, peerLog, bitcoinCli, expectedShape });

  // A2 — the PSBT never traversed the directory (it flowed P2P over the data
  //      channel); no PSBT magic/base64/converged bytes in any stored slot.
  await assertDataPathIsP2P({ dirSlotDumpDir, convergedB64: rustB64 });

  // A3 — the directory is oblivious: relay-only request source, opaque ciphertext
  //      slots, YET SDP/ICE genuinely round-tripped (path exercised, not bypassed).
  await assertDirectoryOblivious({ dirRequestLogPath, dirSlotDumpDir, relayPort, peerLog });

  // The PWA must not have thrown (a thrown error could mask a bypassed path).
  if (pageErrors.length) {
    throw new Error(`PWA raised uncaught page errors:\n${pageErrors.join("\n")}`);
  }

  console.log("PASS webrtc-ohttp e2e: converged both sides; data path P2P; directory oblivious.");
  console.log(LOOPBACK_NOTE);
});
