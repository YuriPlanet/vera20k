//! Ordinary retail firing and original Techno6FD620 copied mode0/1 births.
//! The native component supplies declared inputs and independent expected
//! fields; the production FLH/depth owners remain the only implementation.

use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::RetailBattleRules;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::components::DriveCoord;
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
use crate::sim::rng::SimRng;
use crate::sim::vision::FogState;
use crate::sim::world::{LifecycleOutput, Simulation};
use serde_json::Value;

use super::radiation_beam::CopiedBeamMode;

fn native() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/rad_beam.json")
}

fn native_chrono() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/chrono_beam.json")
}

fn integer(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn xyz(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| integer(&value[axis]))
}

fn rgb(value: &Value) -> [u8; 3] {
    std::array::from_fn(|channel| u8::try_from(integer(&value[channel])).unwrap())
}

fn coordinate(value: &Value) -> ProjectileCoord {
    let [x, y, z] = xyz(value);
    ProjectileCoord::new(x, y, z)
}

fn retail_for_map(name: &str) -> Option<RetailBattleRules> {
    let map = crate::test_fixture::checkout_root()
        .join("tools/procedural_drawing_oracle/rad_beam")
        .join(name);
    crate::rules::retail_ini_fixture::retail_battle_rules_for_map(map.to_str().unwrap())
}

fn retail() -> Option<RetailBattleRules> {
    retail_for_map("ordinary-deso-radbeam.map")
}

#[test]
fn omitted_rad_color_uses_the_original_black_constructor_default() {
    // Rules665663 zeroes EBX;6677EF/F5/FB seed the three bytes. Physical
    // stock green is an explicit Radiation/RadColor read, not this default.
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str("")).expect("default rules");
    assert_eq!(rules.radiation.color, (0, 0, 0));
    assert_eq!(
        [
            rules.radiation.color.0,
            rules.radiation.color.1,
            rules.radiation.color.2
        ],
        rgb(&native()["inputs"]["constructor"]["rad_color"]),
        "original Rules constructor"
    );
    // Only the two missing-key controls belong to this constructor-default
    // regression. Ordered retained-value histories remain a separate owner.
    for row in native()["color_controls"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        if !matches!(name, "absent_section" | "present_missing_key") {
            continue;
        }
        let rules = RuleSet::from_ini(&crate::test_fixture::ini_sections(&row["sections"]))
            .expect("declared native color input");
        assert_eq!(
            [
                rules.radiation.color.0,
                rules.radiation.color.1,
                rules.radiation.color.2
            ],
            rgb(&row["rgb"]),
            "{name} native current constructor default"
        );
    }
}

#[test]
fn retail_deso_attack_emits_a_copied_straight_radiation_beam() {
    let Some(retail) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let deso = rules.object("DESO").expect("retail DESO");
    assert_eq!(deso.primary(), Some("RadBeamWeapon"));
    let weapon = rules.weapon("RadBeamWeapon").unwrap();
    assert!(weapon.is_rad_beam);
    assert!(!weapon.is_laser);
    assert!(!weapon.is_electric_bolt);
    assert!(
        !rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .temporal
    );
    assert_eq!(rules.radiation.color, (0, 255, 0));

    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0xC29D_E500);
    sim.fog = FogState::new(32, 32);
    let owner = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let source = sim
        .spawn_object("DESO", "Americans", 16, 16, 64, rules)
        .unwrap();
    let target = sim
        .spawn_object("MTNK", "Russians", 20, 16, 192, rules)
        .unwrap();
    sim.fog.mark_visible_for_owner(owner, 20, 16);
    assert_eq!(
        sim.substrate.entities.get(source).unwrap().health.current(),
        deso.strength
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: source,
            target_id: target
        },
        Some(rules),
    ));
    sim.lifecycle_outputs.clear();
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..32 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        if let Some(shot) = sim
            .fire_events
            .iter()
            .find(|shot| shot.attacker_id == source)
        {
            assert_eq!(sim.interner.resolve(shot.weapon_id), "RadBeamWeapon");
            let beams: Vec<_> = sim
                .lifecycle_outputs
                .iter()
                .filter_map(|output| {
                    if let LifecycleOutput::RadiationBeamCreated(birth) = output {
                        Some(birth)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                beams.len(),
                1,
                "native FireAt6FF5C6 must create its straight beam on the ordinary shot"
            );
            let beam = beams[0];
            assert_ne!(beam.from, beam.to);
            assert_eq!(beam.color, [0, 255, 0]);
            assert_eq!(beam.duration, 15);
            assert_eq!(beam.amplitude.bits(), 40.0_f64.to_bits());
            return;
        }
    }
    panic!("ordinary in-range retail DESO did not fire within32 ticks");
}

#[test]
fn retail_cleg_first_attack_emits_a_copied_chrono_beam() {
    let Some(retail) = retail_for_map("ordinary-cleg-chrono.map") else {
        return;
    };
    let rules = &retail.rules;
    let cleg = rules.object("CLEG").expect("retail CLEG");
    assert_eq!(cleg.primary(), Some("NeutronRifle"));
    let weapon = rules.weapon("NeutronRifle").unwrap();
    assert!(weapon.is_rad_beam);
    assert!(!weapon.is_laser && !weapon.is_electric_bolt);
    assert!(
        rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .temporal
    );

    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0xC30C_1E60);
    sim.fog = FogState::new(32, 32);
    let owner = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let source = sim
        .spawn_object("CLEG", "Americans", 16, 16, 64, rules)
        .unwrap();
    let target = sim
        .spawn_object("MTNK", "Russians", 20, 16, 192, rules)
        .unwrap();
    sim.fog.mark_visible_for_owner(owner, 20, 16);
    let actor = sim.substrate.entities.get(source).unwrap();
    assert_eq!(actor.health.current(), cleg.strength);
    assert_eq!(
        actor.temporal.warp_target(),
        None,
        "first shot has no held target"
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: source,
            target_id: target,
        },
        Some(rules)
    ));
    sim.lifecycle_outputs.clear();
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..32 {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        if let Some(shot) = sim
            .fire_events
            .iter()
            .find(|shot| shot.attacker_id == source)
        {
            assert_eq!(sim.interner.resolve(shot.weapon_id), "NeutronRifle");
            let beams: Vec<_> = sim
                .lifecycle_outputs
                .iter()
                .filter_map(|output| {
                    if let LifecycleOutput::RadiationBeamCreated(birth) = output {
                        Some(birth)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                beams.len(),
                1,
                "native Infantry51DF60/FireAt6FF5B7 creates mode1 on the first ordinary Temporal shot"
            );
            let beam = beams[0];
            assert_ne!(beam.from, beam.to);
            assert_eq!(
                beam.color,
                [128, 200, 255],
                "physical AudioVisual/ChronoBeamColor"
            );
            assert_eq!(beam.duration, 15);
            assert_eq!(beam.amplitude.bits(), 40.0_f64.to_bits());
            return;
        }
    }
    panic!("ordinary in-range retail CLEG did not fire within32 ticks");
}

#[test]
fn retail_deso_selected_readers_match_the_native_mode0_inputs() {
    let Some(retail) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let expected = &native()["inputs"]["final"];
    assert_eq!(
        [
            rules.radiation.color.0,
            rules.radiation.color.1,
            rules.radiation.color.2
        ],
        rgb(&expected["rad_color"])
    );
    assert_selected_inputs(rules, "DESO", &expected["deso"], expected);
}

#[test]
fn retail_cleg_selected_readers_match_the_native_mode1_inputs() {
    let Some(retail) = retail_for_map("ordinary-cleg-chrono.map") else {
        return;
    };
    let expected = &native_chrono()["inputs"]["final"];
    let cleg = retail.rules.object("CLEG").unwrap();
    assert_eq!(
        cleg.locomotor,
        crate::rules::locomotor_type::LocomotorKind::Teleport,
        "native-read GUID selects the declared Teleport718000 component"
    );
    assert_eq!(
        cleg.teleporter,
        expected["cleg"]["teleporter"].as_bool().unwrap()
    );
    assert_eq!(
        retail.rules.general.chrono_beam_color,
        rgb(&expected["chrono_beam_color"])
    );
    assert_selected_inputs(&retail.rules, "CLEG", &expected["cleg"], expected);
}

fn assert_selected_inputs(rules: &RuleSet, actor: &str, type_fields: &Value, expected: &Value) {
    let object = rules.object(actor).unwrap();
    assert_eq!(object.primary(), type_fields["primary"].as_str());
    assert_eq!(
        object.elite_primary(),
        type_fields["elite_primary"].as_str()
    );
    assert_eq!(object.strength, integer(&type_fields["strength"]));
    let art = super::fire_coord::firer_art(rules, object).unwrap();
    assert_eq!(
        <[i32; 3]>::from(art.primary_fire_flh),
        xyz(&type_fields["primary_flh"])
    );
    assert_eq!(art.turret_offset, integer(&type_fields["turret_offset"]));
    for (name, expected) in expected["weapons"].as_object().unwrap() {
        let weapon = rules.weapon(name).unwrap();
        assert_eq!(weapon.damage, integer(&expected["damage"]), "{name} Damage");
        assert_eq!(
            weapon.range_leptons,
            integer(&expected["range"]),
            "{name} Range"
        );
        assert_eq!(
            weapon.projectile.as_deref(),
            expected["projectile"].as_str(),
            "{name} Projectile"
        );
        assert_eq!(
            weapon.warhead.as_deref(),
            expected["warhead"].as_str(),
            "{name} Warhead"
        );
        assert_eq!(
            weapon.is_laser,
            expected["is_laser"].as_bool().unwrap(),
            "{name} IsLaser"
        );
        assert_eq!(
            weapon.is_electric_bolt,
            expected["is_electric_bolt"].as_bool().unwrap(),
            "{name} IsElectricBolt"
        );
        assert_eq!(
            weapon.is_rad_beam,
            expected["is_rad_beam"].as_bool().unwrap(),
            "{name} IsRadBeam"
        );
        assert_eq!(
            weapon.is_rad_eruption,
            expected["is_rad_eruption"].as_bool().unwrap(),
            "{name} IsRadEruption"
        );
    }
    for (name, expected) in expected["warheads"].as_object().unwrap() {
        let warhead = rules.warhead(name).unwrap();
        assert_eq!(
            warhead.temporal,
            expected["temporal"].as_bool().unwrap(),
            "{name} Temporal"
        );
        assert_eq!(
            warhead.radiation,
            expected["radiation"].as_bool().unwrap(),
            "{name} Radiation"
        );
    }
}

fn rules_for_birth(retail: &RetailBattleRules, actor: &str, input: &Value) -> RuleSet {
    let mut rules = retail.rules.clone();
    if let Some(control) = input["art_flh_control"].as_str() {
        let mut art = (*retail.fixed_art).clone();
        art.merge(&IniFile::from_str(&format!(
            "[{actor}]\nPrimaryFireFLH={control}\n"
        )));
        // Parse the declared ART string through the production owner. Never
        // install row.physical_flh or row.beam.source as an implementation input.
        rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
    }
    rules
}

fn place_native_actor(
    sim: &mut Simulation,
    rules: &RuleSet,
    actor: &str,
    owner: &str,
    at: &Value,
) -> u64 {
    let [x, y, z] = xyz(at);
    let id = sim
        .spawn_object(actor, owner, (x / 256) as u16, (y / 256) as u16, 0, rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .position
        .put_location(DriveCoord { x, y, z });
    id
}

#[test]
fn six_native_deso_births_use_the_shared_flh_and_effect_depth_owners() {
    let Some(retail) = retail() else {
        return;
    };
    let rows = native()["birth_rows"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        6,
        "four cardinal headings plus positive depth/equal raw Y"
    );
    assert_native_births(&retail, "DESO", native(), CopiedBeamMode::Radiation);
}

#[test]
fn four_native_cleg_births_use_the_shared_flh_and_effect_depth_owners() {
    let Some(retail) = retail_for_map("ordinary-cleg-chrono.map") else {
        return;
    };
    assert_eq!(native_chrono()["birth_rows"].as_array().unwrap().len(), 4);
    assert_native_births(&retail, "CLEG", native_chrono(), CopiedBeamMode::Chrono);
}

fn assert_native_births(
    retail: &RetailBattleRules,
    actor: &str,
    native: &Value,
    mode: CopiedBeamMode,
) {
    for row in native["birth_rows"].as_array().unwrap() {
        let input = &row["input"];
        let expected = &row["beam"];
        let name = input["name"].as_str().unwrap();
        let rules = rules_for_birth(retail, actor, input);
        assert_eq!(
            <[i32; 3]>::from(
                super::fire_coord::firer_art(&rules, rules.object(actor).unwrap())
                    .unwrap()
                    .primary_fire_flh
            ),
            xyz(&row["physical_flh"]),
            "{name} original ART reader"
        );
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0xC29D_E500, 64);
        let source = place_native_actor(&mut sim, &rules, actor, "Americans", &input["location"]);
        let target = place_native_actor(&mut sim, &rules, actor, "Russians", &input["target"]);
        sim.resolve_type_handles(&rules);
        // Explicit native component frame (rad_beam.py PhysicalBirth setup),
        // not a simulated scenario clock or timing-event history.
        sim.session.set_binary_frame_for_test(200);
        let source_entity = sim.substrate.entities.get_mut(source).unwrap();
        source_entity
            .body_facing
            .snap(u16::try_from(integer(&input["heading"])).unwrap(), 200);
        assert_eq!(source_entity.weapon_burst.index(), integer(&input["burst"]));
        assert_eq!(source_entity.veterancy.rank(), 0, "{name} rookie input");
        let facing_before = source_entity.body_facing;
        let rng_before = &native["birth_rng_before"];
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(rng_before["main"].as_str().unwrap()).into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(rng_before["scenario"].as_str().unwrap());
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(rng_before["mapgen"].as_str().unwrap());
        let id_before = sim.native_unique_ids.as_ref().unwrap().current_raw();
        let stable_before = sim.next_stable_object_id();
        let logic_before = sim.live_object_order_snapshot();
        sim.lifecycle_outputs.clear();
        super::radiation_beam::create(
            &mut sim,
            &rules,
            source,
            ProjectileTarget::Entity(target),
            mode,
        );
        let [LifecycleOutput::RadiationBeamCreated(birth)] = sim.lifecycle_outputs.as_slice()
        else {
            panic!(
                "{name} expected exactly one copied beam: {:?}",
                sim.lifecycle_outputs
            );
        };
        assert_eq!(birth.frame, 200, "{name} birth frame input");
        assert_eq!(
            birth.from,
            coordinate(&expected["source"]),
            "{name} native GetFLH index0"
        );
        assert_eq!(
            birth.to,
            coordinate(&expected["target"]),
            "{name} native nonbuilding target"
        );
        assert_eq!(
            birth.z_adjust,
            integer(&expected["z_adjust"]),
            "{name} raw-Y effect adjustment"
        );
        assert_eq!(
            birth.color,
            rgb(&expected["rgb"]),
            "{name} native copied RGB"
        );
        assert_eq!(
            birth.duration,
            integer(&expected["duration"]),
            "{name} native duration"
        );
        assert_eq!(
            birth.amplitude.bits(),
            u64::from_str_radix(expected["amplitude_f64_le_hex"].as_str().unwrap(), 16)
                .unwrap()
                .swap_bytes(),
            "{name} original binary64 amplitude"
        );
        let rng_after = &native["birth_rng_after"];
        assert_eq!(
            sim.main_rng.native_state_hex(),
            rng_after["main"].as_str().unwrap(),
            "{name} full Main RNG"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            rng_after["scenario"].as_str().unwrap(),
            "{name} full Scenario RNG"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            rng_after["mapgen"].as_str().unwrap(),
            "{name} full Mapgen RNG"
        );
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw(),
            id_before,
            "{name} no Abstract ID"
        );
        assert_eq!(
            sim.next_stable_object_id(),
            stable_before,
            "{name} no simulation object allocation"
        );
        assert_eq!(
            sim.live_object_order_snapshot(),
            logic_before,
            "{name} no Logic registration"
        );
        let source_entity = sim.substrate.entities.get(source).unwrap();
        assert_eq!(
            source_entity.body_facing, facing_before,
            "{name} facing timer unchanged"
        );
        assert_eq!(
            source_entity.weapon_burst.index(),
            integer(&input["burst"]),
            "{name} burst unchanged"
        );
    }
}
