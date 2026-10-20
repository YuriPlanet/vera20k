//! Original Unit73CF70 body-light controls through the production selector.

use super::*;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::{test_grid, test_smudge_cell};
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::Health;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::scenario_session::{ScenarioLightProfileUnits, ScenarioLightingState};
use crate::sim::superweapon::lightning_storm::LightningStorm;
use crate::sim::superweapon::psychic_dominator::PsychicDominatorState;

#[test]
fn unit_body_light_matches_original_bridge_side_and_weather_controls() {
    let fixture = crate::test_fixture::json("tools/unit_light.json");
    assert_eq!(
        fixture["startup"]["nuke_level"],
        crate::sim::scenario_session::NUKE_LEVEL_UNITS
    );
    let rows = fixture["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 54);
    for row in rows {
        let input = &row["input"];
        let int = |name: &str| input[name].as_i64().unwrap() as i32;
        let flag = |name: &str| input[name].as_bool().unwrap();
        let terrain = test_grid(64, 64, |x, y| {
            let mut cell = test_smudge_cell(x, y);
            cell.level = int("cell_level") as u8;
            cell.bridge_facts.raw_flags = match (x, y) {
                (40, 50) => int("flags") as u32,
                (40, 49) => int("north_flags") as u32,
                (39, 50) => int("west_flags") as u32,
                _ => 0,
            };
            cell
        });
        let mut rules = RuleSet::from_ini(&IniFile::from_str("[VehicleTypes]\n0=TANK\n")).unwrap();
        rules.general.extra_unit_light = int("extra");
        let mut sim = Simulation::new();
        let owner = sim.intern("OWNER");
        let kind = sim.intern("TANK");
        let mut unit = GameEntity::new_at_frame_zero_for_test(
            1,
            40,
            50,
            0,
            0,
            owner,
            Health::new(100),
            kind,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        let xyz = input["xyz"].as_array().unwrap();
        unit.position.put_location(DriveCoord {
            x: xyz[0].as_i64().unwrap() as i32,
            y: xyz[1].as_i64().unwrap() as i32,
            z: xyz[2].as_i64().unwrap() as i32,
        });
        unit.set_on_bridge(flag("on_bridge"));
        unit.lifecycle.set_cell_marked_for_test(flag("marked"));
        let mut normal = ScenarioLightProfileUnits::normal_default();
        normal.level_units = int("level");
        let mut ion = ScenarioLightProfileUnits::ion_default();
        ion.level_units = int("ion_level");
        let mut dominator = ScenarioLightProfileUnits::dominator_default();
        dominator.level_units = int("dominator_level");
        *sim.session_mut_for_test().lighting_mut() =
            ScenarioLightingState::new(normal, ion, dominator, 1, 1);
        sim.session_mut_for_test()
            .lighting_mut()
            .set_nuke_flash_for_test(int("nuke"), 0, 30);
        *sim.psychic_dominator_mut_for_test() =
            PsychicDominatorState::for_test(int("psydom") as u8, (0, 0), None, None);
        if flag("storm") {
            *sim.lightning_storm_mut_for_test() = LightningStorm::raging_for_test(owner, (0, 0));
        }
        let hash = sim.state_hash();
        let dummy = terrain.shared_cell_dummy().snapshot();
        let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(&terrain);
        let extra = body_extra_light(&unit, &rules, &sim, Some(&cells));
        assert_eq!(
            int("word").wrapping_add(extra),
            row["light"].as_i64().unwrap() as i32,
            "{input}"
        );
        assert_eq!(
            sim.state_hash(),
            hash,
            "drawing changes no simulation state"
        );
        assert_eq!(terrain.shared_cell_dummy().snapshot(), dummy);
        assert_eq!(row["unit_unchanged"], true);
        assert_eq!(row["rng_unchanged"], true);
    }
}

#[test]
#[ignore = "slow; physical Hills loader, ordinary bridge movement and lighting"]
fn retail_mtnk_body_light_follows_bridge_arrival_and_ground_side_cell() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut launch = crate::headless_scenario::battle_session(
        "Hills.mmx",
        &[(LaunchCountry::America, AiDifficulty::Easy)],
    );
    launch.options.bases = false;
    launch.options.unit_count = 0;
    launch.options.short_game = false;
    let mut scene =
        crate::headless_scenario::load_session(&retail, "Hills.mmx", 0x0B21_D6E5, launch).unwrap();
    assert_eq!(scene.runtime.resources.rules.general.extra_unit_light, 200);
    assert_eq!(scene.sim().session().lighting().normal().level_units, 14);
    let owner = scene.sim().session().current_house().unwrap();
    let owner_name = scene.sim().resolve(owner).to_owned();
    let fixture = crate::test_fixture::json("tools/unit_light.json");
    let rows = fixture["rows"].as_array().unwrap();
    let cases = fixture["retail"]["cases"].as_array().unwrap();
    let sample =
        |scene: &crate::headless_scenario::HeadlessScenario, id, case: &serde_json::Value| {
            let sim = scene.sim();
            let terrain = sim.resolved_terrain().unwrap();
            let entity = sim.entities().get(id).unwrap();
            let native = &rows[case["row"].as_u64().unwrap() as usize];
            let coord = case["cell"].as_array().unwrap();
            let coord = (
                coord[0].as_u64().unwrap() as u16,
                coord[1].as_u64().unwrap() as u16,
            );
            assert_eq!((entity.position.rx(), entity.position.ry()), coord);
            assert_eq!(entity.on_bridge(), case["on_bridge"].as_bool().unwrap());
            let cell = terrain.cell(coord.0, coord.1).unwrap();
            // Compare only the flags this draw consumes; the source native
            // receipt stops before later gap/occupation publication.
            let mask = 0x10900;
            assert_eq!(
                cell.bridge_facts.raw_flags & mask,
                case["source_cell"]["flags"].as_u64().unwrap() as u32 & mask
            );
            let cells = NativeCellQuery::isolated(terrain);
            assert_eq!(
                crate::sim::movement::air_movement::queried_flight_height(entity, Some(&cells)),
                0
            );
            let hash = sim.state_hash();
            let dummy = terrain.shared_cell_dummy().snapshot();
            let extra = body_extra_light(entity, &scene.runtime.resources.rules, sim, Some(&cells));
            let expected = native["light"].as_i64().unwrap() as i32
                - native["input"]["word"].as_i64().unwrap() as i32;
            assert_eq!(extra, expected, "retail cell {coord:?}");
            assert_eq!(sim.state_hash(), hash);
            assert_eq!(terrain.shared_cell_dummy().snapshot(), dummy);
            eprintln!(
                "retail MTNK {coord:?}: extra={extra}, on_bridge={}",
                entity.on_bridge()
            );
        };
    let id = {
        let rt = &mut scene.runtime;
        rt.simulation
            .spawn_object_with_overlay_registry(
                "MTNK",
                &owner_name,
                75,
                75,
                0,
                &rt.resources.rules,
                &rt.resources.overlay_registry,
            )
            .unwrap()
    };
    sample(&scene, id, &cases[0]);
    let command = CommandEnvelope::new(
        owner,
        scene.sim().session().tick(),
        Command::Move {
            entity_id: id,
            target_rx: 76,
            target_ry: 75,
        },
    );
    scene
        .runtime
        .advance_frame_for_tooling(&[command], crate::headless_scenario::SIM_TICK_MS)
        .unwrap();
    let mut arrived = false;
    for _ in 0..512 {
        let entity = scene.sim().entities().get(id).unwrap();
        if (entity.position.rx(), entity.position.ry()) == (76, 75) && entity.on_bridge() {
            sample(&scene, id, &cases[1]);
            arrived = true;
            break;
        }
        scene.tick();
    }
    assert!(arrived, "ordinary MTNK Move must enter the physical bridge");
    let side = {
        let rt = &mut scene.runtime;
        rt.simulation
            .spawn_object_with_overlay_registry(
                "MTNK",
                &owner_name,
                78,
                76,
                0,
                &rt.resources.rules,
                &rt.resources.overlay_registry,
            )
            .unwrap()
    };
    sample(&scene, side, &cases[2]);
}
