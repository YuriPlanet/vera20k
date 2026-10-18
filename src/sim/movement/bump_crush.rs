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
        if let Some(source) =
            blocker_plane_source(entity, resolved_terrain, interner, rules, retained_foot)
        {
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
/// A building's is the CellClass `+0x122` ring of `BuildingClass::Unlimbo`
/// (`0x00440CD9..0x00440CEA`), which `BuildingClass::Limbo` undoes (`0x00445D11..0x00445D22`).
pub(crate) fn blocker_plane_source(
    entity: &GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    retained_foot: bool,
) -> Option<BlockerPlaneSource> {
    if retained_foot && entity.category() != EntityCategory::Structure {
        return None;
    }
    // Dying corpses are off the occupancy grid: don't let them inflate the
    // A* dynamic-blocker neighbor costs.
    if entity.death.is_dying() || !entity.lifecycle.cell_marked() {
        return None;
    }
    if entity.passenger_role.is_inside_transport()
        || crate::sim::occupancy::cell_list_layer_for_entity(entity, terrain).is_none()
    {
        return None;
    }
    let pos = (entity.position.rx(), entity.position.ry());
    if entity.category() == EntityCategory::Structure {
        let size = rules
            .and_then(|r| interner.object_type(entity.type_ref(), r))
            .map(|obj| crate::rules::foundation::foundation_dimensions(&obj.foundation))
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
/// RESIDUAL: the slave, crew, drop and unload cell-coordinate callers lack the
/// live Gate context and still supply `false`. A ground object bit
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
                .and_then(|rules| interner.object_type(building.type_ref(), rules))
                .is_some_and(|object| object.gate)
                && building.is_open_gate()
        })
}

/// [`ground_gate_is_open`] where `PlaceInfantryInCell` reaches it
/// (`0x00481298..0x00481313`): only for a cell whose selected plane holds no
/// vehicle (`0x20`) and whose ground holds an object (`0x40`). The shared
/// off-map cell holds no Gate.
pub(crate) fn native_ground_gate_open(
    raw: &crate::sim::occupancy::RawCellOccupationGrid,
    occupancy: &OccupancyGrid,
    entities: &EntityStore,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    key: crate::sim::occupancy::RawCellKey,
    layer: MovementLayer,
) -> bool {
    let ground = raw.bits_at(key, MovementLayer::Ground);
    raw.bits_at(key, layer) & cell_kernel::INFANTRY_OCCUPATION_VEHICLE_BIT == 0
        && ground & cell_kernel::INFANTRY_OCCUPATION_OBJECT_BIT != 0
        && match key {
            crate::sim::occupancy::RawCellKey::Real(x, y) => {
                ground_gate_is_open(occupancy, entities, rules, interner, (x, y))
            }
            crate::sim::occupancy::RawCellKey::Dummy => false,
        }
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

impl crate::sim::world::Simulation {
    /// `MapClass::GetCellAt @ 0x00565730` for `point`, then
    /// [`place_infantry_in_native_cell`] on that cell's ground plane without
    /// priority: the spot, or `None` where it answers the zeroed coordinate
    /// at `0x0089E778` (`0x004812EC`, `0x004813CC`).
    /// AircraftClass::Drop_Payload (`0x00415D9B..0x00415DD1`),
    /// InfantryClass::ReceiveDamage's InfDeath 9 arm
    /// (`0x0051897C..0x00518A0D`), the Infantry tube exit (`0x0051B967`) and
    /// ParasiteClass::GetReleaseCoords (`0x0062ADE9`) ask it so.
    pub(crate) fn place_infantry_in_ground_cell(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        point: DriveCoord,
    ) -> Option<u8> {
        use crate::sim::occupancy::RawCellKey;
        let key = match self.resolved_terrain.as_ref() {
            Some(terrain) => RawCellKey::from_native(
                terrain,
                crate::map::resolved_terrain::NativeCellQuery::canonical(terrain)
                    .lookup_world(point.x, point.y),
            ),
            None => RawCellKey::Real((point.x / 256) as u16, (point.y / 256) as u16),
        };
        self.place_infantry_in_ground_key(rules, key, point)
    }

    /// PlaceInfantryInCell with useCellCoords (`0x00481212`) on `cell`, the
    /// two Approach calls (`0x004D5EC6`, `0x004D60E4`): `request` picks the
    /// spot as [`Self::place_infantry_in_ground_cell`] does, the spot's XY is
    /// the cell's own origin (its `vt+0x48` centre, low byte cleared) plus the
    /// spot's offset, and its Z the ground at `request` (`0x00578080` at
    /// `0x0048146E`). `None` is the empty coordinate.
    pub(crate) fn place_infantry_spot_in_cell(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        cell: crate::map::cell_index::NativeCellIdentity,
        request: DriveCoord,
    ) -> Result<Option<DriveCoord>, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("PlaceInfantryInCell requires map cells")?;
        let key = crate::sim::occupancy::RawCellKey::from_native(terrain, cell);
        let Some(slot) = self.place_infantry_in_ground_key(rules, key, request) else {
            return Ok(None);
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(
            self.resolved_terrain.as_ref().unwrap(),
        );
        let [x, y] = cell_kernel::native_cell_own_xy(cell, &cells);
        let z = super::ground_pose::query_ground_height(&cells, request)?;
        Ok(Some(super::walk_head::selected_head(
            DriveCoord { x, y, z },
            slot,
            z,
            false,
        )))
    }

    fn place_infantry_in_ground_key(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        key: crate::sim::occupancy::RawCellKey,
        request: DriveCoord,
    ) -> Option<u8> {
        let gate_open = native_ground_gate_open(
            &self.substrate.raw_cell_occupation,
            &self.substrate.occupancy,
            &self.substrate.entities,
            Some(rules),
            &self.interner,
            key,
            MovementLayer::Ground,
        );
        place_infantry_in_native_cell(
            &self.substrate.raw_cell_occupation,
            key,
            MovementLayer::Ground,
            request,
            false,
            gate_open,
            &mut self.scenario_rng,
        )
    }
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
}

pub const CRUSH_DISTANCE_SQ_LIMIT: i64 = 0x3fff;

pub fn within_crush_distance_sq(crusher: (i32, i32), victim: (i32, i32)) -> bool {
    let dx = i64::from(victim.0 - crusher.0);
    let dy = i64::from(victim.1 - crusher.1);
    dx * dx + dy * dy <= CRUSH_DISTANCE_SQ_LIMIT
}

/// The victim's GetCoords XY (vt+0x48, `0x007417FB`) `TechnoClass::DistanceSquaredTo`
/// (`0x005F6560`) measures in `UnitClass::Crush_Or_Prescatter_Cell` (`0x007416A0`).
fn entity_crush_coord(entity: &GameEntity) -> (i32, i32) {
    (
        i32::from(entity.position.rx()) * 256 + entity.position.sub_x().to_num::<i32>(),
        i32::from(entity.position.ry()) * 256 + entity.position.sub_y().to_num::<i32>(),
    )
}

/// The victim-side inputs of the native crush predicate.
///
/// Exactly five type-level facts and two instance facts enter the decision:
/// `Crushable=`, `OmniCrushResistant=`, the victim's class id, the ally
/// relation (the crusher's House asked about the victim's `owner`), the
/// deploy crush-immunity byte, and the Iron Curtain timer. There is no
/// weight, size, armour or `TypeImmune=` term.
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
    /// The victim's owning House, which the ally test asks about.
    pub owner: crate::sim::intern::InternedId,
}

impl CrushTarget {
    /// Read the crush inputs off a live entity at a known sim frame.
    pub fn from_entity(entity: &GameEntity, current_frame: u32) -> Self {
        Self {
            category: entity.category(),
            crushable: entity.crushable_facts.crushable(),
            deploy_crush_immune: deploy_crush_immune(entity),
            omni_crush_resistant: entity.mover_facts.omni_crush_resistant(),
            iron_curtained: crate::sim::superweapon::invulnerability::is_invulnerable(
                entity.invulnerability.as_ref(),
                current_frame,
            ),
            owner: entity.owner(),
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
pub fn can_crush(
    capability: CrushCapability,
    target: CrushTarget,
    allied: impl FnOnce(crate::sim::intern::InternedId) -> bool,
) -> bool {
    capability.can_crush_units() && object_is_crushable_by(capability.omni_crusher, target, allied)
}

/// `ObjectClass::Is_Crushable_By @ 0x005F6CD0` after the caller's capability
/// gate: the omni arm, else the ordinary arm, each refused for a victim the
/// crusher's House is allied with or one Iron Curtained. The post-crush-latch
/// Unit entry tail invokes this without retesting Crusher.
///
/// `allied` is the crusher's owning House (`+0x21C`) asking
/// `HouseClass::Is_Ally_ByObject @ 0x004F9A90` about the victim's owner, at
/// `0x005F6D1A` and again at `0x005F6D67`
/// ([`crate::sim::combat::combat_weapon::is_ally_by_object`]). It reads only
/// the asking House's mask, so a victim whose House allies the crusher's one
/// way (Neutral's with a paranoid computer's, `sim::house_diplomacy`) is still
/// crushed. `Can_Enter_Cell` asks the same question again at `0x0073FB53`, so
/// an allied or own crushable never reaches its crush latch.
pub(crate) fn object_is_crushable_by(
    omni_crusher: bool,
    target: CrushTarget,
    allied: impl FnOnce(crate::sim::intern::InternedId) -> bool,
) -> bool {
    ((omni_crusher && target.category != EntityCategory::Structure && !target.omni_crush_resistant)
        || (target.crushable && !target.deploy_crush_immune))
        && !allied(target.owner)
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

/// Emit the normal crush-path `EntityCrushed` (CrushSound) event for a single
/// victim. Native crush teardown does not also enter the ordinary DieSound
/// path. The event is skipped when CrushSound is absent. Caller must invoke BEFORE
/// removing the victim from the EntityStore so victim.position and
/// victim.type_ref are still valid.
#[cfg(any(test, feature = "test-support"))]
pub fn emit_crush_kill_sounds(
    victim: &crate::sim::game_entity::GameEntity,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &mut crate::sim::intern::StringInterner,
    sound_events: &mut Vec<crate::sim::world::SimSoundEvent>,
) {
    emit_crush_kill_sounds_at(
        victim,
        (
            i32::from(victim.position.rx()),
            i32::from(victim.position.ry()),
        ),
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
    let crusher_owner = crusher.owner();
    occ.iter()
        .copied()
        .filter(|&id| id != crusher_id)
        .filter(|&id| {
            entities.get(id).is_some_and(|victim| {
                within_crush_distance_sq(crusher_coord, entity_crush_coord(victim))
                    && can_crush(
                        capability,
                        CrushTarget::from_entity(victim, current_frame),
                        |owner| {
                            crate::sim::combat::combat_weapon::is_ally_by_object(
                                Some(alliances),
                                interner,
                                crusher_owner,
                                owner,
                            )
                        },
                    )
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
