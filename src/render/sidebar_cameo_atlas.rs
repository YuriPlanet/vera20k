//! Sidebar cameo atlas.
//!
//! Loads build-palette cameo SHPs, or Ares PCXs with their embedded palettes,
//! and packs them into one GPU texture for the sidebar's ordinary batch.

use std::collections::HashMap;
use std::path::Path;

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::rules::ruleset::RuleSet;

const CAMEO_PADDING: u32 = 2;
const DEBUG_SAMPLE_COUNT: usize = 8;
const DEBUG_CELL_PADDING: u32 = 8;
const DEBUG_LABEL_HEIGHT: u32 = 18;

#[derive(Debug, Clone, Copy)]
pub struct SidebarCameoEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    /// Visible, alpha-cropped texture dimensions stored in the atlas.
    pub pixel_size: [f32; 2],
    /// Original SHP canvas dimensions before transparent borders were cropped.
    pub canvas_size: [f32; 2],
    /// Top-left of the visible crop in original-canvas pixels.
    pub crop_origin: [f32; 2],
}

pub struct SidebarCameoAtlas {
    pub texture: BatchTexture,
    entries: HashMap<String, SidebarCameoEntry>,
    /// Each type's `AltCameo=` image that loaded, by type id.
    alt_entries: HashMap<String, SidebarCameoEntry>,
    /// Per-SuperWeaponType icons, independent of object IDs and filenames.
    superweapon_entries: HashMap<String, SidebarCameoEntry>,
}

impl SidebarCameoAtlas {
    pub fn superweapon(&self, type_id: &str) -> Option<&SidebarCameoEntry> {
        self.superweapon_entries.get(&type_id.to_ascii_uppercase())
    }

    pub fn get(&self, type_id: &str) -> Option<&SidebarCameoEntry> {
        self.entries.get(&type_id.to_ascii_uppercase())
    }

    /// Loaded-art admission for the shared alternate-cameo selector.
    pub fn has_alternate(&self, type_id: &str) -> bool {
        self.alt_entries.contains_key(&type_id.to_ascii_uppercase())
    }

    /// The cameo `TechnoTypeClass::GetCameo @ 0x00712040` returns: the
    /// type's `AltCameo=` image when `alt` holds and it loaded
    /// (`pAltCameo != NULL`), else its plain cameo.
    pub fn cameo(&self, type_id: &str, alt: bool) -> Option<&SidebarCameoEntry> {
        let key = type_id.to_ascii_uppercase();
        alt.then(|| self.alt_entries.get(&key))
            .flatten()
            .or_else(|| self.entries.get(&key))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CameoKind {
    Ordinary,
    Promoted,
    SuperWeapon,
}

struct RenderedCameo {
    type_id: String,
    kind: CameoKind,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    canvas_width: u32,
    canvas_height: u32,
    crop_x: u32,
    crop_y: u32,
}

pub fn build_sidebar_cameo_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &AssetManager,
    rules: &RuleSet,
    palette: &Palette,
) -> Option<SidebarCameoAtlas> {
    let rendered = collect_sidebar_cameos(asset_manager, rules, palette);
    if rendered.is_empty() {
        log::warn!("Sidebar cameo atlas: no cameo art found");
        return None;
    }
    let atlas = pack_cameos(gpu, batch, &rendered)?;
    log::info!(
        "Sidebar cameo atlas built: {} entries, {}x{} px",
        atlas.entries.len() + atlas.superweapon_entries.len(),
        atlas.texture.width,
        atlas.texture.height
    );
    Some(atlas)
}

fn collect_sidebar_cameos(
    asset_manager: &AssetManager,
    rules: &RuleSet,
    palette: &Palette,
) -> Vec<RenderedCameo> {
    let mut rendered: Vec<RenderedCameo> = Vec::new();
    let type_ids = rules
        .building_ids
        .iter()
        .chain(rules.infantry_ids.iter())
        .chain(rules.vehicle_ids.iter())
        .chain(rules.aircraft_ids.iter());

    for type_id in type_ids {
        if let Some(cameo) = render_cameo(type_id, asset_manager, rules, palette) {
            rendered.push(cameo);
        }
        if let Some(cameo) = render_alt_cameo(type_id, asset_manager, rules, palette) {
            rendered.push(cameo);
        }
    }

    // Ares1005E410 selects a surface owned by this SuperWeaponType's extension.
    // A shared invalid PCX filename may have a different SHP fallback per type.
    for sw in rules.super_weapons.values() {
        if let Some(cameo) = render_superweapon_cameo(sw, asset_manager, palette) {
            rendered.push(cameo);
        }
    }
    rendered
}

/// Export a side-by-side palette comparison sheet for a few cameos.
///
/// This is an empirical debugging tool: when sidebar art looks washed out, a
/// single PNG with the same cameo rendered under several palettes is faster and
/// more reliable than guessing palette order from memory.
pub fn export_debug_palette_sheet(
    asset_manager: &AssetManager,
    rules: &RuleSet,
    output_path: &Path,
    palette_names: &[&str],
) {
    let sample_type_ids: Vec<&str> = rules
        .building_ids
        .iter()
        .chain(rules.infantry_ids.iter())
        .chain(rules.vehicle_ids.iter())
        .chain(rules.aircraft_ids.iter())
        .take(DEBUG_SAMPLE_COUNT)
        .map(String::as_str)
        .collect();
    if sample_type_ids.is_empty() || palette_names.is_empty() {
        return;
    }

    let mut palette_rows: Vec<(&str, Vec<RenderedCameo>)> = Vec::new();
    let mut max_cameo_w = 1u32;
    let mut max_cameo_h = 1u32;
    for &palette_name in palette_names {
        let Some(data) = asset_manager.get_ref(palette_name) else {
            continue;
        };
        let Ok(palette) = Palette::from_bytes(data) else {
            continue;
        };
        let mut row: Vec<RenderedCameo> = Vec::new();
        for type_id in &sample_type_ids {
            if let Some(cameo) = render_cameo(type_id, asset_manager, rules, &palette) {
                max_cameo_w = max_cameo_w.max(cameo.width);
                max_cameo_h = max_cameo_h.max(cameo.height);
                row.push(cameo);
            }
        }
        if !row.is_empty() {
            palette_rows.push((palette_name, row));
        }
    }
    if palette_rows.is_empty() {
        return;
    }

    let cols = sample_type_ids.len() as u32;
    let rows = palette_rows.len() as u32;
    let cell_w = max_cameo_w + DEBUG_CELL_PADDING * 2;
    let cell_h = max_cameo_h + DEBUG_CELL_PADDING * 2 + DEBUG_LABEL_HEIGHT;
    let canvas_w = cols * cell_w;
    let canvas_h = rows * cell_h;
    let mut rgba = vec![32u8; (canvas_w * canvas_h * 4) as usize];
    for px in rgba.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }

    for (row_idx, (palette_name, cameos)) in palette_rows.iter().enumerate() {
        log::info!("Sidebar cameo debug row for palette {}", palette_name);
        for (col_idx, cameo) in cameos.iter().enumerate() {
            let cell_x = col_idx as u32 * cell_w;
            let cell_y = row_idx as u32 * cell_h;
            let origin_x = cell_x + DEBUG_CELL_PADDING;
            let origin_y = cell_y + DEBUG_CELL_PADDING;
            blit_rgba(
                &mut rgba,
                canvas_w,
                canvas_h,
                origin_x,
                origin_y,
                &cameo.rgba,
                cameo.width,
                cameo.height,
            );
        }
    }

    if let Some(img) = image::RgbaImage::from_raw(canvas_w, canvas_h, rgba) {
        if let Err(err) = img.save(output_path) {
            log::warn!(
                "Failed to save sidebar cameo palette debug sheet {}: {}",
                output_path.display(),
                err
            );
        } else {
            log::info!(
                "Saved sidebar cameo palette debug sheet {} ({}x{})",
                output_path.display(),
                canvas_w,
                canvas_h
            );
        }
    }
}

fn render_cameo(
    type_id: &str,
    asset_manager: &AssetManager,
    rules: &RuleSet,
    palette: &Palette,
) -> Option<RenderedCameo> {
    let rules_image = rules
        .object(type_id)
        .map(|obj| obj.image.clone())
        .unwrap_or_else(|| type_id.to_string());
    #[cfg(feature = "ares")]
    if let [Some(name), _] = rules
        .art()
        .resolve_declared_pcx_cameos(type_id, &rules_image)
        && let Some(cameo) = render_pcx_cameo(type_id, name, asset_manager)
    {
        return Some(cameo);
    }
    let resolved_image = rules
        .art()
        .resolve_effective_image_id(type_id, &rules_image);
    let resolved_cameo = rules.art().resolve_declared_cameo_id(type_id, &rules_image);

    let mut candidates: Vec<String> = Vec::with_capacity(8);
    push_unique(&mut candidates, format!("{resolved_cameo}.SHP"));
    if !resolved_image.eq_ignore_ascii_case(&resolved_cameo) {
        push_unique(&mut candidates, format!("{resolved_image}.SHP"));
    }
    candidates.extend(legacy_cameo_fallback_candidates(
        type_id,
        &resolved_image,
        &resolved_cameo,
    ));

    let (data, file_name) = candidates
        .iter()
        .find_map(|name| asset_manager.get_ref(name).map(|data| (data, name.clone())))?;
    let cameo = render_shp_frame0(type_id, data, palette)?;
    log::debug!("Sidebar cameo {} loaded from {}", type_id, file_name);
    Some(cameo)
}

/// A type's `AltCameo=` image: `<AltCameo>.SHP`, read from the type's image
/// section only (`TechnoTypeClass::ReadINI`, section `+0x1F8` at
/// `0x00715A73`), with no fallback when it is missing.
fn render_alt_cameo(
    type_id: &str,
    asset_manager: &AssetManager,
    rules: &RuleSet,
    palette: &Palette,
) -> Option<RenderedCameo> {
    let rules_image = rules
        .object(type_id)
        .map(|obj| obj.image.clone())
        .unwrap_or_else(|| type_id.to_string());
    // Ares3.0 Cameos1005E430..1005E470: use promoted PCX only if loaded;
    // otherwise the ordinary PCX still takes precedence over an AltCameo SHP.
    #[cfg(feature = "ares")]
    {
        let [normal, promoted] = rules
            .art()
            .resolve_declared_pcx_cameos(type_id, &rules_image);
        for name in [promoted, normal].into_iter().flatten() {
            if let Some(mut cameo) = render_pcx_cameo(type_id, name, asset_manager) {
                cameo.kind = CameoKind::Promoted;
                return Some(cameo);
            }
        }
    }
    let alt_cameo = rules
        .art()
        .resolve_declared_alt_cameo_id(type_id, &rules_image)?;
    let shp_name = format!("{alt_cameo}.SHP");
    let mut cameo = render_shp_frame0(type_id, asset_manager.get_ref(&shp_name)?, palette)?;
    cameo.kind = CameoKind::Promoted;
    log::debug!("Sidebar alt cameo {} loaded from {}", type_id, shp_name);
    Some(cameo)
}

/// Load a superweapon sidebar image SHP directly by name.
///
/// SW sidebar images are standalone SHP files (e.g. `BOLTICON.SHP`) in
/// `cameo(md).mix`, not referenced in art.ini. No image/cameo resolution needed.
fn render_superweapon_cameo(
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    asset_manager: &AssetManager,
    palette: &Palette,
) -> Option<RenderedCameo> {
    let key = &sw.id;
    #[cfg(feature = "ares")]
    if let Some(name) = sw.sidebar_pcx()
        && let Some(mut cameo) = render_pcx_cameo(key, name, asset_manager)
    {
        cameo.kind = CameoKind::SuperWeapon;
        return Some(cameo);
    }
    let image_name = sw.sidebar_image.as_deref()?;
    let shp_name = format!("{image_name}.SHP");
    let mut cameo = render_shp_frame0(key, asset_manager.get_ref(&shp_name)?, palette)?;
    cameo.kind = CameoKind::SuperWeapon;
    log::debug!("SW sidebar cameo {} loaded from {}", image_name, shp_name);
    Some(cameo)
}

/// Ares3.0 Cameos1005E32D..1005E366 / gamemd PCX blit6BA580: embedded
/// palette, then RGB(255,0,255) keyed in the packed surface-word domain.
/// Normal, promoted and superweapon consumers share this decoder. Ares supplies
/// a60x48 destination canvas;6BA5EF..600 uses source dimensions without scaling.
/// No house remap or cameo.pal applies.
/// https://ares-developers.github.io/Ares-docs/new/pcxcameos.html
#[cfg(feature = "ares")]
fn render_pcx_cameo(
    key: &str,
    filename: &str,
    asset_manager: &AssetManager,
) -> Option<RenderedCameo> {
    let image =
        crate::assets::pcx_file::PcxFile::from_bytes(asset_manager.get_ref(filename)?).ok()?;
    if !image.is_indexed() {
        return None;
    }
    let mut rgba = image.to_rgba(None);
    crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
        .apply_packed_color_key_rgba8(&mut rgba, [255, 0, 255]);
    log::debug!("Sidebar PCX cameo {key} loaded from {filename}");
    // A loaded surface remains selected even if every word is keyed out.
    // Cropping it to None would incorrectly select the legacy SHP instead.
    Some(RenderedCameo {
        type_id: key.to_ascii_uppercase(),
        kind: CameoKind::Ordinary,
        rgba,
        width: u32::from(image.width),
        height: u32::from(image.height),
        canvas_width: 60,
        canvas_height: 48,
        crop_x: 0,
        crop_y: 0,
    })
}

/// Frame 0 of a cameo SHP on its full canvas, keyed by `key`.
fn render_shp_frame0(key: &str, data: &[u8], palette: &Palette) -> Option<RenderedCameo> {
    let shp = ShpFile::from_bytes(data).ok()?;
    if shp.frames.is_empty() {
        return None;
    }
    let frame_rgba = shp.frame_to_rgba(0, palette).ok()?;
    let frame = &shp.frames[0];
    let full_w = shp.width as u32;
    let full_h = shp.height as u32;
    if full_w == 0 || full_h == 0 {
        return None;
    }
    let mut full_rgba = vec![0u8; (full_w * full_h * 4) as usize];
    blit_frame_into_full_bounds(&mut full_rgba, full_w, full_h, frame, &frame_rgba);
    rendered_cameo_from_canvas(key, full_rgba, full_w, full_h)
}

/// Repo-only fallback guesses kept out of `rules::art_data`.
///
/// `needs verification`: these guesses match current repo behavior and common RA2
/// sidebar conventions, but they are not part of exact art-data resolution.
fn legacy_cameo_fallback_candidates(
    type_id: &str,
    resolved_image: &str,
    resolved_cameo: &str,
) -> Vec<String> {
    let upper_type = type_id.to_ascii_uppercase();
    let mut candidates: Vec<String> = Vec::with_capacity(8);

    if !resolved_cameo.to_ascii_uppercase().ends_with("ICON") {
        push_unique(&mut candidates, format!("{resolved_cameo}ICON.SHP"));
    }
    push_unique(&mut candidates, format!("{resolved_image}ICON.SHP"));

    if !upper_type.eq_ignore_ascii_case(resolved_cameo)
        && !upper_type.eq_ignore_ascii_case(resolved_image)
    {
        push_unique(&mut candidates, format!("{upper_type}ICON.SHP"));
        push_unique(&mut candidates, format!("{upper_type}.SHP"));
    }

    for name in [&upper_type, resolved_image] {
        if name.len() > 2 {
            let prefix = &name[..2];
            if matches!(prefix, "GA" | "NA" | "YA" | "CA") {
                push_unique(&mut candidates, format!("{}ICON.SHP", &name[2..]));
            }
        }
    }

    candidates
}

fn push_unique(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn blit_frame_into_full_bounds(
    full_rgba: &mut [u8],
    full_w: u32,
    full_h: u32,
    frame: &crate::assets::shp_file::ShpFrame,
    frame_rgba: &[u8],
) {
    let fw = frame.frame_width as u32;
    let fh = frame.frame_height as u32;
    let fx = frame.frame_x as u32;
    let fy = frame.frame_y as u32;
    for y in 0..fh {
        let dst_y = fy + y;
        if dst_y >= full_h {
            break;
        }
        let src_start = (y * fw * 4) as usize;
        let dst_start = ((dst_y * full_w + fx) * 4) as usize;
        let copy_w = fw.min(full_w.saturating_sub(fx));
        let byte_count = (copy_w * 4) as usize;
        if src_start + byte_count <= frame_rgba.len() && dst_start + byte_count <= full_rgba.len() {
            full_rgba[dst_start..dst_start + byte_count]
                .copy_from_slice(&frame_rgba[src_start..src_start + byte_count]);
        }
    }
}

fn rendered_cameo_from_canvas(
    type_id: &str,
    full_rgba: Vec<u8>,
    canvas_width: u32,
    canvas_height: u32,
) -> Option<RenderedCameo> {
    let (rgba, width, height, crop_x, crop_y) =
        crop_visible_bounds(&full_rgba, canvas_width, canvas_height)?;
    Some(RenderedCameo {
        type_id: type_id.to_ascii_uppercase(),
        kind: CameoKind::Ordinary,
        rgba,
        width,
        height,
        canvas_width,
        canvas_height,
        crop_x,
        crop_y,
    })
}

fn crop_visible_bounds(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Option<(Vec<u8>, u32, u32, u32, u32)> {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;

    for y in 0..height {
        for x in 0..width {
            let alpha = rgba[((y * width + x) * 4 + 3) as usize];
            if alpha == 0 {
                continue;
            }
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    if !found {
        return None;
    }

    let cropped_w = max_x - min_x + 1;
    let cropped_h = max_y - min_y + 1;
    let mut cropped = vec![0u8; (cropped_w * cropped_h * 4) as usize];

    for y in 0..cropped_h {
        let src_start = (((min_y + y) * width + min_x) * 4) as usize;
        let dst_start = (y * cropped_w * 4) as usize;
        let byte_count = (cropped_w * 4) as usize;
        cropped[dst_start..dst_start + byte_count]
            .copy_from_slice(&rgba[src_start..src_start + byte_count]);
    }

    Some((cropped, cropped_w, cropped_h, min_x, min_y))
}

fn blit_rgba(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    dst_x: u32,
    dst_y: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
) {
    for y in 0..src_h {
        let out_y = dst_y + y;
        if out_y >= dst_h {
            break;
        }
        let src_start = (y * src_w * 4) as usize;
        let dst_start = ((out_y * dst_w + dst_x) * 4) as usize;
        let copy_w = src_w.min(dst_w.saturating_sub(dst_x));
        let byte_count = (copy_w * 4) as usize;
        if src_start + byte_count <= src.len() && dst_start + byte_count <= dst.len() {
            dst[dst_start..dst_start + byte_count]
                .copy_from_slice(&src[src_start..src_start + byte_count]);
        }
    }
}

fn pack_cameos(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    cameos: &[RenderedCameo],
) -> Option<SidebarCameoAtlas> {
    let mut indices: Vec<usize> = (0..cameos.len()).collect();
    indices.sort_by(|&a, &b| cameos[b].height.cmp(&cameos[a].height));

    let total_area: u64 = cameos
        .iter()
        .map(|cameo| {
            (cameo.width as u64 + CAMEO_PADDING as u64)
                * (cameo.height as u64 + CAMEO_PADDING as u64)
        })
        .sum();
    let max_texture_dim = gpu.device.limits().max_texture_dimension_2d;
    let widest_cameo = cameos.iter().map(|cameo| cameo.width).max().unwrap_or(1);
    if widest_cameo > max_texture_dim {
        log::error!("Sidebar cameo width {widest_cameo} exceeds GPU limit {max_texture_dim}");
        return None;
    }
    let mut atlas_width =
        ((total_area as f64).sqrt().ceil() as u32).clamp(widest_cameo.max(64), max_texture_dim);
    let placements;
    let atlas_height;

    loop {
        let (trial_placements, trial_height) = shelf_pack(cameos, &indices, atlas_width);
        if trial_height <= max_texture_dim {
            placements = trial_placements;
            atlas_height = trial_height;
            break;
        }
        if atlas_width >= max_texture_dim {
            log::error!(
                "Sidebar cameo atlas exceeds GPU limit even at max width: height={} limit={}",
                trial_height,
                max_texture_dim
            );
            return None;
        }
        atlas_width = (atlas_width.saturating_mul(2)).min(max_texture_dim);
    }
    let mut rgba = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    let mut entries = HashMap::with_capacity(cameos.len());
    let mut alt_entries = HashMap::new();
    let mut superweapon_entries = HashMap::new();
    let aw = atlas_width as f32;
    let ah = atlas_height as f32;

    for (idx, px, py) in placements {
        let cameo = &cameos[idx];
        for y in 0..cameo.height {
            let src_start = (y * cameo.width * 4) as usize;
            let dst_start = (((py + y) * atlas_width + px) * 4) as usize;
            let byte_count = (cameo.width * 4) as usize;
            rgba[dst_start..dst_start + byte_count]
                .copy_from_slice(&cameo.rgba[src_start..src_start + byte_count]);
        }
        let target = match cameo.kind {
            CameoKind::Ordinary => &mut entries,
            CameoKind::Promoted => &mut alt_entries,
            CameoKind::SuperWeapon => &mut superweapon_entries,
        };
        target.insert(
            cameo.type_id.clone(),
            SidebarCameoEntry {
                uv_origin: [px as f32 / aw, py as f32 / ah],
                uv_size: [cameo.width as f32 / aw, cameo.height as f32 / ah],
                pixel_size: [cameo.width as f32, cameo.height as f32],
                canvas_size: [cameo.canvas_width as f32, cameo.canvas_height as f32],
                crop_origin: [cameo.crop_x as f32, cameo.crop_y as f32],
            },
        );
    }

    Some(SidebarCameoAtlas {
        texture: batch.create_texture(gpu, &rgba, atlas_width, atlas_height),
        entries,
        alt_entries,
        superweapon_entries,
    })
}

fn shelf_pack(
    cameos: &[RenderedCameo],
    indices: &[usize],
    atlas_width: u32,
) -> (Vec<(usize, u32, u32)>, u32) {
    let mut placements: Vec<(usize, u32, u32)> = Vec::with_capacity(cameos.len());
    let mut cursor_x = 0;
    let mut cursor_y = 0;
    let mut shelf_height = 0;
    for &idx in indices {
        let cameo = &cameos[idx];
        if cursor_x + cameo.width > atlas_width {
            cursor_y += shelf_height + CAMEO_PADDING;
            cursor_x = 0;
            shelf_height = 0;
        }
        placements.push((idx, cursor_x, cursor_y));
        cursor_x += cameo.width + CAMEO_PADDING;
        shelf_height = shelf_height.max(cameo.height);
    }
    let atlas_height = placements
        .iter()
        .map(|(idx, _, py)| py + cameos[*idx].height)
        .max()
        .unwrap_or(1);
    (placements, atlas_height)
}

#[cfg(test)]
mod tests {
    use super::{legacy_cameo_fallback_candidates, rendered_cameo_from_canvas};

    #[test]
    fn test_legacy_cameo_fallbacks_start_after_declared_candidates() {
        let candidates = legacy_cameo_fallback_candidates("GACNST", "GACNST", "CIVICON");
        assert_eq!(candidates[0], "GACNSTICON.SHP");
        assert!(candidates.contains(&"CNSTICON.SHP".to_string()));
        assert!(!candidates.contains(&"CIVICONICON.SHP".to_string()));
    }

    #[test]
    fn test_rendered_cameo_retains_canvas_and_nonzero_crop_origin() {
        let canvas_width = 6;
        let canvas_height = 5;
        let mut rgba = vec![0u8; (canvas_width * canvas_height * 4) as usize];
        for y in 2..=3 {
            for x in 1..=4 {
                let offset = ((y * canvas_width + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[10, 20, 30, 255]);
            }
        }

        let cameo =
            rendered_cameo_from_canvas("testicon", rgba, canvas_width, canvas_height).unwrap();

        assert_eq!(cameo.type_id, "TESTICON");
        assert_eq!([cameo.canvas_width, cameo.canvas_height], [6, 5]);
        assert_eq!([cameo.crop_x, cameo.crop_y], [1, 2]);
        assert_eq!([cameo.width, cameo.height], [4, 2]);
        assert_eq!(cameo.rgba.len(), 4 * 2 * 4);
    }

    // These run through the production rules/art reader and actual atlas
    // loader, with a body SHP trap reproducing the reported wrong-art fallback.
    fn cameo_rules() -> crate::rules::ruleset::RuleSet {
        use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, ruleset::RuleSet};
        let art = IniFile::from_str(
            "[TEST]\nCameoPCX=wrong-type.pcx\n\
                 [IMAGE]\nImage=BODY\nCameo=OLDICON\nAltCameo=OLDALT\nCameoPCX=Normal.PCX\nAltCameoPCX=Promoted.PCX\n\
                 [BODY]\nCameoPCX=wrong-body.pcx\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[BuildingTypes]\n0=TEST\n[TEST]\nImage=IMAGE\n"),
            &art,
        )
        .unwrap();
        // The production loader installs the metadata after the fixed ART
        // rules pass; fixed-art input alone does not publish visual metadata.
        rules.install_art_data(ArtRegistry::from_ini(&art));
        rules
    }

    fn write_old_cameos(dir: &crate::map::source::test_support::TestDirectory) {
        let mut shp = Vec::new();
        for word in [0u16, 2, 2, 1] {
            shp.extend_from_slice(&word.to_le_bytes());
        }
        let mut frame = [0u8; 24];
        frame[4..6].copy_from_slice(&2u16.to_le_bytes());
        frame[6..8].copy_from_slice(&2u16.to_le_bytes());
        frame[8] = 1;
        frame[20..24].copy_from_slice(&32u32.to_le_bytes());
        shp.extend_from_slice(&frame);
        shp.extend_from_slice(&[1; 4]);
        for name in ["oldicon.shp", "oldalt.shp", "body.shp"] {
            dir.write(name, &shp);
        }
    }

    fn write_pcx(
        dir: &crate::map::source::test_support::TestDirectory,
        name: &str,
        color: [u8; 3],
    ) {
        let mut bytes = vec![0u8; 128];
        bytes[0] = 0x0a;
        bytes[2] = 1;
        bytes[3] = 8;
        bytes[8..10].copy_from_slice(&59u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&47u16.to_le_bytes());
        bytes[65] = 1;
        bytes[66..68].copy_from_slice(&60u16.to_le_bytes());
        let mut pixels = vec![2u8; 60 * 48];
        pixels[..6].copy_from_slice(&[0, 1, 2, 3, 4, 5]);
        bytes.extend_from_slice(&pixels);
        bytes.push(0x0c);
        let mut palette = [0u8; 768];
        palette[0..3].copy_from_slice(&[10, 20, 30]); // index0 remains visible
        palette[3..6].copy_from_slice(&[255, 0, 255]); // F81F key
        palette[6..9].copy_from_slice(&color);
        palette[9..12].copy_from_slice(&[248, 3, 248]); // also F81F after packing
        palette[12..15].copy_from_slice(&[0, 0, 0]); // opaque black
        palette[15..18].copy_from_slice(&[248, 0, 240]); // F81E, not the key
        bytes.extend_from_slice(&palette);
        dir.write(name, &bytes);
    }

    #[test]
    fn cameo_pcx_feature_gate_embedded_palette_and_packed_key() {
        use crate::{
            assets::asset_manager::AssetManager, assets::pal_file::Palette,
            map::source::test_support::TestDirectory,
        };
        let dir = TestDirectory::new("cameo-pcx");
        write_old_cameos(&dir);
        write_pcx(&dir, "normal.pcx", [17, 41, 83]);
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let wrong_palette = Palette::from_rgb(|_| [200, 10, 180]);
        let cameo = super::render_cameo("TEST", &assets, &cameo_rules(), &wrong_palette).unwrap();
        #[cfg(feature = "ares")]
        {
            assert_eq!([cameo.canvas_width, cameo.canvas_height], [60, 48]);
            assert_eq!(&cameo.rgba[0..4], &[10, 20, 30, 255]);
            assert_eq!(&cameo.rgba[4..8], &[255, 0, 255, 0]);
            assert_eq!(&cameo.rgba[8..12], &[17, 41, 83, 255]);
            assert_eq!(cameo.rgba[15], 0, "packed F81F is transparent too");
            // Original6BA580 word control: F81F skips,0000 andF81E write.
            assert_eq!(&cameo.rgba[16..20], &[0, 0, 0, 255]);
            assert_eq!(cameo.rgba[23], 255);
        }
        #[cfg(not(feature = "ares"))]
        {
            assert_eq!([cameo.canvas_width, cameo.canvas_height], [2, 2]);
            assert_eq!(&cameo.rgba[0..4], &[200, 10, 180, 255]);
        }
    }

    #[cfg(feature = "ares")]
    #[test]
    fn promoted_pcx_falls_back_to_plain_pcx_before_legacy_alt_shp() {
        use crate::{
            assets::asset_manager::AssetManager, assets::pal_file::Palette,
            map::source::test_support::TestDirectory,
        };
        let dir = TestDirectory::new("cameo-promoted-pcx");
        write_old_cameos(&dir);
        write_pcx(&dir, "normal.pcx", [20, 40, 60]);
        let palette = Palette::from_rgb(|_| [1, 2, 3]);
        let rules = cameo_rules();
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let normal = super::render_alt_cameo("TEST", &assets, &rules, &palette).unwrap();
        assert_eq!(normal.kind, super::CameoKind::Promoted);
        assert_eq!([normal.canvas_width, normal.canvas_height], [60, 48]);
        assert_eq!(&normal.rgba[8..12], &[20, 40, 60, 255]);
        write_pcx(&dir, "promoted.pcx", [80, 90, 100]);
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let promoted = super::render_alt_cameo("TEST", &assets, &rules, &palette).unwrap();
        assert_eq!(promoted.kind, super::CameoKind::Promoted);
        assert_eq!(&promoted.rgba[8..12], &[80, 90, 100, 255]);
    }

    #[cfg(feature = "ares")]
    #[test]
    fn missing_and_invalid_pcx_keep_the_existing_shp_path() {
        use crate::{
            assets::asset_manager::AssetManager, assets::pal_file::Palette,
            map::source::test_support::TestDirectory,
        };
        let dir = TestDirectory::new("cameo-invalid-pcx");
        write_old_cameos(&dir);
        dir.write("normal.pcx", b"not a PCX");
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let palette = Palette::from_rgb(|_| [1, 2, 3]);
        let rules = cameo_rules();
        let normal = super::render_cameo("TEST", &assets, &rules, &palette).unwrap();
        let promoted = super::render_alt_cameo("TEST", &assets, &rules, &palette).unwrap();
        assert_eq!([normal.canvas_width, normal.canvas_height], [2, 2]);
        assert_eq!([promoted.canvas_width, promoted.canvas_height], [2, 2]);
    }

    #[test]
    fn superweapon_pcx_key_and_shp_fallback_share_the_projected_identity() {
        use crate::{
            assets::asset_manager::AssetManager,
            assets::pal_file::Palette,
            map::source::test_support::TestDirectory,
            rules::{ini_parser::IniFile, superweapon_type::SuperWeaponType},
        };
        let dir = TestDirectory::new("superweapon-pcx");
        write_old_cameos(&dir);
        let ini = IniFile::from_str(
            "[SW]\nType=PsychicReveal\nSidebarImage=oldicon\nSidebarPCX=Normal.PCX\n",
        );
        let sw = SuperWeaponType::from_ini_section("SW", ini.section("SW").unwrap());
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let palette = Palette::from_rgb(|_| [1, 2, 3]);
        let fallback = super::render_superweapon_cameo(&sw, &assets, &palette).unwrap();
        assert_eq!([fallback.canvas_width, fallback.canvas_height], [2, 2]);
        write_pcx(&dir, "normal.pcx", [30, 50, 70]);
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let selected = super::render_superweapon_cameo(&sw, &assets, &palette).unwrap();
        #[cfg(feature = "ares")]
        {
            assert_eq!(fallback.type_id, "SW");
            assert_eq!(selected.type_id, "SW");
            assert_eq!([selected.canvas_width, selected.canvas_height], [60, 48]);
            let ini = IniFile::from_str("[PCXONLY]\nType=PsychicReveal\nSidebarPCX=Normal.PCX\n");
            let pcx_only =
                SuperWeaponType::from_ini_section("PCXONLY", ini.section("PCXONLY").unwrap());
            assert!(super::render_superweapon_cameo(&pcx_only, &assets, &palette).is_some());
        }
        #[cfg(not(feature = "ares"))]
        {
            assert_eq!(selected.type_id, "SW");
            assert_eq!([selected.canvas_width, selected.canvas_height], [2, 2]);
        }
    }

    #[test]
    fn shared_missing_pcx_keeps_per_type_shp_and_object_namespaces() {
        use crate::{
            assets::{asset_manager::AssetManager, pal_file::Palette},
            map::source::test_support::TestDirectory,
            rules::{ini_parser::IniFile, ruleset::RuleSet},
        };
        let dir = TestDirectory::new("cameo-type-identities");
        write_old_cameos(&dir);
        let original = std::fs::read(dir.path().join("oldicon.shp")).unwrap();
        for (name, index) in [("object.shp", 1), ("red.shp", 2), ("blue.shp", 3)] {
            let mut shp = original.clone();
            let start = shp.len() - 4;
            shp[start..].fill(index);
            dir.write(name, &shp);
        }
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=SHARED\n[SuperWeaponTypes]\n0=SHARED\n1=OTHER\n\
             [SHARED]\nType=PsychicReveal\nSidebarImage=red\nSidebarPCX=missing.pcx\n\
             [OTHER]\nType=PsychicReveal\nSidebarImage=blue\nSidebarPCX=missing.pcx\n",
        );
        let art = IniFile::from_str("[SHARED]\nCameo=object\n");
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let palette = Palette::from_rgb(|index| match index {
            1 => [10, 20, 30],
            2 => [200, 20, 30],
            3 => [10, 20, 230],
            _ => [0, 0, 0],
        });
        let cameos = super::collect_sidebar_cameos(&assets, &rules, &palette);
        let pixel = |kind, id| {
            let cameo = cameos
                .iter()
                .find(|cameo| cameo.kind == kind && cameo.type_id == id)
                .unwrap();
            cameo.rgba[..4].to_vec()
        };
        assert_eq!(
            pixel(super::CameoKind::Ordinary, "SHARED"),
            [10, 20, 30, 255]
        );
        assert_eq!(
            pixel(super::CameoKind::SuperWeapon, "SHARED"),
            [200, 20, 30, 255]
        );
        assert_eq!(
            pixel(super::CameoKind::SuperWeapon, "OTHER"),
            [10, 20, 230, 255]
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn loaded_transparent_pcx_does_not_select_any_shp_fallback() {
        use crate::{
            assets::{asset_manager::AssetManager, pal_file::Palette},
            map::source::test_support::TestDirectory,
        };
        let dir = TestDirectory::new("cameo-transparent-pcx");
        write_old_cameos(&dir);
        write_pcx(&dir, "normal.pcx", [255, 0, 255]);
        let path = dir.path().join("normal.pcx");
        let mut bytes = std::fs::read(&path).unwrap();
        let palette = bytes.len() - 768;
        for rgb in bytes[palette..].chunks_exact_mut(3) {
            rgb.copy_from_slice(&[255, 0, 255]);
        }
        dir.write("normal.pcx", &bytes);
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let palette = Palette::from_rgb(|_| [1, 2, 3]);
        let rules = cameo_rules();
        for cameo in [
            super::render_cameo("TEST", &assets, &rules, &palette).unwrap(),
            super::render_alt_cameo("TEST", &assets, &rules, &palette).unwrap(),
        ] {
            assert_eq!([cameo.width, cameo.height], [60, 48]);
            assert!(cameo.rgba.chunks_exact(4).all(|pixel| pixel[3] == 0));
        }
    }

    #[cfg(feature = "ares")]
    #[test]
    fn pcx_source_dimensions_do_not_resize_the_native_cameo_canvas() {
        use crate::{
            assets::asset_manager::AssetManager, map::source::test_support::TestDirectory,
        };
        let dir = TestDirectory::new("cameo-pcx-dimensions");
        write_pcx(&dir, "wide.pcx", [20, 40, 60]);
        let bytes = std::fs::read(dir.path().join("wide.pcx")).unwrap();
        let mut wide = bytes[..128].to_vec();
        wide[8..10].copy_from_slice(&63u16.to_le_bytes());
        wide[66..68].copy_from_slice(&64u16.to_le_bytes());
        wide.extend(std::iter::repeat_n(2, 64 * 48));
        wide.extend_from_slice(&bytes[bytes.len() - 769..]);
        dir.write("wide.pcx", &wide);
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let cameo = super::render_pcx_cameo("TEST", "wide.pcx", &assets).unwrap();
        assert_eq!([cameo.width, cameo.height], [64, 48]);
        assert_eq!([cameo.canvas_width, cameo.canvas_height], [60, 48]);
    }

    #[cfg(feature = "ares")]
    #[test]
    fn original_pcx_surface_selectors_choose_the_same_loaded_art() {
        use crate::{
            assets::{asset_manager::AssetManager, pal_file::Palette},
            map::source::test_support::TestDirectory,
            rules::{ini_parser::IniFile, superweapon_type::SuperWeaponType},
        };
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/pcx_cameos.json",
        ))
        .unwrap();
        for (index, control) in native["selectors"].as_array().unwrap().iter().enumerate() {
            let dir = TestDirectory::new(&format!("pcx-native-selector-{index}"));
            write_old_cameos(&dir);
            if control["normal_surface"].as_u64().unwrap() != 0 {
                write_pcx(&dir, "normal.pcx", [20, 40, 60]);
            }
            if control["promoted_surface"].as_u64().unwrap() != 0 {
                write_pcx(&dir, "promoted.pcx", [80, 90, 100]);
            }
            let assets = AssetManager::from_loose_root_for_test(dir.path());
            let palette = Palette::from_rgb(|_| [1, 2, 3]);
            let rules = cameo_rules();
            let selected = if control["elite"].as_bool().unwrap() {
                super::render_alt_cameo("TEST", &assets, &rules, &palette)
            } else {
                super::render_cameo("TEST", &assets, &rules, &palette)
            }
            .unwrap();
            let surface = control["selected_surface"].as_u64().unwrap();
            if surface == 0 {
                assert_eq!([selected.width, selected.height], [2, 2], "{control}");
            } else {
                let color = if surface == control["normal_surface"].as_u64().unwrap() {
                    [20, 40, 60]
                } else {
                    [80, 90, 100]
                };
                assert_eq!(&selected.rgba[8..11], &color, "{control}");
            }
        }
        for (index, control) in native["superweapons"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let dir = TestDirectory::new(&format!("pcx-native-super-{index}"));
            write_old_cameos(&dir);
            if control["surface"].as_u64().unwrap() != 0 {
                write_pcx(&dir, "normal.pcx", [20, 40, 60]);
            }
            let ini = IniFile::from_str(
                "[SW]\nType=PsychicReveal\nSidebarPCX=normal.pcx\nSidebarImage=oldicon\n",
            );
            let sw = SuperWeaponType::from_ini_section("SW", ini.section("SW").unwrap());
            let assets = AssetManager::from_loose_root_for_test(dir.path());
            let palette = Palette::from_rgb(|_| [1, 2, 3]);
            let selected = super::render_superweapon_cameo(&sw, &assets, &palette).unwrap();
            let expected_width = if control["selected_surface"].as_u64().unwrap() == 0 {
                2
            } else {
                60
            };
            assert_eq!(selected.width, expected_width, "{control}");
            assert_eq!(selected.kind, super::CameoKind::SuperWeapon);
        }
    }

    #[test]
    fn original_pcx_palette_and_packed_word_writes_match_rgba_keying() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/pcx_cameos.json",
        ))
        .unwrap();
        let presentation = crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
        let format = presentation.format;
        for (rgb, word) in native["palette"]["rgb"]
            .as_array()
            .unwrap()
            .iter()
            .zip(native["palette"]["words"].as_array().unwrap())
        {
            let rgb = [
                rgb[0].as_u64().unwrap() as u8,
                rgb[1].as_u64().unwrap() as u8,
                rgb[2].as_u64().unwrap() as u8,
            ];
            assert_eq!(u64::from(format.pack_rgb8(rgb)), word.as_u64().unwrap());
        }
        for control in native["blits"].as_array().unwrap() {
            let source = control["source_words"].as_array().unwrap();
            let mut rgba: Vec<u8> = source
                .iter()
                .flat_map(|word| {
                    let rgb = format.unpack_rgb8(word.as_u64().unwrap() as u16);
                    [rgb[0], rgb[1], rgb[2], 255]
                })
                .collect();
            presentation.apply_packed_color_key_rgba8(&mut rgba, [255, 0, 255]);
            let pitch = control["destination_size"][0].as_u64().unwrap() as usize;
            let width = control["source_size"][0].as_u64().unwrap() as usize;
            let x = control["rectangle"][0].as_u64().unwrap() as usize;
            let y = control["rectangle"][1].as_u64().unwrap() as usize;
            for (index, pixel) in rgba.chunks_exact(4).enumerate() {
                let destination = (y + index / width) * pitch + x + index % width;
                let original_wrote =
                    control["after_words"][destination] != control["before_words"][destination];
                assert_eq!(
                    pixel[3] != 0,
                    original_wrote,
                    "{} pixel{index}",
                    control["name"]
                );
            }
        }
    }
}
