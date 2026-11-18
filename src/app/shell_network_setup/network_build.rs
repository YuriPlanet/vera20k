//! What one source file adds to `VERA20K_NETWORK_BUILD`, the build hash
//! [`super::agreement::version`] carries. build.rs includes this file; the
//! game compiles it only for its test.

/// Appends the file at `path` below the checkout, holding `bytes`, to the
/// build hash's input, so that one source gives one hash on every platform:
/// the path with '/' between its parts, and the bytes with each CRLF as LF,
/// since Git for Windows checks text files out with CRLF line endings.
pub fn add_source(input: &mut Vec<u8>, path: &str, bytes: &[u8]) {
    input.extend_from_slice(path.replace('\\', "/").as_bytes());
    input.push(0);
    let mut lf = Vec::with_capacity(bytes.len());
    for (index, &byte) in bytes.iter().enumerate() {
        if byte != b'\r' || bytes.get(index + 1) != Some(&b'\n') {
            lf.push(byte);
        }
    }
    input.extend_from_slice(&(lf.len() as u64).to_le_bytes());
    input.extend_from_slice(&lf);
}

/// A Windows checkout, with '\' between path parts and CRLF line endings,
/// adds what an LF checkout does; a lone CR still counts.
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
}
