//! Bridge body artwork and the original indexed shadow stencils.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::overlay_atlas::{OverlaySpriteEntry, stored_frame_offset};
use crate::rules::art_data::{self, ArtRegistry};
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::{OverlayTypeFlags, OverlayTypeRegistry, is_high_bridge_index};

const SPRITE_PADDING: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BridgeFrameKind {
    Body,
    Shadow,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BridgeAtlasKey {
    pub name: String,
    pub frame: u8,
    pub kind: BridgeFrameKind,
}

pub struct BridgeAtlas {
    pub texture: BatchTexture,
    entries: HashMap<BridgeAtlasKey, OverlaySpriteEntry>,
}

impl BridgeAtlas {
    pub fn body_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
        self.entries.get(&BridgeAtlasKey {
            name: name.to_string(),
            frame,
            kind: BridgeFrameKind::Body,
        })
    }

    pub fn shadow_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
        self.entries.get(&BridgeAtlasKey {
            name: name.to_string(),
            frame,
            kind: BridgeFrameKind::Shadow,
        })
    }
}

/// Atlas lookup interface — abstracts the GPU-backed `BridgeAtlas` so that
/// instance builders can be exercised in unit tests with a pure-data mock.
pub trait BridgeAtlasLookup {
    fn body_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry>;
    fn shadow_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry>;
}

impl BridgeAtlasLookup for BridgeAtlas {
    fn body_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
        BridgeAtlas::body_entry(self, name, frame)
    }

    fn shadow_entry(&self, name: &str, frame: u8) -> Option<&OverlaySpriteEntry> {
        BridgeAtlas::shadow_entry(self, name, frame)
    }
}

struct RenderedBridge {
    key: BridgeAtlasKey,
    rgba: Vec<u8>,
    source_indices: Vec<u8>,
    width: u32,
    height: u32,
    offset_x: f32,
    offset_y: f32,
}

pub fn is_high_bridge_body_name(name: &str) -> bool {
    ["BRIDGE1", "BRIDGEB1", "BRIDGE2", "BRIDGEB2"]
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

/// Route a live high-bridge body by the CellClass numeric identity first.
///
/// Native `OverlayClass::Mark @ 0x005FC570` dispatches the four high-anchor
/// setters by numeric OverlayType index, so a layered rules file may retain
/// high-bridge semantics under a noncanonical art name. Canonical names remain
/// accepted for the established destroy-band cells written by bridge runtime.
pub fn is_high_bridge_body_identity(overlay_id: u8, name: &str) -> bool {
    is_high_bridge_index(overlay_id) || is_high_bridge_body_name(name)
}

/// Every identity the bridge body and shadow builders may draw: each named
/// type that routes as a high-bridge body. Bridge repair writes 0x18/0x19/
/// 0xED/0xEE from the ramp family and direction (`sim::bridge_state::
/// ramp_repair`), and a crate image may name one, so map-authored cells alone
/// do not bound the set; gamemd loads every OverlayType's image.
fn needed_bridge_sprite_keys(overlay_names: &BTreeMap<u8, String>) -> HashSet<BridgeAtlasKey> {
    let names: HashSet<&String> = overlay_names
        .iter()
        .filter(|&(&overlay_id, name)| is_high_bridge_body_identity(overlay_id, name))
        .map(|(_, name)| name)
        .collect();
    let mut needed = HashSet::new();
    for name in names {
        for frame in 0u8..18u8 {
            needed.insert(BridgeAtlasKey {
                name: name.clone(),
                frame,
                kind: BridgeFrameKind::Body,
            });
            needed.insert(BridgeAtlasKey {
                name: name.clone(),
                frame,
                kind: BridgeFrameKind::Shadow,
            });
        }
    }
    needed
}

pub fn build_bridge_atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    overlay_names: &BTreeMap<u8, String>,
    asset_manager: &AssetManager,
    theater_palette: &Palette,
    unit_palette: &Palette,
    theater_ext: &str,
    theater_name: &str,
    overlay_registry: &OverlayTypeRegistry,
    rules_ini: &IniFile,
    art_registry: &ArtRegistry,
) -> Option<BridgeAtlas> {
    let needed = needed_bridge_sprite_keys(overlay_names);
    if needed.is_empty() {
        return None;
    }

    let mut rendered: Vec<RenderedBridge> = Vec::with_capacity(needed.len());
    for key in &needed {
        let flags: OverlayTypeFlags = overlay_registry
            .flags_by_name(&key.name)
            .cloned()
            .unwrap_or_default();
        let palette: &Palette = if flags.wall {
            unit_palette
        } else {
            theater_palette
        };
        if let Some(sprite) = render_bridge_sprite(
            asset_manager,
            palette,
            key,
            theater_ext,
            theater_name,
            rules_ini,
            art_registry,
            &flags,
        ) {
            rendered.push(sprite);
        }
    }
    if rendered.is_empty() {
        return None;
    }

    Some(pack_bridge_sprites(device, queue, batch, &rendered))
}

/// RGBA preview of the stencil. The shared destination-edit renderer consumes
/// the separately retained source indices and owns native darkening and Z.
fn shadow_stencil_to_rgba(stencil: &[u8]) -> Vec<u8> {
    let mut rgba: Vec<u8> = Vec::with_capacity(stencil.len() * 4);
    for &index in stencil {
        let alpha: u8 = if index == 0 { 0 } else { 255 };
        rgba.extend_from_slice(&[0, 0, 0, alpha]);
    }
    rgba
}

fn render_bridge_sprite(
    asset_manager: &AssetManager,
    palette: &Palette,
    key: &BridgeAtlasKey,
    theater_ext: &str,
    theater_name: &str,
    rules_ini: &IniFile,
    art_registry: &ArtRegistry,
    flags: &OverlayTypeFlags,
) -> Option<RenderedBridge> {
    let image_id: String = art_registry.resolve_overlay_image_id(&key.name, rules_ini);
    let mut candidates: Vec<String> = art_data::overlay_shp_candidates(
        Some(art_registry),
        &key.name,
        &image_id,
        theater_ext,
        theater_name,
    );
    if let Some(alias) = decrement_numeric_suffix(&key.name) {
        candidates.push(format!("{}.{}", alias, theater_ext));
        candidates.push(format!("{}.shp", alias));
        candidates.push(format!("{}.{}", alias.to_ascii_lowercase(), theater_ext));
        candidates.push(format!("{}.shp", alias.to_ascii_lowercase()));
    }

    let shp: ShpFile = candidates.iter().find_map(|name| {
        let data = asset_manager.get_ref(name)?;
        let shp = ShpFile::from_bytes(data).ok()?;
        let has_drawable = shp
            .frames
            .iter()
            .any(|fr| fr.frame_width > 0 && fr.frame_height > 0);
        has_drawable.then_some(shp)
    })?;

    // CellClass::DrawOverlay_Shadow @ 0047F510 indexes raw state + count/2.
    // Empty/out-of-range shadow frames draw nothing; no substitute artwork.
    // Keep the original cropped rectangle for CC_Draw_Shape's row walker.
    // Executable comparison: tools/spatial_oracle/bridge_shadow_render.py.
    if key.kind == BridgeFrameKind::Shadow {
        let frame_idx = shp.frames.len() / 2 + usize::from(key.frame);
        let frame = shp.frames.get(frame_idx)?;
        if frame.frame_width == 0 || frame.frame_height == 0 {
            return None;
        }
        let (offset_x, offset_y) = stored_frame_offset(
            shp.width,
            shp.height,
            frame.frame_x,
            frame.frame_y,
            flags.y_draw_offset(),
        );
        return Some(RenderedBridge {
            key: key.clone(),
            rgba: shadow_stencil_to_rgba(&frame.pixels),
            source_indices: frame.pixels.clone(),
            width: u32::from(frame.frame_width),
            height: u32::from(frame.frame_height),
            offset_x,
            offset_y,
        });
    }

    // Cell47F6A0 forwards the selected raw frame unchanged. Empty frames
    // draw nothing; never clamp or substitute another populated frame.
    let frame_idx = usize::from(key.frame);
    let frame = shp.frames.get(frame_idx)?;
    if frame.frame_width == 0 || frame.frame_height == 0 {
        return None;
    }
    let frame_rgba = shp.frame_to_rgba(frame_idx, palette).ok()?;
    let full_w: u32 = shp.width as u32;
    let full_h: u32 = shp.height as u32;
    let mut full_rgba: Vec<u8> = vec![0u8; (full_w * full_h * 4) as usize];
    let mut full_indices = vec![0u8; (full_w * full_h) as usize];

    let fw: u32 = frame.frame_width as u32;
    let fh: u32 = frame.frame_height as u32;
    let fx: u32 = frame.frame_x as u32;
    let fy: u32 = frame.frame_y as u32;
    for y in 0..fh {
        let dst_y: u32 = fy + y;
        if dst_y >= full_h {
            break;
        }
        let src_off: usize = (y * fw * 4) as usize;
        let dst_off: usize = ((dst_y * full_w + fx) * 4) as usize;
        let copy_w: u32 = fw.min(full_w.saturating_sub(fx));
        let bytes: usize = (copy_w * 4) as usize;
        if src_off + bytes <= frame_rgba.len() && dst_off + bytes <= full_rgba.len() {
            full_rgba[dst_off..dst_off + bytes]
                .copy_from_slice(&frame_rgba[src_off..src_off + bytes]);
            full_indices[dst_off / 4..dst_off / 4 + copy_w as usize]
                .copy_from_slice(&frame.pixels[src_off / 4..src_off / 4 + copy_w as usize]);
        }
    }

    Some(RenderedBridge {
        key: key.clone(),
        rgba: full_rgba,
        source_indices: full_indices,
        width: full_w,
        height: full_h,
        offset_x: -((full_w / 2) as f32),
        offset_y: -((full_h / 2) as f32) + flags.y_draw_offset(),
    })
}

fn pack_bridge_sprites(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    sprites: &[RenderedBridge],
) -> BridgeAtlas {
    let mut indices: Vec<usize> = (0..sprites.len()).collect();
    indices.sort_by(|&a, &b| sprites[b].height.cmp(&sprites[a].height));

    let total_area: u64 = sprites
        .iter()
        .map(|s| {
            (s.width as u64 + SPRITE_PADDING as u64) * (s.height as u64 + SPRITE_PADDING as u64)
        })
        .sum();
    let estimated_side: u32 = (total_area as f64).sqrt().ceil() as u32;
    let max_texture_dim: u32 = device.limits().max_texture_dimension_2d;
    let mut atlas_width: u32 = estimated_side.clamp(64, max_texture_dim);

    let placements: Vec<(usize, u32, u32)>;
    let atlas_height: u32;
    loop {
        let mut trial: Vec<(usize, u32, u32)> = Vec::with_capacity(sprites.len());
        let mut cx: u32 = 0;
        let mut cy: u32 = 0;
        let mut shelf_h: u32 = 0;
        for &idx in &indices {
            let w: u32 = sprites[idx].width;
            let h: u32 = sprites[idx].height;
            if cx + w > atlas_width {
                cy += shelf_h + SPRITE_PADDING;
                cx = 0;
                shelf_h = 0;
            }
            trial.push((idx, cx, cy));
            cx += w + SPRITE_PADDING;
            shelf_h = shelf_h.max(h);
        }
        let trial_height: u32 = trial
            .iter()
            .map(|&(idx, _, py)| py + sprites[idx].height)
            .max()
            .unwrap_or(1);
        if trial_height <= max_texture_dim {
            placements = trial;
            atlas_height = trial_height;
            break;
        }
        if atlas_width >= max_texture_dim {
            placements = trial;
            atlas_height = trial_height.min(max_texture_dim);
            break;
        }
        atlas_width = (atlas_width.saturating_mul(2)).min(max_texture_dim);
    }

    let mut rgba: Vec<u8> = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    let mut source_indices = vec![0u8; (atlas_width * atlas_height) as usize];
    let mut entries: HashMap<BridgeAtlasKey, OverlaySpriteEntry> =
        HashMap::with_capacity(placements.len());
    let aw: f32 = atlas_width as f32;
    let ah: f32 = atlas_height as f32;

    for &(idx, px, py) in &placements {
        let spr: &RenderedBridge = &sprites[idx];
        let w: u32 = spr.width;
        let h: u32 = spr.height;
        for y in 0..h {
            let src_start: usize = (y * w * 4) as usize;
            let src_end: usize = src_start + (w * 4) as usize;
            let dst_start: usize = (((py + y) * atlas_width + px) * 4) as usize;
            let dst_end: usize = dst_start + (w * 4) as usize;
            if src_end <= spr.rgba.len() && dst_end <= rgba.len() {
                rgba[dst_start..dst_end].copy_from_slice(&spr.rgba[src_start..src_end]);
                source_indices[dst_start / 4..dst_end / 4]
                    .copy_from_slice(&spr.source_indices[src_start / 4..src_end / 4]);
            }
        }
        entries.insert(
            spr.key.clone(),
            OverlaySpriteEntry {
                uv_origin: [px as f32 / aw, py as f32 / ah],
                uv_size: [w as f32 / aw, h as f32 / ah],
                pixel_size: [w as f32, h as f32],
                offset_x: spr.offset_x,
                offset_y: spr.offset_y,
            },
        );
    }

    let texture = batch.create_texture_on_device(
        device,
        queue,
        &rgba,
        atlas_width,
        atlas_height,
        Some(&source_indices),
    );
    BridgeAtlas { texture, entries }
}

fn decrement_numeric_suffix(name: &str) -> Option<String> {
    let split: usize = name.rfind(|c: char| !c.is_ascii_digit())?;
    if split + 1 >= name.len() {
        return None;
    }
    let (prefix, digits) = name.split_at(split + 1);
    let width: usize = digits.len();
    let n: u32 = digits.parse().ok()?;
    if n == 0 {
        return None;
    }
    Some(format!("{}{:0width$}", prefix, n - 1, width = width))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    #[test]
    fn body_and_shadow_keys_are_distinct() {
        let body = BridgeAtlasKey {
            name: "BRIDGE1".into(),
            frame: 5,
            kind: BridgeFrameKind::Body,
        };
        let shadow = BridgeAtlasKey {
            name: "BRIDGE1".into(),
            frame: 5,
            kind: BridgeFrameKind::Shadow,
        };
        assert_ne!(body, shadow);
    }

    #[test]
    fn every_high_bridge_identity_preloads_without_a_map_authored_cell() {
        // Repair and crate scatter can write any of the four high-bridge
        // indices; a renamed index still routes by number, and a canonical
        // name routes by name. Low bridges and plain types stay out.
        let mut rules_text = String::from("[OverlayTypes]\n");
        for id in 0..=0xEEu8 {
            let name = match id {
                0x18 => "HIGHANCHOR".to_string(),
                0x19 => "BRIDGE2".to_string(),
                0x30 => "BRIDGE1".to_string(),
                0x50 => "LOBRDG07".to_string(),
                0xED => "BRIDGEB1".to_string(),
                0xEE => "BRIDGEB2".to_string(),
                _ => format!("OV{id:03}"),
            };
            writeln!(&mut rules_text, "{id}={name}").unwrap();
        }
        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(&rules_text), None);

        let keys = needed_bridge_sprite_keys(&crate::render::overlay_assets::render_overlay_names(
            &registry,
        ));

        let names = ["HIGHANCHOR", "BRIDGE2", "BRIDGE1", "BRIDGEB1", "BRIDGEB2"];
        assert_eq!(keys.len(), names.len() * 36);
        for name in names {
            for frame in 0..18 {
                for kind in [BridgeFrameKind::Body, BridgeFrameKind::Shadow] {
                    assert!(keys.contains(&BridgeAtlasKey {
                        name: name.to_string(),
                        frame,
                        kind,
                    }));
                }
            }
        }
    }

    #[test]
    fn shadow_frames_become_a_black_stencil_not_palette_colour() {
        // A shadow frame carries only 0 (nothing) and non-zero (shadow). The
        // native blitter TESTs the byte and halves the destination; it never
        // reads a colour, so any palette lookup here is wrong by construction.
        let rgba = shadow_stencil_to_rgba(&[0, 1, 0, 4, 255]);
        assert_eq!(rgba.len(), 5 * 4);
        // Index 0 → fully transparent, destination untouched.
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0]);
        // Every non-zero index → the same black darken texel, whatever its value.
        for offset in [4usize, 12, 16] {
            assert_eq!(
                &rgba[offset..offset + 4],
                &[0, 0, 0, 255],
                "non-zero stencil byte at {offset} must darken, not tint"
            );
        }
    }
}
