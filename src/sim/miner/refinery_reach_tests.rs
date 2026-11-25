//! The refinery scanner's zone gate (`FUN_004DEE80`, `0x004DEF29..0x004DEFDD`):
//! `MapClass::Can_Reach_Zone` from the cell of the miner's `vt+0x4C`
//! coordinate to the cell of the refinery's GetCoords.

use super::miner_system::find_docking_bay;
use super::pad_nudge_tests::{scene, scene_with_size, spawn};
use super::{CargoBale, MinerState, ResourceType};
use crate::rules::tiberium_type::TiberiumTypeId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::tiberium::test_support::{overlay_registry_with_land, place_tiberium_on_map};
use crate::sim::world::{FrameEffects, Simulation};

/// A War Miner standing in an ore field, a refinery at NW (22, 10) and a
/// farther one at NW (4, 24) on the miner's ground, with the cells `water`
/// flooded after placement. Answers the refinery the scanner picks and the
/// two candidates.
fn pick(water: impl IntoIterator<Item = (u16, u16)>) -> Option<(Option<u64>, u64, u64)> {
    let (mut sim, rules, _) = scene()?;
    let near = spawn(&mut sim, &rules, "NAREFN", "Russians", 22, 10);
    let far = spawn(&mut sim, &rules, "NAREFN", "Russians", 4, 24);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 14, 12);
    crate::sim::arena_fixture::flood(&mut sim, &rules, 40, water);
    *sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(40, 40));
    for y in 11..=13 {
        for x in 13..=15 {
            place_tiberium_on_map(&mut sim, (x, y), ResourceType::Ore, 5);
        }
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let found = find_docking_bay(
        &mut sim,
        &rules,
        miner,
        &rules.object("HARV").unwrap().dock,
        false,
        false,
        &mut crate::sim::world::FrameEffects::default(),
    );
    Some((found, near, far))
}

/// Across water the nearer refinery's zone is unreachable and the scanner
/// takes the farther one. The miner's own Cell is ore, which keeps its zone.
#[test]
fn a_miner_skips_a_refinery_across_water() {
    let Some((found, _, far)) = pick((0..40).flat_map(|y| (18..=20).map(move |x| (x, y)))) else {
        return;
    };
    assert_eq!(found, Some(far));
}

/// The gate measures to the refinery's GetCoords Cell, its foundation centre
/// (`0x00447AC0`: NW + (W·128 − 128, H·128 − 128) leptons, so (24, 11) for
/// the 4x3 NAREFN), not its dock pad (25, 11). Water under that one Cell
/// alone puts the nearer refinery out of reach.
#[test]
fn the_zone_gate_reads_the_refinerys_centre_cell() {
    let Some((found, _, far)) = pick([(24, 11)]) else {
        return;
    };
    assert_eq!(found, Some(far));
}

/// VERA's map-size extension accepts width plus height up to 1024, past the
/// original's 512. On a 300x300 map the 230-cell separation below wraps a
/// signed 32-bit squared distance negative, so the farther refinery reads as
/// nearer. The scanner must take the truly nearest one on a map the original
/// cannot load.
#[test]
fn an_extended_map_picks_the_true_nearest_refinery() {
    let Some((mut sim, rules, _)) = scene_with_size(300) else {
        return;
    };
    // The session stores `[Map] Size` width plus height as the canonical
    // cell-array extent; 300 + 300 is past the original's 512.
    sim.session.set_map_width_for_test(600);
    sim.session.set_map_height_for_test(600);
    // The shared arena's fixed playfield stops short of an extended map's
    // eastern and southern cells, which would refuse the far refinery.
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1000,
        off_100: -1000,
        off_104: 2000,
        off_108: 2000,
    }));
    let near = spawn(&mut sim, &rules, "NAREFN", "Russians", 20, 20);
    let _far = spawn(&mut sim, &rules, "NAREFN", "Russians", 235, 235);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 5, 5);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let found = find_docking_bay(
        &mut sim,
        &rules,
        miner,
        &rules.object("HARV").unwrap().dock,
        false,
        false,
        &mut FrameEffects::default(),
    );
    assert_eq!(found, Some(near));
}

/// A map the original reads (width plus height 512 or less) keeps the native
/// 32-bit distance, even where it wraps: stock play stays as gamemd plays it.
/// On a 256x256 map the same far separation wraps negative, so the scanner
/// still picks the farther refinery.
#[test]
fn a_native_readable_map_keeps_the_original_wrapped_distance() {
    let Some((mut sim, rules, _)) = scene_with_size(256) else {
        return;
    };
    // 256 + 256 is exactly the original's 512 sum boundary.
    sim.session.set_map_width_for_test(512);
    sim.session.set_map_height_for_test(512);
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1000,
        off_100: -1000,
        off_104: 2000,
        off_108: 2000,
    }));
    let _near = spawn(&mut sim, &rules, "NAREFN", "Russians", 20, 20);
    let far = spawn(&mut sim, &rules, "NAREFN", "Russians", 251, 251);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 5, 5);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let found = find_docking_bay(
        &mut sim,
        &rules,
        miner,
        &rules.object("HARV").unwrap().dock,
        false,
        false,
        &mut FrameEffects::default(),
    );
    assert_eq!(found, Some(far));
}

/// #4997's reported game: a 512x512 random map. The loaded War Miner sits on
/// the near refinery's pad while another refinery is 176 cells away; at that
/// separation the native 32-bit squared distance wraps negative, so the
/// original game sends the miner at the near pad to the far refinery (and the
/// one at the far pad back to the near one), the reported ping-pong. On a map
/// only VERA loads, the whole loop must finish at the near refinery: contact
/// it, dock and empty the hold.
#[test]
fn a_512_map_war_miner_returns_to_the_near_refinery_and_deposits() {
    let Some((mut sim, rules, grid)) = scene_with_size(300) else {
        return;
    };
    // A real 512x512 map reports the original's full 1024-cell canonical
    // extent. A 176-cell pair puts `dx² + dy²` past the signed 32-bit range,
    // so the far refinery reads as nearer under the native kernel.
    sim.session.set_map_width_for_test(1024);
    sim.session.set_map_height_for_test(1024);
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1000,
        off_100: -1000,
        off_104: 2000,
        off_108: 2000,
    }));
    *sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(300, 300));
    let near = spawn(&mut sim, &rules, "NAREFN", "Americans", 20, 20);
    let far = spawn(&mut sim, &rules, "NAREFN", "Americans", 196, 196);
    let pad = crate::sim::radio::receive::dock_pad_cell(20, 20);
    let miner = spawn(&mut sim, &rules, "HARV", "Americans", pad.0, pad.1);
    assert_eq!(
        find_docking_bay(
            &mut sim,
            &rules,
            miner,
            &rules.object("HARV").unwrap().dock,
            false,
            false,
            &mut FrameEffects::default(),
        ),
        Some(near),
        "the return pass picks the refinery beside the miner, not the far one"
    );
    let _ =
        sim.substrate
            .mission_assign_exact(miner, MissionId::from_known(MissionType::Harvest), 100);
    arm_full_ore_return(&mut sim, miner, &rules);
    let credits_before = crate::sim::production::credits_for_owner(&sim, "Americans");

    let mut contacted_near = false;
    let mut deposited_at = None;
    for tick in 0..900u32 {
        let commands = sim.take_due_commands();
        sim.advance_tick(
            &commands,
            Some(&rules),
            Some(&grid),
            Some(overlay_registry_with_land()),
            33,
        );
        let entity = sim.substrate.entities.get(miner).expect("miner");
        contacted_near |= entity.seldom.radio_contacts.contains(near);
        assert!(
            !entity.seldom.radio_contacts.contains(far),
            "the miner contacted the far refinery at tick {tick}"
        );
        if entity
            .miner
            .as_ref()
            .expect("miner component")
            .cargo
            .is_empty()
        {
            deposited_at = Some(tick);
            break;
        }
    }
    let deposited_at = deposited_at.expect("the miner docks with the near refinery and unloads");
    assert!(
        contacted_near,
        "the return pass never HELLOed the near refinery"
    );
    let paid = crate::sim::production::credits_for_owner(&sim, "Americans") - credits_before;
    let capacity = sim
        .substrate
        .entities
        .get(miner)
        .and_then(|entity| entity.miner.as_ref())
        .expect("miner component")
        .capacity_bales;
    assert_eq!(
        paid,
        rules
            .tiberium_types
            .get(TiberiumTypeId(0))
            .expect("first Tiberium type")
            .value
            * i32::from(capacity),
        "the whole hold is paid at the near refinery (deposited on tick {deposited_at})"
    );
}

/// The Chrono Miner half of #4997 on the same 512x512 map: a loaded CMIN
/// 23 cells from the near refinery, with the far one 176 cells away so the
/// native 32-bit pair wraps negative. The fixed scan must pick the near
/// refinery, warp the miner onto its pad and unload the hold.
#[test]
fn a_512_map_chrono_miner_warps_to_the_near_refinery_and_deposits() {
    let Some((mut sim, rules, _)) = scene_with_size(300) else {
        return;
    };
    sim.session.set_map_width_for_test(1024);
    sim.session.set_map_height_for_test(1024);
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1000,
        off_100: -1000,
        off_104: 2000,
        off_108: 2000,
    }));
    *sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(300, 300));
    let near = spawn(&mut sim, &rules, "NAREFN", "Americans", 20, 20);
    let _far = spawn(&mut sim, &rules, "NAREFN", "Americans", 196, 196);
    let pad = crate::sim::radio::receive::dock_pad_cell(20, 20);
    let miner = spawn(&mut sim, &rules, "CMIN", "Americans", 43, 43);
    assert_eq!(
        find_docking_bay(
            &mut sim,
            &rules,
            miner,
            &rules.object("CMIN").unwrap().dock,
            false,
            false,
            &mut FrameEffects::default(),
        ),
        Some(near),
        "the Chrono return pass picks the refinery beside it, not the far one"
    );
    let _ =
        sim.substrate
            .mission_assign_exact(miner, MissionId::from_known(MissionType::Harvest), 100);
    arm_full_ore_return(&mut sim, miner, &rules);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .expect("navigation grid");

    let credits_before = crate::sim::production::credits_for_owner(&sim, "Americans");
    let mut warped = false;
    let mut deposited = false;
    for _ in 0..1200u32 {
        let sounds_before = sim.sound_events.len();
        let commands = sim.take_due_commands();
        sim.advance_tick(
            &commands,
            Some(&rules),
            Some(&grid),
            Some(overlay_registry_with_land()),
            33,
        );
        for event in &sim.sound_events[sounds_before..] {
            if let crate::sim::world::SimSoundEvent::ChronoTeleport { world, .. } = event {
                let (rx, ry, ..) = world.to_cell_sub_z();
                warped |= (rx, ry) == pad;
            }
        }
        let entity = sim.substrate.entities.get(miner).expect("miner");
        if entity
            .miner
            .as_ref()
            .expect("miner component")
            .cargo
            .is_empty()
        {
            deposited = true;
            break;
        }
    }
    assert!(
        warped,
        "the dock's MOVE_HERE warps the Chrono Miner onto the pad"
    );
    assert!(deposited, "the warped Chrono Miner unloads its hold");
    let paid = crate::sim::production::credits_for_owner(&sim, "Americans") - credits_before;
    let capacity = sim
        .substrate
        .entities
        .get(miner)
        .and_then(|entity| entity.miner.as_ref())
        .expect("miner component")
        .capacity_bales;
    assert_eq!(
        paid,
        rules
            .tiberium_types
            .get(TiberiumTypeId(0))
            .expect("first Tiberium type")
            .value
            * i32::from(capacity),
        "the whole hold is paid at the near refinery"
    );
}

fn arm_full_ore_return(sim: &mut Simulation, miner: u64, rules: &crate::rules::ruleset::RuleSet) {
    let capacity = sim
        .substrate
        .entities
        .get(miner)
        .and_then(|entity| entity.miner.as_ref())
        .expect("miner component")
        .capacity_bales;
    let entity = sim.substrate.entities.get_mut(miner).expect("miner");
    let bales = (0..capacity)
        .map(|_| {
            CargoBale::of_type(TiberiumTypeId(0), ResourceType::Ore, rules)
                .expect("rules list the first Tiberium type")
        })
        .collect();
    entity.miner.as_mut().expect("miner component").cargo = bales;
    entity
        .mission
        .set_handler_state(MinerState::ReturnToRefinery.cursor());
}
