#[macro_use]
mod joinable_struct_macro;
mod values;

pub mod global;
pub mod input;
mod input_set;
pub mod output;
mod output_set;
pub mod tx;

pub(crate) trait SetLen {
    fn len(&self) -> usize;
}
