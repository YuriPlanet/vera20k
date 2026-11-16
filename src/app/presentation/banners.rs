//! Phobos `BannerClass::Render` (`New/Entity/BannerClass.cpp:56-100`) behind
//! `0x6D4B25 TacticalClass_Render_Banner` (`Misc/Hooks.UI.cpp:310-316`): each
//! tactical draw walks the session's banners, ticks the blink period and
//! draws the type's PCX, else its SHP (frame advancing per draw), else its
//! CSF text with the optional scenario-variable value.
//!
//! The session owns the instances ([`crate::sim::banner`]); this module owns
//! the blink phase (`Render` counts per drawn frame, so the phase is
//! presentation state) and the decoded art.

use std::collections::BTreeMap;

use crate::app::AppState;
use crate::assets::{
    asset_manager::AssetManager, pal_file::Palette, pcx_file::PcxFile, shp_file::ShpFile,
};
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;

/// The blink phase of one banner instance plus its copied facts
/// (`BannerClass.h:9-21` minus the scaled position, which the view percent
/// pair re-derives per frame).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BannerDisplay {
    pub(crate) id: i32,
    pub(crate) type_name: String,
    pub(crate) position_percent: [i32; 2],
    pub(crate) variable: i32,
    pub(crate) global_variable: bool,
    /// `BannerClass::Duration`: frames left shown; negative draws always.
    shown: i32,
    /// `BannerClass::Delay`: frames left hidden between periods.
    hidden: i32,
    /// `BannerClass::ShapeFrameIndex`, advancing once per drawn frame.
    shape_frame: u16,
}

impl BannerDisplay {
    fn new(
        id: i32,
        type_name: String,
        position_percent: [i32; 2],
        variable: i32,
        global: bool,
    ) -> Self {
        Self {
            id,
            type_name,
            position_percent,
            variable,
            global_variable: global,
            shown: 0,
            hidden: 0,
            shape_frame: 0,
        }
    }
}

/// One type's atlas window. A PCX type has one image; an SHP type lays its
/// frames left-to-right in one atlas row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BannerArtSlot {
    pub(crate) image_origin: [u32; 2],
    pub(crate) image_size: [u32; 2],
    /// SHP frames only: one origin per frame, in draw order.
    pub(crate) frame_origins: Vec<[u32; 2]>,
    /// SHP frames only: one size per frame, in draw order.
    pub(crate) frame_sizes: Vec<[u32; 2]>,
}

impl BannerArtSlot {
    fn pcx(origin: [u32; 2], size: [u32; 2]) -> Self {
        Self {
            image_origin: origin,
            image_size: size,
            frame_origins: Vec::new(),
            frame_sizes: Vec::new(),
        }
    }
}

/// Every banner type's art at match start. A type without a loadable PCX or
/// SHP carries no slot; its CSF arm can still draw.
pub(crate) struct BannerArtSet {
    texture: BatchTexture,
    atlas: [u32; 2],
    slots: BTreeMap<String, BannerArtSlot>,
}

/// One decoded image: pixels plus its atlas footprint.
struct DecodedImage {
    size: [u32; 2],
    rgba: Vec<u8>,
}

impl BannerArtSet {
    /// Decode every `[BannerTypes]` entry's PCX, else SHP (with its
    /// `SHP.Palette=`), into one atlas row per type. A type naming neither
    /// image, or naming an unreadable one, contributes no slot — like the
    /// native lazy surface that never materializes.
    pub(crate) fn build(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        types: &crate::rules::banner_type::BannerTypeCatalog,
    ) -> Option<Self> {
        let mut slots: Vec<(String, BannerArtSlot, Vec<DecodedImage>)> = Vec::new();
        let mut atlas_width = 0u32;
        let mut atlas_height = 0u32;
        for banner_type in types.types() {
            let images: Vec<DecodedImage> = if banner_type.pcx.is_some() {
                build_pcx(assets, banner_type).into_iter().collect()
            } else {
                build_shp(assets, banner_type).unwrap_or_default()
            };
            if images.is_empty() {
                continue;
            }
            let origin = [0, atlas_height];
            let width: u32 = images.iter().map(|image| image.size[0]).sum();
            let height = images.iter().map(|image| image.size[1]).max().unwrap_or(0);
            let frame_origins: Vec<[u32; 2]> = images
                .iter()
                .scan(0u32, |x, image| {
                    let origin = [*x, atlas_height];
                    *x += image.size[0];
                    Some(origin)
                })
                .collect();
            let frame_sizes: Vec<[u32; 2]> = images.iter().map(|image| image.size).collect();
            atlas_width = atlas_width.max(width);
            atlas_height += height;
            slots.push((
                banner_type.name.clone(),
                if banner_type.pcx.is_some() {
                    BannerArtSlot::pcx(origin, [width, height])
                } else {
                    BannerArtSlot {
                        image_origin: origin,
                        image_size: [width, height],
                        frame_origins,
                        frame_sizes,
                    }
                },
                images,
            ));
        }
        if slots.is_empty() || atlas_width == 0 || atlas_height == 0 {
            return None;
        }
        let mut rgba = vec![0u8; (atlas_width * atlas_height * 4) as usize];
        for (_, slot, images) in &slots {
            for (image, origin) in images.iter().zip(&slot.frame_origins) {
                for row in 0..image.size[1] {
                    for col in 0..image.size[0] {
                        let from = ((row * image.size[0] + col) * 4) as usize;
                        let to = (((origin[1] + row) * atlas_width + origin[0] + col) * 4) as usize;
                        rgba[to..to + 4].copy_from_slice(&image.rgba[from..from + 4]);
                    }
                }
            }
        }
        Some(Self {
            texture: batch.create_texture(gpu, &rgba, atlas_width, atlas_height),
            atlas: [atlas_width, atlas_height],
            slots: slots
                .into_iter()
                .map(|(name, slot, _)| (name, slot))
                .collect(),
        })
    }

    pub(crate) fn texture(&self) -> &BatchTexture {
        &self.texture
    }

    fn slot(&self, name: &str) -> Option<&BannerArtSlot> {
        self.slots.get(name)
    }
}

fn build_pcx(
    assets: &AssetManager,
    banner_type: &crate::rules::banner_type::BannerType,
) -> Option<DecodedImage> {
    let name = banner_type.pcx.as_deref()?;
    let bytes = assets
        .get_ref(name)
        .or_else(|| assets.get_ref(&format!("{name}.PCX")))?;
    let pcx = PcxFile::from_bytes(bytes).ok()?;
    let size = [u32::from(pcx.width), u32::from(pcx.height)];
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    Some(DecodedImage {
        size,
        rgba: pcx.to_rgba(None),
    })
}

fn build_shp(
    assets: &AssetManager,
    banner_type: &crate::rules::banner_type::BannerType,
) -> Option<Vec<DecodedImage>> {
    let shape = banner_type.shape.as_deref()?;
    let shp = assets
        .get_ref(shape)
        .and_then(|bytes| ShpFile::from_bytes(bytes).ok())?;
    if shp.frames.is_empty() {
        return None;
    }
    let palette = assets
        .get_ref(banner_type.palette.as_deref().unwrap_or("PALETTE.PAL"))
        .or_else(|| assets.get_ref("palette.pal"))
        .and_then(|bytes| Palette::from_bytes(bytes).ok())?;
    Some(
        shp.frames
            .iter()
            .enumerate()
            .filter_map(|(index, frame)| {
                let rgba = shp.frame_to_rgba(index, &palette).ok()?;
                Some(DecodedImage {
                    size: [u32::from(frame.frame_width), u32::from(frame.frame_height)],
                    rgba,
                })
            })
            .collect(),
    )
}

/// Tick every banner's blink phase and emit the draw instances
/// (`BannerClass::Render`). The shapes sample the shared banner atlas; the
/// texts bind the game font.
pub(crate) fn build_banner_instances(
    state: &mut AppState,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    // The read-only phase gathers everything the tick loop needs; the loop
    // then borrows the presentation's blink store mutably on its own.
    let banners: Vec<crate::sim::banner::RuntimeBanner> = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|sim| sim.simulation.session().banners().to_vec())
        .unwrap_or_default();
    if banners.is_empty() {
        return Default::default();
    }
    let banner_types = state.rules().map(|rules| rules.banner_types().clone());
    // `CSF.VariableFormat` prints the variable's extended `Value`
    // (`BannerClass.cpp:146-155`); `None` renders no text at all.
    let variables_values: Vec<Option<i32>> = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|sim| {
            banners
                .iter()
                .map(|banner| {
                    sim.simulation
                        .variable_value(banner.global_variable, banner.variable)
                })
                .collect()
        })
        .unwrap_or_default();
    let fallback_tint = crate::render::sidebar_text::credits_tint(
        crate::app::presentation::sidebar_render::current_sidebar_theme(state),
    )
    .map(|channel| (channel * 255.0).round() as u8);
    let csf = state.process_assets.csf.as_ref();
    let (view_width, view_height) = crate::app::input::camera::tactical_viewport_size(state);
    let view = [view_width as i32, view_height as i32];
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let font = &state.renderer.bit_font;

    let presentation = &mut state.match_state.match_presentation;
    reconcile_displays(&mut presentation.banners, &banners);

    let mut fills = Vec::new();
    let mut texts = Vec::new();
    for (index, display) in presentation.banners.iter_mut().enumerate() {
        let Some(banner_types) = banner_types.as_ref() else {
            continue;
        };
        let Some(banner_type) = banner_types.entry(&display.type_name) else {
            continue;
        };
        // `Render`'s period tick (`BannerClass.cpp:58-77`), one step per
        // drawn frame.
        let mut visible = true;
        if display.shown > 0 {
            display.shown -= 1;
        } else if display.shown == 0 {
            if display.hidden < 0 {
                visible = false;
            } else if display.hidden > 0 {
                display.hidden -= 1;
                visible = false;
            } else {
                display.shown = banner_type.duration;
                display.hidden = banner_type.delay;
                if banner_type.shape_refresh_after_delay {
                    display.shape_frame = 0;
                }
            }
        }
        if !visible {
            continue;
        }
        let position = banner_position(display.position_percent, view);
        let art = presentation
            .banner_art
            .as_ref()
            .and_then(|art| art.slot(&display.type_name).map(|slot| (art, slot)));
        if let Some((art, slot)) = art {
            // `RenderPCX`/`RenderSHP` center on the position
            // (`BannerClass.cpp:79-81, 106-108`); the SHP frame advances once
            // per drawn frame (`:134-138`).
            let (origin_in_atlas, size, shape_frame_in_use) = if slot.frame_sizes.is_empty() {
                (slot.image_origin, slot.image_size, None)
            } else {
                let index = display.shape_frame as usize % slot.frame_sizes.len();
                (
                    slot.frame_origins[index],
                    slot.frame_sizes[index],
                    Some(index),
                )
            };
            let mut origin = [
                position[0] - size[0] as i32 / 2,
                position[1] - size[1] as i32 / 2,
            ];
            // The clamp keeps the image inside the view
            // (`BannerClass.cpp:83-89, 111-117`).
            if banner_type.clamp_to_screen {
                let max_x = (view[0] - size[0] as i32).max(0);
                let max_y = (view[1] - size[1] as i32).max(0);
                origin[0] = origin[0].clamp(0, max_x);
                origin[1] = origin[1].clamp(0, max_y);
            }
            fills.push(SpriteInstance {
                position: [origin[0] as f32 + camera[0], origin[1] as f32 + camera[1]],
                size: size.map(|value| value as f32),
                uv_origin: [
                    origin_in_atlas[0] as f32 / art.atlas[0] as f32,
                    origin_in_atlas[1] as f32 / art.atlas[1] as f32,
                ],
                uv_size: [
                    size[0] as f32 / art.atlas[0] as f32,
                    size[1] as f32 / art.atlas[1] as f32,
                ],
                depth: BANNER_DEPTH,
                tint: [1.0; 3],
                alpha: 1.0,
                ..Default::default()
            });
            if let Some(index) = shape_frame_in_use {
                display.shape_frame = display.shape_frame.wrapping_add(1);
                if u32::from(display.shape_frame) >= slot.frame_sizes.len() as u32 {
                    display.shape_frame = 0;
                }
            }
        }
        if let Some((text, color)) = banner_text(
            csf,
            variables_values.get(index).copied().flatten(),
            fallback_tint,
            banner_type,
        ) {
            let width = font.text_width(&text) as i32;
            let height = font.cell_height() as i32;
            let mut origin = [position[0] - width / 2, position[1] - height / 2];
            if banner_type.clamp_to_screen {
                origin[0] = origin[0].clamp(0, (view[0] - width).max(0));
                origin[1] = origin[1].clamp(0, (view[1] - height).max(0));
            }
            if banner_type.csf_background {
                fills.push(SpriteInstance {
                    position: [
                        origin[0] as f32 - 1.0 + camera[0],
                        origin[1] as f32 - 1.0 + camera[1],
                    ],
                    size: [width as f32 + 2.0, height as f32 + 2.0],
                    uv_origin: [0.0, 0.0],
                    uv_size: [1.0, 1.0],
                    depth: BANNER_DEPTH - 0.00001,
                    tint: [0.0; 3],
                    alpha: 1.0,
                    ..Default::default()
                });
            }
            let tint = color.map(|channel| f32::from(channel) / 255.0);
            texts.extend(font.build_text(
                &text,
                origin[0] as f32,
                origin[1] as f32,
                1.0,
                BANNER_DEPTH,
                tint,
                camera,
            ));
        }
    }
    (fills, texts)
}

/// The percents of the tactical view the constructor scales
/// (`BannerClass.cpp:14-15`, `Ext/TAction/Body.cpp:829`).
fn banner_position(percent: [i32; 2], view: [i32; 2]) -> [i32; 2] {
    [
        (percent[0] as f64 / 100.0 * view[0] as f64) as i32,
        (percent[1] as f64 / 100.0 * view[1] as f64) as i32,
    ]
}

/// The session list is authoritative: a surviving id keeps its blink phase
/// while a replace updates its facts in place (`Ext/TAction/Body.cpp:824-833`),
/// a new id appends, and an id the session dropped leaves the displays.
fn reconcile_displays(
    displays: &mut Vec<BannerDisplay>,
    banners: &[crate::sim::banner::RuntimeBanner],
) {
    for banner in banners {
        if let Some(display) = displays.iter_mut().find(|d| d.id == banner.id) {
            display.type_name = banner.type_name.clone();
            display.position_percent = banner.position_percent;
            display.variable = banner.variable;
            display.global_variable = banner.global_variable;
        } else {
            displays.push(BannerDisplay::new(
                banner.id,
                banner.type_name.clone(),
                banner.position_percent,
                banner.variable,
                banner.global_variable,
            ));
        }
    }
    displays.retain(|display| banners.iter().any(|banner| banner.id == display.id));
}

/// The CSF arm's text and color (`BannerClass.cpp:124-190`): the label, the
/// scenario variable's `Value` joined by the format (`:146-155`,
/// `std::to_wstring`), or nothing when the variable has no extended entry.
/// `CSF.Color`'s fallback is `Drawing::TooltipColor`
/// (`BannerClass.cpp:190`) — YRpp binds it to gamemd's `0xB0FA1C`, the
/// runtime-synced sidebar text colour, which VERA resolves through the
/// sidebar text tint like the weeds counter's same fallback does
/// (`sidebar_render.rs:938`).
fn banner_text(
    csf: Option<&crate::assets::csf_file::CsfFile>,
    variable_value: Option<i32>,
    fallback_tint: [u8; 3],
    banner_type: &crate::rules::banner_type::BannerType,
) -> Option<(String, [u8; 3])> {
    use crate::rules::banner_type::BannerNumberType;
    let color = banner_type.csf_color.unwrap_or(fallback_tint);
    let format = banner_type.csf_variable_format;
    if format != BannerNumberType::None {
        let value = variable_value?.to_string();
        let label = banner_type.csf.as_deref().unwrap_or("");
        let resolved = csf_resolved(csf, label);
        return Some((
            match format {
                BannerNumberType::Variable => value,
                BannerNumberType::Prefixed => format!("{value}{resolved}"),
                BannerNumberType::Suffixed => format!("{resolved}{value}"),
                BannerNumberType::None => resolved,
            },
            color,
        ));
    }
    let label = banner_type.csf.as_deref()?;
    (!label.is_empty()).then(|| (csf_resolved(csf, label), color))
}

/// `CSFText::Text` — the label's localized string, raw when no table loads.
fn csf_resolved(csf: Option<&crate::assets::csf_file::CsfFile>, label: &str) -> String {
    csf.map(|csf| csf.text(label).into_owned())
        .unwrap_or_else(|| label.to_owned())
}

/// `BannerClass::Render`'s depth: above the tactical world, below the HUD
/// text lanes (the outcome draw sits at 0.00022, the message fills at
/// 0.00023).
const BANNER_DEPTH: f32 = 0.0002;

/// The atlas the banner shape pool binds.
pub(crate) fn texture(state: &AppState) -> Option<&BatchTexture> {
    state
        .match_state
        .match_presentation
        .banner_art
        .as_ref()
        .map(BannerArtSet::texture)
}

#[cfg(test)]
mod tests {
    use super::{BannerDisplay, reconcile_displays};

    /// `CreateOrReplaceBanner` (`Ext/TAction/Body.cpp:813-843`): a replace
    /// updates the facts in place and keeps the blink phase, a new id
    /// appends, and `DeleteBanner` drops the id's display.
    #[test]
    fn reconcile_keeps_the_phase_and_updates_the_facts() {
        let banner = |id, x| crate::sim::banner::RuntimeBanner {
            id,
            type_name: "INTRO".to_owned(),
            position_percent: [x, 0],
            variable: 0,
            global_variable: false,
        };
        let mut displays = Vec::new();
        reconcile_displays(&mut displays, &[banner(7, 50)]);
        displays[0].shown = 5;
        assert_eq!(displays.len(), 1);

        reconcile_displays(&mut displays, &[banner(7, 10), banner(9, 0)]);
        assert_eq!(displays.len(), 2, "the new id appends");
        assert_eq!(displays[0].position_percent, [10, 0], "facts update");
        assert_eq!(displays[0].shown, 5, "the blink phase survives a replace");
        assert_eq!(displays[1].id, 9);

        reconcile_displays(&mut displays, &[banner(9, 0)]);
        assert_eq!(displays.len(), 1, "the dropped id leaves");
        assert_eq!(displays[0].id, 9);
    }

    /// The constructor scales the action's percents against the tactical
    /// view (`BannerClass.cpp:14-15`).
    #[test]
    fn banner_position_scales_the_percent_pair() {
        assert_eq!(super::banner_position([50, 25], [800, 600]), [400, 150]);
        assert_eq!(super::banner_position([0, 0], [800, 600]), [0, 0]);
        assert_eq!(super::banner_position([100, 100], [800, 600]), [800, 600]);
    }
}
