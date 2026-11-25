//! The teams' own turns ([`Simulation::run_team_ai_pass`] with batched target
//! scans): what a team times, and what its update must do at once, happens on
//! the same frame with them as without, and `Coordinate_Attack`'s leader
//! check still runs once every 8 frames.

use super::super::*;
use super::*;
use crate::sim::world::FrameEffects;

/// An arena with `teams` teams of one `kind` infantryman each, formed on
/// `script` and about to start its first line. A `SOLDIER`'s only weapon
/// fires at aircraft alone, a `GI`'s at the ground alone.
fn scene(kind: &str, teams: u16, script: &[(i32, i32)]) -> (Simulation, RuleSet, Vec<u64>) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=SOLDIER\n1=GI\n\
         [SOLDIER]\nStrength=100\nSpeed=4\nPrimary=FLAK\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [GI]\nStrength=100\nSpeed=4\nPrimary=GUN\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n\
         [FLAK]\nDamage=10\nROF=20\nRange=6\nProjectile=FLAKP\nWarhead=SA\n\
         [FLAKP]\nAA=yes\nAG=no\n\
         [GUN]\nDamage=10\nROF=20\nRange=6\nProjectile=SHOT\nWarhead=SA\n\
         [SHOT]\nAG=yes\n[SA]\nVerses=100%\n",
    ))
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1302);
    let waypoint = crate::map::waypoints::Waypoint {
        index: 11,
        rx: 30,
        ry: 20,
    };
    sim.session.install_waypoints(&[(11, waypoint)].into());
    let owner = sim.interner.intern("Russians");
    let script_id = sim.interner.intern("CADENCE");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script_id,
        source: TeamAiDefinitionSource::Scenario,
        actions: script
            .iter()
            .map(|&(action_id, argument)| TeamScriptAction {
                action_id,
                argument,
            })
            .collect(),
    });
    let ids = (0..teams)
        .map(|index| {
            let member = sim
                .spawn_object(kind, "Russians", 10, 10 + 2 * index, 0, &rules)
                .unwrap();
            let team = sim
                .team_script_vm
                .create_team(owner, script_id, vec![member], 0);
            sim.team_script_vm.seek_line_for_test(team, 0);
            team
        })
        .collect();
    (sim, rules, ids)
}

/// The first of `frames` after whose team pass `done` holds.
fn first_frame(
    sim: &mut Simulation,
    rules: &RuleSet,
    frames: std::ops::Range<u32>,
    done: impl Fn(&Simulation) -> bool,
) -> Option<u32> {
    frames.into_iter().find(|&frame| {
        sim.session.set_binary_frame_for_test(frame);
        sim.run_team_ai_pass(rules, None, &mut FrameEffects::default());
        done(sim)
    })
}

/// Action 5 guards for its argument times 15 frames (`0x006E97CE`), and the
/// next line starts on the frame after, with own turns as without, on
/// whichever frame of the team's turns it starts.
#[test]
fn a_guard_lasts_as_long_with_own_turns() {
    for start in 100..104 {
        let ends = [false, true].map(|turns| {
            let (mut sim, rules, teams) = scene("SOLDIER", 1, &[(5, 1), (2, 0)]);
            if turns {
                sim.batch_target_scans();
            }
            first_frame(&mut sim, &rules, start..start + 40, |sim| {
                sim.team_script_vm.teams[&teams[0]].cursor == 1
            })
        });
        assert_eq!(ends, [Some(start + 16); 2], "from frame {start}");
    }
}

/// A suspended team (`+0x83`) goes on on the frame its timer runs out
/// (`0x006E9149..0x006E9174`), with own turns as without, though its action
/// (2) would wait for its turn.
#[test]
fn a_suspension_ends_on_the_same_frame_with_own_turns() {
    for start in 100..104 {
        let ends = [false, true].map(|turns| {
            let (mut sim, rules, teams) = scene("SOLDIER", 1, &[(2, 0)]);
            if turns {
                sim.batch_target_scans();
            }
            // The first update starts the line.
            first_frame(&mut sim, &rules, 90..91, |_| true);
            let team = sim.team_script_vm.teams.get_mut(&teams[0]).unwrap();
            team.suspended = true;
            team.suspend_timer = crate::sim::timer::CdTimer::started(start as i32, 10);
            first_frame(&mut sim, &rules, start..start + 20, |sim| {
                !sim.team_script_vm.teams[&teams[0]].suspended
            })
        });
        assert_eq!(ends, [Some(start + 10); 2], "from frame {start}");
    }
}

/// A formed `Reinforce=` team that lost a member, but not so many that it is
/// under strength, recruits (`0x006E9227..0x006E9299`) on the frame an object
/// it can take appears, with own turns as without, though its action (2)
/// would wait for its turn.
#[test]
fn a_reinforced_team_recruits_on_the_same_frame_with_own_turns() {
    for start in 100..104 {
        let joins = [false, true].map(|turns| {
            let (mut sim, rules, _) = scene("SOLDIER", 0, &[(2, 0)]);
            if turns {
                sim.batch_target_scans();
            }
            let members: Vec<u64> = (0..3)
                .map(|index| {
                    sim.spawn_object("SOLDIER", "Russians", 10, 10 + index, 0, &rules)
                        .unwrap()
                })
                .collect();
            let member_type = member_type_identity(sim.substrate.entities.get(members[0]).unwrap());
            let owner = sim.interner.intern("Russians");
            let script = sim.interner.intern("CADENCE");
            let task_force = sim.interner.intern("CADENCE FORCE");
            let team_type = sim.interner.intern("CADENCE TEAM");
            let vm = &mut sim.team_script_vm;
            vm.register_task_force(TeamTaskForceDefinition {
                id: task_force,
                source: TeamAiDefinitionSource::Scenario,
                group: -1,
                entries: vec![TeamTaskForceEntry {
                    member_type,
                    count: 3,
                }],
            });
            vm.register_team_type(TeamTypeDefinition {
                id: team_type,
                script_id: script,
                task_force_id: task_force,
                priority: 0,
                is_base_defense: false,
                suicide: false,
                aggressive: false,
                combined_movement_zone: MovementZone::Normal,
                base_zone_relation_enforced: true,
                transport_crossing_required: false,
            });
            vm.team_type_ini.get_mut(&team_type).unwrap().reinforce = true;
            let candidates: Vec<TeamScriptMember> = members
                .iter()
                .map(|&entity_id| TeamScriptMember {
                    entity_id,
                    member_type,
                })
                .collect();
            let team = vm.create_team_from_type(owner, team_type, &candidates, 0);
            // A member goes; the next update counts two of three, above a
            // third of the TaskForce, so not under strength.
            sim.uninit_with_rules(members[2], &rules);
            first_frame(&mut sim, &rules, 90..91, |_| true);
            let state = &sim.team_script_vm.teams[&team];
            assert_eq!(
                (state.members.len(), state.formed, state.full_strength),
                (2, true, false)
            );
            assert!(!state.under_strength && !state.altered && !state.reforming);
            sim.spawn_object("SOLDIER", "Russians", 12, 10, 0, &rules)
                .unwrap();
            first_frame(&mut sim, &rules, start..start + 8, |sim| {
                sim.team_script_vm.teams[&team].members.len() == 3
            })
        });
        assert_eq!(joins, [Some(start); 2], "from frame {start}");
    }
}

/// A team whose target (`+0x40`) goes while its mission target (`+0x3C`)
/// stays takes the mission target up again on the next frame
/// (`0x006E9402..0x006E940C`), with own turns as without: action 1 aims at
/// what stands on its waypoint's cell (`Coordinate_Attack`), here an
/// infantryman who comes after the action started, and then goes
/// (`PointerExpired`).
#[test]
fn a_team_takes_up_its_mission_target_on_the_next_frame_with_own_turns() {
    for start in 100..104 {
        let ends = [false, true].map(|turns| {
            let (mut sim, rules, teams) = scene("GI", 1, &[(1, 11)]);
            if turns {
                sim.batch_target_scans();
            }
            let team = teams[0];
            first_frame(&mut sim, &rules, 90..91, |_| true);
            let blocker = sim
                .spawn_object("SOLDIER", "Americans", 30, 20, 0, &rules)
                .unwrap();
            first_frame(&mut sim, &rules, 91..start + 1, |_| false);
            let state = &sim.team_script_vm.teams[&team];
            assert!(matches!(
                state.mission_target,
                Some(TeamTarget::Cell { .. })
            ));
            assert_eq!(state.focus, Some(TeamTarget::Object(blocker)));
            sim.uninit_with_rules(blocker, &rules);
            assert_eq!(sim.team_script_vm.teams[&team].focus, None);
            first_frame(&mut sim, &rules, start + 1..start + 9, |sim| {
                sim.team_script_vm.teams[&team].focus.is_some()
            })
        });
        assert_eq!(ends, [Some(start + 1); 2], "from frame {start}");
    }
}

/// `Coordinate_Attack`'s leader check (`0x006EB59A..0x006EB5D2`) runs once
/// every 8 frames with own turns too, on a team's first own turn from the
/// frame it runs on without: a leader whose weapon cannot fire at the cell
/// action 1 attacks restarts the action (`+0x81`) as often.
#[test]
fn the_attack_check_runs_once_every_eight_frames_with_own_turns() {
    let checks = [false, true].map(|turns| {
        let (mut sim, rules, teams) = scene("SOLDIER", 4, &[(1, 11)]);
        if turns {
            sim.batch_target_scans();
        }
        let mut checks = vec![Vec::new(); teams.len()];
        for frame in 100..132 {
            sim.session.set_binary_frame_for_test(frame);
            sim.run_team_ai_pass(&rules, None, &mut FrameEffects::default());
            for (checked, team) in checks.iter_mut().zip(&teams) {
                if sim.team_script_vm.teams[team].retarget {
                    checked.push(frame);
                }
            }
        }
        checks
    });
    assert_eq!(checks[0], vec![vec![100, 108, 116, 124]; 4]);
    for (on, off) in checks[1].iter().zip(&checks[0]) {
        assert_eq!(on.len(), off.len(), "{on:?}");
        assert!(
            on.iter()
                .zip(off)
                .all(|(on, off)| (*off..*off + 4).contains(on)),
            "{on:?}"
        );
    }
    assert_ne!(checks[1], checks[0], "the teams' turns spread the checks");
}
