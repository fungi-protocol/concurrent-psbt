//! Moving between clean values and their result-domain counterparts.
//!
//! A clean value [`Lift`]s into a result-domain value whose joins record
//! conflicts instead of failing. A [`ResultValue`] lowers back to the clean
//! value once it has no conflicts, and [`VisitConflicts`] reports the conflicts
//! it does have.
//!
//! The counterpart is chosen by type: a [`PartialJoin`] scalar lifts into a
//! [`JoinResult`], and `Option` and `BTreeMap` lift their contents (see
//! [`crate::collections`]).

use std::fmt::Debug;

use crate::lattice::join::Join;
use crate::lattice::partial::{JoinResult, PartialJoin};

/// A clean value with a result-domain counterpart.
pub(crate) trait Lift: Sized {
    /// The result-domain counterpart, which lowers back to `Self`.
    type Result: Join + ResultValue<Clean = Self>;
    /// Lift into the result domain, without conflicts.
    fn lift(self) -> Self::Result;
}

/// A result-domain value, which may hold conflicts.
pub(crate) trait ResultValue: Sized {
    /// Clean value extracted when the result-domain value has no conflicts.
    type Clean;
    /// Returns `true` when this value contains no conflicts.
    fn is_ok(&self) -> bool;
    /// Extract the clean value, or return the original result-domain value.
    fn try_unwrap(self) -> Result<Self::Clean, Self>;
}

/// Callback receiving a conflict and the map key it sits under, if any.
pub(crate) type ConflictVisitor<'a> = dyn FnMut(Option<&dyn Debug>, &dyn Debug) + 'a;

/// A result-domain value whose conflicts can be enumerated.
pub(crate) trait VisitConflicts {
    /// Call `f(key, conflict)` for each conflict.
    ///
    /// `key` is the map key under which the conflict sits, or `None` for a
    /// conflict that is not in a map.
    fn visit_conflicts(&self, f: &mut ConflictVisitor<'_>);
}

impl<V: PartialJoin> Lift for V {
    type Result = JoinResult<V>;

    fn lift(self) -> JoinResult<V> {
        self.wrap()
    }
}

impl<V: PartialJoin> ResultValue for JoinResult<V> {
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

impl<V: PartialJoin + Debug> VisitConflicts for JoinResult<V> {
    fn visit_conflicts(&self, f: &mut ConflictVisitor<'_>) {
        if let Err(conflict) = self {
            f(None, conflict);
        }
    }
}

#[cfg(all(test, feature = "prop-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::partial::Conflict;
    use proptest::prelude::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Val(u8);

    impl PartialJoin for Val {
        fn try_join(self, other: Self) -> JoinResult<Self> {
            if self == other {
                Ok(self)
            } else {
                Err(Conflict::from_values([self, other]))
            }
        }
    }

    proptest! {
        #[test]
        fn lift_try_unwrap_roundtrip(a in 0u8..4) {
            let lifted = Val(a).lift();
            prop_assert!(ResultValue::is_ok(&lifted));
            let mut visited = 0;
            lifted.visit_conflicts(&mut |_, _| visited += 1);
            prop_assert_eq!(visited, 0);
            prop_assert_eq!(ResultValue::try_unwrap(lifted), Ok(Val(a)));
        }

        #[test]
        fn conflicts_are_kept_and_visited(a in 0u8..4, b in 0u8..4) {
            prop_assume!(a != b);
            let value: JoinResult<Val> = Err(Conflict::from_values([Val(a), Val(b)]));
            prop_assert!(!ResultValue::is_ok(&value));
            let mut visited = Vec::new();
            value.visit_conflicts(&mut |key, conflict| {
                visited.push((key.is_none(), format!("{conflict:?}")));
            });
            prop_assert_eq!(visited, [(true, format!("{:?}", value.clone().unwrap_err()))]);
            prop_assert_eq!(ResultValue::try_unwrap(value.clone()), Err(value));
        }
    }
}
