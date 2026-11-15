//! Build-script half of the test groups (tools/README.md, "Test groups").
//!
//! Both workspace crates' build scripts include this file. A test module is
//! declared `#[cfg(all(test, tests_<group>))]`; this emits each group's cfg.
//! Every group is on unless the package's `test-focus` feature narrows the
//! build to the groups whose `tests-<group>` feature is on, so a plain
//! `cargo test` always compiles the full suite.

/// Emit the `tests_<group>` cfgs for the package being built. The groups are
/// the `tests-*` features of its own Cargo.toml.
pub fn emit() {
    let manifest = std::path::Path::new(
        &std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"),
    )
    .join("Cargo.toml");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).expect("read package manifest");
    let focus = std::env::var_os("CARGO_FEATURE_TEST_FOCUS").is_some();
    println!("cargo::rustc-check-cfg=cfg(test_focus)");
    if focus {
        println!("cargo::rustc-cfg=test_focus");
    }
    for line in text.lines() {
        let Some((name, _)) = line.split_once('=') else {
            continue;
        };
        let Some(group) = name.trim().strip_prefix("tests-") else {
            continue;
        };
        let cfg = format!("tests_{}", group.replace('-', "_"));
        println!("cargo::rustc-check-cfg=cfg({cfg})");
        let feature = format!("CARGO_FEATURE_{}", cfg.to_uppercase());
        if !focus || std::env::var_os(feature).is_some() {
            println!("cargo::rustc-cfg={cfg}");
        }
    }
}
