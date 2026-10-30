//! House color definitions — runtime per-`[Colors]`-entry team-color ramps.
//!
//! RA2 reserves palette indices 16–31 for "house colors" — 16 shades swapped per
//! player to distinguish units visually (Allied blue, Soviet red, etc.). Each
//! band runs from brightest (index 0) to darkest (index 15); when rendering a
//! unit the base palette's indices 16–31 are replaced with the owning player's
//! band before pixel conversion.
//!
//! Bands are built at load from the rules `[Colors]` H,S,V list via gamemd's
//! fixed-hue trig Saturation/Value sweep ([`build_scheme_ramp`]) and held in a
//! runtime [`HouseColorRamps`] table on the `RuleSet`. A `HouseColorIndex` is a
//! `[Colors]` entry index into that table.
//!
//! ## Dependency rules
//! - Part of rules/ — depends on assets/pal_file (Color) + rules/color_scheme.

use crate::assets::pal_file::Color;
use crate::rules::color_scheme::{ColorSchemeEntry, hsv_to_rgb};

/// A `[Colors]` entry index — selects a player's team-color band.
///
/// Stored as u8 for cheap hashing in atlas keys (HashMap lookups every frame) and
/// reused as the GPU ramp-texture row key (`row = index + 1`). Default (0) is the
/// first `[Colors]` entry.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct HouseColorIndex(pub u8);

/// Sentinel value meaning "do not apply house color remap — use raw palette."
/// Used for Neutral, Special, Civilian buildings that have no player color.
pub const NO_REMAP: HouseColorIndex = HouseColorIndex(255);

/// The colour gamemd prints a house's text in: `HSV_To_RGB` of its colour
/// scheme's base HSV (scheme `+0x308`, its `[Colors]` entry), as
/// Fancy_Text_Print_Wide (`0x004A61C0`) does for the timer lines and the
/// score dialog for its rows (`0x005C9E0C`). White for an entry `[Colors]`
/// lacks.
pub fn house_text_rgb(schemes: &[ColorSchemeEntry], color: HouseColorIndex) -> [u8; 3] {
    crate::rules::color_scheme::scheme_hsv_by_entry(schemes, usize::from(color.0))
        .map_or([0xFF; 3], hsv_to_rgb)
}

/// Number of shades per house color band (matches palette indices 16–31).
const RAMP_SIZE: usize = 16;

/// VERA's house-colour strength range. gamemd has no such control; these are
/// presentation only and never reach simulation state. [`STRENGTH_RETAIL`]
/// reproduces every retail ramp byte for byte, and the top of the range is
/// the strongest boost.
pub const STRENGTH_MIN: u8 = 1;
pub const STRENGTH_MAX: u8 = 10;
/// The strength a table starts at: gamemd's own ramps.
pub const STRENGTH_RETAIL: u8 = STRENGTH_MIN;

/// Exact active-YR `f32` samples selected for the 16 saturation shades.
#[rustfmt::skip]
const SATURATION_FACTOR_BITS: [u32; RAMP_SIZE] = [
    0x3F4422AA, 0x3F4B7F08, 0x3F5289B4, 0x3F591E6A,
    0x3F5F397A, 0x3F64C0EE, 0x3F69E0D7, 0x3F6E7DB7,
    0x3F7284E4, 0x3F76167A, 0x3F791E30, 0x3F7B9107,
    0x3F7D8284, 0x3F7EE5F9, 0x3F7FB848, 0x3F800000,
];

/// Exact active-YR `f32` samples selected for the 16 value shades.
#[rustfmt::skip]
const VALUE_FACTOR_BITS: [u32; RAMP_SIZE] = [
    0x3F7B14BE, 0x3F68AA48, 0x3F5F397A, 0x3F54330F,
    0x3F47DE65, 0x3F3A37B6, 0x3F2B561A, 0x3F1B52BB,
    0x3F0A1E5D, 0x3EF050C4, 0x3ECACE62, 0x3EA3F505,
    0x3E780CBD, 0x3E25C58B, 0x3DA6522F, 0x250D3000,
];

/// Promote the sampled `f32` exactly to `f64`, multiply by the source byte,
/// then truncate toward zero like the active constructor.
fn scale_ramp_byte(source: u8, factor_bits: u32) -> u8 {
    let factor = f64::from(f32::from_bits(factor_bits));
    (factor * f64::from(source)).trunc() as u8
}

fn modulated_sv(hsv: [u8; 3], shade: usize) -> [u8; 2] {
    [
        scale_ramp_byte(hsv[1], SATURATION_FACTOR_BITS[shade]),
        scale_ramp_byte(hsv[2], VALUE_FACTOR_BITS[shade]),
    ]
}

/// Active-YR per-scheme 16-shade team band (palette indices 16..31): fixed hue
/// H with saturation and value scaled by the exact sampled factors. Each
/// `(modS, modV)` pair goes through the 6-sextant integer HSV→RGB conversion.
/// Shade 0 is the brightest palette input. House radar/rally RGB additionally
/// passes through LightConvert's middle row; see render::palette_light.
pub fn build_scheme_ramp(hsv: [u8; 3]) -> [Color; RAMP_SIZE] {
    let h = hsv[0];
    let mut ramp = [Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    }; RAMP_SIZE];
    for (i, slot) in ramp.iter_mut().enumerate() {
        let [mod_s, mod_v] = modulated_sv(hsv, i);
        let [r, g, b] = hsv_to_rgb([h, mod_s, mod_v]);
        *slot = Color { r, g, b, a: 255 };
    }
    ramp
}

/// The fractional strength: 0 at [`STRENGTH_MIN`], 1 at [`STRENGTH_MAX`].
fn strength_fraction(strength: u8) -> f64 {
    let level = strength.clamp(STRENGTH_MIN, STRENGTH_MAX);
    f64::from(level - STRENGTH_MIN) / f64::from(STRENGTH_MAX - STRENGTH_MIN)
}

/// A profile's `[VERA] HouseColorStrength=` integer lifted into the supported
/// range, so a hand-edited file cannot ask for a boost outside it.
pub fn clamped_strength(value: i32) -> u8 {
    value.clamp(i32::from(STRENGTH_MIN), i32::from(STRENGTH_MAX)) as u8
}

/// How far a shade's own saturation scales up at full strength.
const SATURATION_GAIN: f64 = 0.90;
/// How far a shade's own value rises toward white at full strength, as the
/// gamma exponent `1 / (1 + VALUE_GAIN)`.
const VALUE_GAIN: f64 = 0.35;

/// One shade's saturation and value after a boost, in the same units
/// [`modulated_sv`] returns.
///
/// Both scale from the shade's own figures rather than toward a fixed target,
/// for reasons the band itself imposes. Saturation scales, so a saturated band
/// reaches its pure hue while a near-grey scheme keeps the faint tint it had
/// instead of being dragged to a hue it never had. Value follows a gamma curve
/// toward white, so the mid and dark shades rise toward the bright end and the
/// band reads bolder at battle zoom. The two cheaper alternatives both fail
/// here: a symmetric stretch around mid grey only darkens the rest, because the
/// bright end already sits at 255 and clamps, and a plain multiplication clamps
/// the top few shades into one flat block, losing the highlight gradient. The
/// gamma curve never clamps, so every shade keeps its own step. The hue is
/// never touched: a boosted team colour is the same colour, only bolder.
fn boosted_sv([saturation, value]: [u8; 2], fraction: f64) -> [u8; 2] {
    if fraction <= 0.0 {
        return [saturation, value];
    }
    let saturation = f64::from(saturation) * (1.0 + SATURATION_GAIN * fraction);
    let ratio = f64::from(value) / 255.0;
    let value = 255.0 * ratio.powf(1.0 / (1.0 + VALUE_GAIN * fraction));
    [round_byte(saturation), round_byte(value)]
}

fn round_byte(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// The same band as [`build_scheme_ramp`] at `strength`, which is the retail
/// band itself at [`STRENGTH_RETAIL`].
pub fn boosted_scheme_ramp(hsv: [u8; 3], strength: u8) -> [Color; RAMP_SIZE] {
    let fraction = strength_fraction(strength);
    if fraction <= 0.0 {
        return build_scheme_ramp(hsv);
    }
    let hue = hsv[0];
    let mut ramp = [Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    }; RAMP_SIZE];
    for (shade, slot) in ramp.iter_mut().enumerate() {
        let [saturation, value] = boosted_sv(modulated_sv(hsv, shade), fraction);
        let [r, g, b] = hsv_to_rgb([hue, saturation, value]);
        *slot = Color { r, g, b, a: 255 };
    }
    ramp
}

/// `[Colors]` entry index used when a house has no resolvable color. gamemd `InitColor` forces a
/// negative ColorSchemeIndex to 5 (runtime scheme 5 → `[Colors]` entry 2 = LightGrey / white-ish).
pub const DEFAULT_SCHEME_ENTRY: usize = 2;

/// Flat fallback ramp used only when the `[Colors]` list is empty (rules not yet loaded); a real
/// skirmish always has a populated scheme list.
static FALLBACK_RAMP: [Color; RAMP_SIZE] = [Color {
    r: 180,
    g: 180,
    b: 180,
    a: 255,
}; RAMP_SIZE];

/// Runtime per-`[Colors]`-entry house-color ramp table. Index = `[Colors]` entry index =
/// `HouseColorIndex.0`. Built once at load from the parsed `[Colors]` schemes and held on the
/// `RuleSet`. `Default` (empty) is used only when rules are unavailable (headless tests, missing
/// assets); `ramp()` then yields the flat fallback.
#[derive(Debug)]
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
pub struct HouseColorRamps {
    /// Each `[Colors]` entry's HSV, kept so a strength change rebuilds gamemd's
    /// sweep from the retail source instead of boosting an already boosted band.
    schemes: Vec<[u8; 3]>,
    /// The bands at [`Self::strength`].
    ramps: Vec<[Color; RAMP_SIZE]>,
    /// VERA's presentation strength, [`STRENGTH_RETAIL`] until set.
    strength: u8,
}

impl Default for HouseColorRamps {
    fn default() -> Self {
        Self {
            schemes: Vec::new(),
            ramps: Vec::new(),
            strength: STRENGTH_RETAIL,
        }
    }
}

impl HouseColorRamps {
    /// Build one exact sampled-factor ramp per `[Colors]` scheme, in declaration order.
    pub fn from_schemes(schemes: &[ColorSchemeEntry]) -> Self {
        let strength = STRENGTH_RETAIL;
        Self {
            schemes: schemes.iter().map(|s| s.hsv).collect(),
            ramps: schemes
                .iter()
                .map(|s| boosted_scheme_ramp(s.hsv, strength))
                .collect(),
            strength,
        }
    }

    /// The current presentation strength, [`STRENGTH_MIN`]..=[`STRENGTH_MAX`].
    pub fn strength(&self) -> u8 {
        self.strength
    }

    /// Rebuild every band at `strength`, clamped into the supported range.
    /// [`STRENGTH_RETAIL`] restores the exact retail bytes.
    pub fn set_strength(&mut self, strength: u8) {
        let strength = strength.clamp(STRENGTH_MIN, STRENGTH_MAX);
        if strength == self.strength {
            return;
        }
        self.strength = strength;
        for (slot, hsv) in self.ramps.iter_mut().zip(&self.schemes) {
            *slot = boosted_scheme_ramp(*hsv, strength);
        }
    }

    /// Ramp for a house color index. `NO_REMAP` or an out-of-range index falls back to the default
    /// scheme (or a flat ramp if the scheme list is empty).
    pub fn ramp(&self, index: HouseColorIndex) -> &[Color; RAMP_SIZE] {
        if index != NO_REMAP
            && let Some(r) = self.ramps.get(index.0 as usize)
        {
            return r;
        }
        self.ramps
            .get(DEFAULT_SCHEME_ENTRY)
            .unwrap_or(&FALLBACK_RAMP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn house_color_ramps_indexes_by_entry_and_falls_back() {
        let mk = |name: &str, hsv: [u8; 3]| ColorSchemeEntry {
            name: name.into(),
            hsv,
        };
        let schemes = vec![
            mk("A", [0, 230, 255]),
            mk("B", [153, 214, 212]),
            mk("C", [0, 0, 240]),
        ];
        let table = HouseColorRamps::from_schemes(&schemes);
        assert_eq!(
            table.ramp(HouseColorIndex(1)),
            &build_scheme_ramp([153, 214, 212])
        );
        // NO_REMAP and out-of-range fall back to DEFAULT_SCHEME_ENTRY (2).
        assert_eq!(table.ramp(NO_REMAP), table.ramp(HouseColorIndex(2)));
        assert_eq!(
            table.ramp(HouseColorIndex(99)),
            table.ramp(HouseColorIndex(2))
        );
    }

    #[test]
    fn gsi_02_13_build_scheme_ramp_matches_native_intermediate_modulation() {
        type Case = (&'static str, [u8; 3], [[u8; 2]; RAMP_SIZE]);
        #[rustfmt::skip]
        let cases: &[Case] = &[
            ("Gold", [43, 239, 255], [
                [183,250],[189,231],[196,222],[202,211],
                [208,199],[213,185],[218,170],[222,154],
                [226,137],[229,119],[232,101],[234,81],
                [236,61],[237,41],[238,20],[239,0],
            ]),
            ("DarkBlue", [153, 214, 212], [
                [163,207],[170,192],[175,184],[181,175],
                [186,165],[191,154],[195,141],[199,128],
                [202,114],[205,99],[208,83],[210,67],
                [211,51],[213,34],[213,17],[214,0],
            ]),
        ];
        for (name, hsv, expected) in cases {
            for (shade, expected_pair) in expected.iter().enumerate() {
                assert_eq!(
                    modulated_sv(*hsv, shade),
                    *expected_pair,
                    "{name} shade {shade}"
                );
            }
        }
    }

    /// Exact active-YR per-shade RGB oracles for representative stock schemes.
    #[test]
    fn gsi_02_13_build_scheme_ramp_matches_golden_stock_values() {
        type Case = (&'static str, [u8; 3], [[u8; 3]; 16]);
        #[rustfmt::skip]
        let cases: &[Case] = &[
            ("Gold", [43, 239, 255], [
                [248,250,70],[229,231,59],[220,222,51],[209,211,43],
                [197,199,36],[183,185,30],[168,170,24],[152,154,19],
                [135,137,15],[118,119,12],[100,101,9],[80,81,6],
                [60,61,4],[40,41,2],[19,20,1],[0,0,0],
            ]),
            ("DarkRed", [0, 230, 255], [
                [250,77,77],[231,66,66],[222,57,57],[211,49,49],
                [199,42,42],[185,36,36],[170,30,30],[154,24,24],
                [137,20,20],[119,15,15],[101,12,12],[81,9,9],
                [61,6,6],[41,4,4],[20,2,2],[0,0,0],
            ]),
            ("DarkBlue", [153, 214, 212], [
                [74,128,207],[64,115,192],[57,108,184],[50,100,175],
                [44,93,165],[38,85,154],[33,76,141],[28,68,128],
                [23,59,114],[19,51,99],[15,42,83],[11,33,67],
                [8,25,51],[5,17,34],[2,8,17],[0,0,0],
            ]),
        ];
        for (name, hsv, expected) in cases {
            let r = build_scheme_ramp(*hsv);
            for (i, exp) in expected.iter().enumerate() {
                assert_eq!([r[i].r, r[i].g, r[i].b], *exp, "{name} shade {i}");
            }
        }
    }

    fn ramp_luma(ramp: &[Color; RAMP_SIZE], shade: usize) -> f64 {
        0.299 * f64::from(ramp[shade].r)
            + 0.587 * f64::from(ramp[shade].g)
            + 0.114 * f64::from(ramp[shade].b)
    }

    fn scheme(name: &str, hsv: [u8; 3]) -> ColorSchemeEntry {
        ColorSchemeEntry {
            name: name.into(),
            hsv,
        }
    }

    /// The retail strength is gamemd's band byte for byte, and returning to it
    /// after a boost rebuilds the same bytes instead of boosting twice.
    #[test]
    fn retail_strength_reproduces_every_retail_shade() {
        let mut table = HouseColorRamps::from_schemes(&[scheme("DarkRed", [0, 230, 255])]);
        let retail = build_scheme_ramp([0, 230, 255]);
        assert_eq!(table.strength(), STRENGTH_RETAIL);
        assert_eq!(table.ramp(HouseColorIndex(0)), &retail);

        table.set_strength(STRENGTH_MAX);
        assert_ne!(table.ramp(HouseColorIndex(0)), &retail);

        table.set_strength(STRENGTH_RETAIL);
        assert_eq!(table.ramp(HouseColorIndex(0)), &retail);
    }

    /// The shade factors themselves: both rise from the shade's own figures,
    /// and a near-grey shade keeps its faint tint instead of being dragged to
    /// a hue it never had.
    #[test]
    fn boosted_sv_saturates_and_lifts_value() {
        assert_eq!(boosted_sv([180, 140], 0.0), [180, 140]);
        let [dark_saturation, dark_value] = boosted_sv([180, 100], 1.0);
        assert!(dark_saturation > 180, "a dark shade grows more saturated");
        assert!(dark_value > 100, "a dark shade rises toward the bright end");
        let [light_saturation, light_value] = boosted_sv([180, 200], 1.0);
        assert!(light_saturation > 180);
        assert!(light_value > 200, "a light shade grows brighter still");
        // A near-grey scheme stays near grey: the boost is proportional to the
        // tint it already carries, so no house is handed a hue it never had.
        let [grey_saturation, _] = boosted_sv([24, 200], 1.0);
        assert!(
            grey_saturation < 60,
            "a faint tint stays faint, not {}",
            grey_saturation
        );
    }

    /// A boosted band keeps its 16-step falloff: no shade crosses its brighter
    /// neighbour, the bright end keeps its peak, and the darkest shade stays
    /// dark instead of lifting to a mid grey.
    #[test]
    fn strength_keeps_the_band_falloff() {
        let retail = build_scheme_ramp([0, 230, 255]);
        let boosted = boosted_scheme_ramp([0, 230, 255], STRENGTH_MAX);
        for shade in 1..RAMP_SIZE {
            assert!(
                ramp_luma(&boosted, shade) <= ramp_luma(&boosted, shade - 1),
                "shade {shade} stays at or below its brighter neighbour"
            );
        }
        assert!(
            boosted[0].r >= retail[0].r,
            "the bright end keeps its strongest channel"
        );
        assert!(
            ramp_luma(&boosted, RAMP_SIZE - 1) < 0.1 * ramp_luma(&boosted, 0),
            "the darkest shade stays a shadow"
        );
    }

    /// The hue never moves, so a red band reads as red at every strength.
    #[test]
    fn strength_never_moves_the_hue() {
        let boosted = boosted_scheme_ramp([0, 230, 255], STRENGTH_MAX);
        for (shade, color) in boosted.iter().enumerate().take(RAMP_SIZE - 1) {
            assert!(
                color.r >= color.g && color.g >= color.b,
                "shade {shade} keeps the red band's channel order"
            );
        }
    }

    #[test]
    fn strength_is_clamped_to_the_supported_range() {
        let mut table = HouseColorRamps::from_schemes(&[scheme("DarkRed", [0, 230, 255])]);
        table.set_strength(0);
        assert_eq!(table.strength(), STRENGTH_MIN);
        table.set_strength(u8::MAX);
        assert_eq!(table.strength(), STRENGTH_MAX);
        assert_eq!(
            table.ramp(HouseColorIndex(0)),
            &boosted_scheme_ramp([0, 230, 255], STRENGTH_MAX)
        );
    }
}
