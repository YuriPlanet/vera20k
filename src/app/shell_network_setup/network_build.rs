//! What a source file and the enabled Cargo features add to
//! `VERA20K_NETWORK_BUILD`, the build hash [`super::agreement::version`]
//! carries. build.rs includes this file; the game compiles it only for its
//! tests.

use std::io::BufRead;

/// Appends the enabled Cargo features to the build hash's input and returns
/// them, sorted. `variables` are the names of build.rs's environment
/// variables, where Cargo sets `CARGO_FEATURE_<NAME>` for each enabled
/// feature. Two kinds stay out, since neither changes how the game plays, so
/// builds that differ only in them play together:
///
/// - the test groups, which choose the tests compiled (src/test_group_cfgs.rs);
/// - `wasm-threads`, the browser package's Web Worker pool (docs/sim-thread.md,
///   "The browser"). With it a tick runs the `sim::parallel` a desktop build
///   runs, and without it `sim::parallel_web`, which returns the same results
///   in the same order. A launcher seats a threaded browser package and a
///   desktop build of the package's other features in one room, and every
///   network start compares this hash (`StartupAgreement`).
pub fn add_features(
    input: &mut Vec<u8>,
    variables: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut features: Vec<_> = variables
        .into_iter()
        .filter(|name| {
            name.starts_with("CARGO_FEATURE_")
                && !name.starts_with("CARGO_FEATURE_TEST_FOCUS")
                && !name.starts_with("CARGO_FEATURE_TESTS_")
                && name != "CARGO_FEATURE_WASM_THREADS"
        })
        .collect();
    features.sort();
    for feature in &features {
        input.extend_from_slice(feature.as_bytes());
        input.push(0);
    }
    features
}

/// Appends the file at `path` below the checkout, holding `bytes`, to the
/// build hash's input, so that one source gives one hash on every platform:
/// the path with '/' between its parts, and the bytes with each CRLF as LF,
/// since Git for Windows checks text files out with CRLF line endings.
pub fn add_source(input: &mut Vec<u8>, path: &str, bytes: &[u8]) {
    input.extend_from_slice(path.replace('\\', "/").as_bytes());
    input.push(0);
    // `read_until` finds each CR with std's optimized search. Cargo compiles
    // build.rs unoptimized, where a loop over every byte of the ~100 MB of
    // sources took 0.7 s on each rerun and this under 0.1 s (measured on
    // 2026-11-18).
    let mut lf = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        rest.read_until(b'\r', &mut lf)
            .expect("reading a byte slice cannot fail");
        if lf.last() == Some(&b'\r') && rest.first() == Some(&b'\n') {
            lf.pop();
        }
    }
    input.extend_from_slice(&(lf.len() as u64).to_le_bytes());
    input.extend_from_slice(&lf);
}

/// A Windows checkout, with '\' between path parts and CRLF line endings,
/// adds what an LF checkout does; a lone CR still counts, at the end too.
#[test]
fn a_windows_checkout_adds_what_an_lf_checkout_does() {
    let add = |path, bytes: &[u8]| {
        let mut input = Vec::new();
        add_source(&mut input, path, bytes);
        input
    };
    assert_eq!(
        add("src\\main.rs", b"fn main() {}\r\n// a\r\n"),
        add("src/main.rs", b"fn main() {}\n// a\n")
    );
    assert_ne!(add("src/main.rs", b"a\rb\n"), add("src/main.rs", b"ab\n"));
    assert_ne!(add("src/main.rs", b"a\r"), add("src/main.rs", b"a"));
}

/// A threaded browser package (`--network --threads`) adds the features a
/// desktop build with `--features wasm-net` does, whatever tests either
/// compiles; a LAN package still adds its own.
#[test]
fn the_thread_pool_and_the_test_groups_add_no_feature() {
    let add = |variables: &[&str]| {
        let mut input = Vec::new();
        add_features(&mut input, variables.iter().map(|name| name.to_string()));
        input
    };
    let desktop = add(&["CARGO_FEATURE_WASM", "CARGO_FEATURE_WASM_NET", "PATH"]);
    assert_eq!(
        add(&[
            "CARGO_FEATURE_WASM_THREADS",
            "CARGO_FEATURE_WASM_NET",
            "CARGO_FEATURE_WASM",
            "CARGO_FEATURE_TESTS_APP",
            "CARGO_FEATURE_TEST_FOCUS",
        ]),
        desktop
    );
    assert_ne!(
        add(&[
            "CARGO_FEATURE_WASM",
            "CARGO_FEATURE_WASM_LAN",
            "CARGO_FEATURE_WASM_NET",
        ]),
        desktop
    );
}
