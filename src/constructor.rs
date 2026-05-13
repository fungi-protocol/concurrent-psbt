use psbt_v2::v2::Creator as Bip370Creator;
use psbt_v2::v2::Psbt;

use crate::fields::{psbt_global_tx_unordered, UNORDERED_VALUE};

/// Creator for unordered PSBTs.
///
/// Sets the `PSBT_GLOBAL_TX_UNORDERED` proprietary field and both modifiable
/// flags, producing a PSBT ready for an unordered `Constructor`.
pub struct Creator(Psbt);

impl Creator {
    pub fn new() -> Self {
        let mut psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();

        psbt.global
            .proprietaries
            .insert(psbt_global_tx_unordered(), vec![UNORDERED_VALUE]);

        Creator(psbt)
    }

    /// Consume the creator and return the raw PSBT.
    pub fn psbt(self) -> Psbt {
        self.0
    }
}

impl Default for Creator {
    fn default() -> Self {
        Self::new()
    }
}
