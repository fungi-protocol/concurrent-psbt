#!/usr/bin/env bash
# contrib/tests/webrtc-e2e-fixtures.sh
#
# Generate the two complementary PSBT fragments for the WebRTC-over-OHTTP e2e, on
# regtest, using the BUILT `ptj` (on PATH via the nix check's ${ptj-bin}). It
# reuses the existing regtest harness (regtest-lib.sh, already nix-check-wired via
# joinpsbt-gap / sneakernet-lattice / ptj-sneakernet) so the fixtures match the
# canonical two-party split: two distinct funded inputs, two distinct outputs,
# joined into ONE non-trivial converged PSBT (guards A1 against a degenerate join).
#
# This script is SOURCED by the Stage-2 nix check AFTER bitcoind is up. It exports
# the env the node harness + assertions read:
#   E2E_ROOM_SECRET          32-byte hex mailbox-derivation salt (NOT a wallet key)
#   E2E_FRAGMENT_RUST        path to the rust peer's fragment (base64 PSBT file)
#   E2E_FRAGMENT_BROWSER_B64 the browser peer's fragment, inline base64 (URL param)
#   E2E_EXPECTED_SHAPE       JSON {inputs,outputs,total} of the expected join (A1 (c))
#   BITCOIN_CLI_BIN / _ARGS  so assertions.mjs can decodepsbt both results
#
# NB: bitcoind is left RUNNING (regtest-lib.sh installs the EXIT-trap cleanup) so
# the node spec can `bitcoin-cli decodepsbt` the converged PSBTs from both peers.

set -euo pipefail

# shellcheck source=contrib/tests/regtest-lib.sh
# shellcheck disable=SC1091 # treefmt shellcheck does not follow sourced files.
source "$(dirname "$0")/regtest-lib.sh" # brings up bitcoind + $CLI, cleans up on EXIT

WORKDIR=$(mktemp -d)
# Chain our cleanup onto regtest-lib.sh's (which trapped EXIT to stop bitcoind).
trap 'regtest_cleanup; rm -rf "$WORKDIR"' EXIT

# Deterministic 32-byte room secret shared by both peers. It is ONLY a salt for
# deriving mailbox slot IDs H(DOMAIN||secret||role||index); it is never a key.
export E2E_ROOM_SECRET="00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"

# ── Fund two distinct UTXOs, one per peer (mirrors ptj-sneakernet.sh) ──────────
$CLI createwallet e2e >/dev/null
MINE_ADDR=$($CLI -rpcwallet=e2e getnewaddress)
$CLI -rpcwallet=e2e generatetoaddress 110 "$MINE_ADDR" >/dev/null

# Two separate confirmed UTXOs so the join folds two genuinely distinct inputs.
FUND_A=$($CLI -rpcwallet=e2e getnewaddress)
FUND_B=$($CLI -rpcwallet=e2e getnewaddress)
$CLI -rpcwallet=e2e sendtoaddress "$FUND_A" 10 >/dev/null
$CLI -rpcwallet=e2e sendtoaddress "$FUND_B" 10 >/dev/null
$CLI -rpcwallet=e2e generatetoaddress 1 "$MINE_ADDR" >/dev/null

pick_utxo() { # addr -> "txid:vout"
  $CLI -rpcwallet=e2e listunspent 1 9999 "[\"$1\"]" |
    jq -er '.[0] | "\(.txid):\(.vout)"'
}
UTXO_A=$(pick_utxo "$FUND_A")
UTXO_B=$(pick_utxo "$FUND_B")

DEST_A=$($CLI -rpcwallet=e2e getnewaddress) # rust peer's output
DEST_B=$($CLI -rpcwallet=e2e getnewaddress) # browser peer's output

# ── Build the two complementary fragments with the built ptj ──────────────────
# Each peer contributes one input + one output (a coinjoin-style fragment); the
# lattice fold unions them into a 2-in/2-out converged PSBT.
FRAG_RUST="$WORKDIR/fragment.rust.psbt"
FRAG_BROWSER="$WORKDIR/fragment.browser.psbt"
ptj create --network regtest --input "$UTXO_A" --output "$DEST_A:9.999" >"$FRAG_RUST"
ptj create --network regtest --input "$UTXO_B" --output "$DEST_B:9.999" >"$FRAG_BROWSER"

# ── Anchor the expected converged shape (A1 non-triviality, D6) ────────────────
# Compute the join here with the SAME engine both peers will use, then record its
# decoded shape so the spec can assert the live convergence is non-degenerate.
EXPECTED_PSBT=$(ptj join "$FRAG_RUST" "$FRAG_BROWSER")
EXPECTED_DECODED=$(decode_psbt "$EXPECTED_PSBT") # regtest-lib.sh helper
E2E_EXPECTED_SHAPE=$(printf '%s' "$EXPECTED_DECODED" | jq -c '{
  inputs: (.tx.vin | length),
  outputs: (.tx.vout | length),
  total: ([.tx.vout[].value] | add),
}')
export E2E_EXPECTED_SHAPE
# Fail the check early if the fixture is degenerate (must be a real 2-in/2-out).
printf '%s' "$E2E_EXPECTED_SHAPE" | jq -e '.inputs == 2 and .outputs == 2' >/dev/null || {
  echo "FAIL: fixture converged shape is not the expected 2-in/2-out: $E2E_EXPECTED_SHAPE" >&2
  exit 1
}

# ── Export for the node harness ───────────────────────────────────────────────
export E2E_FRAGMENT_RUST="$FRAG_RUST"
E2E_FRAGMENT_BROWSER_B64="$(cat "$FRAG_BROWSER")" # ptj create emits base64
export E2E_FRAGMENT_BROWSER_B64
export BITCOIN_CLI_BIN="bitcoin-cli"
export BITCOIN_CLI_ARGS="-datadir=$DATADIR -regtest -rpcuser=test -rpcpassword=test"

echo "fixtures ready: rust=$FRAG_RUST browser=<inline b64>; shape=$E2E_EXPECTED_SHAPE; room=$E2E_ROOM_SECRET"
