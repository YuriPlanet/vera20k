//! FireAt6FF4CC/SpawnLaser6FD210 and support44ABD0 constructor comparisons.
//! Original Unit GetFLH and allocation failure are explicit native boundaries;
//! these tests do not claim Unit pose or allocator-failure parity.

use super::laser::{self, LaserBirth, LaserColor};
use super::{TargetKind, WeaponSlot};
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::timer::CdTimer;
use crate::sim::world::{DelayedFire, PendingBuildingFire};
use crate::sim::world::{LifecycleOutput, Simulation, TickLane};
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/building_prism.json")
}

fn xyz(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| value[axis].as_i64().unwrap() as i32)
}

fn coordinate(value: &Value) -> ProjectileCoord {
    let [x, y, z] = xyz(value);
    ProjectileCoord::new(x, y, z)
}

fn values_or(value: &Value, fallback: [i32; 3]) -> [i32; 3] {
    if value.is_array() {
        xyz(value)
    } else {
        fallback
    }
}

fn weapon_ini(name: &str, input: &Value) -> String {
    let is_laser = input["is_laser"].as_bool().unwrap_or(true);
    let house = input["house_color"].as_bool().unwrap_or(true);
    let single = input["single_color"].as_bool().unwrap_or(false);
    let big = input["big_laser"].as_bool().unwrap_or(false);
    let duration = input["duration_byte"].as_i64().unwrap_or(15);
    let [r, g, b] = values_or(&input["inner"], [100, 120, 140]);
    let [or, og, ob] = values_or(&input["outer"], [10, 20, 30]);
    let [sr, sg, sb] = values_or(&input["spread"], [0, 0, 0]);
    format!(
        "[{name}]\nIsLaser={is_laser}\nIsHouseColor={house}\nIsSingleColor={single}\nIsBigLaser={big}\n\
         LaserDuration={duration}\nLaserInnerColor={r},{g},{b}\n\
         LaserOuterColor={or},{og},{ob}\nLaserOuterSpread={sr},{sg},{sb}\n"
    )
}

fn fixture_rules(input: &Value) -> RuleSet {
    let support_duration = input["rules"]["duration"].as_i64().unwrap_or(15);
    let selected = weapon_ini("LaserProbe", input);
    let (primary, current) = if input["current_weapon"].is_object() {
        (
            "CurrentProbe",
            weapon_ini("CurrentProbe", &input["current_weapon"]),
        )
    } else {
        ("LaserProbe", String::new())
    };
    let rules_ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nPrismType=ATESLA\nPrismSupportDuration={support_duration}\nPrismSupportDelay=45\n\
         [VehicleTypes]\n0=UNIT\n1=TARGET\n[InfantryTypes]\n0=INF\n\
         [BuildingTypes]\n0=ATESLA\n1=OTHER\n2=GAPOWR\n\
         [ATESLA]\nImage=GAPRIS\nStrength=600\nTurret=no\nPrimary={primary}\nSecondary=LaserProbe\n\
         [OTHER]\nImage=OTHERART\nStrength=600\nTurret=no\nPrimary={primary}\nSecondary=LaserProbe\n\
         [GAPOWR]\nStrength=750\n\
         [UNIT]\nImage=UNITART\nStrength=600\nPrimary=LaserProbe\n\
         [INF]\nStrength=125\nPrimary=LaserProbe\n\
         [TARGET]\nStrength=1000\n{selected}{current}"
    ));
    let art_ini = IniFile::from_str(
        "[GAPRIS]\nFoundation=1x1\nPrimaryFirePixelOffset=0,-4\n\
         PrimaryFireDualOffset=yes\nPrimaryFireFLH=0,0,378\n\
         [OTHERART]\nFoundation=1x1\nPrimaryFirePixelOffset=0,-4\n\
         [GAPOWR]\nFoundation=2x2\n[UNITART]\nPrimaryFireFLH=0,0,0\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    rules
}

fn place(sim: &mut Simulation, id: u64, kind: &str, category: EntityCategory, at: [i32; 3]) {
    let mut entity = GameEntity::test_default(
        id,
        kind,
        "Americans",
        (at[0] / 256) as u16,
        (at[1] / 256) as u16,
    );
    entity.set_type_ref_for_test(sim.intern(kind));
    entity.set_owner_for_test(sim.intern("Americans"));
    entity.set_category_for_test(category);
    entity
        .position
        .set_sub_x_for_test(SimFixed::from_num(at[0] % 256));
    entity
        .position
        .set_sub_y_for_test(SimFixed::from_num(at[1] % 256));
    entity.position.set_exact_z_leptons_for_test(Some(at[2]));
    entity
        .type_facts
        .set_foundation_for_test(if kind == "GAPOWR" { "2x2" } else { "1x1" }.into());
    sim.substrate.entities.insert(entity);
}

fn assert_birth(actual: LaserBirth, expected: &Value, name: &str) {
    assert_eq!(
        actual.frame,
        expected["timer_start"].as_i64().unwrap() as i32,
        "{name} frame"
    );
    assert_eq!(
        actual.from,
        coordinate(&expected["source"]),
        "{name} source"
    );
    assert_eq!(actual.to, coordinate(&expected["target"]), "{name} target");
    assert_eq!(
        actual.z_adjust,
        expected["z_adjust"].as_i64().unwrap() as i32,
        "{name} Z adjustment"
    );
    assert_eq!(
        actual.duration,
        expected["duration"].as_i64().unwrap() as i32,
        "{name} duration"
    );
    assert_eq!(
        actual.width,
        expected["width"].as_i64().unwrap() as i32,
        "{name} width"
    );
    assert_eq!(
        actual.supported,
        expected["supported"] == 1,
        "{name} support flag"
    );
}

#[test]
fn admitted_laser_births_match_original_building_and_unit_callers() {
    let corpus = native();
    let rows = corpus["laser_birth"].as_array().unwrap();
    assert_eq!(rows.len(), 29);
    let mut compared = 0;
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        if input["new_fails"] == true {
            // Rust Vec allocation is not a nullable native operator-new API.
            continue;
        }
        let rules = fixture_rules(input);
        let mut sim = Simulation::new();
        sim.session.set_binary_frame_for_test(200);
        let unit = input["unit_boundary"].is_array();
        let source_kind = if unit {
            "UNIT"
        } else if input["non_prism"] == true {
            "OTHER"
        } else {
            "ATESLA"
        };
        // Native Unit rows explicitly supply this GetFLH boundary. A zero-FLH
        // stationary Rust Unit represents that same admitted coordinate.
        let source_at = if unit {
            xyz(&input["unit_boundary"])
        } else {
            values_or(&input["location"], [3200, 3200, 0])
        };
        place(
            &mut sim,
            1,
            source_kind,
            if unit {
                EntityCategory::Unit
            } else {
                EntityCategory::Structure
            },
            source_at,
        );
        let building_target = input["target_building"].is_object();
        let target_at = if building_target {
            xyz(&input["target_building"]["location"])
        } else {
            values_or(&input["target_location"], [640, 640, 0])
        };
        place(
            &mut sim,
            2,
            if building_target { "GAPOWR" } else { "TARGET" },
            if building_target {
                EntityCategory::Structure
            } else {
                EntityCategory::Unit
            },
            target_at,
        );
        let owner = sim.substrate.entities.get(1).unwrap().owner();
        let count = input["master"]["count"].as_i64().unwrap_or(0) as i32;
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .prism_support_count
            .set_for_test(count);
        if input["kind"] == "support" {
            let to = if name == "production_support" {
                // Receiver FLH is independently established by the actual
                // native reader/GetFLH chain in this row and laser_flh.
                coordinate(&row["laser"]["target"])
            } else {
                let [x, y, z] = values_or(&input["payload"], [3600, 2700, 0]);
                ProjectileCoord::new(x, y, z)
            };
            laser::support(&mut sim, &rules, 1, to);
        } else {
            laser::fired(
                &mut sim,
                &rules,
                1,
                TargetKind::Entity(2),
                input["selected_slot"].as_i64().unwrap_or(0) as i32,
                rules.weapon("LaserProbe").unwrap(),
                count,
            );
        }
        let births = sim
            .lifecycle_outputs
            .iter()
            .filter_map(|output| match output {
                LifecycleOutput::LaserCreated(birth) => Some(*birth),
                _ => None,
            })
            .collect::<Vec<_>>();
        if row["laser"].is_null() {
            assert!(births.is_empty(), "{name}");
        } else {
            assert_eq!(births.len(), 1, "{name}");
            assert_birth(births[0], &row["laser"], name);
            if row["laser"]["house_color"] == 1 {
                assert_eq!(births[0].color, LaserColor::House(owner), "{name}");
            } else {
                let color = |key| xyz(&row["laser"][key]).map(|value| value as u8);
                assert_eq!(
                    births[0].color,
                    LaserColor::Explicit {
                        inner: color("inner"),
                        outer: color("outer"),
                        spread: color("spread"),
                    },
                    "{name}"
                );
            }
        }
        compared += 1;
    }
    assert_eq!(compared, 27);
}

/// FireAt `0x006FF549..0x006FF571`: an infantryman's (LUNR's) house-colour
/// laser is 2 wide like a Unit's; an explicit-colour one keeps 1.
#[test]
fn an_infantry_house_color_laser_is_two_wide() {
    for (house_color, width) in [(true, 2), (false, 1)] {
        let rules = fixture_rules(&json!({ "house_color": house_color }));
        let mut sim = Simulation::new();
        sim.session.set_binary_frame_for_test(200);
        place(
            &mut sim,
            1,
            "INF",
            EntityCategory::Infantry,
            [3200, 3200, 0],
        );
        place(&mut sim, 2, "TARGET", EntityCategory::Unit, [640, 640, 0]);
        laser::fired(
            &mut sim,
            &rules,
            1,
            TargetKind::Entity(2),
            0,
            rules.weapon("LaserProbe").unwrap(),
            0,
        );
        let widths = sim
            .lifecycle_outputs
            .iter()
            .filter_map(|output| match output {
                LifecycleOutput::LaserCreated(birth) => Some(birth.width),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(widths, [width], "house colour {house_color}");
    }
}

/// Phobos `TechnoClass_LaserZap_IsSingleColor` (`0x6FD446`): a stock FireAt
/// laser with `IsSingleColor=` and no house colour keeps only its
/// `LaserInnerColor=`; `IsHouseColor=` still wins, and an ordinary laser
/// keeps the full inner/outer/spread triple.
#[cfg(feature = "phobos")]
#[test]
fn stock_single_color_laser_keeps_only_the_inner_colour() {
    fn birth(
        house_color: bool,
        single_color: bool,
    ) -> (LaserColor, crate::sim::intern::InternedId) {
        let rules = fixture_rules(&json!({
            "house_color": house_color,
            "single_color": single_color,
            "inner": [11, 22, 33],
            "outer": [44, 55, 66],
            "spread": [7, 8, 9],
        }));
        let mut sim = Simulation::new();
        place(&mut sim, 1, "UNIT", EntityCategory::Unit, [1280, 1280, 0]);
        place(&mut sim, 2, "TARGET", EntityCategory::Unit, [1536, 1280, 0]);
        let owner = sim.substrate.entities.get(1).unwrap().owner();
        let birth = *laser::create(
            &mut sim,
            &rules,
            1,
            crate::sim::projectile::ProjectileTarget::Entity(2),
            0,
            rules.weapon("LaserProbe").unwrap(),
            Some(ProjectileCoord::new(1280, 1280, 0)),
            false,
        )
        .unwrap();
        (birth.color, owner)
    }

    assert_eq!(
        birth(false, true).0,
        LaserColor::Explicit {
            inner: [11, 22, 33],
            outer: [0; 3],
            spread: [0; 3]
        }
    );
    assert_eq!(
        birth(false, false).0,
        LaserColor::Explicit {
            inner: [11, 22, 33],
            outer: [44, 55, 66],
            spread: [7, 8, 9]
        }
    );
    let (color, owner) = birth(true, true);
    assert_eq!(color, LaserColor::House(owner));
}

#[test]
fn direct_laser_keeps_explicit_source_and_native_constructor_defaults() {
    let corpus = native();
    let row = corpus["laser_birth"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "production_main")
        .unwrap();
    let input = &row["input"];
    let rules = fixture_rules(input);
    let mut sim = Simulation::new();
    sim.session.set_binary_frame_for_test(200);
    place(
        &mut sim,
        1,
        "ATESLA",
        EntityCategory::Structure,
        xyz(&input["location"]),
    );
    place(
        &mut sim,
        2,
        "GAPOWR",
        EntityCategory::Structure,
        xyz(&input["target_building"]["location"]),
    );
    let owner = sim.substrate.entities.get(1).unwrap().owner();
    let from = ProjectileCoord::new(9000, 12000, 1000);
    let birth = *laser::create(
        &mut sim,
        &rules,
        1,
        crate::sim::projectile::ProjectileTarget::Entity(2),
        0,
        rules.weapon("LaserProbe").unwrap(),
        Some(from),
        false,
    )
    .unwrap();
    assert_eq!(birth.from, from);
    assert_eq!(birth.to, coordinate(&row["laser"]["target"]));
    // SpawnLaser6FD2C4 skips the building adjustment for an explicit source;
    // LaserDraw54FE60 defaults remain untouched without FireAt postprocessing.
    assert_eq!(birth.z_adjust, 0);
    assert_eq!((birth.width, birth.supported), (1, false));
    assert_eq!(
        birth.duration,
        row["laser"]["duration"].as_i64().unwrap() as i32
    );
    assert_eq!(birth.color, LaserColor::House(owner));

    for source in [None, Some(ProjectileCoord::new(0, 0, 0))] {
        let birth = *laser::create(
            &mut sim,
            &rules,
            1,
            crate::sim::projectile::ProjectileTarget::Entity(2),
            0,
            rules.weapon("LaserProbe").unwrap(),
            source,
            false,
        )
        .unwrap();
        assert_eq!(birth.from, coordinate(&row["laser"]["source"]));
        assert_eq!(
            birth.z_adjust,
            row["laser"]["z_adjust"].as_i64().unwrap() as i32
        );
        assert_eq!((birth.width, birth.supported), (1, false));
    }
}

#[test]
fn production_frame_orders_laser_update_before_delayed_support_birth() {
    let rules = fixture_rules(&json!({}));
    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let tower = sim
        .spawn_object("ATESLA", "Americans", 12, 12, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(200);
    let to = ProjectileCoord::new(3600, 2700, 0);
    let source = sim.substrate.entities.get_mut(tower).unwrap();
    source
        .pending_building_fire
        .set_for_test(Some(PendingBuildingFire::new(
            1,
            DelayedFire::SupportBeam { to },
        )));
    source.prism_support_count.set_for_test(2);
    source.mission_leaf.set_building_ready_latch(1);
    source.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Guard),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 200,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::from_raw(200, 100),
    });
    sim.lifecycle_outputs.clear_for_test();
    let frame = sim
        .advance_app_frame(
            &[],
            &rules,
            None,
            67,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    let relevant = frame
        .lifecycle_outputs
        .iter()
        .filter(|output| {
            matches!(
                output,
                LifecycleOutput::LaserUpdate { .. } | LifecycleOutput::LaserCreated(_)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(relevant.len(), 2, "{relevant:?}");
    assert_eq!(*relevant[0], LifecycleOutput::LaserUpdate { frame: 200 });
    let LifecycleOutput::LaserCreated(birth) = relevant[1] else {
        panic!("support birth missing")
    };
    let corpus = native();
    let expected = corpus["laser_birth"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "support")
        .unwrap();
    assert_birth(*birth, &expected["laser"], "production support visit");
    let source = sim.substrate.entities.get(tower).unwrap();
    assert!(source.pending_building_fire.pending().is_none());
    assert_eq!(source.prism_support_count.count(), 0);
    assert_eq!(
        source.rearm_timer.timer(),
        CdTimer::from_raw(
            expected["actor_after"]["rearm"][0].as_i64().unwrap() as i32,
            expected["actor_after"]["rearm"][1].as_i64().unwrap() as i32,
        )
    );
}

#[test]
fn retail_delayed_main_uses_preclear_count_and_failed_admission_emits_no_laser() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let corpus = native();
    let expected = corpus["laser_birth"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "production_main")
        .unwrap();
    for target_x in [45, 60] {
        let mut sim = Simulation::new();
        for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
            let owner = sim.intern(name);
            sim.houses
                .register(HouseState::new(owner, index as u8, None, true, 0, 10));
        }
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(64),
        );
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        sim.spawn_object("GAPOWR", "Americans", 32, 40, 0, rules)
            .unwrap();
        let tower = sim
            .spawn_object("ATESLA", "Americans", 38, 48, 0, rules)
            .unwrap();
        let target = sim
            .spawn_object("GAPOWR", "Russians", target_x, 48, 0, rules)
            .unwrap();
        sim.resolve_type_handles(rules);
        sim.power_states.map_mut_for_test().clear();
        sim.session.set_binary_frame_for_test(200);
        let source = sim.substrate.entities.get_mut(tower).unwrap();
        source
            .attack_target
            .set_for_test(Some(TargetKind::Entity(target)));
        source
            .pending_building_fire
            .set_for_test(Some(PendingBuildingFire::new(
                1,
                DelayedFire::Weapon(WeaponSlot::Primary),
            )));
        source.prism_support_count.set_for_test(1);
        source.rearm_timer.set_for_test(CdTimer::started(100, 0));
        source.mission_leaf.set_building_ready_latch(1);
        source.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Attack),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 200,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::from_raw(200, 100),
        });
        sim.lifecycle_outputs.clear_for_test();
        let frame = sim
            .advance_app_frame(
                &[],
                rules,
                None,
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        let update = frame
            .lifecycle_outputs
            .iter()
            .position(|output| matches!(output, LifecycleOutput::LaserUpdate { frame: 200 }))
            .unwrap();
        let births = frame
            .lifecycle_outputs
            .iter()
            .enumerate()
            .filter_map(|(index, output)| match output {
                LifecycleOutput::LaserCreated(birth) => Some((index, *birth)),
                _ => None,
            })
            .collect::<Vec<_>>();
        if target_x == 45 {
            assert_eq!(births.len(), 1, "{births:?}");
            assert!(update < births[0].0);
            assert_birth(births[0].1, &expected["laser"], "retail delayed main");
            assert_eq!(
                sim.substrate
                    .entities
                    .get(tower)
                    .unwrap()
                    .prism_support_count
                    .count(),
                0
            );
        } else {
            assert!(births.is_empty(), "out-of-range delayed shot");
            assert_eq!(
                sim.substrate
                    .entities
                    .get(tower)
                    .unwrap()
                    .prism_support_count
                    .count(),
                1
            );
        }
    }
}
