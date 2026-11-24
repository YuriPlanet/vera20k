//! House outcome tests: short-game defeat counting, flag-to-die, defeat and victory
//! with passive houses and one-way alliances, building captures and the EVA events
//! they raise.

use super::*;

fn short_game_defeat_test_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[General]\nBaseUnit=AMCV,SMCV,PCV\n\n\
         [InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n1=AMCV\n2=SMCV\n3=PCV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GACNST\n1=CAGAS01\n\n\
         [CAGAS01]\nStrength=400\nArmor=wood\nInsignificant=yes\nCanBeOccupied=yes\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
         [AMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
         [SMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
         [PCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
         [GACNST]\nStrength=1000\nArmor=wood\nConstructionYard=yes\nUndeploysInto=AMCV\n\n\
         [CombatDamage]\nC4Warhead=Super\n\n\
         [Warheads]\n0=Super\n\n\
         [Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[GACNST]\nFoundation=4x3\n"),
    )
    .expect("short game defeat test rules should parse")
}

/// A dying MCV stays tracked until its destructor's Remove_Tracking at the
/// pending-delete drain, so the short game defeats its house only after.
#[test]
fn short_game_counts_a_dying_base_unit_until_it_is_deleted() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options_mut_for_test().short_game = true;
    let owner = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    insert_test_entity_for_owner(&mut sim, 1, owner, "AMCV", EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .expect("AMCV inserted")
        .death
        .set_dying_for_test(true);

    check_defeat_now(&mut sim, Some(&rules));
    assert!(!sim.houses[&owner].is_defeated(), "still tracked");

    sim.uninit(1);
    sim.flush_pending_delete();
    check_defeat_now(&mut sim, Some(&rules));
    assert!(sim.houses[&owner].is_defeated());
}

/// Pending destruction4F87C2 runs before the frame-zero defeat gate;
/// the existing Blowup_All receiver retains damage attribution and ownership.
#[test]
fn flag_to_die_destroys_owned_objects_at_frame_zero_before_defeat_gate() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    let owner = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    let peer = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    insert_test_entity_for_owner(&mut sim, 1, owner, "MTNK", EntityCategory::Unit);
    insert_test_entity_for_owner(&mut sim, 2, peer, "MTNK", EntityCategory::Unit);
    let rng = sim.scenario_rng.logical_state();
    assert!(sim.flag_house_to_die(owner));
    assert!(sim.flag_house_to_die(owner));
    assert_eq!(
        sim.scenario_rng.logical_state(),
        rng,
        "Flag_To_Die has no RNG call"
    );
    assert!(sim.houses[&owner].is_to_die());
    let generated = sim
        .house_rung(&rules, None, false, &mut FrameEffects::default(), None)
        .expect("headless outcome waits complete synchronously");
    assert!(generated.is_empty());
    assert!(!sim.houses[&owner].is_to_die());
    assert!(
        !sim.houses[&owner].is_defeated(),
        "frame-zero defeat gate is separate"
    );
    assert_eq!(sim.entities().get(1).unwrap().health.current(), 0);
    assert!(sim.entities().get(2).unwrap().health.current() > 0);
    assert!(
        sim.entities()
            .get(1)
            .unwrap()
            .seldom
            .kill_record
            .killer()
            .is_none()
    );
}

/// HouseClass::Update's gate calls Blowup_All (`0x004F8F7B`) before
/// MPlayer_Defeated: the straggler dies to its own Health in C4 damage, with
/// no attacker, so no house is credited.
#[test]
fn defeated_house_is_flagged_has_lost_and_its_stragglers_die() {
    for local_loser in [false, true] {
        let rules = short_game_defeat_test_rules();
        let mut sim = Simulation::new();
        sim.session.game_options_mut_for_test().short_game = true;
        let defeated = insert_house_with_counts(&mut sim, "Americans", 0, 1);
        let survivor = insert_house_with_counts(&mut sim, "Russians", 1, 0);
        sim.session
            .set_current_house_for_test(Some(if local_loser { defeated } else { survivor }));
        // A straggler vehicle owned by the losing house.
        insert_test_entity_for_owner(&mut sim, 1, defeated, "MTNK", EntityCategory::Unit);

        check_defeat_now(&mut sim, Some(&rules));

        // Original4FC5AA..4FC6B7 flags only PlayerPtr's result. Defeated
        // membership is shared, while each process owns its local result timer.
        assert!(sim.houses[&defeated].is_defeated());
        assert_eq!(sim.houses[&defeated].has_lost(), local_loser);
        assert!(!sim.houses[&survivor].has_lost());
        assert_eq!(sim.houses[&survivor].has_won(), !local_loser);
        let remote = if local_loser { survivor } else { defeated };
        assert!(sim.houses[&remote].outcome().is_none());
        let straggler = sim
            .entities()
            .get(1)
            .expect("dead objects wait for the drain");
        assert_eq!(
            straggler.health.current(),
            0,
            "Blowup_All killed the straggler"
        );
        assert!(
            straggler.seldom.kill_record.killer().is_none(),
            "no house is credited"
        );
        assert_eq!(sim.houses[&survivor].stats().units_killed(), 0);
    }
}

#[test]
fn gsi_01_04_house_rung_owns_savour_deadline_and_emits_one_transition_edge() {
    use crate::sim::house_state::HouseOutcomeKind;
    use crate::sim::world::SimSoundEvent;

    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options_mut_for_test().short_game = false;
    sim.session.set_tick_for_test(10);
    sim.session.set_binary_frame_for_test(10);
    let winner = insert_house_with_counts(&mut sim, "Americans", 1, 0);
    let loser = insert_house_with_counts(&mut sim, "Russians", 0, 0);
    sim.session.set_current_house_for_test(Some(winner));

    check_defeat_now(&mut sim, Some(&rules));

    let winner_outcome = sim.houses[&winner].outcome().expect("victory accepted");
    assert_eq!(winner_outcome.kind, HouseOutcomeKind::Victory);
    // Original C4 scalar control reuses the same constructor double *900
    // ->7C5F00 (FPCW0E7F): read_double_percent.json absent row is26.
    assert_eq!(
        sim.houses[&winner].outcome_timer(),
        crate::sim::timer::CdTimer::started(10, 26)
    );
    assert!(sim.session.outcome_request().is_none());
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::MatchOutcome { .. }))
            .count(),
        1,
        "only the current player's accepted victory emits its EVA edge"
    );

    sim.sound_events.clear_for_test();
    sim.session.set_tick_for_test(35);
    sim.session.set_binary_frame_for_test(35);
    check_defeat_now(&mut sim, Some(&rules));
    assert!(sim.ready_outcome_for_owner(winner).is_none());
    assert!(!sim.termination_frame_requested());
    assert!(sim.sound_events.is_empty(), "accepted edges never replay");

    sim.session.set_tick_for_test(36);
    sim.session.set_binary_frame_for_test(36);
    check_defeat_now(&mut sim, Some(&rules));
    assert_eq!(
        sim.ready_outcome_for_owner(winner).unwrap().kind,
        HouseOutcomeKind::Victory
    );
    assert!(!sim.houses[&winner].has_won());
    assert!(sim.houses[&loser].is_defeated());
    assert!(
        sim.houses[&loser].outcome().is_none(),
        "remote defeat has no local result timer"
    );
    assert!(sim.termination_frame_requested());
    assert!(sim.sound_events.is_empty(), "expiry does not replay EVA");
}

/// Retail CAGAS01 is `Insignificant=yes`, and Add_Tracking skips
/// Insignificant types (`0x004FF71B`), including on ChangeOwner
/// (`0x007015E6`): a captured garrison does not keep its captor alive, and
/// Blowup_All destroys it with the rest of that house.
#[test]
fn gsi_05_16_a_captured_insignificant_garrison_does_not_keep_its_house_alive() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options_mut_for_test().short_game = true;
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 0, 0);
    let player = insert_house_with_counts(&mut sim, "Americans", 0, 1);
    let enemy = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    sim.session.set_current_house_for_test(Some(enemy));
    let mut garrison = GameEntity::test_default_of_category(
        1,
        "CAGAS01",
        "Neutral",
        10,
        10,
        EntityCategory::Structure,
    );
    garrison.set_owner_for_test(civilian);
    garrison.set_type_ref_for_test(sim.interner.intern("CAGAS01"));
    *garrison.tracking_facts_mut_for_test() =
        crate::sim::house_tracking::TrackingFacts::insignificant_for_test();
    sim.substrate.entities.insert(garrison);
    crate::sim::house_tracking::HouseTracking::register_object(
        &mut sim.substrate,
        &mut sim.houses,
        1,
    );

    // The passenger reconciler uses this chokepoint when the first occupant
    // captures a civilian CanBeOccupied building.
    sim.change_owner(1, player);
    check_defeat_now(&mut sim, Some(&rules));

    assert_eq!(sim.houses[&player].tracking().buildings(), 0);
    assert!(sim.houses[&player].is_defeated());
    assert!(sim.houses[&enemy].has_won());
    assert_eq!(sim.entities().get(1).unwrap().health.current(), 0);
}

/// Insert a `MultiplayPassive=true` house — the stock `Neutral` (Civilian) and
/// `Special` (JP) shape, which every skirmish creates and which owns civilian
/// map objects for the whole match.
fn insert_passive_house_with_counts(
    sim: &mut Simulation,
    name: &str,
    buildings: i32,
    units: i32,
) -> crate::sim::intern::InternedId {
    let owner = insert_house_with_counts(sim, name, buildings, units);
    let house = sim.houses.get_mut(&owner).expect("house just inserted");
    house.set_multiplay_passive_for_test(true);
    // Stock Civilian/JP are never player-controlled.
    house.set_is_human_for_test(false);
    owner
}

/// Build an alliance graph with exactly the edges given — no symmetrization, so
/// a single `(a, b)` pair models a one-way alliance.
fn directed_alliances(edges: &[(&str, &str)]) -> HouseAllianceMap {
    let mut map = HouseAllianceMap::new();
    for (from, to) in edges {
        map.entry(from.to_ascii_uppercase())
            .or_default()
            .insert(to.to_ascii_uppercase());
        map.entry(to.to_ascii_uppercase()).or_default();
    }
    map
}

#[test]
fn passive_house_owning_buildings_does_not_block_last_player_victory() {
    // The player-visible bug: Neutral/Special own civilian structures on most
    // stock maps, so before the passive filter the alive set never reached 1 and
    // the victory screen never appeared after the last opponent died.
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options_mut_for_test().short_game = false;
    let survivor = insert_house_with_counts(&mut sim, "Americans", 3, 4);
    let loser = insert_house_with_counts(&mut sim, "Russians", 0, 0);
    sim.session.set_current_house_for_test(Some(survivor));
    insert_passive_house_with_counts(&mut sim, "Neutral", 7, 0);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(sim.houses[&loser].is_defeated());
    assert!(
        sim.houses[&survivor].has_won(),
        "last non-passive house standing must win despite the Civilian house owning buildings"
    );
}

#[test]
fn explicit_solo_outcomes_advance_and_reach_the_shared_ready_query() {
    use crate::sim::house_state::HouseOutcomeKind;
    for kind in [HouseOutcomeKind::Victory, HouseOutcomeKind::Defeat] {
        let mut sim = Simulation::new();
        let player = insert_house_with_counts(&mut sim, "Americans", 1, 1);
        insert_passive_house_with_counts(&mut sim, "Neutral", 4, 0);
        insert_passive_house_with_counts(&mut sim, "Special", 2, 0);
        sim.session.set_current_house_for_test(Some(player));
        let house = sim.houses.get_mut(&player).unwrap();
        assert!(match kind {
            HouseOutcomeKind::Victory => house.accept_win_result(0, 3, false),
            HouseOutcomeKind::Defeat => house.accept_lose_result(0, 3, false),
        });
        // Accepted outcomes carry no automatic/explicit origin discriminator.
        // Their timer and app-visible readiness must not depend on opponents.
        for tick in 0..=3 {
            sim.session.set_tick_for_test(tick);
            sim.session.set_binary_frame_for_test(tick as u32);
            check_defeat_now(&mut sim, None);
            assert_eq!(sim.termination_frame_requested(), tick == 3);
            assert_eq!(sim.ready_outcome_for_owner(player).is_some(), tick == 3);
        }
        assert_eq!(sim.ready_outcome_for_owner(player).unwrap().kind, kind);
        assert!(
            sim.sound_events.is_empty(),
            "timer advancement must not replay EVA"
        );
    }
}

#[test]
fn one_way_alliance_does_not_end_the_game() {
    // Original4FC3DE..4FC591 scans at a defeat, not every House rung;
    //4FC6AC flags only this process's PlayerPtr after mutual-alliance checks.
    let rules = short_game_defeat_test_rules();
    for mutual in [false, true] {
        let mut sim = Simulation::new();
        sim.session.game_options_mut_for_test().short_game = false;
        let a = insert_house_with_counts(&mut sim, "Americans", 1, 1);
        let b = insert_house_with_counts(&mut sim, "Russians", 1, 1);
        let opponent = insert_house_with_counts(&mut sim, "YuriCountry", 1, 0);
        sim.session.set_current_house_for_test(Some(a));
        *sim.house_alliances_mut_for_test() = if mutual {
            directed_alliances(&[("Americans", "Russians"), ("Russians", "Americans")])
        } else {
            directed_alliances(&[("Americans", "Russians")])
        };
        check_defeat_now(&mut sim, Some(&rules));
        assert!(
            !sim.houses[&a].has_won(),
            "no defeat has triggered completion"
        );
        sim.houses
            .get_mut(&opponent)
            .unwrap()
            .tracking_mut_for_test()
            .set_buildings_for_test(0);
        check_defeat_now(&mut sim, Some(&rules));
        assert!(sim.houses[&opponent].is_defeated());
        assert_eq!(
            sim.houses[&a].has_won(),
            mutual,
            "only mutual allies qualify after opponent defeat"
        );
        assert!(
            !sim.houses[&b].has_won(),
            "remote ally has no local result flag"
        );
        assert!(sim.houses[&b].outcome().is_none());
        assert!(sim.houses[&opponent].outcome().is_none());
    }
}

fn capture_eva_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n1=CAOILD\n\n\
         [GAPOWR]\nStrength=100\nArmor=wood\nCapturable=yes\n\n\
         [CAOILD]\nStrength=1000\nArmor=wood\nCapturable=yes\nNeedsEngineer=yes\n\
         CaptureEvaEvent=EVA_OilRefineryCaptured\n",
    );
    RuleSet::from_ini(&ini).expect("capture eva rules should parse")
}

fn captured_events(
    sim: &Simulation,
) -> Vec<(InternedId, InternedId, bool, bool, Option<InternedId>)> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::BuildingCaptured {
                old_owner,
                new_owner,
                tech_building,
                radar,
                capture_eva_event,
                ..
            } => Some((
                *old_owner,
                *new_owner,
                *tech_building,
                radar.is_some_and(|request| {
                    request.event_type == crate::sim::radar::RadarEventType::BuildingCaptured
                }),
                *capture_eva_event,
            )),
            _ => None,
        })
        .collect()
}

/// `BuildingClass::ChangeOwner 0x004483FB..0x0044848F`: an ordinary building
/// goes through `CreateRadarEvent(10, cell)` and announces only when the
/// client's radar array accepted the event, so the world publishes the type-10
/// request with every such capture (the fourth tuple field). Row 10 of the
/// `0x007F0998` type table is not unique; `render::radar::events` pins that a
/// second capture next to a live event is accepted too.
#[test]
fn engineer_capture_of_an_ordinary_building_requests_the_radar_event() {
    let rules = capture_eva_rules();
    let mut sim = Simulation::new();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let player = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    let enemy = insert_house_with_counts(&mut sim, "Russians", 2, 0);
    sim.houses
        .get_mut(&enemy)
        .expect("enemy")
        .set_is_human_for_test(false);
    insert_test_entity_for_owner(&mut sim, 1, enemy, "GAPOWR", EntityCategory::Structure);
    insert_test_entity_for_owner(&mut sim, 2, enemy, "GAPOWR", EntityCategory::Structure);
    sim.substrate
        .entities
        .get_mut(2)
        .expect("second plant")
        .position
        .set_rx_for_test(11);

    sim.announce_building_capture(1, player, &rules);
    assert_eq!(
        captured_events(&sim),
        vec![(enemy, player, false, true, None)]
    );

    // A second capture one cell away publishes its own request as well.
    sim.sound_events.clear_for_test();
    sim.announce_building_capture(2, player, &rules);
    assert_eq!(
        captured_events(&sim),
        vec![(enemy, player, false, true, None)]
    );
}

/// `0x00448401 MOV CL,[Type+0x1552]` (`NeedsEngineer=`): no radar event; the
/// old owner's `EVA_TechBuildingLost` and the new owner's `CaptureEvaEvent=`
/// are the app's to route.
#[test]
fn engineer_capture_of_a_tech_building_carries_its_capture_eva_event() {
    let rules = capture_eva_rules();
    let mut sim = Simulation::new();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let player = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 1, 0);
    insert_test_entity_for_owner(&mut sim, 1, civilian, "CAOILD", EntityCategory::Structure);

    sim.announce_building_capture(1, player, &rules);
    let line = sim.interner.get("EVA_OilRefineryCaptured");
    assert!(
        line.is_some(),
        "the CaptureEvaEvent name is interned for the app"
    );
    assert_eq!(
        captured_events(&sim),
        vec![(civilian, player, true, false, line)],
        "NeedsEngineer skips CreateRadarEvent"
    );
}

/// `0x004483C6/0x004483D1`: a human house on one side is required, and
/// `0x004483E1`: a `MultiplayPassive` new owner never announces.
#[test]
fn engineer_capture_is_silent_without_a_human_side_or_into_a_passive_house() {
    let rules = capture_eva_rules();
    let mut sim = Simulation::new();
    // Skirmish: `IsControlledByHuman` is the human flag alone (`0x0050B730`).
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let ai_a = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    let ai_b = insert_house_with_counts(&mut sim, "Cubans", 0, 0);
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 0, 0);
    let player = insert_house_with_counts(&mut sim, "Americans", 1, 0);
    for ai in [ai_a, ai_b] {
        let house = sim.houses.get_mut(&ai).expect("ai house");
        house.set_is_human_for_test(false);
        house.set_player_control_for_test(false);
    }
    insert_test_entity_for_owner(&mut sim, 1, ai_a, "GAPOWR", EntityCategory::Structure);
    insert_test_entity_for_owner(&mut sim, 2, player, "GAPOWR", EntityCategory::Structure);

    sim.announce_building_capture(1, ai_b, &rules);
    assert!(
        captured_events(&sim).is_empty(),
        "AI-vs-AI capture: nobody listens"
    );
    sim.announce_building_capture(2, civilian, &rules);
    assert!(
        captured_events(&sim).is_empty(),
        "passive new owner is silent"
    );
}

/// `HouseClass::MPlayer_Defeated 0x004FC30F..0x004FC3BC`: every non-passive
/// defeat is announced; the app splits local from other.
#[test]
fn defeat_of_a_non_passive_house_emits_player_defeated() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    let player = insert_house_with_counts(&mut sim, "Americans", 1, 0);
    let enemy = insert_house_with_counts(&mut sim, "Russians", 0, 0);
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 0, 0);

    check_defeat_now(&mut sim, Some(&rules));

    let defeated: Vec<InternedId> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::PlayerDefeated { house } => Some(*house),
            _ => None,
        })
        .collect();
    assert_eq!(defeated, vec![enemy]);
    assert!(!sim.houses[&player].is_defeated());
    assert!(
        !sim.houses[&civilian].is_defeated(),
        "passive houses are never evaluated"
    );

    // A house already flagged defeated is not announced again.
    sim.sound_events.clear_for_test();
    check_defeat_now(&mut sim, Some(&rules));
    assert!(
        sim.sound_events
            .iter()
            .all(|event| !matches!(event, SimSoundEvent::PlayerDefeated { .. }))
    );
}

/// `BuildingClass::ChangeOwner` takes along a `WeaponsFactory=` building's
/// Foot in radio contact that answers NEED_TO_MOVE with ROGER, wherever it
/// stands (`0x00448559..0x004486C8`). After its own swap the building sends
/// it HELLO and restores the tether on both ends (`0x00449289..0x004492D7`).
#[test]
fn a_captured_war_factory_takes_its_contact_along() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=WEAP\n[VehicleTypes]\n0=TANK\n\
         [WEAP]\nStrength=1000\nWeaponsFactory=yes\nFactory=UnitType\n\
         [TANK]\nStrength=300\nSpeed=6\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    ))
    .expect("war factory rules");
    let mut sim = Simulation::new();
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id, side, None, false, 0, 10,
            ));
    }
    let factory = sim
        .spawn_object_at_height("WEAP", "Americans", 10, 10, 0, 0, &rules)
        .expect("factory");
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", 20, 20, 0, 0, &rules)
        .expect("tank");
    for (id, partner) in [(factory, tank), (tank, factory)] {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.seldom.radio_contacts.insert(partner);
        entity.seldom.tether.attach(partner);
    }
    let russians = sim.interner.intern("Russians");
    sim.change_owner_with_rules(
        factory,
        russians,
        &rules,
        None,
        &mut FrameEffects::default(),
    );
    for (id, partner) in [(factory, tank), (tank, factory)] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(entity.owner(), russians, "{id}");
        assert!(entity.seldom.radio_contacts.contains(partner), "{id}");
        assert_eq!(entity.seldom.tether.partner(), Some(partner), "{id}");
    }
}
