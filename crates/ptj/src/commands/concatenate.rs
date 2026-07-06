use concurrent_psbt::global::GlobalSortExt;
use psbt_v2::v2::{Global, Psbt};

use crate::cli::ConcatenateConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: ConcatenateConfig) -> Result<Psbt> {
    let mut files = config.files.into_iter();
    let first_path = files
        .next()
        .ok_or_else(|| Error::new("concatenate expects at least two ordered PSBT files"))?;
    let mut result = read_ordered(&first_path)?;

    for path in files {
        let psbt = read_ordered(&path)?;
        if !same_global_context(&result.global, &psbt.global) {
            return Err(Error::new(format!(
                "{} has global fields that differ from the first PSBT; concatenate would discard or reorder global information",
                path.display()
            )));
        }
        result.inputs.extend(psbt.inputs);
        result.outputs.extend(psbt.outputs);
    }

    result.global.input_count = result.inputs.len();
    result.global.output_count = result.outputs.len();
    Ok(result)
}

fn read_ordered(path: &std::path::Path) -> Result<Psbt> {
    let psbt = io::read_psbt(path)?;
    if psbt.global.is_unordered() {
        return Err(Error::new(format!(
            "{} is unordered; concatenate only appends ordered PSBTs. Use `ptj join` for unordered lattice merges.",
            path.display()
        )));
    }
    Ok(psbt)
}

fn same_global_context(left: &Global, right: &Global) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.input_count = 0;
    left.output_count = 0;
    right.input_count = 0;
    right.output_count = 0;
    left == right
}
