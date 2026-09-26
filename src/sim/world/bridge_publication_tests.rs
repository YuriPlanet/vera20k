use super::*;
use crate::map::bridge_facts::{BridgeFlagStamp, BridgeStampFamily};
use crate::rules::ini_parser::IniFile;
use crate::sim::overlay_grid::OverlayGrid;

#[path = "bridge_rim_publication_tests.rs"]
mod rim;

#[path = "bridge_middle_publication_tests.rs"]
mod middle;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
         [Super]\nInfDeath=2\nPenetratesBunker=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap()
}

fn world(rules: &RuleSet, anchor_state: u8) -> Simulation {
    let cells = (0..9)
        .flat_map(|y| {
            (0..9).map(move |x| {
                crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(9, 9, cells);
    terrain.apply_runtime_bridge_mark_stamp(
        BridgeFlagStamp::new((4, 4), 6, true),
        BridgeStampFamily::Nesw,
    );
    let anchor = terrain.native_cell_identity((4, 4));
    terrain.write_native_cell_state(anchor, anchor_state);
    terrain.cells[4 * 9 + 4].bridge_facts.overlay_id = Some(25);
    terrain.cells[4 * 9 + 3].bridge_facts.overlay_id = Some(42);
    let mut grid = OverlayGrid::new(9, 9);
    for y in 0..9 {
        for x in 0..9 {
            let facts = terrain.cell(x, y).unwrap().bridge_facts;
            if let Some(overlay) = facts.overlay_id {
                grid.place_overlay(x, y, overlay, facts.state_byte);
            }
            grid.write_literal_bridge_state(&mut terrain, x, y, facts.state_byte);
        }
    }
    let state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1);
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.bridge_state = Some(state);
    sim.overlay_grid = Some(grid);
    sim
}

fn event(sim: &mut Simulation, cell: (u16, u16)) -> BridgeDamageEvent {
    BridgeDamageEvent {
        rx: cell.0,
        ry: cell.1,
        damage: 2,
        warhead_ref: sim.interner.intern("Super"),
        is_ion_cannon: false,
        impact_z_leptons: 416,
    }
}

#[test]
fn bridge_damage_exact_height_runs_body_and_detaches_only_after_collapse() {
    let rules = rules();
    let mut sim = world(&rules, 9);
    let attacker = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 7, 7, 0, 0, &rules)
        .unwrap();
    sim.reveal(attacker);
    sim.substrate
        .entities
        .get_mut(attacker)
        .unwrap()
        .attack_target = Some(crate::sim::combat::AttackTarget::for_cell(4, 4));
    let mut hit = event(&mut sim, (4, 4));
    let before = sim.scenario_rng.logical_state();
    // Native strict lower bound: level0 +208 refuses, +209 enters.
    hit.impact_z_leptons = 208;
    assert!(!apply_bridge_damage_events(&mut sim, &rules, &[hit]));
    assert_eq!(
        sim.bridge_state
            .as_ref()
            .unwrap()
            .cell(4, 4)
            .unwrap()
            .damage_state,
        DamageState::Healthy { variant: 0 }
    );
    assert_eq!(sim.scenario_rng.logical_state(), before);

    hit.impact_z_leptons = 209;
    assert!(!apply_bridge_damage_events(&mut sim, &rules, &[hit]));
    assert_eq!(
        sim.bridge_state
            .as_ref()
            .unwrap()
            .cell(4, 4)
            .unwrap()
            .damage_state,
        DamageState::Damaged
    );
    assert!(
        sim.substrate
            .entities
            .get(attacker)
            .unwrap()
            .attack_target
            .is_some()
    );

    assert!(apply_bridge_damage_events(&mut sim, &rules, &[hit]));
    assert!(
        sim.substrate
            .entities
            .get(attacker)
            .unwrap()
            .attack_target
            .is_none()
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(4, 4)
            .unwrap()
            .bridge_facts
            .raw_flags
            & 0x100,
        0
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "bridge-damage", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert_eq!(restored.bridge_state.as_ref().unwrap().bridge_strength(), 1);
    assert!(
        restored
            .substrate
            .entities
            .get(attacker)
            .unwrap()
            .attack_target
            .is_none()
    );
}

#[test]
fn signed_bridge_strength_survives_reader_runtime_dispatch_and_snapshot() {
    let native: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_damage_admission.json"
    ))
    .unwrap();
    for strength in [-1, 0, 65536] {
        let parsed = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[CombatDamage]\nBridgeStrength={strength}\n"
        )))
        .unwrap();
        assert_eq!(parsed.bridge_rules.strength, strength);
        let rules = rules();
        let mut sim = world(&rules, 9);
        sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
            sim.resolved_terrain.as_ref().unwrap(),
            true,
            parsed.bridge_rules.strength,
        ));
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "signed-strength", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(
            restored.bridge_state.as_ref().unwrap().bridge_strength(),
            strength
        );
        // Snapshots restore map resources separately; exercise the restored
        // authoritative bridge state against the same retained terrain.
        sim.bridge_state = restored.bridge_state;
        let mut hit = event(&mut sim, (4, 4));
        hit.damage = i32::MAX;
        assert!(!apply_bridge_damage_events(&mut sim, &rules, &[hit]));
        assert_eq!(
            sim.bridge_state.as_ref().unwrap().bridge_strength(),
            strength
        );
        let name = format!("strength_{strength}");
        let row = native["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == name)
            .unwrap();
        let state = sim.scenario_rng.logical_view();
        assert_eq!(
            serde_json::json!([state.index_a, state.index_b]),
            row["rng_indices"],
            "{name}"
        );
        let next: Vec<_> = (0..4).map(|_| sim.scenario_rng.next_u32()).collect();
        assert_eq!(serde_json::json!(next), row["next_rng"], "{name}");
    }
}


fn host<'a>(sim: &'a mut Simulation, rules: &'a RuleSet) -> LivePublication<'a> {
    LivePublication {
        sim,
        rules,
        registry: None,
        collapsed: false,
    }
}

#[test]
fn bridge_publication_production_nonanchor_collapse_keeps_other_overlay_and_runs_c4() {
    let rules = rules();
    let mut sim = world(&rules, 15);
    // The previous cache updater only set transition=true, so F2 can retain
    // that value with native bit0x200 clear. Collapse must publish current bits
    // to navigation even when that particular flag did not change this write.
    let forward2 = sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(2, 4)
        .unwrap();
    assert_eq!(forward2.bridge_facts.raw_flags & 0x200, 0);
    forward2.bridge_transition = true;
    // F3/extra may be ordinary ramp projections: their marker-only writes
    // must not erase walkability while the four structural slots collapse.
    for coord in [(1, 4), (6, 4)] {
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let Cell::Real(index) = terrain.native_cell_identity(coord) else {
            panic!()
        };
        terrain.cells[index].bridge_walkable = true;
        terrain.cells[index].bridge_transition = true;
    }
    let before = crate::sim::pathfinding::PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().unwrap(),
        sim.bridge_state.as_ref(),
    );
    for coord in [(4, 4), (3, 4), (2, 4), (5, 4)] {
        assert!(before.cell(coord.0, coord.1).unwrap().bridge_walkable);
    }
    let tank = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 3, 4, 0, 0, &rules)
        .unwrap();
    sim.reveal(tank);
    let hit = event(&mut sim, (3, 4));
    assert!(apply_bridge_damage_events_with_overlay_registry(
        &mut sim,
        &rules,
        &[hit],
        None
    ));
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .is_none_or(|unit| unit.health.current == 0)
    );
    let grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(
        grid.cell(4, 4).overlay_id,
        None,
        "only the canonical body +44 is cleared"
    );
    assert_eq!(grid.cell(3, 4).overlay_id, Some(42));
    for coord in [(4, 4), (3, 4), (2, 4), (5, 4)] {
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(coord.0, coord.1)
            .unwrap();
        assert_eq!(cell.bridge_facts.raw_flags, 0x400);
        assert_eq!(cell.bridge_facts.state_byte, 0);
        assert!(!cell.has_bridge_deck);
        assert!(!cell.bridge_walkable);
        let path = sim.path_grid().unwrap().cell(coord.0, coord.1).unwrap();
        assert!(
            !path.bridge_walkable,
            "collapsed {coord:?} remains navigable as a deck"
        );
        assert!(!path.transition);
        assert_eq!(grid.cell(coord.0, coord.1).overlay_data, 0);
        assert_eq!(
            sim.dynamic_terrain_cells[&coord],
            DynamicTerrainCellState::capture(cell)
        );
        assert!(
            !sim.bridge_state
                .as_ref()
                .unwrap()
                .cell(coord.0, coord.1)
                .unwrap()
                .deck_present
        );
    }
    for coord in [(1, 4), (6, 4)] {
        let path = sim.path_grid().unwrap().cell(coord.0, coord.1).unwrap();
        assert!(path.bridge_walkable, "marker-only {coord:?} lost its ramp");
        assert!(path.transition);
    }
}

#[test]
fn bridge_publication_retained_anchor_reads_legacy_live_overlay_identity() {
    let rules = rules();
    let mut sim = world(&rules, 15);
    // The existing bridgehead collapse writer clears runtime +44 while the
    // high-surface map/overlay projection still retains its original identity.
    sim.bridge_state
        .as_mut()
        .unwrap()
        .cell_mut(4, 4)
        .unwrap()
        .overlay_byte = 0xff;
    let hit = event(&mut sim, (3, 4));
    assert!(!apply_bridge_damage_events_with_overlay_registry(
        &mut sim,
        &rules,
        &[hit],
        None,
    ));
    let host = host(&mut sim, &rules);
    let anchor = host.terrain().native_cell_identity((4, 4));
    assert_eq!(host.state(anchor), 15);
    assert_ne!(host.flags(anchor) & BRIDGE_FLAG_STRUCTURAL, 0);
    assert_eq!(
        host.terrain().cell(4, 4).unwrap().bridge_facts.overlay_id,
        Some(25)
    );
}

struct ReenteringHost<'a> {
    live: LivePublication<'a>,
    anchor: Cell,
    forward: Cell,
    hits: Vec<(u16, u16)>,
}

impl BridgePublicationHost for ReenteringHost<'_> {
    type Cell = Cell;
    fn lookup(&mut self, p: CellCoord) -> Cell {
        self.live.lookup(p)
    }
    fn coord(&self, c: Cell) -> CellCoord {
        self.live.coord(c)
    }
    fn flags(&self, c: Cell) -> u32 {
        self.live.flags(c)
    }
    fn state(&self, c: Cell) -> u8 {
        self.live.state(c)
    }
    fn write_flags(&mut self, c: Cell, f: u32) {
        self.live.write_flags(c, f);
    }
    fn write_state(&mut self, c: Cell, s: u8) {
        self.live.write_state(c, s);
    }
    fn write_anchor(&mut self, c: Cell, a: Option<Cell>) {
        self.live.write_anchor(c, a);
    }
    fn clear_overlay(&mut self, c: Cell) {
        self.live.clear_overlay(c);
    }
    fn radar(&mut self, c: Cell) {
        self.live.radar(c);
    }
    fn perpendicular(&mut self, p: CellCoord, a: Axis, f: Phase, d: u8) {
        self.live.perpendicular(p, a, f, d);
    }
    fn rim(&mut self, p: CellCoord) {
        self.live.rim(p);
    }
    fn zones(&mut self, c: Cell) {
        self.live.zones(c);
    }
    fn fallout(&mut self, c: Cell) {
        self.live.fallout(c);
        let hit = if c == self.anchor {
            Some((2, 4))
        } else if c == self.forward {
            Some((3, 4))
        } else {
            None
        };
        if let Some(coord) = hit {
            self.hits.push(coord);
            let event = event(self.live.sim, coord);
            let changed = apply_bridge_damage_events_with_overlay_registry(
                self.live.sim,
                self.live.rules,
                &[event],
                None,
            );
            assert!(!changed);
            assert_eq!(
                self.state(self.anchor),
                6,
                "cleared F1 must not fall through to its old topology anchor"
            );
        }
    }
}

#[test]
fn bridge_publication_reentrant_cleared_slot_cannot_reenter_through_old_topology() {
    let rules = rules();
    let mut sim = world(&rules, 15);
    let mut live = host(&mut sim, &rules);
    let anchor = live.lookup((4, 4));
    let forward = live.lookup((3, 4));
    let mut host = ReenteringHost {
        live,
        anchor,
        forward,
        hits: Vec::new(),
    };
    // Native587180 +576BA0: the still-structural F2 can address a retained
    // anchor whose flag was already cleared; after that write F1 must reject.
    publication::set_bridge_direction(&mut host, anchor, 6, false);
    assert_eq!(host.hits, vec![(2, 4), (3, 4)]);
    assert_eq!(host.state(anchor), 6);
    assert_eq!(
        host.live
            .sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(4, 4)
            .overlay_id,
        Some(25)
    );
}

#[test]
fn bridge_publication_perpendicular_uses_raw_tile_instead_of_runtime_class() {
    let rules = rules();
    let mut sim = world(&rules, 9);
    // Native572C90 gates on raw +38. A legacy Bridgehead runtime entry cannot
    // turn this unrelated raw tile into a middle-family tile.
    sim.resolved_terrain.as_mut().unwrap().test_set_high_bridge_rim_tiles(
        crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
            0,
            b"[General]\nBridgeMiddle1=7\nBridgeMiddle2=12\n",
        ),
    );
    let mut cell = super::super::tests::seed_bridge_cell(0);
    cell.role = BridgeCellRole::Bridgehead;
    sim.bridge_state
        .as_mut()
        .unwrap()
        .test_seed_cell(4, 3, cell);
    let original_tile = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(4, 3)
        .unwrap()
        .final_tile_index;
    let mut host = host(&mut sim, &rules);
    host.perpendicular((4, 4), Axis::EW, Phase::DamageB, 0);
    assert_eq!(
        host.sim.bridge_state.as_ref().unwrap().cell(4, 3).unwrap().bridgehead_anchor_class,
        crate::sim::bridge_state::BridgeheadAnchorClass::Variant0
    );
    host.perpendicular((4, 4), Axis::EW, Phase::DamageB, 0);
    assert_eq!(
        host.sim.bridge_state.as_ref().unwrap().cell(4, 3).unwrap().bridgehead_anchor_class,
        crate::sim::bridge_state::BridgeheadAnchorClass::Variant0
    );
    assert_eq!(
        host.terrain().cell(4, 3).unwrap().final_tile_index,
        original_tile
    );
}
