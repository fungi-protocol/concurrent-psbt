use transport_arti::{ArtiConfig, ArtiTransport};
use transport_core::{AnonymousChannel, Transport};

#[test]
fn default_build_exposes_the_live_arti_backend() {
    fn assert_channel<T: AnonymousChannel + Transport>() {}
    assert_channel::<ArtiTransport>();

    let _constructor: fn(ArtiConfig) -> transport_core::Result<ArtiTransport> = ArtiTransport::new;

    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read transport-arti manifest");

    assert!(
        !manifest.lines().any(|line| {
            let line = line.trim();
            line == "arti = []" || line.starts_with("arti = [")
        }),
        "transport-arti must not hide its backend behind an `arti` feature"
    );

    for dependency in [
        "arti-client",
        "tor-rtcompat",
        "tor-hsservice",
        "tor-proto",
        "tor-cell",
        "tokio",
        "futures",
        "safelog",
    ] {
        let declaration = manifest
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("{dependency} =")))
            .unwrap_or_else(|| panic!("missing unconditional `{dependency}` dependency"));
        assert!(
            !declaration.contains("optional = true"),
            "`{dependency}` must be unconditional: {declaration}"
        );
    }
}
