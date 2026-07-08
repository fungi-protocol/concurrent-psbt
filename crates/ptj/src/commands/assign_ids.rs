//! Assign spec identity fields to inputs/outputs that lack them.
//!
//! Imported BIP 174/370 data authored elsewhere carries no
//! `PSBT_OUT_UNIQUE_ID` (and no optional `PSBT_IN_UNIQUE_ID` suffix), so the
//! unordered constructor refuses it and there is no practical way to add the
//! ids by hand (raw proprietary-field editing). `assign-ids` is the explicit
//! operation:
//!
//! - auto (the default with no `--id`): every output missing a
//!   `PSBT_OUT_UNIQUE_ID` gets a fresh random 16-byte id
//!   ([`UniqueId::generate`]); existing ids are preserved, so the operation
//!   is idempotent and errors on nothing. Inputs are never auto-assigned:
//!   the spec identifies inputs by outpoint, and `PSBT_IN_UNIQUE_ID` is an
//!   optional suffix for the removal extension (e.g. interactive-tx
//!   `serial_id`), meaningful only when the caller chooses it.
//! - manual (`--id out:<index>=<bytes>` / `--id in:<index>=<bytes>`,
//!   repeatable): sets a caller-chosen id on one entry. An existing equal id
//!   is accepted idempotently; a differing one errors unless `--overwrite`.
//!   `--auto` combines both: manual directives first, then auto-fill.

use concurrent_psbt::output::{OutputUniqueIdExt, UniqueId};
use concurrent_psbt::removal::InputUniqueIdExt;
use psbt_v2::v2::Psbt;

use crate::cli::{AssignIdsConfig, IdAssignment, IdTarget};
use crate::{Error, Result, io};

pub(super) fn run(config: AssignIdsConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    let psbt = io::read_psbt_source(&config.file, stdin)?;
    let auto = config.auto || config.ids.is_empty();
    assign_ids_psbt(psbt, &config.ids, auto, config.overwrite)
}

pub(crate) fn assign_ids_psbt(
    mut psbt: Psbt,
    ids: &[IdAssignment],
    auto: bool,
    overwrite: bool,
) -> Result<Psbt> {
    for assignment in ids {
        apply_assignment(&mut psbt, assignment, overwrite)?;
    }

    if auto {
        for output in &mut psbt.outputs {
            if !output.has_unique_id() {
                output.set_unique_id(UniqueId::generate());
            }
        }
    }

    reject_manual_output_collisions(&psbt, ids)?;
    Ok(psbt)
}

fn apply_assignment(psbt: &mut Psbt, assignment: &IdAssignment, overwrite: bool) -> Result<()> {
    match assignment.target {
        IdTarget::Input => {
            let count = psbt.inputs.len();
            let input = psbt.inputs.get_mut(assignment.index).ok_or_else(|| {
                Error::new(format!(
                    "--id in:{}: input index out of range ({count} input{})",
                    assignment.index,
                    if count == 1 { "" } else { "s" },
                ))
            })?;
            match InputUniqueIdExt::unique_id(input) {
                Some(existing) if existing == assignment.id => {}
                Some(_) if !overwrite => {
                    return Err(Error::new(format!(
                        "--id in:{}: input already has a different PSBT_IN_UNIQUE_ID; \
                         pass --overwrite to replace it",
                        assignment.index,
                    )));
                }
                _ => input.set_unique_id(assignment.id.clone()),
            }
        }
        IdTarget::Output => {
            let count = psbt.outputs.len();
            let output = psbt.outputs.get_mut(assignment.index).ok_or_else(|| {
                Error::new(format!(
                    "--id out:{}: output index out of range ({count} output{})",
                    assignment.index,
                    if count == 1 { "" } else { "s" },
                ))
            })?;
            match output.unique_id() {
                Some(existing) if existing.as_bytes() == assignment.id => {}
                Some(_) if !overwrite => {
                    return Err(Error::new(format!(
                        "--id out:{}: output already has a different PSBT_OUT_UNIQUE_ID; \
                         pass --overwrite to replace it",
                        assignment.index,
                    )));
                }
                _ => output.set_unique_id(UniqueId::new(assignment.id.clone())),
            }
        }
    }
    Ok(())
}

/// A manually assigned output id colliding with any other output's id defeats
/// the entire point of `PSBT_OUT_UNIQUE_ID` (universal uniqueness), so it is
/// always an error — `--overwrite` does not sanction it. Pre-existing
/// collisions the caller did not touch are passed through untouched (this
/// operation errors on nothing it was not asked to do).
fn reject_manual_output_collisions(psbt: &Psbt, ids: &[IdAssignment]) -> Result<()> {
    for assignment in ids {
        if assignment.target != IdTarget::Output {
            continue;
        }
        for (index, output) in psbt.outputs.iter().enumerate() {
            if index == assignment.index {
                continue;
            }
            if output.unique_id().map(UniqueId::into_bytes) == Some(assignment.id.clone()) {
                return Err(Error::new(format!(
                    "--id out:{}: the id is already used by output {index}; \
                     PSBT_OUT_UNIQUE_ID must be unique",
                    assignment.index,
                )));
            }
        }
    }
    Ok(())
}
