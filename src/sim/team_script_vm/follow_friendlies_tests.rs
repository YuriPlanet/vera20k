//! Asset-independent regression scenes for action 10. These are not native
//! execution goldens or a retail playtest.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::DriveCoord;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation};

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    member: u64,
    team: u64,
}

impl Fixture {
    fn new() -> Self {
        let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=TANK\n1=UNARMED\n\
             [InfantryTypes]\n0=SOLDIER\n[BuildingTypes]\n0=PLANT\n\
             [TANK]\nStrength=200\nSpeed=6\nROT=5\nPrimary=GUN\n\
             Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
             [UNARMED]\nStrength=200\nSpeed=6\nROT=5\n\
             Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
             [SOLDIER]\nStrength=100\nSpeed=4\nPrimary=GUN\n\
             Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
             [PLANT]\nStrength=500\n\
             [GUN]\nDamage=10\nROF=20\nRange=6\nWarhead=SA\n\
             [SA]\nVerses=100%\n",
        ))
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6EDB50);
        let member = sim
            .spawn_object("TANK", "Russians", 4, 4, 0, &rules)
            .unwrap();
        let owner = sim.interner.intern("Russians");
        let script = sim.interner.intern("FOLLOW");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![
                TeamScriptAction {
                    action_id: 10,
                    argument: 0,
                },
                TeamScriptAction {
                    action_id: 2,
                    argument: 0,
                },
            ],
        });
        let team = sim
            .team_script_vm
            .create_team(owner, script, vec![member], 0);
        Self {
            sim,
            rules,
            member,
            team,
        }
    }

    fn spawn(&mut self, name: &str, owner: &str, x: u16, y: u16) -> u64 {
        self.sim
            .spawn_object(name, owner, x, y, 0, &self.rules)
            .unwrap()
    }

    fn step(&mut self) {
        self.sim
            .team_step_script(self.team, &self.rules, None, FrameEffects::default());
    }

    fn center(&mut self) -> Option<TeamTarget> {
        self.sim.team_calc_center(self.team, &self.rules, None);
        self.sim.team_script_vm.team(self.team).unwrap().zone
    }

    fn put(&mut self, id: u64, delta: [i32; 3]) {
        let head = self
            .sim
            .team_target_coord(TeamTarget::Object(self.member))
            .unwrap();
        self.sim
            .substrate
            .entities
            .get_mut(id)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: head.x + delta[0],
                y: head.y + delta[1],
                z: head.z + delta[2],
            });
    }

    fn target(&mut self, id: u64, target: Option<TargetKind>) {
        self.sim
            .assign_target_represented(id, target, Some(&self.rules), None, FrameEffects::default())
            .unwrap();
    }
}

#[test]
fn follow_friendlies_dispatch_moves_to_friendly_without_replacing_mission_target() {
    let mut f = Fixture::new();
    let friendly = f.spawn("TANK", "Russians", 18, 4);
    let old = Some(TeamTarget::Cell { x: 25, y: 25 });
    f.sim
        .team_script_vm
        .teams
        .get_mut(&f.team)
        .unwrap()
        .mission_target = old;
    f.step();
    let team = f.sim.team_script_vm.team(f.team).unwrap();
    assert_eq!(team.refusal(), None);
    assert_eq!(team.zone, Some(TeamTarget::Object(friendly)));
    assert_eq!(team.focus, team.zone);
    assert_eq!(team.closest_member, Some(f.member));
    assert_eq!(team.mission_target, old);
    assert!(!team.advance_pending());
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.member)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::Object { id: friendly })
    );
}

#[test]
fn follow_friendlies_filters_lifecycle_and_non_foot_classes_in_both_candidate_arrays() {
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;
    for name in ["TANK", "SOLDIER"] {
        let mut f = Fixture::new();
        for (index, flag) in ["inactive", "dead", "limbo", "own_team"]
            .into_iter()
            .enumerate()
        {
            let id = f.spawn(name, "Russians", 6 + index as u16, 4);
            let entity = f.sim.substrate.entities.get_mut(id).unwrap();
            match flag {
                "inactive" => entity.lifecycle.set_object_alive_for_test(false),
                "dead" => entity.health.set_for_test(0),
                "limbo" => entity.lifecycle.set_in_limbo_for_test(true),
                _ => f.sim.team_script_vm.link_member(
                    f.team,
                    TeamMember {
                        id,
                        initiated: true,
                    },
                ),
            }
        }
        let owner = f.sim.interner.intern("Russians");
        for (id, class) in [
            (100, EntityCategory::Structure),
            (101, EntityCategory::Aircraft),
        ] {
            let mut entity =
                GameEntity::test_default_of_category(id, "OTHER", "Russians", 5, 4, class);
            entity.set_owner_for_test(owner);
            f.sim.substrate.entities.insert(entity);
        }
        let eligible = f.spawn(name, "Russians", 18, 4);
        assert_eq!(f.center(), Some(TeamTarget::Object(eligible)), "{name}");
    }
}

#[test]
fn follow_friendlies_alliance_is_asked_by_candidate_not_team_owner() {
    let mut f = Fixture::new();
    let other = f.spawn("TANK", "Americans", 8, 4);
    f.sim
        .house_alliances
        .insert("RUSSIANS".into(), ["AMERICANS".into()].into());
    assert_eq!(
        f.center(),
        None,
        "team owner's one-way declaration is not enough"
    );
    f.sim.house_alliances.clear();
    f.sim
        .house_alliances
        .insert("AMERICANS".into(), ["RUSSIANS".into()].into());
    assert_eq!(f.center(), Some(TeamTarget::Object(other)));
}

#[test]
fn follow_friendlies_unit_array_wins_ties_then_stable_order_within_class() {
    let mut f = Fixture::new();
    let infantry = f.spawn("SOLDIER", "Russians", 12, 4);
    let first_unit = f.spawn("TANK", "Russians", 13, 4);
    let second_unit = f.spawn("TANK", "Russians", 14, 4);
    for id in [infantry, first_unit, second_unit] {
        f.put(id, [1024, 0, 0]);
    }
    assert_eq!(f.center(), Some(TeamTarget::Object(first_unit)));
    f.sim
        .substrate
        .entities
        .get_mut(first_unit)
        .unwrap()
        .health
        .set_for_test(0);
    assert_eq!(f.center(), Some(TeamTarget::Object(second_unit)));
}

#[test]
fn follow_friendlies_full_z_distance_can_choose_farther_projected_infantry() {
    let mut f = Fixture::new();
    let high = f.spawn("TANK", "Russians", 8, 4);
    let ground = f.spawn("SOLDIER", "Russians", 12, 4);
    f.put(high, [256, 0, 2048]);
    f.put(ground, [1024, 0, 0]);
    assert_eq!(f.center(), Some(TeamTarget::Object(ground)));
}

#[test]
fn follow_friendlies_compares_truncated_length_not_exact_squared_distance() {
    let mut f = Fixture::new();
    let first = f.spawn("TANK", "Russians", 12, 4);
    let closer = f.spawn("TANK", "Russians", 13, 4);
    f.put(first, [1024, 1, 0]);
    f.put(closer, [1024, 0, 0]);
    assert_eq!(f.center(), Some(TeamTarget::Object(first)));
}

#[test]
fn follow_friendlies_arrival_uses_shared_completion_and_next_step_restores_mean_center() {
    for formed in [false, true] {
        let mut f = Fixture::new();
        let friendly = f.spawn("TANK", "Russians", 5, 4);
        f.put(friendly, [128, 0, 0]);
        f.sim.team_script_vm.teams.get_mut(&f.team).unwrap().formed = formed;
        f.step();
        assert_eq!(
            f.sim.team_script_vm.team(f.team).unwrap().advance_pending(),
            formed
        );
        if formed {
            f.step();
            assert_eq!(f.sim.team_script_vm.team(f.team).unwrap().cursor(), 1);
            assert_eq!(f.center(), Some(TeamTarget::Cell { x: 4, y: 4 }));
        }
    }
}

#[test]
fn follow_friendlies_missing_candidate_keeps_coordinate_move_mission_fallback() {
    let mut f = Fixture::new();
    let fallback = Some(TeamTarget::Cell { x: 18, y: 18 });
    let state = f.sim.team_script_vm.teams.get_mut(&f.team).unwrap();
    state.zone = Some(TeamTarget::Object(f.member));
    state.focus = state.zone;
    state.mission_target = fallback;
    f.step();
    let state = f.sim.team_script_vm.team(f.team).unwrap();
    assert_eq!(state.zone, None);
    assert_eq!(state.closest_member, None);
    assert_eq!(state.focus, fallback);
    assert_eq!(
        f.sim
            .substrate
            .entities
            .get(f.member)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::Cell { rx: 18, ry: 18 })
    );
}

#[test]
fn follow_friendlies_only_stationary_armed_guard_members_mirror_attack() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::{DriveLocomotionRuntime, locomotor::LocomotorState};
    for (armed, moving, mission, mirrors) in [
        (true, false, MissionType::Guard, true),
        (false, false, MissionType::Guard, false),
        (true, true, MissionType::Guard, false),
        (true, false, MissionType::Unload, false),
        (true, false, MissionType::Attack, false),
    ] {
        let mut f = Fixture::new();
        let friendly = f.spawn("TANK", "Russians", 5, 4);
        let enemy = f.spawn("TANK", "Americans", 10, 4);
        f.put(friendly, [128, 0, 0]);
        f.target(friendly, Some(TargetKind::Entity(enemy)));
        if !armed {
            let unarmed = f.sim.interner.intern("UNARMED");
            f.sim
                .substrate
                .entities
                .get_mut(f.member)
                .unwrap()
                .set_type_ref_for_test(unarmed);
        }
        let entity = f.sim.substrate.entities.get_mut(f.member).unwrap();
        entity
            .mission
            .set_current_for_test(MissionId::from_known(mission), 0);
        if moving {
            let mut loco = LocomotorState::for_test_kind(LocomotorKind::Drive);
            assert!(
                loco.install_drive_state_for_test(Some(
                    DriveLocomotionRuntime::default()
                        .with_destination_for_test(Some(DriveCoord::cell(20, 20, 0)))
                ))
            );
            entity.locomotor = Some(loco);
        }
        f.step();
        let entity = f.sim.substrate.entities.get(f.member).unwrap();
        assert_eq!(
            entity.attack_target.target(),
            mirrors.then_some(TargetKind::Entity(enemy)),
            "armed={armed} moving={moving} mission={mission:?}"
        );
        if mirrors {
            assert_eq!(
                entity.mission.queued(),
                MissionId::from_known(MissionType::Attack)
            );
            assert_eq!(
                entity.mission.effective(),
                MissionId::from_known(MissionType::Guard),
                "the handler queues but does not Commence"
            );
        }
    }
}

#[test]
fn follow_friendlies_clears_mismatched_target_even_without_a_new_attack() {
    for shared in [None, Some(TargetKind::Cell(20, 20))] {
        let mut f = Fixture::new();
        let friendly = f.spawn("TANK", "Russians", 5, 4);
        let enemy = f.spawn("TANK", "Americans", 10, 4);
        f.put(friendly, [128, 0, 0]);
        f.target(f.member, Some(TargetKind::Entity(enemy)));
        f.target(friendly, shared);
        f.sim
            .substrate
            .entities
            .get_mut(f.member)
            .unwrap()
            .mission
            .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
        f.step();
        assert_eq!(
            f.sim
                .substrate
                .entities
                .get(f.member)
                .unwrap()
                .attack_target
                .target(),
            shared
        );
    }
}

#[test]
fn follow_friendlies_tube_uses_exit_cell_and_does_not_touch_attack_target() {
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_loader_clear_cell};
    use crate::map::tube_facts::{TubeFact, TubeId};
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
    let mut f = Fixture::new();
    let friendly = f.spawn("TANK", "Russians", 8, 4);
    let enemy = f.spawn("TANK", "Americans", 20, 4);
    let cells = (0..32)
        .flat_map(|y| (0..32).map(move |x| test_loader_clear_cell(x, y)))
        .collect();
    f.sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells_with_tubes(
        32,
        32,
        cells,
        vec![TubeFact::explicit((8, 4), (18, 4), 2, vec![2; 10])],
    ));
    f.sim
        .substrate
        .entities
        .get_mut(friendly)
        .unwrap()
        .low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
        TubeId(0),
        0,
        DriveCoord::cell(9, 4, 0),
    ));
    f.target(f.member, Some(TargetKind::Entity(enemy)));
    f.step();
    let state = f.sim.team_script_vm.team(f.team).unwrap();
    assert_eq!(state.zone, Some(TeamTarget::Cell { x: 18, y: 4 }));
    assert_eq!(state.closest_member, Some(f.member));
    let member = f.sim.substrate.entities.get(f.member).unwrap();
    assert_eq!(
        member.navigation.nav_com(),
        Some(NavTargetRef::Cell { rx: 18, ry: 4 })
    );
    assert_eq!(
        member.attack_target.target(),
        Some(TargetKind::Entity(enemy))
    );
}

#[test]
fn follow_friendlies_snapshot_keeps_focus_and_continues_reselection_without_rng_draws() {
    use crate::sim::snapshot::GameSnapshot;
    let mut f = Fixture::new();
    let first = f.spawn("TANK", "Russians", 12, 4);
    let second = f.spawn("TANK", "Russians", 18, 4);
    let rng = [
        f.sim.scenario_rng.native_state_hex(),
        f.sim.main_rng.native_state_hex(),
        f.sim.mapgen_rng.native_state_hex(),
    ];
    f.step();
    assert_eq!(
        rng,
        [
            f.sim.scenario_rng.native_state_hex(),
            f.sim.main_rng.native_state_hex(),
            f.sim.mapgen_rng.native_state_hex()
        ]
    );
    let bytes = GameSnapshot::save(&f.sim, 0, 0, "follow-fixture", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(
        restored.team_script_vm.team(f.team),
        f.sim.team_script_vm.team(f.team)
    );
    for sim in [&mut f.sim, &mut restored] {
        sim.substrate
            .entities
            .get_mut(first)
            .unwrap()
            .health
            .set_for_test(0);
        sim.team_step_script(f.team, &f.rules, None, FrameEffects::default());
        assert_eq!(
            sim.team_script_vm.team(f.team).unwrap().zone,
            Some(TeamTarget::Object(second))
        );
    }
    assert_eq!(
        restored.team_script_vm.team(f.team),
        f.sim.team_script_vm.team(f.team)
    );
}

#[test]
fn follow_friendlies_placement_scope_admits_limbo_without_leaking() {
    let mut f = Fixture::new();
    let friendly = f.spawn("TANK", "Russians", 12, 4);
    f.sim
        .substrate
        .entities
        .get_mut(friendly)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert_eq!(f.center(), None);
    let team = f.team;
    f.sim
        .with_object_placement_scope(|sim| sim.team_calc_center(team, &f.rules, None));
    assert_eq!(
        f.sim.team_script_vm.team(team).unwrap().zone,
        Some(TeamTarget::Object(friendly))
    );
    assert_eq!(f.center(), None, "scope does not leak to later calls");
}
