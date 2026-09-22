//! Cell occupancy, infantry sub-cell, crush, and scatter logic for ground movement.
//!
//! Extracted from movement.rs to keep that file under 600 lines. Contains:
//! - `CellOccupancy` — tracks what entities occupy each cell (vehicles vs infantry sub-cells)
//! - `OccupancyGrid` — persistent per-cell occupancy (see sim/occupancy.rs)
//! - Sub-cell allocation for infantry (spots 2, 3, 4 — max 3 per cell)
//! - Crush checks: Crusher/CrusherAll movement zones vs crushable/omni_crush_resistant
//! - Scatter: issue movement commands to displace friendly blockers (replaces old teleport "bump")
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/entity_store, sim/game_entity, sim/locomotor,
//!   sim/pathfinding, sim/rng, rules/locomotor_type.

use std::collections::BTreeSet;

use crate::sim::cell_kernel::{self, CellQueryPoint};
use crate::sim::pathfinding::{BlockerNeighborCounts, LayeredEntityBlockMap};

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellOccupancy, OccupancyGrid};
use crate::sim::pathfinding::PathGrid;
use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;

/// Functional infantry sub-cell positions. The original engine uses sub-cells
/// 2 (NE), 3 (SW), 4 (SE) — three corners of the isometric diamond. Sub-cells
/// 0 (center) and 1 (NW) are never assigned to infantry by the placement function
/// (FUN_00481180 explicitly skips them: `if (uVar11 != 0 && uVar11 != 1)`).
pub const FUNCTIONAL_SUB_CELLS: [u8; 3] = [2, 3, 4];

/// Maximum infantry that can share one cell (one per functional sub-cell spot).
pub const MAX_INFANTRY_PER_CELL: usize = 3;

/// Determine which sub-cell quadrant a lepton position falls in.
///
/// Returns: 0 (center/NW), 2 (NE), 3 (SW), 4 (SE). Never returns 1.
fn get_subcell_quadrant(sub_x: SimFixed, sub_y: SimFixed) -> u8 {
    cell_kernel::infantry_preferred_spot(CellQueryPoint {
        x: sub_x.to_num::<i32>(),
        y: sub_y.to_num::<i32>(),
    })
}

/// The 8 directional offsets in isometric cell coordinates (dx, dy).
const NEIGHBOR_OFFSETS: [(i32, i32); 8] = [
    (0, -1),  // N
    (1, -1),  // NE
    (1, 0),   // E
    (1, 1),   // SE
    (0, 1),   // S
    (-1, 1),  // SW
    (-1, 0),  // W
    (-1, -1), // NW
];

/// Build the set of cells blocked by entities for pathfinding purposes.
///
/// RA2 key optimization: **moving friendly units are treated as passable terrain**
/// during path calculation. Only stationary units/buildings and enemy units block.
/// This prevents convoy deadlocks and constant repath thrashing in group movement.
///
/// `mover_owner` is the owner of the unit requesting the path.
/// `alliances` is the house alliance graph for friendship checks.
/// Build layer-separated sets of cells blocked by entities for pathfinding.
///
/// Returns `(ground_blocks, bridge_blocks)`. Units on the bridge layer only
/// block bridge pathfinding, and ground units only block ground pathfinding.
/// This enables units to coexist above and below a bridge simultaneously,
/// matching the original engine's `FirstObject`/`AltObject` dual-layer system.
///
/// RA2 cooperative pathfinding: friendly-moving units are recorded in an
/// `entity_block_map` keyed by selected object-list layer and the blocker's
/// current cell, with value equal to the blocker's next cell
/// (movement_target.path[next_index]). The A* cost function walks this map to
/// compute the code-2 dynamic cost per gamemd.exe AStar_compute_edge_cost
/// (0x00429830). Stationary units/buildings and enemies hard-block via the
/// BTreeSet outputs.
///
/// When `rules` is provided, structure footprints are expanded across all
/// occupied cells (foundation + AddOccupy − RemoveOccupy). Without `rules`
/// only the anchor cell is marked, which can let A* route through buildings.
///
/// Returns `(ground_blocks, bridge_blocks, entity_block_map)`.
pub fn build_entity_block_sets(
    entities: &EntityStore,
    mover_owner: &str,
    alliances: &crate::map::houses::HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> (
    BTreeSet<(u16, u16)>,
    BTreeSet<(u16, u16)>,
    LayeredEntityBlockMap,
) {
    // One rule and one fold, shared with the index that keeps these sets
    // current between movement passes (`block_index`). Buildings are always on
    // the ground layer, so the bridge set is empty.
    let (ground_blocked, entity_block_map) = super::block_index::build_owner_block_set(
        entities,
        mover_owner,
        alliances,
        interner,
        rules,
    );
    (ground_blocked, BTreeSet::new(), entity_block_map)
}

/// Build a combined block set (both layers merged) for the flat A* pathfinder
/// which doesn't distinguish layers. Returns `(blocks, entity_block_map)`.
pub fn build_entity_block_set(
    entities: &EntityStore,
    mover_owner: &str,
    alliances: &crate::map::houses::HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> (BTreeSet<(u16, u16)>, LayeredEntityBlockMap) {
    let (ground, bridge, entity_block_map) =
        build_entity_block_sets(entities, mover_owner, alliances, interner, rules);
    (ground.union(&bridge).copied().collect(), entity_block_map)
}

#[cfg(test)]
pub(crate) fn build_blocker_neighbor_counts(
    entities: &EntityStore,
    width: u16,
    height: u16,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> BlockerNeighborCounts {
    build_blocker_neighbor_counts_with_overlays(
        entities,
        width,
        height,
        resolved_terrain,
        None,
        None,
        interner,
        rules,
    )
}

pub(crate) fn build_blocker_neighbor_counts_with_overlays(
    entities: &EntityStore,
    width: u16,
    height: u16,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> BlockerNeighborCounts {
    let mut counts = blocker_plane_base(
        width,
        height,
        resolved_terrain,
        overlay_grid,
        overlay_registry,
    );
    let retained_foot = overlay_grid.is_some_and(|grid| grid.retained_neighbor_counts().is_some());
    for entity in entities.values() {
        if let Some(source) = blocker_plane_source(entity, interner, rules, retained_foot) {
            source.add_to(&mut counts);
        }
    }
    counts
}

/// Retained wall/Foot bytes plus the existing derived terrain contribution.
/// Foot lifecycle writes are authoritative when the retained plane is present;
/// only legacy fixtures without it reconstruct mobile position contributions.
pub(crate) fn blocker_plane_base(
    width: u16,
    height: u16,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> BlockerNeighborCounts {
    let retained_wall_counts = overlay_grid.and_then(|grid| {
        if grid.retained_neighbor_counts().is_some() {
            assert_eq!(
                (grid.width(), grid.height()),
                (width, height),
                "retained wall-neighbor authority must match pathfinding grid"
            );
        }
        grid.retained_neighbor_counts()
    });
    let mut counts = retained_wall_counts
        .map(|plane| BlockerNeighborCounts::from_retained_wall_plane(width, height, plane))
        .unwrap_or_else(|| BlockerNeighborCounts::new(width, height));

    if let Some(terrain) = resolved_terrain {
        for y in 0..height {
            for x in 0..width {
                let Some(cell) = terrain.cell(x, y) else {
                    continue;
                };
                if cell.terrain_object_occupation.is_some() {
                    counts.add_single_cell_neighbor_source(x, y);
                }
            }
        }
    }

    // Legacy constructors have not yet crossed the consumed-once finalized
    // payload boundary. Only they may reconstruct current walls. A retained
    // plane, including an all-zero one, is the sole authored/runtime wall
    // authority and must never be supplemented from final identities.
    if retained_wall_counts.is_none()
        && let (Some(grid), Some(registry)) = (overlay_grid, overlay_registry)
    {
        for y in 0..height {
            for x in 0..width {
                if grid
                    .cell(x, y)
                    .overlay_id
                    .and_then(|id| registry.flags(id))
                    .is_some_and(|flags| flags.wall)
                {
                    counts.add_single_cell_neighbor_source(x, y);
                }
            }
        }
    }
    counts
}

/// What one entity adds to the blocker plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockerPlaneSource {
    Cell(u16, u16),
    /// A building: its foundation expanded by one cell all round.
    Building {
        origin: (u16, u16),
        size: (u16, u16),
    },
}

impl BlockerPlaneSource {
    pub(crate) fn add_to(self, counts: &mut BlockerNeighborCounts) {
        match self {
            Self::Cell(x, y) => counts.add_single_cell_neighbor_source(x, y),
            Self::Building { origin, size } => {
                counts.add_building_expanded_foundation(origin.0, origin.1, size.0, size.1)
            }
        }
    }

    /// The exact inverse of [`Self::add_to`]: the counts wrap.
    pub(crate) fn remove_from(self, counts: &mut BlockerNeighborCounts) {
        match self {
            Self::Cell(x, y) => counts.remove_single_cell_neighbor_source(x, y),
            Self::Building { origin, size } => {
                counts.remove_building_expanded_foundation(origin.0, origin.1, size.0, size.1)
            }
        }
    }
}

/// The one rule for what an entity adds to the blocker plane.
pub(crate) fn blocker_plane_source(
    entity: &GameEntity,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    retained_foot: bool,
) -> Option<BlockerPlaneSource> {
    if retained_foot && entity.category != EntityCategory::Structure {
        return None;
    }
    // Dying corpses are off the occupancy grid: don't let them inflate the
    // A* dynamic-blocker neighbor costs.
    if entity.dying || !entity.lifecycle.cell_marked {
        return None;
    }
    if entity.passenger_role.is_inside_transport() || entity.occupancy_list_layer().is_none() {
        return None;
    }
    let pos = (entity.position.rx, entity.position.ry);
    if entity.category == EntityCategory::Structure {
        let size = rules
            .and_then(|r| r.object(interner.resolve(entity.type_ref())))
            .map(|obj| crate::sim::production::foundation_dimensions(&obj.foundation))
            .unwrap_or((1, 1));
        Some(BlockerPlaneSource::Building { origin: pos, size })
    } else {
        Some(BlockerPlaneSource::Cell(pos.0, pos.1))
    }
}

// ---------------------------------------------------------------------------
// Sub-cell allocation
// ---------------------------------------------------------------------------

/// Find the first available sub-cell in a cell. Returns `None` if the cell is
/// full (3 infantry) or contains a vehicle/structure.
pub fn allocate_sub_cell(occ: Option<&CellOccupancy>, layer: MovementLayer) -> Option<u8> {
    let Some(o) = occ else {
        // Empty cell — first infantry gets sub-cell 2 (NE corner).
        return Some(FUNCTIONAL_SUB_CELLS[0]);
    };
    // Vehicle/structure in cell blocks all sub-cells.
    if o.has_blockers_on(layer) {
        return None;
    }
    let infantry: Vec<(u64, u8)> = o.infantry(layer).collect();
    if infantry.len() >= MAX_INFANTRY_PER_CELL {
        return None;
    }
    // Find first sub-cell not already occupied.
    FUNCTIONAL_SUB_CELLS
        .iter()
        .copied()
        .find(|&spot| !infantry.iter().any(|&(_, s)| s == spot))
}

/// Can infantry enter this cell? True if there's an available sub-cell and no
/// vehicles/structures blocking.
pub fn cell_passable_for_infantry(occ: Option<&CellOccupancy>, layer: MovementLayer) -> bool {
    allocate_sub_cell(occ, layer).is_some()
}

/// Find the first available sub-cell, accounting for both the (stale) occupancy
/// map and sub-cells reserved by earlier movers this tick.
///
/// This prevents duplicate sub-cell assignment when multiple infantry enter
/// the same cell within one simulation tick. Without this, the stale occupancy
/// map shows the cell as empty for all movers, causing overlapping sub-cells
/// and subsequent blocking/repath oscillation.
pub fn allocate_sub_cell_with_reserved(
    occ: Option<&CellOccupancy>,
    layer: MovementLayer,
    reserved: Option<&[u8]>,
) -> Option<u8> {
    // Vehicle/structure in cell blocks all sub-cells.
    if let Some(o) = occ {
        if o.has_blockers_on(layer) {
            return None;
        }
    }
    let infantry: Vec<(u64, u8)> = occ.map_or_else(Vec::new, |o| o.infantry(layer).collect());
    let stale_count: usize = infantry.len();
    let reserved_count: usize = reserved.map_or(0, |v| v.len());
    if stale_count + reserved_count >= MAX_INFANTRY_PER_CELL {
        return None;
    }
    FUNCTIONAL_SUB_CELLS.iter().copied().find(|&spot| {
        let in_stale: bool = infantry.iter().any(|&(_, s)| s == spot);
        let in_reserved: bool = reserved.is_some_and(|v| v.contains(&spot));
        !in_stale && !in_reserved
    })
}

/// Allocate sub-cell using quadrant-based directional preference tables.
///
/// Infantry approaching from a specific direction prefers the sub-cell on that
/// side of the diamond. If occupied, a directional preference table biases the
/// fallback. For center/NW entries, a random rotation picks which sub-cell to
/// try first.
///
/// Use this when the infantry's lepton position (approach direction) and RNG
/// are available. Falls back to `allocate_sub_cell_with_reserved` semantics
/// at call sites without position data (spawning, terrain checks).
pub fn allocate_sub_cell_with_preference(
    occ: Option<&CellOccupancy>,
    layer: MovementLayer,
    reserved: Option<&[u8]>,
    sub_x: SimFixed,
    sub_y: SimFixed,
    rng: &mut SimRng,
) -> Option<u8> {
    // Vehicle/structure blocks all infantry.
    if let Some(o) = occ {
        if o.has_blockers_on(layer) {
            return None;
        }
    }
    let infantry: Vec<(u64, u8)> = occ.map_or_else(Vec::new, |o| o.infantry(layer).collect());
    let stale_count: usize = infantry.len();
    let reserved_count: usize = reserved.map_or(0, |v| v.len());
    if stale_count + reserved_count >= MAX_INFANTRY_PER_CELL {
        return None;
    }

    let quadrant: u8 = get_subcell_quadrant(sub_x, sub_y);
    let occupied_mask = infantry
        .iter()
        .fold(0u8, |mask, &(_, spot)| mask | (1 << spot))
        | reserved
            .into_iter()
            .flatten()
            .fold(0u8, |mask, &spot| mask | (1 << spot));
    // `CellClass::PlaceInfantryInCell` @ `0x00481180` — reached from
    // `WalkLocomotionClass::FindSubCellDest` @ `0x0075C240` — draws its
    // `Random__RandomRanged(0, 3)` row rotation only on the centre/NW path.
    // There is no `CellClass::FindInfantrySubposition` in this program; the name
    // this comment used to carry was invented.
    let random_row = (quadrant == 0).then(|| rng.next_range_u32(4) as u8);
    cell_kernel::select_infantry_subcell(quadrant, occupied_mask, false, random_row)
}

/// Sub-cell placement for a mover whose mission carries placement priority.
///
/// The original engine's placement function takes a `priority` byte; when it is
/// set, control jumps straight past every gate to the offset table and returns
/// `offset[quadrant]` — **no occupancy test, no vehicle/structure blocker test,
/// no garrison test, and no random draw**, because the random row selection sits
/// on the branch the jump skips. Quadrant 0 resolves to the cell *centre* slot,
/// which the ordinary path can never assign.
///
/// The flag is raised for missions Enter, Capture, Eaten, Area Guard and Patrol
/// when the mover's NavCom sits in the cell being entered — an engineer walking
/// into the building it is capturing, a spy into an enemy structure, an
/// infantryman into a garrison, a dog onto the man it is running down.
pub fn priority_sub_cell(sub_x: SimFixed, sub_y: SimFixed) -> u8 {
    get_subcell_quadrant(sub_x, sub_y)
}

/// Recover the functional sub-cell slot a stored lepton destination names.
///
/// The three functional slots sit at distinct lepton offsets, so this is an
/// exact inverse of the slot → offset mapping over `FUNCTIONAL_SUB_CELLS`.
/// Returns `None` for the cell centre and for any other offset.
pub fn functional_sub_cell_from_offset(dest: (SimFixed, SimFixed)) -> Option<u8> {
    FUNCTIONAL_SUB_CELLS
        .iter()
        .copied()
        .find(|&slot| crate::util::lepton::subcell_lepton_offset(Some(slot)) == dest)
}

/// Claim the sub-cell an infantryman already reserved while walking toward this
/// cell — the arrival side of the sub-cell handshake.
///
/// The original engine does **no** sub-cell selection on arrival: the arrival
/// branch passes a null coordinate to the sub-cell chooser, which stores the
/// null destination and returns before reaching the placement function. The slot
/// the man ends up standing in was decided one cell earlier, by the look-ahead
/// placement that ran while he was still in the previous cell. So arrival costs
/// **zero random draws** and re-runs no preference table.
///
/// This mirrors that contract: take the pre-reserved slot when it is still free,
/// otherwise fall back to the deterministic first-free scan. `self_id` is
/// excluded from the occupancy test because the caller has already moved the
/// mover into this cell carrying its previous slot. Neither path touches the RNG
/// — that is the point of the function.
pub fn claim_reserved_sub_cell(
    occ: Option<&CellOccupancy>,
    layer: MovementLayer,
    self_id: u64,
    preferred: Option<u8>,
) -> Option<u8> {
    if let Some(o) = occ
        && o.has_blockers_on(layer)
    {
        return None;
    }
    let others: Vec<u8> = occ.map_or_else(Vec::new, |o| {
        o.infantry(layer)
            .filter(|&(id, _)| id != self_id)
            .map(|(_, slot)| slot)
            .collect()
    });
    if let Some(slot) = preferred
        && FUNCTIONAL_SUB_CELLS.contains(&slot)
        && !others.contains(&slot)
    {
        return Some(slot);
    }
    if others.len() >= MAX_INFANTRY_PER_CELL {
        return None;
    }
    FUNCTIONAL_SUB_CELLS
        .iter()
        .copied()
        .find(|spot| !others.contains(spot))
}

// ---------------------------------------------------------------------------
// Crush logic
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrushCapability {
    pub regular_crusher: bool,
    pub omni_crusher: bool,
}

impl CrushCapability {
    pub const fn new(regular_crusher: bool, omni_crusher: bool) -> Self {
        Self {
            regular_crusher,
            omni_crusher,
        }
    }

    pub const fn can_crush_units(self) -> bool {
        self.regular_crusher || self.omni_crusher
    }

    /// The one crush authority for an object: its type's `Crusher=`
    /// (`UnitTypeClass+0xD28`) and `OmniCrusher=`. Every path search and every
    /// crossing reads it through here. Native has no per-caller flag: each
    /// `Can_Enter_Cell` evaluation reads the type (`0x0073F438..F446` for the
    /// wall arm's crusher route, `0x0073FB2A..FB6C` for the occupant crush
    /// latch). `MovementZone=CrusherAll` is a separate wall-arm route
    /// (`0x0073F465`) that `cell_entry` keys on the zone, not on this flag.
    pub const fn of(entity: &GameEntity) -> Self {
        Self::new(entity.regular_crusher, entity.omni_crusher)
    }

    /// The key of the wall arm's crusher route: `Crusher=` alone
    /// (`UnitTypeClass+0xD28`, read at `0x0073F438`). Every cell-entry context
    /// built for a known mover takes it from here.
    pub const fn wall_arm_crusher(self) -> bool {
        self.regular_crusher
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveCrushPhase {
    EnteringCell,
    FullyInCell,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriveCrushOutcome {
    None,
    Scatter { blockers: Vec<u64> },
    Kill { victims: Vec<u64> },
}

pub const CRUSH_DISTANCE_SQ_LIMIT: i64 = 0x3fff;

pub fn within_crush_distance_sq(crusher: (i32, i32), victim: (i32, i32)) -> bool {
    let dx = i64::from(victim.0 - crusher.0);
    let dy = i64::from(victim.1 - crusher.1);
    dx * dx + dy * dy <= CRUSH_DISTANCE_SQ_LIMIT
}

fn entity_crush_coord(entity: &GameEntity) -> (i32, i32) {
    (
        i32::from(entity.position.rx) * 256 + entity.position.sub_x.to_num::<i32>(),
        i32::from(entity.position.ry) * 256 + entity.position.sub_y.to_num::<i32>(),
    )
}

/// Cell-entry legality is classified where the sim frame is not threaded, so the
/// Iron Curtain gate cannot be evaluated there.
///
/// **Recorded residual, and the binary side is now settled rather than assumed.**
/// `UnitClass::Can_Enter_Cell` reaches the crush predicate from two sites, and
/// both call the same shared `CanCrushCheck` the kill site uses — the one whose
/// last gate, on the omni path *and* the ordinary path, is the Iron Curtain
/// slot. So retail refuses entry to a curtained infantryman's cell and the tank
/// routes around; VERA reads it as crushable for path legality, enters, and then
/// fails to kill.
///
/// Closing it means threading a sim frame down through the whole cell-entry
/// classifier, which is also the frame-independent path-planning predicate, so
/// it is left unfunded here rather than half-plumbed. Frequency: only while an
/// Iron Curtain is up over infantry a vehicle is pathing through — a few seconds
/// per curtain use, several times a match for a Soviet player, never otherwise.
const IRON_CURTAIN_UNAVAILABLE: bool = false;

/// The victim-side inputs of the native crush predicate.
///
/// Exactly five type-level facts and two instance facts enter the decision:
/// `Crushable=`, `OmniCrushResistant=`, the victim's class id, the ally
/// relation (tested by the caller), the deploy crush-immunity byte, and the
/// Iron Curtain timer. There is no weight, size, armour or `TypeImmune=` term.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrushTarget {
    pub category: EntityCategory,
    /// `Crushable=` — defaults **yes** for infantry types, no for everything else.
    pub crushable: bool,
    /// The deploy crush-immunity instance byte: raised on deploy for exactly the
    /// infantry types carrying `DeployedCrushable=no`, cleared on undeploy.
    pub deploy_crush_immune: bool,
    /// `OmniCrushResistant=`.
    pub omni_crush_resistant: bool,
    /// Iron Curtain (or Force Shield) currently active on the victim.
    pub iron_curtained: bool,
}

impl CrushTarget {
    /// Read the crush inputs off a live entity at a known sim frame.
    pub fn from_entity(entity: &GameEntity, current_frame: u32) -> Self {
        Self {
            category: entity.category,
            crushable: entity.crushable,
            deploy_crush_immune: deploy_crush_immune(entity),
            omni_crush_resistant: entity.omni_crush_resistant,
            iron_curtained: crate::sim::superweapon::invulnerability::is_invulnerable(
                entity.invulnerability.as_ref(),
                current_frame,
            ),
        }
    }

    /// Read the crush inputs where no sim frame is available — cell-entry
    /// legality only. See [`IRON_CURTAIN_UNAVAILABLE`].
    fn from_entity_without_frame(entity: &GameEntity) -> Self {
        Self {
            category: entity.category,
            crushable: entity.crushable,
            deploy_crush_immune: deploy_crush_immune(entity),
            omni_crush_resistant: entity.omni_crush_resistant,
            iron_curtained: IRON_CURTAIN_UNAVAILABLE,
        }
    }
}

/// Whether a crusher with `capability` can crush `target`.
///
/// Mirrors the two blocks of the native predicate:
///
/// 1. **Omni path** — an `OmniCrusher=` crusher crushes any non-building,
///    non-ally, non-`OmniCrushResistant=`, non-Iron-Curtained victim, ignoring
///    `Crushable=` entirely (stock: only the Battle Fortress).
/// 2. **Ordinary path** — the victim must be `Crushable=`, must not carry the
///    deploy crush-immunity byte, must not be an ally, and must not be Iron
///    Curtained.
///
/// The Iron Curtain test is the **last** gate of each block, after the ally
/// test, which is why it cannot be hoisted to the top.
///
/// The category exclusions below are **VERA-internal, gamemd equivalent
/// UNCHECKED**. The original's class test sits in the omni block only; the
/// ordinary block reads the type's crushable byte, an abstract flag, the deploy
/// byte, the ally test and the Iron Curtain slot, and nothing else. Stock
/// `rulesmd` marks sandbags and all three fence walls `Crushable=yes`, so a
/// retail Crusher flattens them and VERA's category gate does not. That gate
/// predates this function; it is recorded here rather than credited to the
/// original. Aircraft never appear in the ground occupant list, so excluding
/// them is outcome-identical.
/// NO-DIFF (GSI-08.17) — pass 1's named gap is not one. A crushed unit's
/// `DeathWeapon=` does not fire in gamemd either:
/// `TechnoClass::Fire_Death_Weapon @ 0x0070D690` has exactly two callers,
/// `ReceiveDamage @ 0x00701900` and `FlyLocomotionClass::Process @ 0x004CD600`,
/// and the crush loop at `0x007416A0` enters neither. Detonating a crushed
/// Terrorist would be a regression, not a fix. Native's crush consequences are
/// the crusher-positioned `CrushSound`, `FreeAllMindControlCaptures`,
/// `Record_The_Kill` (score, trigger events, EVA and the crusher's veterancy,
/// which this engine now pays), then unmark, limbo and `UnInit` — no anim, no
/// smudge, no RNG.
///
/// RESIDUAL (GSI-08.17) — two real gaps sit underneath it.
/// - **Overlay crushing — CLOSED.** `Per_Cell_Process @ 0x0073AFD4` flattens any
///   overlay whose `ObjectTypeClass::Crushable=` (`+0x22D`) is set, and a
///   `MovementZone=CrusherAll` type (stock: the Battle Fortress alone) also
///   flattens `Wall=yes` overlays. Both halves are implemented:
///   `Simulation::apply_wall_crush_on_driveover` landed the `Crushable=` half
///   (PR #375) and the `CrusherAll` gate (`+0x5B4 == 0xC` at `0x0073B02D`).
///   This block previously described the rule correctly while listing it as
///   open, and the gate it describes was for a while implemented as a Drive
///   locomotor test instead.
/// - **Mind-control release.** A crushed controller does not free its captives
///   here, so their ownership stays with a dead object.
pub fn can_crush(capability: CrushCapability, target: CrushTarget) -> bool {
    // Structures and aircraft are never crushed.
    if matches!(
        target.category,
        EntityCategory::Structure | EntityCategory::Aircraft
    ) {
        return false;
    }
    // Omni path: OmniCrushResistant blocks it, Iron Curtain ends it.
    if capability.omni_crusher {
        return !target.omni_crush_resistant && !target.iron_curtained;
    }
    // Ordinary path. OmniCrushResistant is not read by this block in the
    // original, but every stock OmniCrushResistant type is a vehicle and the
    // ordinary block only ever passes infantry, so keeping the guard here is
    // outcome-identical and cheaper than a category re-test.
    if target.omni_crush_resistant {
        return false;
    }

    capability.regular_crusher
        && target.category == EntityCategory::Infantry
        && target.crushable
        && !target.deploy_crush_immune
        && !target.iron_curtained
}

/// The deploy crush-immunity byte of the native `TechnoClass` instance.
///
/// The only writers in the binary are the `TechnoClass` constructor (clear) and
/// the infantry deploy sequencer, which raises the byte on deploy **only** when
/// the type carries `DeployedCrushable=no` and clears it again on undeploy.
/// `DeployedCrushable=` itself defaults to yes, so stock YR has exactly one
/// crush-immune-on-deploy type: the Guardian GI. A deployed GI is crushable.
///
/// Prone has **no** write site at this offset anywhere in the binary, so lying
/// down never confers crush immunity.
fn deploy_crush_immune(entity: &GameEntity) -> bool {
    if entity.category != EntityCategory::Infantry {
        return false;
    }
    matches!(
        entity.deploy_state,
        Some(crate::sim::deploy::DeployPhase::Deployed)
            | Some(crate::sim::deploy::DeployPhase::Undeploying { .. })
    ) && !entity.deployed_crushable
}

/// Collect entity IDs in a cell that the mover would crush on entry.
///
/// Who is crushing, for the ally question native asks at admission.
///
/// `Is_Crushable_By 0x005F6CD0` tests `HouseClass::Is_Ally_ByObject 0x004F9A90`
/// on the victim's owning house (`+0x21C`) at `0x005F6D1A` and again at
/// `0x005F6D67`, and `Can_Enter_Cell` re-tests at `0x0073FB53`, so an allied or
/// own crushable never reaches the crush latch at all.
///
/// Carried as a required parameter, not an `Option`: a missing-alliance default
/// would fail open into the very case this exists to refuse.
#[derive(Clone, Copy)]
pub struct CrushAllyGate<'a> {
    /// The crusher's owning house, already resolved by the caller.
    crusher_owner: &'a str,
    alliances: &'a crate::map::houses::HouseAllianceMap,
    interner: &'a crate::sim::intern::StringInterner,
}

impl<'a> CrushAllyGate<'a> {
    pub fn new(
        crusher_owner: &'a str,
        alliances: &'a crate::map::houses::HouseAllianceMap,
        interner: &'a crate::sim::intern::StringInterner,
    ) -> Self {
        Self {
            crusher_owner,
            alliances,
            interner,
        }
    }

    /// True when native would spare this victim for being allied or own.
    ///
    /// The one predicate for the whole crush mechanism: admission asks it here
    /// and so does the kill site, because the two disagreeing was the defect.
    ///
    /// No id fast path. An earlier version carried the crusher's `InternedId`
    /// to settle own-house without allocating, which was wasted work:
    /// `are_houses_friendly` already returns on a case-insensitive name compare
    /// *before* it normalizes anything (`houses.rs`), so own-house never
    /// allocated. The allocating case is a genuinely foreign house, which an id
    /// compare cannot shortcut anyway.
    pub fn spares(&self, victim: &GameEntity) -> bool {
        crate::map::houses::are_houses_friendly(
            self.alliances,
            self.crusher_owner,
            self.interner.resolve(victim.owner()),
        )
    }
}

/// Returns an empty vec if the mover can't crush anything there.
pub fn collect_crush_victims(
    cell: (u16, u16),
    occupancy: &OccupancyGrid,
    layer: MovementLayer,
    crush_capability: CrushCapability,
    entities: &EntityStore,
    ally_gate: CrushAllyGate<'_>,
) -> Vec<u64> {
    // A mover that crushes nothing has no victims, so it must not pay an
    // alliance test per occupant. Without this, every occupant of every occupied
    // cell evaluated by a NON-crusher started paying one - and this runs per
    // mover per step at the 20k target.
    if !crush_capability.can_crush_units() {
        return Vec::new();
    }
    let Some(occ) = occupancy.get(cell.0, cell.1) else {
        return Vec::new();
    };
    let mut victims: Vec<u64> = Vec::new();

    for occupant in occ.iter_layer(layer) {
        if let Some(e) = entities.get(occupant.entity_id) {
            if ally_gate.spares(e) {
                continue;
            }
            if can_crush(crush_capability, CrushTarget::from_entity_without_frame(e)) {
                victims.push(occupant.entity_id);
            }
        }
    }

    victims
}

/// Emit the normal crush-path `EntityCrushed` (CrushSound) event for a single
/// victim. Native crush teardown does not also enter the ordinary DieSound
/// path. The event is skipped when CrushSound is absent. Caller must invoke BEFORE
/// removing the victim from the EntityStore so victim.position and
/// victim.type_ref are still valid.
#[cfg(test)]
pub fn emit_crush_kill_sounds(
    victim: &crate::sim::game_entity::GameEntity,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &mut crate::sim::intern::StringInterner,
    sound_events: &mut Vec<crate::sim::world::SimSoundEvent>,
) {
    emit_crush_kill_sounds_at(
        victim,
        (i32::from(victim.position.rx), i32::from(victim.position.ry)),
        rules,
        interner,
        sound_events,
    );
}

pub fn emit_crush_kill_sounds_at(
    victim: &crate::sim::game_entity::GameEntity,
    crush_coord: (i32, i32),
    rules: &crate::rules::ruleset::RuleSet,
    interner: &mut crate::sim::intern::StringInterner,
    sound_events: &mut Vec<crate::sim::world::SimSoundEvent>,
) {
    let rx = crush_coord.0.clamp(0, i32::from(u16::MAX)) as u16;
    let ry = crush_coord.1.clamp(0, i32::from(u16::MAX)) as u16;
    let type_str = interner.resolve(victim.type_ref()).to_string();
    let Some(obj) = rules.object(&type_str) else {
        return;
    };
    if let Some(ref crush_sound) = obj.crush_sound {
        let id = interner.intern(crush_sound);
        sound_events.push(crate::sim::world::SimSoundEvent::EntityCrushed {
            crush_sound_id: id,
            rx,
            ry,
        });
    }
}

/// Check whether a mover can enter a cell after crushing all occupants.
///
/// Returns `true` if the mover can crush everything in the cell (i.e. the cell
/// would become empty after crush kills are applied).
pub fn cell_passable_after_crush(
    cell: (u16, u16),
    occupancy: &OccupancyGrid,
    layer: MovementLayer,
    crush_capability: CrushCapability,
    entities: &EntityStore,
    ally_gate: CrushAllyGate<'_>,
) -> bool {
    let Some(occ) = occupancy.get(cell.0, cell.1) else {
        return true; // empty cell
    };
    // Boolean crush passability is category-specific; it does not choose a
    // first occupant from CellClass list order.
    // All blockers must be crushable.
    for eid in occ.blockers(layer) {
        if let Some(e) = entities.get(eid) {
            // An allied blocker is not crushable, so the cell is not passable
            // by crushing it - native answers 6 or 2 here, not the crush latch.
            if ally_gate.spares(e)
                || !can_crush(crush_capability, CrushTarget::from_entity_without_frame(e))
            {
                return false;
            }
        }
    }
    // All infantry must be crushable.
    for (eid, _) in occ.infantry(layer) {
        if let Some(e) = entities.get(eid) {
            if ally_gate.spares(e)
                || !can_crush(crush_capability, CrushTarget::from_entity_without_frame(e))
            {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Cell scatter dispatch eligibility (the `force = 0` gate)
// ---------------------------------------------------------------------------

/// Elite veterancy level — the pre-scan in the native cell scatter asks each
/// occupant's `VeterancyClass::IsElite`.
const ELITE_VETERANCY: u16 = 200;

/// Live Techno inputs at one Cell Scatter dispatch. Non-Technos have no such
/// facts and pass only a cell-wide override. Native: 481771..4817C1.
#[derive(Clone, Copy, Debug)]
pub struct ScatterTechno {
    pub has_scatter_ability: bool,
    pub house_iq: i32,
}

impl ScatterTechno {
    fn from_entity(
        entity: &GameEntity,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        houses: &std::collections::BTreeMap<
            crate::sim::intern::InternedId,
            crate::sim::house_state::HouseState,
        >,
        interner: &crate::sim::intern::StringInterner,
    ) -> Self {
        use crate::sim::combat::veterancy::{has_weapon_ability, rank_from_u16};
        Self {
            has_scatter_ability: rules
                .and_then(|rules| rules.object(interner.resolve(entity.type_ref())))
                .is_some_and(|object| {
                    has_weapon_ability(
                        rank_from_u16(entity.veterancy),
                        object,
                        crate::rules::object_type::Ability::Scatter,
                    )
                }),
            // A rulesless/component fixture may omit its House. Runtime houses
            // own CurrentIQ; zero is their constructor default, not a human/AI
            // inference. The value is already saved/restored by HouseState.
            house_iq: houses
                .get(&entity.owner())
                .map_or(0, |house| house.current_iq),
        }
    }
}

/// Rules inputs of the native cell-scatter dispatch gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScatterEligibility {
    /// `[CombatDamage] PlayerScatter` — stock `no`.
    pub player_scatter: bool,
    /// `[IQ] Scatter` — stock `2`, constructor default `3`.
    pub iq_scatter: i32,
}

impl Default for ScatterEligibility {
    /// The RulesClass constructor values, used when no ruleset is loaded.
    fn default() -> Self {
        Self {
            player_scatter: false,
            iq_scatter: 3,
        }
    }
}

impl ScatterEligibility {
    pub fn from_rules(rules: Option<&crate::rules::ruleset::RuleSet>) -> Self {
        rules.map_or_else(Self::default, |rules| Self {
            player_scatter: rules.general.player_scatter,
            iq_scatter: rules.general.iq_scatter,
        })
    }
}

/// Whether the native cell scatter actually dispatches to one occupant.
///
/// The dispatch condition is
/// `eliteFound || force != 0 || PlayerScatter || (HasWeaponAbility(3) || IQ.Scatter <= occupantHouse.IQ)`.
/// `eliteFound` is a **per-cell** pre-scan result — the walk breaks on the first
/// elite occupant and the answer then applies to every occupant of that cell —
/// while the IQ term is per-occupant.
///
/// `HasWeaponAbility(3)` is SCATTER, using the shared rank/ability owner.
/// Evidence: tools/spatial_oracle/cell_scatter.{py,json,meta.json} executes the
/// original full dispatcher and ability reader. Eligibility itself draws no
/// RNG; the recipient's Scatter virtual may draw, so dispatch is not RNG-free.
pub fn scatter_dispatch_allowed(
    eligibility: ScatterEligibility,
    forced: bool,
    elite_in_cell: bool,
    techno: Option<ScatterTechno>,
) -> bool {
    elite_in_cell
        || forced
        || eligibility.player_scatter
        || techno.is_some_and(|facts| {
            facts.has_scatter_ability || facts.house_iq >= eligibility.iq_scatter
        })
}

/// The per-cell elite pre-scan: does any occupant of this cell carry elite rank?
///
/// The native pre-scan runs only for an unforced scatter and breaks on the first
/// elite it finds. It walks the *cell's* occupants, so the vehicle doing the
/// scattering is not one of them — an elite crusher must not release the cell's
/// own dispatch gate, which is why `skip_id` exists.
pub fn cell_has_elite_occupant(occupants: &[u64], skip_id: u64, entities: &EntityStore) -> bool {
    occupants.iter().any(|&id| {
        id != skip_id
            && entities
                .get(id)
                .is_some_and(|entity| entity.veterancy >= ELITE_VETERANCY)
    })
}

/// Classify what a crusher does to the occupants of the cell it is touching.
///
/// `EnteringCell` mirrors `UnitClass::PerCellProcess(entering != 0)`, which
/// scatters the cell with **force = 0** and never crushes; `FullyInCell` mirrors
/// the `entering == 0` crush loop. The unforced scatter is subject to the
/// dispatch gate — an elite in the cell, `PlayerScatter`, or the occupant's
/// house IQ — which is why player-owned infantry stand still under an
/// approaching tank in retail instead of dodging.
///
/// `current_frame` feeds the Iron Curtain gate of the crush predicate.
#[allow(clippy::too_many_arguments)]
pub fn classify_drive_crush_phase(
    phase: DriveCrushPhase,
    occ: &[u64],
    entities: &EntityStore,
    crusher_id: u64,
    alliances: &crate::map::houses::HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
    crusher_coord: (i32, i32),
    capability: CrushCapability,
    eligibility: ScatterEligibility,
    current_frame: u32,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    houses: &std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    >,
) -> DriveCrushOutcome {
    if !capability.can_crush_units() {
        return DriveCrushOutcome::None;
    }
    let Some(crusher) = entities.get(crusher_id) else {
        return DriveCrushOutcome::None;
    };
    let crusher_owner = interner.resolve(crusher.owner());
    // Per-cell pre-scan, exactly once, before the dispatch walk.
    let elite_in_cell = match phase {
        DriveCrushPhase::EnteringCell => cell_has_elite_occupant(occ, crusher_id, entities),
        DriveCrushPhase::FullyInCell => false,
    };
    let mut selected = Vec::new();
    for &id in occ {
        if id == crusher_id {
            continue;
        }
        let Some(victim) = entities.get(id) else {
            continue;
        };
        match phase {
            DriveCrushPhase::EnteringCell => {
                if scatter_dispatch_allowed(
                    eligibility,
                    false,
                    elite_in_cell,
                    Some(ScatterTechno::from_entity(victim, rules, houses, interner)),
                ) {
                    selected.push(id);
                }
            }
            DriveCrushPhase::FullyInCell => {
                // The same gate admission uses. Two inline copies of one
                // predicate is how admission and the kill came to disagree.
                if CrushAllyGate::new(crusher_owner, alliances, interner).spares(victim) {
                    continue;
                }
                if !within_crush_distance_sq(crusher_coord, entity_crush_coord(victim)) {
                    continue;
                }
                if can_crush(capability, CrushTarget::from_entity(victim, current_frame)) {
                    selected.push(id);
                }
            }
        }
    }
    // Native `CellClass::Scatter_Objects` @ `0x00481670` dispatches its
    // selected-list snapshot
    // forward. Sorting by stable id here would erase Cell list authority.
    match (phase, selected.is_empty()) {
        (_, true) => DriveCrushOutcome::None,
        (DriveCrushPhase::EnteringCell, false) => DriveCrushOutcome::Scatter { blockers: selected },
        (DriveCrushPhase::FullyInCell, false) => DriveCrushOutcome::Kill { victims: selected },
    }
}

// ---------------------------------------------------------------------------
// Scatter displacement (replaces old "bump" teleport)
// ---------------------------------------------------------------------------
//
// The original engine uses CellClass::Scatter_Objects to tell occupants to
// move out of the way. All 6 locomotor call sites pass force=1 with a
// NullCoord, which triggers UnitClass::Scatter Branch A: random direction,
// Set_Destination only (no mission change). The blocker walks away via its
// normal locomotor — it is never teleported.
//
// Our implementation: find a walkable, unoccupied adjacent cell and issue
// the blocker a 1-cell movement command via `issue_direct_move`.

/// Frames a blocked mover waits after telling the cell to scatter.
///
/// The drive locomotor writes the literal 10 into the mover's wait field
/// immediately after its `Scatter_Objects(force = 1)` call, and the head of the
/// next `Process_Movement` decrements it and skips the move while it is
/// positive. This is a hardcoded constant, **not** `[AI] BlockagePathDelay`,
/// which is a different (60-frame) timer with a different consumer.
pub const POST_SCATTER_WAIT_FRAMES: i32 = 10;

// The full-infantry-cell force-scatter helpers used to live here and are gone
// with the clause that called them: the three-infantry-bit test is real but the
// block around it is dominated by a radio-tether byte VERA does not model, and
// the original scatters the man's OWN cell on arrival rather than the
// destination cell of a blocked step. See the note at the head of
// `movement_occupancy::handle_deferred_occupancy`.

/// A forced blocked-cell scatter loses its force when Infantry's locomotor
/// reports moving. After the mission gate, the final Fraidycat gate at
/// `InfantryClass::Scatter 0x0051D20E..0x0051D220` rejects ordinary moving
/// infantry even without an attack target. `UnitClass::Scatter 0x00743A50`
/// never demotes force. See `tools/infantry_scatter_oracle.py` for the bounded
/// native gate comparison.
fn moving_blocker_accepts_forced_scatter(
    blocker: &GameEntity,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    is_fraidycat: bool,
) -> bool {
    if blocker.category != EntityCategory::Infantry {
        return true;
    }
    let mission_allows = match (blocker.mission.current().known(), rules) {
        (Some(mission), Some(rules)) => rules
            .mission_control
            .entry(mission)
            .is_none_or(|entry| entry.scatter),
        _ => true,
    };
    mission_allows && is_fraidycat
}

/// Read a blocker type's `Fraidycat=` flag for [`scatter_blocker`].
///
/// Stock `rulesmd.ini` sets `Fraidycat=yes` on 26 sections, all civilians — so
/// every combat infantry type takes the refusing branch of the second scatter
/// gate. An absent ruleset resolves to the constructed default `false`.
pub fn blocker_is_fraidycat(
    entities: &EntityStore,
    blocker_id: u64,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    let Some(blocker) = entities.get(blocker_id) else {
        return false;
    };
    if blocker.category != EntityCategory::Infantry {
        return false;
    }
    rules
        .and_then(|rules| rules.object(interner.resolve(blocker.type_ref())))
        .is_some_and(|obj| obj.fraidycat)
}

/// Try to scatter a blocker to an adjacent cell by issuing a movement command.
///
/// Compatibility displacement for the blocked-cell caller: search eight
/// neighbours from a random direction and issue a movement order. Residual:
/// native NULL-source Infantry Scatter first tries FNPC, then uses the shared
/// eight-neighbour fallback; this adapter still needs that class migration.
///
/// `rules` resolves the Infantry scatter gates and the normal movement speed.
/// Scatter changes the destination; it does not grant a special speed.
///
/// Returns `true` if the blocker was given a scatter movement command.
#[allow(clippy::too_many_arguments)]
pub fn scatter_blocker(
    entities: &mut EntityStore,
    blocker_id: u64,
    path_grid: Option<&PathGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    occupancy: &OccupancyGrid,
    layer: MovementLayer,
    rng: &mut SimRng,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    timing: crate::sim::movement::DestinationTiming,
) -> bool {
    // Read blocker properties (immutable borrow).
    let Some(blocker) = entities.get(blocker_id) else {
        return false;
    };
    // Buildings are immutable obstacles — never scatter targets. Bail before
    // the RNG read so determinism is preserved for all legitimate cases.
    if blocker.category == EntityCategory::Structure {
        return false;
    }
    // MovementTarget is VERA's existing walking destination authority. Native
    // WalkLocomotion::Is_Moving (0x0075AB30) reads its moving byte, distinct
    // from Is_Moving_Now; an installed target stands in for that byte here.
    let is_fraidycat = blocker_is_fraidycat(entities, blocker_id, rules, interner);
    if blocker.movement_target.is_some()
        && !moving_blocker_accepts_forced_scatter(blocker, rules, is_fraidycat)
    {
        return false;
    }
    let bpos = (blocker.position.rx, blocker.position.ry);
    let speed = scatter_movement_speed(blocker, rules, interner);
    let ordinary_track = blocker.locomotor.as_ref().is_some_and(|locomotor| {
        matches!(
            locomotor.kind,
            crate::rules::locomotor_type::LocomotorKind::Drive
                | crate::rules::locomotor_type::LocomotorKind::Ship
        )
    });
    let config = rules
        .and_then(|rules| rules.object(interner.resolve(blocker.type_ref())))
        .map(|object| {
            (
                object.accel_factor,
                object.decel_factor,
                SimFixed::from_num(object.slowdown_distance),
            )
        });
    // Find a valid adjacent cell. Random start direction matches Branch A.
    let start_dir = rng.next_range_u32(8) as usize;
    let mut target: Option<(u16, u16)> = None;

    for i in 0..8 {
        let dir = (start_dir + i) % 8;
        let (dx, dy) = NEIGHBOR_OFFSETS[dir];
        let nx = bpos.0 as i32 + dx;
        let ny = bpos.1 as i32 + dy;
        if nx < 0 || ny < 0 {
            continue;
        }
        let (nx, ny) = (nx as u16, ny as u16);

        // Must be walkable terrain.
        if let Some(grid) = path_grid {
            if !grid.is_walkable(nx, ny) {
                continue;
            }
        }
        // Must not be occupied by vehicles/structures. Infantry sub-cells OK.
        if let Some(occ) = occupancy.get(nx, ny) {
            if occ.has_blockers_on(layer) {
                continue;
            }
        }
        target = Some((nx, ny));
        break;
    }

    let Some(dest) = target else {
        return false;
    };

    // Unit Scatter744063..744070 converts the chosen coord to Cell* and
    // dispatches ordinary SetDestination741970 (+480); it writes no speed.
    // Use the same shared-track destination/head acceptance and type ramp
    // configuration as a normal command. Scripted direct-move users remain
    // independent of this receiver, as do the other locomotor adapters.
    let accepted = if ordinary_track {
        if let Some(grid) = path_grid {
            super::movement_commands::issue_move_command_with_layered(
                entities,
                grid,
                blocker_id,
                dest,
                speed,
                false,
                None,
                None,
                resolved_terrain,
                None,
                None,
                None,
                None,
                None,
                timing,
            )
        } else {
            let accepted = super::movement_commands::issue_direct_move(
                entities, blocker_id, dest, speed, timing,
            );
            if accepted {
                if let Some(entity) = entities.get_mut(blocker_id) {
                    super::navcom::set_destination_internal_cell(entity, dest, resolved_terrain);
                }
            }
            accepted
        }
    } else {
        super::movement_commands::issue_direct_move(entities, blocker_id, dest, speed, timing)
    };
    if accepted && ordinary_track {
        if let Some((accel, decel, slowdown)) = config {
            if let Some(target) = entities
                .get_mut(blocker_id)
                .and_then(|entity| entity.movement_target.as_mut())
            {
                target.accel_factor = accel;
                target.decel_factor = decel;
                target.slowdown_distance = slowdown;
            }
        }
    }
    accepted
}

/// Normal speed shared by blocked-cell and damage-triggered displacement.
fn scatter_movement_speed(
    entity: &GameEntity,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
) -> SimFixed {
    // Scatter installs a destination; the walking process still calls
    // InfantryClass::GetCurrentSpeed (0x00521D80), delegating to
    // FootClass::GetCurrentSpeed (0x004DB1A0). Use the same resolver as ordinary
    // Move orders, including FASTER and the locomotor multiplier. The former
    // literal 1024 made a stock Speed=4 GI scatter at 6.8 times normal speed.
    let obj = rules.and_then(|r| r.object(interner.resolve(entity.type_ref())));
    let base_speed = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        entity,
        obj,
        obj.map_or(4, |o| o.speed),
        rules.map_or(1.0, |r| r.general.veteran_speed),
    );
    let multiplier = entity
        .locomotor
        .as_ref()
        .map_or(SimFixed::from_num(1), |loco| loco.speed_multiplier);
    (base_speed * multiplier).max(SimFixed::from_num(25))
}

/// One accepted nonfatal Infantry damage scatter, selected before the
/// receiver's fear callback mutates the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InfantryDamageScatter {
    pub(crate) destination: (u16, u16),
    pub(crate) speed: SimFixed,
}

/// Select the native attacker-relative displacement used by the Infantry
/// damage receiver.
///
/// The source-aware callback draws inclusive `0..=4`, offsets the direction
/// away from the attacker by `-2..=+2`, then scans eight neighbours of Foot's
/// navigation coordinate. It retains the first legal fallback while seeking
/// a direct nonstructural surface. Combat commits the result between the HP
/// write and fear. The separate NULL-source class path is still a residual
/// on [`scatter_blocker`], as are this caller's entry/setter adapters below.
#[allow(clippy::too_many_arguments)]
pub(crate) fn select_infantry_damage_scatter(
    infantry: &GameEntity,
    attacker_coord: (i32, i32),
    terrain: Option<&ResolvedTerrainGrid>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    occupancy: &OccupancyGrid,
    rules: &crate::rules::ruleset::RuleSet,
    owner_controlled_by_human: bool,
    teams: &crate::sim::team_script_vm::TeamScriptVm,
    rng: &mut SimRng,
    interner: &crate::sim::intern::StringInterner,
) -> Option<InfantryDamageScatter> {
    if infantry.category != EntityCategory::Infantry
        || infantry.dying
        || infantry.health.current == 0
        || infantry.locomotor.is_none()
    {
        return None;
    }

    let doing = infantry.mission_leaf.as_infantry()?.doing();
    // With ReceiveDamage's literal false/false arguments, a player-owned man
    // in the four deploy-family actions returns at the entry branch. This is
    // independent of the permission-table byte (28..30 are otherwise allowed).
    if owner_controlled_by_human && (0x1b..=0x1e).contains(&doing) {
        return None;
    }

    // ReceiveDamage calls the Infantry virtual directly; the CurrentIQ versus
    // IQ.Scatter gate belongs only to CellClass::Scatter_Objects and must not
    // be imported here. With force=false, the current mission's Scatter flag
    // is an unconditional pre-RNG gate.
    let mission_scatter = infantry
        .mission
        .current()
        .known()
        .and_then(|mission| rules.mission_control.entry(mission))
        .map_or(true, |entry| entry.scatter);
    if !mission_scatter {
        return None;
    }
    // 51D1AA reads retained Doing+6C4, independent of the displayed sequence.
    // -1 and 31 bypass the table. All 42 native actions are represented by the
    // mission leaf, including the nine without a presentation SequenceKind.
    if !crate::rules::infantry_sequence::scatter_allowed_by_doing(doing)
        .expect("mission leaf retains a valid native Doing")
    {
        return None;
    }
    // gamemd 51D212..51D220: every path with effective first flag=false
    // requires Fraidycat, even with no combat target or with SCATTER ability.
    // This also subsumes the earlier non-Fraidycat/Target test51D196..51D1A4.
    // Evidence: tools/spatial_oracle/infantry_damage_scatter.{py,json,meta.json}.
    let object = rules.object(interner.resolve(infantry.type_ref()))?;
    if !object.fraidycat {
        return None;
    }
    let has_scatter_ability = crate::sim::combat::veterancy::has_weapon_ability(
        crate::sim::combat::veterancy::rank_from_u16(infantry.veterancy),
        object,
        crate::rules::object_type::Ability::Scatter,
    );
    // 51D200 tests Foot.Team+5D4, not NavCom+5A4: Add_Member6EA56E
    // installs the Team receiver and Remove_Member6EA99D clears it. The world
    // supplies live TeamScriptVm membership; CellClass's IQ gate is separate.
    if !rules.general.player_scatter
        && !has_scatter_ability
        && owner_controlled_by_human
        && teams.team_for_member(infantry.stable_id()).is_none()
    {
        return None;
    }

    let defender_x = i32::from(infantry.position.rx)
        .wrapping_mul(256)
        .wrapping_add(infantry.position.sub_x.to_num::<i32>());
    let defender_y = i32::from(infantry.position.ry)
        .wrapping_mul(256)
        .wrapping_add(infantry.position.sub_y.to_num::<i32>());
    let start_direction =
        super::scatter_cell::source_start_direction((defender_x, defender_y), attacker_coord, rng);
    let terrain = terrain?;
    let bounds = playfield_bounds?;
    // 51D4A5 seeds the scan from Foot+4C, which can be a paid head or Tube
    // exit. The source-relative heading above deliberately uses Object+9C.
    let navigation = super::foot_coordinate::navigation_coordinate(infantry, Some(terrain)).ok()?;
    let seed = ((navigation.x / 256) as i16, (navigation.y / 256) as i16);
    let layer = infantry.movement_layer_or_ground();
    let locomotor = infantry.locomotor.as_ref().expect("checked above");

    let destination =
        super::scatter_cell::select_neighbor(seed, start_direction, |candidate, _| {
            // Keep the first Get_CellClass before the height-aware playfield query:
            // both lookups can stamp the map's shared dummy identity.
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let cell = cells.lookup(candidate);
            if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(candidate.0), i32::from(candidate.1)),
                Some(bounds),
                Some(terrain),
            ) {
                return None;
            }
            let crate::map::cell_index::NativeCellIdentity::Real(index) = cell else {
                return None;
            };
            let cell = &terrain.cells()[index];
            let terrain_allows = match layer {
                MovementLayer::Ground => {
                    crate::sim::pathfinding::passability::is_passable_for_zone(
                        cell.zone_type,
                        locomotor.movement_zone,
                    )
                }
                MovementLayer::Bridge => cell.bridge_walkable,
                MovementLayer::Air | MovementLayer::Underground => false,
            };
            // Residual: this existing entry adapter has not yet migrated to the
            // world-owned Infantry+1AC numeric query. Source selection now retains
            // native fallback/preference, but that does not prove class legality.
            if !terrain_allows
                || !cell_passable_for_infantry(occupancy.get(cell.rx, cell.ry), layer)
            {
                return None;
            }
            Some(super::scatter_cell::preferred_surface(terrain, candidate))
        })?;

    Some(InfantryDamageScatter {
        destination: (destination.0 as u16, destination.1 as u16),
        speed: scatter_movement_speed(infantry, Some(rules), interner),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use crate::sim::game_entity::{GameEntity, InfantryRuntime};
    use crate::sim::occupancy::CellListInsertion;

    fn flat_resolved_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        let land = LandType::Clear.as_index();
        let speed_costs = SpeedCostProfile::default();
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: true,
            tileset_index: None,
            land_type: land,
            yr_cell_land_type: land,
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs,
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: true,
            allows_tiberium: false,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: land,
            base_yr_cell_land_type: land,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: speed_costs,
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn flat_resolved_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let cells = (0..height)
            .flat_map(|ry| (0..width).map(move |rx| flat_resolved_cell(rx, ry)))
            .collect();
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    /// Owns what a `CrushAllyGate` borrows, so a test can make one in a line.
    ///
    /// The crusher is "Soviets" and the fixtures' victims are "Allies", so
    /// these tests keep exercising the crush path rather than the ally refusal;
    /// the refusal has its own tests below.
    struct GateFixture {
        alliances: crate::map::houses::HouseAllianceMap,
        interner: crate::sim::intern::StringInterner,
    }

    impl GateFixture {
        fn new() -> Self {
            Self {
                alliances: crate::map::houses::HouseAllianceMap::new(),
                interner: crate::sim::intern::test_interner(),
            }
        }

        fn enemy(&self) -> CrushAllyGate<'_> {
            CrushAllyGate::new("Soviets", &self.alliances, &self.interner)
        }
    }

    /// A crusher never latches its own infantry, the way native never does.
    ///
    /// `Is_Crushable_By 0x005F6CD0` asks `HouseClass::Is_Ally_ByObject
    /// 0x004F9A90` about the victim's house (`+0x21C`) at `0x005F6D1A` and
    /// again at `0x005F6D67`, and `Can_Enter_Cell` re-tests at `0x0073FB53`, so
    /// an own or allied crushable answers code 6 or 2 and never reaches the
    /// crush latch. Before this gate, admission answered `Crushable` and only
    /// the kill site refused - so a tank parked on its own GI and neither
    /// crushed nor scattered it.
    #[test]
    fn ally_gate_spares_the_crushers_own_infantry() {
        let mut store = EntityStore::new();
        store.insert(infantry(1, 5, 5, 2)); // owned by "Allies"
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);

        let alliances = crate::map::houses::HouseAllianceMap::new();
        let interner = crate::sim::intern::test_interner();
        let own = CrushAllyGate::new("Allies", &alliances, &interner);

        assert!(
            collect_crush_victims(
                (5, 5),
                &grid,
                MovementLayer::Ground,
                CrushCapability::new(true, false),
                &store,
                own,
            )
            .is_empty(),
            "a crusher must not list its own infantry as a crush victim"
        );
        assert!(
            !cell_passable_after_crush(
                (5, 5),
                &grid,
                MovementLayer::Ground,
                CrushCapability::new(true, false),
                &store,
                own,
            ),
            "and the cell is not passable by crushing what it may not crush"
        );

        // The same fixture with an enemy crusher still crushes, so the gate is
        // refusing on alliance and not on something incidental.
        let enemy = CrushAllyGate::new("Soviets", &alliances, &interner);
        assert_eq!(
            collect_crush_victims(
                (5, 5),
                &grid,
                MovementLayer::Ground,
                CrushCapability::new(true, false),
                &store,
                enemy,
            ),
            vec![1]
        );
    }

    /// The same refusal for a declared ally, not just for the same house.
    #[test]
    fn ally_gate_spares_a_declared_allys_infantry() {
        let mut store = EntityStore::new();
        store.insert(infantry(1, 5, 5, 2)); // owned by "Allies"
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);

        // `are_houses_friendly` normalizes to upper case and reads either
        // direction, so one entry is enough to express the pact.
        let mut alliances = crate::map::houses::HouseAllianceMap::new();
        alliances
            .entry("SOVIETS".to_string())
            .or_default()
            .insert("ALLIES".to_string());
        let interner = crate::sim::intern::test_interner();
        let allied = CrushAllyGate::new("Soviets", &alliances, &interner);

        assert!(
            collect_crush_victims(
                (5, 5),
                &grid,
                MovementLayer::Ground,
                CrushCapability::new(true, false),
                &store,
                allied,
            )
            .is_empty(),
            "a declared ally's infantry is spared exactly as one's own is"
        );
    }

    /// The gate answers exactly what the shared helper answers.
    ///
    /// Admission and the kill site both go through `spares`, so this is the one
    /// place the predicate is pinned. Their disagreement was the A7 defect.
    #[test]
    fn ally_gate_answers_what_the_shared_helper_answers() {
        let alliances = crate::map::houses::HouseAllianceMap::new();
        let victim = infantry(1, 5, 5, 2); // "Allies"
        let interner = crate::sim::intern::test_interner();

        for crusher in ["Allies", "Soviets", "NoSuchHouse"] {
            let gate = CrushAllyGate::new(crusher, &alliances, &interner);
            assert_eq!(
                gate.spares(&victim),
                crate::map::houses::are_houses_friendly(
                    &alliances,
                    crusher,
                    interner.resolve(victim.owner())
                ),
                "gate and helper disagree for crusher {crusher}"
            );
        }
    }

    /// A mover that cannot crush pays no alliance test at all.
    ///
    /// `collect_crush_victims` runs per mover per step, so an ungated gate would
    /// charge every occupant of every occupied cell an alliance question for
    /// movers that have no crush victims by definition.
    #[test]
    fn crush_admission_returns_before_asking_about_a_non_crusher() {
        let mut store = EntityStore::new();
        store.insert(infantry(1, 5, 5, 2));
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);
        let fixture = GateFixture::new();

        assert!(
            collect_crush_victims(
                (5, 5),
                &grid,
                MovementLayer::Ground,
                CrushCapability::new(false, false),
                &store,
                fixture.enemy(),
            )
            .is_empty(),
            "a non-crusher has no victims whoever occupies the cell"
        );
    }

    fn infantry(id: u64, rx: u16, ry: u16, sub: u8) -> GameEntity {
        let mut e = GameEntity::test_default(id, "E1", "Allies", rx, ry);
        e.category = EntityCategory::Infantry;
        e.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
            EntityCategory::Infantry,
        );
        e.sub_cell = Some(sub);
        e.crushable = true;
        e
    }

    fn vehicle(id: u64, rx: u16, ry: u16) -> GameEntity {
        let mut e = GameEntity::test_default(id, "MTNK", "Allies", rx, ry);
        e.category = EntityCategory::Unit;
        e.crushable = false;
        e
    }

    fn structure(id: u64, rx: u16, ry: u16) -> GameEntity {
        let mut e = GameEntity::test_default(id, "GAREFN", "Allies", rx, ry);
        e.category = EntityCategory::Structure;
        e.crushable = false;
        e
    }

    #[test]
    fn blocker_neighbor_counts_include_bridge_layer_occupants_globally() {
        let mut entities = EntityStore::new();
        let mut blocker = vehicle(1, 2, 2);
        blocker.on_bridge = true;
        blocker.lifecycle.in_limbo = false;
        blocker.lifecycle.cell_marked = true;
        entities.insert(blocker);
        let interner = crate::sim::intern::StringInterner::new();

        let counts = build_blocker_neighbor_counts(&entities, 5, 5, None, &interner, None);

        assert_eq!(counts.count_at(1, 2), 1);
        assert_eq!(counts.count_at(3, 3), 1);
        assert_eq!(counts.count_at(2, 2), 0);
    }

    #[test]
    fn gsi_04_10_zero_occupation_terrain_still_contributes_neighbor_blocker() {
        let entities = EntityStore::new();
        let interner = crate::sim::intern::StringInterner::new();
        let mut terrain = flat_resolved_terrain(5, 5);
        let source = terrain.cell_mut(2, 2).expect("source cell");
        source.terrain_object_occupation = Some(0);
        source.terrain_object_blocks = false;

        let counts =
            build_blocker_neighbor_counts(&entities, 5, 5, Some(&terrain), &interner, None);
        for y in 1..=3 {
            for x in 1..=3 {
                assert_eq!(counts.count_at(x, y), u8::from((x, y) != (2, 2)));
            }
        }

        terrain
            .cell_mut(2, 2)
            .expect("interior source cell")
            .terrain_object_occupation = None;
        let edge_source = terrain.cell_mut(0, 0).expect("edge source cell");
        edge_source.terrain_object_occupation = Some(0);
        edge_source.terrain_object_blocks = false;
        let edge_counts =
            build_blocker_neighbor_counts(&entities, 5, 5, Some(&terrain), &interner, None);
        assert_eq!(edge_counts.count_at(1, 0), 1);
        assert_eq!(edge_counts.count_at(0, 1), 1);
        assert_eq!(edge_counts.count_at(1, 1), 1);
        let edge_counts_ref = &edge_counts;
        assert_eq!(
            (0..5)
                .flat_map(|y| (0..5).map(move |x| edge_counts_ref.count_at(x, y) as u32))
                .sum::<u32>(),
            3,
            "an edge Terrain contributes only to its three valid neighbors"
        );

        terrain
            .cell_mut(0, 0)
            .expect("edge source cell")
            .terrain_object_occupation = None;
        let removed =
            build_blocker_neighbor_counts(&entities, 5, 5, Some(&terrain), &interner, None);
        let removed_ref = &removed;
        assert_eq!(
            (0..5)
                .flat_map(|y| (0..5).map(move |x| removed_ref.count_at(x, y) as u32))
                .sum::<u32>(),
            0,
            "removing the live terrain identity reverses all eight contributions"
        );
    }

    #[test]
    fn blocker_neighbor_counts_building_uses_expanded_foundation_rectangle_once() {
        let mut entities = EntityStore::new();
        let mut building = structure(1, 2, 2);
        building.lifecycle.in_limbo = false;
        building.lifecycle.cell_marked = true;
        entities.insert(building);
        let interner = crate::sim::intern::StringInterner::new();

        let counts = build_blocker_neighbor_counts(&entities, 5, 5, None, &interner, None);

        for y in 1..=3 {
            for x in 1..=3 {
                assert_eq!(
                    counts.count_at(x, y),
                    1,
                    "1x1 fallback structure should count expanded rectangle cell ({x},{y})"
                );
            }
        }
        assert_eq!(counts.count_at(0, 2), 0);
        assert_eq!(counts.count_at(2, 0), 0);
    }

    /// Helper: build an OccupancyGrid from a set of entity descriptions.
    fn make_occ(entries: &[(u16, u16, u64, MovementLayer, Option<u8>)]) -> OccupancyGrid {
        let mut grid = OccupancyGrid::new();
        for &(rx, ry, eid, layer, sub) in entries {
            grid.add(
                rx,
                ry,
                eid,
                layer,
                sub,
                CellListInsertion::PrependNonBuilding,
            );
        }
        grid
    }

    // -- can_crush tests --

    /// Build a crush target directly, bypassing entity construction.
    fn target(
        category: EntityCategory,
        crushable: bool,
        deploy_crush_immune: bool,
        omni_crush_resistant: bool,
        iron_curtained: bool,
    ) -> CrushTarget {
        CrushTarget {
            category,
            crushable,
            deploy_crush_immune,
            omni_crush_resistant,
            iron_curtained,
        }
    }

    #[test]
    fn gsi_04_07_placement_neighbor_plane_counts_only_wall_overlay_and_reverses() {
        use crate::map::overlay_types::OverlayTypeRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::overlay_grid::OverlayGrid;

        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=WALL\n1=ROCK\n2=ZEROWHEEL\n\
             [Wall]\nWheel=100%\n\
             [Rock]\nWheel=0%\n\
             [WALL]\nWall=yes\n\
             [ROCK]\nIsARock=yes\n\
             [ZEROWHEEL]\nLand=Rock\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let entities = EntityStore::new();
        let interner = crate::sim::intern::StringInterner::new();

        for non_wall in [1u8, 2u8] {
            let mut overlays = OverlayGrid::new(5, 5);
            overlays.place_overlay(2, 2, non_wall, 0);
            let counts = build_blocker_neighbor_counts_with_overlays(
                &entities,
                5,
                5,
                None,
                Some(&overlays),
                Some(&registry),
                &interner,
                None,
            );
            let counts_ref = &counts;
            assert_eq!(
                (0..5)
                    .flat_map(|y| (0..5).map(move |x| counts_ref.count_at(x, y) as u32))
                    .sum::<u32>(),
                0,
                "non-wall overlay {non_wall} must not produce neighbor counts"
            );
        }

        let mut overlays = OverlayGrid::new(5, 5);
        overlays.place_overlay(2, 2, 0, 0);
        let mut counts = build_blocker_neighbor_counts_with_overlays(
            &entities,
            5,
            5,
            None,
            Some(&overlays),
            Some(&registry),
            &interner,
            None,
        );
        for y in 1..=3 {
            for x in 1..=3 {
                assert_eq!(counts.count_at(x, y), u8::from((x, y) != (2, 2)));
            }
        }
        counts.remove_single_cell_neighbor_source(2, 2);
        let counts_ref = &counts;
        assert_eq!(
            (0..5)
                .flat_map(|y| (0..5).map(move |x| counts_ref.count_at(x, y) as u32))
                .sum::<u32>(),
            0
        );
    }

    #[test]
    fn finalized_wall_plane_is_sole_baseline_without_identity_double_count() {
        use crate::map::authored_overlay::FinalizedOverlayPayload;
        use crate::map::overlay_types::OverlayTypeRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::overlay_grid::OverlayGrid;

        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=WALL\n1=BODY\n\
             [WALL]\nWall=yes\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let entities = EntityStore::new();
        let interner = crate::sim::intern::StringInterner::new();
        let mut wall_plane = vec![0u8; 25];
        for index in [6usize, 7, 8, 11, 13, 16, 17, 18] {
            wall_plane[index] = 1;
        }

        let mut surviving_cells = vec![(-1, 0); 25];
        surviving_cells[12] = (0, 0);
        let surviving = OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(5, 5, surviving_cells, wall_plane.clone()),
        );
        let surviving_counts = build_blocker_neighbor_counts_with_overlays(
            &entities,
            5,
            5,
            None,
            Some(&surviving),
            Some(&registry),
            &interner,
            None,
        );
        for y in 1..=3 {
            for x in 1..=3 {
                assert_eq!(
                    surviving_counts.count_at(x, y),
                    u8::from((x, y) != (2, 2)),
                    "surviving final wall must not be scanned a second time"
                );
            }
        }

        let mut overwritten_cells = vec![(-1, 0); 25];
        overwritten_cells[12] = (1, 2);
        let overwritten = OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(5, 5, overwritten_cells, wall_plane),
        );
        let overwritten_counts = build_blocker_neighbor_counts_with_overlays(
            &entities,
            5,
            5,
            None,
            Some(&overwritten),
            Some(&registry),
            &interner,
            None,
        );
        assert_eq!(overwritten_counts.count_at(2, 1), 1);
        assert_eq!(overwritten_counts.count_at(2, 2), 0);

        let mut poisoned_cells = vec![(-1, 0); 25];
        poisoned_cells[12] = (0, 0);
        let authoritative_zero = OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(5, 5, poisoned_cells, vec![0; 25]),
        );
        let zero_counts = build_blocker_neighbor_counts_with_overlays(
            &entities,
            5,
            5,
            None,
            Some(&authoritative_zero),
            Some(&registry),
            &interner,
            None,
        );
        let zero_counts_ref = &zero_counts;
        assert_eq!(
            (0..5)
                .flat_map(|y| (0..5).map(move |x| u32::from(zero_counts_ref.count_at(x, y))))
                .sum::<u32>(),
            0,
            "Some(all-zero) is authority, not permission to reconstruct walls"
        );
    }

    #[test]
    fn gsi_04_15_detached_tube_mover_is_absent_from_both_blocker_snapshots() {
        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(7, "MTNK", "Americans", 2, 2);
        mover.lifecycle.in_limbo = false;
        mover.lifecycle.cell_marked = false;
        entities.insert(mover);
        let interner = crate::sim::intern::test_interner();
        let alliances = crate::map::houses::HouseAllianceMap::new();

        let (ground, bridge, dynamic) =
            build_entity_block_sets(&entities, "Russians", &alliances, &interner, None);
        assert!(ground.is_empty());
        assert!(bridge.is_empty());
        assert!(!dynamic.contains_any(&(2, 2)));

        let counts = build_blocker_neighbor_counts_with_overlays(
            &entities, 5, 5, None, None, None, &interner, None,
        );
        let counts_ref = &counts;
        assert_eq!(
            (0..5)
                .flat_map(|y| (0..5).map(move |x| u32::from(counts_ref.count_at(x, y))))
                .sum::<u32>(),
            0
        );
    }

    #[test]
    fn test_crusher_crushes_crushable_infantry() {
        assert!(can_crush(
            CrushCapability::new(true, false),
            target(EntityCategory::Infantry, true, false, false, false),
        ));
    }

    #[test]
    fn test_crusher_cannot_crush_non_crushable_infantry() {
        assert!(!can_crush(
            CrushCapability::new(true, false),
            target(EntityCategory::Infantry, false, false, false, false),
        ));
    }

    #[test]
    fn test_regular_crusher_cannot_crush_deploy_immune_infantry() {
        assert!(!can_crush(
            CrushCapability::new(true, false),
            target(EntityCategory::Infantry, true, true, false, false),
        ));
    }

    #[test]
    fn test_omni_crusher_crushes_non_crushable_infantry() {
        assert!(can_crush(
            CrushCapability::new(false, true),
            // The Omni block reads neither Crushable= nor the deploy byte.
            target(EntityCategory::Infantry, false, true, false, false),
        ));
    }

    #[test]
    fn test_omni_crusher_crushes_vehicles() {
        assert!(can_crush(
            CrushCapability::new(false, true),
            target(EntityCategory::Unit, false, false, false, false),
        ));
    }

    #[test]
    fn test_omni_crush_resistant_blocks_all() {
        assert!(!can_crush(
            CrushCapability::new(false, true),
            target(EntityCategory::Infantry, true, true, true, false),
        ));
    }

    #[test]
    fn test_structures_never_crushable() {
        assert!(!can_crush(
            CrushCapability::new(false, true),
            target(EntityCategory::Structure, true, false, false, false),
        ));
    }

    #[test]
    fn test_crusher_cannot_crush_vehicles() {
        assert!(!can_crush(
            CrushCapability::new(true, false),
            target(EntityCategory::Unit, false, false, false, false),
        ));
    }

    #[test]
    fn test_normal_zone_cannot_crush() {
        assert!(!can_crush(
            CrushCapability::new(false, false),
            target(EntityCategory::Infantry, true, false, false, false),
        ));
    }

    #[test]
    fn normal_zone_regular_crusher_crushes_crushable_infantry() {
        assert!(can_crush(
            CrushCapability::new(true, false),
            target(EntityCategory::Infantry, true, false, false, false),
        ));
    }

    #[test]
    fn missing_crusher_flag_does_not_crush_infantry() {
        assert!(!can_crush(
            CrushCapability::new(false, false),
            target(EntityCategory::Infantry, true, false, false, false),
        ));
    }

    #[test]
    fn iron_curtained_infantry_survives_a_regular_crusher() {
        // The last gate of the ordinary block, after the ally test.
        assert!(can_crush(
            CrushCapability::new(true, false),
            target(EntityCategory::Infantry, true, false, false, false),
        ));
        assert!(!can_crush(
            CrushCapability::new(true, false),
            target(EntityCategory::Infantry, true, false, false, true),
        ));
    }

    #[test]
    fn iron_curtained_victim_survives_an_omni_crusher() {
        // Same gate at the tail of the Omni block: a Battle Fortress crushes a
        // Crushable=no Desolator, but not a curtained one.
        assert!(can_crush(
            CrushCapability::new(false, true),
            target(EntityCategory::Infantry, false, false, false, false),
        ));
        assert!(!can_crush(
            CrushCapability::new(false, true),
            target(EntityCategory::Infantry, false, false, false, true),
        ));
    }

    #[test]
    fn crush_distance_gate_includes_0x3fff() {
        assert!(within_crush_distance_sq((0, 0), (127, 14)));
    }

    #[test]
    fn crush_distance_gate_excludes_0x4000() {
        assert!(!within_crush_distance_sq((0, 0), (128, 0)));
    }

    /// Stock skirmish gate values: `[CombatDamage] PlayerScatter=no`,
    /// `[IQ] Scatter=2`.
    fn stock_eligibility() -> ScatterEligibility {
        ScatterEligibility {
            player_scatter: false,
            iq_scatter: 2,
        }
    }

    #[test]
    fn classify_drive_crush_phase_entering_holds_player_infantry_still() {
        // Retail: the crusher's cell-entry scatter passes force = 0, so with
        // PlayerScatter=no, no elite present and a human house (IQ 0 < 2) the
        // occupant is never dispatched. Player infantry stand and get squashed.
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();

        let outcome = classify_drive_crush_phase(
            DriveCrushPhase::EnteringCell,
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            stock_eligibility(),
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(outcome, DriveCrushOutcome::None);
    }

    #[test]
    fn classify_drive_crush_phase_entering_scatters_when_an_elite_shares_the_cell() {
        // The elite pre-scan is per-cell: one elite occupant releases the
        // dispatch for every occupant of that cell, rookie or not.
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut rookie = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        rookie.category = EntityCategory::Infantry;
        rookie.crushable = true;
        entities.insert(rookie);
        let mut elite = GameEntity::test_default(3, "E1", "Soviet", 5, 5);
        elite.category = EntityCategory::Infantry;
        elite.crushable = true;
        elite.veterancy = 200;
        entities.insert(elite);
        let interner = crate::sim::intern::test_interner();

        let outcome = classify_drive_crush_phase(
            DriveCrushPhase::EnteringCell,
            &[3, 2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            stock_eligibility(),
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(
            outcome,
            DriveCrushOutcome::Scatter {
                blockers: vec![3, 2]
            }
        );
    }

    #[test]
    fn classify_drive_crush_phase_entering_scatters_when_player_scatter_is_on() {
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();

        let outcome = classify_drive_crush_phase(
            DriveCrushPhase::EnteringCell,
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            ScatterEligibility {
                player_scatter: true,
                iq_scatter: 2,
            },
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(outcome, DriveCrushOutcome::Scatter { blockers: vec![2] });
    }

    #[test]
    fn scatter_dispatch_gate_matches_the_native_disjunction() {
        let stock = stock_eligibility();
        // Nothing set: no dispatch.
        assert!(!scatter_dispatch_allowed(
            stock,
            false,
            false,
            Some(ScatterTechno {
                has_scatter_ability: false,
                house_iq: 0
            })
        ));
        // force = 1 (every locomotor blocked-cell caller) always dispatches.
        assert!(scatter_dispatch_allowed(
            stock,
            true,
            false,
            Some(ScatterTechno {
                has_scatter_ability: false,
                house_iq: 0
            })
        ));
        // An elite in the cell releases it.
        assert!(scatter_dispatch_allowed(
            stock,
            false,
            true,
            Some(ScatterTechno {
                has_scatter_ability: false,
                house_iq: 0
            })
        ));
        // An AI house at MaxIQLevels=5 clears [IQ] Scatter=2.
        assert!(scatter_dispatch_allowed(
            stock,
            false,
            false,
            Some(ScatterTechno {
                has_scatter_ability: false,
                house_iq: 5
            })
        ));
        // Exactly at the threshold: `IQ.Scatter <= house.IQ`.
        assert!(scatter_dispatch_allowed(
            stock,
            false,
            false,
            Some(ScatterTechno {
                has_scatter_ability: false,
                house_iq: 2
            })
        ));
        assert!(!scatter_dispatch_allowed(
            stock,
            false,
            false,
            Some(ScatterTechno {
                has_scatter_ability: false,
                house_iq: 1
            })
        ));
    }

    #[test]
    fn scatter_eligibility_defaults_are_the_rules_constructor_values() {
        let defaults = ScatterEligibility::default();
        assert!(!defaults.player_scatter);
        assert_eq!(defaults.iq_scatter, 3);
    }

    #[test]
    fn cell_scatter_recipient_gate_matches_original_execution() {
        use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
        use crate::sim::{house_state::HouseState, intern::StringInterner};
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/cell_scatter.json"
        ))
        .unwrap();
        for row in corpus.as_array().unwrap() {
            let input = &row["input"];
            let objects = input["objects"].as_array().unwrap();
            let mut ini = String::from("[VehicleTypes]\n");
            for (n, object) in objects.iter().enumerate() {
                ini.push_str(&format!("{n}=T{}\n", object["id"]));
            }
            for object in objects {
                ini.push_str(&format!(
                    "[T{}]\nVeteranAbilities={}\nEliteAbilities={}\n",
                    object["id"],
                    if object["veteran_scatter"].as_bool().unwrap_or(false) {
                        "SCATTER"
                    } else {
                        ""
                    },
                    if object["elite_scatter"].as_bool().unwrap_or(false) {
                        "SCATTER"
                    } else {
                        ""
                    },
                ));
            }
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            let mut interner = StringInterner::new();
            let mut entities = EntityStore::new();
            let mut houses = std::collections::BTreeMap::new();
            for object in objects {
                let id = object["id"].as_u64().unwrap();
                let owner = interner.intern(&format!("H{id}"));
                let kind = interner.intern(&format!("T{id}"));
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    id,
                    5,
                    5,
                    0,
                    0,
                    owner,
                    crate::sim::components::Health { current: 100 },
                    kind,
                    EntityCategory::Unit,
                    0,
                    5,
                    true,
                );
                entity.veterancy = object["rank"].as_u64().unwrap_or(0) as u16 * 100;
                let mut house = HouseState::new(owner, 0, None, true, 0, 10);
                house.current_iq = object["iq"].as_i64().unwrap_or(0) as i32;
                houses.insert(owner, house);
                entities.insert(entity);
            }
            let bridge = input["bridge"].as_bool().unwrap_or(false);
            let selected: Vec<_> = objects
                .iter()
                .filter(|object| object["bridge"].as_bool().unwrap_or(false) == bridge)
                .collect();
            let elite = selected.iter().any(|object| {
                object["techno"].as_bool().unwrap_or(true)
                    && object["rank"].as_u64().unwrap_or(0) >= 2
            });
            let eligibility = ScatterEligibility {
                player_scatter: input["player_scatter"].as_bool().unwrap_or(false),
                iq_scatter: input["threshold"].as_i64().unwrap_or(2) as i32,
            };
            let actual: Vec<u64> = selected
                .iter()
                .filter_map(|object| {
                    let id = object["id"].as_u64().unwrap();
                    let facts = object["techno"].as_bool().unwrap_or(true).then(|| {
                        ScatterTechno::from_entity(
                            entities.get(id).unwrap(),
                            Some(&rules),
                            &houses,
                            &interner,
                        )
                    });
                    scatter_dispatch_allowed(
                        eligibility,
                        input["dispatch_all"].as_bool().unwrap_or(false),
                        elite,
                        facts,
                    )
                    .then_some(id)
                })
                .collect();
            let expected: Vec<u64> = row["dispatch"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_u64().unwrap())
                .collect();
            assert_eq!(actual, expected, "{}", input["name"]);
        }
    }

    /// Exercise the production cell-entry classifier with live HouseState and
    /// parsed type abilities. No replacement IQ cache is introduced.
    #[test]
    fn entering_cell_scatter_reads_house_iq_and_shared_ability_owner() {
        let mut entities = EntityStore::new();
        entities.insert(vehicle(1, 5, 5));
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        let owner = victim.owner();
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[E1]\nVeteranAbilities=SCATTER\n",
            ))
            .unwrap();
        let mut houses = std::collections::BTreeMap::new();
        houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        for (iq, rank, expected) in [(1, 0, false), (2, 0, true), (1, 100, true), (1, 0, false)] {
            houses.get_mut(&owner).unwrap().current_iq = iq;
            entities.get_mut(2).unwrap().veterancy = rank;
            let result = classify_drive_crush_phase(
                DriveCrushPhase::EnteringCell,
                &[2],
                &entities,
                1,
                &crate::map::houses::HouseAllianceMap::new(),
                &interner,
                (1280, 1280),
                CrushCapability::new(true, false),
                stock_eligibility(),
                0,
                Some(&rules),
                &houses,
            );
            assert_eq!(
                result,
                if expected {
                    DriveCrushOutcome::Scatter { blockers: vec![2] }
                } else {
                    DriveCrushOutcome::None
                }
            );
        }
    }

    #[test]
    fn classify_drive_crush_phase_full_cell_kills_centered_enemy() {
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();

        let outcome = classify_drive_crush_phase(
            DriveCrushPhase::FullyInCell,
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            stock_eligibility(),
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(outcome, DriveCrushOutcome::Kill { victims: vec![2] });
    }

    #[test]
    fn classify_drive_crush_phase_full_cell_skips_allied_victim() {
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = infantry(2, 5, 5, 2);
        victim.crushable = true;
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();

        let outcome = classify_drive_crush_phase(
            DriveCrushPhase::FullyInCell,
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            stock_eligibility(),
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(outcome, DriveCrushOutcome::None);
    }

    #[test]
    fn prone_infantry_are_crushed_like_standing_infantry() {
        // Prone has no write site at the deploy crush-immunity byte anywhere in
        // the binary, so lying down is not crush immunity: a Grizzly clears a
        // suppressed squad in one pass.
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        victim.infantry = Some(InfantryRuntime {
            is_prone: true,
            ..InfantryRuntime::new()
        });
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();

        let outcome = classify_drive_crush_phase(
            DriveCrushPhase::FullyInCell,
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            stock_eligibility(),
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(outcome, DriveCrushOutcome::Kill { victims: vec![2] });
    }

    #[test]
    fn deployed_gi_is_crushable_but_deployed_guardian_gi_is_not() {
        // `DeployedCrushable=` defaults yes; stock YR sets it to no on exactly
        // one type. So the intuition runs the wrong way — a deployed GI dies
        // under a tank, a deployed Guardian GI does not.
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut gi = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        gi.category = EntityCategory::Infantry;
        gi.crushable = true;
        gi.deployed_crushable = true;
        gi.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
        entities.insert(gi);
        let mut ggi = GameEntity::test_default(3, "GGI", "Soviet", 5, 5);
        ggi.category = EntityCategory::Infantry;
        ggi.crushable = true;
        ggi.deployed_crushable = false;
        ggi.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
        entities.insert(ggi);
        let interner = crate::sim::intern::test_interner();

        let outcome = classify_drive_crush_phase(
            DriveCrushPhase::FullyInCell,
            &[2, 3],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            stock_eligibility(),
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(outcome, DriveCrushOutcome::Kill { victims: vec![2] });
    }

    #[test]
    fn iron_curtained_infantry_is_not_crushed_at_the_kill_site() {
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        victim.invulnerability = Some(
            crate::sim::superweapon::invulnerability::InvulnerabilityState {
                start_frame: 10,
                duration_frames: 750,
                kind: crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
            },
        );
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();
        let call = |frame: u32| {
            classify_drive_crush_phase(
                DriveCrushPhase::FullyInCell,
                &[2],
                &entities,
                1,
                &crate::map::houses::HouseAllianceMap::new(),
                &interner,
                (5 * 256 + 128, 5 * 256 + 128),
                CrushCapability::new(true, false),
                stock_eligibility(),
                frame,
                None,
                &std::collections::BTreeMap::new(),
            )
        };

        assert_eq!(call(100), DriveCrushOutcome::None, "curtain still running");
        assert_eq!(
            call(760),
            DriveCrushOutcome::Kill { victims: vec![2] },
            "curtain expired"
        );
    }

    #[test]
    fn entering_cell_scatter_consumes_no_rng() {
        // The whole native cell-scatter body contains no random draw; the
        // dispatch gate is pure boolean.
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();
        let rng = SimRng::new(0x1234_5678);
        let before = rng.clone();

        let _ = classify_drive_crush_phase(
            DriveCrushPhase::EnteringCell,
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            stock_eligibility(),
            0,
            None,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(rng.state(), before.state());
    }

    // `post_scatter_wait_is_ten_frames_not_blockage_path_delay` used to sit
    // here asserting `POST_SCATTER_WAIT_FRAMES == 10` — a constant against
    // itself, which no behavioural regression could break. The claim it was
    // reaching for is now pinned by observation in
    // `movement_tests::code_two_post_scatter_wait_rearms_on_every_pass_while_the_block_holds`,
    // which watches a blocked mover's timer and sees the 10-frame sawtooth
    // rather than a `BlockagePathDelay` span.

    // -- sub-cell allocation tests --

    #[test]
    fn test_allocate_sub_cell_empty_cell() {
        // No occupancy entry → first spot (2 = NE corner).
        assert_eq!(allocate_sub_cell(None, MovementLayer::Ground), Some(2));
    }

    #[test]
    fn test_allocate_sub_cell_one_infantry() {
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);
        let occ = grid.get(5, 5).unwrap();
        assert_eq!(allocate_sub_cell(Some(occ), MovementLayer::Ground), Some(3));
    }

    #[test]
    fn test_allocate_sub_cell_two_infantry() {
        let grid = make_occ(&[
            (5, 5, 1, MovementLayer::Ground, Some(2)),
            (5, 5, 2, MovementLayer::Ground, Some(3)),
        ]);
        let occ = grid.get(5, 5).unwrap();
        assert_eq!(allocate_sub_cell(Some(occ), MovementLayer::Ground), Some(4));
    }

    #[test]
    fn test_allocate_sub_cell_full() {
        let grid = make_occ(&[
            (5, 5, 1, MovementLayer::Ground, Some(2)),
            (5, 5, 2, MovementLayer::Ground, Some(3)),
            (5, 5, 3, MovementLayer::Ground, Some(4)),
        ]);
        let occ = grid.get(5, 5).unwrap();
        assert_eq!(allocate_sub_cell(Some(occ), MovementLayer::Ground), None);
    }

    #[test]
    fn test_vehicle_blocks_all_sub_cells() {
        let grid = make_occ(&[(5, 5, 99, MovementLayer::Ground, None)]);
        let occ = grid.get(5, 5).unwrap();
        assert_eq!(allocate_sub_cell(Some(occ), MovementLayer::Ground), None);
    }

    #[test]
    fn test_cell_passable_for_infantry_empty() {
        assert!(cell_passable_for_infantry(None, MovementLayer::Ground));
    }

    #[test]
    fn test_cell_passable_for_infantry_with_vehicle() {
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, None)]);
        let occ = grid.get(5, 5).unwrap();
        assert!(!cell_passable_for_infantry(
            Some(occ),
            MovementLayer::Ground
        ));
    }

    // -- collect_crush_victims tests --

    #[test]
    fn test_collect_crush_victims_infantry() {
        let mut store = EntityStore::new();
        let inf = infantry(1, 5, 5, 2);
        store.insert(inf);

        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);

        let fixture = GateFixture::new();
        let victims = collect_crush_victims(
            (5, 5),
            &grid,
            MovementLayer::Ground,
            CrushCapability::new(true, false),
            &store,
            fixture.enemy(),
        );
        assert_eq!(victims, vec![1]);
    }

    #[test]
    fn test_collect_crush_victims_non_crushable() {
        let mut store = EntityStore::new();
        let mut inf = infantry(1, 5, 5, 2);
        inf.crushable = false;
        store.insert(inf);

        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);

        let fixture = GateFixture::new();
        let victims = collect_crush_victims(
            (5, 5),
            &grid,
            MovementLayer::Ground,
            CrushCapability::new(true, false),
            &store,
            fixture.enemy(),
        );
        assert!(victims.is_empty());
    }

    #[test]
    fn test_collect_crush_victims_skips_deployed_uncrushable_infantry() {
        let mut store = EntityStore::new();
        let mut inf = infantry(1, 5, 5, 2);
        inf.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
        inf.deployed_crushable = false;
        store.insert(inf);

        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);

        let fixture = GateFixture::new();
        let victims = collect_crush_victims(
            (5, 5),
            &grid,
            MovementLayer::Ground,
            CrushCapability::new(true, false),
            &store,
            fixture.enemy(),
        );
        assert!(victims.is_empty());
    }

    #[test]
    fn test_collect_crush_victims_keeps_deployed_crushable_infantry_crushable() {
        let mut store = EntityStore::new();
        let mut inf = infantry(1, 5, 5, 2);
        inf.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
        inf.deployed_crushable = true;
        store.insert(inf);

        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);

        let fixture = GateFixture::new();
        let victims = collect_crush_victims(
            (5, 5),
            &grid,
            MovementLayer::Ground,
            CrushCapability::new(true, false),
            &store,
            fixture.enemy(),
        );
        assert_eq!(victims, vec![1]);
    }

    #[test]
    fn test_collect_crush_victims_keeps_prone_infantry_for_regular_crusher() {
        // Prone is not crush immunity — the crush predicate reads only the
        // deploy byte, and nothing in the binary writes it for prone.
        let mut store = EntityStore::new();
        let mut inf = infantry(1, 5, 5, 2);
        inf.infantry = Some(InfantryRuntime {
            fear_level: 50,
            is_prone: true,
            ..InfantryRuntime::new()
        });
        store.insert(inf);

        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);

        let fixture = GateFixture::new();
        let victims = collect_crush_victims(
            (5, 5),
            &grid,
            MovementLayer::Ground,
            CrushCapability::new(true, false),
            &store,
            fixture.enemy(),
        );
        assert_eq!(victims, vec![1]);
    }

    // -- scatter_blocker tests --

    /// Native interior gate evidence only: forced obstruction calls, before
    /// candidate selection and RNG. The harness bypasses special animation
    /// entry gates; this is not a complete Scatter or walking parity claim.
    #[test]
    fn infantry_forced_scatter_gates_match_native_oracle() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../tools/infantry_scatter_oracle.json"))
                .unwrap();
        let mut checked = 0;
        for case in oracle["scatter_gates"].as_array().unwrap() {
            let flag = |key: &str| case[key].as_bool().unwrap();
            if !flag("first_bool") {
                continue;
            }
            let rules = scatter_rules(flag("fraidycat"));
            let mut gi = infantry(1, 5, 5, 2);
            set_mission(
                &mut gi,
                if flag("mission_scatter") {
                    crate::sim::mission::MissionType::Move
                } else {
                    crate::sim::mission::MissionType::Sleep
                },
            );
            if flag("has_target") {
                gi.attack_target = Some(crate::sim::combat::AttackTarget::new(9));
            }
            let admitted = !flag("moving")
                || moving_blocker_accepts_forced_scatter(&gi, Some(&rules), flag("fraidycat"));
            assert_eq!(admitted, flag("gate_admitted"), "native case: {case}");
            checked += 1;
        }
        assert_eq!(checked, 32);
    }

    #[test]
    fn damage_scatter_admission_matches_original_execution() {
        use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
        use crate::sim::animation::{Animation, SequenceKind};
        use crate::sim::house_state::HouseState;
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/infantry_damage_scatter.json"
        ))
        .unwrap();
        let mut checked = 0;
        for row in corpus.as_array().unwrap() {
            let input = &row["input"];
            let flag = |name: &str, default: bool| input[name].as_bool().unwrap_or(default);
            let doing = input["doing"].as_i64().unwrap_or(-1);
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[General]\nFixture=1\n[InfantryTypes]\n0=E1\n[E1]\nSpeed=4\nFraidycat={}\nVeteranAbilities={}\nEliteAbilities={}\n[Guard]\nScatter={}\n[CombatDamage]\nPlayerScatter={}\n",
                flag("fraidycat", true),
                if flag("veteran_scatter", false) { "SCATTER" } else { "" },
                if flag("elite_scatter", false) { "SCATTER" } else { "" },
                flag("mission_scatter", true), flag("player_scatter", false),
            ))).unwrap();
            let mut victim = infantry(1, 5, 5, 2);
            let interner = crate::sim::intern::test_interner();
            victim.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
                EntityCategory::Infantry,
            );
            victim
                .mission_leaf
                .set_infantry_doing_verified(doing as i32)
                .unwrap();
            // Deliberately disagree with Doing: presentation cannot admit or
            // refuse simulation work, including native-only action codes.
            victim.animation = Some(Animation::new(SequenceKind::Die1));
            victim.veterancy = input["rank"].as_u64().unwrap_or(0) as u16 * 100;
            victim.locomotor = Some(
                crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                    crate::rules::locomotor_type::LocomotorKind::Walk,
                ),
            );
            set_mission(&mut victim, crate::sim::mission::MissionType::Guard);
            if flag("target", false) {
                victim.attack_target = Some(crate::sim::combat::AttackTarget::new(9));
            }
            if flag("nav", false) {
                victim.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(9, 9));
            }
            if flag("moving", false) {
                victim.movement_target = Some(crate::sim::components::MovementTarget {
                    path: vec![(5, 5), (6, 5)],
                    next_index: 1,
                    speed: SimFixed::from_num(100),
                    ..Default::default()
                });
            }
            let mut house = HouseState::new(victim.owner(), 0, None, flag("human", false), 0, 10);
            house.player_control = flag("player_control", false);
            let mut teams = crate::sim::team_script_vm::TeamScriptVm::default();
            if flag("team", false) {
                teams.create_team(victim.owner(), victim.type_ref(), vec![1], None, 0);
            }
            let mut rng = SimRng::new(42);
            let before_rng = rng.state();
            let result = select_infantry_damage_scatter(
                &victim,
                (1000, 1000),
                Some(&flat_resolved_terrain(20, 20)),
                Some(
                    crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(
                        16, -16, -16, 64, 64,
                    ),
                ),
                &OccupancyGrid::new(),
                &rules,
                house.is_controlled_by_human(flag("game_mode_nonzero", true)),
                &teams,
                &mut rng,
                &interner,
            );
            let admitted = row["admitted"].as_bool().unwrap();
            assert_eq!(result.is_some(), admitted, "{input}");
            assert_eq!(
                rng.state() != before_rng,
                admitted,
                "refused call must not draw: {input}"
            );
            checked += 1;
        }
        assert_eq!(checked, 278);
    }

    #[test]
    fn damage_scatter_uses_the_same_normal_type_speed() {
        let rules = scatter_rules(true);
        let mut civilian = infantry(1, 5, 5, 2);
        let interner = crate::sim::intern::test_interner();
        civilian.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object("E1").unwrap(),
                0,
            ),
        );
        let mut rng = SimRng::new(42);
        let scatter = select_infantry_damage_scatter(
            &civilian,
            (0, 0),
            Some(&flat_resolved_terrain(20, 20)),
            Some(
                crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(
                    16, -16, -16, 64, 64,
                ),
            ),
            &OccupancyGrid::new(),
            &rules,
            false,
            &crate::sim::team_script_vm::TeamScriptVm::default(),
            &mut rng,
            &interner,
        )
        .expect("unoccupied civilian damage scatter");
        assert_eq!(
            scatter.speed,
            crate::util::fixed_math::ra2_speed_to_leptons_per_second(4)
        );
    }

    #[test]
    fn test_scatter_blocker_issues_movement() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(42);

        let mut store = EntityStore::new();
        let v = vehicle(1, 5, 5);
        store.insert(v);

        let result = scatter_blocker(
            &mut store,
            1,
            Some(&grid),
            None,
            &occupancy,
            MovementLayer::Ground,
            &mut rng,
            None,
            &crate::sim::intern::test_interner(),
            crate::sim::movement::DestinationTiming::new(0, 60),
        );
        assert!(result, "scatter_blocker should succeed with open cells");

        // Blocker should now have a movement_target (walking, not teleported).
        let e = store.get(1).unwrap();
        assert!(
            e.movement_target.is_some(),
            "Blocker should have a movement command"
        );
        // Position should NOT have changed yet — blocker walks on next tick.
        assert_eq!(e.position.rx, 5);
        assert_eq!(e.position.ry, 5);
    }

    #[test]
    fn test_scatter_blocker_all_blocked() {
        let grid = PathGrid::new(3, 3);
        let mut occupancy = OccupancyGrid::new();
        for &(dx, dy) in &NEIGHBOR_OFFSETS {
            let nx = (1 + dx) as u16;
            let ny = (1 + dy) as u16;
            occupancy.add(
                nx,
                ny,
                100,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let mut rng = SimRng::new(42);

        let mut store = EntityStore::new();
        let v = vehicle(1, 1, 1);
        store.insert(v);

        let result = scatter_blocker(
            &mut store,
            1,
            Some(&grid),
            None,
            &occupancy,
            MovementLayer::Ground,
            &mut rng,
            None,
            &crate::sim::intern::test_interner(),
            crate::sim::movement::DestinationTiming::new(0, 60),
        );
        assert!(!result, "scatter_blocker should fail when all blocked");
        assert!(store.get(1).unwrap().movement_target.is_none());
    }

    fn moving_target() -> crate::sim::components::MovementTarget {
        crate::sim::components::MovementTarget {
            path: vec![(5, 5), (6, 5)],
            path_layers: vec![MovementLayer::Ground; 2],
            next_index: 1,
            speed: crate::util::fixed_math::SimFixed::from_num(1024),
            ..Default::default()
        }
    }

    fn set_mission(entity: &mut GameEntity, mission: crate::sim::mission::MissionType) {
        entity
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: crate::sim::mission::MissionId::from_known(mission),
                suspended: crate::sim::mission::MissionId::NONE,
                queued: crate::sim::mission::MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
    }

    fn scatter_rules(fraidycat: bool) -> crate::rules::ruleset::RuleSet {
        let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[E1]\nSpeed=4\nFraidycat={fraidycat}\n[MTNK]\nSpeed=6\n[Move]\nRate=.016\n[Sleep]\nScatter=no\n"
        ));
        crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap()
    }

    /// `UnitClass::Scatter` never queries its locomotor — with the force byte
    /// the locomotor blocked-cell path always passes, a MOVING vehicle is
    /// displaced. VERA previously refused every moving blocker outright.
    #[test]
    fn scatter_blocker_displaces_a_moving_vehicle_under_force() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(42);
        let rules = scatter_rules(false);

        let mut store = EntityStore::new();
        let mut v = vehicle(1, 5, 5);
        v.movement_target = Some(moving_target());
        set_mission(&mut v, crate::sim::mission::MissionType::Move);
        store.insert(v);

        assert!(
            scatter_blocker(
                &mut store,
                1,
                Some(&grid),
                None,
                &occupancy,
                MovementLayer::Ground,
                &mut rng,
                Some(&rules),
                &crate::sim::intern::test_interner(),
                crate::sim::movement::DestinationTiming::new(0, 60),
            ),
            "a moving vehicle must still be scattered — the vehicle body has no Is_Moving gate"
        );
    }

    /// `InfantryClass::Scatter` has a SECOND force-gated early-out after the
    /// mission-`Scatter=` test: a non-`Fraidycat` type that already holds a
    /// shoot-at target refuses the scatter once the force byte has been demoted
    /// (which happens for exactly the infantry whose locomotor reports moving).
    #[test]
    fn scatter_blocker_refuses_a_moving_targeting_non_fraidycat_infantryman() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(42);
        let rules = scatter_rules(false);

        let mut store = EntityStore::new();
        let mut i = infantry(1, 5, 5, 0);
        i.movement_target = Some(moving_target());
        i.attack_target = Some(crate::sim::combat::AttackTarget::new(9));
        set_mission(&mut i, crate::sim::mission::MissionType::Move);
        store.insert(i);

        assert!(
            !scatter_blocker(
                &mut store,
                1,
                Some(&grid),
                None,
                &occupancy,
                MovementLayer::Ground,
                &mut rng,
                Some(&rules),
                &crate::sim::intern::test_interner(), // Fraidycat=no — every stock combat infantry type
                crate::sim::movement::DestinationTiming::new(0, 60),
            ),
            "a moving, targeting, non-Fraidycat infantryman refuses the demoted-force scatter"
        );

        // A Fraidycat type in exactly the same state still scatters.
        let rules = scatter_rules(true);
        let mut store = EntityStore::new();
        let mut i = infantry(1, 5, 5, 0);
        i.movement_target = Some(moving_target());
        i.attack_target = Some(crate::sim::combat::AttackTarget::new(9));
        set_mission(&mut i, crate::sim::mission::MissionType::Move);
        store.insert(i);
        assert!(
            scatter_blocker(
                &mut store,
                1,
                Some(&grid),
                None,
                &occupancy,
                MovementLayer::Ground,
                &mut rng,
                Some(&rules),
                &crate::sim::intern::test_interner(), // Fraidycat=yes
                crate::sim::movement::DestinationTiming::new(0, 60),
            ),
            "the Fraidycat branch skips the early-out entirely"
        );
    }

    /// The final Fraidycat gate matters even when Move allows scatter and the
    /// soldier has no attack target. A refused request must not replace the
    /// player's destination or consume the scatter direction draw.
    #[test]
    fn scatter_blocker_preserves_moving_gi_order_and_rng() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(42);
        let rules = scatter_rules(false);
        let mut store = EntityStore::new();
        let mut gi = infantry(1, 5, 5, 2);
        gi.movement_target = Some(moving_target());
        set_mission(&mut gi, crate::sim::mission::MissionType::Move);
        store.insert(gi);
        let before_rng = rng.state();
        let before_target = store.get(1).unwrap().movement_target.clone();
        assert!(!scatter_blocker(
            &mut store,
            1,
            Some(&grid),
            None,
            &occupancy,
            MovementLayer::Ground,
            &mut rng,
            Some(&rules),
            &crate::sim::intern::test_interner(),
            crate::sim::movement::DestinationTiming::new(0, 60),
        ));
        assert_eq!(rng.state(), before_rng);
        let target = store.get(1).unwrap().movement_target.as_ref().unwrap();
        let original = before_target.unwrap();
        assert_eq!(target.path, original.path);
        assert_eq!(target.next_index, original.next_index);
        assert_eq!(target.speed, original.speed);
        assert_eq!(
            store.get(1).unwrap().mission.current().known(),
            Some(crate::sim::mission::MissionType::Move)
        );
    }

    /// ...and the same infantryman on a `Scatter=no` mission is refused, because
    /// the demotion left `forced == 0` and the mission flag is the only other
    /// way through the gate.
    #[test]
    fn scatter_blocker_refuses_a_moving_infantryman_on_a_scatter_no_mission() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(42);
        let rules = scatter_rules(true);

        let mut store = EntityStore::new();
        let mut i = infantry(1, 5, 5, 0);
        i.movement_target = Some(moving_target());
        set_mission(&mut i, crate::sim::mission::MissionType::Sleep);
        store.insert(i);

        assert!(
            !scatter_blocker(
                &mut store,
                1,
                Some(&grid),
                None,
                &occupancy,
                MovementLayer::Ground,
                &mut rng,
                Some(&rules),
                &crate::sim::intern::test_interner(),
                crate::sim::movement::DestinationTiming::new(0, 60),
            ),
            "[Sleep] Scatter=no and the force byte was demoted, so the gate refuses"
        );
    }

    /// A STATIONARY infantryman never reaches the demotion, so the caller's
    /// force byte survives and the mission flag cannot refuse it.
    #[test]
    fn scatter_blocker_displaces_a_stationary_infantryman_on_a_scatter_no_mission() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(42);
        let rules = scatter_rules(false);

        let mut store = EntityStore::new();
        let mut i = infantry(1, 5, 5, 0);
        set_mission(&mut i, crate::sim::mission::MissionType::Sleep);
        store.insert(i);

        assert!(
            scatter_blocker(
                &mut store,
                1,
                Some(&grid),
                None,
                &occupancy,
                MovementLayer::Ground,
                &mut rng,
                Some(&rules),
                &crate::sim::intern::test_interner(),
                crate::sim::movement::DestinationTiming::new(0, 60),
            ),
            "forced=1 survives when the object is not moving"
        );
        assert_eq!(
            store
                .get(1)
                .unwrap()
                .movement_target
                .as_ref()
                .unwrap()
                .speed,
            crate::util::fixed_math::ra2_speed_to_leptons_per_second(4),
            "stationary GI scatter retains its ordinary retail type speed"
        );
    }

    #[test]
    fn test_scatter_blocker_skips_structure() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(42);

        let mut store = EntityStore::new();
        store.insert(structure(100, 5, 5));

        let result = scatter_blocker(
            &mut store,
            100,
            Some(&grid),
            None,
            &occupancy,
            MovementLayer::Ground,
            &mut rng,
            None,
            &crate::sim::intern::test_interner(),
            crate::sim::movement::DestinationTiming::new(0, 60),
        );

        assert!(
            !result,
            "scatter_blocker must refuse Structure blockers — buildings are \
             never scatter targets in the original engine"
        );

        // Structure must not have been issued any movement.
        let e = store.get(100).expect("structure still alive");
        assert!(
            e.movement_target.is_none(),
            "Structure must not receive a movement_target from scatter"
        );

        // RNG must NOT have been consumed (determinism: a fresh rng with the
        // same seed gives the same first value as one that hasn't been touched).
        let mut control_rng = SimRng::new(42);
        assert_eq!(
            rng.next_range_u32(8),
            control_rng.next_range_u32(8),
            "scatter_blocker must not consume RNG when bailing on a Structure blocker"
        );
    }

    #[test]
    fn test_scatter_deterministic() {
        let grid = PathGrid::new(10, 10);
        let occupancy = OccupancyGrid::new();

        let mut store1 = EntityStore::new();
        store1.insert(vehicle(1, 5, 5));
        let mut rng1 = SimRng::new(42);
        scatter_blocker(
            &mut store1,
            1,
            Some(&grid),
            None,
            &occupancy,
            MovementLayer::Ground,
            &mut rng1,
            None,
            &crate::sim::intern::test_interner(),
            crate::sim::movement::DestinationTiming::new(0, 60),
        );

        let mut store2 = EntityStore::new();
        store2.insert(vehicle(1, 5, 5));
        let mut rng2 = SimRng::new(42);
        scatter_blocker(
            &mut store2,
            1,
            Some(&grid),
            None,
            &occupancy,
            MovementLayer::Ground,
            &mut rng2,
            None,
            &crate::sim::intern::test_interner(),
            crate::sim::movement::DestinationTiming::new(0, 60),
        );

        let t1 = store1.get(1).unwrap().movement_target.as_ref().unwrap();
        let t2 = store2.get(1).unwrap().movement_target.as_ref().unwrap();
        assert_eq!(t1.path, t2.path, "Scatter must be deterministic");
    }

    // -- allocate_sub_cell_with_reserved tests --

    #[test]
    fn test_allocate_with_reserved_empty_cell_no_reservations() {
        assert_eq!(
            allocate_sub_cell_with_reserved(None, MovementLayer::Ground, None),
            Some(2)
        );
    }

    #[test]
    fn test_allocate_with_reserved_skips_reserved_spot() {
        let reserved: Vec<u8> = vec![2];
        assert_eq!(
            allocate_sub_cell_with_reserved(None, MovementLayer::Ground, Some(&reserved)),
            Some(3)
        );
    }

    #[test]
    fn test_allocate_with_reserved_full_from_reservations() {
        let reserved: Vec<u8> = vec![2, 3, 4];
        assert_eq!(
            allocate_sub_cell_with_reserved(None, MovementLayer::Ground, Some(&reserved)),
            None
        );
    }

    #[test]
    fn test_allocate_with_reserved_full_mixed() {
        let grid = make_occ(&[
            (5, 5, 1, MovementLayer::Ground, Some(2)),
            (5, 5, 2, MovementLayer::Ground, Some(3)),
        ]);
        let occ = grid.get(5, 5).unwrap();
        let reserved: Vec<u8> = vec![4];
        assert_eq!(
            allocate_sub_cell_with_reserved(Some(occ), MovementLayer::Ground, Some(&reserved)),
            None
        );
    }

    #[test]
    fn test_allocate_with_reserved_vehicle_blocks() {
        let grid = make_occ(&[(5, 5, 99, MovementLayer::Ground, None)]);
        let occ = grid.get(5, 5).unwrap();
        assert_eq!(
            allocate_sub_cell_with_reserved(Some(occ), MovementLayer::Ground, None),
            None
        );
    }

    // -- quadrant detection tests --

    #[test]
    fn test_quadrant_center() {
        // Distance from (128,128) is 0 — well within 60-lepton threshold.
        assert_eq!(
            get_subcell_quadrant(SimFixed::from_num(128), SimFixed::from_num(128)),
            0
        );
    }

    #[test]
    fn test_quadrant_near_center() {
        // (150, 140): distance = sqrt(22^2 + 12^2) ≈ 25 — within 60-lepton threshold.
        assert_eq!(
            get_subcell_quadrant(SimFixed::from_num(150), SimFixed::from_num(140)),
            0
        );
    }

    #[test]
    fn test_quadrant_nw_returns_zero() {
        // (40, 40): X<=128, Y<=128 → NW quadrant → returns 0 (merged with center).
        assert_eq!(
            get_subcell_quadrant(SimFixed::from_num(40), SimFixed::from_num(40)),
            0
        );
    }

    #[test]
    fn test_quadrant_ne() {
        // (200, 40): X>128, Y<=128 → bits=1 → returns 2 (NE).
        assert_eq!(
            get_subcell_quadrant(SimFixed::from_num(200), SimFixed::from_num(40)),
            2
        );
    }

    #[test]
    fn test_quadrant_sw() {
        // (40, 200): X<=128, Y>128 → bits=2 → returns 3 (SW).
        assert_eq!(
            get_subcell_quadrant(SimFixed::from_num(40), SimFixed::from_num(200)),
            3
        );
    }

    #[test]
    fn test_quadrant_se() {
        // (200, 200): X>128, Y>128 → bits=3 → returns 4 (SE).
        assert_eq!(
            get_subcell_quadrant(SimFixed::from_num(200), SimFixed::from_num(200)),
            4
        );
    }

    // -- arrival-side claim: the zero-draw half of the sub-cell handshake --

    /// The slot reserved by the look-ahead one cell earlier is the slot the man
    /// stands in on arrival — retail never re-selects.
    #[test]
    fn gsi_06_14_arrival_claims_the_pre_reserved_slot() {
        assert_eq!(
            claim_reserved_sub_cell(None, MovementLayer::Ground, 1, None),
            Some(2),
            "empty cell falls to the first free slot",
        );
        let grid = make_occ(&[(5, 5, 2, MovementLayer::Ground, Some(2))]);
        let occ = grid.get(5, 5);
        assert_eq!(
            claim_reserved_sub_cell(occ, MovementLayer::Ground, 1, Some(4)),
            Some(4),
            "the reserved slot wins over the first-free scan",
        );
        // Taken by someone else meanwhile: fall back deterministically.
        assert_eq!(
            claim_reserved_sub_cell(occ, MovementLayer::Ground, 1, Some(2)),
            Some(3),
        );
    }

    /// The mover has already been inserted into the new cell carrying its old
    /// slot, so it must not count against itself — three men still fit.
    #[test]
    fn gsi_06_14_arrival_claim_excludes_the_mover_itself() {
        let grid = make_occ(&[
            (5, 5, 1, MovementLayer::Ground, Some(2)),
            (5, 5, 2, MovementLayer::Ground, Some(3)),
            (5, 5, 3, MovementLayer::Ground, Some(4)),
        ]);
        let occ = grid.get(5, 5);
        assert_eq!(
            claim_reserved_sub_cell(occ, MovementLayer::Ground, 1, Some(2)),
            Some(2),
            "self-occupancy must not refuse the mover its own slot",
        );
        // A genuine fourth man finds nothing.
        assert_eq!(
            claim_reserved_sub_cell(occ, MovementLayer::Ground, 9, None),
            None,
        );
    }

    /// The lepton-offset inverse used to recover the reserved slot is exact over
    /// the three functional slots and rejects the centre.
    #[test]
    fn gsi_06_14_functional_sub_cell_offset_inverse_is_exact() {
        for slot in FUNCTIONAL_SUB_CELLS {
            let offset = crate::util::lepton::subcell_lepton_offset(Some(slot));
            assert_eq!(functional_sub_cell_from_offset(offset), Some(slot));
        }
        let centre = crate::util::lepton::subcell_lepton_offset(Some(0));
        assert_eq!(functional_sub_cell_from_offset(centre), None);
    }

    // -- priority placement --

    /// GSI-06.14 G1. With the priority byte set, the retail placement function
    /// jumps past every gate straight to `offset[quadrant]`: no occupancy test,
    /// no vehicle/structure blocker test, no garrison test, and — because the
    /// random row selection sits on the branch that jump skips — no draw.
    /// Quadrant 0 resolves to the cell-centre slot, which the ordinary path can
    /// never assign.
    #[test]
    fn gsi_06_14_priority_placement_ignores_occupancy_and_takes_no_draw() {
        let rng = SimRng::new(7);
        let before = rng.state();
        // NE approach → slot 2, even though the ordinary allocator would refuse.
        assert_eq!(
            priority_sub_cell(SimFixed::from_num(200), SimFixed::from_num(40)),
            2,
        );
        assert_eq!(
            priority_sub_cell(SimFixed::from_num(40), SimFixed::from_num(200)),
            3,
        );
        assert_eq!(
            priority_sub_cell(SimFixed::from_num(200), SimFixed::from_num(200)),
            4,
        );
        // Centre request → slot 0, the centre offset.
        assert_eq!(
            priority_sub_cell(SimFixed::from_num(128), SimFixed::from_num(128)),
            0,
        );
        assert_eq!(rng.state(), before, "priority placement draws nothing");
    }

    /// The ordinary allocator refuses exactly the cases priority must accept —
    /// a full cell and a cell holding a vehicle or structure.
    #[test]
    fn gsi_06_14_priority_accepts_what_the_ordinary_allocator_refuses() {
        let full = make_occ(&[
            (5, 5, 1, MovementLayer::Ground, Some(2)),
            (5, 5, 2, MovementLayer::Ground, Some(3)),
            (5, 5, 3, MovementLayer::Ground, Some(4)),
        ]);
        let blocked = make_occ(&[(6, 6, 4, MovementLayer::Ground, None)]);
        let mut rng = SimRng::new(1);
        let ne = (SimFixed::from_num(200), SimFixed::from_num(40));

        assert_eq!(
            allocate_sub_cell_with_preference(
                full.get(5, 5),
                MovementLayer::Ground,
                None,
                ne.0,
                ne.1,
                &mut rng,
            ),
            None,
        );
        assert_eq!(
            allocate_sub_cell_with_preference(
                blocked.get(6, 6),
                MovementLayer::Ground,
                None,
                ne.0,
                ne.1,
                &mut rng,
            ),
            None,
            "a structure or vehicle closes the cell to the ordinary path",
        );
        // Priority reads neither cell's occupancy.
        assert_eq!(priority_sub_cell(ne.0, ne.1), 2);
    }

    // -- preference-aware allocation tests --

    #[test]
    fn test_preference_ne_entry_fast_path() {
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            None,
            MovementLayer::Ground,
            None,
            SimFixed::from_num(200),
            SimFixed::from_num(40),
            &mut rng,
        );
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_preference_ne_entry_occupied_fallback() {
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(2))]);
        let occ = grid.get(5, 5).unwrap();
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            Some(occ),
            MovementLayer::Ground,
            None,
            SimFixed::from_num(200),
            SimFixed::from_num(40),
            &mut rng,
        );
        assert_eq!(result, Some(4));
    }

    #[test]
    fn test_preference_sw_entry() {
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            None,
            MovementLayer::Ground,
            None,
            SimFixed::from_num(40),
            SimFixed::from_num(200),
            &mut rng,
        );
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_preference_sw_entry_occupied_fallback() {
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(3))]);
        let occ = grid.get(5, 5).unwrap();
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            Some(occ),
            MovementLayer::Ground,
            None,
            SimFixed::from_num(40),
            SimFixed::from_num(200),
            &mut rng,
        );
        assert_eq!(result, Some(4));
    }

    #[test]
    fn test_preference_se_entry() {
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            None,
            MovementLayer::Ground,
            None,
            SimFixed::from_num(200),
            SimFixed::from_num(200),
            &mut rng,
        );
        assert_eq!(result, Some(4));
    }

    #[test]
    fn test_preference_se_entry_occupied_fallback() {
        let grid = make_occ(&[(5, 5, 1, MovementLayer::Ground, Some(4))]);
        let occ = grid.get(5, 5).unwrap();
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            Some(occ),
            MovementLayer::Ground,
            None,
            SimFixed::from_num(200),
            SimFixed::from_num(200),
            &mut rng,
        );
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_preference_center_entry_randomizes() {
        let mut seen: BTreeSet<u8> = BTreeSet::new();
        for seed in 0..20u64 {
            let mut rng = SimRng::new(seed);
            let result = allocate_sub_cell_with_preference(
                None,
                MovementLayer::Ground,
                None,
                SimFixed::from_num(128),
                SimFixed::from_num(128),
                &mut rng,
            );
            assert!(result.is_some());
            seen.insert(result.unwrap());
        }
        assert!(seen.contains(&2), "expected sub-cell 2 from randomization");
        assert!(seen.contains(&3), "expected sub-cell 3 from randomization");
        assert!(seen.contains(&4), "expected sub-cell 4 from randomization");
    }

    #[test]
    fn test_preference_all_occupied() {
        let grid = make_occ(&[
            (5, 5, 1, MovementLayer::Ground, Some(2)),
            (5, 5, 2, MovementLayer::Ground, Some(3)),
            (5, 5, 3, MovementLayer::Ground, Some(4)),
        ]);
        let occ = grid.get(5, 5).unwrap();
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            Some(occ),
            MovementLayer::Ground,
            None,
            SimFixed::from_num(200),
            SimFixed::from_num(40),
            &mut rng,
        );
        assert_eq!(result, None);
    }

    #[test]
    fn test_preference_respects_reserved() {
        let reserved: Vec<u8> = vec![2];
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            None,
            MovementLayer::Ground,
            Some(&reserved),
            SimFixed::from_num(200),
            SimFixed::from_num(40),
            &mut rng,
        );
        assert_eq!(result, Some(4));
    }

    #[test]
    fn test_preference_vehicle_blocks() {
        let grid = make_occ(&[(5, 5, 99, MovementLayer::Ground, None)]);
        let occ = grid.get(5, 5).unwrap();
        let mut rng = SimRng::new(42);
        let result = allocate_sub_cell_with_preference(
            Some(occ),
            MovementLayer::Ground,
            None,
            SimFixed::from_num(200),
            SimFixed::from_num(40),
            &mut rng,
        );
        assert_eq!(result, None);
    }

    // -- emit_crush_kill_sounds tests --

    fn build_test_rules(
        crush_sound: Option<&str>,
        die_sound: Option<&str>,
    ) -> crate::rules::ruleset::RuleSet {
        let mut e1 = String::from("Strength=125\nArmor=none\nSpeed=4\n");
        if let Some(s) = crush_sound {
            e1.push_str(&format!("CrushSound={}\n", s));
        }
        if let Some(s) = die_sound {
            e1.push_str(&format!("DieSound={}\n", s));
        }
        let ini_text = format!(
            "[InfantryTypes]\n0=E1\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n[E1]\n{}\n",
            e1
        );
        let ini = crate::rules::ini_parser::IniFile::from_str(&ini_text);
        crate::rules::ruleset::RuleSet::from_ini(&ini).expect("test rules build")
    }

    fn build_victim(
        interner: &mut crate::sim::intern::StringInterner,
        rx: u16,
        ry: u16,
    ) -> GameEntity {
        let mut victim = infantry(1, rx, ry, 2);
        victim.type_ref = interner.intern("E1");
        victim
    }

    #[test]
    fn emit_crush_kill_sounds_uses_only_crush_sound_when_both_keys_set() {
        let rules = build_test_rules(Some("InfantrySquish"), Some("GIDie"));
        let mut interner = crate::sim::intern::StringInterner::new();
        let victim = build_victim(&mut interner, 5, 5);
        let mut events = Vec::new();

        emit_crush_kill_sounds(&victim, &rules, &mut interner, &mut events);

        assert_eq!(events.len(), 1, "expected 1 event, got {:?}", events);
        let crushed = events.iter().find_map(|e| match e {
            crate::sim::world::SimSoundEvent::EntityCrushed {
                crush_sound_id,
                rx,
                ry,
            } => Some((*crush_sound_id, *rx, *ry)),
            _ => None,
        });
        let (cid, crx, cry) = crushed.expect("missing EntityCrushed");
        assert_eq!(interner.resolve(cid), "InfantrySquish");
        assert_eq!((crx, cry), (5, 5));

        assert!(
            !events
                .iter()
                .any(|event| matches!(event, crate::sim::world::SimSoundEvent::EntityDied { .. }))
        );
    }

    #[test]
    fn emit_crush_kill_sounds_skips_crush_when_field_is_none() {
        let rules = build_test_rules(None, Some("GIDie"));
        let mut interner = crate::sim::intern::StringInterner::new();
        let victim = build_victim(&mut interner, 7, 9);
        let mut events = Vec::new();

        emit_crush_kill_sounds(&victim, &rules, &mut interner, &mut events);

        assert!(events.is_empty());
    }

    #[test]
    fn emit_crush_kill_sounds_emits_crush_when_die_field_is_none() {
        let rules = build_test_rules(Some("InfantrySquish"), None);
        let mut interner = crate::sim::intern::StringInterner::new();
        let victim = build_victim(&mut interner, 3, 4);
        let mut events = Vec::new();

        emit_crush_kill_sounds(&victim, &rules, &mut interner, &mut events);

        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            crate::sim::world::SimSoundEvent::EntityCrushed { .. }
        ));
    }

    #[test]
    fn emit_crush_kill_sounds_no_events_when_both_none() {
        let rules = build_test_rules(None, None);
        let mut interner = crate::sim::intern::StringInterner::new();
        let victim = build_victim(&mut interner, 1, 1);
        let mut events = Vec::new();

        emit_crush_kill_sounds(&victim, &rules, &mut interner, &mut events);

        assert!(events.is_empty(), "expected no events, got {:?}", events);
    }
}
