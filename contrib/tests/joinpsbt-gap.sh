#!/usr/bin/env bash
# Demonstrate gaps in Bitcoin Core's joinpsbts and combinepsbt that
# concurrent-psbt's lattice-based join addresses.
#
# Gap 1: joinpsbts duplicates outputs — joining a PSBT with itself
#         doubles the outputs, silently creating an overspend.
# Gap 2: combinepsbt rejects PSBTs with different unsigned transactions,
#         so it cannot merge two constructors' contributions.
set -euo pipefail

# shellcheck source=contrib/tests/regtest-lib.sh
# shellcheck disable=SC1091 # treefmt shellcheck does not follow sourced files.
source "$(dirname "$0")/regtest-lib.sh"

WORKDIR=$(mktemp -d)
trap 'regtest_cleanup; rm -rf "$WORKDIR"' EXIT

# ── Create wallet and fund it ──────────────────────────────────────

$CLI createwallet "test" >/dev/null
ADDR=$($CLI -rpcwallet=test getnewaddress)
# Mine 102 blocks so we have at least 2 mature coinbase UTXOs
$CLI -rpcwallet=test generatetoaddress 102 "$ADDR" >/dev/null

UTXOS=$($CLI -rpcwallet=test listunspent 1 9999999 '[]' true)
N_UTXOS=$(echo "$UTXOS" | jq 'length')
if [ "$N_UTXOS" -lt 2 ]; then
  echo "FAIL: need at least 2 UTXOs, got $N_UTXOS"
  exit 1
fi

UTXO_A_TXID=$(echo "$UTXOS" | jq -r '.[0].txid')
UTXO_A_VOUT=$(echo "$UTXOS" | jq -r '.[0].vout')
UTXO_B_TXID=$(echo "$UTXOS" | jq -r '.[1].txid')
UTXO_B_VOUT=$(echo "$UTXOS" | jq -r '.[1].vout')

DEST=$($CLI -rpcwallet=test getnewaddress)

# ── Gap 1: joinpsbts duplicates outputs ────────────────────────────
# Two PSBTs with disjoint inputs but the same output.
# joinpsbts produces 2 copies of the output.

PSBT_A=$($CLI -rpcwallet=test createpsbt \
  "[{\"txid\": \"$UTXO_A_TXID\", \"vout\": $UTXO_A_VOUT}]" \
  "[{\"$DEST\": 1.0}]")

PSBT_B=$($CLI -rpcwallet=test createpsbt \
  "[{\"txid\": \"$UTXO_B_TXID\", \"vout\": $UTXO_B_VOUT}]" \
  "[{\"$DEST\": 1.0}]")

JOINED=$($CLI -rpcwallet=test joinpsbts "[\"$PSBT_A\", \"$PSBT_B\"]")
DECODED=$($CLI -rpcwallet=test decodepsbt "$JOINED")

N_INPUTS=$(echo "$DECODED" | jq '.tx.vin | length')
N_OUTPUTS=$(echo "$DECODED" | jq '.tx.vout | length')

echo "Gap 1: joinpsbts output duplication"
echo "  Two PSBTs, each with 1 input and 1 output (same destination, 1.0 BTC)"
echo "  After joinpsbts: $N_INPUTS inputs, $N_OUTPUTS outputs"

if [ "$N_OUTPUTS" -ne 1 ]; then
  TOTAL=$(echo "$DECODED" | jq '[.tx.vout[].value] | add')
  echo "  DEMONSTRATED: output duplicated. Total output: $TOTAL BTC from 2 inputs."
  echo "  The intent was 1 output of 1.0 BTC, but joinpsbts produced $N_OUTPUTS copies."
else
  echo "  UNEXPECTED: joinpsbts did not duplicate (Bitcoin Core may have changed behavior)"
  exit 1
fi

# ── Gap 2: combinepsbt rejects different transactions ──────────────
# Two PSBTs spending the same input to different destinations.
# A concurrent constructor scenario: Alice adds her output, Bob adds his.

DEST2=$($CLI -rpcwallet=test getnewaddress)

PSBT_ALICE=$($CLI -rpcwallet=test createpsbt \
  "[{\"txid\": \"$UTXO_A_TXID\", \"vout\": $UTXO_A_VOUT}]" \
  "[{\"$DEST\": 1.0}]")

PSBT_BOB=$($CLI -rpcwallet=test createpsbt \
  "[{\"txid\": \"$UTXO_A_TXID\", \"vout\": $UTXO_A_VOUT}]" \
  "[{\"$DEST\": 1.0}, {\"$DEST2\": 1.0}]")

echo
echo "Gap 2: combinepsbt rejects non-identical transactions"

if COMBINED=$($CLI -rpcwallet=test combinepsbt "[\"$PSBT_ALICE\", \"$PSBT_BOB\"]" 2>&1); then
  echo "  UNEXPECTED: combinepsbt succeeded"
  echo "$COMBINED"
  exit 1
else
  echo "  Two PSBTs: same input, but Bob added an extra output."
  echo "  combinepsbt rejects them because the unsigned transactions differ."
  echo "  DEMONSTRATED: combinepsbt cannot merge concurrent constructor contributions."
fi

echo
echo "ptj positive controls"

ptj create --network regtest \
  --input "$UTXO_A_TXID:$UTXO_A_VOUT" \
  --output "$DEST:1.0" >"$WORKDIR/ptj-a.psbt"

ptj join "$WORKDIR/ptj-a.psbt" "$WORKDIR/ptj-a.psbt" >"$WORKDIR/ptj-aa.psbt"
PTJ_AA=$(cat "$WORKDIR/ptj-aa.psbt")
assert_psbt_content "ptj join(A, A)" "$PTJ_AA" 1 1 1.0
echo "  ptj join(A, A): idempotent, no duplicated output"

ptj create --network regtest \
  --input "$UTXO_A_TXID:$UTXO_A_VOUT" \
  --output "$DEST:1.0" >"$WORKDIR/ptj-alice.psbt"

ptj create --network regtest \
  --input "$UTXO_A_TXID:$UTXO_A_VOUT" \
  --output "$DEST2:1.0" >"$WORKDIR/ptj-bob.psbt"

ptj join "$WORKDIR/ptj-alice.psbt" "$WORKDIR/ptj-bob.psbt" >"$WORKDIR/ptj-alice-bob.psbt"
PTJ_ALICE_BOB=$(cat "$WORKDIR/ptj-alice-bob.psbt")
assert_psbt_content "ptj same-input output union" "$PTJ_ALICE_BOB" 1 2 2.0
echo "  ptj join(Alice, Bob): same input retained once; distinct outputs unioned"

echo
echo "concurrent-psbt addresses both gaps:"
echo "  - Join is idempotent: join(A, A) = A (no output duplication)"
echo "  - Inputs keyed by outpoint, outputs by unique ID (merge by identity)"
echo "  - Different outputs are unioned, not rejected"
echo "  - Conflicts are detected and preserved, not silently resolved"

# shellcheck disable=SC2154 # Nix sets $out for the treefmt check derivation.
mkdir -p "$out"
