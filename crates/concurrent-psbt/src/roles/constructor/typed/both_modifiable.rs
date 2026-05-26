use std::marker::PhantomData;

use psbt_v2::v2::{Input, Output, Psbt};

use crate::output::{OutputUniqueIdExt, UniqueId};
use crate::sorter::Unset;
use crate::tx::UnorderedPsbt;

use super::super::{
    BothModifiable, ConstructorError, InputsModifiable, OutputsModifiable, validate_flags,
};
use super::Constructor;

impl Constructor<BothModifiable, Unset> {
    /// Parse a v2 PSBT into a both-modifiable constructor.
    ///
    /// # Errors
    /// Returns [`ConstructorError::FlagsMismatch`] if bits 0 and 1 of
    /// `tx_modifiable_flags` are not both set, or propagates lower-level
    /// unordered PSBT parse errors.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, ConstructorError> {
        validate_flags(psbt.global.tx_modifiable_flags, 0x03)?;
        Ok(Constructor::from_unordered(UnorderedPsbt::try_from_psbt(
            psbt,
        )?))
    }
}

impl<S> Constructor<BothModifiable, S> {
    /// Add an input to the PSBT.
    pub fn input(mut self, input: Input) -> Self {
        self.0.inputs.add(input);
        self
    }

    /// Add an output to the PSBT.
    pub fn output(mut self, output: Output) -> Self {
        self.0.outputs.add(output);
        self
    }

    /// Add an output, stamping a freshly generated [`UniqueId`].
    ///
    /// Each call generates a new random `PSBT_OUT_UNIQUE_ID`, so adding
    /// copies of the same txout yields distinct outputs.
    pub fn output_with_new_uid(self, mut output: Output) -> Self {
        output.set_unique_id(UniqueId::generate());
        self.output(output)
    }

    /// Transition to [`OutputsModifiable`]: no more inputs will be added.
    ///
    /// Clears bit 0 of `tx_modifiable_flags`.
    pub fn no_more_inputs(mut self) -> Constructor<OutputsModifiable, S> {
        self.0.global.tx_modifiable_flags &= !0x01;
        Constructor(self.0, PhantomData)
    }

    /// Transition to [`InputsModifiable`]: no more outputs will be added.
    ///
    /// Clears bit 1 of `tx_modifiable_flags`.
    pub fn no_more_outputs(mut self) -> Constructor<InputsModifiable, S> {
        self.0.global.tx_modifiable_flags &= !0x02;
        Constructor(self.0, PhantomData)
    }
}
