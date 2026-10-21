//! A real RandomLoopDelay retains Display membership but bypasses DrawIt's
//! mutative FPS query. Synthetic ART/atlas markers are declared scene inputs;
//! the test drives the ordinary Anim AI, without writing its runtime or Display.

use super::*;
use crate::app::presentation::detail::DetailState;
use crate::app::presentation::instances::{DisplayEffectInputs, build_display_effect_instances};
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::{FrameEffects, Simulation};
use std::cell::RefCell;

fn render(
    sim: &Simulation,
    rules: &RuleSet,
    atlas: &SpriteAtlas,
    detail: &RefCell<DetailState>,
    id: u64,
) -> Vec<PlannedObjectInstance> {
    let coord = sim.anim_absolute_coord(id).unwrap();
    let center = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    let lighting = crate::map::lighting::CellLightGrid::new();
    let colors = Default::default();
    let hidden = Default::default();
    let before = (sim.state_hash(), sim.rng_state());
    let mut parents = Vec::new();
    build_display_effect_instances(
        &DisplayEffectInputs {
            interpolation: None,
            sim,
            rules,
            camera: [center.0 - 80., center.1 - 60.],
            render_size: [160, 120],
            tactical: [0, 0, 160, 120],
            zoom: 1.,
            axis: crate::render::batch::DepthAxis::NONE,
            atlas: Some(atlas),
            detail,
            detail_level: 2,
            lighting: &lighting,
            house_colors: &colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner: None,
            observe_particles: false,
        },
        &mut parents,
        &NativeDisplayOrder::from_display(sim.display_layers()),
        Default::default(),
    );
    assert_eq!((sim.state_hash(), sim.rng_state()), before);
    parents
}

#[test]
fn random_loop_delay_skips_detail_until_real_anim_ai_restarts_it() {
    let art_ini = IniFile::from_str(
        "[LOOP]\nLayer=ground\nStart=0\nEnd=2\nLoopStart=0\nLoopEnd=2\n\
         LoopCount=-1\nRate=450\nRandomLoopDelay=3,3\nDetailLevel=0\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[Animations]\n0=LOOP\n"),
        &art_ini,
    )
    .unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("LOOP", 4);
    rules.install_art_data(art);
    let config = rules.art().anim_runtime_config("LOOP").unwrap();
    assert!(config.art_body_read);
    assert_eq!(config.random_loop_delay, Some((3, 3)));
    // Detail0 draws even below the FPS minimum, so delay suppression cannot
    // accidentally pass merely because the later performance gate hides it.
    assert_eq!(config.detail_level, 0);

    let mut sim = Simulation::new();
    let ty = sim.intern("LOOP");
    let mut descriptor = AnimClassSpawnDescriptor::new(
        ty,
        10,
        10,
        crate::util::fixed_math::SIM_ZERO,
        crate::util::fixed_math::SIM_ZERO,
        0,
    );
    descriptor.draw_flags = 0x600;
    let id = sim
        .spawn_anim_at_world(
            &rules,
            descriptor,
            AnimWorldCoord {
                x: 2688,
                y: 2688,
                z: 0,
            },
        )
        .unwrap();
    assert_eq!(sim.anim(id).unwrap().runtime().delay_remaining, 0);
    let display = sim.display_layers().layer_of(id).unwrap();
    let atlas = SpriteAtlas::from_test_entries(
        (0..4)
            .map(|frame| {
                (
                    ShpSpriteKey {
                        palette_context: ShpPaletteContext::GlobalAnim,
                        type_id: "LOOP".to_owned(),
                        facing: 0,
                        frame,
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
                )
            })
            .collect(),
    );
    let initial = render(&sim, &rules, &atlas, &RefCell::new(DetailState::new()), id);
    assert_eq!(initial.len(), 1, "the actual emitter has a visible body");
    assert_eq!(initial[0].parent.id, id);
    assert_eq!(initial[0].pieces[0].instance.source_palette, 1);

    // Supply successive caller frame anchors to ordinary AI. Only AI writes
    // the loop stage, delay, timer and retained Display state.
    let mut frame = sim.session().binary_frame();
    for _ in 0..16 {
        frame += 1;
        sim.session_mut_for_test().set_binary_frame_for_test(frame);
        sim.visit_anim(id, &rules, None, FrameEffects::default());
        if sim.anim(id).unwrap().runtime().delay_remaining != 0 {
            break;
        }
    }
    assert_eq!(sim.anim(id).unwrap().runtime().delay_remaining, 3);
    assert_eq!(sim.display_layers().layer_of(id), Some(display));

    let detail = RefCell::new(DetailState::new());
    let untouched = serde_json::to_value(detail.borrow().observation()).unwrap();
    assert_eq!(untouched["frame_rate"], 0);
    assert_eq!(untouched["minimum"], 15);
    assert_eq!(untouched["reduced"], false);
    for remaining in [3, 2, 1] {
        assert_eq!(sim.anim(id).unwrap().runtime().delay_remaining, remaining);
        assert_eq!(sim.display_layers().layer_of(id), Some(display));
        assert!(render(&sim, &rules, &atlas, &detail, id).is_empty());
        assert_eq!(
            serde_json::to_value(detail.borrow().observation()).unwrap(),
            untouched,
            "delay returns before the mutative FPS query"
        );
        frame += 1;
        sim.session_mut_for_test().set_binary_frame_for_test(frame);
        sim.visit_anim(id, &rules, None, FrameEffects::default());
    }
    assert_eq!(sim.anim(id).unwrap().runtime().delay_remaining, 0);
    assert_eq!(sim.display_layers().layer_of(id), Some(display));
    let resumed = render(&sim, &rules, &atlas, &detail, id);
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].parent.id, id);
    assert_eq!(
        serde_json::to_value(detail.borrow().observation()).unwrap()["reduced"],
        true,
        "ordinary AI restarts the same Anim and its FPS query resumes"
    );
}
