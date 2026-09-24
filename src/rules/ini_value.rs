//! Typed INI accessor service — the gamemd CCINIClass `ReadX` analog.
//!
//! Sits on top of the raw `IniSection` store (the "INIClass" analog). Reproduces
//! the gamemd parse CONTRACT bit-for-bit on the resolved value: $xx/xxh hex,
//! C-atoi leniency, first-char bool, '%'-anywhere ×0.01 double, strtrim ≤0x20.
//!
//! INVARIANT: the raw loader omits empty keys and empty values. A stored
//! nonempty key returns its parsed value (malformed numeric text may still
//! parse as zero); `default` is returned when a key is absent or omitted.
//!
//! ## Dependency rules
//! - rules/ only: depends on `crate::rules::ini_parser`. No sim/render/ui/audio/net.
//! - Returns un-truncated f64 from `read_double`; the single f64->SimFixed
//!   conversion stays in `util::fixed_math`. No float enters sim/.

use crate::rules::ini_parser::IniSection;

/// 0x20 = ASCII space; gamemd `strtrim` strips bytes <= 0x20 (space + all ASCII
/// control) at BOTH ends — NOT Unicode whitespace.
const STRTRIM_MAX: u8 = 0x20;

impl IniSection {
    fn fold_rules_values<T: Copy>(
        &self,
        key: &str,
        default: T,
        mut apply: impl FnMut(T, &str) -> T,
    ) -> T {
        if let Some(values) = self.projected_values(key) {
            values
                .iter()
                .fold(default, |current, raw| apply(current, raw))
        } else {
            self.get(key).map_or(default, |raw| apply(default, raw))
        }
    }

    /// ReadInt (P1–P4, P18): `$xx`/`xxh` (case-insensitive `h`) hex, else C-atoi
    /// leniency. Default ONLY on absent key. Present-but-nonnumeric -> atoi (0).
    pub fn read_int(&self, key: &str, default: i32) -> i32 {
        self.fold_rules_values(key, default, parse_read_int)
    }

    /// ReadBool (P6, P18): `toupper(first char)` in {'1','T','Y'}=true,
    /// {'0','F','N'}=false, else default. `on`/`off` (first char 'o') -> default.
    /// Present-empty (no first char) -> default.
    pub fn read_bool(&self, key: &str, default: bool) -> bool {
        self.fold_rules_values(key, default, parse_read_bool)
    }

    /// ReadDouble (P7): sscanf "%f" (leading float, single-precision) widened to
    /// f64, then ×0.01 iff the value string contains '%' ANYWHERE. Returns the
    /// gamemd double UN-truncated; the consumer truncates toward zero at ITS
    /// boundary (never `.round()` / never truncate here). Default ONLY on absent.
    /// Present junk is absent from stock retail data; native exposes stale
    /// 32-bit ABI argument bits on failed `%f`, while Rust deterministically
    /// returns zero rather than importing that non-portable accident.
    pub fn read_double(&self, key: &str, default: f64) -> f64 {
        self.fold_rules_values(key, default, |_current, raw| parse_read_double(raw))
    }

    /// ReadString (P5, P18): copy at most `capacity - 1` bytes, force the final
    /// NUL, then trim bytes ≤0x20 at both ends. Capacities are caller-specific
    /// in retail, so they are explicit here too.
    ///
    /// `CCINIClass__ReadString @ 0x00528A10` is `strncpy(dst, src, capacity)`
    /// followed by `dst[capacity - 1] = 0`, so the cut is by BYTE, not by
    /// character. Truncating by `char` would keep text native discards on any
    /// value whose first `capacity - 1` characters span more bytes than that.
    pub fn read_string(&self, key: &str, default: &str, capacity: usize) -> String {
        if capacity == 0 {
            return String::new();
        }
        let raw = self.get(key).unwrap_or(default);
        strtrim_ascii(truncate_bytes(raw, capacity - 1)).to_string()
    }

    /// Read3Int (P8): comma "%d,%d,%d". All-defaults on ABSENT key. Each field
    /// atoi-lenient; missing trailing fields keep the corresponding default.
    #[cfg(test)]
    pub fn read_3int(&self, key: &str, default: [i32; 3]) -> [i32; 3] {
        self.fold_rules_values(key, default, |mut current, raw| {
            for (index, token) in strtrim_ascii(raw).split(',').enumerate().take(3) {
                current[index] = atoi_lenient(strtrim_ascii(token));
            }
            current
        })
    }

    /// ReadMinMax (P8): comma "%d,%d". All-defaults on ABSENT key.
    pub fn read_minmax(&self, key: &str, default: [i32; 2]) -> [i32; 2] {
        self.fold_rules_values(key, default, |mut current, raw| {
            for (index, token) in strtrim_ascii(raw).split(',').enumerate().take(2) {
                current[index] = atoi_lenient(strtrim_ascii(token));
            }
            current
        })
    }

    /// ReadPoint/ReadSize (P9, COMMA): "%d,%d". All-defaults on ABSENT key.
    #[cfg(test)]
    pub fn read_point(&self, key: &str, default: (i32, i32)) -> (i32, i32) {
        let [x, y] = self.read_minmax(key, [default.0, default.1]);
        (x, y)
    }

    /// ReadRect (P9, COMMA): "%d,%d,%d,%d". gamemd seeds "0,0,0,0" so missing
    /// fields keep the default component; all-defaults on ABSENT key.
    #[cfg(test)]
    pub fn read_rect(&self, key: &str, default: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
        let current = [default.0, default.1, default.2, default.3];
        let out = self.fold_rules_values(key, current, |mut current, raw| {
            for (index, token) in strtrim_ascii(raw).split(',').enumerate().take(4) {
                current[index] = atoi_lenient(strtrim_ascii(token));
            }
            current
        });
        (out[0], out[1], out[2], out[3])
    }

    /// ReadColorRGB (P21): COMMA "%d,%d,%d" -> [u8;3]. Per-component plain %d
    /// (stops at first non-digit; NO atoi-leniency beyond sign, NO hex). Default
    /// RGB on absent key or short value; component byte-narrowed to u8 (gamemd
    /// packs the sscanf int into a byte).
    ///
    /// `atoi_lenient` matches sscanf `%d` over the whole stock domain (leading
    /// sign + decimal digits, stop at non-digit) — corpus-confirmed ZERO `$`/`h`
    /// triplet components (plan-review C-R3). The `$`/`h` hex lives in `read_int`,
    /// NOT in `atoi_lenient`, so reusing it here stays faithful to `%d`.
    pub fn read_color_rgb(&self, key: &str, default: [u8; 3]) -> [u8; 3] {
        self.fold_rules_values(key, default, |mut current, raw| {
            for (index, token) in strtrim_ascii(raw).split(',').enumerate().take(3) {
                current[index] = atoi_lenient(strtrim_ascii(token)) as u8;
            }
            current
        })
    }

    /// ReadSpeed (P19): `read_int(-1)` sentinel; -1 -> default; else clamp 0..100,
    /// `(v<<8)/100` truncate-toward-zero (Rust i32 `/` truncates toward 0),
    /// clamp255. `100→255`, `50→128`, `7→17`, `0→0`. (ledger #18)
    ///
    /// NB present-empty `Speed=` -> `read_int("")` = atoi("") = 0 (NOT the -1
    /// sentinel) -> `(0<<8)/100` = 0, NOT the call-site default. Correct per
    /// P4/P18; corpus harness scans stock for present-empty Speed/Range.
    ///
    /// Retail provenance: INI speed conversion — `CCINIClass__ReadSpeed` @ `0x00474810`.
    #[cfg(test)]
    pub fn read_speed(&self, key: &str, default: i32) -> i32 {
        self.fold_rules_values(key, default, |current, raw| {
            let parsed = parse_read_int(-1, raw);
            if parsed == -1 {
                current
            } else {
                let capped = parsed.clamp(0, 100);
                let scaled = (capped << 8) / 100;
                scaled.min(255)
            }
        })
    }

    /// `CCINIClass__ReadRange @ 0x00474620`: read the key through
    /// `ReadDouble` with a hardcoded `-1.0` default (`0x00474628`), compare
    /// against `0x007E4900` (= `-1.0`) and return the CALLER's default
    /// unconverted on a match — covering both an absent key and a literal `-1`.
    ///
    /// Otherwise the value is a CELL count that the reader converts to leptons:
    /// `FMUL double ptr [0x007E1710]` at `0x0047464C`, where that address holds
    /// `0x4070000000000000` = `256.0`, then `Math__ftol @ 0x007C5F00`, whose
    /// control word `0x0E7F` selects chop. `f64 as i32` truncates toward zero
    /// like it does, and NOT `util::sim_to_i32`, which floors toward −∞ (DRIFT
    /// on negatives, ledger #18).
    ///
    /// Two residuals at the extremes, both out of reach of retail data:
    ///
    /// * `Math__ftol` executes `FISTP qword` and the caller keeps only the low
    ///   dword, so a magnitude past `i32` WRAPS while `f64 as i32` saturates.
    ///   That band starts at |value| >= 8388608 cells and ends at 2^63, beyond
    ///   which `FISTP` stores integer-indefinite and the low dword is zero.
    ///   `±inf` therefore yields 0 natively against `i32::MAX` here.
    /// * A NaN never reaches `ftol` at all: `FCOM` against `-1.0` sets C3 for
    ///   unordered as well as equal, and the `TEST AH,0x40` at `0x0047463E`
    ///   takes the sentinel exit, returning the caller's default. Rust returns
    ///   `0`. Inert in practice because `parse_read_double` scans a numeric
    ///   prefix and cannot produce NaN.
    ///
    /// The lepton scale is load-bearing and was missing here until it was
    /// re-derived from the disassembly on 2026-09-02 — the Ghidra decompiler
    /// elides the `FMUL` because it is folded into the x87 argument chain that
    /// `Math__ftol` consumes, so the pseudocode shows a bare `ftol`. Stock
    /// `[CrateRules] CrateRadius=3.0` is 768 leptons, consistent with the
    /// RulesClass constructor default of `0x280` (2.5 cells).
    pub fn read_range(&self, key: &str, default: i32) -> i32 {
        self.fold_rules_values(key, default, |current, raw| {
            let parsed = parse_read_double(raw);
            if parsed == -1.0 {
                current
            } else {
                (parsed * 256.0) as i32
            }
        })
    }
}

fn parse_read_int(default: i32, raw: &str) -> i32 {
    parse_read_int_value(raw).unwrap_or(default)
}

/// Source bytes `INIClass__ReadCommaHexUTF16` copies before trimming: the
/// `strncpy(0x5000)` at `0x00529097` is terminated at `0x4FFF`.
const COMMA_HEX_ENCODED_BYTES: usize = 0x4fff;

/// `INIClass__ReadCommaHexUTF16` @ `0x00528F00`: decode a comma-separated
/// list of hexadecimal UTF-16 code units.
///
/// The raw value is `strtrim`med (bytes <= 0x20) and comma-tokenized with
/// `strtok`, so empty tokens are skipped. Each token is read with
/// `sscanf("%x")` into one scratch dword whose result is ignored: a failed
/// conversion repeats the previous unit. On the fresh-file section-pointer
/// cache miss that scratch starts as the section-name CRC, so a failed first
/// conversion emits its low 16 bits. At most `max_units` units are written;
/// the visible result ends at the first zero unit. A missing or trimmed-empty
/// value yields `default` (itself capped at `max_units`).
pub(crate) fn read_comma_hex_utf16(
    raw: Option<&str>,
    default: &[u16],
    max_units: usize,
    section_crc: u32,
) -> Vec<u16> {
    let bytes = raw.unwrap_or_default().as_bytes();
    let bytes = &bytes[..bytes.len().min(COMMA_HEX_ENCODED_BYTES)];
    let bytes = &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())];
    // strtrim at 0x0052909F removes bytes <= 0x20, unlike sscanf whitespace.
    let start = bytes
        .iter()
        .position(|b| *b > STRTRIM_MAX)
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| *b > STRTRIM_MAX)
        .map_or(start, |i| i + 1);
    let bytes = &bytes[start..end];
    if bytes.is_empty() {
        return default
            .iter()
            .copied()
            .take(max_units)
            .take_while(|u| *u != 0)
            .collect();
    }
    let mut scratch = section_crc;
    let mut units = Vec::new();
    // strtok skips empty comma-delimited tokens. A whitespace-only token is
    // still a token and a failed conversion repeats the preceding value.
    for token in bytes
        .split(|b| *b == b',')
        .filter(|token| !token.is_empty())
        .take(max_units)
    {
        if let Some(value) = scan_hex_u32(token) {
            scratch = value;
        }
        units.push(scratch as u16);
    }
    // The original scans up to its token count, appends NUL, then returns
    // wcslen: keep its visible result.
    units.truncate(units.iter().position(|u| *u == 0).unwrap_or(units.len()));
    units
}

/// CRT `sscanf("%x")` for one comma token: optional whitespace and sign, an
/// optional `0x` prefix only when digits follow, then hex digits.
fn scan_hex_u32(token: &[u8]) -> Option<u32> {
    let start = token
        .iter()
        .position(|b| !matches!(*b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))?;
    let mut bytes = &token[start..];
    let negative = bytes.first() == Some(&b'-');
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        bytes = &bytes[1..];
    }
    // CRT sscanf rejects a bare/invalid 0x prefix; parsing just its leading
    // zero would incorrectly replace the prior conversion with zero.
    if bytes.starts_with(b"0x") || bytes.starts_with(b"0X") {
        bytes = &bytes[2..];
    }
    let mut any = false;
    let mut value = 0u32;
    for byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => break,
        };
        any = true;
        value = value.wrapping_mul(16).wrapping_add(u32::from(digit));
    }
    any.then_some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// `INIClass__PutUTF16AsHexCSV` @ `0x00528E00`: each code unit as unpadded
/// lowercase hex followed by a comma, including after the last unit.
pub(crate) fn encode_comma_hex_utf16(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len() * 5);
    for unit in units.iter().take_while(|unit| **unit != 0) {
        out.push_str(&format!("{unit:x}"));
        out.push(',');
    }
    out
}

pub(crate) fn parse_read_int_value(raw: &str) -> Option<i32> {
    let value = strtrim_ascii(raw);
    if let Some(rest) = value.strip_prefix('$') {
        parse_leading_hex(rest)
    } else if ends_with_h(value) {
        parse_leading_hex(&value[..value.len() - 1])
    } else {
        Some(atoi_lenient(value))
    }
}

fn parse_read_bool(default: bool, raw: &str) -> bool {
    match strtrim_ascii(raw)
        .bytes()
        .next()
        .map(|byte| byte.to_ascii_uppercase())
    {
        Some(b'1') | Some(b'T') | Some(b'Y') => true,
        Some(b'0') | Some(b'F') | Some(b'N') => false,
        _ => default,
    }
}

pub(crate) fn parse_read_double(raw: &str) -> f64 {
    let value = strtrim_ascii(raw);
    let widened = f64::from(parse_leading_f32(value));
    if value.as_bytes().contains(&b'%') {
        widened * 0.01_f64
    } else {
        widened
    }
}

/// Byte-wise `strncpy` truncation. A cut that would land inside a multi-byte
/// sequence backs up to the preceding boundary: native writes the partial bytes
/// and the forced NUL, which is not representable as a Rust `str`, and no INI
/// value in retail data is non-ASCII.
pub(crate) fn truncate_bytes(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// strtrim equivalent (P5): strip bytes <= 0x20 from BOTH ends. ASCII-only by
/// design (RA2 INI is ASCII); does NOT use `str::trim` (Unicode whitespace).
pub(crate) fn strtrim_ascii(s: &str) -> &str {
    let b = s.as_bytes();
    let mut start = 0usize;
    while start < b.len() && b[start] <= STRTRIM_MAX {
        start += 1;
    }
    let mut end = b.len();
    while end > start && b[end - 1] <= STRTRIM_MAX {
        end -= 1;
    }
    &s[start..end]
}

/// tolower(last char) == 'h' (P2). Case-insensitive via ASCII.
fn ends_with_h(s: &str) -> bool {
    s.as_bytes().last().map(|b| b.to_ascii_lowercase()) == Some(b'h')
}

/// Parse a leading run of hex digits (after `$` strip or before `h` strip).
/// sscanf "$%x"/"%xh" accepts an optional sign and `0x` prefix, then stops at
/// the first non-hex char. No conversion returns `None`, leaving the caller's
/// initialized default untouched.
pub(crate) fn parse_leading_hex(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    let mut index = 0;
    let negative = if bytes.first() == Some(&b'-') {
        index += 1;
        true
    } else {
        index += usize::from(bytes.first() == Some(&b'+'));
        false
    };
    if bytes.get(index) == Some(&b'0')
        && bytes
            .get(index + 1)
            .is_some_and(|byte| matches!(byte, b'x' | b'X'))
        && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
    {
        index += 2;
    }

    let mut acc: u32 = 0;
    let mut any = false;
    for &c in &bytes[index..] {
        let d = match c {
            b'0'..=b'9' => u32::from(c - b'0'),
            b'a'..=b'f' => u32::from(c - b'a' + 10),
            b'A'..=b'F' => u32::from(c - b'A' + 10),
            _ => break,
        };
        any = true;
        acc = acc.wrapping_mul(16).wrapping_add(d);
    }
    any.then_some(if negative {
        0u32.wrapping_sub(acc) as i32
    } else {
        acc as i32
    })
}

/// C-atoi-equivalent leading-numeric parse (P3): optional leading sign, then
/// leading decimal digits, stop at first non-digit. `5cells`->5, `abc`->0,
/// ``->0, `  7 `->7 (already strtrimmed), `-50`->-50, `+9`->9. NB `0x1A`->0
/// (atoi does NOT treat `0x` as hex; the `$`/`h` branches are separate).
pub(crate) fn atoi_lenient(s: &str) -> i32 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut acc: i64 = 0;
    let mut any = false;
    while i < b.len() && b[i].is_ascii_digit() {
        any = true;
        acc = acc.wrapping_mul(10).wrapping_add((b[i] - b'0') as i64);
        i += 1;
    }
    if !any {
        return 0;
    }
    let v = if neg { -acc } else { acc };
    v as i32
}

/// One native CRT sscanf `%d` conversion, retaining the unconsumed input for
/// the caller's literal separators. Decimal arithmetic retains the low32 bits.
/// Original7CA530 consumers: Building4615CA and Infantry523DB0; executable
/// fixtures building_body_rules and infantry_sequence_rules preserve both.
pub(crate) fn scan_decimal_i32(bytes: &mut &[u8]) -> Option<i32> {
    while bytes
        .first()
        .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 11 | 12))
    {
        *bytes = &bytes[1..];
    }
    let negative = bytes.first() == Some(&b'-');
    if negative || bytes.first() == Some(&b'+') {
        *bytes = &bytes[1..];
    }
    if !bytes.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut value = 0_u32;
    while let Some(&digit) = bytes.first().filter(|digit| digit.is_ascii_digit()) {
        value = value.wrapping_mul(10).wrapping_add(u32::from(digit - b'0'));
        *bytes = &bytes[1..];
    }
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    } as i32)
}

/// sscanf "%f"-equivalent leading float (P7): optional sign, decimal mantissa,
/// and optional exponent. Parsing stops at the first byte outside that token
/// (`12.5%` -> 12.5, `1.25e2junk` -> 125). Empty/junk -> 0.0.
pub(crate) fn parse_leading_f32(s: &str) -> f32 {
    let b = s.as_bytes();
    let mut end = usize::from(b.first().is_some_and(|c| matches!(c, b'-' | b'+')));
    let mut mantissa_digits = 0usize;

    while end < b.len() && b[end].is_ascii_digit() {
        mantissa_digits += 1;
        end += 1;
    }
    if b.get(end) == Some(&b'.') {
        end += 1;
        while end < b.len() && b[end].is_ascii_digit() {
            mantissa_digits += 1;
            end += 1;
        }
    }
    if mantissa_digits == 0 {
        return 0.0;
    }

    if b.get(end).is_some_and(|c| matches!(c, b'e' | b'E')) {
        let exponent_mark = end;
        end += 1;
        if b.get(end).is_some_and(|c| matches!(c, b'-' | b'+')) {
            end += 1;
        }
        let exponent_start = end;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
        if end == exponent_start {
            end = exponent_mark;
        }
    }

    s[..end].parse::<f32>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{atoi_lenient, parse_leading_f32};
    use crate::rules::ini_parser::IniFile;

    fn sec(body: &str) -> IniFile {
        IniFile::from_str(body)
    }

    #[test] // P1/P2
    fn test_read_int_hex() {
        let ini = sec("[S]\nA=$1A\nB=1Ah\nC=0FFH\nD=$0\nE=$FF\nF=$0xFF\nG=0xFFh\nH=$-1\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_int("A", -9), 26);
        assert_eq!(s.read_int("B", -9), 26);
        assert_eq!(s.read_int("C", -9), 255);
        assert_eq!(s.read_int("D", -9), 0);
        assert_eq!(s.read_int("E", -9), 255);
        assert_eq!(s.read_int("F", -9), 255);
        assert_eq!(s.read_int("G", -9), 255);
        assert_eq!(s.read_int("H", -9), -1);
    }

    #[test] // P3/P4/P18
    fn test_read_int_atoi_leniency() {
        let ini = sec("[S]\nA=5cells\nB=abc\nC=-50\nD=\nE=  7 \n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_int("A", -9), 5);
        assert_eq!(s.read_int("B", -9), 0); // present-nonnumeric -> 0, NOT default
        assert_eq!(s.read_int("C", -9), -50);
        assert_eq!(s.read_int("D", -9), -9); // empty values are omitted
        assert_eq!(s.read_int("E", -9), 7);
        assert_eq!(s.read_int("MISSING", -9), -9); // absent -> default
    }

    #[test] // OQ3: 0x is NOT hex via atoi fallback
    fn test_read_int_0x_prefix_is_zero() {
        let ini = sec("[S]\nA=0x1A\n");
        // atoi("0x1A") = 0 (stops at 'x'); $/h branches don't fire.
        assert_eq!(ini.section("S").unwrap().read_int("A", -9), 0);
    }

    #[test]
    fn test_read_int_signs_and_edges() {
        let ini = sec("[S]\nA=+9\nB=-0\nC=$\nD=h\nE=$junk\nF=junkh\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_int("A", -1), 9);
        assert_eq!(s.read_int("B", -1), 0);
        assert_eq!(s.read_int("C", -1), -1);
        assert_eq!(s.read_int("D", -1), -1);
        assert_eq!(s.read_int("E", -1), -1);
        assert_eq!(s.read_int("F", -1), -1);
    }

    #[test] // P6/P18
    fn test_read_bool_first_char() {
        let ini = sec(
            "[S]\nA=yes\nB=Y\nC=T\nD=true\nE=1\nF=no\nG=N\nH=F\nI=false\nJ=0\nK=off\nL=xyz\nM=\n",
        );
        let s = ini.section("S").unwrap();
        for k in ["A", "B", "C", "D", "E"] {
            assert!(s.read_bool(k, false), "{k}");
        }
        for k in ["F", "G", "H", "I", "J"] {
            assert!(!s.read_bool(k, true), "{k}");
        }
        assert!(s.read_bool("K", true)); // 'off' first char 'o' -> default
        assert!(s.read_bool("L", true)); // xyz -> default
        assert!(s.read_bool("M", true)); // empty values are omitted -> default
        assert!(s.read_bool("MISSING", true)); // absent -> default
    }

    #[test] // P7 (after the S0 gate pins precision)
    fn test_read_double_percent() {
        let ini = sec("[S]\nA=50%\nB=100%\nC=7\nD=0.5\nE=12.5%\n");
        let s = ini.section("S").unwrap();
        assert!((s.read_double("A", -1.0) - 0.5).abs() < 1e-6);
        assert!((s.read_double("B", -1.0) - 1.0).abs() < 1e-6);
        assert!((s.read_double("C", -1.0) - 7.0).abs() < 1e-6);
        assert!((s.read_double("D", -1.0) - 0.5).abs() < 1e-6);
        assert!((s.read_double("E", -1.0) - 0.125).abs() < 1e-6);
        assert!((s.read_double("MISSING", -42.0) + 42.0).abs() < 1e-9); // absent -> default
    }

    #[test] // P5/P18
    fn test_read_string_trim_default() {
        let ini = sec("[S]\nA=  hello  \nB=\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_string("A", "D", 32), "hello");
        assert_eq!(s.read_string("A", "D", 4), "hel");
        assert_eq!(s.read_string("B", "D", 32), "D"); // empty values are omitted
        assert_eq!(s.read_string("MISSING", "D", 32), "D");
        assert_eq!(s.read_string("MISSING", " default ", 6), "defa");
        assert_eq!(s.read_string("A", "D", 0), "");
    }

    #[test]
    fn test_atoi_and_leading_f32_helpers() {
        assert_eq!(atoi_lenient("5cells"), 5);
        assert_eq!(atoi_lenient("-50"), -50);
        assert_eq!(atoi_lenient("+9"), 9);
        assert_eq!(atoi_lenient(""), 0);
        assert!((parse_leading_f32("12.5%") - 12.5).abs() < 1e-6);
        assert!((parse_leading_f32(".9") - 0.9).abs() < 1e-6);
        assert!((parse_leading_f32("1.25e2junk") - 125.0).abs() < 1e-6);
        assert!((parse_leading_f32("-2.5E-1%") + 0.25).abs() < 1e-6);
        assert!((parse_leading_f32("1e") - 1.0).abs() < 1e-6);
    }

    #[test] // P9 COMMA
    fn test_read_point_comma() {
        let ini = sec("[S]\nP=3,5\nR=1,2,3,4\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_point("P", (0, 0)), (3, 5));
        assert_eq!(s.read_rect("R", (0, 0, 0, 0)), (1, 2, 3, 4));
        assert_eq!(s.read_point("MISSING", (9, 9)), (9, 9)); // absent -> default
    }

    #[test] // P8 partial keeps default component
    fn test_read_3int_partial_keeps_default() {
        let ini = sec("[S]\nA=10,20\n"); // only 2 of 3 fields
        assert_eq!(
            ini.section("S").unwrap().read_3int("A", [1, 2, 3]),
            [10, 20, 3]
        );
    }

    #[test] // P21
    fn test_read_color_rgb() {
        let ini = sec("[S]\nC=12,34,56\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_color_rgb("C", [0, 0, 0]), [12, 34, 56]);
        assert_eq!(s.read_color_rgb("MISSING", [1, 2, 3]), [1, 2, 3]);
    }

    #[test] // P19
    fn test_read_speed_clamp() {
        let ini = sec("[S]\nA=100\nB=50\nC=7\nD=0\nE=-2\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_speed("A", -1), 255); // (100<<8)/100=256 -> clamp 255
        assert_eq!(s.read_speed("B", -1), 128); // (50<<8)/100=128
        assert_eq!(s.read_speed("C", -1), 17); // (7<<8)/100=17 (trunc)
        assert_eq!(s.read_speed("D", -1), 0);
        assert_eq!(s.read_speed("E", 42), 0); // negative non-sentinel clamps to zero
        assert_eq!(s.read_speed("MISSING", 42), 42); // absent -> default (sentinel -1)
    }

    #[test] // P20 truncate toward zero (ledger #18)
    fn test_read_range_truncates() {
        let ini = sec("[S]\nA=5.9\nB=5\nC=0.4\n");
        let s = ini.section("S").unwrap();
        // Values are cells; the reader scales to leptons and truncates toward
        // zero. 5.9 cells is 1510.4 leptons -> 1510, never 1511.
        assert_eq!(s.read_range("A", -1), 1510);
        assert_eq!(s.read_range("B", -1), 1280, "5 cells");
        assert_eq!(s.read_range("C", -1), 102, "0.4 cells is 102.4 leptons");
        assert_eq!(s.read_range("MISSING", 7), 7); // absent -> default (sentinel -1.0)
    }
}

/// S2 corpus equivalence harness — "the shadow assert". Read-only, NOT
/// hash-relevant: builds an `IniFile` and compares accessor outputs; never
/// touches `World`, `state_hash`, or `SNAPSHOT_VERSION`. It is a test, not a
/// consumer flip.
#[cfg(test)]
mod corpus_tests {
    use std::path::Path;

    use crate::assets::asset_manager::AssetManager;
    use crate::rules::ini_parser::IniFile;

    const CONTRACT_RULES: &str =
        include_str!("../../tests/fixtures/ini/accessor_rules_contract.ini");
    const CONTRACT_ART: &str = include_str!("../../tests/fixtures/ini/accessor_art_contract.ini");

    /// Smallest gamemd per-accessor ReadString cap; values over (cap-1) chars
    /// would truncate in an enum/zone/action read.
    const READSTRING_CAP: usize = 32;

    /// Keys where the NEW accessor INTENTIONALLY diverges from the OLD one,
    /// each with the gamemd-correct reason. Empty: corpus-confirmed (plan-review
    /// C-R3) that stock has ZERO `$`/`h`/`0x`/exponent values, so over the stock
    /// numeric domain the new `read_int`/`read_double` agree with the old
    /// `get_i32`/`get_f32` everywhere they both return a value. Any divergence
    /// surfaced by the test below must be classified against P1–P21 and added
    /// here with a cited reason, OR proven a new accessor bug and fixed.
    /// Format: (section, key, reason).
    const DIVERGENCES: &[(&str, &str, &str)] = &[];

    fn is_documented(section: &str, key: &str) -> bool {
        DIVERGENCES
            .iter()
            .any(|(s, k, _)| s.eq_ignore_ascii_case(section) && k.eq_ignore_ascii_case(key))
    }

    #[test]
    fn test_ini_accessor_contract_corpus_parity() {
        let mut ini = IniFile::from_str(CONTRACT_RULES);
        ini.merge(&IniFile::from_str(CONTRACT_ART));
        assert_ini_accessor_corpus_parity(ini);
    }

    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn test_retail_ini_accessor_corpus_parity() {
        let root =
            std::env::var("RA2_DIR").expect("set RA2_DIR to the installed retail RA2/YR directory");
        let assets = AssetManager::new(Path::new(&root)).expect("load retail archive stack");
        let rulesmd = assets
            .get("rulesmd.ini")
            .expect("rulesmd.ini in retail archive stack");
        let artmd = assets
            .get("artmd.ini")
            .expect("artmd.ini in retail archive stack");
        let mut ini = IniFile::from_bytes(&rulesmd).expect("parse retail rulesmd.ini");
        ini.merge(&IniFile::from_bytes(&artmd).expect("parse retail artmd.ini"));
        assert_ini_accessor_corpus_parity(ini);
    }

    fn assert_ini_accessor_corpus_parity(ini: IniFile) {
        // Scans the MERGED view (rules then art on top). Production keeps them
        // separate, but a per-key parse-EQUIVALENCE scan is unaffected: the
        // old and new accessor see the same stored string for each key.
        let mut undocumented: Vec<String> = Vec::new();
        let mut zero_x: Vec<String> = Vec::new();
        let mut present_empty_transform: Vec<String> = Vec::new();
        let mut over_cap: Vec<String> = Vec::new();
        let mut exponent: Vec<String> = Vec::new();

        // Collect section names first to avoid borrowing `ini` while iterating.
        let names: Vec<String> = ini.section_names().iter().map(|s| s.to_string()).collect();
        for name in &names {
            let section = match ini.section(name) {
                Some(s) => s,
                None => continue,
            };
            let keys: Vec<String> = section.keys().map(|k| k.to_string()).collect();
            for key in &keys {
                let raw = section.get(key).unwrap_or("");
                let trimmed = raw.trim();

                // 0x-prefix scan (OQ3): would atoi to 0 if read as an int.
                if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                    zero_x.push(format!("[{name}] {key}={raw}"));
                }
                // Buffer-cap scan (smallest gamemd cap 32).
                if trimmed.len() > READSTRING_CAP - 1 {
                    over_cap.push(format!("[{name}] {key} len={}", trimmed.len()));
                }
                // Exponent-notation scan: confirm parse_leading_f32 needs no
                // exponent branch over the stock domain.
                if (raw.contains('e') || raw.contains('E')) && looks_numeric(trimmed) {
                    exponent.push(format!("[{name}] {key}={raw}"));
                }
                // Int equivalence: old get_i32 (None on parse-fail / absent) vs
                // new read_int. Only compare where the OLD returns Some — that is
                // the set the consumers actually use today.
                if let Some(old_i) = section.get_i32(key) {
                    let new_i = section.read_int(key, i32::MIN);
                    if old_i != new_i && !is_documented(name, key) {
                        undocumented.push(format!(
                            "[{name}] {key}: old_i32={old_i} new_int={new_i} raw={raw}"
                        ));
                    }
                }
                // Bool equivalence: old get_bool (whole-word, None otherwise) vs
                // new read_bool (first-char). Compare only where old returns Some.
                if let Some(old_b) = section.get_bool(key) {
                    let new_b = section.read_bool(key, !old_b); // sentinel != old_b
                    if old_b != new_b && !is_documented(name, key) {
                        undocumented.push(format!(
                            "[{name}] {key}: old_bool={old_b} new_bool={new_b} raw={raw}"
                        ));
                    }
                }
                // Present-empty transform scan (OQ4/C4): would resolve to 0 vs
                // the call-site default in read_speed/read_range.
                if trimmed.is_empty()
                    && matches!(
                        key.to_ascii_lowercase().as_str(),
                        "speed" | "range" | "minimumrange"
                    )
                {
                    present_empty_transform.push(format!("[{name}] {key}="));
                }
            }
        }

        // Fail ONLY on undocumented parse divergences. Each must be classified
        // (gamemd-correct fix -> DIVERGENCES, or accessor bug -> fix the code).
        assert!(
            undocumented.is_empty(),
            "UNDOCUMENTED parse divergences (each must be added to DIVERGENCES \
             with a gamemd-correct reason or proven a real fix):\n{}",
            undocumented.join("\n")
        );

        // Surface the remaining scans for the later flip slices. These are the
        // precise input lists for the consumer-flip parity re-baseline.
        if !zero_x.is_empty() {
            eprintln!("0x-prefixed values:\n{}", zero_x.join("\n"));
        }
        if !present_empty_transform.is_empty() {
            eprintln!(
                "present-empty Speed/Range/MinimumRange:\n{}",
                present_empty_transform.join("\n")
            );
        }
        if !over_cap.is_empty() {
            eprintln!(">31-char values (cap-32 scan):\n{}", over_cap.join("\n"));
        }
        if !exponent.is_empty() {
            eprintln!("exponent-notation values:\n{}", exponent.join("\n"));
        }
    }

    /// Cheap "is this a numeric value" test so the exponent scan does not flag
    /// every string key that happens to contain an 'e'/'E' (image names etc.).
    fn looks_numeric(s: &str) -> bool {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E' | b'%'))
    }
}
