//! Native LightConvert color resolution for ordinary indexed draws.
//!
//! `0x00556090 -> 0x007DE200` constructs RGB565 rows; `0x00420140`
//! selects them from brightness and A-buffer. The captured retail process uses
//! MMX, RGB565 and x87 CW 0x0e7f. See
//! `docs/research/LIGHTCONVERT_ROW_RGB565_ORACLE_2026_09_09.md`.
//! Palette ownership (cell versus ColorScheme), scalar brightness and the
//! per-index mask remain separate. Zero/default means precomposed RGBA/UI.

use crate::map::lighting::CellLightGrid;

/// House50B840/500DF7 initializes +56F9 from ColorScheme+330 (index16),
/// Convert+174 (the middle N53 row), then unpacks the RGB565 word. Raw house
/// ramps remain palette inputs; this is the shared resolved house RGB reader.
/// The separate normalized laser color at +56FC is not this value.
/// Native execution: tools/procedural_drawing_oracle/house_color.
pub(crate) fn house_color_rgb(
    ramps: &crate::rules::house_colors::HouseColorRamps,
    index: crate::rules::house_colors::HouseColorIndex,
) -> [u8; 3] {
    let color = ramps.ramp(index)[0];
    let word =
        PaletteLight::color_scheme([1000; 3], 1000).rgb565([color.r, color.g, color.b], 16, 127);
    crate::render::native_surface_format::RGB565.unpack_rgb8(word)
}

/// Player/AI House +56FC..56FE, produced by ComputeRemap50BA00 after
/// InitColor50B840 in Create_Houses687F10. Campaign500ECC..501086 produces
/// the same bytes. This is separate from palette remapping and radar RGB.
/// Native execution covers all 65,536 resolved RGB565 inputs and all 21
/// physical Colors entries: tools/procedural_drawing_oracle/house_color.
pub(crate) fn house_laser_rgb(
    ramps: &crate::rules::house_colors::HouseColorRamps,
    index: crate::rules::house_colors::HouseColorIndex,
) -> [u8; 3] {
    normalized_house_laser_rgb(house_color_rgb(ramps, index))
}

fn normalized_house_laser_rgb(rgb: [u8; 3]) -> [u8; 3] {
    use crate::util::native_x87::{NativeF64Bits, X87Chop53, sqrt_approx_f32};

    // Presentation-only binary64 arithmetic; the shared native Sqrt_Approx
    // owner supplies its chopped f32 input and mantissa table. Ordinary f64
    // operations match the resulting bytes for the complete RGB565 domain,
    // without introducing another x87 arithmetic implementation.
    let normalize = |[r, g, b]: [f64; 3]| {
        let squared = b * b + g * g + r * r;
        let squared = X87Chop53::load_f64(NativeF64Bits::from_bits(squared.to_bits()))
            .expect("house color squares are finite");
        let root = f64::from(f32::from_bits(
            sqrt_approx_f32(squared)
                .expect("house color magnitude is finite")
                .bits(),
        ));
        if root == 0.0 {
            ([255.0; 3], false)
        } else {
            (
                [r, g, b].map(|channel| (channel * 240.0 / root).min(255.0)),
                true,
            )
        }
    };
    let (mut channels, nonzero) = normalize(rgb.map(f64::from));
    if nonzero {
        channels = channels.map(|channel| if channel < 96.0 { 0.0 } else { channel });
    }
    normalize(channels).0.map(|channel| channel as u8)
}

/// RGB scale uses bits 0..17; red's high byte stores N, green bit 30 the
/// plain-Convert flag and bit 31 the ColorScheme mask. Bits 18.. of each
/// channel word carry that channel's part of the colour word
/// ([`Self::with_colour_word`]). The fourth word retains the signed brightness
/// argument.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PaletteLight(pub [u32; 4]);

impl PaletteLight {
    pub fn new(rgb_milli: [i32; 3], rows: u32, brightness: i32, color_scheme: bool) -> Self {
        assert!(matches!(rows, 1 | 27 | 53));
        let rgb = rgb_milli.map(native_scale16);
        Self([
            rgb[0] | (rows << 24),
            rgb[1] | (u32::from(color_scheme) << 31),
            rgb[2],
            brightness as u32,
        ])
    }

    /// CellClass owns its normalized hue-profile and row count (0x00544E70).
    pub fn cell(grid: &CellLightGrid, cell: (u16, u16), top: bool) -> Self {
        let (rgb, brightness) = grid.cell_light_at(cell).map_or(([1000; 3], 1000), |light| {
            (
                light.rgb_key,
                if top {
                    light.top_scalar
                } else {
                    light.common_scalar
                },
            )
        });
        Self::new(
            grid.palette_rgb(rgb),
            if rgb.iter().sum::<i32>() < 2000 {
                27
            } else {
                53
            },
            brightness,
            false,
        )
    }

    /// The selected house Convert uses 53 rows even on a cell with 27 rows
    /// (0x0066D3A0, 0x00705D70, selector caller 0x0070720E).
    pub fn color_scheme(rgb_milli: [i32; 3], brightness: i32) -> Self {
        Self::new(rgb_milli, 53, brightness, true)
    }

    /// Ordinary ConvertClass (0048E740 -> 004BBB00) uses exact unity and
    /// scalar channel multiplication, independent of MMX availability.
    pub fn plain(rows: u32, brightness: i32) -> Self {
        assert!(matches!(rows, 1 | 53));
        Self([
            65536 | (rows << 24),
            65536 | (1 << 30),
            65536,
            brightness as u32,
        ])
    }

    pub fn with_brightness(mut self, brightness: i32) -> Self {
        self.0[3] = brightness as u32;
        self
    }

    /// The colour word the blit ORs into each pixel it draws, after the
    /// conversion (`0x00494CD9`, `0x00498293`, `0x004993C2`): red's five
    /// bits ride bits 18..23 of the red word, green's six bits 18..24 of the
    /// green word and blue's five bits 18..23 of the blue word.
    pub fn with_colour_word(mut self, word: u16) -> Self {
        let word = u32::from(word);
        self.0[0] = self.0[0] & !(0x1F << 18) | (word >> 11 & 0x1F) << 18;
        self.0[1] = self.0[1] & !(0x3F << 18) | (word >> 5 & 0x3F) << 18;
        self.0[2] = self.0[2] & !(0x1F << 18) | (word & 0x1F) << 18;
        self
    }

    pub fn colour_word(self) -> u16 {
        ((self.0[0] >> 18 & 0x1F) << 11 | (self.0[1] >> 18 & 0x3F) << 5 | (self.0[2] >> 18 & 0x1F))
            as u16
    }

    pub fn rows(self) -> u32 {
        self.0[0] >> 24
    }
    pub fn brightness(self) -> i32 {
        self.0[3] as i32
    }

    pub fn rgb565(self, rgb: [u8; 3], source_index: u8, a: u8) -> u16 {
        let n = self.rows();
        assert!(n != 0, "precomposed RGBA has no native palette");
        if source_index == 0 && self.0[1] & (1 << 30) == 0 {
            return 0;
        }
        let row = native_row(self.brightness(), a, n);
        let mask = self.0[1] >> 31 != 0 && (240..=254).contains(&source_index);
        let scale = if mask {
            let d = (n * 30 / 200).saturating_sub(1).min((n - 1) / 2);
            [if n > 1 && row <= d {
                row * 65536 / d
            } else {
                65536
            }; 3]
        } else {
            [self.0[0], self.0[1], self.0[2]].map(|s| {
                let base = s & 0x3ffff;
                if n == 1 {
                    base
                } else {
                    base * 2 * row / (n - 1)
                }
            })
        };
        let lit: [u32; 3] = std::array::from_fn(|i| {
            if self.0[1] & (1 << 30) != 0 {
                ((u32::from(rgb[i]) * scale[i]) >> 16).min(255)
            } else {
                ((u32::from(rgb[i]) * (scale[i] >> 4)) >> 12).min(255)
            }
        });
        let word = (((lit[0] >> 3) << 11) | ((lit[1] >> 2) << 5) | (lit[2] >> 3)) as u16;
        // A blitter never draws index 0, so no word reaches it.
        if source_index == 0 {
            word
        } else {
            word | self.colour_word()
        }
    }
}

/// Exact reduction of the 53-bit/chop x87 expression on the clamped domain
/// 0..2000. Binary64 0.065536 is slightly below nominal: neutral is 65535.
/// The original-byte oracle covers all 2001 inputs.
pub fn native_scale16(light_milli: i32) -> u32 {
    let light = light_milli.clamp(0, 2000) as u32;
    (light * 65536).saturating_sub(1) / 1000
}

/// 0x00420140 table and 0x00493DF0/0x00494B60 active selectors.
pub fn native_row(brightness: i32, a: u8, rows: u32) -> u32 {
    let q = ((i64::from(brightness.max(0)) * 261) >> 11).min(254) as u32;
    (u32::from(a) * q * (rows - 1) / 32258).min(rows - 1)
}

/// One shared shader implementation and one existing presentation-profile
/// owner. wgpu27 sRGB texture reads decode RGB; framebuffer writes encode it.
/// https://docs.rs/wgpu/27.0.1/wgpu/enum.TextureFormat.html
pub(crate) fn shader_source(body: &str) -> String {
    let profile = super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
    let words = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "const RETAIL_FIVE = array<u32,32>({});\nconst RETAIL_SIX = array<u32,64>({});\n{}\n{}",
        words(&profile.five_bit),
        words(&profile.six_bit),
        include_str!("palette_light.wgsl"),
        body
    )
}

#[cfg(test)]
pub(crate) struct NativePaletteFixture {
    pub rows: u32,
    pub rgb: [i32; 3],
    pub house: bool,
    pub plain: bool,
    pub bytes: &'static [u8],
}

#[cfg(test)]
pub(crate) fn native_fixtures() -> [NativePaletteFixture; 9] {
    macro_rules! fixture {
        ($n:expr, $rgb:expr, $house:expr, $file:literal) => {
            NativePaletteFixture {
                rows: $n,
                rgb: $rgb,
                house: $house,
                plain: false,
                bytes: crate::test_fixture::bytes(concat!("tools/palette_oracle/fixtures/", $file)),
            }
        };
    }
    [
        fixture!(1, [1000; 3], true, "palette-1-1000-1000-1000-mmx-house.bin"),
        fixture!(53, [1000; 3], false, "palette-53-1000-1000-1000-mmx.bin"),
        fixture!(53, [992, 768, 512], false, "palette-53-992-768-512-mmx.bin"),
        fixture!(27, [512, 640, 768], false, "palette-27-512-640-768-mmx.bin"),
        fixture!(53, [2000, 1, 1999], false, "palette-53-2000-1-1999-mmx.bin"),
        fixture!(
            53,
            [1000; 3],
            true,
            "palette-53-1000-1000-1000-mmx-house.bin"
        ),
        fixture!(
            53,
            [992, 768, 512],
            true,
            "palette-53-992-768-512-mmx-house.bin"
        ),
        NativePaletteFixture {
            rows: 1,
            rgb: [1000; 3],
            house: false,
            plain: true,
            bytes: crate::test_fixture::bytes("tools/palette_oracle/fixtures/palette-1-plain.bin"),
        },
        NativePaletteFixture {
            rows: 53,
            rgb: [1000; 3],
            house: false,
            plain: true,
            bytes: crate::test_fixture::bytes("tools/palette_oracle/fixtures/palette-53-plain.bin"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_house_colors_match_original_normal_and_campaign_initialization() {
        use crate::rules::house_colors::{HouseColorIndex, NO_REMAP};
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/house_color.json",
        ))
        .unwrap();
        assert_eq!(native["constructor_palette_index"], 16);
        let colors = native["colors"].as_array().unwrap();
        assert_eq!(colors.len(), 21);
        assert_eq!(rules.color_schemes.len(), colors.len());
        for (index, row) in colors.iter().enumerate() {
            let scheme = &rules.color_schemes[index];
            assert_eq!(scheme.name, row["name"].as_str().unwrap());
            assert_eq!(serde_json::json!(scheme.hsv), row["hsv"], "{}", scheme.name);
            let index = HouseColorIndex(index as u8);
            let ramp: Vec<_> = rules
                .house_color_ramps
                .ramp(index)
                .iter()
                .map(|color| [color.r, color.g, color.b])
                .collect();
            assert_eq!(serde_json::json!(ramp), row["ramp_rgb"], "{}", scheme.name);
            let rgb = serde_json::json!(house_color_rgb(&rules.house_color_ramps, index));
            let laser_rgb = serde_json::json!(house_laser_rgb(&rules.house_color_ramps, index));
            for mode in ["scalar", "cmov", "mmx"] {
                assert_eq!(rgb, row["modes"][mode]["rgb"], "{} {mode}", scheme.name);
                assert_eq!(
                    rgb, row["modes"][mode]["campaign_rgb"],
                    "{} {mode}",
                    scheme.name
                );
                assert_eq!(
                    laser_rgb, row["modes"][mode]["laser_rgb"],
                    "{} {mode}",
                    scheme.name
                );
                assert_eq!(
                    laser_rgb, row["modes"][mode]["campaign_laser_rgb"],
                    "{} {mode}",
                    scheme.name
                );
            }
        }
        assert_eq!(
            serde_json::json!(house_color_rgb(&rules.house_color_ramps, NO_REMAP)),
            colors[crate::rules::house_colors::DEFAULT_SCHEME_ENTRY]["modes"]["scalar"]["rgb"]
        );
        // The negative-index control supplies a synthetic lookup table. It
        // establishes registry index5 (N53 variant of physical entry2), not
        // the RGB of the stock LightGrey table checked above.
        assert_eq!(native["negative_scheme"]["normal"]["scheme_index"], 5);
        assert_eq!(native["negative_scheme"]["campaign"]["scheme_index"], 5);
    }

    #[test]
    fn house_laser_normalization_matches_every_original_rgb565_input() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/house_color.json",
        ))
        .unwrap();
        let domain = &native["laser_rgb565_domain"];
        assert_eq!(domain["count"], 65536);
        assert_eq!(domain["bytes_per_result"], 3);
        let hex = domain["rgb_hex"].as_str().unwrap();
        assert_eq!(hex.len(), 65536 * 6);
        for (word, bytes) in hex.as_bytes().chunks_exact(6).enumerate() {
            let expected: [u8; 3] = std::array::from_fn(|channel| {
                u8::from_str_radix(
                    std::str::from_utf8(&bytes[channel * 2..channel * 2 + 2]).unwrap(),
                    16,
                )
                .unwrap()
            });
            let rgb = crate::render::native_surface_format::RGB565.unpack_rgb8(word as u16);
            assert_eq!(
                normalized_house_laser_rgb(rgb),
                expected,
                "RGB565 {word:04X}"
            );
        }
    }

    #[test]
    fn all_clamped_base_scales_match_original_x87_bytes() {
        let golden =
            crate::test_fixture::bytes("tools/palette_oracle/fixtures/base-scales-0-2000.bin");
        for (light, value) in golden.chunks_exact(4).enumerate() {
            assert_eq!(
                native_scale16(light as i32),
                u32::from_le_bytes(value.try_into().unwrap()),
                "light={light}"
            );
        }
    }

    #[test]
    fn brightness_a_row_selection_matches_original_generated_luts() {
        for (n, bytes) in [
            (
                27,
                crate::test_fixture::bytes("tools/palette_oracle/fixtures/intensity-27.bin"),
            ),
            (
                53,
                crate::test_fixture::bytes("tools/palette_oracle/fixtures/intensity-53.bin"),
            ),
        ] {
            for b in -1i32..=2000 {
                let q = ((b.max(0) * 261) >> 11).min(254) as usize;
                for a in 0..=255u8 {
                    let offset = (usize::from(a) * 256 + q) * 2;
                    let native = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) >> 8;
                    assert_eq!(
                        native_row(b, a, n),
                        u32::from(native),
                        "n={n}, b={b}, A={a}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_entry_in_original_palette_tables_matches() {
        for fixture in native_fixtures() {
            for row in 0..fixture.rows {
                let b = (0..=2000)
                    .find(|b| native_row(*b, 127, fixture.rows) == row)
                    .unwrap();
                let light = if fixture.plain {
                    PaletteLight::plain(fixture.rows, b)
                } else {
                    PaletteLight::new(fixture.rgb, fixture.rows, b, fixture.house)
                };
                for index in 0..=255u8 {
                    let rgb = [index, index.wrapping_mul(73), 255 - index];
                    let offset = (row as usize * 256 + usize::from(index)) * 2;
                    let native =
                        u16::from_le_bytes([fixture.bytes[offset], fixture.bytes[offset + 1]]);
                    assert_eq!(
                        light.rgb565(rgb, index, 127),
                        native,
                        "rgb={:?}, n={}, house={}, row={row}, index={index}",
                        fixture.rgb,
                        fixture.rows,
                        fixture.house
                    );
                }
            }
        }
    }
}
