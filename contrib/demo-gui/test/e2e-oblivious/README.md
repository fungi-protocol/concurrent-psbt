# e2e-oblivious — browser-WebRTC ⇄ rust-str0m over the oblivious directory

The end-to-end test for the frontier constraint: two peers (headless-Chromium
PWA answerer, rust `ptj-e2e-peer` str0m offerer) exchange WebRTC SDP/ICE ONLY
through a BIP-77 payjoin directory reached via an OHTTP relay — never a
direct/localhost signaling server — then converge a two-fragment PSBT over the
P2P data channel and prove it (A1 convergence both sides, A2 the PSBT never
traversed the directory, A3 the directory is oblivious: relay-only request
sources + opaque HPKE ciphertext slots, YET real SDP/ICE round-tripped).

This directory is deliberately NOT `test/e2e/` — the mainline
`demo-gui-playwright` check runs every `test/e2e/*.spec.mjs` and must not
require this suite's binaries.

## Status: wired [WIP]-manual (the live nix check is a deferred stub)

`nix/checks.nix` has two stages (see DECISIONS D8 in the staging record):

* `webrtc-e2e-authored` — GREEN now. Cheap guard: the harness/spec parse, the
  gating greps hold (rust peer stays required-features-gated; the deferred
  crate keeps its TODO(ground-deps)), and the anti-false-positive assertions
  (`assertDirectoryOblivious`, `assertDataPathIsP2P`) cannot be deleted
  without failing it. Joins quick/lint/nightly.
* `webrtc-e2e-live` — a deliberately FAILING deferred stub in the `frontier`
  aggregate only (off quick/lint/nightly). It cannot run yet because:
  1. transport-payjoin-dir's network deps are un-wired (the probed
     payjoin 0.25 API has no directory-mailbox client; imp.rs needs a rewrite);
  2. `ptj-e2e-peer --features e2e-peer` is therefore unbuildable (and its
     source still codes against the pre-async channel seam);
  3. no `payjoin-directory` / `ohttp-relay` packages exist in the pinned
     nixpkgs to stand up the real oblivious path (mocking it would be exactly
     the false positive A3 exists to prevent).

## Manual run (once the deps ground)

1. Build the peer: `cargo build -p ptj-e2e-peer --features e2e-peer`.
2. Build the PWA bundle (`crates/concurrent-psbt-wasm/build-wasm.sh` + the
   PWA shell) into a static dir; export `PWA_DIST=<that dir>`.
3. Provide `PAYJOIN_DIRECTORY_BIN` (rust-payjoin v2 test directory, patched to
   log `src=<ip>:<port>` per request into `DIR_REQUEST_LOG` and dump raw slot
   bodies into `DIR_SLOT_DUMP`) and `OHTTP_RELAY_BIN`.
4. Start regtest + fixtures: `source contrib/tests/webrtc-e2e-fixtures.sh`
   (exports E2E_ROOM_SECRET, E2E_FRAGMENT_*, E2E_EXPECTED_SHAPE,
   BITCOIN_CLI_*).
5. Export the Playwright env the demo-gui-playwright check uses
   (PLAYWRIGHT_CORE, CHROMIUM_BIN) and `PTJ_E2E_PEER_BIN`, then:
   `node contrib/demo-gui/test/e2e-oblivious/webrtc-ohttp.spec.mjs`.

The single-host loopback limitation is documented in `assertions.mjs`
(`LOOPBACK_NOTE`): IP-hiding is asserted at socket/port granularity; the
IP-independent strong claim is ciphertext opacity.
