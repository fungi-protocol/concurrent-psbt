//! TLV envelope for transport messages.
//!
//! A transport moves opaque byte blobs; the envelope says what each blob
//! is: a serialized PSBT to fold into the lattice join, or an out-of-band
//! negotiation message (a payment a peer wants constructed, or a
//! confirmation of a converged result). Encoding is a single TLV record:
//! one type byte, a u32 big-endian length, then the value bytes.
//!
//! Moved verbatim from the ptj CLI's `crate::transport::message` (now `pub`)
//! so every `transport-<name>` crate shares one envelope and the legacy
//! raw-PSBT decode fallback. This is orthogonal to [`crate::framing`]: the
//! `Message` type-tag says WHAT a record is; framing delimits records on a
//! stream.

use crate::{Error, Result};

const TYPE_PSBT: u8 = 0x00;
const TYPE_PAYMENT: u8 = 0x01;
const TYPE_CONFIRMATION: u8 = 0x02;

/// One transport message.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A serialized PSBT (the base64 text form produced by `io::encode_psbt`).
    Psbt(Vec<u8>),
    /// An opaque payment record (negotiation metadata, not part of the join).
    Payment(Vec<u8>),
    /// An opaque confirmation record.
    Confirmation(Vec<u8>),
}

impl Message {
    fn type_byte(&self) -> u8 {
        match self {
            Message::Psbt(_) => TYPE_PSBT,
            Message::Payment(_) => TYPE_PAYMENT,
            Message::Confirmation(_) => TYPE_CONFIRMATION,
        }
    }

    fn value(&self) -> &[u8] {
        match self {
            Message::Psbt(value) | Message::Payment(value) | Message::Confirmation(value) => value,
        }
    }

    /// Encode as a TLV record: type byte, u32 big-endian length, value.
    pub fn encode(&self) -> Vec<u8> {
        let value = self.value();
        let mut out = Vec::with_capacity(5 + value.len());
        out.push(self.type_byte());
        out.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
        out.extend_from_slice(value);
        out
    }

    /// Decode a TLV record, falling back to treating the whole blob as a
    /// legacy raw PSBT when it does not parse as an envelope.
    ///
    /// The fallback is unambiguous: legacy blobs are base64 text
    /// (`io::encode_psbt`, first byte `c` of `cHNidP...`) or binary PSBTs
    /// (magic `psbt`, first byte 0x70), both far above the envelope type
    /// range 0x00..=0x02.
    pub fn decode(bytes: &[u8]) -> Result<Message> {
        match bytes.first() {
            Some(&kind @ (TYPE_PSBT | TYPE_PAYMENT | TYPE_CONFIRMATION)) => {
                if bytes.len() < 5 {
                    return Err(Error::new("transport message too short for TLV header"));
                }
                let len = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
                let value = &bytes[5..];
                if value.len() != len {
                    return Err(Error::new(format!(
                        "transport message length mismatch: header says {len}, got {}",
                        value.len()
                    )));
                }
                let value = value.to_vec();
                Ok(match kind {
                    TYPE_PSBT => Message::Psbt(value),
                    TYPE_PAYMENT => Message::Payment(value),
                    _ => Message::Confirmation(value),
                })
            }
            Some(_) => Ok(Message::Psbt(bytes.to_vec())),
            None => Err(Error::new("empty transport message")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_kinds() {
        for message in [
            Message::Psbt(b"cHNidP8BAgQC".to_vec()),
            Message::Payment(vec![0xAA; 40]),
            Message::Confirmation(vec![0xBB; 65]),
        ] {
            assert_eq!(Message::decode(&message.encode()).unwrap(), message);
        }
    }

    #[test]
    fn legacy_raw_psbt_falls_back() {
        let legacy = b"cHNidP8BAgQCAAAAAQMEAAAAAAEEAQABBQEAAQb8D2NvbmN1cnJlbnQtcHNidBABAQAA";
        assert_eq!(
            Message::decode(legacy).unwrap(),
            Message::Psbt(legacy.to_vec())
        );
    }

    #[test]
    fn legacy_binary_psbt_magic_falls_back() {
        // Binary PSBT magic is `psbt\xff` -> first byte 0x70, above the envelope
        // type range, so it decodes via the legacy fallback as a raw PSBT.
        let legacy = b"psbt\xffrest-of-binary-psbt";
        assert_eq!(
            Message::decode(legacy).unwrap(),
            Message::Psbt(legacy.to_vec())
        );
    }

    #[test]
    fn truncated_envelope_is_error() {
        assert!(Message::decode(&[TYPE_PAYMENT, 0, 0]).is_err());
        assert!(Message::decode(&[]).is_err());
        let mut bad = Message::Payment(vec![1, 2, 3]).encode();
        bad.truncate(bad.len() - 1);
        assert!(Message::decode(&bad).is_err());
    }

    #[test]
    fn empty_value_roundtrips() {
        for message in [
            Message::Psbt(Vec::new()),
            Message::Payment(Vec::new()),
            Message::Confirmation(Vec::new()),
        ] {
            let encoded = message.encode();
            assert_eq!(encoded.len(), 5);
            assert_eq!(Message::decode(&encoded).unwrap(), message);
        }
    }
}
