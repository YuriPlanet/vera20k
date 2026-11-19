//! Phobos `AutoDeath`'s timed arm (`Ext/Techno/Body.Update.cpp:173-184`) driven
//! through the production AI visit, plus the `FootExt::UpdateTypeData` reset
//! (`Ext/Foot/Body.cpp:219-221`).
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

const RULES: &str = "\
[InfantryTypes]\n0=E1\n1=E2\n\
[VehicleTypes]\n0=TANK\n1=TANKONLYDELAY\n2=TANKAMMO\n3=TANKPOWER\n4=TANKMONEY\n5=TANKEXIST\n6=TANKDONT\n7=TANKANY\n8=TANKALL\n9=TANKJUNK\n10=TANKDONTALL\n11=TANKENEMY\n12=TANKOWNER\n13=TANKNOOC\n14=TANKOTOH\n15=TANKLIMBO\n16=TANKVANISH\n\
[BuildingTypes]\n0=BARR\n1=BARRNOB\n2=BUNK\n\
[E1]\nStrength=125\nSight=0\n\
[E2]\nStrength=125\nSight=0\n\
[TANK]\nStrength=100\nSight=0\nSpeed=5\nAutoDeath.Behavior=kill\nAutoDeath.AfterDelay=3\n\
[TANKONLYDELAY]\nStrength=100\nSight=0\nSpeed=5\nAutoDeath.AfterDelay=1\n\
[TANKAMMO]\nStrength=100\nSight=0\nSpeed=5\nAmmo=1\n\
AutoDeath.Behavior=kill\nAutoDeath.OnAmmoDepletion=yes\n\
[TANKPOWER]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.PlayerPowerState=low\n\
[TANKMONEY]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.PlayerMoney.Max=50\n\
[TANKEXIST]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.TechnosExist=E1\n\
[TANKDONT]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.TechnosDontExist=E1\n\
[TANKANY]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.TechnosExist=E1,E2\n\
[TANKALL]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.TechnosExist=E1,E2\nAutoDeath.TechnosExist.Any=no\n\
[TANKJUNK]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.TechnosExist=NOTATYPE\nAutoDeath.TechnosExist.Any=no\n\
[TANKDONTALL]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.TechnosDontExist=E1,E2\nAutoDeath.TechnosDontExist.Any=yes\n\
[TANKENEMY]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.TechnosExist=E1\nAutoDeath.TechnosExist.Houses=enemies\n\
[TANKOWNER]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.OnOwnerChange=yes\n\
[TANKNOOC]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.OnOwnerChange=no\n\
[TANKOTOH]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.OnOwnerChange=yes\nAutoDeath.OnOwnerChange.HumanToComputer=no\n\
[TANKLIMBO]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.OnOwnerChange=yes\nAutoDeath.AllowLimboed=no\n\
[TANKVANISH]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=vanish\nAutoDeath.AfterDelay=1\nAutoDeath.VanishAnimation=VA\n\
[BARR]\nStrength=500\nSight=0\nAutoDeath.Behavior=sell\nAutoDeath.AfterDelay=1\n\
[BARRNOB]\nStrength=500\nSight=0\nAutoDeath.Behavior=sell\nAutoDeath.AfterDelay=1\n\
[BUNK]\nStrength=500\nSight=0\nAutoDeath.Behavior=vanish\nAutoDeath.AfterDelay=1\n\
[Animations]\n0=VA\n\
[VA]\n\
[Warheads]\n0=C4WH\n\
[C4WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
[CombatDamage]\nC4Warhead=C4WH\n\
[Projectiles]\n0=SHOT\n[SHOT]\nInviso=yes\n";

fn arena() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(RULES)).unwrap();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    (sim, rules)
}

fn alive(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.lifecycle.object_alive())
}

fn tick(sim: &mut Simulation, rules: &RuleSet, count: usize) {
    for _ in 0..count {
        sim.advance_tick(&[], Some(rules), None, None, 22);
    }
}

/// `:175-183`: the first visit only starts `AutoDeathTimer`, and the object
/// dies once the countdown completes. The C4 warhead carries the object's own
/// Health (`KillSelf` `:820`), so the object is gone by the time it expires.
#[test]
fn a_timed_auto_death_starts_its_clock_then_kills_the_object() {
    let (mut sim, rules) = arena();
    let tank = sim
        .spawn_object("TANK", "Americans", 10, 10, 0, &rules)
        .expect("TANK spawns");
    assert!(alive(&sim, tank));
    tick(&mut sim, &rules, 1);
    assert!(
        alive(&sim, tank),
        "the first visit only starts the countdown"
    );
    // Two more frames are one short of the three-frame countdown.
    tick(&mut sim, &rules, 2);
    assert!(alive(&sim, tank), "the frame before expiry still lives");
    tick(&mut sim, &rules, 1);
    assert!(
        !alive(&sim, tank),
        "AfterDelay=3 completes on the third frame after the clock starts"
    );
}

/// The scan counts on-map instances (`HouseClass::CountOwnedAndPresent`,
/// `0x0049FAE0`), so the fixture registers each spawn the way the scenario
/// loader does: a House, then `house_added_to_game`.
fn register_on_map(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    if sim.houses().get(&owner).is_none() {
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 0,
            ));
    }
    sim.house_added_to_game(id, rules);
}

/// `Ext/Techno/Body.Update.cpp:247-255`: `AutoDeath.TechnosExist=` kills the
/// object while any listed type (its default `Any=yes`) is present for the
/// selected houses.
#[cfg(feature = "phobos")]
#[test]
fn technos_exist_kills_only_when_the_listed_type_is_present() {
    for (with_crew, expected_alive) in [(false, true), (true, false)] {
        let (mut sim, rules) = arena();
        let tank = sim
            .spawn_object("TANKEXIST", "Americans", 10, 10, 0, &rules)
            .expect("TANKEXIST spawns");
        register_on_map(&mut sim, &rules, tank);
        if with_crew {
            let crew = sim
                .spawn_object("E1", "Americans", 12, 10, 0, &rules)
                .expect("E1 spawns");
            register_on_map(&mut sim, &rules, crew);
        }
        sim.session.set_binary_frame_for_test(5);
        sim.object_ai_stage(Some(&rules));
        assert_eq!(
            alive(&sim, tank),
            expected_alive,
            "TechnosExist=E1 with the type present = {with_crew}"
        );
    }
}

/// `:236-245`: `AutoDeath.TechnosDontExist=` kills it while none of the listed
/// types exist; native inverts `Any`, whose default `no` becomes "any of them".
#[cfg(feature = "phobos")]
#[test]
fn technos_dont_exist_kills_only_when_the_listed_type_is_absent() {
    for (with_crew, expected_alive) in [(false, false), (true, true)] {
        let (mut sim, rules) = arena();
        let tank = sim
            .spawn_object("TANKDONT", "Americans", 10, 10, 0, &rules)
            .expect("TANKDONT spawns");
        register_on_map(&mut sim, &rules, tank);
        if with_crew {
            let crew = sim
                .spawn_object("E1", "Americans", 12, 10, 0, &rules)
                .expect("E1 spawns");
            register_on_map(&mut sim, &rules, crew);
        }
        sim.session.set_binary_frame_for_test(5);
        sim.object_ai_stage(Some(&rules));
        assert_eq!(
            alive(&sim, tank),
            expected_alive,
            "TechnosDontExist=E1 with the type present = {with_crew}"
        );
    }
}

/// Spawn `kind`, optionally spawn each listed type for `crew_owner`, register
/// them the way the loader does, and run one AI visit.
fn alive_after_presence(kind: &str, present: &[&str], crew_owner: &str) -> bool {
    let (mut sim, rules) = arena();
    let tank = sim
        .spawn_object(kind, "Americans", 10, 10, 0, &rules)
        .expect("the auto-death type spawns");
    register_on_map(&mut sim, &rules, tank);
    for (index, name) in present.iter().enumerate() {
        let id = sim
            .spawn_object(name, crew_owner, 12 + index as u16, 10, 0, &rules)
            .expect("the presence type spawns");
        register_on_map(&mut sim, &rules, id);
    }
    sim.session.set_binary_frame_for_test(5);
    sim.object_ai_stage(Some(&rules));
    alive(&sim, tank)
}

/// `:205-208`: `Any=yes` is `any_of` and `Any=no` is `all_of`. One type
/// satisfies `any_of` alone; `all_of` needs both.
#[test]
fn technos_exist_any_and_all_are_distinct() {
    assert!(
        !alive_after_presence("TANKANY", &["E1"], "Americans"),
        "Any=yes kills on the first listed type"
    );
    assert!(
        alive_after_presence("TANKALL", &["E1"], "Americans"),
        "Any=no needs every listed type"
    );
    assert!(
        !alive_after_presence("TANKALL", &["E1", "E2"], "Americans"),
        "Any=no kills once both are present"
    );
}

/// `:239`: `TechnosDontExist` inverts `Any`, so `Any=yes` asks "do all exist?"
/// and kills when they do not.
#[test]
fn technos_dont_exist_inverts_any() {
    assert!(
        !alive_after_presence("TANKDONTALL", &["E1"], "Americans"),
        "Any=yes kills while not every listed type exists"
    );
    assert!(
        alive_after_presence("TANKDONTALL", &["E1", "E2"], "Americans"),
        "Any=yes spares once all exist"
    );
}

/// `:188-208` with `Utilities/TemplateDef.h:1727-1742`: an unknown token never
/// reaches the list, so the arm keeps its emptiness guard; and `Houses=` picks
/// the scanned house set rather than the owner alone.
#[test]
fn an_unknown_name_and_an_enemy_house_select_the_scan() {
    assert!(
        alive_after_presence("TANKJUNK", &[], "Americans"),
        "an unknown type name is dropped, leaving the arm empty"
    );
    assert!(
        alive_after_presence("TANKENEMY", &["E1"], "Americans"),
        "Houses=enemies ignores the owner's own type"
    );
    assert!(
        !alive_after_presence("TANKENEMY", &["E1"], "Russians"),
        "Houses=enemies finds an enemy house's type"
    );
}

/// Two houses, one human and one computer, with the object moving from the
/// human one to the computer one (or the reverse when `from_human` is false).
fn owner_change_spares(kind: &str, from_human: bool) -> bool {
    owner_change_spares_between(kind, from_human, !from_human)
}

/// As [`owner_change_spares`], with each house's control chosen separately.
fn owner_change_spares_between(kind: &str, old_human: bool, new_human: bool) -> bool {
    let (mut sim, rules) = arena();
    let americans = sim.intern("Americans");
    let russians = sim.intern("Russians");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            americans, 0, None, old_human, 0, 0,
        ));
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            russians, 0, None, new_human, 0, 0,
        ));
    let tank = sim
        .spawn_object(kind, "Americans", 10, 10, 0, &rules)
        .expect("the auto-death type spawns");
    sim.house_added_to_game(tank, &rules);
    sim.change_owner_with_rules(
        tank,
        russians,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    sim.session.set_binary_frame_for_test(5);
    sim.object_ai_stage(Some(&rules));
    alive(&sim, tank)
}

/// The owner-change hook's flag for a unit carrying the retained `Transporter`
/// field, read before any AI visit (a boarded object may not get one).
fn owner_change_flag_transported(kind: &str) -> bool {
    let (mut sim, rules) = arena();
    let americans = sim.intern("Americans");
    let russians = sim.intern("Russians");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            americans, 0, None, true, 0, 0,
        ));
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            russians, 0, None, false, 0, 0,
        ));
    let tank = sim
        .spawn_object(kind, "Americans", 10, 10, 0, &rules)
        .expect("the auto-death type spawns");
    sim.house_added_to_game(tank, &rules);
    sim.substrate.entities.get_mut(tank).unwrap().passenger_role =
        crate::sim::passenger::PassengerRole::boarded_for_test(99, false);
    sim.change_owner_with_rules(
        tank,
        russians,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    sim.entities().get(tank).unwrap().should_be_dead()
}

/// `TechnoClass_Captured_UpdateTracking @ 0x7015C9`
/// (`Ext/House/Hooks.cpp:231-259`): the master `OnOwnerChange` switch arms both
/// directions, and `ShouldBeDead` then kills on the next
/// `CheckDeathConditions` visit (`Ext/Techno/Body.Update.cpp:159-163`).
#[cfg(feature = "phobos")]
#[test]
fn an_owner_change_sets_should_be_dead_by_direction() {
    assert!(
        !owner_change_spares("TANKOWNER", true),
        "the master key on kills a human-to-computer change"
    );
    assert!(
        !owner_change_spares("TANKOWNER", false),
        "the master key on kills a computer-to-human change"
    );
    assert!(
        owner_change_spares("TANKNOOC", true),
        "the master key off spares the human-to-computer change"
    );
}

/// The direction keys default to the master and override it one way at a time.
#[cfg(feature = "phobos")]
#[test]
fn the_direction_keys_override_the_master() {
    assert!(
        owner_change_spares("TANKOTOH", true),
        "HumanToComputer=no spares that direction while the master is on"
    );
    assert!(
        !owner_change_spares("TANKOTOH", false),
        "ComputerToHuman still takes the master's yes"
    );
}

/// `Ext/House/Hooks.cpp:240-243`: both direction keys on kills on any change,
/// even between two houses of the same control kind.
#[cfg(feature = "phobos")]
#[test]
fn both_directions_kill_across_two_human_houses() {
    assert!(
        !owner_change_spares_between("TANKOWNER", true, true),
        "both keys on kills even when both houses are human"
    );
}

/// `:253-258` with `Ext/Rules/Body.h:1027`: `[CombatDamage] AutoDeath.AllowLimboed`
/// constructs `true`, so a transported object dies by default and only an
/// explicit `AllowLimboed=no` clears the flag.
#[cfg(feature = "phobos")]
#[test]
fn the_allow_limboed_gate_spares_only_when_it_is_off() {
    assert!(
        owner_change_flag_transported("TANKOWNER"),
        "the global default yes keeps the flag on a transported object"
    );
    assert!(
        !owner_change_flag_transported("TANKLIMBO"),
        "AllowLimboed=no clears it"
    );
}

/// `:165-170`: a type whose `Ammo=` capacity is above zero dies at zero.
#[test]
fn a_type_with_ammo_dies_when_the_ammo_is_gone() {
    let (mut sim, rules) = arena();
    let tank = sim
        .spawn_object("TANKAMMO", "Americans", 10, 10, 0, &rules)
        .expect("TANKAMMO spawns");
    sim.session.set_binary_frame_for_test(5);
    sim.object_ai_stage(Some(&rules));
    assert!(alive(&sim, tank), "Ammo=1 is not depleted yet");
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .ammo_mut()
        .expect("Ammo=1 installs a counter")
        .set_current_for_test(0);
    sim.session.set_binary_frame_for_test(6);
    sim.object_ai_stage(Some(&rules));
    assert!(!alive(&sim, tank), "zero ammo triggers the arm");
}

/// `:210-221`: the retained low-power flag. The first-frame guard binds to the
/// `Low` arm only, so frame 5 is enough for it to fire.
#[test]
fn a_low_power_house_kills_its_auto_death_type() {
    let (mut sim, rules) = arena();
    let tank = sim
        .spawn_object("TANKPOWER", "Americans", 10, 10, 0, &rules)
        .expect("TANKPOWER spawns");
    let owner = sim.substrate.entities.get(tank).unwrap().owner();
    sim.power_states_mut_for_test()
        .entry(owner)
        .or_default()
        .set_is_low_power_for_test(true);
    sim.session.set_binary_frame_for_test(5);
    sim.object_ai_stage(Some(&rules));
    assert!(!alive(&sim, tank), "a low-power house kills the type");
}

/// `:223-234`: a closed money range kills, and the same type survives once the
/// balance is above `PlayerMoney.Max`.
#[test]
fn the_owner_money_range_kills_or_spares() {
    for (credits, expected_alive) in [(0, false), (100, true)] {
        let (mut sim, rules) = arena();
        let tank = sim
            .spawn_object("TANKMONEY", "Americans", 10, 10, 0, &rules)
            .expect("TANKMONEY spawns");
        let owner = sim.substrate.entities.get(tank).unwrap().owner();
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 0,
            ));
        sim.houses_mut_for_test()
            .get_mut(&owner)
            .expect("house")
            .economy_mut_for_test()
            .set_credits_for_test(credits);
        sim.session.set_binary_frame_for_test(5);
        sim.object_ai_stage(Some(&rules));
        assert_eq!(
            alive(&sim, tank),
            expected_alive,
            "PlayerMoney.Max=50 with a balance of {credits}"
        );
    }
}

/// `Ext/Techno/Body.Visuals.cpp:696-717`: the digital display shows the
/// clock's remaining time over its full duration, and a type without the
/// mechanism draws nothing.
#[test]
fn the_digital_display_reports_the_remaining_countdown() {
    let (mut sim, rules) = arena();
    let tank = sim
        .spawn_object("TANK", "Americans", 10, 10, 0, &rules)
        .expect("TANK spawns");
    let delay = sim
        .spawn_object("TANKONLYDELAY", "Americans", 12, 10, 0, &rules)
        .expect("TANKONLYDELAY spawns");
    // Start the three-frame clock at frame 10, then read it two frames later.
    sim.session.set_binary_frame_for_test(10);
    sim.object_ai_stage(Some(&rules));
    sim.session.set_binary_frame_for_test(12);
    let read = |sim: &Simulation, id: u64| {
        let entity = sim.substrate.entities.get(id).unwrap();
        let object = sim
            .interner
            .object_type(entity.type_ref(), &rules)
            .unwrap()
            .clone();
        crate::sim::digital_display::values_for_display(
            sim,
            entity,
            &object,
            &rules,
            crate::rules::digital_display::DigitalInfoType::AutoDeath,
            0,
            None,
        )
    };
    assert_eq!(
        read(&sim, tank),
        Some((1, 3)),
        "GetTimeLeft over TimeLeft two frames into a three-frame countdown"
    );
    assert_eq!(
        read(&sim, delay),
        None,
        "AfterDelay with no Behavior never draws an AutoDeath countdown"
    );
}

/// `:142`: an unset `AutoDeath.Behavior` returns before the countdown, so
/// `AutoDeath.AfterDelay=` alone never arms anything.
#[test]
fn after_delay_without_a_behavior_never_arms_the_clock() {
    let (mut sim, rules) = arena();
    let tank = sim
        .spawn_object("TANKONLYDELAY", "Americans", 10, 10, 0, &rules)
        .expect("TANKONLYDELAY spawns");
    tick(&mut sim, &rules, 8);
    assert!(
        alive(&sim, tank),
        "AfterDelay=1 with no Behavior must never start a countdown"
    );
}

/// `Ext/Foot/Body.cpp:219-221` through `TechnoClass::ChangeType`. VERA reaches
/// that owner only from the Ares `Convert.Water=` surface conversion, so the
/// test drives a real shoreline crossing: a timed unit converts to an untimed
/// type and must not carry its countdown across. The stale clock would complete
/// two frames after the crossing and kill the converted unit.
#[test]
fn an_ares_surface_conversion_stops_the_countdown() {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=LAND\n1=WATER\n\
         [LAND]\nStrength=100\nSpeed=5\nSpeedType=Hover\nMovementZone=Amphibious\n\
         Convert.Water=WATER\nAutoDeath.Behavior=kill\nAutoDeath.AfterDelay=5\n\
         Locomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n\
         [WATER]\nStrength=200\nSpeed=5\nSpeedType=Hover\nMovementZone=Amphibious\n\
         Convert.Land=LAND\nConvert.Water=WATER\n\
         Locomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n\
         [Warheads]\n0=C4WH\n\
         [C4WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nC4Warhead=C4WH\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 29);
    let id = sim
        .spawn_object("LAND", "Americans", 10, 10, 0, &rules)
        .expect("LAND spawns");
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .power_off();
    // Frames 100..104 on land start the five-frame countdown (start 100,
    // expires 105).
    for frame in 100..104 {
        sim.session.set_binary_frame_for_test(frame);
        sim.object_ai_stage(Some(&rules));
    }
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(id).unwrap().type_ref()),
        "LAND",
        "the unit is still on land"
    );
    // Cross to water at 104 and stay well past the countdown: the untimed
    // WATER type never runs a check, so only the conversion reset can clear
    // the clock.
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(10, 10)
        .unwrap()
        .land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    for frame in 104..110 {
        sim.session.set_binary_frame_for_test(frame);
        sim.object_ai_stage(Some(&rules));
    }
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(id).unwrap().type_ref()),
        "WATER",
        "the crossing converted the unit"
    );
    // Back to land at 110. Without the reset the timer is long expired and the
    // first timed visit kills the unit immediately; a reset one restarts here
    // and leaves four frames.
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(10, 10)
        .unwrap()
        .land_type = crate::rules::terrain_rules::LandType::Clear.as_index();
    for frame in 110..114 {
        sim.session.set_binary_frame_for_test(frame);
        sim.object_ai_stage(Some(&rules));
    }
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(id).unwrap().type_ref()),
        "LAND",
        "the unit converted back"
    );
    assert!(
        alive(&sim, id),
        "a returning timed type must start a fresh countdown, not inherit a completed one"
    );
}

/// `TechnoExt::KillSelf`'s `Vanish` arm
/// (`Ext/Techno/Body.Update.cpp:771-790`): a random
/// `AutoDeath.VanishAnimation` plays at the object's GetCoords owned by its
/// house, and the object leaves the world without a death.
#[cfg(feature = "phobos")]
#[test]
fn vanish_plays_its_animation_and_erases_the_object() {
    let (mut sim, mut rules) = arena();
    // The fixture registers every rule type and an empty art table, as the
    // scenario loader does, so the spawn owner resolves its `[Animations]`
    // entry and constructs with the default draw state.
    rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let tank = sim
        .spawn_object("TANKVANISH", "Americans", 10, 10, 0, &rules)
        .expect("TANKVANISH spawns");
    let owner = sim.entities().get(tank).unwrap().owner();
    tick(&mut sim, &rules, 2);
    assert!(
        sim.entities().get(tank).is_none(),
        "Vanish UnInits the object"
    );
    let anims: Vec<_> = sim.anims().collect();
    assert_eq!(anims.len(), 1, "one vanish animation");
    let (_, anim) = anims[0];
    assert_eq!(sim.interner.resolve(anim.type_id()), "VA");
    assert_eq!(anim.owner_house(), Some(owner));
    let world = anim.world_coord();
    assert_eq!(
        [world.x, world.y, world.z],
        [10 * 256 + 128, 10 * 256 + 128, 0],
        "the anim anchors on the object's cell-centre GetCoords"
    );
}

/// `AnimExt::CreateRandomAnim(pVanishAnimation, pThis->GetCoords(), pThis,
/// nullptr, true)` (`Ext/Techno/Body.Update.cpp:773`) passes five arguments:
/// `invoker` is true and `ownedObject` keeps its `Ext/Anim/Body.h:122` default
/// `false`. The vanish animation therefore names the dying object as its Phobos
/// `Invoker` (with that object's house as `InvokerHouse`) and its house as
/// owner house, while `AnimClass::SetOwnerObject @ 0x00424B50` is never called
/// (`Ext/Anim/Body.cpp:420-421`). Giving it an owner object would be invented
/// behavior: the animation would then follow, and die with, an object native
/// leaves it detached from.
#[cfg(feature = "phobos")]
#[test]
fn the_vanish_animation_gets_the_dying_object_as_invoker_and_no_owner_object() {
    let (mut sim, mut rules) = arena();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let tank = sim
        .spawn_object("TANKVANISH", "Americans", 10, 10, 0, &rules)
        .expect("TANKVANISH spawns");
    let owner = sim.entities().get(tank).unwrap().owner();
    tick(&mut sim, &rules, 2);
    let anims: Vec<_> = sim.anims().collect();
    assert_eq!(anims.len(), 1, "one vanish animation");
    let anim = anims[0].1;
    assert_eq!(sim.interner.resolve(anim.type_id()), "VA");
    assert_eq!(
        anim.invoker(),
        Some(tank),
        "the dying object is the animation's invoker"
    );
    assert_eq!(
        anim.invoker_house(),
        Some(owner),
        "and its house rides along as InvokerHouse"
    );
    assert_eq!(anim.owner_house(), Some(owner), "its house owns the anim");
    assert_eq!(
        anim.owner_entity(),
        None,
        "native passes no ownedObject, so the anim has no owner object"
    );
}

/// `TechnoExt::KillSelf`'s `Sell` arm (`:792-807`): a building whose Buildup
/// SHP is bound (`+0x6E9`) takes the Selling mission instead of dying; a
/// building without one falls through to the `Kill` arm.
#[cfg(feature = "phobos")]
#[test]
fn sell_starts_the_selling_mission_or_falls_back_to_kill() {
    let (mut sim, mut rules) = arena();
    rules.set_buildup_control_for_test("BARR", [8, 1, 0]);
    let building = sim
        .spawn_object("BARR", "Americans", 10, 10, 0, &rules)
        .expect("BARR spawns");
    tick(&mut sim, &rules, 2);
    let entity = sim.entities().get(building).expect("the building lives");
    assert!(entity.lifecycle.object_alive());
    assert_eq!(
        entity.mission.current().known(),
        Some(crate::sim::mission::MissionType::Selling),
        "the trigger queues and commences Selling"
    );

    let (mut sim, rules) = arena();
    let building = sim
        .spawn_object("BARRNOB", "Americans", 10, 10, 0, &rules)
        .expect("BARRNOB spawns");
    tick(&mut sim, &rules, 2);
    assert!(
        !alive(&sim, building),
        "no Buildup SHP falls through to the Kill arm"
    );
}

/// `KillSelf`'s Vanish arm unloads a building's bunker through
/// `BuildingClass::UnloadBunker @ 0x004593A0`, the sell/death release: the
/// installed unit's reciprocal link is cleared, and no reposition, NavCom or
/// Move is produced. An empty `AutoDeath.VanishAnimation` draws nothing.
#[cfg(feature = "phobos")]
#[test]
fn vanish_unloads_a_bunker_without_repositioning_the_unit() {
    let (mut sim, mut rules) = arena();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::empty());
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let bunker = sim
        .spawn_object("BUNK", "Americans", 14, 14, 0, &rules)
        .expect("BUNK spawns");
    let unit = sim
        .spawn_object("TANK", "Americans", 14, 15, 0, &rules)
        .expect("TANK spawns");
    crate::sim::docking::bunker_link::install_bunker_link(&mut sim, bunker, unit, &rules);
    assert_eq!(
        sim.entities().get(bunker).unwrap().bunker.occupant(),
        Some(unit)
    );
    let (rx, ry) = {
        let position = sim.entities().get(unit).unwrap().position;
        (position.rx(), position.ry())
    };
    tick(&mut sim, &rules, 2);
    assert!(sim.entities().get(bunker).is_none(), "the bunker vanished");
    assert_eq!(sim.anims().count(), 0, "an empty list draws nothing");
    let unit_entity = sim
        .entities()
        .get(unit)
        .expect("the installed unit survives");
    assert!(
        unit_entity.bunker.link().installed_in().is_none(),
        "the reciprocal bunker link is cleared"
    );
    assert_eq!(
        (unit_entity.position.rx(), unit_entity.position.ry()),
        (rx, ry),
        "the release does not reposition the unit"
    );
    assert!(
        unit_entity.navigation.nav_com().is_none(),
        "the release queues no Move"
    );
}
