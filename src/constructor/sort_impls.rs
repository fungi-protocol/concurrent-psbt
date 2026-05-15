//! Blanket `try_sort` / `sort` impls on [`Constructor<M, S>`].
//!
//! These live here rather than in `mod.rs` to keep the constructor module
//! focused on construction concerns.

use psbt_v2::v2::{Mod, Psbt};

use crate::sort::{Sortable, SortMode, TrySortable};
use crate::constructor::{Constructor, SortingError};

impl<M, S> Constructor<M, S>
where
    M: Mod,
    S: SortMode + 'static,
    crate::sort::Sorter<S>: TrySortable,
{
    /// Sort into a [`Psbt`].
    ///
    /// Returns `Err` only for [`crate::sort::ExplicitSortKeys`] when a sort
    /// key is missing or duplicated. Infallible for seeded modes.
    pub fn try_sort(self) -> Result<Psbt, SortingError> {
        self.into_sorter().try_sort_psbt()
    }
}

impl<M, S> Constructor<M, S>
where
    M: Mod,
    S: SortMode + 'static,
    crate::sort::Sorter<S>: Sortable,
{
    /// Sort into a [`Psbt`] infallibly.
    ///
    /// Only available for seeded or explicit-key sort modes (infallible).
    pub fn sort(self) -> Psbt {
        self.into_sorter().sort_psbt()
    }
}
