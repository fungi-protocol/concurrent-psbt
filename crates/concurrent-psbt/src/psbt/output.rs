#![allow(clippy::result_large_err)]

//! PSBT output fields.
//!
//! [`ResultOutput`] joins fields independently, retaining incompatible values
//! as conflicts.

use std::collections::BTreeMap;

use bitcoin::bip32::KeySource;
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::taproot::TapTree;
use bitcoin::{Amount, ScriptBuf, TapLeafHash};
use psbt_v2::raw;

use super::input::{InvalidSortKey, SortKey, SortKeyField};
use super::proprietary::ProprietaryField;
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
        amount: Amount,
        script_pubkey: ScriptBuf,
        redeem_script: Option<ScriptBuf>,
        witness_script: Option<ScriptBuf>,
        bip32_derivations: BTreeMap<PublicKey, KeySource>,
        tap_internal_key: Option<XOnlyPublicKey>,
        tap_tree: Option<TapTree>,
        tap_key_origins: BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>,
        proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        unknowns: BTreeMap<raw::Key, Vec<u8>>,
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
    /// Its `PSBT_OUT_UNIQUE_ID` is shorter than [`UniqueId::MIN_LEN`] or
    /// longer than [`UniqueId::MAX_LEN`] bytes.
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
/// [`PSBT_OUT_UNIQUE_ID_SUBTYPE`]. Identifies an output independently of its
/// position in a transaction.
///
/// A unique ID is between [`UniqueId::MIN_LEN`] and [`UniqueId::MAX_LEN`]
/// bytes long, inclusive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UniqueId(Vec<u8>);

impl UniqueId {
    /// The minimum length of a unique ID in bytes.
    pub const MIN_LEN: usize = 16;
    /// The maximum length of a unique ID in bytes.
    pub const MAX_LEN: usize = 32;

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

        let mut bytes = [0u8; Self::MIN_LEN];
        OsRng.unwrap_err().fill_bytes(&mut bytes);
        Self(bytes.to_vec())
    }
}

impl crate::values::IdempotentValue for UniqueId {}

/// Output ID bytes whose length is outside the allowed range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidUniqueId(pub Vec<u8>);

impl TryFrom<Vec<u8>> for UniqueId {
    type Error = InvalidUniqueId;

    fn try_from(bytes: Vec<u8>) -> Result<Self, InvalidUniqueId> {
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&bytes.len()) {
            return Err(InvalidUniqueId(bytes));
        }
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for InvalidUniqueId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unique ID is {} bytes, expected {} to {}",
            self.0.len(),
            UniqueId::MIN_LEN,
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
    /// Write raw bytes to the proprietary ID field to exercise decoding.
    fn set_unique_id_bytes(&mut self, bytes: Vec<u8>);
}

#[cfg(test)]
impl OutputUniqueIdExt for psbt_v2::v2::Output {
    fn set_unique_id_bytes(&mut self, bytes: Vec<u8>) {
        self.proprietaries
            .insert(super::proprietary::key(PSBT_OUT_UNIQUE_ID_SUBTYPE), bytes);
    }
}

/// Subtype for `PSBT_OUT_SORT_KEY`.
///
/// Up to [`SortKey::MAX_LEN`] bytes of arbitrary data used as a lexicographic
/// sort key.
pub const PSBT_OUT_SORT_KEY_SUBTYPE: u8 = 0x10;

pub use super::output_set::{DuplicateOutput, OutputSet, ResultOutputSet};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;

    fn output(tag: u8) -> Output {
        Output::new(
            UniqueId::try_from(vec![tag; 16]).unwrap(),
            Amount::ZERO,
            ScriptBuf::new(),
        )
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use bitcoin::bip32::{DerivationPath, Fingerprint};
        use bitcoin::hashes::Hash;
        use bitcoin::secp256k1::{Secp256k1, SecretKey};
        use bitcoin::taproot::TaprootBuilder;
        use std::collections::BTreeMap;

        #[test]
        fn outputs_with_different_amounts_conflict() {
            let with_amount = |sats| Output {
                amount: Amount::from_sat(sats),
                ..output(1)
            };
            let joined = with_amount(100).wrap().join(with_amount(200).wrap());
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }

        /// An output with every field set, so a conversion that drops one
        /// changes the output.
        fn populated_output() -> Output {
            let secret_key = SecretKey::from_slice(&[1; 32]).expect("valid secret key");
            let public_key = PublicKey::new(secret_key.public_key(&Secp256k1::new()));
            let (x_only_public_key, _) = public_key.inner.x_only_public_key();
            let source = (Fingerprint::from([2; 4]), DerivationPath::master());
            let script = ScriptBuf::from_bytes(vec![3]);
            let tap_tree = TaprootBuilder::new()
                .add_leaf(0, script.clone())
                .expect("valid leaf");
            Output {
                amount: bitcoin::Amount::from_sat(4),
                script_pubkey: script.clone(),
                redeem_script: Some(script.clone()),
                witness_script: Some(script),
                bip32_derivations: BTreeMap::from([(public_key, source.clone())]),
                tap_internal_key: Some(x_only_public_key),
                tap_tree: Some(TapTree::try_from(tap_tree).expect("complete tree")),
                tap_key_origins: BTreeMap::from([(
                    x_only_public_key,
                    (vec![TapLeafHash::from_byte_array([5; 32])], source),
                )]),
                proprietaries: BTreeMap::from([(
                    raw::ProprietaryKey {
                        prefix: vec![6],
                        subtype: 7,
                        key: vec![],
                    },
                    vec![8],
                )]),
                unknowns: BTreeMap::from([(
                    raw::Key {
                        type_value: 0xfe,
                        key: vec![9],
                    },
                    vec![10],
                )]),
                ..output(11)
            }
        }

        #[test]
        fn v2_conversion_keeps_every_field() {
            let output = populated_output();
            assert_eq!(
                Output::try_from(psbt_v2::v2::Output::from(output.clone())),
                Ok(output)
            );
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
            fn a_set_field_reads_back(
                mut o in arb_output(),
                redeem in proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
            ) {
                let redeem = redeem.map(ScriptBuf::from_bytes);
                o.set_redeem_script(redeem.clone());
                prop_assert_eq!(o.redeem_script(), &redeem);
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
                    prop_assert!(!(16..=32).contains(&uid.len()));
                    let error = InvalidUniqueId(uid.clone());
                    prop_assert!(error.to_string().contains("expected 16 to 32"));
                    prop_assert_eq!(UniqueId::try_from(uid), Err(error));
                    return Ok(());
                };
                prop_assert!((16..=32).contains(&uid.len()));
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

                v2.set_unique_id_bytes(uid.clone());
                match Output::try_from(v2.clone()) {
                    Ok(output) => {
                        prop_assert!((16..=32).contains(&uid.len()));
                        prop_assert_eq!(output.unique_id().as_bytes(), uid.as_slice());
                        prop_assert!(output.proprietaries.is_empty());
                        prop_assert_eq!(output.amount, Amount::from_sat(amount));
                        prop_assert_eq!(psbt_v2::v2::Output::from(output), v2);
                    }
                    Err(error) => {
                        prop_assert!(!(16..=32).contains(&uid.len()));
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
            assert_eq!(v2.proprietaries.get(&key), Some(&vec![7; 16]));
            let mut set = psbt_v2::v2::Output::default();
            set.set_unique_id_bytes(vec![7; 16]);
            assert_eq!(set, v2);
            assert_eq!(
                InvalidOutput::MissingUniqueId.to_string(),
                "output missing PSBT_OUT_UNIQUE_ID"
            );
        }

        #[test]
        fn unique_id_is_16_to_32_bytes() {
            for len in [16, 32] {
                let mut v2 = psbt_v2::v2::Output::default();
                v2.set_unique_id_bytes(vec![0; len]);
                let output = Output::try_from(v2).expect("a valid unique ID");
                assert_eq!(output.unique_id().as_bytes(), &[0; 32][..len]);
            }
            for len in [0, 15, 33] {
                let error = UniqueId::try_from(vec![0; len]).unwrap_err();
                assert_eq!(error, InvalidUniqueId(vec![0; len]));
                assert_eq!(
                    error.to_string(),
                    format!("unique ID is {len} bytes, expected 16 to 32")
                );
                let mut v2 = psbt_v2::v2::Output::default();
                v2.set_unique_id_bytes(vec![0; len]);
                let error = Output::try_from(v2).unwrap_err();
                assert_eq!(
                    error.to_string(),
                    format!("output unique ID is {len} bytes, expected 16 to 32")
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
