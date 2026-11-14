#[macro_use]
mod joinable_struct_macro;
pub(crate) mod global;
pub mod input;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "keyed sets are collected by the input and output sets, and frozen by the unordered PSBT, in later commits"
    )
)]
mod keyed_set;
pub mod output;
mod output_set;
pub(crate) mod proprietary;
mod tap_leaf_hashes;
mod values;

pub use tap_leaf_hashes::TapLeafHashes;
