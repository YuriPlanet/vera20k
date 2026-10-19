//! Rust regression tests for the transport script actions 14 (load onto
//! transport), 43 (wait until fully loaded) and 8 (unload), for the team
//! join of a passenger a team's transport lets out, and the native replay of
//! action 9 (deploy), which orders Unload as action 8 does.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::house_state::HouseState;
use crate::sim::passenger::PassengerRole;
use crate::sim::pathfinding::PathGrid;
use crate::sim::team_script_vm::{TeamScriptMember, member_type_identity};
use crate::sim::world::ConcealOutcome;

const RULES: &str = "\
[InfantryTypes]
0=E1
[VehicleTypes]
0=APC
[BuildingTypes]
0=GAPOWR
[E1]
Strength=125
Speed=4
Size=1
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[APC]
Strength=300
Speed=6
ROT=5
Passengers=2
SizeLimit=2
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[GAPOWR]
Strength=750
[Enter]
Recruitable=no
[Unload]
Recruitable=no
";

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    grid: PathGrid,
    owner: InternedId,
}

impl Fixture {
    /// The Russians on a flat arena, with one TeamType, `CARRY`: an APC
    /// (`Passengers=2`) and two E1 running `script`, with
    /// `TransportsReturnOnUnload=` as `returns`. As in retail, Enter and
    /// Unload are not `Recruitable=`, and the TeamType is `Autocreate=` with
    /// `AreTeamMembersRecruitable=no`, so a passenger rejoining it passes
    /// `Can_Add`'s mission and recruit gates.
    fn new(script: &[(i32, i32)], returns: bool) -> Self {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(RULES),
            &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
        )
        .expect("transport action rules");
        let mut sim = Simulation::with_seed(0x6ED200);
        let owner = sim.interner.intern("Russians");
        sim.houses
            .insert(owner, HouseState::new(owner, 1, None, false, 0, 10));
        sim.session.house_order.push(owner);
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let lines: String = script
            .iter()
            .enumerate()
            .map(|(line, (action, argument))| format!("{line}={action},{argument}\n"))
            .collect();
        let aimd = IniFile::from_str(&format!(
            "[TeamTypes]\n0=CARRY\n\
             [CARRY]\nScript=CARRY_S\nTaskForce=CARRY_TF\nTransportsReturnOnUnload={}\n\
             Autocreate=yes\nAreTeamMembersRecruitable=no\n\
             [ScriptTypes]\n0=CARRY_S\n[CARRY_S]\n{lines}\
             [TaskForces]\n0=CARRY_TF\n[CARRY_TF]\n0=1,APC\n1=2,E1\n",
            if returns { "yes" } else { "no" },
        ));
        let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.install_team_ai_registry(&registry, &rules)
            .expect("the test AIMD installs");
        Self {
            sim,
            rules,
            grid,
            owner,
        }
    }

    fn spawn(&mut self, name: &str, rx: u16, ry: u16) -> u64 {
        self.sim
            .spawn_object_at_height(name, "Russians", rx, ry, 0, 0, &self.rules)
            .unwrap_or_else(|| panic!("{name} spawns"))
    }

    /// A `CARRY` team of `members`, taken in TaskForce order (the APC first).
    fn team(&mut self, members: &[u64]) -> u64 {
        let candidates: Vec<TeamScriptMember> = members
            .iter()
            .map(|&entity_id| TeamScriptMember {
                entity_id,
                member_type: member_type_identity(
                    self.sim.substrate.entities.get(entity_id).unwrap(),
                ),
            })
            .collect();
        let team_type = self.sim.interner.intern("CARRY");
        self.sim
            .team_script_vm
            .create_team_from_type(self.owner, team_type, &candidates, 0)
    }

    /// A team without a TeamType of `members` in this order, on `CARRY_S`.
    fn plain_team(&mut self, members: &[u64]) -> u64 {
        let script = self.sim.interner.intern("CARRY_S");
        self.sim
            .team_script_vm
            .create_team(self.owner, script, members.to_vec(), 0)
    }

    /// Put `count` new E1 aboard `transport` as boarding leaves them:
    /// concealed, then at the cargo head.
    fn load(&mut self, transport: u64, count: usize) -> Vec<u64> {
        (0..count)
            .map(|index| {
                let passenger = self.spawn("E1", 2 + index as u16, 2);
                assert_eq!(
                    self.sim
                        .techno_limbo_with_rules(passenger, &self.rules, None),
                    ConcealOutcome::Concealed
                );
                let entity = self.sim.substrate.entities.get_mut(transport).unwrap();
                entity
                    .passenger_role
                    .cargo_mut()
                    .expect("transport cargo")
                    .board_forced_chain(&[(passenger, 1)]);
                self.sim
                    .substrate
                    .entities
                    .get_mut(passenger)
                    .unwrap()
                    .passenger_role = PassengerRole::inside_for_test(transport, false);
                passenger
            })
            .collect()
    }

    fn tick(&mut self) {
        let commands = self.sim.take_due_commands();
        self.sim
            .advance_tick(&commands, Some(&self.rules), Some(&self.grid), None, 33);
    }

    /// Advance until team `team` is past line `line`, at most `frames`.
    fn run_past(&mut self, team: u64, line: i32, frames: usize) {
        for _ in 0..frames {
            if self.cursor(team) > line {
                return;
            }
            self.tick();
        }
    }

    fn cursor(&self, team: u64) -> i32 {
        self.sim
            .team_script_vm
            .team(team)
            .map_or(i32::MIN, |team| team.cursor())
    }

    fn finished(&self, team: u64) -> bool {
        self.sim
            .team_script_vm
            .teams
            .get(&team)
            .is_some_and(|team| team.advance_pending)
    }

    fn members(&self, team: u64) -> Vec<u64> {
        self.sim
            .team_script_vm
            .team(team)
            .map(|team| team.members().collect())
            .unwrap_or_default()
    }

    /// `id`'s first radio contact, which an infantryman's Enter destination
    /// setter HELLOs (`0x0051ACD4`).
    fn contact(&self, id: u64) -> Option<u64> {
        self.sim.substrate.entities.get(id)?.radio_contacts.slot(0)
    }

    fn cargo(&self, transport: u64) -> Vec<u64> {
        self.sim
            .substrate
            .entities
            .get(transport)
            .and_then(|entity| entity.passenger_role.cargo())
            .map(|cargo| cargo.passengers().to_vec())
            .unwrap_or_default()
    }

    fn entity(&self, id: u64) -> &GameEntity {
        self.sim.substrate.entities.get(id).unwrap()
    }

    fn load_action(&mut self, team: u64) {
        self.sim
            .team_action_load_onto_transport(team, &self.rules, None, FrameEffects::default());
    }

    fn wait_action(&mut self, team: u64) {
        self.sim.team_action_wait_until_loaded(team, &self.rules);
    }

    fn unload_action(&mut self, team: u64, argument: i32) {
        self.sim
            .team_action_unload(team, argument, &self.rules, None, FrameEffects::default());
    }
}

/// The mission an object was last ordered: its queued one, else its current.
fn ordered(entity: &GameEntity) -> Option<MissionType> {
    let queued = entity.mission.queued();
    if queued == MissionId::NONE {
        entity.mission.current().known()
    } else {
        queued.known()
    }
}

/// Action 14 (`0x006ED200`): one member is sent aboard (Enter, the
/// transport as its destination), whose destination setter makes the
/// transport his radio contact (`0x0051AC25..0x0051AD17`); while it holds
/// that contact no one else is ordered (`0x006ED2EE..0x006ED2F5`), so the
/// next goes once the first is in and his Limbo broke it. Each passenger
/// leaves the team as it boards (Conceal's Foot `Detach_All`,
/// `0x004D9744`); once only the transport is left action 14 is finished,
/// and action 43 with the transport full.
#[test]
fn the_passengers_are_sent_aboard_one_per_update_and_leave_the_team() {
    let mut f = Fixture::new(&[(14, 0), (43, 0), (2, 0)], false);
    let apc = f.spawn("APC", 16, 10);
    let first = f.spawn("E1", 12, 10);
    let second = f.spawn("E1", 12, 12);
    assert!(crate::sim::passenger::can_enter_transport(
        &f.sim, &f.rules, first, apc,
    ));
    let team = f.team(&[apc, first, second]);
    assert_eq!(f.members(team), [apc, first, second]);

    for _ in 0..5 {
        f.tick();
        if f.contact(first).is_some() {
            break;
        }
    }
    assert_eq!(f.contact(first), Some(apc));
    let entity = f.entity(first);
    assert_eq!(ordered(entity), Some(MissionType::Enter));
    assert_eq!(entity.navigation.nav_com(), Some(NavTargetRef::object(apc)));
    // Further updates while he walks in order no one else.
    for _ in 0..5 {
        f.tick();
    }
    assert!(f.cargo(apc).is_empty(), "the first is still walking in");
    assert_eq!(f.contact(second), None);
    assert_ne!(
        ordered(f.entity(second)),
        Some(MissionType::Enter),
        "no one else is ordered while the transport holds a contact"
    );

    f.run_past(team, 1, 900);
    assert_eq!(f.cursor(team), 2, "actions 14 and 43 are finished");
    assert_eq!(f.members(team), [apc], "the passengers left the team");
    let mut aboard = f.cargo(apc);
    aboard.sort_unstable();
    assert_eq!(aboard, [first, second]);
    assert!(f.sim.team_script_vm.team(team).unwrap().refusal().is_none());
    assert_eq!(f.entity(apc).archive_target(), None);
}

/// While the transport is in radio contact (`0x0065AE30`) action 14 orders
/// no one (`0x006ED2EE..0x006ED2F5`).
#[test]
fn a_transport_in_radio_contact_holds_the_load_order() {
    let mut f = Fixture::new(&[(14, 0)], false);
    let apc = f.spawn("APC", 16, 10);
    let passenger = f.spawn("E1", 12, 10);
    let team = f.team(&[apc, passenger]);
    let contact = f.spawn("E1", 20, 20);
    f.sim
        .substrate
        .entities
        .get_mut(apc)
        .unwrap()
        .radio_contacts
        .insert(contact);
    f.load_action(team);
    assert_eq!(f.contact(passenger), None);
    assert!(!f.finished(team));

    f.sim
        .substrate
        .entities
        .get_mut(apc)
        .unwrap()
        .radio_contacts
        .remove(contact);
    f.load_action(team);
    assert_eq!(f.contact(passenger), Some(apc));
    assert!(!f.finished(team));
}

/// The transport is the first member, in list order, that is a vehicle
/// with a free seat (`+0x114` below `Passengers=`); with none action 14 is
/// finished at once (`0x006ED2B8..0x006ED2CA`). (Any other live joined
/// member is sent aboard, a full transport too.)
#[test]
fn the_transport_is_the_first_vehicle_with_a_free_seat() {
    let mut f = Fixture::new(&[(14, 0)], false);
    let full = f.spawn("APC", 16, 10);
    let passenger = f.spawn("E1", 12, 10);
    let free = f.spawn("APC", 16, 14);
    f.load(full, 2);
    let team = f.plain_team(&[passenger, full, free]);
    f.load_action(team);
    assert_eq!(f.contact(passenger), Some(free));
    assert!(!f.finished(team));

    f.load(free, 2);
    f.load_action(team);
    assert!(f.finished(team), "no transport has a free seat");
}

/// With the transport the only member left (`+0x48` at most 1), action 14
/// is finished; a `TransportsReturnOnUnload=` TeamType's transport first
/// archives the cell holding its Location (`0x006ED43F..0x006ED47C`).
#[test]
fn a_returning_transport_archives_its_loading_cell() {
    for returns in [false, true] {
        let mut f = Fixture::new(&[(14, 0)], returns);
        let apc = f.spawn("APC", 16, 10);
        let team = f.team(&[apc]);
        f.load_action(team);
        assert!(f.finished(team));
        let expected = returns.then_some(crate::sim::combat::TargetKind::Cell(16, 10));
        assert_eq!(f.entity(apc).archive_target(), expected, "{returns}");
    }
}

/// Action 43 (`0x006E9C22`) waits while any member's type has
/// `Passengers=` above its passenger count, and is finished otherwise, also
/// without members.
#[test]
fn wait_until_loaded_waits_for_every_free_seat() {
    let mut f = Fixture::new(&[(43, 0)], false);
    let apc = f.spawn("APC", 16, 10);
    let walker = f.spawn("E1", 12, 10);
    let team = f.plain_team(&[walker, apc]);
    f.load(apc, 1);
    f.wait_action(team);
    assert!(!f.finished(team), "one seat is free");
    f.load(apc, 1);
    f.wait_action(team);
    assert!(f.finished(team), "the APC is full; the walker has no seats");

    let empty = f.plain_team(&[]);
    f.wait_action(empty);
    assert!(f.finished(empty));
}

/// Action 8 (`0x006EF110`) first has each joined member carrying passengers
/// unload: destination and target dropped, Unload queued
/// (`0x006EF27A..0x006EF29C`), unless it already is on Unload or a building
/// stands in its cell (`0x0047C520`); the action waits while any carries one.
#[test]
fn unload_orders_each_loaded_transport_off_buildings() {
    let mut f = Fixture::new(&[(8, 0)], false);
    let apc = f.spawn("APC", 16, 10);
    f.load(apc, 2);
    let team = f.team(&[apc]);
    f.unload_action(team, 0);
    assert!(!f.finished(team));
    let entity = f.entity(apc);
    assert_eq!(ordered(entity), Some(MissionType::Unload));
    assert_eq!(entity.navigation.nav_com(), None);
    assert!(entity.attack_target.target().is_none());

    let parked = f.spawn("APC", 24, 24);
    f.load(parked, 1);
    f.spawn("GAPOWR", 8, 20);
    {
        let entity = f.sim.substrate.entities.get_mut(parked).unwrap();
        entity.position.set_rx_for_test(9);
        entity.position.set_ry_for_test(21);
    }
    let before = ordered(f.entity(parked));
    let team = f.team(&[parked]);
    f.unload_action(team, 0);
    assert!(!f.finished(team), "it still carries a passenger");
    assert_eq!(ordered(f.entity(parked)), before, "no order on a building");
    assert_ne!(before, Some(MissionType::Unload));
}

/// A team transport's Unload lets its passengers out last aboard first, and
/// each rejoins the transport's team at the head of its list
/// (`Add_Member(passenger, 0)`, `0x0073DC0C..0x0073DC19`); once none is left
/// aboard action 8 is finished, and mode 0 keeps everyone in the team.
#[test]
fn unloaded_passengers_rejoin_the_team() {
    let mut f = Fixture::new(&[(8, 0), (2, 0)], false);
    let apc = f.spawn("APC", 16, 10);
    let passengers = f.load(apc, 2);
    let team = f.team(&[apc]);
    f.run_past(team, 0, 600);
    assert_eq!(f.cursor(team), 1, "action 8 is finished");
    assert!(f.cargo(apc).is_empty());
    assert_eq!(f.members(team), [passengers[0], passengers[1], apc]);
    for &passenger in &passengers {
        assert!(!f.entity(passenger).lifecycle.in_limbo());
    }
}

/// Once no member carries passengers, action 8 sorts the members
/// (`0x006EF2BD..0x006EF43F`): a `TransportsReturnOnUnload=` TeamType's
/// transport leaves the team for its ArchiveTarget on Move and drops it;
/// otherwise the argument decides who leaves (1 the others, 2 the
/// transports, 3 both), dropping target and destination.
#[test]
fn unload_argument_decides_who_leaves_the_team() {
    let home = crate::sim::combat::TargetKind::Cell(5, 6);
    for (returns, argument, transport_leaves, others_leave) in [
        (false, 0, false, false),
        (false, 1, false, true),
        (false, 2, true, false),
        (false, 3, true, true),
        (true, 0, true, false),
        (true, 1, true, true),
    ] {
        let case = (returns, argument);
        let mut f = Fixture::new(&[(8, argument)], returns);
        let apc = f.spawn("APC", 16, 10);
        let first = f.spawn("E1", 12, 10);
        let second = f.spawn("E1", 12, 12);
        f.sim
            .substrate
            .entities
            .get_mut(apc)
            .unwrap()
            .set_archive_target(Some(home));
        let team = f.team(&[apc, first, second]);
        f.unload_action(team, argument);
        assert!(f.finished(team), "{case:?}");
        let members = f.members(team);
        assert_eq!(!members.contains(&apc), transport_leaves, "{case:?}");
        assert_eq!(!members.contains(&first), others_leave, "{case:?}");
        assert_eq!(!members.contains(&second), others_leave, "{case:?}");
        let transport = f.entity(apc);
        if returns {
            assert_eq!(
                transport.navigation.nav_com(),
                Some(NavTargetRef::cell(5, 6)),
                "{case:?}"
            );
            assert_eq!(ordered(transport), Some(MissionType::Move), "{case:?}");
            assert_eq!(transport.archive_target(), None, "{case:?}");
        } else {
            assert_eq!(transport.archive_target(), Some(home), "{case:?}");
        }
    }
}

/// The retail rules and AIMD through the production readers, installed in a
/// simulation; `None` without the retail INIs.
fn retail_team_ai() -> Option<(RuleSet, Simulation)> {
    use crate::rules::retail_ini_fixture::retail_ini;
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
    let aimd = retail_ini("aimd.ini").expect("retail AIMD");
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules)
        .expect("retail AIMD installs");
    Some((rules, sim))
}

/// Through the production readers: every retail AIMD TeamType whose script
/// names action 14 runs only ported actions, and none of the 163 TeamTypes'
/// scripts names an unported one.
#[test]
fn the_retail_transport_scripts_run_ported_actions() {
    let Some((_, sim)) = retail_team_ai() else {
        return;
    };
    let vm = &sim.team_script_vm;
    let mut transport_teams = 0;
    let mut first_unported = std::collections::BTreeMap::new();
    for team_type in vm.team_types.values() {
        let actions = &vm.scripts[&team_type.script_id].actions;
        if actions.iter().any(|action| action.action_id == 14) {
            transport_teams += 1;
            assert!(
                actions
                    .iter()
                    .all(|action| action_is_ported(action.action_id))
            );
        }
        if let Some(action) = actions
            .iter()
            .find(|action| !action_is_ported(action.action_id))
        {
            *first_unported.entry(action.action_id).or_insert(0) += 1;
        }
    }
    assert_eq!(vm.team_types.len(), 163);
    assert_eq!(transport_teams, 16);
    assert!(first_unported.is_empty(), "{first_unported:?}");
}

/// Through the production readers: the five retail AIMD TeamTypes whose
/// scripts name action 9 each bring members of one of its arms. The three
/// "MCV" teams carry one MCV that `DeploysInto=` a `ConstructionYard=`
/// building; the two "SChop" teams carry two `IsSimpleDeployer=` Siege
/// Choppers.
#[test]
fn the_retail_deploy_teams_bring_members_action_9_deploys() {
    let Some((rules, sim)) = retail_team_ai() else {
        return;
    };
    let vm = &sim.team_script_vm;
    let mut teams: Vec<(&str, Vec<(&str, i32, bool, bool)>)> = vm
        .team_types
        .values()
        .filter(|team_type| {
            vm.scripts[&team_type.script_id]
                .actions
                .iter()
                .any(|action| action.action_id == 9)
        })
        .map(|team_type| {
            let members = vm.task_forces[&team_type.task_force_id]
                .entries
                .iter()
                .map(|entry| {
                    let name = sim.interner.resolve(entry.member_type.id);
                    let object = rules.object(name).expect("member type");
                    let yard = object
                        .deploys_into
                        .as_deref()
                        .and_then(|building| rules.object(building))
                        .is_some_and(|building| building.construction_yard);
                    (name, entry.count, yard, object.is_simple_deployer)
                })
                .collect();
            (sim.interner.resolve(team_type.id), members)
        })
        .collect();
    teams.sort();
    assert_eq!(
        teams,
        [
            ("043F29AC-G", vec![("AMCV", 1, true, false)]),
            ("043F83EC-G", vec![("SMCV", 1, true, false)]),
            ("043F89AC-G", vec![("PCV", 1, true, false)]),
            ("07C3092C-G", vec![("SCHP", 2, false, true)]),
            ("07C460CC-G", vec![("SCHP", 2, false, true)]),
        ]
    );
}

/// `tools/team_recruit_oracle.py`'s rows of `section`.
fn oracle_rows(section: &str) -> Vec<serde_json::Value> {
    let oracle: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_recruit_oracle.json")).unwrap();
    oracle[section].as_array().unwrap().clone()
}

fn int(value: &serde_json::Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &serde_json::Value) -> bool {
    value.as_bool().unwrap()
}

fn point(value: &serde_json::Value) -> [i32; 3] {
    [int(&value[0]), int(&value[1]), int(&value[2])]
}

/// The oracle rows' types: one per class and `Passengers=` they use, and
/// the deploy rows' deployers with the building `UD` and `UDS` deploy into.
const ORACLE_RULES: &str = "\
[InfantryTypes]
0=I0
1=I2
2=ID
[VehicleTypes]
0=U0
1=U1
2=U2
3=U5
4=UN
5=UD
6=US
7=UDS
[AircraftTypes]
0=A0
1=A2
[BuildingTypes]
0=BD
[Clear]
Buildable=yes
[ID]
Strength=100
Speed=4
Deployer=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[UD]
Strength=300
Speed=6
DeploysInto=BD
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[US]
Strength=300
Speed=6
IsSimpleDeployer=yes
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[UDS]
Strength=300
Speed=6
DeploysInto=BD
IsSimpleDeployer=yes
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[BD]
Strength=1000
[I0]
Strength=100
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[I2]
Strength=100
Speed=4
Passengers=2
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[U0]
Strength=300
Speed=6
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[U1]
Strength=300
Speed=6
Passengers=1
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[U2]
Strength=300
Speed=6
Passengers=2
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[U5]
Strength=300
Speed=6
Passengers=5
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[UN]
Strength=300
Speed=6
Passengers=-1
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[A0]
Strength=100
Speed=12
Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}
[A2]
Strength=100
Speed=12
Passengers=2
Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}
";

/// `BD`'s foundation: a member's cell and the one east of it.
const ORACLE_ART: &str = "[BD]\nFoundation=2x1\n";

/// A deploy row member's type, by its `DeploysInto=`, `IsSimpleDeployer=`
/// and `Deployer=`.
fn deploy_type(member: &serde_json::Value) -> &'static str {
    let class = member["class"].as_str().unwrap();
    match (class, flag(&member["deploys"]), flag(&member["simple"])) {
        ("unit", false, false) => "U0",
        ("unit", true, false) => "UD",
        ("unit", false, true) => "US",
        ("unit", true, true) => "UDS",
        ("infantry", ..) if flag(&member["deployer"]) => "ID",
        _ => oracle_type(class, 0),
    }
}

fn oracle_type(class: &str, passengers: i32) -> &'static str {
    match (class, passengers) {
        ("infantry", 0) => "I0",
        ("infantry", 2) => "I2",
        ("unit", 0) => "U0",
        ("unit", 1) => "U1",
        ("unit", 2) => "U2",
        ("unit", 5) => "U5",
        ("unit", -1) => "UN",
        ("aircraft", 0) => "A0",
        ("aircraft", 2) => "A2",
        other => panic!("no oracle type for {other:?}"),
    }
}

/// The archive target a row's `archive` member holds before the action.
const ORACLE_ARCHIVE: crate::sim::combat::TargetKind = crate::sim::combat::TargetKind::Cell(3, 3);

/// A row's team. Each member is spawned off to the side, then given the
/// row's Location, live bytes, passenger count, NavCom, mission, radio
/// contact, archive target and building; the zone is an infantryman at the
/// row's location; the team runs TeamType `ROW` (the row's TaskForce
/// entries and `TransportsReturnOnUnload=`) on a script of `action` alone,
/// under the row's `Stray=`. Action 9's objects are spawned on their own
/// cells instead, which a building's foundation is tested from, and a
/// member whose placement the row refuses has a `BD` east of it.
struct OracleTeam {
    sim: Simulation,
    rules: RuleSet,
    team: u64,
    members: Vec<u64>,
    zone: u64,
}

impl OracleTeam {
    fn new(row: &serde_json::Value, action: i32) -> Self {
        let entries: String = row
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .enumerate()
                    .map(|(slot, entry)| {
                        let name = oracle_type(entry[0].as_str().unwrap(), int(&entry[1]));
                        format!("{slot}=1,{name}\n")
                    })
                    .collect()
            })
            .unwrap_or_default();
        let returns = row.get("returns").is_some_and(flag);
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(ORACLE_RULES),
            &IniFile::from_str(ORACLE_ART),
        )
        .expect("oracle rules");
        if let Some(stray) = row.get("stray") {
            rules.general.stray = int(stray);
        }
        let mut sim = Simulation::with_seed(0x6EF110);
        let owner = sim.interner.intern("Russians");
        sim.houses
            .insert(owner, HouseState::new(owner, 1, None, false, 0, 10));
        sim.session.house_order.push(owner);
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let aimd = IniFile::from_str(&format!(
            "[TeamTypes]\n0=ROW\n\
             [ROW]\nScript=ROW_S\nTaskForce=ROW_TF\nTransportsReturnOnUnload={}\n\
             [ScriptTypes]\n0=ROW_S\n[ROW_S]\n0={action},0\n\
             [TaskForces]\n0=ROW_TF\n[ROW_TF]\n{entries}",
            if returns { "yes" } else { "no" },
        ));
        let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.install_team_ai_registry(&registry, &rules)
            .expect("the row's AIMD installs");
        let deploy = action == 9;
        let place = |sim: &mut Simulation, name: &str, slot: u16, [x, y, z]: [i32; 3]| {
            let (rx, ry) = if deploy {
                ((x / 256) as u16, (y / 256) as u16)
            } else {
                (1 + 2 * slot, 30)
            };
            let id = sim
                .spawn_object_at_height(name, "Russians", rx, ry, 0, 0, &rules)
                .unwrap_or_else(|| panic!("{name} spawns"));
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity
                .position
                .put_location(crate::sim::components::DriveCoord { x, y, z });
            id
        };
        let zone = place(
            &mut sim,
            "I0",
            15,
            row.get("zone").map_or([0x80, 0x80, 0], point),
        );
        assert_eq!(
            sim.substrate.entities.get(zone).unwrap().category(),
            EntityCategory::Infantry,
            "the radio rows' contact is an infantryman"
        );
        let mut members = Vec::new();
        for (slot, member) in row["members"].as_array().unwrap().iter().enumerate() {
            let passengers = int(&member["passengers"]);
            let name = if deploy {
                deploy_type(member)
            } else {
                oracle_type(member["class"].as_str().unwrap(), passengers)
            };
            let [x, y, z] = point(&member["xyz"]);
            let id = place(&mut sim, name, slot as u16, [x, y, z]);
            if deploy && flag(&member["deploys"]) && !flag(&member["can_place"]) {
                sim.spawn_object(
                    "BD",
                    "Russians",
                    (x / 256 + 1) as u16,
                    (y / 256) as u16,
                    0,
                    &rules,
                )
                .expect("the blocking building spawns");
            }
            if flag(&member["on_building"]) {
                sim.substrate.occupancy.add(
                    (x / 256) as u16,
                    (y / 256) as u16,
                    u64::MAX - slot as u64,
                    MovementLayer::Ground,
                    None,
                    crate::sim::occupancy::CellListInsertion::AppendBuilding,
                );
            }
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity
                .lifecycle
                .set_object_alive_for_test(flag(&member["active"]));
            entity.health.set_for_test(int(&member["health"]));
            entity
                .lifecycle
                .set_in_limbo_for_test(flag(&member["limbo"]));
            let count = usize::try_from(int(&member["count"])).unwrap();
            if count > 0 {
                let mut cargo = crate::sim::passenger::PassengerCargo::new(
                    u32::try_from(passengers.max(0)).unwrap(),
                    0,
                );
                cargo.set_passengers_for_test(vec![u64::MAX; count]);
                cargo.set_passenger_sizes_for_test(vec![1; count]);
                entity.passenger_role = PassengerRole::transport(cargo);
            }
            entity
                .navigation
                .set_nav_com_for_test(flag(&member["nav"]).then(|| NavTargetRef::cell(1, 1)));
            entity
                .mission
                .set_current_for_test(MissionId::from_raw(int(&member["mission"])), 0);
            if flag(&member["radio"]) {
                entity.radio_contacts.insert(zone);
            }
            entity.set_archive_target(flag(&member["archive"]).then_some(ORACLE_ARCHIVE));
            entity
                .attack_target
                .set_for_test(Some(crate::sim::combat::TargetKind::Cell(2, 2)));
            members.push(id);
        }
        let script = sim.interner.intern("ROW_S");
        let team = sim
            .team_script_vm
            .create_team(owner, script, members.clone(), 0);
        let team_type = sim.interner.intern("ROW");
        let task_force = sim.interner.intern("ROW_TF");
        let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
        state.team_type_id = Some(team_type);
        state.task_force_id = Some(task_force);
        state.zone = Some(TeamTarget::Object(zone));
        for (record, member) in state
            .members
            .iter_mut()
            .zip(row["members"].as_array().unwrap())
        {
            record.initiated = flag(&member["initiated"]);
        }
        Self {
            sim,
            rules,
            team,
            members,
            zone,
        }
    }

    fn finished(&self) -> bool {
        self.sim.team_script_vm.teams[&self.team].advance_pending
    }

    /// Each member as the row's events leave it: its NavCom, its target
    /// (each starts with one), its last ordered mission and whether it
    /// commenced, its team membership and +0x689, and its archive target.
    /// Remove_Member is a recording stub in the oracle, so its own effects
    /// are added from its port (`team_remove_member`, `0x006EA870`): once no
    /// initiated member is left, the head is initiated, and a removed
    /// infantryman's or aircraft's later orders, which its idle mode decides
    /// (an archive in another cell sends an infantryman there; an aircraft's
    /// Retreat refuses the Move after it), are not compared.
    fn check(&self, row: &serde_json::Value, case: usize, loading: bool) {
        let state = &self.sim.team_script_vm.teams[&self.team];
        assert_eq!(
            self.finished(),
            int(&row["finished"]) != 0,
            "case {case}: finished"
        );
        let count = self.members.len();
        let (mut mission, mut destination, mut removed) =
            (vec![None; count], vec![None; count], vec![false; count]);
        let (mut untargeted, mut commenced) = (vec![false; count], vec![false; count]);
        let mut cells = Vec::new();
        let slot = |value: &serde_json::Value| usize::try_from(value.as_u64().unwrap()).unwrap();
        for event in row["events"].as_array().unwrap() {
            match event[0].as_str().unwrap() {
                "queue_mission" => mission[slot(&event[1])] = Some(int(&event[2])),
                "assign_destination" => destination[slot(&event[1])] = Some(event[2].clone()),
                "remove_member" => removed[slot(&event[1])] = true,
                "set_target" => {
                    assert_eq!(int(&event[2]), 0, "case {case}: only Assign_Target(NULL)");
                    untargeted[slot(&event[1])] = true;
                }
                "commence" => commenced[slot(&event[1])] = true,
                "cell_at" => cells.push(point(&event[1])),
                _ => {}
            }
        }
        let initiated = |index: usize| int(&row["initiated"][index]) != 0;
        let remaining: Vec<usize> = (0..count).filter(|&index| !removed[index]).collect();
        let head_initiated =
            remaining.len() < count && !remaining.iter().any(|&index| initiated(index));
        for (index, &id) in self.members.iter().enumerate() {
            let member = &row["members"][index];
            let at = format!("case {case} member {index}");
            let entity = self.sim.substrate.entities.get(id).unwrap();
            let record = state.members.iter().find(|record| record.id == id);
            assert_eq!(record.is_none(), removed[index], "{at}: removed");
            if let Some(record) = record {
                assert_eq!(
                    record.initiated,
                    initiated(index) || (head_initiated && remaining.first() == Some(&index)),
                    "{at}: +0x689"
                );
            }
            let skip_orders = removed[index]
                && matches!(
                    entity.category(),
                    EntityCategory::Infantry | EntityCategory::Aircraft
                );
            // The oracle records Assign_Destination's argument, not the
            // class setter body, so the setter's two effects are compared
            // as the member's state shows them. Unit741AD1..741B22: a
            // transport (`Passengers=` above 0) whose first contact is an
            // infantryman breaks it when sent to a Techno it holds no
            // contact with. A member's only contact here is the zone's
            // infantryman and no member is one, so a transport sent to a
            // member must have lost it and every other contact must stand;
            // the setter's other gates have their own controls
            // (`gunner_tests`).
            let had_contact = flag(&member["radio"]);
            let sent_to_member = destination[index]
                .as_ref()
                .and_then(serde_json::Value::as_u64)
                .is_some();
            let breaks = had_contact
                && member["class"] == "unit"
                && int(&member["passengers"]) > 0
                && sent_to_member;
            assert!(
                entity
                    .radio_contacts
                    .iter_live()
                    .all(|contact| contact == self.zone),
                "{at}: the zone's infantryman is the only contact"
            );
            assert_eq!(
                entity.radio_contacts.contains(self.zone),
                had_contact && !breaks,
                "{at}: contact with the zone"
            );
            // Unit741C4F..741CCC then parks an Enter target that is a Unit
            // in +500 and clears the archive and NavCom, when no contact is
            // left; movement and boarding integration is covered by
            // transport_unload.
            let pending_entry = if member["class"] == "unit"
                && entity.radio_contacts.is_empty()
                && mission[index] == Some(MissionType::Enter as i32)
            {
                destination[index]
                    .as_ref()
                    .and_then(serde_json::Value::as_u64)
                    .map(|target| target as usize)
                    .filter(|&target| row["members"][target]["class"] == "unit")
                    .map(|target| self.members[target])
            } else {
                None
            };
            if !skip_orders {
                let nav = match &destination[index] {
                    None => flag(&member["nav"]).then(|| NavTargetRef::cell(1, 1)),
                    Some(serde_json::Value::Null) => None,
                    Some(serde_json::Value::String(target)) if target == "zone" => {
                        Some(NavTargetRef::object(self.zone))
                    }
                    Some(serde_json::Value::String(target)) if target == "cell" => {
                        Some(NavTargetRef::cell(3, 3))
                    }
                    Some(target) => Some(NavTargetRef::object(self.members[slot(target)])),
                };
                assert_eq!(entity.pending_entry(), pending_entry, "{at}: pending entry");
                assert_eq!(
                    entity.navigation.nav_com(),
                    nav.filter(|_| pending_entry.is_none()),
                    "{at}: NavCom"
                );
                assert_eq!(
                    entity.attack_target.target(),
                    (!untargeted[index]).then_some(crate::sim::combat::TargetKind::Cell(2, 2)),
                    "{at}: target"
                );
                // Ready_To_Commence answers from the row in the oracle and from
                // the member's state here, so only a commence is compared.
                if commenced[index] {
                    assert_eq!(
                        (entity.mission.current(), entity.mission.queued()),
                        (
                            MissionId::from_raw(mission[index].unwrap()),
                            MissionId::NONE
                        ),
                        "{at}: commenced"
                    );
                }
                match mission[index] {
                    Some(mission) => assert_eq!(
                        ordered(entity),
                        MissionId::from_raw(mission).known(),
                        "{at}: mission"
                    ),
                    None if !removed[index] => {
                        assert_eq!(entity.mission.queued(), MissionId::NONE, "{at}: no order")
                    }
                    None => {}
                }
            }
            let [x, y, z] = point(&member["xyz"]);
            let archive = row["archive"][index]
                .as_str()
                .filter(|_| pending_entry.is_none())
                .map(|_| {
                    if loading && cells.contains(&[x, y, z]) {
                        crate::sim::combat::TargetKind::Cell((x / 256) as u16, (y / 256) as u16)
                    } else {
                        ORACLE_ARCHIVE
                    }
                });
            assert_eq!(entity.archive_target(), archive, "{at}: archive");
        }
    }
}

/// Action 14 against `tools/team_recruit_oracle.py`'s `load` rows, which run
/// the original `0x006ED200` over synthetic teams: the transport pick, the
/// radio wait, the join-up, the one Enter order, the completion and the
/// archive.
#[test]
fn load_onto_transport_matches_the_original() {
    for (case, row) in oracle_rows("load").iter().enumerate() {
        let mut team = OracleTeam::new(row, 14);
        team.sim.team_action_load_onto_transport(
            team.team,
            &team.rules,
            None,
            FrameEffects::default(),
        );
        team.check(row, case, true);
    }
}

/// Action 43 against the `wait_loaded` rows (`0x006E9C22..0x006E9C5C`).
#[test]
fn wait_until_loaded_matches_the_original() {
    for (case, row) in oracle_rows("wait_loaded").iter().enumerate() {
        let mut team = OracleTeam::new(row, 43);
        team.sim
            .team_action_wait_until_loaded(team.team, &team.rules);
        assert_eq!(team.finished(), flag(&row["finished"]), "case {case}");
    }
}

/// Action 8 against the `unload` rows (`0x006EF110`): the join-up, the
/// Unload orders and their tests, and the sort by argument and
/// `TransportsReturnOnUnload=`.
#[test]
fn unload_matches_the_original() {
    for (case, row) in oracle_rows("unload").iter().enumerate() {
        let mut team = OracleTeam::new(row, 8);
        team.sim.team_action_unload(
            team.team,
            int(&row["argument"]),
            &team.rules,
            None,
            FrameEffects::default(),
        );
        team.check(row, case, false);
    }
}

/// Action 9 against the `deploy` rows (`0x006ED4D0`): the join-up, a
/// `DeploysInto=` unit's Unload test, its placement question with its own
/// cell as the origin, then the site flush for its house or the orders, the
/// simple and infantry deployers' Unload orders, and the completion. Each
/// member Mark lifted is put back in its cell. The rows' placement answers
/// hold here by the `BD` beside a refused member; Flush_For_Placement is a
/// recording stub in the oracle, and that building ends its scan here.
#[test]
fn deploy_matches_the_original() {
    for (case, row) in oracle_rows("deploy").iter().enumerate() {
        let mut team = OracleTeam::new(row, 9);
        team.sim
            .team_action_deploy(team.team, &team.rules, None, FrameEffects::default());
        team.check(row, case, false);
        let owner = team.sim.interner.get("Russians");
        for event in row["events"].as_array().unwrap() {
            let kind = event[0].as_str().unwrap();
            if !matches!(kind, "mark" | "can_place" | "flush") {
                continue;
            }
            let index = usize::try_from(event[1].as_u64().unwrap()).unwrap();
            let id = team.members[index];
            let entity = team.sim.substrate.entities.get(id).unwrap();
            let at = format!("case {case} member {index}");
            let cell = [
                i64::from(entity.position.rx()),
                i64::from(entity.position.ry()),
            ];
            match kind {
                "mark" => assert!(
                    entity.lifecycle.cell_marked()
                        && team.sim.substrate.occupancy.contains_entity(
                            entity.position.rx(),
                            entity.position.ry(),
                            id
                        ),
                    "{at}: put back"
                ),
                "can_place" => {
                    assert_eq!(event[2], serde_json::json!(cell), "{at}: origin");
                    assert_eq!(int(&event[3]), 0, "{at}: for no house");
                }
                _ => {
                    assert_eq!(event[2], serde_json::json!(cell), "{at}: flush origin");
                    assert_eq!(event[3], "house", "{at}: flushed for its house");
                    assert_eq!(Some(entity.owner()), owner, "{at}: its house");
                }
            }
        }
    }
}
