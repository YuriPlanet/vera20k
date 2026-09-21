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

/// Number of shades per house color band (matches palette indices 16–31).
const RAMP_SIZE: usize = 16;

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
/// Shade 0 is the brightest (the radar/UI/target-line color).
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
#[derive(Debug, Default)]
pub struct HouseColorRamps {
    ramps: Vec<[Color; RAMP_SIZE]>,
}

impl HouseColorRamps {
    /// Build one exact sampled-factor ramp per `[Colors]` scheme, in declaration order.
    pub fn from_schemes(schemes: &[ColorSchemeEntry]) -> Self {
        Self {
            ramps: schemes.iter().map(|s| build_scheme_ramp(s.hsv)).collect(),
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
    fn build_scheme_ramp_16_opaque_brightest_first() {
        let r = build_scheme_ramp([153, 214, 212]); // DarkBlue HSV
        assert_eq!(r.len(), 16);
        assert!(r.iter().all(|c| c.a == 255));
        let lum = |c: &Color| c.r as u32 + c.g as u32 + c.b as u32;
        assert!(
            lum(&r[0]) > lum(&r[15]),
            "shade 0 must be brighter than shade 15"
        );
        assert!(
            r[0].b >= r[0].r && r[0].b >= r[0].g,
            "blue hue preserved: {:?}",
            r[0]
        );
    }

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
        #[rustfmt::skip]
        let cases: &[(&str, [u8; 3], [[u8; 2]; RAMP_SIZE])] = &[
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
        #[rustfmt::skip]
        let cases: &[(&str, [u8; 3], [[u8; 3]; 16])] = &[
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
}
