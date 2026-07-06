use concurrent_psbt::global::GlobalSortExt;
use psbt_v2::v2::{Global, Psbt};

use crate::cli::ConcatenateConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: ConcatenateConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    let psbts = config
        .files
        .into_iter()
        .map(|path| io::read_psbt_source(&path, stdin).map(|psbt| (io::source_label(&path), psbt)))
        .collect::<Result<Vec<_>>>()?;
    concatenate_psbts(psbts)
}

pub(crate) fn concatenate_psbts(psbts: Vec<(String, Psbt)>) -> Result<Psbt> {
    let mut psbts = psbts.into_iter();
    let (first_label, mut result) = psbts
        .next()
        .ok_or_else(|| Error::new("concatenate expects at least two ordered PSBTs"))?;
    validate_ordered(&first_label, &result)?;

    let mut count = 1;
    for (label, psbt) in psbts {
        validate_ordered(&label, &psbt)?;
        if !same_global_context(&result.global, &psbt.global) {
            return Err(Error::new(format!(
                "{label} has global fields that differ from the first PSBT; concatenate would discard or reorder global information",
            )));
        }
        result.inputs.extend(psbt.inputs);
        result.outputs.extend(psbt.outputs);
        count += 1;
    }

    if count < 2 {
        return Err(Error::new("concatenate expects at least two ordered PSBTs"));
    }

    result.global.input_count = result.inputs.len();
    result.global.output_count = result.outputs.len();
    Ok(result)
}

fn validate_ordered(label: &str, psbt: &Psbt) -> Result<()> {
    if psbt.global.is_unordered() {
        return Err(Error::new(format!(
            "{label} is unordered; concatenate only appends ordered PSBTs. Use `ptj join` for unordered lattice merges.",
        )));
    }
    Ok(())
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
