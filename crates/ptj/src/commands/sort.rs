use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::sorter::{Deterministic, ExplicitSortKeys, Sorter, Unset};
use concurrent_psbt::tx::UnorderedPsbt;
use psbt_v2::v2::Psbt;

use crate::cli::SortConfig;
use crate::{Result, io};

pub(super) fn run(config: SortConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    let constructor = io::read_modifiable_source(&config.file, stdin)?;
    sort_psbt(
        constructor.into_inner(),
        config.seed.map(crate::cli::HexSeed::into_bytes),
    )
}

pub(crate) fn sort_psbt(mut psbt: UnorderedPsbt, seed: Option<Vec<u8>>) -> Result<Psbt> {
    if let Some(seed) = seed {
        psbt.global.set_sort_seed(seed);
    }
    match psbt.global.sort_deterministic() {
        Some(0x01) => {
            let sorter: Sorter<Deterministic> = Sorter::from_unordered_psbt(psbt);
            sorter.into_ordered_psbt()
        }
        Some(0x00) => {
            let sorter: Sorter<ExplicitSortKeys> = Sorter::from_unordered_psbt(psbt);
            sorter.into_ordered_psbt()
        }
        _ => {
            let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(psbt);
            sorter.into_ordered_psbt()
        }
    }
    .map_err(|error| crate::Error::new(error.to_string()))
}
