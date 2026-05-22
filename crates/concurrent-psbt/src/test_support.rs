//! Fixtures shared by tests in several modules.

use crate::input::Input;
use bitcoin::absolute::{Height, Time};
use bitcoin::hashes::Hash;

/// Two inputs whose lock time requirements conflict: the first requires a
/// time-based lock time and the second a height-based one.
pub(crate) fn conflicting_lock_time_inputs() -> [Input; 2] {
    let [mut time, mut height] = [1, 2].map(|tag| {
        Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([tag; 32]),
            vout: 0,
        })
    });
    time.min_time = Some(Time::MIN);
    height.min_height = Some(Height::MIN);
    [time, height]
}
