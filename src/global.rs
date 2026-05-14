pub use psbt_v2::v2::Global;

use crate::lattice::join::Join;
use crate::lattice::partial::PartialJoin;

use crate::collections::btreemap::BTreeMapExt;
use crate::collections::btreemap::Transpose;
use crate::collections::option::OptionExt;
use crate::collections::option::ResultOptionExt;

pub trait GlobalExt {
    fn into_ok(self) -> ResultGlobal;

    // FIXME enable this, move from fields.rs
    // fn clear_inputs_ordered_flag(&mut self);
    // fn clear_outputs_ordered_flag(&mut self);
}

// impl GlobalExt for Global {
//     fn clear_inputs_ordered_flag(&mut self) {
//         todo!("modify proprietary field")
//     }

//     fn clear_outputs_ordered_flag(&mut self) {
//         todo!("modify proprietary field")
//     }
// }

impl GlobalExt for Global {
    // FIXME rename to lift
    fn into_ok(self) -> ResultGlobal {
        ResultGlobal {
            version: self.version.into_ok(),
            tx_version: self.tx_version.into_ok(),
            fallback_lock_time: self.fallback_lock_time.into_ok(),
            xpubs: self.xpubs.into_ok(),
            proprietaries: self.proprietaries.into_ok(),
            unknowns: self.unknowns.into_ok(),
            tx_modifiable_flags: self.tx_modifiable_flags.into_ok(),
            input_count: self.input_count.into_ok(),
            output_count: self.output_count.into_ok(),
        }
    }
}

mod result {
    pub use std::collections::BTreeMap;

    use bitcoin::bip32::{KeySource, Xpub};
    use bitcoin::locktime::absolute;
    use bitcoin::transaction;

    use psbt_v2::raw;
    use psbt_v2::Version;

    use crate::lattice::partial::JoinResult;

    #[derive(Debug)]
    pub struct ResultGlobal {
        /// The version number of this PSBT.
        pub version: JoinResult<Version>,

        /// The version number of the transaction being built.
        pub tx_version: JoinResult<transaction::Version>,

        /// The transaction locktime to use if no inputs specify a required locktime.
        pub fallback_lock_time: Option<JoinResult<absolute::LockTime>>,

        /// A bitfield for various transaction modification flags.
        pub tx_modifiable_flags: JoinResult<u8>,

        /// The number of inputs in this PSBT.
        pub input_count: JoinResult<usize>,

        /// The number of outputs in this PSBT.
        pub output_count: JoinResult<usize>,

        /// A map from xpub to the used key fingerprint and derivation path as defined by BIP 32.
        pub xpubs: BTreeMap<Xpub, JoinResult<KeySource>>,

        /// Global proprietary key-value pairs.
        pub proprietaries: BTreeMap<raw::ProprietaryKey, JoinResult<Vec<u8>>>,

        /// Unknown global key-value pairs.
        pub unknowns: BTreeMap<raw::Key, JoinResult<Vec<u8>>>,
    }
}

pub use result::ResultGlobal;

impl Join for ResultGlobal {
    fn join(self, other: Self) -> Self {
        ResultGlobal {
            version: self.version.join(other.version),
            tx_version: self.tx_version.join(other.tx_version),
            fallback_lock_time: self.fallback_lock_time.join(other.fallback_lock_time),
            tx_modifiable_flags: self.tx_modifiable_flags.join(other.tx_modifiable_flags),
            input_count: self.input_count.join(other.input_count),
            output_count: self.output_count.join(other.output_count),
            xpubs: self.xpubs.join(other.xpubs),
            proprietaries: self.proprietaries.join(other.proprietaries),
            unknowns: self.unknowns.join(other.unknowns),
        }
    }
}

impl ResultGlobal {
    pub fn transpose(self) -> Result<Global, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        Ok(Global {
            version: self.version.expect("verified all fields are Ok"),
            tx_version: self.tx_version.expect("verified all fields are Ok"),
            fallback_lock_time: self
                .fallback_lock_time
                .transpose()
                .expect("verified all fields are Ok"),
            tx_modifiable_flags: self
                .tx_modifiable_flags
                .expect("verified all fields are Ok"),
            input_count: self.input_count.expect("verified all fields are Ok"),
            output_count: self.output_count.expect("verified all fields are Ok"),
            xpubs: self.xpubs.transpose().expect("verified all fields are Ok"),
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
        self.version.is_ok()
            && self.tx_version.is_ok()
            && self.fallback_lock_time.is_ok()
            && self.tx_modifiable_flags.is_ok()
            && self.input_count.is_ok()
            && self.output_count.is_ok()
            && self.xpubs.is_ok()
            && self.proprietaries.is_ok()
            && self.unknowns.is_ok()
    }
}

#[test]
fn test_global_fields() {
    // Global(todo!())
}
