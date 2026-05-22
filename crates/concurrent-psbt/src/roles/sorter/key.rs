use bitcoin::consensus::Encodable;
use bitcoin::hashes::{Hash, HashEngine, sha256, sha256t_hash_newtype};

use super::SortError;
use crate::input::{Input, out_point};
use crate::output::Output;

sha256t_hash_newtype! {
    /// Tag for the BIP 341-style tagged hash used to derive sort keys.
    pub struct SortKeyTag = hash_str("concurrent-psbt/deterministic-ordering");

    /// A sort key derived via tagged hash: `H(seed || identifier)`.
    #[hash_newtype(forward)]
    pub struct SortKeyHash(_);
}

/// Derive a sort key from a seed and an identifier via tagged hash.
///
/// `SHA256(H(tag) || H(tag) || seed || id)` where
/// `H(tag)` = SHA256("concurrent-psbt/deterministic-ordering") and
/// `write_id` writes `id` into the hash engine.
pub(super) fn derive_sort_key(
    seed: &[u8],
    write_id: impl FnOnce(&mut sha256::HashEngine),
) -> [u8; 32] {
    let mut engine = SortKeyHash::engine();
    engine.input(seed);
    write_id(&mut engine);
    SortKeyHash::from_engine(engine).to_byte_array()
}

/// Write the input's identifier, the consensus encoding of its outpoint.
pub(super) fn write_input_sort_identifier(input: &Input, engine: &mut sha256::HashEngine) {
    out_point(input)
        .consensus_encode(engine)
        .expect("writing to a hash engine cannot fail");
}

/// Write the output's identifier, its unique ID.
pub(super) fn write_output_sort_identifier(output: &Output, engine: &mut sha256::HashEngine) {
    engine.input(output.unique_id().as_bytes());
}

pub(super) fn derived_input_sort_key(
    seed: Option<&[u8]>,
    input: &Input,
) -> Result<Vec<u8>, SortError> {
    let seed = seed.ok_or(SortError::MissingSortSeed)?;
    Ok(derive_sort_key(seed, |engine| write_input_sort_identifier(input, engine)).to_vec())
}

pub(super) fn derived_output_sort_key(
    seed: Option<&[u8]>,
    output: &Output,
) -> Result<Vec<u8>, SortError> {
    let seed = seed.ok_or(SortError::MissingSortSeed)?;
    Ok(derive_sort_key(seed, |engine| write_output_sort_identifier(output, engine)).to_vec())
}

pub(super) fn explicit_input_sort_key(input: &Input) -> Result<Vec<u8>, SortError> {
    input
        .sort_key()
        .ok_or(SortError::MissingInputSortKey(out_point(input)))
        .map(|key| key.to_vec())
}

pub(super) fn explicit_output_sort_key(output: &Output) -> Result<Vec<u8>, SortError> {
    output
        .sort_key()
        .ok_or(SortError::MissingOutputSortKey)
        .map(|key| key.to_vec())
}

pub(super) fn mixed_input_sort_key(
    seed: Option<&[u8]>,
    input: &Input,
) -> Result<Vec<u8>, SortError> {
    explicit_input_sort_key(input).or_else(|_| derived_input_sort_key(seed, input))
}

pub(super) fn mixed_output_sort_key(
    seed: Option<&[u8]>,
    output: &Output,
) -> Result<Vec<u8>, SortError> {
    explicit_output_sort_key(output).or_else(|_| derived_output_sort_key(seed, output))
}
