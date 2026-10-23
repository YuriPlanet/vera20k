use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules_for_map};
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::combat::combat_weapon::get_weapon;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::position_world_xy;
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};

#[test]
fn retail_guard_slower_center_keeps_yuri_within_stray() {
    let Some(Fixture {
        sim, team, actors, ..
    }) = Fixture::new(true, false)
    else {
        return;
    };
    let state = &sim.team_script_vm.teams[&team];
    assert_eq!(state.zone, Some(TeamTarget::Cell { x: 14, y: 10 }));
    assert_eq!(state.closest_member, Some(actors[0]));
    let yuri = sim.entities().get(actors[2]).unwrap();
    assert_eq!(yuri.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(yuri.mission.queued(), MissionId::NONE);
    assert_eq!(yuri.navigation.nav_com(), None);
    // Regroup6EB870 queues Move with commence_now=0. The existing Guard
    // stays current until the later ordinary object visit promotes the queue.
    for index in [0, 1, 3] {
        let member = sim.entities().get(actors[index]).unwrap();
        assert_eq!(member.mission.current().known(), Some(MissionType::Guard));
        assert_eq!(member.mission.queued().known(), Some(MissionType::Move));
        assert_eq!(
            member.navigation.nav_com(),
            Some(NavTargetRef::cell(14, 10))
        );
    }
}

#[test]
fn retail_guard_slower_no_orders_yuri_to_the_unweighted_center() {
    let Some(Fixture {
        sim, team, actors, ..
    }) = Fixture::new(false, false)
    else {
        return;
    };
    assert_eq!(
        sim.team_script_vm.teams[&team].zone,
        Some(TeamTarget::Cell { x: 13, y: 10 })
    );
    let yuri = sim.entities().get(actors[2]).unwrap();
    assert_eq!(yuri.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(yuri.mission.queued().known(), Some(MissionType::Move));
    assert_eq!(yuri.navigation.nav_com(), Some(NavTargetRef::cell(13, 10)));
}

#[test]
fn retail_guard_slower_center_falls_back_when_infantry_cannot_enter_water() {
    let Some(Fixture {
        sim,
        rules,
        overlays,
        team,
        actors,
    }) = Fixture::new(true, true)
    else {
        return;
    };
    let state = &sim.team_script_vm.teams[&team];
    assert_eq!(state.closest_member, Some(actors[0]));
    assert_eq!(state.zone, Some(TeamTarget::Object(actors[0])));
    assert_ne!(
        sim.mover_can_enter(
            actors[0],
            (14, 10),
            InfantryEntryArgs::REPAIR,
            EntryQueryMode::CheckLocomotor,
            &rules,
            Some(&overlays),
        )
        .unwrap(),
        0
    );
}

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    overlays: OverlayTypeRegistry,
    team: u64,
    actors: [u64; 4],
}

impl Fixture {
    fn new(guard_slower: bool, water: bool) -> Option<Self> {
        let retail = retail_battle_rules_for_map("XMP03T4.MAP")?;
        let (_, assets) = retail_assets()?;
        let overlays =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        // Keep the stock Team/TaskForce; author Guard as the first script action
        // instead of the stock attack action, so ordinary AI reaches Regroup.
        let mut scenario = IniFile::from_str("[ScriptTypes]\n0=08A29DBC-G\n[08A29DBC-G]\n0=5,1\n");
        if !guard_slower {
            scenario.merge(&IniFile::from_str(
                "[TeamTypes]\n0=05FBBBEC-G\n[05FBBBEC-G]\nGuardSlower=no\n",
            ));
        }
        let registry = TeamAiIniRegistry::load_retail(&assets, &scenario, true).unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4DBFD0);
        if water {
            crate::sim::arena_fixture::flood(&mut sim, &rules, 32, [(14, 10)]);
        }
        let owner = sim.interner.intern("YuriCountry");
        sim.houses
            .register(HouseState::new(owner, 0, None, false, 0, 10));
        sim.install_team_ai_registry(&registry, &rules).unwrap();
        let team_type = sim.interner.get("05FBBBEC-G").unwrap();
        let definition = &sim.team_script_vm.team_types[&team_type];
        assert_eq!(
            definition.task_force_id,
            sim.interner.get("05FBBADC-G").unwrap()
        );
        assert_eq!(
            sim.team_script_vm.team_type_ini[&team_type].guard_slower,
            guard_slower
        );
        let mut actors = [0; 4];
        for (index, kind) in ["BRUTE", "BRUTE", "YURI", "VIRUS"].into_iter().enumerate() {
            let id = sim
                .spawn_object(
                    kind,
                    "YuriCountry",
                    10 + index as u16 % 2,
                    10 + index as u16 / 2,
                    0,
                    &rules,
                )
                .unwrap();
            let entity = sim.entities().get(id).unwrap();
            let object = sim.object_type(entity.type_ref(), &rules).unwrap();
            // These armed, buildable, non-deploying members reach the speed gate.
            assert!(
                get_weapon(entity, object, 0, sim.entities(), &rules, &sim.interner,).is_some()
            );
            assert_ne!(sim.object_tech_level(object), -1);
            assert!(object.deploys_into.is_none());
            assert_eq!(object.speed, if index < 2 { 6 } else { 4 });
            actors[index] = id;
        }
        // Recruit/form while clustered, so earlier formation orders cannot hide
        // the later Regroup destination. No Team flags or script cursor are set.
        sim.session.set_binary_frame_for_test(100);
        let team = sim.create_team(team_type, Some(owner), &rules).unwrap();
        for frame in 100..110 {
            sim.session.set_binary_frame_for_test(frame);
            sim.run_team_ai_pass(&rules, Some(&overlays), FrameEffects::default());
            if sim.team_script_vm.teams[&team].current_action(&sim.team_script_vm) == 5 {
                break;
            }
        }
        let state = &sim.team_script_vm.teams[&team];
        assert!(state.formed && state.has_been_full && !state.reforming);
        assert_eq!(state.slot_counts, [2, 1, 1, 0, 0, 0]);
        assert_eq!(state.cursor, 0);
        assert_eq!(state.current_action(&sim.team_script_vm), 5);
        let frame = sim.session.binary_frame() as i32;
        assert!(state.guard_timer.remaining(frame) > 0);
        // Each short-entry call searches Infantry for any still-needed type.
        // This seeded placement recruits BRUTE0, YURI2, BRUTE1, VIRUS3;
        // AddMember prepends each one, counting against its actual type slot.
        assert_eq!(
            state.members().collect::<Vec<_>>(),
            [actors[3], actors[1], actors[2], actors[0]]
        );
        for (&id, x) in actors.iter().zip([2688, 2752, 4224, 4288]) {
            assert!(
                sim.entities()
                    .get(id)
                    .unwrap()
                    .navigation
                    .nav_com()
                    .is_none()
            );
            sim.foot_set_location_marked(
                id,
                DriveCoord { x, y: 2688, z: 0 },
                &rules,
                Some(&overlays),
                FrameEffects::default(),
            );
            let entity = sim.entities().get(id).unwrap();
            assert_eq!(position_world_xy(&entity.position), [x, 2688]);
            assert!(entity.in_playfield() && !entity.lifecycle.in_limbo());
        }
        sim.session
            .set_binary_frame_for_test(sim.session.binary_frame() + 1);
        sim.run_team_ai_pass(&rules, Some(&overlays), FrameEffects::default());
        // Calc_Center6EAEE0 uses Foot4DBFD0's converted native speed, then asks
        // the actual closest member's CanEnter before retaining the mean cell.
        assert_eq!(rules.general.stray, 512);
        Some(Self {
            sim,
            rules,
            overlays,
            team,
            actors,
        })
    }
}
