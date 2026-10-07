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
//! from the player's own copy, at load time. Executable versions may place them
//! at different addresses: compatibility depends on the tables' contents, not
//! the executable's checksum, language, signature or other unrelated bytes.
//!
//! Both trig functions share one table. Cosine is the same data read a quarter
//! period further along, so there is a single array and two index derivations.

use std::fmt;

use crate::util::native_x87::{NativeF32Bits, X87Chop53, X87Value};
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
// First four entries read from the reference table at TABLE_VA. These prefixes
// only locate candidates; the complete table must still pass its FNV check.
const TABLE_PREFIX: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0xd9, 0x0f, 0x49, 0x3a, 0xd5, 0x0f, 0xc9, 0x3a, 0xdb, 0xcb, 0x16, 0x3b,
];

/// Virtual address and exact size of `Acos_lookup @ 0x004CADB0`'s signed
/// arcsine table. `WaveClass` indexes the inclusive endpoints, hence 4097
/// entries rather than 4096.
const ACOS_TABLE_VA: u32 = 0x0085_9094;
pub const ACOS_TABLE_LEN: usize = 0x1001;
pub const ACOS_RETAIL_FNV1A64: u64 = 0x9251_751b_f328_3bc1;
const ACOS_TABLE_PREFIX: &[u8] = &[
    0xda, 0x0f, 0xc9, 0xbf, 0xcf, 0x0f, 0xc5, 0xbf, 0x94, 0x67, 0xc3, 0xbf, 0x04, 0x22, 0xc2, 0xbf,
];

/// Something went wrong reading the table out of the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrigTableError {
    NotPeFile,
    UnmappedAddress(u32),
    Truncated { need: usize, have: usize },
    TableNotFound(u32),
}

impl fmt::Display for TrigTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPeFile => write!(f, "not a valid PE32 executable"),
            Self::UnmappedAddress(va) => {
                write!(f, "address {va:#010x} is not inside any section")
            }
            Self::Truncated { need, have } => {
                write!(
                    f,
                    "executable data needs {need} bytes, only {have} available"
                )
            }
            Self::TableNotFound(va) => {
                write!(
                    f,
                    "required math table (reference address {va:#010x}) was not found in \
                     executable sections; the original uncompressed table bytes are required"
                )
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

#[derive(Debug)]
struct FileSection<'a> {
    rva: u32,
    file_offset: usize,
    bytes: &'a [u8],
}

#[derive(Debug)]
struct PeSections<'a> {
    image_base: u32,
    sections: Vec<FileSection<'a>>,
}

impl<'a> PeSections<'a> {
    /// Read only initialized, file-backed section data. In particular, a
    /// section's zero-filled virtual tail and the certificate overlay are not
    /// table storage. PE32 layout: https://learn.microsoft.com/en-us/windows/win32/debug/pe-format
    fn parse(image: &'a [u8]) -> Result<Self, TrigTableError> {
        let u16_at = |bytes: &[u8], off: usize| -> Result<u16, TrigTableError> {
            let word = bytes
                .get(off..)
                .and_then(|tail| tail.get(..2))
                .ok_or(TrigTableError::NotPeFile)?;
            Ok(u16::from_le_bytes([word[0], word[1]]))
        };
        let u32_at = |bytes: &[u8], off: usize| -> Result<u32, TrigTableError> {
            let word = bytes
                .get(off..)
                .and_then(|tail| tail.get(..4))
                .ok_or(TrigTableError::NotPeFile)?;
            Ok(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        };

        if image.get(..2) != Some(b"MZ") {
            return Err(TrigTableError::NotPeFile);
        }
        let pe = image
            .get(u32_at(image, 0x3c)? as usize..)
            .ok_or(TrigTableError::NotPeFile)?;
        if pe.get(..4) != Some(b"PE\0\0") {
            return Err(TrigTableError::NotPeFile);
        }
        let section_count = usize::from(u16_at(pe, 6)?);
        let optional_size = usize::from(u16_at(pe, 20)?);
        let optional_and_sections = pe.get(24..).ok_or(TrigTableError::NotPeFile)?;
        let optional = optional_and_sections
            .get(..optional_size)
            .ok_or(TrigTableError::NotPeFile)?;
        // PE32+, or a short optional header, must never be decoded as PE32.
        if optional.len() < 96 || u16_at(optional, 0)? != 0x10b {
            return Err(TrigTableError::NotPeFile);
        }
        let image_base = u32_at(optional, 28)?;
        let headers = optional_and_sections
            .get(optional_size..)
            .and_then(|tail| tail.get(..section_count * 40))
            .ok_or(TrigTableError::NotPeFile)?;
        let mut sections = Vec::with_capacity(section_count);
        for header in headers.chunks_exact(40) {
            let virtual_size = u32_at(header, 8)? as usize;
            let rva = u32_at(header, 12)?;
            let raw_size = u32_at(header, 16)? as usize;
            let file_offset = u32_at(header, 20)? as usize;
            if raw_size == 0 {
                continue;
            }
            let raw = image
                .get(file_offset..)
                .and_then(|tail| tail.get(..raw_size))
                .ok_or(TrigTableError::Truncated {
                    need: raw_size,
                    have: image.len().saturating_sub(file_offset),
                })?;
            let mapped_size = if virtual_size == 0 {
                raw_size
            } else {
                raw_size.min(virtual_size)
            };
            sections.push(FileSection {
                rva,
                file_offset,
                bytes: &raw[..mapped_size],
            });
        }
        Ok(Self {
            image_base,
            sections,
        })
    }

    fn at_va(&self, va: u32) -> Option<(usize, &'a [u8])> {
        let rva = va.checked_sub(self.image_base)?;
        self.sections.iter().find_map(|section| {
            let within = rva.checked_sub(section.rva)? as usize;
            (within < section.bytes.len())
                .then(|| (section.file_offset + within, &section.bytes[within..]))
        })
    }
}

/// Translate a VA to initialized section bytes, excluding virtual zero-fill.
#[cfg(test)]
pub(crate) fn file_offset_of(image: &[u8], va: u32) -> Result<usize, TrigTableError> {
    PeSections::parse(image)?
        .at_va(va)
        .map(|(offset, _)| offset)
        .ok_or(TrigTableError::UnmappedAddress(va))
}

/// The reference VA is only a fast path. Releases can move identical data
/// without changing the math consumed by VERA20k. A short native prefix avoids
/// hashing a whole table at every offset; both paths check every table byte.
fn find_table_bytes<'a>(
    image: &'a [u8],
    reference_va: u32,
    byte_len: usize,
    prefix: &[u8],
    expected_fnv: u64,
) -> Result<&'a [u8], TrigTableError> {
    let pe = PeSections::parse(image)?;
    let matches = |bytes: &[u8]| {
        crate::util::fnv::fnv1a64_fold_bytes(crate::util::fnv::FNV1A64_OFFSET_BASIS, bytes)
            == expected_fnv
    };
    if let Some((_, bytes)) = pe.at_va(reference_va)
        && let Some(table) = bytes.get(..byte_len)
        && matches(table)
    {
        return Ok(table);
    }
    for section in &pe.sections {
        if let Some(table) = section
            .bytes
            .windows(byte_len)
            .find(|bytes| bytes.starts_with(prefix) && matches(bytes))
        {
            return Ok(table);
        }
    }
    Err(TrigTableError::TableNotFound(reference_va))
}

impl TrigTable {
    /// The repository's checked copy of the retail table
    /// (`util::native_trig`, verified by `walk_direction_table.py --check`),
    /// for simulation readers that must not depend on an installed game.
    /// Hover's altitude bob (0x00513E4F) reads `Math__SinFromTable` here.
    pub(crate) fn embedded() -> &'static Self {
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

    /// Read the table out of a retail `gamemd.exe` image.
    pub fn from_executable(image: &[u8]) -> Result<Self, TrigTableError> {
        let bytes = find_table_bytes(image, TABLE_VA, TABLE_LEN * 4, TABLE_PREFIX, RETAIL_FNV1A64)?;
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
        let bytes = find_table_bytes(
            image,
            ACOS_TABLE_VA,
            ACOS_TABLE_LEN * 4,
            ACOS_TABLE_PREFIX,
            ACOS_RETAIL_FNV1A64,
        )?;
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
    }

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
            ) {
                (Ok(trig), Ok(acos)) => Some(RetailMathTables { trig, acos }),
                (Err(err), _) | (_, Err(err)) => {
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
                    Some((trig, acos))
                });
            exact.unwrap_or_else(|| (TrigTable::synthetic(), AcosTable::synthetic()))
        });
        return (&tables.0, &tables.1);
    }

    #[cfg(not(test))]
    panic!("verified gamemd sine/Acos tables were not installed before native math");
}
