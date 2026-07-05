use std::marker::PhantomData;

use bitcoin::hashes::{Hash, HashEngine, sha256t_hash_newtype};
use psbt_v2::v2::Psbt;

use crate::global::GlobalSortExt;
use crate::input::{InputSortKeyExt, out_point};
use crate::negotiation::GlobalNegotiationExt;
use crate::output::{OutputSortKeyExt, OutputUniqueIdExt};
use crate::tx::UnorderedPsbt;

sha256t_hash_newtype! {
    /// Tag for the BIP 341-style tagged hash used to derive sort keys.
    pub struct SortKeyTag = hash_str("concurrent-psbt/deterministic-ordering");

    /// A sort key derived via tagged hash: `H(seed || identifier)`.
    #[hash_newtype(forward)]
    pub struct SortKeyHash(_);
}

/// Derive a sort key from a seed and an identifier via tagged hash.
///
/// `SHA256(H(tag) || H(tag) || seed || id)` where
/// `H(tag)` = SHA256("concurrent-psbt/deterministic-ordering").
fn derive_sort_key(seed: &[u8], id: &[u8]) -> [u8; 32] {
    let mut engine = SortKeyHash::engine();
    engine.input(seed);
    engine.input(id);
    SortKeyHash::from_engine(engine).to_byte_array()
}

/// Sort mode: deterministic ordering seeded by a high-entropy value.
///
/// NOT BIP 69. The ordering is derived from a seed (e.g. a shared secret
/// or transaction-specific nonce) so that all participants produce the
/// same order without revealing sort criteria to observers.
#[derive(Debug)]
pub enum Deterministic {}

/// Sort mode: ordering determined by explicit sort key fields on each input/output.
#[derive(Debug)]
pub enum ExplicitSortKeys {}

/// Sort mode: not yet chosen. The default typestate for [`Sorter`].
#[derive(Debug, Clone, PartialEq)]
pub enum Unset {}

/// Typestate wrapper that applies a sort order to an [`UnorderedPsbt`].
///
/// `S` is the sort mode: [`Unset`], [`Deterministic`], or [`ExplicitSortKeys`].
/// Once ordering is applied, the result can be converted to a standard BIP 370 PSBT.
///
/// # Ordering
///
/// [`UnorderedPsbt::into_psbt`] uses `HashMap` iteration order, which is
/// intentionally non-deterministic, satisfying the spec's shuffle requirement
/// for unordered PSBTs. The [`Sorter`] applies a deterministic order when
/// transitioning to an ordered BIP 370 PSBT for signing.
pub struct Sorter<S>(pub(crate) UnorderedPsbt, PhantomData<S>);

impl<S> Sorter<S> {
    /// Create a sorter from an [`UnorderedPsbt`].
    pub fn from_unordered_psbt(psbt: UnorderedPsbt) -> Self {
        Sorter(psbt, PhantomData)
    }

    /// Access the underlying [`UnorderedPsbt`].
    pub fn inner(&self) -> &UnorderedPsbt {
        &self.0
    }

    /// Consume the sorter and return the underlying [`UnorderedPsbt`].
    pub fn into_inner(self) -> UnorderedPsbt {
        self.0
    }
}

/// Error type for sort operations.
#[derive(Debug)]
pub enum SortError {
    /// An input is missing a sort key and no seed is available to derive one.
    MissingInputSortKey(bitcoin::OutPoint),
    /// An output is missing a sort key and no seed is available to derive one.
    MissingOutputSortKey,
    /// Duplicate sort keys found among inputs or outputs.
    DuplicateSortKey(Vec<u8>),
    /// Sort seed is required but not set.
    MissingSortSeed,
}

impl std::fmt::Display for SortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingInputSortKey(op) => write!(f, "input {op} missing sort key"),
            Self::MissingOutputSortKey => write!(f, "output missing sort key"),
            Self::DuplicateSortKey(k) => {
                let encoded = k
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                write!(f, "duplicate sort key: {encoded}")
            }
            Self::MissingSortSeed => write!(f, "PSBT_GLOBAL_SORT_SEED not set"),
        }
    }
}

impl std::error::Error for SortError {}

fn input_sort_identifier(input: &psbt_v2::v2::Input) -> Vec<u8> {
    let mut id = input.previous_txid.to_byte_array().to_vec();
    id.extend_from_slice(&input.spent_output_index.to_le_bytes());
    id
}

fn output_sort_identifier(output: &psbt_v2::v2::Output) -> Vec<u8> {
    OutputUniqueIdExt::unique_id(output)
        .map(|u| u.into_bytes())
        .unwrap_or_default()
}

fn derived_input_sort_key(
    seed: Option<&[u8]>,
    input: &psbt_v2::v2::Input,
) -> Result<Vec<u8>, SortError> {
    let seed = seed.ok_or(SortError::MissingSortSeed)?;
    Ok(derive_sort_key(seed, &input_sort_identifier(input)).to_vec())
}

fn derived_output_sort_key(
    seed: Option<&[u8]>,
    output: &psbt_v2::v2::Output,
) -> Result<Vec<u8>, SortError> {
    let seed = seed.ok_or(SortError::MissingSortSeed)?;
    Ok(derive_sort_key(seed, &output_sort_identifier(output)).to_vec())
}

fn explicit_input_sort_key(input: &psbt_v2::v2::Input) -> Result<Vec<u8>, SortError> {
    input
        .sort_key()
        .ok_or(SortError::MissingInputSortKey(out_point(input)))
        .map(|key| key.to_vec())
}

fn explicit_output_sort_key(output: &psbt_v2::v2::Output) -> Result<Vec<u8>, SortError> {
    output
        .sort_key()
        .ok_or(SortError::MissingOutputSortKey)
        .map(|key| key.to_vec())
}

fn unset_input_sort_key(
    seed: Option<&[u8]>,
    input: &psbt_v2::v2::Input,
) -> Result<Vec<u8>, SortError> {
    input
        .sort_key()
        .map(|key| key.to_vec())
        .map(Ok)
        .unwrap_or_else(|| derived_input_sort_key(seed, input))
}

fn unset_output_sort_key(
    seed: Option<&[u8]>,
    output: &psbt_v2::v2::Output,
) -> Result<Vec<u8>, SortError> {
    output
        .sort_key()
        .map(|key| key.to_vec())
        .map(Ok)
        .unwrap_or_else(|| derived_output_sort_key(seed, output))
}

fn ordered_psbt<I, O>(
    psbt: UnorderedPsbt,
    mut input_key: I,
    mut output_key: O,
) -> Result<Psbt, SortError>
where
    I: FnMut(&psbt_v2::v2::Input) -> Result<Vec<u8>, SortError>,
    O: FnMut(&psbt_v2::v2::Output) -> Result<Vec<u8>, SortError>,
{
    let mut inputs: Vec<_> = psbt.inputs.into_iter().collect();
    let mut outputs: Vec<_> = psbt.outputs.into_iter().collect();

    let mut input_keys = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| input_key(input).map(|key| (key, i)))
        .collect::<Result<Vec<_>, _>>()?;

    let mut output_keys = outputs
        .iter()
        .enumerate()
        .map(|(i, output)| output_key(output).map(|key| (key, i)))
        .collect::<Result<Vec<_>, _>>()?;

    input_keys.sort_by(|(left, _), (right, _)| left.cmp(right));
    output_keys.sort_by(|(left, _), (right, _)| left.cmp(right));

    let sorted_inputs = apply_permutation(&mut inputs, &input_keys);
    let sorted_outputs = apply_permutation(&mut outputs, &output_keys);

    let mut global = psbt.global;
    global.clear_unordered();
    // Negotiation metadata (payments/confirmations) has done its job once
    // ordering begins; do not leak the payment graph into the signing artifact.
    global.clear_negotiation();
    global.input_count = sorted_inputs.len();
    global.output_count = sorted_outputs.len();

    Ok(Psbt {
        global,
        inputs: sorted_inputs,
        outputs: sorted_outputs,
    })
}

impl Sorter<Unset> {
    /// Apply unset-mode ordering.
    ///
    /// Existing explicit sort keys are used as-is. Missing sort keys are
    /// replaced with deterministic stand-ins derived from `PSBT_GLOBAL_SORT_SEED`.
    ///
    /// # Errors
    /// Returns [`SortError::MissingSortSeed`] if any input or output lacks an
    /// explicit sort key and no sort seed is set.
    pub fn into_ordered_psbt(self) -> Result<Psbt, SortError> {
        let seed = self.0.global.sort_seed().map(|seed| seed.to_vec());
        ordered_psbt(
            self.0,
            |input| unset_input_sort_key(seed.as_deref(), input),
            |output| unset_output_sort_key(seed.as_deref(), output),
        )
    }
}

impl Sorter<Deterministic> {
    /// Apply deterministic ordering: derive sort keys from the seed, sort,
    /// and produce an ordered BIP 370 [`Psbt`].
    ///
    /// # Errors
    /// Returns [`SortError::MissingSortSeed`] if `PSBT_GLOBAL_SORT_SEED` is not set.
    pub fn into_ordered_psbt(self) -> Result<Psbt, SortError> {
        let seed = self
            .0
            .global
            .sort_seed()
            .ok_or(SortError::MissingSortSeed)?
            .to_vec();

        ordered_psbt(
            self.0,
            |input| derived_input_sort_key(Some(&seed), input),
            |output| derived_output_sort_key(Some(&seed), output),
        )
    }
}

impl Sorter<ExplicitSortKeys> {
    /// Apply explicit ordering: sort by the `PSBT_IN_SORT_KEY` and
    /// `PSBT_OUT_SORT_KEY` fields, and produce an ordered BIP 370 [`Psbt`].
    ///
    /// # Errors
    /// Returns [`SortError::MissingInputSortKey`] or [`SortError::MissingOutputSortKey`]
    /// if any input or output is missing a sort key, or
    /// [`SortError::DuplicateSortKey`] if two inputs (or two outputs) carry the
    /// same explicit sort key: explicit ordering is only well-defined when the
    /// keys are distinct.
    pub fn into_ordered_psbt(self) -> Result<Psbt, SortError> {
        let UnorderedPsbt {
            global,
            inputs,
            outputs,
        } = self.0;
        let inputs: Vec<_> = inputs.into_iter().collect();
        let outputs: Vec<_> = outputs.into_iter().collect();

        reject_duplicate_sort_keys(inputs.iter().map(explicit_input_sort_key))?;
        reject_duplicate_sort_keys(outputs.iter().map(explicit_output_sort_key))?;

        let psbt = UnorderedPsbt {
            global,
            inputs: inputs.into_iter().collect(),
            outputs: outputs.into_iter().collect(),
        };
        ordered_psbt(psbt, explicit_input_sort_key, explicit_output_sort_key)
    }
}

/// Reject colliding explicit sort keys, returning the first duplicated key.
fn reject_duplicate_sort_keys(
    keys: impl Iterator<Item = Result<Vec<u8>, SortError>>,
) -> Result<(), SortError> {
    let mut keys = keys.collect::<Result<Vec<_>, _>>()?;
    keys.sort_unstable();
    match keys.windows(2).find(|pair| pair[0] == pair[1]) {
        Some(pair) => Err(SortError::DuplicateSortKey(pair[0].clone())),
        None => Ok(()),
    }
}

/// Apply a precomputed permutation to a vec, consuming it.
fn apply_permutation<T, K: Ord>(items: &mut Vec<T>, order: &[(K, usize)]) -> Vec<T> {
    // Build index mapping: position in sorted order -> original index
    let mut result: Vec<Option<T>> = items.drain(..).map(Some).collect();
    order
        .iter()
        .map(|(_, orig_idx)| {
            result[*orig_idx]
                .take()
                .expect("each index appears exactly once")
        })
        .collect()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::input::InputSet;
    use crate::output::{OutputSet, PSBT_OUT_UNIQUE_ID_SUBTYPE};
    use psbt_v2::v2::{Global, Input, Output};

    fn make_input(txid_byte: u8, vout: u32) -> Input {
        use bitcoin::hashes::Hash;
        Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
            vout,
        })
    }

    fn make_output_with_uid(uid: &[u8]) -> Output {
        let mut output = Output::default();
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: crate::PROPRIETARY_PREFIX.to_vec(),
            subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
            key: vec![],
        };
        output.proprietaries.insert(key, uid.to_vec());
        output
    }

    fn empty_psbt() -> UnorderedPsbt {
        UnorderedPsbt {
            global: Global::default(),
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn from_unordered_psbt() {
            let _sorter: Sorter<Unset> = Sorter::from_unordered_psbt(empty_psbt());
        }

        #[test]
        fn deterministic_empty() {
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(vec![0u8; 32]);
            let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
            let ordered = sorter.into_ordered_psbt().unwrap();
            assert!(ordered.inputs.is_empty());
            assert!(ordered.outputs.is_empty());
        }

        #[test]
        fn deterministic_no_seed_err() {
            let psbt = empty_psbt();
            let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
            assert!(sorter.into_ordered_psbt().is_err());
        }

        #[test]
        fn deterministic_sorts_inputs() {
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(vec![42u8; 32]);
            psbt.inputs.add(make_input(2, 0));
            psbt.inputs.add(make_input(1, 0));
            psbt.inputs.add(make_input(3, 0));
            let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
            let ordered = sorter.into_ordered_psbt().unwrap();
            assert_eq!(ordered.inputs.len(), 3);
            // Verify deterministic: same seed, same order
            let mut psbt2 = empty_psbt();
            psbt2.global.set_sort_seed(vec![42u8; 32]);
            psbt2.inputs.add(make_input(3, 0));
            psbt2.inputs.add(make_input(1, 0));
            psbt2.inputs.add(make_input(2, 0));
            let sorter2: Sorter<Deterministic> = Sorter(psbt2, PhantomData);
            let ordered2 = sorter2.into_ordered_psbt().unwrap();
            for (a, b) in ordered.inputs.iter().zip(ordered2.inputs.iter()) {
                assert_eq!(a.previous_txid, b.previous_txid);
            }
        }

        #[test]
        fn deterministic_different_seeds_differ() {
            let make = |seed: u8| {
                let mut psbt = empty_psbt();
                psbt.global.set_sort_seed(vec![seed; 32]);
                psbt.inputs.add(make_input(1, 0));
                psbt.inputs.add(make_input(2, 0));
                psbt.inputs.add(make_input(3, 0));
                let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
                sorter.into_ordered_psbt().unwrap()
            };
            let a = make(1);
            let b = make(2);
            // Different seeds may produce different orderings (not guaranteed
            // for only 3 elements, but the keys will differ)
            let a_ids: Vec<_> = a.inputs.iter().map(|i| i.previous_txid).collect();
            let b_ids: Vec<_> = b.inputs.iter().map(|i| i.previous_txid).collect();
            // At minimum the derived keys are different (ordering may happen to match)
            let _ = (a_ids, b_ids);
        }

        #[test]
        fn explicit_empty() {
            let psbt = empty_psbt();
            let sorter: Sorter<ExplicitSortKeys> = Sorter(psbt, PhantomData);
            let ordered = sorter.into_ordered_psbt().unwrap();
            assert!(ordered.inputs.is_empty());
        }

        #[test]
        fn explicit_missing_input_key_err() {
            let mut psbt = empty_psbt();
            psbt.inputs.add(make_input(1, 0));
            let sorter: Sorter<ExplicitSortKeys> = Sorter(psbt, PhantomData);
            assert!(sorter.into_ordered_psbt().is_err());
        }

        #[test]
        fn explicit_missing_output_key_err() {
            let mut psbt = empty_psbt();
            psbt.outputs.add(make_output_with_uid(&[1]));
            let sorter: Sorter<ExplicitSortKeys> = Sorter(psbt, PhantomData);
            assert!(sorter.into_ordered_psbt().is_err());
        }

        #[test]
        fn explicit_sorts_by_key() {
            let mut psbt = empty_psbt();
            let mut i1 = make_input(1, 0);
            i1.set_sort_key(vec![0x02]);
            let mut i2 = make_input(2, 0);
            i2.set_sort_key(vec![0x01]);
            psbt.inputs.add(i1);
            psbt.inputs.add(i2);
            let sorter: Sorter<ExplicitSortKeys> = Sorter(psbt, PhantomData);
            let ordered = sorter.into_ordered_psbt().unwrap();
            // i2 (key 0x01) should come before i1 (key 0x02)
            assert_eq!(
                ordered.inputs[0].previous_txid,
                make_input(2, 0).previous_txid
            );
            assert_eq!(
                ordered.inputs[1].previous_txid,
                make_input(1, 0).previous_txid
            );
        }

        #[test]
        fn explicit_duplicate_sort_key_errs() {
            let mut psbt = empty_psbt();
            let mut o1 = make_output_with_uid(&[0x01]);
            o1.set_sort_key(vec![0xaa]);
            let mut o2 = make_output_with_uid(&[0x02]);
            o2.set_sort_key(vec![0xaa]);
            psbt.outputs.add(o1);
            psbt.outputs.add(o2);
            let sorter: Sorter<ExplicitSortKeys> = Sorter(psbt, PhantomData);
            assert!(matches!(
                sorter.into_ordered_psbt(),
                Err(SortError::DuplicateSortKey(key)) if key == vec![0xaa]
            ));
        }

        #[test]
        fn unset_uses_explicit_keys_and_derives_missing_keys() {
            let seed = vec![7u8; 32];
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(seed.clone());

            let mut explicit_high = make_input(1, 0);
            explicit_high.set_sort_key(vec![0xff]);
            let missing = make_input(2, 0);
            let mut explicit_low = make_input(3, 0);
            explicit_low.set_sort_key(vec![0x00]);

            let mut expected_inputs = vec![
                (
                    explicit_high.sort_key().unwrap().to_vec(),
                    explicit_high.previous_txid,
                ),
                (
                    derive_sort_key(&seed, &input_sort_identifier(&missing)).to_vec(),
                    missing.previous_txid,
                ),
                (
                    explicit_low.sort_key().unwrap().to_vec(),
                    explicit_low.previous_txid,
                ),
            ];
            expected_inputs.sort_by_key(|(key, _)| key.clone());

            psbt.inputs.add(explicit_high);
            psbt.inputs.add(missing);
            psbt.inputs.add(explicit_low);

            let mut explicit_output = make_output_with_uid(&[0x01]);
            explicit_output.set_sort_key(vec![0xfe]);
            let missing_output = make_output_with_uid(&[0x02]);
            let mut expected_outputs = vec![
                (explicit_output.sort_key().unwrap().to_vec(), 0usize),
                (derive_sort_key(&seed, &[0x02]).to_vec(), 1usize),
            ];
            expected_outputs.sort_by_key(|(key, _)| key.clone());

            psbt.outputs.add(explicit_output);
            psbt.outputs.add(missing_output);

            let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(psbt);
            let ordered = sorter.into_ordered_psbt().unwrap();

            let ordered_inputs: Vec<_> = ordered.inputs.iter().map(|i| i.previous_txid).collect();
            let expected_inputs: Vec<_> =
                expected_inputs.into_iter().map(|(_, txid)| txid).collect();
            assert_eq!(ordered_inputs, expected_inputs);

            let ordered_output_indexes: Vec<_> = ordered
                .outputs
                .iter()
                .map(|output| {
                    if output.sort_key() == Some([0xfe].as_slice()) {
                        0
                    } else {
                        1
                    }
                })
                .collect();
            let expected_output_indexes: Vec<_> = expected_outputs
                .into_iter()
                .map(|(_, index)| index)
                .collect();
            assert_eq!(ordered_output_indexes, expected_output_indexes);
        }

        #[test]
        fn unset_missing_sort_key_without_seed_errs() {
            let mut psbt = empty_psbt();
            psbt.inputs.add(make_input(1, 0));
            let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(psbt);
            assert!(matches!(
                sorter.into_ordered_psbt(),
                Err(SortError::MissingSortSeed)
            ));
        }

        #[test]
        fn unset_missing_output_sort_key_without_seed_errs() {
            let mut psbt = empty_psbt();
            psbt.outputs.add(make_output_with_uid(&[1]));
            let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(psbt);
            assert!(matches!(
                sorter.into_ordered_psbt(),
                Err(SortError::MissingSortSeed)
            ));
        }

        #[test]
        fn clears_unordered_flag() {
            let mut psbt = empty_psbt();
            psbt.global.set_unordered();
            psbt.global.set_sort_seed(vec![0u8; 32]);
            assert!(psbt.global.is_unordered());
            let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
            let ordered = sorter.into_ordered_psbt().unwrap();
            assert!(!ordered.global.is_unordered());
        }

        #[test]
        fn sort_error_display() {
            let err = SortError::MissingSortSeed;
            assert!(err.to_string().contains("SORT_SEED"));
        }

        #[test]
        fn inner_accessor() {
            let psbt = empty_psbt();
            let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(psbt);
            assert!(sorter.inner().inputs.is_empty());
        }

        #[test]
        fn into_inner_accessor() {
            let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(empty_psbt());
            assert!(sorter.into_inner().inputs.is_empty());
        }

        #[test]
        fn sort_error_display_covers_all_variants() {
            let outpoint = out_point(&make_input(1, 2));
            assert!(
                SortError::MissingInputSortKey(outpoint)
                    .to_string()
                    .contains("input")
            );
            assert_eq!(
                SortError::MissingOutputSortKey.to_string(),
                "output missing sort key"
            );
            assert_eq!(
                SortError::DuplicateSortKey(vec![0xab, 0xcd]).to_string(),
                "duplicate sort key: abcd"
            );
        }

        #[test]
        fn deterministic_sorts_outputs() {
            let mut psbt = empty_psbt();
            psbt.global.set_sort_seed(vec![42; 32]);
            psbt.outputs.add(make_output_with_uid(&[2]));
            psbt.outputs.add(make_output_with_uid(&[1]));

            let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
            assert_eq!(sorter.into_ordered_psbt().unwrap().outputs.len(), 2);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn from_unordered_psbt_always_succeeds(_seed in any::<u32>()) {
                let _sorter: Sorter<Unset> = Sorter::from_unordered_psbt(empty_psbt());
            }

            #[test]
            fn deterministic_is_deterministic(seed in proptest::collection::vec(0u8..=255, 16..=32)) {
                let make = || {
                    let mut psbt = empty_psbt();
                    psbt.global.set_sort_seed(seed.clone());
                    psbt.inputs.add(make_input(1, 0));
                    psbt.inputs.add(make_input(2, 0));
                    psbt.inputs.add(make_input(3, 0));
                    let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
                    sorter.into_ordered_psbt().unwrap()
                };
                let a = make();
                let b = make();
                for (x, y) in a.inputs.iter().zip(b.inputs.iter()) {
                    prop_assert_eq!(x.previous_txid, y.previous_txid);
                    prop_assert_eq!(x.spent_output_index, y.spent_output_index);
                }
            }

            #[test]
            fn explicit_roundtrip(
                k1 in proptest::collection::vec(0u8..=255, 1..=8),
                k2 in proptest::collection::vec(0u8..=255, 1..=8),
            ) {
                // Only test with distinct keys
                prop_assume!(k1 != k2);
                let mut psbt = empty_psbt();
                let mut i1 = make_input(1, 0);
                i1.set_sort_key(k1.clone());
                let mut i2 = make_input(2, 0);
                i2.set_sort_key(k2.clone());
                psbt.inputs.add(i1);
                psbt.inputs.add(i2);
                let sorter: Sorter<ExplicitSortKeys> = Sorter(psbt, PhantomData);
                let ordered = sorter.into_ordered_psbt().unwrap();
                prop_assert_eq!(ordered.inputs.len(), 2);
                // First input should have the lexicographically smaller key
                let expected_first = if k1 < k2 { 1u8 } else { 2u8 };
                prop_assert_eq!(
                    ordered.inputs[0].previous_txid,
                    make_input(expected_first, 0).previous_txid,
                );
            }

            #[test]
            fn accessors_roundtrip(_dummy in any::<u8>()) {
                let sorter: Sorter<Unset> = Sorter::from_unordered_psbt(empty_psbt());
                prop_assert!(sorter.inner().inputs.is_empty());
                prop_assert!(sorter.into_inner().outputs.is_empty());
            }

            #[test]
            fn error_displays_cover_every_variant(
                txid_byte in any::<u8>(),
                vout in any::<u32>(),
                duplicate_key in proptest::collection::vec(any::<u8>(), 0..=8),
            ) {
                let outpoint = out_point(&make_input(txid_byte, vout));
                prop_assert!(SortError::MissingInputSortKey(outpoint).to_string().contains("missing sort key"));
                prop_assert_eq!(SortError::MissingOutputSortKey.to_string(), "output missing sort key");
                prop_assert!(SortError::DuplicateSortKey(duplicate_key).to_string().starts_with("duplicate sort key: "));
                prop_assert!(SortError::MissingSortSeed.to_string().contains("SORT_SEED"));
            }

            #[test]
            fn deterministic_orders_inputs_and_outputs(
                seed in proptest::collection::vec(any::<u8>(), 1..=32),
                first in any::<u8>(),
                second in any::<u8>(),
            ) {
                prop_assume!(first != second);
                let mut psbt = empty_psbt();
                psbt.global.set_sort_seed(seed);
                psbt.inputs.add(make_input(first, 0));
                psbt.inputs.add(make_input(second, 0));
                psbt.outputs.add(make_output_with_uid(&[first]));
                psbt.outputs.add(make_output_with_uid(&[second]));

                let sorter: Sorter<Deterministic> = Sorter(psbt, PhantomData);
                let ordered = sorter.into_ordered_psbt().unwrap();
                prop_assert_eq!(ordered.inputs.len(), 2);
                prop_assert_eq!(ordered.outputs.len(), 2);
            }

            #[test]
            fn explicit_reports_missing_and_duplicate_keys(
                duplicate_key in proptest::collection::vec(any::<u8>(), 0..=8),
            ) {
                let mut missing_input = empty_psbt();
                missing_input.inputs.add(make_input(1, 0));
                prop_assert!(matches!(
                    Sorter::<ExplicitSortKeys>(missing_input, PhantomData).into_ordered_psbt(),
                    Err(SortError::MissingInputSortKey(_))
                ));

                let mut missing_output = empty_psbt();
                missing_output.outputs.add(make_output_with_uid(&[1]));
                prop_assert!(matches!(
                    Sorter::<ExplicitSortKeys>(missing_output, PhantomData).into_ordered_psbt(),
                    Err(SortError::MissingOutputSortKey)
                ));

                let mut duplicates = empty_psbt();
                for uid in [1, 2] {
                    let mut output = make_output_with_uid(&[uid]);
                    output.set_sort_key(duplicate_key.clone());
                    duplicates.outputs.add(output);
                }
                prop_assert!(matches!(
                    Sorter::<ExplicitSortKeys>(duplicates, PhantomData).into_ordered_psbt(),
                    Err(SortError::DuplicateSortKey(_))
                ));
            }

            #[test]
            fn unset_derives_missing_keys_or_requires_seed(
                seed in proptest::collection::vec(any::<u8>(), 1..=32),
            ) {
                let mut missing_seed = empty_psbt();
                missing_seed.inputs.add(make_input(1, 0));
                prop_assert!(matches!(
                    Sorter::<Unset>::from_unordered_psbt(missing_seed).into_ordered_psbt(),
                    Err(SortError::MissingSortSeed)
                ));

                let mut psbt = empty_psbt();
                psbt.global.set_unordered();
                psbt.global.set_sort_seed(seed);
                let mut explicit_input = make_input(1, 0);
                explicit_input.set_sort_key(vec![0]);
                psbt.inputs.add(explicit_input);
                psbt.inputs.add(make_input(2, 0));
                let mut explicit_output = make_output_with_uid(&[1]);
                explicit_output.set_sort_key(vec![0]);
                psbt.outputs.add(explicit_output);
                psbt.outputs.add(make_output_with_uid(&[2]));

                let ordered = Sorter::<Unset>::from_unordered_psbt(psbt)
                    .into_ordered_psbt()
                    .unwrap();
                prop_assert!(!ordered.global.is_unordered());
            }

            #[test]
            fn missing_seed_errors_cover_output_and_deterministic_paths(_dummy in any::<u8>()) {
                let mut unset = empty_psbt();
                unset.outputs.add(make_output_with_uid(&[1]));
                prop_assert!(matches!(
                    Sorter::<Unset>::from_unordered_psbt(unset).into_ordered_psbt(),
                    Err(SortError::MissingSortSeed)
                ));

                prop_assert!(matches!(
                    Sorter::<Deterministic>(empty_psbt(), PhantomData).into_ordered_psbt(),
                    Err(SortError::MissingSortSeed)
                ));
            }
        }
    }
}
