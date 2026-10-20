//! Synthetic atlas markers exercise the production slot emitter and retained
//! Display lowering. Native field/sort goldens are independent of Rust. This
//! is not a retail-pixel golden; depth_gpu_tests separately reads real GPU output.
use super::*;
use crate::app::presentation::render::draw_plan_lowering::lower_ground_object_instances;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{game_entity::GameEntity, world::Simulation};
use std::collections::HashMap;

fn fixture(building_name: &str, damaged: bool) -> (Simulation, RuleSet, u64, SpriteAtlas) {
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_slot_replacement.display.json",
    ))
    .unwrap();
    let mut art_text = format!("[{building_name}]\n");
    for (key, value) in golden["art"]["physical_sections"][building_name]
        .as_object()
        .unwrap()
    {
        art_text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
    }
    let mut registry = format!(
        "[BuildingTypes]\n0={building_name}\n[{building_name}]\nStrength=100\n[Animations]\n"
    );
    let mut entries = HashMap::new();
    let mut names = Vec::new();
    for row in golden["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["building"] == building_name)
    {
        for variant in ["normal", "damaged"] {
            let name = row["slot"][variant].as_str().unwrap();
            registry.push_str(&format!("{}={name}\n", names.len()));
            names.push(name.to_string());
            // A nonzero Start catches selecting relative AC as an atlas frame.
            // The offsets are atlas frame offsets, independent of slot X/Y.
            art_text.push_str(&format!("[{name}]\nLayer=ground\nLoopCount=-1\nStart=3\n"));
            entries.insert(
                ShpSpriteKey {
                    palette_context: ShpPaletteContext::SelectedScheme,
                    type_id: name.to_string(),
                    facing: 0,
                    frame: 3,
                    house_color: HouseColorIndex(1),
                },
                ShpSpriteEntry {
                    uv_origin: [0.0; 2],
                    uv_size: [1.0; 2],
                    pixel_size: [8.0; 2],
                    offset_x: 2.0,
                    offset_y: -4.0,
                    canvas_rect: [0.0, 0.0, 8.0, 8.0],
                    extended: false,
                    page: row["slot"]["slot"].as_u64().unwrap() as u8,
                    source_palette: 1,
                },
            );
        }
    }
    let art_ini = IniFile::from_str(&art_text);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&registry), &art_ini).unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    for name in names {
        art.bind_anim_frame_count_for_test(&name, 40);
    }
    rules.install_art_data(art);
    let mut sim = Simulation::new();
    let id = sim.substrate_mut_for_test().allocate_stable_id();
    let mut building = GameEntity::test_default_of_category(
        id,
        building_name,
        "A",
        64,
        60,
        EntityCategory::Structure,
    );
    building.set_type_ref_for_test(sim.intern(building_name));
    building.set_owner_for_test(sim.intern("A"));
    building.health.set_for_test(if damaged { 40 } else { 100 });
    sim.entities_mut_for_test().insert(building);
    let slots: &[u8] = if building_name == "NATBNK" {
        &[10, 11]
    } else {
        &[3]
    };
    for &slot in slots {
        sim.set_building_anim_slot(id, slot, damaged, false, 0, &rules)
            .unwrap();
    }
    (sim, rules, id, SpriteAtlas::from_test_entries(entries))
}

fn emit(
    sim: &Simulation,
    rules: &RuleSet,
    building: u64,
    atlas: &SpriteAtlas,
    order: &NativeDisplayOrder,
) -> Vec<PlannedObjectInstance> {
    let mut out = Vec::new();
    let building = sim.entities().get(building).unwrap();
    let grid = crate::map::lighting::CellLightGrid::new();
    emit_building_anims(
        &mut out,
        order,
        atlas,
        rules.art(),
        Some(rules),
        sim.interner().resolve(building.type_ref()),
        HouseColorIndex(1),
        [1.0; 3],
        crate::render::palette_light::PaletteLight::default(),
        &grid,
        (building.position.rx(), building.position.ry()),
        sim,
        building,
        &|_| 0,
        0.0,
        10000.0,
        [-10000.0, -10000.0, 20000.0, 20000.0],
        DrawState::default(),
    );
    out
}

#[test]
fn bunker_slot_emitter_interleaves_front_and_back_around_the_unit() {
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_slot_replacement.display.json",
    ))
    .unwrap();
    for damaged in [false, true] {
        let (mut sim, rules, building, atlas) = fixture("NATBNK", damaged);
        let slots = sim
            .entities()
            .get(building)
            .unwrap()
            .building_art
            .anim_slots();
        let back = slots[11].unwrap();
        let front = slots[10].unwrap();
        let unit = sim.substrate_mut_for_test().allocate_stable_id();
        let identities = [back, unit, front];
        let native_order: Vec<_> = golden["orders"][0]["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| identities[v.as_u64().unwrap() as usize])
            .collect();
        // Parent has no Display membership at all. Only the actual Anim ids
        // appear in this native order; parent admission cannot supply it.
        let order = NativeDisplayOrder::new(&native_order);
        let mut planned = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(planned.len(), 2);
        assert!(planned.iter().all(|entry| entry.building_pieces.is_none()));
        assert!(planned.iter().all(|entry| entry.parent.id != building));
        planned.push(PlannedObjectInstance::object(
            order.object_draw(unit, SpriteEncoding::Voxel).unwrap(),
            vec![ObjectPieceInstance {
                target: ObjectTexture::UnitAtlasPage(0),
                render_z: crate::render::tactical_draw_plan::RenderZPolicy::ReadOnly,
                instance: SpriteInstance::default(),
            }],
        ));
        let pass = lower_ground_object_instances(planned);
        assert_eq!(pass.owners, native_order);
        assert_eq!(
            pass.runs.iter().map(|run| run.target).collect::<Vec<_>>(),
            [
                ObjectTexture::ShpPage(11),
                ObjectTexture::UnitAtlasPage(0),
                ObjectTexture::ShpPage(10)
            ]
        );
        let hidden = sim.anims_mut_for_test().get_mut(front).unwrap();
        hidden.draw_runtime_mut_for_test().hidden = true;
        let remaining = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(
            remaining
                .iter()
                .map(|entry| entry.parent.id)
                .collect::<Vec<_>>(),
            [back]
        );
        sim.clear_building_anim_slot(building, 11);
        assert!(sim.anim(back).is_none());
        assert!(sim.display_layers().layer_of(back).is_none());
        assert!(
            emit(
                &sim,
                &rules,
                building,
                &atlas,
                &NativeDisplayOrder::from_display(sim.display_layers())
            )
            .is_empty()
        );
        let replacement = sim
            .set_building_anim_slot(building, 10, damaged, false, 0, &rules)
            .unwrap();
        assert_ne!(replacement, front);
        assert!(sim.anim(front).is_none());
        assert!(sim.display_layers().layer_of(front).is_none());
        let visible = emit(
            &sim,
            &rules,
            building,
            &atlas,
            &NativeDisplayOrder::from_display(sim.display_layers()),
        );
        assert_eq!(
            visible
                .iter()
                .map(|entry| entry.parent.id)
                .collect::<Vec<_>>(),
            [replacement]
        );
    }
}

#[test]
fn bunker_migration_keeps_active_anim_offsets_start_and_independent_membership() {
    for name in ["CAOILD", "GAHPAD"] {
        let (mut sim, rules, building, atlas) = fixture(name, false);
        let anim_id = sim
            .entities()
            .get(building)
            .unwrap()
            .building_art
            .anim_slots()[3]
            .unwrap();
        let order = NativeDisplayOrder::new(&[anim_id]);
        let before = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(before.len(), 1);
        let coord = sim.anim_absolute_coord(anim_id).unwrap();
        let (x, y, ..) = super::super::overlays::anim_world_render_coords(coord);
        assert_eq!(before[0].pieces[0].instance.position, [x + 2.0, y - 4.0]);
        assert_eq!(before[0].pieces[0].instance.source_palette, 1);
        // The retained Anim coordinate is the owner. A changed body coordinate
        // cannot silently recalculate its child's location in presentation.
        sim.entities_mut_for_test()
            .get_mut(building)
            .unwrap()
            .position
            .set_rx_for_test(2);
        let after = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(
            after[0].pieces[0].instance.position,
            before[0].pieces[0].instance.position
        );
        assert!(
            emit(
                &sim,
                &rules,
                building,
                &atlas,
                &NativeDisplayOrder::new(&[])
            )
            .is_empty()
        );
    }
}
