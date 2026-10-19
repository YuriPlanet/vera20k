//! Infantry death progress through the production load preparation route.
use super::*;
use crate::sim::world::InfantryDeathSequence;

#[test]
fn infantry_fatal_frame_exit_preserves_delivered_cleanup_through_load() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{LifecycleTestEvent, SimSoundEvent, TickLane};
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=SHOOTER\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=100\nSpeed=4\n\
         [SHOOTER]\nStrength=300\nSpeed=6\nSight=8\nPrimary=Gun\n\
         [Gun]\nDamage=1000\nROF=50\nRange=10\nProjectile=INVIS\nWarhead=KILL\n\
         [INVIS]\nInviso=yes\n[Warheads]\n0=KILL\n\
         [KILL]\nInfDeath=3\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\nInfantryExplode=DEATHTEST\n",
    )).unwrap();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[DEATHTEST]\nReport=DeathReport\nEnd=80\n",
    ));
    art.bind_anim_frame_count_for_test("DEATHTEST", 80);
    rules.replace_art_registry_for_test(art);
    let (mut saved, terrain) = death_load_world(&rules);
    saved.input_delay_ticks = 0;
    let owner = saved.interner.intern("Americans");
    saved
        .houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1000, 10));
    saved.session.house_order.push(owner);
    let shooter = saved
        .spawn_object_at_height(
            "SHOOTER",
            "Americans",
            1,
            1,
            crate::util::direction_tables::facing8_from_delta(1, 0),
            0,
            &rules,
        )
        .unwrap();
    let victim = saved
        .spawn_object_at_height("E1", "Russians", 2, 1, 0, 0, &rules)
        .unwrap();
    assert!(crate::sim::combat::install_entity_attack_target_for_test(
        &mut saved.substrate.entities,
        shooter,
        victim
    ));
    saved.clear_lifecycle_test_events_for_test();
    let registry = OverlayTypeRegistry::empty();
    let output = saved
        .advance_app_frame(
            &[CommandEnvelope::new(owner, 1, Command::ExitMatch)],
            Some(&rules),
            Some(&registry),
            67,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(!output.tick.frame_committed);
    let object = saved.substrate.entities.get(victim).unwrap();
    assert!(
        object.death.is_dying() && object.lifecycle.in_limbo() && !object.lifecycle.object_alive()
    );
    assert!(!object.in_logic_vector.is_member());
    assert!(!saved.substrate.occupancy.contains_entity(2, 1, victim));
    assert_eq!(
        saved
            .substrate
            .pending_delete
            .object_tickets()
            .filter(|&id| id == victim)
            .count(),
        1
    );
    assert_eq!(saved.lifecycle_test_events_for_test().iter().filter(|event|
        matches!(event, LifecycleTestEvent::UninitAliveCleared { stable_id } if *stable_id == victim)).count(), 1);
    assert!(
        !saved
            .lifecycle_test_events_for_test()
            .contains(&LifecycleTestEvent::PendingDeleteDrainStarted)
    );
    let death_ids = |sim: &Simulation| {
        sim.anims()
            .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "DEATHTEST")
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    };
    let effects = death_ids(&saved);
    assert_eq!(
        effects.len(),
        1,
        "ordinary consequence delivered before exit"
    );
    assert_eq!(
        output
            .sound_events
            .iter()
            .filter(|event| matches!(event,
        SimSoundEvent::AnimationStarted { sound_id, .. } if sound_id == "DEATHREPORT"))
            .count(),
        1
    );
    saved.scenario_rng = crate::sim::rng::SimRng::new(0);
    let bytes = snapshot_bytes(&saved, &rules);
    let mut restored = PreparedLoad::prepare_candidate(
        &bytes,
        Some(&saved),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(&rules),
        Some(&terrain),
        Some(&registry),
    )
    .unwrap()
    .simulation;
    assert_eq!(
        restored.substrate.pending_delete,
        saved.substrate.pending_delete
    );
    assert_eq!(death_ids(&restored), effects);
    assert_eq!(restored.state_hash(), saved.state_hash());
    let next = restored
        .advance_app_frame(
            &[],
            Some(&rules),
            Some(&registry),
            67,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    // Exit requests are transient: production load resumes the match and the
    // first committed frame drains the already-delivered victim normally.
    assert!(next.tick.frame_committed);
    assert!(!restored.substrate.entities.contains(victim));
    assert!(!restored.substrate.pending_delete.contains_object(&victim));
    assert_eq!(death_ids(&restored), effects);
    assert!(!next.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::AnimationStarted { sound_id, .. } if sound_id == "DEATHREPORT")));
}

/// A corpse saved mid-sequence keeps its action and Stage through the load
/// and reaches its sequencer's UnInit on the same visit as the saved world.
#[test]
fn infantry_prepared_load_preserves_death_sequence_progress_and_cleanup_visit() {
    for sequence in [InfantryDeathSequence::Die1, InfantryDeathSequence::Die2] {
        // An authored three-frame death goes through the same native ART
        // reader as the physical GI records, rather than a sprite-only edit.
        let art = IniFile::from_str(
            &crate::rules::retail_ini_fixture::GI_ART_EXCERPT
                .replace("Die1=134,15,0", "Die1=134,3,0")
                .replace("Die2=149,15,0", "Die2=149,3,0"),
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nImage=GI\nStrength=100\nSpeed=4\n",
            ),
            &art,
        )
        .unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules.bind_animation_sequences(
            &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
        );
        let (mut saved, terrain) = death_load_world(&rules);
        let victim = saved
            .spawn_object_at_height("E1", "Americans", 1, 1, 0, 0, &rules)
            .unwrap();
        saved
            .substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .health
            .set_for_test(0);
        saved.begin_infantry_death_sequence(
            victim,
            sequence,
            &rules,
            crate::sim::world::FrameEffects::default(),
        );
        // DoAction starts the native one-frame countdown at frame0.
        // Its frame0 Logic visit holds; frame1 performs the first step.
        saved.advance_tick(&[], Some(&rules), None, None, 100);
        assert_eq!(
            saved
                .substrate
                .entities
                .get(victim)
                .unwrap()
                .native_stage()
                .value(),
            0
        );
        saved.advance_tick(&[], Some(&rules), None, None, 100);
        let corpse = saved.substrate.entities.get(victim).unwrap();
        assert_eq!(corpse.native_stage().value(), 1);
        let doing = corpse.mission_leaf.as_infantry().unwrap().doing();
        // Native in-scenario load restarts Scenario RNG at zero. Compare the
        // death's continuation from that same cursor rather than hide reset.
        saved.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = snapshot_bytes(&saved, &rules);
        let registry = OverlayTypeRegistry::empty();
        let mut restored = PreparedLoad::prepare_candidate(
            &bytes,
            Some(&saved),
            Some(LOAD_FIXTURE_MAP_HASH),
            Some(&rules),
            Some(&terrain),
            Some(&registry),
        )
        .expect("actual validate/deserialize/restore/rebuild preparation")
        .simulation;
        let corpse = restored.substrate.entities.get(victim).unwrap();
        assert_eq!(corpse.mission_leaf.as_infantry().unwrap().doing(), doing);
        assert_eq!(corpse.native_stage().value(), 1);
        assert_eq!(corpse.health.current(), 0);
        assert!(!corpse.death.is_dying() && corpse.lifecycle.object_alive());
        for visit in 1..=2 {
            saved.advance_tick(&[], Some(&rules), None, None, 100);
            restored.advance_tick(&[], Some(&rules), None, None, 100);
            assert_eq!(
                restored.substrate.entities.contains(victim),
                visit < 2,
                "{sequence:?}, visit {visit}"
            );
            assert_eq!(
                saved.substrate.entities.contains(victim),
                restored.substrate.entities.contains(victim)
            );
            assert_eq!(saved.scenario_rng.state(), restored.scenario_rng.state());
            assert_eq!(
                saved.state_hash(),
                restored.state_hash(),
                "{sequence:?}, visit {visit}"
            );
        }
        assert!(!restored.substrate.occupancy.contains_entity(1, 1, victim));
        assert!(!restored.live_object_order_snapshot().contains(&victim));
    }
}

fn death_load_world(rules: &RuleSet) -> (Simulation, ResolvedTerrainGrid) {
    let terrain = ResolvedTerrainGrid::from_cells(
        4,
        4,
        (0..4)
            .flat_map(|ry| {
                (0..4).map(move |rx| {
                    let mut cell = compatibility_snapshot_cell(0);
                    cell.rx = rx;
                    cell.ry = ry;
                    let clear = SpeedCostProfile {
                        foot: Some(100),
                        track: Some(100),
                        wheel: Some(100),
                        hover: Some(100),
                        amphibious: Some(100),
                        ..SpeedCostProfile::default()
                    };
                    cell.speed_costs = clear;
                    cell.base_speed_costs = clear;
                    cell
                })
            })
            .collect(),
    );
    let mut saved = load_fixture_simulation(true);
    saved.overlay_grid = Some(OverlayGrid::new(4, 4));
    saved.install_resolved_terrain_for_new_map(terrain.clone());
    saved.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -100,
        off_100: -100,
        off_104: 200,
        off_108: 200,
    });
    saved.intern_rule_type_ids(rules);
    saved.resolve_type_handles(rules);
    assert!(saved.rebuild_dynamic_navigation(rules));
    (saved, terrain)
}
