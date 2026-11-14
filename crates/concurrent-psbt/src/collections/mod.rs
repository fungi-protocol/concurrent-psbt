//! Join implementations for standard library collection types.
//!
//! Each collection type provides crate-internal joins for the result domain,
//! and `Option` and `BTreeMap` implement [`Lift`](crate::lattice::lift::Lift),
//! [`ResultValue`](crate::lattice::lift::ResultValue) and
//! [`VisitConflicts`](crate::lattice::lift::VisitConflicts) by lifting their
//! contents.

pub(crate) mod btreemap;
pub(crate) mod hashmap;
pub(crate) mod option;
