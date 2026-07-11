use std::fs;
use std::path::Path;

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("contract input is readable")
}

#[test]
fn default_manifest_is_a_complete_standalone_backend() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let root_manifest = read(root.join("Cargo.toml"));
    let manifest = read(crate_dir.join("Cargo.toml"));
    let source = read(crate_dir.join("src/lib.rs"));

    assert!(
        root_manifest.contains("\"crates/transport-nym\""),
        "the root workspace must explicitly exclude transport-nym"
    );
    assert!(
        manifest.contains("[workspace]"),
        "transport-nym must own its Cargo.lock"
    );
    assert!(
        manifest.contains("nym-sdk = \"1.21.2\""),
        "the default library must depend on the grounded nym-sdk version"
    );
    assert!(
        !manifest.contains("nym = ["),
        "the backend must not be hidden behind an internal nym feature"
    );
    assert!(
        !source.contains("feature = \"nym\""),
        "the default source must compile the real backend"
    );
    assert!(
        !source.contains("BUILT_WITHOUT_NYM"),
        "the default library must not contain a non-functional skeleton"
    );
    assert!(
        !source.contains("wrap_outgoing") && !source.contains("unwrap_incoming"),
        "native mixnet messages must preserve their own message boundaries"
    );
}
