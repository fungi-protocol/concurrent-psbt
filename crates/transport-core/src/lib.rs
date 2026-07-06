//! transport-core — the dependency-light layer-0 hub for the transport family.
//!
//! Every `transport-<name>` crate depends on exactly this crate plus its own
//! upstream SDK. transport-core itself pulls in only `bitcoin` and `std`: NO
//! async runtime, NO transport SDK. It owns five things, all authored here:
//!
//!   1. The two [`channel`] traits — [`AnonymousChannel`] and
//!      [`AttributableChannel`] — the uniform abstraction across ALL transports.
//!      Both move opaque bytes; the ONLY difference is what `recv` yields about
//!      the sender (bare bytes vs `(SenderId, bytes)`). This is a MESSAGING
//!      distinction, not a security one; transport-core contains zero
//!      security / threat-model reasoning.
//!   2. [`SenderId`] — the opaque sender-identity newtype an attributable
//!      channel yields. Never interpreted here; the lattice join ignores it.
//!   3. The [`Message`] TLV envelope (Psbt / Payment / Confirmation), moved
//!      verbatim from the ptj CLI (legacy raw-PSBT decode fallback intact).
//!   4. Generic length-prefixed [`framing`] for stream transports, orthogonal
//!      to `Message` (framing delimits records; `Message` tags what a record
//!      is).
//!   5. The driver-facing [`Transport`] seam plus the channel bridges
//!      ([`Transport`] blanket impl for anonymous channels, [`Attributed`]
//!      wrapper for attributable ones) so the sync driver keeps calling
//!      `publish` / `collect` unchanged.
//!
//! It also re-exports the pinned [`bitcoin`] crate so every transport shares
//! the exact same version through the hub rather than pinning it independently.

// A transport moves bytes; strings-as-errors are sufficient. Missing docs on
// public items are worth catching in a small, shared foundation crate.
#![warn(missing_docs)]

pub mod channel;
pub mod error;
pub mod framing;
pub mod message;
pub mod transport;

// Re-export the pinned bitcoin crate so transport-<name> crates share one
// version through the hub (the crate's single, genuinely-used non-std dep,
// honoring the "bitcoin/std only" constraint without a dead dependency).
pub use bitcoin;

pub use channel::{AnonymousChannel, AttributableChannel, SenderId};
pub use error::{Error, Result};
pub use framing::{deframe, frame, read_frame, write_frame, MAX_FRAME_LEN};
pub use message::Message;
pub use transport::{Attributed, Transport};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitcoin_reexport_is_reachable() {
        // Touch the re-exported crate so it is a genuinely-used dependency:
        // build a tiny bitcoin type through the hub path.
        let amount = crate::bitcoin::Amount::from_sat(21_000);
        assert_eq!(amount.to_sat(), 21_000);
    }

    #[test]
    fn end_to_end_message_over_framing() {
        // The two owned formats compose: type-tag a record with Message, then
        // delimit it on a stream with framing.
        let envelope = Message::Payment(vec![0x11; 8]).encode();
        let framed = frame(&envelope);

        let mut buf = framed;
        let record = deframe(&mut buf).unwrap().expect("one full record");
        assert!(buf.is_empty());
        assert_eq!(Message::decode(&record).unwrap(), Message::Payment(vec![0x11; 8]));
    }
}
