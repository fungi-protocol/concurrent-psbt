//! The [`Lift`] trait's [`wrap`](Lift::wrap) method lifts an arbitrary value into the Result-domain,
//! where wrapped values can be [`Join::join`]ed, with incompatible values creating
//! conflicts instead of failing. [`ResultWrapped::try_unwrap`] lowers back to the value
//! domain once it has no conflicts. [`conflicts`](crate::Conflicts::conflicts) allows enumeration
//! of the conflicts in a Result domain value that cannot be lowered back to the value
//! domain.
//!
//! Value types implementing [`PartialJoin`] lift into a [`JoinResult`].
//! Collections, i.e. `Option`, `BTreeMap` and `HashMap` lift their value types
//! recursively (see [`crate::collections`]).

use crate::lattice::join::Join;
use crate::lattice::partial::{JoinResult, PartialJoin};

/// A clean value with a result-domain counterpart.
pub trait Lift: Sized {
    /// The result-domain counterpart, which lowers back to `Self`.
    type Result: Join + ResultWrapped<Clean = Self>;
    /// Lift into the result domain, without conflicts.
    fn wrap(self) -> Self::Result;
}

/// A result-domain value, which may hold conflicts.
pub trait ResultWrapped: Sized {
    /// Clean value extracted when the result-domain value has no conflicts.
    type Clean;
    /// Returns `true` when this value contains no conflicts.
    fn is_ok(&self) -> bool;
    /// Extract the clean value, or return the original result-domain value.
    fn try_unwrap(self) -> Result<Self::Clean, Self>;
}

impl<V: PartialJoin> Lift for V {
    type Result = JoinResult<V>;

    fn wrap(self) -> JoinResult<V> {
        Ok(self)
    }
}

impl<V: PartialJoin> ResultWrapped for JoinResult<V> {
    type Clean = V;

    fn is_ok(&self) -> bool {
        Result::is_ok(self)
    }

    fn try_unwrap(self) -> Result<V, Self> {
        match self {
            Ok(value) => Ok(value),
            Err(conflict) => Err(Err(conflict)),
        }
    }
}

#[cfg(all(test, feature = "prop-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::conflicts::Conflicts;
    use crate::lattice::partial::Conflict;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn lift_try_unwrap_roundtrip(a in 0u8..4) {
            let lifted = a.wrap();
            prop_assert!(ResultWrapped::is_ok(&lifted));
            prop_assert_eq!(lifted.conflicts().count(), 0);
            prop_assert_eq!(ResultWrapped::try_unwrap(lifted), Ok(a));
        }

        #[test]
        fn conflicts_are_kept_and_visited(a in 0u8..4, b in 0u8..4) {
            prop_assume!(a != b);
            let value: JoinResult<u8> = Err(Conflict::from_values([a, b]));
            prop_assert!(!ResultWrapped::is_ok(&value));
            prop_assert_eq!(value.conflicts().collect::<Vec<_>>(), vec![value.as_ref().unwrap_err()]);
            prop_assert_eq!(ResultWrapped::try_unwrap(value.clone()), Err(value));
        }
    }
}
