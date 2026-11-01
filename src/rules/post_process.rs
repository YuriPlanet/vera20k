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
//! SMAA=no
//! SMAA.Threshold=0.1
//! SMAA.SearchSteps=8
//! DLAA=no
//! DLAA.Strength=1
//! DLAA.Lambda=3
//! DLAA.Epsilon=0.05
//! DLAA.LongEdgeThreshold=0.35
//! DLAA.EdgeContrast=2
//! Blur=no
//! Blur.Radius=2
//! Blur.Strength=1
//! Sharpen=no
//! Sharpen.Strength=0.35
//! Sharpen.Clamp=0.035
//! SuperResolution=no
//! SuperResolution.Sharpness=0.7
//! SuperResolution.RenderScale=1
//! SuperResolution.Fsr=no
//! Deband=no
//! Deband.Radius=4
//! Deband.Threshold=0.02
//! Deband.Grain=1
//! Clarity=no
//! Clarity.Radius=6
//! Clarity.Strength=0.3
//! Vibrance=no
//! Vibrance.Amount=0.3
//! DepthOfField=no
//! DepthOfField.Focus=0.5
//! DepthOfField.Range=0.2
//! DepthOfField.Strength=3
//! SSAO=no
//! SSAO.Radius=6
//! SSAO.Intensity=0.35
//! SSAO.Bias=0.02
//! Tonemap=no
//! Tonemap.Operator=aces
//! Tonemap.Exposure=1
//! Lut=no
//! Lut.File=
//! Lut.Strength=1
//! Lut.File2=
//! Lut.Strength2=1
//! Lut.File3=
//! Lut.Strength3=1
//! Lut.Atlas=no
//! Lut.AtlasFile=
//! Lut.AtlasSelector=0
//! Lut.AtlasChroma=1
//! Lut.AtlasLuma=1
//! Bloom=no
//! Bloom.Threshold=0.5
//! Bloom.Intensity=0.4
//! Bloom.Radius=1
//! ArcaneBloom=no
//! PirateBloom=no
//! PirateBloom.Threshold=0.5
//! PirateBloom.Strength=0.5
//! PirateBloom.Radius=5
//! PirateBloom.Saturation=1
//! PirateBloom.Blend=1
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
    /// The SMAA-class morphological antialiaser; `None` leaves it out.
    pub smaa: Option<SmaaRules>,
    /// Barbatos' spatial DLAA (Andreev); `None` leaves it out.
    pub dlaa: Option<DlaaRules>,
    /// The Gaussian blur; `None` leaves it out.
    pub blur: Option<BlurRules>,
    /// The unsharp-mask sharpen; `None` leaves it out.
    pub sharpen: Option<SharpenRules>,
    /// The contrast-adaptive `SuperResolution` sharpen; `None` leaves it out.
    pub super_resolution: Option<SuperResolutionRules>,
    /// Gradient banding removal with a dither; `None` leaves it out.
    pub deband: Option<DebandRules>,
    /// Wide-radius local contrast; `None` leaves it out.
    pub clarity: Option<ClarityRules>,
    /// `Pirate_Curve`: a luma/chroma contrast curve; `None` leaves it out.
    pub curve: Option<CurveRules>,
    /// Chroma gain on muted colours; `None` leaves it out.
    pub vibrance: Option<VibranceRules>,
    /// Depth-driven focus blur; `None` leaves it out.
    pub depth_of_field: Option<DepthOfFieldRules>,
    /// Depth-contrast ambient occlusion; `None` leaves it out.
    pub ssao: Option<SsaoRules>,
    /// HDR-ish tone curve applied at presentation; `None` leaves it out.
    pub tonemap: Option<TonemapRules>,
    /// A `.cube` 3D colour lookup applied at presentation; `None` leaves it out.
    pub lut: Option<LutRules>,
    /// `MultiLUT.fx`: one atlas sheet holding stacked 32x32 LUTs; `None`
    /// leaves it out.
    pub lut_atlas: Option<AtlasLutRules>,
    /// The multi-pass bloom; `None` leaves it out.
    pub bloom: Option<BloomRules>,
    /// luluco250's `ArcaneBloom`: five Gaussian levels plus an optional
    /// luminance-adaptation exposure; `None` leaves it out.
    pub arcane_bloom: Option<ArcaneBloomRules>,
    /// `Pirate_Bloom.fx`: two thresholded box-blur pass pairs at quarter
    /// resolution, composited back over the frame; `None` leaves it out.
    pub pirate_bloom: Option<PirateBloomRules>,
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

/// SMAA 1x (Jimenez et al. 2013) with the AreaTex/SearchTex lookup tables.
/// Diagonals stay off, i.e. the same tier as SMAA's Medium preset: luma edge
/// detection, run-length search, area lookup and corner rounding. The
/// threshold is in gamma luma; `search_steps` caps how far the run search
/// walks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmaaRules {
    pub threshold: f32,
    pub search_steps: f32,
}

impl Default for SmaaRules {
    fn default() -> Self {
        Self {
            threshold: 0.1,
            search_steps: 8.0,
        }
    }
}

/// Spatial half of Barbatos' `BaBa_DLAA-T.fx` (Directionally Localized AA,
/// Andreev GDC 2011). No temporal accumulation: the effect only needs the
/// current frame, so it ports as a prefilter pass plus a spatial pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DlaaRules {
    /// Blend of the antialiased result over the frame, 0..=1.
    pub strength: f32,
    /// Short-edge effectiveness (TFU2 used 3.0).
    pub lambda: f32,
    /// Bias subtracted from the edge crush and short-edge gate.
    pub epsilon: f32,
    /// Noise rejection: the two long-edge masks must differ by more than this.
    pub long_edge_threshold: f32,
    /// High-pass gain in the prefilter.
    pub edge_contrast: f32,
}

impl Default for DlaaRules {
    fn default() -> Self {
        Self {
            strength: 1.0,
            lambda: 3.0,
            epsilon: 0.05,
            long_edge_threshold: 0.35,
            edge_contrast: 2.0,
        }
    }
}

/// Gaussian blur over the composed frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurRules {
    /// Kernel step in frame pixels; 0 is a no-op.
    pub radius: f32,
    /// Blend of the blurred image over the original; 1 is fully blurred.
    pub strength: f32,
}

impl Default for BlurRules {
    fn default() -> Self {
        Self {
            radius: 2.0,
            strength: 1.0,
        }
    }
}

/// Unsharp-mask sharpen, in the style of ReShade `LumaSharpen.fx`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SharpenRules {
    /// Edge gain; 0 is neutral.
    pub strength: f32,
    /// Per-channel cap on the sharpening delta, in gamma space.
    pub clamp: f32,
}

impl Default for SharpenRules {
    fn default() -> Self {
        Self {
            strength: 0.35,
            clamp: 0.035,
        }
    }
}

/// Contrast-adaptive sharpen in the style of the ReShade `SuperResolution.fx`
/// edge-enhancement shader: the gain follows local contrast, so flat areas stay
/// clean and edges tighten without ringing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuperResolutionRules {
    /// Edge gain; 0 is neutral.
    pub sharpness: f32,
    /// Battlefield render resolution as a fraction of the frame (0.5..=1).
    /// 1 renders native; below it the world layer is rendered smaller and
    /// reconstructed to the frame, while UI keeps its native pixels.
    pub render_scale: f32,
    /// Use the FSR1 pair: the EASU reconstruction on the world upscale and the
    /// RCAS sharpen. `no` keeps the bicubic reconstruction and the CAS-style
    /// sharpener.
    pub fsr: bool,
}

impl Default for SuperResolutionRules {
    fn default() -> Self {
        Self {
            sharpness: 0.7,
            render_scale: 1.0,
            fsr: false,
        }
    }
}

/// Banding removal in the spirit of ReShade `Deband.fx`: average the nearby
/// texels whose luma is inside `threshold` (which smooths 8-bit steps in
/// gradients but leaves edges), then dither so the quantizer cannot recreate
/// the same steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebandRules {
    /// Tap distance in frame pixels.
    pub radius: f32,
    /// Largest luma difference a tap may have and still be averaged.
    pub threshold: f32,
    /// Dither amplitude, as a fraction of the threshold.
    pub grain: f32,
}

impl Default for DebandRules {
    fn default() -> Self {
        Self {
            radius: 4.0,
            threshold: 0.02,
            grain: 1.0,
        }
    }
}

/// `Pirate_Curve.fx`: raises luma, chroma or both to `1 + strength`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveRules {
    /// 0 luma only, 1 chroma only, 2 both (a plain per-channel gamma).
    pub method: u32,
    /// Negative flattens contrast, positive deepens it; 0 is neutral.
    pub strength: f32,
}

impl Default for CurveRules {
    fn default() -> Self {
        Self {
            method: 2,
            strength: 0.2,
        }
    }
}

/// Wide-radius local contrast: texture and unit silhouettes read stronger than
/// a small unsharp mask, without the tight halos.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClarityRules {
    /// Blur radius in frame pixels.
    pub radius: f32,
    /// Local-contrast gain; 0 is neutral.
    pub strength: f32,
}

impl Default for ClarityRules {
    fn default() -> Self {
        Self {
            radius: 6.0,
            strength: 0.3,
        }
    }
}

/// Chroma gain that falls off as a texel saturates, so muted terrain gains
/// life while already-saturated colours stay put.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VibranceRules {
    pub amount: f32,
}

impl Default for VibranceRules {
    fn default() -> Self {
        Self { amount: 0.3 }
    }
}

/// Depth-driven focus blur.
///
/// The engine's Z buffer stores the native row/height sort key (0 far, 1 near)
/// rather than camera distance, so this reads as a focus band across the
/// isometric world: everything whose native Z sits outside the `focus` band
/// blurs, and the band edge falls off over `range`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthOfFieldRules {
    /// Native Z the sharp band is centred on, 0..=1.
    pub focus: f32,
    /// Native Z half-width of the fully sharp band.
    pub range: f32,
    /// Peak blur radius in frame pixels.
    pub strength: f32,
}

impl Default for DepthOfFieldRules {
    fn default() -> Self {
        Self {
            focus: 0.5,
            range: 0.2,
            strength: 3.0,
        }
    }
}

/// Depth-contrast ambient occlusion: darkens pixels that sit behind a
/// neighbour much nearer in native Z, which reads as contact shading where an
/// object meets the ground behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SsaoRules {
    /// Sampling radius in frame pixels.
    pub radius: f32,
    /// Darkening at full occlusion; 0 is neutral.
    pub intensity: f32,
    /// Native Z gap below which a neighbour does not count as occluding.
    pub bias: f32,
}

impl Default for SsaoRules {
    fn default() -> Self {
        Self {
            radius: 6.0,
            intensity: 0.35,
            bias: 0.02,
        }
    }
}

/// Presentation tone curve. The frame is LDR, so this shapes mid-tones rather
/// than recovering highlights; `Exposure` scales linear light first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TonemapOperator {
    Reinhard,
    Hable,
    #[default]
    Aces,
}

impl TonemapOperator {
    /// The uniform lane the shader switches on.
    pub fn lane(self) -> f32 {
        match self {
            Self::Reinhard => 0.0,
            Self::Hable => 1.0,
            Self::Aces => 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TonemapRules {
    pub operator: TonemapOperator,
    pub exposure: f32,
}

impl Default for TonemapRules {
    fn default() -> Self {
        Self {
            operator: TonemapOperator::Aces,
            exposure: 1.0,
        }
    }
}

/// A `.cube` 3D lookup table. `File` is resolved beside the player's
/// VERARULES.INI; the table is sampled trilinearly and mixed by `Strength`.
/// Up to three tables can be stacked (MultiLUT): each one is applied in turn,
/// so `File2`/`File3` refine the colour the earlier tables produced.
#[derive(Debug, Clone, PartialEq)]
pub struct LutRules {
    pub file: String,
    pub strength: f32,
    pub file2: String,
    pub strength2: f32,
    pub file3: String,
    pub strength3: f32,
}

impl Default for LutRules {
    fn default() -> Self {
        Self {
            file: String::new(),
            strength: 1.0,
            file2: String::new(),
            strength2: 1.0,
            file3: String::new(),
            strength3: 1.0,
        }
    }
}

/// `MultiLUT.fx`: one sheet of stacked 32x32 3D LUTs (17 rows, 32 tiles
/// across), sampled by `selector` and mixed by the chroma/luma amounts.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasLutRules {
    /// `MultiLut_atlas1.png`-shaped sheet beside VERARULES.INI; empty loads
    /// the copy the renderer ships.
    pub file: String,
    /// LUT row in the sheet, 0..=16 (row 0 is neutral).
    pub selector: u32,
    /// Chroma mix against the table, 0..=1.
    pub chroma: f32,
    /// Luma mix against the table, 0..=1.
    pub luma: f32,
}

impl Default for AtlasLutRules {
    fn default() -> Self {
        Self {
            file: String::new(),
            selector: 0,
            chroma: 1.0,
            luma: 1.0,
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

/// `ArcaneBloom.fx` (luluco250): the graded frame goes through five downsample
/// + 13-tap Gaussian levels whose sum is mixed back as HDR bloom, and an
/// optional adaptation term scales the whole frame by how bright the scene
/// averages over `adapt_time`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcaneBloomRules {
    /// Bloom mix strength (the shader's `uBloomIntensity`).
    pub intensity: f32,
    /// How much of the adaptation exposure applies, 0..=1.
    pub adapt_intensity: f32,
    /// Seconds the adaptation takes to follow the scene.
    pub adapt_time: f32,
    /// Multiplier on the averaged luminance before it clamps.
    pub sensitivity: f32,
    /// Luminance mip the adaptation reads: 0 averages the whole scene,
    /// higher picks a finer level.
    pub precision: u32,
    /// Clamp the adaptation luminance to `limit_min..=limit_max`.
    pub limits: bool,
    pub limit_min: f32,
    pub limit_max: f32,
    /// Exposure applied while compositing.
    pub exposure: f32,
    /// Scene brightness that maps to the shader's HDR ceiling.
    pub max_brightness: f32,
}

impl Default for ArcaneBloomRules {
    fn default() -> Self {
        Self {
            intensity: 1.0,
            adapt_intensity: 1.0,
            adapt_time: 1.0,
            sensitivity: 1.0,
            precision: 0,
            limits: true,
            limit_min: 0.0,
            limit_max: 1.0,
            exposure: 1.0,
            max_brightness: 100.0,
        }
    }
}

/// `Pirate_Bloom.fx`: a thresholded bright pass, two horizontal/vertical
/// 12-tap box pairs at a quarter of the frame, then a composite with the
/// effect's blend modes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PirateBloomRules {
    /// Bright-pass floor; texels below it contribute nothing.
    pub threshold: f32,
    /// Bloom contribution strength.
    pub strength: f32,
    /// Tap spacing in frame pixels.
    pub radius: f32,
    /// 1 keeps the bloom's chroma, 0 grays it.
    pub saturation: f32,
    /// 0 add, 1 add without clipping, 2 screen, 3 soft light, 4 colour dodge.
    pub blend: u32,
}

impl Default for PirateBloomRules {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            strength: 0.5,
            radius: 5.0,
            saturation: 1.0,
            blend: 1,
        }
    }
}

/// Water-tile refraction driven by the engine water mask.
///
/// The terrain (Z-depth) shader offsets water-tile UVs by the field this
/// generates, so bridges, units, overlays and UI drawn later are never
/// displaced. The amplitude/wavelength/speed pair drives the field itself;
/// `anim_interval_ms` steps the field's animation instead of scrolling it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterRippleRules {
    /// Peak sampling offset in frame pixels.
    pub amplitude: f32,
    /// Distortion-field noise cell size in frame pixels.
    pub wavelength: f32,
    /// Animation speed, in cells per second.
    pub speed: f32,
    /// Step the distortion animation this many milliseconds per frame; 0
    /// scrolls continuously.
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
            smaa: section.read_bool("SMAA", false).then(|| {
                let defaults = SmaaRules::default();
                SmaaRules {
                    threshold: section.read_float("SMAA.Threshold", defaults.threshold),
                    search_steps: section
                        .read_float("SMAA.SearchSteps", defaults.search_steps),
                }
            }),
            dlaa: section.read_bool("DLAA", false).then(|| {
                let defaults = DlaaRules::default();
                DlaaRules {
                    strength: section.read_float("DLAA.Strength", defaults.strength),
                    lambda: section.read_float("DLAA.Lambda", defaults.lambda),
                    epsilon: section.read_float("DLAA.Epsilon", defaults.epsilon),
                    long_edge_threshold: section
                        .read_float("DLAA.LongEdgeThreshold", defaults.long_edge_threshold),
                    edge_contrast: section
                        .read_float("DLAA.EdgeContrast", defaults.edge_contrast),
                }
            }),
            blur: section.read_bool("Blur", false).then(|| {
                let defaults = BlurRules::default();
                BlurRules {
                    radius: section.read_float("Blur.Radius", defaults.radius),
                    strength: section.read_float("Blur.Strength", defaults.strength),
                }
            }),
            sharpen: section.read_bool("Sharpen", false).then(|| {
                let defaults = SharpenRules::default();
                SharpenRules {
                    strength: section.read_float("Sharpen.Strength", defaults.strength),
                    clamp: section.read_float("Sharpen.Clamp", defaults.clamp),
                }
            }),
            super_resolution: section.read_bool("SuperResolution", false).then(|| {
                let defaults = SuperResolutionRules::default();
                SuperResolutionRules {
                    sharpness: section
                        .read_float("SuperResolution.Sharpness", defaults.sharpness),
                    render_scale: section.read_float(
                        "SuperResolution.RenderScale",
                        defaults.render_scale,
                    ),
                    fsr: section.read_bool("SuperResolution.Fsr", defaults.fsr),
                }
            }),
            deband: section.read_bool("Deband", false).then(|| {
                let defaults = DebandRules::default();
                DebandRules {
                    radius: section.read_float("Deband.Radius", defaults.radius),
                    threshold: section.read_float("Deband.Threshold", defaults.threshold),
                    grain: section.read_float("Deband.Grain", defaults.grain),
                }
            }),
            clarity: section.read_bool("Clarity", false).then(|| {
                let defaults = ClarityRules::default();
                ClarityRules {
                    radius: section.read_float("Clarity.Radius", defaults.radius),
                    strength: section.read_float("Clarity.Strength", defaults.strength),
                }
            }),
            curve: section.read_bool("Curve", false).then(|| {
                let defaults = CurveRules::default();
                CurveRules {
                    method: section.read_int("Curve.Method", defaults.method as i32).clamp(0, 2) as u32,
                    strength: section.read_float("Curve.Strength", defaults.strength),
                }
            }),
            vibrance: section.read_bool("Vibrance", false).then(|| VibranceRules {
                amount: section.read_float(
                    "Vibrance.Amount",
                    VibranceRules::default().amount,
                ),
            }),
            bloom: section.read_bool("Bloom", false).then(|| {
                let defaults = BloomRules::default();
                BloomRules {
                    threshold: section.read_float("Bloom.Threshold", defaults.threshold),
                    intensity: section.read_float("Bloom.Intensity", defaults.intensity),
                    radius: section.read_float("Bloom.Radius", defaults.radius),
                }
            }),
            arcane_bloom: section.read_bool("ArcaneBloom", false).then(|| {
                let defaults = ArcaneBloomRules::default();
                ArcaneBloomRules {
                    intensity: section
                        .read_float("ArcaneBloom.Intensity", defaults.intensity),
                    adapt_intensity: section
                        .read_float("ArcaneBloom.Adapt.Intensity", defaults.adapt_intensity),
                    adapt_time: section
                        .read_float("ArcaneBloom.Adapt.Time", defaults.adapt_time),
                    sensitivity: section
                        .read_float("ArcaneBloom.Adapt.Sensitivity", defaults.sensitivity),
                    precision: section
                        .read_int("ArcaneBloom.Adapt.Precision", defaults.precision as i32)
                        .clamp(0, 11) as u32,
                    limits: section.read_bool("ArcaneBloom.Adapt.Limits", defaults.limits),
                    limit_min: section
                        .read_float("ArcaneBloom.Adapt.LimitMin", defaults.limit_min),
                    limit_max: section
                        .read_float("ArcaneBloom.Adapt.LimitMax", defaults.limit_max),
                    exposure: section.read_float("ArcaneBloom.Exposure", defaults.exposure),
                    max_brightness: section
                        .read_float("ArcaneBloom.MaxBrightness", defaults.max_brightness),
                }
            }),
            pirate_bloom: section.read_bool("PirateBloom", false).then(|| {
                let defaults = PirateBloomRules::default();
                PirateBloomRules {
                    threshold: section
                        .read_float("PirateBloom.Threshold", defaults.threshold),
                    strength: section.read_float("PirateBloom.Strength", defaults.strength),
                    radius: section.read_float("PirateBloom.Radius", defaults.radius),
                    saturation: section
                        .read_float("PirateBloom.Saturation", defaults.saturation),
                    blend: section
                        .read_int("PirateBloom.Blend", defaults.blend as i32)
                        .clamp(0, 4) as u32,
                }
            }),
            depth_of_field: section.read_bool("DepthOfField", false).then(|| {
                let defaults = DepthOfFieldRules::default();
                DepthOfFieldRules {
                    focus: section.read_float("DepthOfField.Focus", defaults.focus),
                    range: section.read_float("DepthOfField.Range", defaults.range),
                    strength: section.read_float("DepthOfField.Strength", defaults.strength),
                }
            }),
            ssao: section.read_bool("SSAO", false).then(|| {
                let defaults = SsaoRules::default();
                SsaoRules {
                    radius: section.read_float("SSAO.Radius", defaults.radius),
                    intensity: section.read_float("SSAO.Intensity", defaults.intensity),
                    bias: section.read_float("SSAO.Bias", defaults.bias),
                }
            }),
            tonemap: section.read_bool("Tonemap", false).then(|| {
                let defaults = TonemapRules::default();
                TonemapRules {
                    operator: match section
                        .read_string("Tonemap.Operator", "aces", 16)
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "reinhard" => TonemapOperator::Reinhard,
                        "hable" | "filmic" | "uncharted" => TonemapOperator::Hable,
                        _ => TonemapOperator::Aces,
                    },
                    exposure: section.read_float("Tonemap.Exposure", defaults.exposure),
                }
            }),
            lut: section.read_bool("Lut", false).then(|| {
                let defaults = LutRules::default();
                LutRules {
                    file: section.read_string("Lut.File", &defaults.file, 260),
                    strength: section.read_float("Lut.Strength", defaults.strength),
                    file2: section.read_string("Lut.File2", &defaults.file2, 260),
                    strength2: section.read_float("Lut.Strength2", defaults.strength2),
                    file3: section.read_string("Lut.File3", &defaults.file3, 260),
                    strength3: section.read_float("Lut.Strength3", defaults.strength3),
                }
            }),
            lut_atlas: section.read_bool("Lut.Atlas", false).then(|| {
                let defaults = AtlasLutRules::default();
                AtlasLutRules {
                    file: section.read_string("Lut.AtlasFile", &defaults.file, 260),
                    selector: section
                        .read_int("Lut.AtlasSelector", defaults.selector as i32)
                        .clamp(0, 16) as u32,
                    chroma: section.read_float("Lut.AtlasChroma", defaults.chroma),
                    luma: section.read_float("Lut.AtlasLuma", defaults.luma),
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
             SMAA=yes\nDLAA=yes\nBlur=yes\nSharpen=yes\nSuperResolution=yes\n\
             Deband=yes\nClarity=yes\nVibrance=yes\n\
             DepthOfField=yes\nSSAO=yes\nTonemap=yes\nLut=yes\nLut.Atlas=yes\n\
             Bloom=yes\nPirateBloom=yes\nWaterRipple=yes\n",
        ));
        assert_eq!(defaults.filmic_pass, Some(FilmicPassRules::default()));
        assert_eq!(
            defaults.colourfulness,
            Some(ColourfulnessRules::default())
        );
        assert_eq!(defaults.sepia, Some(SepiaRules::default()));
        assert_eq!(defaults.fxaa, Some(FxaaRules::default()));
        assert_eq!(defaults.smaa, Some(SmaaRules::default()));
        assert_eq!(defaults.dlaa, Some(DlaaRules::default()));
        assert_eq!(defaults.blur, Some(BlurRules::default()));
        assert_eq!(defaults.sharpen, Some(SharpenRules::default()));
        assert_eq!(
            defaults.super_resolution,
            Some(SuperResolutionRules::default())
        );
        assert_eq!(defaults.deband, Some(DebandRules::default()));
        assert_eq!(defaults.clarity, Some(ClarityRules::default()));
        assert_eq!(defaults.vibrance, Some(VibranceRules::default()));
        assert_eq!(
            defaults.depth_of_field,
            Some(DepthOfFieldRules::default())
        );
        assert_eq!(defaults.ssao, Some(SsaoRules::default()));
        assert_eq!(defaults.tonemap, Some(TonemapRules::default()));
        assert_eq!(defaults.lut, Some(LutRules::default()));
        assert_eq!(defaults.lut_atlas, Some(AtlasLutRules::default()));
        assert_eq!(defaults.bloom, Some(BloomRules::default()));
        assert_eq!(defaults.pirate_bloom, Some(PirateBloomRules::default()));
        assert_eq!(
            defaults.water_ripple,
            Some(WaterRippleRules::default())
        );

        let off = PostProcessRules::from_ini(&IniFile::from_str(
            "[PostProcess]\nFilmicPass=no\nFilmicPass.Strength=0\n\
             Colourfulness=no\nColourfulness.Amount=0\nSepia=no\nSepia.Strength=0\n\
             FXAA=no\nFXAA.Subpix=1\nSMAA=no\nSMAA.Threshold=0.9\n\
             DLAA=no\nDLAA.Strength=0\nDLAA.Lambda=1\nBlur=no\nBlur.Strength=0\n\
             Sharpen=no\nSharpen.Strength=0\nSuperResolution=no\nSuperResolution.Sharpness=0\n\
             SuperResolution.Fsr=yes\nDeband=no\nDeband.Threshold=0.2\n\
             Clarity=no\nClarity.Strength=1\nVibrance=no\nVibrance.Amount=1\n\
             DepthOfField=no\nDepthOfField.Strength=0\nSSAO=no\nSSAO.Intensity=0\n\
             Tonemap=no\nTonemap.Exposure=0\nLut=no\nLut.Strength=0\nLut.Strength2=0\n\
             Lut.Strength3=0\nLut.Atlas=no\nLut.AtlasFile=atlas.png\nLut.AtlasChroma=0\n\
             Bloom=no\nBloom.Intensity=1\nPirateBloom=no\nPirateBloom.Strength=1\n\
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
             SMAA=yes\nSMAA.Threshold=0.18\nSMAA.SearchSteps=12\n\
             DLAA=yes\nDLAA.Strength=0.8\nDLAA.Lambda=4.5\nDLAA.Epsilon=0.1\n\
             DLAA.LongEdgeThreshold=0.5\nDLAA.EdgeContrast=3\n\
             Blur=yes\nBlur.Radius=3.5\nBlur.Strength=0.4\n\
             Sharpen=yes\nSharpen.Strength=0.7\nSharpen.Clamp=0.06\n\
             SuperResolution=yes\nSuperResolution.Sharpness=0.9\nSuperResolution.RenderScale=0.75\n\
             SuperResolution.Fsr=yes\nDeband=yes\nDeband.Radius=7\n\
             Deband.Threshold=0.03\nDeband.Grain=0.5\nClarity=yes\nClarity.Radius=5\n\
             Clarity.Strength=0.4\nVibrance=yes\nVibrance.Amount=0.6\n\
             DepthOfField=yes\nDepthOfField.Focus=0.4\nDepthOfField.Range=0.1\n\
             DepthOfField.Strength=6\nSSAO=yes\nSSAO.Radius=9\nSSAO.Intensity=0.6\n\
             SSAO.Bias=0.05\nTonemap=yes\nTonemap.Operator=reinhard\n\
             Tonemap.Exposure=0.8\nLut=yes\nLut.File=film.cube\nLut.Strength=0.5\n\
             Lut.File2=warm.cube\nLut.Strength2=0.4\nLut.File3=cool.cube\nLut.Strength3=0.25\n\
             Lut.Atlas=yes\nLut.AtlasFile=atlas.png\nLut.AtlasSelector=3\n\
             Lut.AtlasChroma=0.5\nLut.AtlasLuma=0.2\n\
             Bloom=yes\nBloom.Threshold=0.3\nBloom.Intensity=0.6\nBloom.Radius=1.5\n\
             PirateBloom=yes\nPirateBloom.Threshold=0.4\nPirateBloom.Strength=0.7\n\
             PirateBloom.Radius=4\nPirateBloom.Saturation=0.5\nPirateBloom.Blend=2\n\
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
        let smaa = authored.smaa.expect("authored smaa");
        assert!((smaa.threshold - 0.18).abs() < 1e-6);
        assert!((smaa.search_steps - 12.0).abs() < 1e-6);
        let dlaa = authored.dlaa.expect("authored dlaa");
        assert!((dlaa.strength - 0.8).abs() < 1e-6);
        assert!((dlaa.lambda - 4.5).abs() < 1e-6);
        assert!((dlaa.epsilon - 0.1).abs() < 1e-6);
        assert!((dlaa.long_edge_threshold - 0.5).abs() < 1e-6);
        assert!((dlaa.edge_contrast - 3.0).abs() < 1e-6);
        let blur = authored.blur.expect("authored blur");
        assert!((blur.radius - 3.5).abs() < 1e-6);
        assert!((blur.strength - 0.4).abs() < 1e-6);
        let sharpen = authored.sharpen.expect("authored sharpen");
        assert!((sharpen.strength - 0.7).abs() < 1e-6);
        assert!((sharpen.clamp - 0.06).abs() < 1e-6);
        let super_resolution = authored
            .super_resolution
            .expect("authored super resolution");
        assert!((super_resolution.sharpness - 0.9).abs() < 1e-6);
        assert!((super_resolution.render_scale - 0.75).abs() < 1e-6);
        assert!(super_resolution.fsr);
        let deband = authored.deband.expect("authored deband");
        assert!((deband.radius - 7.0).abs() < 1e-6);
        assert!((deband.threshold - 0.03).abs() < 1e-6);
        assert!((deband.grain - 0.5).abs() < 1e-6);
        let clarity = authored.clarity.expect("authored clarity");
        assert!((clarity.radius - 5.0).abs() < 1e-6);
        assert!((clarity.strength - 0.4).abs() < 1e-6);
        let vibrance = authored.vibrance.expect("authored vibrance");
        assert!((vibrance.amount - 0.6).abs() < 1e-6);
        let depth_of_field = authored
            .depth_of_field
            .expect("authored depth of field");
        assert!((depth_of_field.focus - 0.4).abs() < 1e-6);
        assert!((depth_of_field.range - 0.1).abs() < 1e-6);
        assert!((depth_of_field.strength - 6.0).abs() < 1e-6);
        let ssao = authored.ssao.expect("authored ssao");
        assert!((ssao.radius - 9.0).abs() < 1e-6);
        assert!((ssao.intensity - 0.6).abs() < 1e-6);
        assert!((ssao.bias - 0.05).abs() < 1e-6);
        let tonemap = authored.tonemap.expect("authored tonemap");
        assert_eq!(tonemap.operator, TonemapOperator::Reinhard);
        assert!((tonemap.exposure - 0.8).abs() < 1e-6);
        let lut = authored.lut.expect("authored lut");
        assert_eq!(lut.file, "film.cube");
        assert!((lut.strength - 0.5).abs() < 1e-6);
        assert_eq!(lut.file2, "warm.cube");
        assert!((lut.strength2 - 0.4).abs() < 1e-6);
        assert_eq!(lut.file3, "cool.cube");
        assert!((lut.strength3 - 0.25).abs() < 1e-6);
        let bloom = authored.bloom.expect("authored bloom");
        assert!((bloom.threshold - 0.3).abs() < 1e-6);
        assert!((bloom.intensity - 0.6).abs() < 1e-6);
        assert!((bloom.radius - 1.5).abs() < 1e-6);
        let atlas = authored.lut_atlas.expect("authored lut atlas");
        assert_eq!(atlas.file, "atlas.png");
        assert_eq!(atlas.selector, 3);
        assert!((atlas.chroma - 0.5).abs() < 1e-6);
        assert!((atlas.luma - 0.2).abs() < 1e-6);
        let pirate = authored.pirate_bloom.expect("authored pirate bloom");
        assert!((pirate.threshold - 0.4).abs() < 1e-6);
        assert!((pirate.strength - 0.7).abs() < 1e-6);
        assert!((pirate.radius - 4.0).abs() < 1e-6);
        assert!((pirate.saturation - 0.5).abs() < 1e-6);
        assert_eq!(pirate.blend, 2);
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
