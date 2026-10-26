//! Real repair System/children, free Anim and building-slot emission share one
//! retained Display query history. Atlas markers are declared presence inputs;
//! the separate physical GAPOWR test owns source geometry, not pixel output.

use super::*;
use crate::app::presentation::detail::DetailState;
use crate::app::presentation::instances::{DisplayEffectInputs, build_display_effect_instances};
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::render::tactical_compat::welding_tests as native;
use crate::rules::ruleset::DetailRules;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::display_layers::DisplayLayer;
use std::cell::RefCell;
use std::collections::HashMap;

#[test]
fn retail_building_slot_preserves_mixed_display_detail_query_order() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = retail.rules;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    rules.bind_anim_class_assets(&["GAPOWR_A".to_owned()], &assets, "tem", "TEMPERATE");
    let config = rules.art().anim_runtime_config("GAPOWR_A").unwrap();
    assert!(config.art_body_read && config.raw_shp_frame_count.is_some());
    assert_eq!(config.detail_level, 2);

    let fixture =
        crate::app::input::context_order::retail_ifv_repair_tests::ordinary_welding_children(
            &rules,
        );
    let mut sim = fixture.sim;
    let system = fixture.system;
    let at = sim.particle_systems().get(system).unwrap().coords();
    assert!(sim.anims().next().is_none(), "no unrelated Anim queries");
    let children: Vec<_> = sim
        .display_layers()
        .members(DisplayLayer::AIR)
        .iter()
        .copied()
        .filter(|&id| {
            sim.particle_systems()
                .get(system)
                .unwrap()
                .particles()
                .iter()
                .any(|particle| particle.stable_id() == id)
        })
        .collect();
    assert!(!children.is_empty());
    let ty = sim.intern("GAPOWR_A");
    let free = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor::new(
                ty,
                16,
                16,
                crate::util::fixed_math::SIM_ZERO,
                crate::util::fixed_math::SIM_ZERO,
                0,
            ),
            AnimWorldCoord {
                x: at.x + 256,
                y: at.y + 256,
                z: at.z,
            },
        )
        .unwrap();
    let plant = sim
        .spawn_object("GAPOWR", "Americans", 16, 20, 0, &rules)
        .expect("ordinary flat-arena placement creates the stock plant");
    let slot = sim.entities().get(plant).unwrap().building_art.anim_slots()[3]
        .expect("normal GrandOpening creates the real slot");
    assert!(sim.anim(slot).unwrap().is_building_anim());
    assert_eq!(sim.anim(slot).unwrap().runtime().current_frame, 0);
    let ground = sim.display_layers().members(DisplayLayer::GROUND);
    let rank = |id| ground.iter().position(|&value| value == id).unwrap();
    assert!(rank(system) < rank(free) && rank(free) < rank(slot));

    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let center = crate::util::lepton::absolute_leptons_to_screen(at.x, at.y, at.z);
    let camera = [center.0 - 80., center.1 - 60.];
    let size = [640, 480];
    let lighting = crate::map::lighting::CellLightGrid::new();
    let colors = Default::default();
    let hidden = Default::default();
    let control = native::native()["spark_draw_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "unsigned_threshold_wrap")
        .unwrap();
    let has = |parents: &[PlannedObjectInstance], id| {
        parents.iter().any(|parent| {
            parent.parent.id == id && (parent.body_draw.is_some() || !parent.pieces.is_empty())
        })
    };

    for present in [true, false] {
        let entries = if present {
            HashMap::from([(
                ShpSpriteKey {
                    palette_context: ShpPaletteContext::AnimScheme,
                    type_id: "GAPOWR_A".to_owned(),
                    facing: 0,
                    frame: 0,
                    house_color: HouseColorIndex(0),
                },
                ShpSpriteEntry {
                    uv_origin: [0.; 2],
                    uv_size: [1.; 2],
                    pixel_size: [8.; 2],
                    offset_x: -4.,
                    offset_y: -4.,
                    canvas_rect: [-4., -4., 8., 8.],
                    extended: false,
                    page: 0,
                    source_palette: 1,
                },
            )])
        } else {
            HashMap::new()
        };
        // Neither atlas contains the free Anim's GlobalAnim key. Its query
        // remains between System and slot; here the FPS gate rejects it first.
        let atlas = SpriteAtlas::from_test_entries(entries);
        let before = (sim.state_hash(), sim.rng_state());
        let mut prepared = Vec::new();
        let building = sim.entities().get(plant).unwrap();
        emit_building_anims(
            &mut prepared,
            &order,
            &atlas,
            rules.art(),
            Some(&rules),
            "GAPOWR",
            HouseColorIndex(0),
            [1.; 3],
            Default::default(),
            &lighting,
            (building.position.rx(), building.position.ry()),
            &sim,
            Some(building.owner()),
            building,
            &|_| 0,
            0.,
            10000.,
            [camera[0], camera[1], size[0] as f32, size[1] as f32],
            DrawState::default(),
            2,
        );
        // The shared body/shadow emitter retains an empty parent when neither
        // source frame is available. That parent still reaches admission.
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].parent.id, slot);
        assert_eq!(!prepared[0].pieces.is_empty(), present);
        if present {
            assert_eq!(prepared[0].pieces[0].instance.source_palette, 1);
        }
        let mut detail = DetailState::new();
        detail.configure_normal(&DetailRules {
            min_frame_rate_normal: native::unsigned(&control["input"]["minimum"]) as i32,
            buffer_zone_width: native::unsigned(&control["input"]["buffer"]) as i32,
            ..Default::default()
        });
        detail.throttle_tail(0);
        let detail = RefCell::new(detail);
        let mut parents = Vec::new();
        build_display_effect_instances(
            &DisplayEffectInputs {
                interpolation: None,
                sim: &sim,
                rules: &rules,
                camera,
                render_size: size,
                tactical: [0, 0, size[0], size[1]],
                zoom: 1.,
                axis: crate::render::batch::DepthAxis::NONE,
                atlas: Some(&atlas),
                detail: &detail,
                detail_level: 2,
                lighting: &lighting,
                retained_anim_intensities: None,
                house_colors: &colors,
                hidden_super_anims: &hidden,
                show_hidden: false,
                local_owner: sim.session().current_house(),
                observe_particles: false,
            },
            &mut parents,
            &order,
            prepared
                .into_iter()
                .map(|parent| (parent.parent.id, parent))
                .collect(),
        );
        assert_eq!(
            has(&parents, system),
            control["draws"][0]["performance_passed"]
        );
        assert!(!has(&parents, free));
        assert_eq!(
            has(&parents, slot),
            present && control["draws"][2]["performance_passed"].as_bool().unwrap()
        );
        assert_eq!(
            has(&parents, children[0]),
            control["draws"][1]["performance_passed"],
            "the third Anim query shifts the first AIR child even without a slot piece"
        );
        let calls = children.len() + 3;
        assert_eq!(
            serde_json::to_value(detail.borrow().observation()).unwrap()["reduced"],
            control["draws"][(calls - 1) % 2]["reduced_after"]
        );
        assert_eq!((sim.state_hash(), sim.rng_state()), before);
    }
}
