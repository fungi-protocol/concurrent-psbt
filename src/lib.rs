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

pub mod constructor;

#[cfg(test)]
mod tests;
