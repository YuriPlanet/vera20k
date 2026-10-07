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

use crate::sim::cell_kernel::{self, CellQueryPoint};
use crate::sim::pathfinding::BlockerNeighborCounts;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::components::DriveCoord;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellOccupancy, OccupancyGrid};
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
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> BlockerNeighborCounts {
    let mut counts = blocker_plane_base(width, height, resolved_terrain, overlay_grid);
    let retained_foot = overlay_grid.is_some();
    for entity in entities.values() {
        if let Some(source) = blocker_plane_source(entity, interner, rules, retained_foot) {
            source.add_to(&mut counts);
        }
    }
    counts
}

/// Retained wall/Foot bytes plus the existing derived terrain contribution.
/// Foot lifecycle writes are authoritative when an overlay grid is present;
/// only fixtures without one reconstruct mobile position contributions.
pub(crate) fn blocker_plane_base(
    width: u16,
    height: u16,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
) -> BlockerNeighborCounts {
    let retained_wall_counts = overlay_grid.map(|grid| {
        assert_eq!(
            (grid.width(), grid.height()),
            (width, height),
            "retained wall-neighbor authority must match pathfinding grid"
        );
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

/// `CellClass::PlaceInfantryInCell @ 0x00481180` with priority 0: the infantry
/// spot for a request at `(sub_x, sub_y)` inside cell `(rx, ry)` on `layer`,
/// read from the cell's native occupation bytes.
///
/// The selected byte (deck on a bridge, else ground) refuses the whole cell on
/// its vehicle bit (`0x0048126B..0x0048128A`); the ground byte's object bit
/// refuses either plane (`0x00481298`) unless the 0x40 occupier is a passable
/// Gate (`Gate=`, `+0x16B7`, `0x004525F0`).
///
/// RESIDUAL: the slave, crew, drop, unload and parasite cell-coordinate callers
/// lack the live Gate context and still supply `false`. A ground object bit
/// belonging to an open Gate therefore refuses before RNG where native may
/// admit; their native comparisons do not cover that exception. Callers with
/// the context use the native-cell owner.
/// The building bit (0x80) is not read, so
/// a dying building's own cells admit its crew. Those refusals return before
/// any draw. A request within 60 leptons of the centre, or in the north-west
/// quadrant (preference 0, `0x004811F5..0x00481212`), then draws its
/// `RandomRanged(0, 3)` row rotation on the Scenario stream before scanning,
/// even when every spot is taken; a NE, SW or SE request prefers its own spot
/// and draws nothing.
pub(crate) fn place_infantry_in_cell(
    raw: &crate::sim::occupancy::RawCellOccupationGrid,
    rx: u16,
    ry: u16,
    layer: MovementLayer,
    sub_x: SimFixed,
    sub_y: SimFixed,
    rng: &mut SimRng,
) -> Option<u8> {
    place_infantry_in_native_cell(
        raw,
        crate::sim::occupancy::RawCellKey::Real(rx, ry),
        layer,
        DriveCoord {
            x: (i32::from(rx) << 8).wrapping_add(sub_x.to_num::<i32>()),
            y: (i32::from(ry) << 8).wrapping_add(sub_y.to_num::<i32>()),
            z: 0,
        },
        false,
        false,
        rng,
    )
}

/// The ground Gate input to `CellClass::PlaceInfantryInCell @ 0x00481180`:
/// `0x00481298..0x00481313` reads the first ground building (`0x0047C4D0`),
/// `Gate=` and its canonical `BuildingClass::IsOpenGate @ 0x004525F0` answer.
/// The caller keeps the priority, selected-plane vehicle and ground-object
/// gates before this live read; no occupation byte or RNG is changed here.
pub(crate) fn ground_gate_is_open(
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    cell: (u16, u16),
) -> bool {
    occupancy
        .first_building_on_layer(cell.0, cell.1, MovementLayer::Ground)
        .and_then(|id| entities.get(id))
        .is_some_and(|building| {
            rules
                .and_then(|rules| rules.object(interner.resolve(building.type_ref())))
                .is_some_and(|object| object.gate)
                && building.is_open_gate()
        })
}

/// `CellClass::PlaceInfantryInCell @ 0x00481180` selection on the CellClass
/// a Map lookup returned, including the shared off-map cell. The caller owns
/// the input XYZ, selected plane and the ground Gate's passability. Preference
/// reads incoming XY low bytes; priority skips raw blockers and RNG. Ordinary
/// placement checks selected-plane vehicles and ground objects before the
/// centre-row Scenario draw, including when the selected plane is full.
///
/// `walk_head_occupation` compares all 176 original selection controls. Packed
/// output XYZ remains with `walk_head::selected_head`; its caller samples ground
/// at the original input independently of the selected subcell offset.
pub(crate) fn place_infantry_in_native_cell(
    raw: &crate::sim::occupancy::RawCellOccupationGrid,
    cell: crate::sim::occupancy::RawCellKey,
    layer: MovementLayer,
    input: DriveCoord,
    priority: bool,
    ground_gate_open: bool,
    rng: &mut SimRng,
) -> Option<u8> {
    let preferred = cell_kernel::infantry_preferred_spot(CellQueryPoint {
        x: input.x,
        y: input.y,
    });
    if priority {
        return Some(preferred);
    }
    let ground = raw.bits_at(cell, MovementLayer::Ground);
    let mask = if layer == MovementLayer::Bridge {
        raw.bits_at(cell, MovementLayer::Bridge)
    } else {
        ground
    };
    let refusal_bits = (mask & cell_kernel::INFANTRY_OCCUPATION_VEHICLE_BIT)
        | (ground & cell_kernel::INFANTRY_OCCUPATION_OBJECT_BIT);
    if !cell_kernel::infantry_occupation_allows(refusal_bits, true, ground_gate_open) {
        return None;
    }
    let random_row = (preferred == 0).then(|| rng.next_range_u32(4) as u8);
    cell_kernel::select_infantry_subcell(preferred, mask, false, random_row)
}

/// The older cell-list approximation of [`place_infantry_in_cell`], still
/// used by the walk FindSubCellDest pre-allocation (`movement_step`), the tube
/// exit and the landed-aircraft unload.
///
/// RESIDUAL: unlike `CellClass::PlaceInfantryInCell @ 0x00481180` it refuses
/// a building occupant (native ignores bit 0x80) and returns before the
/// centre-row `RandomRanged(0, 3)` draw on a cell already holding three
/// infantry (native draws first). Trigger: infantry walking or unloading into
/// a crowded cell. Effect: one Scenario draw fewer per such placement.
/// Moving the walk and tube callers to the native byte read needs the
/// movement pass's raw occupation to be current at the crossing, which the
/// movement ledger owns. The landed-aircraft unload (`0x00415B10`) reaches
/// PlaceInfantryInCell through `InfantryClass::Unlimbo @ 0x0051DFF0`, whose
/// Z gate (`0x0051E01B`) skips placement for a coordinate above the floor, so
/// it moves with the aircraft unload port, not here.
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
/// Object5F6CD0 restricts buildings only in the omni arm. Its ordinary arm
/// has no class restriction and is also the fallback from a refused omni arm.
/// All GameEntity categories carry the native Techno abstract bit; non-Techno
/// objects and wall overlays use their own receivers. Native comparisons:
/// tools/spatial_oracle/unit_entry and cell_entry_crush_tail.
/// NO-DIFF (GSI-08.17) — pass 1's named gap is not one. A crushed unit's
/// `DeathWeapon=` does not fire in gamemd either:
/// `TechnoClass::Fire_Death_Weapon @ 0x0070D690` has exactly three callers,
/// `ReceiveDamage @ 0x00701900`, `0x004CD600` (which
/// `FlyLocomotionClass::Process @ 0x004CCB40` calls every frame)
/// and the Unit's crash notice (`0x007461EF`), and the crush loop at
/// `0x007416A0` enters none of them. Detonating a crushed
/// Terrorist would be a regression, not a fix. Native's crush consequences are
/// the crusher-positioned `CrushSound`, the victim's `vt+0x170` (`0x007418E5`:
/// Infantry `0x00710460` `FreeAllMindControlCaptures`; Unit `0x00746D60`,
/// `UnitClass::Death_Explosion` with its Scenario picks and anims, then the
/// same release), `Record_The_Kill` (score, trigger events, EVA and the
/// crusher's veterancy, which this engine now pays), then unmark, limbo and
/// `UnInit`; no smudge. A crushed vehicle's `Death_Explosion` is not run
/// here (residual in `combat/destruction_effects.rs`).
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
    capability.can_crush_units() && object_is_crushable_by(capability.omni_crusher, target)
}

/// Object5F6CD0 after the caller's capability gate and alliance check.
/// The post-crush-latch Unit entry tail invokes this without retesting Crusher.
pub(crate) fn object_is_crushable_by(omni_crusher: bool, target: CrushTarget) -> bool {
    ((omni_crusher && target.category != EntityCategory::Structure && !target.omni_crush_resistant)
        || (target.crushable && !target.deploy_crush_immune))
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
    // Native admission and kill-site readers consume the retained byte,
    // not current Doing or the Unit deployment controller. Infantry's
    // sequencer520B4E/520BAD owns its writes through GameEntity.
    entity.native_crush_immunity() != 0
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

/// The occupants a crusher fully inside the cell kills, in cell-list order:
/// the `entering == 0` crush loop of `UnitClass::PerCellProcess`. The
/// entering cell's unforced scatter is `Scatter_Objects`' dispatch walk
/// ([`super::scatter::scatter_objects_admitted`]).
///
/// `current_frame` feeds the Iron Curtain gate of the crush predicate.
#[allow(clippy::too_many_arguments)]
pub fn select_crush_victims(
    occ: &[u64],
    entities: &EntityStore,
    crusher_id: u64,
    alliances: &crate::map::houses::HouseAllianceMap,
    interner: &crate::sim::intern::StringInterner,
    crusher_coord: (i32, i32),
    capability: CrushCapability,
    current_frame: u32,
) -> Vec<u64> {
    if !capability.can_crush_units() {
        return Vec::new();
    }
    let Some(crusher) = entities.get(crusher_id) else {
        return Vec::new();
    };
    // The same gate admission uses. Two inline copies of one predicate is how
    // admission and the kill came to disagree.
    let allies = CrushAllyGate::new(interner.resolve(crusher.owner()), alliances, interner);
    occ.iter()
        .copied()
        .filter(|&id| id != crusher_id)
        .filter(|&id| {
            entities.get(id).is_some_and(|victim| {
                !allies.spares(victim)
                    && within_crush_distance_sq(crusher_coord, entity_crush_coord(victim))
                    && can_crush(capability, CrushTarget::from_entity(victim, current_frame))
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Scatter
// ---------------------------------------------------------------------------
//
// `CellClass::Scatter_Objects`, its dispatch gate and the class Scatter
// receivers live in `movement::scatter` and `movement::infantry_scatter`.

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
// destination cell of a blocked step.

/// Normal speed shared by blocked-cell and damage-triggered displacement.
pub(super) fn scatter_movement_speed(
    entity: &GameEntity,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    houses: &std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    >,
) -> SimFixed {
    // Scatter installs a destination; the walking process still calls
    // InfantryClass::GetCurrentSpeed (0x00521D80), delegating to
    // FootClass::GetCurrentSpeed (0x004DB1A0), so it stamps an ordinary Move's
    // speed.
    let obj = rules.and_then(|r| r.object(interner.resolve(entity.type_ref())));
    super::order_speed(entity, obj, rules, houses)
}

/// One accepted nonfatal Infantry damage scatter, selected before the
/// receiver's fear callback mutates the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InfantryDamageScatter {
    pub(crate) destination: (u16, u16),
    pub(crate) speed: SimFixed,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::game_entity::{GameEntity, InfantryRuntime};
    use crate::sim::occupancy::CellListInsertion;
    use std::collections::BTreeSet;

    fn flat_resolved_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_loader_clear_cell,
        )
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
        let mut e = GameEntity::test_default_of_category(
            id,
            "GAREFN",
            "Allies",
            rx,
            ry,
            EntityCategory::Structure,
        );
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
    fn finalized_wall_plane_is_sole_baseline_without_identity_double_count() {
        use crate::map::authored_overlay::FinalizedOverlayPayload;
        use crate::sim::overlay_grid::OverlayGrid;

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

        let (ground, dynamic) = super::super::block_index::build_owner_block_set(
            &entities, "Russians", &alliances, &interner, None,
        );
        assert!(ground.is_empty());
        assert!(!dynamic.contains_any(&(2, 2)));

        let counts = build_blocker_neighbor_counts_with_overlays(
            &entities, 5, 5, None, None, &interner, None,
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
    fn test_omni_resistance_and_deploy_immunity_refuse_both_arms() {
        assert!(!can_crush(
            CrushCapability::new(false, true),
            target(EntityCategory::Infantry, true, true, true, false),
        ));
    }

    #[test]
    fn test_omni_building_refusal_falls_back_to_explicit_crushable() {
        for crushable in [false, true] {
            assert_eq!(
                can_crush(
                    CrushCapability::new(false, true),
                    target(EntityCategory::Structure, crushable, false, false, false),
                ),
                crushable
            );
        }
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

    #[test]
    fn full_cell_crush_kills_centered_enemy() {
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
        victim.category = EntityCategory::Infantry;
        victim.crushable = true;
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();

        let outcome = select_crush_victims(
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            0,
        );

        assert_eq!(outcome, vec![2]);
    }

    #[test]
    fn full_cell_crush_skips_allied_victim() {
        let mut entities = EntityStore::new();
        let mut crusher = vehicle(1, 5, 5);
        crusher.regular_crusher = true;
        entities.insert(crusher);
        let mut victim = infantry(2, 5, 5, 2);
        victim.crushable = true;
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();

        let outcome = select_crush_victims(
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            0,
        );

        assert_eq!(outcome, Vec::<u64>::new());
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

        let outcome = select_crush_victims(
            &[2],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            0,
        );

        assert_eq!(outcome, vec![2]);
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
        gi.set_infantry_deploy_crush_immunity(0);
        entities.insert(gi);
        let mut ggi = GameEntity::test_default(3, "GGI", "Soviet", 5, 5);
        ggi.category = EntityCategory::Infantry;
        ggi.crushable = true;
        ggi.deployed_crushable = false;
        ggi.set_infantry_deploy_crush_immunity(1);
        entities.insert(ggi);
        let interner = crate::sim::intern::test_interner();

        let outcome = select_crush_victims(
            &[2, 3],
            &entities,
            1,
            &crate::map::houses::HouseAllianceMap::new(),
            &interner,
            (5 * 256 + 128, 5 * 256 + 128),
            CrushCapability::new(true, false),
            0,
        );

        assert_eq!(outcome, vec![2]);
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
            crate::sim::superweapon::invulnerability::InvulnerabilityState::new(
                crate::sim::timer::CdTimer::started(10, 750),
                crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
            ),
        );
        entities.insert(victim);
        let interner = crate::sim::intern::test_interner();
        let call = |frame: u32| {
            select_crush_victims(
                &[2],
                &entities,
                1,
                &crate::map::houses::HouseAllianceMap::new(),
                &interner,
                (5 * 256 + 128, 5 * 256 + 128),
                CrushCapability::new(true, false),
                frame,
            )
        };

        assert_eq!(call(100), Vec::<u64>::new(), "curtain still running");
        assert_eq!(call(760), vec![2], "curtain expired");
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
        inf.set_infantry_deploy_crush_immunity(1);
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
        inf.set_infantry_deploy_crush_immunity(0);
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

    // -- scatter speed --

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
        assert_eq!(
            scatter_movement_speed(
                &civilian,
                Some(&rules),
                &interner,
                &std::collections::BTreeMap::new(),
            ),
            crate::util::fixed_math::ra2_speed_to_leptons_per_second(4)
        );
    }

    fn scatter_rules(fraidycat: bool) -> crate::rules::ruleset::RuleSet {
        let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[E1]\nSpeed=4\nFraidycat={fraidycat}\n[MTNK]\nSpeed=6\n[Move]\nRate=.016\n[Sleep]\nScatter=no\n"
        ));
        crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap()
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

    fn raw_cells(bits: &[(u16, u16, u8)]) -> crate::sim::occupancy::RawCellOccupationGrid {
        let mut raw = crate::sim::occupancy::RawCellOccupationGrid::new();
        for &(rx, ry, mask) in bits {
            raw.mark_ground(rx, ry, mask);
        }
        raw
    }

    const SPOTS_FULL: u8 = (1 << 2) | (1 << 3) | (1 << 4);

    fn place(raw: &crate::sim::occupancy::RawCellOccupationGrid, x: i32, y: i32) -> Option<u8> {
        let mut rng = SimRng::new(42);
        place_infantry_in_cell(
            raw,
            5,
            5,
            MovementLayer::Ground,
            SimFixed::from_num(x),
            SimFixed::from_num(y),
            &mut rng,
        )
    }

    /// PlaceInfantryInCell with priority 0 refuses exactly the cases priority
    /// placement must accept: a full cell and a cell holding a vehicle.
    #[test]
    fn gsi_06_14_priority_accepts_what_the_ordinary_allocator_refuses() {
        let full = raw_cells(&[(5, 5, SPOTS_FULL)]);
        let vehicle = raw_cells(&[(5, 5, 0x20)]);
        let ne = (SimFixed::from_num(200), SimFixed::from_num(40));

        assert_eq!(place(&full, 200, 40), None);
        assert_eq!(
            place(&vehicle, 200, 40),
            None,
            "a vehicle closes the cell to the ordinary path",
        );
        // Priority reads neither cell's occupancy.
        assert_eq!(priority_sub_cell(ne.0, ne.1), 2);
    }

    #[test]
    fn test_preference_quadrants_take_their_own_spot_or_fall_back() {
        let empty = raw_cells(&[]);
        assert_eq!(place(&empty, 200, 40), Some(2), "NE");
        assert_eq!(place(&empty, 40, 200), Some(3), "SW");
        assert_eq!(place(&empty, 200, 200), Some(4), "SE");
        assert_eq!(place(&raw_cells(&[(5, 5, 1 << 2)]), 200, 40), Some(4));
        assert_eq!(place(&raw_cells(&[(5, 5, 1 << 3)]), 40, 200), Some(4));
        assert_eq!(place(&raw_cells(&[(5, 5, 1 << 4)]), 200, 200), Some(2));
    }

    #[test]
    fn test_preference_center_entry_randomizes() {
        let empty = raw_cells(&[]);
        let mut seen: BTreeSet<u8> = BTreeSet::new();
        for seed in 0..20u64 {
            let mut rng = SimRng::new(seed);
            let result = place_infantry_in_cell(
                &empty,
                5,
                5,
                MovementLayer::Ground,
                SimFixed::from_num(128),
                SimFixed::from_num(128),
                &mut rng,
            );
            seen.insert(result.expect("an empty cell admits the centre request"));
        }
        assert!(seen.contains(&2), "expected sub-cell 2 from randomization");
        assert!(seen.contains(&3), "expected sub-cell 3 from randomization");
        assert!(seen.contains(&4), "expected sub-cell 4 from randomization");
    }

    /// `0x0048138A..0x0048139F` draws the centre row before the scan, so a
    /// full cell still spends it; an off-centre request and a refused cell
    /// draw nothing.
    #[test]
    fn a_centre_request_draws_its_row_even_on_a_full_cell() {
        let full = raw_cells(&[(5, 5, SPOTS_FULL)]);
        let centre = SimFixed::from_num(128);
        let mut rng = SimRng::new(7);
        let before = rng.state();
        let result =
            place_infantry_in_cell(&full, 5, 5, MovementLayer::Ground, centre, centre, &mut rng);
        assert_eq!(result, None);
        let mut one_draw = SimRng::new(7);
        let _ = one_draw.next_range_u32(4);
        assert_eq!(rng.state(), one_draw.state());

        let mut rng = SimRng::new(7);
        let refused = raw_cells(&[(5, 5, 0x20)]);
        let result = place_infantry_in_cell(
            &refused,
            5,
            5,
            MovementLayer::Ground,
            centre,
            centre,
            &mut rng,
        );
        assert_eq!(result, None);
        assert_eq!(
            rng.state(),
            before,
            "a vehicle refusal returns before the draw"
        );
    }

    /// The building bit (0x80) is not read, so a dying building's own cells
    /// admit its crew. The vehicle bit is read from the requested plane
    /// (`0x0048126B..0x0048128A`); the object bit (0x40) always from the
    /// ground byte (`0x00481298`), so it refuses a deck request too.
    #[test]
    fn the_building_bit_does_not_block_and_the_ground_object_bit_blocks_both_planes() {
        use crate::sim::occupancy::RawCellOccupationGrid;

        assert_eq!(place(&raw_cells(&[(5, 5, 0x80)]), 200, 40), Some(2));
        assert_eq!(place(&raw_cells(&[(5, 5, 0x40)]), 200, 40), None);
        let deck_request = |mark: fn(&mut RawCellOccupationGrid)| {
            let mut raw = RawCellOccupationGrid::new();
            mark(&mut raw);
            let mut rng = SimRng::new(42);
            place_infantry_in_cell(
                &raw,
                5,
                5,
                MovementLayer::Bridge,
                SimFixed::from_num(200),
                SimFixed::from_num(40),
                &mut rng,
            )
        };
        assert_eq!(deck_request(|raw| raw.mark_deck(5, 5, 0x40)), Some(2));
        assert_eq!(deck_request(|raw| raw.mark_ground(5, 5, 0x40)), None);
        assert_eq!(deck_request(|raw| raw.mark_deck(5, 5, 0x20)), None);
        assert_eq!(deck_request(|raw| raw.mark_ground(5, 5, 0x20)), Some(2));
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
