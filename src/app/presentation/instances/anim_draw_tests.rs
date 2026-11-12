//! Synthetic presentation inputs exercise the shared ordinary pass builder.
//! Expected geometry follows the cited DrawIt instructions; these tests are
//! not a captured native-pixel corpus.

use super::*;
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::ShpPaletteContext;
use crate::rules::{art_data::ArtRegistry, house_colors::HouseColorIndex, ini_parser::IniFile};
use std::collections::HashMap;

fn fixture(keys: &str) -> (SpriteAtlas, ShpSpriteKey, AnimTypeRuntimeConfig) {
    // Native-style INI lookup retains the first duplicate key, so each
    // control must author a single Shadow value rather than append an override.
    let shadow = if keys.contains("Shadow=") {
        ""
    } else {
        "Shadow=yes\n"
    };
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(&format!(
        "[ANIM]\n{shadow}YDrawOffset=7\nEnd=2\n{keys}"
    )));
    art.bind_anim_frame_count_for_test("ANIM", 11);
    let config = art.anim_runtime_config("ANIM").unwrap().clone();
    let key = ShpSpriteKey {
        type_id: "ANIM".into(),
        palette_context: ShpPaletteContext::AnimScheme,
        facing: 0,
        frame: 1,
        house_color: HouseColorIndex(2),
    };
    let entry = |page, offset_x, offset_y| ShpSpriteEntry {
        page,
        offset_x,
        offset_y,
        pixel_size: [8.0, 6.0],
        uv_origin: [f32::from(page) / 4.0, 0.25],
        uv_size: [0.1, 0.2],
        canvas_rect: [0.0, 0.0, 24.0, 24.0],
        extended: true,
        source_palette: u32::from(page) + 4,
    };
    let mut shadow_key = key.clone();
    shadow_key.frame = 6;
    let atlas = SpriteAtlas::from_test_entries(HashMap::from([
        (key.clone(), entry(1, 3.0, 11.0)),
        (shadow_key, entry(2, -5.0, 19.0)),
    ]));
    (atlas, key, config)
}

fn input<'a>(
    atlas: &'a SpriteAtlas,
    key: &'a ShpSpriteKey,
    config: &'a AnimTypeRuntimeConfig,
) -> AnimDrawInput<'a> {
    let entry = atlas.get(key).unwrap();
    AnimDrawInput {
        key,
        entry: Some(entry),
        point: [100.0, 200.0],
        config: Some(config),
        flags: 0x1604,
        lift_px: 9,
        axis: DepthAxis {
            origin_y: 0.0,
            world_height: 10000.0,
        },
        viewport: [-10000.0, -10000.0, 20000.0, 20000.0],
        legacy_policy: RenderZPolicy::ReadOnly,
        bounce_drop_px: None,
        body: SpriteInstance {
            position: [100.0 + entry.offset_x, 200.0 + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            source_palette: entry.source_palette,
            depth: 0.5,
            alpha: 0.5,
            tint: [0.4, 0.7, 0.8],
            palette_light: PaletteLight::color_scheme([900, 800, 700], 600)
                .with_colour_word(0x1234),
            z_adjust: lifted_z_adjust(9, 17 + 7 - 2),
            z_gradient: pack_z_gradient(ZGradient::Vertical, false),
            ..Default::default()
        },
    }
}

#[test]
fn ordinary_body_then_shadow_keep_distinct_frames_crops_and_materials() {
    let (atlas, key, config) = fixture("");
    assert_eq!(config.end, 2, "authored End is independent of raw count11");
    let pieces = anim_draw_pieces(&atlas, input(&atlas, &key, &config));
    assert_eq!(pieces.len(), 2);
    let body = &pieces[0];
    let shadow = &pieces[1];
    assert_eq!(body.target, ObjectTexture::ShpPage(1));
    assert_eq!(body.instance.position, [103.0, 211.0]);
    assert_eq!(body.instance.z_adjust, 13.0);
    assert_eq!(
        body.instance.alpha, 0.5,
        "existing body translucency is preserved"
    );
    assert_eq!(body.instance.palette_light.brightness(), 600);
    assert_eq!(body.instance.palette_light.colour_word(), 0x1234);
    assert_eq!(
        shadow.target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
    assert_eq!(shadow.instance.position, [95.0, 212.0]);
    assert_eq!(
        shadow.instance.source_palette, 6,
        "same palette context, shadow frame's entry"
    );
    assert_eq!(
        shadow.instance.z_adjust, -11.0,
        "instance ZAdjust and YDrawOffset do not enter shadow Z"
    );
    assert_eq!(
        shadow.instance.z_gradient,
        pack_z_gradient(ZGradient::Flat, false)
    );
    assert_eq!(shadow.instance.palette_light.brightness(), 1000);
    assert_eq!(shadow.instance.palette_light.colour_word(), 0);
    assert_eq!(shadow.instance.draw_state.native_selector_bits(), 1);
    assert_eq!(shadow.render_z, RenderZPolicy::ReadOnly);
}

#[test]
fn producer_flags_select_shadow_depth_independently_of_the_body() {
    let (atlas, key, config) = fixture("");
    for (flags, body_policy, shadow_policy) in [
        (0x600, RenderZPolicy::ReadOnly, RenderZPolicy::None),
        (0x1600, RenderZPolicy::ReadOnly, RenderZPolicy::ReadOnly),
    ] {
        let mut draw = input(&atlas, &key, &config);
        draw.flags = flags;
        let pieces = anim_draw_pieces(&atlas, draw);
        assert_eq!(pieces[0].render_z, body_policy);
        assert_eq!(pieces[1].render_z, shadow_policy);
        assert_eq!(pieces[0].instance.palette_light.colour_word(), 0x1234);
    }
}

#[test]
fn flat_has_one_flat_body_with_its_separate_minus_three_term() {
    let (atlas, key, config) = fixture("Flat=yes\n");
    let pieces = anim_draw_pieces(&atlas, input(&atlas, &key, &config));
    assert_eq!(pieces.len(), 1);
    assert_eq!(
        pieces[0].instance.z_gradient,
        pack_z_gradient(ZGradient::Flat, false)
    );
    assert_eq!(pieces[0].instance.z_adjust, 12.0);
}

#[test]
fn a_disabled_shadow_draws_no_second_pass() {
    let (atlas, key, config) = fixture("Shadow=no\n");
    let pieces = anim_draw_pieces(&atlas, input(&atlas, &key, &config));
    assert_eq!(pieces.len(), 1);
}

/// Tiled423636 precedes Flat and Shadow=. Frame 0 is 10 tall, so the rows
/// climb by 10 from 5 above the point, and the Z terms fall by 15 from 48
/// below the ordinary body's. The view's top is row 170: row 165 is the last.
#[test]
fn the_tiled_column_rises_by_frame_zero_through_anim_pal_until_a_row_leaves_the_view() {
    let (source, key, config) = fixture("Tiled=yes\nFlat=yes\n");
    let tile = ShpSpriteEntry {
        page: 3,
        source_palette: 9,
        ..*source.get(&key).unwrap()
    };
    let frame_zero = ShpSpriteEntry {
        pixel_size: [8.0, 10.0],
        ..tile
    };
    let column = |frame_zero: Option<ShpSpriteEntry>| {
        let mut entries = HashMap::from([
            (key.clone(), *source.get(&key).unwrap()),
            (crate::render::sprite_atlas::tiled_anim_key("ANIM", 1), tile),
        ]);
        if let Some(entry) = frame_zero {
            entries.insert(
                crate::render::sprite_atlas::tiled_anim_key("ANIM", 0),
                entry,
            );
        }
        SpriteAtlas::from_test_entries(entries)
    };
    let atlas = column(Some(frame_zero));
    let mut draw = input(&atlas, &key, &config);
    draw.viewport[1] = 170.0;
    draw.bounce_drop_px = Some(-4);
    let pieces = anim_draw_pieces(&atlas, draw);
    assert_eq!(
        pieces[0].target,
        ObjectTexture::SpriteAtlasEdit(3, TerrainPiece::Shadow),
        "the ground shadow comes first, from the column's frame"
    );
    assert_eq!(pieces[0].instance.position, [103.0, 207.0]);
    let tiles = &pieces[1..];
    assert_eq!(
        tiles
            .iter()
            .map(|tile| (tile.instance.position, tile.instance.z_adjust))
            .collect::<Vec<_>>(),
        [
            ([103.0, 206.0], -35.0),
            ([103.0, 196.0], -50.0),
            ([103.0, 186.0], -65.0),
            ([103.0, 176.0], -80.0),
        ]
    );
    for tile in tiles {
        assert_eq!(tile.target, ObjectTexture::ShpPage(3));
        assert_eq!(tile.render_z, RenderZPolicy::ReadOnly);
        assert_eq!(tile.instance.source_palette, 9);
        assert_eq!(
            tile.instance.palette_light,
            PaletteLight::plain(53, 600).with_colour_word(0x1234),
            "ANIM.PAL at the shared intensity and colour word"
        );
        assert_eq!((tile.instance.alpha, tile.instance.tint), (1.0, [1.0; 3]));
        assert_eq!(
            tile.instance.draw_state,
            DrawState::from_native_selector(0, 4)
        );
        assert_eq!(
            tile.instance.z_gradient,
            pack_z_gradient(ZGradient::Vertical, false)
        );
    }

    // An empty frame 0 would hang gamemd; VERA draws the first tile alone.
    let atlas = column(None);
    let pieces = anim_draw_pieces(&atlas, input(&atlas, &key, &config));
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].instance.position, [103.0, 211.0]);
    assert_eq!(pieces[0].instance.z_adjust, -35.0);
}

#[test]
fn bouncer_and_meteor_bodies_take_the_ordinary_arms_without_a_type_keyed_ground_shadow() {
    // DrawIt has no Bouncer/IsMeteor body arm: 4237AE draws their body and
    // 42382C their Shadow= pass. Only the instance byte +194 adds a pass.
    let summary = |pieces: Vec<ObjectPieceInstance>| {
        pieces
            .into_iter()
            .map(|piece| {
                let bytes = bytemuck::bytes_of(&piece.instance).to_vec();
                (piece.target, piece.render_z, bytes)
            })
            .collect::<Vec<_>>()
    };
    let (atlas, key, config) = fixture("");
    let ordinary = summary(anim_draw_pieces(&atlas, input(&atlas, &key, &config)));
    assert_eq!(ordinary.len(), 2);
    for keys in ["Bouncer=yes\n", "IsMeteor=yes\n"] {
        let (atlas, key, config) = fixture(keys);
        let pieces = summary(anim_draw_pieces(&atlas, input(&atlas, &key, &config)));
        assert_eq!(pieces, ordinary, "{keys}");
    }
}

#[test]
fn has_bounce_draws_the_body_frame_as_a_z_read_ground_shadow_before_the_body() {
    let (atlas, key, config) = fixture("Bouncer=yes\n");
    let mut draw = input(&atlas, &key, &config);
    // Producer flags 600: the Shadow= pass keeps them and never tests Z,
    // while 4233D2's own 2601 always reads it.
    draw.flags = 0x600;
    draw.bounce_drop_px = Some(37);
    let pieces = anim_draw_pieces(&atlas, draw);
    assert_eq!(pieces.len(), 3);
    let [ground, body, shadow] = [&pieces[0], &pieces[1], &pieces[2]];
    assert_eq!(
        ground.target,
        ObjectTexture::SpriteAtlasEdit(1, TerrainPiece::Shadow),
        "the body frame's own page is the stencil"
    );
    assert_eq!(ground.render_z, RenderZPolicy::ReadOnly);
    assert_eq!(
        ground.instance.position,
        [103.0, 248.0],
        "point + AdjustForZ(GetHeight), then the body frame's offset"
    );
    assert_eq!(ground.instance.size, body.instance.size);
    assert_eq!(ground.instance.uv_origin, body.instance.uv_origin);
    assert_eq!(ground.instance.uv_size, body.instance.uv_size);
    assert_eq!(ground.instance.source_palette, 5);
    assert_eq!(
        ground.instance.z_adjust, -2.0,
        "YDrawOffset 7 - AdjustForZ(GetCoordZ) 9"
    );
    assert_eq!(
        ground.instance.z_gradient,
        pack_z_gradient(ZGradient::Flat, false)
    );
    assert_eq!(ground.instance.draw_state.native_selector_bits(), 1);
    assert_eq!(ground.instance.palette_light.brightness(), 1000);
    assert_eq!(ground.instance.palette_light.colour_word(), 0);
    assert_eq!(ground.instance.alpha, 1.0);
    assert_eq!(body.target, ObjectTexture::ShpPage(1));
    assert_eq!(body.instance.position, [103.0, 211.0]);
    assert_eq!(body.instance.z_adjust, 13.0);
    assert_eq!(
        shadow.target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
    assert_eq!(shadow.render_z, RenderZPolicy::None);
}

#[test]
fn the_ground_shadow_needs_a_drawn_body_frame() {
    // A zero-sized body frame has no entry: the same frame draws no shadow,
    // while the later Shadow= pass keeps its own frame.
    let (source, key, config) = fixture("Bouncer=yes\n");
    let mut shadow_key = key.clone();
    shadow_key.frame = 6;
    let atlas = SpriteAtlas::from_test_entries(HashMap::from([(
        shadow_key.clone(),
        *source.get(&shadow_key).unwrap(),
    )]));
    let mut draw = input(&source, &key, &config);
    draw.entry = atlas.get(&key);
    draw.bounce_drop_px = Some(37);
    let pieces = anim_draw_pieces(&atlas, draw);
    assert_eq!(pieces.len(), 1);
    assert_eq!(
        pieces[0].target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
}

#[test]
fn a_visible_shadow_is_not_culled_by_its_offscreen_body_crop() {
    let (atlas, key, config) = fixture("");
    let mut draw = input(&atlas, &key, &config);
    let mut offscreen_entry = *draw.entry.unwrap();
    offscreen_entry.offset_x = 1000.0;
    offscreen_entry.offset_y = 1000.0;
    draw.entry = Some(&offscreen_entry);
    draw.point = [0.0, 0.0];
    draw.viewport = [0.0, 0.0, 16.0, 16.0];
    let pieces = anim_draw_pieces(&atlas, draw);
    assert_eq!(pieces.len(), 1);
    assert_eq!(
        pieces[0].target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
    assert_eq!(pieces[0].instance.position, [-5.0, 12.0]);
}

#[test]
fn empty_body_bounds_still_draw_the_companion_shadow_at_the_resolved_point() {
    let (source, key, config) = fixture("");
    let mut shadow_key = key.clone();
    shadow_key.frame = 6;
    let atlas = SpriteAtlas::from_test_entries(HashMap::from([(
        shadow_key.clone(),
        *source.get(&shadow_key).unwrap(),
    )]));
    let mut draw = input(&source, &key, &config);
    draw.entry = atlas.get(&key);
    assert!(
        draw.entry.is_none(),
        "zero-sized SHP frames have no atlas entry"
    );
    // The shadow uses the explicit draw point, without deriving it from any
    // body rectangle. Native DrawIt continues after an empty CC_Draw_Shape.
    draw.body.position = [f32::NAN; 2];
    let pieces = anim_draw_pieces(&atlas, draw);
    assert_eq!(pieces.len(), 1);
    assert_eq!(
        pieces[0].target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
    assert_eq!(pieces[0].instance.position, [95.0, 212.0]);
    assert_eq!(pieces[0].instance.z_adjust, -11.0);
}

fn free_anim_draw(
    empty_body: bool,
) -> Vec<crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance> {
    free_anim_draw_with("", 0, empty_body, None)
}

/// One free ANIM, `extra_keys` appended to its art, constructed `level`
/// height levels up, through the production Display caller: mapless (ground
/// 0) without `ground_level`.
fn free_anim_draw_with(
    extra_keys: &str,
    level: u8,
    empty_body: bool,
    ground_level: Option<u8>,
) -> Vec<crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance> {
    use crate::app::presentation::instances::{
        DisplayEffectInputs, build_display_effect_instances,
    };
    use crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder;
    use crate::sim::{components::AnimClassSpawnDescriptor, world::Simulation};
    use crate::util::fixed_math::SIM_ZERO;
    use std::cell::RefCell;

    let art_ini = IniFile::from_str(&format!(
        "[ANIM]\nLayer=ground\nShadow=yes\nStart=1\nEnd=2\nYDrawOffset=7\n{extra_keys}"
    ));
    let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[Animations]\n0=ANIM\n"),
        &art_ini,
    )
    .unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("ANIM", 11);
    rules.install_art_data(art);
    let (source, key, _) = fixture("");
    let entries = [1, 6]
        .into_iter()
        .filter(|&frame| !empty_body || frame != 1)
        .map(|frame| {
            let mut old = key.clone();
            old.frame = frame;
            let entry = *source.get(&old).unwrap();
            old.palette_context = ShpPaletteContext::GlobalAnim;
            old.house_color = HouseColorIndex(0);
            (old, entry)
        })
        .collect();
    let atlas = SpriteAtlas::from_test_entries(entries);
    let mut sim = Simulation::new();
    if let Some(ground_level) = ground_level {
        *sim.resolved_terrain_mut_for_test() =
            Some(crate::map::resolved_terrain::test_grid(5, 5, |rx, ry| {
                let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
                cell.level = ground_level;
                cell
            }));
    }
    let mut descriptor =
        AnimClassSpawnDescriptor::new(sim.intern("ANIM"), 4, 4, SIM_ZERO, SIM_ZERO, level);
    descriptor.draw_flags = 0x600;
    let id = sim.spawn_anim_object(&rules, descriptor).unwrap();
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let detail = RefCell::new(crate::app::presentation::detail::DetailState::new());
    let lighting = Default::default();
    let house_colors = Default::default();
    let hidden = Default::default();
    let before = sim.state_hash();
    let mut planned = Vec::new();
    build_display_effect_instances(
        &DisplayEffectInputs {
            sim: &sim,
            frame: &crate::app::presentation::frame::PresentationFrame::of_test(&sim, &rules),
            rules: &rules,
            camera: [-1000.0; 2],
            render_size: [2000; 2],
            tactical: [0, 0, 2000, 2000],
            zoom: 1.0,
            axis: DepthAxis::NONE,
            atlas: Some(&atlas),
            detail: &detail,
            detail_level: 2,
            lighting: &lighting,
            retained_anim_intensities: None,
            house_colors: &house_colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner: None,
            observe_particles: false,
        },
        &mut planned,
        &order,
        Default::default(),
    );
    assert_eq!(
        sim.state_hash(),
        before,
        "draw construction cannot mutate simulation"
    );
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].parent.id, id);
    planned
}

#[test]
fn free_anim_caller_uses_start_and_retains_its_no_z_shadow_in_one_parent() {
    let planned = free_anim_draw(false);
    assert_eq!(planned[0].pieces.len(), 2);
    assert_eq!(planned[0].pieces[0].target, ObjectTexture::ShpPage(1));
    assert_eq!(
        planned[0].pieces[1].target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
    assert_eq!(planned[0].pieces[0].render_z, RenderZPolicy::ReadOnly);
    assert_eq!(planned[0].pieces[1].render_z, RenderZPolicy::None);
}

#[test]
fn free_anim_caller_draws_the_shadow_of_an_empty_body_frame() {
    let planned = free_anim_draw(true);
    assert_eq!(planned[0].pieces.len(), 1);
    assert_eq!(
        planned[0].pieces[0].target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
    assert_eq!(planned[0].pieces[0].render_z, RenderZPolicy::None);
    let full = free_anim_draw(false);
    assert_eq!(
        planned[0].pieces[0].instance.position,
        full[0].pieces[1].instance.position
    );
    assert_eq!(
        planned[0].pieces[0].instance.z_adjust,
        full[0].pieces[1].instance.z_adjust
    );
}

#[test]
fn bounce_drop_measures_the_drawn_location_against_the_ground_read_only() {
    use crate::sim::{components::AnimClassSpawnDescriptor, world::Simulation};
    use crate::util::fixed_math::SIM_ZERO;
    use crate::util::native_x87::adjust_for_z_standard;
    let art_ini = IniFile::from_str(
        "[DEBRIS]\nBouncer=yes\nElasticity=0.0\nMaxXYVel=25.0\nMinZVel=25.0\n[PUFF]\nEnd=2\n",
    );
    let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[Animations]\n0=DEBRIS\n1=PUFF\n"),
        &art_ini,
    )
    .unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("DEBRIS", 15);
    art.bind_anim_frame_count_for_test("PUFF", 2);
    rules.install_art_data(art);
    let mut sim = Simulation::new();
    // Flat level-2 ground: Map578080 reads 208 leptons everywhere on it.
    *sim.resolved_terrain_mut_for_test() =
        Some(crate::map::resolved_terrain::test_grid(5, 5, |rx, ry| {
            let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
            cell.level = 2;
            cell
        }));
    let mut spawn = |name| {
        let descriptor =
            AnimClassSpawnDescriptor::new(sim.intern(name), 2, 2, SIM_ZERO, SIM_ZERO, 5);
        sim.spawn_anim_object(&rules, descriptor).unwrap()
    };
    let (debris, puff) = (spawn("DEBRIS"), spawn("PUFF"));
    let before = (
        sim.state_hash(),
        sim.resolved_terrain()
            .unwrap()
            .shared_cell_dummy()
            .snapshot(),
    );
    let stored = sim.anim(debris).unwrap().world_coord();
    assert_eq!(stored.z, 520);
    let drop = |id, drawn| anim_bounce_drop_px(&sim, sim.anim(id).unwrap(), drawn);
    assert_eq!(
        drop(debris, [stored.x, stored.y, stored.z]),
        Some(adjust_for_z_standard(520 - 208))
    );
    // A free anim's height is read at its drawn point.
    assert_eq!(
        drop(debris, [stored.x, stored.y, 400]),
        Some(adjust_for_z_standard(400 - 208))
    );
    // Off the map the isolated query stamps only its own dummy.
    assert!(drop(debris, [-5000, -5000, 300]).is_some());
    assert_eq!(drop(puff, [stored.x, stored.y, stored.z]), None);
    let after = (
        sim.state_hash(),
        sim.resolved_terrain()
            .unwrap()
            .shared_cell_dummy()
            .snapshot(),
    );
    assert_eq!(after, before, "drawing cannot mutate simulation");
}

#[test]
fn free_bouncing_anim_draws_its_ground_shadow_first_at_its_height() {
    use crate::util::native_x87::adjust_for_z_standard;
    // Stock DBRIS1LG's launch keys, three levels (312 leptons) up over
    // level-2 ground (208).
    let planned = free_anim_draw_with(
        "Bouncer=yes\nElasticity=0.0\nMaxXYVel=25.0\nMinZVel=25.0\n",
        3,
        false,
        Some(2),
    );
    let pieces = &planned[0].pieces;
    assert_eq!(pieces.len(), 3);
    let (ground, body, shadow) = (&pieces[0], &pieces[1], &pieces[2]);
    assert_eq!(
        ground.target,
        ObjectTexture::SpriteAtlasEdit(1, TerrainPiece::Shadow)
    );
    assert_eq!(ground.render_z, RenderZPolicy::ReadOnly);
    assert_eq!(body.target, ObjectTexture::ShpPage(1), "Start=1 frame");
    assert_eq!(
        shadow.target,
        ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow)
    );
    // GetHeight at the drawn Location, against the ground under it: not the
    // drawn Z without the ground.
    let drop = adjust_for_z_standard(312 - 208);
    assert_ne!(adjust_for_z_standard(312), drop);
    assert_eq!(
        ground.instance.position,
        [
            body.instance.position[0],
            body.instance.position[1] + drop as f32
        ]
    );
    // Both Z terms subtract the same drawn lift: YDrawOffset 7 here, -2 for
    // the Shadow= pass (42382C).
    assert_eq!(ground.instance.z_adjust - shadow.instance.z_adjust, 9.0);
}
