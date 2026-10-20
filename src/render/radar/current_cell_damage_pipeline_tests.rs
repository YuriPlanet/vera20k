use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::{RadarColorMetadata, ResolvedTerrainCell};
use crate::map::terrain::build_terrain_grid_from_resolved;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::bridge_state::{BridgeDamageEvent, BridgeRuntimeState};
use crate::sim::world::Simulation;

const SIDE: u16 = 64;
const CENTER: (u16, u16) = (25, 25);
const FLOOD: [(u16, u16); 3] = [(26, 25), (26, 24), (26, 23)];
const REPAIR_START: (u16, u16) = (26, 26);

fn bounds() -> PlayfieldBounds {
    PlayfieldBounds {
        base: 40,
        off_fc: 2,
        off_100: 2,
        off_104: 36,
        off_108: 30,
    }
}

fn cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    let in_flood = FLOOD.contains(&(rx, ry));
    ResolvedTerrainCell {
        source_tile_index: if in_flood { 42 } else { 0 },
        final_tile_index: if in_flood { 42 } else { 0 },
        level: 4,
        accepts_smudge: true,
        radar_left: [20, 40, 60],
        radar_right: [7, 8, 9],
        has_damaged_data: in_flood,
        ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
    }
}

fn simulation_fixture() -> (Simulation, crate::map::terrain::TerrainGrid) {
    let mut terrain = crate::map::resolved_terrain::test_grid(SIDE, SIDE, cell);
    // Native572230 selects pavement from the raw tile independently of its
    // overlay-state write. Give synthetic tile42 the retail relative identity
    // BridgeBottomRight1=3 with BridgeSet base40; an overlay alone is not enough.
    terrain.test_set_high_bridge_rim_tiles(
        crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
            40,
            b"[General]\nBridgeTopLeft1=1\nBridgeTopLeft2=2\n\
              BridgeBottomRight1=3\nBridgeBottomRight2=3\n\
              BridgeTopRight1=4\nBridgeTopRight2=5\n\
              BridgeBottomLeft1=6\nBridgeBottomLeft2=6\n\
              BridgeMiddle1=7\nBridgeMiddle2=12\n",
        ),
    );
    // 587180 admits an actual self-anchored overlay24 body, not the former
    // topology-only center with raw flags0/overlay0. Native NS setter is dir0.
    terrain.apply_runtime_bridge_mark_stamp(
        crate::map::bridge_facts::BridgeFlagStamp::new(CENTER, 0, true),
        crate::map::bridge_facts::BridgeStampFamily::Nesw,
    );
    terrain
        .cell_mut_for_test(CENTER.0, CENTER.1)
        .unwrap()
        .bridge_facts
        .overlay_id = Some(24);
    terrain
        .cell_mut_for_test(FLOOD[0].0, FLOOD[0].1)
        .expect("perpendicular anchor terrain cell")
        .bridge_facts
        .raw_flags |= BRIDGE_FLAG_ANCHOR_SELF;
    for &(rx, ry) in &FLOOD {
        terrain.test_set_damaged_radar_metadata(
            rx,
            ry,
            RadarColorMetadata {
                left: [200, 100, 50],
                right: [9, 8, 7],
                valid: true,
            },
        );
    }
    // Ordinary high 0xD1 identities on the perpendicular anchor and the
    // repair start (CellClass+44).
    for (rx, ry) in [FLOOD[0], REPAIR_START] {
        terrain
            .cell_mut_for_test(rx, ry)
            .unwrap()
            .bridge_facts
            .overlay_id = Some(0xD1);
    }
    let grid = build_terrain_grid_from_resolved(&terrain, None);
    let bridge_state = BridgeRuntimeState::from_resolved_terrain_with_map_size(
        &terrain,
        true,
        1500,
        (bounds().base, 40),
    );

    let mut sim = Simulation::new();
    *sim.resolved_terrain_mut_for_test() = Some(terrain);
    *sim.bridge_state_mut_for_test() = Some(bridge_state);
    (sim, grid)
}

#[test]
fn gsi_04_01_bridge_collapse_publishes_variant_preorder_before_setter_radar() {
    let (mut sim, _grid) = simulation_fixture();
    let rules =
        RuleSet::from_ini(&IniFile::from_str("[General]\n")).expect("minimal bridge damage rules");
    sim.resolve_type_handles(&rules);

    let collapsed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: CENTER.0,
            ry: CENTER.1,
            damage: 1,
            warhead_ref: Default::default(),
            is_ion_cannon: true,
            impact_z_leptons: 8 * 104, // Ground level4 plus the structural deck.
        }],
    );

    assert!(collapsed, "Ion retries the state machine through collapse");
    // Native56E990 publishes three per-cell radar callbacks before four
    // setter callbacks; each newly dirty coordinate advances the generation.
    assert_eq!(sim.radar_terrain_dirty_generation(), 7);
    assert_eq!(
        sim.radar_terrain_dirty_cells(),
        [FLOOD.as_slice(), &[CENTER, (25, 24), (25, 23), (25, 26)]].concat(),
    );
    let mut unique = sim.radar_terrain_dirty_cells().to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), sim.radar_terrain_dirty_cells().len());
}
