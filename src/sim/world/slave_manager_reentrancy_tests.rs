//! FreeSlaves6B0AE0 through generated retail slaves, Team recruitment and
//! a synchronous event48/DestroyAttached32 callback. The sole Rules override
//! in the positive controls is authored [Harvest] Recruitable=yes. These are
//! Rust composition regressions, not a joined original-executable replay.

use super::{FrameEffects, SimSoundEvent, Simulation};
use crate::map::map_file::MapFile;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::vision::FogState;

struct Scene {
    sim: Simulation,
    rules: RuleSet,
    overlays: OverlayTypeRegistry,
    master: u64,
    slaves: [u64; 5],
    old_house: InternedId,
    new_house: InternedId,
    team_type: InternedId,
}

fn scene(recruitable: bool, tagged: bool) -> Option<Scene> {
    let patch = IniFile::from_str(if recruitable {
        "[Harvest]\nRecruitable=yes\n"
    } else {
        ""
    });
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)?;
    let aimd = crate::rules::retail_ini_fixture::retail_ini("aimd.ini")?;
    let rules = retail.rules;
    let master_type = rules.object("YAREFN").unwrap();
    assert_eq!(master_type.enslaves.as_deref(), Some("SLAV"));
    assert_eq!(master_type.slaves_number, 5);
    let slave_type = rules.object("SLAV").unwrap();
    assert!(slave_type.slaved && slave_type.dont_score);
    assert_eq!((slave_type.strength, slave_type.harvest_rate), (125, 150));
    assert_eq!(rules.bridge_warheads.c4_name, "Super");
    assert_eq!(rules.warhead("Super").unwrap().inf_death, 2);
    assert!(rules.object("TANY").unwrap().c4);
    assert_eq!(
        crate::sim::mission::control::mission_recruitable(
            &rules,
            MissionId::from_known(MissionType::Harvest),
        ),
        recruitable,
    );
    let tag = if tagged { "Tag=FREE_SLAVES_TAG\n" } else { "" };
    let map = MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,32,32\nLocalSize=0,0,32,32\n\
             [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
             [Basic]\nNewINIFormat=4\n\
             [TeamTypes]\n0=FREE_SLAVES_TEAM\n\
             [FREE_SLAVES_TEAM]\nHouse=YuriCountry\nScript=SLAVE_HOLD\nTaskForce=SLAVE_FORCE\n\
             Max=1\nRecruiter=yes\nAutocreate=no\nOnTransOnly=no\n{tag}\
             [ScriptTypes]\n0=SLAVE_HOLD\n[SLAVE_HOLD]\n0=5,10000\n\
             [TaskForces]\n0=SLAVE_FORCE\n[SLAVE_FORCE]\n0=5,SLAV\n\
             [Triggers]\nFREE_SLAVES=YuriCountry,<none>,Freed,0,1,1,1,0\n\
             [Events]\nFREE_SLAVES=1,48,0,0\n\
             [Actions]\nFREE_SLAVES=1,32,0,0,0,0,0,0,A\n\
             [Tags]\nFREE_SLAVES_TAG=0,Freed,FREE_SLAVES\n"
        )
        .as_bytes(),
    )
    .unwrap();
    let teams = TeamAiIniRegistry::from_sources(&aimd, &map.ini, true);
    assert!(teams.fixed_source_is_complete());
    // Only terrain, ore, initial Houses and finished refinery placement are
    // arena priors. No slave, node, mission, Tag or callback is installed.
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let mut sim =
        super::refinery_dock_oracle_tests::world_with(&rules, &retail.processed_rules, |_| {});
    sim.fog = FogState::new(33, 33);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().short_game = false;
    sim.session.publish_started();
    sim.set_input_delay_ticks(0);
    for (name, side) in [("YuriCountry", 2), ("Americans", 0)] {
        let id = sim.interner.intern(name);
        let mut house = HouseState::new(id, side, Some(id), true, 10_000, 10);
        house.project_country_mults(&rules, &sim.interner);
        sim.houses.register(house);
    }
    let old_house = sim.interner.get("YuriCountry").unwrap();
    let new_house = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(new_house));
    // Keep each House in play after the refinery and tagged children die.
    for (typ, owner, cell) in [
        ("YAPOWR", "YuriCountry", (24, 23)),
        ("GAPOWR", "Americans", (27, 23)),
    ] {
        sim.spawn_object(typ, owner, cell.0, cell.1, 0, &rules)
            .expect("House anchor");
    }
    let ore = overlays.id_for_name("TIB01").expect("retail ore overlay");
    for y in 10..=16 {
        for x in 17..=21 {
            let grid = sim.overlay_grid.as_mut().unwrap();
            grid.place_overlay(x, y, ore, 11);
            grid.recalculate_runtime_cell(
                sim.resolved_terrain.as_mut().unwrap(),
                &overlays,
                (x, y),
            );
        }
    }
    sim.install_team_ai_registry(&teams, &rules).unwrap();
    sim.initialize_trigger_definitions(&map, &teams, None, None);
    sim.classify_map_triggers(&rules);
    let team_type = sim.interner.get("FREE_SLAVES_TEAM").unwrap();
    let master = sim
        .spawn_object("YAREFN", "YuriCountry", 12, 12, 0, &rules)
        .expect("retail refinery Unlimbos on the declared flat foundation");
    let slaves: [u64; 5] = sim
        .entities()
        .get(master)
        .unwrap()
        .slave_manager
        .as_ref()
        .unwrap()
        .nodes()
        .iter()
        .map(|node| node.slave().expect("constructor generated a slave"))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    for id in slaves {
        let slave = sim.entities().get(id).unwrap();
        assert!(slave.lifecycle.in_limbo());
        assert_eq!(slave.slave.owner(), Some(master));
        assert_eq!(sim.interner.resolve(slave.type_ref()), "SLAV");
    }
    Some(Scene {
        sim,
        rules,
        overlays,
        master,
        slaves,
        old_house,
        new_house,
        team_type,
    })
}

impl Scene {
    fn frame(&mut self) {
        let commands = self.sim.take_due_commands();
        let grid = self.sim.path_grid_snapshot();
        self.sim.advance_tick(
            &commands,
            Some(&self.rules),
            grid.as_deref(),
            Some(&self.overlays),
            67,
        );
    }

    fn wait_for_harvest(&mut self) {
        for _ in 0..1500 {
            self.frame();
            if self.slaves.iter().all(|&id| {
                let slave = self.sim.entities().get(id).unwrap();
                slave.health.current() > 0
                    && !slave.lifecycle.in_limbo()
                    && slave.mission.effective().known() == Some(MissionType::Harvest)
            }) {
                return;
            }
        }
        panic!("generated slaves did not naturally reach Harvest");
    }

    fn create_team(&mut self) -> u64 {
        // Enter the existing Action4 creator once all five are harvesting;
        // early Guard/Move recruitment would evade the Harvest admission.
        self.sim
            .trigger_create_team(
                self.team_type,
                &self.rules,
                Some(&self.overlays),
                FrameEffects::default(),
            )
            .expect("authored TeamType and Country resolve")
    }

    fn recruit(&mut self, tagged: bool) {
        let team = self.create_team();
        for _ in 0..120 {
            self.frame();
            if self
                .sim
                .team_script_vm
                .team(team)
                .is_some_and(|team| team.formed())
            {
                let mut members: Vec<_> = self
                    .sim
                    .team_script_vm
                    .team(team)
                    .unwrap()
                    .members()
                    .collect();
                members.sort_unstable();
                let mut slaves = self.slaves;
                slaves.sort_unstable();
                assert_eq!(members, slaves);
                for id in self.slaves {
                    assert_eq!(self.sim.object_has_trigger_tag(id), tagged);
                    assert_eq!(
                        self.sim.entities().get(id).unwrap().slave.owner(),
                        Some(self.master)
                    );
                }
                return;
            }
        }
        panic!("ordinary TeamAI did not recruit all five generated slaves");
    }

    fn destroy_master(&mut self) {
        let tanya = self
            .sim
            .spawn_object("TANY", "Americans", 13, 16, 0, &self.rules)
            .unwrap();
        self.sim.queue_command(CommandEnvelope::new(
            self.new_house,
            self.sim.session.tick() + 1,
            Command::Attack {
                attacker_id: tanya,
                target_id: self.master,
            },
        ));
        let mut planted = false;
        for _ in 0..512 {
            self.frame();
            let master = self.sim.entities().get(self.master);
            if let Some(charge) = master.and_then(|master| master.pending_c4_detonation.pending()) {
                assert_eq!(charge.source_entity_id(), Some(tanya));
                planted = true;
            }
            if master.is_none_or(|master| master.health.current() == 0) {
                assert!(
                    planted,
                    "Attack must reach Tanya's real arrival and C4 fuse"
                );
                assert!(
                    self.sim
                        .entities()
                        .get(tanya)
                        .is_some_and(|tanya| tanya.is_alive())
                );
                return;
            }
            assert!(
                self.slaves.iter().all(|&id| self
                    .sim
                    .entities()
                    .get(id)
                    .is_some_and(|slave| slave.is_alive())),
                "the Tag must first fire during FreeSlaves, not an earlier slave death"
            );
        }
        panic!("Tanya Attack did not destroy the refinery through ordinary frames");
    }

    fn score(&self) -> i32 {
        self.sim.houses[&self.new_house].economy().score()
    }

    fn price(&self, typ: &str) -> i32 {
        self.sim
            .cost_of(self.old_house, self.rules.object(typ).unwrap(), &self.rules)
    }

    fn assert_released(&self) {
        assert!(
            self.sim
                .entities()
                .get(self.master)
                .is_none_or(|master| master.slave_manager.is_none())
        );
        for id in self.slaves {
            assert_eq!(self.sim.entities().get(id).unwrap().slave.owner(), None);
        }
        assert_eq!(self.sim.sound_events.iter().filter(|event| matches!(event,
            SimSoundEvent::VocAt { sound_id, .. }
                if Some(sound_id.as_str()) == self.rules.general.slaves_free_sound.as_deref()
        )).count(), 1, "the first freed slave owns the liberation sound");
    }
}

#[test]
fn authored_harvest_team_tag_does_not_capture_later_dead_slaves() {
    let Some(mut s) = scene(true, true) else {
        return;
    };
    s.wait_for_harvest();
    s.recruit(true);
    let score = s.score();
    s.destroy_master();
    s.assert_released();
    // Original6B0B4C rereads nodes cleared by fatal Infantry51808E ->
    // RemoveSlave6B0A20 during the first ChangeOwner's event48/Action32.
    // The current child still finishes its owner change; later corpses do not.
    for (index, id) in s.slaves.into_iter().enumerate() {
        let slave = s.sim.entities().get(id).expect("Die2 retains the corpse");
        assert_eq!(slave.health.current(), 0);
        assert!(slave.lifecycle.object_alive() && !slave.lifecycle.in_limbo());
        assert_eq!(slave.infantry_sprite_pose().unwrap().0, 12);
        assert_eq!(
            slave.owner(),
            if index == 4 { s.new_house } else { s.old_house }
        );
    }
    assert!(
        s.sim
            .tag_dispatch_trace_for_test()
            .iter()
            .any(|&(_, kind)| kind == 48)
    );
    assert_eq!(
        s.score() - score,
        s.price("YAREFN") + s.price("SLAV"),
        "later DontScore corpses must not add capture cost"
    );
    for _ in 0..120 {
        s.frame();
    }
    assert!(
        s.slaves
            .iter()
            .all(|&id| s.sim.entities().get(id).is_none()),
        "ordinary Die2 sequencing and deletion drain the corpses"
    );
    assert!(s.sim.entities().get(s.master).is_none());
    assert_eq!(s.score() - score, s.price("YAREFN") + s.price("SLAV"));
}

#[test]
fn generated_slaves_without_a_team_tag_all_pass_to_the_killer() {
    let Some(mut s) = scene(true, false) else {
        return;
    };
    s.wait_for_harvest();
    s.recruit(false);
    let score = s.score();
    s.destroy_master();
    s.assert_released();
    for id in s.slaves {
        let slave = s.sim.entities().get(id).unwrap();
        assert!(slave.is_alive() && !slave.lifecycle.in_limbo());
        assert_eq!(slave.owner(), s.new_house);
        assert_eq!(slave.infantry_sprite_pose().unwrap().0, 32);
    }
    assert_eq!(s.score() - score, s.price("YAREFN") + 5 * s.price("SLAV"));
    for _ in 0..120 {
        s.frame();
    }
    assert!(s.sim.entities().get(s.master).is_none());
    assert!(s.slaves.iter().all(|&id| {
        s.sim
            .entities()
            .get(id)
            .is_some_and(|slave| slave.is_alive())
    }));
    assert_eq!(
        s.score() - score,
        s.price("YAREFN") + 5 * s.price("SLAV"),
        "destruction cannot free the same children twice"
    );
}

#[test]
fn retail_harvest_mission_refuses_the_slave_team_recruitment() {
    let Some(mut s) = scene(false, true) else {
        return;
    };
    s.wait_for_harvest();
    let team = s.create_team();
    // TeamAI scans before the object turns. Every candidate enters this
    // visit on Harvest; a later natural return on Move has another gate.
    s.frame();
    assert_eq!(s.sim.team_script_vm.team(team).unwrap().member_count(), 0);
    for id in s.slaves {
        let slave = s.sim.entities().get(id).unwrap();
        assert_eq!(slave.slave.owner(), Some(s.master));
        assert!(!s.sim.object_has_trigger_tag(id));
    }
}
