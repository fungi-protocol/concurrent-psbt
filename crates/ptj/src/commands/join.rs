use std::collections::BTreeSet;
use std::path::Path;

use concurrent_psbt::Join;
use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::output::OutputUniqueIdExt;
use concurrent_psbt::roles::combiner::ResultOrderedPsbt;
use concurrent_psbt::roles::constructor::dynamic;
use psbt_v2::v2::Psbt;

use crate::cli::JoinConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: JoinConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    join_sources(config.files.iter().map(std::path::PathBuf::as_path), stdin)
}

pub(crate) fn join_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Result<Psbt> {
    let psbts = paths
        .into_iter()
        .map(io::read_psbt)
        .collect::<Result<Vec<_>>>()?;
    join_psbts(psbts)
}

pub(crate) fn join_sources<'a>(
    paths: impl IntoIterator<Item = &'a Path>,
    stdin: Option<&[u8]>,
) -> Result<Psbt> {
    let psbts = paths
        .into_iter()
        .map(|path| io::read_psbt_source(path, stdin))
        .collect::<Result<Vec<_>>>()?;
    join_psbts(psbts)
}

pub(crate) fn join_psbts(psbts: impl IntoIterator<Item = Psbt>) -> Result<Psbt> {
    let psbts: Vec<Psbt> = psbts.into_iter().collect();

    // Joinability: a modifiable PSBT is joinable only while unordered —
    // additions rely on set identity, not position. An unmodifiable PSBT is
    // always joinable: updating and signing only add data to fixed entries,
    // which is monotone.
    for psbt in &psbts {
        if psbt.global.tx_modifiable_flags & 0x03 != 0 && !psbt.global.is_unordered() {
            return Err(Error::new(
                "PSBT is not joinable: modifiable but not unordered",
            ));
        }
    }

    if psbts.iter().all(|psbt| psbt.global.is_unordered()) {
        join_unordered(psbts)
    } else if psbts.iter().any(|psbt| psbt.global.is_unordered()) {
        Err(Error::new(
            "PSBTs are not joinable: cannot mix unordered and ordered PSBTs",
        ))
    } else {
        combine_ordered(psbts)
    }
}

/// Join unordered PSBTs: inputs keyed by outpoint, outputs by unique id.
fn join_unordered(psbts: Vec<Psbt>) -> Result<Psbt> {
    // Distinct identifiers between operands (added inputs or outputs) are
    // allowed only while the corresponding TX_MODIFIABLE bit is set. The key
    // sets are recorded before parsing consumes the operands.
    let inputs_differ = differ(psbts.iter().map(|psbt| {
        psbt.inputs
            .iter()
            .map(|input| (input.previous_txid, input.spent_output_index))
            .collect::<BTreeSet<_>>()
    }));
    let outputs_differ = differ(psbts.iter().map(|psbt| {
        psbt.outputs
            .iter()
            .map(|output| output.unique_id().map(|id| id.as_bytes().to_vec()))
            .collect::<BTreeSet<_>>()
    }));

    let result = psbts
        .into_iter()
        .map(|psbt| {
            dynamic::ResultConstructor::try_from_psbt(psbt)
                .map_err(|error| Error::new(format!("PSBT is not joinable: {error}")))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .reduce(|left, right| left.join(right))
        .ok_or_else(|| Error::new("join expects at least one PSBT file"))?;

    if !result.is_ok() {
        return Err(conflict_error(|f| {
            result.for_each_conflict(|section, field, conflict| f(section, field, conflict));
        }));
    }

    let constructor = match result.try_unwrap() {
        Ok(constructor) => constructor,
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    };
    let psbt = constructor.into_psbt();

    let flags = psbt.global.tx_modifiable_flags;
    if inputs_differ && flags & 0x01 == 0 {
        return Err(Error::new(
            "PSBTs are not joinable: input sets differ but inputs are not modifiable",
        ));
    }
    if outputs_differ && flags & 0x02 == 0 {
        return Err(Error::new(
            "PSBTs are not joinable: output sets differ but outputs are not modifiable",
        ));
    }
    Ok(psbt)
}

/// Combine ordered PSBTs positionally: entry i of every operand must describe
/// the same input or output, and each field joins in the strict result domain.
fn combine_ordered(psbts: Vec<Psbt>) -> Result<Psbt> {
    let result = psbts
        .into_iter()
        .map(ResultOrderedPsbt::from_psbt)
        .reduce(|left, right| left.join(right))
        .ok_or_else(|| Error::new("join expects at least one PSBT file"))?;

    if !result.is_ok() {
        return Err(conflict_error(|f| {
            result.for_each_conflict(|section, field, conflict| f(section, field, conflict));
        }));
    }

    match result.try_unwrap() {
        Ok(psbt) => Ok(psbt),
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    }
}

fn differ<T: Eq>(sets: impl IntoIterator<Item = T>) -> bool {
    let mut sets = sets.into_iter();
    match sets.next() {
        Some(first) => sets.any(|set| set != first),
        None => false,
    }
}

fn conflict_error(visit: impl FnOnce(&mut dyn FnMut(&str, &str, &dyn std::fmt::Debug))) -> Error {
    let mut details = vec![
        "join produced conflicting fields".to_string(),
        String::new(),
    ];
    visit(&mut |section, field, conflict| {
        details.push(format!("  {section}.{field}: {conflict:?}"));
    });
    Error::new(details.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use bitcoin::hashes::Hash;
    use concurrent_psbt::output::UniqueId;
    use psbt_v2::v2::{Global, Input, Output};

    fn input(txid_byte: u8) -> Input {
        Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
            vout: 0,
        })
    }

    fn output(amount: u64, uid: u8) -> Output {
        let mut output = Output {
            amount: bitcoin::Amount::from_sat(amount),
            ..Output::default()
        };
        output.set_unique_id(UniqueId::new(vec![uid]));
        output
    }

    fn psbt(flags: u8, unordered: bool, inputs: Vec<Input>, outputs: Vec<Output>) -> Psbt {
        let mut global = Global {
            tx_modifiable_flags: flags,
            input_count: inputs.len(),
            output_count: outputs.len(),
            ..Global::default()
        };
        if unordered {
            global.set_unordered();
        }
        Psbt {
            global,
            inputs,
            outputs,
        }
    }

    #[test]
    fn unmodifiable_ordered_is_joinable() {
        // Two parties enrich the same signable skeleton in parallel:
        // updating and signing are monotone, so the join must succeed.
        let mut left = psbt(0x00, false, vec![input(1), input(2)], vec![output(10, 1)]);
        left.inputs[0].unknowns.insert(
            psbt_v2::raw::Key {
                type_value: 0xa0,
                key: vec![],
            },
            vec![1],
        );
        let mut right = left.clone();
        right.inputs[0].unknowns.clear();
        right.inputs[1].unknowns.insert(
            psbt_v2::raw::Key {
                type_value: 0xa1,
                key: vec![],
            },
            vec![2],
        );

        let joined = join_psbts([left, right]).expect("unmodifiable PSBTs are joinable");
        assert_eq!(joined.inputs[0].unknowns.len(), 1);
        assert_eq!(joined.inputs[1].unknowns.len(), 1);
        // Positional combining preserves the committed order.
        assert_eq!(
            joined.inputs[0].previous_txid,
            bitcoin::Txid::from_byte_array([1; 32])
        );
    }

    #[test]
    fn modifiable_unordered_is_joinable() {
        // Construction phase: both sides add outputs; the union survives
        // because the outputs-modifiable bit is still set.
        let left = psbt(0x03, true, vec![input(1)], vec![output(10, 1)]);
        let right = psbt(0x03, true, vec![input(1)], vec![output(20, 2)]);

        let joined = join_psbts([left, right]).expect("modifiable unordered PSBTs are joinable");
        assert_eq!(joined.outputs.len(), 2);
    }

    #[test]
    fn modifiable_ordered_is_refused() {
        let left = psbt(0x03, false, vec![input(1)], vec![output(10, 1)]);
        let right = left.clone();

        let error = join_psbts([left, right]).expect_err("modifiable requires unordered");
        assert!(
            error.to_string().contains("modifiable but not unordered"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn mixed_phases_are_refused() {
        let unordered = psbt(0x00, true, vec![input(1)], vec![output(10, 1)]);
        let ordered = psbt(0x00, false, vec![input(1)], vec![output(10, 1)]);

        let error = join_psbts([unordered, ordered]).expect_err("phases cannot mix");
        assert!(
            error.to_string().contains("unordered and ordered"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn unordered_additions_require_the_modifiable_bit() {
        // Both operands are unordered but no longer modifiable: their sets
        // are frozen, so an output present on one side only must refuse.
        let left = psbt(0x00, true, vec![input(1)], vec![output(10, 1)]);
        let right = psbt(0x00, true, vec![input(1)], vec![output(10, 1), output(20, 2)]);

        let error = join_psbts([left, right]).expect_err("frozen sets must not grow");
        assert!(
            error.to_string().contains("output sets differ"),
            "unexpected error: {error}"
        );
    }
}
