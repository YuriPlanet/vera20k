use super::*;

use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
use crate::map::entities::EntityCategory;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::{RadarColorMetadata, ResolvedTerrainCell};
use crate::map::terrain::build_terrain_grid_from_resolved;
use crate::render::radar::projection::MinimapPlayfieldProjection;
use crate::render::radar::terrain_updates::{
    RadarTerrainUpdateLayers, stage_radar_terrain_dirty_generation,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::TerrainClass;
use crate::sim::bridge_state::{BridgeDamageEvent, BridgeRuntimeState};
use crate::sim::combat::Health;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::runtime::SimRuntime;
use crate::sim::world::Simulation;
use crate::sim::world::TickLane;

const SIDE: u16 = 64;
const CENTER: (u16, u16) = (25, 25);
const FLOOD: [(u16, u16); 3] = [(26, 25), (26, 24), (26, 23)];
const REPAIR_START: (u16, u16) = (26, 26);
const PRISTINE: [u8; 3] = [10, 20, 30];
const DAMAGED: [u8; 3] = [100, 50, 25];

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
        base_terrain_class: TerrainClass::Clear,
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
        .cell_mut(CENTER.0, CENTER.1)
        .unwrap()
        .bridge_facts
        .overlay_id = Some(24);
    terrain
        .cell_mut(FLOOD[0].0, FLOOD[0].1)
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
        terrain.cell_mut(rx, ry).unwrap().bridge_facts.overlay_id = Some(0xD1);
    }
    let grid = build_terrain_grid_from_resolved(&terrain, None, None);
    let bridge_state = BridgeRuntimeState::from_resolved_terrain_with_map_size(
        &terrain,
        true,
        1500,
        (bounds().base, 40),
    );

    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(terrain);
    sim.bridge_state = Some(bridge_state);
    (sim, grid)
}

fn projection(
    sim: &Simulation,
    grid: &crate::map::terrain::TerrainGrid,
) -> MinimapPlayfieldProjection {
    MinimapPlayfieldProjection::derive(
        grid,
        sim.resolved_terrain.as_ref(),
        &[],
        &HashMap::new(),
        "TEMPERATE",
        Some(bounds()),
        Some(CurrentRadarCellAuthority::new(
            sim.resolved_terrain.as_ref(),
            sim.overlay_grid.as_ref(),
            None,
            None,
        )),
    )
}

fn raw_pair(projection: &MinimapPlayfieldProjection, cell: (u16, u16)) -> [[u8; 3]; 2] {
    let geometry = projection.native_radar_surface.expect("native surface");
    let raw = projection
        .native_radar_terrain
        .as_ref()
        .expect("native terrain")
        .raw_rgb();
    let (x, y) = geometry.cell_to_raw_pixel(cell);
    let index = (y * geometry.raw_size().0 + x) as usize;
    [raw[index], raw[index + 1]]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InjectedUploadFailure;

fn present_runtime_projection(
    projection: &mut MinimapPlayfieldProjection,
    runtime: &mut SimRuntime,
    last_generation: &mut u64,
    uploaded_rgba: &mut Vec<u8>,
    fail_upload: bool,
) -> Result<
    crate::app::presentation::render::minimap_transaction::MinimapPresentationCommit,
    InjectedUploadFailure,
> {
    crate::app::presentation::render::minimap_transaction::present_minimap_frame(
        runtime,
        |runtime| {
            let view = runtime.view();
            let (cells, generation) = view.radar_terrain_dirty();
            let staged = stage_radar_terrain_dirty_generation(
                RadarTerrainUpdateLayers {
                    base_rgba: &mut projection.base_rgba,
                    terrain_pixels: &projection.terrain_pixels,
                    surface_pixels: &mut projection.surface_pixels,
                    overlay_pixels: &mut projection.overlay_pixels,
                    native_surface: projection.native_radar_surface,
                    native_terrain: &mut projection.native_radar_terrain,
                },
                CurrentRadarCellAuthority::new(
                    view.resolved_terrain(),
                    view.overlay_grid(),
                    Some(&runtime.resources.overlay_registry),
                    Some(&runtime.resources.rules),
                ),
                [0, 0, 0],
                &HashMap::new(),
                cells,
                generation,
                *last_generation,
            );
            staged.finish_with_upload(last_generation, || {
                if fail_upload {
                    return Err(InjectedUploadFailure);
                }
                uploaded_rgba.clone_from(&projection.base_rgba);
                Ok(())
            })
        },
    )
}

fn production_ini() -> IniFile {
    let mut ini = IniFile::from_str(
        "[InfantryTypes]\n0=ENGI\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=CABHUT\n1=TARGB\n\n\
         [ENGI]\nStrength=75\nArmor=none\nSpeed=4\nPrimary=none\nEngineer=yes\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\n\
         [MTNK]\nStrength=10000\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [CABHUT]\nStrength=200\nArmor=concrete\nFoundation=1x1\nBridgeRepairHut=yes\n\n\
         [TARGB]\nStrength=10000\nArmor=heavy\nFoundation=1x1\n\n\
         [105mm]\nDamage=1501\nROF=50\nRange=6\nWarhead=AP\n\n\
         [AP]\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
         [AudioVisual]\nRepairBridgeSound=BridgeRepaired\n",
    );
    let mut terrain_and_overlays = String::from(
        "[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\n\
         [Road]\nFoot=100%\nTrack=100%\nWheel=100%\n[OverlayTypes]\n",
    );
    for id in 0..=238 {
        terrain_and_overlays.push_str(&format!("{id}=O{id}\n"));
    }
    for id in 0..=238 {
        terrain_and_overlays.push_str(&format!("[O{id}]\nNoUseTileLandType=no\n"));
    }
    ini.merge(&IniFile::from_str(&terrain_and_overlays));
    ini
}

fn spawn(
    sim: &mut Simulation,
    owner_name: &str,
    type_name: &str,
    category: EntityCategory,
    cell: (u16, u16),
    z: u8,
    health: i32,
    facing: u8,
) -> u64 {
    let owner = sim.interner.intern(owner_name);
    let type_ref = sim.interner.intern(type_name);
    let id = sim.allocate_stable_id();
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        cell.0,
        cell.1,
        z,
        facing,
        owner,
        Health::new(health),
        type_ref,
        category,
        0,
        5,
        category == EntityCategory::Unit,
    );
    entity.set_in_playfield(true);
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.reveal(id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    id
}

#[test]
fn gsi_04_01_production_tick_keeps_pavement_damage_through_ordinary_overlay_repair() {
    let (mut sim, grid) = simulation_fixture();
    // This ordinary damage/repair regression starts after map startup. Keep
    // its exact three-cell damage notification outside the independently
    // covered120-frame578100 sweep, which also publishes radar edge changes.
    sim.session.binary_frame = 1;
    let ini = production_ini();
    let rules = RuleSet::from_ini(&ini).expect("production bridge damage/repair rules");
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let attacker = spawn(
        &mut sim,
        "Americans",
        "MTNK",
        EntityCategory::Unit,
        (CENTER.0 - 2, CENTER.1),
        4,
        10_000,
        64,
    );
    // This regression attacks the bridge deck. A ground-level building below
    // it lies outside native Apply_area_damage's structural height window.
    let owner = sim.interner.get("Americans").expect("owner interned");
    sim.fog = crate::sim::vision::FogState::new(SIDE, SIDE);
    crate::sim::vision::reveal_radius(&mut sim.fog, owner, CENTER.0, CENTER.1, 6);
    assert!(sim.apply_command(
        "Americans",
        &Command::ForceAttackCell {
            attacker_id: attacker,
            target_rx: CENTER.0,
            target_ry: CENTER.1,
        },
        Some(&rules),
    ));
    let mut radar = projection(&sim, &grid);
    let mut last_generation = 0;
    assert_eq!(raw_pair(&radar, FLOOD[0]), [PRISTINE; 2]);

    let mut runtime = SimRuntime::from_simulation(sim);
    runtime.resources.overlay_registry =
        crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    runtime.resources.rules = rules;
    let mut fire_count = 0;
    for _ in 0..32 {
        let output = runtime
            .advance_frame(
                &[],
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        fire_count += output.fire_events.len();
        if runtime.simulation.radar_terrain_dirty_generation != 0 {
            break;
        }
    }

    assert_eq!(
        runtime.simulation.radar_terrain_dirty_cells,
        FLOOD,
        "fires={fire_count}, attacker_target={:?}, visible={}, logic={:?}",
        runtime
            .simulation
            .substrate
            .entities
            .get(attacker)
            .and_then(|entity| entity.attack_target.target()),
        runtime
            .simulation
            .fog
            .is_cell_visible(owner, CENTER.0, CENTER.1),
        runtime.simulation.logic_order(),
    );
    assert_eq!(runtime.simulation.radar_terrain_dirty_generation, 3);
    let mut uploaded_rgba = Vec::new();
    let failed = present_runtime_projection(
        &mut radar,
        &mut runtime,
        &mut last_generation,
        &mut uploaded_rgba,
        true,
    );
    assert_eq!(failed, Err(InjectedUploadFailure));
    assert_eq!(last_generation, 0);
    assert!(uploaded_rgba.is_empty());
    assert_eq!(runtime.simulation.radar_terrain_dirty_cells, FLOOD);

    let completed = present_runtime_projection(
        &mut radar,
        &mut runtime,
        &mut last_generation,
        &mut uploaded_rgba,
        false,
    )
    .expect("damage frame composition and upload retry completes");
    assert_eq!(completed.consumed_generation, Some(3));
    assert!(completed.acknowledged);
    assert_eq!(uploaded_rgba, radar.base_rgba);
    for cell in FLOOD {
        assert_eq!(raw_pair(&radar, cell), [DAMAGED; 2]);
    }
    assert!(runtime.simulation.radar_terrain_dirty_cells.is_empty());

    // Supply the ordinary strip's actual Cell overlay identities and native
    // map inputs before the independent repair command. The old fixture had
    // only legacy runtime overlay bytes and no playfield/Foot terrain rows;
    // that cannot enter a hut through Foot+2CC / +1AC or select57F200 repair.
    runtime.simulation.playfield_bounds = Some(bounds());
    runtime.simulation.playfield_size_height = Some(40);
    let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(&ini);
    let clear_costs = terrain_rules
        .semantics_for_land_type(0)
        .unwrap()
        .speed_costs;
    let terrain = runtime.simulation.resolved_terrain.as_mut().unwrap();
    for cell in &mut terrain.cells {
        cell.speed_costs = clear_costs;
        cell.base_speed_costs = clear_costs;
    }
    terrain.test_set_high_bridge_set_starts(Some(40), Some(200));
    crate::map::resolved_terrain::install_ordinary_repair_test_catalog(terrain);
    let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(SIDE, SIDE);
    for point in [FLOOD[0], REPAIR_START] {
        terrain
            .cell_mut(point.0, point.1)
            .unwrap()
            .bridge_facts
            .overlay_id = Some(0xD1);
        overlays.write_bridge_overlay_identity(point.0, point.1, 0xD1);
    }
    runtime.simulation.overlay_grid = Some(overlays);
    let cabhut = spawn(
        &mut runtime.simulation,
        "Soviets",
        "CABHUT",
        EntityCategory::Structure,
        FLOOD[0],
        4,
        200,
        0,
    );
    // Repair runs when the engineer's completed Walk step enters the hut cell
    // (Infantry PerCell 0x519B58); an engineer already standing inside the
    // footprint has no entry step. Start adjacent and walk in. The hut must be
    // the cell's first ground Building (0x47C520), so register its occupancy.
    runtime.simulation.add_entity_occupancy(cabhut);
    let engineer = spawn(
        &mut runtime.simulation,
        "Americans",
        "ENGI",
        EntityCategory::Infantry,
        (FLOOD[0].0 + 1, FLOOD[0].1),
        4,
        75,
        0,
    );
    // The bare test spawn installs no locomotor; production spawns install the
    // type's Walk locomotor, which owns the entry step.
    runtime
        .simulation
        .substrate
        .entities
        .get_mut(engineer)
        .expect("engineer")
        .locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Walk,
        ),
    );
    assert!(
        runtime
            .simulation
            .rebuild_dynamic_navigation(&runtime.resources.rules)
    );
    // The repair order is the player's: Mission_Capture sends a computer
    // house's capturer without a target hunting (`0x004D4C14..0x004D4C6B`).
    let americans = runtime.simulation.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(americans, 0, None, false, 0, 0);
    house.player_control = true;
    runtime.simulation.houses.insert(americans, house);
    assert!(runtime.simulation.apply_command(
        "Americans",
        &Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: cabhut,
        },
        Some(&runtime.resources.rules),
    ));
    for _ in 0..100 {
        let _ = runtime
            .advance_frame(
                &[],
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        if runtime
            .simulation
            .substrate
            .entities
            .get(engineer)
            .is_none()
        {
            break;
        }
    }

    assert!(
        runtime
            .simulation
            .substrate
            .entities
            .get(engineer)
            .is_none(),
        "the engineer enters the hut and is consumed by the repair"
    );
    let repaired = runtime
        .simulation
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(REPAIR_START.0, REPAIR_START.1)
        .unwrap()
        .bridge_facts
        .overlay_id;
    assert!(
        matches!(repaired, Some(205..=208)),
        "ordinary strip changed: {repaired:?}"
    );
    // Original ordinary strip repair has no56E990 clear/radar callback.
    // Its overlay change must not publish pristine pavement or a fake batch.
    assert_eq!(runtime.simulation.radar_terrain_dirty_generation, 3);
    assert!(runtime.simulation.radar_terrain_dirty_cells.is_empty());
    let completed = present_runtime_projection(
        &mut radar,
        &mut runtime,
        &mut last_generation,
        &mut uploaded_rgba,
        false,
    )
    .expect("presentation remains coherent after ordinary overlay repair");
    assert_eq!(completed.consumed_generation, None);
    assert!(!completed.acknowledged);
    for cell in FLOOD {
        assert!(
            runtime
                .view()
                .resolved_terrain()
                .unwrap()
                .pavement_damaged_at(cell.0, cell.1)
        );
        assert_eq!(raw_pair(&radar, cell), [DAMAGED; 2]);
    }
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
    assert_eq!(sim.radar_terrain_dirty_generation, 7);
    assert_eq!(
        sim.radar_terrain_dirty_cells,
        [FLOOD.as_slice(), &[CENTER, (25, 24), (25, 23), (25, 26)]].concat(),
    );
    let mut unique = sim.radar_terrain_dirty_cells.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), sim.radar_terrain_dirty_cells.len());
}
