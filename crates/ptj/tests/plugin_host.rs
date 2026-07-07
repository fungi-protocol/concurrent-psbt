//! Plugin-host loopback tests: spawn the REAL `ptj-fake-plugin` child (an
//! in-memory plugin speaking the full transport-plugin-api protocol — see
//! src/bin/fake_plugin.rs) and drive `PluginTransport` through the
//! `Transport` seam. This exercises spawn, the capnp twoparty vat over child
//! stdin/stdout, handshake/version negotiation, config passthrough,
//! publish/collect marshalling, and shutdown-on-drop — end to end, over real
//! pipes, with no network.
#![cfg(feature = "plugin-transports")]

use std::path::PathBuf;

use ptj::PluginTransport;
use transport_core::Transport;

/// The fake plugin binary cargo built alongside these tests.
fn fake_plugin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ptj-fake-plugin"))
}

/// The tests' async->sync edge, mirroring `commands::sync::drive_async`:
/// the host handle is `Send` and runtime-agnostic, so a plain current-thread
/// runtime drives it.
fn drive<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("building the test runtime")
        .block_on(future)
}

fn config(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// The core loopback: bytes published through the host land in the plugin
/// and come back on collect — including our own publishes, as a fresh
/// snapshot per call (the Transport contract).
#[test]
fn publish_collect_roundtrips_over_child_stdio() {
    let mut transport =
        PluginTransport::spawn(&fake_plugin(), Vec::new()).expect("spawn + handshake");
    drive(async {
        transport.publish(b"alpha".to_vec()).await.unwrap();
        transport.publish(b"beta".to_vec()).await.unwrap();
        let expected = vec![b"alpha".to_vec(), b"beta".to_vec()];
        assert_eq!(transport.collect().await.unwrap(), expected);
        // Snapshot semantics: polling again returns the same frontier.
        assert_eq!(transport.collect().await.unwrap(), expected);
    });
    // Dropping the handle closes the child's stdin and joins the actor; the
    // test completing (rather than hanging) IS the shutdown assertion.
    drop(transport);
}

/// An attributable plugin is driven through its own interface and its
/// sender ids are dropped on collect (the `Attributed` precedent) — and the
/// `fake-kind` entry proves config passthrough reaches the plugin.
#[test]
fn attributable_plugin_collects_bare_bytes() {
    let mut transport = PluginTransport::spawn(
        &fake_plugin(),
        config(&[("fake-kind", "attributable")]),
    )
    .expect("spawn + attributable handshake");
    drive(async {
        transport.publish(b"attributed".to_vec()).await.unwrap();
        // Bare bytes out: the b"fake-sender" ids were dropped by the host.
        assert_eq!(
            transport.collect().await.unwrap(),
            vec![b"attributed".to_vec()]
        );
    });
}

/// A plugin answering the wrong protocol version is refused with BOTH
/// versions named.
#[test]
fn protocol_version_mismatch_names_both_versions() {
    // `PluginTransport` is not `Debug`: match errors out instead of
    // `expect_err` (here and below).
    let Err(error) = PluginTransport::spawn(&fake_plugin(), config(&[("fake-version", "99")]))
    else {
        panic!("host must refuse a version-99 plugin");
    };
    let text = error.to_string();
    assert!(text.contains("version mismatch"), "got: {text}");
    assert!(
        text.contains("host speaks 1") && text.contains("answered 99"),
        "got: {text}"
    );
}

/// A plugin's structured refusal (HandshakeResult.err) surfaces its message.
#[test]
fn plugin_refusal_surfaces_the_plugin_message() {
    let Err(error) = PluginTransport::spawn(
        &fake_plugin(),
        config(&[("fake-refuse", "credentials file missing")]),
    ) else {
        panic!("host must surface the refusal");
    };
    let text = error.to_string();
    assert!(text.contains("refused"), "got: {text}");
    assert!(text.contains("credentials file missing"), "got: {text}");
}

/// A binary that is not a plugin at all (here: ptj itself, which exits
/// after printing usage) fails the HANDSHAKE stage with the binary named —
/// not a hang, not a panic.
#[test]
fn non_plugin_binary_fails_the_handshake() {
    let not_a_plugin = PathBuf::from(env!("CARGO_BIN_EXE_ptj"));
    let Err(error) = PluginTransport::spawn(&not_a_plugin, Vec::new()) else {
        panic!("ptj itself is not a plugin");
    };
    let text = error.to_string();
    assert!(text.contains("handshake"), "got: {text}");
    assert!(text.contains("ptj"), "got: {text}");
}
