//! `lotse-hook`: agents' hooks call this; it forwards the hook JSON to the `lotsed` socket. WP2 implements it.

fn main() {
    println!(
        "lotse-hook {} (proto {}): not implemented yet",
        env!("CARGO_PKG_VERSION"),
        lotse_proto::PROTO_VERSION
    );
}
