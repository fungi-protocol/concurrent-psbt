#![allow(clippy::result_large_err)]

//! IO-free session state machine driving join → confirmation → export.
//!
//! Mirrors the `Session` sketch in `contrib/design/traits.md` and the
//! lifecycle in `contrib/design/ptj-net.md` (§Session lifecycle). The session
//! is a *pure function from messages to state*: it processes serialized PSBTs
//! and confirmation attestations, computes the lattice join locally, and
//! reports a [`Phase`]. It never performs IO — the caller owns the transport.
//!
//! ```text
//! Contributing → Converging → Confirming → Ready
//!   (only local)  (join has    (join clean,  (every required
//!                  conflicts     all peers     confirmer has
//!                  or peers      present)      attested current uid)
//!                  missing)
//! ```
//!
//! The confirmation logic is entirely in [`crate::readiness`] over the payment
//! graph in [`crate::graph`]; the confirmation and payment *wire* fields are
//! the pre-existing `PSBT_GLOBAL_CONFIRMATION`/`PSBT_GLOBAL_PAYMENT` sets in
//! [`crate::negotiation`]. This module only sequences them.

use crate::graph::{ParticipantId, PaymentGraph, RecipientResolver};
use crate::lattice::join::Join;
use crate::negotiation::{Confirmation, unordered_unique_id};
use crate::tx::{ResultUnorderedPsbt, UnorderedPsbt, UnorderedPsbtError};

use psbt_v2::v2::Psbt;

/// The four session phases (matches `traits.md` and the Cap'n Proto / WIT
/// enums).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No peer PSBTs yet — only the local contribution has been processed.
    Contributing,
    /// The join has conflicts, or not all expected peers have been seen.
    Converging,
    /// The join is conflict-free and all expected peers are present; awaiting
    /// confirmations of the current unique id.
    Confirming,
    /// Every required confirmer has attested the current unique id — ready to
    /// sign.
    Ready,
}

/// Errors surfaced by [`Session::process`] and [`Session::export`].
#[derive(Debug, Clone, PartialEq)]
pub enum SessionError {
    /// A processed message was not a decodable v2 PSBT.
    Decode,
    /// The message was a PSBT but could not enter the unordered domain
    /// (e.g. an output missing `PSBT_OUT_UNIQUE_ID`).
    Unordered(UnorderedPsbtError),
    /// [`Session::export`] was called while the join still has conflicts.
    Conflicted,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode => write!(f, "message is not a valid v2 PSBT"),
            Self::Unordered(e) => write!(f, "cannot enter unordered domain: {e}"),
            Self::Conflicted => write!(f, "join has unresolved conflicts"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<UnorderedPsbtError> for SessionError {
    fn from(e: UnorderedPsbtError) -> Self {
        SessionError::Unordered(e)
    }
}

/// IO-free collaborative-construction session.
///
/// `resolve` maps a recipient `script_pubkey` to a participant id for the
/// payment-graph netting; it is injected so the core stays free of Layer 1
/// addressing concerns. The `'r` lifetime ties the session to the resolver.
pub struct Session<'r> {
    /// Accumulated lattice join of every PSBT seen so far (local + peers).
    state: ResultUnorderedPsbt,
    /// How many peer contributions to expect before leaving `Converging`.
    /// `None` (CLI mode) means "as many as processed"; presence is not gated.
    expected_peers: Option<usize>,
    /// How many distinct PSBT messages have been folded in (including local).
    contributions: usize,
    /// Confirmations recorded via [`Session::add_confirmation`], keyed by peer.
    /// A later confirmation from the same peer supersedes an earlier one
    /// (monotone toward the current LUB, per the eventual-consistency rule).
    confirmations: std::collections::BTreeMap<ParticipantId, Confirmation>,
    resolve: &'r RecipientResolver<'r>,
}

impl<'r> Session<'r> {
    /// Start a session from a serialized local PSBT.
    ///
    /// `expected_peers` is the total number of contributions (including the
    /// local one) required before the session may leave `Converging`. `None`
    /// disables the presence gate (the CLI feeds a fixed file list and expects
    /// `Ready` once all are processed).
    ///
    /// # Errors
    /// Returns [`SessionError::Decode`] if `local_psbt` is not a valid PSBT, or
    /// [`SessionError::Unordered`] if it cannot enter the unordered domain.
    pub fn new(
        local_psbt: &[u8],
        expected_peers: Option<usize>,
        resolve: &'r RecipientResolver<'r>,
    ) -> Result<Self, SessionError> {
        let state = parse(local_psbt)?;
        Ok(Session {
            state,
            expected_peers,
            contributions: 1,
            confirmations: std::collections::BTreeMap::new(),
            resolve,
        })
    }

    /// Fold a serialized peer PSBT into the join. Idempotent: re-processing the
    /// same PSBT does not change the state (the lattice absorbs duplicates).
    ///
    /// # Errors
    /// Returns [`SessionError::Decode`]/[`SessionError::Unordered`] if the
    /// message is not a joinable PSBT.
    pub fn process(&mut self, message: &[u8]) -> Result<Phase, SessionError> {
        let incoming = parse(message)?;
        // Join is commutative/associative/idempotent, so ordering and dupes are
        // harmless. Take by value and rejoin.
        let prev = std::mem::replace(&mut self.state, empty_state());
        self.state = prev.join(incoming);
        self.contributions += 1;
        Ok(self.phase())
    }

    /// Record a peer's confirmation of a unique id. A newer confirmation from
    /// the same peer replaces the older one (monotone re-confirmation).
    pub fn add_confirmation(&mut self, peer_id: &[u8], unique_id: &[u8]) {
        let (Ok(peer), Ok(uid)) = (
            <[u8; 32]>::try_from(peer_id),
            <[u8; 32]>::try_from(unique_id),
        ) else {
            return;
        };
        self.confirmations.insert(
            peer,
            Confirmation {
                peer_id: peer,
                unique_id: uid,
            },
        );
    }

    /// This peer's own confirmation of the current unique id, or `None` if the
    /// join still has conflicts (nothing stable to attest yet).
    pub fn local_confirmation(&self, peer_id: &[u8]) -> Option<Confirmation> {
        let peer = <[u8; 32]>::try_from(peer_id).ok()?;
        let uid = self.current_unique_id()?;
        Some(Confirmation {
            peer_id: peer,
            unique_id: uid,
        })
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        if self.contributions <= 1 {
            return Phase::Contributing;
        }
        if !self.state.is_ok() {
            return Phase::Converging;
        }
        if let Some(expected) = self.expected_peers
            && self.contributions < expected
        {
            return Phase::Converging;
        }
        // Join is clean and all expected peers present. Decide Confirming vs
        // Ready from the confirmation set against the current unique id.
        let Some(uid) = self.current_unique_id() else {
            return Phase::Converging;
        };
        let psbt = self.export_ordered_unchecked();
        let graph = PaymentGraph::from_global(&psbt.global, self.resolve);
        // Confirmation provenance: read the WIRE confirmation set
        // (PSBT_GLOBAL_CONFIRMATION 0x21) embedded in the joined PSBT AND union
        // it with any session-local confirmations recorded via
        // `add_confirmation`. A peer's embedded attestation therefore advances
        // readiness with no side channel (review caveat).
        let extra: Vec<Confirmation> = self.confirmations.values().cloned().collect();
        if crate::readiness::is_ready_from_global(&graph, &psbt.global, &extra, &uid) {
            Phase::Ready
        } else {
            Phase::Confirming
        }
    }

    /// Recipients that receive value but were not resolved to a real peer id.
    ///
    /// Non-empty means a [`Phase::Ready`] verdict is *provisional*: some payee is
    /// unidentified and cannot attest, so it is excluded from the readiness gate
    /// (see [`crate::readiness::required_confirmers`]). The caller should treat
    /// readiness as conditional on resolving these. Empty while the join is
    /// conflicted.
    pub fn unresolved_recipients(&self) -> Vec<crate::graph::ParticipantId> {
        crate::readiness::unresolved_recipients(&self.payment_graph())
    }

    /// The order-independent unique id of the current joined PSBT, or `None`
    /// while the join has conflicts.
    pub fn current_unique_id(&self) -> Option<[u8; 32]> {
        if !self.state.is_ok() {
            return None;
        }
        Some(unordered_unique_id(&self.export_ordered_unchecked()))
    }

    /// Export the joined PSBT, if conflict-free.
    ///
    /// # Errors
    /// Returns [`SessionError::Conflicted`] if any field still conflicts.
    pub fn export(&self) -> Result<UnorderedPsbt, SessionError> {
        self.state
            .clone()
            .try_unwrap()
            .map_err(|_| SessionError::Conflicted)
    }

    /// Access the raw joined state, including conflicts, for diagnostics.
    pub fn export_raw(&self) -> &ResultUnorderedPsbt {
        &self.state
    }

    /// The payment graph over the current joined state (empty while
    /// conflicted).
    pub fn payment_graph(&self) -> PaymentGraph {
        match self.state.clone().try_unwrap() {
            Ok(u) => PaymentGraph::from_global(&u.into_psbt().global, self.resolve),
            Err(_) => PaymentGraph::default(),
        }
    }

    // ---- internals ----

    /// Build a `Psbt` view of the current clean state for id computation. Only
    /// called when `state.is_ok()`; `unordered_unique_id` is permutation
    /// invariant, so the arbitrary `into_psbt` order does not matter.
    fn export_ordered_unchecked(&self) -> Psbt {
        self.state
            .clone()
            .try_unwrap()
            .expect("caller checked is_ok")
            .into_psbt()
    }
}

fn parse(message: &[u8]) -> Result<ResultUnorderedPsbt, SessionError> {
    let psbt = Psbt::deserialize(message).map_err(|_| SessionError::Decode)?;
    ResultUnorderedPsbt::try_from_psbt(psbt).map_err(SessionError::from)
}

fn empty_state() -> ResultUnorderedPsbt {
    UnorderedPsbt {
        global: psbt_v2::v2::Global::default(),
        inputs: crate::input::InputSet::default(),
        outputs: crate::output::OutputSet::default(),
    }
    .wrap()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::negotiation::{GlobalNegotiationExt, PAYMENT_KIND_REAL, Payment};
    use crate::output::{OutputUniqueIdExt, UniqueId};

    fn resolve_by_first_byte(script: &[u8]) -> Option<ParticipantId> {
        script.first().map(|b| [*b; 32])
    }

    /// A serialized PSBT carrying one payment Alice(0x0a) -> script [0x0b].
    fn psbt_with_payment(payer: u8, script: &[u8], amount: u64, uid: u8) -> Vec<u8> {
        let mut global = psbt_v2::v2::Global::default();
        global.add_payment(
            [uid; 16],
            Payment {
                kind: PAYMENT_KIND_REAL,
                payer: [payer; 32],
                amount_sats: amount,
                script_pubkey: script.to_vec(),
                label: String::new(),
            }
            .encode(),
        );
        // Give the PSBT one output so it has stable content; must carry a UID.
        let mut output = psbt_v2::v2::Output {
            amount: bitcoin::Amount::from_sat(amount),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(script.to_vec()),
            ..Default::default()
        };
        output.set_unique_id(UniqueId::new(vec![uid; 16]));
        global.output_count = 1;
        let psbt = Psbt {
            global,
            inputs: vec![],
            outputs: vec![output],
        };
        Psbt::serialize(&psbt)
    }

    fn empty_psbt() -> Vec<u8> {
        Psbt::serialize(&Psbt {
            global: psbt_v2::v2::Global::default(),
            inputs: vec![],
            outputs: vec![],
        })
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn new_session_is_contributing() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let s = Session::new(&empty_psbt(), None, r).unwrap();
            assert_eq!(s.phase(), Phase::Contributing);
        }

        #[test]
        fn bad_message_is_decode_error() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let err = Session::new(&[0xff, 0x00], None, r).err();
            assert_eq!(err, Some(SessionError::Decode));
        }

        #[test]
        fn process_bad_message_is_decode_error() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let mut s = Session::new(&empty_psbt(), None, r).unwrap();
            assert_eq!(s.process(&[0xff, 0x00]), Err(SessionError::Decode));
        }

        #[test]
        fn empty_transfer_session_is_ready_after_peers() {
            // Two PSBTs with no payments: no net transfer, trivially ready.
            let r: &RecipientResolver = &resolve_by_first_byte;
            let mut s = Session::new(&empty_psbt(), None, r).unwrap();
            let phase = s.process(&empty_psbt()).unwrap();
            assert_eq!(phase, Phase::Ready, "no payments ⇒ nothing to confirm");
        }

        #[test]
        fn payment_session_waits_for_confirmation() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let local = psbt_with_payment(0x0a, &[0x0b], 1000, 1);
            let mut s = Session::new(&local, None, r).unwrap();
            // Peer re-sends the same PSBT (idempotent join keeps one payment).
            let phase = s.process(&local).unwrap();
            assert_eq!(phase, Phase::Confirming, "Bob has not confirmed yet");

            // Bob confirms the current unique id.
            let uid = s.current_unique_id().unwrap();
            s.add_confirmation(&[0x0b; 32], &uid);
            assert_eq!(s.phase(), Phase::Ready);
        }

        #[test]
        fn stale_confirmation_keeps_confirming() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let local = psbt_with_payment(0x0a, &[0x0b], 1000, 1);
            let mut s = Session::new(&local, None, r).unwrap();
            s.process(&local).unwrap();
            // Bob confirms a wrong / stale id.
            s.add_confirmation(&[0x0b; 32], &[0x11; 32]);
            assert_eq!(s.phase(), Phase::Confirming);
            // Re-confirming the correct id supersedes it.
            let uid = s.current_unique_id().unwrap();
            s.add_confirmation(&[0x0b; 32], &uid);
            assert_eq!(s.phase(), Phase::Ready);
        }

        #[test]
        fn expected_peers_gates_confirming() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let local = psbt_with_payment(0x0a, &[0x0b], 1000, 1);
            // Expect 3 total contributions.
            let mut s = Session::new(&local, Some(3), r).unwrap();
            assert_eq!(s.phase(), Phase::Contributing);
            s.process(&local).unwrap();
            // Only 2 of 3 present ⇒ still converging even though join is clean.
            assert_eq!(s.phase(), Phase::Converging);
            s.process(&local).unwrap();
            // 3 present, join clean, but Bob unconfirmed ⇒ Confirming.
            assert_eq!(s.phase(), Phase::Confirming);
        }

        #[test]
        fn conflict_is_converging_and_no_local_confirmation() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            // Two PSBTs with conflicting tx_version → join conflicts.
            let a = psbt_v2::v2::Global {
                tx_version: bitcoin::transaction::Version::ONE,
                ..Default::default()
            };
            let b = psbt_v2::v2::Global {
                tx_version: bitcoin::transaction::Version::TWO,
                ..Default::default()
            };
            let pa = Psbt::serialize(&Psbt {
                global: a,
                inputs: vec![],
                outputs: vec![],
            });
            let pb = Psbt::serialize(&Psbt {
                global: b,
                inputs: vec![],
                outputs: vec![],
            });
            let mut s = Session::new(&pa, None, r).unwrap();
            s.process(&pb).unwrap();
            assert_eq!(s.phase(), Phase::Converging);
            assert!(s.current_unique_id().is_none());
            assert!(s.local_confirmation(&[0x0b; 32]).is_none());
            assert_eq!(s.export(), Err(SessionError::Conflicted));
        }

        #[test]
        fn export_returns_joined_psbt_when_clean() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let local = psbt_with_payment(0x0a, &[0x0b], 1000, 1);
            let mut s = Session::new(&local, None, r).unwrap();
            s.process(&local).unwrap();
            assert!(s.export().is_ok());
        }

        #[test]
        fn local_confirmation_matches_current_id() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let local = psbt_with_payment(0x0a, &[0x0b], 1000, 1);
            let s = Session::new(&local, None, r).unwrap();
            let uid = s.current_unique_id().unwrap();
            let conf = s.local_confirmation(&[0x0a; 32]).unwrap();
            assert_eq!(conf.unique_id, uid);
            assert_eq!(conf.peer_id, [0x0a; 32]);
        }

        #[test]
        fn cli_flow_two_senders_cycle_ready_after_all_confirm() {
            // Alice -> Bob 500 (uid 1), Bob -> Alice 300 (uid 2): net Alice -200.
            // Both are senders; both must be confirmed by their receiver.
            let r: &RecipientResolver = &resolve_by_first_byte;
            let alice = psbt_with_payment(0x0a, &[0x0b], 500, 1);
            let bob = psbt_with_payment(0x0b, &[0x0a], 300, 2);
            let mut s = Session::new(&alice, None, r).unwrap();
            s.process(&bob).unwrap();
            assert_eq!(s.phase(), Phase::Confirming);
            let uid = s.current_unique_id().unwrap();
            s.add_confirmation(&[0x0b; 32], &uid); // Bob is Alice's receiver
            assert_eq!(s.phase(), Phase::Confirming, "Alice still owes Bob");
            s.add_confirmation(&[0x0a; 32], &uid); // Alice is Bob's receiver
            assert_eq!(s.phase(), Phase::Ready);
        }

        #[test]
        fn add_confirmation_rejects_wrong_length() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let mut s = Session::new(&empty_psbt(), None, r).unwrap();
            s.add_confirmation(&[1, 2, 3], &[4, 5, 6]); // ignored, wrong length
            assert!(s.confirmations.is_empty());
        }

        #[test]
        fn payment_graph_accessor() {
            let r: &RecipientResolver = &resolve_by_first_byte;
            let local = psbt_with_payment(0x0a, &[0x0b], 1000, 1);
            let s = Session::new(&local, None, r).unwrap();
            let g = s.payment_graph();
            assert_eq!(g.net_balance(&[0x0a; 32]), -1000);
        }

        #[test]
        fn embedded_wire_confirmation_advances_readiness() {
            // Provenance caveat, end to end: Bob's confirmation rides IN the
            // PSBT he broadcasts (PSBT_GLOBAL_CONFIRMATION 0x21), and it must
            // advance the session to Ready with NO side-channel add_confirmation.
            let r: &RecipientResolver = &resolve_by_first_byte;
            let local = psbt_with_payment(0x0a, &[0x0b], 1000, 1);
            let mut s = Session::new(&local, None, r).unwrap();
            s.process(&local).unwrap();
            assert_eq!(s.phase(), Phase::Confirming, "Bob has not confirmed yet");
            let uid = s.current_unique_id().unwrap();

            // Peer broadcasts a PSBT that embeds Bob's 0x21 attestation of uid.
            let mut peer = psbt_v2::v2::Global::default();
            peer.add_payment(
                [1u8; 16],
                Payment {
                    kind: PAYMENT_KIND_REAL,
                    payer: [0x0a; 32],
                    amount_sats: 1000,
                    script_pubkey: vec![0x0b],
                    label: String::new(),
                }
                .encode(),
            );
            peer.add_confirmation(
                [0xbb; 16],
                Confirmation {
                    peer_id: [0x0b; 32],
                    unique_id: uid,
                }
                .encode(),
            );
            let mut output = psbt_v2::v2::Output {
                amount: bitcoin::Amount::from_sat(1000),
                script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0x0b]),
                ..Default::default()
            };
            output.set_unique_id(UniqueId::new(vec![1u8; 16]));
            peer.output_count = 1;
            let peer_msg = Psbt::serialize(&Psbt {
                global: peer,
                inputs: vec![],
                outputs: vec![output],
            });

            let phase = s.process(&peer_msg).unwrap();
            assert_eq!(
                phase,
                Phase::Ready,
                "embedded 0x21 confirmation must advance readiness"
            );
        }

        #[test]
        fn unresolved_recipient_is_provisionally_ready() {
            // Alice pays an UNRESOLVED script (resolver returns None). The payee
            // cannot confirm; the session must not deadlock — it reports Ready
            // (provisional) and flags the unresolved recipient.
            let r: &RecipientResolver = &|_| None;
            let local = psbt_with_payment(0x0a, &[0xde, 0xad], 1000, 1);
            let mut s = Session::new(&local, None, r).unwrap();
            s.process(&local).unwrap();
            assert_eq!(
                s.phase(),
                Phase::Ready,
                "must not wedge on an unconfirmable pseudonym"
            );
            assert_eq!(s.unresolved_recipients().len(), 1);
        }
    }
}
