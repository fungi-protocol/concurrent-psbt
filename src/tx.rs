pub use psbt_v2::v2::Psbt;

use crate::global::Global;
use crate::global::GlobalExt;
use crate::global::ResultGlobal;
use crate::input::{InputSet, ResultInputSet};
use crate::output::{OutputSet, ResultOutputSet};

// TODO ResultUnorderedPsbt

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnorderedPsbt {
    /// The global map.
    pub global: Global,
    /// The corresponding collection for each input in the unsigned transaction.
    pub inputs: InputSet,
    /// The corresponding key-value map for each output in the unsigned transaction.
    pub outputs: OutputSet,
}

impl UnorderedPsbt {
    /// Infallible, lossy conversion from PSBT (forgets order). You probably
    /// want `crate::Constructor` instead.
    ///
    /// This constructor does not check that the PSBT is marked as unordered.
    pub fn from_psbt(psbt: Psbt) -> Self {
        Self {
            global: psbt.global,
            inputs: psbt.inputs.into_iter().collect(),
            outputs: psbt.outputs.into_iter().collect(),
        }
    }

    pub fn to_psbt(self) -> Psbt {
        Psbt {
            global: self.global,
            inputs: self.inputs.into_iter().collect(),
            outputs: self.outputs.into_iter().collect(),
        }
    }

    pub fn into_ok(self) -> ResultUnorderedPsbt {
        ResultUnorderedPsbt {
            global: self.global.into_ok(),
            inputs: self.inputs.into_ok(),
            outputs: self.outputs.into_ok(),
        }
    }

    pub fn is_unordered(&self) -> bool {
        // const psbt_global_unordered_proprietary_key = psbt_v2::raw::ProprietaryKey{
        //     // prefix: Vec<u8>,
        //     // subtype: Subtype,
        //     // key: Vec<u8>,
        // }

        // self.global.proprietaries[psbt_global_unordered_proprietary_key] == 0b11
        todo!()
    }
}

#[derive(Debug)]
pub struct ResultUnorderedPsbt {
    /// The global map.
    pub global: ResultGlobal,
    /// The corresponding collection for each input in the unsigned transaction.
    pub inputs: ResultInputSet,
    /// The corresponding key-value map for each output in the unsigned transaction.
    pub outputs: ResultOutputSet,
}

impl ResultUnorderedPsbt {
    pub fn transpose(self) -> Result<UnorderedPsbt, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        Ok(UnorderedPsbt {
            global: self.global.transpose().expect("verified all fields are Ok"),
            inputs: self.inputs.transpose().expect("verified all fields are Ok"),
            outputs: self
                .outputs
                .transpose()
                .expect("verified all fields are Ok"),
        })
    }

    pub fn is_ok(&self) -> bool {
        self.global.is_ok() && self.inputs.is_ok() && self.outputs.is_ok()
    }
}

#[test]
fn test_tx() {}
