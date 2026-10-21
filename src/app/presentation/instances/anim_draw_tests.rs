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
        palette_context: ShpPaletteContext::SelectedScheme,
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
fn disabled_shadow_and_special_draw_arms_do_not_gain_a_second_pass() {
    for keys in [
        "Shadow=no\n",
        "Tiled=yes\n",
        "Bouncer=yes\n",
        "IsMeteor=yes\n",
    ] {
        let (atlas, key, config) = fixture(keys);
        let pieces = anim_draw_pieces(&atlas, input(&atlas, &key, &config));
        assert_eq!(pieces.len(), 1, "{keys}");
        if config.tiled || config.bouncer || config.is_meteor {
            assert_eq!(
                pieces[0].instance.alpha, 0.5,
                "special arm keeps its existing body path"
            );
        }
    }
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
    use crate::app::presentation::instances::{
        DisplayEffectInputs, build_display_effect_instances,
    };
    use crate::app::presentation::render::draw_plan_lowering::NativeDisplayOrder;
    use crate::sim::{components::AnimClassSpawnDescriptor, world::Simulation};
    use crate::util::fixed_math::SIM_ZERO;
    use std::cell::RefCell;

    let art_ini =
        IniFile::from_str("[ANIM]\nLayer=ground\nShadow=yes\nStart=1\nEnd=2\nYDrawOffset=7\n");
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
    let mut descriptor =
        AnimClassSpawnDescriptor::new(sim.intern("ANIM"), 4, 4, SIM_ZERO, SIM_ZERO, 0);
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
            interpolation: None,
            sim: &sim,
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
            house_colors: &house_colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner: None,
            observe_particles: false,
        },
        &mut planned,
        &order,
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
