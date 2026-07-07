# Partial Transaction Joiner Demo GUI

This is a WIP browser surface for explaining collaborative PSBT construction
without committing to a wallet API or network transport yet. It is a static
demo: all PSBTs are represented by small in-browser payloads containing inputs,
outputs, and deterministic ordering seeds.

## Run

Build the TypeScript once, then open `contrib/demo-gui/index.html` directly, or
serve the directory:

```sh
nix build --no-link --print-out-paths .#checks.aarch64-darwin.demo-gui
nix build --no-link --print-out-paths .#checks.aarch64-darwin.demo-gui-playwright
python3 -m http.server 8034 -d contrib/demo-gui
```

The page starts with a small sample graph. All peers in a session must be
trusted with privacy, because anything they can read can be leaked, and with
liveness, because they can stop cooperating. Funds remain protected by the
wallet signing check: signing only proceeds after the final transaction gives
each participant what they approved and accounts for every sat through inputs,
outputs, and explicit fee contributions.

The home workspace is layered:
peers at the top, unordered session registers in the middle, and fragment PSBTs
at the bottom. Descriptors sit in a fixed dock along the bottom edge, each with
its own color; descriptor-linked coins and addresses inherit that color, and
hovering a descriptor highlights associated coins. Descriptor chips use a small
actions menu for tagging the descriptor as mine/other, changing its display
color, and generating a BIP 321 payment request URI. Each PSBT card uses a
two-column layout with inputs on the left
and outputs on the right. Graph coin rows stay collapsed so card layout does not
reflow while selecting PSBTs; a later inspector view can expose the elided
details without hiding unrelated rows. Coins show the eight integer sat digits to
the right of a muted bitcoin-scale decimal point, with leading zero digits at
low opacity, plus a dummy visual script fingerprint. The fingerprint is
deliberately a placeholder until a LifeHash-style rendering is wired in.

Each PSBT also shows a compact balance ledger. The local `"mine"` bucket is
tracked separately from `"other"` in the model. `"Mine"` being balanced means the
local inputs equal the local outputs plus explicit local fee contribution, so
there is no hidden local fee/loss in the elided PSBT. That is only a necessary
signing precondition; wallet policy still has to validate scripts, addresses,
fees, and every output. Coins with no recognized descriptor use a black
boundary, while descriptor-linked coins keep their descriptor color. In unordered
PSBT cards, recognized inputs and outputs are grouped by descriptor before the
unrecognized rows. Those groups render as balance-sheet subtransactions:
explicit fee contributions are summed on the output side above the sum line, the
input and output accounting totals sit below it, and the remaining implicit fee
or shortfall is displayed as the difference. Explicit fee contributions are not
rendered as transaction outputs.

Sessions are CRDT registers for unordered PSBT data. A PSBT that is still in
BIP 370 ordered form can be imported and inspected as a fragment, but it cannot
be promoted into a session, absorbed by a session, or shared through a
peer-session read capability until it is manually converted to unordered form.
Fixed BIP 174-compatible transactions are shown as a separate role state: they
are no longer constructor/edit surfaces, so the mockup only admits updater and
signer-style accumulation for them.
Sneakernet does not add another join mode: it is just PSBT fragments carried by
copy/paste or files. The same graph `Join` operation merges peerless fragments
as it does networked fragments. Sessions are optional monotone registers with
identity, useful for organizing and sharing a running LUB, but the workspace
still makes sense when no peers are present. A future offline-only build can
hide or forbid peering entirely without changing the PSBT fragment operations.

The home-screen paste target is the import surface for npubs, relay URIs,
magic-wormhole codes, BIP 321 bitcoin URIs, base64 PSBTs, ptj demo payloads,
descriptors, and dropped binary PSBTs. The side dock is intentionally smaller:
`Export` prepares a sneakernet payload for the selected PSBT, `Create` adds
manual peers/descriptors/payment intents/empty sessions, and `Inspect` shows the
selected vertex. There are two distinct selection modes:

- click one vertex for unary actions such as make unordered, split PSBT, and
  promote fragment
- drag wires between vertices to build an explicit join-select graph

Releasing a valid wire leaves an animated dashed pending edge with its own
`Join` control. Press that edge-local `Join` to apply just that wire, or press
the toolbar `Join` to apply the whole pending join-select. Multiple disconnected
pending components can exist at once; applying the toolbar join only collapses
objects along those explicit wired components. The action is inferred from each
wire's endpoint types:

- peer + session: grant a read capability so the peer observes that unordered
  session register
- peer + peer: bridge the peers and replicate the union of readable sessions to
  every peer in the bridge block, without merging distinct session vertices
- session + session: replace both session PSBTs with their LUB
- fragment + session: absorb the fragment into the session and remove the
  fragment vertex
- fragment + fragment: replace both fragment PSBTs with their LUB

The graph also sketches click-and-drag wiring. Dragging onto a valid target
snaps the preview wire to that target and shows the action label. Incompatible
PSBT targets do not snap or highlight.

Local joins mutate immediately. Operations that affect a shared session then
enter a simulated network quiescence period: the session boundary becomes
dashed, session-peer wires show a spinner, and the PSBT card displays `k/n`
replica acknowledgements until all simulated peers have announced the same LUB.
When a fragment is absorbed into a shared session, the contributed inputs,
outputs, and other contributed PSBT rows are also dashed until that session
update converges.
Each peer has a stable latency profile, but every wire samples its own ack
delays independently. A wire terminating at a peer bridge stays pending until
the last peer in that bridge block has acked. The delays only exist to make the
latency shape visible in the mockup.

Fragments can also be promoted directly into sessions. A live session can be
aborted, which breaks its peer links and preserves the current state as a
fragment PSBT. Atomize is intentionally available only for non-atomic fragments;
atomizing a live session register would be a non-monotone edit, so terminate it
first.
Fragments do not keep persistent edges: once a fragment is linked to another
PSBT, the linked objects are merged and replaced by the LUB. Persistent edges in
the workspace are peer-session read capabilities and peer-peer bridges.

Each PSBT card and inspector now exposes a protocol role state. Unordered
fragments/registers expose constructor/combiner/sync actions. BIP 370
constructor fragments expose constructor/updater actions according to their
modifiable state. Sorter output is shown as a sorted BIP 370 candidate, while
fixed BIP 174-compatible transactions are restricted to updater/signer roles.
Toolbar buttons are derived from that state: unavailable role transitions are
hidden instead of being presented as generic PSBT actions.

Descriptors are optional when importing seed PSBTs. When descriptors are used,
public descriptors are the default and encouraged path; private descriptors can
be recorded for local/manual demos without changing the merge model. The demo can
generate BIP 321-style bitcoin URIs from descriptors; pasted bitcoin URIs become
PSBT fragments with a local UI note that the fragment came from a URI. That note
is not modeled as serialized PSBT data.

The GUI is written in TypeScript. Pure model operations live in `src/model.ts`
and are unit-tested outside the browser by the `demo-gui` flake check with Node's
built-in coverage thresholds set to 100% for that model module. Browser smoke
tests run through the `demo-gui-playwright` flake check, which launches the
Nix store Chromium through `playwright-core` without npm downloads or a
user-level Playwright browser cache.

## WIP Notes

- Peer-pushed fragments need a session/peer policy switch for automatic versus
  manual acceptance. The default can be session-scoped, with peer-specific
  overrides for trusted peers.
- Accepted peer-pushed fragments should tag their PSBT rows with a
  pseudo-descriptor such as `added by peer Alice`; local fragments should get the
  analogous `added by me` tag.
- Actual descriptor grouping remains the default transaction subdivision. A
  later display mode should allow disabling pseudo-descriptors or grouping by
  contributing peer when that view is useful.
- Pseudo-descriptors are provenance views, not serialized PSBT descriptor data.
  They should be available as alternate groupings over the same joined PSBT rows:
  actual descriptors first by default, provenance grouping on demand.
- Peers should be expandable to show fragments they contributed. Absorbed
  fragments should remain available in a special provenance view even after the
  session has joined them into its current LUB.
- Amount rendering currently emphasizes integer sats within an eight-decimal BTC
  scale by muting leading zeroes. Later refinements may add alternate visual
  display modes, including exponential-style notation for values that are round
  in binary rather than decimal.
- Labels are local/user-facing metadata and should be editable everywhere a
  labeled object appears, including descriptor dock menus, UTXOs, PSBT
  fragments, sessions, and peer records. For PSBT-related labels, represent the
  durable form as BIP 329-style label data carried in a proprietary PSBT field;
  the mockup currently treats labels as local UI metadata only.
- Internal graph ids such as `frag-04` are implementation details. Any remaining
  visible use of those ids should be replaced with a stable PSBT unique id,
  UUID, outpoint, output unique id, or an unlabeled placeholder depending on the
  object type.
- Protocol identity display is still backed by deterministic mock hashes over
  elided payloads. The real implementation must compute the transaction id from
  the ordered transaction serialization when a SegWit PSBT is ordered and
  non-modifiable before signing. Other PSBT states should expose the relevant
  BIP 174, BIP 370, or `psbt.md` unique id. For unordered PSBTs, that unique id
  is the spec's canonical-sort-then-hash identity; it must stay separate from
  shuffled unordered serialization and from the sorter role's final ordering.
- Input display needs a previous-output-data indicator. If
  `PSBT_IN_NON_WITNESS_UTXO` or `PSBT_IN_WITNESS_UTXO` is absent, the UI must
  not pretend it can show a trustworthy amount, script fingerprint, or
  descriptor match for that input.
- Session termination should be framed as fixing the input/output sets and their
  ordering into an updater/signer PSBT candidate. It is not BIP 174 finalization,
  and sessions should not expose independent "sort" and "remove modifiability"
  actions because distributed session updates do not have a simple total order.

## Live Demo Ladder

Target week: 2026-06-28 through 2026-07-03.

1. Basic CLI sneakernet workflow: use `ptj create`, `ptj join`, and `ptj sort`
   against the regtest flow already covered by `contrib/tests/ptj-sneakernet.sh`.
1. Slightly more sophisticated sneakernet: show redundant and reordered
   propagation with `contrib/tests/sneakernet-lattice.sh`, including convergence
   and final broadcast.
1. Minimal live network transport: spike iroh first because the existing design
   treats documents as opaque PSBT message sets; fall back to mdk/nostr if the
   iroh Darwin/Nix path is higher friction than expected. The demo target is two
   or more peers exchanging fragments, computing the same LUB, sorting, signing,
   finalizing, and broadcasting on regtest.
1. GUI tail: this page is the first visualization. The next step is a thin
   `ptj webgui` command or WASM/CLI bridge that replaces the synthetic payloads
   with real PSBT bytes.

## Boundaries

- Output descriptors and UTXOs are synthetic; no wallet scan happens here.
- Payment intents are TxOut-shaped payloads, not signed transactions.
- LUB is modeled as deterministic set union over elided PSBT payloads, with
  conflict detection by payload identity. Real validation still belongs in
  `concurrent-psbt` and `ptj`.
- Import, conversion, and atomization are manual state transitions over elided
  payload metadata. The page does not parse or validate real PSBT bytes.
- No public API follows from this commit.
