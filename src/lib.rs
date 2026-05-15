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

// FIXME this pub stuff needs to be done deliberately, or its pubness should be
// cfg(feature = _internal). fields are an implementation detail
pub mod fields;

// FIXME move the following modules under a `psbt` module
mod global;
mod input;
mod output;
mod psbt_ext;
mod tx;

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
    pub use crate::global::{Global, GlobalExt, ResultGlobal};
    pub use crate::input::{Input, InputSet, ResultInput, ResultInputSet};
    pub use crate::lattice::join::Join;
    pub use crate::lattice::partial::PartialJoin;
    pub use crate::output::{Output, OutputSet, ResultOutput, ResultOutputSet};
    pub use crate::tx::UnorderedPsbt;
}

#[cfg(test)]
mod tests;
