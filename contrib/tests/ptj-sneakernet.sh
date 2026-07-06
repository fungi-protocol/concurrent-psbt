#!/usr/bin/env bash
# Integration test: ptj handles the sneakernet lattice correctly.
#
# Uses bitcoin-cli for wallet operations (funding, address generation)
# and ptj for PSBT construction and joining.
#
# ## Scenario
#
# Three parties construct a coinjoin via sneakernet (USB sticks).
# Messages arrive redundantly and out of order.
#
# ```mermaid
# sequenceDiagram
#     participant A as Alice
#     participant B as Bob
#     participant C as Carol
#
#     Note over A,C: Each party creates their PSBT contribution
#     A->>A: ptj create --input A --output A_dest
#     B->>B: ptj create --input B --output B_dest
#     C->>C: ptj create --input C --output C_dest
#
#     Note over A,C: Pairwise meetings (sneakernet)
#     A->>B: USB stick with PSBT_A
#     B->>A: USB stick with PSBT_B
#     B->>C: USB stick with PSBT_B
#     C->>B: USB stick with PSBT_C
#     A->>C: USB stick with PSBT_A
#     C->>A: USB stick with PSBT_C
#
#     Note over A,C: Each party joins what they have
#     A->>A: ptj join A B C → ABC
#     B->>B: ptj join B A C → ABC
#     C->>C: ptj join C A B → ABC
#
#     Note over A,C: All produce identical results ✓
# ```
#
# ## Lattice structure
#
# ```mermaid
# graph BT
#     A[PSBT_A] --> AB[A ⊔ B]
#     B[PSBT_B] --> AB
#     B --> BC[B ⊔ C]
#     C[PSBT_C] --> BC
#     A --> AC[A ⊔ C]
#     C --> AC
#     AB --> ABC["A ⊔ B ⊔ C<br/>(all paths converge)"]
#     BC --> ABC
#     AC --> ABC
#     ABC --> ABC_ABC["ABC ⊔ ABC = ABC<br/>(idempotent)"]
#     ABC --> ABC_A["ABC ⊔ A = ABC<br/>(absorbing)"]
# ```
#
# Every path through the lattice produces the same join.
# Re-merging a copy already incorporated is a no-op.
set -euo pipefail

# shellcheck source=contrib/tests/regtest-lib.sh
# shellcheck disable=SC1091 # treefmt shellcheck does not follow sourced files.
source "$(dirname "$0")/regtest-lib.sh"

WORKDIR=$(mktemp -d)
trap 'regtest_cleanup; rm -rf "$WORKDIR"' EXIT

# ── Setup wallets and fund them ────────────────────────────────────

for PARTY in alice bob carol; do
  $CLI createwallet "$PARTY" >/dev/null
done

MINE_ADDR=$($CLI -rpcwallet=alice getnewaddress)
$CLI -rpcwallet=alice generatetoaddress 110 "$MINE_ADDR" >/dev/null

ALICE_FUND=$($CLI -rpcwallet=alice getnewaddress)
BOB_FUND=$($CLI -rpcwallet=bob getnewaddress)
CAROL_FUND=$($CLI -rpcwallet=carol getnewaddress)
$CLI -rpcwallet=alice sendtoaddress "$ALICE_FUND" 10 >/dev/null
$CLI -rpcwallet=alice sendtoaddress "$BOB_FUND" 10 >/dev/null
$CLI -rpcwallet=alice sendtoaddress "$CAROL_FUND" 10 >/dev/null
$CLI -rpcwallet=alice generatetoaddress 1 "$MINE_ADDR" >/dev/null

# ── Gather UTXOs and destinations ──────────────────────────────────

A_UTXO=$($CLI -rpcwallet=alice listunspent | jq -r '[.[] | select(.amount == 10)][0]')
B_UTXO=$($CLI -rpcwallet=bob listunspent | jq -r '.[0]')
C_UTXO=$($CLI -rpcwallet=carol listunspent | jq -r '.[0]')

A_TXID=$(echo "$A_UTXO" | jq -r '.txid')
A_VOUT=$(echo "$A_UTXO" | jq -r '.vout')
B_TXID=$(echo "$B_UTXO" | jq -r '.txid')
B_VOUT=$(echo "$B_UTXO" | jq -r '.vout')
C_TXID=$(echo "$C_UTXO" | jq -r '.txid')
C_VOUT=$(echo "$C_UTXO" | jq -r '.vout')

A_DEST=$($CLI -rpcwallet=alice getnewaddress)
B_DEST=$($CLI -rpcwallet=bob getnewaddress)
C_DEST=$($CLI -rpcwallet=carol getnewaddress)

# ── Each party creates their PSBT via ptj create ──────────────────

ptj create --network regtest --input "$A_TXID:$A_VOUT" --output "$A_DEST:9.999" >"$WORKDIR/a.psbt"
ptj create --network regtest --input "$B_TXID:$B_VOUT" --output "$B_DEST:9.999" >"$WORKDIR/b.psbt"
ptj create --network regtest --input "$C_TXID:$C_VOUT" --output "$C_DEST:9.999" >"$WORKDIR/c.psbt"

echo "=== ptj Sneakernet Lattice Test ==="
echo
echo "Three parties, each creates a PSBT with 1 input + 1 output."
echo

# ── Test 1: Pairwise joins ────────────────────────────────────────

ptj join "$WORKDIR/a.psbt" "$WORKDIR/b.psbt" >"$WORKDIR/ab.psbt"
ptj join "$WORKDIR/b.psbt" "$WORKDIR/c.psbt" >"$WORKDIR/bc.psbt"
ptj join "$WORKDIR/a.psbt" "$WORKDIR/c.psbt" >"$WORKDIR/ac.psbt"

echo "Pairwise joins: AB, BC, AC created"

# ── Test 2: Three-way join (direct) ──────────────────────────────

ptj join "$WORKDIR/a.psbt" "$WORKDIR/b.psbt" "$WORKDIR/c.psbt" >"$WORKDIR/abc_direct.psbt"
echo "Direct three-way: A+B+C created"

# ── Test 3: Sequential paths ─────────────────────────────────────

ptj join "$WORKDIR/ab.psbt" "$WORKDIR/c.psbt" >"$WORKDIR/abc_via_ab_c.psbt"
ptj join "$WORKDIR/a.psbt" "$WORKDIR/bc.psbt" >"$WORKDIR/abc_via_a_bc.psbt"
ptj join "$WORKDIR/ac.psbt" "$WORKDIR/b.psbt" >"$WORKDIR/abc_via_ac_b.psbt"

echo "Sequential paths: (AB)+C, A+(BC), (AC)+B created"

# ── Test 4: Idempotence ──────────────────────────────────────────

ptj join "$WORKDIR/abc_direct.psbt" "$WORKDIR/abc_direct.psbt" >"$WORKDIR/abc_idem.psbt"
echo "Idempotent join: ABC+ABC created"

# ── Test 5: Redundant component ──────────────────────────────────

ptj join "$WORKDIR/abc_direct.psbt" "$WORKDIR/a.psbt" >"$WORKDIR/abc_plus_a.psbt"
echo "Redundant merge: ABC+A created"

# ── Test 6: Gossip — all pairwise merges combined ────────────────

ptj join "$WORKDIR/ab.psbt" "$WORKDIR/bc.psbt" "$WORKDIR/ac.psbt" >"$WORKDIR/abc_gossip.psbt"
echo "Gossip merge: join(AB, BC, AC) created"

echo

# ── Verify: all paths produce identical results ──────────────────
# Sort with the same seed so ordering is deterministic for comparison.

SEED="deadbeefdeadbeefdeadbeefdeadbeef"

for f in abc_direct abc_via_ab_c abc_via_a_bc abc_via_ac_b abc_idem abc_plus_a abc_gossip; do
  ptj sort --seed "$SEED" "$WORKDIR/$f.psbt" >"$WORKDIR/${f}_sorted.psbt"
  ptj export-bip174 "$WORKDIR/${f}_sorted.psbt" >"$WORKDIR/${f}_bip174.psbt"
done

# ── Verify: every path preserves the expected transaction content ──
# Bitcoin Core consumes the fixed BIP 174 export, not unordered constructor PSBTs.

for f in abc_direct abc_via_ab_c abc_via_a_bc abc_via_ac_b abc_idem abc_plus_a abc_gossip; do
  PSBT=$(cat "$WORKDIR/${f}_bip174.psbt")
  assert_psbt_content "$f" "$PSBT" 3 3 29.997
done

echo "Content checks: every path has 3 inputs, 3 outputs, 29.997 BTC"
echo

REFERENCE_V2=$(cat "$WORKDIR/abc_direct_sorted.psbt")
PASS=0
FAIL=0

for f in abc_via_ab_c abc_via_a_bc abc_via_ac_b abc_idem abc_plus_a abc_gossip; do
  RESULT=$(cat "$WORKDIR/${f}_sorted.psbt")
  if [ "$RESULT" = "$REFERENCE_V2" ]; then
    echo "  ✓ $f matches direct ABC"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $f DIFFERS from direct ABC"
    FAIL=$((FAIL + 1))
  fi
done

echo
echo "Results: $PASS passed, $FAIL failed"

if [ "$FAIL" -ne 0 ]; then
  echo "FAIL: lattice convergence violated"
  exit 1
fi

echo
echo "All join paths produce identical PSBTs."
echo "Join is idempotent, commutative, and associative."

echo
echo "Signing, finalizing, broadcasting, and mining the sorted PSBT."
REFERENCE=$(cat "$WORKDIR/abc_direct_bip174.psbt")
SIGNED=$(sign_with_wallets "$REFERENCE" alice bob carol)
TXID=$(finalize_and_broadcast_psbt "$SIGNED" "$MINE_ADDR" 3 29.997)
echo "Broadcast transaction mined: $TXID"

# shellcheck disable=SC2154 # Nix sets $out for the treefmt check derivation.
mkdir -p "$out"
