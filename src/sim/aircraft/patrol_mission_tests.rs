//! Replay executed417300 controls with the same substituted virtuals and
//! production search/landing/coordinate/Rate/RNG owners as the native fixture.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::world::{FrameEffects, aircraft_search_fixture};
use serde_json::{Value, json};

const OWNER: u64 = 1;
const BLOCKER: u64 = 4;

fn param(input: &Value) -> Option<NavTargetRef> {
    match &input["param"] {
        Value::Null => None,
        Value::String(_) => Some(NavTargetRef::object(BLOCKER)),
        cell => Some(NavTargetRef::cell(
            cell[0].as_u64().unwrap() as u16,
            cell[1].as_u64().unwrap() as u16,
        )),
    }
}

fn name(nav: Option<NavTargetRef>) -> Value {
    match nav {
        None => Value::Null,
        Some(NavTargetRef::Cell { rx, ry }) => json!(["cell", rx, ry]),
        Some(NavTargetRef::Object { id: BLOCKER }) => json!("blocker"),
        other => panic!("unexpected {other:?}"),
    }
}

struct Recorder<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    input: &'a Value,
    calls: Vec<Value>,
    ammo: i32,
    attached_team: bool,
}

impl Recorder<'_> {
    fn nav(&self) -> Option<NavTargetRef> {
        self.sim
            .substrate
            .entities
            .get(OWNER)
            .unwrap()
            .navigation
            .nav_com()
    }
}

impl PatrolHost for Recorder<'_> {
    type Object = u64;
    fn nav_com(&self) -> bool {
        self.nav().is_some()
    }
    fn enter_idle(&mut self) {
        self.calls.push(json!(["enter_idle_mode", 0, 1]));
        if self.input["idle_guard"].as_bool() == Some(true) {
            self.sim
                .substrate
                .mission_assign_exact(OWNER, MissionId::from_known(MissionType::Guard), 0)
                .unwrap();
        }
    }
    fn assign_attack_cell(&mut self) {
        let nav = self
            .sim
            .aircraft_find_attack_cell(OWNER, self.nav(), self.rules);
        self.calls.push(json!(["assign_destination", name(nav), 1]));
        if self.input["attach_team"].as_bool() == Some(true) {
            self.attached_team = true;
            self.sim
                .substrate
                .entities
                .get_mut(OWNER)
                .unwrap()
                .navigation
                .set_nav_com_for_test(Some(NavTargetRef::object(BLOCKER)));
        }
    }
    fn assign_team_target(&mut self) {
        if self.attached_team {
            self.calls.push(json!(["team_target", name(self.nav())]));
        }
    }
    fn move_to_nav_com(&mut self) {
        let coord = nav_target_coordinate(
            self.nav().unwrap(),
            Some(OWNER),
            self.sim,
            self.sim.resolved_terrain.as_ref(),
            Some((self.rules, &self.sim.interner)),
        )
        .unwrap();
        self.calls
            .push(json!(["move_to", [coord.x, coord.y, coord.z]]));
    }
    fn ammo(&self) -> i32 {
        self.ammo
    }
    fn find_dock(&mut self) -> Option<u64> {
        self.calls.push(json!(["find_dock"]));
        if let Some(ammo) = self.input["dock_ammo"].as_i64() {
            self.ammo = ammo as i32;
        }
        (self.input["dock"].as_bool() == Some(true)).then_some(BLOCKER)
    }
    fn greatest_threat(&mut self) -> Option<u64> {
        self.calls.push(json!(["scan", 2, 0]));
        (self.input["threat"].as_bool() == Some(true)
            && self.input["techno"].as_bool() != Some(false))
        .then_some(BLOCKER)
    }
    fn override_mission(&mut self, mission: MissionType, _: u64) {
        self.calls.push(json!([
            "override",
            mission.id(),
            if mission == MissionType::Attack {
                json!("blocker")
            } else {
                Value::Null
            },
            if mission == MissionType::Enter {
                json!("blocker")
            } else {
                Value::Null
            }
        ]));
        self.sim
            .substrate
            .entities
            .get_mut(OWNER)
            .unwrap()
            .mission
            .set_handler_state(99);
    }
    fn is_moving(&mut self) -> bool {
        self.calls.push(json!(["is_moving"]));
        self.input["moving"].as_bool().unwrap()
    }
    fn nav_cell_free(&mut self) -> bool {
        self.sim.aircraft_nav_cell_free(OWNER, self.rules)
    }
    fn epilogue(&mut self) -> i32 {
        Simulation::mission_rate_epilogue_for(
            &mut self.sim.scenario_rng,
            &self.sim.substrate,
            self.rules,
            OWNER,
            MissionType::Patrol,
        )
    }
}

#[test]
fn patrol_matches_original_states_calls_and_rng() {
    let rows =
        crate::test_fixture::json("tools/spatial_oracle/aircraft_patrol.json")["mission_patrol"]
            .as_array()
            .unwrap();
    assert_eq!(rows.len(), 54);
    for row in rows {
        let input = &row["input"];
        let label = input["name"].as_str().unwrap();
        let (mut sim, mut rules) = aircraft_search_fixture(input);
        rules.mission_control =
            crate::rules::mission_data::MissionControl::from_ini(&IniFile::from_str(&format!(
                "[Patrol]\nRate={}\n[Guard]\nRate=.03\n",
                input["rate"].as_f64().unwrap_or(0.016)
            )));
        let state = input["state"].as_u64().unwrap() as u32;
        sim.substrate
            .mission_assign_exact(OWNER, MissionId::from_known(MissionType::Patrol), 0)
            .unwrap();
        let entity = sim.substrate.entities.get_mut(OWNER).unwrap();
        entity.mission.set_handler_state(state);
        entity.navigation.set_nav_com_for_test(param(input));
        let mut host = Recorder {
            sim: &mut sim,
            rules: &rules,
            input,
            calls: Vec::new(),
            ammo: input["ammo"].as_i64().unwrap_or(1) as i32,
            attached_team: false,
        };
        let ((next, delay), draws) =
            crate::sim::rng::trace_draws(|| patrol_visit(state, &mut host));
        if let Some(next) = next {
            host.sim
                .substrate
                .entities
                .get_mut(OWNER)
                .unwrap()
                .mission
                .set_handler_state(next);
        }
        assert_eq!(
            json!(
                host.sim
                    .substrate
                    .entities
                    .get(OWNER)
                    .unwrap()
                    .mission
                    .handler_state()
            ),
            row["state"],
            "{label}: state"
        );
        assert_eq!(json!(delay), row["delay"], "{label}: delay");
        assert_eq!(json!(host.calls), row["calls"], "{label}: calls");
        assert_eq!(
            draws
                .iter()
                .map(|draw| draw["value"].clone())
                .collect::<Value>(),
            row["raw"],
            "{label}: raw RNG"
        );
        assert_eq!(
            json!(host.sim.scenario_rng.next_u32()),
            row["next_random"],
            "{label}: next RNG"
        );
    }
}

const RULES: &str = "[General]\nFlightLevel=1500\n[VehicleTypes]\n0=TANK\n[AircraftTypes]\n0=ORCA\n[BuildingTypes]\n0=GAAIRC\n\
[ORCA]\nStrength=150\nSpeed=14\nAmmo=1\nLandable=yes\nAirportBound=yes\nFighter=yes\nPrimary=Gun\nDock=GAAIRC\nLocomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
[GAAIRC]\nStrength=1000\nFoundation=2x2\nHelipad=yes\nUnitReload=yes\nNumberOfDocks=4\nHasStupidGuardMode=no\n\
[Gun]\nDamage=10\nRange=1\nROF=60\nProjectile=Shot\nWarhead=WH\n[Shot]\nROT=1\n[WH]\nVerses=100%\n\
[TANK]\nStrength=1000\nPrimary=Gun\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n";

fn scene() -> (Simulation, RuleSet, u64) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(RULES)).unwrap();
    let mut sim = Simulation::with_seed(31);
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner,
                index as u8,
                None,
                true,
                0,
                10,
            ));
    }
    let plane = sim
        .spawn_object_at_height("ORCA", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .mission_assign_exact(plane, MissionId::from_known(MissionType::Patrol), 0)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .mission
        .set_handler_state(2);
    sim.assign_aircraft_destination(
        plane,
        Some(NavTargetRef::cell(20, 10)),
        &rules,
        FrameEffects::default(),
    );
    (sim, rules, plane)
}

#[test]
fn patrol_scan_preserves_the_aircraft_class_mask_and_clears_empty_scan_latch() {
    let (mut sim, rules, plane) = scene();
    let _tank = sim
        .spawn_object("TANK", "Russians", 15, 10, 0, &rules)
        .unwrap();
    let destination = sim
        .substrate
        .entities
        .get(plane)
        .unwrap()
        .navigation
        .nav_com();
    // Aircraft inherits Foot4D9920 without Unit/Infantry's projectile-bit OR.
    // Literal2 (or Foot688-coerced1) builds zero class flags6F8F29..6F8F76;
    // EvaluateCandidate6F821A rejects every Techno class. Do not manufacture
    // an auto-attack by widening the caller's literal mask.
    for narrow in [false, true] {
        sim.substrate
            .mission_assign_exact(plane, MissionId::from_known(MissionType::Patrol), 0)
            .unwrap();
        let entity = sim.substrate.entities.get_mut(plane).unwrap();
        entity.mission.set_handler_state(2);
        if narrow {
            entity.foot_retarget_after_stop.mark_stopped_cannot_fire();
        }
        super::super::dispatch_mission(&mut sim, plane, &rules, ObjectAiCtx::default());
        let entity = sim.substrate.entities.get(plane).unwrap();
        assert_eq!(entity.mission.current().known(), Some(MissionType::Patrol));
        assert_eq!(entity.mission.suspended(), MissionId::NONE);
        assert_eq!(entity.attack_target.target(), None);
        assert_eq!(entity.navigation.nav_com(), destination);
        assert!(
            !entity.foot_retarget_after_stop.get(),
            "completed empty Foot scan clears688"
        );
    }
}

#[test]
fn spent_patrol_overrides_enter_and_preserves_suspended_leg() {
    let (mut sim, rules, plane) = scene();
    let dock = sim
        .spawn_object("GAAIRC", "Americans", 4, 4, 0, &rules)
        .unwrap();
    let destination = sim
        .substrate
        .entities
        .get(plane)
        .unwrap()
        .navigation
        .nav_com();
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .ammo_mut()
        .unwrap()
        .set_current_for_test(0);
    super::super::dispatch_mission(&mut sim, plane, &rules, ObjectAiCtx::default());
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.mission.current().known(), Some(MissionType::Enter));
    assert_eq!(
        entity.mission.suspended().known(),
        Some(MissionType::Patrol)
    );
    assert_eq!(
        entity.navigation.nav_com(),
        Some(NavTargetRef::object(dock))
    );
    assert_eq!(entity.mission.handler_state(), 0);
    super::super::enter_idle_mode_for(&mut sim, plane, &rules, None, FrameEffects::default());
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.mission.current().known(), Some(MissionType::Patrol));
    assert_eq!(entity.navigation.nav_com(), destination);
}

#[test]
fn spent_patrol_flies_home_lands_reloads_and_guards() {
    let (mut sim, rules, plane) = scene();
    sim.session.game_options_mut_for_test().short_game = false;
    let dock = sim
        .spawn_object("GAAIRC", "Americans", 4, 4, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .ammo_mut()
        .unwrap()
        .set_current_for_test(0);
    let mut entered = false;
    let mut landed_linked = false;
    let mut reloaded = false;
    let mut transitions = Vec::new();
    for _ in 0..4000 {
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let entity = sim
            .substrate
            .entities
            .get(plane)
            .expect("returning aircraft survives");
        let transition = (entity.mission.current(), entity.mission.suspended());
        if transitions.last() != Some(&transition) {
            transitions.push(transition);
        }
        entered |= entity.mission.current().known() == Some(MissionType::Enter);
        landed_linked |= crate::sim::movement::air_movement::current_fly_height(entity, None) == 0
            && entity.radio_contacts.contains(dock)
            && sim
                .substrate
                .entities
                .get(dock)
                .unwrap()
                .radio_contacts
                .contains(plane);
        if landed_linked
            && entity.ammo().unwrap().current() == 1
            && entity.mission.current().known() == Some(MissionType::Guard)
        {
            reloaded = true;
            break;
        }
    }
    assert!(
        entered && landed_linked && reloaded,
        "Enter, land/contact, reload, Guard: {entered}/{landed_linked}/{reloaded}"
    );
    // Native UnitReload release44C904 calls EnterIdle(0,1), consuming
    // suspended Patrol, then44C910 assigns Guard. The release owner keeps
    // that order; it does not resume the old patrol after refuelling.
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.mission.suspended().known(), None, "{transitions:?}");
}

#[test]
fn patrol_initialization_updates_the_team_through_its_target_owner() {
    use crate::sim::team_script_vm::TeamTarget;
    let (mut sim, rules, plane) = scene();
    let wing = sim
        .spawn_object("ORCA", "Americans", 11, 10, 0, &rules)
        .unwrap();
    let owner = sim.substrate.entities.get(plane).unwrap().owner();
    let script = sim.interner.intern("PATROL-TEAM");
    let team = sim
        .team_script_vm
        .create_team(owner, script, vec![plane, wing], 0);
    let old = TeamTarget::Cell { x: 18, y: 10 };
    sim.team_assign_mission_target(team, Some(old), &rules, None, FrameEffects::default());
    sim.assign_aircraft_destination(
        wing,
        Some(NavTargetRef::cell(18, 10)),
        &rules,
        FrameEffects::default(),
    );
    sim.assign_target_represented(
        wing,
        Some(old.target_kind()),
        &rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .mission
        .set_handler_state(0);
    super::super::dispatch_mission(&mut sim, plane, &rules, ObjectAiCtx::default());
    let wing = sim.substrate.entities.get(wing).unwrap();
    assert_eq!(
        wing.navigation.nav_com(),
        None,
        "changed team target releases the old leg"
    );
    assert_eq!(
        wing.attack_target.target(),
        None,
        "changed team target releases the old aim"
    );
    let plane = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(plane.mission.handler_state(), 1);
    assert_eq!(plane.navigation.nav_com(), Some(NavTargetRef::cell(20, 10)));
}

#[test]
fn team_do_mission_25_reaches_patrol_dispatch_and_enters_idle() {
    use crate::rules::team_ai_ini::TeamAiDefinitionSource;
    use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};
    let (mut sim, rules, plane) = scene();
    sim.assign_aircraft_destination(plane, None, &rules, FrameEffects::default());
    sim.substrate
        .mission_assign_exact(plane, MissionId::from_known(MissionType::Guard), 0)
        .unwrap();
    let owner = sim.substrate.entities.get(plane).unwrap().owner();
    let script = sim.interner.intern("DO-PATROL");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        actions: vec![TeamScriptAction {
            action_id: 11,
            argument: 25,
        }],
        source: TeamAiDefinitionSource::FixedAimd,
    });
    sim.team_script_vm
        .create_team(owner, script, vec![plane], 0);
    sim.run_team_ai_pass(&rules, None, FrameEffects::default());
    // Queue may already commence through its synchronous readiness call.
    sim.substrate.mission_commence_exact(plane, 0).unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(plane)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Patrol)
    );
    super::super::dispatch_mission(&mut sim, plane, &rules, ObjectAiCtx::default());
    assert_ne!(
        sim.substrate
            .entities
            .get(plane)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Patrol)
    );
}

#[test]
fn authored_retail_harrier_patrol_reaches_dispatch_without_a_destination() {
    let Some(bundle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = bundle.rules;
    let mut sim = Simulation::with_seed(31);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let entries = crate::map::entities::parse_map_entities(&IniFile::from_str(
        "[Basic]\nNewINIFormat=4\n[Aircraft]\n0=Americans,ORCA,256,10,10,0,pAtRoL,None,0,-1,1,1\n",
    ));
    assert_eq!(entries[0].mission, Some(MissionType::Patrol));
    assert_eq!(sim.spawn_from_map(&entries, Some(&rules)), 1);
    let plane = sim
        .substrate
        .entities
        .values_sorted()
        .find(|entity| entity.category() == crate::map::entities::EntityCategory::Aircraft)
        .unwrap()
        .stable_id();
    assert_eq!(
        sim.substrate
            .entities
            .get(plane)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::Patrol)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(plane)
            .unwrap()
            .navigation
            .nav_com(),
        None
    );
    sim.object_ai_visit_one(plane, Some(&rules), ObjectAiCtx::default());
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_ne!(entity.mission.current().known(), Some(MissionType::Patrol));
    assert!(
        entity.mission.dispatch_timer().delay() > 0,
        "idle's current mission supplies the Rate epilogue"
    );
}
