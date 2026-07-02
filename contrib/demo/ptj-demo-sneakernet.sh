#!/usr/bin/env bash
set -euo pipefail

KEEP_NODE_RUNNING=0
if [ "${1:-}" = "--keep-node-running" ]; then
  KEEP_NODE_RUNNING=1
  shift
fi

if [ "$#" -ne 1 ]; then
  echo "usage: ptj-demo-sneakernet [--keep-node-running] OUTPUT_DIR" >&2
  exit 2
fi

OUT_DIR=$1
SEED="deadbeefdeadbeefdeadbeefdeadbeef"
RPC_USER="test"
RPC_PASSWORD="test"

mkdir -p "$OUT_DIR"
OUT_DIR=$(cd "$OUT_DIR" && pwd)
if [ -n "$(find "$OUT_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
  echo "error: $OUT_DIR is not empty; choose an empty output directory" >&2
  exit 1
fi

BITCOIN_DATADIR="$OUT_DIR/bitcoin"
CLI=(bitcoin-cli -datadir="$BITCOIN_DATADIR" -regtest -rpcuser="$RPC_USER" -rpcpassword="$RPC_PASSWORD")
PTJ_BIN=$(command -v ptj)
BITCOIN_CLI_BIN=$(command -v bitcoin-cli)
JQ_BIN=$(command -v jq)

cleanup_on_error() {
  "${CLI[@]}" stop >/dev/null 2>&1 || true
}

if [ -e "$BITCOIN_DATADIR" ]; then
  echo "error: $BITCOIN_DATADIR already exists; choose an empty output directory" >&2
  exit 1
fi

mkdir -p \
  "$BITCOIN_DATADIR" \
  "$OUT_DIR/alice" \
  "$OUT_DIR/bob" \
  "$OUT_DIR/carol" \
  "$OUT_DIR/shared" \
  "$OUT_DIR/joined"

trap cleanup_on_error EXIT

bitcoind -datadir="$BITCOIN_DATADIR" -regtest -daemon -txindex \
  -rpcuser="$RPC_USER" -rpcpassword="$RPC_PASSWORD" -fallbackfee=0.00001 \
  -blockfilterindex=0 -peerblockfilters=0 -coinstatsindex=0

for _ in $(seq 1 30); do
  if "${CLI[@]}" getblockchaininfo >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done
"${CLI[@]}" getblockchaininfo >/dev/null || {
  echo "error: bitcoind did not start" >&2
  exit 1
}

for party in alice bob carol; do
  "${CLI[@]}" createwallet "$party" >/dev/null
done

mine_addr=$("${CLI[@]}" -rpcwallet=alice getnewaddress)
"${CLI[@]}" -rpcwallet=alice generatetoaddress 110 "$mine_addr" >/dev/null

alice_fund=$("${CLI[@]}" -rpcwallet=alice getnewaddress)
bob_fund=$("${CLI[@]}" -rpcwallet=bob getnewaddress)
carol_fund=$("${CLI[@]}" -rpcwallet=carol getnewaddress)
"${CLI[@]}" -rpcwallet=alice sendtoaddress "$alice_fund" 10 >/dev/null
"${CLI[@]}" -rpcwallet=alice sendtoaddress "$bob_fund" 10 >/dev/null
"${CLI[@]}" -rpcwallet=alice sendtoaddress "$carol_fund" 10 >/dev/null
"${CLI[@]}" -rpcwallet=alice generatetoaddress 1 "$mine_addr" >/dev/null

alice_utxo=$("${CLI[@]}" -rpcwallet=alice listunspent | jq -r '[.[] | select(.amount == 10)][0]')
bob_utxo=$("${CLI[@]}" -rpcwallet=bob listunspent | jq -r '.[0]')
carol_utxo=$("${CLI[@]}" -rpcwallet=carol listunspent | jq -r '.[0]')

alice_txid=$(echo "$alice_utxo" | jq -r '.txid')
alice_vout=$(echo "$alice_utxo" | jq -r '.vout')
bob_txid=$(echo "$bob_utxo" | jq -r '.txid')
bob_vout=$(echo "$bob_utxo" | jq -r '.vout')
carol_txid=$(echo "$carol_utxo" | jq -r '.txid')
carol_vout=$(echo "$carol_utxo" | jq -r '.vout')

alice_dest=$("${CLI[@]}" -rpcwallet=alice getnewaddress)
bob_dest=$("${CLI[@]}" -rpcwallet=bob getnewaddress)
carol_dest=$("${CLI[@]}" -rpcwallet=carol getnewaddress)

cat >"$OUT_DIR/wallets.json" <<JSON
{
  "alice": { "funding_address": "$alice_fund", "destination": "$alice_dest" },
  "bob": { "funding_address": "$bob_fund", "destination": "$bob_dest" },
  "carol": { "funding_address": "$carol_fund", "destination": "$carol_dest" }
}
JSON

cat >"$OUT_DIR/utxos.json" <<JSON
{
  "alice": { "txid": "$alice_txid", "vout": $alice_vout },
  "bob": { "txid": "$bob_txid", "vout": $bob_vout },
  "carol": { "txid": "$carol_txid", "vout": $carol_vout }
}
JSON

ptj --binary create --network regtest \
  --input "$alice_txid:$alice_vout" \
  --output "$alice_dest:9.999" \
  --output-file "$OUT_DIR/alice/a.psbt"

ptj --binary create --network regtest \
  --input "$bob_txid:$bob_vout" \
  --output "$bob_dest:9.999" \
  --output-file "$OUT_DIR/bob/b.psbt"

ptj --binary create --network regtest \
  --input "$carol_txid:$carol_vout" \
  --output "$carol_dest:9.999" \
  --output-file "$OUT_DIR/carol/c.psbt"

cp "$OUT_DIR/alice/a.psbt" "$OUT_DIR/shared/alice.psbt"
cp "$OUT_DIR/bob/b.psbt" "$OUT_DIR/shared/bob.psbt"
cp "$OUT_DIR/carol/c.psbt" "$OUT_DIR/shared/carol.psbt"

ptj --binary sync "$OUT_DIR/shared" --state "$OUT_DIR/joined/abc.psbt"
ptj --binary sort --seed "$SEED" "$OUT_DIR/joined/abc.psbt" \
  --output-file "$OUT_DIR/joined/abc-sorted.psbt"
ptj export-bip174 "$OUT_DIR/joined/abc-sorted.psbt" \
  --output-file "$OUT_DIR/joined/abc-bip174.psbt"

decoded=$("${CLI[@]}" decodepsbt "$(cat "$OUT_DIR/joined/abc-bip174.psbt")")
echo "$decoded" | jq -e '.tx.vin | length == 3' >/dev/null
echo "$decoded" | jq -e '.tx.vout | length == 3' >/dev/null
echo "$decoded" | jq -e '([.tx.vout[].value] | add) == 29.997' >/dev/null

psbt=$(cat "$OUT_DIR/joined/abc-bip174.psbt")
for wallet in alice bob carol; do
  processed=$("${CLI[@]}" -rpcwallet="$wallet" walletprocesspsbt "$psbt")
  psbt=$(echo "$processed" | jq -er '.psbt')
done
printf '%s\n' "$psbt" >"$OUT_DIR/joined/abc-signed.psbt"

finalized=$("${CLI[@]}" finalizepsbt "$psbt")
echo "$finalized" | jq -e '.complete == true' >/dev/null
hex=$(echo "$finalized" | jq -r '.hex')
printf '%s\n' "$hex" >"$OUT_DIR/joined/final.hex"

txid=$("${CLI[@]}" sendrawtransaction "$hex" 0)
"${CLI[@]}" generatetoaddress 1 "$mine_addr" >/dev/null
printf '%s\n' "$txid" >"$OUT_DIR/joined/txid.txt"

cat >"$OUT_DIR/expected.json" <<JSON
{
  "inputs": 3,
  "outputs": 3,
  "total_output_btc": 29.997,
  "sort_seed": "$SEED",
  "txid": "$txid"
}
JSON

cat >"$OUT_DIR/env.sh" <<SH
export BITCOIN_DATADIR='$BITCOIN_DATADIR'
export BITCOIN_RPC_USER='$RPC_USER'
export BITCOIN_RPC_PASSWORD='$RPC_PASSWORD'
export PTJ_DEMO_DIR='$OUT_DIR'
export PTJ_BIN='$PTJ_BIN'
export BITCOIN_CLI_BIN='$BITCOIN_CLI_BIN'
export JQ_BIN='$JQ_BIN'

bitcoin_demo_cli() {
  "\$BITCOIN_CLI_BIN" -datadir="\$BITCOIN_DATADIR" -regtest \\
    -rpcuser="\$BITCOIN_RPC_USER" -rpcpassword="\$BITCOIN_RPC_PASSWORD" "\$@"
}
SH

cat >"$OUT_DIR/stop-bitcoind.sh" <<SH
#!/usr/bin/env bash
set -euo pipefail
'$BITCOIN_CLI_BIN' -datadir='$BITCOIN_DATADIR' -regtest -rpcuser='$RPC_USER' -rpcpassword='$RPC_PASSWORD' stop
SH
chmod +x "$OUT_DIR/stop-bitcoind.sh"

trap - EXIT
if [ "$KEEP_NODE_RUNNING" -eq 0 ]; then
  "${CLI[@]}" stop >/dev/null
else
  echo "bitcoind is still running for inspection."
  echo "Stop it with: $OUT_DIR/stop-bitcoind.sh"
fi

echo "Demo artifacts written to $OUT_DIR"
echo "Broadcast transaction: $txid"
