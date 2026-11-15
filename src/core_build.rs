//! Build script of `vera20k-core`: the test group cfgs.

#[path = "test_group_cfgs.rs"]
mod test_group_cfgs;

fn main() {
    println!("cargo::rerun-if-changed=core_build.rs");
    println!("cargo::rerun-if-changed=test_group_cfgs.rs");
    test_group_cfgs::emit();
}
