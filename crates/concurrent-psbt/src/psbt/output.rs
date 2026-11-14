#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;

use bitcoin::bip32::KeySource;
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::taproot::{TapLeafHash, TapTree};
use bitcoin::{Amount, ScriptBuf};
use psbt_v2::raw;

joinable_struct! {
    /// A PSBT output, mirroring the BIP 370 per-output map.
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
        pub tap_key_origins: BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>,
        pub proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        pub unknowns: BTreeMap<raw::Key, Vec<u8>>,
    },
}

impl Output {
    /// Create an output paying `amount` to `script_pubkey`, with every other
    /// field unset.
    pub fn new(amount: Amount, script_pubkey: ScriptBuf) -> Self {
        Self::from_v2_fields(psbt_v2::v2::Output {
            amount,
            script_pubkey,
            ..psbt_v2::v2::Output::default()
        })
    }
}

impl From<psbt_v2::v2::Output> for Output {
    fn from(output: psbt_v2::v2::Output) -> Self {
        Self::from_v2_fields(output)
    }
}

impl From<Output> for psbt_v2::v2::Output {
    fn from(output: Output) -> Self {
        output.encode_v2(psbt_v2::v2::Output::default())
    }
}

/// Opaque byte-vector identifier for a PSBT output.
///
/// Computed from the `PSBT_OUT_UNIQUE_ID` proprietary field, keyed by subtype
/// [`PSBT_OUT_UNIQUE_ID_SUBTYPE`]. Used as the map key in `OutputSet`
/// so that outputs can be merged in an order-independent way.
///
/// Built with [`TryFrom`], a unique ID is at most [`UniqueId::MAX_LEN`]
/// bytes long. One read from an output is not checked.
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

/// The bytes are too long for a [`UniqueId`]; they are returned unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidUniqueId(pub Vec<u8>);

impl TryFrom<Vec<u8>> for UniqueId {
    type Error = InvalidUniqueId;

    fn try_from(bytes: Vec<u8>) -> Result<Self, InvalidUniqueId> {
        if bytes.len() > Self::MAX_LEN {
            return Err(InvalidUniqueId(bytes));
        }
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for InvalidUniqueId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unique ID is {} bytes, more than {}",
            self.0.len(),
            UniqueId::MAX_LEN
        )
    }
}

impl std::error::Error for InvalidUniqueId {}

pub const PSBT_OUT_UNIQUE_ID_SUBTYPE: u8 = 0x01;

/// Extension trait on [`Output`] for accessing the unique-ID proprietary field.
pub trait OutputUniqueIdExt {
    /// Return the [`UniqueId`] stored in the `PSBT_OUT_UNIQUE_ID` proprietary field, if present.
    fn unique_id(&self) -> Option<UniqueId>;
    /// Return `true` if this output carries a [`UniqueId`].
    fn has_unique_id(&self) -> bool {
        self.unique_id().is_some()
    }
    /// Store `id` in the `PSBT_OUT_UNIQUE_ID` proprietary field, replacing
    /// any existing value.
    fn set_unique_id(&mut self, id: UniqueId);
}

impl OutputUniqueIdExt for Output {
    fn unique_id(&self) -> Option<UniqueId> {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: b"concurrent-psbt".to_vec(),
            subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.get(&key).map(|v| UniqueId(v.clone()))
    }

    fn set_unique_id(&mut self, id: UniqueId) {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: b"concurrent-psbt".to_vec(),
            subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.insert(key, id.into_bytes());
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;

    /// An output with every field unset.
    fn output() -> Output {
        Output::new(Amount::ZERO, ScriptBuf::new())
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
                ..output()
            };
            let joined = with_amount(100).wrap().join(with_amount(200).wrap());
            let mut fields = Vec::new();
            joined.for_each_conflict(|field, _, _| fields.push(field.to_owned()));
            assert_eq!(fields, ["amount"]);
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
            }
        }

        #[test]
        fn v2_conversion_keeps_every_field() {
            let output = populated_output();
            assert_eq!(
                Output::from(psbt_v2::v2::Output::from(output.clone())),
                output
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
            )
                .prop_map(|(amount, redeem, witness)| Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    script_pubkey: ScriptBuf::new(),
                    redeem_script: redeem.map(ScriptBuf::from_bytes),
                    witness_script: witness.map(ScriptBuf::from_bytes),
                    ..output()
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
                prop_assert_eq!(o.clone().wrap().try_unwrap(), Ok(o.clone()));
                prop_assert_eq!(Output::from(psbt_v2::v2::Output::from(o.clone())), o);
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
                    prop_assert!(uid.len() > UniqueId::MAX_LEN);
                    let error = InvalidUniqueId(uid.clone());
                    prop_assert!(error.to_string().contains("more than 32"));
                    prop_assert_eq!(UniqueId::try_from(uid), Err(error));
                    return Ok(());
                };
                prop_assert_eq!(&id, &UniqueId::new(uid.clone()));
                prop_assert_eq!(id.as_bytes(), uid.as_slice());

                let mut output = output();
                prop_assert!(!output.has_unique_id());
                output.set_unique_id(id);
                prop_assert!(output.has_unique_id());
                prop_assert_eq!(output.unique_id().unwrap().into_bytes(), uid);
            }
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit_uid {
        use super::*;

        fn output_with_uid(uid: &[u8]) -> Output {
            let mut output = output();
            let key = psbt_v2::raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                key: vec![],
            };
            output.proprietaries.insert(key, uid.to_vec());
            output
        }

        #[test]
        fn unique_id_present() {
            let output = output_with_uid(&[1, 2, 3]);
            assert!(output.has_unique_id());
            assert_eq!(output.unique_id().unwrap().into_bytes(), vec![1, 2, 3]);
        }

        #[test]
        fn unique_id_absent() {
            let output = output();
            assert!(!output.has_unique_id());
            assert!(output.unique_id().is_none());
        }

        #[test]
        fn unique_id_construction_generation_and_setting() {
            let id = UniqueId::new(vec![1, 2, 3]);
            assert_eq!(id.as_bytes(), &[1, 2, 3]);

            let generated = UniqueId::generate();
            assert_eq!(generated.as_bytes().len(), 16);

            let mut output = output();
            output.set_unique_id(id);
            assert_eq!(output.unique_id().unwrap().into_bytes(), vec![1, 2, 3]);
        }
    }
}
