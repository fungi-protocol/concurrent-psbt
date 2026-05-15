//! Extension trait for [`psbt_v2::v2::Psbt`] adding output validation.

use crate::constructor::Error;
use super::output::OutputExt as _;
use psbt_v2::v2::Psbt;
use std::collections::HashSet;

/// Extension methods on [`Psbt`] for validation.
pub(crate) trait PsbtExt {
    /// Check that every output carries a unique `PSBT_OUT_UNIQUE_ID`.
    ///
    /// Returns `Err(MissingOutputUniqueId)` if any output lacks the field, or
    /// `Err(DuplicateOutputUniqueId)` if two outputs share the same value.
    fn validate_all_outputs_have_unique_ids(&self) -> Result<(), Error>;
}

impl PsbtExt for Psbt {
    fn validate_all_outputs_have_unique_ids(&self) -> Result<(), Error> {
        let mut seen = HashSet::new();
        for output in &self.outputs {
            if !output.has_unique_id() {
                return Err(Error::MissingOutputUniqueId);
            }
            if !seen.insert(output.unique_id()) {
                return Err(Error::DuplicateOutputUniqueId);
            }
        }
        Ok(())
    }
}
