//! Error types for the unordered Constructor role.

/// Error returned when a PSBT is not suitable for an unordered Constructor.
// PartialEq is manual: JoinConflict compares equal regardless of payload.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The `PSBT_GLOBAL_TX_UNORDERED` field is missing or has a wrong value.
    #[error("PSBT is not marked unordered")]
    NotUnordered,

    /// The inputs-modifiable flag is not set.
    #[error("inputs are not modifiable")]
    InputsNotModifiable,

    /// The outputs-modifiable flag is not set.
    #[error("outputs are not modifiable")]
    OutputsNotModifiable,

    /// An output is missing the `PSBT_OUT_UNIQUE_ID` proprietary field.
    #[error("an output is missing PSBT_OUT_UNIQUE_ID")]
    MissingOutputUniqueId,

    /// Joining the new input or output with the existing PSBT produced a conflict.
    #[error("joining the new input or output produced a conflict")]
    JoinConflict(crate::tx::ResultUnorderedPsbt),

    /// Neither the inputs-modifiable nor the outputs-modifiable flag is set.
    #[error("neither inputs-modifiable nor outputs-modifiable flag is set")]
    NeitherModifiable,

    /// A locked (non-modifiable) set contained items not present in the other side.
    #[error("a locked set contained items not present in the other constructor")]
    LockedSetMismatch,
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Error::NotUnordered, Error::NotUnordered)
                | (Error::InputsNotModifiable, Error::InputsNotModifiable)
                | (Error::OutputsNotModifiable, Error::OutputsNotModifiable)
                | (Error::MissingOutputUniqueId, Error::MissingOutputUniqueId)
                | (Error::JoinConflict(_), Error::JoinConflict(_))
                | (Error::NeitherModifiable, Error::NeitherModifiable)
                | (Error::LockedSetMismatch, Error::LockedSetMismatch)
        )
    }
}

impl Eq for Error {}

/// Error returned when sorting an unordered Constructor into a fixed order.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SortingError {
    /// An input or output is missing its sort key.
    // TODO split into MissingSortKeyForInput(OutPoint) and MissingSortKeyForOutput(unique id)
    #[error("an input or output is missing its sort key")]
    MissingSortKey,
    /// Two inputs or two outputs share the same sort key.
    // TODO (OutPoint, OutPoint) or (unique id, unique id) pointing out which collide
    #[error("two inputs or two outputs share the same sort key")]
    DuplicateSortKey,
}
