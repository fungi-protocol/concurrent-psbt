use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::sorter::{Sorter, Unset};
use concurrent_psbt::tx::UnorderedPsbt;
use psbt_v2::v2::Psbt;

use crate::cli::SortConfig;
use crate::{Result, io};

pub(super) fn run(config: SortConfig) -> Result<Psbt> {
    let constructor = io::read_modifiable(&config.file)?;
    sort_psbt(
        constructor.into_inner(),
        config.seed.map(crate::cli::HexSeed::into_bytes),
    )
}

pub(crate) fn sort_psbt(mut psbt: UnorderedPsbt, seed: Option<Vec<u8>>) -> Result<Psbt> {
    if let Some(seed) = seed {
        psbt.global.set_sort_seed(seed);
    }
    let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(psbt);
    sorter
        .into_ordered_psbt()
        .map_err(|error| crate::Error::new(error.to_string()))
}
