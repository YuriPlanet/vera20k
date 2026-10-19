//! Rust regressions for Script60/CaptureManager -> Eaten -> Foot arrival -> UnInit.
//! Expected control flow comes from4DFA70,6E9DF9,4D4CB0,51973C; numeric
//! distance/refund/RNG kernels retain their existing native comparisons.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::combat::TargetKind;
use crate::sim::mission::MissionDispatchTimer;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::movement::PerCellReason;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};

fn fixture() -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[General]\nRefundPercent=50%\n\
         [Countries]\n0=Russians\n1=Americans\n\
         [InfantryTypes]\n0=CIV\n[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=GRINDER\n1=OTHER\n\
         [Animations]\n0=ACTIVE\n1=GRIND\n\
         [CIV]\nImage=GI\nStrength=100\nCost=100\nSoylent=75\nSpeed=4\n\
         VoiceDie=VoiceA,VoiceB\nDieSound=DieA,DieB\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [TANK]\nStrength=300\nCost=600\nSoylent=300\nSpeed=6\nSpeedType=Track\nPassengers=5\nSizeLimit=15\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [GRINDER]\nStrength=1000\nGrinding=yes\n\
         [OTHER]\nStrength=1000\n\
         [Eaten]\nRate=.016\n\
         [AudioVisual]\nEnterGrinderSound=GrinderGrinding\n",
    );
    let art = IniFile::from_str(&format!(
        "{}\n[GRINDER]\nFoundation=1x1\nActiveAnim=ACTIVE\nSpecialAnim=GRIND\n\
         [OTHER]\nFoundation=1x1\n[ACTIVE]\nLoopCount=-1\nRate=1\n\
         [GRIND]\nLoopCount=1\nRate=1\n",
        crate::rules::retail_ini_fixture::GI_ART_EXCERPT,
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut art = ArtRegistry::from_ini(&art);
    art.bind_anim_frame_count_for_test("ACTIVE", 8);
    art.bind_anim_frame_count_for_test("GRIND", 8);
    rules.install_art_data(art);
    rules.bind_type_sound_references(
        &ini,
        &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=GrinderGrinding\n1=VoiceA\n2=VoiceB\n3=DieA\n4=DieB\n",
        )),
    );
    let sim = crate::sim::arena_fixture::rival_arena(&rules, 60);
    (sim, rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, x: u16, y: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, x, y, 0, 0, rules)
        .unwrap()
}

fn team(sim: &mut Simulation, members: Vec<u64>) -> u64 {
    let script = sim.interner.intern("Grind");
    let owner = sim.interner.intern("Russians");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: vec![TeamScriptAction {
            action_id: 60,
            argument: 0,
        }],
    });
    sim.team_script_vm.create_team(owner, script, members, 0)
}

fn mission(sim: &mut Simulation, id: u64, current: MissionType, queued: MissionId) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(current),
            queued,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
}

#[test]
fn script60_uses_only_own_grinders_and_releases_members_only_on_success() {
    let (mut sim, rules) = fixture();
    spawn(&mut sim, &rules, "GRINDER", "Americans", 14, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    let team = team(&mut sim, vec![civ]);
    sim.run_team_ai_pass(&rules, None, FrameEffects::default());
    let state = sim.team_script_vm.team(team).unwrap();
    assert_eq!(state.members().collect::<Vec<_>>(), [civ]);
    assert!(state.advance_pending());

    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 12);
    let team = self::team(&mut sim, vec![civ]);
    sim.run_team_ai_pass(&rules, None, FrameEffects::default());
    assert_eq!(sim.team_script_vm.team(team).unwrap().member_count(), 0);
    assert_eq!(sim.team_script_vm.team_for_member(civ), None);
    let foot = sim.substrate.entities.get(civ).unwrap();
    assert_eq!(
        foot.navigation.nav_com(),
        Some(NavTargetRef::object(grinder))
    );
    assert_eq!(foot.mission.queued().known(), Some(MissionType::Eaten));
    assert!(!foot.building_entry.seeking());
}

#[test]
fn grinder_seek_keeps_the_last_registered_building_on_distance_ties() {
    let (mut sim, rules) = fixture();
    let first = spawn(&mut sim, &rules, "GRINDER", "Russians", 10, 10);
    let last = spawn(&mut sim, &rules, "GRINDER", "Russians", 20, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 15, 10);
    assert!(sim.foot_enter_grinder(civ, &rules, None, FrameEffects::default()));
    assert_eq!(
        sim.substrate
            .entities
            .get(civ)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::object(last))
    );
    sim.uninit_with_context(
        last,
        crate::sim::world::UninitContext::new(Some(&rules), None),
    );
    assert!(sim.foot_enter_grinder(civ, &rules, None, FrameEffects::default()));
    assert_eq!(
        sim.substrate
            .entities
            .get(civ)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::object(first))
    );
}

#[test]
fn vehicle_eaten_walk_reaches_the_grinder_and_refunds_once() {
    let (mut sim, rules) = fixture();
    let owner = sim.interner.get("Russians").unwrap();
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Russians", 10, 10);
    let credits = sim.houses[&owner].economy.credits();
    assert!(sim.foot_enter_grinder(tank, &rules, None, FrameEffects::default()));
    for _ in 0..500 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        if sim.entities().get(tank).is_none() {
            break;
        }
    }
    assert!(
        sim.entities().get(tank).is_none(),
        "Drive/Eaten must complete arrival and deferred removal"
    );
    assert_eq!(sim.houses[&owner].economy.credits(), credits + 300);
    assert!(
        sim.entities()
            .get(grinder)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    for _ in 0..10 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert_eq!(sim.houses[&owner].economy.credits(), credits + 300);
}

#[test]
fn vehicle_grinding_refunds_both_cargo_levels_and_removes_each_passenger() {
    let (mut sim, rules) = fixture();
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Americans", 14, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Russians", 10, 10);
    let outer = spawn(&mut sim, &rules, "TANK", "Russians", 11, 10);
    let nested = spawn(&mut sim, &rules, "CIV", "Russians", 12, 10);
    let sibling = spawn(&mut sim, &rules, "CIV", "Russians", 13, 10);
    let context = crate::sim::world::UninitContext::new(Some(&rules), None);
    assert!(sim.add_cargo_passenger_chain(outer, &[(nested, 1.0)], context));
    assert!(sim.add_cargo_passenger_chain(tank, &[(sibling, 1.0)], context));
    assert!(sim.add_cargo_passenger_chain(tank, &[(outer, 1.0)], context));
    sim.substrate.occupancy.add(
        10,
        10,
        grinder,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::AppendBuilding,
    );
    mission(&mut sim, tank, MissionType::Eaten, MissionId::NONE);
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::object(grinder)));
    let beneficiary = sim.interner.get("Americans").unwrap();
    let credits = sim.houses[&beneficiary].economy.credits();
    sim.per_cell_process(
        tank,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    // Unit73A0A5 first refunds itself, then nested75 + outer300 + sibling75.
    assert_eq!(sim.houses[&beneficiary].economy.credits(), credits + 750);
    for id in [tank, outer, nested, sibling] {
        let victim = sim.entities().get(id).unwrap();
        assert!(!victim.lifecycle.object_alive(), "{id} must be UnInit");
        assert!(
            victim
                .passenger_role
                .cargo()
                .is_none_or(|cargo| cargo.is_empty())
        );
    }
}

#[test]
fn eaten_arrival_uses_effective_mission_and_attack_or_navigation_identity() {
    for (current, queued, target_matches, consumed) in [
        (
            MissionType::Guard,
            MissionId::from_known(MissionType::Eaten),
            true,
            false,
        ),
        (
            MissionType::Eaten,
            MissionId::from_known(MissionType::Guard),
            true,
            true,
        ),
        (
            MissionType::None,
            MissionId::from_known(MissionType::Eaten),
            true,
            true,
        ),
        (MissionType::Eaten, MissionId::NONE, false, false),
        (MissionType::Eaten, MissionId::NONE, true, true),
    ] {
        let (mut sim, rules) = fixture();
        let building = spawn(&mut sim, &rules, "OTHER", "Americans", 14, 10);
        let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
        // Explicit arrival prior, without running Walk: match the native
        //ground list while leaving NavCom NULL to test the TarCom alternative.
        sim.substrate.occupancy.add(
            10,
            10,
            building,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        mission(&mut sim, civ, current, queued);
        if target_matches {
            sim.substrate
                .entities
                .get_mut(civ)
                .unwrap()
                .attack_target
                .set_for_test(Some(TargetKind::Entity(building)));
        }
        let owner = sim.interner.get("Americans").unwrap();
        let before = sim.houses[&owner].economy.credits();
        assert_eq!(
            sim.foot_per_cell_eaten(civ, &rules, None, FrameEffects::default()),
            consumed
        );
        assert_eq!(
            sim.houses[&owner].economy.credits() - before,
            if consumed { 75 } else { 0 }
        );
        if consumed {
            assert!(
                !sim.substrate
                    .entities
                    .get(civ)
                    .unwrap()
                    .lifecycle
                    .object_alive()
            );
            assert!(
                !sim.substrate
                    .occupancy
                    .get(10, 10)
                    .unwrap()
                    .iter_layer(MovementLayer::Ground)
                    .any(|member| member.entity_id == civ)
            );
        }
    }
}

#[test]
fn script60_walks_to_the_grinder_refunds_once_and_resumes_after_save() {
    let (mut sim, rules) = fixture();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    let owner = sim.interner.get("Russians").unwrap();
    let credits = sim.houses[&owner].economy.credits();
    team(&mut sim, vec![civ]);
    sim.run_team_ai_pass(&rules, None, FrameEffects::default());
    for _ in 0..12 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    let moved = sim.substrate.entities.get(civ).unwrap().position.rx();
    assert!(
        moved > 10 && moved < 18,
        "the production Walk must approach before saving: {moved}"
    );
    let bytes = GameSnapshot::save_validated(&sim, 0, 0, "Grind approach", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let mut completed = false;
    for _ in 0..500 {
        let a = sim.advance_tick(&[], Some(&rules), None, None, 67);
        let b = restored.advance_tick(&[], Some(&rules), None, None, 67);
        assert_eq!(a.state_hash, b.state_hash);
        if sim.substrate.entities.get(civ).is_none() {
            completed = true;
            break;
        }
    }
    assert!(
        completed,
        "ordinary Walk must reach PerCell and deferred deletion"
    );
    assert!(restored.substrate.entities.get(civ).is_none());
    assert_eq!(sim.houses[&owner].economy.credits(), credits + 75);
    for _ in 0..30 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert_eq!(sim.houses[&owner].economy.credits(), credits + 75);
    assert!(
        sim.substrate
            .entities
            .get(grinder)
            .unwrap()
            .lifecycle
            .object_alive()
    );
}

#[test]
fn sov03_authored_trigger80_teams_walk_and_recycle_all_four_civilian_types() {
    use crate::map::map_file::MapFile;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::house_state::HouseState;

    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let authored = MapFile::from_bytes(assets.get_ref("SOV03UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("SOV03UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "SOV03UMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, _, art, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &authored.ini,
        )
        .unwrap()
        .into_parts();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let aimd = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI").unwrap();
    let teams = TeamAiIniRegistry::from_sources(&aimd.ini, &authored.ini, true);
    assert!(rules.object("YAGRND").unwrap().grinding);
    assert_eq!(
        rules.object("YAGRND").unwrap().power,
        0,
        "SOV03 map override"
    );
    assert_eq!(
        rules.general.enter_grinder_sound.as_deref(),
        Some("GrinderGrinding")
    );

    for (tag_id, first_trigger, team_type, type_id) in [
        ("07A648CC", "07A64A1C", "0B0D66AC", "CIV1"),
        ("08EF49DC", "08EF363C", "0B0D61FC", "CIV2"),
        ("043F233C", "043F241C", "0B0D7DEC", "CIV3"),
        ("043F2B8C", "043F2C6C", "0B0D7BAC", "CIVSF"),
    ] {
        assert_eq!(rules.object(type_id).unwrap().soylent, 100);
        let tag = &authored.tags[tag_id];
        let mut chain = Vec::new();
        let mut next = tag.trigger();
        while let Some(id) = next {
            chain.push(id);
            next = authored.triggers[id].linked_trigger_id();
        }
        assert_eq!(chain.len(), 7);
        assert_eq!(chain.last().copied(), Some(first_trigger));
        let mut records = String::new();
        for section in ["Triggers", "Events", "Actions"] {
            records.push_str(&format!("[{section}]\n"));
            for id in &chain {
                let value = authored
                    .ini
                    .section(section)
                    .unwrap()
                    .get_for_test(id)
                    .unwrap();
                records.push_str(&format!("{id}={value}\n"));
            }
        }
        let waypoints: String = (26..=32)
            .map(|index| format!("{index}={}\n", (index - 18) * 1000 + 10))
            .collect();
        // Original trigger/event/action/Tag records and Team definitions;
        //the declared flat crop relocates all seven AA..AG reinforcement cells.
        let cropped = MapFile::from_bytes(
            format!(
                "[Map]\nSize=0,0,32,32\nLocalSize=0,0,32,32\nTheater=NEWURBAN\n\
             [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
             {records}[Tags]\n{tag_id}={}\n[Waypoints]\n{waypoints}",
                authored
                    .ini
                    .section("Tags")
                    .unwrap()
                    .get_for_test(tag_id)
                    .unwrap(),
            )
            .as_bytes(),
        )
        .unwrap();
        let mut sim = Simulation::with_seed(60);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let house = sim.interner.intern("Civie3");
        let country = sim.interner.intern("Civie3Country");
        let mut state = HouseState::new(house, 2, Some(country), false, 0, 1);
        state.project_country_mults(&rules, &sim.interner);
        sim.houses.insert(house, state);
        sim.session.house_order.push(house);
        sim.install_team_ai_registry(&teams, &rules).unwrap();
        sim.session.install_waypoints(&cropped.waypoints);
        spawn(&mut sim, &rules, "YAGRND", "Civie3", 18, 10);
        sim.initialize_trigger_definitions(&cropped, &teams, None, None);
        sim.classify_map_triggers(Some(&rules));
        let frame = |sim: &mut Simulation| {
            sim.advance_app_frame(
                &[],
                Some(&rules),
                None,
                67,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        };
        frame(&mut sim);
        assert_eq!(sim.team_script_vm.teams_in_order().count(), 0);
        // Reach the authored elapsed-time threshold without simulating an
        //unrelated campaign. The real Event13 timer and Action80 run here.
        sim.session.tick = 1600;
        sim.session.binary_frame = 1600;
        frame(&mut sim);
        let members: Vec<_> = sim
            .substrate
            .entities
            .iter_sorted()
            .filter(|(_, e)| e.category() == crate::map::entities::EntityCategory::Infantry)
            .map(|(id, e)| {
                assert_eq!(sim.interner.resolve(e.type_ref()), type_id);
                assert_eq!(e.owner(), house);
                id
            })
            .collect();
        assert_eq!(
            members.len(),
            7,
            "{tag_id} must run all seven actual Action80/{team_type} records"
        );
        let before = sim.houses[&house].economy.credits();
        let mut approached = std::collections::BTreeSet::new();
        for _ in 0..600 {
            frame(&mut sim);
            for &civilian in &members {
                if sim
                    .substrate
                    .entities
                    .get(civilian)
                    .is_some_and(|actor| actor.position.rx() > 10)
                {
                    approached.insert(civilian);
                }
            }
            if members
                .iter()
                .all(|&id| sim.substrate.entities.get(id).is_none())
            {
                break;
            }
        }
        assert_eq!(approached.len(), 7, "every {type_id} must actually walk");
        assert!(
            members
                .iter()
                .all(|&id| sim.substrate.entities.get(id).is_none()),
            "all seven {type_id} must be recycled"
        );
        assert_eq!(
            sim.houses[&house].economy.credits(),
            before + 700,
            "{type_id} Soylent"
        );
        assert!(
            members
                .iter()
                .all(|&id| sim.team_script_vm.team_for_member(id).is_none())
        );
        assert_eq!(
            sim.team_script_vm.teams_in_order().count(),
            0,
            "the empty Script60 team retires"
        );
    }
}

#[test]
fn grinder_sound_retains_valid_references_across_native_rules_passes() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=First\n1=Second\n",
    ));
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\n[AudioVisual]\nEnterGrinderSound=First\n",
    ));
    for (pass, expected) in [
        ("[AudioVisual]\nEnterGrinderSound=\n", "First"),
        ("[AudioVisual]\nEnterGrinderSound=Unknown\n", "First"),
        ("[AudioVisual]\nEnterGrinderSound=Second\n", "Second"),
        ("[AudioVisual]\nentergrindersound=First\n", "Second"),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(pass));
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(rules.general.enter_grinder_sound.as_deref(), Some(expected));
    }
    assert_eq!(
        crate::rules::ruleset::GeneralRules::default().enter_grinder_sound,
        None
    );
}

#[test]
fn infantry_arrival_switches_active_to_special_without_a_foot_tail() {
    let (mut sim, rules) = fixture();
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 14, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    sim.substrate.occupancy.add(
        10,
        10,
        grinder,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::AppendBuilding,
    );
    mission(&mut sim, civ, MissionType::Eaten, MissionId::NONE);
    sim.substrate
        .entities
        .get_mut(civ)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::object(grinder)));
    sim.set_building_anim_slot(grinder, 3, false, false, 0, &rules)
        .unwrap();
    sim.per_cell_process(
        civ,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    let slots = sim
        .substrate
        .entities
        .get(grinder)
        .unwrap()
        .building_art
        .anim_slots();
    assert!(slots[3].is_none());
    assert!(slots[10].is_some());
    assert!(
        !sim.substrate
            .entities
            .get(civ)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    assert_eq!(sim.sound_events.iter().filter(|event| matches!(event,
        crate::sim::world::SimSoundEvent::VocAt { sound_id, .. } if sound_id == "GrinderGrinding"
    )).count(), 1);
}

#[test]
fn eaten_idles_when_its_grinder_expires_without_falling_back_to_hunt() {
    let (mut sim, rules) = fixture();
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 18, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Russians", 10, 10);
    assert!(sim.foot_enter_grinder(civ, &rules, None, FrameEffects::default()));
    mission(&mut sim, civ, MissionType::Eaten, MissionId::NONE);
    sim.uninit_with_context(
        grinder,
        crate::sim::world::UninitContext::new(Some(&rules), None),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(civ)
            .unwrap()
            .navigation
            .nav_com(),
        None
    );
    crate::sim::world::dispatch_foot_mission(
        &mut sim,
        civ,
        &rules,
        crate::sim::world::ObjectAiCtx::default(),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(civ)
            .unwrap()
            .mission
            .queued()
            .known(),
        Some(MissionType::Guard)
    );
    assert!(
        sim.substrate
            .entities
            .get(civ)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    let owner = sim.interner.get("Russians").unwrap();
    assert_eq!(sim.houses[&owner].economy.credits(), 0);
}

#[test]
fn arrival_callbacks_precede_refund_and_playback_precedes_the_next_main_draw() {
    use crate::map::map_file::MapFile;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::world::{SimSoundEvent, SoundBoundary};
    use std::cell::{Cell, RefCell};

    struct Playback {
        delivered: Cell<usize>,
        sounds: RefCell<Vec<String>>,
        before_play: [crate::sim::rng::SimRngLogicalState; 3],
    }
    impl SoundBoundary for Playback {
        fn consume(&self, sim: &Simulation, _: &RuleSet, events: &[SimSoundEvent]) {
            let owner = sim.interner.get("Americans").unwrap();
            for event in &events[self.delivered.get()..] {
                let SimSoundEvent::VocAt { sound_id, .. } = event else {
                    continue;
                };
                let grinder = sound_id == "GrinderGrinding";
                assert_eq!(
                    sim.main_rng.logical_state(),
                    self.before_play[self.sounds.borrow().len()],
                    "playback must occur before selecting the next death list"
                );
                assert_eq!(
                    sim.houses[&owner].economy.credits(),
                    if grinder { 75 } else { 0 }
                );
                assert_eq!(
                    sim.global_variables()[8],
                    grinder,
                    "victim Tag48 follows refund and precedes EnterGrinderSound"
                );
                self.sounds.borrow_mut().push(sound_id.clone());
                // A reached audio consumer shares Main with list selection.
                sim.main_rng.next_u32();
            }
            self.delivered.set(events.len());
        }
        fn delivered(&self) -> usize {
            self.delivered.get()
        }
    }

    let (mut sim, rules) = fixture();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    for house in sim.houses.values_mut() {
        house.country = Some(house.name);
    }
    let grinder = spawn(&mut sim, &rules, "GRINDER", "Russians", 14, 10);
    let civ = spawn(&mut sim, &rules, "CIV", "Americans", 10, 10);
    sim.substrate.occupancy.add(
        10,
        10,
        grinder,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::AppendBuilding,
    );
    mission(&mut sim, civ, MissionType::Eaten, MissionId::NONE);
    sim.substrate
        .entities
        .get_mut(civ)
        .unwrap()
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::object(grinder)));
    let map = MapFile::from_bytes(
        b"[Map]\nSize=0,0,32,32\nLocalSize=0,0,32,32\n\
        [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
        [Triggers]\nOWNER=Americans,<none>,Owner,0,1,1,1,0\nDONE=Americans,<none>,Done,0,1,1,1,0\n\
        [Events]\nOWNER=1,1,0,1\nDONE=1,48,0,0\n\
        [Actions]\nOWNER=1,14,0,8997,0,0,0,0,A\nDONE=1,28,0,8,0,0,0,0,A\n\
        [Tags]\nOWNER_TAG=0,Owner,OWNER\nDONE_TAG=0,Done,DONE\n",
    )
    .unwrap();
    let teams = TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &map.ini, false);
    sim.initialize_trigger_definitions(&map, &teams, None, None);
    sim.classify_map_triggers(Some(&rules));
    sim.attach_object_trigger_tag(grinder, Some("OWNER_TAG"));
    sim.attach_object_trigger_tag(civ, Some("DONE_TAG"));
    sim.clear_tag_dispatch_trace_for_test();
    sim.sound_events.clear();
    sim.main_rng = crate::sim::rng::MainRng::new(60);
    let mut expected = crate::sim::rng::SimRng::new(60);
    let voice = ["VoiceA", "VoiceB"][(expected.next_u32() % 2) as usize];
    let voice_state = expected.logical_state();
    expected.next_u32();
    let die = ["DieA", "DieB"][(expected.next_u32() % 2) as usize];
    let die_state = expected.logical_state();
    expected.next_u32();
    let grinder_state = expected.logical_state();
    expected.next_u32();
    let playback = Playback {
        delivered: Cell::new(0),
        sounds: RefCell::new(Vec::new()),
        before_play: [voice_state, die_state, grinder_state],
    };
    assert!(sim.foot_per_cell_eaten(civ, &rules, None, FrameEffects::for_sound(&playback)));
    assert_eq!(*playback.sounds.borrow(), [voice, die, "GrinderGrinding"]);
    assert_eq!(sim.main_rng.logical_state(), expected.logical_state());
    let owner = sim.interner.get("Americans").unwrap();
    assert_eq!(
        sim.substrate.entities.get(grinder).unwrap().owner(),
        owner,
        "building Tag1 changes ownership before refund"
    );
    assert_eq!(sim.houses[&owner].economy.credits(), 75);
    let dispatches: Vec<_> = sim
        .tag_dispatch_trace_for_test()
        .iter()
        .copied()
        .filter(|(_, kind)| [1, 48].contains(kind))
        .collect();
    // ChangeOwner7015A8 calls RecordKill on the building while its Tag1 is
    //dispatching. Its nested Tag48 is distinct from the civilian's later48.
    assert_eq!(
        dispatches.iter().map(|(_, kind)| *kind).collect::<Vec<_>>(),
        [1, 48, 48]
    );
    assert_eq!(dispatches[0].0, dispatches[1].0);
    assert_ne!(dispatches[0].0, dispatches[2].0);
    assert!(
        !sim.substrate
            .entities
            .get(civ)
            .unwrap()
            .lifecycle
            .object_alive()
    );
}
