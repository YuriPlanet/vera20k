//! Asset-free compatibility controls for the actual free-Anim emitter.
//!
//! Parsed ART, real Anim construction and retained Display membership feed a
//! metadata-only atlas. These assert the scoped material change and unchanged
//! special-path fallback, not native pixels or complete special-path parity.

use super::{DisplayEffectInputs, build_display_effect_instances};
use crate::app::presentation::detail::DetailState;
use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectTexture, PlannedObjectInstance, lower_object_instances,
};
use crate::map::houses::HouseColorMap;
use crate::map::lighting::CellLightGrid;
use crate::render::batch::DepthAxis;
use crate::render::draw_state::DrawState;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, ShpSpriteKey, SpriteAtlas};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::rules::{
    art_data::ArtRegistry, house_colors::HouseColorIndex, ini_parser::IniFile, ruleset::RuleSet,
};
use crate::sim::anim_class::{AnimDrawRuntime, AnimWorldCoord};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;
use std::cell::RefCell;
use std::collections::HashMap;

fn fixture(
    name: &str,
    keys: &str,
    draw_flags: u32,
    ramp: u8,
) -> (Simulation, RuleSet, u64, SpriteAtlas) {
    let rules_ini = IniFile::from_str(&format!("[Animations]\n0={name}\n"));
    let art_ini = IniFile::from_str(&format!("[{name}]\nEnd=20\nTranslucency=50\n{keys}"));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    // Fixed ART supplies the ordered reader's inputs; map loading separately
    // installs the runtime metadata registry before binding image headers.
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules.bind_anim_frame_count_for_test(name, 20);
    let mut sim = Simulation::with_seed(31);
    let type_id = sim.intern(name);
    let coord = AnimWorldCoord {
        x: 16512,
        y: 16512,
        z: 0,
    };
    let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
    let id = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor {
                draw_flags,
                draw_runtime: AnimDrawRuntime {
                    translucency_ramp: ramp,
                    ..Default::default()
                },
                ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
            },
            coord,
        )
        .unwrap();
    assert!(sim.display_layers().layer_of(id).is_some());
    assert_eq!(sim.anim(id).unwrap().runtime().current_frame, 0);
    let atlas = SpriteAtlas::from_test_entries(HashMap::from([(
        ShpSpriteKey {
            palette_context: ShpPaletteContext::GlobalAnim,
            type_id: name.into(),
            facing: 0,
            frame: 0,
            house_color: HouseColorIndex(0),
        },
        ShpSpriteEntry {
            uv_origin: [0.25, 0.125],
            uv_size: [0.125, 0.25],
            pixel_size: [8., 6.],
            offset_x: -4.,
            offset_y: -3.,
            canvas_rect: [-4., -3., 8., 6.],
            extended: false,
            page: 2,
            source_palette: 5,
        },
    )]));
    (sim, rules, id, atlas)
}

fn emit(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    atlas: &SpriteAtlas,
) -> Vec<PlannedObjectInstance> {
    emit_with_house_colors(sim, rules, id, atlas, &Default::default())
}

fn emit_with_house_colors(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    atlas: &SpriteAtlas,
    house_colors: &HouseColorMap,
) -> Vec<PlannedObjectInstance> {
    emit_with_lighting(sim, rules, id, atlas, house_colors, &CellLightGrid::new())
}

fn emit_with_lighting(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    atlas: &SpriteAtlas,
    house_colors: &HouseColorMap,
    lighting: &CellLightGrid,
) -> Vec<PlannedObjectInstance> {
    // Meteor's real constructor changes its launch coordinate. Follow the
    // resulting location with the camera; do not overwrite the instance.
    let coord = sim.anim_absolute_coord(id).unwrap();
    let (rx, ry, sub_x, sub_y, _) = coord.to_cell_sub_z();
    let point = crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, coord.z);
    let camera = [point.0 - 80., point.1 - 70.];
    let mut detail = DetailState::new();
    for _ in 0..60 {
        detail.record_logic_visit();
    }
    detail.throttle_tail(0);
    let detail = RefCell::new(detail);
    let hidden = Default::default();
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let before_hash = sim.state_hash();
    let before_rng = sim.rng_state();
    let mut out = Vec::new();
    build_display_effect_instances(
        &DisplayEffectInputs {
            interpolation: None,
            sim,
            rules,
            camera,
            render_size: [160, 160],
            tactical: [0, 0, 160, 160],
            zoom: 1.,
            axis: DepthAxis::NONE,
            atlas: Some(atlas),
            detail: &detail,
            detail_level: 2,
            lighting,
            house_colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner: None,
            observe_particles: false,
        },
        &mut out,
        &order,
        Default::default(),
    );
    assert_eq!(sim.state_hash(), before_hash);
    assert_eq!(sim.rng_state(), before_rng);
    out
}

#[test]
fn retained_squid_light_follows_event_order_and_keeps_live_palette_colours() {
    use crate::app::presentation::lighting::MatchLighting;
    use crate::map::lighting::{LightingConfig, radiation_point_light};
    use crate::sim::light_sources::{LightingEvent, PaletteUpdate};
    // This is an asset-free transport history, using the actual free-Anim
    // emitter. The stock attack/owner gate is exercised in squid_grapple_tests.
    let (mut sim, rules, id, global_atlas) = fixture("SQDG", "UseCellDrawer=yes\n", 0x600, 0);
    let terrain = crate::map::resolved_terrain::test_flat_ground_grid(128);
    sim.install_resolved_terrain_for_new_map(terrain);
    let house = sim.intern("RemapOwner");
    sim.anims_mut_for_test().set_anim_house_remap(id, house);
    assert!(
        !sim.anim(id).unwrap().use_cell_drawer(),
        "instance flag is independent of type"
    );
    let global_key = ShpSpriteKey {
        palette_context: ShpPaletteContext::GlobalAnim,
        type_id: "SQDG".into(),
        facing: 0,
        frame: 0,
        house_color: HouseColorIndex(0),
    };
    let remap_atlas = SpriteAtlas::from_test_entries(HashMap::from([(
        ShpSpriteKey {
            palette_context: ShpPaletteContext::SelectedScheme,
            house_color: HouseColorIndex(3),
            ..global_key.clone()
        },
        ShpSpriteEntry {
            source_palette: 9,
            ..*global_atlas.get(&global_key).unwrap()
        },
    )]));
    let colors = HashMap::from([("RemapOwner".into(), HouseColorIndex(3))]);
    let cell = (64, 64);
    let mut high = MatchLighting::default();
    let mut low = MatchLighting::default();
    for (owner, detail) in [(&mut high, 2), (&mut low, 0)] {
        owner.install(
            CellLightGrid::new(),
            LightingConfig::default(),
            detail,
            Some((sim.resolved_terrain().unwrap(), &sim)),
        );
    }
    let lamp = radiation_point_light(cell.0, cell.1, 512, 200, [0; 3]);
    let capture = LightingEvent::RetainAnimTop { anim: id, cell };
    let events = [
        LightingEvent::Building {
            id: 42,
            source: Some(lamp),
        },
        capture.clone(),
    ];
    let before = (sim.state_hash(), sim.rng_state());
    high.apply_events(sim.resolved_terrain().unwrap(), &events);
    low.apply_events(sim.resolved_terrain().unwrap(), &events);
    assert_eq!(high.grid().retained_anim_top(id), Some(1150));
    assert_eq!(low.grid().retained_anim_top(id), Some(950));
    assert_eq!(
        (sim.state_hash(), sim.rng_state()),
        before,
        "local detail cannot affect synchronization"
    );

    // A raw height store changes later samples, and source removal must finish
    // before the global retained refresh. No operation recaptures an owner.
    sim.resolved_terrain_mut_for_test()
        .as_mut()
        .unwrap()
        .cell_mut_for_test(cell.0, cell.1)
        .unwrap()
        .level = 2;
    let frame = sim.session().binary_frame() as i32;
    sim.start_nuke_flash_for_test(frame);
    sim.publish_global_lighting(PaletteUpdate::from_scenario(sim.session().lighting()));
    let global = sim.lighting_sources().pending().last().unwrap().clone();
    high.apply_events(
        sim.resolved_terrain().unwrap(),
        &[
            LightingEvent::CellLevels {
                cells: vec![(cell, 2)],
            },
            LightingEvent::Building {
                id: 42,
                source: None,
            },
            global,
        ],
    );
    assert_eq!(high.grid().cell_light_at(cell).unwrap().top_scalar, 1100);
    let drawn = emit_with_lighting(&sim, &rules, id, &remap_atlas, &colors, high.grid());
    let body = drawn[0].pieces[0].instance;
    assert_eq!(
        body.palette_light,
        crate::render::palette_light::PaletteLight::color_scheme([1750, 1500, 1250], 1150)
    );
    assert_eq!(body.source_palette, 9);
    assert_eq!(body.alpha, 1.);
    assert_eq!(body.draw_state.native_selector_bits(), 4);
    let mut normal_rules = rules.clone();
    normal_rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(
        "[SQDG]\nEnd=20\nTranslucency=50\nUseNormalLight=yes\n",
    )));
    let normal = emit_with_lighting(&sim, &normal_rules, id, &remap_atlas, &colors, high.grid());
    assert_eq!(
        normal[0].pieces[0].instance.palette_light,
        crate::render::palette_light::PaletteLight::color_scheme([1750, 1500, 1250], 1000)
    );
    high.apply_events(
        sim.resolved_terrain().unwrap(),
        &[LightingEvent::Global {
            state: *sim.session().lighting(),
            cell_profile: sim.lighting_cell_profile(),
            palette: PaletteUpdate::Normal([1200, 800, 600]),
            constructor_rgb: sim.palette_constructor_rgb(),
        }],
    );
    high.refresh(sim.resolved_terrain().unwrap(), &sim, 0);
    assert_eq!(
        high.grid().retained_anim_top(id),
        Some(1150),
        "detail refresh preserves FC"
    );
    let after_detail = emit_with_lighting(&sim, &rules, id, &remap_atlas, &colors, high.grid());
    assert_eq!(
        after_detail[0].pieces[0].instance.palette_light,
        crate::render::palette_light::PaletteLight::color_scheme([1200, 800, 600], 1150)
    );
    assert_eq!(
        high.grid()
            .palette_rgb(high.grid().profiles().default_profile_id()),
        [1750, 1500, 1250],
        "Cell Convert is reconstructed independently"
    );
    high.apply_events(sim.resolved_terrain().unwrap(), &[capture]);
    assert_eq!(
        high.grid().retained_anim_top(id),
        Some(1100),
        "later owner assignment recaptures"
    );
    assert_eq!(
        high.normal_scheme_for_user_load(None, sim.session().theater()),
        [1200, 800, 600],
        "same-theater menu load retains the process normal cache"
    );
    assert_eq!(
        high.normal_scheme_for_user_load(Some(&sim), "different-theater"),
        [1000; 3],
        "Init_Theater recreates schemes on a theater change"
    );
    let mut restarted = high.clone();
    restarted.install(
        CellLightGrid::new(),
        LightingConfig::default(),
        0,
        Some((sim.resolved_terrain().unwrap(), &sim)),
    );
    assert_eq!(restarted.grid().color_scheme_normal_rgb(), [1000; 3]);
    assert_eq!(
        restarted.grid().retained_anim_top(id),
        None,
        "fresh/restart Rules reset recreates schemes even in the same theater"
    );

    // A null instance Convert takes the ordinary current-cell branch even
    // when the old retained copy still exists; it must not activate FC alone.
    sim.anims_mut_for_test()
        .set_cell_anim_draw_authority(id, None, 0);
    let plain = emit_with_lighting(&sim, &rules, id, &global_atlas, &colors, high.grid());
    assert_eq!(
        plain[0].pieces[0].instance.palette_light,
        crate::render::palette_light::PaletteLight::plain(53, 1100)
    );
}

#[test]
fn remapped_free_anim_keeps_its_existing_material_and_replay_route() {
    // Unit SimpleDeploy assigns a House remap through this same store owner.
    // The translucent ART combination here is synthetic compatibility input;
    // it is not a captured retail SCHPDEPL rendering result.
    let (mut sim, rules, id, global_atlas) = fixture("DEPLOY", "", 0x600, 0);
    let plain = emit(&sim, &rules, id, &global_atlas);
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].pieces[0].instance.alpha, 1.);
    assert_eq!(
        plain[0].pieces[0]
            .instance
            .draw_state
            .native_selector_bits(),
        4
    );
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::GlobalAnim,
        type_id: "DEPLOY".into(),
        facing: 0,
        frame: 0,
        house_color: HouseColorIndex(0),
    };
    let remap_entry = ShpSpriteEntry {
        page: 3,
        source_palette: 9,
        ..*global_atlas.get(&key).unwrap()
    };
    let remap_atlas = SpriteAtlas::from_test_entries(HashMap::from([(
        ShpSpriteKey {
            palette_context: ShpPaletteContext::SelectedScheme,
            house_color: HouseColorIndex(3),
            ..key
        },
        remap_entry,
    )]));
    let house = sim.intern("RemapOwner");
    assert!(sim.anims_mut_for_test().set_anim_house_remap(id, house));
    let colors = HashMap::from([("RemapOwner".to_string(), HouseColorIndex(3))]);
    let remapped = emit_with_house_colors(&sim, &rules, id, &remap_atlas, &colors);
    assert_eq!(remapped.len(), 1);
    assert_eq!(remapped[0].parent.id, id);
    assert_eq!(remapped[0].pieces.len(), 1);
    let layer_index = remapped[0].parent.layer.0 as usize;
    let piece = &remapped[0].pieces[0];
    assert_eq!(piece.target, ObjectTexture::ShpPage(3));
    assert_eq!(piece.instance.source_palette, 9);
    assert_eq!(piece.instance.palette_light, Default::default());
    assert_eq!(piece.instance.alpha, 0.5);
    assert_eq!(piece.instance.draw_state, DrawState::default());
    assert_eq!(
        piece.instance.position,
        plain[0].pieces[0].instance.position
    );
    assert_eq!(piece.instance.size, plain[0].pieces[0].instance.size);
    let layers = lower_object_instances(remapped);
    assert_eq!(layers[layer_index].owners, [id]);
    assert_eq!(layers[layer_index].runs.len(), 1);
    assert_eq!(layers[layer_index].runs[0].packed_parent, None);
}

#[test]
fn free_anim_material_transport_preserves_special_draw_fallbacks() {
    use crate::render::terrain_draw::TerrainPiece;
    for (name, keys, flags, alpha, selector) in [
        ("ORDINARY", "", 0x600, 1., 4),
        ("FLAT", "Flat=yes\n", 0x600, 1., 4),
        // The constructor's existing selector is ORed by DrawIt, not replaced.
        ("INHERITED", "", 0x602, 1., 6),
        // DrawIt has no Bouncer/IsMeteor body arm: 4237AE draws their bodies,
        // after the separate HasBounce ground shadow.
        ("BOUNCER", "Bouncer=yes\n", 0x600, 1., 4),
        ("METEOR", "IsMeteor=yes\n", 0x600, 1., 4),
        ("RING1", "", 0x600, 0.5, 0),
        ("TILED", "Tiled=yes\n", 0x600, 0.5, 0),
    ] {
        let (sim, rules, id, atlas) = fixture(name, keys, flags, 0);
        let bounce = sim.anim(id).unwrap().bounce().is_some();
        assert_eq!(bounce, matches!(name, "BOUNCER" | "METEOR"), "{name}");
        let out = emit(&sim, &rules, id, &atlas);
        assert_eq!(out.len(), 1, "{name}: real Display instance admitted");
        assert_eq!(out[0].parent.id, id);
        let count = 1 + usize::from(bounce);
        assert_eq!(out[0].pieces.len(), count, "{name}");
        if bounce {
            let ground = &out[0].pieces[0];
            assert_eq!(
                ground.target,
                ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow),
                "{name}"
            );
            assert_eq!(ground.render_z, RenderZPolicy::ReadOnly, "{name}");
        }
        let layer_index = out[0].parent.layer.0 as usize;
        let piece = out[0].pieces.last().unwrap();
        assert_eq!(piece.target, ObjectTexture::ShpPage(2), "{name}");
        assert_eq!(piece.render_z, RenderZPolicy::ReadOnly, "{name}");
        let instance = &piece.instance;
        assert_eq!(instance.alpha, alpha, "{name}: source alpha");
        assert_eq!(
            instance.draw_state.native_selector_bits(),
            selector,
            "{name}: selector transport"
        );
        assert_eq!(instance.uv_origin, [0.25, 0.125], "{name}");
        assert_eq!(instance.uv_size, [0.125, 0.25], "{name}");
        assert_eq!(instance.size, [8., 6.], "{name}");
        assert_eq!(instance.source_palette, 5, "{name}");
        if selector == 0 {
            assert_eq!(
                instance.draw_state,
                DrawState::default(),
                "{name}: complete legacy material"
            );
        }
        let layers = lower_object_instances(out);
        let layer = &layers[layer_index];
        assert_eq!(layer.owners, vec![id; count], "{name}");
        assert_eq!(layer.runs.len(), count, "{name}");
        assert_eq!(
            layer.runs.last().unwrap().packed_parent,
            (selector != 0).then_some(id),
            "{name}: actual replay route"
        );
        assert_eq!(layer.instances.last().unwrap().alpha, alpha, "{name}");
    }
}

#[test]
fn free_anim_suppressed_ramp_emits_no_piece_for_ordinary_or_special_paths() {
    for (name, keys) in [
        ("ORDINARY", ""),
        ("FLAT", "Flat=yes\n"),
        ("RING1", ""),
        ("TILED", "Tiled=yes\n"),
        ("BOUNCER", "Bouncer=yes\n"),
        ("METEOR", "IsMeteor=yes\n"),
    ] {
        let (sim, rules, id, atlas) = fixture(name, keys, 0x600, 16);
        // The image and Display membership exist; only DrawIt's retained
        // instance ramp suppresses this supplied presentation state.
        let out = emit(&sim, &rules, id, &atlas);
        assert!(
            out.is_empty(),
            "{name}: suppressed draw must not become an opaque piece"
        );
        let layers = lower_object_instances(out);
        assert!(
            layers
                .iter()
                .all(|layer| layer.instances.is_empty() && layer.runs.is_empty())
        );
    }
}
