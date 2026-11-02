//! `lotsed`: owns PTYs and state, serves the Unix socket. WP1 implements it.
//!
//! Alpha rules: Unix socket only (mode 0600 in a 0700 directory), token first, no TCP listener.

fn main() {
    println!(
        "lotsed {} (proto {}): not implemented yet",
        env!("CARGO_PKG_VERSION"),
        lotse_proto::PROTO_VERSION
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn links_the_shared_contracts() {
        assert_eq!(lotse_proto::PROTO_VERSION, 1);
    }
}
