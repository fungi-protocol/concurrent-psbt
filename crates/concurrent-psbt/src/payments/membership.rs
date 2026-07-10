//! Transport-free shared-session membership and PSBT state.
//!
//! A party edge is monotonic: joining sessions unions their parties and joins
//! their PSBT fragments in the existing conflict-preserving result domain.
//! Transaction consent is intentionally absent from this type; Bitcoin
//! signatures express consent after construction converges.

use std::collections::BTreeSet;

use crate::tx::ResultUnorderedPsbt;
use crate::Join;

/// Ephemeral identity of a party in a shared session.
pub type PartyId = crate::payments::graph::ParticipantId;

/// Shared monotonic state of one collaborative PSBT session.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedSession {
    parties: BTreeSet<PartyId>,
    state: ResultUnorderedPsbt,
}

impl SharedSession {
    /// Promote a local PSBT fragment into a session owned by its first party.
    pub fn promote(party: PartyId, state: ResultUnorderedPsbt) -> Self {
        Self {
            parties: [party].into(),
            state,
        }
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
    fn join(self, other: Self) -> Self {
        let Self { mut parties, state } = self;
        parties.extend(other.parties);
        Self {
            parties,
            state: state.join(other.state),
        }
    }
}
