use crate::global::Global;
use crate::input::InputSet;
use crate::output::OutputSet;

use crate::partial_join::PartialJoin;
use crate::values::ValueError;
use psbt_v2::v2::Psbt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnorderedPsbt {
    /// The global map.
    pub global: Global,
    /// The corresponding key-value map for each input in the unsigned transaction.
    pub inputs: InputSet,
    /// The corresponding key-value map for each output in the unsigned transaction.
    pub outputs: OutputSet,
}

// TODO
// InputByOutpoint etc makes no sense when join()ing elements because .intersect or .union will arbitrarily pick one
// needs to be reimpl as struct InputSet(HashMap<())

impl PartialJoin for UnorderedPsbt {
    type Error = ValueError;

    fn join(&self, other: &Self) -> Result<Self, Self::Error> {
        let inputs = self.inputs.join(&other.inputs)?;
        let outputs = self.outputs.join(&other.outputs)?;

        let mut global = self.global.join(&other.global)?;
        global.input_count = inputs.len();
        global.output_count = inputs.len();

        Ok(Self {
            global,
            inputs,
            outputs,
        })
    }
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
        self.global
            .proprietaries
            .get(&crate::fields::psbt_global_tx_unordered())
            .map_or(false, |v| v.as_slice() == [crate::fields::UNORDERED_VALUE])
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
}
