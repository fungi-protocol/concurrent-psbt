use core::marker::PhantomData;

use psbt_v2::v2::Constructor as Bip370Constructor;
use psbt_v2::v2::Creator as Bip370Creator;
use psbt_v2::v2::{InputsOnlyModifiable, Mod, Modifiable, OutputsOnlyModifiable, Psbt};

use crate::fields::{
    psbt_global_sort_deterministic, psbt_global_tx_unordered, psbt_out_unique_id,
    GlobalModifiableExt as _, UNORDERED_VALUE,
};
use crate::input::InputExt as _;
use crate::output::OutputExt as _;
use crate::tx::UnorderedPsbt;

use psbt_v2::v2::{Input, Output};

// FIXME use thiserror instead of implementing manually
/// Error returned when a PSBT is not suitable for an unordered Constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The `PSBT_GLOBAL_TX_UNORDERED` field is missing or has a wrong value.
    NotUnordered,
    /// The inputs-modifiable flag is not set.
    InputsNotModifiable,
    /// The outputs-modifiable flag is not set.
    OutputsNotModifiable,
    /// An output is missing the `PSBT_OUT_UNIQUE_ID` proprietary field.
    MissingOutputUniqueId,
    /// Joining the new input or output with the existing PSBT produced a conflict.
    JoinConflict,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotUnordered => f.write_str("PSBT is not marked unordered"),
            Error::InputsNotModifiable => f.write_str("inputs are not modifiable"),
            Error::OutputsNotModifiable => f.write_str("outputs are not modifiable"),
            Error::MissingOutputUniqueId => f.write_str("an output is missing PSBT_OUT_UNIQUE_ID"),
            Error::JoinConflict => {
                f.write_str("joining the new input or output produced a conflict")
            }
        }
    }
}

// FIXME use thiserror instead of implementing manually
/// Error returned when sorting an unordered Constructor into a fixed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortingError {
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is not set.
    MissingSortDeterministic,
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` has an unrecognized value.
    InvalidSortDeterministic,
    /// An input or output is missing its sort key.
    MissingSortKey, // TODO split into MissingSortKeyForInput(OutPoint) and
    // MissingSortKeyForOutput(unique id)
    /// Two inputs or two outputs share the same sort key.
    DuplicateSortKey, // TODO (OutPoint, OutPoint) or (unique id, unique id) poitning out which
                      // inputs/outputs collide
}

impl core::fmt::Display for SortingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SortingError::MissingSortDeterministic => {
                f.write_str("PSBT_GLOBAL_SORT_DETERMINISTIC is not set")
            }
            SortingError::InvalidSortDeterministic => {
                f.write_str("PSBT_GLOBAL_SORT_DETERMINISTIC has an unrecognized value")
            }
            SortingError::MissingSortKey => {
                f.write_str("an input or output is missing its sort key")
            }
            SortingError::DuplicateSortKey => {
                f.write_str("two inputs or two outputs share the same sort key")
            }
        }
    }
}

// -- Helpers -----------------------------------------------------------------

/// Extract sort keys from items via `take_key`, sort by key, return items in order.
///
/// Fails if any key is missing or if two items share the same sort key.
fn sort_by_extracted_key<T>(
    items: impl IntoIterator<Item = T>,
    mut take_key: impl FnMut(&mut T) -> Option<Vec<u8>>,
) -> Result<Vec<T>, SortingError> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<Vec<u8>, T> = BTreeMap::new();
    for mut item in items {
        let key = take_key(&mut item).ok_or(SortingError::MissingSortKey)?;
        if map.insert(key, item).is_some() {
            return Err(SortingError::DuplicateSortKey);
        }
    }
    Ok(map.into_values().collect())
}

// -- Validation --------------------------------------------------------------

/// Check that every output in a raw `Psbt` carries `PSBT_OUT_UNIQUE_ID`.
fn validate_output_unique_ids(psbt: &Psbt) -> Result<(), Error> {
    let key = psbt_out_unique_id();
    for output in &psbt.outputs {
        if !output.proprietaries.contains_key(&key) {
            return Err(Error::MissingOutputUniqueId);
        }
    }
    Ok(())
}

/// Check that a single output carries `PSBT_OUT_UNIQUE_ID`.
fn validate_output_unique_id(output: &Output) -> Result<(), Error> {
    if output.proprietaries.contains_key(&psbt_out_unique_id()) {
        Ok(())
    } else {
        Err(Error::MissingOutputUniqueId)
    }
}

// -- Constructor -------------------------------------------------------------

/// Unordered Constructor, mirrors the BIP 370 Constructor but for unordered PSBTs.
pub struct Constructor<M: Mod>(UnorderedPsbt, PhantomData<M>);

// Manual impl: derive would add an unnecessary `M: Debug` bound.
impl<M: Mod> core::fmt::Debug for Constructor<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Constructor").field(&self.0).finish()
    }
}

// Manual impl: derive would add an unnecessary `M: PartialEq` bound.
impl<M: Mod> PartialEq for Constructor<M> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

// TODO
// `DETERMINISTIC` modeled as Option<bool> const generic? is that possible? and
// whether or not a seed is set also makes sense to track in typestate
//
// the constructor should expose a JOIN operation and this should agree with the
// constraints, not just based on the value conflicts the global fields would
// generate, also represented in the type via generics so that JOIN is only defined
// for constructors with the same type parameters.
//
// try_join on an enum of these variants can also be defined which provides
// provides useful errors for the incompatible types
//
// this would facilitate ensuring invariants, such as all sort keys are defined
// for non-deterministically sorted transactions
//
// this would also enable cleaner APIs, for example a deterministic constructor
// could have a `set_seed()` and from that point behave as if the sort keys are
// set via typestate, or provide a finalize_order(self, seed) that sets it (with
// the typestate ensuring that it isn't set)
impl<M: Mod> Constructor<M> {
    /// Return the inner `UnorderedPsbt`.
    pub fn into_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Sort inputs and outputs by their explicit sort keys, producing an ordered BIP 370 `Psbt`.
    ///
    /// Requires `PSBT_GLOBAL_SORT_DETERMINISTIC` = `0x00` (explicit sort keys).
    /// Deterministic derivation (`0x01`) is not yet implemented.
    fn finalize_order_inner(self) -> Result<Psbt, SortingError> {
        let det_key = psbt_global_sort_deterministic();
        let det_value = self
            .0
            .global
            .proprietaries
            .get(&det_key)
            .ok_or(SortingError::MissingSortDeterministic)?;

        match det_value.as_slice() {
            [0x00] => {}
            [0x01] => todo!("deterministic not supported"),
            _ => return Err(SortingError::InvalidSortDeterministic),
        }

        // TODO when DETERMINISTIC=0x01, derive missing keys from seed instead of failing.
        let inputs = sort_by_extracted_key(self.0.inputs, |i| i.take_sort_key())?;
        let outputs = sort_by_extracted_key(self.0.outputs, |o| o.take_sort_key())?;

        let mut global = self.0.global;
        global.proprietaries.remove(&psbt_global_tx_unordered());

        Ok(Psbt {
            global,
            inputs,
            outputs,
        })
    }
}

// TODO enum of Constructor<Modifiable> etc constructible from a Psbt

impl Constructor<Modifiable> {
    /// Wrap an existing PSBT, validating it is unordered and fully modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        validate_output_unique_ids(&psbt)?;
        let unordered = UnorderedPsbt::from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_inputs_modifiable() {
            return Err(Error::InputsNotModifiable);
        }
        if !unordered.global.is_outputs_modifiable() {
            return Err(Error::OutputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Add an input.
    ///
    /// Returns `Err(Error::JoinConflict)` if the same outpoint is already
    /// present with conflicting field values.
    pub fn input(self, input: Input) -> Result<Self, Error> {
        // FIXME this should not clone self.0 but start with a new unordered
        // PSBT. UnorderedPsbt::from_output
        let mut singleton = self.0.clone();
        singleton.inputs = [input].into_iter().collect();
        singleton.outputs = Default::default();
        self.0
            .join(singleton) // FIXME define try_join, there is no join for UnorderedPsbt
            .map(|p| Constructor(p, PhantomData))
            .map_err(|_| Error::JoinConflict) // FIXME replace the inputs_count with a recalculated value, then try_unwrap()
    }

    /// Add an output. Requires `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Returns `Err(Error::JoinConflict)` if the same unique ID is already
    /// present with conflicting field values.
    pub fn output(self, output: Output) -> Result<Self, Error> {
        validate_output_unique_id(&output)?;
        let mut singleton = self.0.clone();
        singleton.inputs = Default::default();
        singleton.outputs = [output].into_iter().collect();
        self.0
            .join(singleton)
            .map(|p| Constructor(p, PhantomData))
            .map_err(|_| Error::JoinConflict)
    }

    /// Sort inputs/outputs and produce a BIP 370 `Constructor<Modifiable>`.
    pub fn finalize_order(self) -> Result<Bip370Constructor<Modifiable>, SortingError> {
        let psbt = self.finalize_order_inner()?;
        Ok(Bip370Constructor::<Modifiable>::new(psbt).expect("modifiable flags are preserved"))
    }

    /// Lock inputs: transition to `OutputsOnlyModifiable`.
    pub fn no_more_inputs(mut self) -> Constructor<OutputsOnlyModifiable> {
        self.0.global.clear_inputs_modifiable();
        Constructor(self.0, PhantomData)
    }

    /// Lock outputs: transition to `InputsOnlyModifiable`.
    pub fn no_more_outputs(mut self) -> Constructor<InputsOnlyModifiable> {
        self.0.global.clear_outputs_modifiable();
        Constructor(self.0, PhantomData)
    }
}

impl Constructor<InputsOnlyModifiable> {
    /// Add an input.
    ///
    /// Returns `Err(Error::JoinConflict)` if the same outpoint is already
    /// present with conflicting field values.
    // FIXME reduce duplication by only implementing this once the generic Constructor<M>,
    pub fn input(self, input: Input) -> Result<Self, Error> {
        let mut singleton = self.0.clone();
        singleton.inputs = [input].into_iter().collect();
        singleton.outputs = Default::default();
        self.0
            .join(singleton)
            .map(|p| Constructor(p, PhantomData))
            .map_err(|_| Error::JoinConflict)
    }

    /// Wrap an existing PSBT, validating it is unordered and inputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        validate_output_unique_ids(&psbt)?;
        let unordered = UnorderedPsbt::from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_inputs_modifiable() {
            return Err(Error::InputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Sort inputs/outputs and produce a BIP 370 `Constructor<InputsOnlyModifiable>`.
    pub fn finalize_order(self) -> Result<Bip370Constructor<InputsOnlyModifiable>, SortingError> {
        let psbt = self.finalize_order_inner()?;
        Ok(Bip370Constructor::<InputsOnlyModifiable>::new(psbt)
            .expect("inputs-modifiable flag is preserved"))
    }

    /// Lock inputs: both sides now locked, return the `UnorderedPsbt`.
    pub fn no_more_inputs(mut self) -> UnorderedPsbt {
        self.0.global.clear_inputs_modifiable();
        self.0
    }
}

impl Constructor<OutputsOnlyModifiable> {
    /// Add an output. Requires `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Returns `Err(Error::JoinConflict)` if the same unique ID is already
    /// present with conflicting field values.
    pub fn output(self, output: Output) -> Result<Self, Error> {
        validate_output_unique_id(&output)?;
        // FIXME this should not clone self.0 but start with a new unordered
        // PSBT. UnorderedPsbt::from_output
        let mut singleton = self.0.clone();
        singleton.inputs = Default::default();
        singleton.outputs = [output].into_iter().collect();
        self.0
            .join(singleton) // FIXME define try_join, there is no join for UnorderedPsbt
            .map(|p| Constructor(p, PhantomData))
            .map_err(|_| Error::JoinConflict) // FIXME replace the outputs_count with a recalculated value, then try_unwrap
    }

    /// Wrap an existing PSBT, validating it is unordered and outputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        validate_output_unique_ids(&psbt)?;
        let unordered = UnorderedPsbt::from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !unordered.global.is_outputs_modifiable() {
            return Err(Error::OutputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Sort inputs/outputs and produce a BIP 370 `Constructor<OutputsOnlyModifiable>`.
    pub fn finalize_order(self) -> Result<Bip370Constructor<OutputsOnlyModifiable>, SortingError> {
        let psbt = self.finalize_order_inner()?;
        Ok(Bip370Constructor::<OutputsOnlyModifiable>::new(psbt)
            .expect("outputs-modifiable flag is preserved"))
    }

    /// Lock outputs: both sides now locked, return the `UnorderedPsbt`.
    pub fn no_more_outputs(mut self) -> UnorderedPsbt {
        self.0.global.clear_outputs_modifiable();
        self.0
    }
}

// -- Creator -----------------------------------------------------------------

/// Creator for unordered PSBTs.
///
/// Sets the `PSBT_GLOBAL_TX_UNORDERED` proprietary field and both modifiable
/// flags, producing a PSBT ready for an unordered `Constructor`.
pub struct Creator(UnorderedPsbt);

impl Creator {
    pub fn new() -> Self {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();

        let mut unordered = UnorderedPsbt::from_psbt(psbt);

        unordered
            .global
            .proprietaries
            .insert(psbt_global_tx_unordered(), vec![UNORDERED_VALUE]);

        Creator(unordered)
    }

    /// Set `PSBT_GLOBAL_SORT_DETERMINISTIC`.
    ///
    /// - `None`        — leave the field unset (default)
    /// - `Some(false)` — `0x00`: explicit sort keys required
    /// - `Some(true)`  — `0x01`: sort keys derived deterministically from seed
    pub fn sort_deterministic(mut self, value: Option<bool>) -> Self {
        let key = psbt_global_sort_deterministic();
        match value {
            None => {
                self.0.global.proprietaries.remove(&key);
            }
            Some(v) => {
                self.0.global.proprietaries.insert(key, vec![v as u8]);
            }
        }
        self
    }

    /// Consume the creator and return the `UnorderedPsbt`.
    pub fn into_unordered_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Consume the creator and return a fully-modifiable Constructor.
    pub fn constructor(self) -> Constructor<Modifiable> {
        // Convert back to Psbt for Constructor::new validation path.
        Constructor::<Modifiable>::new(self.0.to_psbt())
            .expect("Creator always produces a valid unordered PSBT")
    }
}

impl Default for Creator {
    fn default() -> Self {
        Self::new()
    }
}

// -- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_sort_deterministic_none_does_not_set_field() {
        use crate::fields::psbt_global_sort_deterministic;
        let psbt = Creator::new()
            .sort_deterministic(None)
            .into_unordered_psbt();
        assert!(!psbt
            .global
            .proprietaries
            .contains_key(&psbt_global_sort_deterministic()));
    }

    #[test]
    fn creator_sort_deterministic_false_sets_field_to_0x00() {
        use crate::fields::psbt_global_sort_deterministic;
        let psbt = Creator::new()
            .sort_deterministic(Some(false))
            .into_unordered_psbt();
        assert_eq!(
            psbt.global
                .proprietaries
                .get(&psbt_global_sort_deterministic()),
            Some(&vec![0x00]),
        );
    }

    #[test]
    fn creator_sort_deterministic_true_sets_field_to_0x01() {
        use crate::fields::psbt_global_sort_deterministic;
        let psbt = Creator::new()
            .sort_deterministic(Some(true))
            .into_unordered_psbt();
        assert_eq!(
            psbt.global
                .proprietaries
                .get(&psbt_global_sort_deterministic()),
            Some(&vec![0x01]),
        );
    }

    #[test]
    fn creator_produces_valid_constructor() {
        let c = Creator::new().constructor();
        let unordered: UnorderedPsbt = c.into_psbt();
        assert!(unordered.is_unordered());
    }

    #[test]
    fn new_modifiable_rejects_non_unordered() {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();
        assert_eq!(
            Constructor::<Modifiable>::new(psbt),
            Err(Error::NotUnordered)
        );
    }

    #[test]
    fn new_modifiable_rejects_missing_inputs_flag() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_inputs_modifiable();
        assert_eq!(
            Constructor::<Modifiable>::new(psbt),
            Err(Error::InputsNotModifiable)
        );
    }

    #[test]
    fn new_modifiable_rejects_missing_outputs_flag() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();
        psbt.global.clear_outputs_modifiable();
        assert_eq!(
            Constructor::<Modifiable>::new(psbt),
            Err(Error::OutputsNotModifiable)
        );
    }

    #[test]
    fn no_more_inputs_then_no_more_outputs() {
        let c = Creator::new().constructor();
        let c = c.no_more_inputs(); // Modifiable → OutputsOnlyModifiable
        let unordered = c.no_more_outputs(); // OutputsOnlyModifiable → UnorderedPsbt
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn no_more_outputs_then_no_more_inputs() {
        let c = Creator::new().constructor();
        let c = c.no_more_outputs(); // Modifiable → InputsOnlyModifiable
        let unordered = c.no_more_inputs(); // InputsOnlyModifiable → UnorderedPsbt
        assert!(!unordered.global.is_inputs_modifiable());
        assert!(!unordered.global.is_outputs_modifiable());
    }

    #[test]
    fn finalize_order_sorts_by_explicit_sort_keys() {
        use crate::fields::{
            psbt_global_sort_deterministic, psbt_in_sort_key, psbt_out_sort_key, psbt_out_unique_id,
        };

        let mut creator = Creator::new();
        creator
            .0
            .global
            .proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x00]);

        let mut psbt = creator.into_unordered_psbt().to_psbt();

        // Two inputs: sort key 0x02 first, 0x01 second (reverse order).
        let mut oa = bitcoin::OutPoint::null();
        oa.vout = 0;
        let mut ob = bitcoin::OutPoint::null();
        ob.vout = 1;

        let mut input_b = psbt_v2::v2::Input::new(&ob);
        input_b.proprietaries.insert(psbt_in_sort_key(), vec![0x02]);

        let mut input_a = psbt_v2::v2::Input::new(&oa);
        input_a.proprietaries.insert(psbt_in_sort_key(), vec![0x01]);

        psbt.inputs = vec![input_b, input_a];
        psbt.global.input_count = 2;

        // Two outputs: sort key 0x02 first, 0x01 second (reverse order).
        let mut output_y = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(2000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xBB]),
        });
        output_y
            .proprietaries
            .insert(psbt_out_sort_key(), vec![0x02]);
        output_y
            .proprietaries
            .insert(psbt_out_unique_id(), vec![0x02; 16]);

        let mut output_x = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xAA]),
        });
        output_x
            .proprietaries
            .insert(psbt_out_sort_key(), vec![0x01]);
        output_x
            .proprietaries
            .insert(psbt_out_unique_id(), vec![0x01; 16]);

        psbt.outputs = vec![output_y, output_x];
        psbt.global.output_count = 2;

        let constructor = Constructor::<Modifiable>::new(psbt).unwrap();
        let bip370 = constructor.finalize_order().unwrap();
        let ordered = bip370.psbt().unwrap();

        // After sorting: sort_key 0x01 (vout=0) before 0x02 (vout=1).
        assert_eq!(ordered.inputs[0].spent_output_index, 0);
        assert_eq!(ordered.inputs[1].spent_output_index, 1);

        // After sorting: sort_key 0x01 (1000 sat) before 0x02 (2000 sat).
        assert_eq!(ordered.outputs[0].amount, bitcoin::Amount::from_sat(1000));
        assert_eq!(ordered.outputs[1].amount, bitcoin::Amount::from_sat(2000));

        // Sort keys are scrubbed from the ordered PSBT.
        use crate::input::InputExt as _;
        use crate::output::OutputExt as _;
        assert!(ordered.inputs.iter().all(|i| i.sort_key().is_none()));
        assert!(ordered.outputs.iter().all(|o| o.sort_key().is_none()));
    }

    #[test]
    fn finalize_order_produces_valid_updater() {
        use crate::fields::psbt_global_sort_deterministic;

        let mut creator = Creator::new();
        creator
            .0
            .global
            .proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x00]);

        let constructor = creator.constructor();
        let bip370 = constructor.finalize_order().unwrap();
        // Proceed through the BIP 370 pipeline to Updater.
        let _updater = bip370.updater().unwrap();
    }

    #[test]
    fn finalize_order_rejects_missing_sort_keys() {
        use crate::fields::psbt_global_sort_deterministic;

        let mut creator = Creator::new();
        creator
            .0
            .global
            .proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x00]);

        let mut psbt = creator.into_unordered_psbt().to_psbt();

        // Input without a sort key.
        let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        psbt.inputs = vec![input];
        psbt.global.input_count = 1;

        let constructor = Constructor::<Modifiable>::new(psbt).unwrap();
        assert!(matches!(
            constructor.finalize_order(),
            Err(SortingError::MissingSortKey)
        ));
    }

    #[test]
    fn finalize_order_rejects_missing_deterministic_field() {
        let psbt = Creator::new().into_unordered_psbt().to_psbt();
        let constructor = Constructor::<Modifiable>::new(psbt).unwrap();
        assert!(matches!(
            constructor.finalize_order(),
            Err(SortingError::MissingSortDeterministic)
        ));
    }

    #[test]
    fn finalize_order_rejects_invalid_deterministic_value() {
        use crate::fields::psbt_global_sort_deterministic;

        let mut creator = Creator::new();
        creator
            .0
            .global
            .proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0xFF]);
        let psbt = creator.into_unordered_psbt().to_psbt();
        let constructor = Constructor::<Modifiable>::new(psbt).unwrap();
        assert!(matches!(
            constructor.finalize_order(),
            Err(SortingError::InvalidSortDeterministic)
        ));
    }

    #[test]
    fn finalize_order_rejects_duplicate_input_sort_keys() {
        use crate::fields::{psbt_global_sort_deterministic, psbt_in_sort_key};

        let mut creator = Creator::new();
        creator
            .0
            .global
            .proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x00]);
        let mut psbt = creator.into_unordered_psbt().to_psbt();

        let mut input_a = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        input_a.proprietaries.insert(psbt_in_sort_key(), vec![0x01]);

        let mut ob = bitcoin::OutPoint::null();
        ob.vout = 1;
        let mut input_b = psbt_v2::v2::Input::new(&ob);
        input_b.proprietaries.insert(psbt_in_sort_key(), vec![0x01]); // same key

        psbt.inputs = vec![input_a, input_b];
        psbt.global.input_count = 2;

        let constructor = Constructor::<Modifiable>::new(psbt).unwrap();
        assert!(matches!(
            constructor.finalize_order(),
            Err(SortingError::DuplicateSortKey)
        ));
    }

    #[test]
    fn new_modifiable_rejects_missing_output_unique_id() {
        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();

        // Add an output without PSBT_OUT_UNIQUE_ID.
        let output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xAA]),
        });
        psbt.outputs = vec![output];
        psbt.global.output_count = 1;

        assert_eq!(
            Constructor::<Modifiable>::new(psbt),
            Err(Error::MissingOutputUniqueId),
        );
    }

    #[test]
    fn new_modifiable_accepts_output_with_unique_id() {
        use crate::fields::psbt_out_unique_id;

        let mut psbt = Creator::new().into_unordered_psbt().to_psbt();

        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::from_bytes(vec![0xAA]),
        });
        output
            .proprietaries
            .insert(psbt_out_unique_id(), vec![0x01; 16]);
        psbt.outputs = vec![output];
        psbt.global.output_count = 1;

        assert!(Constructor::<Modifiable>::new(psbt).is_ok());
    }

    #[test]
    fn inputs_only_new_rejects_locked_inputs() {
        let c = Creator::new().constructor();
        let unordered = c.no_more_inputs().no_more_outputs();
        assert_eq!(
            Constructor::<InputsOnlyModifiable>::new(unordered.to_psbt()),
            Err(Error::InputsNotModifiable)
        );
    }

    #[test]
    fn outputs_only_new_rejects_locked_outputs() {
        let c = Creator::new().constructor();
        let unordered = c.no_more_outputs().no_more_inputs();
        assert_eq!(
            Constructor::<OutputsOnlyModifiable>::new(unordered.to_psbt()),
            Err(Error::OutputsNotModifiable)
        );
    }

    #[test]
    fn input_count_is_correct_after_adding_inputs() {
        let c = Creator::new().constructor();
        let mut op1 = bitcoin::OutPoint::null();
        op1.vout = 0;
        let mut op2 = bitcoin::OutPoint::null();
        op2.vout = 1;
        let c = c
            .input(psbt_v2::v2::Input::new(&op1))
            .unwrap()
            .input(psbt_v2::v2::Input::new(&op2))
            .unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.global.input_count, 2);
        assert_eq!(psbt.inputs.len(), 2);
    }

    #[test]
    fn output_count_is_correct_after_adding_outputs() {
        use crate::fields::psbt_out_unique_id;

        let c = Creator::new().constructor();
        let mut o1 = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        o1.proprietaries
            .insert(psbt_out_unique_id(), vec![0x01; 16]);
        let mut o2 = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(2000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        o2.proprietaries
            .insert(psbt_out_unique_id(), vec![0x02; 16]);
        let c = c.output(o1).unwrap().output(o2).unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.global.output_count, 2);
        assert_eq!(psbt.outputs.len(), 2);
    }

    #[test]
    fn input_join_conflict_returns_error() {
        // Same outpoint, different sequence → conflict.
        let c = Creator::new().constructor();
        let op = bitcoin::OutPoint::null();
        let mut input_a = psbt_v2::v2::Input::new(&op);
        input_a.sequence = Some(bitcoin::Sequence::MAX);
        let mut input_b = psbt_v2::v2::Input::new(&op);
        input_b.sequence = Some(bitcoin::Sequence::ENABLE_LOCKTIME_NO_RBF);
        let c = c.input(input_a).unwrap();
        assert_eq!(c.input(input_b), Err(Error::JoinConflict));
    }

    #[test]
    fn modifiable_input_adds_to_set() {
        let c = Creator::new().constructor();
        let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        let c = c.input(input).unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.inputs.len(), 1);
    }

    #[test]
    fn modifiable_output_adds_to_set() {
        use crate::fields::psbt_out_unique_id;

        let c = Creator::new().constructor();
        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output
            .proprietaries
            .insert(psbt_out_unique_id(), vec![0xAA; 16]);
        let c = c.output(output).unwrap();
        let psbt = c.into_psbt();
        assert_eq!(psbt.outputs.len(), 1);
    }

    #[test]
    fn modifiable_output_rejects_missing_unique_id() {
        let c = Creator::new().constructor();
        let output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        assert_eq!(c.output(output), Err(Error::MissingOutputUniqueId));
    }

    #[test]
    fn inputs_only_input_adds_to_set() {
        let c = Creator::new().constructor().no_more_outputs();
        let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        let c = c.input(input).unwrap();
        let psbt = c.no_more_inputs();
        assert_eq!(psbt.inputs.len(), 1);
    }

    #[test]
    fn outputs_only_output_adds_to_set() {
        use crate::fields::psbt_out_unique_id;

        let c = Creator::new().constructor().no_more_inputs();
        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output
            .proprietaries
            .insert(psbt_out_unique_id(), vec![0xBB; 16]);
        let c = c.output(output).unwrap();
        let psbt = c.no_more_outputs();
        assert_eq!(psbt.outputs.len(), 1);
    }

    #[test]
    fn constructor_eq_reflexive() {
        let a = Creator::new().constructor();
        let b = Creator::new().constructor();
        assert_eq!(a, b);
    }

    #[test]
    fn constructor_eq_differs_after_input() {
        let a = Creator::new().constructor();
        let b = Creator::new()
            .constructor()
            .input(psbt_v2::v2::Input::new(&bitcoin::OutPoint::null()))
            .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn finalize_order_inputs_only() {
        use crate::fields::{psbt_global_sort_deterministic, psbt_in_sort_key};

        let mut creator = Creator::new();
        creator
            .0
            .global
            .proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x00]);

        let mut psbt = creator.into_unordered_psbt().to_psbt();

        let mut input = psbt_v2::v2::Input::new(&bitcoin::OutPoint::null());
        input.proprietaries.insert(psbt_in_sort_key(), vec![0x01]);
        psbt.inputs = vec![input];
        psbt.global.input_count = 1;
        psbt.global.clear_outputs_modifiable();

        let c = Constructor::<InputsOnlyModifiable>::new(psbt).unwrap();
        let ordered = c.finalize_order().unwrap().psbt().unwrap();
        assert_eq!(ordered.inputs.len(), 1);
        assert_eq!(
            ordered.inputs[0].previous_txid,
            bitcoin::OutPoint::null().txid
        );
    }

    #[test]
    fn finalize_order_outputs_only() {
        use crate::fields::{
            psbt_global_sort_deterministic, psbt_out_sort_key, psbt_out_unique_id,
        };

        let mut creator = Creator::new();
        creator
            .0
            .global
            .proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x00]);

        let mut psbt = creator.into_unordered_psbt().to_psbt();

        let mut output = psbt_v2::v2::Output::new(bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(1000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        });
        output.proprietaries.insert(psbt_out_sort_key(), vec![0x01]);
        output
            .proprietaries
            .insert(psbt_out_unique_id(), vec![0x01; 16]);
        psbt.outputs = vec![output];
        psbt.global.output_count = 1;
        psbt.global.clear_inputs_modifiable();

        let c = Constructor::<OutputsOnlyModifiable>::new(psbt).unwrap();
        let ordered = c.finalize_order().unwrap().psbt().unwrap();
        assert_eq!(ordered.outputs.len(), 1);
        assert_eq!(ordered.outputs[0].amount, bitcoin::Amount::from_sat(1000));
    }
}
