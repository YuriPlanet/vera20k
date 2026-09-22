//! The teleport locomotor's `[General] WarpOut=` animations are real
//! `AnimClass` instances constructed inside the mover's own object turn.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};

fn rules(bind_warp_art: bool) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nWarpOut=WARPOUT\n\n[InfantryTypes]\n0=CLEG\n\n\
         [CLEG]\nStrength=100\nSpeed=4\nSensorsSight=1\n",
    ))
    .unwrap();
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[WARPOUT]\nFlat=yes\nTranslucent=yes\nRate=120\n",
    ));
    if bind_warp_art {
        art.bind_anim_frame_count_for_test("WARPOUT", 13);
    }
    rules.art_registry = art;
    rules
}

fn relocating_legionnaire(target: (u16, u16)) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    sim.fog.width = 32;
    sim.fog.height = 32;
    let mut entity = GameEntity::test_default(1, "CLEG", "Americans", 5, 5);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("CLEG");
    entity.position.z = 2;
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
    entity.teleport_state = Some(TeleportState {
        phase: TeleportPhase::Relocate,
        target_rx: target.0,
        target_ry: target.1,
        being_warped_ticks: 0,
    });
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = 2;
    assert!(matches!(
        sim.reveal(1),
        super::super::RevealOutcome::Revealed { .. }
    ));
    sim
}

#[test]
fn relocation_constructs_departure_and_arrival_warp_anims_in_the_mover_turn() {
    let rules = rules(true);
    let mut sim = relocating_legionnaire((8, 9));
    let warp_out = sim.intern("WARPOUT");

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let anims: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| anim)
        .filter(|anim| anim.type_id == warp_out)
        .collect();
    assert_eq!(anims.len(), 2, "one at the origin, one at the destination");
    let mut cells: Vec<_> = anims
        .iter()
        .map(|anim| {
            let (rx, ry, _, _, z) = anim.world_coord.to_cell_sub_z();
            (rx, ry, z)
        })
        .collect();
    cells.sort_unstable();
    assert_eq!(cells, vec![(5, 5, 2), (8, 9, 2)]);
    for anim in &anims {
        assert_eq!(anim.draw_flags, 0x600);
        assert_eq!(anim.z_adjust, 0);
        assert_eq!(anim.runtime.delay_remaining, 0);
        assert_eq!(
            anim.runtime.rate_reload,
            crate::rules::art_data::art_rate_to_logic_frames(120),
            "the art section's own Rate=, through the one AnimType rule"
        );
    }
}

#[test]
fn unbound_warp_art_relocates_without_an_anim() {
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));

    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();

    let mover = sim.substrate.entities.get(1).unwrap();
    assert_eq!((mover.position.rx, mover.position.ry), (8, 9));
    assert_eq!(sim.substrate.anims.len(), 0);
}

#[test]
fn teleport_object_turn_moves_retained_foot_neighbor_counts() {
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    use crate::sim::overlay_grid::OverlayGrid;
    let rules = rules(false);
    let mut sim = relocating_legionnaire((8, 9));
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(
        32,
        32,
        (0..32)
            .flat_map(|y| (0..32).map(move |x| test_flat_cell(x, y)))
            .collect(),
    ));
    sim.overlay_grid = Some(OverlayGrid::new_with_retained_wall_plane(32, 32));
    // This fixture's original reveal preceded its map installation. Establish
    // that admitted Unlimbo's counters before executing the real object turn.
    sim.foot_neighbors_after_unlimbo(1, Some(&rules));
    sim.advance_live_object_turn(1, Some(&rules), techno_ai::ObjectAiCtx::default())
        .unwrap();
    let plane = sim
        .overlay_grid
        .as_ref()
        .unwrap()
        .retained_neighbor_counts()
        .unwrap();
    assert_eq!(plane.iter().map(|v| u32::from(*v)).sum::<u32>(), 8);
    assert_eq!(plane[4 * 32 + 4], 0, "released origin neighbors");
    assert_eq!(
        plane[8 * 32 + 7],
        1,
        "destination neighbors see the retained update"
    );
    sim.techno_limbo(1);
    assert!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .retained_neighbor_counts()
            .unwrap()
            .iter()
            .all(|v| *v == 0),
        "Limbo removes the destination source saved by Teleport's callback"
    );
}
