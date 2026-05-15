// TODO
// - tech debt
//   - remove commented out stuff
//   - reorganize in logical order
//   - decide where result lives
// - ergonomics:
//   - pub and re-exports
//   - operator overloading?

mod lattice;

mod collections;

mod values;

// Implementation detail. Public only for fuzzing/internal tooling via `_internal` feature.
#[cfg(not(feature = "_internal"))]
mod fields;
#[cfg(feature = "_internal")]
pub mod fields;

mod psbt;

// FIXME this pub stuff needs to be done deliberately. there should be a single
// `mod reexports` that does pub use of these, and then the top level can pub use
// `use reexports::*` to do all the re-exporting.
pub mod sort;
pub use sort::Sorter;
pub mod creator;
pub use creator::{Creator, CreatorWith};
pub mod constructor;
pub mod dynamic;

/// Re-exports for fuzzing and internal tooling. Not part of the public API.
#[cfg(feature = "_internal")]
pub mod _internal {
    pub use crate::psbt::global::{Global, GlobalExt, ResultGlobal};
    pub use crate::psbt::input::{Input, InputSet, ResultInput, ResultInputSet};
    pub use crate::lattice::join::Join;
    pub use crate::lattice::partial::PartialJoin;
    pub use crate::psbt::output::{Output, OutputSet, ResultOutput, ResultOutputSet};
    pub use crate::psbt::tx::UnorderedPsbt;
}

#[cfg(test)]
mod tests;
