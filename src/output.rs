use std::collections::HashMap;

pub use psbt_v2::v2::Output;

use crate::lattice::join::Join;
use crate::lattice::partial::PartialJoin;

use crate::collections::btreemap::BTreeMapExt;
use crate::collections::btreemap::Transpose;
use crate::collections::option::OptionExt;
use crate::collections::option::ResultOptionExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSet(HashMap<Vec<u8>, Output>);

impl OutputSet {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn insert(&mut self, output: Output) {
        let key = output.unique_id();
        self.0.insert(key, output);
    }

    pub fn into_ok(self) -> ResultOutputSet {
        // FIXME generic?
        ResultOutputSet(self.0.into_iter().map(|(k, v)| (k, v.into_ok())).collect())
    }
}

impl FromIterator<Output> for OutputSet {
    fn from_iter<T: IntoIterator<Item = Output>>(iter: T) -> Self {
        Self(
            iter.into_iter()
                .map(|output| (output.unique_id(), output))
                .collect(),
        )
    }
}

impl IntoIterator for OutputSet {
    type Item = Output;
    type IntoIter = std::collections::hash_map::IntoValues<Vec<u8>, Output>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_values()
    }
}

pub(crate) trait OutputExt {
    fn unique_id(&self) -> Vec<u8>;
    fn sort_key(&self) -> Option<&Vec<u8>>;
    fn take_sort_key(&mut self) -> Option<Vec<u8>>;

    fn into_ok(self) -> ResultOutput;
}

impl OutputExt for Output {
    fn unique_id(&self) -> Vec<u8> {
        self.proprietaries
            .get(&crate::fields::psbt_out_unique_id())
            .expect("PSBT_OUT_UNIQUE_ID must be set (validate before constructing OutputSet)")
            .clone()
    }

    fn sort_key(&self) -> Option<&Vec<u8>> {
        self.proprietaries.get(&crate::fields::psbt_out_sort_key())
    }

    fn take_sort_key(&mut self) -> Option<Vec<u8>> {
        self.proprietaries.remove(&crate::fields::psbt_out_sort_key())
    }

    fn into_ok(self) -> ResultOutput {
        ResultOutput {
            amount: self.amount.into_ok(),
            script_pubkey: self.script_pubkey.into_ok(),
            redeem_script: self.redeem_script.into_ok(),
            witness_script: self.witness_script.into_ok(),
            tap_internal_key: self.tap_internal_key.into_ok(),
            tap_tree: self.tap_tree.into_ok(),
            bip32_derivations: self.bip32_derivations.into_ok(),
            tap_key_origins: self.tap_key_origins.into_ok(),
            proprietaries: self.proprietaries.into_ok(),
            unknowns: self.unknowns.into_ok(),
        }
    }
}

#[derive(Debug)]
pub struct ResultOutputSet(HashMap<Vec<u8>, ResultOutput>);

impl Join for ResultOutputSet {
    fn join(self, other: Self) -> Self {
        ResultOutputSet(self.0.join(other.0))
    }
}

impl ResultOutputSet {
    pub fn transpose(self) -> Result<OutputSet, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        Ok(OutputSet(
            self.0
                .into_iter()
                .map(|(k, v)| (k, v.transpose().expect("verified is_ok()")))
                .collect(),
        ))
    }

    pub fn is_ok(&self) -> bool {
        self.0.values().all(|v| v.is_ok())
    }
}

mod result {
    pub use std::collections::BTreeMap;

    use bitcoin::bip32::KeySource;
    use bitcoin::key::XOnlyPublicKey;
    use bitcoin::taproot::{TapLeafHash, TapTree};
    use bitcoin::{secp256k1, Amount, ScriptBuf};

    use psbt_v2::raw;

    use crate::lattice::partial::JoinResult;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ResultOutput {
        /// The output's amount (serialized as satoshis).
        pub amount: JoinResult<Amount>,

        /// The script for this output, also known as the scriptPubKey.
        pub script_pubkey: JoinResult<ScriptBuf>,

        /// The redeem script for this output.
        pub redeem_script: Option<JoinResult<ScriptBuf>>,
        /// The witness script for this output.
        pub witness_script: Option<JoinResult<ScriptBuf>>,
        /// A map from public keys needed to spend this output to their
        /// corresponding master key fingerprints and derivation paths.
        pub bip32_derivations: BTreeMap<secp256k1::PublicKey, JoinResult<KeySource>>,
        /// The internal pubkey.
        pub tap_internal_key: Option<JoinResult<XOnlyPublicKey>>,
        /// Taproot Output tree.
        pub tap_tree: Option<JoinResult<TapTree>>,
        /// Map of tap root x only keys to origin info and leaf hashes contained in it.
        pub tap_key_origins: BTreeMap<XOnlyPublicKey, JoinResult<(Vec<TapLeafHash>, KeySource)>>,
        /// Proprietary key-value pairs for this output.
        pub proprietaries: BTreeMap<raw::ProprietaryKey, JoinResult<Vec<u8>>>,
        /// Unknown key-value pairs for this output.
        pub unknowns: BTreeMap<raw::Key, JoinResult<Vec<u8>>>,
    }
}

pub use result::ResultOutput;

impl Join for ResultOutput {
    fn join(self, other: Self) -> Self {
        ResultOutput {
            amount: self.amount.join(other.amount),
            script_pubkey: self.script_pubkey.join(other.script_pubkey),
            redeem_script: self.redeem_script.join(other.redeem_script),
            witness_script: self.witness_script.join(other.witness_script),
            tap_internal_key: self.tap_internal_key.join(other.tap_internal_key),
            tap_tree: self.tap_tree.join(other.tap_tree),
            bip32_derivations: self.bip32_derivations.join(other.bip32_derivations),
            tap_key_origins: self.tap_key_origins.join(other.tap_key_origins),
            proprietaries: self.proprietaries.join(other.proprietaries),
            unknowns: self.unknowns.join(other.unknowns),
        }
    }
}

impl ResultOutput {
    pub fn transpose(self) -> Result<Output, ResultOutput> {
        if !self.is_ok() {
            return Err(self);
        }

        Ok(Output {
            amount: self.amount.expect("verified all fields are Ok"),
            script_pubkey: self.script_pubkey.expect("verified all fields are Ok"), // FIXME allow empty to non-empty to behave like Option<ScriptBuf> instead of ScriptBuf under equality
            redeem_script: self
                .redeem_script
                .transpose()
                .expect("verified all fields are Ok"),
            witness_script: self
                .witness_script
                .transpose()
                .expect("verified all fields are Ok"),
            tap_internal_key: self
                .tap_internal_key
                .transpose()
                .expect("verified all fields are Ok"),
            tap_tree: self
                .tap_tree
                .transpose()
                .expect("verified all fields are Ok"),
            bip32_derivations: self
                .bip32_derivations
                .transpose()
                .expect("verified all fields are Ok"),
            tap_key_origins: self
                .tap_key_origins
                .transpose()
                .expect("verified all fields are Ok"),
            proprietaries: self
                .proprietaries
                .transpose()
                .expect("verified all fields are Ok"),
            unknowns: self
                .unknowns
                .transpose()
                .expect("verified all fields are Ok"),
        })
    }

    pub fn is_ok(&self) -> bool {
        self.amount.is_ok()
            && self.script_pubkey.is_ok()
            && self.redeem_script.is_ok()
            && self.witness_script.is_ok()
            && self.tap_internal_key.is_ok()
            && self.tap_tree.is_ok()
            && self.bip32_derivations.is_ok()
            && self.tap_key_origins.is_ok()
            && self.proprietaries.is_ok()
            && self.unknowns.is_ok()
    }
}

// FIXME add tests
