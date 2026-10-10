//! The one INI reader API: gamemd's `CCINIClass` `ReadX` family.
//!
//! Every INI value VERA consumes goes through the `IniSection` readers below
//! and the token parsers under them. `ini_parser.rs` is the raw store (the
//! `INIClass` analog): loading and exact-case lookup. This module is its child,
//! so only these readers see raw value text; other code tests presence with
//! `IniSection::is_present`. The store's two walks, `raw_entries` and
//! `registry_ids`, are not readers, and `architecture_guards` pins their
//! callers. Its unit tests run under `rules::ini_parser::ini_value::`.
//!
//! Each reader reproduces one native reader's contract on the resolved value.
//! Where gamemd reads some keys through a different parser, that parser is its
//! own reader here, named for the native function it mirrors:
//!
//! | Reader | Native |
//! |---|---|
//! | `read_int` | `CCINIClass::ReadInt` 0x005276D0 |
//! | `read_bool`, `read_bool_value` | `CCINIClass::ReadBool` 0x005295F0 |
//! | `read_double` (`_bits`, `_to_float`, `_with`, `read_float`) | `CCINIClass::ReadDouble` 0x005283D0 |
//! | `read_string`, `read_name`, `read_type_name` | `CCINIClass::ReadString` 0x00528A10 |
//! | `read_movie` | `CCINIClass::ReadMovie` 0x004757D0 |
//! | `read_edge` | `CCINIClass::ReadEdge` 0x00475980 |
//! | `read_color_scheme` | `CCINIClass::ReadColor` 0x00474A90 |
//! | `read_list` | ReadString, then `strtok(",")` (every type, house, sound and ability list) |
//! | `read_sound_list` | `CCINIClass::ReadSoundList` 0x00525430 (ReadString 0x80, `strtok`) |
//! | `read_trimmed_list` | ReadString, then `CString::Tokenize(",")` 0x007B5F10 and space trims |
//! | `read_string_with` | ReadString, then the caller's own parse of the buffer |
//! | `read_int_fields` (`_complete`) | ReadString, then `sscanf("%d,%d,...")` over the current fields |
//! | `read_int_list` | `DifficultyClass::ReadINI_IntVector` 0x00475D70 |
//! | `read_coord3` | `CCINIClass::Read3Int` 0x00529CA0, `sscanf("%d,%d,%d")` |
//! | `read_coord_tokens` | `0x00476420`: three `strtok(",")` + `atoi` |
//! | `read_float_tokens` | `0x00476340`: three `strtok(",")` + `atof` |
//! | `read_color_list` | `0x00476B20`: `strtok(",")` in `(r,g,b)` groups |
//! | `read_minmax` | `CCINIClass::ReadMinMax` 0x00529880, `sscanf("%d,%d")` |
//! | `read_rect` | `INIClass::ReadRect` 0x00527CC0, `sscanf("%d,%d,%d,%d")` |
//! | `read_color_rgb` | `CCINIClass::ReadColorRGB` 0x00474B50 |
//! | `read_speed`, `read_range`, `read_speed_type` | 0x00474810, 0x00474620, 0x00476FC0 |
//! | `read_comma_hex_utf16` | `INIClass::ReadCommaHexUTF16` 0x00528F00 |
//! | `read_packed_text` | packed-section reader 0x00526FB0 (IsoMapPack5, OverlayPack, PreviewPack) |
//!
//! Token parsers for text a reader has already copied: [`strtok`] (CRT
//! 0x007C9CC2), [`crt_atoi`] (CRT 0x007C9B72), [`parse_leading_f64`] (CRT
//! `atof`), [`scan_decimal_i32`] (one `sscanf` `%d`).
//!
//! INVARIANT: the raw loader omits empty keys and empty values. A stored
//! nonempty key returns its parsed value (malformed numeric text may still
//! parse as zero); `default` is returned when a key is absent or omitted.
//!
//! Native call sites and their buffer capacities come from the retail
//! executable: `python -m tools.native_inspect calls <reader VA>` lists a
//! reader's direct callers, and `disasm` before a call shows the pushed key,
//! default and capacity.
//!
//! ## Dependency rules
//! - rules/ only: depends on `crate::rules::ini_parser` and `crate::util::native_x87`.
//!   No sim/render/ui/audio/net.
//! - Returns un-truncated f64 from `read_double`; the single f64->SimFixed
//!   conversion stays in `util::fixed_math`. No float enters sim/.

use super::{IniSection, is_native_none_type_name};
use crate::rules::locomotor_type::SpeedType;
use crate::util::native_x87::{MaskedX87Chop53, NativeF32Bits, NativeF64Bits};

/// 0x20 = ASCII space; gamemd `strtrim` strips bytes <= 0x20 (space + all ASCII
/// control) at BOTH ends — NOT Unicode whitespace.
const STRTRIM_MAX: u8 = 0x20;

/// The comma delimiter every native list reader passes to `strtok`
/// (`DAT_00817F70`).
const COMMA: &[char] = &[','];

impl IniSection {
    /// ReadColor474A90: ReadString32 defaults to the current scheme's name,
    /// then a case-insensitive scan skips the shade-count1 member of each
    /// registered pair. Unknown names keep the supplied native index. A
    /// missing key can therefore normalize valid shade1 index0 to shade53
    /// index1; a stored-empty copy does not match and retains the current index.
    ///
    /// The catalog is the one present at this read, not a later rules-pass
    /// projection. Native House500DF7 validates the result separately.
    pub(crate) fn read_color_scheme<'a>(
        &self,
        key: &str,
        current: i32,
        mut schemes: impl Iterator<Item = &'a str> + Clone,
    ) -> i32 {
        let default_name = usize::try_from(current)
            .ok()
            .and_then(|index| schemes.clone().nth(index / 2))
            .unwrap_or("");
        let name = self.read_string(key, default_name, 0x20);
        schemes
            .position(|scheme| scheme.eq_ignore_ascii_case(&name))
            .map_or(current, |index| {
                (index as i32).wrapping_mul(2).wrapping_add(1)
            })
    }

    /// CCINIClass::ReadEdge475980: ReadString into128 bytes with an empty
    /// default. Empty retains the caller's value; case-insensitive cardinal
    /// names map to0..3 and every other nonempty name returns literal-1.
    pub fn read_edge(&self, key: &str, default: i32) -> i32 {
        self.read_string_with(key, 128, default, |_, value| {
            ["North", "East", "South", "West"]
                .iter()
                .position(|name| name.eq_ignore_ascii_case(value))
                .map_or(-1, |index| index as i32)
        })
    }

    /// `CCINIClass::ReadMovie @ 0x004757D0`: ReadString with an empty
    /// default and a 128-byte buffer, followed by the process movie lookup.
    /// Empty, `<none>` and unknown names retain the caller's current index.
    pub fn read_movie(
        &self,
        key: &str,
        default: i32,
        movies: &crate::rules::movies::MovieRegistry,
    ) -> i32 {
        let name = self.read_string(key, "", 128);
        let index = movies.find_index(&name);
        if index == -1 { default } else { index }
    }

    fn fold_rules_values<T>(
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
            match self.get(key) {
                Some(raw) => apply(default, raw),
                None => default,
            }
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

    /// [`Self::read_bool`] where the caller supplies the default later: `None`
    /// wherever ReadBool would return its default argument (an absent key,
    /// or a first character outside `1TY0FN`).
    pub fn read_bool_value(&self, key: &str) -> Option<bool> {
        self.fold_rules_values(key, None, |current, raw| {
            read_bool_decision(raw).or(current)
        })
    }

    /// ReadDouble (P7, `0x005283D0`): sscanf "%f" (leading float,
    /// single-precision) widened to f64, then ×0.01 chopped at 53 bits iff the
    /// value string contains '%' ANYWHERE ([`parse_read_double`]). Returns the
    /// gamemd double UN-truncated; the consumer truncates toward zero at ITS
    /// boundary (never `.round()` / never truncate here). Default ONLY on absent.
    /// Present junk is absent from stock retail data; native exposes stale
    /// 32-bit ABI argument bits on failed `%f`, while Rust deterministically
    /// returns zero rather than importing that non-portable accident.
    ///
    /// A native double field VERA keeps as `f32` narrows the result at the
    /// call site (`as f32`, round-to-nearest). A native float field reads
    /// through [`Self::read_float`] / [`Self::read_double_to_float`], which
    /// store with the chop control word; the two differ only for a percent
    /// value that is not exact in `f32`.
    pub fn read_double(&self, key: &str, default: f64) -> f64 {
        self.fold_rules_values(key, default, |_current, raw| parse_read_double(raw))
    }

    /// ReadDouble on each rules pass, handing the scanned value and the
    /// field's current value to `apply`: the shape of native readers that
    /// test or scale the result before storing it (a zero that keeps the
    /// field, a minutes-to-frames scale). Absent keeps `current`.
    pub fn read_double_with<T>(
        &self,
        key: &str,
        current: T,
        mut apply: impl FnMut(T, f64) -> T,
    ) -> T {
        self.fold_rules_values(key, current, |current, raw| {
            apply(current, parse_read_double(raw))
        })
    }

    /// ReadDouble into a double field, keeping the stored bits.
    pub fn read_double_bits(&self, key: &str, current: NativeF64Bits) -> NativeF64Bits {
        NativeF64Bits::from_bits(
            self.read_double(key, f64::from_bits(current.bits()))
                .to_bits(),
        )
    }

    /// ReadDouble into a float field: `FLD dword` widens the field into the
    /// default and `FSTP dword` stores the result back under the process's
    /// chop control word.
    pub fn read_double_to_float(&self, key: &str, current: NativeF32Bits) -> NativeF32Bits {
        MaskedX87Chop53::store_f32_masked_chop(MaskedX87Chop53::load_f64(self.read_double_bits(
            key,
            NativeF64Bits::from_bits(f64::from(f32::from_bits(current.bits())).to_bits()),
        )))
    }

    /// [`Self::read_double_to_float`] for a float field VERA holds as `f32`.
    pub fn read_float(&self, key: &str, current: f32) -> f32 {
        f32::from_bits(
            self.read_double_to_float(key, NativeF32Bits::from_bits(current.to_bits()))
                .bits(),
        )
    }

    /// ReadString (P5, P18): copy at most `capacity - 1` bytes, force the final
    /// NUL, then trim bytes ≤0x20 at both ends. Capacities are caller-specific
    /// in retail, so they are explicit here too.
    ///
    /// `CCINIClass__ReadString @ 0x00528A10` is `strncpy(dst, src, capacity)`
    /// followed by `dst[capacity - 1] = 0`: the cut counts source bytes, which
    /// are this store's characters ([`truncate_native_bytes`]).
    /// A present value is copied even when it trims to nothing; each rules
    /// pass passes the previous result as its default, so the last pass that
    /// holds the key decides.
    pub fn read_string(&self, key: &str, default: &str, capacity: usize) -> String {
        let Some(payload) = capacity.checked_sub(1) else {
            return String::new();
        };
        strtrim_ascii(truncate_native_bytes(
            self.get(key).unwrap_or(default),
            payload,
        ))
        .to_string()
    }

    /// ReadString's copy when it returns nonzero: `None` when no rules pass
    /// holds a value that survives the cut and trim, where native callers keep
    /// their current field (`if (ReadString(section, key, "", buffer,
    /// capacity)) ...`), so a later pass whose copy is empty keeps the earlier
    /// pass's value. Same cut and trim as [`Self::read_string`].
    pub fn read_name(&self, key: &str, capacity: usize) -> Option<&str> {
        let payload = capacity.checked_sub(1)?;
        let copy = |raw: &str| !strtrim_ascii(truncate_native_bytes(raw, payload)).is_empty();
        let raw = match self.projected_values(key) {
            Some(values) => values
                .iter()
                .rev()
                .map(String::as_str)
                .find(|raw| copy(raw))?,
            None => self.get(key).filter(|raw| copy(raw))?,
        };
        Some(strtrim_ascii(truncate_native_bytes(raw, payload)))
    }

    /// [`Self::read_name`] for a name a type factory resolves: `None` also for
    /// `none` and `<none>`, which the factories answer null
    /// ([`is_native_none_type_name`]).
    pub fn read_type_name(&self, key: &str, capacity: usize) -> Option<&str> {
        self.read_name(key, capacity)
            .filter(|name| !is_native_none_type_name(name))
    }

    /// The list read every native type, house, sound and ability list shares:
    /// ReadString(key, "", buffer, `capacity`), then [`strtok`] on `","`.
    /// `None` when ReadString returns 0 (absent key), where the native
    /// readers keep the caller's current list. A present value replaces it,
    /// even with no tokens (`,,,`). Tokens keep their spaces; resolving each
    /// one (type lookup, `atoi`, ability name) is the caller's.
    ///
    /// Native buffers: 0x80 for type, house, sound and ability lists
    /// (`0x0067B550`, `0x004750D0`, `0x00525430`, `0x00477640`,
    /// `Prerequisite_INI_Parser @ 0x004770E0`); 0x200 for the int vectors.
    pub fn read_list(&self, key: &str, capacity: usize) -> Option<Vec<&str>> {
        self.read_name(key, capacity)
            .map(|value| strtok(value, COMMA).collect())
    }

    /// ReadHousesList475260: ReadString128, comma-only strtok, then the
    /// caller's byte-exact House::FromName50C170 lookup. An unknown (-1)
    /// index sets bit31 through x86's masked shift. Missing/empty keeps the
    /// supplied mask. House array order, not token order, consumes this mask.
    pub(crate) fn read_houses_list(
        &self,
        key: &str,
        default: u32,
        mut find_house: impl FnMut(&str) -> Option<usize>,
    ) -> u32 {
        let Some(tokens) = self.read_list(key, 0x80) else {
            return default;
        };
        tokens.into_iter().fold(0, |mask, token| {
            mask | 1u32.wrapping_shl(find_house(token).map_or(u32::MAX, |index| index as u32))
        })
    }

    /// `CCINIClass::ReadSoundList @ 0x00525430`: [`Self::read_list`] at 0x80.
    /// `None` for an absent key, which keeps the current list.
    ///
    /// RESIDUAL: native adds only the tokens `VocClass::FindPtrByName`
    /// (`0x00751520`) resolves (`0x005254AD`). VoiceSelect uses the fixed
    /// SoundRegistry binder; other current consumers keep every token.
    /// Trigger: a token naming a sound `soundmd.ini` lacks,
    /// spaces included. Effect: a longer list, so a pick drawn over it (death
    /// sounds, Gattling and per-shot reports) can choose a different item.
    /// Frequency: never on retail data.
    pub fn read_sound_list(&self, key: &str) -> Option<Vec<&str>> {
        self.read_list(key, 0x80)
    }

    /// [`Self::read_list`] through the `CString` tokenizer instead: the
    /// ReadString copy goes through `Tokenize(",")` (`0x007B5F10`, the same
    /// boundaries as [`strtok`]) and each token is trimmed of spaces only
    /// (`0x007B51D0`/`0x007B5230` with `" "`). A token of spaces stays as an
    /// empty token. Used by the game-mode roster rows and the map and PKT
    /// `GameMode=` filter lists.
    pub fn read_trimmed_list(&self, key: &str, capacity: usize) -> Option<Vec<&str>> {
        self.read_list(key, capacity).map(|tokens| {
            tokens
                .into_iter()
                .map(|token| token.trim_matches(' '))
                .collect()
        })
    }

    /// ReadString into `char[capacity]` on each rules pass, handing the
    /// copied text and the field's current value to `apply`: the shape of
    /// native readers that parse the ReadString buffer themselves and keep
    /// the field on text they reject (enum names, catalog lookups). Absent
    /// keeps `current`.
    pub fn read_string_with<T>(
        &self,
        key: &str,
        capacity: usize,
        current: T,
        mut apply: impl FnMut(T, &str) -> T,
    ) -> T {
        let Some(payload) = capacity.checked_sub(1) else {
            return current;
        };
        self.fold_rules_values(key, current, |current, raw| {
            match strtrim_ascii(truncate_native_bytes(raw, payload)) {
                "" => current,
                value => apply(current, value),
            }
        })
    }

    /// ReadString into `char[capacity]`, then one `sscanf("%d,%d,...")`
    /// over `current`: each converted field replaces its slot and the scan
    /// stops at the first it cannot convert (a comma must follow each number
    /// at once). Absent keeps `current`.
    pub fn read_int_fields<const N: usize>(
        &self,
        key: &str,
        capacity: usize,
        current: [i32; N],
    ) -> [i32; N] {
        self.read_string_with(key, capacity, current, |mut out, value| {
            scan_int_fields(value, &mut out);
            out
        })
    }

    /// [`Self::read_int_fields`]' scan alone: `Some` only when all `N`
    /// fields convert, where a caller rejects text the native scan would
    /// leave partly unwritten.
    pub fn read_int_fields_complete<const N: usize>(
        &self,
        key: &str,
        capacity: usize,
    ) -> Option<[i32; N]> {
        let mut out = [0; N];
        let value = self.read_name(key, capacity)?;
        (scan_int_fields(value, &mut out) == N).then_some(out)
    }

    /// `DifficultyClass::ReadINI_IntVector @ 0x00475D70`: ReadString into a
    /// 0x200 buffer, `strtok(",")`, [`crt_atoi`] per token. `None` for an
    /// absent key, where the native reader copies the caller's vector.
    pub fn read_int_list(&self, key: &str) -> Option<Vec<i32>> {
        self.read_list(key, 0x200)
            .map(|tokens| tokens.into_iter().map(crt_atoi).collect())
    }

    /// `0x00476340` (ParticleSystemType `SpawnDirection=`): ReadString into
    /// 0x200 bytes, then three `strtok(",")` tokens through CRT `atof`
    /// ([`parse_leading_f64`]) into a float vector. Absent keeps `default`;
    /// a missing token reads as 0 where native passes NULL to `atof`.
    pub fn read_float_tokens(&self, key: &str, default: [f32; 3]) -> [f32; 3] {
        let Some(tokens) = self.read_list(key, 0x200) else {
            return default;
        };
        std::array::from_fn(|index| {
            tokens
                .get(index)
                .map_or(0.0, |token| parse_leading_f64(token) as f32)
        })
    }

    /// `0x00476B20` (ParticleType `ColorList=`): ReadString into 0x200 bytes,
    /// then `strtok(",")` in groups of three. The first token of a group
    /// loses its first byte (the `(`), the third its last (the `)`), and each
    /// goes through [`crt_atoi`] into a byte; a group missing a token adds
    /// nothing. `None` for an absent key, which keeps the current list.
    pub fn read_color_list(&self, key: &str) -> Option<Vec<[u8; 3]>> {
        let tokens = self.read_list(key, 0x200)?;
        let mut tokens = tokens.into_iter();
        let mut colors = Vec::new();
        while let Some(red) = tokens.next() {
            let (Some(green), Some(blue)) = (tokens.next(), tokens.next()) else {
                break;
            };
            let red = red
                .char_indices()
                .nth(1)
                .map_or("", |(start, _)| &red[start..]);
            let blue = blue
                .char_indices()
                .last()
                .map_or("", |(end, _)| &blue[..end]);
            colors.push([red, green, blue].map(|component| crt_atoi(component) as u8));
        }
        Some(colors)
    }

    /// TechnoTypeClass::ReadINI's text-coordinate read `0x00476420`
    /// (`DamageSmokeOffset`, `DestroySmokeOffset`, `RefinerySmokeOffset*`,
    /// `NaturalParticleLocation`; ParticleSystemType `NextParticleOffset`):
    /// ReadString into 0x200 bytes, then three `strtok(",")` tokens through
    /// [`crt_atoi`]. Unlike [`Self::read_coord3`], a space before a comma is
    /// harmless, and stock `100, 100, 275` reads whole. Absent keeps
    /// `default`.
    ///
    /// Fewer than three tokens pass NULL to `atoi`, which faults natively;
    /// VERA reads each missing component as 0. Stock values are all triples.
    pub fn read_coord_tokens(&self, key: &str, default: [i32; 3]) -> [i32; 3] {
        let Some(tokens) = self.read_list(key, 0x200) else {
            return default;
        };
        std::array::from_fn(|index| tokens.get(index).map_or(0, |token| crt_atoi(token)))
    }

    /// TechnoType7121D1..7121EB -> ReadSpeedType476FC0: exact key,
    /// 128-byte ReadString, empty retains the current field, other unknown
    /// names store -1 without clamping. Each rules pass supplies its prior
    /// field as default. Executed controls: rules_oracle/infantry_speed_type.
    pub fn read_speed_type(&self, key: &str, default: SpeedType) -> SpeedType {
        self.fold_rules_values(key, default, |current, raw| {
            let value = strtrim_ascii(truncate_native_bytes(raw, 127));
            if value.is_empty() {
                current
            } else {
                SpeedType::from_ini(value)
            }
        })
    }

    /// ReadCoord529CA0 (`CCINIClass` coordinate read `0x00529CA0`, the art FLH
    /// keys): a 64-byte buffer, trim, then `%d,%d,%d` (`0x008189B0`), each `,`
    /// a literal that must follow the previous number at once.
    /// Missing/empty input returns the current coordinate. Complete decimal
    /// triples retain signed32 wrapping and ignore text after the third number.
    /// Original incomplete nonempty scans expose stale ABI argument bits (the
    /// key, section and default pointers); modded malformed coordinates
    /// deterministically retain `default` here. Stock coordinates are all
    /// full triples.
    /// Executable coverage: tools/projectile_oracle/ifv_fire_coord.
    pub fn read_coord3(&self, key: &str, default: [i32; 3]) -> [i32; 3] {
        self.read_coord3_value(key).unwrap_or(default)
    }

    /// [`Self::read_coord3`]'s scan alone: `None` wherever it keeps its default.
    pub fn read_coord3_value(&self, key: &str) -> Option<[i32; 3]> {
        self.read_int_fields_complete(key, 64)
    }

    /// `CCINIClass::ReadMinMax @ 0x00529880`: the value cut to 63 bytes and
    /// trimmed, then `sscanf("%d,%d")` (`0x0081C000`); the comma must follow
    /// the first number at once. Absent keeps `default`.
    ///
    /// The scan's destinations are the reader's own section and key argument
    /// slots, so an incomplete scan stores stale pointer bits natively; like
    /// [`Self::read_coord3`], VERA keeps `default` for that malformed input.
    /// Each rules pass supplies its prior pair.
    pub fn read_minmax(&self, key: &str, default: [i32; 2]) -> [i32; 2] {
        self.fold_rules_values(key, default, |current, raw| {
            let mut out = [0; 2];
            let complete = scan_int_fields(strtrim_ascii(truncate_native_bytes(raw, 63)), &mut out)
                == out.len();
            if complete { out } else { current }
        })
    }

    /// `INIClass::ReadRect @ 0x00527CC0`: a missing key scans the reader's
    /// literal `"0,0,0,0"` (`0x00825BC8`) whatever the caller's default. A
    /// present value is cut to 63 bytes, trimmed, and scanned with
    /// `sscanf("%d,%d,%d,%d")` over destinations preloaded from `default`, so
    /// a valid prefix overlays the defaults field by field and text after the
    /// fourth number is ignored.
    pub fn read_rect(&self, key: &str, default: [i32; 4]) -> [i32; 4] {
        let Some(value) = self.read_name(key, 64) else {
            return [0; 4];
        };
        let mut out = default;
        scan_int_fields(value, &mut out);
        out
    }

    /// Original474B50: ReadString64 then one `%d,%d,%d` scan, byte narrowing.
    /// Literal commas must immediately follow each of the first two numbers;
    /// a suffix there stops the WHOLE scan. Absent/empty input keeps the default.
    /// Original incomplete nonempty scans copy uninitialized stack bytes. We
    /// deterministically retain the current RGB for that malformed domain,
    /// matching read_coord3's policy instead of reproducing fixture stack data.
    /// Full-reader controls: tools/projectile_oracle/line_trail.json.
    pub fn read_color_rgb(&self, key: &str, default: [u8; 3]) -> [u8; 3] {
        self.fold_rules_values(key, default, |current, raw| {
            let mut out = [0; 3];
            let value = strtrim_ascii(truncate_native_bytes(raw, 63));
            if scan_int_fields(value, &mut out) == out.len() {
                out.map(|component| component as u8)
            } else {
                current
            }
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

    /// TechnoType ReadINI71464A..71469F reads Speed through ReadInt(-1).
    /// Each -1 keeps the prior rules-pass value; other values are signed
    /// clamped to0..100. Retain that canonical percent representation here;
    /// util::fixed_math::ra2_speed_to_leptons_per_frame owns conversion to the
    /// native Type+678 whole leptons. This is distinct from ReadSpeed474810.
    /// Native execution: spatial_oracle/base_defense_response speed_history.
    pub fn read_techno_speed(&self, key: &str, default: i32) -> i32 {
        self.fold_rules_values(key, default, |current, raw| {
            let parsed = parse_read_int(-1, raw);
            if parsed == -1 {
                current
            } else {
                parsed.clamp(0, 100)
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
    /// like it does. Fixed-point `to_num::<i32>()` instead floors toward −∞;
    /// it is not interchangeable with this reader's chop conversion.
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

    /// `ReadINIBase64BinarySectionSourceOrder @ 0x00526FB0` (IsoMapPack5,
    /// OverlayPack, OverlayDataPack, PreviewPack): every entry's value in
    /// source order, each cut to 127 bytes and trimmed, joined for the Base64
    /// decoder. Entry names are not parsed or sorted.
    pub fn read_packed_text(&self) -> String {
        self.keys()
            .filter_map(|key| self.get(key))
            .map(|value| strtrim_ascii(truncate_native_bytes(value, 0x7F)))
            .collect()
    }

    /// `INIClass::ReadCommaHexUTF16 @ 0x00528F00` on this section's `key`; see
    /// [`read_comma_hex_utf16`]. The failed-first-token scratch is this
    /// section name's `CRCEngine` hash, as on the native fresh-file cache miss.
    pub fn read_comma_hex_utf16(&self, key: &str, default: &[u16], max_units: usize) -> Vec<u16> {
        let name: Vec<u8> = self.name.chars().map(|character| character as u8).collect();
        read_comma_hex_utf16(
            self.get(key),
            default,
            max_units,
            crate::assets::mix_hash::crc_engine(&name),
        )
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
fn read_comma_hex_utf16(
    raw: Option<&str>,
    default: &[u16],
    max_units: usize,
    section_crc: u32,
) -> Vec<u16> {
    let raw = raw.unwrap_or_default();
    let raw = &raw[..raw.find('\0').unwrap_or(raw.len())];
    // strtrim at 0x0052909F removes bytes <= 0x20, unlike sscanf whitespace.
    let value = strtrim_ascii(truncate_native_bytes(raw, COMMA_HEX_ENCODED_BYTES));
    if value.is_empty() {
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
    for token in strtok(value, COMMA).take(max_units) {
        if let Some(value) = scan_hex_u32(token.as_bytes()) {
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
    let start = token.iter().position(|b| !is_crt_space(*b))?;
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

/// ReadInt's parse of the stored value, untrimmed (`0x0052784A..0x005278C6`):
/// a leading `$` scans `"$%x"`, a last byte `h`/`H` scans `"%xh"` (a failed
/// `%x`, `None`, keeps the caller's default), anything else is [`crt_atoi`].
/// Executed controls: tools/rules_oracle/ini_token_readers.
pub(crate) fn parse_read_int_value(raw: &str) -> Option<i32> {
    let hex = if let Some(rest) = raw.strip_prefix('$') {
        rest
    } else if raw
        .as_bytes()
        .last()
        .is_some_and(|b| b.eq_ignore_ascii_case(&b'h'))
    {
        raw
    } else {
        return Some(crt_atoi(raw));
    };
    scan_hex_u32(hex.as_bytes()).map(|value| value as i32)
}

/// ReadBool's parse of a present value; see [`IniSection::read_bool`].
pub(crate) fn parse_read_bool(default: bool, raw: &str) -> bool {
    read_bool_decision(raw).unwrap_or(default)
}

/// ReadBool's verdict on the stored value's untrimmed first byte
/// (`0x0052976B`): `None` where it returns its default.
fn read_bool_decision(raw: &str) -> Option<bool> {
    match raw.bytes().next().map(|byte| byte.to_ascii_uppercase()) {
        Some(b'1') | Some(b'T') | Some(b'Y') => Some(true),
        Some(b'0') | Some(b'F') | Some(b'N') => Some(false),
        _ => None,
    }
}

/// The binary64 0.01 at `0x007E3808`: the percent scale of ReadDouble
/// (`0x0052857E`), ReadPowerups (`0x00673FAF`) and the Verses reader.
pub(crate) const PERCENT_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3f84_7ae1_47ae_147b);

pub(crate) fn parse_read_double(raw: &str) -> f64 {
    let value = strtrim_ascii(raw);
    let widened = f64::from(parse_leading_f32(value));
    if !value.as_bytes().contains(&b'%') {
        return widened;
    }
    // `fld qword; fmul qword [0x007E3808]; fstp qword` (`0x0052857A..0x00528584`).
    scale_percent(widened)
}

/// A double times [`PERCENT_SCALE`] under the game's control word 0x0E7F,
/// which Math__ftol (`0x007C5F00`) installs and never restores: the product is
/// chopped at 53 bits, one ulp below the nearest-rounded product for values
/// such as `70%` (0.7's own double) or `90%` (the double below 0.9).
pub(crate) fn scale_percent(value: f64) -> f64 {
    let scaled = MaskedX87Chop53::mul(
        MaskedX87Chop53::load_f64(NativeF64Bits::from_bits(value.to_bits())),
        MaskedX87Chop53::load_f64(PERCENT_SCALE),
    );
    f64::from_bits(MaskedX87Chop53::store_f64_masked_chop(scaled).bits())
}

/// A `strncpy` cut to `max_bytes` source bytes. The store widens each source
/// byte to one character (`util::native_string::widen_bytes`), so the cut
/// counts characters, not UTF-8 bytes; test text outside that byte domain has
/// no native identity and is cut the same way.
pub(crate) fn truncate_native_bytes(value: &str, max_bytes: usize) -> &str {
    value
        .char_indices()
        .nth(max_bytes)
        .map_or(value, |(end, _)| &value[..end])
}

/// `strtrim` @ `0x00727CF0`, over chars (one per native byte): drop the
/// leading chars <= 0x20, shift the text down, then clear trailing chars
/// <= 0x20 until one is kept or the char at the old start offset has been
/// cleared. A trailing run therefore reaches back no further than that
/// offset: `"  x  "` -> `"x "`, `"  x "` -> `"x "`, `" ab "` -> `"ab"`.
/// Executed controls: tools/rules_oracle/ini_token_readers.
pub(crate) fn strtrim_ascii(s: &str) -> &str {
    let leading = s.bytes().take_while(|&b| b <= STRTRIM_MAX).count();
    let text = &s[leading..];
    let trimmed = text.trim_end_matches(|c: char| u32::from(c) <= u32::from(STRTRIM_MAX));
    if leading == 0 {
        return trimmed;
    }
    let chars = text.chars().count();
    let kept = trimmed.chars().count();
    let end = match leading.cmp(&chars) {
        std::cmp::Ordering::Less => kept.max(leading),
        // The cleared char is the shifted text's terminator: nothing is trimmed.
        std::cmp::Ordering::Equal => chars,
        // The old offset lies past the shifted text and is never cleared.
        std::cmp::Ordering::Greater => kept,
    };
    truncate_native_bytes(text, end)
}

/// CRT `strtok` (`0x007C9CC2`) over one copied value: tokens are the maximal
/// runs of non-delimiter characters, so leading, repeated and trailing
/// delimiters produce no empty token, and no token is trimmed. Every native
/// comma list passes `","`; the sound registry passes `" \t\n"`.
pub(crate) fn strtok<'a>(value: &'a str, delimiters: &'a [char]) -> impl Iterator<Item = &'a str> {
    value.split(delimiters).filter(|token| !token.is_empty())
}

/// The C-locale `isspace` set CRT `atoi` and `sscanf` skip (ctype `_SPACE`):
/// tab, LF, VT, FF, CR and space.
fn is_crt_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

/// CRT `atoi` (`0x007C9B72`, reached through `0x007C9BFD`): skip C-locale
/// `isspace` bytes, take one optional sign, then decimal digits, stopping at
/// the first other byte; the running total wraps in 32 bits and a leading `-`
/// negates it. `5cells`->5, `abc`->0, ``->0, ` 7`->7, `-50`->-50, `+9`->9,
/// `0x1A`->0 (ReadInt's `$`/`h` hex branches are separate).
pub(crate) fn crt_atoi(s: &str) -> i32 {
    let bytes = s.as_bytes();
    let mut index = bytes
        .iter()
        .position(|byte| !is_crt_space(*byte))
        .unwrap_or(bytes.len());
    let negative = bytes.get(index) == Some(&b'-');
    if matches!(bytes.get(index), Some(b'-' | b'+')) {
        index += 1;
    }
    let mut total = 0_i32;
    while let Some(digit) = bytes.get(index).filter(|byte| byte.is_ascii_digit()) {
        total = total.wrapping_mul(10).wrapping_add(i32::from(digit - b'0'));
        index += 1;
    }
    if negative {
        total.wrapping_neg()
    } else {
        total
    }
}

/// One native CRT sscanf `%d` conversion, retaining the unconsumed input for
/// the caller's literal separators. Decimal arithmetic retains the low32 bits.
/// Original7CA530 consumers: Building4615CA and Infantry523DB0; executable
/// fixtures building_body_rules and infantry_sequence_rules preserve both.
pub(crate) fn scan_decimal_i32(bytes: &mut &[u8]) -> Option<i32> {
    while bytes.first().is_some_and(|b| is_crt_space(*b)) {
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

/// `sscanf(value, "%d,%d,...")` with one `%d` per slot of `out`: each comma is
/// a literal that must follow the previous number at once. Scanned fields
/// overwrite `out` in order until the first failure; returns how many did.
fn scan_int_fields(value: &str, out: &mut [i32]) -> usize {
    let mut bytes = value.as_bytes();
    let last = out.len().saturating_sub(1);
    for (index, slot) in out.iter_mut().enumerate() {
        let Some(scanned) = scan_decimal_i32(&mut bytes) else {
            return index;
        };
        *slot = scanned;
        if index < last {
            let Some(rest) = bytes.strip_prefix(b",") else {
                return index + 1;
            };
            bytes = rest;
        }
    }
    out.len()
}

/// sscanf "%f"-equivalent leading float (P7): optional sign, decimal mantissa,
/// and optional exponent. Parsing stops at the first byte outside that token
/// (`12.5%` -> 12.5, `1.25e2junk` -> 125). Empty/junk -> 0.0.
pub(crate) fn parse_leading_f32(s: &str) -> f32 {
    leading_float_token(s)
        .and_then(|token| token.parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// CRT `atof` (`0x007C9D66`), the Verses (`0x0075DE39`) and ReadPowerups
/// (`0x00673FAA`, `0x00673FC2`) route: leading `isspace` bytes skipped, then
/// the decimal/exponent prefix. Unlike ReadDouble's `%f` route, this retains
/// binary64. Executed finite, malformed and exponent forms are pinned in
/// bridge_landing_inputs.json; arbitrary extreme CRT rounding/range behavior
/// is not certified here.
pub(crate) fn parse_leading_f64(s: &str) -> f64 {
    let s = s.trim_start_matches(['\t', '\n', '\x0b', '\x0c', '\r', ' ']);
    leading_float_token(s)
        .and_then(|token| token.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn leading_float_token(s: &str) -> Option<&str> {
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
        return None;
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

    Some(&s[..end])
}

#[cfg(test)]
mod tests {
    use super::{crt_atoi, parse_leading_f32, strtok, truncate_native_bytes};
    use crate::rules::ini_parser::{IniFile, IniSection};

    fn sec(body: &str) -> IniFile {
        IniFile::from_str(body)
    }

    #[test]
    fn read_edge_matches_original_campaign_controls() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start_houses.json",
        ))
        .expect("executed original House/Edge controls");
        let rows = corpus["edge_controls"].as_array().unwrap();
        assert_eq!(rows.len(), 20);
        for row in rows {
            // Native supplies cached sections, including an empty section or
            // stored-empty value. A physical INI load discards both, so retain
            // the actual reader input through the parser's test owner.
            let mut section = IniSection::new("Control".to_string());
            for (key, value) in row["sections"]["Control"].as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            let ini = IniFile::from_sections_for_test([section]);
            let default = i32::try_from(row["default"].as_i64().unwrap()).unwrap();
            assert_eq!(
                ini.section("Control").unwrap().read_edge("Edge", default),
                i32::try_from(row["result"].as_i64().unwrap()).unwrap(),
                "{row}, default={default}",
            );
        }
    }

    #[test]
    fn read_color_scheme_matches_original_campaign_controls() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start_houses.json",
        ))
        .unwrap();
        let controls = &native["color_controls"];
        let names: Vec<_> = controls["registry"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|scheme| scheme["shade_count"] == 1)
            .map(|scheme| scheme["name"].as_str().unwrap())
            .collect();
        let rows = controls["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 22);
        for row in rows {
            let ini = if let Some(physical) = row["physical_ini"].as_str() {
                IniFile::from_str(physical)
            } else {
                IniFile::from_sections_for_test(row["sections"].as_object().unwrap().iter().map(
                    |(name, values)| {
                        let mut section = IniSection::new(name.clone());
                        for (key, value) in values.as_object().unwrap() {
                            section.set(key, value.as_str().unwrap());
                        }
                        section
                    },
                ))
            };
            let current = row["current"].as_i64().unwrap() as i32;
            assert_eq!(
                ini.section_or_empty("Control").read_color_scheme(
                    "Color",
                    current,
                    names.iter().copied(),
                ),
                row["result"].as_i64().unwrap() as i32,
                "original474A90 {row}",
            );
        }
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

    /// `strncpy` counts source bytes, and the store widens each byte to one
    /// character: a `0xE9` byte is one of the 3 bytes a capacity of 4 keeps.
    #[test]
    fn read_string_cuts_widened_bytes_not_utf8_bytes() {
        let ini = IniFile::from_bytes(b"[S]\nA=\xe9t\xe9s\n").unwrap();
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_string("A", "", 4), "\u{e9}t\u{e9}");
        assert_eq!(truncate_native_bytes("\u{e9}\u{e9}", 1), "\u{e9}");
    }

    /// ReadString returns 0 only for an absent key; `read_name` keeps that
    /// distinction so callers retain their current field.
    #[test]
    fn read_name_is_present_value_after_cut_and_trim() {
        let ini = sec("[S]\nA=  GAPOWR \nB=0123456789\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_name("A", 0x80), Some("GAPOWR"));
        assert_eq!(s.read_name("B", 5), Some("0123"));
        assert_eq!(s.read_name("MISSING", 0x80), None);
        assert_eq!(s.read_name("A", 0), None);
    }

    /// `strtok(",")`: empty fields collapse, tokens keep their spaces, the
    /// ReadString cut applies first, and a present comma-only value is an
    /// empty replacement list rather than an absent one.
    #[test]
    fn read_list_is_readstring_then_strtok() {
        let ini = sec("[S]\nL=,A, B ,,C,\nCommas=,,,\nLong=AB,CD,EF\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_list("L", 0x80), Some(vec!["A", " B ", "C"]));
        assert_eq!(s.read_list("Commas", 0x80), Some(vec![]));
        assert_eq!(s.read_list("Long", 5), Some(vec!["AB", "C"]));
        assert_eq!(s.read_list("MISSING", 0x80), None);
        assert_eq!(
            strtok(" a\tb\n\nc ", &[' ', '\t', '\n']).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn read_houses_list_matches_original_exact_lookup_and_default_controls() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start_houses.json",
        ))
        .unwrap();
        let controls = &native["allies_reader_controls"];
        assert_eq!(controls["delimiter_hex"], "2c00");
        let names: Vec<_> = controls["registry"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        let rows = controls["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 27);
        for row in rows {
            let mut ini = IniFile::empty();
            let section = ini.projection_section_mut("Control");
            for (key, value) in row["sections"]["Control"].as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            let actual = section.read_houses_list(
                "Allies",
                row["default"].as_u64().unwrap() as u32,
                |name| names.iter().position(|candidate| *candidate == name),
            );
            assert_eq!(actual, row["result"].as_u64().unwrap() as u32, "{row}");
        }
    }

    /// `DifficultyClass::ReadINI_IntVector`: CRT `atoi` per token, so spaces
    /// before a number and trailing text are both harmless.
    #[test]
    fn read_int_list_atois_each_strtok_token() {
        let ini = sec("[S]\nV=8, 8,\t6x,,-2\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_int_list("V"), Some(vec![8, 8, 6, -2]));
        assert_eq!(s.read_int_list("MISSING"), None);

        let ini = sec("[S]\nWrap=1,,  -2junk,,+3,abc,4294967297,-2147483649\n");
        let s = ini.section("S").unwrap();
        assert_eq!(
            s.read_int_list("Wrap"),
            Some(vec![1, -2, 3, 0, 1, i32::MAX])
        );

        // Bytes beyond the char[512] payload cannot add another entry.
        let ini = sec(&format!("[S]\nBig=1,{},7\n", "0".repeat(509)));
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_int_list("Big"), Some(vec![1, 0]));
    }

    /// `0x00476B20`: stock `ColorList=(0,128,255),(255,255,255)` groups by
    /// three tokens, dropping the first token's leading byte and the third's
    /// trailing byte; an incomplete group adds nothing.
    #[test]
    fn read_color_list_groups_three_tokens() {
        let ini =
            sec("[S]\nStock=(0,128,255),(255,255,255)\nShort=(1,2,3),(4,5\nWrap=(300,-1,256)\n");
        let s = ini.section("S").unwrap();
        assert_eq!(
            s.read_color_list("Stock"),
            Some(vec![[0, 128, 255], [255, 255, 255]])
        );
        assert_eq!(s.read_color_list("Short"), Some(vec![[1, 2, 3]]));
        assert_eq!(s.read_color_list("Wrap"), Some(vec![[44, 255, 0]]));
        assert_eq!(s.read_color_list("MISSING"), None);
    }

    /// `0x00476340` reads three `atof` tokens.
    #[test]
    fn read_float_tokens_atofs_three_tokens() {
        let ini = sec("[S]\nV=0.5, -1,2e1\nShort=3\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_float_tokens("V", [9.0; 3]), [0.5, -1.0, 20.0]);
        assert_eq!(s.read_float_tokens("Short", [9.0; 3]), [3.0, 0.0, 0.0]);
        assert_eq!(s.read_float_tokens("MISSING", [9.0; 3]), [9.0; 3]);
    }

    /// `0x00476420` tokenizes where `read_coord3` scans: stock
    /// `DamageSmokeOffset=100, 100, 275` reads whole, and a space before a
    /// comma does not stop it.
    #[test]
    fn read_coord_tokens_reads_three_atoi_tokens() {
        let ini = sec("[S]\nA=100, 100, 275\nB=-92 ,208 ,312\nShort=5,6\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_coord_tokens("A", [1, 2, 3]), [100, 100, 275]);
        assert_eq!(s.read_coord_tokens("B", [1, 2, 3]), [-92, 208, 312]);
        assert_eq!(s.read_coord_tokens("Short", [1, 2, 3]), [5, 6, 0]);
        assert_eq!(s.read_coord_tokens("MISSING", [1, 2, 3]), [1, 2, 3]);
    }

    #[test]
    fn test_crt_atoi_and_leading_f32_helpers() {
        assert_eq!(crt_atoi("5cells"), 5);
        assert_eq!(crt_atoi("-50"), -50);
        assert_eq!(crt_atoi("+9"), 9);
        assert_eq!(crt_atoi(""), 0);
        assert_eq!(crt_atoi(" \t\x0b\x0c\r\n7"), 7);
        assert_eq!(crt_atoi("\x01 7"), 0, "only C isspace bytes are skipped");
        assert_eq!(crt_atoi("- 7"), 0);
        assert_eq!(crt_atoi("4294967297"), 1, "the total wraps in 32 bits");
        assert!((parse_leading_f32("12.5%") - 12.5).abs() < 1e-6);
        assert!((parse_leading_f32(".9") - 0.9).abs() < 1e-6);
        assert!((parse_leading_f32("1.25e2junk") - 125.0).abs() < 1e-6);
        assert!((parse_leading_f32("-2.5E-1%") + 0.25).abs() < 1e-6);
        assert!((parse_leading_f32("1e") - 1.0).abs() < 1e-6);
    }

    /// `ReadMinMax` scans `"%d,%d"`: the comma must follow the first number,
    /// the second number may follow blanks, and an incomplete scan keeps the
    /// default where native would store its stale argument slots.
    #[test]
    fn read_minmax_scans_two_decimal_fields() {
        let ini = sec("[S]\nP=3,5\nQ=3, 5junk\nSpace=3 ,5\nOne=3\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_minmax("P", [0, 0]), [3, 5]);
        assert_eq!(s.read_minmax("Q", [0, 0]), [3, 5]);
        assert_eq!(s.read_minmax("Space", [9, 9]), [9, 9]);
        assert_eq!(s.read_minmax("One", [9, 9]), [9, 9]);
        assert_eq!(s.read_minmax("MISSING", [9, 9]), [9, 9]);
    }

    /// `INIClass::ReadRect`: a missing key scans the literal `"0,0,0,0"`; a
    /// present value overlays its valid prefix on the default.
    #[test]
    fn read_rect_overlays_the_scanned_prefix() {
        let ini = sec("[S]\nR=1,2,3,4\nShort=2,3,40\nBad=2,invalid,40,41\nSpace=2 ,3,40,41\n");
        let s = ini.section("S").unwrap();
        let default = [1, 1, 50, 50];
        assert_eq!(s.read_rect("R", default), [1, 2, 3, 4]);
        assert_eq!(s.read_rect("Short", default), [2, 3, 40, 50]);
        assert_eq!(s.read_rect("Bad", default), [2, 1, 50, 50]);
        assert_eq!(s.read_rect("Space", default), [2, 1, 50, 50]);
        assert_eq!(s.read_rect("MISSING", default), [0, 0, 0, 0]);
    }

    /// `0x00529CA0`: sscanf `"%d,%d,%d"` after the 63-byte cut and strtrim.
    #[test]
    fn read_coord3_scans_like_the_native_coordinate_read() {
        let ini = IniFile::from_str(
            "[S]\nFull=45,-190,90;gun port\nPad= 80, 0, 120 \nSpaced=80 ,0,120\n\
             Pair=100,-25\nJunk=abc,1,2\nPlus=+7,-0,3x\nBlank=\n",
        );
        let section = ini.section("S").unwrap();
        let default = [1, 2, 3];
        assert_eq!(section.read_coord3("Full", default), [45, -190, 90]);
        // `%d` skips the blanks before a number, never before the comma.
        assert_eq!(section.read_coord3("Pad", default), [80, 0, 120]);
        assert_eq!(section.read_coord3("Plus", default), [7, 0, 3]);
        // Incomplete scans: native leaves stack words, VERA the default.
        assert_eq!(section.read_coord3("Spaced", default), default);
        assert_eq!(section.read_coord3("Pair", default), default);
        assert_eq!(section.read_coord3("Junk", default), default);
        assert_eq!(section.read_coord3_value("Junk"), None);
        assert_eq!(section.read_coord3("Blank", default), default);
        assert_eq!(section.read_coord3("Absent", default), default);
    }

    #[test] // P21
    fn test_read_color_rgb() {
        let ini = sec("[S]\nC=12,34,56\nWide=300,-1,256\nShort=1,2\n");
        let s = ini.section("S").unwrap();
        assert_eq!(s.read_color_rgb("C", [0, 0, 0]), [12, 34, 56]);
        assert_eq!(s.read_color_rgb("Wide", [0, 0, 0]), [44, 255, 0]);
        assert_eq!(s.read_color_rgb("Short", [7, 8, 9]), [7, 8, 9]);
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

    /// A supplied existing native cache entry differs from a physical empty
    /// INI line, which both lexical loaders omit. Replay the scalar control.
    #[test]
    fn native_base_response_type_speed_reads_an_empty_supplied_cache_entry() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/base_defense_response.json",
        ))
        .unwrap();
        let row = native["speed_history"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["raw"] == "")
            .unwrap();
        let mut section = IniSection::new("MTNK".to_owned());
        section.entries.insert("Speed".to_owned(), String::new());
        assert_eq!(
            crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
                section.read_techno_speed("Speed", 7),
            ),
            row["output"]["types"]["MTNK"]["speed"].as_i64().unwrap() as i32,
        );
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

    /// The scratch seed is the section name's CRCEngine hash; these are the
    /// two values the native reader was executed with.
    #[test]
    fn read_comma_hex_utf16_seeds_with_the_section_crc() {
        let name_crc = |name: &str| crate::assets::mix_hash::crc_engine(name.as_bytes());
        assert_eq!(name_crc("Network"), 0x70CA_A741);
        assert_eq!(name_crc("RandomMap"), 0x1597_B573);
        let ini = sec("[Network]\nNetID=zz,41,,42\n");
        let s = ini.section("Network").unwrap();
        assert_eq!(
            s.read_comma_hex_utf16("NetID", &[], 8),
            [0xA741, 0x41, 0x42]
        );
        assert_eq!(s.read_comma_hex_utf16("MISSING", &[1, 2, 0, 3], 8), [1, 2]);
    }
}
