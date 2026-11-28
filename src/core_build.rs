//! Build script of `vera20k-core`: test group cfgs and identities for installed ice resources.

#[path = "test_group_cfgs.rs"]
mod test_group_cfgs;

#[path = "ice_build.rs"]
mod ice_build;

#[allow(dead_code)]
#[path = "util/sha256.rs"]
mod sha256;

fn main() {
    println!("cargo::rerun-if-changed=core_build.rs");
    println!("cargo::rerun-if-changed=test_group_cfgs.rs");
    test_group_cfgs::emit();

    // A match may only load the terrain data that shipped with its build.
    // Generate the expected lengths and hashes; the files travel separately.
    println!("cargo::rerun-if-changed=util/sha256.rs");
    println!("cargo::rerun-if-changed=ice_build.rs");
    let files = ice_build::identities(std::path::Path::new("../Terrain"));
    let out =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(out.join("ice_file_identities.rs"), format!("{files:?}"))
        .expect("write ice file identities");
}
