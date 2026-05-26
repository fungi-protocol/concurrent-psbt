use psbt_v2::v2::{Output, Psbt};

use crate::sort::Sorter;
use crate::tx::UnorderedPsbt;

use super::Constructor;
use super::super::{ConstructorError, OutputsModifiable, validate_flags};

impl<S> Constructor<OutputsModifiable, S> {
    /// Parse a v2 PSBT into an outputs-modifiable constructor.
    ///
    /// # Errors
    /// Returns [`ConstructorError::FlagsMismatch`] if `tx_modifiable_flags`
    /// does not have bit 1 set and bit 0 clear, or propagates lower-level
    /// unordered PSBT parse errors.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, ConstructorError> {
        validate_flags(psbt.global.tx_modifiable_flags, 0x02)?;
        Ok(Constructor::from_unordered(UnorderedPsbt::try_from_psbt(
            psbt,
        )?))
    }

    /// Add an output to the PSBT.
    pub fn output(mut self, output: Output) -> Self {
        self.0.outputs.add(output);
        self
    }
}

impl<S> Constructor<OutputsModifiable, S> {
    /// Finalize outputs and transition to the [`Sorter`] for ordering.
    pub fn no_more_outputs(self) -> Sorter<S> {
        Sorter::from_unordered_psbt(self.0)
    }
}
