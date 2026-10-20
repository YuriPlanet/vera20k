//! Owned, serialization-only evidence leaves for tactical capture.
//!
//! These types snapshot production observations for the immutable manifest.
//! They do not select assets, mutate app/simulation state, or interpret native
//! parity. `session.rs` owns the richer stable/run objects assembled from them.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::app::presentation::render::{GameRenderInstanceCounts, GameRenderOutput};
use crate::render::bit_font::BitFontCaptureObservation;
use crate::render::gpu::GpuAdapterObservation;
use crate::render::sidebar_chrome::{
    ResolvedSidebarChromeIdentity, SidebarChromeAssetIdentity, SidebarChromeAtlasIdentity,
    SidebarTheme,
};
use crate::ui::sidebar::Rect;

use super::integrity::FileDigest;

/// Stable identity of one filesystem input observed by the capture.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactEvidence {
    pub(crate) path: String,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

impl ArtifactEvidence {
    pub(crate) fn from_path_digest(path: &Path, digest: &FileDigest) -> Self {
        Self {
            path: evidence_path(path),
            byte_length: digest.byte_length,
            sha256: digest.sha256.clone(),
        }
    }
}

fn evidence_path(path: &Path) -> String {
    let displayed = path.display().to_string();
    #[cfg(windows)]
    {
        displayed.replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        displayed
    }
}

/// Owned JSON representation of the exact adapter selected by `GpuContext`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct GpuAdapterEvidence {
    pub(crate) name: String,
    pub(crate) vendor: u32,
    pub(crate) device: u32,
    pub(crate) device_type: String,
    pub(crate) driver: String,
    pub(crate) driver_info: String,
    pub(crate) backend: String,
}

impl GpuAdapterEvidence {
    pub(crate) fn from_observation(observation: GpuAdapterObservation<'_>) -> Self {
        Self {
            name: observation.name.to_owned(),
            vendor: observation.vendor,
            device: observation.device,
            device_type: device_type_name(observation.device_type).to_owned(),
            driver: observation.driver.to_owned(),
            driver_info: observation.driver_info.to_owned(),
            backend: backend_name(observation.backend).to_owned(),
        }
    }
}

fn device_type_name(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::Other => "Other",
        wgpu::DeviceType::IntegratedGpu => "IntegratedGpu",
        wgpu::DeviceType::DiscreteGpu => "DiscreteGpu",
        wgpu::DeviceType::VirtualGpu => "VirtualGpu",
        wgpu::DeviceType::Cpu => "Cpu",
    }
}

fn backend_name(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Noop => "Noop",
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Metal => "Metal",
        wgpu::Backend::Dx12 => "Dx12",
        wgpu::Backend::Gl => "Gl",
        wgpu::Backend::BrowserWebGpu => "BrowserWebGpu",
    }
}

/// Production GAME.FNT lookup, matched to the bytes consumed by BitFont.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct BitmapFontSourceEvidence {
    pub(crate) logical_name: String,
    pub(crate) source_archive: String,
    pub(crate) entry_id: i32,
    pub(crate) payload_byte_length: u64,
    pub(crate) payload_sha256: String,
}

/// The atlas actually installed in the shared production bitmap-font owner.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct BitmapFontEvidence {
    pub(crate) source: BitmapFontSourceEvidence,
    pub(crate) atlas_width: u32,
    pub(crate) atlas_height: u32,
    pub(crate) glyph_count: u64,
    pub(crate) cell_height: u32,
    pub(crate) bitmap_rows: u32,
    pub(crate) missing_glyph_present: bool,
    pub(crate) darken_texture_present: bool,
}

impl BitmapFontEvidence {
    pub(crate) fn from_observations(
        font: BitFontCaptureObservation<'_>,
        source: BitmapFontSourceEvidence,
    ) -> Result<Self> {
        let consumed = font.source_identity.context(
            "tactical capture requires retail GAME.FNT; built-in bitmap font is not admitted",
        )?;
        ensure!(
            consumed.byte_length == source.payload_byte_length
                && consumed.sha256 == source.payload_sha256,
            "production bitmap atlas consumed different bytes from the selected GAME.FNT asset"
        );
        let [atlas_width, atlas_height] = font
            .atlas_dimensions
            .context("production bitmap atlas is not populated")?;
        ensure!(
            atlas_width > 0
                && atlas_height > 0
                && font.glyph_count > 0
                && font.cell_height > 0
                && font.bitmap_rows > 0
                && font.missing_glyph_present
                && font.darken_texture_present,
            "production bitmap font is not ready for tactical text"
        );
        Ok(Self {
            source,
            atlas_width,
            atlas_height,
            glyph_count: u64::try_from(font.glyph_count)
                .context("bitmap glyph count exceeds u64")?,
            cell_height: font.cell_height,
            bitmap_rows: font.bitmap_rows,
            missing_glyph_present: font.missing_glyph_present,
            darken_texture_present: font.darken_texture_present,
        })
    }
}

/// Stable graphics inputs and identities required by the wrapper validator.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct GraphicsEvidence {
    pub(crate) adapter: GpuAdapterEvidence,
    pub(crate) surface_format: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) window_scale_factor: f64,
    pub(crate) app_ui_scale: f64,
    pub(crate) bitmap_font: BitmapFontEvidence,
}

impl GraphicsEvidence {
    pub(crate) fn from_observations(
        adapter: GpuAdapterObservation<'_>,
        window_scale_factor: f64,
        surface_format: impl Into<String>,
        render_extent: [u32; 2],
        app_ui_scale: f32,
        bitmap_font: BitmapFontEvidence,
    ) -> Result<Self> {
        ensure!(
            render_extent[0] > 0 && render_extent[1] > 0,
            "tactical graphics extent must be nonzero"
        );
        ensure!(
            window_scale_factor.is_finite() && window_scale_factor > 0.0,
            "tactical window scale factor must be finite and positive"
        );
        ensure!(
            app_ui_scale.is_finite() && app_ui_scale > 0.0,
            "tactical app UI scale must be finite and positive"
        );
        let surface_format = surface_format.into();
        ensure!(
            !surface_format.is_empty(),
            "tactical surface format identity must be nonempty"
        );

        Ok(Self {
            adapter: GpuAdapterEvidence::from_observation(adapter),
            surface_format,
            width: render_extent[0],
            height: render_extent[1],
            window_scale_factor,
            app_ui_scale: f64::from(app_ui_scale),
            bitmap_font,
        })
    }
}

/// One logical sidebar asset and the archive selected by production lookup.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SidebarAssetEvidence {
    pub(crate) logical_name: String,
    pub(crate) source_archive: Option<String>,
}

impl From<&SidebarChromeAssetIdentity> for SidebarAssetEvidence {
    fn from(identity: &SidebarChromeAssetIdentity) -> Self {
        Self {
            logical_name: identity.logical_name.clone(),
            source_archive: identity.source_archive.clone(),
        }
    }
}

/// Requested-versus-resolved production sidebar source identity.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SidebarSourceEvidence {
    pub(crate) requested_theme: String,
    pub(crate) actual_theme: String,
    pub(crate) atlas_theme: String,
    pub(crate) parent_archive: String,
    pub(crate) radar: SidebarAssetEvidence,
    pub(crate) theme_palette: SidebarAssetEvidence,
    pub(crate) generic_palette: SidebarAssetEvidence,
    pub(crate) backgrounds: Option<[SidebarAssetEvidence; 3]>,
}

impl SidebarSourceEvidence {
    pub(crate) fn from_identity(identity: &ResolvedSidebarChromeIdentity) -> Self {
        let atlas = &identity.atlas;
        Self {
            requested_theme: sidebar_theme_name(identity.requested_theme).to_owned(),
            actual_theme: sidebar_theme_name(identity.actual_theme).to_owned(),
            atlas_theme: sidebar_theme_name(atlas.atlas_theme).to_owned(),
            parent_archive: atlas.parent_archive.clone(),
            radar: SidebarAssetEvidence::from(&atlas.radar),
            theme_palette: SidebarAssetEvidence::from(&atlas.theme_palette),
            generic_palette: SidebarAssetEvidence::from(&atlas.generic_palette),
            backgrounds: background_evidence(atlas),
        }
    }
}

fn background_evidence(atlas: &SidebarChromeAtlasIdentity) -> Option<[SidebarAssetEvidence; 3]> {
    atlas.backgrounds.as_ref().map(|backgrounds| {
        [
            SidebarAssetEvidence::from(&backgrounds[0]),
            SidebarAssetEvidence::from(&backgrounds[1]),
            SidebarAssetEvidence::from(&backgrounds[2]),
        ]
    })
}

fn sidebar_theme_name(theme: SidebarTheme) -> &'static str {
    match theme {
        SidebarTheme::Allied => "Allied",
        SidebarTheme::Soviet => "Soviet",
        SidebarTheme::Yuri => "Yuri",
    }
}

/// Finite render-space rectangle recorded without taking layout authority.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RenderRectEvidence {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl RenderRectEvidence {
    pub(crate) fn from_rect(rect: Rect) -> Result<Self> {
        let values = [rect.x, rect.y, rect.w, rect.h];
        ensure!(
            values.iter().all(|value| value.is_finite()),
            "tactical render rectangle must be finite"
        );
        Ok(Self {
            x: f64::from(rect.x),
            y: f64::from(rect.y),
            width: f64::from(rect.w),
            height: f64::from(rect.h),
        })
    }
}

/// Valid minimap aperture observed inside the final render extent.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApertureEvidence {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl ApertureEvidence {
    pub(crate) fn from_rect(rect: Rect, render_extent: [u32; 2]) -> Result<Self> {
        let rect = RenderRectEvidence::from_rect(rect)?;
        ensure!(
            render_extent[0] > 0 && render_extent[1] > 0,
            "tactical aperture render extent must be nonzero"
        );
        ensure!(
            rect.x >= 0.0
                && rect.y >= 0.0
                && rect.width > 0.0
                && rect.height > 0.0
                && rect.x + rect.width <= f64::from(render_extent[0])
                && rect.y + rect.height <= f64::from(render_extent[1]),
            "tactical minimap aperture is outside the final render extent"
        );
        Ok(Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        })
    }
}

/// Counts of the exact sidebar vectors uploaded and drawn by `render_game`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RenderInstanceCountEvidence {
    pub(crate) minimap: u64,
    pub(crate) viewport_rect: u64,
    pub(crate) radar_animation: u64,
}

impl RenderInstanceCountEvidence {
    pub(crate) fn from_counts(counts: GameRenderInstanceCounts) -> Result<Self> {
        Ok(Self {
            minimap: u64::try_from(counts.minimap)
                .context("tactical minimap instance count exceeds u64")?,
            viewport_rect: u64::try_from(counts.viewport_rect)
                .context("tactical viewport-rectangle instance count exceeds u64")?,
            radar_animation: u64::try_from(counts.radar_animation)
                .context("tactical radar-animation instance count exceeds u64")?,
        })
    }
}

/// Final sidebar/render observation built from the production render output.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SidebarRenderEvidence {
    pub(crate) sidebar_view_present: bool,
    pub(crate) sidebar_panel: Option<RenderRectEvidence>,
    pub(crate) minimap_aperture: ApertureEvidence,
    pub(crate) radar_content_insets: [u32; 4],
    pub(crate) instance_counts: RenderInstanceCountEvidence,
    pub(crate) sidebar_text_instances: u64,
}

impl SidebarRenderEvidence {
    pub(crate) fn from_render_output(
        output: &GameRenderOutput,
        minimap_aperture: Rect,
        radar_content_insets: [u32; 4],
        render_extent: [u32; 2],
    ) -> Result<Self> {
        let sidebar_panel = output
            .sidebar_view
            .as_ref()
            .map(|view| RenderRectEvidence::from_rect(view.panel_rect))
            .transpose()?;
        Ok(Self {
            sidebar_view_present: output.sidebar_view.is_some(),
            sidebar_panel,
            minimap_aperture: ApertureEvidence::from_rect(minimap_aperture, render_extent)?,
            radar_content_insets,
            instance_counts: RenderInstanceCountEvidence::from_counts(output.instance_counts)?,
            sidebar_text_instances: u64::try_from(output.instance_counts.sidebar_text)
                .context("tactical sidebar-text instance count exceeds u64")?,
        })
    }
}

/// Deterministic final state fingerprint recorded at the capture checkpoint.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FinalFingerprint {
    pub(crate) simulation_tick: u64,
    pub(crate) total_simulation_ms: u64,
    pub(crate) binary_frame: u32,
    pub(crate) deterministic_state_hash: u64,
}

/// Build the manifest evidence envelope with exactly two object-valued keys.
pub(crate) fn build_evidence(stable: Value, run: Value) -> Result<Value> {
    ensure!(
        stable.is_object(),
        "tactical stable evidence must be a JSON object"
    );
    ensure!(
        run.is_object(),
        "tactical run evidence must be a JSON object"
    );
    let mut evidence = Map::with_capacity(2);
    evidence.insert("stable".to_owned(), stable);
    evidence.insert("run".to_owned(), run);
    ensure!(
        evidence.len() == 2,
        "tactical evidence envelope must contain two keys"
    );
    Ok(Value::Object(evidence))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_evidence_rejects_fallback_unpopulated_atlas_and_mismatched_bytes() {
        let source = BitmapFontSourceEvidence {
            logical_name: "GAME.FNT".to_owned(),
            source_archive: "fixture.mix".to_owned(),
            entry_id: 1,
            payload_byte_length: 17,
            payload_sha256: "01".repeat(32),
        };
        let identity = crate::assets::fnt_file::FntSourceIdentity {
            byte_length: 17,
            sha256: source.payload_sha256.clone(),
        };
        let observation = |source_identity, atlas_dimensions| BitFontCaptureObservation {
            source_identity,
            atlas_dimensions,
            glyph_count: 1,
            cell_height: 17,
            bitmap_rows: 16,
            missing_glyph_present: true,
            darken_texture_present: true,
        };
        assert!(
            BitmapFontEvidence::from_observations(
                observation(None, Some([32, 16])),
                source.clone()
            )
            .is_err()
        );
        assert!(
            BitmapFontEvidence::from_observations(
                observation(Some(&identity), None),
                source.clone()
            )
            .is_err()
        );
        let mut wrong = source.clone();
        wrong.payload_sha256 = "02".repeat(32);
        assert!(
            BitmapFontEvidence::from_observations(
                observation(Some(&identity), Some([32, 16])),
                wrong
            )
            .is_err()
        );
        let evidence = BitmapFontEvidence::from_observations(
            observation(Some(&identity), Some([32, 16])),
            source.clone(),
        )
        .unwrap();
        assert_eq!(evidence.source, source);
        assert_eq!(evidence.glyph_count, 1);
    }

    #[test]
    fn evidence_envelope_has_only_stable_and_run_objects() {
        let value = build_evidence(
            serde_json::json!({"fingerprint": 1}),
            serde_json::json!({"process_id": 2}),
        )
        .expect("valid evidence");
        let object = value.as_object().expect("evidence object");

        assert_eq!(object.len(), 2);
        assert!(object.contains_key("stable"));
        assert!(object.contains_key("run"));
        assert!(build_evidence(Value::Null, serde_json::json!({})).is_err());
        assert!(build_evidence(serde_json::json!({}), Value::Null).is_err());
    }
}
