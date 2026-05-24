#![allow(clippy::result_large_err)]

//! Confirmation readiness: the predicate that decides
//! [`Phase::Ready`](crate::session::Phase::Ready).
//!
//! This is pure logic over two already-existing ingredients:
//!
//! - the grow-only confirmation set ([`crate::negotiation::Confirmation`],
//!   subtype `0x21`), a set of `(peer_id, unique_id)` attestations, and
//! - the order-independent [`unordered_unique_id`](crate::negotiation::unordered_unique_id)
//!   of the current joined PSBT.
//!
//! It answers: *given the payment graph and the confirmations collected so
//! far, is every party that must confirm confirmed against the CURRENT unique
//! id?* No new PSBT field is introduced.
//!
//! ## The spec rule (`psbt.md` §"Confirmation of successful payment prior to
//! signing")
//!
//! - A **sender** (any party with an outgoing payment) signs only after it has
//!   a matching confirmation of the current unique id from *every one of its
//!   receivers*.
//! - A **net-non-negative receiver** confirms immediately (its outputs are all
//!   present).
//! - A **net-negative receiver** confirms only after all receivers for whom it
//!   is a sender have confirmed.
//! - Cyclic graphs terminate because `Σ net == 0` forces at least one
//!   net-positive receiver to exist, which is the base case that propagates
//!   confirmation up the DAG.
//!
//! ## Fixpoint formulation
//!
//! "Party `p` is *confirmable*" is a least fixpoint:
//!
//! ```text
//! confirmable(p) ⇐ p is net-non-negative                       (base case)
//! confirmable(p) ⇐ every receiver r of p is confirmable        (induction)
//! ```
//!
//! We do not need the message-level confirmation set to compute *who should*
//! confirm — that is a graph property. We use the confirmation set only to
//! check *who has actually* confirmed the current unique id. Readiness for the
//! whole session ([`is_ready`]) holds when every party that is required to
//! confirm (every net-receiver, i.e. every non-net-negative-sender's receiver
//! set, and transitively every net-non-negative receiver) has an attestation
//! whose `unique_id` equals the current one.
//!
//! Because confirmations attest to the *content* id (which changes whenever
//! the joined set changes), a stale confirmation against an old id simply does
//! not count — the session stays in
//! [`Phase::Confirming`](crate::session::Phase::Confirming) until every required
//! party re-confirms the current least upper bound. This is exactly the
//! eventual-consistency behaviour the spec requires: no consensus, only
//! convergence on the current LUB.

use std::collections::{BTreeMap, BTreeSet};

use psbt_v2::v2::Global;

use crate::graph::{ParticipantId, PaymentGraph};
use crate::negotiation::{Confirmation, GlobalNegotiationExt};

/// Compute, for every participant in the graph, whether it is *confirmable*:
/// its net balance is non-negative, or every party it pays is confirmable.
///
/// Returned as a map from participant to a boolean. This is the least fixpoint
/// of the two rules in the module docs, computed by iterating to convergence.
/// Termination is guaranteed: the set of confirmable participants only grows,
/// and it is bounded by the (finite) participant set.
///
/// Note this is a *structural* property of the payment graph — it says who
/// *may* confirm, independent of who *has* confirmed. Cycles resolve because
/// at least one net-non-negative node always exists (`Σ net == 0`).
pub fn confirmable(graph: &PaymentGraph) -> BTreeMap<ParticipantId, bool> {
    let participants = graph.participants();
    let mut confirmable: BTreeMap<ParticipantId, bool> = participants
        .iter()
        .map(|p| (*p, graph.is_net_non_negative(p)))
        .collect();

    // Monotone iteration to a fixpoint: a party becomes confirmable once all
    // of its receivers are confirmable. The confirmable set only grows, so at
    // most |participants| passes are needed.
    loop {
        let mut changed = false;
        for p in &participants {
            if confirmable[p] {
                continue;
            }
            let receivers = graph.receivers_of(p);
            let all_receivers_confirmable = receivers
                .iter()
                .all(|r| confirmable.get(r).copied().unwrap_or(false));
            if all_receivers_confirmable {
                confirmable.insert(*p, true);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    confirmable
}

/// The set of participants that are *required* to have confirmed the current
/// unique id before any sender may sign.
///
/// Per the spec, a sender waits for confirmations from every one of its
/// receivers. The transitive closure of "receiver of a sender" over the graph
/// is exactly the set of net-receivers that appear as someone's payee. Since a
/// party may be both a sender and a receiver, and a net-non-negative receiver
/// is the base case, the required set is: every participant that is the
/// recipient of at least one real payment.
///
/// Equivalently: everyone who must attest so that the money that changed hands
/// is accounted for. A participant that only sends and is nobody's payee is not
/// required to confirm (nobody is waiting on it), but it is itself a sender and
/// must *wait*.
///
/// ## Unresolved-recipient deadlock (review caveat)
///
/// A *pseudonymous* recipient — one the addressing layer could not resolve to a
/// real 32-byte peer id, so it is bucketed under a script-derived
/// [`script_pseudonym`](crate::graph) — can **never** produce a
/// `PSBT_GLOBAL_CONFIRMATION` under a real `peer_id`. Including it here would
/// wedge [`is_ready`] in [`Phase::Confirming`](crate::session::Phase) forever.
/// We therefore **exclude pseudonymous recipients** from the required set and
/// surface them separately via [`unresolved_recipients`]: readiness then depends
/// only on parties that *can* confirm, and a `Ready` verdict while
/// `unresolved_recipients` is non-empty is *known-provisional* (some payee is
/// unidentified). The netting still credits pseudonyms so conservation
/// (`Σ net == 0`) holds; only the *confirmation duty* is dropped for them.
pub fn required_confirmers(graph: &PaymentGraph) -> BTreeSet<ParticipantId> {
    let mut required = BTreeSet::new();
    for edge in graph.edges() {
        if edge.amount_sats > 0 && !graph.is_pseudonymous(&edge.recipient) {
            required.insert(edge.recipient);
        }
    }
    required
}

/// Recipients that receive value but were not resolved to a real peer id.
///
/// Non-empty means [`is_ready`]/[`Phase::Ready`](crate::session::Phase) is only
/// *provisional*: those payees cannot attest, so they are excluded from
/// [`required_confirmers`], and the caller should treat readiness as
/// conditional on resolving them. This is the diagnostic half of the
/// unresolved-recipient fix (see [`required_confirmers`]).
pub fn unresolved_recipients(graph: &PaymentGraph) -> Vec<ParticipantId> {
    graph.unresolved_recipients()
}

/// `true` if `peer` has an attestation in `confirmations` whose `unique_id`
/// equals `current_unique_id`.
pub fn has_confirmed(
    confirmations: &[Confirmation],
    peer: &ParticipantId,
    current_unique_id: &[u8; 32],
) -> bool {
    confirmations
        .iter()
        .any(|c| &c.peer_id == peer && &c.unique_id == current_unique_id)
}

/// Whether a specific `sender` may sign: it must be a sender (has an outgoing
/// payment) and every one of its receivers must have confirmed the current
/// unique id. A non-sender (contributes no outgoing payment) is not gated by
/// this rule and may sign once its outputs are covered — [`can_sign`] returns
/// `true` for it.
pub fn can_sign(
    graph: &PaymentGraph,
    confirmations: &[Confirmation],
    sender: &ParticipantId,
    current_unique_id: &[u8; 32],
) -> bool {
    if !graph.is_sender(sender) {
        // Non-senders (pure receivers / non-participants) are not blocked on
        // confirmations from others; their own outputs are committed by
        // SIGHASH_ALL.
        return true;
    }
    graph
        .receivers_of(sender)
        .iter()
        .all(|r| has_confirmed(confirmations, r, current_unique_id))
}

/// Session-level readiness: every required confirmer has attested to the
/// current unique id.
///
/// When this holds, every sender's `can_sign` predicate is satisfiable and the
/// session may transition to [`Phase::Ready`](crate::session::Phase::Ready).
/// This is the predicate the [`Session`](crate::session::Session) uses to leave
/// [`Phase::Confirming`](crate::session::Phase::Confirming).
///
/// The empty payment graph (no real payments — e.g. a pure coinjoin with no
/// net transfer, or a not-yet-populated session) has no required confirmers
/// and is trivially ready: with no net transfer, SIGHASH_ALL over the output
/// set is sufficient (`psbt.md`: parties which neither send nor receive can
/// sign as soon as their outputs are covered).
pub fn is_ready(
    graph: &PaymentGraph,
    confirmations: &[Confirmation],
    current_unique_id: &[u8; 32],
) -> bool {
    required_confirmers(graph)
        .iter()
        .all(|r| has_confirmed(confirmations, r, current_unique_id))
}

/// Decode every `PSBT_GLOBAL_CONFIRMATION` (subtype `0x21`) attestation carried
/// on the wire in `global`.
///
/// This is the *provenance* source the readiness check must trust: a peer that
/// embeds its confirmation into the PSBT it broadcasts advances readiness for
/// everyone who joins that PSBT, with no side channel. Encrypted or malformed
/// confirmation blobs are skipped (they are not decidable here). Mirrors
/// [`PaymentGraph::from_global`]'s treatment of the payment set.
pub fn confirmations_from_global(global: &Global) -> Vec<Confirmation> {
    global
        .confirmations()
        .into_iter()
        .filter_map(|(_, blob)| Confirmation::decode(&blob).ok())
        .collect()
}

/// Session-level readiness reading the confirmation set **from the wire**.
///
/// Reads the `PSBT_GLOBAL_CONFIRMATION` (`0x21`) attestations embedded in
/// `global` and unions them with any `extra` session-local confirmations (a
/// peer's own not-yet-broadcast attestation), then applies [`is_ready`]. This is
/// the confirmation-provenance fix (review caveat): readiness advances because a
/// peer's embedded `0x21` attestation is actually consulted, not only a
/// side-channel map.
///
/// Pass `&[]` for `extra` to check readiness purely from the wire.
pub fn is_ready_from_global(
    graph: &PaymentGraph,
    global: &Global,
    extra: &[Confirmation],
    current_unique_id: &[u8; 32],
) -> bool {
    let mut confirmations = confirmations_from_global(global);
    confirmations.extend_from_slice(extra);
    is_ready(graph, &confirmations, current_unique_id)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::negotiation::{PAYMENT_KIND_REAL, Payment};

    fn pay(payer: u8, script: &[u8], amount: u64) -> Payment {
        Payment {
            kind: PAYMENT_KIND_REAL,
            payer: [payer; 32],
            amount_sats: amount,
            script_pubkey: script.to_vec(),
            label: String::new(),
        }
    }

    fn resolve_by_first_byte(script: &[u8]) -> Option<ParticipantId> {
        script.first().map(|b| [*b; 32])
    }

    fn confirm(peer: u8, uid: [u8; 32]) -> Confirmation {
        Confirmation {
            peer_id: [peer; 32],
            unique_id: uid,
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        const UID: [u8; 32] = [0x77; 32];
        const STALE: [u8; 32] = [0x11; 32];

        #[test]
        fn empty_graph_is_ready() {
            let g = PaymentGraph::from_payments(&[], &|_| None);
            assert!(is_ready(&g, &[], &UID));
            assert!(required_confirmers(&g).is_empty());
        }

        #[test]
        fn linear_payment_needs_receiver_confirmation() {
            // Alice(0x0a) -> Bob(0x0b): Bob must confirm.
            let g = PaymentGraph::from_payments(&[pay(0x0a, &[0x0b], 1000)], &resolve_by_first_byte);
            assert_eq!(
                required_confirmers(&g),
                BTreeSet::from([[0x0b; 32]])
            );
            // Without Bob's confirmation, not ready and Alice can't sign.
            assert!(!is_ready(&g, &[], &UID));
            assert!(!can_sign(&g, &[], &[0x0a; 32], &UID));
            // Bob is a non-sender; he can sign regardless.
            assert!(can_sign(&g, &[], &[0x0b; 32], &UID));
            // With Bob's confirmation of the current uid, ready.
            let confs = [confirm(0x0b, UID)];
            assert!(is_ready(&g, &confs, &UID));
            assert!(can_sign(&g, &confs, &[0x0a; 32], &UID));
        }

        #[test]
        fn stale_confirmation_does_not_count() {
            let g = PaymentGraph::from_payments(&[pay(0x0a, &[0x0b], 1000)], &resolve_by_first_byte);
            // Bob confirmed an OLD unique id; against the current one he has not.
            let confs = [confirm(0x0b, STALE)];
            assert!(!is_ready(&g, &confs, &UID));
            assert!(!can_sign(&g, &confs, &[0x0a; 32], &UID));
            // Against the id he actually attested, it would be ready.
            assert!(is_ready(&g, &confs, &STALE));
        }

        #[test]
        fn confirmable_base_case_is_net_non_negative() {
            let g = PaymentGraph::from_payments(&[pay(0x0a, &[0x0b], 1000)], &resolve_by_first_byte);
            let c = confirmable(&g);
            assert!(c[&[0x0b; 32]], "net-positive receiver is confirmable");
            // Alice is net-negative; her only receiver Bob is confirmable, so
            // she becomes confirmable transitively.
            assert!(c[&[0x0a; 32]]);
        }

        #[test]
        fn net_negative_receiver_waits_for_its_receivers() {
            // Chain: A -> B (1000), B -> C (600).
            // Net: A -1000, B +400, C +600. B is net-positive here, so this
            // exercises the transitive rule with B both sender and receiver.
            let g = PaymentGraph::from_payments(
                &[pay(0x0a, &[0x0b], 1000), pay(0x0b, &[0x0c], 600)],
                &resolve_by_first_byte,
            );
            // Required confirmers: B and C (both are payees).
            assert_eq!(
                required_confirmers(&g),
                BTreeSet::from([[0x0b; 32], [0x0c; 32]])
            );
            // A (sender) waits on B; B (sender) waits on C.
            assert!(!can_sign(&g, &[], &[0x0a; 32], &UID));
            assert!(!can_sign(&g, &[], &[0x0b; 32], &UID));
            // C confirms first (net-positive, no receivers).
            let confs_c = [confirm(0x0c, UID)];
            assert!(can_sign(&g, &confs_c, &[0x0b; 32], &UID), "B waits only on C");
            assert!(!is_ready(&g, &confs_c, &UID), "B still owes a confirmation");
            // Then B confirms.
            let confs_bc = [confirm(0x0c, UID), confirm(0x0b, UID)];
            assert!(is_ready(&g, &confs_bc, &UID));
            assert!(can_sign(&g, &confs_bc, &[0x0a; 32], &UID));
        }

        #[test]
        fn true_net_negative_receiver() {
            // A -> B (200), B -> C (500). Net: A -200, B -300, C +500.
            // B is a net-NEGATIVE receiver: it must wait for C before it can
            // confirm to A.
            let g = PaymentGraph::from_payments(
                &[pay(0x0a, &[0x0b], 200), pay(0x0b, &[0x0c], 500)],
                &resolve_by_first_byte,
            );
            assert!(!g.is_net_non_negative(&[0x0b; 32]));
            let c = confirmable(&g);
            assert!(c[&[0x0c; 32]], "C is the net-positive base case");
            assert!(c[&[0x0b; 32]], "B confirmable once C is (its only receiver)");
            assert!(c[&[0x0a; 32]]);
        }

        #[test]
        fn cyclic_graph_terminates() {
            // A->B 500, B->C 300, C->A 100. Net: A -400, B +200, C +200.
            let g = PaymentGraph::from_payments(
                &[
                    pay(0x0a, &[0x0b], 500),
                    pay(0x0b, &[0x0c], 300),
                    pay(0x0c, &[0x0a], 100),
                ],
                &resolve_by_first_byte,
            );
            // All three are payees, so all three are required confirmers.
            assert_eq!(
                required_confirmers(&g),
                BTreeSet::from([[0x0a; 32], [0x0b; 32], [0x0c; 32]])
            );
            // confirmable fixpoint: B and C are net-positive base cases; A
            // becomes confirmable once B (its receiver) is. All confirmable ⇒
            // the cycle is broken (no deadlock).
            let c = confirmable(&g);
            assert!(c.values().all(|&v| v), "cycle must fully resolve: {c:?}");
            // Full readiness needs all three to attest the current uid.
            let all = [confirm(0x0a, UID), confirm(0x0b, UID), confirm(0x0c, UID)];
            assert!(is_ready(&g, &all, &UID));
            // Missing any one keeps it in Confirming.
            let missing_a = [confirm(0x0b, UID), confirm(0x0c, UID)];
            assert!(!is_ready(&g, &missing_a, &UID));
        }

        #[test]
        fn non_sender_can_always_sign() {
            // Pure receiver Bob in A->B.
            let g = PaymentGraph::from_payments(&[pay(0x0a, &[0x0b], 1000)], &resolve_by_first_byte);
            assert!(can_sign(&g, &[], &[0x0b; 32], &UID));
            // A non-participant not in the graph at all: also a non-sender.
            assert!(can_sign(&g, &[], &[0x0f; 32], &UID));
        }

        // ── caveat (b): unresolved-recipient deadlock ──────────────────────

        #[test]
        fn pseudonymous_recipient_excluded_from_required_and_not_deadlocking() {
            // Alice pays a script that resolves to NOBODY (unknown recipient).
            // The pseudonymous payee can never confirm, so if it were a required
            // confirmer the session would deadlock. It must be excluded and
            // surfaced as unresolved instead.
            let g = PaymentGraph::from_payments(&[pay(0x0a, &[0xde, 0xad], 1000)], &|_| None);
            // No real peer is required to confirm ⇒ required set is empty ⇒ the
            // session is (provisionally) ready rather than wedged forever.
            assert!(required_confirmers(&g).is_empty());
            assert!(is_ready(&g, &[], &UID), "must not deadlock on a pseudonym");
            // But the unresolved diagnostic flags that Ready is provisional.
            assert_eq!(unresolved_recipients(&g).len(), 1);
        }

        #[test]
        fn resolved_recipient_still_required() {
            // Contrast: a resolved payee IS a required confirmer and gates
            // readiness normally; nothing is flagged unresolved.
            let g = PaymentGraph::from_payments(&[pay(0x0a, &[0x0b], 1000)], &resolve_by_first_byte);
            assert_eq!(required_confirmers(&g), BTreeSet::from([[0x0b; 32]]));
            assert!(!is_ready(&g, &[], &UID));
            assert!(unresolved_recipients(&g).is_empty());
        }

        #[test]
        fn mixed_resolved_and_unresolved_only_gates_on_resolved() {
            // Alice -> Bob(resolved) 500, Alice -> unknown 700. Resolver knows
            // only Bob's script (0x0b); the other script is unresolved.
            let resolve_only_bob = |script: &[u8]| match script.first() {
                Some(0x0b) => Some([0x0b; 32]),
                _ => None,
            };
            let g = PaymentGraph::from_payments(
                &[pay(0x0a, &[0x0b], 500), pay(0x0a, &[0xde, 0xad], 700)],
                &resolve_only_bob,
            );
            // Only Bob is required; the pseudonym is excluded but flagged.
            assert_eq!(required_confirmers(&g), BTreeSet::from([[0x0b; 32]]));
            assert_eq!(unresolved_recipients(&g).len(), 1);
            assert!(!is_ready(&g, &[], &UID));
            assert!(is_ready(&g, &[confirm(0x0b, UID)], &UID), "Bob suffices");
        }

        // ── caveat (c): confirmation provenance from the wire (0x21) ────────

        #[test]
        fn readiness_reads_wire_confirmations() {
            use crate::negotiation::GlobalNegotiationExt;
            let g = PaymentGraph::from_payments(&[pay(0x0a, &[0x0b], 1000)], &resolve_by_first_byte);
            // Bob embeds his confirmation of the current uid into a Global via
            // the 0x21 wire field.
            let mut global = psbt_v2::v2::Global::default();
            assert!(!is_ready_from_global(&g, &global, &[], &UID));
            global.add_confirmation([0xbb; 16], confirm(0x0b, UID).encode());
            // The embedded 0x21 attestation now advances readiness — no side
            // channel needed.
            assert!(is_ready_from_global(&g, &global, &[], &UID));
            // A stale embedded confirmation (wrong uid) does not count.
            let mut stale = psbt_v2::v2::Global::default();
            stale.add_confirmation([0xbb; 16], confirm(0x0b, STALE).encode());
            assert!(!is_ready_from_global(&g, &stale, &[], &UID));
            // decode helper skips undecodable blobs.
            let mut junk = psbt_v2::v2::Global::default();
            junk.add_confirmation([0x01; 16], vec![0xff, 0x00]);
            assert!(confirmations_from_global(&junk).is_empty());
        }

        #[test]
        fn wire_and_extra_confirmations_union() {
            use crate::negotiation::GlobalNegotiationExt;
            // A -> B 500, B -> A 300: both are payees, both required.
            let g = PaymentGraph::from_payments(
                &[pay(0x0a, &[0x0b], 500), pay(0x0b, &[0x0a], 300)],
                &resolve_by_first_byte,
            );
            let mut global = psbt_v2::v2::Global::default();
            global.add_confirmation([0xbb; 16], confirm(0x0b, UID).encode());
            // Wire has Bob; Alice's attestation supplied as session-local extra.
            assert!(!is_ready_from_global(&g, &global, &[], &UID));
            assert!(is_ready_from_global(&g, &global, &[confirm(0x0a, UID)], &UID));
        }
    }
}
