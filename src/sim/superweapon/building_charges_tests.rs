//! VERA's Superweapon Limit (`SuperWeaponPerBuilding=`,
//! `docs/superweapon-limit.md`): each such building charges its own shot.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;

const OWNER: &str = "Americans";
const REVEAL: &str = "RevealSpecial";

fn rules() -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
        [BuildingTypes]\n0=SILO\n1=LAB\n2=NUKES\n3=RADAR\n\
        [SILO]\nStrength=1000\nSuperWeapon=RevealSpecial\nSuperWeaponPerBuilding=yes\n\
        [LAB]\nStrength=1000\nSuperWeapon=RevealSpecial\n\
        [RADAR]\nStrength=1000\nRadar=yes\n\
        [NUKES]\nStrength=1000\nNukeSilo=yes\nSuperWeapon=NukeSpecial\nSuperWeaponPerBuilding=yes\n\
        [SuperWeaponTypes]\n0=RevealSpecial\n1=NukeSpecial\n\
        [RevealSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
        [NukeSpecial]\nType=MultiMissile\nRechargeTime=1\nIsPowered=no\nShowTimer=yes\n",
    );
    let art = IniFile::from_str(
        "[SILO]\nFoundation=1x1\n[LAB]\nFoundation=1x1\n[NUKES]\nFoundation=1x1\n\
         [RADAR]\nFoundation=1x1\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap()
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(23);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern(OWNER);
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 2000, 10));
    construct_house_supers(&mut sim, rules, owner, false);
    sim.session.game_options_mut_for_test().super_weapons = true;
    *sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |x, y| {
        super::cell_receiver_tests::test_terrain_cell(x, y)
    }));
    sim.set_playfield_bounds_for_test(Some(super::cell_receiver_tests::test_playfield_bounds()));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim
}

fn build(sim: &mut Simulation, rules: &RuleSet, name: &str, x: u16) -> u64 {
    let id = sim
        .spawn_object_at_height(name, OWNER, x, 10, 0, 0, rules)
        .unwrap();
    let owner = sim.interner.get(OWNER).unwrap();
    sim.recheck_house_tech_tree(owner, rules, &mut Default::default());
    id
}

fn super_of<'a>(sim: &'a Simulation, name: &str) -> &'a SuperWeaponInstance {
    &sim.super_weapons[&sim.interner.get(OWNER).unwrap()][&sim.interner.get(name).unwrap()]
}

fn super_of_mut<'a>(sim: &'a mut Simulation, name: &str) -> &'a mut SuperWeaponInstance {
    let owner = sim.interner.get(OWNER).unwrap();
    let id = sim.interner.get(name).unwrap();
    sim.super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap()
}

fn recharge(rules: &RuleSet) -> i32 {
    rules.super_weapon(REVEAL).unwrap().recharge_time_frames
}

/// A building leaves the map: its charges leave with it. Limbo is what the
/// live-building walk reads, and the tech tree recheck syncs the charges.
fn remove(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    sim.techno_limbo_with_rules(id, rules);
    let owner = sim.interner.get(OWNER).unwrap();
    sim.recheck_house_tech_tree(owner, rules, &mut Default::default());
}

/// [`rules`] with the charge animations a silo plays, and a `ChargedAnimTime`
/// that a nearly ready charge passes but a fresh one does not.
fn animated_rules() -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
        [BuildingTypes]\n0=SILO\n\
        [SILO]\nStrength=1000\nSuperWeapon=RevealSpecial\nSuperWeaponPerBuilding=yes\n\
        ChargedAnimTime=0.05\n\
        [SuperWeaponTypes]\n0=RevealSpecial\n\
        [RevealSpecial]\nType=PsychicReveal\nRechargeTime=900\nIsPowered=no\n\
        [Animations]\n0=CHARGE\n1=OPENING\n2=OPEN\n3=CLOSING\n",
    );
    let art = IniFile::from_str(
        "[SILO]\nFoundation=1x1\nSuperAnim=CHARGE\nSuperAnimTwo=OPENING\n\
         SuperAnimThree=OPEN\nSuperAnimFour=CLOSING\n\
         [CHARGE]\nEnd=1\n[OPENING]\nEnd=1\n[OPEN]\nEnd=1\n[CLOSING]\nEnd=1\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    // Processing fixed ART resolves type fields; the runtime animation
    // metadata and the assets' frame counts come from the install below, as
    // the production loader supplies them.
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    for name in ["CHARGE", "OPENING", "OPEN", "CLOSING"] {
        rules.bind_anim_frame_count_for_test(name, 1);
    }
    rules
}

fn reveal(sim: &Simulation) -> &SuperWeaponInstance {
    super_of(sim, REVEAL)
}

fn fire(sim: &mut Simulation, rules: &RuleSet, name: &str, cell: (u16, u16)) -> bool {
    let owner = sim.interner.get(OWNER).unwrap();
    let id = sim.interner.get(name).unwrap();
    sim.fire_super_weapon(
        rules,
        owner,
        id,
        cell,
        None,
        &mut FrameEffects::default(),
        false,
    )
}

/// One SuperClass::AI frame for the house's Supers at `frame`.
fn tick(sim: &mut Simulation, rules: &RuleSet, frame: u32) {
    sim.session.set_binary_frame_for_test(frame);
    let owner = sim.interner.get(OWNER).unwrap();
    tick_house_superweapon_instances(sim, rules, owner, &mut Default::default());
}

#[test]
fn a_second_silo_charges_its_own_shot_and_a_lab_without_the_key_does_not() {
    let rules = rules();
    let mut sim = world(&rules);
    let first = build(&mut sim, &rules, "SILO", 10);
    let second = build(&mut sim, &rules, "SILO", 12);
    build(&mut sim, &rules, "LAB", 14);
    let recharge = rules.super_weapon(REVEAL).unwrap().recharge_time_frames;
    assert!(reveal(&sim).is_active());
    // The first silo's shot is the house's own timer; the second charges
    // beside it, and each answers for itself.
    assert_eq!(reveal(&sim).charge_owner, Some(first));
    assert_eq!(
        reveal(&sim).building_charges,
        [BuildingCharge {
            building: second,
            left: recharge
        }]
    );
    assert_eq!(
        reveal(&sim).building_charge_remaining(first, 0),
        Some(recharge)
    );
    assert_eq!(
        reveal(&sim).building_charge_remaining(second, 0),
        Some(recharge)
    );
    for frame in 1..=recharge as u32 {
        tick(&mut sim, &rules, frame);
    }
    assert!(reveal(&sim).is_ready());
    assert_eq!(reveal(&sim).ready_shots(), 2);

    assert!(fire(&mut sim, &rules, REVEAL, (15, 15)));
    // The first silo fired, so its shot starts over while the second's takes
    // the house's timer: the second silo's shot is next, ready on the
    // following frame.
    assert_eq!(reveal(&sim).charge_owner, Some(second));
    assert_eq!(
        reveal(&sim).building_charges,
        [BuildingCharge {
            building: first,
            left: recharge
        }]
    );
    tick(&mut sim, &rules, recharge as u32 + 1);
    assert!(reveal(&sim).is_ready());
    assert_eq!(reveal(&sim).ready_shots(), 1);
}

#[test]
fn a_lost_building_takes_its_own_charge_with_it() {
    let rules = rules();
    let mut sim = world(&rules);
    let first = build(&mut sim, &rules, "SILO", 10);
    let second = build(&mut sim, &rules, "SILO", 12);
    let third = build(&mut sim, &rules, "SILO", 14);
    let frame = recharge(&rules);
    // The first silo holds the house's timer; the other two charge beside it.
    assert_eq!(reveal(&sim).charge_owner, Some(first));
    assert_eq!(
        reveal(&sim).building_charge_remaining(third, 0),
        Some(frame)
    );
    // Losing the second costs its own charge, not the emptiest one: the
    // first and third keep theirs.
    remove(&mut sim, &rules, second);
    assert_eq!(reveal(&sim).charge_owner, Some(first));
    assert_eq!(reveal(&sim).building_charge_remaining(second, 0), None);
    assert_eq!(
        reveal(&sim).building_charge_remaining(third, 0),
        Some(frame)
    );
    // Losing the silo that held the house's timer hands it to the first
    // survivor, keeping that survivor's frames left.
    remove(&mut sim, &rules, first);
    assert_eq!(reveal(&sim).charge_owner, Some(third));
    assert_eq!(
        reveal(&sim).building_charge_remaining(third, 0),
        Some(frame)
    );
}

/// One silo's SuperAnim slots after its first opening and one
/// UpdateAnimation, for a building whose own charge has `left` frames left.
fn anim_slots(sim: &Simulation, id: u64) -> (bool, bool) {
    let entity = sim.substrate.entities.get(id).unwrap();
    (
        entity.building_art.anim_slots()[14].is_some(),
        entity.building_art.anim_slots()[16].is_some(),
    )
}

#[test]
fn each_silo_closes_and_opens_on_its_own_charge() {
    let rules = animated_rules();
    let mut sim = world(&rules);
    let first = build(&mut sim, &rules, "SILO", 10);
    let second = build(&mut sim, &rules, "SILO", 12);
    // Both silos are almost ready: each shows its open slot.
    for id in [first, second] {
        super_of_mut(&mut sim, REVEAL).set_building_charge_left_for_test(id, 10, 0);
        assert_eq!(
            super_of(&sim, REVEAL).building_charge_remaining(id, 0),
            Some(10),
            "own charge for {id}"
        );
        sim.open_super_weapon_anims(id, false, false, &rules);
        assert_eq!(anim_slots(&sim, id), (false, true), "opened {id}");
    }
    // The first silo fires and starts over; the second keeps its charge.
    super_of_mut(&mut sim, REVEAL).set_building_charge_left_for_test(first, 900, 0);
    sim.update_super_weapon_anims(first, &rules);
    sim.update_super_weapon_anims(second, &rules);
    assert_eq!(
        anim_slots(&sim, first),
        (false, false),
        "the fired silo closes"
    );
    assert_eq!(
        anim_slots(&sim, second),
        (false, true),
        "the other silo stays open"
    );
}

/// Every line a building draws: its own shot while it holds one, the House's
/// shared charge for a provider without `SuperWeaponPerBuilding=` (a battle
/// lab), and nothing for a building that provides no Super at all (a radar).
#[test]
fn each_building_reports_the_lines_it_draws() {
    let rules = rules();
    let mut sim = world(&rules);
    let first = build(&mut sim, &rules, "SILO", 10);
    let second = build(&mut sim, &rules, "SILO", 12);
    let lab = build(&mut sim, &rules, "LAB", 14);
    super_of_mut(&mut sim, REVEAL).set_building_charge_left_for_test(first, 30, 0);
    super_of_mut(&mut sim, REVEAL).set_building_charge_left_for_test(second, 7, 0);
    let reveal = sim.interner.get(REVEAL).unwrap();
    let lines =
        |sim: &Simulation, id| crate::sim::superweapon::building_super_charges(sim, &rules, id);
    assert_eq!(
        lines(&sim, first),
        [SuperChargeLine {
            super_id: reveal,
            left: 30,
            own: true
        }]
    );
    assert_eq!(
        lines(&sim, second),
        [SuperChargeLine {
            super_id: reveal,
            left: 7,
            own: true
        }]
    );
    // A Battle Lab keeps the House's one charge, the native timer's, as its
    // animations do, and it charges no shot of its own.
    assert_eq!(
        lines(&sim, lab),
        [SuperChargeLine {
            super_id: reveal,
            left: 30,
            own: false
        }]
    );
    // A radar brings no `SuperWeapon=`, so it has no line to draw.
    let radar = build(&mut sim, &rules, "RADAR", 16);
    assert!(lines(&sim, radar).is_empty());
    // A Super that is momentarily not granted keeps its timer: the label must
    // not blink out while the House's grants are recomputed.
    super_of_mut(&mut sim, REVEAL).set_is_active_for_test(false);
    assert_eq!(
        lines(&sim, second),
        [SuperChargeLine {
            super_id: reveal,
            left: 7,
            own: true
        }]
    );
}

#[test]
fn a_reset_such_as_a_spy_costs_one_charged_shot_not_all() {
    let rules = rules();
    let mut sim = world(&rules);
    build(&mut sim, &rules, "SILO", 10);
    build(&mut sim, &rules, "SILO", 12);
    let recharge = rules.super_weapon(REVEAL).unwrap().recharge_time_frames;
    for frame in 1..=recharge as u32 {
        tick(&mut sim, &rules, frame);
    }
    let owner = sim.interner.get(OWNER).unwrap();
    reset_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &mut sim.sound_events,
        &mut sim.super_weapons,
        &rules,
        owner,
        REVEAL,
    );
    tick(&mut sim, &rules, recharge as u32 + 1);
    assert!(reveal(&sim).is_ready());
    assert_eq!(reveal(&sim).ready_shots(), 1);
}

#[test]
fn two_quick_nukes_leave_from_two_silos_each_with_its_own_target() {
    let rules = rules();
    let mut sim = world(&rules);
    build(&mut sim, &rules, "NUKES", 10);
    build(&mut sim, &rules, "NUKES", 12);
    let recharge = rules
        .super_weapon("NukeSpecial")
        .unwrap()
        .recharge_time_frames;
    for frame in 1..=recharge as u32 {
        tick(&mut sim, &rules, frame);
    }
    assert!(fire(&mut sim, &rules, "NukeSpecial", (15, 15)));
    tick(&mut sim, &rules, recharge as u32 + 1);
    assert!(super_of(&sim, "NukeSpecial").is_ready());
    assert!(fire(&mut sim, &rules, "NukeSpecial", (20, 20)));
    let owner = sim.interner.get(OWNER).unwrap();
    let mut targets: Vec<_> = sim
        .houses
        .get(&owner)
        .unwrap()
        .base_projection()
        .buildings()
        .iter()
        .filter_map(|&id| {
            sim.substrate
                .entities
                .get(id)?
                .mission_leaf
                .as_building()?
                .own_nuke_target()
        })
        .collect();
    targets.sort();
    assert_eq!(targets, [(15, 15), (20, 20)]);
}

/// The next missile leaves the silo whose own charge the ready shot is, not
/// the first silo in building order. A launch resets the fired building and
/// promotes the fullest other charge, so from the second launch on the two
/// differ: the first silo is back on Guard long before the next shot, while
/// its replacement holds the charge.
#[test]
fn the_next_missile_leaves_the_silo_that_holds_the_ready_charge() {
    let rules = rules();
    let mut sim = world(&rules);
    let first = build(&mut sim, &rules, "NUKES", 10);
    let second = build(&mut sim, &rules, "NUKES", 12);
    build(&mut sim, &rules, "NUKES", 14);
    let owner = sim.interner.get(OWNER).unwrap();
    let sw = rules.super_weapon("NukeSpecial").unwrap();
    let sw_type = sim.interner.get("NukeSpecial").unwrap();
    let silo_type = sim.interner.get("NUKES").unwrap();
    // The first silo holds the House's shot until a launch spends it.
    assert_eq!(super_of(&sim, "NukeSpecial").charge_owner(), Some(first));
    assert_eq!(
        super::nuke::find_charged_silo(&sim, owner, sw_type, silo_type),
        Some(first)
    );
    // ClickFire's recharge half: the fired silo starts over empty, the fullest
    // other charge takes the native timer keeping its frames left.
    super_of_mut(&mut sim, "NukeSpecial").set_is_ready_for_test(false);
    assert!(super_of_mut(&mut sim, "NukeSpecial").stop_preclick(sw, 100));
    assert_eq!(super_of(&sim, "NukeSpecial").charge_owner(), Some(second));
    // Building order still starts at the idle first silo, which is exactly the
    // building a launch must not use.
    assert_eq!(
        super::nuke::find_idle_silo(&sim, owner, silo_type),
        Some(first)
    );
    assert_eq!(
        super::nuke::find_charged_silo(&sim, owner, sw_type, silo_type),
        Some(second)
    );
}

/// The countdown a player watches must not blink: over a full recharge cycle
/// the silo reports its line on every tick, whatever the grants, the power or
/// the ready state are doing. A line that came and went here would blank the
/// label no matter how the frame is drawn.
#[test]
fn a_silo_reports_its_line_on_every_tick() {
    let rules = rules();
    let mut sim = world(&rules);
    let silo = build(&mut sim, &rules, "NUKES", 10);
    for frame in 1..=900u32 {
        tick(&mut sim, &rules, frame);
        let lines = crate::sim::superweapon::building_super_charges(&sim, &rules, silo);
        assert_eq!(lines.len(), 1, "frame {frame}: {lines:?}");
        assert!(lines[0].left >= 0, "frame {frame}: {}", lines[0].left);
        assert!(lines[0].own, "frame {frame}: a silo charges its own shot");
    }
}

/// An enemy's providers draw only the Supers its timer list draws anyway: a
/// grant with no timer place (`SW.ShowTimer=no`) keeps no right-hand line,
/// and one above the building would leak a capability the player cannot
/// otherwise read. The player's own and its allies' providers draw
/// everything, and a view with no local House (an observer) reads all.
#[test]
fn an_enemy_draws_only_the_lines_its_timer_list_draws() {
    let rules = rules();
    let mut sim = world(&rules);
    // New houses start at war, so this one is the local player's enemy.
    let enemy = sim.interner.intern("SOVIETS");
    sim.houses
        .register(HouseState::new(enemy, 1, None, false, 2000, 10));
    construct_house_supers(&mut sim, &rules, enemy, false);
    // The nuke made the timer list at its grant; the fixture's reveal type
    // leaves `ShowTimer=` unset, so its grant keeps no timer place.
    let nuke = sim.interner.get("NukeSpecial").unwrap();
    sim.super_weapons
        .map_mut_for_test()
        .get_mut(&enemy)
        .unwrap()
        .get_mut(&nuke)
        .unwrap()
        .timer_place = Some(1);
    let player = sim.interner.get(OWNER).unwrap();
    sim.session.set_current_house_for_test(Some(player));
    let lines = |sim: &Simulation, id: u64| {
        crate::sim::superweapon::building_super_charges(sim, &rules, id)
    };

    let enemy_silo = sim
        .spawn_object_at_height("SILO", "SOVIETS", 14, 10, 0, 0, &rules)
        .unwrap();
    sim.recheck_house_tech_tree(enemy, &rules, &mut Default::default());
    assert!(lines(&sim, enemy_silo).is_empty());

    let enemy_nukes = sim
        .spawn_object_at_height("NUKES", "SOVIETS", 16, 10, 0, 0, &rules)
        .unwrap();
    sim.recheck_house_tech_tree(enemy, &rules, &mut Default::default());
    let nuke_lines = lines(&sim, enemy_nukes);
    assert_eq!(nuke_lines.len(), 1, "{nuke_lines:?}");
    assert_eq!(nuke_lines[0].super_id, nuke);

    // The player's own providers draw everything: even a reveal whose grant
    // keeps no timer place has its line above the building.
    let own_silo = build(&mut sim, &rules, "SILO", 10);
    assert_eq!(lines(&sim, own_silo).len(), 1);

    // An ally the player names one draws everything, too.
    let friend = sim.interner.intern("ALLIES");
    sim.houses
        .register(HouseState::new(friend, 2, None, false, 2000, 10));
    construct_house_supers(&mut sim, &rules, friend, false);
    sim.house_alliances_mut_for_test()
        .entry(crate::map::houses::normalize_house_name("Americans"))
        .or_default()
        .insert(crate::map::houses::normalize_house_name("ALLIES"));
    let friend_silo = sim
        .spawn_object_at_height("SILO", "ALLIES", 18, 10, 0, 0, &rules)
        .unwrap();
    sim.recheck_house_tech_tree(friend, &rules, &mut Default::default());
    assert_eq!(lines(&sim, friend_silo).len(), 1);
}
