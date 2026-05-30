#!/usr/bin/env bash
# Demonstrate lattice property violations in a sneakernet scenario.
#
# Three parties (Alice, Bob, Carol) construct a coinjoin. Messages
# propagate asynchronously on USB sticks. Some copies are partial
# merges that share overlapping content.
#
# The lattice structure:
#
#        A       B       C          (each party's contribution)
#       / \     / \     / \
#     AB   AC  AB  BC  AC  BC      (pairwise merges via joinpsbts)
#      \       /       \   /
#       (AB)+C          A+(BC)     (3-way via sequential pairwise)
#
# In a correct semilattice: all paths to ABC produce the same result,
# and re-merging a copy that's already incorporated is a no-op.
# joinpsbts violates both properties.
set -euo pipefail

# shellcheck source=contrib/tests/regtest-lib.sh
# shellcheck disable=SC1091 # treefmt shellcheck does not follow sourced files.
source "$(dirname "$0")/regtest-lib.sh"

WORKDIR=$(mktemp -d)
trap 'regtest_cleanup; rm -rf "$WORKDIR"' EXIT

# ── Setup: 3 wallets, each with a funded UTXO ─────────────────────

for PARTY in alice bob carol; do
  $CLI createwallet "$PARTY" >/dev/null
done

MINE_ADDR=$($CLI -rpcwallet=alice getnewaddress)
$CLI -rpcwallet=alice generatetoaddress 110 "$MINE_ADDR" >/dev/null

BOB_FUND=$($CLI -rpcwallet=bob getnewaddress)
CAROL_FUND=$($CLI -rpcwallet=carol getnewaddress)
$CLI -rpcwallet=alice sendtoaddress "$BOB_FUND" 10 >/dev/null
$CLI -rpcwallet=alice sendtoaddress "$CAROL_FUND" 10 >/dev/null
$CLI -rpcwallet=alice generatetoaddress 1 "$MINE_ADDR" >/dev/null

ALICE_UTXO=$($CLI -rpcwallet=alice listunspent | jq -r '[.[] | select(.amount >= 10)][0]')
BOB_UTXO=$($CLI -rpcwallet=bob listunspent | jq -r '.[0]')
CAROL_UTXO=$($CLI -rpcwallet=carol listunspent | jq -r '.[0]')

A_TXID=$(echo "$ALICE_UTXO" | jq -r '.txid')
A_VOUT=$(echo "$ALICE_UTXO" | jq -r '.vout')
B_TXID=$(echo "$BOB_UTXO" | jq -r '.txid')
B_VOUT=$(echo "$BOB_UTXO" | jq -r '.vout')
C_TXID=$(echo "$CAROL_UTXO" | jq -r '.txid')
C_VOUT=$(echo "$CAROL_UTXO" | jq -r '.vout')

ALICE_DEST=$($CLI -rpcwallet=alice getnewaddress)
BOB_DEST=$($CLI -rpcwallet=bob getnewaddress)
CAROL_DEST=$($CLI -rpcwallet=carol getnewaddress)

# ── Each party creates their PSBT ─────────────────────────────────

PSBT_A=$($CLI createpsbt \
  "[{\"txid\": \"$A_TXID\", \"vout\": $A_VOUT}]" \
  "[{\"$ALICE_DEST\": 9.999}]")

PSBT_B=$($CLI createpsbt \
  "[{\"txid\": \"$B_TXID\", \"vout\": $B_VOUT}]" \
  "[{\"$BOB_DEST\": 9.999}]")

PSBT_C=$($CLI createpsbt \
  "[{\"txid\": \"$C_TXID\", \"vout\": $C_VOUT}]" \
  "[{\"$CAROL_DEST\": 9.999}]")

echo "=== Sneakernet Lattice Test ==="
echo
echo "Three parties (Alice, Bob, Carol), each contributes 1 input + 1 output."
echo

count_outputs() {
  $CLI decodepsbt "$1" | jq '.tx.vout | length'
}

count_inputs() {
  $CLI decodepsbt "$1" | jq '.tx.vin | length'
}

total_output_value() {
  $CLI decodepsbt "$1" | jq '[.tx.vout[].value] | add'
}

PROBLEMS=0

# ── Scenario 1: Idempotence ───────────────────────────────────────
# Alice has the merged ABC. Bob hands her his copy again (redundant).
# A correct join: join(ABC, B) = ABC. joinpsbts: fails.

echo "--- Scenario 1: Idempotence (redundant message) ---"
echo "Alice has ABC. Bob hands her his USB stick again."

JOIN_ABC=$($CLI joinpsbts "[\"$PSBT_A\", \"$PSBT_B\", \"$PSBT_C\"]")
echo "  ABC: $(count_inputs "$JOIN_ABC") inputs, $(count_outputs "$JOIN_ABC") outputs"

if JOIN_ABC_B=$($CLI joinpsbts "[\"$JOIN_ABC\", \"$PSBT_B\"]" 2>&1); then
  NO=$(count_outputs "$JOIN_ABC_B")
  echo "  join(ABC, B): $(count_inputs "$JOIN_ABC_B") inputs, $NO outputs"
  if [ "$NO" -ne 3 ]; then
    echo "  PROBLEM: Bob's output was duplicated"
    PROBLEMS=$((PROBLEMS + 1))
  fi
else
  echo "  join(ABC, B): REJECTED (overlapping inputs)"
  echo "  PROBLEM: cannot safely re-merge a redundant copy"
  PROBLEMS=$((PROBLEMS + 1))
fi

echo

# ── Scenario 2: Convergence of partial merges ─────────────────────
# Alice has AB (met Bob first). Carol has BC (met Bob first).
# They meet and merge their copies. Bob's contribution is in both.

echo "--- Scenario 2: Merging overlapping partial merges ---"
echo "Alice has AB, Carol has BC. They meet and merge."

JOIN_AB=$($CLI joinpsbts "[\"$PSBT_A\", \"$PSBT_B\"]")
JOIN_BC=$($CLI joinpsbts "[\"$PSBT_B\", \"$PSBT_C\"]")

echo "  AB: $(count_inputs "$JOIN_AB") inputs, $(count_outputs "$JOIN_AB") outputs"
echo "  BC: $(count_inputs "$JOIN_BC") inputs, $(count_outputs "$JOIN_BC") outputs"

if JOIN_AB_BC=$($CLI joinpsbts "[\"$JOIN_AB\", \"$JOIN_BC\"]" 2>&1); then
  NI=$(count_inputs "$JOIN_AB_BC")
  NO=$(count_outputs "$JOIN_AB_BC")
  TOTAL=$(total_output_value "$JOIN_AB_BC")
  echo "  join(AB, BC): $NI inputs, $NO outputs, total value: $TOTAL BTC"
  if [ "$NO" -ne 3 ]; then
    echo "  PROBLEM: expected 3 outputs, got $NO"
    echo "  Bob's output appears in both AB and BC."
    echo "  joinpsbts duplicated it, creating $TOTAL BTC in outputs"
    echo "  from ~30 BTC in inputs."
    PROBLEMS=$((PROBLEMS + 1))
  fi
else
  echo "  join(AB, BC): REJECTED (overlapping inputs)"
  echo "  PROBLEM: Bob's input is in both copies, merge rejected"
  PROBLEMS=$((PROBLEMS + 1))
fi

echo

# ── Scenario 3: Full duplication via gossip ───────────────────────
# Worst case: each pair meets independently, then all three partial
# merges are combined. Every output is duplicated.

echo "--- Scenario 3: Gossip network (all pairwise merges combined) ---"
echo "AB, AC, and BC all exist. Someone tries to combine all three."

JOIN_AC=$($CLI joinpsbts "[\"$PSBT_A\", \"$PSBT_C\"]")

if JOIN_ALL=$($CLI joinpsbts "[\"$JOIN_AB\", \"$JOIN_AC\", \"$JOIN_BC\"]" 2>&1); then
  NI=$(count_inputs "$JOIN_ALL")
  NO=$(count_outputs "$JOIN_ALL")
  TOTAL=$(total_output_value "$JOIN_ALL")
  echo "  join(AB, AC, BC): $NI inputs, $NO outputs, total value: $TOTAL BTC"
  if [ "$NO" -ne 3 ]; then
    echo "  PROBLEM: expected 3 outputs, got $NO"
    echo "  Each party's output appeared in 2 of the 3 pairwise merges."
    echo "  joinpsbts produced $NO outputs totaling $TOTAL BTC"
    echo "  from ~30 BTC in inputs."
    PROBLEMS=$((PROBLEMS + 1))
  fi
else
  echo "  join(AB, AC, BC): REJECTED (overlapping inputs)"
  echo "  PROBLEM: cannot combine overlapping partial merges"
  PROBLEMS=$((PROBLEMS + 1))
fi

echo
echo "--- Summary ---"
echo

if [ "$PROBLEMS" -gt 0 ]; then
  echo "$PROBLEMS problem(s) demonstrated."
  echo
  echo "In a sneakernet coinjoin, copies propagate unpredictably."
  echo "joinpsbts cannot handle:"
  echo "  - Redundant copies (not idempotent, or rejects overlapping inputs)"
  echo "  - Overlapping partial merges (duplicates outputs or rejects)"
  echo "  - Gossip-style propagation (each pair's merge has shared content)"
  echo
  echo "concurrent-psbt's join is a semilattice:"
  echo "  join(A, A) = A                    (idempotent)"
  echo "  join(A, B) = join(B, A)            (commutative)"
  echo "  join(join(A, B), C) = join(A, join(B, C))  (associative)"
  echo "  => all merge paths converge to the same result"
else
  echo "No problems detected (unexpected)."
  exit 1
fi

echo
echo "--- ptj positive control: same lattice converges ---"
echo

ptj create --network regtest \
  --input "$A_TXID:$A_VOUT" \
  --output "$ALICE_DEST:9.999" >"$WORKDIR/ptj-a.psbt"

ptj create --network regtest \
  --input "$B_TXID:$B_VOUT" \
  --output "$BOB_DEST:9.999" >"$WORKDIR/ptj-b.psbt"

ptj create --network regtest \
  --input "$C_TXID:$C_VOUT" \
  --output "$CAROL_DEST:9.999" >"$WORKDIR/ptj-c.psbt"

ptj join "$WORKDIR/ptj-a.psbt" "$WORKDIR/ptj-b.psbt" >"$WORKDIR/ptj-ab.psbt"
ptj join "$WORKDIR/ptj-b.psbt" "$WORKDIR/ptj-c.psbt" >"$WORKDIR/ptj-bc.psbt"
ptj join "$WORKDIR/ptj-a.psbt" "$WORKDIR/ptj-c.psbt" >"$WORKDIR/ptj-ac.psbt"

ptj join "$WORKDIR/ptj-a.psbt" "$WORKDIR/ptj-b.psbt" "$WORKDIR/ptj-c.psbt" \
  >"$WORKDIR/ptj-abc-direct.psbt"
ptj join "$WORKDIR/ptj-ab.psbt" "$WORKDIR/ptj-c.psbt" >"$WORKDIR/ptj-abc-via-ab-c.psbt"
ptj join "$WORKDIR/ptj-a.psbt" "$WORKDIR/ptj-bc.psbt" >"$WORKDIR/ptj-abc-via-a-bc.psbt"
ptj join "$WORKDIR/ptj-ac.psbt" "$WORKDIR/ptj-b.psbt" >"$WORKDIR/ptj-abc-via-ac-b.psbt"
ptj join "$WORKDIR/ptj-abc-direct.psbt" "$WORKDIR/ptj-abc-direct.psbt" \
  >"$WORKDIR/ptj-abc-idem.psbt"
ptj join "$WORKDIR/ptj-abc-direct.psbt" "$WORKDIR/ptj-a.psbt" \
  >"$WORKDIR/ptj-abc-plus-a.psbt"
ptj join "$WORKDIR/ptj-ab.psbt" "$WORKDIR/ptj-bc.psbt" "$WORKDIR/ptj-ac.psbt" \
  >"$WORKDIR/ptj-abc-gossip.psbt"

for f in \
  ptj-abc-direct \
  ptj-abc-via-ab-c \
  ptj-abc-via-a-bc \
  ptj-abc-via-ac-b \
  ptj-abc-idem \
  ptj-abc-plus-a \
  ptj-abc-gossip; do
  PSBT=$(cat "$WORKDIR/$f.psbt")
  assert_psbt_content "$f" "$PSBT" 3 3 29.997
done

SEED="deadbeefdeadbeefdeadbeefdeadbeef"
for f in \
  ptj-abc-direct \
  ptj-abc-via-ab-c \
  ptj-abc-via-a-bc \
  ptj-abc-via-ac-b \
  ptj-abc-idem \
  ptj-abc-plus-a \
  ptj-abc-gossip; do
  ptj sort --seed "$SEED" "$WORKDIR/$f.psbt" >"$WORKDIR/${f}-sorted.psbt"
done

PTJ_REFERENCE=$(cat "$WORKDIR/ptj-abc-direct-sorted.psbt")
for f in \
  ptj-abc-via-ab-c \
  ptj-abc-via-a-bc \
  ptj-abc-via-ac-b \
  ptj-abc-idem \
  ptj-abc-plus-a \
  ptj-abc-gossip; do
  RESULT=$(cat "$WORKDIR/${f}-sorted.psbt")
  if [ "$RESULT" != "$PTJ_REFERENCE" ]; then
    echo "FAIL: $f does not converge to direct ABC after sorting"
    exit 1
  fi
done

echo "ptj: all merge paths have 3 inputs, 3 outputs, 29.997 BTC, and converge."

# shellcheck disable=SC2154 # Nix sets $out for the treefmt check derivation.
mkdir -p "$out"
