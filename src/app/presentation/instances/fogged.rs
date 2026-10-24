//! FoggedObjectClass's immutable building draw, separate from live Display.
//!
//! Original constructor4D0EF0 and DrawAll4D1890 retain only type/frame records,
//! house, and XYZ. Neither a source entity nor its current art state is read.

use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    ObjectLayerPass, ObjectPieceInstance, ObjectTexture, lower_fogged_object_pieces,
};
use crate::map::houses::HouseColorMap;
use crate::map::lighting::CellLightGrid;
use crate::render::batch::{DepthAxis, SpriteInstance};
use crate::render::native_z::{self, ZGradient, pack_z_gradient};
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, ShpSpriteKey, SpriteAtlas};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::rules::house_colors::{HouseColorIndex, NO_REMAP};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::StringInterner;
use crate::sim::vision::{FoggedDrawKind, FrozenBuilding};

pub(crate) fn build_fogged_instances(state: &AppState) -> ObjectLayerPass {
    let (Some(runtime), Some(atlas)) = (
        state.match_state.sim_runtime.as_ref(),
        state.match_state.match_presentation.sprite_atlas.as_ref(),
    ) else {
        return ObjectLayerPass::default();
    };
    if state.match_state.sandbox_full_visibility {
        return ObjectLayerPass::default();
    }
    let sim = &runtime.simulation;
    let Some(viewer) = crate::app::input::commands::preferred_local_owner_name(state)
        .and_then(|owner| sim.interner().get(&owner))
    else {
        return ObjectLayerPass::default();
    };
    let presentation = &state.match_state.match_presentation;
    let input = FrozenDrawInputs {
        interner: sim.interner(),
        rules: &runtime.resources.rules,
        atlas,
        lighting: presentation.lighting.grid(),
        house_colors: &presentation.house_color_map,
        axis: super::depth_axis(state),
        building_zshape: presentation.building_zshape.is_some(),
        detail_level: presentation.in_game_options.detail_level as i32,
    };
    // Constructor4D0EF0 writes this signed key; DrawAll4D18C6
    // sorts it ascending with4D2ED0. This index is not a Display layer.
    let mut snapshots: Vec<_> = sim
        .fog()
        .fogged_objects()
        .values()
        .filter(|record| record.viewer == viewer)
        .filter(|record| {
            // VERA's translucent fog must not expose enemy buildings the
            // viewer does not currently see; remembered pictures are that
            // concealment in the native renderer.
            if let Some(draw) = record.drawing.as_ref() {
                let fog_of_war = sim.session().game_options().fog_of_war;
                let owner = sim.interner().resolve(draw.owner);
                if !crate::app::input::commands::preferred_local_owner_name(state)
                    .as_deref()
                    .is_some_and(|local| {
                        crate::map::houses::is_allied_with(sim.house_alliances(), local, owner)
                    })
                {
                    let cell = (
                        (draw.location.x / 256) as u16,
                        (draw.location.y / 256) as u16,
                    );
                    if super::helpers::translucent_fog_conceals_cell(
                        Some(viewer),
                        Some(sim.fog()),
                        cell.0,
                        cell.1,
                        fog_of_war,
                    ) {
                        return false;
                    }
                }
            }
            true
        })
        .filter_map(|record| record.drawing.as_ref().map(|draw| (record.id, draw)))
        .filter(|(_, draw)| draw.visible)
        .collect();
    snapshots.sort_by_key(|(_, draw)| {
        i32::MAX
            .wrapping_sub(i32::from((draw.location.x / 256) as i16).wrapping_mul(0x944a))
            .wrapping_sub(i32::from((draw.location.y / 256) as i16).wrapping_mul(0x93b6))
            .wrapping_sub(6)
    });
    lower_fogged_object_pieces(
        snapshots
            .into_iter()
            .map(|(id, draw)| (id, frozen_building_pieces(&input, draw))),
    )
}

struct FrozenDrawInputs<'a> {
    interner: &'a StringInterner,
    rules: &'a RuleSet,
    atlas: &'a SpriteAtlas,
    lighting: &'a CellLightGrid,
    house_colors: &'a HouseColorMap,
    axis: DepthAxis,
    building_zshape: bool,
    detail_level: i32,
}

/// FoggedObjectClass::DrawAll4D1890: body, shadow, bib, then saved anims.
/// Its fixed selectors carry no live tint, buildup, turret, health or pips.
fn frozen_building_pieces(
    input: &FrozenDrawInputs<'_>,
    frozen: &FrozenBuilding,
) -> Vec<ObjectPieceInstance> {
    let mut pieces = Vec::new();
    let Some(body) = frozen.records.first() else {
        return pieces;
    };
    if !frozen.visible || body.kind != FoggedDrawKind::Building {
        return pieces;
    }
    let name = input.interner.resolve(body.type_ref);
    let Some(object) = input.rules.object(name) else {
        return pieces;
    };
    //4D1D49..4D1D5F skips the entire building record list.
    if object.invisible_in_game {
        return pieces;
    }
    let art = input
        .rules
        .art()
        .resolve_metadata_entry(name, &object.image);
    let location = frozen.location;
    let cell = ((location.x / 256) as u16, (location.y / 256) as u16);
    //DrawAll4D1890 projects (Location.X-128, Location.Y-128, Location.Z).
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(
        location.x.wrapping_sub(128),
        location.y.wrapping_sub(128),
        location.z,
    );
    let lift = crate::util::native_x87::adjust_for_z_standard(location.z);
    let depth = super::compute_sprite_depth_params_lifted(
        input.axis.origin_y,
        input.axis.world_height,
        y,
        lift,
    );
    let house_color = input
        .house_colors
        .get(input.interner.resolve(frozen.owner))
        .copied()
        .unwrap_or(NO_REMAP);
    let brightness = input
        .lighting
        .cell_light_at(cell)
        .map_or(1000, |light| light.top_scalar)
        .wrapping_add(art.map_or(0, |art| i32::from(art.extra_light as i16)));
    //4D1E96..4D1EE9 selects cell Convert for TerrainPalette, but keeps the
    // same top-plus-ExtraLight intensity as the house-Convert arm.
    let palette = if art.is_some_and(|art| art.terrain_palette) {
        PaletteLight::cell(input.lighting, cell, false).with_brightness(brightness)
    } else {
        PaletteLight::color_scheme(
            input
                .lighting
                .palette_rgb(input.lighting.profiles().default_profile_id()),
            brightness,
        )
    };
    let Ok(frame) = u16::try_from(body.frame) else {
        return pieces;
    };
    let body_key = ShpSpriteKey {
        palette_context: art.map_or(ShpPaletteContext::SelectedScheme, |art| {
            crate::render::sprite_atlas::building_art_palette_context(art)
        }),
        type_id: name.to_string(),
        facing: 0,
        frame,
        house_color,
    };
    if let Some(entry) = input.atlas.get(&body_key) {
        let mut instance = sprite(entry, [x, y], depth, palette);
        let zshape = input.building_zshape && !body.no_zshape;
        let normal_z = if body.no_zshape {
            0
        } else {
            art.map_or(0, |art| art.normal_z_adjust)
        };
        instance.z_adjust = super::lifted_z_adjust(lift, normal_z.wrapping_sub(2));
        instance.z_gradient = if body.no_zshape {
            pack_z_gradient(ZGradient::Flat, false)
        } else {
            native_z::pack_building_z_gradient(zshape, entry.extended)
        };
        if zshape {
            let origin = native_z::zshape_origin(
                (x as i32, y as i32),
                object.foundation.dimensions(),
                art.map_or((0, 0), |art| art.z_shape_point_move),
            );
            instance.zshape_origin = [origin.0 as f32, origin.1 as f32];
        }
        pieces.push(piece(entry, instance, RenderZPolicy::ReadWrite));
    }
    //4D1FCB..4D200A always attempts frame + image-frame-count/2. It does
    // not ask the live Techno shadow/visual-state predicate (705E00).
    if let Some(shadow_frame) = input
        .atlas
        .object_shp_frame_count(name)
        .and_then(|count| frame.checked_add(count / 2))
    {
        let shadow_key = ShpSpriteKey {
            frame: shadow_frame,
            ..body_key
        };
        if let Some(entry) = input.atlas.get(&shadow_key) {
            let mut instance = sprite(entry, [x, y], depth, palette.with_brightness(1000));
            instance.z_adjust = super::lifted_z_adjust(lift, -2);
            instance.z_gradient = pack_z_gradient(ZGradient::Flat, false);
            pieces.push(ObjectPieceInstance {
                target: ObjectTexture::SpriteAtlasEdit(
                    entry.page as usize,
                    crate::render::terrain_draw::TerrainPiece::Shadow,
                ),
                render_z: RenderZPolicy::ReadWrite,
                instance,
            });
        }
    }
    if let Some(art) = art
        && let Some(bib) = art.bib_shape.as_deref()
    {
        let key = ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::building_art_palette_context(art),
            type_id: bib.to_ascii_uppercase(),
            facing: 0,
            frame,
            house_color,
        };
        if let Some(entry) = input.atlas.get(&key) {
            let mut instance = sprite(entry, [x, y], depth, palette);
            instance.z_adjust = super::lifted_z_adjust(lift, -3);
            instance.z_gradient = pack_z_gradient(ZGradient::Flat, false);
            pieces.push(piece(entry, instance, RenderZPolicy::ReadWrite));
        }
    }
    for record in &frozen.records[1..] {
        if record.kind != FoggedDrawKind::Animation {
            continue;
        }
        let name = input.interner.resolve(record.type_ref);
        let config = input.rules.art().anim_runtime_config(name);
        if config.is_some_and(|config| config.detail_level > input.detail_level) {
            continue;
        }
        let Ok(frame) = u16::try_from(record.frame) else {
            continue;
        };
        let context = crate::render::sprite_atlas::frozen_anim_palette_context(config);
        let key = ShpSpriteKey {
            palette_context: context,
            type_id: name.to_string(),
            facing: 0,
            frame,
            house_color: if context == ShpPaletteContext::AnimPalette {
                HouseColorIndex(0)
            } else {
                house_color
            },
        };
        let Some(entry) = input.atlas.get(&key) else {
            continue;
        };
        let brightness = if config.is_some_and(|config| config.use_normal_light) {
            1000
        } else {
            input
                .lighting
                .cell_light_at(cell)
                .map_or(1000, |light| light.top_scalar)
        };
        let palette = if context == ShpPaletteContext::AnimPalette {
            PaletteLight::plain(53, brightness)
        } else {
            PaletteLight::color_scheme(
                input
                    .lighting
                    .palette_rgb(input.lighting.profiles().default_profile_id()),
                brightness,
            )
        };
        let mut instance = sprite(entry, [x, y], depth, palette);
        instance.z_adjust = super::lifted_z_adjust(lift, record.z_adjust.wrapping_sub(2));
        instance.z_gradient = pack_z_gradient(
            if config.is_some_and(|config| config.flat) {
                ZGradient::Flat
            } else {
                ZGradient::Vertical
            },
            false,
        );
        pieces.push(piece(entry, instance, RenderZPolicy::ReadOnly));
    }
    pieces
}

fn sprite(
    entry: &ShpSpriteEntry,
    anchor: [f32; 2],
    depth: f32,
    palette_light: PaletteLight,
) -> SpriteInstance {
    SpriteInstance {
        position: [anchor[0] + entry.offset_x, anchor[1] + entry.offset_y],
        size: entry.pixel_size,
        uv_origin: entry.uv_origin,
        uv_size: entry.uv_size,
        source_palette: entry.source_palette,
        depth,
        palette_light,
        tint: [1.0; 3],
        alpha: 1.0,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::DriveCoord;
    use crate::sim::vision::FoggedDrawRecord;

    #[test]
    fn saved_draw_records_keep_body_shadow_bib_and_anim_without_a_live_building() {
        let mut interner = StringInterner::new();
        let owner = interner.intern("A");
        let building = interner.intern("TEST");
        let anim = interner.intern("FLAME");
        let art = IniFile::from_str(
            "[TEST]\nFoundation=1x1\nBibShape=PAD\nNormalZAdjust=9\n[FLAME]\nStart=4\nYDrawOffset=42\nShadow=yes\nTranslucency=50\nAltPalette=yes\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[BuildingTypes]\n0=TEST\n[TEST]\nStrength=100\nNoShadow=yes\n[Animations]\n0=FLAME\n"),
            &art,
        )
        .unwrap();
        // Processing the rules establishes read order; production then binds
        // the immutable ART registry before presentation builds its sprites.
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        assert_eq!(
            rules.art().get("TEST").unwrap().bib_shape.as_deref(),
            Some("PAD")
        );
        let frozen = FrozenBuilding {
            owner,
            location: DriveCoord::cell(7, 8, 104),
            level: 1,
            visible: true,
            records: vec![
                FoggedDrawRecord {
                    kind: FoggedDrawKind::Building,
                    type_ref: building,
                    frame: 2,
                    no_zshape: false,
                    z_adjust: 0,
                },
                FoggedDrawRecord {
                    kind: FoggedDrawKind::Animation,
                    type_ref: anim,
                    frame: 7,
                    no_zshape: false,
                    z_adjust: 13,
                },
            ],
        };
        let mut entries = std::collections::HashMap::new();
        for (type_id, frame, palette_context, page) in [
            ("TEST", 2, ShpPaletteContext::SelectedScheme, 0),
            ("TEST", 6, ShpPaletteContext::SelectedScheme, 1),
            ("PAD", 2, ShpPaletteContext::SelectedScheme, 2),
            ("FLAME", 7, ShpPaletteContext::AnimScheme, 3),
            // The snapshot never switches to a buildup image or re-adds Start.
            ("TEST_MAKE", 2, ShpPaletteContext::Legacy, 4),
            ("FLAME", 11, ShpPaletteContext::AnimScheme, 5),
        ] {
            entries.insert(
                ShpSpriteKey {
                    palette_context,
                    type_id: type_id.into(),
                    facing: 0,
                    frame,
                    house_color: HouseColorIndex(1),
                },
                ShpSpriteEntry {
                    uv_origin: [0.0; 2],
                    uv_size: [1.0; 2],
                    pixel_size: [8.0; 2],
                    offset_x: 3.0,
                    offset_y: -4.0,
                    canvas_rect: [0.0, 0.0, 8.0, 8.0],
                    extended: true,
                    page,
                    source_palette: 1,
                },
            );
        }
        let mut atlas = SpriteAtlas::from_test_entries(entries);
        atlas.set_object_shp_frame_count_for_test("TEST", 8);
        let lighting = CellLightGrid::new();
        let colors = [("A".into(), HouseColorIndex(1))].into_iter().collect();
        let input = FrozenDrawInputs {
            interner: &interner,
            rules: &rules,
            atlas: &atlas,
            lighting: &lighting,
            house_colors: &colors,
            axis: DepthAxis {
                origin_y: 0.0,
                world_height: 1000.0,
            },
            building_zshape: true,
            detail_level: 2,
        };
        let pieces = frozen_building_pieces(&input, &frozen);
        assert_eq!(
            pieces.iter().map(|piece| piece.target).collect::<Vec<_>>(),
            vec![
                ObjectTexture::ShpPage(0),
                ObjectTexture::SpriteAtlasEdit(
                    1,
                    crate::render::terrain_draw::TerrainPiece::Shadow
                ),
                ObjectTexture::ShpPage(2),
                ObjectTexture::ShpPage(3),
            ]
        );
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(7 * 256, 8 * 256, 104);
        assert!(
            pieces
                .iter()
                .all(|piece| piece.instance.position == [x + 3.0, y - 4.0])
        );
        assert!(pieces.iter().all(|piece| piece.instance.alpha == 1.0
            && piece.instance.draw_state.native_selector_bits() == 0
            && piece.instance.palette_light.colour_word() == 0));
        assert_eq!(pieces[3].render_z, RenderZPolicy::ReadOnly);
        assert_eq!(
            pieces[3].instance.z_adjust,
            11.0 - crate::util::native_x87::adjust_for_z_standard(104) as f32
        );
    }
}

fn piece(
    entry: &ShpSpriteEntry,
    instance: SpriteInstance,
    render_z: RenderZPolicy,
) -> ObjectPieceInstance {
    ObjectPieceInstance {
        target: ObjectTexture::ShpPage(entry.page as usize),
        render_z,
        instance,
    }
}
