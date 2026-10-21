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
    let lighting = CellLightGrid::new();
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
            lighting: &lighting,
            house_colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner: None,
            observe_particles: false,
        },
        &mut out,
        &order,
    );
    assert_eq!(sim.state_hash(), before_hash);
    assert_eq!(sim.rng_state(), before_rng);
    out
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
    for (name, keys, flags, alpha, selector) in [
        ("ORDINARY", "", 0x600, 1., 4),
        ("FLAT", "Flat=yes\n", 0x600, 1., 4),
        // The constructor's existing selector is ORed by DrawIt, not replaced.
        ("INHERITED", "", 0x602, 1., 6),
        ("RING1", "", 0x600, 0.5, 0),
        ("TILED", "Tiled=yes\n", 0x600, 0.5, 0),
        ("BOUNCER", "Bouncer=yes\n", 0x600, 0.5, 0),
        ("METEOR", "IsMeteor=yes\n", 0x600, 0.5, 0),
    ] {
        let (sim, rules, id, atlas) = fixture(name, keys, flags, 0);
        let out = emit(&sim, &rules, id, &atlas);
        assert_eq!(out.len(), 1, "{name}: real Display instance admitted");
        assert_eq!(out[0].parent.id, id);
        assert_eq!(out[0].pieces.len(), 1, "{name}");
        let layer_index = out[0].parent.layer.0 as usize;
        let piece = &out[0].pieces[0];
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
        assert_eq!(layer.owners, [id], "{name}");
        assert_eq!(layer.runs.len(), 1, "{name}");
        assert_eq!(
            layer.runs[0].packed_parent,
            (selector != 0).then_some(id),
            "{name}: actual replay route"
        );
        assert_eq!(layer.instances[0].alpha, alpha, "{name}");
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
