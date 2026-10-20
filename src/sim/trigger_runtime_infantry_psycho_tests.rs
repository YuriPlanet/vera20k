//! Execute91 6DDAF9..6DDB5A and Infantry5220F0, Steam gamemd.
//! Instruction-established controls, exercised through production owners.
use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::intern::InternedId;

#[test]
fn attack_friendlies_uses_native_boolean_defaults_layers_and_retail_inputs() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ));
    let read = |layers: &RulesLayerStack| RuleSet::from_rules_layers(layers).unwrap();
    assert!(!read(&layers).object("E1").unwrap().attacks_friendlies());
    for (text, expected) in [
        ("[E1]\nAttackFriendlies=yes\n", true),
        ("[E1]\nStrength=75\nattackfriendlies=no\n", true),
        ("[E1]\nAttackFriendlies=\n", true),
        ("[E1]\nAttackFriendlies=no\n", false),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(text));
        assert_eq!(
            read(&layers).object("E1").unwrap().attacks_friendlies(),
            expected,
            "{text}"
        );
    }
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert!(
        retail
            .rules
            .all_objects()
            .all(|ty| !ty.attacks_friendlies()),
        "selected retail startup/mode/Hills layers leave this branch dormant"
    );
    let Some(patched) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str("[E1]\nAttackFriendlies=yes\n"),
    ) else {
        return;
    };
    assert!(patched.rules.object("E1").unwrap().attacks_friendlies());
}

#[test]
fn unmatched_go_berzerk_does_not_abort_the_following_trigger_action() {
    let (mut sim, rules, registry, houses) = fixture(false);
    sim.session.set_binary_frame_for_test(8);
    sim.raise_house_tags(
        houses[1],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(sim.trigger_runtime.globals_set.contains(&7));
}

fn fixture(
    attack_friendlies: bool,
) -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    [InternedId; 3],
) {
    let (mut sim, rules, registry, houses) = tests::event_ingress_fixture_with_rules(&format!(
        "[InfantryTypes]\n2=SHOOTER\n[SHOOTER]\nStrength=100\nSpeed=4\nSight=5\nSpeedType=Foot\nLocomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\nPrimary=RIFLE\nAttackFriendlies={}\n\
         [WeaponTypes]\n0=RIFLE\n[RIFLE]\nDamage=10\nROF=30\nRange=6\nProjectile=INVIS\nWarhead=SA\n\
         [Projectiles]\n0=INVIS\n[INVIS]\nInviso=yes\nAA=yes\nAG=yes\n",
        if attack_friendlies { "yes" } else { "no" }
    ));
    sim.session.set_current_house_for_test(Some(houses[0]));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nCONTROL=Country7,<none>,Control,0,1,1,1,0\nOTHER=Country7,<none>,Other,0,1,1,1,0\n\
         [Events]\nCONTROL=1,8,0,0\nOTHER=1,8,0,0\n\
         [Actions]\nCONTROL=2,91,0,-999,0,0,0,0,A,28,0,7,0,0,0,0,A\nOTHER=1,0,0,0,0,0,0,0,A\n\
         [Tags]\nCONTROLTAG=2,Control,CONTROL\nOTHERTAG=2,Other,OTHER\n",
    );
    (sim, rules, registry, houses)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, typ: &str, owner: &str, x: u16) -> u64 {
    sim.spawn_object(typ, owner, x, 15, 0, rules)
        .expect("formed production actor")
}

fn run(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    live: Option<usize>,
) -> bool {
    let typ = sim
        .trigger_runtime
        .definitions
        .trigger_types
        .iter()
        .position(|t| t.id == "CONTROL")
        .unwrap();
    let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: live,
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        Some(registry),
        FrameEffects::default(),
    )
}

fn psycho(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .infantry
        .as_ref()
        .is_some_and(|state| state.is_psycho())
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

#[test]
fn go_berzerk_admission_uses_live_trigger_identity_and_the_infantry_array() {
    for variant in [
        "matching",
        "wrong_tag",
        "untagged",
        "dead",
        "unmarked",
        "limbo",
        "zero_health",
        "vehicle",
        "null_live",
    ] {
        let (mut sim, rules, registry, _) = fixture(false);
        let actor = spawn(
            &mut sim,
            &rules,
            if variant == "vehicle" {
                "TAGUNIT"
            } else {
                "SHOOTER"
            },
            "House0",
            15,
        );
        if variant != "untagged" {
            sim.attach_object_trigger_tag(
                actor,
                Some(if variant == "wrong_tag" {
                    "OTHERTAG"
                } else {
                    "CONTROLTAG"
                }),
            );
        }
        let control = sim
            .trigger_runtime
            .definitions
            .tag_type("CONTROLTAG")
            .unwrap();
        let tag = sim.shared_trigger_tag(control);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let entity = sim.substrate.entities.get_mut(actor).unwrap();
        match variant {
            "dead" => entity.lifecycle.set_object_alive_for_test(false),
            "unmarked" => entity.lifecycle.set_cell_marked_for_test(false),
            "limbo" => entity.lifecycle.set_in_limbo_for_test(true),
            "zero_health" => entity.health.set_for_test(0),
            _ => {}
        }
        let mission = entity.mission.current();
        let before = rng(&sim);
        let hash = sim.state_hash();
        let admitted = matches!(variant, "matching" | "zero_health");
        assert_eq!(
            run(
                &mut sim,
                &rules,
                &registry,
                (variant != "null_live").then_some(live)
            ),
            admitted,
            "{variant}"
        );
        assert_eq!(psycho(&sim, actor), admitted, "{variant}");
        assert_eq!(
            sim.substrate.entities.get(actor).unwrap().mission.current(),
            mission
        );
        assert!(
            !sim.substrate
                .entities
                .get(actor)
                .unwrap()
                .berserk
                .is_active()
        );
        assert_eq!(rng(&sim), before);
        assert_eq!(
            sim.state_hash() != hash,
            admitted,
            "{variant}: retained flag is hashed"
        );
        if admitted {
            let once = sim.state_hash();
            assert!(run(&mut sim, &rules, &registry, Some(live)));
            assert_eq!(
                sim.state_hash(),
                once,
                "already-set remains a successful no-op"
            );
        }
    }
}

#[test]
fn ordered_go_berzerk_survives_snapshot_tag_removal_and_reconstruction_starts_clear() {
    let (mut sim, rules, registry, houses) = fixture(false);
    let actor = spawn(&mut sim, &rules, "SHOOTER", "House0", 15);
    sim.attach_object_trigger_tag(actor, Some("CONTROLTAG"));
    let before = rng(&sim);
    sim.session.set_binary_frame_for_test(8);
    sim.raise_house_tags(
        houses[1],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    assert!(psycho(&sim, actor));
    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(rng(&sim), before);
    sim.attach_object_trigger_tag(actor, None);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Psycho", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    assert!(psycho(&loaded, actor));
    assert!(
        !loaded
            .substrate
            .entities
            .get(actor)
            .unwrap()
            .berserk
            .is_active()
    );
    loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    loaded.resolve_type_handles(&rules);
    loaded.uninit_with_context(
        actor,
        crate::sim::world::UninitContext::new(Some(&rules), Some(&registry)),
    );
    loaded.flush_pending_delete();
    let fresh = spawn(&mut loaded, &rules, "SHOOTER", "House0", 18);
    assert!(!psycho(&loaded, fresh));
}

#[test]
fn psycho_stops_the_cell_on_an_ally_while_hunt_still_sees_the_enemy_behind_it() {
    use crate::sim::combat::{ScanMission, combat_targeting};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
    let (mut sim, rules, registry, _) = fixture(false);
    let actor = spawn(&mut sim, &rules, "SHOOTER", "House0", 15);
    let enemy = spawn(&mut sim, &rules, "SHOOTER", "House1", 17);
    let ally = spawn(&mut sim, &rules, "SHOOTER", "House0", 19);
    // Supply the same-cell list boundary, not an overlapping movement
    // admission claim. Both native scans consume this retained ordering.
    sim.substrate
        .entities
        .get_mut(ally)
        .unwrap()
        .position
        .put_location(crate::sim::components::DriveCoord::cell(17, 15, 0));
    let mut cells = OccupancyGrid::new();
    for id in [enemy, ally] {
        cells.add(
            17,
            15,
            id,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
    }
    let scan = |sim: &Simulation, mask| {
        combat_targeting::greatest_threat_for_entity(
            &sim.substrate.entities,
            &cells,
            &rules,
            &sim.interner,
            actor,
            None,
            None,
            false,
            mask,
            None,
            crate::sim::combat::line_of_fire::LineOfFireInputs::terrain_only(),
            None,
            None,
        )
        .target()
    };
    assert_eq!(scan(&sim, ScanMission::Guard), Some(enemy));
    sim.attach_object_trigger_tag(actor, Some("CONTROLTAG"));
    let typ = sim
        .trigger_runtime
        .definitions
        .tag_type("CONTROLTAG")
        .unwrap();
    let tag = sim.shared_trigger_tag(typ);
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    assert!(run(&mut sim, &rules, &registry, Some(live)));
    assert_eq!(
        scan(&sim, ScanMission::Guard),
        None,
        "native cell stopping consumes the ally, then the offer rejects it"
    );
    assert_eq!(
        scan(&sim, ScanMission::Hunt),
        Some(enemy),
        "the flat walk has no Psycho ally-offer override"
    );
}

#[test]
fn attack_friendlies_is_consumed_by_the_shared_ground_and_flat_scans() {
    use crate::sim::combat::{ScanMission, combat_targeting};
    for enabled in [false, true] {
        let (mut sim, rules, _, _) = fixture(enabled);
        let actor = spawn(&mut sim, &rules, "SHOOTER", "House0", 15);
        let ally = spawn(&mut sim, &rules, "SHOOTER", "House0", 17);
        assert_eq!(
            rules.object("SHOOTER").unwrap().attacks_friendlies(),
            enabled
        );
        let cells = crate::sim::occupancy::OccupancyGrid::rebuild(&sim.substrate.entities);
        for mask in [ScanMission::Guard, ScanMission::Hunt] {
            let result = combat_targeting::greatest_threat_for_entity(
                &sim.substrate.entities,
                &cells,
                &rules,
                &sim.interner,
                actor,
                None,
                None,
                false,
                mask,
                None,
                crate::sim::combat::line_of_fire::LineOfFireInputs::terrain_only(),
                None,
                None,
            )
            .target();
            assert_eq!(result, enabled.then_some(ally), "enabled{enabled} {mask:?}");
        }
    }
}
