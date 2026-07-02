# Pseudo-descriptors

A pseudo-descriptor is the declared-membership analogue of an output
descriptor: instead of proving script ownership, a participant declares
which session artifacts (fragments, payments, confirmations) they
authored or broadcast. The webgui uses pseudo-descriptors to track
provenance — who broadcast what — so the same UI works across settings.

## Scope: honest setting only

Pseudo-descriptors are meaningful **only in the honest setting**
(attributable transports: iroh, MDK, matrix, and authenticated variants).
In the semi-honest/anonymous setting, knowing which peer *delivered* a
message says nothing about which peer *authored* it: anonymous
broadcast exists precisely so txouts are unlinkable from any txins and
txins are unlinkable from each other. A UI in the anonymous setting must
not attribute artifacts to delivery peers; provenance columns render as
unattributed.

## Relation to confirmations

The confirmation protocol (ptj-net.md, Layer 4) carries `(peer_id, unique_id)` pairs. In the honest setting these feed pseudo-descriptors
directly. In the anonymous setting `peer_id` is a session-scoped
pseudonym at most; it must not be linked to transport-level identities.
