//! Retail math tables: the shared sine/cosine table and `Acos_lookup`'s
//! arcsine table. Neither reads an installed game.
//!
//! The sine table is the repository's checked copy of the retail bytes
//! ([`crate::util::native_trig::RETAIL_SINE_TABLE`], hashed whole against
//! `RETAIL_FNV1A64` by this module's tests). Rounding a double-precision sine
//! to nearest disagrees with it on 4997 of its first 10240 indexed entries.
//! Truncating instead, gamemd's x87 rounding mode, reproduces every entry except
//! 4096 and 8192, whose tiny residues of sin pi and sin 2pi differ from double
//! precision's. So the sine bytes stay the retail ones.
//!
//! The Acos table is computed ([`AcosTable::retail`]). Tests hash it against
//! the retail bytes and, with `RA2_DIR` set, compare it byte for byte with the
//! player's `gamemd.exe`.
//!
//! Both trig functions share one table. Cosine is the same data read a quarter
//! period further along, so there is a single array and two index derivations.

use crate::util::native_x87::{
    MaskedX87Chop53, MaskedX87Value, NativeF32Bits, NativeF64Bits, X87Chop53, X87Value,
};
use std::sync::OnceLock;

/// Virtual address of the sine table in the retail image.
#[cfg(any(test, feature = "test-support"))]
const TABLE_VA: u32 = 0x0084_F084;
/// Entries. The extra quarter period past a full turn is what lets the cosine
/// offset run past the end of the sine range without wrapping; the final exact
/// `1.0` is part of the retail data extent even though the lookup ceilings stop
/// one entry before it.
#[cfg(test)]
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
#[cfg(test)]
const UNITS_PER_TURN: f64 = (2 * PERIOD) as f64;

/// FNV-1a (64-bit) over the table's raw little-endian bytes in the retail
/// image, machine-derived by reading all 40964 bytes out of the binary. This is
/// the whole-table check: it is a genuine exhaustive comparison, not a sample.
#[cfg(test)]
pub const RETAIL_FNV1A64: u64 = 0x74ac_b749_b33d_5aa7;

/// Virtual address of `Acos_lookup @ 0x004CADB0`'s signed arcsine table.
#[cfg(any(test, feature = "test-support"))]
const ACOS_TABLE_VA: u32 = 0x0085_9094;
/// Entries. `WaveClass` indexes the inclusive endpoints, hence 4097 rather than
/// 4096.
pub const ACOS_TABLE_LEN: usize = 0x1001;
/// FNV-1a (64-bit) over the retail table's 16388 little-endian bytes, read out
/// of the binary.
#[cfg(test)]
pub const ACOS_RETAIL_FNV1A64: u64 = 0x9251_751b_f328_3bc1;
/// `[0x007E8978]`: binary32 -2048, `Acos_lookup`'s negative index scale.
const ACOS_INDEX_SCALE: NativeF32Bits = NativeF32Bits::from_bits(0xc500_0000);
/// `[0x007E2820]`: binary64 pi/2, which `Acos_lookup` subtracts the entry from.
const ACOS_PI_HALF: NativeF64Bits = NativeF64Bits::from_bits(0x3ff9_21fb_5444_2d18);

/// The retail sine table at `0x0084F084`, read by `Math::SinFromTable` and
/// `Math::CosFromTable`.
#[derive(Debug, Clone)]
pub struct TrigTable {
    entries: Vec<f32>,
}

/// `Acos_lookup @ 0x004CADB0`'s signed arcsine table. The lookup indexes it
/// by `(x + 1) * 2048` and returns pi/2 minus the entry.
#[derive(Debug, Clone)]
pub struct AcosTable {
    entries: Vec<f32>,
}

impl TrigTable {
    /// The retail table, from the repository's checked copy
    /// (`util::native_trig`, verified by `walk_direction_table.py --check`).
    /// It is the only sine source: no reader depends on an installed game.
    pub fn embedded() -> &'static Self {
        static TABLE: OnceLock<TrigTable> = OnceLock::new();
        TABLE.get_or_init(|| Self {
            entries: crate::util::native_trig::RETAIL_SINE_TABLE
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&c| f32::from_le_bytes(c))
                .collect(),
        })
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

    /// `Math::SinFromTable @ 0x004CACB0` for its double argument under the
    /// process's x87 mode: the table unit is `Math::ftol` of the radians times
    /// the binary32 scale at `0x008223B0` (16384/2pi), and the entry comes back
    /// as the binary32 it is.
    pub fn sin_from_table(&self, radians: impl Into<MaskedX87Value>) -> X87Value {
        table_value(self.sin(from_table_units(radians)))
    }

    /// `Math::CosFromTable @ 0x004CAD00`, as [`Self::sin_from_table`].
    pub fn cos_from_table(&self, radians: impl Into<MaskedX87Value>) -> X87Value {
        table_value(self.cos(from_table_units(radians)))
    }
}

impl AcosTable {
    /// The retail table, computed once.
    ///
    /// Entry `i` is `asin(i / 2048 - 1)` in double precision, stored to binary32
    /// with truncation (`FSTP m32real` under gamemd's chop mode). That reproduces
    /// all 4097 retail entries (`ACOS_RETAIL_FNV1A64`). The image's table is
    /// static data: the only code that writes it, `0x004CB200` (no caller),
    /// copies each entry onto itself. Every double result lies at least 37,325
    /// ulps from a truncation boundary, and none but entry 2048's zero is an
    /// exact binary32, so any `asin` within a few ulps of correct builds the same
    /// table on every platform.
    pub(crate) fn retail() -> &'static Self {
        static TABLE: OnceLock<AcosTable> = OnceLock::new();
        TABLE.get_or_init(|| Self {
            entries: (0..ACOS_TABLE_LEN)
                .map(|index| {
                    let angle = (index as f64 / 2048.0 - 1.0).asin();
                    let angle = X87Chop53::load_f64(NativeF64Bits::from_bits(angle.to_bits()))
                        .expect("an arcsine of [-1, 1] is finite");
                    let stored = X87Chop53::store_f32(angle).expect("|asin| <= pi/2 fits binary32");
                    f32::from_bits(stored.bits())
                })
                .collect(),
        })
    }

    #[cfg(test)]
    pub fn entry(&self, index: usize) -> f32 {
        self.entries[index]
    }

    /// `Acos_lookup @ 0x004CADB0` for a finite argument: `FLD double x;
    /// FADD 1.0f; FMUL -2048.0f; Math::ftol`, then the entry at `0x00859094`
    /// minus four times that offset, subtracted from double pi/2. Returns the
    /// entry index with the angle.
    pub(crate) fn acos_lookup(&self, argument: X87Value) -> (usize, X87Value) {
        let (index, angle) = self.asin_lookup(argument);
        (
            index,
            X87Chop53::sub(X87Chop53::load_f64(ACOS_PI_HALF).unwrap(), angle),
        )
    }

    /// Original AsinFromTable4CAD80: current double argument, native index,
    /// and the raw arcsine entry (Acos4CADB0 subtracts this from pi/2).
    pub(crate) fn asin_lookup(&self, argument: X87Value) -> (usize, X87Value) {
        let argument = X87Chop53::store_f64(argument)
            .and_then(X87Chop53::load_f64)
            .expect("AsinFromTable takes a finite double");
        let scaled = X87Chop53::mul(
            X87Chop53::add(argument, X87Chop53::load_i32(1)),
            X87Chop53::load_f32(ACOS_INDEX_SCALE).expect("finite binary32 scale"),
        );
        let index = X87Chop53::ftol_i32_low_masked(scaled).wrapping_neg() as usize;
        (index, table_value(self.entries[index]))
    }

    /// `Acos_lookup` for a NaN argument (a native 0/0): `Math::ftol` returns the
    /// integer indefinite `0x80000000`, which `SHL 2` wraps to offset zero, so
    /// the lookup reads entry 0.
    pub(crate) fn acos_lookup_nan(&self) -> X87Value {
        self.lookup_angle(0)
    }

    fn lookup_angle(&self, index: usize) -> X87Value {
        let entry = NativeF32Bits::from_bits(self.entries[index].to_bits());
        X87Chop53::sub(
            X87Chop53::load_f64(ACOS_PI_HALF).expect("finite binary64 pi/2"),
            X87Chop53::load_f32(entry).expect("finite Acos entry"),
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    fn fnv1a64(&self) -> u64 {
        let mut hash = crate::util::fnv::FNV1A64_OFFSET_BASIS;
        for entry in &self.entries {
            hash = crate::util::fnv::fnv1a64_fold_bytes(hash, &entry.to_le_bytes());
        }
        hash
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
/// `Math::SinFromTable`. `Math::CosFromTable` reads its copy at `0x008223B4`,
/// which holds the same bits.
const UNITS_PER_RADIAN_F32: u32 = 0x4522_F983;

fn from_table_units(radians: impl Into<MaskedX87Value>) -> i32 {
    //4CACB0/4CAD00 call qword ftol even for a masked exceptional operand.
    // Its low dword is0 for the integer indefinite. RadBeam659B14 can reach
    // this on a short segment's zero divisor; finite callers keep the same
    // arithmetic through the shared finite owner inside MaskedX87Chop53.
    let scale = MaskedX87Chop53::load_f32(NativeF32Bits::from_bits(UNITS_PER_RADIAN_F32));
    MaskedX87Chop53::ftol_i32_low_masked(MaskedX87Chop53::mul(radians.into(), scale))
}

fn table_value(entry: f32) -> X87Value {
    X87Chop53::load_f32(NativeF32Bits::from_bits(entry.to_bits()))
        .unwrap_or_else(|_| X87Chop53::load_i32(0))
}

#[cfg(test)]
pub(crate) mod executable;
#[cfg(test)]
mod executable_tests;

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

    /// The exhaustive check of the sine source. Not a sample: every one of the
    /// 40964 checked-in bytes feeds the hash, and the expected value was
    /// derived by reading them out of the binary rather than computed by hand.
    #[test]
    fn the_checked_sine_copy_is_the_retail_table() {
        let bytes = crate::util::native_trig::RETAIL_SINE_TABLE;
        assert_eq!(bytes.len(), TABLE_LEN * 4);
        assert_eq!(
            crate::util::fnv::fnv1a64_fold_bytes(crate::util::fnv::FNV1A64_OFFSET_BASIS, bytes),
            RETAIL_FNV1A64
        );
        let table = TrigTable::embedded();
        assert_eq!(table.entries.len(), TABLE_LEN);
        assert!(
            table
                .entries
                .iter()
                .zip(bytes.as_chunks::<4>().0)
                .all(|(entry, raw)| entry.to_bits() == u32::from_le_bytes(*raw)),
            "decoding changed a checked table entry"
        );
    }

    /// The exhaustive check of the computed Acos table: every entry feeds the
    /// hash, and the expected value was read out of the binary.
    #[test]
    fn the_computed_acos_table_is_the_retail_table() {
        let acos = AcosTable::retail();
        assert_eq!(acos.entries.len(), ACOS_TABLE_LEN);
        assert_eq!(acos.fnv1a64(), ACOS_RETAIL_FNV1A64);
        // Truncation, not rounding: asin(-1) stores one ulp short of -pi/2.
        assert_eq!(acos.entry(0).to_bits(), 0xbfc9_0fda);
        assert_eq!(acos.entry(2048).to_bits(), 0);
        assert_eq!(acos.entry(4096).to_bits(), 0x3fc9_0fda);
    }

    /// The installed image carries the checked sine bytes and the computed Acos
    /// table, byte for byte, at their retail addresses.
    #[test]
    fn the_retail_image_matches_the_checked_tables_byte_for_byte() {
        let Some(image) = retail_executable() else {
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return;
        };
        let table_at = |va: u32, len: usize| {
            let offset = executable::file_offset_of(&image, va).expect("mapped table address");
            image[offset..offset + len].to_vec()
        };
        assert_eq!(
            table_at(TABLE_VA, TABLE_LEN * 4),
            crate::util::native_trig::RETAIL_SINE_TABLE.as_slice()
        );
        let computed: Vec<u8> = AcosTable::retail()
            .entries
            .iter()
            .flat_map(|entry| entry.to_le_bytes())
            .collect();
        assert_eq!(table_at(ACOS_TABLE_VA, ACOS_TABLE_LEN * 4), computed);
    }

    #[test]
    fn the_table_has_the_shape_of_a_sine() {
        let table = TrigTable::embedded();
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
}
