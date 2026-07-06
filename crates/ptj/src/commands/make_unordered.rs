use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::roles::constructor::dynamic;
use psbt_v2::v2::Psbt;

use crate::cli::MakeUnorderedConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: MakeUnorderedConfig) -> Result<Psbt> {
    let mut psbt = io::read_psbt(&config.file)?;
    psbt.global.set_unordered();
    dynamic::Constructor::try_from_psbt(psbt)
        .map(dynamic::Constructor::into_psbt)
        .map_err(|error| Error::new(format!("{}: {error}", config.file.display())))
}
