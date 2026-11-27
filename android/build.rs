fn main() {
    // The game's threads get bigger stacks through `__wrap_pthread_create`.
    // Android only: the host linkers a desktop `cargo test --workspace` uses
    // for this crate's test binary have no `--wrap`.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg=-Wl,--wrap=pthread_create");
    }
}
