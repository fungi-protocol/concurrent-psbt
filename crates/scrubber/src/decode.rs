use psbt_v2::bitcoin::consensus::encode::{Decodable, MAX_VEC_SIZE, VarInt};
use psbt_v2::raw::{Key, Pair};

use crate::scrub::Error;

// Workaround: Pair::decode and Key::decode are pub(crate) in psbt-v2 0.3.0.
// This replicates the upstream logic exactly using the same bitcoin primitives.
// This can be removed once / if the visibility becomes more permissible upstream
pub(crate) trait PairDecode: Sized {
    fn decode(input: &mut &[u8]) -> Result<Option<Self>, Error>;
}

impl PairDecode for Pair {
    fn decode(input: &mut &[u8]) -> Result<Option<Self>, Error> {
        let VarInt(byte_size) =
            Decodable::consensus_decode(input).map_err(|_| Error::UnexpectedEof)?;
        if byte_size == 0 {
            return Ok(None);
        }
        let key_byte_size = byte_size - 1;
        if key_byte_size > MAX_VEC_SIZE as u64 {
            return Err(Error::UnexpectedEof);
        }
        let type_value: u8 =
            Decodable::consensus_decode(input).map_err(|_| Error::UnexpectedEof)?;
        let mut key = Vec::with_capacity(key_byte_size as usize);
        for _ in 0..key_byte_size {
            key.push(Decodable::consensus_decode(input).map_err(|_| Error::UnexpectedEof)?);
        }
        let value: Vec<u8> =
            Decodable::consensus_decode(input).map_err(|_| Error::UnexpectedEof)?;
        Ok(Some(Pair {
            key: Key { type_value, key },
            value,
        }))
    }
}
