//! SOV01's authored zero-HP CASANF10 gap must admit an ordinary transport
//! order after the shared Building AI cleanup. This is a retail-loader/Rust
//! integration check; the original terminal gate has separate native controls.

use super::*;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;

#[test]
#[ignore = "requires retail SOV01 assets"]
fn sov01_transport_crosses_water_below_golden_gate() {
    let retail = retail_dir().expect("active retail install");
    let map = std::env::var("VERA20K_SOV01_MAP").unwrap_or_else(|_| "SOV01UMD.MAP".into());
    let mut scenario = headless_scenario::load(&retail, &map, SEED).expect("retail SOV01");
    let owner_name = prepare_commanding_house(&mut scenario);
    let start = (66, 104);
    let goal = (66, 95);
    let collapsed = scenario
        .sim()
        .entities()
        .values()
        .find(|b| {
            scenario.sim().interner.resolve(b.type_ref()) == "CASANF10"
                && (b.position.rx(), b.position.ry()) == (65, 99)
        })
        .expect("authored collapsed bridge segment")
        .stable_id();
    assert_eq!(
        scenario
            .sim()
            .entities()
            .get(collapsed)
            .unwrap()
            .health
            .current(),
        0
    );
    let id = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        let obj = resources.rules.object("SAPC").expect("retail transport");
        println!(
            "SAPC zone={:?} speed={:?} locomotor={:?}",
            obj.movement_zone, obj.speed_type, obj.locomotor
        );
        simulation
            .spawn_object("SAPC", &owner_name, start.0, start.1, 0, &resources.rules)
            .expect("water beside the bridge is free")
    };
    {
        let sim = scenario.sim();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let receiver = sim
            .foot_entry_receiver(
                id,
                &scenario.runtime.resources.rules,
                Some(&scenario.runtime.resources.overlay_registry),
                None,
            )
            .unwrap();
        let zones = sim
            .zone_grid
            .as_ref()
            .unwrap()
            .map_for(MovementZone::Amphibious)
            .unwrap();
        assert_eq!(
            receiver
                .can_enter(
                    terrain.native_cell_identity((64, 101)),
                    InfantryEntryArgs {
                        direction: -1,
                        height: 0,
                        previous_cell: None
                    }
                )
                .unwrap(),
            7,
            "the intact CASANF11 support remains an obstacle"
        );
        for y in goal.1..=start.1 {
            let c = terrain.cell(start.0, y).unwrap();
            let entry = receiver
                .can_enter(
                    terrain.native_cell_identity((start.0 as i16, y as i16)),
                    InfantryEntryArgs {
                        direction: 0,
                        height: 0,
                        previous_cell: None,
                    },
                )
                .unwrap();
            let building = sim
                .occupancy()
                .first_building_on_layer(
                    start.0,
                    y,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                )
                .and_then(|id| sim.entities().get(id))
                .map(|b| (sim.interner.resolve(b.type_ref()), b.health.current()));
            println!(
                "cell=({}, {y}) land={} zone={} flags={:#x} level={} slope={} groundzone={} entry={entry} building={building:?}",
                start.0,
                c.land_type,
                c.zone_type,
                c.bridge_facts.raw_flags,
                c.level,
                c.slope_type,
                zones.zone_at(
                    start.0,
                    y,
                    crate::sim::movement::locomotor::MovementLayer::Ground
                )
            );
        }
    }
    let owner = scenario.sim().interner.get(&owner_name).unwrap();
    let tick = scenario.sim().session.tick() + 1;
    scenario
        .runtime
        .advance_frame(
            &[CommandEnvelope::new(
                owner,
                tick,
                Command::Move {
                    entity_id: id,
                    target_rx: goal.0,
                    target_ry: goal.1,
                },
            )],
            SIM_TICK_MS,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert!(
        scenario
            .sim()
            .entities()
            .get(collapsed)
            .is_none_or(|b| b.lifecycle.in_limbo()),
        "the initial zero-HP segment must release its foundation on its first AI visit"
    );
    let rows = record_until(&mut scenario, id, goal);
    let visited: Vec<_> = rows
        .iter()
        .map(|row| row.cell)
        .fold(Vec::new(), |mut cells, cell| {
            if cells.last() != Some(&cell) {
                cells.push(cell);
            }
            cells
        });
    println!("transport route: {visited:?}");
    assert_eq!(
        rows.last().map(|row| row.cell),
        Some(goal),
        "transport must reach the far water bank"
    );
    assert!(
        rows.iter()
            .any(|row| (65..=67).contains(&row.cell.0) && (99..=103).contains(&row.cell.1)),
        "order must cross the collapsed CASANF10 gap"
    );
    assert!(
        rows.iter().all(|row| !row.on_bridge),
        "water crossing must stay below the deck"
    );
}
