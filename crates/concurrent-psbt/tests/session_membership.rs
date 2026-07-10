use bitcoin::transaction;
use concurrent_psbt::payments::membership::SharedSession;
use concurrent_psbt::tx::UnorderedPsbt;
use concurrent_psbt::{Join, input::InputSet, output::OutputSet};
use psbt_v2::v2::Global;

fn fragment(version: transaction::Version) -> concurrent_psbt::tx::ResultUnorderedPsbt {
    UnorderedPsbt {
        global: Global {
            tx_version: version,
            ..Default::default()
        },
        inputs: InputSet::default(),
        outputs: OutputSet::default(),
    }
    .wrap()
}

#[test]
fn joining_sessions_unions_parties() {
    let alice = [0x0a; 32];
    let bob = [0x0b; 32];

    let joined = SharedSession::promote(alice, fragment(transaction::Version::ONE)).join(
        SharedSession::promote(bob, fragment(transaction::Version::ONE)),
    );

    assert_eq!(joined.parties().copied().collect::<Vec<_>>(), [alice, bob]);
}

#[test]
fn joining_sessions_preserves_psbt_conflicts() {
    let alice = [0x0a; 32];
    let bob = [0x0b; 32];

    let joined = SharedSession::promote(alice, fragment(transaction::Version::ONE)).join(
        SharedSession::promote(bob, fragment(transaction::Version::TWO)),
    );

    assert!(joined.state().clone().try_unwrap().is_err());
}
