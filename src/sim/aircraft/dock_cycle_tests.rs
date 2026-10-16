//! The airfield loop through the live frame: a spent AirportBound aircraft
//! on Mission_Guard (`0x0041A5C0`, a human house's) or Mission_AreaGuard
//! (`0x0041A940`, a computer house's) hunts its dock and commences
//! Mission_Enter (`0x00419C80`), which lands it on the pad its radio contact
//! names; the airfield's Mission_Repair (`0x0044C836`) reloads it and
//! releases it to Guard on the pad, still in radio contact. A Target sends
//! it off its pad again (Mission_Guard's Target arm, `0x0041A822`).

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
[TestGun]\nDamage=10\nRange=6\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n\
[TestShot]\nROT=0\n[TestWH]\nVerses=100%\n\
[VICTIM]\nStrength=1000\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
[E1]\nStrength=100\n";

fn mission(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.substrate.entities.get(id)?.mission.current().known()
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
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, human, 0, 10),
    );
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

#[test]
fn a_guarding_aircraft_with_a_target_leaves_its_pad_to_attack() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules");
    let (mut sim, orca, airfield) = spent_orca(&rules, true);
    let victim = sim
        .spawn_object("VICTIM", "Russians", 40, 30, 0, &rules)
        .expect("target");
    let seen = run_cycle(&mut sim, &rules, orca, airfield);
    assert_eq!(seen.guarding_frames, 300, "the aircraft reloads and guards");

    sim.substrate
        .entities
        .get_mut(orca)
        .unwrap()
        .attack_target
        .set_for_test(Some(crate::sim::combat::TargetKind::Entity(victim)));
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
        sim.houses.insert(
            enemy,
            crate::sim::house_state::HouseState::new(enemy, 1, None, true, 0, 10),
        );
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

/// The loop on a retail map through the production loader and the player's
/// order path: two Harriers of a human house, beside its Airforce Command
/// (`GAAIRC`, four docks), strike an enemy Rhino, fly home, land on the pads
/// their radio contacts name, reload and guard there.
#[test]
#[ignore = "physical retail map, rules and art (RA2_DIR)"]
fn retail_harriers_strike_return_reload_and_guard_at_their_airfield() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::runtime::SimRuntime;
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR"));
    let mut scenario =
        crate::headless_scenario::load(&retail, "Death.mmx", 0x5EED_0001).expect("retail map");
    let SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;
    // Both houses human, so no house AI moves the Rhino.
    let mut owners = Vec::new();
    for (index, name) in ["Player", "Enemy"].into_iter().enumerate() {
        let id = sim.interner.intern(name);
        let mut house = HouseState::new(id, index as u8, None, true, 0, 10);
        house.multiplay_passive = true;
        sim.houses.insert(id, house);
        sim.session.house_order.push(id);
        owners.push(id);
    }
    let mut cells: Vec<(u16, u16)> = sim
        .resolved_terrain
        .iter()
        .flat_map(|terrain| terrain.iter().map(|cell| (cell.rx, cell.ry)))
        .collect();
    let (sum_x, sum_y) = cells.iter().fold((0_i64, 0_i64), |(x, y), &(rx, ry)| {
        (x + i64::from(rx), y + i64::from(ry))
    });
    let centre = (sum_x / cells.len() as i64, sum_y / cells.len() as i64);
    let reach = |(rx, ry): (u16, u16)| {
        (i64::from(rx) - centre.0).pow(2) + (i64::from(ry) - centre.1).pow(2)
    };
    cells.sort_by_key(|&cell| (reach(cell), cell));
    // Each object takes the cell nearest the map centre at least `cells_out`
    // cells out that admits it; the airfield a site its owner could build on
    // (`BuildingTypeClass::CanPlaceAt @ 0x00464AC0`).
    let airfield_type = rules.object("GAAIRC").expect("GAAIRC");
    let mut spawn = |name: &str, owner: usize, cells_out: i64| {
        let house = sim.interner.resolve(owners[owner]).to_owned();
        for &(rx, ry) in cells
            .iter()
            .filter(|&&cell| reach(cell) >= cells_out * cells_out)
        {
            let site = (rx as i16, ry as i16);
            if name == "GAAIRC"
                && !crate::sim::build_site::can_place_building_at(
                    sim,
                    rules,
                    None,
                    airfield_type,
                    site,
                    Some(owners[owner]),
                    None,
                )
            {
                continue;
            }
            if let Some(id) = sim.spawn_object(name, &house, rx, ry, 0, rules) {
                return id;
            }
        }
        panic!("no room for {name}")
    };
    let airfield = spawn("GAAIRC", 0, 0);
    let harriers = [spawn("ORCA", 0, 4), spawn("ORCA", 0, 5)];
    let rhino = spawn("HTNK", 1, 18);
    sim.resolve_type_handles(rules);
    let tick = sim.session.tick + 1;
    let orders: Vec<CommandEnvelope> = harriers
        .iter()
        .map(|&attacker_id| {
            CommandEnvelope::new(
                owners[0],
                tick,
                Command::Attack {
                    attacker_id,
                    target_id: rhino,
                },
            )
        })
        .collect();
    let runtime = &mut scenario.runtime;
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
}
