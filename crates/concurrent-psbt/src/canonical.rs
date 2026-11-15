//! Canonical forms of values that have more than one representation.
//!
//! Some PSBT fields can be written in several ways that mean the same thing,
//! such as the leaf hashes of a taproot key origin, which BIP 371 gives no
//! order. A [`Canonicalization`] policy decides whether converting a value
//! puts it in its canonical form at once ([`Canonicalize`]), or keeps it as it
//! was given and leaves canonicalizing to the joins that need it
//! ([`Preserve`], the default).

/// A value that can be put into a canonical form.
///
/// Canonicalizing never changes what a value is equal to, and canonicalizing
/// a canonical value leaves it unchanged.
pub trait Canonicalizable {
    /// Put the value into its canonical form.
    fn canonicalize(&mut self);
}

/// When to canonicalize a value, chosen by type, so that code converting
/// values is written once for every policy.
pub trait Canonicalization {
    /// Apply the policy to a freshly converted value.
    fn apply<T: Canonicalizable>(value: T) -> T;
}

/// Keep values as they were given, canonicalizing only where a join needs
/// to. This is the default.
#[derive(Debug)]
pub enum Preserve {}

impl Canonicalization for Preserve {
    fn apply<T: Canonicalizable>(value: T) -> T {
        value
    }
}

/// Canonicalize every value as soon as it is converted.
#[derive(Debug)]
pub enum Canonicalize {}

impl Canonicalization for Canonicalize {
    fn apply<T: Canonicalizable>(mut value: T) -> T {
        value.canonicalize();
        value
    }
}
