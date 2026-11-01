//! Full-frame post-processing at the presentation boundary.
//!
//! The composed match frame already lives in an offscreen texture
//! (`RendererState::game_frame_texture`) and reaches the window through a 1:1
//! `copy_texture_to_texture`. This module replaces that copy with a shader
//! chain when effects are configured, and keeps the copy when none are.
//!
//! ## Identity
//! The chain must be invisible when it has nothing to do: with every effect
//! disabled the fragment shader returns the `textureLoad` texel before any
//! sampling, filtering or colour conversion, so it reproduces the copy byte for
//! byte. `post_process_tests` compares the two paths on the production surface
//! format; the shader also has CPU-side parse tests so the checked-in WGSL is
//! validated without a GPU.
//!
//! ## Effects
//! - `ColorGrade` runs in linear RGB on the decoded texel.
//! - `FXAA` ports the ReShade FXAA 3.11 quality pass; it samples the source
//!   through a filtering sampler and blends edge texels before the other
//!   effects run.
//! - `FilmicPass`, `Colourfulness` and `Sepia` are ports of the ReShade
//!   community shaders of those names. They are authored in gamma space, so
//!   the shader encodes to sRGB before running them and decodes back for the
//!   sRGB target.
//! - `Bloom` is a four-level downsample/upsample chain derived from the
//!   ReShade bloom shaders: a display-luma bright pass, box downsamples and
//!   additive tent upsamples into RGBA16Float levels, then a composite over
//!   the graded frame.
//! - `SMAA` is SMAA 1x (Jimenez et al. 2013): the edge pass marks luma steps,
//!   the weight pass searches runs with `SearchTex` and looks coverage up in
//!   `AreaTex` (both shipped as `.dds`), and the presentation pass blends
//!   across the weights. Diagonals are off, which is SMAA's Medium tier.
//! - `Lut` stacks up to three `.cube` tables in binding order 5/7/8, each mixed
//!   by its own strength, so later tables refine the colour earlier ones made.
//! - `Lut.Atlas` samples `MultiLUT`'s sheet: 17 stacked 32x32 LUTs at binding 9,
//!   mixed by its own chroma/luma amounts.
//! - `Pirate_Bloom` runs the quarter-resolution threshold box chain after the
//!   bloom stage (see `render/pirate_bloom.rs`).
//! - `DLAA` finishes the presentation with Barbatos' spatial Directionally
//!   Localized AA (see `render/dlaa.rs`); its optional TAA is not ported.
//! - `Deband` averages rotated neighbours inside a luma threshold and dithers
//!   only where the neighbourhood varies, so gradients lose their 8-bit steps.
//! - `Clarity` lifts local contrast at a wide radius; `Vibrance` scales chroma
//!   by how unsaturated a texel already is.
//! - `SuperResolution.Fsr` pairs the FSR1 EASU reconstruction
//!   (`scaled_copy.wgsl`) with the RCAS sharpen here. The default path keeps
//!   the bicubic reconstruction and the CAS-style sharpener.
//!
//! ## Colour space
//! The frame target carries the surface's sRGB suffix, so `textureLoad` decodes
//! to linear and the target re-encodes on write. Bloom levels stay linear.
//!
//! Depends on: `render/gpu.rs` (through the caller) and wgpu only.

use crate::rules::post_process::{ArcaneBloomRules, DlaaRules, PirateBloomRules};
use wgpu::util::DeviceExt;

const SHADER_SRC: &str = include_str!("post_process.wgsl");
const BLOOM_SHADER_SRC: &str = include_str!("post_process_bloom.wgsl");
const BLOOM_COMPOSITE_SRC: &str = include_str!("post_process_bloom_composite.wgsl");
const SMAA_SHADER_SRC: &str = include_str!("post_process_smaa.wgsl");
/// SMAA's coverage table (160x560 RG8) and run-length table (64x16 R8), the
/// reference `Textures/AreaTexDX9.dds` and `Textures/SearchTex.dds` from
/// Jimenez et al.'s SMAA repository.
const SMAA_AREA_TEX: &[u8] = include_bytes!("smaa_area_tex.dds");
const SMAA_SEARCH_TEX: &[u8] = include_bytes!("smaa_search_tex.dds");
/// `MultiLut_atlas1.png`: the sheet the `MultiLUT` arm samples when
/// `Lut.AtlasFile` names no replacement (32x32 tiles, 32 across, 17 rows).
const MULTILUT_ATLAS: &[u8] = include_bytes!("multilut_atlas1.png");


/// Bloom levels below half resolution: 1/2, 1/4, 1/8 and 1/16 of the frame.
const BLOOM_LEVELS: usize = 4;

/// Uniform payload for [`PostChain`]; eight vec4s keep std140 alignment trivial.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PostParams {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    /// 1 runs the colour-grade arm; 0 leaves the texel untouched.
    color_grade: f32,

    /// `(x, y, width, height)` of the tactical viewport in frame pixels. Every
    /// texel outside it — the sidebar column and the bottom strip — returns
    /// unchanged, so UI keeps its retail colours.
    viewport: [f32; 4],

    filmic_strength: f32,
    filmic_fade: f32,
    filmic_contrast: f32,
    filmic_linearization: f32,

    filmic_bleach: f32,
    filmic_saturation: f32,
    /// 1 runs the FilmicPass arm; 0 skips it.
    filmic_enabled: f32,
    _pad0: f32,

    colour_amount: f32,
    colour_lim_luma: f32,
    /// 1 runs the Colourfulness arm; 0 skips it.
    colour_enabled: f32,
    /// 1 runs the Sepia arm; 0 skips it.
    sepia_enabled: f32,

    sepia_strength: f32,
    sepia_tint_r: f32,
    sepia_tint_g: f32,
    sepia_tint_b: f32,

    fxaa_subpix: f32,
    fxaa_edge_threshold: f32,
    fxaa_edge_threshold_min: f32,
    /// 1 runs the FXAA arm; 0 skips it.
    fxaa_enabled: f32,

    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    /// 1 runs the bloom chain; 0 composites nothing.
    bloom_enabled: f32,

    /// `Pirate_Curve`: contrast curve strength and method (0 luma, 1 chroma,
    /// 2 both), then whether it runs.
    curve_strength: f32,
    curve_method: f32,
    curve_enabled: f32,
    _pad1_d: f32,

    /// 1 tints masked texels magenta, to verify the engine's mask alignment;
    /// then the `MultiLUT` atlas selector row, chroma mix and luma mix.
    mask_debug: f32,
    atlas_selector: f32,
    atlas_chroma: f32,
    atlas_luma: f32,

    /// Gaussian blur: kernel step in pixels, blend, and the enable flag.
    blur_radius: f32,
    blur_strength: f32,
    blur_enabled: f32,
    sharpen_strength: f32,

    /// Unsharp mask: per-channel delta cap, enable flag; then the
    /// contrast-adaptive `SuperResolution` gain and its enable flag.
    sharpen_clamp: f32,
    sharpen_enabled: f32,
    sr_sharpness: f32,
    sr_enabled: f32,

    /// Depth target texels per frame pixel, then the focus band.
    depth_scale_x: f32,
    depth_scale_y: f32,
    dof_focus: f32,
    dof_range: f32,

    /// Focus blur radius and enable flag, then the AO radius and strength.
    dof_strength: f32,
    dof_enabled: f32,
    ssao_radius: f32,
    ssao_intensity: f32,

    /// Native-Z gap an occluder must clear, then the AO enable flag, the
    /// `MultiLUT` atlas enable flag and padding.
    ssao_bias: f32,
    ssao_enabled: f32,
    atlas_enabled: f32,
    _pad3_c: f32,

    /// Tone curve lane (0 Reinhard, 1 Hable, 2 ACES), exposure, enable flag,
    /// then the first LUT mix strength.
    tonemap_operator: f32,
    tonemap_exposure: f32,
    tonemap_enabled: f32,
    lut_strength: f32,

    /// Second and third LUT mix strengths (0 skips that table), the LUT enable
    /// flag and the SMAA enable flag.
    lut_strength2: f32,
    lut_strength3: f32,
    lut_enabled: f32,
    smaa_enabled: f32,

    /// SMAA edge threshold and search steps, mirrored into the SMAA passes'
    /// own uniform, then padding.
    smaa_threshold: f32,
    smaa_search: f32,
    _pad5_a: f32,
    _pad5_b: f32,

    /// Deband: search radius, luma threshold, dither grain, enable flag.
    deband_radius: f32,
    deband_threshold: f32,
    deband_grain: f32,
    deband_enabled: f32,

    /// Clarity: wide-radius local contrast, its strength and enable flag.
    clarity_radius: f32,
    clarity_strength: f32,
    clarity_enabled: f32,
    _pad6_a: f32,

    /// Vibrance amount and enable flag.
    vibrance_amount: f32,
    vibrance_enabled: f32,
    _pad6_b: f32,
    _pad6_c: f32,

    /// SuperResolution lane: 0 the CAS-style arm, 1 the RCAS-style arm.
    sr_mode: f32,
    _pad6_d: f32,
    _pad6_e: f32,
    _pad6_f: f32,
}

/// The depth-driven effects' inputs. `Default` binds no depth work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DepthEffects {
    /// Depth-target texels per frame pixel, so the shader can address the
    /// (possibly smaller) battlefield depth buffer from frame coordinates.
    pub scale: [f32; 2],
    /// Focus, range, strength.
    pub depth_of_field: Option<[f32; 3]>,
    /// Radius, intensity, bias.
    pub ssao: Option<[f32; 3]>,
}

impl Default for DepthEffects {
    fn default() -> Self {
        Self {
            scale: [1.0, 1.0],
            depth_of_field: None,
            ssao: None,
        }
    }
}

impl PostParams {
    /// The byte-identical pass: every effect disabled.
    pub(crate) fn identity() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            color_grade: 0.0,
            viewport: [0.0; 4],
            filmic_strength: 0.0,
            filmic_fade: 0.0,
            filmic_contrast: 1.0,
            filmic_linearization: 1.0,
            filmic_bleach: 0.0,
            filmic_saturation: 0.0,
            filmic_enabled: 0.0,
            _pad0: 0.0,
            colour_amount: 0.0,
            colour_lim_luma: 1.0,
            colour_enabled: 0.0,
            sepia_enabled: 0.0,
            sepia_strength: 0.0,
            sepia_tint_r: 1.0,
            sepia_tint_g: 1.0,
            sepia_tint_b: 1.0,
            fxaa_subpix: 0.0,
            fxaa_edge_threshold: 0.125,
            fxaa_edge_threshold_min: 0.0,
            fxaa_enabled: 0.0,
            bloom_threshold: 0.0,
            bloom_intensity: 0.0,
            bloom_radius: 1.0,
            bloom_enabled: 0.0,
            curve_strength: 0.0,
            curve_method: 2.0,
            curve_enabled: 0.0,
            _pad1_d: 0.0,
            mask_debug: 0.0,
            atlas_selector: 0.0,
            atlas_chroma: 1.0,
            atlas_luma: 1.0,
            blur_radius: 0.0,
            blur_strength: 0.0,
            blur_enabled: 0.0,
            sharpen_strength: 0.0,
            sharpen_clamp: 0.035,
            sharpen_enabled: 0.0,
            sr_sharpness: 0.0,
            sr_enabled: 0.0,
            depth_scale_x: 1.0,
            depth_scale_y: 1.0,
            dof_focus: 0.5,
            dof_range: 0.2,
            dof_strength: 0.0,
            dof_enabled: 0.0,
            ssao_radius: 0.0,
            ssao_intensity: 0.0,
            ssao_bias: 0.02,
            ssao_enabled: 0.0,
            atlas_enabled: 0.0,
            _pad3_c: 0.0,
            tonemap_operator: 2.0,
            tonemap_exposure: 1.0,
            tonemap_enabled: 0.0,
            lut_strength: 0.0,
            lut_strength2: 0.0,
            lut_strength3: 0.0,
            lut_enabled: 0.0,
            smaa_enabled: 0.0,
            smaa_threshold: 0.1,
            smaa_search: 8.0,
            _pad5_a: 0.0,
            _pad5_b: 0.0,
            deband_radius: 4.0,
            deband_threshold: 0.02,
            deband_grain: 1.0,
            deband_enabled: 0.0,
            clarity_radius: 6.0,
            clarity_strength: 0.0,
            clarity_enabled: 0.0,
            _pad6_a: 0.0,
            vibrance_amount: 0.0,
            vibrance_enabled: 0.0,
            _pad6_b: 0.0,
            _pad6_c: 0.0,
            sr_mode: 0.0,
            _pad6_d: 0.0,
            _pad6_e: 0.0,
            _pad6_f: 0.0,
        }
    }

    /// Build the uniform from the configured effects; `None` means the chain
    /// has nothing to do and the caller should keep the texture copy.
    ///
    /// - `filmic`: strength, fade, contrast, linearization, bleach, saturation.
    /// - `colourfulness`: amount, lim_luma.
    /// - `sepia`: strength, tint red, tint green, tint blue.
    /// - `fxaa`: subpix, edge threshold, edge threshold minimum.
    /// - `blur`: radius in pixels, blend strength.
    /// - `sharpen`: strength, per-channel clamp.
    /// - `super_resolution`: contrast-adaptive sharpening gain.
    /// - `depth_effects`: depth buffer scale plus the DOF/AO parameters.
    /// - `bloom`: threshold, intensity, radius multiplier.
    /// - `mask_debug`: tint the engine water mask magenta.
    pub(crate) fn from_effects(
        color_grade: Option<[f32; 3]>,
        filmic: Option<[f32; 6]>,
        colourfulness: Option<[f32; 2]>,
        sepia: Option<[f32; 4]>,
        fxaa: Option<[f32; 3]>,
        blur: Option<[f32; 2]>,
        sharpen: Option<[f32; 2]>,
        super_resolution: Option<f32>,
        depth_effects: DepthEffects,
        bloom: Option<[f32; 3]>,
        mask_debug: bool,
        viewport: [f32; 4],
    ) -> Option<Self> {
        if color_grade.is_none()
            && filmic.is_none()
            && colourfulness.is_none()
            && sepia.is_none()
            && fxaa.is_none()
            && blur.is_none()
            && sharpen.is_none()
            && super_resolution.is_none()
            && depth_effects.depth_of_field.is_none()
            && depth_effects.ssao.is_none()
            && bloom.is_none()
            && !mask_debug
        {
            return None;
        }
        let (brightness, contrast, saturation, color_grade) = match color_grade {
            Some([brightness, contrast, saturation]) => (brightness, contrast, saturation, 1.0),
            None => (0.0, 1.0, 1.0, 0.0),
        };
        let [
            filmic_strength,
            filmic_fade,
            filmic_contrast,
            filmic_linearization,
            filmic_bleach,
            filmic_saturation,
        ] = filmic.unwrap_or([0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
        let [colour_amount, colour_lim_luma] = colourfulness.unwrap_or([0.0, 1.0]);
        let [sepia_strength, sepia_tint_r, sepia_tint_g, sepia_tint_b] =
            sepia.unwrap_or([0.0, 1.0, 1.0, 1.0]);
        let [fxaa_subpix, fxaa_edge_threshold, fxaa_edge_threshold_min] =
            fxaa.unwrap_or([0.0, 0.125, 0.0]);
        let [bloom_threshold, bloom_intensity, bloom_radius] =
            bloom.unwrap_or([0.0, 0.0, 1.0]);
        let [blur_radius, blur_strength] = blur.unwrap_or([0.0, 0.0]);
        let [sharpen_strength, sharpen_clamp] = sharpen.unwrap_or([0.0, 0.035]);
        let sr_sharpness = super_resolution.unwrap_or(0.0);
        let [dof_focus, dof_range, dof_strength] =
            depth_effects.depth_of_field.unwrap_or([0.5, 0.2, 0.0]);
        let [ssao_radius, ssao_intensity, ssao_bias] =
            depth_effects.ssao.unwrap_or([0.0, 0.0, 0.02]);
        Some(Self {
            brightness,
            contrast,
            saturation,
            color_grade,
            viewport,
            filmic_strength,
            filmic_fade,
            filmic_contrast,
            filmic_linearization,
            filmic_bleach,
            filmic_saturation,
            filmic_enabled: filmic.is_some() as u8 as f32,
            _pad0: 0.0,
            colour_amount,
            colour_lim_luma,
            colour_enabled: colourfulness.is_some() as u8 as f32,
            sepia_enabled: sepia.is_some() as u8 as f32,
            sepia_strength,
            sepia_tint_r,
            sepia_tint_g,
            sepia_tint_b,
            fxaa_subpix,
            fxaa_edge_threshold,
            fxaa_edge_threshold_min,
            fxaa_enabled: fxaa.is_some() as u8 as f32,
            bloom_threshold,
            bloom_intensity,
            bloom_radius,
            bloom_enabled: bloom.is_some() as u8 as f32,
            curve_strength: 0.0,
            curve_method: 2.0,
            curve_enabled: 0.0,
            _pad1_d: 0.0,
            mask_debug: mask_debug as u8 as f32,
            atlas_selector: 0.0,
            atlas_chroma: 1.0,
            atlas_luma: 1.0,
            blur_radius,
            blur_strength,
            blur_enabled: blur.is_some() as u8 as f32,
            sharpen_strength,
            sharpen_clamp,
            sharpen_enabled: sharpen.is_some() as u8 as f32,
            sr_sharpness,
            sr_enabled: super_resolution.is_some() as u8 as f32,
            depth_scale_x: depth_effects.scale[0],
            depth_scale_y: depth_effects.scale[1],
            dof_focus,
            dof_range,
            dof_strength,
            dof_enabled: depth_effects.depth_of_field.is_some() as u8 as f32,
            ssao_radius,
            ssao_intensity,
            ssao_bias,
            ssao_enabled: depth_effects.ssao.is_some() as u8 as f32,
            atlas_enabled: 0.0,
            _pad3_c: 0.0,
            tonemap_operator: 2.0,
            tonemap_exposure: 1.0,
            tonemap_enabled: 0.0,
            lut_strength: 0.0,
            lut_strength2: 0.0,
            lut_strength3: 0.0,
            lut_enabled: 0.0,
            smaa_enabled: 0.0,
            smaa_threshold: 0.1,
            smaa_search: 8.0,
            _pad5_a: 0.0,
            _pad5_b: 0.0,
            deband_radius: 4.0,
            deband_threshold: 0.02,
            deband_grain: 1.0,
            deband_enabled: 0.0,
            clarity_radius: 6.0,
            clarity_strength: 0.0,
            clarity_enabled: 0.0,
            _pad6_a: 0.0,
            vibrance_amount: 0.0,
            vibrance_enabled: 0.0,
            _pad6_b: 0.0,
            _pad6_c: 0.0,
            sr_mode: 0.0,
            _pad6_d: 0.0,
            _pad6_e: 0.0,
            _pad6_f: 0.0,
        })
    }

    /// Add the presentation colour stage (tone curve and up to three LUTs).
    /// Kept out of [`Self::from_effects`] so its arity stays stable for the FX
    /// arms. A strength of 0 leaves that table out; `lut_enabled` is set when
    /// any table contributes.
    pub(crate) fn with_color(
        mut self,
        tonemap: Option<[f32; 2]>,
        lut_strengths: [f32; 3],
    ) -> Self {
        if let Some([operator, exposure]) = tonemap {
            self.tonemap_operator = operator;
            self.tonemap_exposure = exposure;
            self.tonemap_enabled = 1.0;
        }
        self.lut_strength = lut_strengths[0];
        self.lut_strength2 = lut_strengths[1];
        self.lut_strength3 = lut_strengths[2];
        self.lut_enabled = if lut_strengths.iter().any(|strength| *strength > 0.0) {
            1.0
        } else {
            0.0
        };
        self
    }

    /// Configure the SMAA passes: edge threshold in gamma luma, search steps.
    /// The main pass reads the weight map the helper passes write.
    pub(crate) fn with_smaa(mut self, smaa: Option<[f32; 2]>) -> Self {
        if let Some([threshold, search]) = smaa {
            self.smaa_enabled = 1.0;
            self.smaa_threshold = threshold;
            self.smaa_search = search;
        }
        self
    }

    /// Configure the image-quality arms: deband `(radius, threshold, grain)`,
    /// clarity `(radius, strength)` and the vibrance amount. `None` leaves an
    /// arm out.
    pub(crate) fn with_quality(
        mut self,
        deband: Option<[f32; 3]>,
        clarity: Option<[f32; 2]>,
        vibrance: Option<f32>,
    ) -> Self {
        if let Some([radius, threshold, grain]) = deband {
            self.deband_radius = radius;
            self.deband_threshold = threshold;
            self.deband_grain = grain;
            self.deband_enabled = 1.0;
        }
        if let Some([radius, strength]) = clarity {
            self.clarity_radius = radius;
            self.clarity_strength = strength;
            self.clarity_enabled = 1.0;
        }
        if let Some(amount) = vibrance {
            self.vibrance_amount = amount;
            self.vibrance_enabled = 1.0;
        }
        self
    }

    /// `Pirate_Curve`: `(method, strength)`, where method 0 raises luma only,
    /// 1 chroma only and 2 both. `None` leaves the frame untouched.
    pub(crate) fn with_curve(mut self, curve: Option<[f32; 2]>) -> Self {
        if let Some([method, strength]) = curve {
            self.curve_strength = strength;
            self.curve_method = method;
            self.curve_enabled = 1.0;
        }
        self
    }

    /// `MultiLUT` atlas: `(selector, chroma, luma)`, sampled from the sheet
    /// bound at the atlas slot. `None` leaves the frame untouched.
    pub(crate) fn with_atlas_lut(mut self, atlas: Option<[f32; 3]>) -> Self {
        if let Some([selector, chroma, luma]) = atlas {
            self.atlas_selector = selector;
            self.atlas_chroma = chroma;
            self.atlas_luma = luma;
            self.atlas_enabled = 1.0;
        }
        self
    }

    /// Use the RCAS super-resolution arm — the sharpen FSR1 pairs with its
    /// upscale — instead of the CAS-style one.
    pub(crate) fn with_rcas(mut self, enabled: bool) -> Self {
        self.sr_mode = if enabled { 1.0 } else { 0.0 };
        self
    }

    /// The tactical-viewport rect the pass may touch; texels outside it stay
    /// byte copies.
    pub(crate) fn with_viewport(mut self, viewport: [f32; 4]) -> Self {
        self.viewport = viewport;
        self
    }

    /// Whether only the presentation stage is configured, so the caller must
    /// start from [`Self::identity`] rather than keep the 1:1 copy.
    pub(crate) fn presentation_configures_chain(
        tonemap: Option<[f32; 2]>,
        lut_strengths: [f32; 3],
        smaa: Option<[f32; 2]>,
    ) -> bool {
        tonemap.is_some() || smaa.is_some() || lut_strengths.iter().any(|s| *s > 0.0)
    }

    /// Colour grading; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn color_grade(
        brightness: f32,
        contrast: f32,
        saturation: f32,
        viewport: [f32; 4],
    ) -> Self {
        Self::from_effects(
            Some([brightness, contrast, saturation]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
            None,
            false,
            viewport,
        )
        .expect("colour grade enables the chain")
    }

    /// FilmicPass at its authored defaults; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn filmic(viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            Some([0.85, 0.4, 1.0, 0.5, 0.0, -0.15]),
            None,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
            None,
            false,
            viewport,
        )
        .expect("filmic pass enables the chain")
    }

    /// Sepia at its authored defaults; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn sepia(viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            None,
            None,
            Some([0.58, 0.55, 0.43, 0.42]),
            None,
            None,
            None,
            None,
            Default::default(),
            None,
            false,
            viewport,
        )
        .expect("sepia enables the chain")
    }

    /// FXAA at its authored defaults; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn fxaa(viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            None,
            None,
            None,
            Some([0.25, 0.125, 0.0]),
            None,
            None,
            None,
            Default::default(),
            None,
            false,
            viewport,
        )
        .expect("fxaa enables the chain")
    }

    /// Bloom with an explicit threshold; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn bloom(threshold: f32, intensity: f32, viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
            Some([threshold, intensity, 1.0]),
            false,
            viewport,
        )
        .expect("bloom enables the chain")
    }

    /// Mask debug tint; the tests build chains this way.
    #[cfg(test)]
    pub(crate) fn mask_debug(viewport: [f32; 4]) -> Self {
        Self::from_effects(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
            None,
            true,
            viewport,
        )
        .expect("mask debug enables the chain")
    }
}

/// One downsample level of the bloom chain.
struct BloomLevel {
    size: (u32, u32),
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// One full-frame shader chain drawn in place of the frame texture copy.
pub(crate) struct PostChain {
    format: wgpu::TextureFormat,
    bind_group_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    params_buffer: wgpu::Buffer,
    /// One uniform per bloom level: each pass needs its own target size, and
    /// queue writes all land before the single submitted command buffer, so a
    /// shared buffer would show every pass the last written size.
    bloom_level_params: Vec<wgpu::Buffer>,
    bloom_threshold: f32,
    bloom_intensity: f32,
    bloom_radius: f32,
    viewport: [f32; 4],
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    down_threshold_pipeline: wgpu::RenderPipeline,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    intermediate: Option<wgpu::Texture>,
    intermediate_view: Option<wgpu::TextureView>,
    levels: Vec<BloomLevel>,
    size: (u32, u32),
    bloom_enabled: bool,
    /// Far-plane depth bound when the caller has none, so the DOF/AO arms
    /// always have a valid `texture_depth_2d` to read.
    neutral_depth: wgpu::TextureView,
    _neutral_depth_texture: wgpu::Texture,
    /// The LUTs currently bound (neutral tables where none is loaded).
    neutral_lut: wgpu::TextureView,
    lut_views: [wgpu::TextureView; 3],
    _neutral_lut_texture: wgpu::Texture,
    /// The `MultiLUT` sheet bound at the atlas slot (neutral where none is
    /// loaded).
    atlas_view: wgpu::TextureView,
    neutral_atlas: wgpu::TextureView,
    _neutral_atlas_texture: wgpu::Texture,
    /// SMAA's edge and weight maps, rebuilt with the frame size.
    smaa_layout: wgpu::BindGroupLayout,
    smaa_params_buffer: wgpu::Buffer,
    smaa_edge_pipeline: wgpu::RenderPipeline,
    smaa_weight_pipeline: wgpu::RenderPipeline,
    smaa_edges: Option<wgpu::Texture>,
    smaa_edges_view: Option<wgpu::TextureView>,
    smaa_weights: Option<wgpu::Texture>,
    smaa_weights_view: Option<wgpu::TextureView>,
    /// The SMAA tables the weight pass samples.
    smaa_area_view: wgpu::TextureView,
    smaa_search_view: wgpu::TextureView,
    _smaa_area_texture: wgpu::Texture,
    _smaa_search_texture: wgpu::Texture,
    smaa_enabled: bool,
    /// Bound at the weight slot when SMAA is off, so the layout stays whole.
    neutral_weights: wgpu::TextureView,
    _neutral_weights_texture: wgpu::Texture,
    /// luluco250's `ArcaneBloom`, built on first use; it replaces the plain
    /// bloom composite whenever `arcane_rules` is set.
    arcane: Option<crate::render::arcane_bloom::ArcaneBloom>,
    arcane_rules: Option<ArcaneBloomRules>,
    /// `ArcaneBloom` writes here when `Pirate_Bloom` follows it, so the latter
    /// can read a finished frame instead of the one it is drawing over.
    _arcane_output: Option<wgpu::Texture>,
    arcane_output_view: Option<wgpu::TextureView>,
    /// `Pirate_Bloom`, built on first use; it runs after the bloom stage and
    /// composites into the destination.
    pirate: Option<crate::render::pirate_bloom::PirateBloom>,
    pirate_rules: Option<PirateBloomRules>,
    /// The frame the chain draws into when DLAA finishes the presentation.
    _dlaa_output: Option<wgpu::Texture>,
    dlaa_output_view: Option<wgpu::TextureView>,
    /// Barbatos' spatial DLAA, built on first use.
    dlaa: Option<crate::render::dlaa::Dlaa>,
    dlaa_rules: Option<DlaaRules>,
}

impl PostChain {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        params: PostParams,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process bloom"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_SHADER_SRC.into()),
        });
        let bloom_composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process bloom composite"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_COMPOSITE_SRC.into()),
        });
        let smaa_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post process smaa"),
            source: wgpu::ShaderSource::Wgsl(SMAA_SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Post process source"),
            entries: &[
                texture_entry(0, true),
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3, false),
                depth_entry(4),
                lut_entry(5),
                texture_entry(6, true),
                lut_entry(7),
                lut_entry(8),
                texture_entry(9, true),
            ],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Post process bloom composite"),
            entries: &[
                texture_entry(0, true),
                texture_entry(1, true),
                uniform_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Post process params"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));

        // Linear filtering, clamped: FXAA and the bloom upsample refetch texels.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Post process sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Post process"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = post_pipeline(
            device,
            "Post process",
            &layout,
            &shader,
            "fs_main",
            format,
            None,
        );

        let bloom_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Post process bloom"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let bloom_format = wgpu::TextureFormat::Rgba16Float;
        let down_threshold_pipeline = post_pipeline(
            device,
            "Post process bloom threshold",
            &bloom_layout,
            &bloom_shader,
            "fs_down_threshold",
            bloom_format,
            None,
        );
        let down_pipeline = post_pipeline(
            device,
            "Post process bloom down",
            &bloom_layout,
            &bloom_shader,
            "fs_down",
            bloom_format,
            None,
        );
        let up_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let up_pipeline = post_pipeline(
            device,
            "Post process bloom up",
            &bloom_layout,
            &bloom_shader,
            "fs_up",
            bloom_format,
            Some(up_blend),
        );

        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Post process bloom composite"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = post_pipeline(
            device,
            "Post process bloom composite",
            &composite_pipeline_layout,
            &bloom_composite_shader,
            "fs_composite",
            format,
            None,
        );

        let (neutral_depth, neutral_depth_texture) = create_neutral_depth(device);
        let (neutral_lut, neutral_lut_texture) = create_neutral_lut(device, queue);
        let (neutral_atlas, neutral_atlas_texture) = create_neutral_atlas(device, queue);

        // SMAA's helper passes share one 48-byte uniform: the edge threshold,
        // the search limit, the viewport rect and the corner rounding.
        let smaa_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Post process smaa"),
            entries: &[
                texture_entry(0, true),
                uniform_entry(1),
                texture_entry(2, true),
                texture_entry(3, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let smaa_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Post process smaa"),
                bind_group_layouts: &[Some(&smaa_layout)],
                immediate_size: 0,
            });
        let smaa_edge_pipeline = post_pipeline(
            device,
            "Post process smaa edges",
            &smaa_pipeline_layout,
            &smaa_shader,
            "fs_edges",
            wgpu::TextureFormat::Rgba8Unorm,
            None,
        );
        let smaa_weight_pipeline = post_pipeline(
            device,
            "Post process smaa weights",
            &smaa_pipeline_layout,
            &smaa_shader,
            "fs_weights",
            wgpu::TextureFormat::Rgba8Unorm,
            None,
        );
        let smaa_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Post process smaa params"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &smaa_params_buffer,
            0,
            bytemuck::cast_slice(&[
                params.smaa_threshold,
                params.smaa_search,
                0.0,
                0.0,
                params.viewport[0],
                params.viewport[1],
                params.viewport[2],
                params.viewport[3],
                0.25,
                0.0,
                0.0,
                0.0,
            ]),
        );
        let (smaa_area_view, smaa_area_texture) = create_smaa_table(
            device,
            queue,
            SMAA_AREA_TEX,
            wgpu::TextureFormat::Rg8Unorm,
            "SMAA area",
        );
        let (smaa_search_view, smaa_search_texture) = create_smaa_table(
            device,
            queue,
            SMAA_SEARCH_TEX,
            wgpu::TextureFormat::R8Unorm,
            "SMAA search",
        );
        let (neutral_weights, neutral_weights_texture) = create_neutral_weights(device, queue);
        Self {
            format,
            bind_group_layout,
            composite_layout,
            params_buffer,
            bloom_level_params: Vec::new(),
            bloom_threshold: params.bloom_threshold,
            bloom_intensity: params.bloom_intensity,
            bloom_radius: params.bloom_radius,
            viewport: params.viewport,
            sampler,
            pipeline,
            down_threshold_pipeline,
            down_pipeline,
            up_pipeline,
            composite_pipeline,
            intermediate: None,
            intermediate_view: None,
            levels: Vec::new(),
            size: (0, 0),
            bloom_enabled: params.bloom_enabled != 0.0,
            neutral_depth,
            _neutral_depth_texture: neutral_depth_texture,
            neutral_lut: neutral_lut.clone(),
            lut_views: [
                neutral_lut.clone(),
                neutral_lut.clone(),
                neutral_lut,
            ],
            _neutral_lut_texture: neutral_lut_texture,
            atlas_view: neutral_atlas.clone(),
            neutral_atlas,
            _neutral_atlas_texture: neutral_atlas_texture,
            smaa_layout,
            smaa_params_buffer,
            smaa_edge_pipeline,
            smaa_weight_pipeline,
            smaa_edges: None,
            smaa_edges_view: None,
            smaa_weights: None,
            smaa_weights_view: None,
            smaa_area_view,
            smaa_search_view,
            _smaa_area_texture: smaa_area_texture,
            _smaa_search_texture: smaa_search_texture,
            smaa_enabled: params.smaa_enabled != 0.0,
            neutral_weights,
            _neutral_weights_texture: neutral_weights_texture,
            arcane: None,
            arcane_rules: None,
            _arcane_output: None,
            arcane_output_view: None,
            pirate: None,
            pirate_rules: None,
            _dlaa_output: None,
            dlaa_output_view: None,
            dlaa: None,
            dlaa_rules: None,
        }
    }

    /// Select luluco250's `ArcaneBloom` as the bloom stage. `None` restores the
    /// plain threshold bloom (or no bloom at all).
    pub(crate) fn set_arcane_bloom(&mut self, rules: Option<ArcaneBloomRules>) {
        self.arcane_rules = rules;
        if rules.is_none() {
            self.arcane = None;
        }
    }

    /// Select `Pirate_Bloom`'s quarter-resolution chain. `None` drops it.
    pub(crate) fn set_pirate_bloom(&mut self, rules: Option<PirateBloomRules>) {
        self.pirate_rules = rules;
        if rules.is_none() {
            self.pirate = None;
        }
    }

    /// Select the spatial DLAA finish. `None` presents the chain output as is.
    pub(crate) fn set_dlaa(&mut self, rules: Option<DlaaRules>) {
        self.dlaa_rules = rules;
        if rules.is_none() {
            self.dlaa = None;
        }
    }

    /// Bind the loaded MultiLUT tables; `None` slots keep the neutral table.
    pub(crate) fn set_luts(&mut self, views: [Option<wgpu::TextureView>; 3]) {
        let mut bound = [
            self.neutral_lut.clone(),
            self.neutral_lut.clone(),
            self.neutral_lut.clone(),
        ];
        for (slot, view) in bound.iter_mut().zip(views) {
            if let Some(view) = view {
                *slot = view;
            }
        }
        self.lut_views = bound;
    }

    /// Bind the `MultiLUT` sheet; `None` keeps the neutral stand-in.
    pub(crate) fn set_atlas(&mut self, view: Option<wgpu::TextureView>) {
        self.atlas_view = view.unwrap_or_else(|| self.neutral_atlas.clone());
    }

    /// The colour format every target of this chain must use.
    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Update the uniform parameters without rebuilding the chain.
    pub(crate) fn set_params(&mut self, queue: &wgpu::Queue, params: PostParams) {
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(
            &self.smaa_params_buffer,
            0,
            bytemuck::cast_slice(&[
                params.smaa_threshold,
                params.smaa_search,
                0.0,
                0.0,
                params.viewport[0],
                params.viewport[1],
                params.viewport[2],
                params.viewport[3],
                0.25,
                0.0,
                0.0,
                0.0,
            ]),
        );
        self.smaa_enabled = params.smaa_enabled != 0.0;
        self.bloom_enabled = params.bloom_enabled != 0.0;
        self.bloom_threshold = params.bloom_threshold;
        self.bloom_intensity = params.bloom_intensity;
        self.bloom_radius = params.bloom_radius;
        self.viewport = params.viewport;
        for buffer in &self.bloom_level_params {
            queue.write_buffer(
                buffer,
                0,
                bytemuck::cast_slice(&[
                    params.bloom_threshold,
                    params.bloom_intensity,
                    params.bloom_radius,
                    0.0,
                ]),
            );
            queue.write_buffer(buffer, 32, bytemuck::cast_slice(&params.viewport));
        }
    }

    /// Draw `source` into `destination`, one texel per texel.
    ///
    /// With bloom configured this renders the effect pass into an intermediate
    /// frame, builds the four down/up sample levels, and composites the result;
    /// otherwise the effect pass writes straight to the destination.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        destination: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        self.draw_with_depth(device, queue, encoder, source, mask, None, destination, size);
    }

    /// As [`Self::draw`], with the battlefield depth the DOF/AO arms read.
    /// `None` binds a far-plane 1×1 depth so the layout stays complete.
    ///
    /// With DLAA configured the chain renders into its own frame and the
    /// antialiaser finishes into `destination`; the reference preset runs
    /// ArcaneBloom before DTLAA, so the bloom stage keeps its position.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_with_depth(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        destination: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let Some(rules) = self.dlaa_rules else {
            self.draw_chain(device, queue, encoder, source, mask, depth, destination, size);
            return;
        };
        self.ensure_targets(device, queue, size);
        let finale = self
            .dlaa_output_view
            .as_ref()
            .expect("targets exist after ensure_targets")
            .clone();
        self.draw_chain(device, queue, encoder, source, mask, depth, &finale, size);
        let dlaa = self
            .dlaa
            .get_or_insert_with(|| crate::render::dlaa::Dlaa::new(device, self.format));
        dlaa.prepare(device, queue, size, rules, self.viewport);
        dlaa.draw(device, encoder, &finale, destination);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_chain(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        destination: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let depth = depth.cloned().unwrap_or_else(|| self.neutral_depth.clone());
        // SMAA's two helper passes must finish before the effect pass reads the
        // weight map; they write full-frame RGBA8 targets.
        if self.smaa_enabled {
            self.ensure_smaa_targets(device, size);
            let edges = self
                .smaa_edges_view
                .as_ref()
                .expect("smaa targets exist after ensure_smaa_targets")
                .clone();
            let weights = self
                .smaa_weights_view
                .as_ref()
                .expect("smaa targets exist after ensure_smaa_targets")
                .clone();
            let group = self.smaa_bind_group(device, source);
            self.run_pass(
                encoder,
                &self.smaa_edge_pipeline,
                &group,
                &edges,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            let group = self.smaa_bind_group(device, &edges);
            self.run_pass(
                encoder,
                &self.smaa_weight_pipeline,
                &group,
                &weights,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
        }
        let weights = if self.smaa_enabled {
            self.smaa_weights_view
                .as_ref()
                .expect("smaa targets exist after ensure_smaa_targets")
                .clone()
        } else {
            self.neutral_weights.clone()
        };
        let bloom_stage = self.arcane_rules.is_some() || self.pirate_rules.is_some();
        let simple_bloom = self.bloom_enabled && !bloom_stage;
        if !bloom_stage && !simple_bloom {
            let bind_group = self.source_bind_group(device, source, mask, &depth, &weights);
            self.run_pass(
                encoder,
                &self.pipeline,
                &bind_group,
                destination,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            return;
        }

        // The graded frame feeds the bloom stages; DLAA runs after them in the
        // presentation wrapper.
        self.ensure_targets(device, queue, size);
        let intermediate = self
            .intermediate_view
            .as_ref()
            .expect("targets exist after ensure_targets")
            .clone();
        let bind_group = self.source_bind_group(device, source, mask, &depth, &weights);
        self.run_pass(
            encoder,
            &self.pipeline,
            &bind_group,
            &intermediate,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );

        let scene = intermediate.clone();

        if bloom_stage {
            // ArcaneBloom and Pirate_Bloom chain in the preset's order:
            // ArcaneBloom grades into a second frame and Pirate_Bloom
            // composites over that.
            let mut stage_output: Option<wgpu::TextureView> = None;
            if let Some(rules) = self.arcane_rules {
                let output = if self.pirate_rules.is_some() {
                    self.arcane_output_view
                        .as_ref()
                        .expect("targets exist after ensure_targets")
                        .clone()
                } else {
                    destination.clone()
                };
                let arcane = self.arcane.get_or_insert_with(|| {
                    crate::render::arcane_bloom::ArcaneBloom::new(device, self.format)
                });
                arcane.prepare(device, queue, size, rules, self.viewport);
                arcane.draw(device, encoder, &scene, &output);
                if self.pirate_rules.is_some() {
                    stage_output = Some(output);
                }
            } else {
                stage_output = Some(scene.clone());
            }
            if let Some(rules) = self.pirate_rules {
                let input = stage_output.as_ref().unwrap_or(&scene);
                let pirate = self.pirate.get_or_insert_with(|| {
                    crate::render::pirate_bloom::PirateBloom::new(device, self.format)
                });
                pirate.prepare(device, queue, size, rules, self.viewport);
                pirate.draw(device, encoder, input, destination);
            }
            return;
        }

        // Plain threshold bloom, reading the (possibly DLAA'd) frame.
        let threshold_group =
            self.bloom_bind_group(device, &scene, mask, &depth, &self.bloom_level_params[0]);
        self.run_pass(
            encoder,
            &self.down_threshold_pipeline,
            &threshold_group,
            &self.levels[0].view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        for index in 0..BLOOM_LEVELS - 1 {
            let params = &self.bloom_level_params[index + 1];
            let bind_group = self.bloom_bind_group(
                device,
                &self.levels[index].view,
                mask,
                &depth,
                params,
            );
            self.run_pass(
                encoder,
                &self.down_pipeline,
                &bind_group,
                &self.levels[index + 1].view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
        }
        for index in (1..BLOOM_LEVELS).rev() {
            let params = &self.bloom_level_params[index - 1];
            let bind_group = self.bloom_bind_group(
                device,
                &self.levels[index].view,
                mask,
                &depth,
                params,
            );
            // Additive: keep this level's downsample content and add the smaller
            // level's tent-filtered contribution on top.
            self.run_pass(
                encoder,
                &self.up_pipeline,
                &bind_group,
                &self.levels[index - 1].view,
                wgpu::LoadOp::Load,
            );
        }

        let composite_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post process bloom composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&scene),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.levels[0].view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.bloom_level_params[0].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.run_pass(
            encoder,
            &self.composite_pipeline,
            &composite_group,
            destination,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
    }

    fn source_bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        smaa: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post process source"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&self.lut_views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(smaa),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.lut_views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&self.lut_views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&self.atlas_view),
                },
            ],
        })
    }

    /// The SMAA helper passes' bind group: the frame (edge pass) or the edge
    /// map (weight pass), their uniform, and the two lookup tables.
    fn smaa_bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post process smaa source"),
            layout: &self.smaa_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.smaa_params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.smaa_area_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.smaa_search_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Rebuild SMAA's edge and weight maps when the frame size changes.
    fn ensure_smaa_targets(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if self.smaa_edges.is_some()
            && self
                .smaa_weights
                .as_ref()
                .is_some_and(|texture| (texture.width(), texture.height()) == size)
        {
            return;
        }
        let edges = create_map_texture(device, size, "Post process smaa edges");
        let weights = create_map_texture(device, size, "Post process smaa weights");
        self.smaa_edges_view = Some(edges.create_view(&wgpu::TextureViewDescriptor::default()));
        self.smaa_edges = Some(edges);
        self.smaa_weights_view =
            Some(weights.create_view(&wgpu::TextureViewDescriptor::default()));
        self.smaa_weights = Some(weights);
    }

    fn bloom_bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post process bloom source"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&self.lut_views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.neutral_weights),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.lut_views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&self.lut_views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&self.atlas_view),
                },
            ],
        })
    }

    fn run_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        view: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Post process"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Follow the frame size: the intermediate and the four bloom levels are
    /// rebuilt only when the size actually changes.
    fn ensure_targets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if self.intermediate.is_some() && size == self.size {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process intermediate"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.intermediate_view = Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        self.intermediate = Some(texture);

        // The second full-frame target ArcaneBloom writes when Pirate_Bloom
        // follows it.
        let arcane_output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process arcane output"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.arcane_output_view =
            Some(arcane_output.create_view(&wgpu::TextureViewDescriptor::default()));
        self._arcane_output = Some(arcane_output);

        // The frame the chain draws into when DLAA finishes the presentation.
        let dlaa_output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process DLAA output"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.dlaa_output_view =
            Some(dlaa_output.create_view(&wgpu::TextureViewDescriptor::default()));
        self._dlaa_output = Some(dlaa_output);

        self.levels.clear();
        let mut level = size;
        for _ in 0..BLOOM_LEVELS {
            level = ((level.0 / 2).max(1), (level.1 / 2).max(1));
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Post process bloom level"),
                size: wgpu::Extent3d {
                    width: level.0,
                    height: level.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.levels.push(BloomLevel {
                size: level,
                view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
                _texture: texture,
            });
        }

        self.bloom_level_params.clear();
        for level in &self.levels {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Post process bloom params"),
                size: 48,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(
                &buffer,
                0,
                bytemuck::cast_slice(&[
                    self.bloom_threshold,
                    self.bloom_intensity,
                    self.bloom_radius,
                    0.0,
                    level.size.0 as f32,
                    level.size.1 as f32,
                    0.0,
                    0.0,
                ]),
            );
            queue.write_buffer(&buffer, 32, bytemuck::cast_slice(&self.viewport));
            self.bloom_level_params.push(buffer);
        }
        self.size = size;
    }
}

/// A 1×1 depth view for chains with no battlefield depth to read. Its texel
/// stays at the cleared value, which keeps the DOF/AO arms inactive; the game
/// path always binds the real battlefield depth instead.
fn create_neutral_depth(device: &wgpu::Device) -> (wgpu::TextureView, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Post process neutral depth"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (view, texture)
}

/// A 3D colour table read as `texture_3d<f32>` for the LUT arm.
fn lut_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    }
}

/// A 1×1×1 mid-grey table bound when no `.cube` is loaded. The shader never
/// samples it unless `lut_enabled` is set, which the caller only does with a
/// real table.
fn create_neutral_lut(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Post process neutral LUT"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[128, 128, 128, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (view, texture)
}

/// A 1×1 neutral sheet bound at the `MultiLUT` atlas slot when no sheet is
/// loaded. The arm never samples it unless `atlas_enabled` is set, which the
/// caller only does with a real sheet.
fn create_neutral_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Post process neutral atlas"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[128, 128, 128, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (view, texture)
}

/// A 1×1 zero weight map bound at the SMAA slot when the effect is off.
fn create_neutral_weights(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Texture) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Post process neutral weights"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[0, 0, 0, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (view, texture)
}

/// A full-frame RGBA8 working map for SMAA's helper passes.
fn create_map_texture(device: &wgpu::Device, size: (u32, u32), label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// Upload one of SMAA's uncompressed `.dds` tables (128-byte header, then mip
/// 0 rows). The shader's coordinates assume the table's exact size, so the
/// size is read from the header rather than hard-coded.
fn create_smaa_table(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    dds: &[u8],
    format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::TextureView, wgpu::Texture) {
    assert!(
        dds.len() >= 128 && &dds[..4] == b"DDS ",
        "{label}: not a DDS payload"
    );
    let height = u32::from_le_bytes(dds[12..16].try_into().expect("DDS height"));
    let width = u32::from_le_bytes(dds[16..20].try_into().expect("DDS width"));
    let bytes_per_texel = format
        .block_copy_size(None)
        .expect("SMAA's tables are uncompressed") as usize;
    let row = width as usize * bytes_per_texel;
    let padded = row.div_ceil(256) * 256;
    let payload = &dds[128..];
    let mut bytes = Vec::with_capacity(padded * height as usize);
    for y in 0..height as usize {
        let start = y * row;
        bytes.extend_from_slice(&payload[start..start + row]);
        bytes.resize(bytes.len() + (padded - row), 0);
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(padded as u32),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (view, texture)
}

/// A loaded `.cube` table, kept with its texture for GPU ownership.
pub(crate) struct PostLut {
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
}

impl PostLut {
    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Parse Resolve/Adobe `.cube` text into a trilinear 3D table.
    /// `None` for malformed input or a size outside 2..=64.
    pub(crate) fn from_cube(device: &wgpu::Device, queue: &wgpu::Queue, text: &str) -> Option<Self> {
        let mut size = 0usize;
        let mut domain_min = [0.0f32; 3];
        let mut domain_max = [1.0f32; 3];
        let mut values: Vec<[f32; 3]> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(head) = parts.next() else { continue };
            match head {
                "LUT_3D_SIZE" => size = parts.next()?.parse().ok()?,
                "LUT_1D_SIZE" | "TITLE" => {}
                "DOMAIN_MIN" => {
                    for lane in &mut domain_min {
                        *lane = parts.next()?.parse().ok()?;
                    }
                }
                "DOMAIN_MAX" => {
                    for lane in &mut domain_max {
                        *lane = parts.next()?.parse().ok()?;
                    }
                }
                _ => {
                    let r: f32 = head.parse().ok()?;
                    let g: f32 = parts.next()?.parse().ok()?;
                    let b: f32 = parts.next()?.parse().ok()?;
                    values.push([r, g, b]);
                }
            }
        }
        if !(2..=64).contains(&size) || values.len() != size * size * size {
            return None;
        }
        let mut bytes = Vec::with_capacity(size * size * size * 4);
        let lane = |value: f32, min: f32, max: f32| {
            let span = (max - min).abs().max(1e-6);
            (((value - min) / span).clamp(0.0, 1.0) * 255.0) as u8
        };
        for [r, g, b] in values {
            bytes.extend_from_slice(&[
                lane(r, domain_min[0], domain_max[0]),
                lane(g, domain_min[1], domain_max[1]),
                lane(b, domain_min[2], domain_max[2]),
                255,
            ]);
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Post process LUT"),
            size: wgpu::Extent3d {
                width: size as u32,
                height: size as u32,
                depth_or_array_layers: size as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Rows must be 256-byte aligned, so the table goes through a padded
        // buffer copy (Queue::write_texture enforces the same alignment).
        let row_bytes = size * 4;
        let padded_row = row_bytes.div_ceil(256) * 256;
        let mut padded = Vec::with_capacity(padded_row * size * size);
        for slice in 0..size {
            for row in 0..size {
                let start = (slice * size + row) * row_bytes;
                padded.extend_from_slice(&bytes[start..start + row_bytes]);
                padded.resize(padded.len() + (padded_row - row_bytes), 0);
            }
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Post process LUT upload"),
            contents: &padded,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Post process LUT upload"),
        });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row as u32),
                    rows_per_image: Some(size as u32),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: size as u32,
                height: size as u32,
                depth_or_array_layers: size as u32,
            },
        );
        queue.submit([encoder.finish()]);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Some(Self {
            view,
            _texture: texture,
        })
    }
}

/// The `MultiLUT` atlas sheet, kept with its texture for GPU ownership.
pub(crate) struct PostAtlas {
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
}

impl PostAtlas {
    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The sheet the renderer ships for `Lut.Atlas` with an empty
    /// `Lut.AtlasFile`.
    pub(crate) fn builtin(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        Self::from_png(device, queue, MULTILUT_ATLAS)
    }

    /// Decode a `MultiLut_atlas1.png`-shaped sheet (32x32 tiles, 32 across,
    /// 17 rows) into an RGBA8 texture. `None` for undecodable input.
    pub(crate) fn from_png(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
    ) -> Option<Self> {
        let sheet = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (width, height) = (sheet.width(), sheet.height());
        if width == 0 || height == 0 {
            return None;
        }
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Post process MultiLUT atlas"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            sheet.as_raw(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Some(Self {
            view,
            _texture: texture,
        })
    }
}

fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A depth attachment read as `texture_depth_2d` for the DOF/AO arms.
fn depth_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn post_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entry: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{BLOOM_COMPOSITE_SRC, BLOOM_SHADER_SRC, SHADER_SRC, SMAA_SHADER_SRC};

    fn shader_parses(source: &str, label: &str) {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(source).expect(label);
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect(label);
    }

    #[test]
    fn the_builtin_multilut_sheet_has_the_expected_shape() {
        let sheet = image::load_from_memory(super::MULTILUT_ATLAS)
            .expect("decode the built-in MultiLUT sheet")
            .to_rgba8();
        // 32x32 tiles, 32 across, 17 stacked LUTs.
        assert_eq!((sheet.width(), sheet.height()), (1024, 544));
    }

    #[test]
    fn post_process_shaders_compile() {
        shader_parses(SHADER_SRC, "the post-process shader");
        shader_parses(BLOOM_SHADER_SRC, "the bloom shader");
        shader_parses(BLOOM_COMPOSITE_SRC, "the bloom composite shader");
        shader_parses(SMAA_SHADER_SRC, "the SMAA helper shader");
    }

    #[test]
    fn the_identity_params_disable_every_effect() {
        let params = super::PostParams::identity();
        assert_eq!(
            bytemuck::cast::<_, [f32; 88]>(params),
            [
                0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.125, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                0.0, 2.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 1.0,
                0.0, 0.0, 0.0, 0.0,
                0.035, 0.0, 0.0, 0.0,
                1.0, 1.0, 0.5, 0.2,
                0.0, 0.0, 0.0, 0.0,
                0.02, 0.0, 0.0, 0.0,
                2.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.1, 8.0, 0.0, 0.0,
                4.0, 0.02, 1.0, 0.0,
                6.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
            ]
        );
    }

    #[test]
    fn from_effects_enables_only_the_configured_arms() {
        // Every part disabled: the caller keeps the texture copy.
        assert!(super::PostParams::from_effects(
            None, None, None, None, None, None, None, None, Default::default(), None, false, [0.0; 4]
        )
        .is_none());

        let grade = super::PostParams::from_effects(
            Some([0.1, 1.2, 0.5]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
            None,
            false,
            [0.0; 4],
        )
        .expect("colour grade configures the chain");
        assert_eq!(
            bytemuck::cast::<_, [f32; 88]>(grade),
            [
                0.1, 1.2, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.125, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                0.0, 2.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 1.0,
                0.0, 0.0, 0.0, 0.0,
                0.035, 0.0, 0.0, 0.0,
                1.0, 1.0, 0.5, 0.2,
                0.0, 0.0, 0.0, 0.0,
                0.02, 0.0, 0.0, 0.0,
                2.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.1, 8.0, 0.0, 0.0,
                4.0, 0.02, 1.0, 0.0,
                6.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
            ],
            "grade-only leaves the ReShade arms disabled"
        );

        let all = super::PostParams::from_effects(
            None,
            Some([0.85, 0.4, 1.0, 0.5, 0.0, -0.15]),
            Some([0.4, 0.7]),
            Some([0.58, 0.55, 0.43, 0.42]),
            Some([0.25, 0.125, 0.0]),
            Some([2.0, 0.6]),
            Some([0.5, 0.05]),
            Some(0.7),
            Default::default(),
            Some([0.5, 0.4, 1.0]),
            false,
            [0.0, 0.0, 800.0, 600.0],
        )
        .expect("ReShade arms configure the chain");
        assert_eq!(
            bytemuck::cast::<_, [f32; 88]>(all),
            [
                0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 800.0, 600.0, 0.85, 0.4, 1.0, 0.5, 0.0, -0.15,
                1.0, 0.0, 0.4, 0.7, 1.0, 1.0, 0.58, 0.55, 0.43, 0.42, 0.25, 0.125, 0.0, 1.0, 0.5,
                0.4, 1.0, 1.0, 0.0, 2.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 1.0,
                2.0, 0.6, 1.0, 0.5,
                0.05, 1.0, 0.7, 1.0,
                1.0, 1.0, 0.5, 0.2,
                0.0, 0.0, 0.0, 0.0,
                0.02, 0.0, 0.0, 0.0,
                2.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.1, 8.0, 0.0, 0.0,
                4.0, 0.02, 1.0, 0.0,
                6.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
            ]
        );
    }
}

#[cfg(test)]
#[path = "post_process_tests.rs"]
mod gpu_tests;
