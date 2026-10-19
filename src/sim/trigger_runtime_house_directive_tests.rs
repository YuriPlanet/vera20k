//! House directives through production readers, retained state and consumers.
use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;

fn scene() -> (Simulation, RuleSet, OverlayTypeRegistry, [InternedId; 3]) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[General]\nParanoid=no\n[MultiplayerDialogSettings]\nAlliesAllowed=yes\n\
         [Countries]\n0=Country0\n1=Country1\n2=Country2\n[Sides]\nA=Country0\nB=Country1\nC=Country2\n[Country0]\n[Country1]\n[Country2]\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nStrength=300\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\nPrimary=GUN\nLeadershipRating=10\nOwner=Country0,Country1,Country2\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n1=BASE\n[BASE]\nStrength=600\nFoundation=1x1\nImmuneToPsionics=no\nPrimary=MIND\nOwner=Country0,Country1,Country2\n\
         [GUN]\nDamage=10\nROF=30\nRange=6\nProjectile=INVIS\nWarhead=SA\n\
         [MIND]\nDamage=1\nROF=30\nRange=6\nProjectile=INVIS\nWarhead=CONTROL\n\
         [Projectiles]\n0=INVIS\n[INVIS]\nInviso=yes\nAA=yes\nAG=yes\n\
         [Warheads]\n2=CONTROL\n[CONTROL]\nMindControl=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [SuperWeaponTypes]\n0=REVEAL\n[REVEAL]\nType=PsychicReveal\nRechargeTime=.1\nIsPowered=no\nRevealRadius=1\n",
    );
    sim.session.game_mode_nonzero = true;
    let houses = std::array::from_fn(|i| {
        let name = sim.interner.intern(&format!("H{i}"));
        let country = sim.interner.intern(&format!("Country{i}"));
        sim.houses.insert(
            name,
            HouseState::new(name, i as u8, Some(country), false, 10000, 10),
        );
        sim.session.house_order.push(name);
        name
    });
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nCONTROL=Country0,<none>,CONTROL,0,1,1,1,0\n[Events]\nCONTROL=1,8,0,0\n[Actions]\nCONTROL=1,0,0,0,0,0,0,0,A\n[Tags]\nCONTROLTAG=2,CONTROL,CONTROL\n",
    );
    (sim, rules, registry, houses)
}

fn run(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    owner: Option<InternedId>,
    kind: i32,
    value: &str,
    live: bool,
) -> bool {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
        "[Actions]\nA=1,{kind},0,{value},0,0,0,0,A\n"
    ));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("A")
        .unwrap()
        .entries()[0]
        .clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    let live_index = live.then(|| sim.trigger_runtime.trigger_order[0]);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index,
            incoming_house: owner,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        Some(registry),
        FrameEffects::default(),
    )
}

fn building_scene() -> (
    Simulation,
    RuleSet,
    OverlayTypeRegistry,
    [InternedId; 3],
    [u64; 5],
) {
    let (mut sim, rules, registry, houses) = scene();
    let ids = [
        sim.spawn_object("BASE", "H0", 15, 15, 0, &rules).unwrap(),
        sim.spawn_object("BASE", "H1", 17, 14, 0, &rules).unwrap(),
        sim.spawn_object("BASE", "H1", 17, 16, 0, &rules).unwrap(),
        sim.spawn_object("BASE", "H2", 16, 15, 0, &rules).unwrap(),
        sim.spawn_object("TANK", "H1", 17, 15, 0, &rules).unwrap(),
    ];
    (sim, rules, registry, houses, ids)
}

#[test]
fn house_directive_target_kind_signed_reader_null_house_and_snapshot_keep_cell_independent() {
    for (raw, value) in [
        ("1", 1),
        ("5", 5),
        ("257suffix", 257),
        ("-2147483648", i32::MIN),
        ("2147483647", i32::MAX),
        (".2", 0),
    ] {
        let (mut sim, rules, registry, houses) = scene();
        sim.houses
            .get_mut(&houses[0])
            .unwrap()
            .set_preferred_super_weapon_target((19, 20));
        let before = sim.state_hash();
        let rng = sim.rng_state();
        assert!(run(
            &mut sim,
            &rules,
            &registry,
            Some(houses[0]),
            35,
            raw,
            false
        ));
        assert_eq!(
            sim.houses[&houses[0]].preferred_super_weapon_target_kind(),
            value
        );
        assert_eq!(
            sim.houses[&houses[0]].preferred_super_weapon_target(),
            (19, 20)
        );
        assert_eq!(
            sim.houses[&houses[1]].preferred_super_weapon_target_kind(),
            1
        );
        assert_eq!(sim.state_hash() == before, value == 1);
        let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Target kind", 0);
        let loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
            .unwrap()
            .sim;
        assert_eq!(
            loaded.houses[&houses[0]].preferred_super_weapon_target_kind(),
            value
        );
        let hash = sim.state_hash();
        assert!(run(&mut sim, &rules, &registry, None, 35, "99", false));
        assert_eq!(sim.state_hash(), hash);
        assert_eq!(sim.rng_state(), rng);
    }
}

#[test]
fn house_directive_make_ally_runs_both_directions_and_preserves_native_receipt() {
    let (mut sim, rules, registry, houses) = scene();
    let rng = sim.rng_state();
    assert!(!run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        37,
        "1",
        false
    ));
    assert!(!run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        37,
        "-1",
        true
    ));
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        37,
        "1",
        true
    ));
    assert!(crate::map::houses::is_allied_with(
        &sim.house_alliances,
        "H0",
        "H1"
    ));
    assert!(crate::map::houses::is_allied_with(
        &sim.house_alliances,
        "H1",
        "H0"
    ));
    assert!(!crate::map::houses::is_allied_with(
        &sim.house_alliances,
        "H0",
        "H2"
    ));
    let hash = sim.state_hash();
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        37,
        "1",
        true
    ));
    assert_eq!(sim.state_hash(), hash);
    // CanAlly checks the calling House, so defeat both callers to refuse both directions.
    // A valid selection still reports success.
    sim.houses.get_mut(&houses[0]).unwrap().is_defeated = true;
    sim.houses.get_mut(&houses[2]).unwrap().is_defeated = true;
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        37,
        "2",
        true
    ));
    assert!(!crate::map::houses::is_allied_with(
        &sim.house_alliances,
        "H0",
        "H2"
    ));
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn house_directive_base_control_and_saved_return_use_reverse_live_lists_and_shared_consumers() {
    let (mut sim, rules, registry, houses, ids) = building_scene();
    assert!(
        sim.can_capture(ids[3], ids[1], &rules),
        "the custom building is capturable before transfer"
    );
    let rng = sim.rng_state();
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        123,
        "1",
        true
    ));
    assert_eq!(
        sim.houses[&houses[0]].base_projection.buildings(),
        &[ids[0], ids[2], ids[1]]
    );
    assert!(
        sim.houses[&houses[1]]
            .base_projection
            .buildings()
            .is_empty()
    );
    for id in [ids[1], ids[2]] {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(entity.owner(), houses[0]);
        assert_eq!(entity.mind_control.house_controller(), Some(houses[0]));
        assert_eq!(entity.mind_control.house_original_owner(), Some(houses[1]));
        assert!(
            !entity.mind_control.is_mind_controlled(),
            "House control is a separate native gate"
        );
        assert_eq!(sim.techno_original_owner(id), Some(houses[1]));
        assert!(!sim.can_capture(ids[3], id, &rules));
    }
    assert_eq!(
        sim.substrate.entities.get(ids[4]).unwrap().owner(),
        houses[1],
        "units are outside the native Building list"
    );
    assert_eq!(
        sim.substrate.entities.get(ids[3]).unwrap().owner(),
        houses[2]
    );
    assert_eq!(sim.rng_state(), rng);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Controlled base", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .unwrap();
    loaded.resolve_type_handles(&rules);
    assert_eq!(
        loaded.houses[&houses[0]].base_projection.buildings(),
        &[ids[0], ids[2], ids[1]]
    );
    assert_eq!(loaded.techno_original_owner(ids[1]), Some(houses[1]));
    assert!(!loaded.can_capture(ids[3], ids[1], &rules));
    assert!(run(
        &mut loaded,
        &rules,
        &registry,
        Some(houses[0]),
        124,
        "1",
        true
    ));
    assert_eq!(
        loaded.houses[&houses[1]].base_projection.buildings(),
        &[ids[1], ids[2]]
    );
    assert_eq!(
        loaded.houses[&houses[0]].base_projection.buildings(),
        &[ids[0]]
    );
    for id in [ids[1], ids[2]] {
        let e = loaded.substrate.entities.get(id).unwrap();
        assert_eq!(e.owner(), houses[1]);
        assert_eq!(e.mind_control.house_controller(), None);
        assert_eq!(e.mind_control.house_original_owner(), None);
        assert_eq!(loaded.techno_original_owner(id), Some(houses[1]));
    }
    assert!(loaded.can_capture(ids[3], ids[1], &rules));
}

#[test]
fn house_directive_ordinary_capture_keeps_controller_but_clears_original_and_restore_filter() {
    let (mut sim, rules, registry, houses, ids) = building_scene();
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        123,
        "1",
        true
    ));
    let hash = sim.state_hash();
    sim.change_owner_with_rules(
        ids[1],
        houses[2],
        &rules,
        Some(&registry),
        FrameEffects::default(),
    );
    let entity = sim.substrate.entities.get(ids[1]).unwrap();
    assert_eq!(entity.mind_control.house_controller(), Some(houses[0]));
    assert_eq!(entity.mind_control.house_original_owner(), None);
    assert_eq!(sim.techno_original_owner(ids[1]), None);
    assert_ne!(sim.state_hash(), hash);
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        124,
        "1",
        true
    ));
    assert_eq!(
        sim.substrate.entities.get(ids[1]).unwrap().owner(),
        houses[2]
    );
    assert_eq!(
        sim.substrate.entities.get(ids[2]).unwrap().owner(),
        houses[1]
    );
    // Re-taking a base establishes a fresh pair; it does not revive the old one.
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        123,
        "2",
        true
    ));
    assert_eq!(sim.techno_original_owner(ids[1]), Some(houses[2]));
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        124,
        "2",
        true
    ));
    assert_eq!(
        sim.substrate.entities.get(ids[1]).unwrap().owner(),
        houses[2]
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(ids[1])
            .unwrap()
            .mind_control
            .house_controller(),
        None
    );
}

#[test]
fn house_directive_self_control_and_retained_selector_keep_native_state_writes() {
    let (mut sim, rules, registry, houses, ids) = building_scene();
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        123,
        "0",
        true
    ));
    assert_ne!(
        sim.state_hash(),
        hash,
        "same-owner ChangeOwner refuses, but the caller still stores its flags"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(ids[0])
            .unwrap()
            .mind_control
            .house_controller(),
        Some(houses[0])
    );
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        124,
        "0",
        true
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(ids[0])
            .unwrap()
            .mind_control
            .house_controller(),
        None
    );
    let live = sim.trigger_runtime.trigger_order[0];
    sim.trigger_runtime.triggers[live]
        .as_mut()
        .unwrap()
        .retained_house = Some(houses[1]);
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        123,
        "8997",
        true
    ));
    assert_eq!(
        sim.substrate.entities.get(ids[1]).unwrap().owner(),
        houses[0]
    );
    assert!(!run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        124,
        "1",
        false
    ));
    assert!(!run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        124,
        "-1",
        true
    ));
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn house_directive_target_kind_reaches_first_own_team_threat_scan_and_real_super_launch() {
    let (mut sim, rules, registry, houses) = scene();
    let leader = sim.spawn_object("TANK", "H0", 15, 15, 0, &rules).unwrap();
    let enemy = sim.spawn_object("TANK", "H1", 17, 15, 0, &rules).unwrap();
    let other = sim.spawn_object("TANK", "H2", 16, 15, 0, &rules).unwrap();
    let script = sim.interner.intern("EMPTY");
    sim.team_script_vm
        .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
            id: script,
            actions: Vec::new(),
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
        });
    sim.team_script_vm
        .create_team(houses[2], script, vec![other], 0);
    let team = sim
        .team_script_vm
        .create_team(houses[0], script, vec![leader], 0);
    sim.houses.get_mut(&houses[0]).unwrap().enemy_house = Some(houses[1]);
    assert!(run(
        &mut sim,
        &rules,
        &registry,
        Some(houses[0]),
        35,
        "5",
        false
    ));
    let sw =
        crate::sim::superweapon::chronosphere_tests::charge_super(&mut sim, houses[0], "REVEAL");
    crate::sim::superweapon::ai_fire::try_fire(
        &mut sim,
        &rules,
        houses[0],
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(sim.sound_events.iter().any(|e| matches!(e,crate::sim::world::SimSoundEvent::SuperWeaponLaunched { owner,sw_type,rx:17,ry:15 } if *owner==houses[0] && *sw_type==sw)), "the closer third House is excluded by native arg3=true");
    assert!(
        !crate::sim::superweapon::superweapon_views_for_owner(&sim, &rules, &houses[0])
            .iter()
            .any(|view| view.type_id == sw && view.is_ready)
    );
    assert_eq!(
        sim.substrate.entities.get(enemy).unwrap().owner(),
        houses[1]
    );
    // A scripted cell wins over the quarry and bypasses the ground +2 offset.
    sim.sound_events.clear();
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .set_preferred_super_weapon_target((16, 15));
    crate::sim::superweapon::chronosphere_tests::charge_super(&mut sim, houses[0], "REVEAL");
    crate::sim::superweapon::ai_fire::try_fire(
        &mut sim,
        &rules,
        houses[0],
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(sim.sound_events.iter().any(|e| matches!(
        e,
        crate::sim::world::SimSoundEvent::SuperWeaponLaunched { rx: 16, ry: 15, .. }
    )));
    sim.houses
        .get_mut(&houses[0])
        .unwrap()
        .clear_preferred_super_weapon_target();
    sim.destroy_team(team, &rules, FrameEffects::default());
    sim.sound_events.clear();
    crate::sim::superweapon::chronosphere_tests::charge_super(&mut sim, houses[0], "REVEAL");
    let rng = sim.rng_state();
    crate::sim::superweapon::ai_fire::try_fire(
        &mut sim,
        &rules,
        houses[0],
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(
        crate::sim::superweapon::superweapon_views_for_owner(&sim, &rules, &houses[0])
            .iter()
            .any(|view| view.type_id == sw && view.is_ready)
    );
    assert!(!sim.sound_events.iter().any(|e| matches!(
        e,
        crate::sim::world::SimSoundEvent::SuperWeaponLaunched { .. }
    )));
    assert_eq!(sim.rng_state(), rng);
}
