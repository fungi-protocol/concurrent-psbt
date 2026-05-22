#[macro_use]
mod joinable_struct_macro;
pub mod global;
pub mod input;
mod input_set;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "keyed sets are collected by the input and output sets, and frozen by the unordered PSBT, in later commits"
    )
)]
mod keyed_set;
pub mod output;
pub(crate) mod proprietary;
mod values;
