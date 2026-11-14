#![allow(clippy::result_large_err)]

use bitcoin::bip32::KeySource;
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::taproot::TapTree;
use bitcoin::{Amount, ScriptBuf};
use psbt_v2::raw;

use super::tap_leaf_hashes::{TapLeafHashes, tap_key_origins_from_v2, tap_key_origins_into_v2};

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
    fields: {
        unique_id: UniqueId,
        pub amount: Amount,
        pub script_pubkey: ScriptBuf,
        pub redeem_script: Option<ScriptBuf>,
        pub witness_script: Option<ScriptBuf>,
        pub bip32_derivations: BTreeMap<PublicKey, KeySource>,
        pub tap_internal_key: Option<XOnlyPublicKey>,
        pub tap_tree: Option<TapTree>,
        pub tap_key_origins: BTreeMap<XOnlyPublicKey, (TapLeafHashes, KeySource)>,
        pub proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        pub unknowns: BTreeMap<raw::Key, Vec<u8>>,
    }
}

impl Output {
    /// Create an output paying `amount` to `script_pubkey`, identified by
    /// `unique_id`, with every other field unset.
    pub fn new(unique_id: UniqueId, amount: Amount, script_pubkey: ScriptBuf) -> Self {
        Self::with_unique_id(
            psbt_v2::v2::Output {
                amount,
                script_pubkey,
                ..psbt_v2::v2::Output::default()
            },
            unique_id,
        )
    }

    /// The [`UniqueId`] identifying this output.
    pub fn unique_id(&self) -> &UniqueId {
        &self.unique_id
    }

    /// Take the fields of `output`, other than its `PSBT_OUT_UNIQUE_ID`.
    fn with_unique_id(output: psbt_v2::v2::Output, unique_id: UniqueId) -> Self {
        let psbt_v2::v2::Output {
            amount,
            script_pubkey,
            redeem_script,
            witness_script,
            bip32_derivations,
            tap_internal_key,
            tap_tree,
            tap_key_origins,
            mut proprietaries,
            unknowns,
            ..
        } = output;
        proprietaries.remove(&unique_id_key());
        Self {
            unique_id,
            amount,
            script_pubkey,
            redeem_script,
            witness_script,
            bip32_derivations,
            tap_internal_key,
            tap_tree,
            tap_key_origins: tap_key_origins_from_v2(tap_key_origins),
            proprietaries,
            unknowns,
        }
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
}

impl std::fmt::Display for InvalidOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingUniqueId => write!(f, "output missing PSBT_OUT_UNIQUE_ID"),
            Self::InvalidUniqueId(error) => write!(f, "output {error}"),
        }
    }
}

impl std::error::Error for InvalidOutput {}

/// Reads the unique ID from `PSBT_OUT_UNIQUE_ID`.
impl TryFrom<psbt_v2::v2::Output> for Output {
    type Error = InvalidOutput;

    fn try_from(output: psbt_v2::v2::Output) -> Result<Self, InvalidOutput> {
        let bytes = output
            .proprietaries
            .get(&unique_id_key())
            .ok_or(InvalidOutput::MissingUniqueId)?;
        let unique_id =
            UniqueId::try_from(bytes.clone()).map_err(InvalidOutput::InvalidUniqueId)?;
        Ok(Self::with_unique_id(output, unique_id))
    }
}

/// Writes the unique ID to `PSBT_OUT_UNIQUE_ID`.
impl From<Output> for psbt_v2::v2::Output {
    #[expect(
        clippy::needless_update,
        reason = "psbt-v2's silent-payments feature adds fields this crate does not support yet"
    )]
    fn from(output: Output) -> Self {
        let mut proprietaries = output.proprietaries;
        proprietaries.insert(unique_id_key(), output.unique_id.into_bytes());
        Self {
            amount: output.amount,
            script_pubkey: output.script_pubkey,
            redeem_script: output.redeem_script,
            witness_script: output.witness_script,
            bip32_derivations: output.bip32_derivations,
            tap_internal_key: output.tap_internal_key,
            tap_tree: output.tap_tree,
            tap_key_origins: tap_key_origins_into_v2(output.tap_key_origins),
            proprietaries,
            unknowns: output.unknowns,
            ..psbt_v2::v2::Output::default()
        }
    }
}

/// Opaque byte-vector identifier for a PSBT output.
///
/// Computed from the `PSBT_OUT_UNIQUE_ID` proprietary field, keyed by subtype
/// [`PSBT_OUT_UNIQUE_ID_SUBTYPE`]. Used as the map key in `OutputSet`
/// so that outputs can be merged in an order-independent way.
///
/// Built with [`TryFrom`], a unique ID is 1 to [`UniqueId::MAX_LEN`] bytes
/// long, as is one read from a BIP 370 output. An empty ID would make
/// unrelated outputs join as one.
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

fn unique_id_key() -> raw::ProprietaryKey {
    raw::ProprietaryKey {
        prefix: b"concurrent-psbt".to_vec(),
        subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
        key: vec![],
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
        self.proprietaries.insert(unique_id_key(), id.into_bytes());
    }
}

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
        use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};
        use bitcoin::secp256k1::{Secp256k1, SecretKey};

        const ALL_CONFLICT_FIELDS: &[&str] = &[
            "unique_id",
            "amount",
            "script_pubkey",
            "redeem_script",
            "witness_script",
            "bip32_derivations",
            "tap_internal_key",
            "tap_tree",
            "tap_key_origins",
            "proprietaries",
            "unknowns",
        ];

        fn empty_conflict<T: PartialJoin>() -> JoinResult<T> {
            Err(Conflict::from_values([]))
        }

        fn all_fields_conflict(seed: u8) -> ResultOutput {
            let mut result = output(seed).wrap();
            result.unique_id = empty_conflict();
            result.amount = empty_conflict();
            result.script_pubkey = empty_conflict();
            result.redeem_script = Some(empty_conflict());
            result.witness_script = Some(empty_conflict());
            result.tap_internal_key = Some(empty_conflict());
            result.tap_tree = Some(empty_conflict());

            let secret_key = SecretKey::from_slice(&[seed; 32]).expect("bounded nonzero seed");
            let secp_public_key =
                bitcoin::secp256k1::PublicKey::from_secret_key(&Secp256k1::new(), &secret_key);
            let public_key = PublicKey::new(secp_public_key);
            let (x_only_public_key, _) = secp_public_key.x_only_public_key();
            result
                .bip32_derivations
                .insert(public_key, empty_conflict());
            result
                .tap_key_origins
                .insert(x_only_public_key, empty_conflict());
            result.proprietaries.insert(
                raw::ProprietaryKey {
                    prefix: vec![seed],
                    subtype: seed,
                    key: vec![seed],
                },
                empty_conflict(),
            );
            result.unknowns.insert(
                raw::Key {
                    type_value: seed,
                    key: vec![seed],
                },
                empty_conflict(),
            );
            result
        }

        #[test]
        fn try_unwrap_conflicting_returns_err() {
            let a = Output {
                amount: bitcoin::Amount::from_sat(100),
                ..output(1)
            };
            let b = Output {
                amount: bitcoin::Amount::from_sat(200),
                ..output(1)
            };
            assert!(a.wrap().join(b.wrap()).try_unwrap().is_err());
        }

        #[test]
        fn for_each_conflict_reports_every_conflicted_field() {
            let result = all_fields_conflict(1);
            let mut fields = Vec::new();
            result.for_each_conflict(|field, _, _| fields.push(field.to_owned()));
            assert_eq!(fields, ALL_CONFLICT_FIELDS);
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
            )
                .prop_map(|(amount, redeem, witness)| Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    script_pubkey: ScriptBuf::new(),
                    redeem_script: redeem.map(ScriptBuf::from_bytes),
                    witness_script: witness.map(ScriptBuf::from_bytes),
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
                let wrapped = o.wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn is_ok_reflects_content(r in arb_result_output()) {
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
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
