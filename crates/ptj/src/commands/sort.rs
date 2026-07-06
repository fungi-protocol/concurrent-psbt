use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::sorter::{Sorter, Unset};
use psbt_v2::v2::Psbt;

use crate::cli::SortConfig;
use crate::{Result, io};

pub(super) fn run(config: SortConfig) -> Result<Psbt> {
    let constructor = io::read_modifiable(&config.file)?;
    let mut psbt = constructor.into_inner();
    if let Some(seed) = config.seed {
        psbt.global.set_sort_seed(seed.into_bytes());
    }

    let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(psbt);
    sorter
        .into_ordered_psbt()
        .map_err(|error| crate::Error::new(error.to_string()))
}
