//! Execute95 ABI goldens and the authored HighExpR nuclear-strike route.
//! Geometry crops below are supplied priors, not the map's physical terrain.

use super::*;
use crate::assets::mix_archive::MixArchive;
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::{ResolvedTerrainCell, test_flat_cell, test_grid};
use crate::map::waypoints::Waypoint;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::RAD_NO_ATTACKER;
use crate::sim::projectile::{ProjectileTarget, ProjectileTrajectory};
use crate::util::sha256::sha256_hex;
use serde_json::Value;
use std::collections::HashMap;

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_nuke_strike.json")
}

fn integer(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn velocity_bits(value: &Value) -> [u64; 3] {
    std::array::from_fn(|i| u64::from_str_radix(value[i].as_str().unwrap(), 16).unwrap())
}

fn run(sim: &mut Simulation, rules: &RuleSet, entry: crate::map::actions::ActionEntry) -> bool {
    let house = sim.interner.intern("Americans");
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: Some(house),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        None,
        FrameEffects::default(),
    )
}

#[test]
fn nuke_strike_dispatch_matches_original_launch_controls() {
    for row in native()["dispatch_controls"].as_array().unwrap() {
        let input = &row["input"];
        let x = integer(&input["cell"][0]);
        let y = integer(&input["cell"][1]);
        // A negative supplied Ground return and a forced Fire refusal are
        // ABI-boundary controls in the native corpus. They are not a claim
        // that this flat, real-cell Rust crop reproduces those prerequisites.
        if x < 0 || y < 0 || !input["fire_accepts"].as_bool().unwrap() {
            continue;
        }
        let damage = integer(&input["damage"]);
        let speed = integer(&input["weapon_speed"]);
        let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=CARRIER\n[CARRIER]\nPrimary=NukePayload\n\
             [NukePayload]\nDamage={damage}\nProjectile=Down\nWarhead=WH\n\
             [Down]\nVertical=yes\nAcceleration=1\nDetonationAltitude=30000\n\
             [Warheads]\n0=WH\n[WH]\n"
        )))
        .unwrap();
        // The original control supplies this field after the type reader;
        // this is not a Speed= parser/postpass golden.
        rules.set_weapon_speed_for_test("NukePayload", speed);
        let mut sim = Simulation::with_seed(11);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let cell = (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
        let ground = integer(&input["ground"]);
        assert_eq!(ground % 104, 0);
        let flags = input["flags"].as_u64().unwrap() as u32;
        sim.install_resolved_terrain_for_new_map(test_grid(180, 180, |rx, ry| {
            let mut resolved = ResolvedTerrainCell {
                level: if (rx, ry) == cell {
                    u8::try_from(ground / 104).unwrap()
                } else {
                    0
                },
                ..test_flat_cell(rx, ry)
            };
            if (rx, ry) == cell {
                resolved.bridge_facts.raw_flags = flags;
            }
            resolved
        }));
        let waypoint = integer(&input["waypoint"]) as u32;
        sim.session.install_waypoints(&HashMap::from([(
            waypoint,
            Waypoint {
                index: waypoint,
                rx: cell.0,
                ry: cell.1,
            },
        )]));
        let before = [
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ];
        assert!(run(
            &mut sim,
            &rules,
            crate::map::actions::ActionEntry {
                kind: 95,
                params: vec!["0".into(); 6],
                waypoint_index: Some(waypoint),
            },
        ));
        let bullets: Vec<_> = sim.projectiles.iter().collect();
        assert_eq!(bullets.len(), 1, "{row}");
        let bullet = bullets[0].1;
        let events = row["events"].as_array().unwrap();
        let construct = &events.iter().find(|event| event[0] == "construct").unwrap()[1];
        let fire = &events.iter().find(|event| event[0] == "fire").unwrap()[1];
        assert_eq!(
            [
                bullet.launch_origin().x,
                bullet.launch_origin().y,
                bullet.launch_origin().z
            ],
            std::array::from_fn::<_, 3, _>(|i| integer(&fire["origin"][i])),
            "{row}"
        );
        assert_eq!(
            bullet.velocity().native().map(|value| value.bits()),
            velocity_bits(&fire["velocity_bits"]),
            "{row}"
        );
        assert_eq!(bullet.payload().base_damage, integer(&construct["damage"]));
        assert_eq!(
            bullet.payload().bright(),
            integer(&construct["bright"]) != 0
        );
        assert_eq!(bullet.source_id(), RAD_NO_ATTACKER);
        assert_eq!(
            bullet.firer_house(),
            None,
            "incoming Trigger House is ignored"
        );
        assert_eq!(
            bullet.target(),
            ProjectileTarget::Cell {
                rx: cell.0,
                ry: cell.1
            }
        );
        assert_eq!(
            bullet.trajectory(),
            ProjectileTrajectory::Vertical {
                detonation_altitude: 30000,
                acceleration: 1,
                max_speed: integer(&construct["speed"]),
            }
        );
        assert_eq!(
            sim.interner
                .resolve(bullet.payload().installed_weapon().unwrap()),
            "NukePayload"
        );
        assert_eq!(
            [
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex(),
            ],
            before,
            "a direct launch does not draw RNG: {row}"
        );
    }
}

#[test]
fn high_explosive_revenge_authored_strikes_launch_and_reach_damage() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let retail = &native()["retail"];
    let archive_bytes =
        std::fs::read(root.join(retail["archive"]["name"].as_str().unwrap())).unwrap();
    assert_eq!(sha256_hex(&archive_bytes), retail["archive"]["sha256"]);
    let archive = MixArchive::from_bytes(archive_bytes).unwrap();
    let raw = archive
        .get_by_id(retail["map"]["entry_hash"].as_u64().unwrap() as i32)
        .unwrap();
    assert_eq!(sha256_hex(raw), retail["map"]["sha256"]);
    let map = MapFile::from_bytes(raw).unwrap();
    let mode = crate::rules::retail_sources::select_ini(&assets, "MPBattleMD.INI").unwrap();
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&mode.ini)),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let art = crate::rules::retail_sources::select_ini(&assets, "ARTMD.INI").unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art.ini));
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    assert_eq!(
        rules.weapon("NukePayload").unwrap().damage,
        350,
        "the physical map overrides base600"
    );

    for record in retail["actions"].as_array().unwrap() {
        let entry = map.actions[record["trigger_id"].as_str().unwrap()]
            .entries()
            .iter()
            .find(|entry| entry.kind == 95)
            .unwrap()
            .clone();
        assert_eq!(
            entry.waypoint_index.map(i64::from),
            record["materialized"]["waypoint"].as_i64()
        );
        let expected = native()["dispatch_controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == record["trigger_id"])
            .unwrap();
        let fire = &expected["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "fire")
            .unwrap()[1];
        let mut sim = Simulation::with_seed(11);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.install_resolved_terrain_for_new_map(test_grid(180, 180, test_flat_cell));
        sim.session.install_waypoints(&map.waypoints);
        assert!(run(&mut sim, &rules, entry));
        let (_, bullet) = sim.projectiles.iter().next().unwrap();
        assert_eq!(bullet.payload().base_damage, 350);
        assert_eq!(
            [
                bullet.launch_origin().x,
                bullet.launch_origin().y,
                bullet.launch_origin().z
            ],
            std::array::from_fn::<_, 3, _>(|i| integer(&fire["origin"][i])),
            "the physical action/waypoint reaches the native launch point"
        );
        assert!(!bullet.payload().bright());
    }

    // The original bounded descending history, through the actual frame
    // consumer. Other cells/actors and full map startup are not certified.
    let flight = &native()["production_flight"];
    let input = &flight["input"];
    let at = (
        integer(&input["target_cell"][0]) as u16,
        integer(&input["target_cell"][1]) as u16,
    );
    let (rules, mut sim, _) = crate::sim::superweapon::chronosphere_tests::world_with(
        rules,
        integer(&input["map_size"][0]) as u16,
        &[(at, integer(&input["flat_level"]) as u8)],
    );
    let entry = map.actions[retail["actions"][0]["trigger_id"].as_str().unwrap()]
        .entries()
        .iter()
        .find(|entry| entry.kind == 95)
        .unwrap()
        .clone();
    let waypoint = entry.waypoint_index.unwrap();
    sim.session.install_waypoints(&HashMap::from([(
        waypoint,
        Waypoint {
            index: waypoint,
            rx: at.0,
            ry: at.1,
        },
    )]));
    assert!(run(&mut sim, &rules, entry));
    let (&id, bullet) = sim.projectiles.iter().next().unwrap();
    assert_eq!(
        [
            bullet.position().x,
            bullet.position().y,
            bullet.position().z
        ],
        std::array::from_fn::<_, 3, _>(|i| integer(&flight["launch"]["position"][i])),
    );
    for frame in flight["frames"].as_array().unwrap() {
        crate::sim::superweapon::chronosphere_tests::step(&mut sim, &rules);
        let bullet = sim
            .projectiles
            .get(id)
            .expect("the native descending missile is alive");
        assert_eq!(
            [
                bullet.position().x,
                bullet.position().y,
                bullet.position().z
            ],
            std::array::from_fn::<_, 3, _>(|i| integer(&frame["position"][i])),
            "native frame {}",
            frame["frame"],
        );
        assert_eq!(
            bullet.velocity().native().map(|v| v.bits()),
            velocity_bits(&frame["velocity_bits"])
        );
        assert_eq!(
            bullet.awaiting_anim(),
            frame["impact_entry"].as_bool().unwrap()
        );
    }

    // One ordinary ground path through production frames, including actual
    // SHP-bound NUKEBALL retirement, radiation and a damaged receiver. This
    // is a Rust integration regression, not a full-map native comparison.
    let (rules, mut sim, _) =
        crate::sim::superweapon::chronosphere_tests::world_with(rules, 64, &[]);
    // The native Map Size diamond requires x+y>64. The rectangular terrain
    // crop alone does not make (30,30) playable: that exercises map exit.
    let impact_cell = (40, 40);
    let victim = sim
        .spawn_object_at_height(
            "MTNK",
            "Americans",
            impact_cell.0,
            impact_cell.1,
            0,
            0,
            &rules,
        )
        .unwrap();
    let health = sim.entities().get(victim).unwrap().health.current();
    let entry = map.actions[retail["actions"][0]["trigger_id"].as_str().unwrap()]
        .entries()
        .iter()
        .find(|entry| entry.kind == 95)
        .unwrap()
        .clone();
    let waypoint = entry.waypoint_index.unwrap();
    sim.session.install_waypoints(&HashMap::from([(
        waypoint,
        Waypoint {
            index: waypoint,
            rx: impact_cell.0,
            ry: impact_cell.1,
        },
    )]));
    assert!(run(&mut sim, &rules, entry));
    let id = *sim.projectiles.iter().next().unwrap().0;
    let mut waited = false;
    for _ in 0..1200 {
        crate::sim::superweapon::chronosphere_tests::step(&mut sim, &rules);
        match sim.projectiles.get(id) {
            Some(bullet) => waited |= bullet.awaiting_anim(),
            None => break,
        }
    }
    assert!(waited, "the bound NUKEBALL mediates delayed detonation");
    assert!(sim.projectiles.get(id).is_none(), "the missile retires");
    assert!(
        sim.entities()
            .get(victim)
            .is_none_or(|e| e.health.current() < health)
    );
    assert!(sim.radiation.site_at(impact_cell).is_some());
}
