use psbt_v2::v2::Psbt;

use super::SortError;
use crate::input::Input;
use crate::output::Output;
use crate::tx::UnorderedPsbt;

pub(super) fn ordered_psbt<I, O>(
    psbt: UnorderedPsbt,
    mut input_key: I,
    mut output_key: O,
) -> Result<Psbt, SortError>
where
    I: FnMut(&Input) -> Result<Vec<u8>, SortError>,
    O: FnMut(&Output) -> Result<Vec<u8>, SortError>,
{
    let mut inputs: Vec<_> = psbt.inputs.into_iter().collect();
    let mut outputs: Vec<_> = psbt.outputs.into_iter().collect();

    let mut input_keys = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| input_key(input).map(|key| (key, i)))
        .collect::<Result<Vec<_>, _>>()?;

    let mut output_keys = outputs
        .iter()
        .enumerate()
        .map(|(i, output)| output_key(output).map(|key| (key, i)))
        .collect::<Result<Vec<_>, _>>()?;

    input_keys.sort_by(|(left, _), (right, _)| left.cmp(right));
    output_keys.sort_by(|(left, _), (right, _)| left.cmp(right));

    for keys in [&input_keys, &output_keys] {
        if let Some(pair) = keys.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(SortError::DuplicateSortKey(pair[0].0.clone()));
        }
    }

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
/// so `order` no longer describes the permutation afterwards.
fn apply_permutation<T, K>(items: &mut [T], order: &mut [(K, usize)]) {
    for start in 0..order.len() {
        let mut position = start;
        loop {
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
