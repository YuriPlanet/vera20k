fn main() {
    println!("cargo:rustc-link-arg=-Wl,--wrap=pthread_create");
}
