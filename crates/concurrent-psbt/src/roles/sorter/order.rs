use psbt_v2::v2::Psbt;

use super::SortError;
use super::key::{derivation_engine, input_sort_key, output_sort_key};
use crate::tx::UnorderedPsbt;

/// Order `psbt` by full sort key, as [`input_sort_key`] and
/// [`output_sort_key`] compute it, and clear `PSBT_GLOBAL_TX_UNORDERED`.
///
/// Inputs are distinct by outpoint and outputs by unique ID, so their full
/// sort keys differ unless SHA-256 collides, and the order is total.
///
/// # Errors
/// Returns [`SortError::MissingUuid`] if `PSBT_GLOBAL_TX_UUID` is unset,
/// then the first input, then the first output, missing an explicit
/// sort key if `explicit`.
pub(super) fn ordered_psbt(psbt: UnorderedPsbt, explicit: bool) -> Result<Psbt, SortError> {
    let engine = derivation_engine(&psbt.global)?;
    let mut inputs: Vec<_> = psbt.inputs.into_iter().collect();
    let mut outputs: Vec<_> = psbt.outputs.into_iter().collect();

    let mut input_keys = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| input_sort_key(&engine, explicit, input).map(|key| (key, i)))
        .collect::<Result<Vec<_>, _>>()?;

    let mut output_keys = outputs
        .iter()
        .enumerate()
        .map(|(i, output)| output_sort_key(&engine, explicit, output).map(|key| (key, i)))
        .collect::<Result<Vec<_>, _>>()?;

    input_keys.sort_by(|(left, _), (right, _)| left.cmp(right));
    output_keys.sort_by(|(left, _), (right, _)| left.cmp(right));

    apply_permutation(&mut inputs, &mut input_keys);
    apply_permutation(&mut outputs, &mut output_keys);

    let mut global = psbt.global;
    global.clear_unordered();

    Ok(Psbt {
        global: global.into_v2(inputs.len(), outputs.len()),
        inputs: inputs.into_iter().map(Into::into).collect(),
        outputs: outputs.into_iter().map(Into::into).collect(),
    })
}

/// Reorder `items` in place so that position `i` holds the item that was at
/// index `order[i].1`.
///
/// Each cycle of the permutation is followed once with swaps. Visited
/// positions are marked by overwriting their index with the position itself,
/// so `order` no longer describes the permutation afterwards. A cycle has at
/// most `order.len()` positions, which bounds the walk.
fn apply_permutation<T, K>(items: &mut [T], order: &mut [(K, usize)]) {
    for start in 0..order.len() {
        let mut position = start;
        for _ in 0..order.len() {
            let source = std::mem::replace(&mut order[position].1, position);
            if source == start {
                break;
            }
            items.swap(position, source);
            position = source;
        }
    }
}

#[cfg(all(test, feature = "prop-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn apply_permutation_moves_each_item_to_its_position(
                permutation in (0usize..32)
                    .prop_flat_map(|n| Just((0..n).collect::<Vec<_>>()).prop_shuffle()),
            ) {
                let mut items: Vec<_> = (0..permutation.len()).map(|i| i * 10).collect();
                let mut order: Vec<_> = permutation.iter().map(|&i| ((), i)).collect();
                apply_permutation(&mut items, &mut order);
                let expected: Vec<_> = permutation.iter().map(|&i| i * 10).collect();
                prop_assert_eq!(items, expected);
            }
        }
    }
}
