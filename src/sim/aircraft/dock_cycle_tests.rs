//! The airfield loop through the live frame: a spent AirportBound aircraft
//! on Mission_Guard (`0x0041A5C0`, a human house's) or Mission_AreaGuard
//! (`0x0041A940`, a computer house's) hunts its dock and commences
//! Mission_Enter (`0x00419C80`), which lands it on the pad its radio contact
//! names; the airfield's Mission_Repair (`0x0044C836`) reloads it and
//! releases it to Guard on the pad, still in radio contact. A Target sends
//! it off its pad again (Mission_Guard's Target arm, `0x0041A822`), unless
//! the airfield's Guard comes first and its Repair puts it to sleep there.
//! The airfield clears `HasStupidGuardMode=` as retail GAAIRC does, so its
//! Guard runs the UnitReload re-queue (`0x00449970..0x004499B5`).

use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::air_movement;
use crate::sim::world::Simulation;

const RULES: &str = "[General]\nFlightLevel=1500\nReloadRate=.3\n\
[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=VICTIM\n[AircraftTypes]\n0=ORCA\n\
[BuildingTypes]\n0=GAAIRC\n\
[ORCA]\nStrength=150\nSpeed=14\nAmmo=1\nLandable=yes\nAirportBound=yes\nFighter=yes\n\
Primary=TestGun\nDock=GAAIRC\nLocomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
[GAAIRC]\nStrength=1000\nFoundation=2x2\nHelipad=yes\nUnitReload=yes\nNumberOfDocks=4\n\
HasStupidGuardMode=no\n\
[TestGun]\nDamage=10\nRange=6\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n\
[TestShot]\nROT=0\n[TestWH]\nVerses=100%\n\
[VICTIM]\nStrength=1000\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
[E1]\nStrength=100\n";

fn mission(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.substrate.entities.get(id)?.mission.current().known()
}

/// Frames until `id`'s next mission dispatch; 0 or less is the next frame's.
fn frames_to_dispatch(sim: &Simulation, id: u64) -> i32 {
    let timer = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .mission
        .dispatch_timer();
    timer
        .start_frame()
        .wrapping_add(timer.delay())
        .wrapping_sub(sim.session.binary_frame() as i32)
}

/// Runs frames until the next one dispatches `first` and `second`'s next
/// dispatch is `lead` or more frames after it.
fn advance_until_dispatch_leads(
    sim: &mut Simulation,
    rules: &RuleSet,
    first: u64,
    second: u64,
    lead: i32,
) {
    for _ in 0..1000 {
        if frames_to_dispatch(sim, first) <= 0 && frames_to_dispatch(sim, second) >= lead {
            return;
        }
        let _ = sim.advance_tick(&[], Some(rules), None, None, 33);
    }
    panic!("{first} never dispatches {lead} frames before {second}");
}

fn set_target(sim: &mut Simulation, id: u64, target: u64) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .attack_target
        .set_for_test(Some(crate::sim::combat::TargetKind::Entity(target)));
}

/// What a frame shows of the loop.
#[derive(Debug, Default)]
struct Milestones {
    entered: bool,
    linked: bool,
    landed_linked: bool,
    reloaded: bool,
    guarding_frames: u32,
}

fn run_cycle(sim: &mut Simulation, rules: &RuleSet, orca: u64, airfield: u64) -> Milestones {
    let mut seen = Milestones::default();
    for _ in 0..4000 {
        let _ = sim.advance_tick(&[], Some(rules), None, None, 33);
        let entity = sim.substrate.entities.get(orca).expect("aircraft survives");
        let linked = entity.radio_contacts.contains(airfield)
            && sim
                .substrate
                .entities
                .get(airfield)
                .is_some_and(|af| af.radio_contacts.contains(orca));
        let height = air_movement::current_fly_height(entity, sim.resolved_terrain.as_ref());
        seen.entered |= mission(sim, orca) == Some(MissionType::Enter);
        seen.linked |= linked;
        seen.landed_linked |= linked && height == 0;
        let full = entity
            .ammo()
            .is_some_and(|ammo| ammo.current() == ammo.max());
        seen.reloaded |= seen.landed_linked && full;
        if seen.reloaded
            && mission(sim, orca) == Some(MissionType::Guard)
            && height == 0
            && linked
            && !air_movement::tests::fly_moving(entity)
        {
            seen.guarding_frames += 1;
            if seen.guarding_frames == 300 {
                break;
            }
        }
    }
    seen
}

/// An Orca of a human or a computer house, landed and out of Ammo.
fn spent_orca(rules: &RuleSet, human: bool) -> (Simulation, u64, u64) {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, human, 0, 10,
        ));
    let airfield = sim
        .spawn_object_at_height("GAAIRC", "Americans", 10, 10, 0, 0, rules)
        .expect("airfield");
    let orca = sim
        .spawn_object_at_height("ORCA", "Americans", 24, 12, 0, 0, rules)
        .expect("aircraft");
    // Unlimbo's idle mode commenced its pick on the ground: Area Guard for
    // a computer house's armed aircraft, else Guard (`0x00417787`).
    let pick = if human {
        MissionType::Guard
    } else {
        MissionType::AreaGuard
    };
    assert_eq!(mission(&sim, orca), Some(pick));
    let entity = sim.substrate.entities.get_mut(orca).unwrap();
    entity.ammo_mut().unwrap().set_current_for_test(0);
    (sim, orca, airfield)
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn a_spent_aircraft_enters_its_dock_reloads_and_guards_on_the_pad() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules");
    for human in [true, false] {
        let (mut sim, orca, airfield) = spent_orca(&rules, human);
        let seen = run_cycle(&mut sim, &rules, orca, airfield);
        assert!(seen.entered, "it Enters its dock (human {human}): {seen:?}");
        assert!(seen.landed_linked, "it lands in radio contact: {seen:?}");
        assert!(seen.reloaded, "the airfield reloads it: {seen:?}");
        assert_eq!(
            seen.guarding_frames, 300,
            "reloaded, it guards on its pad in contact (human {human}): {seen:?}"
        );
    }
}

/// A Target reaches the guarding aircraft just before its own Guard visit,
/// while the airfield's next Guard visit is four or more frames away. The
/// visit queues Attack (`0x0041A822`); Mission_Attack's state 1
/// (`0x00418031`) assigns it a destination, and Assign_Destination's pad
/// tail sends the airfield OVER_OUT (`0x0041AD86..0x0041ADA6`). Several
/// hundred frames on it still attacks, airborne, and the airfield's later
/// Guard and Repair visits no longer reach it.
#[test]
fn a_guarding_aircraft_with_a_target_leaves_its_pad_to_attack() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules");
    let (mut sim, orca, airfield) = spent_orca(&rules, true);
    let victim = sim
        .spawn_object("VICTIM", "Russians", 40, 30, 0, &rules)
        .expect("target");
    let seen = run_cycle(&mut sim, &rules, orca, airfield);
    assert_eq!(seen.guarding_frames, 300, "the aircraft reloads and guards");

    advance_until_dispatch_leads(&mut sim, &rules, orca, airfield, 4);
    set_target(&mut sim, orca, victim);
    let mut relaunched = false;
    for _ in 0..600 {
        let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
        let entity = sim.substrate.entities.get(orca).expect("aircraft survives");
        if entity.mission.current() == MissionId::from_known(MissionType::Attack)
            && air_movement::tests::fly_moving(entity)
        {
            relaunched = true;
            break;
        }
    }
    assert!(relaunched, "a guarding aircraft holding a Target attacks");
    for _ in 0..300 {
        let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
        assert_eq!(
            mission(&sim, orca),
            Some(MissionType::Attack),
            "it keeps attacking"
        );
        assert!(
            !sim.substrate
                .entities
                .get(airfield)
                .unwrap()
                .radio_contacts
                .contains(orca),
            "off its pad it is no contact of the airfield"
        );
    }
    let entity = sim.substrate.entities.get(orca).unwrap();
    assert!(air_movement::current_fly_height(entity, sim.resolved_terrain.as_ref()) > 0);
}

/// The other order: the airfield's Guard visit comes first and the
/// aircraft's two or more frames later. The UnitReload re-queue
/// (`0x00449970..0x004499B5`) asks the aircraft QUERY_RELOADED, which its
/// Target refuses (`0x00419153`); stopped on its pad it answers NEED_TO_MOVE
/// with ROGER (`0x00419274`), and the airfield queues Repair, which its
/// after-AI ready check commences that frame. The next frame Repair finds the
/// aircraft, not on Enter, answering NEED_TO_MOVE with ROGER and queues Sleep
/// on it (`0x0044C8C9`), and with no Guard visit of its own that frame to
/// queue Attack over it, its Fly Process commences it
/// (`0x004CCB84..0x004CCBAD`). Asleep with its Target and at least half its
/// Ammo it answers each later RELOAD with ROGER (`0x00419109`), so the
/// airfield never releases it and it sleeps on its pad, still holding the
/// Target. Native behavior established by instruction reading of each step.
#[test]
fn an_airfield_guard_first_puts_a_guarding_aircraft_with_a_target_to_sleep() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules");
    let (mut sim, orca, airfield) = spent_orca(&rules, true);
    let victim = sim
        .spawn_object("VICTIM", "Russians", 40, 30, 0, &rules)
        .expect("target");
    let seen = run_cycle(&mut sim, &rules, orca, airfield);
    assert_eq!(seen.guarding_frames, 300, "the aircraft reloads and guards");

    advance_until_dispatch_leads(&mut sim, &rules, airfield, orca, 2);
    assert_eq!(mission(&sim, airfield), Some(MissionType::Guard));
    set_target(&mut sim, orca, victim);
    let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
    assert_eq!(mission(&sim, airfield), Some(MissionType::Repair));
    let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
    // Past two more Repair visits, `ReloadRate=` 270 frames apart.
    for _ in 0..600 {
        let entity = sim.substrate.entities.get(orca).expect("aircraft survives");
        assert_eq!(mission(&sim, orca), Some(MissionType::Sleep));
        assert!(
            entity.attack_target.target().is_some(),
            "it holds its Target"
        );
        assert_eq!(
            air_movement::current_fly_height(entity, sim.resolved_terrain.as_ref()),
            0
        );
        assert!(
            sim.substrate
                .entities
                .get(airfield)
                .unwrap()
                .radio_contacts
                .contains(orca)
        );
        assert_eq!(mission(&sim, airfield), Some(MissionType::Repair));
        let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
}

/// Building4422F9 saves all four contacts before Destroy/Stun clears the
/// live radio links. Its NowDead4425B6 continuation must still enter every
/// aircraft's receiver, including the normal death effects and UnInit.
#[test]
fn an_airfield_killing_hit_destroys_all_four_docked_aircraft() {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::world::FrameEffects;

    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "{RULES}\n[CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
         [Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    )))
    .unwrap();
    let (mut sim, first, airfield) = spent_orca(&rules, true);
    let mut aircraft = vec![first];
    for x in 25..28 {
        let id = sim
            .spawn_object("ORCA", "Americans", x, 12, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .ammo_mut()
            .unwrap()
            .set_current_for_test(0);
        aircraft.push(id);
    }
    let docked = |sim: &Simulation, id| {
        let e = sim.substrate.entities.get(id).unwrap();
        e.radio_contacts.contains(airfield)
            && sim
                .substrate
                .entities
                .get(airfield)
                .unwrap()
                .radio_contacts
                .contains(id)
            && air_movement::current_fly_height(e, sim.resolved_terrain.as_ref()) == 0
            && mission(sim, id) == Some(MissionType::Guard)
            && e.ammo().unwrap().current() == e.ammo().unwrap().max()
            && !air_movement::tests::fly_moving(e)
    };
    for _ in 0..4000 {
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        if aircraft.iter().all(|&id| docked(&sim, id)) {
            break;
        }
    }
    assert!(
        aircraft.iter().all(|&id| docked(&sim, id)),
        "all four pads are occupied"
    );
    // A plane flying nearby without a radio link is not a hosted contact.
    let airborne = sim
        .spawn_object_at_height("ORCA", "Americans", 18, 18, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(airborne)
        .unwrap()
        .position
        .put_z(800);
    assert!(
        !sim.substrate
            .entities
            .get(airfield)
            .unwrap()
            .radio_contacts
            .contains(airborne)
    );
    let c4 = sim.interner.intern("Super");
    sim.commit_direct_damage_receiver(
        &rules,
        None,
        EntityDamageEvent::direct_receiver(
            airfield,
            1000,
            0,
            RAD_NO_ATTACKER,
            None,
            c4,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: true,
            },
        ),
        FrameEffects::default(),
    );
    for id in aircraft {
        assert!(
            sim.substrate.entities.get(id).is_none_or(|e| !e.is_alive()),
            "docked aircraft {id} must die in the building's killing hit",
        );
    }
    assert!(sim.substrate.entities.get(airborne).unwrap().is_alive());
}

/// The native Helipad gate does not test aircraft height. An already-linked
/// incoming plane takes its ordinary receiver and Crash, whose deferred death
/// outputs must reach the outer transaction as well.
#[test]
fn a_linked_airborne_aircraft_crashes_in_the_airfields_damage_transaction() {
    use crate::sim::combat::{
        EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, world_receiver,
    };
    use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
    use crate::sim::world::FrameEffects;

    // A custom C4 reference also guards against hard-coding retail Super.
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "{RULES}\n[CombatDamage]\nC4Warhead=DockC4\n[Warheads]\n0=DockC4\n\
         [DockC4]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    )))
    .unwrap();
    let (mut sim, plane, airfield) = spent_orca(&rules, true);
    sim.substrate
        .entities
        .get_mut(plane)
        .unwrap()
        .position
        .put_z(800);
    assert_eq!(
        radio::transmit(
            &mut sim,
            plane,
            airfield,
            RadioMessage::Hello,
            RadioPayload::default(),
            &rules,
            FrameEffects::default(),
        ),
        RadioResponse::Roger
    );
    assert!(
        sim.substrate
            .entities
            .get(airfield)
            .unwrap()
            .radio_contacts
            .contains(plane)
    );
    let c4 = sim.interner.intern("DockC4");
    let (effects, _) = world_receiver::commit_entities(
        &mut sim,
        &mut world_receiver::ReceiverRun::default(),
        &[EntityDamageEvent::direct_receiver(
            airfield,
            1000,
            0,
            RAD_NO_ATTACKER,
            None,
            c4,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: true,
            },
        )],
        Some(false),
        &rules,
        None,
    );
    let plane = sim
        .substrate
        .entities
        .get(plane)
        .expect("crashing plane remains represented");
    assert_eq!(plane.health.current(), 0);
    assert!(plane.crash.is_crashing());
    assert!(effects.despawned_ids.contains(&plane.stable_id()));
    assert_eq!(
        effects.unit_lost_events.len(),
        1,
        "outer receiver retains the aircraft's announcement"
    );
}

#[test]
fn a_non_helipad_kills_near_contacts_and_releases_distant_contacts() {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::ground_pose;
    use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
    use crate::sim::world::FrameEffects;

    let mut ini = IniFile::from_str(RULES);
    ini.merge(&IniFile::from_str(
        "[GAAIRC]\nHelipad=no\n[CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
         [Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ));
    let rules = RuleSet::from_ini(&ini).unwrap();
    for (delta, killed) in [
        ([255, 0, 0], true),
        ([256, 0, 0], false),
        ([0, 0, 256], false),
    ] {
        let mut sim = Simulation::new();
        let building = sim
            .spawn_object("GAAIRC", "Americans", 10, 10, 0, &rules)
            .unwrap();
        let unit = sim
            .spawn_object("VICTIM", "Americans", 18, 10, 0, &rules)
            .unwrap();
        let centre =
            ground_pose::object_get_coords(sim.substrate.entities.get(building).unwrap(), None);
        sim.substrate
            .entities
            .get_mut(unit)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: centre.x + delta[0],
                y: centre.y + delta[1],
                z: centre.z + delta[2],
            });
        assert_eq!(
            radio::transmit(
                &mut sim,
                unit,
                building,
                RadioMessage::Hello,
                RadioPayload::default(),
                &rules,
                FrameEffects::default(),
            ),
            RadioResponse::Roger
        );
        let c4 = sim.interner.intern("Super");
        sim.commit_direct_damage_receiver(
            &rules,
            None,
            EntityDamageEvent::direct_receiver(
                building,
                1000,
                0,
                RAD_NO_ATTACKER,
                None,
                c4,
                ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
            FrameEffects::default(),
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(unit)
                .is_none_or(|e| !e.is_alive()),
            killed,
            "delta {delta:?}"
        );
    }
}

/// TechnoClass::AI_Update's passive block (`0x006FA65A..0x006FA6EE`) runs
/// for an aircraft too (AircraftClass::AI reaches it through FootClass::AI,
/// `0x00414DA3`). Guarding on its pad, an Orca whose type may acquire
/// passively scans on the targeting timer, with its Scenario draw, but takes
/// nothing: an aircraft has no `+0x3C4` override, so the mask-1 scan's flags
/// word is 0 and the class gate (`0x006F821A`) refuses every candidate.
/// `CanPassiveAquire=no`, which the retail Harrier and Black Eagle carry,
/// refuses the scan at CanAcquireTarget (`0x007091D0`).
#[test]
#[ignore = "slow; run when changing what it covers"]
fn a_guarding_aircraft_scans_passively_unless_its_type_may_not_and_takes_nothing() {
    for passive in [true, false] {
        let text = if passive {
            RULES.to_owned()
        } else {
            RULES.replace("Fighter=yes\n", "Fighter=yes\nCanPassiveAquire=no\n")
        };
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).expect("rules");
        let (mut sim, orca, airfield) = spent_orca(&rules, true);
        let enemy = sim.interner.intern("Russians");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                enemy, 1, None, true, 0, 10,
            ));
        let seen = run_cycle(&mut sim, &rules, orca, airfield);
        assert_eq!(seen.guarding_frames, 300, "the aircraft reloads and guards");
        sim.spawn_object("VICTIM", "Russians", 13, 12, 0, &rules)
            .expect("enemy in range");
        let mut scans = std::collections::BTreeSet::new();
        for _ in 0..300 {
            let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
            let entity = sim.substrate.entities.get(orca).expect("aircraft survives");
            assert_eq!(mission(&sim, orca), Some(MissionType::Guard));
            assert!(
                entity.attack_target.target().is_none(),
                "the scan takes nothing"
            );
            scans.insert(entity.passive_scan.last_scan_frame());
        }
        // Each scan re-arms the timer for the Guard delay plus RandomRanged(0, 2).
        assert_eq!(
            scans.len() > 5,
            passive,
            "CanPassiveAquire={passive}: {scans:?}"
        );
    }
}

/// The loop on stock XMP03T4 through the production loader and the player's
/// order path: two Harriers of a human house, beside its Airforce Command
/// (`GAAIRC`, four docks), strike an enemy Rhino, fly home, land on the pads
/// their radio contacts name, reload and guard there, then die when the
/// airfield takes a killing hit.
#[test]
#[ignore = "physical retail map, rules and art (RA2_DIR)"]
fn retail_harriers_strike_return_reload_and_guard_at_their_airfield() {
    use crate::sim::command::{Command, CommandEnvelope};
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR"));
    let crate::sim::arena_fixture::RetailHarrierDockScene {
        mut runtime,
        player,
        airfield,
        harriers,
        target: rhino,
    } = crate::sim::arena_fixture::retail_harrier_dock_scene_on_map(&retail, "XMP03T4.MAP")
        .expect("retail scene");
    let tick = runtime.simulation.session.tick() + 1;
    let orders: Vec<CommandEnvelope> = harriers
        .iter()
        .map(|&attacker_id| {
            CommandEnvelope::new(
                player,
                tick,
                Command::Attack {
                    attacker_id,
                    target_id: rhino,
                },
            )
        })
        .collect();
    let mut seen = [(); 2].map(|_| Milestones::default());
    let mut fired = [false; 2];
    for frame in 0..9000 {
        let batch = if frame == 0 { &orders[..] } else { &[][..] };
        runtime
            .advance_frame_for_tooling(batch, crate::headless_scenario::SIM_TICK_MS)
            .expect("frame");
        let sim = &runtime.simulation;
        for (n, &harrier) in harriers.iter().enumerate() {
            let entity = sim
                .substrate
                .entities
                .get(harrier)
                .expect("Harrier survives");
            let ammo = entity.ammo().expect("Harrier ammo");
            fired[n] |= ammo.current() < ammo.max();
            let linked = entity.radio_contacts.contains(airfield)
                && sim
                    .substrate
                    .entities
                    .get(airfield)
                    .is_some_and(|af| af.radio_contacts.contains(harrier));
            let height = air_movement::current_fly_height(entity, sim.resolved_terrain.as_ref());
            let seen = &mut seen[n];
            seen.entered |= fired[n] && mission(sim, harrier) == Some(MissionType::Enter);
            seen.linked |= linked;
            seen.landed_linked |= fired[n] && linked && height == 0;
            seen.reloaded |= seen.landed_linked && ammo.current() == ammo.max();
            if seen.reloaded
                && mission(sim, harrier) == Some(MissionType::Guard)
                && height == 0
                && linked
                && !air_movement::tests::fly_moving(entity)
            {
                seen.guarding_frames += 1;
            }
        }
        if seen.iter().all(|seen| seen.guarding_frames >= 300) {
            break;
        }
    }
    for (n, seen) in seen.iter().enumerate() {
        assert!(fired[n], "Harrier {n} fires: {seen:?}");
        assert!(seen.entered, "Harrier {n} Enters its dock: {seen:?}");
        assert!(seen.landed_linked, "Harrier {n} lands linked: {seen:?}");
        assert!(seen.reloaded, "Harrier {n} reloads: {seen:?}");
        assert!(seen.guarding_frames >= 300, "Harrier {n} guards: {seen:?}");
    }
    let rules = &runtime.resources.rules;
    for name in ["GAAIRC", "AMRADR"] {
        let object = rules.object(name).expect("retail airfield");
        assert!(object.helipad, "{name}: the native kill branch is active");
        eprintln!(
            "retail {name}: Helipad={}, NumberOfDocks={}",
            object.helipad, object.number_of_docks
        );
    }
    assert_eq!(rules.object("ORCA").unwrap().strength, 150);
    assert_eq!(rules.object("BEAG").unwrap().strength, 200);
    assert_eq!(rules.bridge_warheads.c4_name, "Super");
    let sim = &mut runtime.simulation;
    let health = sim
        .substrate
        .entities
        .get(airfield)
        .unwrap()
        .health
        .current();
    let c4 = sim.interner.intern(&rules.bridge_warheads.c4_name);
    sim.commit_direct_damage_receiver(
        rules,
        Some(&runtime.resources.overlay_registry),
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            airfield,
            health,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            c4,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: true,
            },
        ),
        crate::sim::world::FrameEffects::default(),
    );
    for harrier in harriers {
        assert!(
            sim.substrate
                .entities
                .get(harrier)
                .is_none_or(|e| !e.is_alive()),
            "retail docked Harrier dies with its airfield"
        );
    }
}
