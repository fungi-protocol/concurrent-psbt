//! Sort-mode typestates and [`Sorter<S>`] for sorting unordered PSBTs.
//!
//! Submodules:
//! - [`traits`] — sort-mode typestate types and sorting capability traits
//! - [`sorter`] — [`Sorter<S>`] struct, key derivation helpers
//! - [`explicit`] — [`Sorter<ExplicitSortKeys>`] impls
//! - [`deterministic`] — [`Sorter<Deterministic<_>>`] impls
//! - [`relaxed`] — [`Sorter<Relaxed<_>>`] impls

pub mod traits;
mod sorter;
mod explicit;
mod deterministic;
mod relaxed;

// Re-export the public surface flat, mirroring the old sort.rs interface.
pub use traits::{Deterministic, ExplicitSortKeys, Relaxed, Seeded, SeedState, SortMode, Unseeded};
pub(crate) use traits::{Sortable, TrySortable};
pub use sorter::{Sorter, SorterError};
pub(crate) use sorter::{derive_sort_key, OutPointIdentifier};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creator::Creator;
    use crate::psbt::input::InputExt as _;

    fn assert_sort_mode<S: SortMode>() {}

    #[test]
    fn sort_modes_implement_trait() {
        assert_sort_mode::<ExplicitSortKeys>();
        assert_sort_mode::<Deterministic<Unseeded>>();
        assert_sort_mode::<Deterministic<Seeded>>();
        assert_sort_mode::<Relaxed<Unseeded>>();
        assert_sort_mode::<Relaxed<Seeded>>();
    }

    // -- Sorter::new checked constructor tests --------------------------------

    #[test]
    fn sorter_explicit_new_rejects_wrong_flag() {
        let u = Creator::new().into_unordered_psbt();
        assert_eq!(
            Sorter::<ExplicitSortKeys>::new(u),
            Err(SorterError::SortModeMismatch)
        );
    }

    #[test]
    fn sorter_explicit_new_accepts_correct_flag() {
        let u = Creator::new().explicit_sort_keys().into_unordered_psbt();
        assert!(Sorter::<ExplicitSortKeys>::new(u).is_ok());
    }

    #[test]
    fn sorter_deterministic_seeded_new_rejects_missing_seed() {
        let u = Creator::new().deterministic_sorting().into_unordered_psbt();
        assert_eq!(
            Sorter::<Deterministic<Seeded>>::new(u),
            Err(SorterError::MissingSeed)
        );
    }

    #[test]
    fn sorter_deterministic_seeded_new_accepts_seed() {
        let u = Creator::new()
            .deterministic_sorting()
            .set_seed(b"seed-16-bytes!!!".to_vec())
            .into_unordered_psbt();
        assert!(Sorter::<Deterministic<Seeded>>::new(u).is_ok());
    }

    #[test]
    fn sorter_relaxed_seeded_new_rejects_wrong_flag() {
        let u = Creator::new().explicit_sort_keys().into_unordered_psbt();
        assert_eq!(
            Sorter::<Relaxed<Seeded>>::new(u),
            Err(SorterError::SortModeMismatch)
        );
    }

    #[test]
    fn sorter_relaxed_seeded_new_accepts_seed() {
        let u = Creator::new()
            .set_seed(b"seed-16-bytes!!!".to_vec())
            .into_unordered_psbt();
        assert!(Sorter::<Relaxed<Seeded>>::new(u).is_ok());
    }

    #[test]
    fn sorter_explicit_standalone() {
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let mut unordered = Creator::new().explicit_sort_keys().into_unordered_psbt();
        let mut input_b = psbt_v2::v2::Input::new(&op_b);
        input_b.set_sort_key(vec![0x01]);
        let mut input_a = psbt_v2::v2::Input::new(&op_a);
        input_a.set_sort_key(vec![0x02]);
        unordered.global.input_count = 2;
        unordered.inputs = [input_b, input_a].into_iter().collect();

        let psbt = Sorter::<ExplicitSortKeys>::new(unordered).unwrap().sort();
        assert_eq!(psbt.inputs[0].spent_output_index, 1);
        assert_eq!(psbt.inputs[1].spent_output_index, 0);
    }

    #[test]
    fn sorter_deterministic_seeded_standalone() {
        let seed = b"standalone-seed!!".to_vec();
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let mut unordered = Creator::new()
            .deterministic_sorting()
            .set_seed(seed.clone())
            .into_unordered_psbt();
        unordered.global.input_count = 2;
        unordered.inputs = [psbt_v2::v2::Input::new(&op_a), psbt_v2::v2::Input::new(&op_b)]
            .into_iter()
            .collect();

        let psbt = Sorter::<Deterministic<Seeded>>::new(unordered).unwrap().sort();
        assert_eq!(psbt.inputs.len(), 2);

        let mut unordered2 = Creator::new()
            .deterministic_sorting()
            .set_seed(seed)
            .into_unordered_psbt();
        unordered2.global.input_count = 2;
        unordered2.inputs =
            [psbt_v2::v2::Input::new(&op_b), psbt_v2::v2::Input::new(&op_a)]
                .into_iter()
                .collect();
        let psbt2 = Sorter::<Deterministic<Seeded>>::new(unordered2).unwrap().sort();

        assert_eq!(
            psbt.inputs.iter().map(|i| i.spent_output_index).collect::<Vec<_>>(),
            psbt2.inputs.iter().map(|i| i.spent_output_index).collect::<Vec<_>>(),
        );
    }
}
