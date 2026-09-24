//! Replays `tools/spatial_oracle/aircraft_states.json`: the original
//! Mission_Attack (`0x00417FE0`) visit per state against the pure states.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::docking::aircraft_dock::AircraftAmmo;
use crate::sim::mission::MissionType;
use crate::sim::rng::SimRng;
use crate::sim::world::edge_cell;
use serde_json::Value;

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_states.json"
    ))
    .expect("aircraft_states.json")
}

/// Every combination a stored row stands for: the union of its `covers`
/// products, each merged over the row's own input.
fn combinations(row: &Value) -> Vec<serde_json::Map<String, Value>> {
    let mut all = Vec::new();
    for cover in row["covers"].as_array().unwrap() {
        let mut partial = vec![row["input"].as_object().unwrap().clone()];
        for (axis, values) in cover.as_object().unwrap() {
            partial = partial
                .into_iter()
                .flat_map(|base| {
                    values.as_array().unwrap().iter().map(move |value| {
                        let mut next = base.clone();
                        next.insert(axis.clone(), value.clone());
                        next
                    })
                })
                .collect();
        }
        all.extend(partial);
    }
    all
}

/// A field of the combination, else the payload default.
fn field<'a>(
    input: &'a serde_json::Map<String, Value>,
    defaults: &'a Value,
    key: &str,
) -> &'a Value {
    input.get(key).unwrap_or(&defaults[key])
}

fn code(raw: i64) -> Option<FireError> {
    use FireError::*;
    Some(match raw {
        0 => Ok,
        1 => Ammo,
        2 => Facing,
        3 => Rearm,
        4 => Rotating,
        5 => Illegal,
        6 => Cant,
        7 => Moving,
        8 => Range,
        9 => Cloaked,
        10 => Busy,
        11 => MustDeploy,
        // Outside the table: the default arm, which 4, 7, 10 and 11 reach too.
        _ => return None,
    })
}

fn rules(mission: MissionType, rate: f64) -> RuleSet {
    let section = match mission {
        MissionType::Attack => "Attack",
        MissionType::Guard => "Guard",
        other => panic!("no oracle row uses {other:?}"),
    };
    RuleSet::from_ini(&IniFile::from_str(&format!("[{section}]\nRate={rate}\n")))
        .expect("mission control rules")
}

/// The strike host the oracle stubs: supplied codes and IsClose, a recorded
/// effect log and the production epilogue on the Scenario stream.
struct Replay<'a> {
    code: FireError,
    is_close: bool,
    burst: i64,
    rules: &'a RuleSet,
    mission: MissionType,
    rng: SimRng,
    events: Vec<&'static str>,
    released: bool,
}

impl StrikeHost for Replay<'_> {
    fn fire_error(&mut self) -> FireError {
        self.events.push("fire_error");
        self.code
    }
    fn is_close(&mut self) -> bool {
        self.events.push("is_close");
        self.is_close
    }
    fn face_target(&mut self) {
        self.events.extend(["facing_set", "facing_set"]);
    }
    fn release(&mut self) {
        self.released = true;
        for _ in 0..self.burst.max(0) {
            self.events.push("fire_at");
        }
        self.events.push("scatter");
    }
    fn fire_at(&mut self) {
        self.events.push("fire_at");
    }
    fn scatter(&mut self) {
        self.events.push("scatter");
    }
    fn assign_target_destination(&mut self) {
        self.events.push("assign_dest");
    }
    fn uncloak(&mut self) {
        self.events.push("uncloak");
    }
    fn epilogue(&mut self) -> i32 {
        self.events.push("draw");
        mission_epilogue(self.rules, self.mission, &mut self.rng)
    }
}

/// The oracle's calls, less the reads the facts already hold.
fn expected_events(row: &Value) -> Vec<&'static str> {
    row["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|call| {
            let name = call["call"].as_str().or(call["native"].as_str())?;
            Some(match name {
                "fire_error" => "fire_error",
                "is_close" => "is_close",
                "facing_set" => "facing_set",
                "fire_at" => "fire_at",
                "scatter" => "scatter",
                "assign_dest" => "assign_dest",
                "uncloak" => "uncloak",
                "random_ranged" => "draw",
                "select_weapon" | "get_weapon" | "strafe" | "fighter" => return None,
                other => panic!("unexpected strike call {other}"),
            })
        })
        .collect()
}

fn strike_rows(payload: &Value) -> impl Iterator<Item = &Value> {
    ["states", "extras", "epilogue", "state9_delay"]
        .into_iter()
        .flat_map(|family| payload[family].as_array().unwrap())
        .filter(|row| row["input"]["state"].as_u64().unwrap() != 10)
}

/// Every stored row and each combination it covers: Mission+0xBC, the delay,
/// the latch and pending bytes, Ammo, the ordered effects (queries, facings,
/// shots, Scatters, destinations, Uncloak, draws) and the next Scenario draw.
#[test]
fn original_attack_state_rows() {
    let payload = oracle();
    let defaults = &payload["defaults"];
    let (mut replayed, mut skipped) = (0, 0);
    for row in strike_rows(&payload) {
        let combos = if row.get("covers").is_some() {
            combinations(row)
        } else {
            vec![row["input"].as_object().unwrap().clone()]
        };
        for input in combos {
            let get = |key| field(&input, defaults, key);
            let Some(code) = code(get("code").as_i64().unwrap()) else {
                skipped += 1;
                continue;
            };
            let class = get("class").as_str().unwrap();
            let select = get("select").as_u64().unwrap() as usize;
            let mission = match get("mission").as_i64().unwrap() {
                1 => MissionType::Attack,
                5 => MissionType::Guard,
                other => panic!("mission {other}"),
            };
            let rules = rules(mission, get("rate").as_f64().unwrap());
            let ammo = get("ammo").as_i64().unwrap() as i32;
            let facts = StrikeFacts {
                state: get("state").as_u64().unwrap() as u8,
                target: get("target").as_bool().unwrap(),
                ammo,
                strafe: matches!(class, "strafe" | "strafe_fighter"),
                fighter: matches!(class, "fighter" | "strafe_fighter"),
                curley_shuffle: get("curley").as_i64().unwrap() != 0,
                weapon0_rof: get("rof")[0].as_i64().unwrap() as i32,
                weapon0_range: get("range")[0].as_i64().unwrap() as i32,
                speed: get("speed").as_i64().unwrap() as i32,
            };
            let mut host = Replay {
                code,
                is_close: get("is_close").as_i64().unwrap() != 0,
                burst: get("burst")[select].as_i64().unwrap(),
                rules: &rules,
                mission,
                rng: SimRng::new(get("seed").as_u64().unwrap()),
                events: Vec::new(),
                released: false,
            };
            let visit = strike_visit(&facts, &mut host);
            let name = &input["name"];
            assert_eq!(
                u64::from(visit.state),
                row["state"].as_u64().unwrap(),
                "{name} {input:?}"
            );
            assert_eq!(
                i64::from(visit.delay),
                row["delay"].as_i64().unwrap(),
                "{name}"
            );
            let latch = visit.latch.unwrap_or(get("latch").as_i64().unwrap() != 0);
            assert_eq!(latch, row["latch"].as_i64().unwrap() != 0, "{name}");
            let pending = host.released || get("pending").as_i64().unwrap() != 0;
            assert_eq!(pending, row["pending"].as_i64().unwrap() != 0, "{name}");
            // No strike state writes Ammo.
            assert_eq!(
                row["ammo"].as_i64().unwrap(),
                row["input"]
                    .get("ammo")
                    .unwrap_or(&defaults["ammo"])
                    .as_i64()
                    .unwrap(),
                "{name}"
            );
            assert_eq!(host.events, expected_events(row), "{name} {input:?}");
            assert_eq!(
                u64::from(host.rng.next_u32()),
                row["next_random"].as_u64().unwrap(),
                "{name}"
            );
            replayed += 1;
        }
    }
    // Every executed state 4..9 combination (5472), the extras (21), the
    // epilogue (19) and state-9 (40) rows; code -1 alone is not representable.
    assert_eq!((replayed, skipped), (5552, 456));
}

/// The state-10 host: the production edge picker over the oracle's map, the
/// rest recorded.
struct ExitReplay {
    edge: i64,
    rng: SimRng,
    bounds: PlayfieldBounds,
    events: Vec<String>,
}

impl ExitHost for ExitReplay {
    fn clear_target(&mut self) {
        self.events.push("set_target(null)".into());
    }
    fn assign_edge_destination(&mut self) {
        let edge = edge_cell::Edge::own_edge(self.edge as u8);
        let (rx, ry) =
            edge_cell::find_paradrop_edge_cell(Some(self.bounds), None, edge, &mut self.rng)
                .expect("an edge cell");
        self.events.push(format!("assign_dest({rx},{ry})"));
    }
    fn retreat(&mut self) {
        self.events.push("queue_mission(4,0)".into());
    }
    fn enter_idle_mode(&mut self) {
        self.events.push("enter_idle(0,1)".into());
    }
}

fn expected_exit_events(row: &Value) -> Vec<String> {
    let mut events = Vec::new();
    let mut cell = None;
    for call in row["calls"].as_array().unwrap() {
        match (call["call"].as_str(), call["native"].as_str()) {
            (_, Some("set_target")) => events.push("set_target(null)".into()),
            (_, Some("pick_edge")) => {
                let ret = &call["ret"];
                cell = Some((ret[0].as_i64().unwrap(), ret[1].as_i64().unwrap()));
            }
            (Some("assign_dest"), _) => {
                let (rx, ry) = cell.take().expect("the edge cell precedes its assignment");
                events.push(format!("assign_dest({rx},{ry})"));
            }
            (Some("queue_mission"), _) => events.push("queue_mission(4,0)".into()),
            (Some("enter_idle"), _) => events.push("enter_idle(0,1)".into()),
            _ => {}
        }
    }
    events
}

/// State 10 (`0x00418BEC`) with its prefix, over every covered combination
/// and the seeded edge scans: Mission+0xBC, delay, latch, pending, Ammo, the
/// target clear by house kind, the own-edge cell and its Scenario draws, and
/// the exit call.
#[test]
fn original_state10_rows() {
    let payload = oracle();
    let defaults = &payload["defaults"];
    let map = &payload["map"];
    let local = map["local_size"].as_array().unwrap();
    let bounds = PlayfieldBounds {
        base: map["width"].as_i64().unwrap() as i32,
        off_fc: local[0].as_i64().unwrap() as i32,
        off_100: local[1].as_i64().unwrap() as i32,
        off_104: local[2].as_i64().unwrap() as i32,
        off_108: local[3].as_i64().unwrap() as i32,
    };
    let mut replayed = 0;
    for row in ["state10", "state10_seeds"]
        .into_iter()
        .flat_map(|family| payload[family].as_array().unwrap())
    {
        let combos = if row.get("covers").is_some() {
            combinations(row)
        } else {
            vec![row["input"].as_object().unwrap().clone()]
        };
        for input in combos {
            let get = |key| field(&input, defaults, key);
            let name = &input["name"];
            // The prefix: the latch clears, pending ammo is consumed.
            let mut ammo = AircraftAmmo::new(10);
            ammo.current = get("ammo").as_i64().unwrap() as i32;
            if get("pending").as_i64().unwrap() != 0 {
                ammo.begin_release();
            }
            ammo.consume_release(true);
            let house = crate::sim::house_state::HouseState {
                is_human: get("house_1ec").as_i64().unwrap() != 0,
                player_control: get("house_1ed").as_i64().unwrap() != 0,
                ..crate::sim::house_state::HouseState::new(
                    crate::sim::intern::InternedId::default(),
                    0,
                    None,
                    false,
                    0,
                    1,
                )
            };
            let facts = ExitFacts {
                ammo: ammo.current,
                target: get("target").as_bool().unwrap(),
                leaves_map: get("flag_3d4").as_i64().unwrap() != 0,
                human: house.is_controlled_by_human(get("game_mode").as_i64().unwrap() != 0),
                airstrike: get("airstrike").as_bool().unwrap(),
            };
            let mut host = ExitReplay {
                edge: get("edge").as_i64().unwrap(),
                rng: SimRng::new(get("seed").as_u64().unwrap()),
                bounds,
                events: Vec::new(),
            };
            let visit = exit_visit(&facts, &mut host);
            assert_eq!(
                u64::from(visit.state),
                row["state"].as_u64().unwrap(),
                "{name}"
            );
            assert_eq!(
                i64::from(visit.delay),
                row["delay"].as_i64().unwrap(),
                "{name}"
            );
            // The prefix cleared the latch; the exit writes it again.
            assert_eq!(
                visit.latch.unwrap_or(false),
                row["latch"].as_i64().unwrap() != 0,
                "{name}"
            );
            assert!(!ammo.release_pending());
            assert_eq!(row["pending"].as_i64().unwrap(), 0, "{name}");
            // Ammo is compared as after minus before over a covered group.
            let input_ammo = get("ammo").as_i64().unwrap();
            let row_input_ammo = row["input"]["ammo"].as_i64().unwrap();
            assert_eq!(
                i64::from(ammo.current) - input_ammo,
                row["ammo"].as_i64().unwrap() - row_input_ammo,
                "{name}"
            );
            assert_eq!(host.events, expected_exit_events(row), "{name} {input:?}");
            assert_eq!(
                u64::from(host.rng.next_u32()),
                row["next_random"].as_u64().unwrap(),
                "{name}"
            );
            replayed += 1;
        }
    }
    assert_eq!(replayed, 3092);
}

/// `0x00418B8A..0x00418BB4`, over the corpus's non-faulting inputs.
#[test]
fn state9_delay_matches_the_original_division() {
    let payload = oracle();
    for row in payload["state9_delay"].as_array().unwrap() {
        let input = &row["input"];
        assert_eq!(
            i64::from(state9_delay(
                input["range"][0].as_i64().unwrap() as i32,
                input["speed"].as_i64().unwrap() as i32,
            )),
            row["delay"].as_i64().unwrap(),
            "{}",
            input["name"]
        );
    }
}

/// State 10 through the production dispatch, airborne: an empty Fighter of
/// a human house lets go of its target (`0x00418C21`), takes a cell on its
/// house's own edge as its destination (`PickCellOnEdge 0x004AA440`, one
/// Scenario draw for North) and enters idle mode in the same visit
/// (`vt+0x484`). With no `Dock=` list, Enter_Idle_Mode leaves that
/// destination (`0x0041796A..0x00417978` skips the dock arm), and a computer
/// house keeps its target. A Harrier-like type with an airfield goes home at
/// once instead: Enter_Idle_Mode replaces the edge cell with its dock
/// (`Assign_Destination(NULL, 1)` at `0x004179B4`, then the dock at
/// `0x004179D7`).
#[test]
fn an_empty_fighter_lets_go_heads_for_its_edge_and_idles() {
    use crate::map::entities::EntityCategory;
    use crate::sim::aircraft::AircraftMission;
    use crate::sim::combat::AttackTarget;
    use crate::sim::components::NavTargetRef;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::leaf::MissionLeafState;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::world::Simulation;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AircraftTypes]\n0=ORCA\n1=HARR\n[BuildingTypes]\n0=AIRF\n\
         [ORCA]\nStrength=150\nSpeed=8\nAmmo=1\nFighter=yes\nLandable=yes\nPrimary=Gun\n\
         Locomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n\
         [HARR]\nStrength=150\nSpeed=8\nAmmo=1\nFighter=yes\nLandable=yes\nPrimary=Gun\n\
         AirportBound=yes\nDock=AIRF\nLocomotor={4A582746-9839-11d1-B709-00A024DDAFD1}\n\
         [AIRF]\nStrength=500\nFoundation=2x2\nHelipad=yes\nUnitReload=yes\n\
         [Gun]\nDamage=10\nROF=20\nRange=6\nProjectile=Shell\nWarhead=WH\n\
         [Shell]\nROT=100\nAG=yes\n\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let bounds = PlayfieldBounds {
        base: 64,
        off_fc: 1,
        off_100: 4,
        off_104: 62,
        off_108: 50,
    };
    let exit = |plane_type: &str, human: bool, airfield: bool| {
        let mut sim = Simulation::with_seed(0x418C);
        let mut plane = GameEntity::test_default(1, plane_type, "Americans", 40, 40);
        if airfield {
            let mut pad = GameEntity::test_default(2, "AIRF", "Americans", 20, 40);
            pad.category = EntityCategory::Structure;
            pad.lifecycle.in_limbo = false;
            sim.substrate.entities.insert(pad);
        }
        // After `test_default` interned the names.
        sim.interner = crate::sim::intern::test_interner();
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, human, 0, 1),
        );
        sim.playfield_bounds = Some(bounds);
        plane.category = EntityCategory::Aircraft;
        plane.mission_leaf = MissionLeafState::aircraft_raw_for_test(1, 1, false);
        plane.aircraft_mission = Some(AircraftMission::Attack { sub_state: 10 });
        plane
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: crate::sim::mission::MissionId::from_raw(1),
                suspended: crate::sim::mission::MissionId::NONE,
                queued: crate::sim::mission::MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
        plane.aircraft_ammo = Some(AircraftAmmo::new(1));
        plane.aircraft_ammo.as_mut().unwrap().current = 0;
        plane.attack_target = Some(AttackTarget::for_cell(45, 40));
        plane.locomotor = Some(LocomotorState::from_object_type(
            rules.object(plane_type).unwrap(),
            0,
        ));
        // In flight: a grounded plane takes Enter_Idle_Mode's landed arm.
        plane.locomotor.as_mut().unwrap().altitude =
            crate::util::fixed_math::SimFixed::from_num(1500);
        sim.substrate.entities.insert(plane);
        sim.set_logic_order_for_test(vec![1]);
        let mut expected_rng = sim.scenario_rng.clone();
        let edge = edge_cell::find_paradrop_edge_cell(
            Some(bounds),
            None,
            edge_cell::Edge::North,
            &mut expected_rng,
        )
        .unwrap();
        crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules, None);
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected_rng.logical_state()
        );
        (sim, edge)
    };

    let (sim, edge) = exit("ORCA", true, false);
    let plane = sim.substrate.entities.get(1).unwrap();
    assert!(plane.attack_target.is_none(), "a human house lets go");
    assert_eq!(
        plane.navigation.nav_com,
        Some(NavTargetRef::cell(edge.0, edge.1))
    );
    assert!(
        matches!(plane.aircraft_mission, Some(AircraftMission::Guard)),
        "no airfield: the idle decision holds station"
    );
    assert_eq!(plane.mission_leaf.as_aircraft().unwrap().action_latch(), 0);

    let (sim, _) = exit("ORCA", false, false);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .is_some(),
        "a computer house keeps its target"
    );

    let (sim, edge) = exit("HARR", true, true);
    let plane = sim.substrate.entities.get(1).unwrap();
    assert!(matches!(
        plane.aircraft_mission,
        Some(AircraftMission::ReturnToBase { airfield_id: 2 })
    ));
    assert_eq!(
        plane.navigation.nav_com,
        Some(NavTargetRef::Building { id: 2 }),
        "home to the dock, not to the edge {edge:?}"
    );
    let target = plane.movement_target.as_ref().unwrap();
    assert_eq!(target.final_goal, Some((21, 41)), "Fly heads for the pad");
}

/// The retail inputs the loop reads, through the production reader:
/// `CurleyShuffle=yes`; the Hornet and the Osprey strafe (weapon 0's
/// projectile has ROT 1 and no Inviso) and wait 76 and 59 frames after their
/// last bomb; the Harrier and the Black Eagle are Fighters that do not. The
/// division is the original's (`state9_delay_matches_the_original_division`);
/// the `Speed=` to `Type+0x678` conversion under 76 and 59 is VERA's reading
/// of `TechnoTypeClass::ReadINI` (`0x0071465F`), not executed.
#[test]
fn retail_attack_loop_inputs() {
    let Some((rules_ini, _)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let rules = RuleSet::from_ini(&rules_ini).unwrap();
    assert!(rules.general.curley_shuffle);
    for (name, strafe, fighter, delay) in [
        ("HORNET", true, false, Some(76)),
        ("ASW", true, false, Some(59)),
        ("ORCA", false, true, None),
        ("BEAG", false, true, None),
    ] {
        let object = rules.object(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(
            crate::sim::combat::combat_weapon::aircraft_strafes(&rules, object, 0),
            strafe,
            "{name}"
        );
        assert_eq!(object.fighter, fighter, "{name}");
        if let Some(delay) = delay {
            let weapon = crate::sim::combat::combat_weapon::primary_for_tier(object, 0)
                .and_then(|weapon| rules.weapon(weapon))
                .unwrap();
            assert_eq!(
                state9_delay(
                    weapon.range_leptons,
                    crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed)
                ),
                delay,
                "{name}"
            );
        }
    }
}
