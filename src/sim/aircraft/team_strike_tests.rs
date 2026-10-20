//! A computer house's Harrier team running its retail AIMD script.

use crate::sim::combat::TargetKind;
use crate::sim::mission::MissionType;
use crate::sim::movement::air_movement;
use crate::sim::world::Simulation;

fn mission(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.substrate.entities.get(id)?.mission.current().known()
}

/// What a frame shows of one Harrier.
#[derive(Debug, Default)]
struct Seen {
    took_miner: bool,
    fired: bool,
    entered: bool,
    landed_linked: bool,
    reloaded: bool,
    guarding: bool,
}

/// Retail "Allied Harrier - H2" (`0CACFEFC-G`: four Harriers on "Allied
/// Harrier Attack", `0,3 0,7 49,0 0,6 0,1`) beside its Airforce Command,
/// with an enemy War Miner on the map. Action 0's quarry 3 (harvesters,
/// mask `0x40`) gives the team the miner; every Harrier strikes it once
/// (`Ammo=1`). Once all four are spent the action finishes, the later ones
/// find nothing (quarry 1's mask 0 admits no class for an aircraft) and the
/// script runs past its end, which destroys the team. The Harriers fly home,
/// land on their pads, reload and guard there.
#[test]
#[ignore = "physical retail map, rules and art (RA2_DIR)"]
fn retail_harrier_team_strikes_the_enemy_miner_and_goes_home() {
    use crate::sim::house_state::HouseState;
    use crate::sim::runtime::SimRuntime;
    use crate::sim::team_script_vm::{TeamScriptMember, member_type_identity};
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR"));
    let mut scenario =
        crate::headless_scenario::load(&retail, "Death.mmx", 0x5EED_0001).expect("retail map");
    let SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;
    // A passive computer house runs no base AI or team creation of its own.
    // The enemy is human, so no house AI moves its miner, and not passive,
    // which would keep its objects out of every scan (`0x006F826E`).
    let mut owners = Vec::new();
    for (index, (name, human)) in [("Player", false), ("Enemy", true)].into_iter().enumerate() {
        let id = sim.interner.intern(name);
        let mut house = HouseState::new(id, index as u8, None, human, 0, 10);
        house.set_multiplay_passive_for_test(!human);
        sim.houses.register(house);
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
    // cells out that admits it; a building a site its owner could build on
    // (`BuildingTypeClass::CanPlaceAt @ 0x00464AC0`).
    let mut spawn = |name: &str, owner: usize, cells_out: i64| {
        let house = sim.interner.resolve(owners[owner]).to_owned();
        let object = rules.object(name).expect(name);
        for &(rx, ry) in cells
            .iter()
            .filter(|&&cell| reach(cell) >= cells_out * cells_out)
        {
            let site = (rx as i16, ry as i16);
            if object.category == crate::rules::object_type::ObjectCategory::Building
                && !crate::sim::build_site::can_place_building_at(
                    sim,
                    rules,
                    None,
                    object,
                    site,
                    Some(owners[owner]),
                    None,
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
    let harriers: Vec<u64> = (0..4).map(|n| spawn("ORCA", 0, 4 + n)).collect();
    let miner = spawn("HARV", 1, 18);
    // Under `ShortGame=` a house without a building is defeated at once.
    spawn("NAPOWR", 1, 30);
    sim.resolve_type_handles(rules);
    let team_type = sim.interner.get("0CACFEFC-G").expect("Allied Harrier - H2");
    let candidates: Vec<TeamScriptMember> = harriers
        .iter()
        .map(|&entity_id| TeamScriptMember {
            entity_id,
            member_type: member_type_identity(sim.substrate.entities.get(entity_id).unwrap()),
        })
        .collect();
    let frame = sim.session.binary_frame() as i32;
    let team = sim
        .team_script_vm
        .create_team_from_type(owners[0], team_type, &candidates, frame);
    let runtime = &mut scenario.runtime;
    let mut seen: Vec<Seen> = harriers.iter().map(|_| Seen::default()).collect();
    let mut team_ended = false;
    for _ in 0..2000 {
        runtime
            .advance_frame_for_tooling(&[], crate::headless_scenario::SIM_TICK_MS)
            .expect("frame");
        let sim = &runtime.simulation;
        team_ended |= sim.team_script_vm.team(team).is_none();
        for (seen, &harrier) in seen.iter_mut().zip(&harriers) {
            let entity = sim
                .substrate
                .entities
                .get(harrier)
                .expect("Harrier survives");
            let ammo = entity.ammo().expect("Harrier ammo");
            seen.took_miner |= entity.attack_target.target() == Some(TargetKind::Entity(miner));
            seen.fired |= ammo.current() < ammo.max();
            seen.entered |= seen.fired && mission(sim, harrier) == Some(MissionType::Enter);
            let linked = entity.radio_contacts.contains(airfield);
            let height = air_movement::current_fly_height(entity, sim.resolved_terrain.as_ref());
            seen.landed_linked |= seen.fired && linked && height == 0;
            seen.reloaded |= seen.landed_linked && ammo.current() == ammo.max();
            seen.guarding |= seen.reloaded
                && team_ended
                && mission(sim, harrier) == Some(MissionType::Guard)
                && linked
                && height == 0;
        }
        if seen.iter().all(|seen| seen.guarding) {
            break;
        }
    }
    let sim = &runtime.simulation;
    let miner_health = sim
        .substrate
        .entities
        .get(miner)
        .map(|e| e.health.current());
    assert!(
        miner_health.is_some_and(|health| health < 1000),
        "the strikes hurt the miner: {miner_health:?}"
    );
    assert!(team_ended, "the script ran past its end");
    for (n, seen) in seen.iter().enumerate() {
        assert!(seen.took_miner, "Harrier {n} takes the miner: {seen:?}");
        assert!(seen.fired, "Harrier {n} strikes: {seen:?}");
        assert!(seen.entered, "Harrier {n} Enters its dock: {seen:?}");
        assert!(seen.landed_linked, "Harrier {n} lands linked: {seen:?}");
        assert!(seen.reloaded, "Harrier {n} reloads: {seen:?}");
        assert!(seen.guarding, "Harrier {n} guards on its pad: {seen:?}");
    }
}
