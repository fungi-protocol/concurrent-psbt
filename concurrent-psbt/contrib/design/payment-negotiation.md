# Payment negotiation — companion spec (working draft)

Status: DRAFT OUTLINE, iterating. Companion document to the unordered PSBT
spec (`multiparty-protocol-docs/psbt.md`); defines the proprietary fields and
format for payment negotiation and confirmation. This file lives in the task
commit and is the working surface for the spec text until it graduates to
`multiparty-protocol-docs`.

## 1. Scope

Payment negotiation is a **purely signaling layer**. It MAY be synchronized
with a specific txin/txout state but is not structurally bound to one.
Participants that ignore these fields MUST still converge on the PSBT itself
(negotiation is optional equipment).

Embedding the signaling in the PSBT's proprietary fields exists to support
transports with no auxiliary channel (sneakernet: one artifact, no side
files). Carriage/trust-model taxonomy is deliberately OUT of this document;
instead the finished spec is audited for composability (the same elements
may equally travel over a transport side channel — the lattice semantics are
location-independent, and content-addressed confirmations are equally valid
from either source).

## 2. Identifier scheme

- Proprietary-key prefix: (inherit the prefix convention from psbt.md).
- Subtypes: `0x20` payment, `0x21` confirmation.
- Element keying: one proprietary entry per element, keyed by a random
  16-byte element id. The id is the uniqueness handle (mirrors output
  unique ids); join = dedup-by-id.

## 3. `PSBT_GLOBAL_PAYMENT` (0x20)

```
format(1) ‖ kind(1) ‖ payer_len ‖ payer(var) ‖ txout ‖ label(var)
```

- The payment body **is a txout** (consensus serialization: amount ‖
  scriptPubKey) — not a bespoke amount+script encoding. A payment by script
  is just a txout.
- **`payer` is arbitrary-length opaque bytes.** The identity scheme is
  unconstrained by this spec (may be a key, a hash, a handle meaningful only
  within the negotiating set; may be empty = unattributed).
- `kind` ∈ { real, dummy }. Dummy payments are padding (see §7) and carry no
  transfer.
- Recipients are not named on the wire: a recipient of a payment is by
  definition a coordinating peer (the payment was negotiated with them; peers
  exchanging PSBT fragments are in communication). Each peer recognizes its
  own scriptPubKeys.

## 4. `PSBT_GLOBAL_CONFIRMATION` (0x21)

```
format(1) ‖ peer_len ‖ peer_id(var) ‖ unique_id(32)
```

- `peer_id`: arbitrary-length opaque bytes (same regime as `payer`).
- `unique_id`: the order-independent content id of the PSBT being confirmed —
  normative derivation: tagged hash over the live (removal-projected) input
  outpoints and `(unique_id, amount, script)` outputs, each in sorted order
  (tag: `concurrent-psbt/unordered-unique-id`; final tag string TBD for the
  spec namespace).
- Confirmations bind to CONTENT, not to field state: any change to the joined
  input/output sets changes the id, so stale confirmations self-invalidate.

## 5. Join semantics

Both fields are grow-only sets of elements keyed by element id:

- same id, same value → deduplicate;
- same id, different value → CONFLICT (surface it; never silently overwrite);
- join is commutative/associative/idempotent (a lattice), so any gossip or
  merge order converges.

Values are opaque at the field layer: an encrypted element joins and stores
identically to a plaintext one.

## 6. Confirmation ordering (transcribing psbt.md §"Confirmation of
successful payment prior to signing")

All predicates are LOCAL and PAIRWISE; there is no global required-confirmer
set and no consensus mechanism — eventual consistency suffices.

- Parties that neither send nor receive payments sign once all their intended
  outputs are covered by the input signature (possibly re-signing as delayed
  messages arrive).
- A **sender** (any party with an outgoing payment, regardless of net
  balance) waits, before signing, for positive confirmation of the current
  unique id from EVERY one of its receivers — whom it knows from the payment
  coordination itself — and checks each confirmed id against its own local
  derivation.
- A **net-non-negative receiver** confirms the unique id to its senders as
  soon as the output set contains all of its added outputs.
- A **net-negative receiver** (sends more than it receives) waits for
  confirmations from all receivers for whom it is a sender before confirming
  to the senders for whom it is a receiver.
- **Cycles terminate structurally**: the sum of payments along a cycle is 0,
  so at least one net-positive receiver exists; net-positive receivers can
  safely initiate confirmations, breaking the circular dependency.
- **Mismatch** means an incomplete message set: a receiver that learns of new
  inputs/outputs after confirming re-confirms the new id; a sender that
  learns of new data derives the new id and stops waiting when it matches its
  receivers' confirmations.

Accounting intuition: with no payments, an input owner can account for every
sat (visible fee contributions + outputs everyone signs). Ceding funds to a
recipient transfers responsibility for those funds to that recipient. The
Σ = 0 structure guarantees at least one non-negative balance holder exists
who can confirm the funds were not misplaced, cascading safety-to-sign back
to the original input owners.

## 7. Confidentiality

- OPEN QUESTION (human deciding): mandatory vs recommended. Leaning: define
  an **HPKE-based confidentiality + integrity + authenticity profile specific
  to this document**, with scheme agility (new schemes slot in as they
  arise), and leave cleartext as an UNSPECCED demo encoding.
- Rationale for encryption even in the honest setting: not all parties to the
  transaction are parties to all payments — confidentiality among subsets is
  a desired property, independent of adversarial models.
- Documented caveat: encryption is necessary but NOT sufficient for privacy —
  element counts, element ids, set growth, and timing still leak metadata.
  Dummy payments (`kind = dummy`) pad graph degree; this document must be
  honest that the mitigation is partial.
- HPKE authenticity also gives `payer`/`peer_id` teeth (sender authenticity
  within the encryption envelope) without a separate signature scheme.
- Implementation note (current code, non-normative): group-secret AEAD with a
  deterministic per-element nonce so identical elements encrypt identically
  and encryption commutes with the join (dedup instead of conflicts). Safety
  precondition if retained: element content must be unique under a given key.

## 8. Field lifecycle / scrubbing

Defer to the scrubber spec (multiparty-protocol-docs PR #9) for field
lifecycle at terminal transitions. The composability requirement from this
side: negotiation fields are pre-signing signaling; nothing in them may be
required for validity of the final transaction.

## 9. Non-goals

- Validity proofs / BFT-profile authentication: a future sibling spec.
  (Almost certainly not over sneakernet; the PSBT embedding here is exactly
  what lets THIS layer work without auxiliary files.)
- Test vectors: none yet.
- Same-script output merging: orthogonal to this document (signaling layer;
  outputs are governed by the unordered PSBT spec).

## Open questions

1. Mandatory HPKE (§7) — pending.
2. Linkage hint: should a real payment carry a reference to the output
   realizing it (e.g. the output's 16-byte unique id), or stay fully unlinked
   at the signaling layer with correspondence left to the accounting rule?
   Unlinked is the default posture (txout embedding makes it natural);
   a hint would simplify receivers' "my outputs are present" check at the
   cost of explicit linkage.
3. Spec-namespace tag strings for the tagged hashes (currently
   `concurrent-psbt/*` in the implementation).

## Implementation deltas (as-shipped code vs this draft)

Tracked as task commits in the repo:

- `wip/task-payments-module`: consolidate `psbt/negotiation.rs`, `graph.rs`,
  `readiness.rs`, `session.rs` into a `payments` module; then companion crate
  or full feature gate.
- `wip/task-local-readiness`: rework readiness to the spec's local pairwise
  predicates; drop RecipientResolver / script-derived pseudonyms /
  pseudonymous-recipient exclusion from protocol logic (non-normative view at
  most).
- Wire deltas once this draft settles: `payer`/`peer_id` fixed 32 bytes →
  arbitrary-length; payment body amount+script → txout serialization;
  encryption format byte semantics → HPKE profile.
