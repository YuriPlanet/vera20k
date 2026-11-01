//! A building's sale as `BuildingClass::DrawIt @ 0x0043D290` draws it, read
//! through a retail GAWEAP's production sale.
use super::*;
use crate::app::presentation::interpolation::RenderInterpolation;
use crate::app::presentation::render::draw_plan_lowering::{NativeDisplayOrder, ObjectTexture};
use crate::sim::building_construction::BuildingUp;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::runtime::SimRuntime;
use crate::sim::world::Simulation;

/// A sale drops `ZShapePointMove=` for the whole Selling mission
/// (`0x0043D6DB..0x0043D712`) and hides the slot anims once the build-up runs
/// backwards in BState 0 (`0x0043D2C1..0x0043D306`), not before.
#[test]
fn a_sale_hides_slot_anims_in_its_build_down_and_drops_the_zshape_move() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    // Asset binding gives GAWEAP its Buildup control; only a type with one
    // sells.
    let mut rules = retail.rules.clone();
    rules.set_buildup_control_for_test("GAWEAP", [0, 25, 2]);
    let rules = &rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 60);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 10, 10, 0, rules)
        .expect("factory");
    let object = rules.object("GAWEAP").unwrap();
    let art = rules.art().resolve_metadata_entry("GAWEAP", &object.image);
    let sample = |sim: &Simulation| {
        sim.entities().get(factory).map(|building| {
            (
                building.building_down(),
                building.in_construction_bstate(),
                slot_anims_hidden_by_sale(building),
                building_zshape_point_move(building, art),
            )
        })
    };
    assert_eq!(sample(&sim), Some((false, false, false, (30, 15))));
    let order = CommandEnvelope::new(
        sim.entities().get(factory).unwrap().owner(),
        sim.session().tick() + 1,
        Command::SellBuilding { entity_id: factory },
    );
    sim.advance_tick(&[order], Some(rules), None, None, 67);
    let mut seen = Vec::new();
    while let Some(state) = sample(&sim)
        && seen.len() < 8
        && sim.session().tick() < 2000
    {
        if seen.last() != Some(&state) {
            seen.push(state);
        }
        sim.advance_tick(&[], Some(rules), None, None, 67);
    }
    assert_eq!(
        seen,
        [(true, false, false, (0, 0)), (true, true, true, (0, 0))]
    );
}

#[test]
fn retail_gate_make_body_uses_native_gate_and_selling_reflections() {
    use crate::map::houses::HouseColorMap;
    use crate::map::lighting::CellLightGrid;
    use crate::render::batch::DepthAxis;
    use crate::render::sprite_atlas::{
        ShpPaletteContext, ShpSpriteEntry, ShpSpriteKey, SpriteAtlas,
    };
    use crate::render::unit_atlas::BarrelImagePitches;
    use std::cell::RefCell;
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut rules = retail.rules;
    let gate_stages = {
        let object = rules.object("GAGATE_A").expect("retail gate type");
        assert!(object.gate);
        rules
            .art()
            .resolve_metadata_entry("GAGATE_A", &object.image)
            .expect("retail gate ART entry")
            .building_gate_stages
    };
    assert_eq!(gate_stages, 9);
    // Seed the already-loaded type state through the shared native control
    // owner. This tests the emitter after Buildup admission, not asset presence.
    let buildup_time =
        crate::util::native_x87::NativeF64Bits::from_bits(rules.general.buildup_time.to_bits());
    let control =
        crate::rules::buildup_asset_catalog::buildup_control(0, Some(gate_stages), buildup_time);
    rules.set_buildup_control_for_test("GAGATE_A", control);
    let control = rules.buildup_control("GAGATE_A");
    assert!(rules.has_buildup("GAGATE_A"));
    assert!(control[1] > 0);
    let make_count = u16::try_from(control[1]).expect("retail make frame count");

    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x2246);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let gate = sim
        .spawn_object("GAGATE_A", "Americans", 10, 10, 0, &rules)
        .expect("retail gate placement");
    sim.entities_mut_for_test()
        .get_mut(gate)
        .unwrap()
        .install_building_up(BuildingUp::completing_in_ticks(control[1], 0), 0);

    let keys = (0..make_count)
        .map(|frame| {
            (
                ShpSpriteKey {
                    palette_context: ShpPaletteContext::Legacy,
                    type_id: "GAGATE_A_MAKE".to_owned(),
                    facing: 0,
                    frame,
                    house_color: crate::rules::house_colors::NO_REMAP,
                },
                ShpSpriteEntry {
                    uv_origin: [f32::from(frame), 0.0],
                    uv_size: [1.0, 1.0],
                    pixel_size: [8.0, 8.0],
                    offset_x: 0.0,
                    offset_y: 0.0,
                    canvas_rect: [0.0, 0.0, 8.0, 8.0],
                    extended: false,
                    page: u8::try_from(frame).expect("small retail gate buildup"),
                    source_palette: 1,
                },
            )
        })
        .collect();
    let mut atlas = SpriteAtlas::from_test_entries(keys);
    atlas
        .make_frame_counts
        .insert("GAGATE_A_MAKE".to_owned(), make_count);
    let mut runtime = SimRuntime::from_simulation(sim);
    runtime.resources.rules = rules.clone();
    let house_colors =
        HouseColorMap::from([("Americans".to_owned(), crate::rules::house_colors::NO_REMAP)]);

    fn emitted_make_frame(
        runtime: &SimRuntime,
        atlas: &SpriteAtlas,
        gate: u64,
        colors: &HouseColorMap,
    ) -> u16 {
        let interpolation = RenderInterpolation::new(false);
        let lighting = CellLightGrid::new();
        let barrel_image_pitches = RefCell::new(BarrelImagePitches::default());
        let input = ShpInputs {
            runtime,
            atlas: Some(atlas),
            overlay_atlas: None,
            unit_atlas: None,
            barrel_image_pitches: &barrel_image_pitches,
            interpolation: &interpolation,
            camera: [-1000.0, -1000.0],
            render_size: [5000, 5000],
            zoom: 1.0,
            building_viewport: crate::app::presentation::render::tactical_world_viewport(
                [-1000.0; 2],
                [0, 0, 5000, 5000],
                None,
            ),
            axis: DepthAxis::NONE,
            local_owner: None,
            ignore_visibility: true,
            lighting: &lighting,
            house_colors: colors,
            building_zshape: false,
        };
        let order = NativeDisplayOrder::from_display(runtime.simulation.display_layers());
        let mut planned = Vec::new();
        build_shp_instances_from_inputs(&input, &mut planned, &order);
        let building = planned
            .iter()
            .find(|planned| planned.parent.id == gate)
            .expect("gate reaches the production SHP emitter");
        let body = building
            .building_pieces
            .as_ref()
            .expect("gate uses building piece owner")
            .iter()
            .find(|piece| {
                piece.kind == crate::render::tactical_draw_plan::BuildingPieceKind::BuildupOrSpecial
            })
            .expect("gate emits its make SHP body");
        match body.target {
            ObjectTexture::ShpPage(frame) => u16::try_from(frame).unwrap(),
            target => panic!("unexpected gate make target: {target:?}"),
        }
    }

    let initial_stage = runtime
        .simulation
        .entities()
        .get(gate)
        .unwrap()
        .construction_stage_value();
    assert_eq!(initial_stage, 0);
    assert_eq!(
        emitted_make_frame(&runtime, &atlas, gate, &house_colors),
        u16::try_from(control[0] + control[1] - initial_stage - 1).unwrap(),
        "GetCurrentFrame reflects a gate's BState-0 build-up"
    );

    runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(gate)
        .unwrap()
        .finish_building_construction_for_test();
    let owner = runtime.simulation.entities().get(gate).unwrap().owner();
    let order = CommandEnvelope::new(
        owner,
        runtime.simulation.session().tick() + 1,
        Command::SellBuilding { entity_id: gate },
    );
    runtime
        .simulation
        .advance_tick(&[order], Some(&rules), None, None, 67);
    for _ in 0..8 {
        let entity = runtime.simulation.entities().get(gate).unwrap();
        if entity.building_down() && entity.in_construction_bstate() {
            break;
        }
        runtime
            .simulation
            .advance_tick(&[], Some(&rules), None, None, 67);
    }
    let entity = runtime.simulation.entities().get(gate).unwrap();
    assert!(entity.building_down() && entity.in_construction_bstate());
    let sale_stage = entity.construction_stage_value();
    assert_eq!(
        emitted_make_frame(&runtime, &atlas, gate, &house_colors),
        u16::try_from(sale_stage).unwrap(),
        "the Selling reflection follows and cancels the gate reflection"
    );
}
