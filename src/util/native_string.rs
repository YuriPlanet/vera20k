//! Active gamemd narrow/wide string boundary helpers.

/// Zero-extend each narrow byte to one Unicode scalar.
///
/// This is the engine's ordinary internal conversion. It is deliberately not
/// UTF-8, CP1252, or the Windows active code page.
pub fn widen_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// Encode UTF-16 text through the Windows active ANSI code page.
#[cfg(windows)]
pub fn acp_encode(text: &str) -> Vec<u8> {
    const CP_ACP: u32 = 0;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            wide: *const u16,
            wide_len: i32,
            narrow: *mut u8,
            narrow_len: i32,
            default_char: *const u8,
            used_default_char: *mut i32,
        ) -> i32;
    }

    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return Vec::new();
    }
    let Ok(wide_len) = i32::try_from(wide.len()) else {
        return Vec::new();
    };
    // SAFETY: the sizing call reads `wide`; the conversion call writes the
    // exactly-sized initialized Vec allocation.
    unsafe {
        let narrow_len = WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            wide_len,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        );
        if narrow_len <= 0 {
            return Vec::new();
        }
        let mut narrow = vec![0u8; narrow_len as usize];
        if WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            wide_len,
            narrow.as_mut_ptr(),
            narrow_len,
            std::ptr::null(),
            std::ptr::null_mut(),
        ) <= 0
        {
            return Vec::new();
        }
        narrow
    }
}

#[cfg(not(windows))]
pub fn acp_encode(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// Decode bytes returned by an ANSI Win32 API through CP_ACP.
#[cfg(windows)]
pub fn acp_decode(bytes: &[u8]) -> String {
    const CP_ACP: u32 = 0;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MultiByteToWideChar(
            code_page: u32,
            flags: u32,
            narrow: *const u8,
            narrow_len: i32,
            wide: *mut u16,
            wide_len: i32,
        ) -> i32;
    }
    if bytes.is_empty() {
        return String::new();
    }
    let Ok(narrow_len) = i32::try_from(bytes.len()) else {
        return String::new();
    };
    // SAFETY: the sizing call reads `bytes`; the conversion call writes the
    // exactly-sized initialized Vec allocation.
    unsafe {
        let wide_len = MultiByteToWideChar(
            CP_ACP,
            0,
            bytes.as_ptr(),
            narrow_len,
            std::ptr::null_mut(),
            0,
        );
        if wide_len <= 0 {
            return String::new();
        }
        let mut wide = vec![0u16; wide_len as usize];
        if MultiByteToWideChar(
            CP_ACP,
            0,
            bytes.as_ptr(),
            narrow_len,
            wide.as_mut_ptr(),
            wide_len,
        ) <= 0
        {
            return String::new();
        }
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(not(windows))]
pub fn acp_decode(bytes: &[u8]) -> String {
    widen_bytes(bytes)
}

/// Round-trip edit-control text through the Windows active ANSI code page.
///
/// Native owner-draw edits cross this boundary in both directions. Characters
/// that the current ACP cannot represent are replaced by the platform default
/// byte (normally `?`).
#[cfg(windows)]
pub fn acp_round_trip(text: &str) -> String {
    acp_decode(&acp_encode(text))
}

#[cfg(not(windows))]
pub fn acp_round_trip(text: &str) -> String {
    text.to_string()
}
