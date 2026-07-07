//! Generate the Rust bindings for schema/transport.capnp at build time.
//!
//! capnpc shells out to the `capnp` tool (provided by the nix devshell —
//! see nix/devshell.nix) and writes `transport_capnp.rs` into OUT_DIR;
//! src/lib.rs `include!`s it. The schema file is the single source of truth
//! for the host<->plugin wire contract.

fn main() {
    capnpc::CompilerCommand::new()
        // Strip the directory from the generated module path: OUT_DIR gets
        // `transport_capnp.rs`, not `schema/transport_capnp.rs`.
        .src_prefix("schema")
        .file("schema/transport.capnp")
        .run()
        .expect("compiling schema/transport.capnp (is the `capnp` tool on PATH? it comes from the nix devshell)");
}
