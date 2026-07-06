# Scenarios

Concrete use cases for concurrent PSBT construction, from simple to
complex. Each scenario describes who is involved, what they're trying
to accomplish, and how the protocol enables it.

______________________________________________________________________

## 1. Batched payments (SharedCoin-style)

**Who:** 5-10 strangers who each want to make a payment.

**What:** Instead of each broadcasting a separate transaction, they
combine their payments into one. Each person adds an input (their
coin) and an output (their recipient). The combined transaction is
smaller per-payment than individual ones, saving fees. Because
multiple people's inputs and outputs are mixed, an observer can't
trivially link each input to its corresponding output.

**How it works:**

A coordinator (could be a server, or one of the participants)
creates the session with `PSBT_GLOBAL_SORT_DETERMINISTIC` set and
`MAX_TRANSACTION_WEIGHT` to cap the size. Participants join via
wormhole codes or a web interface (WebRTC).

Each participant creates their contribution: one input (the coin
they're spending) and one output (where they're paying). The fee
contribution field (`PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION`)
records how much each participant chips in for mining fees.

PSBTs propagate via the directory or nostr. The lattice join
merges them. Everyone's wallet computes the same result. Each
participant verifies their output is present, confirms the unique
ID, and signs with `SIGHASH_ALL`.

The privacy benefit is modest (a determined analyst can cluster by
amounts) but real: it breaks the one-input-one-output heuristic
that chain analysis relies on. The fee savings are proportional to
the number of participants, since they share the fixed overhead of
a transaction.

**Spec features used:** `TX_UNORDERED`, `SORT_DETERMINISTIC`,
`MAX_TRANSACTION_WEIGHT`, `EXPLICIT_FEE_CONTRIBUTION`, confirmation
protocol.

______________________________________________________________________

## 2. Multiparty payjoin / net settlement

**Who:** Alice, Bob, and Carol. Alice owes Bob 0.5 BTC. Bob owes
Carol 0.3 BTC. Carol owes Alice 0.1 BTC.

**What:** Instead of three separate on-chain payments, they settle
in one transaction. The net flows are: Alice sends 0.4 BTC, Bob
receives 0.2 BTC, Carol receives 0.2 BTC. One transaction instead
of three, with smaller total value moved on-chain.

**How it works:**

Each party creates a PSBT contributing their input(s) and their
settlement output(s). The amounts reflect the net flow, not the
gross obligations.

The confirmation protocol handles the cyclic payment graph: Carol
is a net-receiver (she receives 0.2 BTC net), so she confirms
first. Bob is also a net-receiver (0.2 BTC net), so he confirms.
Alice, the net-sender, waits for both confirmations before signing.
The spec guarantees that at least one net-positive receiver exists
in any cyclic payment structure (the sum around a cycle is zero),
so the confirmation chain always terminates.

This generalizes payjoin (which is two-party: sender + receiver
combine into one transaction for privacy) to N-party settlement.
The privacy improvement comes from the settlement transaction
looking like a single entity moving funds, when it's actually
three parties settling debts.

**Spec features used:** `TX_UNORDERED`, confirmation protocol
(cyclic graph resolution), `SIGHASH_ALL`.

______________________________________________________________________

## 3. Exchange operations

**Who:** An exchange with a hot wallet (threshold-signed by
distributed hardware signers), a cold wallet (air-gapped multisig),
and a fee wallet (small, auto-signing for transaction fees).

**What:** Customer withdrawals are batched. The cold wallet
contributes the customer funds (satoshi-precise). The fee wallet
contributes a UTXO to cover the mining fee. The hot wallet
co-signs for rate-limiting. The result is one transaction paying
dozens of customers, with fees paid from a dedicated budget, and
custody funds tracked to the satoshi.

**How it works:**

The withdrawal batching service creates a PSBT with
`TX_UNORDERED` and `SORT_DETERMINISTIC`. It adds one output per
pending withdrawal (each with a unique ID). The sort seed is
derived from the batch ID so ordering is reproducible for audit.

The cold wallet contributes inputs covering the total withdrawal
amount. It runs air-gapped: the PSBT is transferred via animated
QR codes (UR format) to the signing device and back. The cold
wallet's contribution is joined into the batch.

The fee wallet is a small hot wallet that auto-signs. It adds a
single input covering the estimated fee. The fee contribution
field records this precisely.

The hot wallet's role is authorization, not custody. It reviews the
joined PSBT, verifies withdrawal amounts match the approved batch,
and co-signs. Its signers are distributed across availability zones,
using a threshold scheme (e.g. 2-of-3 FROST or a multisig script).

Because the join is a lattice operation, the order doesn't matter:
the cold wallet can sign first (during a scheduled signing
ceremony), the fee wallet can top up later if the fee estimate
changes, and the hot wallet authorizes last. If the fee wallet
needs to replace its input (RBF), the new PSBT is joined with the
existing one. Conflicting fee inputs produce a `Conflict` that
surfaces for review, rather than silently corrupting the
transaction.

Accounting is exact: every satoshi in the cold wallet's inputs is
accounted for in withdrawal outputs plus change. The fee wallet's
contribution is separate and auditable. The two never mix.

**Spec features used:** `TX_UNORDERED`, `SORT_DETERMINISTIC` (with
batch-derived seed for audit), `EXPLICIT_FEE_CONTRIBUTION`,
animated QR transport (cold wallet), internal network transport
(hot + fee wallets).

______________________________________________________________________

## 4. Meetup coinjoin

**Who:** 8 people at a local Bitcoin meetup.

**What:** A coinjoin constructed entirely in person, without any
network connectivity. Everyone contributes one input and gets one
equal-denomination output. Pure sneakernet.

**How it works:**

One person creates the PSBT with `SORT_DETERMINISTIC` and a random
seed, setting `MAX_TRANSACTION_WEIGHT` to something reasonable for
8 participants. They display an animated QR code.

Everyone scans it, adds their input and output to their own copy,
and displays the result as an animated QR. The next person scans
any copy that's been updated, joins it with theirs, and displays
the new result.

The lattice property means it doesn't matter who scans whom in
what order. Someone scans Alice and Bob but not Carol? Fine.
Someone else scans Carol and Alice? Also fine. Eventually every
copy converges. People can even pair off: four conversations of
two happening simultaneously, then two conversations of four,
then one final scan confirms everyone agrees.

After convergence, each person verifies their output is present,
confirms the unique ID, and signs on their own device. Signed
PSBTs propagate the same way: scan, join, pass along. Since
BIP 174's combiner handles merging of partially signed PSBTs with
identical unsigned transactions (the sort step locks the
transaction structure), this works with existing signing
infrastructure.

The last person to collect all signatures broadcasts.

**Spec features used:** `TX_UNORDERED`, `SORT_DETERMINISTIC`,
`MAX_TRANSACTION_WEIGHT`, animated QR transport, confirmation
protocol.

______________________________________________________________________

## 5. Multi-meetup coinjoin

**Who:** Members of a Bitcoin community across three cities. They
meet at local meetups over the course of a month.

**What:** The same coinjoin as scenario 4, but spanning multiple
physical meetings. Week 1: New York meetup (4 people). Week 2:
one of the New Yorkers visits the San Francisco meetup (6 people).
Week 3: one San Franciscan visits the London meetup (5 people).
The final transaction has 15 participants.

**How it works:**

Week 1: The New York group does a local coinjoin (scenario 4).
But instead of signing immediately, they save the merged PSBT.
It has 4 inputs and 4 outputs.

Week 2: The traveler carries the New York PSBT on their phone.
At the SF meetup, 6 locals create their own contributions. The
traveler joins the NY PSBT with the SF contributions. The result
has 10 inputs and 10 outputs. The SF group saves copies.

Week 3: A San Franciscan at the London meetup joins the NY+SF
PSBT with 5 London contributions. 15 inputs, 15 outputs.

Now confirmation needs to propagate back: the London contingent
confirms, the SF contingent confirms (they have contacts from
week 2), and finally the NY contingent confirms (via the
original traveler or any other channel, including email or
messaging apps with PSBTs as attachments).

The lattice property is essential here. If someone in SF also
went to London independently and merged a slightly different
version, it doesn't matter. Any two copies of the PSBT that
contain overlapping information merge cleanly. Duplicates are
absorbed. The final result is always the same.

Signing happens asynchronously: each person signs whenever they
receive the final confirmed PSBT. Signed copies propagate just
like unsigned ones: sneakernet, email, whatever. BIP 174's
combiner merges the signatures.

**Spec features used:** `TX_UNORDERED`, `SORT_DETERMINISTIC`,
sneakernet + email transport, confirmation protocol (asynchronous,
multi-hop), idempotent join (redundant merges are no-ops).

______________________________________________________________________

## 6. Recurring subscription payments

**Who:** A group of 4 friends who share a VPN subscription,
splitting the cost monthly.

**What:** Each month, each friend contributes their share to a
single transaction paying the VPN provider. One transaction
instead of four, and the provider sees one payment instead of
four (simpler accounting on their end).

**How it works:**

The friends have long-term nostr contacts. Each month, one of them
creates a session via nostr DM with `SORT_DETERMINISTIC`. Each
friend's wallet auto-creates a contribution: one input (their
coin) and one output (the VPN provider's address, for their
share of the subscription).

The provider's address might come from a BIP 353 DNS record
(`₿vpn@provider.com`) or a BOLT 12 offer. Each friend resolves
it independently and uses the same address.

Since the friends do this monthly, their wallets can automate most
of the flow: create contribution, join, verify, sign. The only
manual step is approving the transaction amount. Over time, the
wallet learns the pattern and can pre-populate the contribution.

The VPN provider doesn't need to know or care that the payment
came from a multiparty construction. They receive one on-chain
payment for the full amount.

**Spec features used:** `TX_UNORDERED`, `SORT_DETERMINISTIC`,
nostr transport, BIP 353 addressing.

______________________________________________________________________

## 7. Cross-border remittance via hawala network

**Who:** A network of hawaladars (informal value transfer agents)
in different countries, settling their net positions periodically.

**What:** Instead of each hawaladar settling bilaterally (which
requires many small transactions and high aggregate fees), they
settle multilaterally in one transaction. The net flows between
all pairs are computed off-chain, and the on-chain transaction
reflects only the net positions.

**How it works:**

This is scenario 2 (net settlement) at scale. Each hawaladar
tracks their bilateral obligations off-chain. Periodically (daily,
weekly), they agree to settle. Each computes their net position
(how much they owe or are owed, in aggregate, across all
counterparts) and creates a PSBT contribution accordingly.

The settlement PSBT is passed around the network via whatever
channels the hawaladars already use (messaging apps, in-person
meetings, phone calls with PSBT attachments). The lattice join
handles out-of-order delivery. Confirmation propagates from
net-receivers inward.

The on-chain footprint is minimal: one transaction, N inputs,
N outputs, regardless of how many bilateral obligations existed.
Fees are shared proportionally via `EXPLICIT_FEE_CONTRIBUTION`.

**Spec features used:** `TX_UNORDERED`, `SORT_DETERMINISTIC`,
`EXPLICIT_FEE_CONTRIBUTION`, confirmation protocol (multi-hop,
cyclic graph resolution), mixed transports.

______________________________________________________________________

## 8. Crowdfunded transaction with public accountability

**Who:** A nonprofit soliciting on-chain donations for a specific
purpose (e.g. funding a Bitcoin Core developer).

**What:** Donors contribute inputs. The nonprofit contributes
the output (developer's address). The resulting transaction is
publicly verifiable: anyone can see that the inputs came from
separate donors and the output went to the intended recipient.

**How it works:**

The nonprofit publishes a PSBT with the output (developer's
address and target amount) and `TX_UNORDERED`. Donors download
it, add their input, and publish the updated PSBT to IPFS.
Each update's CID is posted to a public nostr relay or web page.

The lattice join means donors don't need to coordinate with each
other. Each donor independently adds their contribution and
publishes. Anyone can merge any subset of published PSBTs and
the result is the same.

When the target is reached, the nonprofit confirms the unique
ID and publishes it. Donors verify and sign. The nonprofit
combines signatures and broadcasts.

The entire construction process is publicly auditable: every
intermediate PSBT is on IPFS, immutable and content-addressed.
The lattice join guarantees that no donor's contribution was
dropped or modified. This level of transparency is not possible
with a centralized coordinator.

**Spec features used:** `TX_UNORDERED`, `SORT_DETERMINISTIC`,
IPFS transport, confirmation protocol, public verifiability.

______________________________________________________________________

## 9. Dead man's switch / inheritance

**Who:** A holder who wants their bitcoin to be recoverable by
their heirs after a period of inactivity.

**What:** A pre-signed transaction that becomes valid after a
timelock. The holder periodically "checks in" by spending the
input before the timelock expires, resetting the clock. If they
stop checking in, the heirs can broadcast the pre-signed
transaction.

**How it works:**

The holder creates a PSBT with a timelocked input (e.g. 6 months
from now) paying to a multisig controlled by the heirs. The heirs
each contribute their key via the PSBT key-value fields (BIP 174
`PSBT_IN_BIP32_DERIVATION`), but not their signatures yet.

The PSBT is shared with the heirs via sneakernet or encrypted
backup (e.g. Shamir's Secret Sharing of the PSBT itself). The
lattice join means each heir can independently add their key
information without coordinating with the others.

When the holder wants to reset the clock, they spend the input to
a new address they control and create a new timelocked PSBT. The
old one becomes invalid (the input is spent).

If the holder becomes incapacitated, the heirs join their key
contributions (they may not have all met, but any subset's
contributions merge cleanly via the lattice), sign the timelocked
transaction, and broadcast after the lock expires.

**Spec features used:** `TX_UNORDERED`, timelock integration,
sneakernet/encrypted backup transport, lattice join for key
contribution merging.

______________________________________________________________________

## 10. Lightning channel opens as part of a coinjoin

**Who:** Several participants who want to open Lightning channels
as part of a joint transaction.

**What:** Some participants are making on-chain payments, others
are opening Lightning channels. The transaction combines both,
providing privacy for channel opens (an observer can't distinguish
channel opens from regular payments) and fee savings.

**How it works:**

Channel-opening participants use explicit sort keys
(`PSBT_OUT_SORT_KEY`) derived from the Lightning interactive-tx
`serial_id` (odd parity, as suggested in the spec rationale).
This preserves compatibility with the interactive-tx protocol
while allowing the channel outputs to participate in the
deterministic ordering.

Regular payment participants use `SORT_DETERMINISTIC` for their
outputs. The two coexist: explicit sort keys and
deterministically-derived keys are both valid in the same
transaction, sorted together lexicographically.

The channel opener pre-negotiates the channel with their
counterparty via Lightning's existing protocol, then contributes
the funding output to the collaborative PSBT. The counterparty
doesn't need to participate in the collaborative construction,
they just need to know the final transaction's txid to complete
the channel open.

**Spec features used:** `TX_UNORDERED`, `PSBT_OUT_SORT_KEY`
(explicit, interop with interactive-tx `serial_id`),
`SORT_DETERMINISTIC` for non-channel outputs.

______________________________________________________________________

## 11. Wallet consolidation across a corporate treasury team

**Who:** A company's treasury team of three people, each holding
keys to a 2-of-3 multisig.

**What:** The company has hundreds of small UTXOs from customer
payments. During a low-fee period, they want to consolidate into
fewer, larger UTXOs. Two of the three key holders are in the
office. The third is traveling.

**How it works:**

The treasury software creates a consolidation PSBT: many inputs
(the small UTXOs), a few outputs (the consolidated amounts), all
paying back to the same multisig. `SORT_DETERMINISTIC` ensures
reproducible ordering for their audit trail.

Key holder A reviews and partially signs in the office. Key
holder B, also in the office, joins A's partial signature with
the original PSBT (using BIP 174's combiner, since the
transaction is now ordered and identical). They partially sign
too.

Key holder C is traveling. They receive the PSBT via encrypted
email or a corporate messaging system. They review it on their
laptop, add their partial signature, and send it back.

The 2-of-3 threshold is met with any two signatures. The lattice
join was used during construction (adding inputs, choosing
consolidation outputs). BIP 174's combiner is used for merging
partial signatures on the finalized transaction. The two
protocols compose naturally: lattice join for construction,
standard combiner for signing.

**Spec features used:** `TX_UNORDERED` (during construction),
`SORT_DETERMINISTIC`, BIP 174 combiner (after ordering),
email/messaging transport.

______________________________________________________________________

## 12. Creator support pact (ProTip reinvented)

**Who:** A group of friends who want to collectively support the
same set of online creators.

**What:** Each friend browses the web normally. Their wallet notes
which creators have bitcoin addresses (via BIP 353 DNS records,
website meta tags, or Lightning addresses). Periodically (monthly),
the friends pool their creator-support budgets into a single
batched transaction.

**Background:** ProTip.is (circa 2015-2019) was a browser extension
that tracked which sites you visited and automatically sent bitcoin
to creators' advertised addresses, proportional to time spent. It
was a solo operation: one user, one wallet, one transaction per
payout cycle. This created two problems:

1. **Privacy footgun.** The transaction's output set was a
   fingerprint of the user's browsing habits. Per Narayanan and
   Shmatikov's work on de-anonymizing the Netflix Prize graph,
   a sparse bipartite graph (users × sites) is trivially
   de-anonymizable even with pseudonymous identifiers. Each
   ProTip transaction published exactly such a subgraph on-chain.

1. **Clustering.** All outputs in a ProTip transaction shared a
   single input (the user's wallet). Chain analysis trivially
   clusters the creator addresses as "paid by the same entity."
   Combined with the browsing-habit fingerprint, this compounds:
   the outputs reveal which sites you visit, and the input reveals
   who you are.

**How the pact fixes this:**

The friends agree on a monthly cadence and a shared session (via
nostr, since they already know each other's npubs). Each friend's
wallet proposes outputs for the creators they want to support,
with their desired amounts. The lattice join merges all proposals.

The critical step happens after the join converges and before
ordering: **outputs with the same `PSBT_OUT_SCRIPT` are merged,
and their values summed.** This is explicitly supported by the
spec. If Alice wants to send 10,000 sats to a podcast and Bob
also wants to send 5,000 sats to the same podcast, the final
transaction has one output of 15,000 sats to that address.

This output merging breaks the fingerprint. An observer sees a
transaction with N inputs (from different wallets) and M outputs
(to various creators). They can't determine which input funded
which output, or which participant chose which creator. The
per-participant browsing subgraph is dissolved into the union.

Multiple inputs from different wallets also break the clustering
heuristic: the common-input-ownership assumption fails because the
inputs genuinely belong to different people.

The pact works on a slower cadence than the original ProTip
(monthly, not per-session). This is deliberate: batching over
time means more outputs per transaction, which means more
anonymity set. It also means lower fees per participant.

**How output merging works in the protocol:**

During construction, each participant adds outputs with unique IDs
(the `PSBT_OUT_UNIQUE_ID` field). Two participants supporting the
same creator produce two distinct outputs with different UIDs but
the same `script_pubkey`. The lattice join treats them as separate
outputs (different UIDs = different entries in the output set).

After joining and before ordering, a merge step scans for outputs
sharing a `script_pubkey`. These are combined: their amounts are
summed, one UID is kept (or a new one derived), and the duplicate
is removed. This step runs after `TX_UNORDERED` is cleared and
the BIP 370 construction rules apply.

This is a monotone operation on transaction effects: the payment
to each address is the sum of all participants' intended amounts.
No information about individual contributions is preserved in the
final transaction.

**Spec features used:** `TX_UNORDERED`, output merging (same-script
value summation), `SORT_DETERMINISTIC`, `EXPLICIT_FEE_CONTRIBUTION`,
nostr transport, BIP 353 addressing for creator discovery.

______________________________________________________________________

## 13. Splitting the tab

**Who:** Six friends finishing dinner at a restaurant.

**What:** The bill is 0.03 BTC. Instead of one person paying and
everyone Venmo-ing them back (off-chain, traceable, custodial),
they split the bill on-chain in a single transaction. Each person
contributes their share directly to the restaurant.

**How it works:**

One person scans the restaurant's payment QR (a BIP 321 URI or
BIP 353 address). They create a session and share a wormhole code
with the table: "7-guitarist-revenge." Everyone at the table joins
from their phone.

Each person's wallet creates a PSBT with one input (their coin)
and one output to the restaurant's address for their share of the
bill (0.005 BTC each). The six outputs share the same
`script_pubkey` (the restaurant's address).

The lattice join merges all six contributions. The output merging
step (same-script value summation) combines the six restaurant
outputs into one: 0.03 BTC to the restaurant. Each person also
gets a change output.

The final transaction has 6 inputs, 7 outputs (1 restaurant +
6 change). The restaurant receives the full amount in one output.
An observer sees a multi-input transaction paying one address,
indistinguishable from a single payer consolidating UTXOs.

The whole process takes less time than arguing about who had the
appetizer.

**Spec features used:** `TX_UNORDERED`, output merging (same-script
value summation), wormhole introduction, WebRTC or Bluetooth
transport, BIP 321 addressing.

______________________________________________________________________

## 14. Group deal / Groupon-style collective purchase

**Who:** 20 strangers who want to buy the same product at a bulk
discount. A merchant offers "0.01 BTC each if 20 people commit,
0.015 BTC each otherwise."

**What:** The merchant publishes a PSBT template with their output
(0.2 BTC for 20 units) and `MAX_TRANSACTION_WEIGHT` sized for 20
participants. Buyers join and commit their payments. When 20 have
joined, the deal activates.

**How it works:**

The merchant publishes a session invitation (via their website,
nostr, or a BIP 353 DNS record). The session PSBT has the
merchant's output pre-populated. Buyers join and each adds an
input (their coin) plus an output to the merchant's address for
their share (0.01 BTC).

As buyers join, the lattice join accumulates contributions. The
merchant's output and the buyers' same-address outputs will merge
during the pre-sorting step: all payments to the merchant's
`script_pubkey` combine into one output totaling 0.2 BTC.

The deal has a threshold: the merchant only confirms the unique
ID once 20 inputs are present. Before that, no one signs. This is
a natural use of the confirmation protocol: the merchant is the
net-receiver and initiates confirmation only when the threshold
is met.

If the threshold isn't met by a deadline, the session expires.
No one signed, so no one loses anything. This is fail-safe by
construction: unsigned PSBTs are just data.

If someone wants to drop out before signing, they simply don't
sign. The transaction can't be broadcast without all signatures
(`SIGHASH_ALL`). The remaining participants can either find a
replacement or let the session expire.

**Spec features used:** `TX_UNORDERED`, output merging,
`MAX_TRANSACTION_WEIGHT` (cap at 20 participants), confirmation
protocol (threshold-based), WebRTC or directory transport.
