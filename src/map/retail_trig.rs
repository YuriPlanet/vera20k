//! Shared retail sine/cosine table, read from the retail executable.
//!
//! The river carver steers by a floating-point heading and resolves it through
//! two table lookups every step. The table is **not reproducible from a
//! formula**: computing `sin` in double precision and rounding disagrees with it
//! on 4997 of its first 10240 indexed entries, and so does the correctly-rounded
//! true sine, by up to one unit in the last place. It is an artifact of whatever
//! generated it
//! in the original build. It is not even exactly periodic — one of the 2048
//! wrap-around pairs disagrees, which is what an accumulating recurrence looks
//! like and what a per-index evaluation does not.
//!
//! So the bytes have to come from the retail install. They are read out of
//! `gamemd.exe` the same way the rest of the engine reads retail `.mix` assets:
//! from the player's own copy, at load time. Nothing retail-derived lives in
//! this repository.
//!
//! Both trig functions share one table. Cosine is the same data read a quarter
//! period further along, so there is a single array and two index derivations.

use std::fmt;

use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value};
use std::path::Path;
use std::sync::OnceLock;

/// Virtual address of the table in the retail image.
const TABLE_VA: u32 = 0x0084_F084;
/// Entries. The extra quarter period past a full turn is what lets the cosine
/// offset run past the end of the sine range without wrapping; the final exact
/// `1.0` is part of the retail data extent even though the lookup ceilings stop
/// one entry before it.
pub const TABLE_LEN: usize = 0x2801;
/// Entries in one full turn.
const PERIOD: i32 = 0x2000;
/// Quarter period: the offset that turns the sine table into a cosine table.
const QUARTER: i32 = 0x800;
/// Highest index each lookup will round *up* to.
const SIN_CEILING: i32 = PERIOD - 1;
const COS_CEILING: i32 = PERIOD + QUARTER - 1;

/// Caller angle units per full turn.
///
/// The index derivation halves the caller's value before using it as a table
/// index, so one caller unit is half a table step and a full turn is twice the
/// table period.
pub const UNITS_PER_TURN: f64 = (2 * PERIOD) as f64;

/// FNV-1a (64-bit) over the table's raw little-endian bytes in the retail
/// image, machine-derived by reading all 40964 bytes out of the binary. This is
/// the whole-table check: it is a genuine exhaustive comparison, not a sample.
pub const RETAIL_FNV1A64: u64 = 0x74ac_b749_b33d_5aa7;

/// Virtual address and exact size of `Acos_lookup @ 0x004CADB0`'s signed
/// arcsine table. `WaveClass` indexes the inclusive endpoints, hence 4097
/// entries rather than 4096.
const ACOS_TABLE_VA: u32 = 0x0085_9094;
pub const ACOS_TABLE_LEN: usize = 0x1001;
pub const ACOS_RETAIL_FNV1A64: u64 = 0x9251_751b_f328_3bc1;

/// Virtual address and size of `Math::atan2 @ 0x004CAE30`'s arctangent table:
/// 4097 binary32 entries indexed by `|ftol(y / x / step)|`.
///
/// Like the sine table it is not a formula: 4094 of the 4097 entries differ
/// from `f32(atan(i * step))`, so the bytes come from the player's executable.
const ATAN_TABLE_VA: u32 = 0x0086_10B4;
pub const ATAN_TABLE_LEN: usize = 0x1001;
/// FNV-1a (64-bit) over the table's 16388 raw bytes, read out of the retail
/// image.
pub const ATAN_RETAIL_FNV1A64: u64 = 0x4056_c36f_7f1e_ab9c;
/// Index step: the binary32 at `0x008650B8` (`0x3CC7FE84`, just under
/// 100/4096), so the table spans ratios up to about 100.
const ATAN_STEP_BITS: u32 = 0x3CC7_FE84;
/// `0x007E897C`: binary32 pi/2 (`0x007E8980` is its negation).
const ATAN_HALF_PI_F32_BITS: u32 = 0x3FC9_0FDB;
/// `0x007E44D0`: binary64 pi.
const ATAN_PI_F64_BITS: u64 = 0x4009_21FB_5444_2D18;

/// Something went wrong reading the table out of the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrigTableError {
    NotPeFile,
    UnmappedAddress(u32),
    Truncated { need: usize, have: usize },
}

impl fmt::Display for TrigTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPeFile => write!(f, "not a PE executable"),
            Self::UnmappedAddress(va) => {
                write!(f, "address {va:#010x} is not inside any section")
            }
            Self::Truncated { need, have } => {
                write!(f, "table needs {need} bytes, only {have} available")
            }
        }
    }
}

impl std::error::Error for TrigTableError {}

/// The retail sine table.
#[derive(Debug, Clone)]
pub struct TrigTable {
    entries: Vec<f32>,
}

/// Retail binary32 table consumed by `Acos_lookup @ 0x004CADB0`.
#[derive(Debug, Clone)]
pub struct AcosTable {
    entries: Vec<f32>,
}

/// Retail binary32 table consumed by `Math::atan2 @ 0x004CAE30`.
#[derive(Debug, Clone)]
pub struct AtanTable {
    entries: Vec<f32>,
}

/// Translate a virtual address to a file offset using the PE section table.
///
/// Parsed rather than hardcoded: a hardcoded offset silently reads the wrong
/// bytes if the executable is ever a different build, whereas a section walk
/// either finds the address or says it could not.
pub(crate) fn file_offset_of(image: &[u8], va: u32) -> Result<usize, TrigTableError> {
    let u16_at = |off: usize| -> Option<u16> {
        Some(u16::from_le_bytes(
            image.get(off..off + 2)?.try_into().ok()?,
        ))
    };
    let u32_at = |off: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            image.get(off..off + 4)?.try_into().ok()?,
        ))
    };

    if image.get(..2) != Some(b"MZ") {
        return Err(TrigTableError::NotPeFile);
    }
    let pe = u32_at(0x3C).ok_or(TrigTableError::NotPeFile)? as usize;
    if image.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err(TrigTableError::NotPeFile);
    }

    let sections = u16_at(pe + 6).ok_or(TrigTableError::NotPeFile)? as usize;
    let optional_size = u16_at(pe + 20).ok_or(TrigTableError::NotPeFile)? as usize;
    let optional = pe + 24;
    // PE32 optional header: ImageBase at +28.
    let image_base = u32_at(optional + 28).ok_or(TrigTableError::NotPeFile)?;
    let rva = va
        .checked_sub(image_base)
        .ok_or(TrigTableError::UnmappedAddress(va))?;

    let table = optional + optional_size;
    for i in 0..sections {
        let hdr = table + i * 40;
        let virtual_size = u32_at(hdr + 8).ok_or(TrigTableError::NotPeFile)?;
        let virtual_addr = u32_at(hdr + 12).ok_or(TrigTableError::NotPeFile)?;
        let raw_size = u32_at(hdr + 16).ok_or(TrigTableError::NotPeFile)?;
        let raw_ptr = u32_at(hdr + 20).ok_or(TrigTableError::NotPeFile)?;
        // Sections are often larger in memory than on disk (bss-style tail);
        // the span that actually exists in the file is the raw size.
        let span = virtual_size.max(raw_size);
        if rva >= virtual_addr && rva < virtual_addr.saturating_add(span) {
            return Ok((raw_ptr + (rva - virtual_addr)) as usize);
        }
    }
    Err(TrigTableError::UnmappedAddress(va))
}

impl TrigTable {
    /// Read the table out of a retail `gamemd.exe` image.
    pub fn from_executable(image: &[u8]) -> Result<Self, TrigTableError> {
        let start = file_offset_of(image, TABLE_VA)?;
        let need = TABLE_LEN * 4;
        let bytes = image
            .get(start..start + need)
            .ok_or(TrigTableError::Truncated {
                need,
                have: image.len().saturating_sub(start),
            })?;
        let entries = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(Self { entries })
    }

    /// FNV-1a over the raw little-endian bytes, for comparison against
    /// [`RETAIL_FNV1A64`].
    pub fn fnv1a64(&self) -> u64 {
        let mut hash = crate::util::fnv::FNV1A64_OFFSET_BASIS;
        for entry in &self.entries {
            hash = crate::util::fnv::fnv1a64_fold_bytes(hash, &entry.to_le_bytes());
        }
        hash
    }

    /// Is this the table the retail build shipped?
    pub fn matches_retail(&self) -> bool {
        self.entries.len() == TABLE_LEN && self.fnv1a64() == RETAIL_FNV1A64
    }

    pub fn entry(&self, index: usize) -> f32 {
        self.entries[index]
    }

    /// Sine of an angle given in caller units.
    pub fn sin(&self, angle_units: i32) -> f32 {
        self.entries[self.sin_index(angle_units)]
    }

    /// Cosine of an angle given in caller units.
    pub fn cos(&self, angle_units: i32) -> f32 {
        self.entries[self.cos_index(angle_units)]
    }

    /// Exact raw table index used by `Math::SinFromTable`.
    pub fn sin_index(&self, angle_units: i32) -> usize {
        sin_index(angle_units) as usize
    }

    /// Exact raw table index used by `Math::CosFromTable`.
    pub fn cos_index(&self, angle_units: i32) -> usize {
        cos_index(angle_units) as usize
    }

    /// Sine of an angle in radians. The scaling and the truncation to an integer
    /// index are the caller's job in the original, so they happen here.
    pub fn sin_radians(&self, radians: f64) -> f32 {
        self.sin(radians_to_units(radians) as i32)
    }

    /// Cosine of an angle in radians.
    pub fn cos_radians(&self, radians: f64) -> f32 {
        self.cos(radians_to_units(radians) as i32)
    }

    /// `Math::SinFromTable @ 0x004CACB0` for its double argument under the
    /// process's x87 mode: the table unit is `Math::ftol` of the radians times
    /// the binary32 scale at `0x008223B0` (16384/2pi), and the entry comes back
    /// as the binary32 it is.
    pub fn sin_from_table(&self, radians: X87Value) -> X87Value {
        table_value(self.sin(from_table_units(radians)))
    }

    /// `Math::CosFromTable @ 0x004CAD00`, as [`Self::sin_from_table`].
    pub fn cos_from_table(&self, radians: X87Value) -> X87Value {
        table_value(self.cos(from_table_units(radians)))
    }

    /// A table of the right shape but not the retail contents, for tests that
    /// need geometry rather than exactness.
    #[cfg(test)]
    pub fn synthetic() -> Self {
        Self {
            entries: (0..TABLE_LEN)
                .map(|i| (i as f64 * std::f64::consts::TAU / PERIOD as f64).sin() as f32)
                .collect(),
        }
    }
}

impl AcosTable {
    /// Read the signed arcsine table out of a retail `gamemd.exe` image.
    pub fn from_executable(image: &[u8]) -> Result<Self, TrigTableError> {
        let start = file_offset_of(image, ACOS_TABLE_VA)?;
        let need = ACOS_TABLE_LEN * 4;
        let bytes = image
            .get(start..start + need)
            .ok_or(TrigTableError::Truncated {
                need,
                have: image.len().saturating_sub(start),
            })?;
        let entries = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(Self { entries })
    }

    pub fn fnv1a64(&self) -> u64 {
        let mut hash = crate::util::fnv::FNV1A64_OFFSET_BASIS;
        for entry in &self.entries {
            hash = crate::util::fnv::fnv1a64_fold_bytes(hash, &entry.to_le_bytes());
        }
        hash
    }

    pub fn matches_retail(&self) -> bool {
        self.entries.len() == ACOS_TABLE_LEN && self.fnv1a64() == ACOS_RETAIL_FNV1A64
    }

    pub fn entry(&self, index: usize) -> f32 {
        self.entries[index]
    }

    /// A shape-compatible table used only by unit tests that exercise Wave
    /// lifetime/cell mechanics without a retail installation. Exact geometry
    /// fixtures always load the executable-backed table.
    #[cfg(test)]
    pub fn synthetic() -> Self {
        Self {
            entries: (0..ACOS_TABLE_LEN)
                .map(|i| ((i as f64 - 2048.0) / 2048.0).asin() as f32)
                .collect(),
        }
    }
}

impl AtanTable {
    /// Read the arctangent table out of a retail `gamemd.exe` image.
    pub fn from_executable(image: &[u8]) -> Result<Self, TrigTableError> {
        let start = file_offset_of(image, ATAN_TABLE_VA)?;
        let need = ATAN_TABLE_LEN * 4;
        let bytes = image
            .get(start..start + need)
            .ok_or(TrigTableError::Truncated {
                need,
                have: image.len().saturating_sub(start),
            })?;
        let entries = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok(Self { entries })
    }

    pub fn fnv1a64(&self) -> u64 {
        let mut hash = crate::util::fnv::FNV1A64_OFFSET_BASIS;
        for entry in &self.entries {
            hash = crate::util::fnv::fnv1a64_fold_bytes(hash, &entry.to_le_bytes());
        }
        hash
    }

    pub fn matches_retail(&self) -> bool {
        self.entries.len() == ATAN_TABLE_LEN && self.fnv1a64() == ATAN_RETAIL_FNV1A64
    }

    pub fn entry(&self, index: usize) -> f32 {
        self.entries[index]
    }

    /// `Math::atan2 @ 0x004CAE30`, evaluated in the process's x87 mode.
    ///
    /// gamemd-derived (disassembly read 2026-09-15): both double arguments are
    /// stored as binary32 first (`FSTP float`). A zero `x` answers 0 or the
    /// binary32 +/-pi/2 by the sign of `y`. Otherwise the index is
    /// `|ftol(y32 / x32 / step)|` through `Math::ftol @ 0x007C5F00`; an index of
    /// 0x1001 or more reads pi/2. A negative `x` reflects through binary64 pi,
    /// then a negative `y` negates. Every operation and store runs under the
    /// process control word `0x0E7F` (53-bit precision, round toward zero), which
    /// `WinMain` installs with `_controlfp(0x300, 0x300)` at `0x006BBFC1`, so the
    /// arithmetic is [`X87Chop53`], not `f64`.
    pub fn atan2(&self, y: X87Value, x: X87Value) -> X87Value {
        let zero = X87Chop53::load_i32(0);
        let binary32 = |value: X87Value| {
            X87Chop53::store_f32(value)
                .and_then(X87Chop53::load_f32)
                .unwrap_or(zero)
        };
        let constant32 =
            |bits: u32| X87Chop53::load_f32(NativeF32Bits::from_bits(bits)).unwrap_or(zero);
        let y32 = binary32(y);
        let x32 = binary32(x);
        let half_pi = constant32(ATAN_HALF_PI_F32_BITS);
        if X87Chop53::compare(x32, zero) == X87Ordering::Equal {
            return match X87Chop53::compare(y32, zero) {
                X87Ordering::Equal => zero,
                X87Ordering::Greater => half_pi,
                X87Ordering::Less => X87Chop53::neg(half_pi),
            };
        }
        // `ftol` returns the low dword of `FISTP qword`; `CDQ/XOR/SUB` takes the
        // absolute value of that dword.
        let index = X87Chop53::div(y32, x32)
            .and_then(|ratio| X87Chop53::div(ratio, constant32(ATAN_STEP_BITS)))
            .and_then(X87Chop53::ftol_i64)
            .map_or(0, |value| (value as i32).unsigned_abs() as usize);
        let mut value = if index < ATAN_TABLE_LEN {
            constant32(self.entries[index].to_bits())
        } else {
            half_pi
        };
        if X87Chop53::compare(x32, zero) == X87Ordering::Less {
            let pi =
                X87Chop53::load_f64(NativeF64Bits::from_bits(ATAN_PI_F64_BITS)).unwrap_or(zero);
            value = X87Chop53::sub(pi, value);
        }
        if X87Chop53::compare(y32, zero) == X87Ordering::Less {
            value = X87Chop53::neg(value);
        }
        value
    }

    /// A shape-compatible table for unit tests without a retail install.
    #[cfg(test)]
    pub fn synthetic() -> Self {
        let step = f64::from(f32::from_bits(ATAN_STEP_BITS));
        Self {
            entries: (0..ATAN_TABLE_LEN)
                .map(|i| (i as f64 * step).atan() as f32)
                .collect(),
        }
    }
}

/// Fold a caller's angle into the table's index range.
///
/// The mask keeps the sign bit and the low 13 bits, then the odd-looking
/// `(x - 1) | 0xFFFF_E000` then `+ 1` sign-extends that 13-bit value back to a
/// negative number. Written out rather than tidied because the two callers
/// diverge on what they do with a still-negative result.
/// All arithmetic wraps. The masked value can be `i32::MIN` (sign bit set, low
/// bits clear), and subtracting one from it is exactly the case the original
/// wraps through without noticing — a checked subtraction would panic on an
/// angle the original handles fine.
fn fold(angle_units: i32) -> (i32, bool) {
    let raw = angle_units;
    let mut index = (raw / 2) & 0x8000_1FFFu32 as i32;
    let mut wrapped_negative = false;
    if index < 0 {
        let extended = index.wrapping_sub(1) | 0xFFFF_E000u32 as i32;
        index = extended.wrapping_add(1);
        if index < 0 {
            index = extended;
            wrapped_negative = true;
        }
    }
    (index, wrapped_negative)
}

fn sin_index(angle_units: i32) -> i32 {
    let (folded, wrapped) = fold(angle_units);
    let mut index = if wrapped {
        folded.wrapping_add(PERIOD + 1)
    } else {
        folded
    };
    if angle_units & 1 != 0 && index < SIN_CEILING {
        index += 1;
    }
    index
}

fn cos_index(angle_units: i32) -> i32 {
    let (folded, wrapped) = fold(angle_units);
    let mut index = if wrapped {
        folded.wrapping_add(PERIOD + QUARTER + 1)
    } else {
        folded + QUARTER
    };
    if angle_units & 1 != 0 && index < COS_CEILING {
        index += 1;
    }
    index
}

/// `0x008223B0`: binary32 16384/2pi, the radians-to-units scale of
/// `Math::SinFromTable` and `Math::CosFromTable`.
const UNITS_PER_RADIAN_F32: u32 = 0x4522_F983;

fn from_table_units(radians: X87Value) -> i32 {
    let scale = X87Chop53::load_f32(NativeF32Bits::from_bits(UNITS_PER_RADIAN_F32))
        .expect("the table scale is a finite binary32");
    X87Chop53::ftol_i32_low_masked(X87Chop53::mul(radians, scale))
}

fn table_value(entry: f32) -> X87Value {
    X87Chop53::load_f32(NativeF32Bits::from_bits(entry.to_bits()))
        .unwrap_or_else(|_| X87Chop53::load_i32(0))
}

/// Convert radians to the caller units the lookups take.
pub fn radians_to_units(radians: f64) -> f64 {
    radians * (UNITS_PER_TURN / std::f64::consts::TAU)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the retail install lives. The table check is skipped, loudly, when
    /// it is absent — the same convention the retail tile-resolution pass uses.
    fn retail_executable() -> Option<Vec<u8>> {
        let dir = std::env::var("RA2_DIR").ok()?;
        std::fs::read(std::path::Path::new(&dir).join("gamemd.exe")).ok()
    }

    #[test]
    fn indices_stay_inside_the_table() {
        // Every representable caller angle, at a stride that still visits every
        // residue class of the fold.
        for raw in (i32::MIN..=i32::MAX - 1).step_by(9973) {
            for index in [sin_index(raw), cos_index(raw)] {
                assert!(
                    (0..TABLE_LEN as i32).contains(&index),
                    "angle {raw} produced out-of-range index {index}"
                );
            }
        }
    }

    /// Cosine is the sine table a quarter period along — that relationship is
    /// the whole reason there is one array and not two.
    #[test]
    fn cosine_leads_sine_by_a_quarter_period() {
        for raw in (-40_000..40_000).step_by(7) {
            assert_eq!(
                cos_index(raw),
                sin_index(raw) + QUARTER,
                "angle {raw}: cosine is not a quarter period ahead of sine"
            );
        }
    }

    /// Periodicity holds for **even** caller units only.
    ///
    /// The fold halves the angle with a truncation toward zero, and that is not
    /// symmetric about zero: an odd negative angle rounds up where its positive
    /// counterpart a turn away rounds down, so the two land one index apart.
    /// This is the original's behaviour, not a defect — the first version of
    /// this test asserted periodicity for all angles and failed on -7997 vs
    /// 8387, which is how the asymmetry was found.
    #[test]
    fn a_full_turn_returns_to_the_same_index_for_even_angles() {
        let turn = UNITS_PER_TURN as i32;
        for raw in (-8_000..8_000).step_by(2) {
            assert_eq!(
                sin_index(raw),
                sin_index(raw + turn),
                "even angle {raw} and {} disagree a full turn apart",
                raw + turn
            );
        }
    }

    /// And the odd-angle asymmetry itself, pinned so it cannot be "fixed" later
    /// by someone who assumes it is a bug.
    #[test]
    fn odd_negative_angles_are_asymmetric_across_a_turn() {
        let turn = UNITS_PER_TURN as i32;
        assert_eq!(sin_index(-7997) - sin_index(-7997 + turn), 1);
    }

    /// The exhaustive check. Not a sample: every one of the 40964 bytes feeds
    /// the hash, and the expected value was derived by reading them out of the
    /// binary rather than computed by hand.
    #[test]
    fn the_table_matches_the_retail_image_byte_for_byte() {
        let Some(image) = retail_executable() else {
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return;
        };
        let table = TrigTable::from_executable(&image).expect("read the table");
        assert_eq!(table.entries.len(), TABLE_LEN);
        assert_eq!(
            table.fnv1a64(),
            RETAIL_FNV1A64,
            "the table read out of gamemd.exe is not the one this port was \
             written against"
        );
        assert!(table.matches_retail());
        let acos = AcosTable::from_executable(&image).expect("read the Acos table");
        assert_eq!(acos.entries.len(), ACOS_TABLE_LEN);
        assert_eq!(acos.fnv1a64(), ACOS_RETAIL_FNV1A64);
        assert!(acos.matches_retail());
        let atan = AtanTable::from_executable(&image).expect("read the atan table");
        assert_eq!(atan.entries.len(), ATAN_TABLE_LEN);
        assert_eq!(atan.fnv1a64(), ATAN_RETAIL_FNV1A64);
        assert!(atan.matches_retail());
        assert_eq!(atan.entry(0), 0.0, "atan(0) is zero");
        // Anchors read out of the image: the first step and the last entry.
        assert_eq!(atan.entry(1).to_bits(), 0x3CC7_F458);
        assert_eq!(atan.entry(ATAN_TABLE_LEN - 1).to_bits(), 0x3FC7_C820);
    }

    /// The branches of `Math::atan2 @ 0x004CAE30` that do not depend on table
    /// contents: the zero-denominator answers, the saturated index, and the
    /// quadrant reflections through binary64 pi.
    #[test]
    fn atan2_branches_follow_the_original_quadrant_rules() {
        let table = AtanTable::synthetic();
        let value = |x: f64| X87Chop53::load_f64(NativeF64Bits::from_bits(x.to_bits())).unwrap();
        let atan2 = |y: f64, x: f64| {
            f64::from_bits(
                X87Chop53::store_f64(table.atan2(value(y), value(x)))
                    .unwrap()
                    .bits(),
            )
        };
        let half_pi = f64::from(f32::from_bits(ATAN_HALF_PI_F32_BITS));
        let pi = f64::from_bits(ATAN_PI_F64_BITS);
        assert_eq!(atan2(0.0, 0.0), 0.0);
        assert_eq!(atan2(5.0, 0.0), half_pi);
        assert_eq!(atan2(-5.0, 0.0), -half_pi);
        // |y/x| / step >= 0x1001 saturates to binary32 pi/2 before reflection.
        assert_eq!(atan2(1000.0, 1.0), half_pi);
        let reflected = f64::from_bits(
            X87Chop53::store_f64(X87Chop53::sub(value(pi), value(half_pi)))
                .unwrap()
                .bits(),
        );
        assert_eq!(atan2(1000.0, -1.0), reflected);
        assert_eq!(atan2(-1000.0, -1.0), -reflected);
        // Index 0 in every quadrant: exact zero, pi, and their negations.
        assert_eq!(atan2(0.0, 7.0), 0.0);
        assert_eq!(atan2(0.0, -7.0), pi);
        // A one-step ratio reads entry 1, truncated toward zero for fractions.
        let step = f64::from(f32::from_bits(ATAN_STEP_BITS));
        assert_eq!(atan2(step * 1.5, 1.0), f64::from(table.entry(1)));
        assert_eq!(atan2(-step * 1.5, 1.0), -f64::from(table.entry(1)));
    }

    /// Anchors that would catch a table read at the wrong offset even if a hash
    /// somehow agreed, and that pin the identification as *sine*: a cosine table
    /// would have to start at 1.0.
    #[test]
    fn the_table_has_the_shape_of_a_sine() {
        let Some(image) = retail_executable() else {
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return;
        };
        let table = TrigTable::from_executable(&image).expect("read the table");
        assert_eq!(table.entry(0), 0.0, "sine starts at zero");
        assert_eq!(table.entry(PERIOD as usize / 4), 1.0, "quarter turn is one");
        assert!(
            table.entry(PERIOD as usize / 2).abs() < 1e-6,
            "half turn is zero"
        );
        assert_eq!(
            table.entry(3 * PERIOD as usize / 4),
            -1.0,
            "three-quarter turn is minus one"
        );
        // The quarter-period offset really does behave like a cosine.
        assert_eq!(table.sin(0), 0.0);
        assert_eq!(table.cos(0), 1.0);
    }

    #[test]
    fn a_non_pe_input_is_refused_rather_than_misread() {
        assert_eq!(
            TrigTable::from_executable(b"not an executable").unwrap_err(),
            TrigTableError::NotPeFile
        );
    }
}

/// Process-wide table bundle, installed once from the retail install.
///
/// A global because consumers sit at different engine layers and only startup
/// knows where the retail install is. It is written once and read-only
/// thereafter, so it cannot make a run non-deterministic.
#[derive(Debug)]
struct RetailMathTables {
    trig: TrigTable,
    acos: AcosTable,
    atan: AtanTable,
}

static TABLES: OnceLock<Option<RetailMathTables>> = OnceLock::new();

/// Read the table out of `<ra2_dir>/gamemd.exe` and install it. Later calls are
/// ignored; the first one wins.
pub fn install_from_dir(ra2_dir: &Path) {
    TABLES.get_or_init(|| {
        let path = ra2_dir.join("gamemd.exe");
        match std::fs::read(&path) {
            Ok(image) => match (
                TrigTable::from_executable(&image),
                AcosTable::from_executable(&image),
                AtanTable::from_executable(&image),
            ) {
                (Ok(trig), Ok(acos), Ok(atan))
                    if trig.matches_retail() && acos.matches_retail() && atan.matches_retail() =>
                {
                    Some(RetailMathTables { trig, acos, atan })
                }
                (Ok(_), Ok(_), Ok(_)) => {
                    log::warn!(
                        "{} holds retail math tables this build does not recognise; \
                         retail-table consumers will be disabled",
                        path.display()
                    );
                    None
                }
                (Err(err), _, _) | (_, Err(err), _) | (_, _, Err(err)) => {
                    log::warn!(
                        "retail math tables {}: {err}; retail-table consumers will be disabled",
                        path.display()
                    );
                    None
                }
            },
            Err(err) => {
                log::warn!(
                    "cannot read retail math tables from {}: {err}; \
                     retail-table consumers will be disabled",
                    path.display()
                );
                None
            }
        }
    });
}

/// The installed table, if there is one.
pub fn global() -> Option<&'static TrigTable> {
    TABLES
        .get()
        .and_then(|slot| slot.as_ref())
        .map(|tables| &tables.trig)
}

/// The installed table consumed by `Acos_lookup`, if there is one.
pub fn global_acos() -> Option<&'static AcosTable> {
    TABLES
        .get()
        .and_then(|slot| slot.as_ref())
        .map(|tables| &tables.acos)
}

/// The installed table consumed by `Math::atan2`, if there is one.
pub fn global_atan() -> Option<&'static AtanTable> {
    TABLES
        .get()
        .and_then(|slot| slot.as_ref())
        .map(|tables| &tables.atan)
}

/// Exact production `Math::atan2 @ 0x004CAE30` table access. Headless tests
/// read the retail executable when `RA2_DIR` names one and otherwise fall back
/// to a shape-compatible synthetic table; parity fixtures must check
/// [`AtanTable::matches_retail`] themselves.
pub(crate) fn required_atan_table() -> &'static AtanTable {
    if let Some(atan) = global_atan() {
        return atan;
    }

    #[cfg(test)]
    {
        static TEST_TABLE: OnceLock<AtanTable> = OnceLock::new();
        return TEST_TABLE.get_or_init(|| {
            std::env::var_os("RA2_DIR")
                .and_then(|dir| {
                    std::fs::read(std::path::PathBuf::from(dir).join("gamemd.exe")).ok()
                })
                .and_then(|image| AtanTable::from_executable(&image).ok())
                .filter(AtanTable::matches_retail)
                .unwrap_or_else(AtanTable::synthetic)
        });
    }

    #[cfg(not(test))]
    panic!("verified gamemd atan table was not installed before native math");
}

/// Stock Sonic Wave geometry is active, so a match may start only when both
/// exact executable-backed math tables are available.
pub fn wave_tables_available() -> bool {
    global().is_some() && global_acos().is_some()
}

/// Shared exact production sine/Acos access for Wave and projectile math.
/// Headless tests retain the existing Wave synthetic fixture fallback; parity
/// fixtures must install/load the retail tables and verify their hashes.
pub(crate) fn required_math_tables() -> (&'static TrigTable, &'static AcosTable) {
    if let (Some(trig), Some(acos)) = (global(), global_acos()) {
        return (trig, acos);
    }

    #[cfg(test)]
    {
        use std::sync::OnceLock;
        static TEST_TABLES: OnceLock<(TrigTable, AcosTable)> = OnceLock::new();
        let tables = TEST_TABLES.get_or_init(|| {
            let exact = std::env::var_os("RA2_DIR")
                .and_then(|dir| {
                    std::fs::read(std::path::PathBuf::from(dir).join("gamemd.exe")).ok()
                })
                .and_then(|image| {
                    let trig = TrigTable::from_executable(&image).ok()?;
                    let acos = AcosTable::from_executable(&image).ok()?;
                    (trig.matches_retail() && acos.matches_retail()).then_some((trig, acos))
                });
            exact.unwrap_or_else(|| (TrigTable::synthetic(), AcosTable::synthetic()))
        });
        return (&tables.0, &tables.1);
    }

    #[cfg(not(test))]
    panic!("verified gamemd sine/Acos tables were not installed before native math");
}
