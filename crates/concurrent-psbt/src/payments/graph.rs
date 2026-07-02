#![allow(clippy::result_large_err)]

//! Payment-graph netting over the existing `PSBT_GLOBAL_PAYMENT` set.
//!
//! This module is pure logic layered *on top of* the wire field already
//! defined in [`crate::payments::negotiation`] ([`Payment`], subtype `0x20`). It does
//! **not** define a new PSBT field. It reads the grow-only payment set out of
//! a [`Global`] (via [`GlobalNegotiationExt::payments`]) and answers two
//! questions the confirmation protocol needs:
//!
//! 1. **Net flow per participant** — the signed satoshi balance
//!    (received − sent) each party ends with, obtained by summing directed
//!    payment edges (`payer -> recipient`).
//! 2. **Per-pair roles** — for an ordered pair `(a, b)`, is `a` a *sender* to
//!    `b` (has any outgoing payment to `b`), and is `b` net-non-negative? These
//!    are the exact predicates the spec's confirmation ordering rests on
//!    (`psbt.md` §"Confirmation of successful payment prior to signing").
//!
//! ## From payments to outputs
//!
//! A [`Payment`] carries `{payer, amount_sats, script_pubkey, ...}`. The
//! *recipient* of a payment is the party that controls `script_pubkey`; the
//! transaction output that realizes the payment is a txout paying
//! `amount_sats` to `script_pubkey`. The payment graph is therefore a directed
//! multigraph: one edge `payer -> recipient(script_pubkey)` of weight
//! `amount_sats` per real payment. Because the spec treats outputs as an
//! unordered *set* keyed by a universally-unique id (not merged by identity),
//! **the mapping from real payments to outputs is one-to-one**: every real
//! payment corresponds to exactly one output, and netting is a *reporting*
//! view over the edges — it is NOT applied destructively to the output set.
//! (Only the sorter's `merge_same_script_outputs` step, out of scope here,
//! ever sums same-script outputs, and BIP 352 silent payments deliberately
//! avoid even that.) Netting tells the confirmation layer who is a net-sender
//! and who is a net-receiver; it does not rewrite the outputs.
//!
//! Dummy payments (`PAYMENT_KIND_DUMMY`) are padding for graph-degree privacy
//! and carry no real transfer, so they are excluded from netting.

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::hashes::{Hash, HashEngine, sha256t_hash_newtype};
use psbt_v2::v2::Global;

use crate::payments::negotiation::{GlobalNegotiationExt, Payment};

sha256t_hash_newtype! {
    /// Tag for the in-memory recipient-pseudonym domain.
    ///
    /// DERIVED, non-wire constant: never serialized into a PSBT. It only
    /// buckets recipient scripts the [`RecipientResolver`] cannot map to a real
    /// 32-byte peer id, so conservation (`Σ net == 0`) still holds. Uses the
    /// crate's BIP 341-style tagged-hash convention (cf.
    /// `concurrent-psbt/unordered-unique-id` in [`crate::payments::negotiation`]) and is
    /// domain-separated from every wire tag.
    pub struct RecipientPseudonymTag = hash_str("concurrent-psbt/recipient-pseudonym");

    /// A 32-byte pseudonymous recipient id derived from a scriptPubKey.
    #[hash_newtype(forward)]
    pub struct RecipientPseudonymHash(_);
}

/// A participant identity: the 32-byte `payer` field of a [`Payment`], reused
/// as the recipient key (a recipient is identified by the payer id of a
/// payment they later make, or by explicit registration — see
/// [`PaymentGraph::recipient_of`]).
pub type ParticipantId = [u8; 32];

/// A directed real-payment edge `payer -> recipient` of weight `amount_sats`.
///
/// `recipient` is the party controlling the payment's `script_pubkey`. When the
/// recipient's participant id is not independently known, the edge is still
/// counted against the payer (money leaves the payer) but contributes to the
/// recipient balance under a *script-derived* pseudonymous key so the graph
/// stays balanced. See [`PaymentGraph::from_payments`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentEdge {
    /// Paying participant.
    pub payer: ParticipantId,
    /// Receiving participant (or script-derived pseudonym).
    pub recipient: ParticipantId,
    /// Amount transferred, in satoshis.
    pub amount_sats: u64,
}

/// A resolver from a recipient `script_pubkey` to the participant that controls
/// it. Higher layers (Layer 1 addressing) own this mapping; the netting logic
/// takes it as an injected function so this module stays IO-free and testable.
///
/// Returning `None` means "unknown recipient": the edge still debits the payer,
/// and the recipient is bucketed under a deterministic script-derived
/// pseudonym so that conservation (`sum of net balances == 0`) still holds.
pub type RecipientResolver<'a> = dyn Fn(&[u8]) -> Option<ParticipantId> + 'a;

/// The directed payment multigraph derived from a PSBT's payment set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaymentGraph {
    edges: Vec<PaymentEdge>,
    /// Recipient ids that were *not* resolved to a real 32-byte peer id and are
    /// therefore script-derived pseudonyms (see [`script_pseudonym`]). A
    /// pseudonymous recipient can never be attested by a real `peer_id`, so
    /// [`crate::payments::readiness`] excludes it from the required-confirmer set and
    /// surfaces it as an "unresolved recipient" diagnostic (a `Ready` verdict is
    /// then known-provisional). See [`PaymentGraph::is_pseudonymous`].
    pseudonymous: BTreeSet<ParticipantId>,
}

/// Derive a stable pseudonymous participant id for an unknown recipient from
/// its `script_pubkey`, tagged so it can never collide with a real 32-byte
/// payer id chosen elsewhere by accident of the same script bytes.
///
/// This keeps the graph conservative (`Σ net == 0`) even when the addressing
/// layer has not resolved every recipient to a real identity.
fn script_pseudonym(script_pubkey: &[u8]) -> ParticipantId {
    let mut engine = RecipientPseudonymHash::engine();
    engine.input(script_pubkey);
    RecipientPseudonymHash::from_engine(engine).to_byte_array()
}

impl PaymentGraph {
    /// Build the graph from a PSBT's global payment set.
    ///
    /// `resolve` maps a recipient `script_pubkey` to a participant id; unknown
    /// recipients fall back to a script-derived pseudonym (see the private
    /// `script_pseudonym`). Dummy payments and payments that fail to decode
    /// as plaintext (e.g. still-encrypted blobs) are skipped: netting requires
    /// visibility into the plaintext amount, and an encrypted payment is not
    /// yet decidable here.
    pub fn from_global(global: &Global, resolve: &RecipientResolver<'_>) -> Self {
        let payments: Vec<Payment> = global
            .payments()
            .into_iter()
            .filter_map(|(_, blob)| Payment::decode(&blob).ok())
            .collect();
        Self::from_payments(&payments, resolve)
    }

    /// Build the graph from already-decoded payments. Dummy payments are
    /// excluded (they represent no real transfer).
    ///
    /// Self-payments (a payment whose resolved recipient is the payer itself —
    /// e.g. a change output) are also excluded from the edge set: the spec
    /// defines a *sender* as a party that makes an outgoing payment "to another
    /// party", and confirmation is only owed to *another* party. A self-edge
    /// contributes `0` to `Σ net` anyway (it debits and credits the same node),
    /// so dropping it preserves conservation while avoiding a spurious
    /// self-confirmation requirement in [`crate::payments::readiness`].
    pub fn from_payments(payments: &[Payment], resolve: &RecipientResolver<'_>) -> Self {
        let mut edges = Vec::new();
        let mut pseudonymous = BTreeSet::new();
        for p in payments.iter().filter(|p| !p.is_dummy()) {
            let (recipient, resolved) = match resolve(&p.script_pubkey) {
                Some(id) => (id, true),
                None => (script_pseudonym(&p.script_pubkey), false),
            };
            // Self-payments (change) create no edge and no confirmation duty.
            if p.payer == recipient {
                continue;
            }
            if !resolved {
                pseudonymous.insert(recipient);
            }
            edges.push(PaymentEdge {
                payer: p.payer,
                recipient,
                amount_sats: p.amount_sats,
            });
        }
        Self {
            edges,
            pseudonymous,
        }
    }

    /// The directed edges of the graph.
    pub fn edges(&self) -> &[PaymentEdge] {
        &self.edges
    }

    /// Net satoshi balance per participant: `received − sent`.
    ///
    /// Uses `i128` to hold the signed difference without overflow even for the
    /// full `u64` amount range. The sum of all balances is always exactly `0`
    /// (every satoshi that leaves a payer arrives at a recipient) — this
    /// conservation law is what guarantees a net-non-negative participant
    /// always exists, which is what breaks confirmation cycles.
    pub fn net_balances(&self) -> BTreeMap<ParticipantId, i128> {
        let mut balances: BTreeMap<ParticipantId, i128> = BTreeMap::new();
        for edge in &self.edges {
            let amount = i128::from(edge.amount_sats);
            *balances.entry(edge.payer).or_default() -= amount;
            *balances.entry(edge.recipient).or_default() += amount;
        }
        balances
    }

    /// The net balance of a single participant (`received − sent`); `0` if the
    /// participant appears in no edge.
    pub fn net_balance(&self, who: &ParticipantId) -> i128 {
        self.net_balances().get(who).copied().unwrap_or(0)
    }

    /// `true` if `who` has any *outgoing* real payment (is a sender in the
    /// spec's sense), regardless of net balance.
    pub fn is_sender(&self, who: &ParticipantId) -> bool {
        self.edges
            .iter()
            .any(|e| &e.payer == who && e.amount_sats > 0)
    }

    /// `true` if `who` receives at least as much as they send.
    ///
    /// This is the spec's "net balance is non-negative" predicate: such a
    /// receiver can confirm the unique id immediately, and at least one such
    /// participant always exists (`Σ net == 0`), breaking cyclic dependencies.
    pub fn is_net_non_negative(&self, who: &ParticipantId) -> bool {
        self.net_balance(who) >= 0
    }

    /// The set of participants `who` pays (its receivers). A net-negative
    /// receiver must wait for all of *these* to confirm before it confirms in
    /// turn.
    pub fn receivers_of(&self, who: &ParticipantId) -> Vec<ParticipantId> {
        let mut out: Vec<ParticipantId> = self
            .edges
            .iter()
            .filter(|e| &e.payer == who && e.amount_sats > 0)
            .map(|e| e.recipient)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The recipient participant id for a given recipient `script_pubkey`,
    /// under the same resolver-or-pseudonym rule used when building the graph.
    pub fn recipient_of(script_pubkey: &[u8], resolve: &RecipientResolver<'_>) -> ParticipantId {
        resolve(script_pubkey).unwrap_or_else(|| script_pseudonym(script_pubkey))
    }

    /// Every participant that appears in the graph (as payer or recipient),
    /// sorted.
    pub fn participants(&self) -> Vec<ParticipantId> {
        self.net_balances().into_keys().collect()
    }

    /// `true` if `who` is a script-derived pseudonym (an unresolved recipient),
    /// as opposed to a real 32-byte peer id.
    ///
    /// A pseudonymous recipient cannot produce a `PSBT_GLOBAL_CONFIRMATION`
    /// attestation under a real `peer_id`, so [`crate::payments::readiness`] must not gate
    /// readiness on it (that would deadlock forever). See
    /// [`PaymentGraph::unresolved_recipients`].
    pub fn is_pseudonymous(&self, who: &ParticipantId) -> bool {
        self.pseudonymous.contains(who)
    }

    /// The pseudonymous (unresolved-to-a-real-peer) recipients in the graph,
    /// sorted.
    ///
    /// Non-empty means the addressing layer has not resolved every payee to a
    /// real identity: those payees cannot confirm, so a `Ready`/`is_ready`
    /// verdict that ignores them is only *provisional*. Surface this to the
    /// caller alongside readiness (`readiness::unresolved_recipients`).
    pub fn unresolved_recipients(&self) -> Vec<ParticipantId> {
        // Only count pseudonyms that actually receive value (are someone's
        // positive-amount payee); a pseudonym reached only by 0-sat edges is
        // not a real confirmation dependency.
        let mut out: BTreeSet<ParticipantId> = BTreeSet::new();
        for e in &self.edges {
            if e.amount_sats > 0 && self.pseudonymous.contains(&e.recipient) {
                out.insert(e.recipient);
            }
        }
        out.into_iter().collect()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::payments::negotiation::{PAYMENT_KIND_DUMMY, PAYMENT_KIND_REAL};

    fn pay(payer: u8, script: &[u8], amount: u64, kind: u8) -> Payment {
        Payment {
            kind,
            payer: [payer; 32],
            amount_sats: amount,
            script_pubkey: script.to_vec(),
            label: String::new(),
        }
    }

    /// A resolver mapping the first byte of the script to a participant id.
    fn resolve_by_first_byte(script: &[u8]) -> Option<ParticipantId> {
        script.first().map(|b| [*b; 32])
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn empty_graph_has_zero_balances() {
            let g = PaymentGraph::from_payments(&[], &|_| None);
            assert!(g.net_balances().is_empty());
            assert_eq!(g.net_balance(&[0; 32]), 0);
        }

        #[test]
        fn single_payment_debits_payer_credits_recipient() {
            // Alice (0x0a) pays a script controlled by Bob (0x0b).
            let payments = [pay(0x0a, &[0x0b], 1000, PAYMENT_KIND_REAL)];
            let g = PaymentGraph::from_payments(&payments, &resolve_by_first_byte);
            assert_eq!(g.net_balance(&[0x0a; 32]), -1000);
            assert_eq!(g.net_balance(&[0x0b; 32]), 1000);
            // conservation
            let sum: i128 = g.net_balances().values().sum();
            assert_eq!(sum, 0);
        }

        #[test]
        fn dummy_payments_excluded_from_netting() {
            let payments = [
                pay(0x0a, &[0x0b], 1000, PAYMENT_KIND_REAL),
                pay(0x0a, &[0x0b], 999, PAYMENT_KIND_DUMMY),
            ];
            let g = PaymentGraph::from_payments(&payments, &resolve_by_first_byte);
            assert_eq!(g.edges().len(), 1);
            assert_eq!(g.net_balance(&[0x0a; 32]), -1000);
        }

        #[test]
        fn sender_and_role_predicates() {
            let payments = [pay(0x0a, &[0x0b], 1000, PAYMENT_KIND_REAL)];
            let g = PaymentGraph::from_payments(&payments, &resolve_by_first_byte);
            assert!(g.is_sender(&[0x0a; 32]));
            assert!(!g.is_sender(&[0x0b; 32]));
            // Alice is net-negative, Bob net-non-negative.
            assert!(!g.is_net_non_negative(&[0x0a; 32]));
            assert!(g.is_net_non_negative(&[0x0b; 32]));
            assert_eq!(g.receivers_of(&[0x0a; 32]), vec![[0x0b; 32]]);
            assert!(g.receivers_of(&[0x0b; 32]).is_empty());
        }

        #[test]
        fn multiple_payments_between_pair_net_out() {
            // Alice pays Bob 500, Bob pays Alice 300. Net: Alice -200, Bob +200.
            let payments = [
                pay(0x0a, &[0x0b], 500, PAYMENT_KIND_REAL),
                pay(0x0b, &[0x0a], 300, PAYMENT_KIND_REAL),
            ];
            let g = PaymentGraph::from_payments(&payments, &resolve_by_first_byte);
            assert_eq!(g.net_balance(&[0x0a; 32]), -200);
            assert_eq!(g.net_balance(&[0x0b; 32]), 200);
            // Both are senders (each has an outgoing edge).
            assert!(g.is_sender(&[0x0a; 32]));
            assert!(g.is_sender(&[0x0b; 32]));
        }

        #[test]
        fn cyclic_graph_has_a_net_non_negative_node() {
            // Alice->Bob 500, Bob->Carol 300, Carol->Alice 100.
            // Net: Alice -400, Bob +200, Carol +200.
            let payments = [
                pay(0x0a, &[0x0b], 500, PAYMENT_KIND_REAL),
                pay(0x0b, &[0x0c], 300, PAYMENT_KIND_REAL),
                pay(0x0c, &[0x0a], 100, PAYMENT_KIND_REAL),
            ];
            let g = PaymentGraph::from_payments(&payments, &resolve_by_first_byte);
            let sum: i128 = g.net_balances().values().sum();
            assert_eq!(sum, 0, "conservation");
            let any_non_negative = g
                .participants()
                .iter()
                .any(|p| g.is_net_non_negative(p) && g.net_balance(p) > 0);
            assert!(any_non_negative, "a cycle always has a net-positive node");
        }

        #[test]
        fn unknown_recipient_uses_stable_pseudonym_and_conserves() {
            let payments = [pay(0x0a, &[0xde, 0xad], 700, PAYMENT_KIND_REAL)];
            let g = PaymentGraph::from_payments(&payments, &|_| None);
            let sum: i128 = g.net_balances().values().sum();
            assert_eq!(sum, 0);
            // The pseudonym is deterministic for the same script.
            let pseudo = script_pseudonym(&[0xde, 0xad]);
            assert_eq!(g.net_balance(&pseudo), 700);
            assert_eq!(PaymentGraph::recipient_of(&[0xde, 0xad], &|_| None), pseudo);
        }

        #[test]
        fn pseudonym_is_flagged_unresolved_resolved_is_not() {
            // Unresolved recipient (resolver returns None) → pseudonymous.
            let unresolved = [pay(0x0a, &[0xde, 0xad], 700, PAYMENT_KIND_REAL)];
            let g = PaymentGraph::from_payments(&unresolved, &|_| None);
            let pseudo = script_pseudonym(&[0xde, 0xad]);
            assert!(g.is_pseudonymous(&pseudo));
            assert_eq!(g.unresolved_recipients(), vec![pseudo]);
            // Payer is not pseudonymous.
            assert!(!g.is_pseudonymous(&[0x0a; 32]));

            // Resolved recipient → NOT pseudonymous, no unresolved diagnostic.
            let resolved = [pay(0x0a, &[0x0b], 700, PAYMENT_KIND_REAL)];
            let g2 = PaymentGraph::from_payments(&resolved, &resolve_by_first_byte);
            assert!(!g2.is_pseudonymous(&[0x0b; 32]));
            assert!(g2.unresolved_recipients().is_empty());
        }

        #[test]
        fn from_global_reads_payment_set() {
            let mut global = Global::default();
            let p = pay(0x0a, &[0x0b], 1234, PAYMENT_KIND_REAL);
            global.add_payment([1u8; 16], p.encode());
            let g = PaymentGraph::from_global(&global, &resolve_by_first_byte);
            assert_eq!(g.edges().len(), 1);
            assert_eq!(g.net_balance(&[0x0a; 32]), -1234);
        }

        #[test]
        fn from_global_skips_undecodable_blobs() {
            let mut global = Global::default();
            // A garbage blob that is not a valid plaintext payment.
            global.add_payment([2u8; 16], vec![0xff, 0x00, 0x01]);
            let g = PaymentGraph::from_global(&global, &resolve_by_first_byte);
            assert!(g.edges().is_empty());
        }

        #[test]
        fn self_payment_is_excluded() {
            // Alice pays a script that resolves to Alice herself (change).
            // This is not a payment "to another party": no edge, no
            // self-confirmation requirement, and conservation still holds.
            let payments = [pay(0x0a, &[0x0a], 1000, PAYMENT_KIND_REAL)];
            let g = PaymentGraph::from_payments(&payments, &resolve_by_first_byte);
            assert!(g.edges().is_empty(), "self-payment must not create an edge");
            assert!(!g.is_sender(&[0x0a; 32]));
            assert!(g.receivers_of(&[0x0a; 32]).is_empty());
            assert_eq!(g.net_balance(&[0x0a; 32]), 0);
            let sum: i128 = g.net_balances().values().sum();
            assert_eq!(sum, 0);
        }

        #[test]
        fn zero_amount_payment_is_not_a_send() {
            let payments = [pay(0x0a, &[0x0b], 0, PAYMENT_KIND_REAL)];
            let g = PaymentGraph::from_payments(&payments, &resolve_by_first_byte);
            assert!(!g.is_sender(&[0x0a; 32]));
            assert!(g.receivers_of(&[0x0a; 32]).is_empty());
        }
    }
}
