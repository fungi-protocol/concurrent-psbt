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

// Retained convenience wrapper: file paths -> join. `ptj sync` now gathers via
// the LocalTransport (bytes), so this is no longer on the sync path, but it
// stays as a direct file-join helper for callers/tests.
#[allow(dead_code)]
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
    let mut psbts: Vec<Psbt> = psbts.into_iter().collect();

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

    // Frozen-set rule (psbt.md): distinct identifiers between operands are
    // allowed only while the corresponding TX_MODIFIABLE bit is set. An
    // operand whose bit is clear has frozen its set — the join can only
    // succeed if that frozen set already IS the union, so every operand with
    // the bit still set contributes at most a subset of it.
    let input_sets: Vec<BTreeSet<_>> = psbts
        .iter()
        .map(|psbt| {
            psbt.inputs
                .iter()
                .map(|input| (input.previous_txid, input.spent_output_index))
                .collect()
        })
        .collect();
    let output_sets: Vec<BTreeSet<_>> = psbts
        .iter()
        .map(|psbt| {
            psbt.outputs
                .iter()
                .map(|output| output.unique_id().map(|id| id.as_bytes().to_vec()))
                .collect()
        })
        .collect();
    let input_union: BTreeSet<_> = input_sets.iter().flatten().cloned().collect();
    let output_union: BTreeSet<_> = output_sets.iter().flatten().cloned().collect();
    for (index, psbt) in psbts.iter().enumerate() {
        if psbt.global.tx_modifiable_flags & 0x01 == 0 && input_sets[index] != input_union {
            return Err(Error::new(
                "PSBTs are not joinable: input sets differ but inputs are not modifiable",
            ));
        }
        if psbt.global.tx_modifiable_flags & 0x02 == 0 && output_sets[index] != output_union {
            return Err(Error::new(
                "PSBTs are not joinable: output sets differ but outputs are not modifiable",
            ));
        }
    }

    // Clearing a modifiable bit is monotone (0x03 ≤ {0x01, 0x02} ≤ 0x00), so
    // the joined bits 0/1 are the bitwise AND. Has SIGHASH_SINGLE (bit 2) is
    // grow-only knowledge, so it ORs. The strict field lattice would report a
    // conflict on unequal flags, so the LUB is normalized into every operand
    // before wrapping.
    let joined_flags = psbts
        .iter()
        .map(|psbt| psbt.global.tx_modifiable_flags)
        .reduce(|left, right| (left & right & 0x03) | ((left | right) & 0x04))
        .unwrap_or(0);
    for psbt in &mut psbts {
        psbt.global.tx_modifiable_flags = joined_flags;
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
/// Set admissibility and flag normalization already happened in
/// [`join_psbts`]; this only performs the lattice join.
fn join_unordered(psbts: Vec<Psbt>) -> Result<Psbt> {
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
    Ok(constructor.into_psbt())
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
    fn modifiable_joins_unmodifiable_when_frozen_set_covers_the_union() {
        // A froze its sets (flags 0x00); B is still fully modifiable but only
        // holds a subset of A's elements, so the union IS A's frozen set: the
        // join succeeds and the LUB clears the modifiable bits.
        let frozen = psbt(
            0x00,
            true,
            vec![input(1), input(2)],
            vec![output(10, 1), output(20, 2)],
        );
        let subset = psbt(0x03, true, vec![input(1)], vec![output(10, 1)]);

        let joined = join_psbts([frozen, subset]).expect("subset joins into the frozen set");
        assert_eq!(joined.global.tx_modifiable_flags, 0x00);
        assert_eq!(joined.inputs.len(), 2);
        assert_eq!(joined.outputs.len(), 2);
    }

    #[test]
    fn modifiable_additions_outside_a_frozen_set_are_refused() {
        // B carries an input A's frozen set doesn't contain: the union would
        // grow past the frozen set, so the join must refuse.
        let frozen = psbt(0x00, true, vec![input(1)], vec![output(10, 1)]);
        let extra = psbt(
            0x03,
            true,
            vec![input(1), input(2)],
            vec![output(10, 1)],
        );

        let error = join_psbts([frozen, extra]).expect_err("frozen input set must not grow");
        assert!(
            error.to_string().contains("input sets differ"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn partially_modifiable_flags_join_to_their_lub() {
        // 0x01 (inputs modifiable) ⊔ 0x02 (outputs modifiable) = 0x00: each
        // dimension is frozen on one side, so with equal sets the join
        // succeeds and both bits clear.
        let left = psbt(0x01, true, vec![input(1)], vec![output(10, 1)]);
        let right = psbt(0x02, true, vec![input(1)], vec![output(10, 1)]);

        let joined = join_psbts([left, right]).expect("equal sets join across partial flags");
        assert_eq!(joined.global.tx_modifiable_flags, 0x00);
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
