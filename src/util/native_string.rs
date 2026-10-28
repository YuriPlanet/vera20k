//! Active gamemd narrow/wide string boundary helpers.

/// Zero-extend each narrow byte to one Unicode scalar.
///
/// This is the engine's ordinary internal conversion. It is deliberately not
/// UTF-8, CP1252, or the Windows active code page.
pub fn widen_bytes(bytes: &[u8]) -> String {
    // ASCII bytes are already their own UTF-8 encoding. Retail INIs and maps
    // are ASCII apart from a few EVA strings, so they copy as is.
    if bytes.is_ascii() {
        return String::from_utf8(bytes.to_vec()).expect("ASCII is UTF-8");
    }
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// The inverse of [`widen_bytes`]: each scalar back to its narrow byte. A
/// scalar above U+00FF, which no widened byte gives, becomes `?`.
pub fn narrow_widened(text: &str) -> Vec<u8> {
    text.chars()
        .map(|scalar| u8::try_from(u32::from(scalar)).unwrap_or(b'?'))
        .collect()
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

/// NativeString7350C0's low-byte UTF-16 conversion as consumed by a C-string
/// caller (for example Cooperative49B760). The ring's final byte is forced
/// NUL at index1023; an embedded zero low byte terminates the consumer sooner.
/// No ACP conversion, replacement character or UTF-8 encoding is involved.
pub fn narrow_utf16_c_string(units: impl IntoIterator<Item = u16>) -> Vec<u8> {
    units
        .into_iter()
        .take(0x3ff)
        .map(|unit| unit as u8)
        .take_while(|byte| *byte != 0)
        .collect()
}

/// CRT `_strcmpi` (`CRT__strcmpi`) in the C locale over narrow strings the
/// engine holds widened ([`widen_bytes`]): each character's low byte, with
/// `A..Z` folded to lower case, compared unsigned up to the first difference.
pub fn crt_strcmpi(left: &str, right: &str) -> std::cmp::Ordering {
    let fold = |c: char| (c as u32 as u8).to_ascii_lowercase();
    left.chars().map(fold).cmp(right.chars().map(fold))
}

/// `A..Z` folded to lower case: the C-locale branch of the wide CRT
/// comparisons below.
fn wide_ascii_lower(unit: u16) -> u16 {
    if (0x41..0x5B).contains(&unit) {
        unit + 0x20
    } else {
        unit
    }
}

/// CRT `_wcsicmp(left, right) == 0` (`0x007DD0F8`) in the C locale.
pub fn wcs_eq_ignore_case(left: &[u16], right: &[u16]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| wide_ascii_lower(*a) == wide_ascii_lower(*b))
}

/// CRT `_wcsnicmp(text, prefix, wcslen(prefix)) == 0` (`0x007DDE80`) in the
/// C locale.
pub fn wcs_starts_with_ignore_case(text: &[u16], prefix: &[u16]) -> bool {
    text.len() >= prefix.len() && wcs_eq_ignore_case(&text[..prefix.len()], prefix)
}

/// WWLib's wide `strtrim` (`wcstrim` `0x00727D60`), as the text it leaves.
///
/// Leading units up to `0x20` are removed by copying the rest down; trailing
/// ones are then cleared from the end while the unit now at the old first
/// kept index is not NUL. So when `lead` leading units are removed from a
/// string with at least `lead` kept units, the trailing trim stops at kept
/// index `lead` (with exactly `lead`, nothing trailing is trimmed).
pub fn wcstrim(units: &[u16]) -> &[u16] {
    let lead = units.iter().take_while(|unit| **unit <= 0x20).count();
    let kept = &units[lead..];
    let trimmed = kept
        .iter()
        .rposition(|unit| *unit > 0x20)
        .map_or(0, |i| i + 1);
    let end = if lead == 0 || lead > kept.len() {
        trimmed
    } else {
        trimmed.max(lead)
    };
    &kept[..end]
}

/// CRT `wcstoxl` (`0x007CB9CF`), the body of `wcstol` and `wcstoul`: leading
/// wide whitespace, an optional sign, then digits of `base` (0 picks 8, 10 or
/// 16 from a `0`/`0x` prefix). No digits gives 0; overflow gives `ULONG_MAX`
/// unsigned and `LONG_MAX`/`LONG_MIN` signed; a negative value is negated.
fn crt_wcstoxl(units: &[u16], mut base: u32, unsigned: bool) -> u32 {
    let unit_at = |index: usize| units.get(index).copied().unwrap_or(0);
    let mut index = 0;
    while matches!(unit_at(index), 0x09..=0x0D | 0x20) {
        index += 1;
    }
    let negative = unit_at(index) == u16::from(b'-');
    if negative || unit_at(index) == u16::from(b'+') {
        index += 1;
    }
    if base == 1 || base > 36 {
        return 0;
    }
    let hex_prefix = unit_at(index) == u16::from(b'0') && matches!(unit_at(index + 1), 0x58 | 0x78);
    if base == 0 {
        base = if unit_at(index) != u16::from(b'0') {
            10
        } else if hex_prefix {
            16
        } else {
            8
        };
    }
    if base == 16 && hex_prefix {
        index += 2;
    }
    let mut value: u32 = 0;
    let mut overflow = false;
    while let Some(digit) = char::from_u32(u32::from(unit_at(index)))
        .and_then(|c| c.to_digit(36))
        .filter(|digit| *digit < base)
    {
        match value.checked_mul(base).and_then(|v| v.checked_add(digit)) {
            Some(next) => value = next,
            None => overflow = true,
        }
        index += 1;
    }
    let limit = if negative { 0x8000_0000 } else { 0x7FFF_FFFF };
    if overflow || (!unsigned && value > limit) {
        return if unsigned {
            u32::MAX
        } else if negative {
            0x8000_0000
        } else {
            0x7FFF_FFFF
        };
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// CRT `wcstol(text, NULL, base)` (`0x007CB9B8`).
pub fn crt_wcstol(units: &[u16], base: u32) -> i32 {
    crt_wcstoxl(units, base, false) as i32
}

/// CRT `wcstoul(text, NULL, base)` (`0x007CBB76`).
pub fn crt_wcstoul(units: &[u16], base: u32) -> u32 {
    crt_wcstoxl(units, base, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn wcstrim_stops_its_trailing_pass_at_the_old_first_kept_index() {
        let trim = |text: &str| String::from_utf16(wcstrim(&wide(text))).unwrap();
        assert_eq!(trim(" a b \t"), "a b");
        // Two leading units and two kept ones: the unit at kept index 2 is
        // the copied terminator, so nothing trailing is cleared.
        assert_eq!(trim("  a "), "a ");
        // Trailing clearing may not go below kept index `lead`.
        assert_eq!(trim("   ab    "), "ab ");
        assert_eq!(trim("    "), "");
    }

    #[test]
    fn wcstol_reads_the_base_prefix_sign_and_overflow_as_the_crt() {
        assert_eq!(crt_wcstol(&wide(" \t-12x"), 10), -12);
        assert_eq!(crt_wcstol(&wide("0x1F"), 0), 31);
        assert_eq!(crt_wcstol(&wide("017"), 0), 15);
        assert_eq!(crt_wcstol(&wide("99999999999"), 10), i32::MAX);
        assert_eq!(crt_wcstol(&wide("-2147483648"), 10), i32::MIN);
        assert_eq!(crt_wcstoul(&wide("-1"), 10), u32::MAX);
        assert_eq!(crt_wcstoul(&wide("x"), 10), 0);
    }
}
