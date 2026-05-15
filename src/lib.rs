// TODO
// - tech debt
//   - remove commented out stuff
//   - reorganize in logical order
//   - decide where result lives
// - ergonomics:
//   - pub and re-exports
//   - IntoJoin (uses .into_ok()) for PartialJoin?
//   - operator overloading?
//   - is transpose the right interface?
//   - some method of extracting just the conflict errors? requires Box<dyn Error>

mod lattice;

mod collections;

mod values;

pub mod fields;

// TODO move to psbt mod
mod global;
mod input;
mod output;
mod tx;

pub mod sort;
pub use sort::Sorter;
pub mod creator;
pub use creator::{Creator, CreatorWith};
pub mod dynamic;
pub mod constructor;

/// Re-exports for fuzzing and internal tooling. Not part of the public API.
#[cfg(feature = "_internal")]
pub mod _internal {
    pub use crate::global::{Global, GlobalExt, ResultGlobal};
    pub use crate::input::{Input, InputSet, ResultInput, ResultInputSet};
    pub use crate::lattice::join::Join;
    pub use crate::lattice::partial::PartialJoin;
    pub use crate::output::{Output, OutputSet, ResultOutput, ResultOutputSet};
    pub use crate::tx::UnorderedPsbt;
}

#[cfg(test)]
mod tests;
