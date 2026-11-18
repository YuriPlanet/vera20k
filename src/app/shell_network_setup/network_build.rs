//! What one source file adds to `VERA20K_NETWORK_BUILD`, the build hash
//! [`super::agreement::version`] carries. build.rs includes this file; the
//! game compiles it only for its test.

use std::io::BufRead;

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
