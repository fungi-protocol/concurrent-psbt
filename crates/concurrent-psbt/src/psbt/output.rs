#![allow(clippy::result_large_err)]

//! Per-output field type and output set merging.
//!
//! [`ResultOutput`] wraps each per-output field in the internal result domain.
//! [`OutputSet`] and [`ResultOutputSet`] are sets of outputs identified by their
//! [`UniqueId`]. Joining two sets joins the outputs that have the same unique ID
//! field by field, and fields that differ become conflicts.

use std::collections::BTreeMap;

use bitcoin::bip32::KeySource;
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::taproot::TapTree;
use bitcoin::{Amount, ScriptBuf};
use psbt_v2::raw;

use super::input::{InvalidSortKey, SortKey, SortKeyField};
use super::proprietary::ProprietaryField;
use super::tap_leaf_hashes::{TapKeyOrigins, TapLeafHashes};

joinable_struct! {
    /// A PSBT output, mirroring the BIP 370 per-output map, identified by its
    /// [`UniqueId`].
    ///
    /// The unique ID is set when the output is created with [`Output::new`]
    /// or converted from a BIP 370 output carrying `PSBT_OUT_UNIQUE_ID`, and
    /// cannot change afterwards.
    source: Output,
    /// Result-domain counterpart of [`Output`].
    ///
    /// Each field holds a clean value or a conflict.
    result: ResultOutput,
    v2: psbt_v2::v2::Output,
    fields: {
        pub amount: Amount,
        pub script_pubkey: ScriptBuf,
        pub redeem_script: Option<ScriptBuf>,
        pub witness_script: Option<ScriptBuf>,
        pub bip32_derivations: BTreeMap<PublicKey, KeySource>,
        pub tap_internal_key: Option<XOnlyPublicKey>,
        pub tap_tree: Option<TapTree>,
        pub tap_key_origins: BTreeMap<XOnlyPublicKey, (TapLeafHashes, KeySource)> = TapKeyOrigins,
        pub proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        pub unknowns: BTreeMap<raw::Key, Vec<u8>>,
    },
    proprietary: {
        unique_id: UniqueId = UniqueIdField,
        pub(crate) sort_key: Option<SortKey> = SortKeyField<PSBT_OUT_SORT_KEY_SUBTYPE>,
    },
}

impl Output {
    /// Create an output paying `amount` to `script_pubkey`, identified by
    /// `unique_id`, with every other field unset.
    pub fn new(unique_id: UniqueId, amount: Amount, script_pubkey: ScriptBuf) -> Self {
        Self::from_v2_fields(
            psbt_v2::v2::Output {
                amount,
                script_pubkey,
                ..psbt_v2::v2::Output::default()
            },
            unique_id,
            None,
        )
    }

    /// The [`UniqueId`] identifying this output.
    pub fn unique_id(&self) -> &UniqueId {
        &self.unique_id
    }

    /// Get the sort key, if set.
    pub fn sort_key(&self) -> Option<&[u8]> {
        self.sort_key.as_ref().map(SortKey::as_bytes)
    }

    /// Set the sort key.
    pub fn set_sort_key(&mut self, sort_key: SortKey) {
        self.sort_key = Some(sort_key);
    }
}

/// Why a BIP 370 output cannot be converted into an [`Output`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidOutput {
    /// It has no `PSBT_OUT_UNIQUE_ID`.
    MissingUniqueId,
    /// Its `PSBT_OUT_UNIQUE_ID` is empty or longer than [`UniqueId::MAX_LEN`]
    /// bytes.
    InvalidUniqueId(InvalidUniqueId),
    /// Its `PSBT_OUT_SORT_KEY` is longer than [`SortKey::MAX_LEN`] bytes.
    SortKeyTooLong(InvalidSortKey),
}

impl std::fmt::Display for InvalidOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingUniqueId => write!(f, "output missing PSBT_OUT_UNIQUE_ID"),
            Self::InvalidUniqueId(error) => write!(f, "output {error}"),
            Self::SortKeyTooLong(error) => write!(f, "output {error}"),
        }
    }
}

impl std::error::Error for InvalidOutput {}

impl From<InvalidSortKey> for InvalidOutput {
    fn from(error: InvalidSortKey) -> Self {
        Self::SortKeyTooLong(error)
    }
}

/// Reads the unique ID and sort key out of `PSBT_OUT_UNIQUE_ID` and
/// `PSBT_OUT_SORT_KEY`.
///
/// # Errors
/// Returns the first of [`InvalidOutput`]'s variants that applies.
impl TryFrom<psbt_v2::v2::Output> for Output {
    type Error = InvalidOutput;

    fn try_from(output: psbt_v2::v2::Output) -> Result<Self, InvalidOutput> {
        Self::decode_v2(output)
    }
}

/// Writes the unique ID and sort key to `PSBT_OUT_UNIQUE_ID` and
/// `PSBT_OUT_SORT_KEY`.
impl From<Output> for psbt_v2::v2::Output {
    fn from(output: Output) -> Self {
        output.encode_v2(psbt_v2::v2::Output::default())
    }
}

/// Opaque byte-vector identifier for a PSBT output.
///
/// Stored in the `PSBT_OUT_UNIQUE_ID` proprietary field, subtype
/// [`PSBT_OUT_UNIQUE_ID_SUBTYPE`]. Identifies an output within an
/// [`OutputSet`].
///
/// A unique ID built with [`TryFrom`] or read from a BIP 370 output is 1 to
/// [`UniqueId::MAX_LEN`] bytes long. An empty ID would make unrelated outputs
/// join as one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UniqueId(Vec<u8>);

impl UniqueId {
    /// The maximum length of a unique ID in bytes.
    pub const MAX_LEN: usize = 32;

    /// Create a `UniqueId` from raw bytes without checking their length.
    #[cfg(test)]
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Access the raw bytes of this unique identifier.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consume and return the raw bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Generate a fresh random 16-byte unique identifier.
    ///
    /// The bytes come straight from the operating system's RNG rather than a
    /// userspace generator such as `ThreadRng`, whose state a forked child
    /// would share with its parent. Two calls therefore collide only with
    /// negligible probability, so copies of an otherwise identical txout can
    /// coexist as separate outputs.
    ///
    /// # Panics
    /// Panics if the operating system's RNG is unavailable.
    pub fn generate() -> Self {
        use rand::{RngCore, TryRngCore, rngs::OsRng};

        let mut bytes = [0u8; 16];
        OsRng.unwrap_err().fill_bytes(&mut bytes);
        Self(bytes.to_vec())
    }
}

impl crate::values::IdempotentValue for UniqueId {}

/// Bytes that are not a valid [`UniqueId`]: empty, or longer than
/// [`UniqueId::MAX_LEN`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidUniqueId(pub Vec<u8>);

impl TryFrom<Vec<u8>> for UniqueId {
    type Error = InvalidUniqueId;

    fn try_from(bytes: Vec<u8>) -> Result<Self, InvalidUniqueId> {
        if bytes.is_empty() || bytes.len() > Self::MAX_LEN {
            return Err(InvalidUniqueId(bytes));
        }
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for InvalidUniqueId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unique ID is {} bytes, not 1 to {}",
            self.0.len(),
            UniqueId::MAX_LEN
        )
    }
}

impl std::error::Error for InvalidUniqueId {}

pub const PSBT_OUT_UNIQUE_ID_SUBTYPE: u8 = 0x01;

/// Stores an output's unique ID in `PSBT_OUT_UNIQUE_ID`.
struct UniqueIdField;

impl ProprietaryField for UniqueIdField {
    const SUBTYPE: u8 = PSBT_OUT_UNIQUE_ID_SUBTYPE;
    type Value = UniqueId;
    type Error = InvalidOutput;

    fn decode(value: Option<Vec<u8>>) -> Result<UniqueId, InvalidOutput> {
        let bytes = value.ok_or(InvalidOutput::MissingUniqueId)?;
        UniqueId::try_from(bytes).map_err(InvalidOutput::InvalidUniqueId)
    }

    fn encode(unique_id: UniqueId) -> Option<Vec<u8>> {
        Some(unique_id.into_bytes())
    }
}

/// Sets `PSBT_OUT_UNIQUE_ID` on BIP 370 outputs, for tests that parse them.
#[cfg(test)]
pub(crate) trait OutputUniqueIdExt {
    /// Store `id` in the `PSBT_OUT_UNIQUE_ID` proprietary field, replacing
    /// any existing value.
    fn set_unique_id(&mut self, id: UniqueId);
}

#[cfg(test)]
impl OutputUniqueIdExt for psbt_v2::v2::Output {
    fn set_unique_id(&mut self, id: UniqueId) {
        self.proprietaries.insert(
            super::proprietary::key(PSBT_OUT_UNIQUE_ID_SUBTYPE),
            id.into_bytes(),
        );
    }
}

/// Subtype for `PSBT_OUT_SORT_KEY`.
///
/// Up to [`SortKey::MAX_LEN`] bytes of arbitrary data used as a lexicographic
/// sort key. Must be distinct across all outputs in a PSBT.
pub const PSBT_OUT_SORT_KEY_SUBTYPE: u8 = 0x10;

pub use super::output_set::{DuplicateOutput, OutputSet, ResultOutputSet};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;

    /// An output with unique ID `[tag]` and every other field unset.
    fn output(tag: u8) -> Output {
        Output::new(UniqueId::new(vec![tag]), Amount::ZERO, ScriptBuf::new())
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn outputs_with_different_amounts_conflict() {
            let with_amount = |sats| Output {
                amount: Amount::from_sat(sats),
                ..output(1)
            };
            let joined = with_amount(100).wrap().join(with_amount(200).wrap());
            let mut fields = Vec::new();
            joined.for_each_conflict(|field, _, _| fields.push(field.to_owned()));
            assert_eq!(fields, ["amount"]);
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }

        #[test]
        fn sort_key_is_encoded_under_the_crate_prefix() {
            let key = psbt_v2::raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: 0x10,
                key: vec![],
            };
            let mut output = output(1);
            assert_eq!(output.sort_key(), None);
            output.set_sort_key(SortKey::try_from(vec![0x42]).unwrap());
            assert_eq!(output.sort_key(), Some([0x42].as_slice()));
            let mut v2 = psbt_v2::v2::Output::from(output.clone());
            assert_eq!(v2.proprietaries.get(&key), Some(&vec![0x42]));
            assert_eq!(Output::try_from(v2.clone()), Ok(output));

            v2.proprietaries.insert(key, vec![0; 33]);
            let error = Output::try_from(v2).unwrap_err();
            assert_eq!(
                error.to_string(),
                "output sort key is 33 bytes, more than 32"
            );
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_output() -> impl Strategy<Value = Output> {
            (
                any::<u64>(),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
                proptest::option::of(proptest::collection::vec(0u8..3, 0..=2)),
            )
                .prop_map(|(amount, redeem, witness, sort_key)| Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    script_pubkey: ScriptBuf::new(),
                    redeem_script: redeem.map(ScriptBuf::from_bytes),
                    witness_script: witness.map(ScriptBuf::from_bytes),
                    sort_key: sort_key.map(|key| SortKey::try_from(key).expect("at most 2 bytes")),
                    ..output(1)
                })
        }

        fn arb_result_output() -> impl Strategy<Value = ResultOutput> {
            prop_oneof![
                arb_output().prop_map(|o| o.wrap()),
                (arb_output(), arb_output()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_join_laws!(arb_result_output());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(o in arb_output()) {
                prop_assert_eq!(o.clone().wrap().try_unwrap(), Ok(o));
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(r in arb_result_output()) {
                prop_assert_eq!(r.is_ok(), r.try_unwrap().is_ok());
            }

            #[test]
            fn try_from_reads_the_sort_key(key in proptest::collection::vec(any::<u8>(), 0..=40)) {
                let mut v2 = psbt_v2::v2::Output::from(output(1));
                v2.proprietaries
                    .insert(crate::psbt::proprietary::key(PSBT_OUT_SORT_KEY_SUBTYPE), key.clone());
                match Output::try_from(v2.clone()) {
                    Ok(output) => {
                        prop_assert!(key.len() <= SortKey::MAX_LEN);
                        prop_assert_eq!(output.sort_key(), Some(key.as_slice()));
                        prop_assert_eq!(psbt_v2::v2::Output::from(output), v2);
                    }
                    Err(error) => {
                        prop_assert!(key.len() > SortKey::MAX_LEN);
                        prop_assert_eq!(error, InvalidOutput::SortKeyTooLong(InvalidSortKey(key)));
                    }
                }
            }
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop_uid {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn unique_id_roundtrip(uid in proptest::collection::vec(0u8..=255, 0..=40)) {
                let Ok(id) = UniqueId::try_from(uid.clone()) else {
                    prop_assert!(uid.is_empty() || uid.len() > UniqueId::MAX_LEN);
                    let error = InvalidUniqueId(uid.clone());
                    prop_assert!(error.to_string().contains("not 1 to 32"));
                    prop_assert_eq!(UniqueId::try_from(uid), Err(error));
                    return Ok(());
                };
                prop_assert_eq!(&id, &UniqueId::new(uid.clone()));
                prop_assert_eq!(id.as_bytes(), uid.as_slice());
                prop_assert_eq!(id.into_bytes(), uid);
            }

            #[test]
            fn v2_conversion_reads_and_writes_the_unique_id(
                uid in proptest::collection::vec(0u8..=255, 0..=40),
                amount in any::<u64>(),
                unknown in proptest::option::of(proptest::collection::vec(0u8..=255, 0..=4)),
            ) {
                let mut v2 = psbt_v2::v2::Output {
                    amount: Amount::from_sat(amount),
                    ..psbt_v2::v2::Output::default()
                };
                if let Some(unknown) = unknown {
                    v2.unknowns.insert(raw::Key { type_value: 0xfc, key: vec![] }, unknown);
                }
                prop_assert_eq!(
                    Output::try_from(v2.clone()),
                    Err(InvalidOutput::MissingUniqueId)
                );

                v2.set_unique_id(UniqueId::new(uid.clone()));
                match Output::try_from(v2.clone()) {
                    Ok(output) => {
                        prop_assert!((1..=UniqueId::MAX_LEN).contains(&uid.len()));
                        prop_assert_eq!(output.unique_id().as_bytes(), uid.as_slice());
                        prop_assert!(output.proprietaries.is_empty());
                        prop_assert_eq!(output.amount, Amount::from_sat(amount));
                        prop_assert_eq!(psbt_v2::v2::Output::from(output), v2);
                    }
                    Err(error) => {
                        prop_assert!(uid.is_empty() || uid.len() > UniqueId::MAX_LEN);
                        prop_assert!(error.to_string().starts_with("output unique ID is"));
                        prop_assert_eq!(
                            error,
                            InvalidOutput::InvalidUniqueId(InvalidUniqueId(uid))
                        );
                    }
                }
            }
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit_uid {
        use super::*;

        #[test]
        fn unique_id_is_encoded_under_the_crate_prefix() {
            let key = raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: 0x01,
                key: vec![],
            };
            let v2 = psbt_v2::v2::Output::from(output(7));
            assert_eq!(v2.proprietaries.get(&key), Some(&vec![7]));
            let mut set = psbt_v2::v2::Output::default();
            set.set_unique_id(UniqueId::new(vec![7]));
            assert_eq!(set, v2);
            assert_eq!(
                InvalidOutput::MissingUniqueId.to_string(),
                "output missing PSBT_OUT_UNIQUE_ID"
            );
        }

        #[test]
        fn unique_id_is_1_to_32_bytes() {
            for len in [1, 32] {
                let mut v2 = psbt_v2::v2::Output::default();
                v2.set_unique_id(UniqueId::new(vec![0; len]));
                let output = Output::try_from(v2).expect("a valid unique ID");
                assert_eq!(output.unique_id(), &UniqueId::new(vec![0; len]));
            }
            for len in [0, 33] {
                let error = UniqueId::try_from(vec![0; len]).unwrap_err();
                assert_eq!(error, InvalidUniqueId(vec![0; len]));
                assert_eq!(
                    error.to_string(),
                    format!("unique ID is {len} bytes, not 1 to 32")
                );
                let mut v2 = psbt_v2::v2::Output::default();
                v2.set_unique_id(UniqueId::new(vec![0; len]));
                let error = Output::try_from(v2).unwrap_err();
                assert_eq!(
                    error.to_string(),
                    format!("output unique ID is {len} bytes, not 1 to 32")
                );
            }
        }

        #[test]
        fn unique_id_construction_and_generation() {
            let generated = UniqueId::generate();
            assert_eq!(generated.as_bytes().len(), 16);
            let output = Output::new(generated.clone(), Amount::ONE_SAT, ScriptBuf::new());
            assert_eq!(output.unique_id(), &generated);
            assert_eq!(output.amount, Amount::ONE_SAT);
        }
    }
}
