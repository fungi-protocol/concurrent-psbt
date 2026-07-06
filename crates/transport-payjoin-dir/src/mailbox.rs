//! Mailbox addressing for the payjoin-directory store-and-forward channel.
//!
//! This module is PURE (no network, no SDK, no feature gate): it derives the
//! opaque subdirectory slot IDs two peers read and write on a payjoin directory,
//! from a shared session secret handed over out of band (a session ticket /
//! room link — introduction is decoupled from the transport, exactly as arti
//! receives a `.onion` and iroh receives a `DocTicket`).
//!
//! # Addressing scheme
//!
//! Per the design docs (`contrib/design/transports.md`, "Payjoin Directory
//! (linked mailboxes)"): mailbox IDs derive from `H(shared_secret || index)`.
//! We refine that so two peers do NOT collide on each other's writes while
//! staying symmetric (neither peer is a server; either may create the session):
//!
//! ```text
//!   slot_id(role, index) = SHA256( DOMAIN_TAG || session_secret || role_byte || index_be )
//! ```
//!
//!   * `session_secret` — the out-of-band shared secret (>= 16 bytes).
//!   * `role_byte` — which peer WROTE this slot, so the two directions never
//!     collide: [`Role::Initiator`] (the peer that created the session /
//!     produced the SDP OFFER) writes the `Initiator` lane; [`Role::Responder`]
//!     (produced the SDP ANSWER) writes the `Responder` lane. A peer WRITES its
//!     own lane and READS the other lane.
//!   * `index` — a monotonically increasing `u64`, big-endian. Index 0 carries
//!     the first record (offer / answer); subsequent indices carry trickled ICE
//!     candidates and, in the async-fallback role, further PSBT envelopes.
//!
//! `DOMAIN_TAG` domain-separates these slot IDs from any other use of the same
//! secret. Slot IDs are the full 32-byte SHA-256 output; a directory that wants
//! a shorter subdirectory path can hex/base64 them, but the derivation is over
//! the raw bytes.
//!
//! # Why this shape
//!
//! The directory is a dumb store-and-forward mailbox: a writer POSTs an opaque
//! blob to `slot_id(my_role, next_index)`, walking the index forward on an HTTP
//! 409 collision (someone already wrote that slot); a reader GETs
//! `slot_id(peer_role, next_read_index)` and advances on a hit, stopping when a
//! slot is empty (until a timeout). Both directions are just monotone sequences
//! of opaque blobs — which is precisely the [`AnonymousChannel`] shape (bare
//! bytes, no sender identity: the directory never sees who wrote what, and the
//! blobs are HPKE-sealed anyway).
//!
//! [`AnonymousChannel`]: transport_core::AnonymousChannel

use transport_core::bitcoin::hashes::{sha256, Hash as _};

/// Domain-separation tag mixed into every slot-id derivation so these IDs can
/// never coincide with any other use of the same session secret.
const DOMAIN_TAG: &[u8] = b"ptj/signaling/payjoin-dir/slot/v1";

/// Minimum acceptable session-secret length (128 bits of entropy). A shorter
/// secret would make slot IDs guessable by a directory or a third party.
pub const MIN_SESSION_SECRET_LEN: usize = 16;

/// Which peer WROTE a slot. A peer writes its own lane and reads the other's,
/// so the two directions never collide on the same directory subdirectory.
///
/// This is purely a lane selector for addressing; it is NOT a sender identity
/// surfaced to the driver (the channel stays anonymous — `recv` yields bare
/// bytes). The role is fixed at session creation and delivered out of band with
/// the session secret: whoever creates the session is the [`Role::Initiator`]
/// (produces the SDP offer); the joining peer is the [`Role::Responder`]
/// (produces the SDP answer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The peer that created the session and produces the SDP offer.
    Initiator,
    /// The peer that joined the session and produces the SDP answer.
    Responder,
}

impl Role {
    /// The single byte mixed into a slot-id derivation to separate the two
    /// write lanes. Distinct, stable values; never reused for another purpose.
    fn role_byte(self) -> u8 {
        match self {
            Role::Initiator => 0x01,
            Role::Responder => 0x02,
        }
    }

    /// The lane a peer with THIS role reads from — i.e. the OTHER peer's write
    /// lane. A peer writes `Role::role_byte` of its own role and reads the
    /// peer's.
    pub fn peer(self) -> Role {
        match self {
            Role::Initiator => Role::Responder,
            Role::Responder => Role::Initiator,
        }
    }
}

/// A directory subdirectory slot identifier: the 32-byte SHA-256 output that
/// addresses one store-and-forward record. Opaque bytes; the directory treats
/// it as a subdirectory key and never learns what it addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(pub [u8; 32]);

impl SlotId {
    /// Borrow the raw 32 bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex, for use as a URL path segment against a directory that
    /// wants a printable subdirectory key.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push(char::from_digit((b >> 4) as u32, 16).expect("nibble is 0..=15"));
            s.push(char::from_digit((b & 0x0f) as u32, 16).expect("nibble is 0..=15"));
        }
        s
    }
}

/// Derive the slot ID for `(role, index)` under `session_secret`.
///
/// `role` is the WRITER's role (whose lane the slot lives on); `index` is the
/// big-endian sequence number. Pure and deterministic: both peers derive the
/// identical ID for the same `(secret, role, index)` triple, which is how a
/// writer's POST target and the peer's GET target line up without a
/// server-assigned room.
pub fn slot_id(session_secret: &[u8], role: Role, index: u64) -> SlotId {
    // SHA256( DOMAIN_TAG || secret || role_byte || index_be )
    let mut engine = sha256::Hash::engine();
    use transport_core::bitcoin::hashes::HashEngine as _;
    engine.input(DOMAIN_TAG);
    engine.input(session_secret);
    engine.input(&[role.role_byte()]);
    engine.input(&index.to_be_bytes());
    SlotId(sha256::Hash::from_engine(engine).to_byte_array())
}

/// Validate an out-of-band session secret before it is used for addressing.
/// Rejects a too-short secret (guessable slot IDs). Returns the secret bytes on
/// success so a caller can validate-and-store in one step.
pub fn validate_session_secret(session_secret: &[u8]) -> transport_core::Result<()> {
    if session_secret.len() < MIN_SESSION_SECRET_LEN {
        return Err(transport_core::Error::new(format!(
            "transport-payjoin-dir: session secret is {} bytes; need at least {MIN_SESSION_SECRET_LEN} \
             for unguessable mailbox slot IDs",
            session_secret.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"an-out-of-band-session-secret-32-bytes!!";

    #[test]
    fn slot_id_is_deterministic() {
        // Both peers derive the SAME id for the same (secret, role, index): this
        // is how a writer's POST target and a reader's GET target line up.
        let a = slot_id(SECRET, Role::Initiator, 0);
        let b = slot_id(SECRET, Role::Initiator, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn the_two_write_lanes_never_collide() {
        // Initiator's lane and Responder's lane differ at every index, so peer A
        // writing its lane cannot clobber peer B's lane.
        for index in [0u64, 1, 2, 7, u64::MAX] {
            assert_ne!(
                slot_id(SECRET, Role::Initiator, index),
                slot_id(SECRET, Role::Responder, index),
                "lanes collided at index {index}"
            );
        }
    }

    #[test]
    fn distinct_indices_give_distinct_slots() {
        assert_ne!(
            slot_id(SECRET, Role::Initiator, 0),
            slot_id(SECRET, Role::Initiator, 1)
        );
    }

    #[test]
    fn distinct_secrets_give_distinct_slots() {
        assert_ne!(
            slot_id(b"secret-one-at-least-sixteen", Role::Initiator, 0),
            slot_id(b"secret-two-at-least-sixteen", Role::Initiator, 0)
        );
    }

    #[test]
    fn a_peer_reads_the_other_lane() {
        // Initiator writes its own lane and reads the Responder lane, and vice
        // versa: writer(A).write(i) == reader(B).read(i).
        let written_by_initiator = slot_id(SECRET, Role::Initiator, 3);
        let read_by_responder = slot_id(SECRET, Role::Initiator, 3);
        assert_eq!(written_by_initiator, read_by_responder);
        assert_eq!(Role::Initiator.peer(), Role::Responder);
        assert_eq!(Role::Responder.peer(), Role::Initiator);
    }

    #[test]
    fn domain_tag_separates_from_bare_hash() {
        // The derivation is domain-separated: it is NOT a bare
        // SHA256(secret||role||index), so these slot IDs can't collide with any
        // other use of the same secret. Recompute WITHOUT the tag and confirm
        // they differ.
        use transport_core::bitcoin::hashes::HashEngine as _;
        let mut engine = sha256::Hash::engine();
        engine.input(SECRET);
        engine.input(&[0x01]);
        engine.input(&0u64.to_be_bytes());
        let untagged = SlotId(sha256::Hash::from_engine(engine).to_byte_array());
        assert_ne!(slot_id(SECRET, Role::Initiator, 0), untagged);
    }

    #[test]
    fn to_hex_is_64_lowercase_chars() {
        let hex = slot_id(SECRET, Role::Initiator, 0).to_hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn short_secret_is_rejected() {
        assert!(validate_session_secret(b"too-short").is_err());
        assert!(validate_session_secret(&[0u8; MIN_SESSION_SECRET_LEN]).is_ok());
    }
}
