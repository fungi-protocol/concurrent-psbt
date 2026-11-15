use bitcoin::consensus::Encodable;
use bitcoin::hashes::{Hash, HashEngine, sha256, sha256t_hash_newtype};

use super::SortError;
use crate::global::Global;
use crate::input::{Input, out_point};
use crate::output::Output;

sha256t_hash_newtype! {
    /// Tag for the BIP 340-style tagged hash of `PSBT_GLOBAL_TX_UUID`.
    pub struct UuidTag = hash_str("concurrent-psbt/uuid");

    /// The tagged hash of `PSBT_GLOBAL_TX_UUID`.
    #[hash_newtype(forward)]
    pub struct UuidHash(_);

    /// Tag for the BIP 340-style tagged hash of the auxiliary
    /// `PSBT_GLOBAL_SORT_SEED`.
    pub struct AuxSortSeedTag = hash_str("concurrent-psbt/aux-sort-seed");

    /// The tagged hash of `PSBT_GLOBAL_SORT_SEED`, empty if unset.
    #[hash_newtype(forward)]
    pub struct AuxSortSeedHash(_);

    /// Tag for the BIP 340-style tagged hash that combines the UUID and the
    /// auxiliary seed into the sort seed.
    pub struct SortSeedTag = hash_str("concurrent-psbt/sort-seed");

    /// The sort seed: `H(UuidHash || AuxSortSeedHash)`.
    #[hash_newtype(forward)]
    pub struct SortSeedHash(_);

    /// Tag for the BIP 340-style tagged hash used to derive sort keys.
    pub struct SortKeyTag = hash_str("concurrent-psbt/deterministic-ordering");

    /// A derived sort key: `H(SortSeedHash || identifier)`.
    #[hash_newtype(forward)]
    pub struct SortKeyHash(_);
}

/// The key an input or output is sorted by: its explicit sort key, empty if
/// unset, then its derived sort key, which breaks ties between equal explicit
/// keys.
pub(super) type FullSortKey = (Vec<u8>, [u8; 32]);

/// The sort seed of `global`: the [`SortSeedHash`] of the [`UuidHash`] of
/// `PSBT_GLOBAL_TX_UUID` and the [`AuxSortSeedHash`] of
/// `PSBT_GLOBAL_SORT_SEED`, an unset seed hashing as empty.
///
/// # Errors
/// Returns [`SortError::MissingUuid`] if the UUID is unset.
pub(super) fn sort_seed(global: &Global) -> Result<SortSeedHash, SortError> {
    let uuid = global.uuid().ok_or(SortError::MissingUuid)?;
    let aux = global.sort_seed().unwrap_or_default();
    let mut engine = SortSeedHash::engine();
    engine.input(UuidHash::hash(uuid).as_byte_array());
    engine.input(AuxSortSeedHash::hash(aux).as_byte_array());
    Ok(SortSeedHash::from_engine(engine))
}

/// The tagged hash engine of `global`'s derived sort keys, with its
/// [`sort_seed`] written in.
///
/// # Errors
/// Returns the errors of [`sort_seed`].
pub(super) fn derivation_engine(global: &Global) -> Result<sha256::HashEngine, SortError> {
    let mut engine = SortKeyHash::engine();
    engine.input(sort_seed(global)?.as_byte_array());
    Ok(engine)
}

/// Finish the derived sort key of the identifier `write_id` writes.
fn derive_sort_key(
    engine: &sha256::HashEngine,
    write_id: impl FnOnce(&mut sha256::HashEngine),
) -> [u8; 32] {
    let mut engine = engine.clone();
    write_id(&mut engine);
    SortKeyHash::from_engine(engine).to_byte_array()
}

/// The full sort key of `input`: its identifier is the consensus encoding of
/// its outpoint.
///
/// # Errors
/// Returns [`SortError::MissingInputSortKey`] if `explicit` and the input has
/// no explicit sort key.
pub(super) fn input_sort_key(
    engine: &sha256::HashEngine,
    explicit: bool,
    input: &Input,
) -> Result<FullSortKey, SortError> {
    let key = match input.sort_key() {
        None if explicit => return Err(SortError::MissingInputSortKey(out_point(input))),
        key => key.unwrap_or_default().to_vec(),
    };
    let derived = derive_sort_key(engine, |engine| {
        out_point(input)
            .consensus_encode(engine)
            .expect("writing to a hash engine cannot fail");
    });
    Ok((key, derived))
}

/// The full sort key of `output`: its identifier is its unique ID.
///
/// # Errors
/// Returns [`SortError::MissingOutputSortKey`] if `explicit` and the output
/// has no explicit sort key.
pub(super) fn output_sort_key(
    engine: &sha256::HashEngine,
    explicit: bool,
    output: &Output,
) -> Result<FullSortKey, SortError> {
    let key = match output.sort_key() {
        None if explicit => return Err(SortError::MissingOutputSortKey),
        key => key.unwrap_or_default().to_vec(),
    };
    let derived = derive_sort_key(engine, |engine| {
        engine.input(output.unique_id().as_bytes());
    });
    Ok((key, derived))
}
