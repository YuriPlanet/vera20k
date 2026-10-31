//! VERA-only `[PostProcess]` presentation configuration.
//!
//! These keys are not read by gamemd: retail has no post-processing. They
//! choose whether the match frame is handed to the optional full-frame shader
//! chain ([`crate::render::post_process`]) instead of the 1:1 texture copy.
//! Every effect is off by default, so an unedited rules set keeps the retail
//! presentation path and its cost.
//!
//! The effect mathematics follow the ReShade community shaders of the same
//! names (`FilmicPass.fx`, `Colourfulness.fx`, `Sepia.fx`), which run in gamma
//! space; curve and gamma constants that those shaders expose are kept at their
//! authored defaults here so the INI surface stays small.
//!
//! ## INI format
//! ```ini
//! [PostProcess]
//! ColorGrade=no
//! ColorGrade.Brightness=0
//! ColorGrade.Contrast=1
//! ColorGrade.Saturation=1
//! FilmicPass=no
//! FilmicPass.Strength=0.85
//! FilmicPass.Fade=0.4
//! FilmicPass.Contrast=1
//! FilmicPass.Linearization=0.5
//! FilmicPass.Bleach=0
//! FilmicPass.Saturation=-0.15
//! Colourfulness=no
//! Colourfulness.Amount=0.4
//! Colourfulness.LimLuma=0.7
//! Sepia=no
//! Sepia.Strength=0.58
//! Sepia.TintR=0.55
//! Sepia.TintG=0.43
//! Sepia.TintB=0.42
//! FXAA=no
//! FXAA.Subpix=0.25
//! FXAA.EdgeThreshold=0.125
//! FXAA.EdgeThresholdMin=0
//! Bloom=no
//! Bloom.Threshold=0.5
//! Bloom.Intensity=0.4
//! Bloom.Radius=1
//! WaterRipple=no
//! WaterRipple.Amplitude=2
//! WaterRipple.Wavelength=32
//! WaterRipple.Speed=0.4
//! WaterRipple.AnimInterval=0
//! WaterRipple.Debug=no
//! ```

use crate::rules::ini_parser::IniFile;

/// `[PostProcess]` retained for presentation. Empty by default.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PostProcessRules {
    /// The colour-grade pass; `None` keeps the frame on the copy path.
    pub color_grade: Option<ColorGradeRules>,
    /// The ReShade `FilmicPass` port; `None` leaves it out.
    pub filmic_pass: Option<FilmicPassRules>,
    /// The ReShade `Colourfulness` port; `None` leaves it out.
    pub colourfulness: Option<ColourfulnessRules>,
    /// The ReShade `Sepia`/`Tint` port; `None` leaves it out.
    pub sepia: Option<SepiaRules>,
    /// The ReShade `FXAA` port; `None` leaves it out.
    pub fxaa: Option<FxaaRules>,
    /// The multi-pass bloom; `None` leaves it out.
    pub bloom: Option<BloomRules>,
    /// Mask-gated water displacement; `None` leaves it out.
    pub water_ripple: Option<WaterRippleRules>,
}

/// Linear-space colour grading applied to the composed match frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorGradeRules {
    /// Added to each linear channel; 0 is neutral.
    pub brightness: f32,
    /// Scales each channel around mid-grey; 1 is neutral.
    pub contrast: f32,
    /// Mixes toward luma; 1 is neutral, 0 is greyscale.
    pub saturation: f32,
}

/// ReShade `FilmicPass.fx`: cinema-like curve, fade, bleach and saturation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilmicPassRules {
    pub strength: f32,
    pub fade: f32,
    pub contrast: f32,
    pub linearization: f32,
    pub bleach: f32,
    pub saturation: f32,
}

impl Default for FilmicPassRules {
    fn default() -> Self {
        Self {
            strength: 0.85,
            fade: 0.4,
            contrast: 1.0,
            linearization: 0.5,
            bleach: 0.0,
            saturation: -0.15,
        }
    }
}

/// ReShade `Colourfulness.fx`: saturation raise that soft-limits at the
/// display gamut rather than clipping channel ratios.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColourfulnessRules {
    pub amount: f32,
    pub lim_luma: f32,
}

impl Default for ColourfulnessRules {
    fn default() -> Self {
        Self {
            amount: 0.4,
            lim_luma: 0.7,
        }
    }
}

/// ReShade `Sepia.fx` (`technique Tint`): multiply toward a warm tint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SepiaRules {
    pub strength: f32,
    pub tint: [f32; 3],
}

impl Default for SepiaRules {
    fn default() -> Self {
        Self {
            strength: 0.58,
            tint: [0.55, 0.43, 0.42],
        }
    }
}

/// ReShade `FXAA.fx` (FXAA 3.11 quality): sub-pixel blend and edge gates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxaaRules {
    pub subpix: f32,
    pub edge_threshold: f32,
    pub edge_threshold_min: f32,
}

impl Default for FxaaRules {
    fn default() -> Self {
        Self {
            subpix: 0.25,
            edge_threshold: 0.125,
            edge_threshold_min: 0.0,
        }
    }
}

/// Four-level downsample/upsample bloom over the composed frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BloomRules {
    /// Display-luma floor for the bright pass; 0.0 includes everything.
    pub threshold: f32,
    /// Bloom add strength at the composite step.
    pub intensity: f32,
    /// Upsample tent radius multiplier.
    pub radius: f32,
}

impl Default for BloomRules {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            intensity: 0.4,
            radius: 1.0,
        }
    }
}

/// Water-tile refraction driven by the engine water mask.
///
/// The terrain (Z-depth) shader offsets water-tile UVs by the field this
/// generates, so bridges, units, overlays and UI drawn later are never
/// displaced. The amplitude/wavelength/speed pair drives the field itself;
/// `anim_interval_ms` keeps TC2's stepped `DistortionAnimInterval` cadence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterRippleRules {
    /// Peak sampling offset in frame pixels.
    pub amplitude: f32,
    /// Distortion-field noise cell size in frame pixels.
    pub wavelength: f32,
    /// Animation speed, in cells per second.
    pub speed: f32,
    /// Step the distortion animation this many milliseconds per frame; 0
    /// scrolls continuously (TC2 `DistortionAnimInterval`).
    pub anim_interval_ms: f32,
    /// Tint masked texels magenta, to verify mask alignment.
    pub debug: bool,
}

impl Default for WaterRippleRules {
    fn default() -> Self {
        Self {
            amplitude: 2.0,
            wavelength: 32.0,
            speed: 0.4,
            anim_interval_ms: 0.0,
            debug: false,
        }
    }
}

impl PostProcessRules {
    /// Read `[PostProcess]`. Each `Effect=` key is that effect's own switch;
    /// when it is `no` (the default) the effect's other keys are not read.
    pub fn from_ini(ini: &IniFile) -> Self {
        let section = ini.section_or_empty("PostProcess");
        // The shader consumes binary32, so read each key through the native
        // ReadDouble-into-float control word instead of narrowing an f64.
        Self {
            color_grade: section.read_bool("ColorGrade", false).then(|| ColorGradeRules {
                brightness: section.read_float("ColorGrade.Brightness", 0.0),
                contrast: section.read_float("ColorGrade.Contrast", 1.0),
                saturation: section.read_float("ColorGrade.Saturation", 1.0),
            }),
            filmic_pass: section.read_bool("FilmicPass", false).then(|| {
                let defaults = FilmicPassRules::default();
                FilmicPassRules {
                    strength: section.read_float("FilmicPass.Strength", defaults.strength),
                    fade: section.read_float("FilmicPass.Fade", defaults.fade),
                    contrast: section.read_float("FilmicPass.Contrast", defaults.contrast),
                    linearization: section
                        .read_float("FilmicPass.Linearization", defaults.linearization),
                    bleach: section.read_float("FilmicPass.Bleach", defaults.bleach),
                    saturation: section
                        .read_float("FilmicPass.Saturation", defaults.saturation),
                }
            }),
            colourfulness: section
                .read_bool("Colourfulness", false)
                .then(|| {
                    let defaults = ColourfulnessRules::default();
                    ColourfulnessRules {
                        amount: section.read_float("Colourfulness.Amount", defaults.amount),
                        lim_luma: section
                            .read_float("Colourfulness.LimLuma", defaults.lim_luma),
                    }
                }),
            sepia: section.read_bool("Sepia", false).then(|| {
                let defaults = SepiaRules::default();
                SepiaRules {
                    strength: section.read_float("Sepia.Strength", defaults.strength),
                    tint: [
                        section.read_float("Sepia.TintR", defaults.tint[0]),
                        section.read_float("Sepia.TintG", defaults.tint[1]),
                        section.read_float("Sepia.TintB", defaults.tint[2]),
                    ],
                }
            }),
            fxaa: section.read_bool("FXAA", false).then(|| {
                let defaults = FxaaRules::default();
                FxaaRules {
                    subpix: section.read_float("FXAA.Subpix", defaults.subpix),
                    edge_threshold: section
                        .read_float("FXAA.EdgeThreshold", defaults.edge_threshold),
                    edge_threshold_min: section
                        .read_float("FXAA.EdgeThresholdMin", defaults.edge_threshold_min),
                }
            }),
            bloom: section.read_bool("Bloom", false).then(|| {
                let defaults = BloomRules::default();
                BloomRules {
                    threshold: section.read_float("Bloom.Threshold", defaults.threshold),
                    intensity: section.read_float("Bloom.Intensity", defaults.intensity),
                    radius: section.read_float("Bloom.Radius", defaults.radius),
                }
            }),
            water_ripple: section.read_bool("WaterRipple", false).then(|| {
                let defaults = WaterRippleRules::default();
                WaterRippleRules {
                    amplitude: section.read_float("WaterRipple.Amplitude", defaults.amplitude),
                    wavelength: section
                        .read_float("WaterRipple.Wavelength", defaults.wavelength),
                    speed: section.read_float("WaterRipple.Speed", defaults.speed),
                    anim_interval_ms: section
                        .read_float("WaterRipple.AnimInterval", defaults.anim_interval_ms),
                    debug: section.read_bool("WaterRipple.Debug", defaults.debug),
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_grade_is_off_without_the_section_or_its_switch() {
        assert_eq!(
            PostProcessRules::from_ini(&IniFile::from_str("")),
            PostProcessRules::default()
        );
        let inert = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nColorGrade=no\nColorGrade.Saturation=0\n",
        ));
        assert_eq!(
            inert,
            PostProcessRules::default(),
            "other keys do not enable it"
        );
    }

    #[test]
    fn color_grade_reads_the_neutral_defaults_and_authored_values() {
        let neutral =
            PostProcessRules::from_ini(&IniFile::from_str("[PostProcess]\nColorGrade=yes\n"));
        assert_eq!(
            neutral.color_grade,
            Some(ColorGradeRules {
                brightness: 0.0,
                contrast: 1.0,
                saturation: 1.0,
            })
        );
        let authored = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nColorGrade=yes\n\
             ColorGrade.Brightness=0.1\nColorGrade.Contrast=1.2\nColorGrade.Saturation=0.8\n",
        ));
        let grade = authored.color_grade.expect("authored colour grade");
        assert!((grade.brightness - 0.1).abs() < 1e-6);
        assert!((grade.contrast - 1.2).abs() < 1e-6);
        assert!((grade.saturation - 0.8).abs() < 1e-6);
    }

    /// The ReShade ports default to their authored values and read authored
    /// overrides; a `no` switch keeps the whole effect out.
    #[test]
    fn reshade_ports_read_their_switches_defaults_and_values() {
        let defaults = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nFilmicPass=yes\nColourfulness=yes\nSepia=yes\nFXAA=yes\n\
             Bloom=yes\nWaterRipple=yes\n",
        ));
        assert_eq!(defaults.filmic_pass, Some(FilmicPassRules::default()));
        assert_eq!(
            defaults.colourfulness,
            Some(ColourfulnessRules::default())
        );
        assert_eq!(defaults.sepia, Some(SepiaRules::default()));
        assert_eq!(defaults.fxaa, Some(FxaaRules::default()));
        assert_eq!(defaults.bloom, Some(BloomRules::default()));
        assert_eq!(
            defaults.water_ripple,
            Some(WaterRippleRules::default())
        );

        let off = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nFilmicPass=no\nFilmicPass.Strength=0\n\
             Colourfulness=no\nColourfulness.Amount=0\nSepia=no\nSepia.Strength=0\n\
             FXAA=no\nFXAA.Subpix=1\nBloom=no\nBloom.Intensity=1\n\
             WaterRipple=no\nWaterRipple.Amplitude=9\n",
        ));
        assert_eq!(off, PostProcessRules::default(), "switches gate every key");

        let authored = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nFilmicPass=yes\nFilmicPass.Strength=0.5\nFilmicPass.Fade=0.2\n\
             FilmicPass.Contrast=1.4\nFilmicPass.Linearization=0.8\nFilmicPass.Bleach=0.3\n\
             FilmicPass.Saturation=0.1\nColourfulness=yes\nColourfulness.Amount=0.2\n\
             Colourfulness.LimLuma=0.5\nSepia=yes\nSepia.Strength=0.3\n\
             Sepia.TintR=0.2\nSepia.TintG=0.3\nSepia.TintB=0.4\n\
             FXAA=yes\nFXAA.Subpix=0.6\nFXAA.EdgeThreshold=0.2\nFXAA.EdgeThresholdMin=0.05\n\
             Bloom=yes\nBloom.Threshold=0.3\nBloom.Intensity=0.6\nBloom.Radius=1.5\n\
             WaterRipple=yes\nWaterRipple.Amplitude=3\nWaterRipple.Wavelength=24\n\
             WaterRipple.Speed=0.8\nWaterRipple.AnimInterval=66\n\
             WaterRipple.Debug=yes\n",
        ));
        let filmic = authored.filmic_pass.expect("authored filmic pass");
        assert!((filmic.strength - 0.5).abs() < 1e-6);
        assert!((filmic.fade - 0.2).abs() < 1e-6);
        assert!((filmic.contrast - 1.4).abs() < 1e-6);
        assert!((filmic.linearization - 0.8).abs() < 1e-6);
        assert!((filmic.bleach - 0.3).abs() < 1e-6);
        assert!((filmic.saturation - 0.1).abs() < 1e-6);
        let colour = authored.colourfulness.expect("authored colourfulness");
        assert!((colour.amount - 0.2).abs() < 1e-6);
        assert!((colour.lim_luma - 0.5).abs() < 1e-6);
        let sepia = authored.sepia.expect("authored sepia");
        assert!((sepia.strength - 0.3).abs() < 1e-6);
        assert!((sepia.tint[0] - 0.2).abs() < 1e-6);
        assert!((sepia.tint[1] - 0.3).abs() < 1e-6);
        assert!((sepia.tint[2] - 0.4).abs() < 1e-6);
        let fxaa = authored.fxaa.expect("authored fxaa");
        assert!((fxaa.subpix - 0.6).abs() < 1e-6);
        assert!((fxaa.edge_threshold - 0.2).abs() < 1e-6);
        assert!((fxaa.edge_threshold_min - 0.05).abs() < 1e-6);
        let bloom = authored.bloom.expect("authored bloom");
        assert!((bloom.threshold - 0.3).abs() < 1e-6);
        assert!((bloom.intensity - 0.6).abs() < 1e-6);
        assert!((bloom.radius - 1.5).abs() < 1e-6);
        let ripple = authored.water_ripple.expect("authored water ripple");
        assert!((ripple.amplitude - 3.0).abs() < 1e-6);
        assert!((ripple.wavelength - 24.0).abs() < 1e-6);
        assert!((ripple.speed - 0.8).abs() < 1e-6);
        assert!((ripple.anim_interval_ms - 66.0).abs() < 1e-6);
        assert!(ripple.debug);
    }

    /// The production `RuleSet` publishes the section; an empty INI leaves the
    /// presentation on the copy path.
    #[test]
    fn ruleset_publishes_the_post_process_section() {
        let empty = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str("")).unwrap();
        assert_eq!(empty.post_process, PostProcessRules::default());

        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[PostProcess]\nColorGrade=yes\nColorGrade.Saturation=0.5\n\
             FilmicPass=yes\nFXAA=yes\nBloom=yes\nWaterRipple=yes\n",
        ))
        .unwrap();
        assert_eq!(
            rules.post_process.color_grade,
            Some(ColorGradeRules {
                brightness: 0.0,
                contrast: 1.0,
                saturation: 0.5,
            })
        );
        assert_eq!(
            rules.post_process.filmic_pass,
            Some(FilmicPassRules::default())
        );
        assert_eq!(rules.post_process.fxaa, Some(FxaaRules::default()));
        assert_eq!(rules.post_process.bloom, Some(BloomRules::default()));
        assert_eq!(
            rules.post_process.water_ripple,
            Some(WaterRippleRules::default())
        );
    }
}
