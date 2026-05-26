//! An [`UnorderedPsbt`] represents the shared state of a transaction under construction.

#![allow(clippy::result_large_err)]

use std::fmt;

use bitcoin::{EcdsaSighashType, OutPoint};
use psbt_v2::v2::Psbt;

use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};
use crate::{Conflicts, Lift, ResultWrapped};

use super::conflicts::PsbtConflict;
use super::global::{Global, InvalidGlobal, InvalidTxUuid, ResultGlobal, SortKeyDerivation};
use super::input::{DuplicateInput, Input, InputSet, ResultInputSet, SortKey, out_point};
use super::output::{DuplicateOutput, InvalidOutput, Output, OutputSet, ResultOutputSet, UniqueId};

/// An unordered PSBT for concurrent transaction construction.
///
/// Under the Concurrent PSBT Construction draft, participants can add inputs
/// and outputs to separate copies of the same PSBT and combine their
/// contributions before agreeing on input and output positions. Each input
/// is identified by the outpoint it spends. Each output has a [`UniqueId`],
/// so two payments with the same amount and script can remain distinct.
#[derive(Debug, Clone, PartialEq)]
pub struct UnorderedPsbt {
    pub(crate) global: Global,
    pub(crate) inputs: InputSet,
    pub(crate) outputs: OutputSet,
}

/// Accumulates contributions to the same [`UnorderedPsbt`] with [`Join::join`].
///
/// Composed with `try_unwrap`, `join` implements the strict combiner role defined
/// in the concurrent PSBT spec.
///
/// The join combines global fields, matches inputs by outpoint, and matches
/// outputs by unique ID. A contribution that adds a member to a frozen set
/// creates a membership conflict. Use [`Self::conflicts`] to inspect field
/// and membership conflicts and [`Self::try_unwrap`] to recover the merged PSBT.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultUnorderedPsbt {
    pub(crate) global: ResultGlobal,
    pub(crate) inputs: ResultInputSet,
    pub(crate) outputs: ResultOutputSet,
}

/// Error returned when a PSBT cannot be read as an [`UnorderedPsbt`].
#[derive(Debug, Clone, PartialEq)]
pub enum UnorderedPsbtError {
    /// `PSBT_GLOBAL_INPUT_COUNT` is not the number of inputs.
    InputCountMismatch { declared: usize, found: usize },
    /// `PSBT_GLOBAL_OUTPUT_COUNT` is not the number of outputs.
    OutputCountMismatch { declared: usize, found: usize },
    /// `PSBT_GLOBAL_TX_UNORDERED` is present but its value is not `0x03`.
    InvalidUnorderedFlag(Vec<u8>),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is present but its value is neither
    /// `0x00` nor `0x01`.
    InvalidSortKeyDerivation(Vec<u8>),
    /// The length of `PSBT_GLOBAL_TX_UUID` is outside the allowed range.
    InvalidTxUuid(InvalidTxUuid),
    /// The input spending this outpoint has a `PSBT_IN_SORT_KEY` longer than
    /// [`SortKey::MAX_LEN`].
    InputSortKeyTooLong(OutPoint),
    /// The output at this position is missing the `PSBT_OUT_UNIQUE_ID`
    /// proprietary field.
    MissingOutputUniqueId(usize),
    /// The output at this position has a `PSBT_OUT_UNIQUE_ID` outside the
    /// [`UniqueId::MIN_LEN`] to [`UniqueId::MAX_LEN`] byte range.
    InvalidOutputUniqueId(usize),
    /// The output at this position has a `PSBT_OUT_SORT_KEY` longer than
    /// [`SortKey::MAX_LEN`].
    OutputSortKeyTooLong(usize),
    /// Two inputs spend the same outpoint, so the transaction is invalid.
    DuplicateInput(OutPoint),
    /// Two outputs carry the same `PSBT_OUT_UNIQUE_ID`.
    DuplicateOutputUniqueId(UniqueId),
    /// `PSBT_GLOBAL_TX_UNORDERED` is absent, so the PSBT is ordered. Reading
    /// it as unordered would discard its order.
    NotUnordered,
    /// The PSBT is unordered but `tx_modifiable_flags` has the
    /// `Has SIGHASH_SINGLE` bit set.
    UnorderedSighashSingle,
    /// The input spending this outpoint requests a sighash other than
    /// `SIGHASH_ALL`, including Taproot `DEFAULT` or a nonstandard value.
    UnorderedInputSighashNotAll(OutPoint),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is `0x01`, so every sort key is
    /// derived, but the input spending this outpoint has a `PSBT_IN_SORT_KEY`.
    UnexpectedInputSortKey(OutPoint),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is `0x01`, so every sort key is
    /// derived, but the output with this unique ID has a `PSBT_OUT_SORT_KEY`.
    UnexpectedOutputSortKey(UniqueId),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is `0x00`, so every explicit sort key
    /// must be non-empty, but the input spending this outpoint has an empty
    /// `PSBT_IN_SORT_KEY`.
    EmptyInputSortKey(OutPoint),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is `0x00`, so every explicit sort key
    /// must be non-empty, but the output with this unique ID has an empty
    /// `PSBT_OUT_SORT_KEY`.
    EmptyOutputSortKey(UniqueId),
}

impl From<DuplicateInput> for UnorderedPsbtError {
    fn from(DuplicateInput(outpoint): DuplicateInput) -> Self {
        Self::DuplicateInput(outpoint)
    }
}

impl From<DuplicateOutput> for UnorderedPsbtError {
    fn from(DuplicateOutput(unique_id): DuplicateOutput) -> Self {
        Self::DuplicateOutputUniqueId(unique_id)
    }
}

impl From<InvalidGlobal> for UnorderedPsbtError {
    fn from(error: InvalidGlobal) -> Self {
        match error {
            InvalidGlobal::UnorderedFlag(value) => Self::InvalidUnorderedFlag(value),
            InvalidGlobal::SortKeyDerivation(value) => Self::InvalidSortKeyDerivation(value),
            InvalidGlobal::TxUuid(error) => Self::InvalidTxUuid(error),
        }
    }
}

impl UnorderedPsbtError {
    /// The error for the output at `index`, which could not be converted.
    fn invalid_output(index: usize, error: InvalidOutput) -> Self {
        match error {
            InvalidOutput::MissingUniqueId => Self::MissingOutputUniqueId(index),
            InvalidOutput::InvalidUniqueId(_) => Self::InvalidOutputUniqueId(index),
            InvalidOutput::SortKeyTooLong(_) => Self::OutputSortKeyTooLong(index),
        }
    }
}

impl fmt::Display for UnorderedPsbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputCountMismatch { declared, found } => {
                write!(f, "declares {declared} inputs but has {found}")
            }
            Self::OutputCountMismatch { declared, found } => {
                write!(f, "declares {declared} outputs but has {found}")
            }
            Self::InvalidUnorderedFlag(value) => {
                write!(f, "PSBT_GLOBAL_TX_UNORDERED is {value:02x?}, not 0x03")
            }
            Self::InvalidSortKeyDerivation(value) => write!(
                f,
                "PSBT_GLOBAL_SORT_KEY_DERIVATION is {value:02x?}, not 0x00 or 0x01"
            ),
            Self::InvalidTxUuid(error) => error.fmt(f),
            Self::InputSortKeyTooLong(outpoint) => write!(
                f,
                "sort key of the input spending {outpoint} is longer than {} bytes",
                SortKey::MAX_LEN
            ),
            Self::MissingOutputUniqueId(index) => {
                write!(f, "output {index} missing PSBT_OUT_UNIQUE_ID")
            }
            Self::InvalidOutputUniqueId(index) => write!(
                f,
                "PSBT_OUT_UNIQUE_ID of output {index} must be {} to {} bytes",
                UniqueId::MIN_LEN,
                UniqueId::MAX_LEN
            ),
            Self::OutputSortKeyTooLong(index) => write!(
                f,
                "sort key of output {index} is longer than {} bytes",
                SortKey::MAX_LEN
            ),
            Self::DuplicateInput(outpoint) => write!(f, "two inputs spend {outpoint}"),
            Self::DuplicateOutputUniqueId(_) => {
                write!(f, "two outputs share a PSBT_OUT_UNIQUE_ID")
            }
            Self::NotUnordered => write!(f, "PSBT is not marked unordered"),
            Self::UnorderedSighashSingle => {
                write!(f, "unordered PSBT has the SIGHASH_SINGLE flag set")
            }
            Self::UnorderedInputSighashNotAll(outpoint) => {
                write!(
                    f,
                    "the input spending {outpoint} requests a sighash other than SIGHASH_ALL"
                )
            }
            Self::UnexpectedInputSortKey(outpoint) => write!(
                f,
                "the input spending {outpoint} has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01"
            ),
            Self::UnexpectedOutputSortKey(unique_id) => write!(
                f,
                "the output with unique ID {:02x?} has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01",
                unique_id.as_bytes()
            ),
            Self::EmptyInputSortKey(outpoint) => write!(
                f,
                "the input spending {outpoint} has an empty sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00"
            ),
            Self::EmptyOutputSortKey(unique_id) => write!(
                f,
                "the output with unique ID {:02x?} has an empty sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00",
                unique_id.as_bytes()
            ),
        }
    }
}

impl std::error::Error for UnorderedPsbtError {}

/// `tx_modifiable_flags` bit that allows inputs to be added (BIP 370).
const INPUTS_MODIFIABLE: u8 = 0x01;
/// `tx_modifiable_flags` bit that allows outputs to be added (BIP 370).
const OUTPUTS_MODIFIABLE: u8 = 0x02;
/// `tx_modifiable_flags` bit indicating that the transaction has a
/// `SIGHASH_SINGLE` signature (BIP 370).
const HAS_SIGHASH_SINGLE: u8 = 0x04;

pub(crate) fn check_input_sighash(input: &Input) -> Result<(), UnorderedPsbtError> {
    if input
        .sighash_type()
        .is_some_and(|sighash| sighash.to_u32() != EcdsaSighashType::All.to_u32())
    {
        return Err(UnorderedPsbtError::UnorderedInputSighashNotAll(out_point(
            input,
        )));
    }
    Ok(())
}

/// Check the global fields that rule out an unordered PSBT: the unordered
/// marker must be present and the `Has SIGHASH_SINGLE` bit must be clear.
fn check_unordered(global: &Global) -> Result<(), UnorderedPsbtError> {
    if !global.is_unordered() {
        return Err(UnorderedPsbtError::NotUnordered);
    }
    if global.tx_modifiable_flags() & HAS_SIGHASH_SINGLE != 0 {
        return Err(UnorderedPsbtError::UnorderedSighashSingle);
    }
    Ok(())
}

/// The values a field may hold: its value, or every value of its conflict.
fn values<V: PartialJoin>(field: &Option<JoinResult<V>>) -> std::slice::Iter<'_, V> {
    match field {
        None => [].iter(),
        Some(Ok(value)) => std::slice::from_ref(value).iter(),
        Some(Err(conflict)) => conflict.into_iter(),
    }
}

/// Make the field a conflict holding its values, which it stays under every
/// later join. This is the join with the empty conflict, so marking a field
/// twice is marking it once.
fn mark<V: PartialJoin>(field: &mut Option<JoinResult<V>>) {
    *field = field
        .take()
        .map(|result| result.join(Err(Conflict::from_values([]))));
}

/// Mark every sort key if `derived`, since then no key may be explicit, and
/// every key that may be empty if `explicit`, since then every key must be
/// non-empty. Return `true` if a key was marked.
fn mark_sort_keys<'a>(
    keys: impl Iterator<Item = &'a mut Option<JoinResult<SortKey>>>,
    derived: bool,
    explicit: bool,
) -> bool {
    let mut marked = false;
    for key in keys {
        let empty = || values(key).any(|key| key.as_bytes().is_empty());
        if key.is_some() && (derived || explicit && empty()) {
            mark(key);
            marked = true;
        }
    }
    marked
}

/// The entries whose sort keys are conflicts.
fn marked_sort_keys<'a, K: 'a>(
    entries: impl Iterator<Item = (&'a K, &'a Option<JoinResult<SortKey>>)>,
) -> impl Iterator<Item = &'a K> {
    entries.filter_map(|(key, sort_key)| matches!(sort_key, Some(Err(_))).then_some(key))
}

impl UnorderedPsbt {
    /// Clear the inputs' `tx_modifiable_flags` bit and freeze the inputs.
    pub(crate) fn no_more_inputs(mut self) -> Self {
        self.global
            .set_tx_modifiable_flags(self.global.tx_modifiable_flags() & !INPUTS_MODIFIABLE);
        self.inputs = self.inputs.freeze();
        self
    }

    /// Clear the outputs' `tx_modifiable_flags` bit and freeze the outputs.
    pub(crate) fn no_more_outputs(mut self) -> Self {
        self.global
            .set_tx_modifiable_flags(self.global.tx_modifiable_flags() & !OUTPUTS_MODIFIABLE);
        self.outputs = self.outputs.freeze();
        self
    }
}

impl UnorderedPsbt {
    /// Read a PSBTv2 value, discarding its input and output positions. A clear
    /// input or output modifiability bit freezes the corresponding set.
    ///
    /// Returns an error for a sort-rule conflict recorded by
    /// [`ResultUnorderedPsbt::try_from_psbt`].
    ///
    /// # Errors
    /// Returns the errors of [`ResultUnorderedPsbt::try_from_psbt`]. Then,
    /// under derivation `0x01`, returns
    /// [`UnorderedPsbtError::UnexpectedInputSortKey`] for the smallest outpoint
    /// whose input has a sort key, or
    /// [`UnorderedPsbtError::UnexpectedOutputSortKey`] for the smallest unique
    /// ID whose output has one. Under derivation `0x00`, returns
    /// [`UnorderedPsbtError::EmptyInputSortKey`] for the smallest outpoint
    /// whose input has an empty sort key, or
    /// [`UnorderedPsbtError::EmptyOutputSortKey`] for the smallest unique ID
    /// whose output has one.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        ResultUnorderedPsbt::try_from_psbt(psbt)?
            .try_unwrap()
            .map_err(|psbt| psbt.sort_rule_error())
    }

    /// The global fields.
    pub fn global(&self) -> &Global {
        &self.global
    }

    /// The inputs, keyed by outpoint.
    pub fn inputs(&self) -> &InputSet {
        &self.inputs
    }

    /// The outputs, keyed by unique ID.
    pub fn outputs(&self) -> &OutputSet {
        &self.outputs
    }

    /// Return a PSBTv2 [`Psbt`] marked unordered for exchange during construction.
    /// Its input and output vectors have arbitrary order. Their lengths supply the
    /// global counts.
    /// Use the [`Sorter`](crate::sorter::Sorter) to fix their order before signing.
    pub fn into_psbt(mut self) -> Psbt {
        self.global.set_unordered();
        let inputs: Vec<_> = self.inputs.into_iter().map(Into::into).collect();
        let outputs: Vec<_> = self.outputs.into_iter().map(Into::into).collect();
        Psbt {
            global: self.global.into_v2(inputs.len(), outputs.len()),
            inputs,
            outputs,
        }
    }

    /// Wrap this contribution for joining.
    pub fn wrap(self) -> ResultUnorderedPsbt {
        ResultUnorderedPsbt {
            global: self.global.wrap(),
            inputs: self.inputs.wrap(),
            outputs: self.outputs.wrap(),
        }
        .enforce_sort_rules()
    }
}

impl Join for ResultUnorderedPsbt {
    fn join(self, other: Self) -> Self {
        ResultUnorderedPsbt {
            global: self.global.join(other.global),
            inputs: self.inputs.join(other.inputs),
            outputs: self.outputs.join(other.outputs),
        }
        .enforce_sort_rules()
    }
}

impl ResultUnorderedPsbt {
    /// Borrow every conflict with its typed PSBT path and alternatives.
    /// Input and output identities are stable across ordering; iteration order
    /// between entries is unspecified.
    pub fn conflicts(&self) -> impl Iterator<Item = PsbtConflict<'_>> {
        self.global
            .conflicts()
            .map(PsbtConflict::from)
            .chain(self.inputs.conflicts().map(PsbtConflict::from))
            .chain(self.outputs.conflicts().map(PsbtConflict::from))
    }

    /// Parse a v2 PSBT directly into the result domain.
    ///
    /// Within one PSBT, each input must spend a distinct outpoint and each
    /// output must carry a distinct unique ID.
    ///
    /// The PSBT must be marked unordered, since reading an ordered PSBT as
    /// unordered would discard its order.
    ///
    /// A sort field that breaks the rules of
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is not an error here but a conflict,
    /// as it would be after a join. See
    /// [`UnorderedPsbt::try_from_psbt`] for the errors these conflicts become.
    ///
    /// # Errors
    /// Returns a count mismatch if the declared input or output count is not
    /// the number of inputs or outputs. Otherwise returns a malformed global sort field, then the
    /// first input whose sort key is longer than [`SortKey::MAX_LEN`] bytes,
    /// then the first output whose `PSBT_OUT_UNIQUE_ID` is missing or has
    /// an invalid length, or whose sort key is longer
    /// than [`SortKey::MAX_LEN`] bytes.
    ///
    /// Then returns [`UnorderedPsbtError::NotUnordered`] for an ordered PSBT,
    /// [`UnorderedPsbtError::UnorderedSighashSingle`] if the `SIGHASH_SINGLE`
    /// flag is set, or [`UnorderedPsbtError::UnorderedInputSighashNotAll`] for
    /// the first input whose requested sighash is not exactly `SIGHASH_ALL`.
    /// Finally, checks for repeated input outpoints and then repeated output
    /// unique IDs.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        let Psbt {
            global,
            inputs,
            outputs,
        } = psbt;
        if global.input_count != inputs.len() {
            return Err(UnorderedPsbtError::InputCountMismatch {
                declared: global.input_count,
                found: inputs.len(),
            });
        }
        if global.output_count != outputs.len() {
            return Err(UnorderedPsbtError::OutputCountMismatch {
                declared: global.output_count,
                found: outputs.len(),
            });
        }

        let global = Global::try_from(global)?;
        let inputs = inputs
            .into_iter()
            .map(|input| {
                let outpoint = OutPoint {
                    txid: input.previous_txid,
                    vout: input.spent_output_index,
                };
                Input::try_from(input)
                    .map_err(|_| UnorderedPsbtError::InputSortKeyTooLong(outpoint))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let outputs = outputs
            .into_iter()
            .enumerate()
            .map(|(index, output)| {
                Output::try_from(output)
                    .map_err(|error| UnorderedPsbtError::invalid_output(index, error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        check_unordered(&global)?;
        inputs.iter().try_for_each(check_input_sighash)?;

        let inputs = InputSet::try_from_inputs(inputs)?;
        let inputs = if global.tx_modifiable_flags() & INPUTS_MODIFIABLE == 0 {
            inputs.freeze()
        } else {
            inputs
        };
        let outputs = OutputSet::try_from_outputs(outputs)?;
        let outputs = if global.tx_modifiable_flags() & OUTPUTS_MODIFIABLE == 0 {
            outputs.freeze()
        } else {
            outputs
        };

        Ok(UnorderedPsbt {
            global,
            inputs,
            outputs,
        }
        .wrap())
    }

    /// Mark the sort fields that break the rules of
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` as conflicts: every explicit sort key
    /// if it may be `0x01`, and every empty one if it may be `0x00`. The
    /// derivation is marked with them, since together they are invalid.
    ///
    /// The rules read the values a field may hold, which joins only add to, so
    /// a marked field stays marked under later joins. Marking keeps the values,
    /// so marking again marks nothing new. Marking after every join therefore
    /// keeps it a join.
    fn enforce_sort_rules(mut self) -> Self {
        let may_be =
            |derivation| values(&self.global.sort_key_derivation).any(|value| *value == derivation);
        let derived = may_be(SortKeyDerivation::ExplicitKeysForbidden);
        let explicit = may_be(SortKeyDerivation::ExplicitKeysRequired);
        let inputs_broken = mark_sort_keys(
            self.inputs
                .0
                .entries_mut()
                .map(|(_, input)| &mut input.sort_key),
            derived,
            explicit,
        );
        let outputs_broken = mark_sort_keys(
            self.outputs
                .0
                .entries_mut()
                .map(|(_, output)| &mut output.sort_key),
            derived,
            explicit,
        );
        if inputs_broken || outputs_broken {
            mark(&mut self.global.sort_key_derivation);
        }
        self
    }

    /// The rule that a PSBT read by [`try_from_psbt`](Self::try_from_psbt)
    /// breaks, if it has conflicts. Reading joins nothing, so its only
    /// conflicts are the sort fields that
    /// [`enforce_sort_rules`](Self::enforce_sort_rules) marked, each holding
    /// the one value read.
    fn sort_rule_error(&self) -> UnorderedPsbtError {
        let outpoint = marked_sort_keys(
            self.inputs
                .0
                .entries()
                .map(|(outpoint, input)| (outpoint, &input.sort_key)),
        )
        .min()
        .copied();
        let unique_id = || {
            marked_sort_keys(
                self.outputs
                    .0
                    .entries()
                    .map(|(unique_id, output)| (unique_id, &output.sort_key)),
            )
            .min_by_key(|unique_id| unique_id.as_bytes())
            .expect("a sort key was marked")
            .clone()
        };
        let derived = values(&self.global.sort_key_derivation)
            .any(|value| *value == SortKeyDerivation::ExplicitKeysForbidden);
        match (derived, outpoint) {
            (true, Some(outpoint)) => UnorderedPsbtError::UnexpectedInputSortKey(outpoint),
            (true, None) => UnorderedPsbtError::UnexpectedOutputSortKey(unique_id()),
            (false, Some(outpoint)) => UnorderedPsbtError::EmptyInputSortKey(outpoint),
            (false, None) => UnorderedPsbtError::EmptyOutputSortKey(unique_id()),
        }
    }

    /// Return `true` if there are no field or membership conflicts.
    pub fn is_ok(&self) -> bool {
        self.global.is_ok() && self.inputs.is_ok() && self.outputs.is_ok()
    }

    /// Return the clean [`UnorderedPsbt`], or `Err(self)` on a field or membership conflict.
    pub fn try_unwrap(self) -> Result<UnorderedPsbt, Self> {
        if !self.is_ok() {
            return Err(self);
        }
        Ok(UnorderedPsbt {
            global: self
                .global
                .try_unwrap()
                .expect("verified all fields are Ok"),
            inputs: self
                .inputs
                .try_unwrap()
                .expect("verified all fields are Ok"),
            outputs: self
                .outputs
                .try_unwrap()
                .expect("verified all fields are Ok"),
        })
    }
}

impl Lift for UnorderedPsbt {
    type Result = ResultUnorderedPsbt;

    fn wrap(self) -> Self::Result {
        Self::wrap(self)
    }
}

impl ResultWrapped for ResultUnorderedPsbt {
    type Clean = UnorderedPsbt;

    fn is_ok(&self) -> bool {
        Self::is_ok(self)
    }

    fn try_unwrap(self) -> Result<Self::Clean, Self> {
        Self::try_unwrap(self)
    }
}

impl Conflicts for ResultUnorderedPsbt {
    type Item<'a> = PsbtConflict<'a>;

    fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
        Self::conflicts(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    #[cfg(feature = "prop-tests")]
    mod conflict_properties {
        use super::*;
        use crate::psbt::conflicts::ConflictPath;
        use crate::psbt::global::GlobalField;
        use crate::psbt::input::InputField;
        use crate::psbt::output::OutputField;
        use crate::{Conflicts, Lift, ResultWrapped};
        use bitcoin::hashes::Hash;
        use proptest::prelude::*;
        use std::collections::HashMap;

        proptest! {
            #[test]
            fn nested_psbt_conflicts_locate_scope_entry_and_keydata(
                left_values in any::<[u8; 3]>(), right_values in any::<[u8; 3]>(),
                txid_byte in any::<u8>(), outer_key in any::<u8>(), same in any::<bool>(),
                keydata in proptest::collection::vec(any::<u8>(), 0..8),
            ) {
                let right_values = if same { left_values } else { right_values };
                let key = psbt_v2::raw::Key { type_value: 0x80, key: keydata };
                let outpoint = OutPoint { txid: bitcoin::Txid::from_byte_array([txid_byte; 32]), vout: 0 };
                let id = UniqueId::try_from(vec![1; 16]).unwrap();
                let make = |values: [u8; 3]| {
                    let mut input = psbt_v2::v2::Input::new(&outpoint);
                    input.unknowns.insert(key.clone(), vec![values[1]]);
                    let mut output = v2_output_with_uid(vec![1; 16]);
                    output.unknowns.insert(key.clone(), vec![values[2]]);
                    let mut psbt = v2_psbt(vec![input], vec![output]);
                    psbt.global.unknowns.insert(key.clone(), vec![values[0]]);
                    UnorderedPsbt::try_from_psbt(psbt).unwrap()
                };
                let left = HashMap::from([(outer_key, Some(make(left_values)))]);
                let right = HashMap::from([(outer_key, Some(make(right_values)))]);
                let joined = left.clone().wrap().join(right.wrap());
                let mut expected = Vec::new();
                if left_values[0] != right_values[0] { expected.push(ConflictPath::Global(GlobalField::Unknown(&key))); }
                if left_values[1] != right_values[1] { expected.push(ConflictPath::Input { outpoint: &outpoint, field: InputField::Unknown(&key) }); }
                if left_values[2] != right_values[2] { expected.push(ConflictPath::Output { id: &id, field: OutputField::Unknown(&key) }); }
                let actual: Vec<_> = Conflicts::conflicts(&joined).map(|(found_key, conflict)| {
                    assert_eq!(found_key, &outer_key);
                    conflict.path()
                }).collect();
                prop_assert_eq!(&actual, &expected);
                prop_assert_eq!(ResultWrapped::is_ok(&joined), expected.is_empty());
                if expected.is_empty() {
                    prop_assert_eq!(ResultWrapped::try_unwrap(joined), Ok(left));
                } else {
                    prop_assert_eq!(ResultWrapped::try_unwrap(joined.clone()), Err(joined));
                }
            }
        }
    }
    use super::*;
    use crate::global::SortSeed;
    use crate::lattice::join::Join;
    use crate::psbt::output::OutputUniqueIdExt;

    /// A BIP 370 output with unique ID `uid` and every other field unset.
    fn v2_output_with_uid(uid: Vec<u8>) -> psbt_v2::v2::Output {
        let mut output = psbt_v2::v2::Output::default();
        output.set_unique_id_bytes(uid);
        output
    }

    /// A BIP 370 PSBT with this global map, inputs and outputs, and
    /// consistent counts.
    fn v2_psbt_with(
        global: Global,
        inputs: Vec<psbt_v2::v2::Input>,
        outputs: Vec<psbt_v2::v2::Output>,
    ) -> Psbt {
        Psbt {
            global: global.into_v2(inputs.len(), outputs.len()),
            inputs,
            outputs,
        }
    }

    /// An unordered BIP 370 PSBT with these inputs and outputs.
    fn v2_psbt(inputs: Vec<psbt_v2::v2::Input>, outputs: Vec<psbt_v2::v2::Output>) -> Psbt {
        v2_psbt_with(Global::default(), inputs, outputs)
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use bitcoin::hashes::Hash;
        use bitcoin::{Amount, ScriptBuf, Sequence, Txid, transaction};

        fn outpoint(txid_byte: u8) -> OutPoint {
            OutPoint {
                txid: Txid::from_byte_array([txid_byte; 32]),
                vout: 0,
            }
        }

        /// A modifiable PSBT with one input and one output, both derived from `byte`.
        fn singleton(byte: u8) -> UnorderedPsbt {
            let input = Input::new(&outpoint(byte));
            let output = Output::new(
                UniqueId::try_from(vec![byte; 16]).unwrap(),
                Amount::from_sat(100_000),
                ScriptBuf::new_op_return([byte; 20]),
            );
            let mut global = Global::default();
            global.set_tx_modifiable_flags(INPUTS_MODIFIABLE | OUTPUTS_MODIFIABLE);
            UnorderedPsbt {
                global,
                inputs: InputSet::try_from_inputs([input]).unwrap(),
                outputs: OutputSet::try_from_outputs([output]).unwrap(),
            }
        }

        #[test]
        fn conflicts_have_full_paths_and_typed_values() {
            use crate::conflicts::{ConflictPath, PsbtConflict};
            use crate::global::{GlobalConflict, GlobalField};
            use crate::input::InputField;
            use crate::output::OutputField;
            use psbt_v2::raw;

            let key = raw::Key {
                type_value: 0x80,
                key: vec![1, 2],
            };
            let build = |value| {
                let mut global = Global::default();
                global.set_tx_version(transaction::Version(value));
                let mut input = Input::new(&outpoint(1));
                input.set_sequence(Some(Sequence(value as u32)));
                input.set_unknowns([(key.clone(), vec![value as u8])].into());
                let output = Output::new(
                    UniqueId::try_from(vec![7; 16]).unwrap(),
                    Amount::from_sat(value as u64),
                    ScriptBuf::new(),
                );
                UnorderedPsbt {
                    global,
                    inputs: InputSet::try_from_inputs([input]).unwrap(),
                    outputs: OutputSet::try_from_outputs([output]).unwrap(),
                }
                .wrap()
            };
            let joined = build(1).join(build(2));
            let conflicts: Vec<_> = joined.conflicts().collect();
            let paths: Vec<_> = conflicts.iter().map(|c| c.path()).collect();
            assert_eq!(
                paths,
                [
                    ConflictPath::Global(GlobalField::TxVersion),
                    ConflictPath::Input {
                        outpoint: &outpoint(1),
                        field: InputField::Sequence
                    },
                    ConflictPath::Input {
                        outpoint: &outpoint(1),
                        field: InputField::Unknown(&key)
                    },
                    ConflictPath::Output {
                        id: &UniqueId::try_from(vec![7; 16]).unwrap(),
                        field: OutputField::Amount
                    },
                ]
            );
            let PsbtConflict::Global(GlobalConflict::TxVersion { values }) = conflicts[0] else {
                panic!("expected transaction versions");
            };
            assert_eq!(
                values.into_iter().copied().collect::<Vec<_>>(),
                [transaction::Version(1), transaction::Version(2)]
            );
            assert_eq!(singleton(1).wrap().conflicts().count(), 0);
        }

        #[test]
        fn membership_conflicts_are_reported_once_per_scope() {
            use crate::conflicts::{ConflictPath, PsbtConflict};
            use std::collections::HashSet;

            let freeze = |a: UnorderedPsbt| UnorderedPsbt {
                inputs: a.inputs.freeze(),
                outputs: a.outputs.freeze(),
                ..a
            };
            let joined = freeze(singleton(1))
                .wrap()
                .join(freeze(singleton(2)).wrap())
                .join(singleton(3).wrap());
            let conflicts: Vec<_> = joined.conflicts().collect();
            assert_eq!(
                conflicts.iter().map(|c| c.path()).collect::<Vec<_>>(),
                [
                    ConflictPath::InputMembership,
                    ConflictPath::OutputMembership
                ]
            );
            let PsbtConflict::InputMembership(sets) = &conflicts[0] else {
                panic!("input membership");
            };
            assert_eq!(sets.len(), 3);
            assert!(sets.contains(&HashSet::from([&outpoint(1)])));
            assert!(sets.contains(&HashSet::from([&outpoint(2)])));
            assert!(sets.contains(&HashSet::from([&outpoint(1), &outpoint(2), &outpoint(3)])));
            let PsbtConflict::OutputMembership(sets) = &conflicts[1] else {
                panic!("output membership");
            };
            let ids = [1, 2, 3].map(|byte| UniqueId::try_from(vec![byte; 16]).unwrap());
            assert_eq!(sets.len(), 3);
            assert!(sets.contains(&HashSet::from([&ids[0]])));
            assert!(sets.contains(&HashSet::from([&ids[1]])));
            assert!(sets.contains(&ids.iter().collect()));
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }

        #[test]
        fn different_tx_versions_conflict() {
            let with_version = |tx_version| {
                let mut psbt = singleton(1);
                psbt.global.set_tx_version(tx_version);
                psbt.wrap()
            };
            let joined = with_version(transaction::Version::ONE)
                .join(with_version(transaction::Version::TWO));
            assert!(!joined.is_ok());
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }

        #[test]
        fn errors_display_their_cause() {
            let cases = [
                (
                    UnorderedPsbtError::InputCountMismatch {
                        declared: 1,
                        found: 0,
                    },
                    "declares 1 inputs but has 0".to_string(),
                ),
                (
                    UnorderedPsbtError::OutputCountMismatch {
                        declared: 0,
                        found: 2,
                    },
                    "declares 0 outputs but has 2".to_string(),
                ),
                (
                    UnorderedPsbtError::MissingOutputUniqueId(1),
                    "output 1 missing PSBT_OUT_UNIQUE_ID".to_string(),
                ),
                (
                    UnorderedPsbtError::InvalidOutputUniqueId(1),
                    "PSBT_OUT_UNIQUE_ID of output 1 must be 16 to 32 bytes".to_string(),
                ),
                (
                    UnorderedPsbtError::DuplicateInput(outpoint(1)),
                    format!("two inputs spend {}", outpoint(1)),
                ),
                (
                    UnorderedPsbtError::DuplicateOutputUniqueId(
                        UniqueId::try_from(vec![1; 16]).unwrap(),
                    ),
                    "two outputs share a PSBT_OUT_UNIQUE_ID".to_string(),
                ),
                (
                    UnorderedPsbtError::InvalidUnorderedFlag(vec![1]),
                    "PSBT_GLOBAL_TX_UNORDERED is [01], not 0x03".to_string(),
                ),
                (
                    UnorderedPsbtError::InvalidSortKeyDerivation(vec![2]),
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is [02], not 0x00 or 0x01".to_string(),
                ),
                (
                    UnorderedPsbtError::InputSortKeyTooLong(outpoint(1)),
                    format!(
                        "sort key of the input spending {} is longer than 32 bytes",
                        outpoint(1)
                    ),
                ),
                (
                    UnorderedPsbtError::OutputSortKeyTooLong(1),
                    "sort key of output 1 is longer than 32 bytes".to_string(),
                ),
                (
                    UnorderedPsbtError::NotUnordered,
                    "PSBT is not marked unordered".to_string(),
                ),
                (
                    UnorderedPsbtError::UnorderedSighashSingle,
                    "unordered PSBT has the SIGHASH_SINGLE flag set".to_string(),
                ),
                (
                    UnorderedPsbtError::UnorderedInputSighashNotAll(outpoint(1)),
                    format!(
                        "the input spending {} requests a sighash other than SIGHASH_ALL",
                        outpoint(1)
                    ),
                ),
                (
                    UnorderedPsbtError::UnexpectedInputSortKey(outpoint(1)),
                    format!(
                        "the input spending {} has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01",
                        outpoint(1)
                    ),
                ),
                (
                    UnorderedPsbtError::UnexpectedOutputSortKey(
                        UniqueId::try_from(vec![0xab; 16]).unwrap(),
                    ),
                    format!(
                        "the output with unique ID {:02x?} has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01",
                        [0xab; 16]
                    ),
                ),
                (
                    UnorderedPsbtError::EmptyInputSortKey(outpoint(1)),
                    format!(
                        "the input spending {} has an empty sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00",
                        outpoint(1)
                    ),
                ),
                (
                    UnorderedPsbtError::EmptyOutputSortKey(
                        UniqueId::try_from(vec![0xab; 16]).unwrap(),
                    ),
                    format!(
                        "the output with unique ID {:02x?} has an empty sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00",
                        [0xab; 16]
                    ),
                ),
            ];
            for (error, message) in cases {
                assert_eq!(error.to_string(), message);
            }
        }

        #[test]
        fn psbt_import_checks_the_uuid_length() {
            use crate::global::PSBT_GLOBAL_TX_UUID_SUBTYPE;

            for (len, valid) in [(0, false), (15, false), (16, true), (32, true), (33, false)] {
                let bytes = vec![0; len];
                let mut psbt = v2_psbt_with(Global::default(), vec![], vec![]);
                psbt.global.proprietaries.insert(
                    crate::psbt::proprietary::key(PSBT_GLOBAL_TX_UUID_SUBTYPE),
                    bytes.clone(),
                );
                if valid {
                    let parsed = UnorderedPsbt::try_from_psbt(psbt).unwrap();
                    assert_eq!(parsed.global().uuid(), Some(bytes.as_slice()));
                } else {
                    let error = UnorderedPsbtError::InvalidTxUuid(InvalidTxUuid(bytes));
                    assert_eq!(
                        error.to_string(),
                        format!("PSBT_GLOBAL_TX_UUID is {len} bytes, expected 16 to 32"),
                    );
                    assert_eq!(
                        UnorderedPsbt::try_from_psbt(psbt.clone()),
                        Err(error.clone())
                    );
                    assert_eq!(ResultUnorderedPsbt::try_from_psbt(psbt), Err(error));
                }
            }
        }

        #[test]
        fn invalid_psbts_are_rejected() {
            let input_with_sequence = |sequence| {
                let mut input = psbt_v2::v2::Input::new(&outpoint(1));
                input.sequence = Some(Sequence(sequence));
                input
            };
            let output_with_amount = |sat| {
                let mut output = v2_output_with_uid(vec![1; 16]);
                output.amount = Amount::from_sat(sat);
                output
            };
            let field = |subtype| psbt_v2::raw::ProprietaryKey {
                prefix: crate::PROPRIETARY_PREFIX.to_vec(),
                subtype,
                key: vec![],
            };
            let with_global_field = |subtype, value: Vec<u8>| {
                let mut psbt = v2_psbt(vec![], vec![]);
                psbt.global.proprietaries.insert(field(subtype), value);
                psbt
            };
            let mut long_input_key = psbt_v2::v2::Input::new(&outpoint(1));
            long_input_key
                .proprietaries
                .insert(field(crate::input::PSBT_IN_SORT_KEY_SUBTYPE), vec![0; 33]);
            let mut long_output_key = v2_output_with_uid(vec![2; 16]);
            long_output_key
                .proprietaries
                .insert(field(crate::output::PSBT_OUT_SORT_KEY_SUBTYPE), vec![0; 33]);
            let cases = [
                (
                    with_global_field(crate::global::PSBT_GLOBAL_TX_UNORDERED_SUBTYPE, vec![1]),
                    UnorderedPsbtError::InvalidUnorderedFlag(vec![1]),
                ),
                (
                    with_global_field(
                        crate::global::PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE,
                        vec![2],
                    ),
                    UnorderedPsbtError::InvalidSortKeyDerivation(vec![2]),
                ),
                (
                    v2_psbt(vec![long_input_key], vec![]),
                    UnorderedPsbtError::InputSortKeyTooLong(outpoint(1)),
                ),
                (
                    v2_psbt(vec![input_with_sequence(1), input_with_sequence(2)], vec![]),
                    UnorderedPsbtError::DuplicateInput(outpoint(1)),
                ),
                (
                    v2_psbt(
                        vec![],
                        vec![
                            v2_output_with_uid(vec![1; 16]),
                            psbt_v2::v2::Output::default(),
                        ],
                    ),
                    UnorderedPsbtError::MissingOutputUniqueId(1),
                ),
                (
                    v2_psbt(vec![], vec![v2_output_with_uid(vec![])]),
                    UnorderedPsbtError::InvalidOutputUniqueId(0),
                ),
                (
                    v2_psbt(vec![], vec![v2_output_with_uid(vec![1; 33])]),
                    UnorderedPsbtError::InvalidOutputUniqueId(0),
                ),
                (
                    v2_psbt(
                        vec![],
                        vec![v2_output_with_uid(vec![1; 16]), long_output_key],
                    ),
                    UnorderedPsbtError::OutputSortKeyTooLong(1),
                ),
                (
                    v2_psbt(
                        vec![],
                        vec![output_with_amount(1000), output_with_amount(2000)],
                    ),
                    UnorderedPsbtError::DuplicateOutputUniqueId(
                        UniqueId::try_from(vec![1; 16]).unwrap(),
                    ),
                ),
            ];
            for (psbt, error) in cases {
                assert_eq!(ResultUnorderedPsbt::try_from_psbt(psbt), Err(error));
            }
        }

        #[test]
        fn into_psbt_marks_an_unflagged_global_unordered() {
            use crate::global::PSBT_GLOBAL_TX_UNORDERED_SUBTYPE;
            use crate::psbt::proprietary::key;

            let mut unordered = singleton(1);
            unordered.global.clear_unordered();
            let psbt = Psbt::deserialize(&unordered.into_psbt().serialize()).unwrap();
            assert_eq!(
                psbt.global
                    .proprietaries
                    .get(&key(PSBT_GLOBAL_TX_UNORDERED_SUBTYPE))
                    .map(Vec::as_slice),
                Some([0x03].as_slice())
            );
            assert!(
                UnorderedPsbt::try_from_psbt(psbt)
                    .unwrap()
                    .global()
                    .is_unordered()
            );
        }

        #[test]
        fn psbts_that_break_the_sort_rules_are_rejected() {
            use SortKeyDerivation::{ExplicitKeysForbidden, ExplicitKeysRequired};

            let global = |derivation: Option<SortKeyDerivation>, seeded: bool| {
                let mut global = Global::default();
                if let Some(derivation) = derivation {
                    global.set_sort_key_derivation(derivation);
                }
                if seeded {
                    global.set_sort_seed(SortSeed::from(vec![0; 16]));
                }
                global
            };
            let input = |txid_byte, sort_key: Option<&[u8]>| {
                let mut input = Input::new(&outpoint(txid_byte));
                if let Some(key) = sort_key {
                    input.set_sort_key(SortKey::try_from(key.to_vec()).unwrap());
                }
                psbt_v2::v2::Input::from(input)
            };
            let output = |uid, sort_key: Option<&[u8]>| {
                let mut output = Output::new(
                    UniqueId::try_from(vec![uid; 16]).unwrap(),
                    Amount::ZERO,
                    ScriptBuf::new(),
                );
                if let Some(key) = sort_key {
                    output.set_sort_key(SortKey::try_from(key.to_vec()).unwrap());
                }
                psbt_v2::v2::Output::from(output)
            };
            let mut sighash_single = Global::default();
            sighash_single.set_tx_modifiable_flags(HAS_SIGHASH_SINGLE);
            let cases = [
                (
                    Psbt {
                        global: psbt_v2::v2::Global::default(),
                        inputs: vec![],
                        outputs: vec![],
                    },
                    Err(UnorderedPsbtError::NotUnordered),
                ),
                (
                    v2_psbt_with(sighash_single, vec![], vec![]),
                    Err(UnorderedPsbtError::UnorderedSighashSingle),
                ),
                (
                    v2_psbt_with(
                        global(Some(ExplicitKeysForbidden), true),
                        vec![input(1, None), input(2, Some(&[0])), input(3, Some(&[]))],
                        vec![output(1, Some(&[0]))],
                    ),
                    Err(UnorderedPsbtError::UnexpectedInputSortKey(outpoint(2))),
                ),
                (
                    v2_psbt_with(
                        global(Some(ExplicitKeysForbidden), false),
                        vec![],
                        vec![output(1, None), output(2, Some(&[]))],
                    ),
                    Err(UnorderedPsbtError::UnexpectedOutputSortKey(
                        UniqueId::try_from(vec![2; 16]).unwrap(),
                    )),
                ),
                (
                    v2_psbt_with(
                        global(Some(ExplicitKeysRequired), false),
                        vec![
                            input(1, Some(&[0])),
                            input(2, Some(&[])),
                            input(3, Some(&[])),
                        ],
                        vec![output(1, Some(&[]))],
                    ),
                    Err(UnorderedPsbtError::EmptyInputSortKey(outpoint(2))),
                ),
                (
                    v2_psbt_with(
                        global(Some(ExplicitKeysRequired), true),
                        vec![input(1, Some(&[0]))],
                        vec![
                            output(1, Some(&[0])),
                            output(3, Some(&[])),
                            output(2, Some(&[])),
                        ],
                    ),
                    Err(UnorderedPsbtError::EmptyOutputSortKey(
                        UniqueId::try_from(vec![2; 16]).unwrap(),
                    )),
                ),
                // Derivation 0x01 accepts entries without explicit keys.
                (
                    v2_psbt_with(
                        global(Some(ExplicitKeysForbidden), true),
                        vec![input(1, None)],
                        vec![output(1, None)],
                    ),
                    Ok(()),
                ),
                // Derivation 0x00 accepts a seed, missing keys, which may be
                // added later, and keys that repeat.
                (
                    v2_psbt_with(
                        global(Some(ExplicitKeysRequired), true),
                        vec![input(1, Some(&[0])), input(2, Some(&[0])), input(3, None)],
                        vec![output(1, Some(&[0])), output(2, Some(&[0]))],
                    ),
                    Ok(()),
                ),
                // Without a derivation, keys may be empty or repeat.
                (
                    v2_psbt_with(
                        global(None, true),
                        vec![input(1, Some(&[])), input(2, Some(&[]))],
                        vec![output(1, Some(&[]))],
                    ),
                    Ok(()),
                ),
            ];
            for (psbt, expected) in cases {
                // Reading into the result domain turns a broken sort rule into
                // conflicts, the derivation among them, rather than an error.
                let result = ResultUnorderedPsbt::try_from_psbt(psbt.clone());
                match &expected {
                    Ok(()) => assert!(result.unwrap().is_ok()),
                    Err(
                        UnorderedPsbtError::NotUnordered
                        | UnorderedPsbtError::UnorderedSighashSingle,
                    ) => {
                        assert_eq!(result.map(|_| ()), expected);
                    }
                    Err(_) => {
                        let result = result.unwrap();
                        assert!(matches!(result.global.sort_key_derivation, Some(Err(_))));
                        assert!(!result.is_ok());
                    }
                }
                assert_eq!(UnorderedPsbt::try_from_psbt(psbt).map(|_| ()), expected);
            }
        }

        #[test]
        fn unordered_import_checks_requested_input_sighashes() {
            use psbt_v2::PsbtSighashType;

            let cases = [
                (None, false),
                (Some(0x00), true),  // Taproot DEFAULT
                (Some(0x01), false), // SIGHASH_ALL
                (Some(0x02), true),
                (Some(0x03), true),
                (Some(0x81), true),
                (Some(0x82), true),
                (Some(0x83), true),
                (Some(0x23), true),
                (Some(0xa3), true),
                (Some(0x103), true),
                (Some(0xff), true),
            ];
            for (sighash, invalid) in cases {
                let mut input = psbt_v2::v2::Input::new(&outpoint(2));
                input.sighash_type = sighash.map(PsbtSighashType::from_u32);
                let inputs = vec![psbt_v2::v2::Input::new(&outpoint(1)), input];
                let psbt = v2_psbt_with(Global::default(), inputs, vec![]);
                let expected = if invalid {
                    Err(UnorderedPsbtError::UnorderedInputSighashNotAll(outpoint(2)))
                } else {
                    Ok(())
                };
                assert_eq!(
                    UnorderedPsbt::try_from_psbt(psbt.clone()).map(|_| ()),
                    expected
                );
                assert_eq!(
                    ResultUnorderedPsbt::try_from_psbt(psbt).map(|_| ()),
                    expected
                );
            }
        }

        #[test]
        fn unordered_import_checks_global_flags_before_input_sighash() {
            let input = |byte| {
                let mut input = psbt_v2::v2::Input::new(&outpoint(byte));
                input.sighash_type = Some(EcdsaSighashType::Single.into());
                input
            };
            let mut ordered = Global::default();
            ordered.clear_unordered();
            ordered.set_tx_modifiable_flags(HAS_SIGHASH_SINGLE);
            let mut flagged = Global::default();
            flagged.set_tx_modifiable_flags(HAS_SIGHASH_SINGLE);
            for (global, error) in [
                (ordered, UnorderedPsbtError::NotUnordered),
                (flagged, UnorderedPsbtError::UnorderedSighashSingle),
                (
                    Global::default(),
                    UnorderedPsbtError::UnorderedInputSighashNotAll(outpoint(2)),
                ),
            ] {
                let psbt = v2_psbt_with(global, vec![input(2), input(1)], vec![]);
                assert_eq!(
                    UnorderedPsbt::try_from_psbt(psbt.clone()),
                    Err(error.clone())
                );
                assert_eq!(ResultUnorderedPsbt::try_from_psbt(psbt), Err(error));
            }
        }

        #[test]
        fn joins_that_break_the_sort_rules_mark_the_fields_that_break_them() {
            let empty_key = SortKey::try_from(vec![]).unwrap();
            let keyed = |byte, key: &SortKey| {
                let mut input = Input::new(&outpoint(byte));
                input.set_sort_key(key.clone());
                let mut psbt = singleton(byte);
                psbt.inputs = InputSet::try_from_inputs([input]).unwrap();
                psbt
            };
            let explicit = {
                let mut psbt = singleton(2);
                psbt.global
                    .set_sort_key_derivation(SortKeyDerivation::ExplicitKeysRequired);
                psbt.wrap()
            };
            let joined = explicit.join(keyed(1, &empty_key).wrap());
            assert_eq!(
                joined.global.sort_key_derivation,
                Some(Err(Conflict::from_values([
                    SortKeyDerivation::ExplicitKeysRequired
                ])))
            );
            for (spent, input) in joined.inputs.0.entries() {
                let expected = (*spent == outpoint(1))
                    .then(|| Err(Conflict::from_values([empty_key.clone()])));
                assert_eq!(input.sort_key, expected);
            }
            assert!(joined.clone().try_unwrap().is_err());

            // Without a derivation, nothing is marked: keys may be empty or
            // repeat, and a seed is allowed.
            let mut seeded = keyed(2, &empty_key);
            seeded.global.set_sort_seed(SortSeed::from(vec![0; 16]));
            let joined = keyed(1, &empty_key).wrap().join(seeded.wrap());
            assert!(joined.is_ok());
        }

        #[test]
        fn serialized_psbts_with_disjoint_entries_join_into_their_union() {
            let joined = (1..=3)
                .map(|byte| {
                    let bytes = singleton(byte).into_psbt().serialize();
                    let psbt = Psbt::deserialize(&bytes).unwrap();
                    ResultUnorderedPsbt::try_from_psbt(psbt).unwrap()
                })
                .reduce(Join::join)
                .unwrap();
            let joined = joined.try_unwrap().expect("disjoint entries join cleanly");
            let inputs = (1..=3).map(|byte| Input::new(&outpoint(byte)));
            let outputs = (1..=3).map(|byte| {
                Output::new(
                    UniqueId::try_from(vec![byte; 16]).unwrap(),
                    Amount::from_sat(100_000),
                    ScriptBuf::new_op_return([byte; 20]),
                )
            });
            assert_eq!(joined.inputs, InputSet::try_from_inputs(inputs).unwrap());
            assert_eq!(
                joined.outputs,
                OutputSet::try_from_outputs(outputs).unwrap()
            );
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use crate::global::TxUuid;
        use bitcoin::transaction;
        use proptest::prelude::*;

        /// An UnorderedPsbt with either transaction version, any modifiable
        /// flags, and up to three inputs and outputs.
        fn arb_unordered_psbt() -> impl Strategy<Value = UnorderedPsbt> {
            arb_psbt(false)
        }

        /// As [`arb_unordered_psbt`], with any sort fields that
        /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` allows, from small domains so
        /// that joins break the rules.
        fn arb_sorted_psbt() -> impl Strategy<Value = UnorderedPsbt> {
            arb_psbt(true)
        }

        /// The sort key drawn as `key`, empty for `0`, if `derivation` allows
        /// it: none under `0x01`, and no empty one under `0x00`.
        fn allowed_key(
            sort_fields: bool,
            derivation: Option<SortKeyDerivation>,
        ) -> impl Fn(Option<u8>) -> Option<SortKey> {
            move |key| {
                let key = key
                    .filter(|_| {
                        sort_fields && derivation != Some(SortKeyDerivation::ExplicitKeysForbidden)
                    })
                    .map(|key| if key == 0 { vec![] } else { vec![key] })?;
                let empty_forbidden = derivation == Some(SortKeyDerivation::ExplicitKeysRequired);
                (!(empty_forbidden && key.is_empty()))
                    .then(|| SortKey::try_from(key).expect("at most one byte"))
            }
        }

        fn arb_psbt(sort_fields: bool) -> impl Strategy<Value = UnorderedPsbt> {
            (
                proptest::bool::ANY, // tx_version: ONE or TWO
                0u8..4,              // tx_modifiable_flags
                // inputs: (txid_byte, vout) to sort key
                proptest::collection::hash_map(
                    (0u8..5, 0u32..2),
                    proptest::option::of(0u8..3),
                    0..=3,
                ),
                // outputs: uid_byte to sort key
                proptest::collection::hash_map(0u8..10, proptest::option::of(0u8..3), 0..=3),
                proptest::option::of(prop_oneof![
                    Just(SortKeyDerivation::ExplicitKeysRequired),
                    Just(SortKeyDerivation::ExplicitKeysForbidden),
                ]),
                proptest::option::of(0u8..2), // sort seed: empty or 16 bytes
                proptest::option::of(1u8..3), // UUID byte
            )
                .prop_map(
                    move |(use_v1, flags, input_specs, output_specs, derivation, seed, uuid)| {
                        use bitcoin::hashes::Hash;

                        let derivation = derivation.filter(|_| sort_fields);
                        let input_key = allowed_key(sort_fields, derivation);
                        let inputs = InputSet::try_from_inputs(input_specs.into_iter().map(
                            |((txid_byte, vout), key)| {
                                let mut input = Input::new(&bitcoin::OutPoint {
                                    txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                                    vout,
                                });
                                if let Some(key) = input_key(key) {
                                    input.set_sort_key(key);
                                }
                                input
                            },
                        ))
                        .expect("distinct outpoints");

                        let output_key = allowed_key(sort_fields, derivation);
                        let outputs = OutputSet::try_from_outputs(output_specs.into_iter().map(
                            |(uid_byte, key)| {
                                let mut output = Output::new(
                                    UniqueId::try_from(vec![uid_byte; 16]).unwrap(),
                                    bitcoin::Amount::from_sat(100_000),
                                    bitcoin::ScriptBuf::new_op_return([uid_byte; 20]),
                                );
                                if let Some(key) = output_key(key) {
                                    output.set_sort_key(key);
                                }
                                output
                            },
                        ))
                        .expect("distinct UIDs");

                        let mut global = Global::default();
                        global.set_tx_version(if use_v1 {
                            transaction::Version::ONE
                        } else {
                            transaction::Version::TWO
                        });
                        global.set_tx_modifiable_flags(flags);
                        if let Some(derivation) = derivation {
                            global.set_sort_key_derivation(derivation);
                        }
                        if let Some(len) = seed.filter(|_| sort_fields) {
                            global.set_sort_seed(SortSeed::from(vec![len; 16 * usize::from(len)]));
                        }
                        if let Some(byte) = uuid.filter(|_| sort_fields) {
                            global.set_uuid(TxUuid::try_from(vec![byte; 16]).unwrap());
                        }
                        let psbt = UnorderedPsbt {
                            global,
                            inputs,
                            outputs,
                        };
                        // Reparse so the sets are frozen according to the flags.
                        UnorderedPsbt::try_from_psbt(psbt.into_psbt())
                            .expect("a valid unordered PSBT")
                    },
                )
        }

        /// A wrapped PSBT, or the join of two independently drawn ones, which
        /// may conflict.
        fn arb_result_unordered_psbt() -> impl Strategy<Value = ResultUnorderedPsbt> {
            prop_oneof![
                arb_sorted_psbt().prop_map(|p| p.wrap()),
                (arb_sorted_psbt(), arb_sorted_psbt()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_join_laws!(arb_result_unordered_psbt());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(a in arb_sorted_psbt()) {
                prop_assert_eq!(a.clone().wrap().try_unwrap(), Ok(a));
            }

            #[test]
            fn getters_return_the_parts(a in arb_unordered_psbt()) {
                prop_assert_eq!(a.global(), &a.global);
                prop_assert_eq!(a.inputs(), &a.inputs);
                prop_assert_eq!(a.outputs(), &a.outputs);
            }

            #[test]
            fn result_try_from_psbt_agrees_with_wrap(a in arb_sorted_psbt()) {
                prop_assert_eq!(
                    ResultUnorderedPsbt::try_from_psbt(a.clone().into_psbt()),
                    Ok(a.wrap())
                );
            }

            #[test]
            fn into_psbt_preserves_counts(a in arb_unordered_psbt()) {
                let in_count = a.inputs.len();
                let out_count = a.outputs.len();
                let psbt = a.into_psbt();
                prop_assert_eq!(psbt.inputs.len(), in_count);
                prop_assert_eq!(psbt.outputs.len(), out_count);
                prop_assert_eq!(psbt.global.input_count, psbt.inputs.len());
                prop_assert_eq!(psbt.global.output_count, psbt.outputs.len());
            }

            #[test]
            fn try_from_psbt_rejects_miscounted_psbt(
                a in arb_unordered_psbt(),
                input_count in proptest::option::of(0usize..4),
                output_count in proptest::option::of(0usize..4),
            ) {
                let mut psbt = a.clone().into_psbt();
                let (inputs, outputs) = (psbt.inputs.len(), psbt.outputs.len());
                psbt.global.input_count = input_count.unwrap_or(inputs);
                psbt.global.output_count = output_count.unwrap_or(outputs);
                let expected = if psbt.global.input_count != inputs {
                    Err(UnorderedPsbtError::InputCountMismatch {
                        declared: psbt.global.input_count,
                        found: inputs,
                    })
                } else if psbt.global.output_count != outputs {
                    Err(UnorderedPsbtError::OutputCountMismatch {
                        declared: psbt.global.output_count,
                        found: outputs,
                    })
                } else {
                    Ok(a)
                };
                prop_assert_eq!(UnorderedPsbt::try_from_psbt(psbt), expected);
            }

            #[test]
            fn frozen_sets_join_only_if_identical(
                a in arb_unordered_psbt(),
                mut b in arb_unordered_psbt(),
            ) {
                b.global.set_tx_version(*a.global.tx_version());
                // Reparse `b` so its sets are frozen according to `a`'s flags.
                b.global.set_tx_modifiable_flags(*a.global.tx_modifiable_flags());
                let b = UnorderedPsbt::try_from_psbt(b.into_psbt()).expect("a valid unordered PSBT");
                let flags = *a.global.tx_modifiable_flags();
                let inputs_agree = flags & INPUTS_MODIFIABLE != 0 || a.inputs == b.inputs;
                let outputs_agree = flags & OUTPUTS_MODIFIABLE != 0 || a.outputs == b.outputs;
                let joined = a.wrap().join(b.wrap());
                prop_assert_eq!(joined.is_ok(), inputs_agree && outputs_agree);
            }

            #[test]
            fn error_messages_are_distinct_and_name_the_offending_value(
                index in any::<usize>(),
                len in any::<usize>(),
                value in proptest::collection::vec(any::<u8>(), 0..4),
                vout in any::<u32>(),
            ) {
                let outpoint = OutPoint { vout, ..OutPoint::null() };
                let bytes = format!("{value:02x?}");
                let mut id_bytes = value.clone();
                id_bytes.resize(16, 0);
                let id = UniqueId::try_from(id_bytes).unwrap();
                let formatted_id = format!("{:02x?}", id.as_bytes());
                let errors = [
                    (
                        UnorderedPsbtError::InputCountMismatch { declared: len, found: index },
                        Some(format!("{len} inputs but has {index}")),
                    ),
                    (
                        UnorderedPsbtError::OutputCountMismatch { declared: len, found: index },
                        Some(format!("{len} outputs but has {index}")),
                    ),
                    (UnorderedPsbtError::InvalidUnorderedFlag(value.clone()), Some(bytes.clone())),
                    (UnorderedPsbtError::InvalidSortKeyDerivation(value.clone()), Some(bytes.clone())),
                    (UnorderedPsbtError::InputSortKeyTooLong(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::MissingOutputUniqueId(index), Some(index.to_string())),
                    (UnorderedPsbtError::InvalidOutputUniqueId(index), Some(index.to_string())),
                    (UnorderedPsbtError::OutputSortKeyTooLong(index), Some(index.to_string())),
                    (UnorderedPsbtError::DuplicateInput(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::NotUnordered, None),
                    (UnorderedPsbtError::UnorderedSighashSingle, None),
                    (UnorderedPsbtError::UnorderedInputSighashNotAll(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::UnexpectedInputSortKey(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::UnexpectedOutputSortKey(id.clone()), Some(formatted_id.clone())),
                    (UnorderedPsbtError::EmptyInputSortKey(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::EmptyOutputSortKey(id.clone()), Some(formatted_id.clone())),
                    // The unique ID is opaque bytes, so the message leaves it out.
                    (UnorderedPsbtError::DuplicateOutputUniqueId(id), None),
                ];
                let messages: Vec<_> = errors.iter().map(|(error, _)| error.to_string()).collect();
                for ((_, payload), message) in errors.iter().zip(&messages) {
                    if let Some(payload) = payload {
                        prop_assert!(message.contains(payload.as_str()), "{message:?} lacks {payload:?}");
                    }
                }
                let distinct: std::collections::HashSet<_> = messages.iter().collect();
                prop_assert_eq!(distinct.len(), messages.len());
            }

            #[test]
            fn malformed_sort_fields_reject_the_psbt(
                a in arb_unordered_psbt(),
                key_len in (SortKey::MAX_LEN + 1)..=40,
                derivation in 2u8..=255,
            ) {
                let field = |subtype| psbt_v2::raw::ProprietaryKey {
                    prefix: crate::PROPRIETARY_PREFIX.to_vec(),
                    subtype,
                    key: vec![],
                };
                let mut psbt = a.into_psbt();
                if let Some(input) = psbt.inputs.first_mut() {
                    input
                        .proprietaries
                        .insert(field(crate::input::PSBT_IN_SORT_KEY_SUBTYPE), vec![0; key_len]);
                    let outpoint = OutPoint {
                        txid: input.previous_txid,
                        vout: input.spent_output_index,
                    };
                    prop_assert_eq!(
                        UnorderedPsbt::try_from_psbt(psbt.clone()),
                        Err(UnorderedPsbtError::InputSortKeyTooLong(outpoint))
                    );
                }
                // A malformed global field is reported before any input.
                psbt.global.proprietaries.insert(
                    field(crate::global::PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE),
                    vec![derivation],
                );
                prop_assert_eq!(
                    UnorderedPsbt::try_from_psbt(psbt),
                    Err(UnorderedPsbtError::InvalidSortKeyDerivation(vec![derivation]))
                );
            }

            #[test]
            fn try_from_psbt_rejects_the_first_invalid_unique_id(
                uid_lens in proptest::collection::vec(proptest::option::of(0usize..=34), 0..=4),
            ) {
                let outputs = (0u8..)
                    .zip(&uid_lens)
                    .map(|(byte, len)| match len {
                        Some(len) => v2_output_with_uid(vec![byte; *len]),
                        None => psbt_v2::v2::Output::default(),
                    })
                    .collect();
                let expected = uid_lens
                    .iter()
                    .enumerate()
                    .find_map(|(index, len)| match len {
                        None => Some(UnorderedPsbtError::MissingOutputUniqueId(index)),
                        Some(len) if !(16..=32).contains(len) => {
                            Some(UnorderedPsbtError::InvalidOutputUniqueId(index))
                        }
                        Some(_) => None,
                    })
                    .map_or(Ok(uid_lens.len()), Err);
                let psbt = v2_psbt(vec![], outputs);
                prop_assert_eq!(UnorderedPsbt::try_from_psbt(psbt).map(|u| u.outputs.len()), expected);
            }

            #[test]
            fn try_from_psbt_requires_unordered_without_sighash_single(
                flags in 0u8..8,
                unordered in proptest::bool::ANY,
            ) {
                let mut global = Global::default();
                global.set_tx_modifiable_flags(flags);
                if !unordered {
                    global.clear_unordered();
                }
                let psbt = v2_psbt_with(global, vec![], vec![]);
                let expected = if !unordered {
                    Err(UnorderedPsbtError::NotUnordered)
                } else if flags & HAS_SIGHASH_SINGLE != 0 {
                    Err(UnorderedPsbtError::UnorderedSighashSingle)
                } else {
                    Ok(())
                };
                prop_assert_eq!(ResultUnorderedPsbt::try_from_psbt(psbt).map(|_| ()), expected);
            }

            /// Under derivation `0x01` the smallest entry with a sort key, and
            /// under `0x00` the smallest with an empty one, is rejected. Keys
            /// may repeat, and without a derivation anything goes. Entry `i`
            /// spends output `i` of the null txid or has unique ID `[i; 16]`, so the
            /// smallest is the first.
            #[test]
            fn try_from_psbt_rejects_the_smallest_entry_breaking_the_sort_rules(
                keys in proptest::collection::vec(proptest::option::of(0usize..=2), 0..=5),
                derivation in proptest::option::of(prop_oneof![
                    Just(SortKeyDerivation::ExplicitKeysRequired),
                    Just(SortKeyDerivation::ExplicitKeysForbidden),
                ]),
                on_outputs in proptest::bool::ANY,
            ) {
                let breaks = |key: &Option<usize>| match derivation {
                    Some(SortKeyDerivation::ExplicitKeysForbidden) => key.is_some(),
                    Some(SortKeyDerivation::ExplicitKeysRequired) => *key == Some(0),
                    None => false,
                };
                let first = keys.iter().position(breaks);
                let sort_key = |len| SortKey::try_from(vec![7; len]).expect("at most two bytes");
                let entries = (0u8..).zip(keys);
                let mut global = Global::default();
                if let Some(derivation) = derivation {
                    global.set_sort_key_derivation(derivation);
                }
                let (psbt, expected) = if on_outputs {
                    let outputs = entries
                        .map(|(uid, key)| {
                            let mut output = Output::new(
                                UniqueId::try_from(vec![uid; 16]).unwrap(),
                                bitcoin::Amount::ZERO,
                                bitcoin::ScriptBuf::new(),
                            );
                            if let Some(len) = key {
                                output.set_sort_key(sort_key(len));
                            }
                            output.into()
                        })
                        .collect();
                    let expected = first.map(|index| {
                        let unique_id = UniqueId::try_from(vec![index as u8; 16]).unwrap();
                        match derivation {
                            Some(SortKeyDerivation::ExplicitKeysForbidden) => UnorderedPsbtError::UnexpectedOutputSortKey(unique_id),
                            _ => UnorderedPsbtError::EmptyOutputSortKey(unique_id),
                        }
                    });
                    (v2_psbt_with(global, vec![], outputs), expected)
                } else {
                    let inputs = entries
                        .map(|(vout, key)| {
                            let mut input = Input::new(&OutPoint { vout: vout.into(), ..OutPoint::null() });
                            if let Some(len) = key {
                                input.set_sort_key(sort_key(len));
                            }
                            input.into()
                        })
                        .collect();
                    let expected = first.map(|index| {
                        let outpoint = OutPoint { vout: index as u32, ..OutPoint::null() };
                        match derivation {
                            Some(SortKeyDerivation::ExplicitKeysForbidden) => UnorderedPsbtError::UnexpectedInputSortKey(outpoint),
                            _ => UnorderedPsbtError::EmptyInputSortKey(outpoint),
                        }
                    });
                    (v2_psbt_with(global, inputs, vec![]), expected)
                };
                prop_assert_eq!(
                    UnorderedPsbt::try_from_psbt(psbt).map(|_| ()),
                    expected.map_or(Ok(()), Err)
                );
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(r in arb_result_unordered_psbt()) {
                prop_assert_eq!(r.conflicts().next().is_none(), r.is_ok());
                prop_assert_eq!(r.is_ok(), r.try_unwrap().is_ok());
            }

            #[test]
            fn try_from_psbt_rejects_repeated_outpoints(
                specs in proptest::collection::vec((0u8..3, 0u32..2), 0..=4),
            ) {
                use bitcoin::hashes::Hash;

                let outpoints: Vec<OutPoint> = specs
                    .into_iter()
                    .map(|(txid_byte, vout)| OutPoint {
                        txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                        vout,
                    })
                    .collect();
                let mut seen = std::collections::HashSet::new();
                let first_repeat = outpoints.iter().copied().find(|op| !seen.insert(*op));

                let psbt = v2_psbt(outpoints.iter().map(psbt_v2::v2::Input::new).collect(), vec![]);

                let result = ResultUnorderedPsbt::try_from_psbt(psbt.clone());
                let clean = UnorderedPsbt::try_from_psbt(psbt);
                match first_repeat {
                    // Rejected even when the duplicates are identical.
                    Some(op) => {
                        prop_assert_eq!(result, Err(UnorderedPsbtError::DuplicateInput(op)));
                        prop_assert_eq!(clean, Err(UnorderedPsbtError::DuplicateInput(op)));
                    }
                    None => {
                        prop_assert_eq!(clean.map(|u| u.inputs.len()), Ok(outpoints.len()));
                    }
                }
            }

            #[test]
            fn try_from_psbt_rejects_the_first_input_sighash_other_than_all(
                sighashes in proptest::collection::vec(
                    proptest::option::of(prop_oneof![Just(0x01u32), any::<u32>()]),
                    0..=4,
                ),
            ) {
                use bitcoin::hashes::Hash;

                let outpoints: Vec<OutPoint> = (0u8..)
                    .take(sighashes.len())
                    .map(|txid_byte| OutPoint {
                        txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                        vout: 0,
                    })
                    .collect();
                let inputs = outpoints
                    .iter()
                    .zip(&sighashes)
                    .map(|(outpoint, sighash)| {
                        let mut input = psbt_v2::v2::Input::new(outpoint);
                        input.sighash_type = sighash.map(psbt_v2::PsbtSighashType::from_u32);
                        input
                    })
                    .collect();
                let first_not_all = outpoints
                    .iter()
                    .zip(&sighashes)
                    .find(|(_, sighash)| sighash.is_some_and(|sighash| sighash != 0x01))
                    .map(|(outpoint, _)| *outpoint);

                let psbt = v2_psbt(inputs, vec![]);

                let result = ResultUnorderedPsbt::try_from_psbt(psbt.clone());
                let clean = UnorderedPsbt::try_from_psbt(psbt);
                match first_not_all {
                    Some(op) => {
                        let error = UnorderedPsbtError::UnorderedInputSighashNotAll(op);
                        prop_assert!(error.to_string().contains(&op.to_string()));
                        prop_assert_eq!(result, Err(error.clone()));
                        prop_assert_eq!(clean, Err(error));
                    }
                    None => {
                        prop_assert_eq!(clean.map(|u| u.inputs.len()), Ok(sighashes.len()));
                    }
                }
            }

            #[test]
            fn try_from_psbt_rejects_repeated_unique_ids(
                uid_bytes in proptest::collection::vec(0u8..3, 0..=4),
            ) {
                let outputs = uid_bytes.iter().map(|byte| v2_output_with_uid(vec![*byte; 16])).collect();
                let mut seen = std::collections::HashSet::new();
                let first_repeat = uid_bytes.iter().find(|byte| !seen.insert(**byte));

                let psbt = v2_psbt(vec![], outputs);

                let result = ResultUnorderedPsbt::try_from_psbt(psbt.clone());
                let clean = UnorderedPsbt::try_from_psbt(psbt);
                match first_repeat {
                    // Rejected even when the duplicates are identical.
                    Some(byte) => {
                        let error = UnorderedPsbtError::DuplicateOutputUniqueId(UniqueId::try_from(vec![*byte; 16]).unwrap());
                        prop_assert_eq!(result, Err(error.clone()));
                        prop_assert_eq!(clean, Err(error));
                    }
                    None => {
                        prop_assert_eq!(clean.map(|u| u.outputs.len()), Ok(uid_bytes.len()));
                    }
                }
            }
        }
    }
}
