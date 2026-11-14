use super::partial::{Conflict, JoinResult, PartialJoin};

/// Iterates over conflicts in a value.
/// Maps pair each item from their values' iterators with its key.
pub trait Conflicts {
    type Item<'a>
    where
        Self: 'a;

    fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>>;
}

impl<V: PartialJoin> Conflicts for JoinResult<V> {
    type Item<'a>
        = &'a Conflict<V>
    where
        Self: 'a;

    fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
        self.as_ref().err().into_iter()
    }
}
