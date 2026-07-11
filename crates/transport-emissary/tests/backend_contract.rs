#[test]
fn embedded_backend_is_compiled_unconditionally() {
    let manifest = include_str!("../Cargo.toml");

    assert!(
        !manifest.lines().any(|line| line.starts_with("emissary =")),
        "transport-emissary must not define an internal `emissary` feature"
    );

    for dependency in [
        "emissary-core",
        "emissary-util",
        "yosemite",
        "rand",
        "tokio",
    ] {
        let declaration = manifest
            .lines()
            .find(|line| line.starts_with(&format!("{dependency} =")))
            .unwrap_or_else(|| panic!("missing required backend dependency `{dependency}`"));
        assert!(
            !declaration.contains("optional = true"),
            "backend dependency `{dependency}` must be unconditional"
        );
    }
}
