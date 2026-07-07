# ptj: Collaborative Bitcoin Transactions

## The problem

Bitcoin Core's `joinpsbts` duplicates outputs. Its `combinepsbt`
rejects PSBTs with different transactions. Neither works for
building a transaction with someone else.

`ptj` fixes this. Its join is idempotent, commutative, and
associative: every merge path produces the same result, copies can
arrive out of order or be redundant, and it just works.

## Try it

```bash
# Alice creates her part
ptj create --input txid1:0 --output addr1:0.01 > alice.psbt

# Bob creates his
ptj create --input txid2:0 --output addr2:0.01 > bob.psbt

# Join (order doesn't matter, duplicates are harmless)
ptj join alice.psbt bob.psbt > merged.psbt
ptj join bob.psbt alice.psbt > merged2.psbt  # same result

# Sort for signing
ptj sort --seed $(head -c16 /dev/urandom | xxd -p) merged.psbt > final.psbt
```

## Why this matters

### Splitting the tab

Six friends at dinner. The bill is 0.03 BTC.

One person scans the restaurant's QR code and shares a session
link. Everyone opens `ptj.app` in their browser, pastes their
PSBT (one input, one output to the restaurant). The join merges
all six contributions. Outputs to the same address are summed:
the restaurant receives one payment of 0.03 BTC, not six separate
outputs.

The final transaction has 6 inputs, 7 outputs (1 restaurant +
6 change). An observer sees a multi-input transaction, no way to
tell which input belongs to which diner.

No app install. No coordinator. No signup. Under a minute.

### Exchange withdrawal batching

An exchange batches customer withdrawals. Three wallets are
involved: a cold wallet (air-gapped, multisig) contributes
customer funds, a fee wallet (auto-signing) covers the mining
fee, and a hot wallet (threshold-signed) authorizes the batch.

The cold wallet's PSBT is transferred via animated QR codes to
the signing device and back. The fee wallet tops up via the
internal network. The hot wallet reviews and co-signs last.

Because the join is a lattice operation, order doesn't matter:
the cold wallet can sign during a ceremony on Monday, the fee
wallet adjusts on Tuesday, and the hot wallet authorizes on
Wednesday. If the fee estimate changes, the new PSBT joins with
the existing one. Conflicts surface for review rather than
silently corrupting the transaction.

Accounting is exact: every satoshi in the cold wallet's inputs
maps to a withdrawal output or change. The fee wallet's
contribution is separate and auditable.

### Sneakernet coinjoin

Eight people at a Bitcoin meetup. No internet. Everyone shows
an animated QR code on their phone. Scan anyone, join with yours,
show the result. It doesn't matter who scans whom in what order.
People pair off: four conversations of two, then two of four,
then one final round. The lattice guarantees convergence.

This can span weeks. A New York group builds a 4-person PSBT.
A traveler carries it to the San Francisco meetup, where 6 more
join. A San Franciscan takes the 10-person PSBT to London, where
5 more join. Confirmation and signatures propagate back via
email, messaging apps, or more USB sticks.

### Creator support pact

A group of friends collectively tip online creators. Each person's
wallet tracks which creators have bitcoin addresses (via BIP 353
DNS records). Monthly, they pool contributions into one
transaction.

The critical step: outputs to the same creator address are merged
by summing their values. If Alice sends 10k sats to a podcast and
Bob sends 5k sats to the same podcast, the transaction has one
output of 15k sats.

This breaks the privacy footgun of the original ProTip.is (a
single user's browsing fingerprint encoded in the output set).
Multiple inputs from different wallets break the common-input
ownership heuristic. The per-participant creator subgraph dissolves
into the union.

### Net settlement

Alice owes Bob 0.5 BTC. Bob owes Carol 0.3 BTC. Carol owes Alice
0.1 BTC. Instead of three transactions, they settle in one. Net
flows: Alice sends 0.4, Bob receives 0.2, Carol receives 0.2.

The confirmation protocol handles the cyclic payment graph:
net-receivers confirm first, net-senders wait. The spec proves
at least one net-positive receiver exists in any cycle (the sum
around a cycle is zero), so confirmation always terminates.

## How it works

The library is IO-free. Three functions:

```rust
fn join(a: &[u8], b: &[u8]) -> Vec<u8>  // lattice join
fn is_ok(psbt: &[u8]) -> bool            // conflict-free?
fn sort(psbt: &[u8], seed: &[u8]) -> Vec<u8>  // deterministic order
```

Every transport is a shared directory. Files appear. You join
whatever you find. The transport is how files get there.

```
┌─────────────────────────────────────┐
│           shared directory          │
│  alice.psbt  bob.psbt  carol.psbt  │
└──────┬──────────┬──────────┬────────┘
       │          │          │
     Alice       Bob       Carol
       │          │          │
       └──────────┴──────────┘
              same result
```

### Transports (what puts files in the directory)

| Transport | Where it works | Best for |
|---|---|---|
| Filesystem | CLI | Scripts, automation |
| Animated QR (UR) | Mobile, hardware wallets | In-person, air-gapped |
| Payjoin directory | Everywhere (HTTP) | Remote, async, private (OHTTP) |
| Iroh | CLI, mobile | P2P, push updates, NAT traversal |
| Nostr / mdk | CLI, mobile | Recurring, push notifications, contacts |
| WebRTC | Browser | Zero-install, "send a link" |
| Nearby / Multipeer | Android / iOS | Local, zero-config |
| BLE | Mobile | Cross-platform local |

A peer on two transports is a bridge. The lattice makes this free:
no translation, no protocol negotiation. A PSBT is a PSBT.

## Implementation phases

### Phase 1: Sneakernet CLI

`ptj create`, `ptj join`, `ptj sort` on files. Already working.
`ptj net --dir ./session/` watches a directory for new PSBTs.

### Phase 2: Iroh CLI

`ptj net --iroh` adds P2P sync. Iroh's set reconciliation ensures
all peers converge. NAT traversal via relay. No polling.

### Phase 3: Web and mobile sneakernet

`ptj.app`: static web page, WASM library, QR display/scan. No
backend. Android and iOS apps with animated QR + camera.

### Phase 4: Network transports

Payjoin directory (universal HTTP fallback). Nostr/mdk for push
and contacts. WebRTC for browser P2P. Nearby/Multipeer for local.

### Phase 5: UX

Magic wormhole codes for one-shot pairing. Session management.
Progress display. Auto-retry. Push notifications on mobile.

### Phase 6: Contacts (separate project)

Long-term peer relationships. BIP 353 names. Nostr npubs. TOFU
key exchange. Recurring transaction automation. This is a spin-off
with its own design space (see `.goose/uri.md` notes on
introduction mechanisms and payment instruction formats).

## Further reading

- [Scenarios](scenarios.md): 14 detailed use cases
- [Protocol reference](ptj-net.md): full 5-layer protocol stack, session lifecycle, wallet integration guide
- [Transports](transports.md): per-transport analysis and environment suitability
- [Transport plugins](transport-plugins.md): out-of-process transports (separate binaries, own lockfiles) over Cap'n Proto stdio RPC, and why lockfile conflicts force them
- [Environments](environments.md): POSIX/web/mobile platform matrix, user story taxonomy
- [App suite](app-suite.md): per-platform app architecture (CLI, web, Android, iOS)
- [Traits](traits.md): library interface boundaries (Cap'n Proto, WIT)
- [Security](security.md): threat model and considerations
- [Spec](https://github.com/payjoin/multiparty-protocol-docs/blob/main/psbt.md): the concurrent PSBT construction spec
