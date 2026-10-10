//! The Spy Plane (`spy_plane`, `aircraft::spyplane_mission`): its native
//! comparisons against `tools/superweapon_oracle.json` (`spy_plane_launch`,
//! `send_spy_planes`, `spyplane_missions`; `--check` regenerates them), and
//! flights on retail rules through production frames.

use super::chronosphere_tests::{charge_super, click, retail_rules_binding, step, world_with};
use crate::map::playfield::PlayfieldBounds;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::NavTargetRef;
use crate::sim::intern::InternedId;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::edge_cell::{Edge, find_paradrop_edge_cell};
use crate::sim::world::{PlacementEvidence, SimSoundEvent, Simulation};
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

const SPY_PLANE: &str = "SpyPlaneSpecial";
const TARGET: (u16, u16) = (40, 40);
const FLY: &str = "{4A582746-9839-11D1-B709-00A024DDAFD1}";

pub(super) fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
}

pub(super) fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

pub(super) fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

pub(super) fn flag(row: &Value, key: &str) -> bool {
    row[key].as_bool().unwrap()
}

/// The row's events named `name`, in order.
pub(super) fn events<'a>(row: &'a Value, name: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(move |event| event[0] == name)
}

/// A replay's rules: SPYP as retail authors it, with the row's camera keys
/// and weapon.
struct Keys {
    /// `AllyParaDropInf=`/`AllyParaDropNum=` entries.
    planes: usize,
    /// Whether `[AircraftTypes]` names SPYP.
    known: bool,
    /// `SpyCameraWeapon`'s `Range=`, in leptons, and `Damage=`.
    range: i32,
    damage: i32,
    /// `SpyPlaneCamera=` (else absent) and `SpyPlaneCameraFrames=`.
    camera: bool,
    frames: i32,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            planes: 1,
            known: true,
            range: 5120,
            damage: 6,
            camera: true,
            frames: 12,
        }
    }
}

fn rules(keys: &Keys) -> RuleSet {
    let infantry = vec!["E1"; keys.planes].join(",");
    let counts = vec!["1"; keys.planes].join(",");
    let aircraft = if keys.known { "0=SPYP\n" } else { "" };
    let camera = if keys.camera {
        "SpyPlaneCamera=SpyPlaneSnapshot\n"
    } else {
        ""
    };
    let ini = IniFile::from_str(&format!(
        "[General]\nAllyParaDropInf={infantry}\nAllyParaDropNum={counts}\nFlightLevel=1500\n\
         [AudioVisual]\n{camera}SpyPlaneCameraFrames={frames}\n\
         [InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n{aircraft}[BuildingTypes]\n\
         [SuperWeaponTypes]\n0={SPY_PLANE}\n[{SPY_PLANE}]\nType=SpyPlane\n[E1]\nStrength=125\n\
         [SPYP]\nStrength=200\nSpeed=15\nROT=2\nFlyBy=yes\nLandable=no\nSelectable=no\n\
         Sight=0\nPrimary=SpyCameraWeapon\nLocomotor={FLY}\n\
         [SpyCameraWeapon]\nDamage={damage}\nRange={range}\nROF=10\n",
        frames = keys.frames,
        damage = keys.damage,
        range = f64::from(keys.range) / 256.0,
    ));
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds("[SoundList]\n0=SpyPlaneSnapshot\n"));
    rules
}

fn sounds(ini: &str) -> SoundRegistry {
    SoundRegistry::from_ini(&IniFile::from_str(ini))
}

/// Retail rules with their `[AudioVisual]` sounds resolved against retail
/// SOUNDMD, as the production rules owner binds them.
fn retail_rules() -> Option<RuleSet> {
    let mut rules = retail_rules_binding(&[])?;
    let (rules_ini, _) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    let soundmd = crate::rules::retail_ini_fixture::retail_ini("soundmd.ini")?;
    rules.bind_type_sound_references(&rules_ini, &SoundRegistry::from_ini(&soundmd));
    Some(rules)
}

/// `rules` on a flat 64-cell map whose `Size=` is 32 by 32 with LocalSize
/// 2,4,28,22: the North edge runs along cells whose coordinates sum to 36,
/// the diamond ends at 96, and the target (40, 40) is well inside.
pub(super) fn world(rules: RuleSet) -> (RuleSet, Simulation, InternedId) {
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    sim.playfield_bounds = Some(PlayfieldBounds::from_raw_local_size(32, 32, [2, 4, 28, 22]));
    sim.playfield_size_height = Some(32);
    sim.fog.width = 64;
    sim.fog.height = 64;
    (rules, sim, americans)
}

/// Bind the original reader's authored Edge control through the same map and
/// House owners as campaign loading. The requested edge is supplied by the
/// original send-body corpus; its textual input comes from ReadEdge475980.
pub(super) fn install_campaign_edge(sim: &mut Simulation, owner: InternedId, edge: i32) {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/campaign_start_houses.json",
    ))
    .unwrap();
    let control = native["edge_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["default"] == -1 && row["result"] == edge)
        .expect("original Edge control");
    let name = sim.interner.resolve(owner);
    let mut houses = IniSection::new("Houses".to_owned());
    houses.set("0", name);
    let mut body = IniSection::new(name.to_owned());
    if let Some(value) = control["sections"]["Control"]["Edge"].as_str() {
        body.set("Edge", value);
    }
    let ini = IniFile::from_sections_for_test([houses, body]);
    let roster = crate::map::houses::parse_house_roster(&ini, &[], None);
    let read = roster.houses[0].read_scenario_parameters(&ini, 1);
    let house = sim.houses.get_mut(&owner).unwrap();
    house.initialize_scenario_parameters(read, None, None);
    assert_eq!(house.authored_edge(), edge);
}

fn spy_planes(sim: &Simulation) -> Vec<u64> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == "SPYP" && !entity.dying)
        .map(|entity| entity.stable_id())
        .collect()
}

/// The camera's sounds: `SpyPlaneCamera=` bound to its SOUNDMD entry, which
/// retail spells `SpyPlaneSnapShot` (`VocClass::FindIndex` ignores case).
fn camera_sounds(sim: &Simulation) -> usize {
    sim.sound_events
        .iter()
        .filter(|event| {
            matches!(event, SimSoundEvent::VocAt { sound_id, .. }
                if sound_id.eq_ignore_ascii_case("SpyPlaneSnapshot"))
        })
        .count()
}

/// The edge an oracle event names (`Edge` order: North, East, South, West).
pub(super) fn edge(event: &Value) -> Edge {
    Edge::from_index(u8::try_from(int(&event[1])).unwrap()).unwrap()
}

/// The cell `find_paradrop_edge_cell` picks on `edge` from the current
/// Scenario stream, `draws` words in.
pub(super) fn expected_pick(sim: &Simulation, edge: Edge, draws: usize) -> (u16, u16) {
    let mut rng = sim.scenario_rng.clone();
    for _ in 0..draws {
        rng.next_u32();
    }
    find_paradrop_edge_cell(
        sim.playfield_bounds,
        sim.resolved_terrain.as_ref(),
        edge,
        &mut rng,
    )
    .unwrap()
}

/// Launch case 8: one plane per AllyParaDrop entry, each taking the clicked
/// cell as its target, only for a charged Super, a known SPYP and a real
/// cell. The player's launch drops the queued `EVA_SpyPlaneReady`
/// whatever was sent; VERA reports every launch and the app drops the line
/// for its player (`launch_drops_ready_line`).
#[test]
fn launch_case_eight_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "spy_plane_launch");
    assert!(rows.len() > 10);
    for row in rows {
        let sent = events(row, "send_spy_planes").count();
        if !flag(row, "charged") {
            // ClickFire admits only a charged Super; case 8 checks again.
            assert_eq!(row["events"], json!([]), "{row}");
            continue;
        }
        assert_eq!(
            events(row, "find_aircraft_type").next(),
            Some(&json!(["find_aircraft_type", "SPYP"]))
        );
        let eva: Vec<Value> = events(row, "vox_find").cloned().collect();
        let expected = if flag(row, "player") {
            vec![json!(["vox_find", "EVA_SpyPlaneReady"])]
        } else {
            vec![]
        };
        assert_eq!(eva, expected, "{row}");
        let (infantry, counts) = (int(&row["counts"][0]), int(&row["counts"][1]));
        if infantry != counts || infantry == 0 {
            // The plane-count RESIDUAL in `spy_plane`'s module doc.
            assert_eq!(sent, 0, "{row}");
            continue;
        }
        for event in events(row, "send_spy_planes") {
            // One plane each, Mission_SpyplaneApproach, the clicked cell as
            // its target, no destination.
            assert_eq!(
                event,
                &json!(["send_spy_planes", 5, 1, 30, "target", "null"])
            );
        }
        let keys = Keys {
            planes: infantry as usize,
            known: int(&row["type_index"]) != -1,
            ..Keys::default()
        };
        let (rules, mut sim, americans) = world(rules(&keys));
        let cell = if row["cell"] == "real" {
            TARGET
        } else {
            (200, 200)
        };
        let sw_type = sim.interner.intern(SPY_PLANE);
        let launched = super::spy_plane::launch(
            &mut sim,
            &rules,
            americans,
            sw_type,
            cell,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(spy_planes(&sim).len(), sent, "{row}");
        assert_eq!(launched, sent > 0, "{row}");
        let reports = sim
            .sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }))
            .count();
        assert_eq!(reports, 1, "{row}");
    }
}

/// SendSpyPlanes with case 8's arguments: the plane is mission-only before
/// its edge is picked (the house's own edge, `GetEdge`), queues
/// Mission_SpyplaneApproach, targets the clicked cell, Unlimbos at the picked
/// cell's centre facing North inside the ScenarioInit bracket, and starts
/// its mission at once.
#[test]
fn send_spy_planes_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "send_spy_planes");
    assert!(rows.len() > 8);
    let mut authored_replayed = 0;
    for row in rows {
        let events: Vec<&Value> = row["events"].as_array().unwrap().iter().collect();
        let created = events.iter().find(|event| event[0] == "create").unwrap();
        // CreateObject runs inside the ScenarioInit bracket.
        assert_eq!(created[1], 1, "{row}");
        if created[2] == false {
            // No object: nothing else runs, so no pick draws.
            assert_eq!(events.len(), 1, "{row}");
            continue;
        }
        let names: Vec<&str> = events
            .iter()
            .map(|event| event[0].as_str().unwrap())
            .collect();
        let tail = if int(&row["returned"]) == 1 {
            "commence"
        } else {
            "delete"
        };
        assert_eq!(
            names,
            [
                "create",
                "pick_cell_on_edge",
                "queue",
                "target",
                "unlimbo",
                tail
            ],
            "{row}"
        );
        let pick = events[1];
        assert_eq!(pick[2], "0xb04c38");
        assert_eq!(
            pick.as_array().unwrap()[4..],
            [json!(4), json!(1), json!(0), json!([1])]
        );
        assert_eq!(events[2], &json!(["queue", 0, 30, 0]));
        assert_eq!(events[3], &json!(["target", 0, "target"]));
        // Unlimbo at the picked cell's centre, height 0, facing North,
        // ScenarioInit raised again.
        let unlimbo = events[4];
        let requested = &row["picks"][0];
        let centre = |axis: usize| int(&requested[axis]) * 256 + 128;
        assert_eq!(unlimbo[2], json!([centre(0), centre(1), 0]), "{row}");
        assert_eq!(unlimbo[3], 0, "{row}");
        assert_eq!(unlimbo[4], 1, "{row}");
        if tail == "delete" {
            // A refused Unlimbo deletes the plane.
            continue;
        }
        let edge_field = int(&row["edge"]);
        let waypoint_edge = int(&row["waypoint_edge"]);
        let own = edge(pick);

        let (rules, mut sim, americans) = world(rules(&Keys::default()));
        if (0..=3).contains(&edge_field) {
            install_campaign_edge(&mut sim, americans, edge_field);
            authored_replayed += 1;
        } else {
            assert_eq!(
                own,
                Edge::own_edge(u8::try_from(waypoint_edge).unwrap_or(u8::MAX)),
                "{row}"
            );
        }
        sim.houses.get_mut(&americans).unwrap().waypoint_edge =
            u8::try_from(waypoint_edge).unwrap_or(u8::MAX);
        // The plane's constructor draws one word before the pick.
        let expected = expected_pick(&sim, own, 1);
        let sw_type = sim.interner.intern(SPY_PLANE);
        assert!(super::spy_plane::launch(
            &mut sim,
            &rules,
            americans,
            sw_type,
            TARGET,
            crate::sim::world::FrameEffects::default()
        ));
        let [id] = spy_planes(&sim)[..] else {
            panic!("one plane");
        };
        let plane = sim.substrate.entities.get(id).unwrap();
        assert!(plane.is_mission_only());
        assert_eq!(
            plane.mission.current(),
            MissionId::from_known(MissionType::SpyplaneApproach)
        );
        assert_eq!(plane.mission.queued(), MissionId::NONE);
        assert_eq!(
            plane.attack_target.as_ref().map(|attack| attack.target),
            Some(TargetKind::Cell(TARGET.0, TARGET.1))
        );
        assert_eq!(plane.navigation.nav_com, None);
        assert_eq!((plane.position.rx, plane.position.ry), expected, "{row}");
        assert_eq!(
            (plane.position.sub_x, plane.position.sub_y),
            (SimFixed::from_num(128), SimFixed::from_num(128))
        );
        assert_eq!(plane.body_facing_current(sim.session.binary_frame), 0);
    }
    assert_eq!(authored_replayed, 1);
}

/// The mission rows' Target cell; each plane stands the row's distance east
/// of its centre.
const PLANE_TARGET: (u16, u16) = (10, 40);
/// The NavCom a row with one starts with.
const NAV_CELL: (u16, u16) = (20, 20);

/// The row's plane: SPYP east of [`PLANE_TARGET`]'s centre where
/// `ObjectClass::Distance_To` (`combat::object_distance_to`, its own oracle)
/// answers the row's distance, 1500 leptons up, in the row's mission with
/// its timer due.
fn mission_plane(sim: &mut Simulation, rules: &RuleSet, row: &Value) -> u64 {
    let distance = int(&row["distance"]);
    let y = i32::from(PLANE_TARGET.1) * 256 + 128;
    // As SendSpyPlanes builds one: mission-only before its Unlimbo.
    let id = sim
        .with_object_placement_scope(|sim| {
            sim.construct_object_limbo_at_height("SPYP", "Americans", 0, 0, 0, 0, rules)
        })
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mark_mission_only();
    sim.with_object_placement_scope(|sim| {
        sim.reveal_constructed_object_at_height(
            id,
            PLANE_TARGET.0,
            PLANE_TARGET.1,
            0,
            0,
            PlacementEvidence::MarkSucceeded,
            rules,
            crate::sim::world::FrameEffects::default(),
        )
    })
    .unwrap();
    // The oracle writes NavCom (`+0x5A4`) and leaves the Fly at rest; a
    // flying Fly would make the missions' null destination re-target it
    // (Fly Stop_Moving), which the oracle's recorded setter does not run.
    if flag(row, "nav_com") {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .nav_com = Some(NavTargetRef::cell(NAV_CELL.0, NAV_CELL.1));
    }
    let mission = if row["mission"] == "approach" {
        MissionType::SpyplaneApproach
    } else {
        MissionType::SpyplaneOverfly
    };
    let target = TargetKind::Cell(PLANE_TARGET.0, PLANE_TARGET.1);
    let mut placed = false;
    'search: for dy in 0..64 {
        for dx in (distance - 4).max(0)..distance + 8 {
            let x = i32::from(PLANE_TARGET.0) * 256 + 128 + dx;
            let y = y + dy;
            let plane = sim.substrate.entities.get_mut(id).unwrap();
            plane.position.rx = (x / 256) as u16;
            plane.position.ry = (y / 256) as u16;
            plane.position.sub_x = SimFixed::from_num(x % 256);
            plane.position.sub_y = SimFixed::from_num(y % 256);
            plane.position.exact_z_leptons = Some(1500);
            let entities = &sim.substrate.entities;
            let plane = entities.get(id).unwrap();
            if crate::sim::combat::object_distance_to(plane, &target, entities) == Some(distance) {
                placed = true;
                break 'search;
            }
        }
    }
    assert!(placed, "no spot {distance} leptons out");
    let plane = sim.substrate.entities.get_mut(id).unwrap();
    plane.attack_target =
        flag(row, "target").then(|| AttackTarget::for_cell(PLANE_TARGET.0, PLANE_TARGET.1));
    if !flag(row, "nav_com") {
        plane.navigation.nav_com = None;
    }
    plane.in_playfield = flag(row, "in_playfield");
    plane.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(mission),
        suspended: MissionId::NONE,
        queued: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    id
}

/// One dispatch of Mission_SpyplaneApproach or Mission_SpyplaneOverfly per
/// row: the queued missions, `+0x6D2`, the destination (the Target, none,
/// or the opposite edge's cell), the snapshot's radius (the weapon's
/// `Damage=`, VERA's sight capped at 10), the camera sound and the frames
/// the mission timer takes.
#[test]
fn spyplane_missions_match_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "spyplane_missions");
    assert!(rows.len() > 40);
    for row in rows {
        let pick = events(row, "pick_cell_on_edge").next();
        if let Some(pick) = pick {
            assert_eq!(
                pick.as_array().unwrap()[2..],
                [
                    json!("0x889e68"),
                    json!("0x889e68"),
                    json!(4),
                    json!(1),
                    json!(0)
                ]
            );
        }
        if row["pick"] == json!([0, 0]) {
            // The empty cell is no destination (`0x0041574E..0x00415767`,
            // `0x00415890..0x004158A9`); a normalized playfield's pick never
            // is the empty cell.
            assert!(events(row, "destination").all(|event| event[1] != "edge_cell"));
            continue;
        }
        let keys = Keys {
            range: int(&row["weapon_range"]),
            damage: int(&row["damage"]),
            camera: int(&row["camera"]) != -1,
            frames: int(&row["frames"]),
            ..Keys::default()
        };
        let (rules, mut sim, americans) = world(rules(&keys));
        let house = sim.houses.get_mut(&americans).unwrap();
        house.waypoint_edge = u8::try_from(int(&row["waypoint_edge"])).unwrap_or(u8::MAX);
        house.multiplay_passive = flag(row, "passive");
        let id = mission_plane(&mut sim, &rules, row);
        if flag(row, "latched") {
            // A snapshot held from before, as the row's `+0x250` stands for.
            let config = sim.sight_reveal_config(Some(&rules));
            let plane = sim.substrate.entities.get(id).unwrap();
            crate::sim::vision::force_refresh_entity_vision_at_radius(
                &mut sim.fog,
                plane,
                &config,
                None,
                false,
                &sim.interner,
                4,
            );
        }
        let opposite = pick.map(|pick| {
            let opposite = edge(pick);
            let own = u8::try_from(int(&row["waypoint_edge"])).unwrap_or(u8::MAX);
            assert_eq!(Edge::opposite_edge(own), opposite, "{row}");
            expected_pick(&sim, opposite, 0)
        });
        sim.sound_events.clear();
        let now = sim.session.binary_frame;

        crate::sim::aircraft::dispatch_mission(&mut sim, id, &rules, Default::default());

        let plane = sim.substrate.entities.get(id).unwrap();
        let queued = events(row, "queue")
            .last()
            .map_or(MissionId::NONE, |event| MissionId::from_raw(int(&event[1])));
        assert_eq!(plane.mission.queued(), queued, "{row}");
        assert_eq!(
            plane.mission_leaf.as_aircraft().unwrap().action_latch(),
            u8::try_from(int(&row["action_latch"])).unwrap(),
            "{row}"
        );
        let nav = match events(row, "destination").last() {
            None => flag(row, "nav_com").then(|| NavTargetRef::cell(NAV_CELL.0, NAV_CELL.1)),
            Some(event) if event[1] == "null" => None,
            Some(event) if event[1] == "target" => {
                Some(NavTargetRef::cell(PLANE_TARGET.0, PLANE_TARGET.1))
            }
            Some(_) => opposite.map(|(rx, ry)| NavTargetRef::cell(rx, ry)),
        };
        assert_eq!(plane.navigation.nav_com, nav, "{row}");
        let radius = sim
            .fog
            .sight_admissions
            .get(&(id, americans))
            .map_or(0, |admission| admission.radius);
        let revealed = events(row, "reveal")
            .filter(|event| event[8] == 0)
            .last()
            .map_or(0, |event| int(&event[2]).min(10));
        assert_eq!(i32::from(radius), revealed, "{row}");
        let sounds = events(row, "play_at")
            .filter(|event| int(&event[1]) >= 0)
            .count();
        assert_eq!(camera_sounds(&sim), sounds, "{row}");
        let timer = plane.mission.dispatch_timer();
        assert_eq!(
            (timer.start_frame(), timer.delay()),
            (now as i32, int(&row["returned"])),
            "{row}"
        );
    }
}

/// The player's Spy Plane on retail rules, from the click through
/// production frames. AllyParaDrop authors one entry, so one SPYP enters at
/// the Americans' North edge (its cells sum to 36 on this map) in
/// Mission_SpyplaneApproach with the clicked cell as its Target. Within its
/// camera weapon's 20 cells it photographs (`SpyPlaneSnapshot`, `Damage=6`
/// cells of shroud), within three cells it turns to Overfly, and it flies on
/// straight, photographing without the sound, until it leaves the map's
/// `Size=` diamond, where it is removed. It never fires the camera weapon.
/// The Super recharges.
#[test]
fn retail_spy_plane_photographs_the_target_and_leaves() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world(rules);
    let sw_type = charge_super(&mut sim, americans, SPY_PLANE);
    assert!(!sim.fog.is_cell_revealed(americans, TARGET.0, TARGET.1));

    click(&mut sim, &rules, americans, SPY_PLANE, TARGET);

    assert!(!sim.super_weapons[&americans][&sw_type].is_ready);
    let [plane] = spy_planes(&sim)[..] else {
        panic!("one plane");
    };
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.owner(), americans);
    assert!(entity.is_mission_only());
    assert_eq!(
        entity.mission.current(),
        MissionId::from_known(MissionType::SpyplaneApproach)
    );
    assert_eq!(
        entity.attack_target.as_ref().map(|attack| attack.target),
        Some(TargetKind::Cell(TARGET.0, TARGET.1))
    );
    assert_eq!(entity.position.rx + entity.position.ry, 36);

    let revealed = |sim: &Simulation| {
        (0..64u16)
            .flat_map(|x| (0..64u16).map(move |y| (x, y)))
            .filter(|&(x, y)| sim.fog.is_cell_revealed(americans, x, y))
            .count()
    };
    let mut frames = 0;
    let mut snapshots = 0;
    let mut overfly = None;
    while sim.substrate.entities.contains(plane) {
        sim.sound_events.clear();
        step(&mut sim, &rules);
        frames += 1;
        snapshots += camera_sounds(&sim);
        let current = sim
            .substrate
            .entities
            .get(plane)
            .map(|entity| entity.mission.current());
        if overfly.is_none() && current == Some(MissionId::from_known(MissionType::SpyplaneOverfly))
        {
            overfly = Some(revealed(&sim));
        }
        assert!(sim.projectiles.is_empty(), "the camera never fires");
        assert!(frames < 3000, "the plane never left");
    }
    let at_overfly = overfly.expect("no Overfly");
    assert!(snapshots > 0, "no snapshot");
    assert!(sim.fog.is_cell_revealed(americans, TARGET.0, TARGET.1));
    // Overfly photographs on past the target, silently.
    assert!(revealed(&sim) > at_overfly);
    assert!(!sim.fog.is_cell_revealed(americans, 60, 5));
}

/// The Super Weapons option off (`0x00A8B263`) withholds only the
/// `DisableableFromShell=` types, which retail's Spy Plane is not, through
/// the production frames: both houses' radars grant it at the first frame
/// while the silo grants nothing, its charge runs out in the frame's Super
/// pass, and the click flies the plane.
#[test]
fn retail_spy_plane_flies_with_super_weapons_off() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world(rules);
    let russians: InternedId = sim.interner.intern("Russians");
    sim.session.game_options.super_weapons = false;
    for (kind, owner, at) in [
        ("NARADR", "Americans", (24, 24)),
        ("NAMISL", "Americans", (28, 24)),
        ("NARADR", "Russians", (24, 32)),
    ] {
        sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, 0, &rules)
            .unwrap_or_else(|| panic!("{kind} stands"));
    }
    let spy_plane = sim.interner.intern(SPY_PLANE);
    let nuke = sim.interner.intern("NukeSpecial");
    let granted = |sim: &Simulation, owner: InternedId, sw_type: InternedId| {
        sim.super_weapons
            .get(&owner)
            .and_then(|supers| supers.get(&sw_type))
            .is_some_and(|instance| instance.is_active)
    };

    step(&mut sim, &rules);
    assert!(granted(&sim, americans, spy_plane));
    assert!(granted(&sim, russians, spy_plane));
    assert!(!granted(&sim, americans, nuke));

    let instance = sim
        .super_weapons
        .get_mut(&americans)
        .and_then(|supers| supers.get_mut(&spy_plane))
        .unwrap();
    instance.charge_start_tick -= instance.charge_duration;
    step(&mut sim, &rules);
    assert!(sim.super_weapons[&americans][&spy_plane].is_ready);

    click(&mut sim, &rules, americans, SPY_PLANE, TARGET);
    assert!(!sim.super_weapons[&americans][&spy_plane].is_ready);
    assert_eq!(spy_planes(&sim).len(), 1);
}

/// The computer's Spy Plane on retail rules, through AI_TryFireSW's
/// GroundRallyPoint arm (`0x00509CD0`): its enemy's base cell is passable,
/// so the plane's Target is two cells past it on both axes.
#[test]
fn retail_computer_sends_its_spy_plane_past_the_enemy_base() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world(rules);
    let russians: InternedId = sim.interner.intern("Russians");
    sim.houses.get_mut(&russians).unwrap().enemy_house = Some(americans);
    sim.houses.get_mut(&americans).unwrap().base_center = Some(TARGET);
    let sw_type = charge_super(&mut sim, russians, SPY_PLANE);

    super::ai_fire::try_fire(
        &mut sim,
        &rules,
        russians,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert!(!sim.super_weapons[&russians][&sw_type].is_ready);
    let [plane] = spy_planes(&sim)[..] else {
        panic!("one plane");
    };
    let entity = sim.substrate.entities.get(plane).unwrap();
    assert_eq!(entity.owner(), russians);
    assert_eq!(
        entity.attack_target.as_ref().map(|attack| attack.target),
        Some(TargetKind::Cell(TARGET.0 + 2, TARGET.1 + 2))
    );
}
