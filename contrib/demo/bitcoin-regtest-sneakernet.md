# Partial Transaction Joiner Regtest Demo

This walkthrough shows a three-person Bitcoin transaction assembled from
independent PSBT fragments. Alice, Bob, and Carol each contribute one input and
one output, exchange files, compute the same join, sort it into a Bitcoin Core
compatible PSBT, sign with separate Core wallets, finalize, broadcast, and mine
the transaction on regtest.

The commands are meant for a live demo. They use a Nix app to create a local
fixture directory so the interesting steps can be inspected afterwards.

## Bitcoin User Story

This is the non-technical story to narrate while running the commands:

1. Alice, Bob, and Carol each start with one coin and one destination address.
1. Each wallet writes a small PSBT fragment: "spend this coin, create this
   output." No one signs yet.
1. The fragments can be copied by USB stick, chat attachment, or shared folder.
   Copies and duplicates are harmless because joining the same fragment twice
   gives the same result.
1. Every participant can run the same sync command and independently see the
   same combined transaction.
1. The combined transaction is sorted into ordinary Bitcoin Core PSBT form.
1. Each wallet signs only after checking that its own coin, output, and fee are
   accounted for.
1. After all signatures are collected, Bitcoin Core finalizes and broadcasts the
   transaction.

The point of the demo is convergence: messy file exchange still leads to one
shared transaction, without requiring a coordinator to own the only copy.

## 1. Build The Fixture

From the repository root:

```sh
nix run .#ptj-demo-sneakernet -- ./demo-out
```

For interactive commands during the walkthrough, enter `nix develop .#demo` so
`ptj`, `bitcoin-cli`, and `jq` are on `PATH`.

The app creates `./demo-out` with a private regtest datadir, three wallets,
three participant fragments, shared sneakernet copies, joined PSBTs, and the
broadcast transaction id.

By default the app stops `bitcoind` after writing the artifacts. To keep the
node running for interactive `bitcoin-cli` inspection:

```sh
nix run .#ptj-demo-sneakernet -- --keep-node-running ./demo-out
```

When finished:

```sh
./demo-out/stop-bitcoind.sh
```

## 2. Inspect The Participants

```sh
cd ./demo-out
cat wallets.json | jq
cat utxos.json | jq
```

Each participant has one 10 BTC UTXO and one destination address. The generated
PSBT fragments are plain files:

```sh
ls alice bob carol shared joined
. ./env.sh
"$PTJ_BIN" inspect alice/a.psbt | "$JQ_BIN"
"$PTJ_BIN" inspect bob/b.psbt | "$JQ_BIN"
"$PTJ_BIN" inspect carol/c.psbt | "$JQ_BIN"
```

The fragments are unordered constructor PSBTs. They are safe to copy around and
join repeatedly because the join is idempotent.

## 3. Reproduce The Sneakernet Join

The app already copied the participant fragments into `shared/`. Re-run the join
from those files:

```sh
"$PTJ_BIN" --binary sync shared/ --state joined/abc.psbt
"$PTJ_BIN" --binary sort --seed deadbeefdeadbeefdeadbeefdeadbeef joined/abc.psbt \
  --output-file joined/abc-sorted.psbt
"$PTJ_BIN" export-bip174 joined/abc-sorted.psbt \
  --output-file joined/abc-bip174.psbt
```

`sync` accepts directories, so adding the same file again or adding already
joined results is safe:

```sh
cp joined/abc.psbt shared/already-joined.psbt
"$PTJ_BIN" --binary sync shared/ --state joined/abc-again.psbt
"$PTJ_BIN" --binary sort --seed deadbeefdeadbeefdeadbeefdeadbeef joined/abc-again.psbt \
  --output-file joined/abc-again-sorted.psbt
cmp joined/abc-sorted.psbt joined/abc-again-sorted.psbt
```

No matter how the same fragments arrive, the sorted result is the same.

## 4. Watch A Local Drop Folder Converge

For a more interactive sneakernet demo, run `sync` as a local polling loop. This
is still just the filesystem transport: no peer networking is involved yet.

In terminal A:

```sh
cd ./demo-out
. ./env.sh
mkdir -p live-drop live-joined
cp alice/a.psbt live-drop/alice.psbt
"$PTJ_BIN" --binary sync --ongoing --poll-interval-ms 500 live-drop/ \
  --state live-joined/session.psbt
```

In terminal B, add more fragments while terminal A is still running:

```sh
cd ./demo-out
. ./env.sh
cp bob/b.psbt live-drop/bob.psbt
cp carol/c.psbt live-drop/carol.psbt
"$PTJ_BIN" inspect live-joined/session.psbt | "$JQ_BIN" '{input_count, output_count}'
```

Stop terminal A with `Ctrl-C` after the state file has converged, then sort the
same local LUB into Bitcoin Core compatible form:

```sh
"$PTJ_BIN" --binary sort --seed deadbeefdeadbeefdeadbeefdeadbeef live-joined/session.psbt \
  --output-file live-joined/session-sorted.psbt
cmp joined/abc-sorted.psbt live-joined/session-sorted.psbt
```

The live loop demonstrates the same property as the one-shot command: fragments
can arrive in any order, and the maintained state file monotonically accumulates
the joined PSBT.

To open the preview GUI against the real ptj backend:

```sh
"$PTJ_BIN" webgui
```

Then open <http://127.0.0.1:8035/>.

## 5. Ask Bitcoin Core What It Sees

If the node is still running, load the helper environment and decode the
Bitcoin Core compatible BIP 174 PSBT:

```sh
. ./env.sh
bitcoin_demo_cli decodepsbt "$(cat joined/abc-bip174.psbt)" | "$JQ_BIN" '.tx | {vin, vout}'
```

Expected transaction shape:

```sh
cat expected.json | jq
```

There should be three inputs, three outputs, and `29.997` BTC in outputs. The
missing `0.003` BTC is the fee chosen by the toy fixture.

## 6. Signing And Broadcast

The fixture app has already signed, finalized, broadcast, and mined the joined
transaction. Inspect the generated artifacts:

```sh
cat joined/abc-signed.psbt
cat joined/final.hex
cat joined/txid.txt
```

With the node kept running:

```sh
. ./env.sh
bitcoin_demo_cli getrawtransaction "$(cat joined/txid.txt)" true \
  | "$JQ_BIN" '{txid, confirmations, vin, vout}'
```

## Demo Priority

This guide defines the implementation order for the demo series:

1. Keep the local CLI sneakernet flow working and easy to explain.
1. Extend the same `ptj sync` shape to one-shot peer sync over iroh-docs.
1. Exercise continual local file and directory sync with atomic updates.
1. Wire the offline web GUI to the real ptj backend operations.
1. Add networked GUI sessions over the same sync backend once the CLI path is
   stable.

The CLI should remain useful both as a small scripting tool and as a careful
manual tool for people exchanging PSBTs over USB sticks, messaging apps, or
other low-tech channels.
