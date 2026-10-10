//! Native comparisons for firing a super weapon (`tools/superweapon_oracle.py`
//! sections `click_fire` and `defense_alert`; `--check` regenerates them).

use super::{alert_super_weapon_defense, click_fire};
use crate::map::resolved_terrain::{ResolvedTerrainCell, test_grid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::rng::SimRng;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::superweapon::cell_receiver_tests::{test_playfield_bounds, test_terrain_cell};
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::Value;

const RULES: &str = "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
    [BuildingTypes]\n0=NAMISL\n1=YARD\n2=BIGYARD\n\
    [SuperWeaponTypes]\n0=NukeSpecial\n1=LightningStormSpecial\n2=SpyPlaneSpecial\n\
    3=PsychicDominatorSpecial\n\
    [NukeSpecial]\nType=MultiMissile\nRechargeTime=1\nAIDefendAgainst=yes\n\
    [LightningStormSpecial]\nType=LightningStorm\nRechargeTime=1\n\
    [SpyPlaneSpecial]\nType=SpyPlane\nRechargeTime=1\n\
    [PsychicDominatorSpecial]\nType=PsychicDominator\nRechargeTime=1\n\
    [NAMISL]\nStrength=1000\nNukeSilo=yes\nSuperWeapon=NukeSpecial\nFoundation=1x1\n\
    [YARD]\nStrength=1000\nFoundation=1x1\n\
    [BIGYARD]\nStrength=1000\nFoundation=4x4\n";
const ART: &str = "[NAMISL]\nFoundation=1x1\n[YARD]\nFoundation=1x1\n[BIGYARD]\nFoundation=4x4\n";
const OWNER: &str = "Americans";

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

fn cell(value: &Value) -> (u16, u16) {
    (
        u16::try_from(int(&value[0])).unwrap(),
        u16::try_from(int(&value[1])).unwrap(),
    )
}

fn rules() -> RuleSet {
    let art = IniFile::from_str(ART);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(RULES), &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

/// One house on a 64x64 map whose cells stand at `levels`.
fn sim(rules: &RuleSet, human: bool, levels: &[((u16, u16), u8)]) -> Simulation {
    let mut sim = Simulation::with_seed(23);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern(OWNER);
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, human, 0, 10));
    sim.session.house_order.push(owner);
    sim.session.game_options.super_weapons = true;
    sim.resolved_terrain = Some(test_grid(64, 64, |x, y| ResolvedTerrainCell {
        level: levels
            .iter()
            .find(|(at, _)| *at == (x, y))
            .map_or(0, |&(_, level)| level),
        ..test_terrain_cell(x, y)
    }));
    sim.playfield_bounds = Some(test_playfield_bounds());
    sim
}

/// ClickFire on a Super in the row's state; a silo stands so a MultiMissile
/// launch is visible as `click_fire`'s result. The type flags and recharge
/// time are the row's (`SuperWeaponTypeClass +0xED/+0xEE/+0xF5/+0xB0`).
#[test]
fn click_fire_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "click_fire");
    assert_eq!(rows.len(), 126);
    let rules = rules();
    for row in rows {
        let mut sim = sim(&rules, true, &[]);
        let owner = sim.interner.intern(OWNER);
        sim.spawn_object_at_height("NAMISL", OWNER, 10, 10, 0, 0, &rules)
            .unwrap();
        let name = match int(&row["kind"]) {
            0 => "NukeSpecial",
            2 => "LightningStormSpecial",
            7 => "PsychicDominatorSpecial",
            other => panic!("Type={other}"),
        };
        let sw_type_id = sim.interner.intern(name);
        let mut sw = rules.super_weapon(name).unwrap().clone();
        sw.pre_click = flag(&row["pre_click"]);
        sw.post_click = flag(&row["post_click"]);
        sw.manual_control = flag(&row["manual"]);
        sw.recharge_time_frames = int(&row["recharge"]);
        sim.session.binary_frame = u32::try_from(int(&row["frame"])).unwrap();
        let mut instance = SuperWeaponInstance::new(sw_type_id, owner, 0);
        instance.is_active = flag(&row["granted"]);
        instance.is_ready = flag(&row["charged"]);
        instance.is_suspended = flag(&row["on_hold"]);
        instance.charge_start_tick = int(&row["start"]);
        instance.charge_duration = int(&row["left"]);
        sim.super_weapons
            .entry(owner)
            .or_default()
            .insert(sw_type_id, instance);
        if flag(&row["dominator_active"]) {
            // A Dominator already rises (`PsyDom::Active @ 0x0053B400`).
            sim.psychic_dominator =
                super::super::psychic_dominator::PsychicDominatorState::for_test(
                    2,
                    (3, 3),
                    Some(owner),
                    None,
                );
        }
        if flag(&row["deferment"]) {
            // A storm already counts down.
            super::super::lightning_storm::start(&mut sim, &rules, 180, 250, (3, 3), Some(owner));
        }
        let launched = click_fire(
            &mut sim,
            &rules,
            owner,
            sw_type_id,
            &sw,
            (33, 44),
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let events = row["events"].as_array().unwrap();
        let called = |name: &str| events.iter().any(|event| event[0] == name);
        assert_eq!(launched, called("launch"), "{row}");
        // A refusal arm (`0x006CBAA7`, `0x006CBAD6`): native asked its
        // predicate and the row's stub answered yes. Its line prints only
        // for the row's `player` (Fire_SW's `this == PlayerPtr`); VERA
        // reports every refusal and the app posts the line for the local
        // player's house (`sound_dispatch::super_weapon_messages`).
        let player = flag(&row["player"]);
        let storm_refused = called("has_deferment") && flag(&row["deferment"]);
        let dominator_refused =
            called("psychic_dominator_active") && flag(&row["dominator_active"]);
        assert_eq!(called("storm_message"), storm_refused && player, "{row}");
        assert_eq!(
            called("dominator_message"),
            dominator_refused && player,
            "{row}"
        );
        let refusals: Vec<_> = sim
            .sound_events
            .iter()
            .filter_map(|event| match *event {
                SimSoundEvent::LightningStormRefused { owner } => Some(("storm", owner)),
                SimSoundEvent::PsychicDominatorRefused { owner } => Some(("dominator", owner)),
                _ => None,
            })
            .collect();
        let expected: Vec<_> = [("storm", storm_refused), ("dominator", dominator_refused)]
            .into_iter()
            .filter(|&(_, refused)| refused)
            .map(|(arm, _)| (arm, owner))
            .collect();
        assert_eq!(refusals, expected, "{row}");
        let after = &sim.super_weapons[&owner][&sw_type_id];
        assert_eq!(
            (
                after.is_active,
                after.is_ready,
                after.is_suspended,
                after.charge_start_tick,
                after.charge_duration
            ),
            (
                flag(&row["granted_after"]),
                flag(&row["charged_after"]),
                flag(&row["on_hold_after"]),
                int(&row["start_after"]),
                int(&row["left_after"]),
            ),
            "{row}"
        );
    }
}

/// The launch alert for one house, with the row's draw answer; the yard
/// stands where its GetCoords is the row's.
#[test]
fn the_launch_alert_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "defense_alert");
    assert_eq!(rows.len(), 75);
    let mut rules = rules();
    for row in rows {
        let mut levels: Vec<((u16, u16), u8)> = row["levels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|level| (cell(level), u8::try_from(int(&level[2])).unwrap()))
            .collect();
        let yard = row["yard"].as_array().map(|coords| {
            let coords: Vec<i32> = coords.iter().map(int).collect();
            // A 1x1 yard's centre is its cell's; a 4x4 one's is 512 in.
            let (kind, inset) = if coords[0] % 256 == 128 {
                ("YARD", 128)
            } else {
                ("BIGYARD", 512)
            };
            let at = (
                u16::try_from((coords[0] - inset) / 256).unwrap(),
                u16::try_from((coords[1] - inset) / 256).unwrap(),
            );
            let level = u8::try_from(i64::from(coords[2]) / crate::util::lepton::LEPTONS_PER_LEVEL)
                .unwrap();
            if level != 0 {
                levels.push((at, level));
            }
            (kind, at, coords)
        });
        rules.general.ai_super_defense_distance = int(&row["distance"]);
        rules.general.ai_super_defense_probability = row["probability"]
            .as_array()
            .unwrap()
            .iter()
            .map(int)
            .collect();
        let mut sw = rules.super_weapon("NukeSpecial").unwrap().clone();
        sw.ai_defend_against = flag(&row["defend"]);
        let mut sim = sim(&rules, flag(&row["human"]), &levels);
        let house_id = sim.interner.intern(OWNER);
        if let Some((kind, (rx, ry), coords)) = yard {
            let id = sim
                .spawn_object_at_height(kind, OWNER, rx, ry, 0, 0, &rules)
                .unwrap();
            let at = crate::sim::movement::ground_pose::object_get_coords(
                sim.substrate.entities.get(id).unwrap(),
                sim.resolved_terrain.as_ref(),
            );
            assert_eq!(
                [at.x, at.y, at.z],
                [coords[0], coords[1], coords[2]],
                "{row}"
            );
            sim.houses.get_mut(&house_id).unwrap().build_const_order = vec![id];
        }
        {
            let house = sim.houses.get_mut(&house_id).unwrap();
            house.multiplay_passive = flag(&row["passive"]);
            house.difficulty = HouseDifficulty::from_native(int(&row["difficulty"])).unwrap();
            house.base_center = Some(cell(&row["base"]));
            house.alternate_base_center = cell(&row["alternate"]);
        }
        sim.session.binary_frame = u32::try_from(int(&row["frame"])).unwrap();
        sim.scenario_rng = SimRng::answering(0, 99, int(&row["answer"]));
        // RandomRanged's own retries draw more raw words; count its calls by
        // the cursor they leave.
        let mut drawn = sim.scenario_rng.clone();
        let native_draws = row["draws"].as_array().unwrap();
        for draw in native_draws {
            // The Scenario stream (`+0x218`), RandomRanged(0, 99).
            assert_eq!(
                (int(&draw[0]), int(&draw[1]), int(&draw[2])),
                (0x218, 0, 99)
            );
            drawn.next_range_i32_inclusive(0, 99);
        }
        let before = sim.houses[&house_id].super_weapon_defense();
        alert_super_weapon_defense(&mut sim, &rules, house_id, &sw, cell(&row["cell"]));
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            drawn.native_state_hex(),
            "{row}"
        );
        let after = sim.houses[&house_id].super_weapon_defense();
        if int(&row["defense_frame"]) == -100 {
            assert_eq!(after, before, "{row}");
        } else {
            assert_eq!(
                after,
                (cell(&row["defense_cell"]), int(&row["defense_frame"])),
                "{row}"
            );
        }
    }
}

/// `HouseClass @ 0x0050AF10` holds a charged powered Super while its house
/// is short of power (`SuperClass::Suspend @ 0x006CB4D0` stops the timer),
/// so ClickFire refuses it; restored power releases it.
#[test]
fn a_charged_nuke_held_for_low_power_does_not_launch() {
    let rules = rules();
    let mut sim = sim(&rules, true, &[]);
    let owner = sim.interner.intern(OWNER);
    sim.spawn_object_at_height("NAMISL", OWNER, 10, 10, 0, 0, &rules)
        .unwrap();
    let nuke = sim.interner.intern("NukeSpecial");
    let sw = rules.super_weapon("NukeSpecial").unwrap();
    let mut instance = SuperWeaponInstance::new(nuke, owner, 0);
    instance.activate(10, 0);
    instance.is_ready = true;
    sim.super_weapons
        .entry(owner)
        .or_default()
        .insert(nuke, instance);
    sim.super_weapons_initialized = true;
    sim.session.binary_frame = 20;
    sim.power_states.entry(owner).or_default().is_low_power = true;
    super::super::tick_superweapon_instances(&mut sim, &rules);
    let held = &sim.super_weapons[&owner][&nuke];
    assert_eq!(
        (held.is_ready, held.is_suspended, held.charge_start_tick),
        (true, true, -1)
    );
    assert!(!click_fire(
        &mut sim,
        &rules,
        owner,
        nuke,
        sw,
        (33, 44),
        None,
        crate::sim::world::FrameEffects::default()
    ));
    sim.power_states.get_mut(&owner).unwrap().is_low_power = false;
    sim.session.binary_frame = 30;
    super::super::tick_superweapon_instances(&mut sim, &rules);
    assert!(click_fire(
        &mut sim,
        &rules,
        owner,
        nuke,
        sw,
        (33, 44),
        None,
        crate::sim::world::FrameEffects::default()
    ));
}
