#[macro_use]
mod joinable_struct_macro;
mod values;

pub mod global;
pub mod negotiation;
pub mod fee;
pub mod removal;
#[cfg(test)]
mod removal_laws;
pub mod input;
mod input_set;
pub mod output;
mod output_set;
pub mod tx;
