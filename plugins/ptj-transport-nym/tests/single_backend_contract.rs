use std::fs;
use std::path::Path;

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("contract input is readable")
}

#[test]
fn plugin_is_a_thin_wrapper_over_the_library_adapter() {
    let plugin_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = plugin_dir.join("../..");
    let transport_dir = repo.join("crates/transport-nym");
    let transport_manifest = read(transport_dir.join("Cargo.toml"));
    let transport_source = read(transport_dir.join("src/lib.rs"));
    let plugin_manifest = read(plugin_dir.join("Cargo.toml"));
    let plugin_main = read(plugin_dir.join("src/main.rs"));

    let transport_ignore = fs::read_to_string(transport_dir.join(".gitignore")).unwrap_or_default();
    assert!(
        transport_ignore.lines().any(|line| line == "/target"),
        "the standalone package must ignore its own Cargo target directory"
    );
    assert!(
        transport_manifest.contains("capnp = [\"dep:transport-plugin-api\""),
        "capnp must enable a real adapter dependency"
    );
    assert!(
        transport_manifest.contains(
            "transport-plugin-api = { path = \"../transport-plugin-api\", optional = true }"
        ),
        "the generic plugin API must be optional outside capnp builds"
    );
    assert!(
        transport_source.contains("pub mod capnp"),
        "transport-nym must export the adapter"
    );
    assert!(
        plugin_manifest.contains(
            "transport-nym = { path = \"../../crates/transport-nym\", features = [\"capnp\"] }"
        ),
        "the binary must select the library adapter"
    );
    assert!(
        !plugin_manifest.contains("nym-sdk"),
        "the binary must not carry a second backend dependency"
    );
    assert!(
        plugin_main.contains("transport_nym::capnp::serve_stdio"),
        "main must delegate stdio serving to transport-nym"
    );
    assert!(
        !plugin_dir.join("src/nym.rs").exists(),
        "the duplicated plugin backend must be removed"
    );
}
