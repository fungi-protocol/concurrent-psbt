fn main() {
    if let Err(error) = transport_nym::capnp::serve_stdio() {
        eprintln!("ptj-transport-nym: {error}");
    }
}
