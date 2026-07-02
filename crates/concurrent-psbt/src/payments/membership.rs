//! Transport-free shared-session membership and PSBT state.
//!
//! A party edge is monotonic: joining sessions unions their parties and joins
//! their PSBT fragments in the existing conflict-preserving result domain.
//! Transaction consent is intentionally absent from this type; Bitcoin
//! signatures express consent after construction converges.

use std::collections::BTreeSet;

use crate::Join;
use crate::tx::ResultUnorderedPsbt;

/// Ephemeral identity of a party in a shared session.
pub type PartyId = crate::payments::graph::ParticipantId;

/// A contribution arrived from a party that has not joined this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownParty(pub PartyId);

/// Shared monotonic state of one collaborative PSBT session.
///
/// This is a local value type, not a wire message. Transport adapters attribute
/// remote fragments to a paired party and pass them to [`Self::contribute`];
/// they must not treat an untrusted remote value as a session to [`Join::join`].
#[derive(Debug, Clone, PartialEq)]
pub struct SharedSession {
    parties: BTreeSet<PartyId>,
    state: ResultUnorderedPsbt,
}

impl SharedSession {
    /// Promote a local-only PSBT fragment into a session owned by its first party.
    pub fn promote(party: PartyId, state: ResultUnorderedPsbt) -> Self {
        Self {
            parties: [party].into(),
            state,
        }
    }

    /// Add a party to the session's grow-only membership set.
    pub fn pair(&mut self, party: PartyId) {
        self.parties.insert(party);
    }

    /// Join a paired party's fragment into the shared PSBT state.
    ///
    /// # Errors
    ///
    /// Returns [`UnknownParty`] without changing state when `party` has not
    /// joined this session.
    pub fn contribute(
        &mut self,
        party: PartyId,
        fragment: ResultUnorderedPsbt,
    ) -> Result<(), UnknownParty> {
        if !self.parties.contains(&party) {
            return Err(UnknownParty(party));
        }
        self.state = self.state.clone().join(fragment);
        Ok(())
    }

    /// Iterate over every party that has joined the session.
    pub fn parties(&self) -> impl Iterator<Item = &PartyId> {
        self.parties.iter()
    }

    /// Inspect the joined PSBT state, including any typed conflicts.
    pub fn state(&self) -> &ResultUnorderedPsbt {
        &self.state
    }
}

impl Join for SharedSession {
    /// Merge two already-established local sessions, including all parties.
    fn join(self, other: Self) -> Self {
        let Self { mut parties, state } = self;
        parties.extend(other.parties);
        Self {
            parties,
            state: state.join(other.state),
        }
    }
}
