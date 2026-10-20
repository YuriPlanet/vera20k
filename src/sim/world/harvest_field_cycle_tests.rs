//! War Miner ore-field visits through the production frame (`advance_tick`)
//! on the oracle replay's scene (`harvest_field_oracle_tests::row_scene`):
//! Mission_Harvest state 0 finding ore and driving to it, state 1 cutting a
//! bale per StageClass gate, the hop to the next cell and the return once
//! the field runs dry. The per-step behaviour is pinned row by row against
//! gamemd in `harvest_field_oracle_tests`; these pin how the steps compose
//! over frames.

use super::harvest_field_oracle_tests::{registry, row_scene};
use super::refinery_dock_oracle_tests::Scene;
use crate::sim::miner::MinerState;
use crate::sim::ore_growth::OreGrowthConfig;

/// One miner's observable state after a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    cell: (u16, u16),
    state: Option<MinerState>,
    bales: usize,
    harvesting: bool,
    driving: bool,
}

fn frame(s: &mut Scene) -> Sample {
    let grid = s.sim.path_grid_snapshot();
    s.sim
        .advance_tick(&[], Some(&s.rules), grid.as_deref(), Some(registry()), 67);
    let miner = s.sim.substrate.entities.get(s.miner).unwrap();
    let state = miner.miner.as_ref().unwrap();
    Sample {
        cell: (miner.position.rx(), miner.position.ry()),
        state: miner.miner_state(),
        bales: state.cargo().len(),
        harvesting: state.harvesting(),
        driving: miner.navigation.nav_com().is_some(),
    }
}

/// A War Miner on Mission_Harvest state 0 at `at` with `ore` cells
/// (`[x, y, tiberium, variant, OverlayData]`), no growth or spread, standing
/// on its cell as a production unit does.
fn field(at: (u16, u16), ore: serde_json::Value) -> Scene {
    let mut s = row_scene(&serde_json::json!({
        "name": "field",
        "mission": "harvest",
        "status": 0,
        "miner_cell": [at.0, at.1],
        "unlimbo_at_cell": true,
        "ore": ore,
    }));
    s.sim.ore_growth_config = OreGrowthConfig::disabled();
    s
}

fn bales_at(s: &Scene, cell: (u16, u16)) -> Option<u8> {
    let overlay = s.sim.overlay_grid.as_ref().unwrap().cell(cell.0, cell.1);
    overlay.overlay_id.map(|_| overlay.overlay_data)
}

/// On ore at once: the first dispatch arms the stage (state 0's literal rate
/// 2) and each later bale comes one gate (9 steps of `HarvesterLoadRate` 2,
/// plus the dispatch-before-tick frame) after the last. A cell of density 2
/// gives two bales; the third gate clears it for nothing and the miner hops to
/// the neighbour, where it cuts on the frame after arrival: the stage was
/// never re-armed. With the field empty, state 1 sends it home partly full.
#[test]
fn war_miner_cuts_a_bale_per_gate_hops_without_waiting_and_returns_when_dry() {
    let mut s = field(
        (15, 15),
        serde_json::json!([[15, 15, 0, 0, 2], [16, 15, 0, 0, 2]]),
    );
    let mut samples = Vec::new();
    for _ in 0..400 {
        samples.push(frame(&mut s));
    }
    assert_eq!(
        samples[0].state,
        Some(MinerState::Harvest),
        "on ore at once"
    );
    assert!(samples[0].harvesting);
    let bale_frames: Vec<usize> = (1..samples.len())
        .filter(|&n| samples[n].bales > samples[n - 1].bales)
        .collect();
    assert_eq!(
        bale_frames.len(),
        4,
        "two bales from each cell: {bale_frames:?}"
    );
    // The stage armed at frame 0's dispatch counts 9 steps of 2; the dispatch
    // after the ninth cuts: 19 frames later, and 19 between the two cuts.
    assert_eq!(bale_frames[0], 19);
    assert_eq!(bale_frames[1] - bale_frames[0], 19);
    assert_eq!(
        bales_at(&s, (15, 15)),
        None,
        "the third gate cleared the cell"
    );
    // The hop: the third gate (19 frames after the second cut) sends it east.
    let hop = bale_frames[1] + 19;
    assert!(samples[hop].driving && samples[hop].harvesting);
    assert_eq!(
        samples[hop].state,
        Some(MinerState::Harvest),
        "state 1 holds for the hop"
    );
    let arrival = (hop..samples.len())
        .find(|&n| !samples[n].driving)
        .expect("the hop ends");
    assert_eq!(samples[arrival].cell, (16, 15));
    assert!(
        bale_frames[2] <= arrival + 1,
        "the first bale on the new cell comes on arrival, not a gate later \
         (arrival {arrival}, bale {})",
        bale_frames[2]
    );
    assert_eq!(bale_frames[3] - bale_frames[2], 19);
    let end = samples.last().unwrap();
    assert_eq!(end.bales, 4);
    assert_eq!(
        end.state,
        Some(MinerState::ReturnToRefinery),
        "no ore within TiberiumShortScan: home with a part load"
    );
    assert!(!end.harvesting);
}

/// Frames until a War Miner on `field` has cut its first bale, still cutting.
fn until_first_bale(s: &mut Scene) {
    for _ in 0..60 {
        let sample = frame(s);
        if sample.bales > 0 {
            assert!(sample.harvesting && sample.state == Some(MinerState::Harvest));
            return;
        }
    }
    panic!("no bale");
}

fn order(s: &mut Scene, command: crate::sim::command::Command) {
    assert!(s.sim.apply_command_with_overlays(
        "Americans",
        &command,
        Some(&s.rules),
        Some(registry()),
        crate::sim::world::FrameEffects::default(),
    ));
}

/// A player refinery order on a cutting miner: its mission becomes the
/// order's Enter, and `UnitClass::AI` (`0x007365BB..0x007365D8`) clears
/// Unit+0x6D2 on the next frame, so OREGATH is gone with the byte.
#[test]
fn a_return_order_ends_the_harvesting_byte_and_oregath() {
    let mut s = field((15, 15), serde_json::json!([[15, 15, 0, 0, 5]]));
    until_first_bale(&mut s);
    let miner = s.miner;
    let refinery = s.refinery;
    order(
        &mut s,
        crate::sim::command::Command::MinerReturn {
            entity_id: miner,
            target_refinery_id: Some(refinery),
        },
    );
    let sample = frame(&mut s);
    assert!(!sample.harvesting, "Unit+0x6D2 cleared by the unit AI");
}

/// Physical RULESMD, fixed ARTMD, Battle override and AnyTown scenario through
/// the production source/reader owner. The existing 33x33 field fixture supplies
/// clear terrain and two owned refineries; this is a frame integration test,
/// not a whole-map native comparison.
#[test]
fn retail_war_miner_area_guard_returns_to_harvesting() {
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::sim::mission::MissionType;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, None);
    assert!(retail.rules.object("HARV").unwrap().harvester);
    for on_ore in [true, false] {
        let mut s = super::refinery_dock_oracle_tests::scene_with(
            &serde_json::json!({
                "mission": "guard", "status": 0, "linked": false,
                "miner_cell": [15, 15], "unlimbo_at_cell": true,
            }),
            retail.rules.clone(),
            &retail.processed_rules,
        );
        s.sim.ore_growth_config = OreGrowthConfig::disabled();
        let owner = s.sim.entities().get(s.miner).unwrap().owner();
        s.sim.session.set_current_house_for_test(Some(owner));
        s.sim.houses.reorder_for_test(&[owner]);
        let ore_cell = if on_ore { (15, 15) } else { (16, 15) };
        let ore = registry.id_for_name("TIB01").unwrap();
        let grid = s.sim.overlay_grid.as_mut().unwrap();
        grid.place_overlay(ore_cell.0, ore_cell.1, ore, 10);
        grid.recalculate_runtime_cell(
            s.sim.resolved_terrain.as_mut().unwrap(),
            &registry,
            ore_cell,
        );
        // G's existing selection producer, exact event codec and receiver;
        // no test assigns AreaGuard or Harvest directly.
        let command = s.sim.area_guard_key_command(s.miner, &s.rules).unwrap();
        let record = s.sim.encode_megamission_record(owner, &command).unwrap();
        let command = s.sim.decode_native_command_record(&record, 0).unwrap();
        assert!(s.sim.apply_command_with_overlays(
            "Americans",
            &command.payload,
            Some(&s.rules),
            Some(&registry),
            Default::default(),
        ));
        let mut harvested = false;
        for _ in 0..180 {
            let grid = s.sim.path_grid_snapshot();
            s.sim
                .advance_tick(&[], Some(&s.rules), grid.as_deref(), Some(&registry), 67);
            let miner = s.sim.entities().get(s.miner).unwrap();
            if !miner.miner.as_ref().unwrap().cargo().is_empty() {
                assert_eq!(miner.mission.current().known(), Some(MissionType::Harvest));
                assert_eq!((miner.position.rx(), miner.position.ry()), ore_cell);
                harvested = true;
                break;
            }
        }
        assert!(
            harvested,
            "G must resume harvesting from on_ore={on_ore}; mission {:?}",
            s.sim.entities().get(s.miner).unwrap().mission.current()
        );
    }
}

/// An attack interrupts the same retail miner, then its idle hook resumes work.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_war_miner_attacks_without_cutting_ore_and_resumes_its_idle_mission() {
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::sim::command::Command;
    use crate::sim::house_state::HouseState;
    use crate::sim::mission::{MissionId, MissionType};

    for (on_ore, expected) in [(true, MissionType::Harvest), (false, MissionType::Guard)] {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, None);
        let harv = retail.rules.object("HARV").unwrap();
        assert!(harv.harvester && !harv.weeder);
        assert_eq!(harv.primary(), Some("20mmRapid"));

        let mut s = super::refinery_dock_oracle_tests::scene_with(
            &serde_json::json!({
                "mission": "harvest", "status": 0, "linked": false,
                "miner_cell": [15, 15], "unlimbo_at_cell": true,
            }),
            retail.rules,
            &retail.processed_rules,
        );
        s.sim.ore_growth_config = OreGrowthConfig::disabled();
        let retail_order = |s: &mut Scene, command: Command| {
            assert!(s.sim.apply_command_with_overlays(
                "Americans",
                &command,
                Some(&s.rules),
                Some(&registry),
                crate::sim::world::FrameEffects::default(),
            ));
        };
        if on_ore {
            let ore = registry.id_for_name("TIB01").unwrap();
            let grid = s.sim.overlay_grid.as_mut().unwrap();
            grid.place_overlay(15, 15, ore, 10);
            grid.recalculate_runtime_cell(
                s.sim.resolved_terrain.as_mut().unwrap(),
                &registry,
                (15, 15),
            );
        } else {
            let miner_id = s.miner;
            retail_order(
                &mut s,
                Command::Stop {
                    entity_id: miner_id,
                },
            );
        }
        let step = |s: &mut Scene| {
            let grid = s.sim.path_grid_snapshot();
            s.sim
                .advance_tick(&[], Some(&s.rules), grid.as_deref(), Some(&registry), 67);
        };
        for _ in 0..60 {
            step(&mut s);
            if !on_ore
                || !s
                    .sim
                    .substrate
                    .entities
                    .get(s.miner)
                    .unwrap()
                    .miner
                    .as_ref()
                    .unwrap()
                    .cargo()
                    .is_empty()
            {
                break;
            }
        }
        let bales = s
            .sim
            .substrate
            .entities
            .get(s.miner)
            .unwrap()
            .miner
            .as_ref()
            .unwrap()
            .cargo()
            .len();
        assert_eq!(bales, usize::from(on_ore), "initial field gate");
        let ore_before = bales_at(&s, (15, 15));
        let enemy = s.sim.interner.intern("Russians");
        s.sim
            .houses
            .register(HouseState::new(enemy, 1, None, true, 0, 10));
        let victim = s
            .sim
            .spawn_object("E1", "Russians", 20, 15, 0, &s.rules)
            .unwrap();
        s.sim
            .mission_assign_exact(
                victim,
                MissionId::from_known(MissionType::Sleep),
                s.sim.session.binary_frame(),
            )
            .unwrap();
        let initial_health = s
            .sim
            .substrate
            .entities
            .get(victim)
            .unwrap()
            .health
            .current();
        let miner_id = s.miner;
        retail_order(
            &mut s,
            Command::Attack {
                attacker_id: miner_id,
                target_id: victim,
            },
        );

        let mut damaged = false;
        let mut returned = false;
        for _ in 0..400 {
            step(&mut s);
            if let Some(target) = s.sim.substrate.entities.get(victim) {
                damaged |= target.health.current() < initial_health;
            }
            let miner = s.sim.substrate.entities.get(s.miner).unwrap();
            if miner.mission.current().known() == Some(MissionType::Attack) {
                assert_eq!(
                    miner.miner.as_ref().unwrap().cargo().len(),
                    bales,
                    "Attack does not harvest"
                );
                assert_eq!(
                    bales_at(&s, (15, 15)),
                    ore_before,
                    "Attack leaves the field alone"
                );
                assert!(
                    !miner.miner.as_ref().unwrap().harvesting(),
                    "Unit AI clears OREGATH before fire"
                );
            } else if !s.sim.substrate.entities.contains(victim) {
                assert_eq!(miner.mission.current().known(), Some(expected));
                returned = true;
                break;
            }
        }
        assert!(
            damaged,
            "stock 20mmRapid fired through the production combat host"
        );
        assert!(
            returned,
            "target cleanup reaches the stock harvester idle hook: miner {:?}, victim {:?}",
            s.sim.substrate.entities.get(s.miner).map(|e| (
                e.mission.current(),
                e.mission.queued(),
                e.navigation.nav_com(),
                e.attack_target.target(),
            )),
            s.sim.substrate.entities.get(victim).map(|e| (
                e.health.current(),
                e.death.is_dying(),
                e.is_object_alive(),
                e.infantry_sprite_pose(),
            )),
        );
        if on_ore {
            let mut resumed = false;
            for _ in 0..60 {
                step(&mut s);
                if s.sim
                    .substrate
                    .entities
                    .get(s.miner)
                    .unwrap()
                    .miner
                    .as_ref()
                    .unwrap()
                    .cargo()
                    .len()
                    > bales
                {
                    resumed = true;
                    break;
                }
            }
            assert!(resumed, "promoted Harvest cuts another bale");
        }
    }
}
