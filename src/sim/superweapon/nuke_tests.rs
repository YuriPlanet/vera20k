//! The nuclear missile: Launch case 0 and the `NUKE` warhead's impact
//! against `tools/superweapon_oracle.json` (`nuke_launch`, `nuke_impact`,
//! `nuke_wait`, `nuke_flash`, `nuke_lighting_read`; `--check` regenerates
//! them), and the missile through production frames on retail rules: the
//! launch command, the silo's flight, NukeMaker's falling warhead, its
//! impact's flash and `NUKEBALL`, and its strike.

use super::SuperWeaponInstance;
use super::chronosphere_tests::{retail_rules_binding, world_with};
use crate::map::lighting::{ParsedLightingProfiles, parse_lighting_profiles};
use crate::map::resolved_terrain::{
    ResolvedTerrainCell, SharedCellDummy, test_flat_cell, test_grid,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::anim_class::AnimId;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::light_sources::LightingEvent;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::projectile::{
    NukeImpactContext, Projectile, ProjectileCollisionPolicy, ProjectileCoord,
    ProjectileDetonationReason, ProjectilePayload, ProjectileSpawn, ProjectileStore,
    ProjectileTarget, ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState,
    TargetExpiryPolicy,
};
use crate::sim::radar::RadarEventType;
use crate::sim::scenario_session::{ScenarioLightingProfile, ScenarioLightingState};
use crate::sim::timer::CdTimer;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::{Value, json};

const TARGET: (u16, u16) = (40, 40);
/// `NUKEBALL.SHP`'s frame count.
const NUKE_BALL_FRAMES: i32 = 20;

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn ints(value: &Value) -> Vec<i32> {
    value.as_array().unwrap().iter().map(int).collect()
}

fn coord(value: &Value) -> ProjectileCoord {
    let xyz = ints(value);
    ProjectileCoord::new(xyz[0], xyz[1], xyz[2])
}

fn has_event(row: &Value, name: &str) -> bool {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event[0] == name)
}

/// A straight bullet at `origin` carrying `warhead`, aimed ten cells east.
fn nuke_spawn(
    interner: &mut StringInterner,
    warhead: &str,
    origin: ProjectileCoord,
) -> ProjectileSpawn {
    let target = ProjectileCoord::new(origin.x + 10 * 256, origin.y, origin.z);
    ProjectileSpawn {
        native_unique_id: 0,
        line_trail: None,
        flat: false,
        source_id: 0,
        origin,
        target: ProjectileTarget::Cell {
            rx: (target.x / 256) as u16,
            ry: (target.y / 256) as u16,
        },
        initial_target_position: target,
        payload: ProjectilePayload::new(
            600,
            interner.intern(warhead),
            interner.intern("NukePayload"),
        ),
        speed_leptons_per_frame: 64,
        velocity: ProjectileVelocity::new(64, 0, 0),
        trajectory: ProjectileTrajectory::Straight,
        guidance: None,
        visual: ProjectileVisualState::new(0, 0, 0),
        arm_frames: 0,
        fuse_frames: None,
        ranged_fuse: false,
        tracks_target: false,
        target_expiry: TargetExpiryPolicy::Expire,
        collision: ProjectileCollisionPolicy::NONE,
    }
}

/// The flash's status and timer as native stores them.
fn flash_state(lighting: &ScenarioLightingState) -> Vec<i32> {
    let (status, start, duration) = lighting.nuke_flash_for_test();
    vec![status, start, duration]
}

/// `BulletClass::AI`'s `NUKE` block against the native rows: the warhead
/// test (strcmpi), the ground clamp (a bridge's deck counted), whether the
/// bullet waits (the `NUKEBALL` type found case-blind in the rules'
/// `[Animations]`) and the cell the tail commits; a bullet that does not
/// wait detonates. The fixture's floor answers 416 leptons everywhere; VERA's
/// level-4 cells do on the map, so the clamp of the row off it (whose floor
/// is the shared dummy's) is not compared. The Mark calls around SetHeight
/// (`ObjectClass::Mark @ 0x005F5850`) clear and set IsOnMap again: no state.
#[test]
fn impact_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "nuke_impact");
    assert_eq!(rows.len(), 16);
    let terrain = test_grid(16, 16, |x, y| ResolvedTerrainCell {
        level: 4,
        ..test_flat_cell(x, y)
    });
    let dummy = terrain.shared_cell_dummy();
    for row in rows {
        assert_eq!(int(&row["ground"]), 416);
        let types: String = row["types"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, name)| format!("{index}={}\n", name.as_str().unwrap()))
            .collect();
        let rules =
            RuleSet::from_ini(&IniFile::from_str(&format!("[Animations]\n{types}"))).unwrap();
        let mut interner = StringInterner::new();
        let location = coord(&row["location"]);
        let mut store = ProjectileStore::new();
        let id = store.spawn(
            1,
            nuke_spawn(&mut interner, row["warhead"].as_str().unwrap(), location),
        );
        store.get_mut(id).unwrap().on_bridge = row["on_bridge"].as_bool().unwrap();
        let nuke_ball = || super::nuke::nuke_ball_type(&rules);
        let context = NukeImpactContext::new(&interner, &nuke_ball, &|_| false);

        let impact = store.get_mut(id).unwrap().nuke_impact(
            &context,
            Some(&terrain),
            &dummy,
            coord(&row["candidate"]),
        );

        assert_eq!(
            impact.is_some(),
            has_event(row, "screen_nuke_flash"),
            "{row}"
        );
        let waits = int(&row["waits"]) != 0;
        assert_eq!(impact.is_some_and(|impact| impact.waits()), waits, "{row}");
        assert_eq!(has_event(row, "detonate"), !waits, "{row}");
        let bullet = store.get(id).unwrap();
        assert_eq!(bullet.awaiting_anim(), waits, "{row}");
        assert_eq!(bullet.awaited_anim(), None);
        if location.x >= 0 && location.y >= 0 {
            assert_eq!(bullet.position, coord(&row["location_after"]), "{row}");
        }
        if waits {
            let cell = ints(&row["cell"]);
            assert_eq!(
                bullet.previous_cell,
                (cell[0] as i16, cell[1] as i16),
                "{row}"
            );
        }
    }
}

/// The rest of the block against the native rows, through
/// [`super::nuke::impact`] on retail rules: the flash and its relight, the
/// type-13 radar event at the bullet's cell (truncated toward zero), and
/// `NUKEBALL` at the bullet's Location with a combat explosion's arguments,
/// which the bullet then holds. The anim of the row off the map (VERA's
/// anims stand on map cells) is not compared.
#[test]
fn impact_effects_match_native() {
    let Some(rules) = retail_rules_binding(&[("NUKEBALL", NUKE_BALL_FRAMES)]) else {
        return;
    };
    let oracle = oracle();
    let (rules, mut sim, _) = world_with(rules, 16, &[]);
    let mut compared = 0;
    for row in rows(&oracle, "nuke_impact") {
        if !has_event(row, "screen_nuke_flash") {
            continue;
        }
        compared += 1;
        let location = coord(&row["location_after"]);
        let id = sim.allocate_stable_id();
        let spawn = nuke_spawn(&mut sim.interner, "NUKE", location);
        sim.projectiles.spawn(id, spawn);
        sim.session.binary_frame = 4000;
        sim.session.lighting = ScenarioLightingState::default();
        sim.lighting_sources.pending.clear();
        sim.sound_events.clear();
        let anims_before: Vec<AnimId> = sim.substrate.anims.iter().map(|(&id, _)| id).collect();
        let waits = int(&row["waits"]) != 0;
        // The bullet's side of the block (on its clamped Location, which
        // stands over this map's floor), then the world's.
        let never = |_: AnimId| false;
        let nuke_ball = || waits;
        let context = NukeImpactContext::new(&sim.interner, &nuke_ball, &never);
        let dummy = SharedCellDummy::fresh();
        let impact = sim
            .projectiles
            .get_mut(id)
            .unwrap()
            .nuke_impact(&context, None, &dummy, location)
            .unwrap();
        assert_eq!(sim.projectiles.get(id).unwrap().position, location);

        super::nuke::impact(&mut sim, &rules, impact);

        let lighting = sim.session.lighting;
        assert_eq!(flash_state(&lighting), vec![1, 4000, 30]);
        assert_eq!(lighting.selected_profile, ScenarioLightingProfile::Nuke);
        assert!(
            sim.lighting_sources
                .pending
                .iter()
                .any(|event| matches!(event, LightingEvent::Global { .. }))
        );
        let radar = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "radar")
            .unwrap();
        let cell = ints(&radar[2]);
        let raised: Vec<_> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::SuperWeaponRadarEvent { radar } => Some(*radar),
                _ => None,
            })
            .collect();
        assert_eq!(raised.len(), 1);
        assert_eq!(raised[0].event_type as i32, int(&radar[1]));
        assert_eq!(raised[0].event_type, RadarEventType::ImpactSilent);
        assert_eq!(
            (
                i32::from(raised[0].rx as i16),
                i32::from(raised[0].ry as i16)
            ),
            (cell[0], cell[1]),
            "{row}"
        );
        let new_anims: Vec<AnimId> = sim
            .substrate
            .anims
            .iter()
            .map(|(&id, _)| id)
            .filter(|id| !anims_before.contains(id))
            .collect();
        let bullet = sim.projectiles.get(id).unwrap();
        if !waits {
            assert!(new_anims.is_empty(), "{row}");
            assert_eq!(bullet.awaited_anim(), None);
            continue;
        }
        if location.x < 0 || location.y < 0 {
            continue;
        }
        assert_eq!(new_anims.len(), 1, "{row}");
        assert_eq!(bullet.awaited_anim(), Some(new_anims[0]));
        let event = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "anim")
            .unwrap();
        let anim = sim.anim(new_anims[0]).unwrap();
        assert!(
            sim.interner
                .resolve(anim.type_id)
                .eq_ignore_ascii_case(event[1].as_str().unwrap())
        );
        let at = anim.world_coord;
        assert_eq!(vec![at.x, at.y, at.z], ints(&event[2]), "{row}");
        // (delay, loopCount, drawFlags, zAdjust, reverse)
        assert_eq!(
            (
                i32::from(anim.runtime.loop_remaining),
                anim.draw_flags,
                anim.z_adjust
            ),
            (int(&event[4]), int(&event[5]) as u32, int(&event[6])),
            "{row}"
        );
        assert_eq!((int(&event[3]), int(&event[7])), (0, 0));
    }
    assert_eq!(compared, 13);
}

/// The head of `BulletClass::AI` against the native rows: a waiting bullet
/// returns while its anim lives, and once it is gone (expired, or never
/// constructed) detonates where it stands with the impact flag clear; one
/// that does not wait flies on. VERA's store holds no dead bullet (a
/// detonated one leaves at its commit), so the rows with IsAlive clear are
/// not compared, and the holder list (`0x00B0F5B8`) is not modelled: the
/// anim's liveness answers for its PointerExpired.
#[test]
fn wait_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "nuke_wait");
    assert_eq!(rows.len(), 9);
    let anim: AnimId = 7;
    for row in rows {
        if !row["alive"].as_bool().unwrap() {
            assert_eq!(row["end"], "returns");
            continue;
        }
        let waits = row["waits"].as_bool().unwrap();
        let live = row["anim"].as_bool().unwrap();
        // A waiting bullet holds its anim, or none when it was never built.
        let held: &[Option<AnimId>] = if waits && !live {
            &[Some(anim), None]
        } else {
            &[Some(anim)]
        };
        for &held in held {
            let mut interner = StringInterner::new();
            let mut store = ProjectileStore::new();
            let at = ProjectileCoord::new(2660, 3122, 1000);
            let id = store.spawn(1, nuke_spawn(&mut interner, "NUKE", at));
            let anim_live = |id: AnimId| live && id == anim;
            let context = NukeImpactContext::new(&interner, &|| true, &anim_live);
            let dummy = SharedCellDummy::fresh();
            if waits {
                let bullet = store.get_mut(id).unwrap();
                bullet.nuke_impact(&context, None, &dummy, at).unwrap();
                store.await_anim(id, held);
            }

            let result = store
                .advance_one(
                    id,
                    1,
                    |_| None,
                    None,
                    &dummy,
                    6,
                    false,
                    false,
                    0,
                    |_, _, _| None,
                    Some(&context),
                )
                .unwrap();

            let bullet = store.get(id).unwrap();
            match row["end"].as_str().unwrap() {
                "returns" => {
                    assert!(result.detonations.is_empty(), "{row}");
                    assert!(bullet.awaiting_anim());
                    assert_eq!(bullet.position, at);
                }
                "detonated" => {
                    assert!(has_event(row, "uninit"));
                    assert_eq!(result.detonations.len(), 1, "{row}");
                    assert_eq!(
                        result.detonations[0].reason,
                        ProjectileDetonationReason::AnimEnded
                    );
                    assert_eq!(int(&row["events"][1][1]), 0, "the impact flag");
                    assert!(!bullet.awaiting_anim());
                    assert_eq!(int(&row["waits_after"]), 0);
                }
                "flies" => {
                    assert!(result.detonations.is_empty(), "{row}");
                    assert!(result.nuke_impacts().is_empty(), "{row}");
                    assert_ne!(bullet.position, at, "the flight ran");
                }
                other => panic!("{other}"),
            }
        }
    }
}

/// ScreenNukeFlash and the head of `LightningStorm::Process` once a frame
/// against the native rows: the flash's status and timer, the fade timer,
/// the target and tint the start selects (RecalcLighting's RGB) and the
/// relight when the fade-in ends; then single steps at each test's edges,
/// a timer whose sum wraps included. Status 3 has no writer but the save
/// stream (`0x0053993E`), so its row is not compared; the fade-out function
/// at `0x0053AC50` has no caller.
#[test]
fn flash_matches_native() {
    let oracle = oracle();
    let flash = &oracle["nuke_flash"];
    let runs = flash["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    for run in runs {
        let frame = int(&run["frame"]);
        let mut lighting = ScenarioLightingState::default();
        let timer = ints(&run["timer"]);
        lighting.transition_timer = CdTimer::from_raw(timer[0], timer[1]);
        lighting.target_ambient = int(&run["target_before"]);

        lighting.start_nuke_flash(frame);

        let start = &run["start"];
        assert_eq!(flash_state(&lighting), ints(&start["flash"]));
        let timer = lighting.transition_timer;
        assert_eq!(
            vec![timer.start_frame(), timer.duration()],
            ints(&start["timer"])
        );
        assert_eq!(lighting.target_ambient, int(&start["target"]));
        let recalc = &start["events"][0];
        assert_eq!(recalc[0], "recalc");
        assert_eq!(int(&recalc[4]), 1);
        let rgb = [int(&recalc[1]), int(&recalc[2]), int(&recalc[3])];
        assert_eq!(lighting.alternate_rgb(), Some(rgb));
        let mut steps = Vec::new();
        let mut last = flash_state(&lighting);
        for offset in 1..=int(&run["frames"]) {
            let relit = lighting.step_nuke_flash(frame + offset);
            let now = flash_state(&lighting);
            if now != last || relit {
                steps.push(json!([offset, now[0], now[1], now[2], relit]));
            }
            last = now;
        }
        let native: Vec<Value> = run["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| {
                let relit = step[4]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event[0] == "update_lighting");
                json!([step[0], step[1], step[2], step[3], relit])
            })
            .collect();
        assert_eq!(steps, native);
    }
    let steps = flash["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 11);
    for row in steps {
        let status = int(&row["status"]);
        if status == 3 {
            continue;
        }
        let mut lighting = ScenarioLightingState::default();
        lighting.set_nuke_flash_for_test(status, int(&row["start"]), int(&row["duration"]));
        let relit = lighting.step_nuke_flash(int(&row["frame"]));
        assert_eq!(flash_state(&lighting), ints(&row["after"]), "{row}");
        assert_eq!(relit, has_event(row, "update_lighting"), "{row}");
    }
}

/// The map's `NukeAmbientChangeRate=` against the native rows: Set_Defaults'
/// value, a missing key and authored tokens through the production reader.
#[test]
fn nuke_change_rate_read_matches_native() {
    let oracle = oracle();
    let read = &oracle["nuke_lighting_read"];
    assert_eq!(
        ParsedLightingProfiles::default().nuke_change_rate,
        int(&read["stored_default"])
    );
    let missing = parse_lighting_profiles(&IniFile::from_str("[Lighting]\nAmbient=1\n"));
    assert_eq!(missing.nuke_change_rate, int(&read["default_units"]));
    for authored in read["authored"].as_array().unwrap() {
        let token = authored[0].as_str().unwrap();
        let ini = IniFile::from_str(&format!("[Lighting]\nNukeAmbientChangeRate={token}\n"));
        let profiles = parse_lighting_profiles(&ini);
        assert_eq!(profiles.nuke_change_rate, int(&authored[1]), "{token}");
        assert_eq!(
            ScenarioLightingState::from_map(&profiles).nuke_change_rate(),
            profiles.nuke_change_rate
        );
    }
}

/// `nuke_launch`'s BuildingTypes in `BuildingTypeClass::Array` order, each
/// `(NukeSilo=, SuperWeapon=, SuperWeapon2=)` with an index naming the
/// Super at it (`NukeSpecial` at 3, the oracle's array index) or -1 none.
fn launch_rules(types: &[(bool, i32, i32)]) -> RuleSet {
    const SUPERS: [&str; 4] = ["SW0", "SW1", "SW2", "NukeSpecial"];
    let mut rules = String::from(
        "[General]\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [SuperWeaponTypes]\n0=SW0\n1=SW1\n2=SW2\n3=NukeSpecial\n\
         [SW0]\nType=IronCurtain\n[SW1]\nType=IronCurtain\n[SW2]\nType=IronCurtain\n\
         [NukeSpecial]\nType=MultiMissile\n[BuildingTypes]\n",
    );
    let mut art = String::new();
    for slot in 0..types.len() {
        rules += &format!("{slot}=SILO{slot}\n");
    }
    for (slot, &(silo, weapon, weapon2)) in types.iter().enumerate() {
        rules += &format!("[SILO{slot}]\nStrength=1000\nFoundation=1x1\nNukeSilo={silo}\n");
        for (key, index) in [("SuperWeapon", weapon), ("SuperWeapon2", weapon2)] {
            if let Ok(index) = usize::try_from(index) {
                rules += &format!("{key}={}\n", SUPERS[index]);
            }
        }
        art += &format!("[SILO{slot}]\nFoundation=1x1\n");
    }
    let art = IniFile::from_str(&art);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&rules), &art).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules
}

/// Case 0 (`0x006CDA67`, `0x006CDCF0..0x006CDE36`) against `nuke_launch`,
/// through the production launch with the row's BuildingTypes
/// ([`launch_rules`]) and, where native's Find_Building_Of_Type answers a
/// building, one building of each of them. Compared:
/// - the charge gate;
/// - the type scan: only the building of the type native asks for fires;
/// - the Missile mission queued and commenced on it, the house's
///   NukeTarget (`+0x5784`) and the silo's firing type (`+0x5F8`);
/// - the launch event at the cell of PlayAtCoord's coordinate, for the
///   app's sound and line (`sound_dispatch::launch_lines_match_native`).
///
/// Not compared: the player's tail (the app's, `super_selection`), the
/// recheck flag `+0x1FC` and the mute row (module RESIDUALS).
#[test]
fn the_launch_matches_native() {
    let oracle = oracle();
    let rows = rows(&oracle, "nuke_launch");
    assert_eq!(rows.len(), 12);
    for row in rows.iter().filter(|row| row["mute"] != true) {
        let types: Vec<(bool, i32, i32)> = row["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| (entry[0] == true, int(&entry[1]), int(&entry[2])))
            .collect();
        let (rules, mut sim, owner) = world_with(launch_rules(&types), 128, &[]);
        let events = row["events"].as_array().unwrap();
        let asked = events
            .iter()
            .find(|event| event[0] == "find")
            .map(|event| int(&event[2]) as usize);
        let buildings: Vec<u64> = if row["silo"] == true {
            (0..types.len())
                .map(|slot| {
                    let x = 64 + 2 * slot as u16;
                    sim.spawn_object_at_height(
                        &format!("SILO{slot}"),
                        "Americans",
                        x,
                        64,
                        0,
                        0,
                        &rules,
                    )
                    .expect("the building stands")
                })
                .collect()
        } else {
            Vec::new()
        };
        let sw_type = super::chronosphere_tests::charge_super(&mut sim, owner, "NukeSpecial");
        sim.super_weapons
            .get_mut(&owner)
            .unwrap()
            .get_mut(&sw_type)
            .unwrap()
            .is_ready = row["charged"] == true;
        let target = (int(&row["target"][0]) as u16, int(&row["target"][1]) as u16);
        let sw = rules.super_weapon("NukeSpecial").unwrap().clone();
        let launched = super::nuke::launch(&mut sim, &rules, owner, sw_type, &sw, target);
        assert_eq!(launched, has_event(row, "queue_mission"), "{row}");
        for (slot, &id) in buildings.iter().enumerate() {
            let building = sim.substrate.entities.get(id).unwrap();
            let fires = launched && asked == Some(slot);
            assert_eq!(
                building.mission.current() == MissionId::from_known(MissionType::Missile),
                fires,
                "{row} {slot}"
            );
            assert_eq!(
                building
                    .mission_leaf
                    .as_building()
                    .unwrap()
                    .firing_super_weapon(),
                if fires { int(&row["firing_type"]) } else { -1 },
                "{row} {slot}"
            );
        }
        let nuke_target = ints(&row["nuke_target"]);
        assert_eq!(
            sim.houses[&owner].nuke_target(),
            (nuke_target[0] as u16, nuke_target[1] as u16),
            "{row}"
        );
        let native: Vec<(u16, u16)> = events
            .iter()
            .filter(|event| event[0] == "play_at")
            .map(|event| {
                let xyz = ints(&event[2]);
                ((xyz[0] / 256) as u16, (xyz[1] / 256) as u16)
            })
            .collect();
        let pushed: Vec<(u16, u16)> = sim
            .sound_events
            .iter()
            .filter_map(|event| match *event {
                SimSoundEvent::SuperWeaponLaunched {
                    owner: by,
                    sw_type: kind,
                    rx,
                    ry,
                } if by == owner && kind == sw_type => Some((rx, ry)),
                _ => None,
            })
            .collect();
        assert_eq!(pushed, native, "{row}");
    }
}

fn step(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), None, None, 33);
}

/// Steps until `done` holds, at most `frames` times; the frames it took.
fn run_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    frames: u32,
    mut done: impl FnMut(&Simulation) -> bool,
) -> u32 {
    for frame in 1..=frames {
        step(sim, rules);
        if done(sim) {
            return frame;
        }
    }
    panic!("not reached in {frames} frames");
}

fn bullets_of<'a>(sim: &'a Simulation, weapon: &str) -> Vec<(u64, &'a Projectile)> {
    sim.projectiles
        .iter()
        .filter(|(_, bullet)| sim.interner.resolve(bullet.payload.weapon) == weapon)
        .map(|(&id, bullet)| (id, bullet))
        .collect()
}

fn psi_warnings(sim: &Simulation) -> Vec<Option<u64>> {
    sim.substrate
        .anims
        .iter()
        .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "PSIWARN")
        .map(|(_, anim)| anim.attached_bullet())
        .collect()
}

#[test]
fn retail_nuclear_missile_rises_falls_and_strikes_its_target() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    // The lib suite loads no SHP: bind the anims' frame counts.
    for name in ["PSIWARN", "NUKETO"] {
        rules.bind_anim_frame_count_for_test(name, 20);
    }
    rules.bind_anim_frame_count_for_test("NUKEBALL", NUKE_BALL_FRAMES);
    // The retail values the chain reads.
    let nuke = rules.super_weapon("NukeSpecial").unwrap();
    assert_eq!(
        (
            nuke.kind,
            nuke.weapon_type.as_deref(),
            nuke.ai_defend_against,
            nuke.recharge_time_frames
        ),
        (
            SuperWeaponKind::MultiMissile,
            Some("NukeCarrier"),
            true,
            9000
        )
    );
    let silo_type = rules.object("NAMISL").unwrap();
    assert!(silo_type.nuke_silo);
    assert_eq!(silo_type.super_weapon.as_deref(), Some("NukeSpecial"));
    assert_eq!(silo_type.charged_anim_time, 1.0);
    assert_eq!(rules.general.nuke_take_off, "NUKETO");
    assert_eq!(rules.general.ai_super_defense_probability, vec![90, 50, 10]);
    assert_eq!(rules.general.ai_super_defense_distance, 12 * 256);
    let up = rules
        .projectile(
            rules
                .weapon("NukeCarrier")
                .unwrap()
                .projectile
                .as_deref()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(up.detonation_altitude, 20000);
    let payload = rules.weapon("NukePayload").unwrap();
    assert_eq!(
        (
            payload.damage,
            payload.warhead.as_deref(),
            payload.rad_level
        ),
        (600, Some("NUKE"), 500)
    );

    let mut sim = Simulation::with_seed(11);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let russians = sim.interner.intern("Russians");
    let americans = sim.interner.intern("Americans");
    for (id, side, human) in [(russians, 1, true), (americans, 0, false)] {
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    {
        // A hard computer house takes up the defence 90 times in 100.
        let house = sim.houses.get_mut(&americans).unwrap();
        house.base_center = Some((42, 42));
        house.difficulty = crate::sim::house_state::HouseDifficulty::Hard;
    }
    sim.session.game_options.super_weapons = true;
    sim.resolved_terrain = Some(test_grid(
        64,
        64,
        super::cell_receiver_tests::test_terrain_cell,
    ));
    sim.playfield_bounds = Some(super::cell_receiver_tests::test_playfield_bounds());
    let silo = sim
        .spawn_object_at_height("NAMISL", "Russians", 10, 10, 0, 0, &rules)
        .expect("the silo stands");
    // Power for the silo, so the recharge runs.
    for at in [(20, 10), (24, 10)] {
        sim.spawn_object_at_height("NAPOWR", "Russians", at.0, at.1, 0, 0, &rules)
            .expect("the power plant stands");
    }
    let tank = sim
        .spawn_object_at_height("MTNK", "Americans", TARGET.0, TARGET.1, 0, 0, &rules)
        .unwrap();
    let sw_type: InternedId = sim.interner.intern("NukeSpecial");
    let mut instance = SuperWeaponInstance::new(sw_type, russians, 0);
    instance.activate(9000, sim.session.binary_frame);
    instance.is_ready = true;
    sim.super_weapons
        .entry(russians)
        .or_default()
        .insert(sw_type, instance);

    sim.queue_command(CommandEnvelope::new(
        russians,
        sim.session.tick + 1,
        Command::LaunchSuperWeapon {
            sw_type_id: sw_type,
            target_rx: TARGET.0,
            target_ry: TARGET.1,
        },
    ));
    let launch_frame = sim.session.binary_frame as i32;
    run_until(&mut sim, &rules, 3, |sim| {
        !bullets_of(sim, "NukeCarrier").is_empty()
    });
    // ClickFire restarted the recharge; Launch aimed the silo and the house.
    let instance = &sim.super_weapons[&russians][&sw_type];
    assert!(!instance.is_ready);
    assert_eq!(instance.charge_duration, 9000);
    assert!(instance.charge_start_tick >= launch_frame);
    assert_eq!(sim.houses[&russians].nuke_target(), TARGET);
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::SuperWeaponLaunched { owner, rx: 40, ry: 40, .. } if *owner == russians
    )));
    // The computer house near the target took up the defence of its base.
    assert_eq!(sim.houses[&americans].super_weapon_defense().0, (42, 42));
    let (missile, _) = bullets_of(&sim, "NukeCarrier")[0];
    assert_eq!(sim.projectiles.get(missile).unwrap().source_id, silo);
    // `FirersPalette=yes` (GiantNukeUp): Construct kept the silo's House for
    // the draw's colour scheme (Bullet+0x114).
    assert_eq!(
        sim.projectiles.get(missile).unwrap().firer_house(),
        Some(russians)
    );
    assert_eq!(psi_warnings(&sim), vec![Some(missile)]);

    // The warning shows to a house not allied with the launcher whose
    // powered Psychic Sensor (2x2, `PsychicDetectionRadius=15`) stands within
    // 15 cells of the target cell's centre.
    let detected = |sim: &Simulation, viewer| {
        sim.substrate
            .anims
            .iter()
            .filter(|(_, anim)| sim.interner.resolve(anim.type_id) == "PSIWARN")
            .any(|(_, anim)| sim.psi_warning_detected_by(viewer, anim, &rules))
    };
    let place = |sim: &mut Simulation, kind, owner, at: (u16, u16)| {
        sim.spawn_object_at_height(kind, owner, at.0, at.1, 0, 0, &rules)
            .expect("the building stands");
        step(sim, &rules);
    };
    place(&mut sim, "NAPOWR", "Americans", (50, 50));
    // Its centre is 5248 leptons east of the target cell's.
    place(&mut sim, "NAPSIS", "Americans", (60, 40));
    assert!(!detected(&sim, americans), "beyond the radius");
    place(&mut sim, "NAPSIS", "Russians", (50, 40));
    assert!(!detected(&sim, russians), "the launcher's own warning");
    // sqrt(3712² + 128²) = 3714 leptons, within 15 × 256.
    place(&mut sim, "NAPSIS", "Americans", (54, 40));
    assert!(detected(&sim, americans));
    assert_eq!(psi_warnings(&sim), vec![Some(missile)]);

    // The missile rises out of sight; its warning goes with it and the
    // warhead falls from 20000 leptons over the target.
    run_until(&mut sim, &rules, 600, |sim| {
        sim.projectiles.get(missile).is_none()
    });
    assert!(psi_warnings(&sim).is_empty());
    let falling = bullets_of(&sim, "NukePayload");
    assert_eq!(falling.len(), 1);
    let (warhead, bullet) = falling[0];
    assert_eq!(
        bullet.target,
        ProjectileTarget::Cell {
            rx: TARGET.0,
            ry: TARGET.1
        }
    );
    assert_eq!(bullet.source_id, silo);
    assert_eq!(bullet.firer_house(), Some(russians), "GiantNukeDown");
    assert_eq!(
        [
            bullet.launch_origin.x,
            bullet.launch_origin.y,
            bullet.launch_origin.z
        ],
        [40 * 256 + 128, 40 * 256 + 128, 20000]
    );

    // It strikes the ground at the tank: the screen flashes, the radar
    // marks the cell, and the warhead waits on NUKEBALL before it
    // detonates.
    let tank_health = sim.substrate.entities.get(tank).unwrap().health.current;
    sim.sound_events.clear();
    run_until(&mut sim, &rules, 3000, |sim| {
        sim.projectiles
            .get(warhead)
            .is_some_and(|bullet| bullet.awaiting_anim())
    });
    // The tick's AI ran at the frame before the one it advanced to.
    let impact_frame = sim.session.binary_frame as i32 - 1;
    let ball = sim
        .projectiles
        .get(warhead)
        .unwrap()
        .awaited_anim()
        .expect("NUKEBALL stands");
    assert_eq!(
        sim.interner.resolve(sim.anim(ball).unwrap().type_id),
        "NUKEBALL"
    );
    let lighting = sim.session.lighting;
    assert!(lighting.nuke_flash_fading_in());
    assert_eq!(lighting.nuke_flash_for_test(), (1, impact_frame, 30));
    assert_eq!(lighting.selected_profile, ScenarioLightingProfile::Nuke);
    assert_eq!(lighting.target_ambient, 200, "the nuke's ambient");
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::SuperWeaponRadarEvent { radar }
            if radar.event_type == RadarEventType::ImpactSilent
                && (radar.rx, radar.ry) == TARGET
    )));
    assert_eq!(
        sim.substrate.entities.get(tank).unwrap().health.current,
        tank_health,
        "not yet detonated"
    );

    // The flash fades in for 30 frames, out for 15, and the lighting returns
    // to the ordinary profile. NUKEBALL's own AI, after the warhead's in the
    // logic order, ends it; the warhead's next AI detonates. The anim's
    // 20-frame life comes from its owner (a Rust regression check, not a
    // native run).
    let ball_live = |sim: &Simulation| {
        sim.anim(ball).is_some() && !sim.substrate.pending_delete.contains(&ball)
    };
    let mut flash_changes = Vec::new();
    let mut last = lighting.nuke_flash_for_test().0;
    let (mut ball_ended_after, mut detonated_after) = (None, None);
    for frames in 1..=600 {
        step(&mut sim, &rules);
        let status = sim.session.lighting.nuke_flash_for_test().0;
        if status != last {
            flash_changes.push((frames, status));
            last = status;
        }
        if ball_ended_after.is_none() && !ball_live(&sim) {
            ball_ended_after = Some(frames);
        }
        if detonated_after.is_none() && sim.projectiles.get(warhead).is_none() {
            detonated_after = Some(frames);
        }
        if detonated_after.is_some() && status == 0 {
            break;
        }
    }
    assert_eq!(flash_changes, vec![(31, 2), (47, 0)]);
    assert_eq!(
        sim.session.lighting.selected_profile,
        ScenarioLightingProfile::Normal
    );
    assert_eq!((ball_ended_after, detonated_after), (Some(20), Some(21)));
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .is_none_or(|entity| entity.dying || entity.health.current == 0),
        "the strike destroys the tank"
    );
    assert!(
        sim.radiation.site_at(TARGET).is_some(),
        "NukePayload's RadLevel"
    );
}

/// A save in the middle of the wait and the flash restores both: the
/// restored world hashes as the original does and then steps alike through
/// the detonation and the flash's end.
#[test]
fn retail_nuke_wait_and_flash_survive_save_and_load() {
    let Some(rules) = retail_rules_binding(&[("NUKEBALL", NUKE_BALL_FRAMES)]) else {
        return;
    };
    let mut sim = Simulation::with_seed(17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let at = ProjectileCoord::new(8 * 256 + 128, 8 * 256 + 128, 0);
    let id = sim.allocate_stable_id();
    let spawn = nuke_spawn(&mut sim.interner, "NUKE", at);
    sim.admit_projectile(id, spawn);
    // The impact as `BulletClass::AI` takes it, then five frames of the wait.
    let never = |_: AnimId| false;
    let context = NukeImpactContext::new(&sim.interner, &|| true, &never);
    let impact = sim
        .projectiles
        .get_mut(id)
        .unwrap()
        .nuke_impact(&context, None, &SharedCellDummy::fresh(), at)
        .unwrap();
    super::nuke::impact(&mut sim, &rules, impact);
    for _ in 0..5 {
        step(&mut sim, &rules);
    }
    assert!(sim.projectiles.get(id).unwrap().awaiting_anim());
    assert!(sim.session.lighting.nuke_flash_fading_in());

    let bytes = crate::sim::snapshot::GameSnapshot::save_validated(&sim, 1, 2, "nuke wait", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    // A save carries no map: the load re-installs the scenario's cells.
    crate::sim::arena_fixture::flat_ground(&mut restored, &rules);
    // Align the control run to the load contract: retail's save reader
    // reinitializes the Scenario RNG, and Bullet Load restarts each bullet's
    // arm timer at the load frame (`0x0046AE9C..0x0046AEB0`).
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let frame = sim.session.binary_frame as i32;
    for (_, bullet) in sim.projectiles.iter_mut() {
        bullet.arm_timer.start(frame, 0);
    }
    let wait = |sim: &Simulation| {
        let bullet = sim.projectiles.get(id).unwrap();
        (
            bullet.awaiting_anim(),
            bullet.awaited_anim(),
            bullet.position,
        )
    };
    assert_eq!(wait(&restored), wait(&sim));
    assert_eq!(restored.session.lighting, sim.session.lighting);
    assert_eq!(restored.state_hash(), sim.state_hash());
    for frame in 1..=60 {
        step(&mut sim, &rules);
        step(&mut restored, &rules);
        assert_eq!(restored.state_hash(), sim.state_hash(), "frame {frame}");
    }
    assert!(sim.projectiles.get(id).is_none(), "detonated");
    assert_eq!(sim.session.lighting.nuke_flash_for_test().0, 0);
}
