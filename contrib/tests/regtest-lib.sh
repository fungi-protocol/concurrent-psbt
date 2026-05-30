# shellcheck shell=bash
# Shared regtest bitcoind setup for integration tests.
# Source this file: source "$(dirname "$0")/regtest-lib.sh"
#
# Provides:
#   DATADIR  — temporary data directory (cleaned up on EXIT)
#   CLI      — bitcoin-cli invocation with regtest + auth flags
#
# Callers that need additional cleanup can redefine the trap after
# sourcing, calling regtest_cleanup from their own handler.

DATADIR=$(mktemp -d)

regtest_cleanup() {
  bitcoin-cli -datadir="$DATADIR" -regtest -rpcuser=test -rpcpassword=test stop 2>/dev/null || true
  sleep 1
  rm -rf "$DATADIR"
}
trap regtest_cleanup EXIT

# ── Start regtest bitcoind ──────────────────────────────────────────

bitcoind -datadir="$DATADIR" -regtest -daemon -txindex \
  -rpcuser=test -rpcpassword=test -fallbackfee=0.00001 \
  -blockfilterindex=0 -peerblockfilters=0 -coinstatsindex=0

CLI="bitcoin-cli -datadir=$DATADIR -regtest -rpcuser=test -rpcpassword=test"

for _ in $(seq 1 30); do
  if $CLI getblockchaininfo >/dev/null 2>&1; then break; fi
  sleep 0.5
done
$CLI getblockchaininfo >/dev/null || {
  echo "FAIL: bitcoind did not start"
  exit 1
}
