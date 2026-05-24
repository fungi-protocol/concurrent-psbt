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

decode_psbt() {
  $CLI decodepsbt "$1"
}

psbt_input_count() {
  decode_psbt "$1" | jq '.tx.vin | length'
}

psbt_output_count() {
  decode_psbt "$1" | jq '.tx.vout | length'
}

psbt_total_output_value() {
  decode_psbt "$1" | jq '[.tx.vout[].value] | add'
}

assert_psbt_content() {
  local label="$1"
  local psbt="$2"
  local expected_inputs="$3"
  local expected_outputs="$4"
  local expected_total="$5"
  local decoded
  decoded=$(decode_psbt "$psbt")

  echo "$decoded" | jq -e --argjson expected "$expected_inputs" \
    '.tx.vin | length == $expected' >/dev/null || {
    echo "FAIL: $label expected $expected_inputs inputs"
    echo "$decoded" | jq '.tx.vin | length'
    exit 1
  }

  echo "$decoded" | jq -e --argjson expected "$expected_outputs" \
    '.tx.vout | length == $expected' >/dev/null || {
    echo "FAIL: $label expected $expected_outputs outputs"
    echo "$decoded" | jq '.tx.vout | length'
    exit 1
  }

  echo "$decoded" | jq -e --arg expected "$expected_total" '
    def sats: (. * 100000000 | round);
    ([.tx.vout[].value | sats] | add) == ($expected | tonumber | sats)
  ' >/dev/null || {
    echo "FAIL: $label expected total output $expected_total BTC"
    echo "$decoded" | jq '[.tx.vout[].value] | add'
    exit 1
  }
}

sign_with_wallets() {
  local psbt="$1"
  shift
  local wallet
  for wallet in "$@"; do
    psbt=$($CLI -rpcwallet="$wallet" walletprocesspsbt "$psbt" | jq -r '.psbt')
  done
  printf '%s\n' "$psbt"
}

finalize_and_broadcast_psbt() {
  local psbt="$1"
  local mine_addr="$2"
  local expected_outputs="$3"
  local expected_total="$4"
  local finalized complete hex txid mined raw

  finalized=$($CLI finalizepsbt "$psbt")
  complete=$(echo "$finalized" | jq -r '.complete')
  if [ "$complete" != "true" ]; then
    echo "FAIL: finalizepsbt did not complete"
    echo "$finalized"
    exit 1
  fi

  hex=$(echo "$finalized" | jq -r '.hex')
  txid=$($CLI sendrawtransaction "$hex" 0)
  mined=$($CLI generatetoaddress 1 "$mine_addr")
  raw=$($CLI getrawtransaction "$txid" true)

  echo "$raw" | jq -e --argjson expected "$expected_outputs" \
    '.vout | length == $expected' >/dev/null || {
    echo "FAIL: broadcast tx expected $expected_outputs outputs"
    echo "$raw" | jq '.vout | length'
    exit 1
  }

  echo "$raw" | jq -e --arg expected "$expected_total" '
    def sats: (. * 100000000 | round);
    ([.vout[].value | sats] | add) == ($expected | tonumber | sats)
  ' >/dev/null || {
    echo "FAIL: broadcast tx expected total output $expected_total BTC"
    echo "$raw" | jq '[.vout[].value] | add'
    exit 1
  }

  echo "$mined" | jq -e --arg "$txid" "$txid" 'index($txid) != null' >/dev/null || {
    echo "FAIL: broadcast tx $txid was not mined in the next block"
    exit 1
  }

  printf '%s\n' "$txid"
}
