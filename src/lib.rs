mod lattice;

mod collections;

mod values;

/// Implementation detail. Public only for fuzzing/internal tooling via `_internal` feature.
#[cfg(not(feature = "_internal"))]
mod fields;
#[cfg(feature = "_internal")]
pub mod fields;

mod psbt;

// TODO: decide modifiability of these modules as the API matures.
// `sort` and `creator` are `pub(crate)` — their public types are re-exported
// flat below. `constructor` and `dynamic` remain `pub` because they define
// named sub-namespaces users may want to import from directly.
pub(crate) mod sort;
pub(crate) mod creator;
pub mod constructor;
pub mod dynamic;

// -- Public API surface -------------------------------------------------------
//
// Only the types listed here are part of the stable public API.
// Extend deliberately; prefer adding a TODO comment over an immediate `pub use`.

// Creator entry-points
pub use creator::{Creator, CreatorWith};

// Sort-mode typestates needed to parameterise Creator/Constructor/Sorter
pub use sort::{
    CanSortInfallibly, Deterministic, ExplicitSortKeys, Relaxed, Seeded, SeedState, SortMode,
    Sorter, SorterError, Unseeded,
};

// Constructor errors
pub use constructor::errors::{Error as ConstructorError, SortingError};

// TODO: decide whether Constructor<M,S> itself should be re-exported here, or
//       whether users are expected to name it via `constructor::Constructor`.

// TODO: decide whether dynamic::Constructor and its error types belong here.

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
