#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;

use bitcoin::bip32::KeySource;
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::taproot::{TapLeafHash, TapTree};
use bitcoin::{Amount, ScriptBuf};
use psbt_v2::raw;
use psbt_v2::v2::Output;

joinable_struct! {
    /// Result-domain representation of a PSBT output.
    ///
    /// Each field mirrors its counterpart in [`psbt_v2::v2::Output`] but is
    /// represented in the internal result domain, recording either a clean value
    /// or an accumulated conflict.
    source: Output,
    result: ResultOutput,
    conflicts: OutputConflict, OutputField {
        Amount => amount: Amount,
        ScriptPubkey => script_pubkey: ScriptBuf,
        RedeemScript => redeem_script: ScriptBuf,
        WitnessScript => witness_script: ScriptBuf,
        Bip32Derivation => bip32_derivations[PublicKey]: KeySource,
        TapInternalKey => tap_internal_key: XOnlyPublicKey,
        TapTree => tap_tree: TapTree,
        TapKeyOrigin => tap_key_origins[XOnlyPublicKey]: (Vec<TapLeafHash>, KeySource),
        Proprietary => proprietaries[raw::ProprietaryKey]: Vec<u8>,
        Unknown => unknowns[raw::Key]: Vec<u8>,
    },
    ext: OutputExt,
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

/// Extension trait on [`psbt_v2::v2::Output`] for accessing the unique-ID proprietary field.
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

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn outputs_with_different_amounts_conflict() {
            let with_amount = |sats| Output {
                amount: Amount::from_sat(sats),
                ..Output::default()
            };
            let joined = with_amount(100).wrap().join(with_amount(200).wrap());
            let fields: Vec<_> = joined.conflicts().map(|c| c.path()).collect();
            assert_eq!(fields, [OutputField::Amount]);
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
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
                    ..Output::default()
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

        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop_uid {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn unique_id_roundtrip(uid in proptest::collection::vec(0u8..=255, 1..=40)) {
                let Ok(id) = UniqueId::try_from(uid.clone()) else {
                    prop_assert!(uid.len() > UniqueId::MAX_LEN);
                    let error = InvalidUniqueId(uid.clone());
                    prop_assert!(error.to_string().contains("more than 32"));
                    prop_assert_eq!(UniqueId::try_from(uid), Err(error));
                    return Ok(());
                };
                prop_assert_eq!(&id, &UniqueId::new(uid.clone()));
                prop_assert_eq!(id.as_bytes(), uid.as_slice());

                let mut output = Output::default();
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
            let mut output = Output::default();
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
            let output = Output::default();
            assert!(!output.has_unique_id());
            assert!(output.unique_id().is_none());
        }

        #[test]
        fn unique_id_construction_generation_and_setting() {
            let id = UniqueId::new(vec![1, 2, 3]);
            assert_eq!(id.as_bytes(), &[1, 2, 3]);

            let generated = UniqueId::generate();
            assert_eq!(generated.as_bytes().len(), 16);

            let mut output = Output::default();
            output.set_unique_id(id);
            assert_eq!(output.unique_id().unwrap().into_bytes(), vec![1, 2, 3]);
        }
    }
}
