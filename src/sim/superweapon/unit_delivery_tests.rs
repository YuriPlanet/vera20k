//! Adapted ra2-mono UnitDelivery scenarios, through VERA's production owners.
//! Contributed under VERA's GPLv3, from the author's earlier ra2-mono work.
//! These are synthetic Rust regressions, not native executable comparisons.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::superweapon::{construct_house_supers, trigger_grant_super};

const RULES: &str = "[Countries]\n0=Americans\n1=Russians\n2=Neutral\n3=Special\n4=CIVONE\n5=CIVTWO\n\
 [Sides]\nAllied=Americans\nSoviet=Russians\nCivilian=CIVONE,CIVTWO\n\
 [InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n0=ORCA\n\
 [BuildingTypes]\n0=GAPOWR\n\
 [E1]\nStrength=100\nSpeed=4\nSpeedType=Foot\n\
 Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
 [MTNK]\nStrength=300\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\n\
 Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
 [ORCA]\nStrength=150\nSpeed=8\nSpeedType=Winged\nMovementZone=Fly\n\
 Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n\
 [GAPOWR]\nStrength=1000\nPower=200\nBaseNormal=yes\nAdjacent=1\n\
 [SuperWeaponTypes]\n0=Delivery\n[Delivery]\nType=UnitDelivery\nRechargeTime=1\n\
 IsPowered=no\nAction=Custom\nDeliver.Types=MTNK\n";
const ART: &str = "[GAPOWR]\nFoundation=1x1\n";

fn rules(extra: &str) -> RuleSet {
    rules_with_art(extra, ART)
}

fn rules_with_art(extra: &str, art: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(extra));
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&IniFile::from_str(art))
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=DeliveryCue\n[DeliveryCue]\nSounds=delivery\n",
    ));
    rules.bind_type_sound_references(processed.ini(), &sounds);
    rules
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    for (name, human) in [
        ("Americans", true),
        ("Russians", false),
        ("Neutral", false),
        ("Special", false),
        ("CIVONE", false),
        ("CIVTWO", false),
    ] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, Some(id), human, 2000, 10));
        construct_house_supers(&mut sim, rules, id, false);
    }
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().super_weapons = true;
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    sim
}

fn charge(sim: &mut Simulation, rules: &RuleSet, owner: &str) -> (InternedId, InternedId) {
    let owner = sim.interner.get(owner).unwrap();
    let sw = sim.interner.get("Delivery").unwrap();
    assert!(trigger_grant_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &sim.houses,
        &mut sim.super_weapons,
        rules,
        owner,
        sw,
        false
    ));
    let instance = sim
        .super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw)
        .unwrap();
    instance.is_ready = true;
    instance.charge_start_tick = sim.session.binary_frame() as i32;
    instance.charge_duration = 1;
    (owner, sw)
}

fn fire(sim: &mut Simulation, rules: &RuleSet, owner: &str, cell: (u16, u16)) {
    let (_, sw) = charge(sim, rules, owner);
    sim.apply_command_with_overlays(
        owner,
        &Command::LaunchSuperWeapon {
            sw_type_id: sw,
            target_rx: cell.0,
            target_ry: cell.1,
        },
        rules,
        None,
        FrameEffects::default(),
    );
}

fn payloads(sim: &Simulation, name: &str) -> Vec<u64> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| {
            sim.interner
                .resolve(entity.type_ref())
                .eq_ignore_ascii_case(name)
        })
        .map(|entity| entity.stable_id())
        .collect()
}

#[test]
fn unit_delivery_vehicle_appears_at_deferment_not_one_frame_before() {
    let rules = rules("");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, "Americans", (12, 10));
    assert!(payloads(&sim, "MTNK").is_empty());
    assert_eq!(sim.unit_deliveries.pending.len(), 1);
    for frame in 1..20 {
        sim.session.set_binary_frame_for_test(frame);
        tick(&mut sim, &rules, None, FrameEffects::default());
        assert!(payloads(&sim, "MTNK").is_empty());
    }
    sim.session.set_binary_frame_for_test(20);
    tick(&mut sim, &rules, None, FrameEffects::default());
    assert_eq!(payloads(&sim, "MTNK").len(), 1);
    assert!(sim.unit_deliveries.pending.is_empty());
    tick(&mut sim, &rules, None, FrameEffects::default());
    assert_eq!(payloads(&sim, "MTNK").len(), 1, "apply once");
}

#[test]
fn unit_delivery_zero_deferment_sets_human_area_guard_and_ai_hunt() {
    let rules = rules("[Delivery]\nSW.Deferment=0\n");
    for (owner, mission) in [
        ("Americans", MissionType::AreaGuard),
        ("Russians", MissionType::Hunt),
    ] {
        let mut sim = world(&rules);
        fire(&mut sim, &rules, owner, (12, 10));
        let ids = payloads(&sim, "MTNK");
        assert_eq!(ids.len(), 1);
        let entity = sim.substrate.entities.get(ids[0]).unwrap();
        assert_eq!(sim.interner.resolve(entity.owner()), owner);
        assert_eq!(entity.mission.effective().known(), Some(mission));
    }
}

#[test]
fn unit_delivery_duplicate_payloads_find_free_cells_near_blocked_target() {
    let rules = rules("[Delivery]\nSW.Deferment=0\nDeliver.Types=MTNK,MTNK\n");
    let mut sim = world(&rules);
    sim.spawn_object("GAPOWR", "Americans", 12, 10, 0, &rules)
        .unwrap();
    fire(&mut sim, &rules, "Americans", (12, 10));
    let ids = payloads(&sim, "MTNK");
    assert_eq!(ids.len(), 2);
    let cells: Vec<_> = ids
        .iter()
        .map(|id| {
            let entity = sim.substrate.entities.get(*id).unwrap();
            (entity.position.rx(), entity.position.ry())
        })
        .collect();
    assert_ne!(cells[0], cells[1]);
    for cell in cells {
        assert_ne!(cell, (12, 10));
        assert!(cell.0.abs_diff(12) <= 1 && cell.1.abs_diff(10) <= 1);
    }
}

#[test]
fn unit_delivery_missing_payload_or_invalid_target_preserves_command_state() {
    for (extra, cell) in [
        ("[Delivery]\nDeliver.Types=UNKNOWN\n", (12, 10)),
        ("", (u16::MAX, 10)),
    ] {
        let rules = rules(extra);
        let mut sim = world(&rules);
        let (owner, sw) = charge(&mut sim, &rules, "Americans");
        let before = sim.state_hash();
        assert!(!sim.fire_super_weapon(
            &rules,
            owner,
            sw,
            cell,
            None,
            FrameEffects::default(),
            false
        ));
        assert_eq!(sim.state_hash(), before);
        assert!(sim.unit_deliveries.pending.is_empty());
    }
}

#[test]
fn unit_delivery_owner_modes_use_retained_houses_and_civilian_side_order() {
    for (mode, expected) in [
        ("invoker", "Americans"),
        ("neutral", "Neutral"),
        ("special", "Special"),
        ("civilian", "CIVONE"),
    ] {
        let rules = rules(&format!(
            "[Delivery]\nSW.Deferment=0\nDeliver.Owner={mode}\n"
        ));
        let mut sim = world(&rules);
        fire(&mut sim, &rules, "Americans", (12, 10));
        let ids = payloads(&sim, "MTNK");
        assert_eq!(ids.len(), 1, "{mode}");
        assert_eq!(
            sim.interner
                .resolve(sim.substrate.entities.get(ids[0]).unwrap().owner()),
            expected
        );
    }
}

#[test]
fn unit_delivery_rules_layer_omissions_and_clear_survive_binary_record_roundtrip() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(RULES));
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str(
            "[Delivery]\nSW.Deferment=7\nDeliver.Types=mtNk,MTNK\nDeliver.Owner=neutral\n",
        ),
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Delivery]\nRechargeTime=2\n"),
    );
    let processed = layers
        .process_with_fixed_art(&IniFile::from_str(ART))
        .unwrap();
    let saved = crate::util::save_codec::serialize(&processed).unwrap();
    let restored = crate::util::save_codec::deserialize(&saved).unwrap();
    let rules = RuleSet::from_processed_rules(&restored).unwrap();
    let config = rules
        .super_weapon("Delivery")
        .unwrap()
        .unit_delivery
        .as_ref()
        .unwrap();
    assert_eq!(config.deferment, 7);
    assert_eq!(config.types.len(), 2);
    assert_eq!(config.owner, DeliveryOwner::Neutral);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Delivery]\nDeliver.Types=none\n"),
    );
    let rules = RuleSet::from_processed_rules(
        &layers
            .process_with_fixed_art(&IniFile::from_str(ART))
            .unwrap(),
    )
    .unwrap();
    assert!(
        rules
            .super_weapon("Delivery")
            .unwrap()
            .unit_delivery
            .as_ref()
            .unwrap()
            .types
            .is_empty()
    );
}

#[test]
fn unit_delivery_pending_save_restores_deadline_payload_and_one_shot_completion() {
    let rules = rules("[Delivery]\nDeliver.Types=MTNK,MTNK\nSW.ActivationSound=DeliveryCue\n");
    let mut original = world(&rules);
    fire(&mut original, &rules, "Americans", (12, 10));
    original.session.set_binary_frame_for_test(7);
    let hash = original.state_hash();
    original.unit_deliveries.pending[0].due_frame += 1;
    assert_ne!(
        original.state_hash(),
        hash,
        "deadline participates in lockstep hash"
    );
    original.unit_deliveries.pending[0].due_frame -= 1;
    let bytes = crate::sim::snapshot::GameSnapshot::save(
        &original,
        0,
        rules.simulation_config_hash(),
        "delivery",
        0,
    );
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    // The synthetic snapshot omits resident map resources. Reinstall the same
    // empty arena before resuming its saved pending delivery.
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.unit_deliveries, original.unit_deliveries);
    restored.sound_events.clear_for_test();
    original.sound_events.clear_for_test();
    for frame in [19, 20, 21] {
        for sim in [&mut original, &mut restored] {
            sim.session.set_binary_frame_for_test(frame);
            tick(sim, &rules, None, FrameEffects::default());
            assert_eq!(payloads(sim, "MTNK").len(), if frame < 20 { 0 } else { 2 });
        }
    }
    let records = |sim: &Simulation| {
        payloads(sim, "MTNK")
            .into_iter()
            .map(|id| {
                let entity = sim.substrate.entities.get(id).unwrap();
                (
                    id,
                    entity.owner(),
                    entity.position.rx(),
                    entity.position.ry(),
                    entity.mission.effective(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(records(&restored), records(&original));
    assert_eq!(restored.sound_events.iter().filter(|event| matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "DeliveryCue")).count(), 1);
}

#[test]
fn unit_delivery_base_override_survives_save_and_capture_without_affecting_ordinary_buildings() {
    let rules = rules("[Delivery]\nSW.Deferment=0\nDeliver.Types=GAPOWR\nDeliver.BaseNormal=no\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, "Americans", (12, 10));
    let id = payloads(&sim, "GAPOWR")[0];
    let ordinary = sim
        .spawn_object("GAPOWR", "Americans", 20, 20, 0, &rules)
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(ordinary)
            .unwrap()
            .unit_delivery_provides_base_space()
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "delivered-building", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    assert!(
        !restored
            .substrate
            .entities
            .get(id)
            .unwrap()
            .unit_delivery_provides_base_space()
    );
    assert!(
        restored
            .substrate
            .entities
            .get(ordinary)
            .unwrap()
            .unit_delivery_provides_base_space()
    );
    let other = restored.interner.get("Russians").unwrap();
    restored.change_owner_with_rules(id, other, &rules, None, FrameEffects::default());
    assert!(!crate::sim::production::is_within_build_area(
        &restored,
        &rules,
        "Russians",
        rules.object("GAPOWR").unwrap(),
        13,
        10
    ));
}

#[test]
fn unit_delivery_infantry_groups_use_legal_subcells() {
    let rules = rules("[Delivery]\nSW.Deferment=0\nDeliver.Types=E1,E1,E1,E1,E1,E1\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, "Americans", (12, 10));
    let ids = payloads(&sim, "E1");
    assert_eq!(ids.len(), 6);
    let target_count = ids
        .iter()
        .filter(|id| {
            let entity = sim.substrate.entities.get(**id).unwrap();
            (entity.position.rx(), entity.position.ry()) == (12, 10)
        })
        .count();
    assert_eq!(
        target_count,
        crate::sim::movement::bump_crush::FUNCTIONAL_SUB_CELLS.len(),
        "use the existing native infantry spot capacity before nearby fallback"
    );
}

#[test]
fn unit_delivery_ambiguous_techno_id_is_not_rebound_to_wrong_category() {
    let rules = rules("[AircraftTypes]\n1=MTNK\n");
    let config = rules
        .super_weapon("Delivery")
        .unwrap()
        .unit_delivery
        .as_ref()
        .unwrap();
    assert!(
        config.types.is_empty(),
        "ambiguous cross-category reference is explicitly unsupported"
    );
}

#[test]
fn unit_delivery_naval_payload_uses_water_and_land_payload_cannot_spawn_there() {
    let rules = rules(
        "[VehicleTypes]\n1=BOAT\n[BOAT]\nStrength=300\nSpeed=6\nNaval=yes\n\
        SpeedType=Float\nMovementZone=Water\nLocomotor={4A582743-9839-11d1-B709-00A024DDAFD1}\n\
        [Water]\nFloat=100%\nTrack=0%\nFoot=0%\nBuildable=no\n\
        [Delivery]\nSW.Deferment=0\nDeliver.Types=BOAT,MTNK\n",
    );
    let mut sim = world(&rules);
    crate::sim::arena_fixture::flood(&mut sim, &rules, 32, [(12, 10), (13, 10)]);
    fire(&mut sim, &rules, "Americans", (12, 10));
    assert_eq!(payloads(&sim, "BOAT").len(), 1);
    assert_eq!(payloads(&sim, "MTNK").len(), 1);
    for (name, water) in [("BOAT", true), ("MTNK", false)] {
        let entity = sim.substrate.entities.get(payloads(&sim, name)[0]).unwrap();
        let cell = (entity.position.rx(), entity.position.ry());
        assert_eq!([(12, 10), (13, 10)].contains(&cell), water);
    }
}

#[test]
fn unit_delivery_no_legal_site_discards_constructor_and_tracking_without_retrying_later() {
    let rules = rules("[Water]\nTrack=0%\nBuildable=no\n[Delivery]\nSW.Deferment=0\n");
    let mut sim = world(&rules);
    crate::sim::arena_fixture::flood(
        &mut sim,
        &rules,
        32,
        (0..32).flat_map(|x| (0..32).map(move |y| (x, y))),
    );
    fire(&mut sim, &rules, "Americans", (12, 10));
    assert!(payloads(&sim, "MTNK").is_empty());
    assert!(sim.unit_deliveries.pending.is_empty());
    let owner = sim.interner.get("Americans").unwrap();
    let ty = sim.interner.get("MTNK").unwrap();
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .owned_count(crate::map::entities::EntityCategory::Unit, ty),
        0
    );
    assert_eq!(
        sim.houses[&owner]
            .tracking()
            .active_count(crate::map::entities::EntityCategory::Unit, ty),
        0
    );
    assert!(
        !sim.super_weapons[&owner][&sim.interner.get("Delivery").unwrap()].is_ready,
        "valid launch consumes charge even when no site exists"
    );
}

#[test]
fn unit_delivery_frame_wrap_and_snapshot_owner_validation() {
    let rules = rules("[Delivery]\nSW.Deferment=4\n");
    let mut sim = world(&rules);
    sim.session.set_binary_frame_for_test(u32::MAX - 2);
    fire(&mut sim, &rules, "Americans", (12, 10));
    for frame in [u32::MAX - 1, u32::MAX, 0] {
        sim.session.set_binary_frame_for_test(frame);
        tick(&mut sim, &rules, None, FrameEffects::default());
        assert!(payloads(&sim, "MTNK").is_empty());
    }
    let ghost = sim.interner.intern("MissingHouse");
    let mut bad = sim.unit_deliveries.clone();
    bad.pending[0].owner = ghost;
    assert!(bad.validate(&sim).is_err());
    let mut bad = sim.unit_deliveries.clone();
    bad.pending[0].due_frame = bad.pending[0].issued_frame.wrapping_sub(1);
    assert!(bad.validate(&sim).is_err());
    sim.session.set_binary_frame_for_test(1);
    tick(&mut sim, &rules, None, FrameEffects::default());
    assert_eq!(payloads(&sim, "MTNK").len(), 1);
}

#[test]
fn unit_delivery_bad_ai_configuration_is_rejected_before_target_search() {
    let rules = rules("[Delivery]\nDeliver.Types=none\n");
    let mut sim = world(&rules);
    let (owner, _) = charge(&mut sim, &rules, "Russians");
    let hash = sim.state_hash();
    crate::sim::superweapon::ai_fire::AI_FIRE_LOG.set(Some(Vec::new()));
    crate::sim::superweapon::ai_fire::try_fire(
        &mut sim,
        &rules,
        owner,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        crate::sim::superweapon::ai_fire::AI_FIRE_LOG
            .take()
            .unwrap(),
        vec![crate::sim::superweapon::ai_fire::AiFireEvent::TryFire]
    );
    assert_eq!(sim.state_hash(), hash);
}

#[test]
fn unit_delivery_civilian_uses_first_existing_house_not_first_country() {
    let rules = rules("[Delivery]\nSW.Deferment=0\nDeliver.Owner=civilian\n");
    for remove_first in [true, false] {
        let mut sim = world(&rules);
        let first = sim.interner.get("CIVONE").unwrap();
        let second = sim.interner.get("CIVTWO").unwrap();
        if remove_first {
            sim.houses.remove_for_test(&first);
            sim.super_weapons.map_mut_for_test().remove(&first);
        } else {
            let first_index = sim
                .houses
                .order()
                .iter()
                .position(|id| *id == first)
                .unwrap();
            let second_index = sim
                .houses
                .order()
                .iter()
                .position(|id| *id == second)
                .unwrap();
            sim.houses.swap_order_for_test(first_index, second_index);
        }
        fire(&mut sim, &rules, "Americans", (12, 10));
        let ids = payloads(&sim, "MTNK");
        assert_eq!(ids.len(), 1, "remove_first={remove_first}");
        assert_eq!(sim.substrate.entities.get(ids[0]).unwrap().owner(), second);
    }
}

#[test]
fn unit_delivery_negative_deferment_waits_twenty_frames() {
    let rules = rules("[Delivery]\nSW.Deferment=-1\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, "Americans", (12, 10));
    assert!(payloads(&sim, "MTNK").is_empty());
    sim.session.set_binary_frame_for_test(19);
    tick(&mut sim, &rules, None, FrameEffects::default());
    assert!(payloads(&sim, "MTNK").is_empty());
    sim.session.set_binary_frame_for_test(20);
    tick(&mut sim, &rules, None, FrameEffects::default());
    assert_eq!(payloads(&sim, "MTNK").len(), 1);
    assert!(sim.unit_deliveries.pending.is_empty());
}

#[test]
fn unit_delivery_two_phase_flags_do_not_change_its_one_click_type() {
    for setting in ["PreClick=yes", "PostClick=yes"] {
        let rules = rules(&format!("[Delivery]\nSW.Deferment=0\n{setting}\n"));
        let sw = rules.super_weapon("Delivery").unwrap();
        assert!(!sw.pre_click && !sw.post_click, "{setting}");
        let mut sim = world(&rules);
        fire(&mut sim, &rules, "Americans", (12, 10));
        assert_eq!(payloads(&sim, "MTNK").len(), 1, "{setting}");
    }
}

#[test]
fn unit_delivery_ground_aircraft_avoids_an_occupied_target() {
    let rules = rules("[Delivery]\nSW.Deferment=0\nDeliver.Types=ORCA\n");
    let mut sim = world(&rules);
    sim.spawn_object("GAPOWR", "Americans", 12, 10, 0, &rules)
        .unwrap();
    fire(&mut sim, &rules, "Americans", (12, 10));
    let ids = payloads(&sim, "ORCA");
    assert_eq!(ids.len(), 1);
    let plane = sim.substrate.entities.get(ids[0]).unwrap();
    let cell = (plane.position.rx(), plane.position.ry());
    assert_ne!(cell, (12, 10));
    assert!(cell.0.abs_diff(12) <= 1 && cell.1.abs_diff(10) <= 1);
    assert_eq!(plane.position.exact_z_leptons(), Some(0));
}

#[test]
fn unit_delivery_foot_placement_commits_the_selected_bridge_plane() {
    use crate::sim::movement::locomotor::MovementLayer;

    for name in ["MTNK", "E1"] {
        let rules = rules(&format!(
            "[Water]\nTrack=0%\nFoot=0%\nBuildable=no\n[Delivery]\nSW.Deferment=0\nDeliver.Types={name}\n"
        ));
        let mut sim = world(&rules);
        crate::sim::arena_fixture::flood(&mut sim, &rules, 32, [(12, 10)]);
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(12, 10)
            .unwrap()
            .bridge_facts
            .raw_flags |= 0x100;
        assert!(sim.rebuild_dynamic_navigation(&rules));
        fire(&mut sim, &rules, "Americans", (12, 10));
        let ids = payloads(&sim, name);
        assert_eq!(ids.len(), 1, "{name}");
        let entity = sim.substrate.entities.get(ids[0]).unwrap();
        assert_eq!(
            (entity.position.rx(), entity.position.ry()),
            (12, 10),
            "{name}"
        );
        assert!(entity.on_bridge(), "{name}");
        assert_eq!(
            entity.position.exact_z_leptons(),
            Some(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS),
            "{name}"
        );
        assert_eq!(
            entity.locomotor.as_ref().unwrap().layer(),
            MovementLayer::Bridge,
            "{name}"
        );
        let occupancy = sim.substrate.occupancy.get(12, 10).unwrap();
        assert!(
            occupancy
                .snapshot_layer(MovementLayer::Bridge)
                .contains(&ids[0]),
            "{name}"
        );
        assert!(
            !occupancy
                .snapshot_layer(MovementLayer::Ground)
                .contains(&ids[0]),
            "{name}"
        );
    }
}

#[test]
fn unit_delivery_building_is_centered_on_the_target_cell() {
    let rules = rules_with_art(
        "[Delivery]\nSW.Deferment=0\nDeliver.Types=GAPOWR\n",
        "[GAPOWR]\nFoundation=2x2\n",
    );
    let mut sim = world(&rules);
    fire(&mut sim, &rules, "Americans", (12, 10));
    let ids = payloads(&sim, "GAPOWR");
    assert_eq!(ids.len(), 1);
    let building = sim.substrate.entities.get(ids[0]).unwrap();
    assert_eq!((building.position.rx(), building.position.ry()), (11, 9));
}

#[test]
fn unit_delivery_activation_cue_precedes_building_discovery() {
    let rules = rules(
        "[Delivery]\nSW.Deferment=0\nDeliver.Types=GAPOWR\nSW.ActivationSound=DeliveryCue\n[GAPOWR]\nSuperWeapon=Delivery\n",
    );
    let mut sim = world(&rules);
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    fire(&mut sim, &rules, "Americans", (12, 10));
    let cue = sim.sound_events.iter().position(|event| matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "DeliveryCue")).expect("activation cue");
    let discovery = sim
        .sound_events
        .iter()
        .position(|event| matches!(event, SimSoundEvent::SuperWeaponDetected { .. }))
        .expect("placed provider discovery");
    assert!(cue < discovery);
}

#[test]
fn unit_delivery_retail_flying_foot_lifts_off_without_a_later_order() {
    for name in ["JUMPJET", "ZEP"] {
        let patch = IniFile::from_str(&format!(
            "[SuperWeaponTypes]\n99=Delivery\n[Delivery]\nType=UnitDelivery\nAction=Custom\nIsPowered=no\nSW.Deferment=0\nDeliver.Types={name}\n"
        ));
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "XMP03T4.MAP",
            &patch,
        ) else {
            return;
        };
        let rules = retail.rules;
        let mut sim = world(&rules);
        // This test advances a real match, whose short-game defeat owner
        // destroys a house with no counted building or BaseUnit. Keep two
        // opposing bases alive; a dying Rocketeer can still be airborne and
        // would otherwise hide the same fixture error that removes a Kirov.
        sim.session.game_options_mut_for_test().short_game = true;
        for (owner, cell) in [("Americans", (4, 4)), ("Russians", (24, 24))] {
            sim.spawn_object("GAPOWR", owner, cell.0, cell.1, 0, &rules)
                .expect("retail base building on the clear arena");
        }
        fire(&mut sim, &rules, "Americans", (12, 10));
        let ids = payloads(&sim, name);
        assert_eq!(ids.len(), 1, "{name}");
        let runtime = sim
            .substrate
            .entities
            .get(ids[0])
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .jumpjet_runtime()
            .unwrap();
        assert!(
            runtime.moving(),
            "{name}: post-placement Scatter starts flight"
        );
        for _ in 0..3 {
            sim.advance_tick(&[], Some(&rules), None, None, 67);
        }
        let owner = sim.interner.get("Americans").unwrap();
        assert!(
            !sim.houses[&owner].is_defeated(),
            "{name}: live short-game base"
        );
        let entity = sim
            .substrate
            .entities
            .get(ids[0])
            .expect("delivery remains in the running match");
        assert!(entity.is_ai_alive(), "{name}: a living flying Foot");
        assert!(
            crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref()
            ) > 0,
            "{name}: normal flight owner lifts the delivered object"
        );
    }
}

#[test]
fn unit_delivery_ai_ground_rally_arm_completes_a_delivery() {
    use crate::sim::superweapon::ai_fire::{self, AiFireArm, AiFireEvent};

    let rules = rules("[Delivery]\nSW.Deferment=0\n");
    let mut sim = world(&rules);
    let (owner, sw) = charge(&mut sim, &rules, "Russians");
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_preferred_super_weapon_target((12, 10));
    ai_fire::AI_FIRE_LOG.set(Some(Vec::new()));
    ai_fire::try_fire(&mut sim, &rules, owner, None, FrameEffects::default());
    let log = ai_fire::AI_FIRE_LOG.take().unwrap();
    assert_eq!(
        log,
        vec![
            AiFireEvent::TryFire,
            AiFireEvent::TailArm(AiFireArm::GroundRallyPoint, sw),
            AiFireEvent::Fire(sw, (12, 10))
        ]
    );
    let ids = payloads(&sim, "MTNK");
    assert_eq!(ids.len(), 1);
    let unit = sim.substrate.entities.get(ids[0]).unwrap();
    assert_eq!(unit.owner(), owner);
    assert_eq!((unit.position.rx(), unit.position.ry()), (12, 10));
    assert_eq!(unit.mission.effective().known(), Some(MissionType::Hunt));
    assert!(!sim.super_weapons[&owner][&sw].is_ready);
}

#[test]
fn unit_delivery_same_due_requests_complete_newest_first_and_only_once() {
    let rules = rules("[Delivery]\nSW.Deferment=2\nSW.ActivationSound=DeliveryCue\n");
    let mut sim = world(&rules);
    fire(&mut sim, &rules, "Americans", (12, 10));
    fire(&mut sim, &rules, "Russians", (20, 20));
    assert_eq!(sim.unit_deliveries.pending.len(), 2);
    sim.session.set_binary_frame_for_test(2);
    tick(&mut sim, &rules, None, FrameEffects::default());
    let mut ids = payloads(&sim, "MTNK");
    ids.sort_unstable();
    assert_eq!(ids.len(), 2);
    let owners: Vec<_> = ids
        .iter()
        .map(|&id| {
            sim.interner
                .resolve(sim.substrate.entities.get(id).unwrap().owner())
        })
        .collect();
    assert_eq!(owners, ["Russians", "Americans"]);
    assert!(sim.unit_deliveries.pending.is_empty());
    tick(&mut sim, &rules, None, FrameEffects::default());
    assert_eq!(payloads(&sim, "MTNK").len(), 2);
    assert_eq!(sim.sound_events.iter().filter(|event| matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "DeliveryCue")).count(), 2);
}

#[test]
fn superweapon_money_delivery_settles_once_at_enqueue_and_survives_save_and_provider_loss() {
    for amount in [-2000, 2000] {
        let rules = rules(&format!(
            "[Delivery]\nMoney.Amount={amount}\nSW.Deferment=2\nSW.AuxBuildings=GAPOWR\n[GAPOWR]\nSuperWeapon=Delivery\n"
        ));
        let mut sim = world(&rules);
        let (owner, sw) = charge(&mut sim, &rules, "Americans");
        let provider = sim
            .spawn_object("GAPOWR", "Americans", 8, 8, 0, &rules)
            .unwrap();
        if amount < 0 {
            crate::sim::credit_income::spend_money(&mut sim.houses, owner, 1);
            let before = sim.state_hash();
            assert!(!sim.fire_super_weapon(
                &rules,
                owner,
                sw,
                (12, 10),
                None,
                FrameEffects::default(),
                false
            ));
            assert_eq!(sim.state_hash(), before);
            assert!(sim.unit_deliveries.pending.is_empty());
            crate::sim::credit_income::add_credits(&mut sim.houses, owner, 1);
        }
        assert!(sim.fire_super_weapon(
            &rules,
            owner,
            sw,
            (12, 10),
            None,
            FrameEffects::default(),
            false
        ));
        let settled = 2000 + amount;
        assert_eq!(sim.houses[&owner].economy().credits(), settled);
        assert_eq!(sim.unit_deliveries.pending.len(), 1);
        assert!(!sim.fire_super_weapon(
            &rules,
            owner,
            sw,
            (12, 10),
            None,
            FrameEffects::default(),
            false
        ));
        assert_eq!(sim.houses[&owner].economy().credits(), settled);
        sim.uninit_with_rules(provider, &rules);
        crate::sim::credit_income::add_credits(&mut sim.houses, owner, -1);
        let bytes = crate::sim::snapshot::GameSnapshot::save(
            &sim,
            0,
            rules.simulation_config_hash(),
            "paid delivery",
            0,
        );
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
        restored.restore_after_snapshot_load().unwrap();
        for state in [&mut sim, &mut restored] {
            assert_eq!(state.houses[&owner].economy().credits(), settled - 1);
            state.session.set_binary_frame_for_test(2);
            tick(state, &rules, None, FrameEffects::default());
            assert_eq!(payloads(state, "MTNK").len(), 1);
            assert!(state.unit_deliveries.pending.is_empty());
            tick(state, &rules, None, FrameEffects::default());
            assert_eq!(payloads(state, "MTNK").len(), 1);
            assert_eq!(state.houses[&owner].economy().credits(), settled - 1);
        }
    }
}
