//! Shared live cell-entry authority for search, movement and repair +1AC receivers.
//! Infantry and Unit retain numeric accumulators; consumers project only the
//! answers they need. Both classes use Foot4D9C60's height/list prelude.
//! Ordered terminal answers remain significant.
//! Numeric Unit evidence: tools/spatial_oracle/unit_entry.{py,json,meta.json}.
use super::Simulation;
use crate::map::{
    cell_index::NativeCellIdentity as Cell, entities::EntityCategory,
    resolved_terrain::ResolvedTerrainGrid,
};
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::rules::{
    locomotor_type::{LocomotorKind, SpeedType},
    mission_data::MissionType,
    object_type::ObjectType,
};
use crate::sim::combat::combat_weapon;
use crate::sim::combat::veterancy::has_weapon_ability;
use crate::sim::movement::bump_crush::{self, CrushCapability};
use crate::sim::movement::infantry_entry::PreludeCells;
use crate::sim::{components::NavTargetRef, game_entity::GameEntity, intern::InternedId};
use crate::sim::{movement::locomotor::MovementLayer, occupancy::CellObjectMember};

#[path = "entry_facts.rs"]
mod entry_facts;
pub(crate) use entry_facts::EntryFacts;
use entry_facts::{BlockerFacts, CellFacts, PlainCell};

#[cfg(test)]
#[path = "unit_entry_tests.rs"]
mod unit_entry_tests;

#[cfg(test)]
#[path = "infantry_entry_priority_tests.rs"]
mod infantry_entry_priority_tests;

#[cfg(test)]
#[path = "wall_entry_tests.rs"]
mod wall_entry_tests;

#[cfg(test)]
#[path = "plain_entry_tests.rs"]
mod plain_entry_tests;

/// The +1AC decision reads live simulation state without borrowing the bridge
/// publisher. Native Map lookups still stamp the canonical shared Dummy through
/// its existing interior-mutable owner, in the same order as movement/repair.
struct EntryReadContext<'a> {
    sim: &'a Simulation,
    rules: &'a RuleSet,
    registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    /// A path batch's kept cell and blocker facts; `None` reads the world at
    /// each ask.
    facts: Option<&'a EntryFacts>,
}

impl<'a> EntryReadContext<'a> {
    fn terrain(&self) -> &'a ResolvedTerrainGrid {
        self.sim
            .resolved_terrain
            .as_ref()
            .expect("live cell-entry terrain")
    }

    fn entry_cell(&self, cell: Cell) -> EntryCell {
        match (cell, self.facts) {
            (Cell::Real(index), Some(facts)) => {
                EntryCell::Kept(*facts.cell(index, || CellFacts::of(self.sim, index)))
            }
            _ => EntryCell::Live(cell),
        }
    }

    /// Real cell `index`'s [`PlainCell`], read through the batch's `facts`.
    fn plain_cell(&self, facts: &EntryFacts, index: usize) -> PlainCell {
        facts.plain(index, || {
            PlainCell::new(
                facts.cell(index, || CellFacts::of(self.sim, index)),
                self.terrain()
                    .tube_for_native_cell(Cell::Real(index))
                    .is_some(),
            )
        })
    }

    fn prelude_reads(&self) -> PreludeReads<'a> {
        PreludeReads {
            sim: self.sim,
            facts: self.facts,
            query: crate::map::resolved_terrain::NativeCellQuery::canonical(self.terrain()),
        }
    }

    /// Blocker `id`'s facts, with its entity for the rare reads they leave
    /// out.
    fn blocker(&self, id: u64) -> Option<(BlockerFacts, &'a GameEntity)> {
        let (slot, entity) = self.sim.substrate.entities.get_in_slot(id)?;
        let read = || BlockerFacts::of(entity, self.sim.session.binary_frame());
        let facts = match self.facts {
            Some(facts) => *facts.blocker(slot, read),
            None => read(),
        };
        Some((facts, entity))
    }

    fn coord(&self, cell: Cell) -> (i16, i16) {
        self.terrain().native_cell_coord(cell)
    }

    fn flags(&self, cell: Cell) -> u32 {
        self.terrain().native_cell_flags(cell)
    }

    fn level(&self, cell: Cell) -> i8 {
        self.terrain().native_cell_ground_fields(cell).0 as i8
    }
}

/// The cell one +1AC ask is about: a batch's kept facts of a real cell, or a
/// cell read from the world at each use. The shared Dummy is always read at
/// each use, because lookups off the map stamp it.
#[derive(Clone, Copy)]
enum EntryCell {
    Kept(CellFacts),
    Live(Cell),
}

impl EntryCell {
    fn coord(&self, live: &EntryReadContext<'_>) -> (i16, i16) {
        match *self {
            Self::Kept(facts) => facts.coord,
            Self::Live(cell) => live.coord(cell),
        }
    }

    fn flags(&self, live: &EntryReadContext<'_>) -> u32 {
        match *self {
            Self::Kept(facts) => facts.flags,
            Self::Live(cell) => live.flags(cell),
        }
    }

    fn level(&self, live: &EntryReadContext<'_>) -> i8 {
        match *self {
            Self::Kept(facts) => facts.level,
            Self::Live(cell) => live.level(cell),
        }
    }

    fn ground_raw(&self, live: &EntryReadContext<'_>) -> (u8, Option<InternedId>) {
        match *self {
            Self::Kept(facts) => facts.ground,
            Self::Live(cell) => raw(live, cell, MovementLayer::Ground),
        }
    }

    fn deck_raw(&self, live: &EntryReadContext<'_>) -> (u8, Option<InternedId>) {
        match *self {
            Self::Kept(facts) => facts.deck,
            Self::Live(cell) => raw(live, cell, MovementLayer::Bridge),
        }
    }

    fn in_playfield(&self, live: &EntryReadContext<'_>) -> bool {
        match *self {
            Self::Kept(facts) => facts.in_playfield,
            Self::Live(cell) => crate::sim::cell_rect::retained_cell_is_in_playfield(
                cell,
                live.sim.playfield_bounds,
                &crate::map::resolved_terrain::NativeCellQuery::canonical(live.terrain()),
            ),
        }
    }

    fn overlay(&self, live: &EntryReadContext<'_>) -> crate::sim::overlay_grid::OverlayCell {
        match *self {
            Self::Kept(facts) => facts.overlay,
            Self::Live(cell) => live.sim.cell_overlay(cell),
        }
    }

    /// Whether the cell's land row for `speed` is nonzero; the Dummy has none.
    fn row_nonzero(&self, live: &EntryReadContext<'_>, speed: SpeedType) -> Result<bool, String> {
        let costs = match *self {
            Self::Kept(facts) => facts.speed_costs,
            Self::Live(Cell::Real(index)) => live.terrain().cells()[index].speed_costs,
            Self::Live(Cell::Dummy) => return Ok(false),
        };
        costs
            .cost_for_speed_type(speed)
            .map(|speed| speed != 0)
            .ok_or_else(|| "repair admission has no resolved speed row".into())
    }

    /// `layer`'s object list at `p`, the coordinate already read for this
    /// cell ([`crate::sim::occupancy::OccupancyGrid::cell_objects`]).
    fn objects<'a>(
        &self,
        live: &EntryReadContext<'a>,
        p: (i16, i16),
        layer: MovementLayer,
    ) -> impl Iterator<Item = CellObjectMember> + 'a {
        let (listed, terrain_object) = match *self {
            Self::Kept(facts) => (facts.listed(layer), facts.terrain_object),
            Self::Live(_) => (true, true),
        };
        let at = (p.0 as u16, p.1 as u16);
        let sim = live.sim;
        let terrain_object = terrain_object
            .then(|| sim.terrain_objects.cell_index().get(&at).copied())
            .flatten();
        listed
            .then(|| {
                sim.substrate
                    .occupancy
                    .cell_objects(at.0, at.1, layer, terrain_object)
            })
            .into_iter()
            .flatten()
    }
}

/// The Foot4D9C60 prelude's reads: a real cell's kept facts in a batch,
/// otherwise the world, whose lookups off the map stamp the Dummy.
struct PreludeReads<'a> {
    sim: &'a Simulation,
    facts: Option<&'a EntryFacts>,
    query: crate::map::resolved_terrain::NativeCellQuery<'a>,
}

impl PreludeReads<'_> {
    fn kept(&self, cell: Cell) -> Option<&CellFacts> {
        let (Cell::Real(index), Some(facts)) = (cell, self.facts) else {
            return None;
        };
        Some(facts.cell(index, || CellFacts::of(self.sim, index)))
    }
}

impl PreludeCells for PreludeReads<'_> {
    fn coord(&self, cell: Cell) -> (i16, i16) {
        // A real cell's coordinate is its index's; only the Dummy's is read.
        self.query.coord(cell)
    }

    fn lookup(&self, coord: (i16, i16)) -> Cell {
        self.query.lookup(coord)
    }

    fn level_slope(&self, cell: Cell) -> (i32, u8) {
        self.kept(cell).map_or_else(
            || self.query.level_slope(cell),
            |facts| (i32::from(facts.level), facts.slope),
        )
    }

    fn flags(&self, cell: Cell) -> u32 {
        self.kept(cell)
            .map_or_else(|| self.query.flags(cell), |facts| facts.flags)
    }
}

/// `HouseClass::Is_Ally_ByObject @ 0x004F9A90` / `Is_Ally`: `asker`'s own
/// mask. Can_Enter_Cell asks from the mover's house (`+0x21C`, e.g.
/// `0x0051C579`, `0x0073FB53`), except at a closed gate
/// ([`EntryMover::is_ally_of`]).
fn allied(live: &EntryReadContext<'_>, asker: InternedId, other: InternedId) -> bool {
    crate::sim::combat::combat_weapon::is_ally_by_object(
        Some(live.sim.house_alliances.as_map()),
        &live.sim.interner,
        asker,
        other,
    )
}
/// Infantry5227F0 checks raw disguise first, then resolves the actual center
/// Cell before testing owner alliance, detection and the disguised-as House.
fn infantry_disguised_to(
    live: &EntryReadContext<'_>,
    entity: &GameEntity,
    observer: InternedId,
) -> Result<bool, String> {
    let Some(disguise) = entity
        .disguise
        .as_ref()
        .filter(|state| state.is_disguised())
    else {
        return Ok(false);
    };
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    let cell = live
        .terrain()
        .native_cell_identity(((x / 256) as i16, (y / 256) as i16));
    if allied(live, entity.owner(), observer) {
        return Ok(false);
    }
    let coord = live.coord(cell);
    let detected =
        live.sim
            .fog
            .detects_disguise_for_house(observer, coord.0 as u16, coord.1 as u16);
    Ok(crate::sim::cloak_disguise::is_disguised_to(
        true,
        false,
        detected,
        disguise
            .house()
            .is_some_and(|fake| allied(live, observer, fake)),
        disguise.house().is_some(),
    ))
}
fn raw(live: &EntryReadContext<'_>, cell: Cell, layer: MovementLayer) -> (u8, Option<InternedId>) {
    let key = crate::sim::occupancy::RawCellKey::from_native(live.terrain(), cell);
    live.sim
        .substrate
        .raw_cell_occupation
        .occupant_at(key, layer)
}
// Unit73F5EF/73F628/73F823 compare actual object pointers, never a Cell
// sharing the blocker's coordinates. Infantry has a distinct Cell shortcut.
fn object_target_is(mover: &GameEntity, blocker: u64) -> bool {
    mover.navigation.nav_com() == Some(NavTargetRef::object(blocker))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfantryTargetAdmission {
    Ordinary,
    DirectTarget,
    SkipCurrent,
}

fn infantry_target_admission(
    live: &EntryReadContext<'_>,
    mover: &GameEntity,
    obj: &ObjectType,
    blocker_id: u64,
    blocker: &BlockerFacts,
    selected_cell: Cell,
) -> Result<InfantryTargetAdmission, String> {
    //51C2D3..51C37D: gate effects first. InfantryType EC2=C4 (52453D/
    //825978), EC3=Engineer (524571/82596C). +1D4 is the warp latch.
    if blocker.warped_out {
        return Ok(InfantryTargetAdmission::Ordinary);
    }
    let eligible = match mover.mission.current().known() {
        Some(MissionType::Enter | MissionType::Capture | MissionType::Eaten) => true,
        Some(MissionType::Sabotage) => obj.c4,
        Some(MissionType::AreaGuard | MissionType::Patrol | MissionType::Guard) => obj.engineer,
        _ => false,
    };
    if !eligible {
        return Ok(InfantryTargetAdmission::Ordinary);
    }
    if object_target_is(mover, blocker_id) {
        return Ok(InfantryTargetAdmission::DirectTarget);
    }
    //51C3B1 executes even with NULL or a nonmatching object NavCom. The
    //retained Cell NavCom resolves by identity without another lookup/stamp.
    let current = blocker.position;
    let blocker_cell = live
        .terrain()
        .native_cell_identity(((current.x / 256) as i16, (current.y / 256) as i16));
    let cell_target_is = |cell| {
        if let Some(NavTargetRef::Cell { rx, ry }) = mover.navigation.nav_com() {
            live.terrain()
                .native_fixed_cell_index(rx as i16, ry as i16)
                .map(Cell::Real)
                .unwrap_or(Cell::Dummy)
                == cell
        } else {
            false
        }
    };
    let attack_target_is = |id| {
        mover.attack_target.target().is_some_and(
            |a| matches!(a, crate::sim::combat::TargetKind::Entity(target) if target == id),
        )
    };
    if cell_target_is(blocker_cell) || attack_target_is(blocker_id) {
        return Ok(InfantryTargetAdmission::DirectTarget);
    }

    //51C3CE queries the original selected Cell's GROUND first Building only
    //after all direct-target checks fail. These matches jump to51C70F and
    //continue the list, unlike the direct target's terminal51C71B path.
    let Cell::Real(index) = selected_cell else {
        return Ok(InfantryTargetAdmission::Ordinary);
    };
    let selected = &live.terrain().cells()[index];
    let Some(first_id) = live.sim.substrate.occupancy.first_building_on_layer(
        selected.rx,
        selected.ry,
        MovementLayer::Ground,
    ) else {
        return Ok(InfantryTargetAdmission::Ordinary);
    };
    if first_id == blocker_id {
        return Ok(InfantryTargetAdmission::Ordinary);
    }
    let first = live
        .sim
        .substrate
        .entities
        .get(first_id)
        .ok_or("repair first-Building target has retired entity")?;
    if object_target_is(mover, first_id) {
        return Ok(InfantryTargetAdmission::SkipCurrent);
    }
    //51C3F6 uses Building+48 (foundation center), not the approach+4C.
    //565730 runs even for NULL/nonmatching object NavCom, before +2B4.
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(first);
    let first_cell = live
        .terrain()
        .native_cell_identity(((x / 256) as i16, (y / 256) as i16));
    if cell_target_is(first_cell) || attack_target_is(first_id) {
        return Ok(InfantryTargetAdmission::SkipCurrent);
    }
    Ok(InfantryTargetAdmission::Ordinary)
}

fn head_on(mover: &GameEntity, blocker: &BlockerFacts, frame: u32) -> bool {
    use crate::util::direction_tables::{dir_from_facing16, facing16_from_delta};
    let direction = dir_from_facing16(mover.body_facing_current(frame));
    if direction != dir_from_facing16(blocker.facing.wrapping_add(0x7fff)) {
        return false;
    }
    let a = crate::sim::movement::ground_pose::position_world_coord(&mover.position);
    let b = blocker.position;
    //73F976 uses the same native atan polynomial/constants as FacingFromDelta.
    direction
        == dir_from_facing16(facing16_from_delta(
            b.x.wrapping_sub(a.x),
            b.y.wrapping_sub(a.y),
        ))
        && crate::util::native_x87::distance_3d_leptons([a.x, a.y, a.z], [b.x, b.y, b.z]) <= 511
}

impl Simulation {
    /// Actual Infantry+1AC. The returned quotient preserves the distinct
    /// consumers in Foot4D3920, Infantry51DAF0 and the repair receiver487A10.
    pub(crate) fn infantry_can_enter(
        &self,
        id: u64,
        cell: Cell,
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<crate::sim::movement::infantry_entry::InfantryEntryClass, String> {
        if self
            .substrate
            .entities
            .get(id)
            .is_none_or(|e| e.category() != EntityCategory::Infantry)
        {
            return Err("Infantry entry requires a live Infantry receiver".into());
        }
        self.foot_can_enter(id, cell, args, rules, registry)
            .map(crate::sim::movement::infantry_entry::InfantryEntryClass::from_raw)
    }

    /// The class's own `Can_Enter_Cell` (+0x1AC) for a live Foot receiver:
    /// Infantry 0x0051BF90 or Unit 0x0073F0A0, as the raw native code 0..7.
    /// Foot4D3920's target query and the Drive/Ship Process continuations
    /// (0x4B2B17, 0x4B2FF9 and the Ship twins) consume the code directly.
    /// Native comparisons: tools/spatial_oracle/unit_entry* corpora.
    /// Search and movement share this fallible read context. Canonical Map
    /// queries can still stamp the shared off-map Dummy; they are not cloned.
    pub(crate) fn foot_can_enter(
        &self,
        id: u64,
        cell: Cell,
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<u8, String> {
        if self.substrate.entities.get(id).is_none_or(|e| {
            !matches!(
                e.category(),
                EntityCategory::Infantry | EntityCategory::Unit
            )
        }) {
            return Err("Foot entry requires a live Infantry or Unit receiver".into());
        }
        // An installed oracle seam answers before the receiver's type is
        // resolved, as it did before the search shared this receiver.
        #[cfg(test)]
        if let Some(terrain) = self.resolved_terrain.as_ref()
            && let Some(code) = crate::sim::movement::fresh_oracle_seam::supplied_can_enter(
                terrain.native_cell_coord(cell),
                args.direction,
                args.height,
            )
        {
            return Ok(code);
        }
        self.foot_entry_receiver(id, rules, registry, None)?
            .can_enter(cell, args)
    }

    /// `AircraftClass::Can_Enter_Cell` (`0x004196B0`) for a live aircraft,
    /// as its native code: 0 admits, 7 refuses. Fly landing (`0x004CE840`)
    /// consumes it, and so does [`Self::mover_can_enter`]'s Aircraft arm.
    /// The body reads only its cell argument (`0x004196B5`; it returns with
    /// `RET 0x14`).
    /// - The occupant arm tests Winged passability (SpeedType 4 at
    ///   `0x0041974D`), which always succeeds, so it never refuses.
    /// - In game mode 0, an aircraft owned by the current house that is not
    ///   mission-only (`+0x41A`, `+0x3D4`) refuses a cell whose ground is
    ///   shrouded: `MapClass::IsShrouded` (`0x00586360`, the one port
    ///   `vision::coordinate_is_shrouded`) at the cell's centre
    ///   (`0x00419764..0x004197A7`). A Cell is open when the owner has mapped
    ///   its ground (not current sight); the shared dummy never is.
    ///
    /// Native executions: tools/spatial_oracle/fly_can_enter.{py,json,meta.json}.
    /// The Team arm (`0x004196BC..0x004196DE`) uses the single Team6EC300
    /// port before the shroud test; its executable controls are in
    /// tools/spatial_oracle/team_creation.
    pub(crate) fn aircraft_can_enter(&self, id: u64, cell: (i16, i16)) -> u8 {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return 0;
        };
        let cell = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(cell);
        self.aircraft_can_enter_cell(id, cell)
    }

    /// [`Self::aircraft_can_enter`] on a cell its caller has already looked
    /// up (the native argument is the `CellClass`).
    pub(crate) fn aircraft_can_enter_cell(&self, id: u64, cell: Cell) -> u8 {
        let e = self.substrate.entities.get(id).unwrap();
        // Aircraft4196BC queries Team6EC300 before owner/mission/shroud.
        if self.member_team_is_moving_outside_playfield(id) {
            return 0;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return 0;
        };
        if self.session.game_mode_nonzero()
            || !e.discovery.owned_by_current_house()
            || e.is_mission_only()
        {
            return 0;
        }
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let coord = terrain.native_cell_coord(cell);
        let shrouded = crate::sim::vision::cell_is_shrouded(
            &self.fog,
            &cells,
            e.owner(),
            (coord.0 as u16, coord.1 as u16),
        );
        if shrouded { 7 } else { 0 }
    }

    /// The live Building `id`'s own `Can_Enter_Cell` (vtable `+0x1AC`,
    /// `BuildingClass::Can_Enter_Cell @ 0x00449440`) at `cell`, for input: 0
    /// or 7. It reads no argument but the cell, as Techno What_Action
    /// on a Cell asks it (`0x00700B5F..0x00700B7E`).
    pub(crate) fn building_can_enter(
        &self,
        id: u64,
        cell: (i16, i16),
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<u8, String> {
        let e = self
            .substrate
            .entities
            .get(id)
            .ok_or("Can_Enter_Cell of a retired building")?;
        let obj = self
            .interner
            .object_type(e.type_ref(), rules)
            .ok_or("Can_Enter_Cell of a building without its type")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Building entry requires map cells")?;
        // An input query: its own fallback Cell, never the simulation's.
        let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
        let cell = cells.lookup(cell);
        let live = EntryReadContext {
            sim: self,
            rules,
            registry,
            facts: None,
        };
        building_impassable(&live, e, obj, cell, Some(&cells)).map(|hard| if hard { 7 } else { 0 })
    }

    /// A mover's own `Can_Enter_Cell` (vtable `+0x1AC`) at a cell, as its
    /// native code: [`Self::aircraft_can_enter`] for an Aircraft, which reads
    /// no argument but the cell, otherwise [`Self::foot_can_enter`] with
    /// `args` on the cell's canonical lookup.
    ///
    /// `mode` retains the fifth argument: GetDock44EFB0 uses1 for its
    /// preferred/scan cells and0 for offered/table cells; Object5F4F3C's
    /// ordinary Unlimbo gate uses0. Foot4D9C10 forwards1 to the installed
    /// locomotor's +1C, whose eight active retail receivers are all the
    /// side-effect-free constant-zero55ABF0 leaf. The class owner already
    /// seeds its result0, so both modes have identical numeric behavior.
    /// Native48-call proof: tools/spatial_oracle/foot_locomotor_entry.*;
    /// bounded factory/depot modes: basic-factory-exit-geometry-research.
    ///
    /// Callers: the group spread (`0x0064D52F`), the team centre
    /// (`0x006EAEE0`), Jumpjet State 4 (`0x0054C66D`) and Teleport's Unit arm
    /// (`0x0071911D`). Each decides what an answer VERA cannot read (`Err`)
    /// means for it; native has no such state. Teleport refuses the request.
    pub(crate) fn mover_can_enter(
        &self,
        id: u64,
        cell: (i16, i16),
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
        _mode: crate::sim::movement::infantry_entry::EntryQueryMode,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<u8, String> {
        let mover = self
            .substrate
            .entities
            .get(id)
            .ok_or("Can_Enter_Cell of a retired mover")?;
        if mover.category() == EntityCategory::Aircraft {
            return Ok(self.aircraft_can_enter(id, cell));
        }
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Foot entry requires map cells")?;
        let cell = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain).lookup(cell);
        self.foot_can_enter(id, cell, args, rules, registry)
    }
}

/// Repair's object-list controller uses the same numeric decision owner as
/// Foot search/movement, including non-Foot residents' class-specific receivers.
pub(super) fn classify_object(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    object: CellObjectMember,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Result<u8, String> {
    classify_entry(
        &EntryReadContext {
            sim,
            rules,
            registry,
            facts: None,
        },
        object,
        cell,
        args,
    )
}

fn classify_entry(
    live: &EntryReadContext<'_>,
    object: CellObjectMember,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Result<u8, String> {
    let CellObjectMember::Entity(id) = object else {
        return terrain_impassable(live, object, cell).map(|hard| if hard { 7 } else { 0 });
    };
    let e = live
        .sim
        .substrate
        .entities
        .get(id)
        .ok_or("repair admission retired entity")?;
    let obj = live
        .sim
        .interner
        .object_type(e.type_ref(), live.rules)
        .ok_or("repair admission missing ObjectType")?;
    if e.category() == EntityCategory::Structure {
        return building_impassable(live, e, obj, cell, None).map(|hard| if hard { 7 } else { 0 });
    }
    if e.category() == EntityCategory::Aircraft {
        return aircraft_entry(live, e, cell);
    }
    classify_foot_entry(live, &EntryMover::new(live, id, e, obj), cell, args)
}

/// The receiver's own facts one Foot +1AC call reads: its type, weapon slot 0
/// and crusher state. Nothing in them changes while a Find_Path search runs
/// (Mark0 has already lifted the mover and the search writes no object), so
/// the search resolves them once instead of per neighbour.
struct EntryMover<'a> {
    id: u64,
    e: &'a GameEntity,
    obj: &'a ObjectType,
    weapon0: Option<&'a crate::rules::weapon_type::WeaponType>,
    crusher: bool,
    /// Answers of `allied(mover owner, house)` already asked, and blocker
    /// types already resolved by name; the alliance graph and rules are fixed
    /// for the receiver's lifetime. A search asks these for every blocker it
    /// meets.
    allies: InternedCache<bool>,
    types: InternedCache<&'a ObjectType>,
    /// The receiver's `weapon_damage_value`, which the Infantry branches
    /// compare with zero for each enemy blocker. The receiver is borrowed
    /// unchanged for the mover's lifetime, so one computation serves them all.
    damage: std::cell::OnceCell<i32>,
}

/// Facts already looked up, direct-mapped by interned index. A slot two
/// names share keeps the last one, so the other is only looked up again.
/// Slots hold their name's index plus one: a new cache is all zeros, cheap
/// to build for a receiver that answers one query.
struct InternedCache<T>([std::cell::Cell<(u32, Option<T>)>; 64]);

impl<T: Copy> InternedCache<T> {
    fn new() -> Self {
        Self(std::array::from_fn(|_| std::cell::Cell::new((0, None))))
    }

    /// `id`'s kept fact, else `look_up`'s, kept when there is one.
    fn get_or(&self, id: InternedId, look_up: impl FnOnce() -> Option<T>) -> Option<T> {
        let tag = id.index().wrapping_add(1);
        let slot = &self.0[id.index() as usize % self.0.len()];
        if let (kept, Some(fact)) = slot.get()
            && kept == tag
        {
            return Some(fact);
        }
        let fact = look_up()?;
        slot.set((tag, Some(fact)));
        Some(fact)
    }
}

impl<'a> EntryMover<'a> {
    /// `UnitClass::Can_Enter_Cell @ 0x0073F0A0`'s rank reads: GetWeapon(0) (vt+0x3F8,
    /// `0x0073F49B`), and `Crusher=` (`+0xD28`) or `HasWeaponAbility(0x11)` (`0x0073FC6C`).
    fn new(live: &EntryReadContext<'a>, id: u64, e: &'a GameEntity, obj: &'a ObjectType) -> Self {
        let weapon0 = combat_weapon::weapon_for_index(obj, e.veterancy.rank(), 0)
            .and_then(|(name, _)| live.rules.weapon(name));
        let crusher =
            obj.crusher || has_weapon_ability(e.veterancy.rank_kind(), obj, Ability::Crusher);
        Self {
            id,
            e,
            obj,
            weapon0,
            crusher,
            allies: InternedCache::new(),
            types: InternedCache::new(),
            damage: std::cell::OnceCell::new(),
        }
    }

    fn weapon_damage(&self, live: &EntryReadContext<'_>) -> i32 {
        *self.damage.get_or_init(|| {
            crate::sim::combat::combat_weapon::weapon_damage_value(
                self.e,
                self.obj,
                &live.sim.substrate.entities,
                live.rules,
                &live.sim.interner,
            )
        })
    }

    fn object_type(&self, live: &EntryReadContext<'a>, id: InternedId) -> Option<&'a ObjectType> {
        self.types
            .get_or(id, || live.sim.interner.object_type(id, live.rules))
    }

    fn allied_with(&self, live: &EntryReadContext<'_>, house: InternedId) -> bool {
        self.allies
            .get_or(house, || Some(allied(live, self.e.owner(), house)))
            == Some(true)
    }

    /// `house`'s own mask asked about the mover, as a closed gate's House
    /// asks (`IsAlliedWith @ 0x004F9A50`: Unit `0x0073F6F5..0x0073F702`,
    /// Infantry `0x0051C504..0x0051C511`).
    fn is_ally_of(&self, live: &EntryReadContext<'_>, house: InternedId) -> bool {
        allied(live, house, self.e.owner())
    }
}

/// One live Foot receiver's +1AC with its own facts resolved once. Find_Path's
/// search (`0x00429F54`) asks it for every neighbour; a single query goes
/// through `Simulation::foot_can_enter`, which builds one per call.
pub(crate) struct FootEntryReceiver<'a> {
    live: EntryReadContext<'a>,
    mover: EntryMover<'a>,
}

impl FootEntryReceiver<'_> {
    /// Shared56BCD0 read for this Foot's House. The map lookup and retained
    /// grid remain with their owners; path consumers carry no second census.
    pub(crate) fn house_threat(&self, coord: (i16, i16)) -> Result<i32, String> {
        self.live
            .sim
            .house_threat_at_cell(self.mover.e.owner(), coord)
    }
    pub(crate) fn hierarchy_house_threat(&self, index: i32) -> Result<i32, String> {
        self.live
            .sim
            .houses
            .get(&self.mover.e.owner())
            .ok_or("hierarchy threat requires a live House")?
            .spatial_threat_at_padded_index(index)
    }

    pub(crate) fn can_enter(
        &self,
        cell: Cell,
        args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
    ) -> Result<u8, String> {
        #[cfg(test)]
        if let Some(code) = crate::sim::movement::fresh_oracle_seam::supplied_can_enter(
            self.live.coord(cell),
            args.direction,
            args.height,
        ) {
            return Ok(code);
        }
        classify_foot_entry(&self.live, &self.mover, cell, args)
    }
}

impl Simulation {
    /// The live Infantry or Unit receiver `id`, for repeated +1AC queries
    /// against one unchanged world (a Find_Path search), reading a path
    /// batch's kept `facts` when it has them.
    pub(crate) fn foot_entry_receiver<'a>(
        &'a self,
        id: u64,
        rules: &'a RuleSet,
        registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
        facts: Option<&'a EntryFacts>,
    ) -> Result<FootEntryReceiver<'a>, String> {
        if self.resolved_terrain.is_none() {
            return Err("Foot entry requires map cells".into());
        }
        let live = EntryReadContext {
            sim: self,
            rules,
            registry,
            facts,
        };
        let e = self
            .substrate
            .entities
            .get(id)
            .filter(|e| {
                matches!(
                    e.category(),
                    EntityCategory::Infantry | EntityCategory::Unit
                )
            })
            .ok_or("Foot entry requires a live Infantry or Unit receiver")?;
        let obj = self
            .interner
            .object_type(e.type_ref(), rules)
            .ok_or("repair admission missing ObjectType")?;
        let mover = EntryMover::new(&live, id, e, obj);
        Ok(FootEntryReceiver { live, mover })
    }
}

/// Infantry 0x0051BF90 / Unit 0x0073F0A0 for a resolved Foot receiver.
fn classify_foot_entry<'a>(
    live: &EntryReadContext<'a>,
    mover: &EntryMover<'a>,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Result<u8, String> {
    if let Some(code) = plain_entry(live, mover, cell, args) {
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || full_foot_entry(live, mover, cell, args) == Ok(code),
            "a plain step to {cell:?} answers {code}, unlike the full +1AC"
        );
        return Ok(code);
    }
    full_foot_entry(live, mover, cell, args)
}

/// VERA-only: [`full_foot_entry`]'s answer to a compass step at a known
/// height between two real cells of a path batch when neither cell has a
/// structural bridge (`0x100`) or tube, the target holds no raw occupation,
/// listed object, wall or crate, lies in the playfield and is at the path's
/// height, and the mover is Infantry or an unrestricted Unit. Every test of
/// the full body then passes the step through to the land row: the tube
/// tests find no tube on the target or on the backstep (the source), the
/// Foot4D9C60 prelude admits a level step from a flat cell and selects the
/// ground list and ground raw byte, the object walk finds nothing, and both
/// tails answer 0 for a clear raw byte. So the answer is 0, or 7 for a zero
/// row (a tethered Infantry skips the row and answers 0), read from both
/// cells' [`PlainCell`]s instead of their facts: a search's flood asks this
/// of most cells it reaches. `None` for any other ask, which the full body
/// answers.
fn plain_entry(
    live: &EntryReadContext<'_>,
    mover: &EntryMover<'_>,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Option<u8> {
    let facts = live.facts?;
    let (Cell::Real(target), Some(Cell::Real(source))) = (cell, args.previous_cell) else {
        return None;
    };
    if !(0..8).contains(&args.direction) || args.height == -1 {
        return None;
    }
    let infantry = mover.e.category() == EntityCategory::Infantry;
    if !infantry && mover.obj.movement_restricted_to.is_some() {
        return None;
    }
    let to = live.plain_cell(facts, target);
    if !to.empty() || args.height != i32::from(to.level()) {
        return None;
    }
    if !live.plain_cell(facts, source).flat()
        || crate::sim::movement::infantry_entry::backstep_cell(
            &live.prelude_reads(),
            cell,
            args.direction,
        ) != Cell::Real(source)
    {
        return None;
    }
    if to.overlay() {
        let overlay = facts
            .cell(target, || CellFacts::of(live.sim, target))
            .overlay
            .overlay_id?;
        let flags = live.registry?.flags(overlay)?;
        if flags.crate_type || flags.wall {
            return None;
        }
    }
    if infantry && mover.e.tether.partner().is_some() {
        return Some(0);
    }
    let nonzero = to.row_nonzero(mover.obj.speed_type)?;
    Some(if nonzero { 0 } else { 7 })
}

/// Infantry 0x0051BF90 / Unit 0x0073F0A0, every branch. [`plain_entry`]
/// answers the steps that pass every test here before the land row; a test
/// added before the land row needs its condition there too.
fn full_foot_entry<'a>(
    live: &EntryReadContext<'a>,
    mover: &EntryMover<'a>,
    cell: Cell,
    args: crate::sim::movement::infantry_entry::InfantryEntryArgs,
) -> Result<u8, String> {
    let &EntryMover {
        id,
        e,
        obj,
        weapon0,
        crusher,
        ..
    } = mover;
    let infantry = e.category() == EntityCategory::Infantry;
    let target = live.entry_cell(cell);
    let (mut bits, mut owner) = target.ground_raw(live);
    let initial_level = target.level(live);
    let mut vehicle_occupied = bits & 0x20 != 0;
    if !infantry && let Some(required) = obj.movement_restricted_to {
        let Cell::Real(index) = cell else {
            return Ok(7);
        };
        let c = &live.terrain().cells()[index];
        if c.land_type == crate::rules::terrain_rules::LandType::Tunnel.as_index() {
            //73F12E..73F1D9: passing the Tube shape gate bypasses only
            //required-land equality; later list/speed/raw predicates still run.
            let required_subtile =
                match live.terrain().current_tile_dimensions(c.final_tile_index)? {
                    (5 | 4, 3) => Some(2),
                    (3, 4 | 5) => Some(6),
                    _ => None,
                };
            if required_subtile.is_some_and(|sub| c.final_sub_tile != sub) {
                return Ok(7);
            }
        } else if c.land_type != required.as_index()
            && !(matches!(live.sim.cell_overlay(cell).overlay_id, Some(237 | 238))
                && args.height != i32::from(initial_level))
        {
            return Ok(7);
        }
    }
    let layer = {
        use crate::sim::movement::infantry_entry::{adjust_height_and_list, backstep_cell};
        let mut height = args.height;
        let mut list_bridge = target.flags(live) & 0x100 != 0
            && (height == -1 || height.wrapping_sub(i32::from(initial_level)).wrapping_abs() > 1);
        let terrain = live.terrain();
        let cells = live.prelude_reads();
        let tube = terrain.tube_for_native_cell(cell);
        if args.direction == 8 {
            //51BFFD tests distinct endpoints; Unit73F218 tests a nonzero
            //exit, including an automatic Tube whose endpoints are equal.
            //Both return before neighbor/height/list work. Original calls:
            //tools/spatial_oracle/unit_entry_traversal.{py,json,meta.json}.
            return Ok(
                if tube.is_some_and(|tube| {
                    if infantry {
                        tube.entry != tube.exit
                    } else {
                        tube.exit != (0, 0)
                    }
                }) {
                    0
                } else {
                    7
                },
            );
        }
        let cross_tube = |tube: Option<&crate::map::tube_facts::TubeFact>, direction: i32| {
            tube.is_some_and(|tube| {
                let distance = direction.wrapping_sub(tube.direction).wrapping_abs();
                distance > 2 && distance < 6 && args.direction != -1
            })
        };
        if cross_tube(tube, args.direction) {
            return Ok(7);
        }
        let backstep = backstep_cell(&cells, cell, args.direction);
        if cross_tube(
            terrain.tube_for_native_cell(backstep),
            (args.direction - 4) & 7,
        ) {
            return Ok(7);
        }
        //51C0AE reloads the target level after the first neighbor lookup.
        if infantry && height.wrapping_sub(i32::from(target.level(live))) > 4 {
            return Ok(0);
        }
        if !adjust_height_and_list(
            &cells,
            cell,
            args.direction,
            args.previous_cell,
            &mut height,
            &mut list_bridge,
        ) {
            return Ok(7);
        }
        //51C0FB..136 /73F303..348: raw occupation selection is independent
        //of the object list plane modified by4D9C60. Keep both authorities.
        if height != -1
            && target.flags(live) & 0x100 != 0
            && height == i32::from(target.level(live)) + 4
        {
            (bits, owner) = target.deck_raw(live);
            vehicle_occupied = bits & 0x20 != 0;
        }
        if list_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        }
    };
    //Unit73F34C / Infantry51C13A read A8E7AC, independent of GameMode.
    //Unit still performs the read/+320 when3D5 is false; Infantry skips it.
    //578540 consumes the pointer directly, without578460's extra lookup.
    let boundary_refused = !live.sim.object_placement_scope_active()
        && (!infantry || e.in_playfield())
        && !target.in_playfield(live)
        && !live.sim.foot_allows_outside_playfield(id)?;
    if boundary_refused && e.in_playfield() {
        return Ok(7);
    }
    let p = target.coord(live);
    let capability = CrushCapability::new(crusher, obj.omni_crusher);
    let mut entry_result: u8 = 0;
    let mut stationary_infantry = 0u32;
    let mut crush_latch = false;
    let live_overlay = target.overlay(live);
    if let Some(overlay) = live_overlay.overlay_id {
        let flags = live
            .registry
            .and_then(|r| r.flags(overlay))
            .ok_or("repair overlay receiver lacks registered type")?;
        if flags.crate_type
            && (infantry || !live.sim.session.game_mode_nonzero())
            && !live.sim.owner_is_human(e.owner())
        {
            return Ok(7);
        }
        if flags.wall {
            //4F9A10 asks the mover's House: its own index, or its Allies bit
            //for Cell+0x50 (-1 is no ally). A one-way ally of the mover is not.
            let allied = live_overlay
                .wall_owner
                .is_some_and(|o| mover.allied_with(live, o));
            let warhead = weapon0
                .and_then(|w| w.warhead.as_deref())
                .and_then(|name| live.rules.warhead(name));
            if infantry {
                //51C1BB..51C223: a wall at damage stage DamageLevels (+0x2A0)
                //is no wall to Infantry. Otherwise the primary warhead needs
                //Wall= (772AC0); Infantry has no crusher or Wood= route.
                if u16::from(live_overlay.overlay_data >> 4) != flags.damage_levels {
                    if !combat_weapon::is_armed(e, obj) || !warhead.is_some_and(|w| w.wall) {
                        return Ok(7);
                    }
                    entry_result = entry_result.max(if allied { 4 } else { 5 });
                }
            } else if (flags.crushable && crusher)
                || obj.movement_zone == crate::rules::locomotor_type::MovementZone::CrusherAll
            {
                if allied {
                    entry_result = entry_result.max(4);
                }
            } else {
                if !combat_weapon::is_armed(e, obj)
                    || !warhead.is_some_and(|w| w.wall || (w.wood && flags.armor_is_wood))
                {
                    return Ok(7);
                }
                entry_result = entry_result.max(if allied { 4 } else { 5 });
            }
        }
    }
    //47C520 is asked only inside the Building arms; defer the list walk.
    let first_building = || {
        live.sim.substrate.occupancy.first_building_on_layer(
            p.0 as u16,
            p.1 as u16,
            MovementLayer::Ground,
        )
    };
    // Unit 0x0073F530..0x0073F56F, Infantry 0x0051C259..0x0051C298: a
    // parasite owner (`+0x69C`) holding a victim (`+0x28`) is admitted (0)
    // at the first object, after itself, whose map cell (vt+0x1B8,
    // 0x0041BEA0) is the victim's. A released owner unlimbos at its victim.
    let victim_cell = e
        .parasite
        .class()
        .and_then(|state| state.victim())
        .and_then(|victim| live.sim.substrate.entities.get(victim))
        .map(|victim| {
            crate::sim::movement::coord_cell(
                crate::sim::movement::ground_pose::position_world_coord(&victim.position),
            )
        });
    for member in target.objects(live, p, layer) {
        let CellObjectMember::Entity(blocker_id) = member else {
            // A terrain object's map cell is the cell that lists it.
            if victim_cell == Some(p) {
                return Ok(0);
            }
            if !infantry {
                // Unit73FBAC calls the target-sensitive +2E4 selector. Terrain
                // has no Techno bit and is not a Cell (746CD0 ->6F3330).
                let selected = combat_weapon::what_weapon_should_i_use(
                    live.rules,
                    obj,
                    &combat_weapon::attacker_facts(e, obj),
                    Some(&combat_weapon::TargetFacts::Terrain),
                );
                let weapon = combat_weapon::weapon_for_index(obj, e.veterancy.rank(), selected)
                    .and_then(|(name, _)| live.rules.weapon(name));
                if !weapon
                    .and_then(|w| w.warhead.as_deref())
                    .and_then(|name| live.rules.warhead(name))
                    .is_some_and(|w| w.wood)
                {
                    return Ok(7);
                }
                entry_result = entry_result.max(5);
            }
            continue;
        };
        if blocker_id == id {
            if !infantry {
                bits &= !0x20;
                vehicle_occupied = false;
            }
            continue;
        }
        let (b, blocker) = live
            .blocker(blocker_id)
            .ok_or("repair list has retired blocker")?;
        if victim_cell.is_some_and(|cell| cell == crate::sim::movement::coord_cell(b.position)) {
            return Ok(0);
        }
        let bt = mover
            .object_type(live, b.type_ref)
            .ok_or("repair list missing blocker type")?;
        if infantry && e.slave.owner() == Some(blocker_id) {
            let query = crate::sim::slave_deposit::SlaveDepositQuery {
                entities: &live.sim.substrate.entities,
                occupancy: &live.sim.substrate.occupancy,
                terrain: live.terrain(),
                rules: live.rules,
                interner: &live.sim.interner,
            };
            if query.admits(id, blocker_id, cell) {
                //51C2CA clears the local vehicle latch and CONTINUES; earlier
                //soft results and later blockers retain their significance.
                vehicle_occupied = false;
                continue;
            }
        }
        let allied = mover.allied_with(live, b.owner);
        let mission = if infantry {
            e.mission.current().known()
        } else {
            e.mission.effective().known()
        };
        if infantry {
            match infantry_target_admission(live, e, obj, blocker_id, &b, cell)? {
                InfantryTargetAdmission::Ordinary => {}
                InfantryTargetAdmission::SkipCurrent => continue,
                InfantryTargetAdmission::DirectTarget => {
                    if b.invulnerable() {
                        return Ok(7);
                    }
                    return Ok(
                        if layer == MovementLayer::Ground
                            && e.tether.partner().is_none()
                            && !target.row_nonzero(live, obj.speed_type)?
                        {
                            7
                        } else {
                            0
                        },
                    );
                }
            }
        }
        if b.category() == EntityCategory::Structure {
            if !infantry {
                //73F57C..5A2: a contacted Building whose458A00 result is
                //false is skipped before the ordinary Building branches.
                if decide_live_vehicle_building_entry(LiveVehicleBuildingEntry {
                    branch: VehicleBuildingEntryBranch::RadioContact {
                        mover_has_contact: e.has_live_contact_with(blocker_id),
                    },
                    checked_building_id: blocker_id,
                    candidate_building_id: first_building(),
                    candidate_x: p.0 as u16,
                    building_origin_x: b.rx,
                    number_impassable_rows: bt.number_impassable_rows,
                    is_unit_repair: bt.unit_repair,
                    is_bunker: bt.bunker,
                    bunker_occupied: b.bunker_occupied,
                }) == BuildingOccupantEntryDecision::SkipBlocker
                {
                    continue;
                }
                //73F5EF/73F628 have distinct type/mission pairs. +1D4 is
                //the live warp latch.
                if object_target_is(e, blocker_id)
                    && ((mission == Some(MissionType::Enter) && bt.unit_absorb)
                        || (mission == Some(MissionType::Eaten) && bt.grinding))
                    && !b.warped_out
                {
                    return Ok(0);
                }
                //73F661..6CF: Grinding additionally walks CURRENT ground E4,
                //without mission/NavCom/radio/warp gates. Do the actual lookup
                //at this list position, preserving shared-Dummy stamping.
                if bt.grinding {
                    let current =
                        crate::sim::movement::ground_pose::position_world_coord(&e.position);
                    let current_cell = live
                        .terrain()
                        .native_cell_identity(((current.x / 256) as i16, (current.y / 256) as i16));
                    if let Cell::Real(index) = current_cell {
                        let c = &live.terrain().cells()[index];
                        if live
                            .sim
                            .substrate
                            .occupancy
                            .get(c.rx, c.ry)
                            .is_some_and(|list| {
                                list.iter_layer(MovementLayer::Ground)
                                    .any(|entry| entry.entity_id == blocker_id)
                            })
                        {
                            return Ok(0);
                        }
                    }
                }
            }
            if infantry && bt.invisible_in_game {
                continue;
            }
            if bt.gate {
                if b.open_gate {
                    continue;
                }
                let allied = mover.is_ally_of(live, b.owner);
                if !allied && !combat_weapon::is_armed(e, obj) {
                    return Ok(7);
                }
                entry_result = entry_result.max(if allied { 3 } else { 5 });
                continue;
            }
            if !infantry {
                if (bt.unit_repair || bt.bunker)
                    && first_building() == Some(blocker_id)
                    && decide_live_vehicle_building_entry(LiveVehicleBuildingEntry {
                        branch: VehicleBuildingEntryBranch::UnitRepairOrBunker,
                        checked_building_id: blocker_id,
                        candidate_building_id: first_building(),
                        candidate_x: p.0 as u16,
                        building_origin_x: b.rx,
                        number_impassable_rows: bt.number_impassable_rows,
                        is_unit_repair: bt.unit_repair,
                        is_bunker: bt.bunker,
                        bunker_occupied: b.bunker_occupied,
                    }) == BuildingOccupantEntryDecision::SkipBlocker
                {
                    continue;
                }
                if bt.invisible_in_game {
                    continue;
                }
                if bt.bib
                    && live.sim.substrate.occupancy.first_building_on_layer(
                        (p.0 as u16).wrapping_add(1),
                        p.1 as u16,
                        MovementLayer::Ground,
                    ) != Some(blocker_id)
                {
                    continue;
                }
            }
            if allied {
                return Ok(7);
            }
        }
        //73F823..844: this exact Unit target is admitted immediately. Other
        //transport capacity/radio requirements belong to the order/PerCell
        //producers; there is no extra gate in this admission arm.
        if !infantry
            && mission == Some(MissionType::Enter)
            && b.category() == EntityCategory::Unit
            && object_target_is(e, blocker_id)
        {
            return Ok(0);
        }
        //Infantry51C579..58D's occupant-list alliance override is local to
        //this gate. Earlier Building/wall checks and the later raw-owner
        //query use actual House alliances, including during crew escape.
        let allied = allied || (infantry && live.sim.object_placement_scope_active());
        if !allied {
            if b.fully_cloaked {
                entry_result = entry_result.max(1);
                continue;
            }
            if !infantry
                && crusher
                && bump_crush::can_crush(capability, b.crush, |owner| {
                    mover.allied_with(live, owner)
                })
            {
                crush_latch = true;
                continue;
            }
            //73FB71..73FB90: an unarmed Unit refuses here unless it is
            //IsTrain= (+0xC94); a train goes on to the object's kind.
            if if infantry {
                mover.weapon_damage(live) <= 0
            } else {
                weapon0.is_none() && !obj.is_train
            } {
                return Ok(7);
            }
            if b.category() == EntityCategory::Structure && bt.bridge_repair_hut {
                return Ok(7);
            }
            if infantry {
                if b.category() == EntityCategory::Infantry {
                    if b.disguised && infantry_disguised_to(live, blocker, e.owner())? {
                        //51C61F is an assignment, not a boolean soft latch.
                        entry_result = 6;
                    }
                } else {
                    entry_result = entry_result.max(5);
                }
            } else {
                entry_result = entry_result.max(5);
            }
        } else if !infantry {
            //73F865..8C0: Foot NavCom, body turn, then active IsMoving.
            if b.nav_com || b.rotating || b.moving == Some(true) {
                if head_on(e, &b, live.sim.session.binary_frame()) {
                    return Ok(7);
                }
                if (b.foot_occupation && b.category() != EntityCategory::Infantry)
                    || b.occupant_slot_a4
                {
                    entry_result = entry_result.max(2);
                }
            } else {
                entry_result = entry_result.max(6);
            }
        } else {
            match b.category() {
                EntityCategory::Aircraft | EntityCategory::Structure => return Ok(7),
                EntityCategory::Unit => {
                    if b.moving != Some(true) && !b.nav_com {
                        entry_result = entry_result.max(6);
                    } else if b.foot_occupation || b.occupant_slot_a4 {
                        entry_result = entry_result.max(2);
                    }
                }
                EntityCategory::Infantry => {
                    if b.moving != Some(true) {
                        stationary_infantry = stationary_infantry.wrapping_add(1);
                    }
                }
            }
        }
    }
    if layer == MovementLayer::Ground
        && (!infantry || e.tether.partner().is_none())
        && !target.row_nonzero(live, obj.speed_type)?
    {
        return Ok(7);
    }
    if infantry {
        if entry_result == 0 && vehicle_occupied {
            //51C7DF..7F7 returns2 before owner/full-subcell tests.
            return Ok(2);
        }
        if let Some(owner) = owner {
            if mover.allied_with(live, owner) {
                if bits & 0x1c == 0x1c && entry_result < 2 {
                    entry_result = if stationary_infantry == 3 { 6 } else { 2 };
                }
            } else {
                if mover.weapon_damage(live) <= 0 {
                    return Ok(7);
                }
                entry_result = entry_result.max(5);
            }
        }
        Ok(if entry_result == 0 && bits & 0x1c == 0x1c {
            7
        } else {
            entry_result
        })
    } else {
        Ok(unit_post_walk_tail(
            live,
            mover,
            p,
            UnitWalk {
                running_code: entry_result,
                crush_latch,
                bits,
                vehicle_occupied,
                owner,
            },
        ))
    }
}

/// Unit-only building entry branch that may reach the row helper. Infantry's
/// Can_Enter_Cell has neither the radio/contact nor the UnitRepair/Bunker
/// NumberImpassableRows branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VehicleBuildingEntryBranch {
    /// Contact-vector branch. The caller must supply whether this mover has
    /// RadioClass contact with the checked building.
    RadioContact { mover_has_contact: bool },
    /// UnitRepair/Bunker branch. This branch is gated by the checked building's
    /// type flags, not by RadioClass contact.
    UnitRepairOrBunker,
}

/// Decision for a checked building occupant in UnitClass-style cell entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildingOccupantEntryDecision {
    /// Keep the checked building in the ordinary blocker classification path.
    KeepBlocker,
    /// Skip this building occupant and continue scanning later occupants in the
    /// cell's object list.
    SkipBlocker,
}

/// Explicit live facts needed by the UnitClass building row-helper decision.
///
/// Caller responsibilities:
/// - `candidate_building_id` must be the result of a live
///   Look_up_building_in_cell-style lookup for the candidate cell.
/// - `checked_building_id` and type/runtime flags must describe the building
///   occupant currently being inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveVehicleBuildingEntry {
    branch: VehicleBuildingEntryBranch,
    checked_building_id: u64,
    candidate_building_id: Option<u64>,
    candidate_x: u16,
    building_origin_x: u16,
    number_impassable_rows: i32,
    is_unit_repair: bool,
    is_bunker: bool,
    bunker_occupied: bool,
}

/// Decide whether a Unit's Can_Enter_Cell skips a building occupant.
///
/// This models `FUN_00458A00` at its two UnitClass::Can_Enter_Cell callsites:
/// radio/contact and UnitRepair/Bunker. A `KeepBlocker` result means the caller
/// should continue with the existing Can_Enter_Cell return-code classification;
/// `SkipBlocker` means only this building occupant is ignored.
fn decide_live_vehicle_building_entry(
    input: LiveVehicleBuildingEntry,
) -> BuildingOccupantEntryDecision {
    let branch_active = match input.branch {
        VehicleBuildingEntryBranch::RadioContact { mover_has_contact } => mover_has_contact,
        VehicleBuildingEntryBranch::UnitRepairOrBunker => input.is_unit_repair || input.is_bunker,
    };
    if !branch_active {
        return BuildingOccupantEntryDecision::KeepBlocker;
    }

    if input.candidate_building_id != Some(input.checked_building_id) {
        //458A00 returns false on identity mismatch. The radio caller73F5A2
        //skips then; UnitRepair/Bunker73F761 has a separate equality gate.
        return match input.branch {
            VehicleBuildingEntryBranch::RadioContact { .. } => {
                BuildingOccupantEntryDecision::SkipBlocker
            }
            VehicleBuildingEntryBranch::UnitRepairOrBunker => {
                BuildingOccupantEntryDecision::KeepBlocker
            }
        };
    }
    if input.number_impassable_rows == -1 {
        return BuildingOccupantEntryDecision::KeepBlocker;
    }
    if input.is_bunker && input.bunker_occupied {
        return BuildingOccupantEntryDecision::KeepBlocker;
    }

    let first_clear_x = i32::from(input.building_origin_x) + input.number_impassable_rows;
    if i32::from(input.candidate_x) >= first_clear_x {
        BuildingOccupantEntryDecision::SkipBlocker
    } else {
        BuildingOccupantEntryDecision::KeepBlocker
    }
}

/// What `UnitClass::Can_Enter_Cell`'s object walk leaves for its
/// continuation at `0x0073FC24`.
struct UnitWalk {
    /// The running code (EBP).
    running_code: u8,
    /// A crushable enemy met in the walk (`[ESP+0x17]`).
    crush_latch: bool,
    /// The selected plane's raw occupation byte (`[ESP+0x14]`), its vehicle
    /// latch (`[ESP+0x15]`) and raw owner (`[ESP+0x1C]`, -1 as `None`).
    bits: u8,
    vehicle_occupied: bool,
    owner: Option<InternedId>,
}

/// `UnitClass::Can_Enter_Cell` after the object walk and land row
/// (`0x0073FC24..0x0073FD43`), at the cell `p`:
/// - A nonzero running code wins.
/// - After a crush latch (`0x0073FCF6`), a clear vehicle latch admits.
///   Otherwise the first GROUND Unit decides, even when the walk read the
///   deck (`GetUnit(false) @ 0x0047EBA0`), through `IsCrushableBy`
///   (`0x005F6CD0`, called at `0x0073FD17`): 0 when it is crushable, 2 when
///   it is not or there is none.
/// - Without one, a raw byte with nothing in `0x3F` admits. The vehicle latch
///   or an allied raw owner (`0x004F9A10`) answers 2, a crusher 0, and
///   `IsTrain=` 5 (`0x0073FC91`). Otherwise weapon slot 0 decides: 5 for an
///   `AG=` projectile, else 7, also with no weapon.
///
/// Native controls: `tools/spatial_oracle/cell_entry_crush_tail` enters at
/// `0x0073FC24` with a crush latch.
fn unit_post_walk_tail(
    live: &EntryReadContext<'_>,
    mover: &EntryMover<'_>,
    p: (i16, i16),
    walk: UnitWalk,
) -> u8 {
    if walk.running_code != 0 {
        return walk.running_code;
    }
    if walk.crush_latch {
        let first_unit_is_crushable = || {
            live.sim
                .substrate
                .occupancy
                .first_category_on_layer(
                    p.0 as u16,
                    p.1 as u16,
                    MovementLayer::Ground,
                    EntityCategory::Unit,
                    &live.sim.substrate.entities,
                )
                .and_then(|id| live.blocker(id))
                .is_some_and(|(unit, _)| {
                    // The caller's latch stands for the Crusher test.
                    bump_crush::object_is_crushable_by(
                        mover.obj.omni_crusher,
                        unit.crush,
                        |owner| mover.allied_with(live, owner),
                    )
                })
        };
        return if !walk.vehicle_occupied || first_unit_is_crushable() {
            0
        } else {
            2
        };
    }
    if walk.bits & 0x3f == 0 {
        return 0;
    }
    if walk.vehicle_occupied
        || walk
            .owner
            .is_some_and(|owner| mover.allied_with(live, owner))
    {
        return 2;
    }
    //73FC6C..73FC8B: Crusher= (+0xD28) or the veteran Crusher ability.
    if mover.crusher {
        return 0;
    }
    if mover.obj.is_train {
        return 5;
    }
    if mover
        .weapon0
        .and_then(|w| w.projectile.as_deref())
        .and_then(|name| live.rules.projectile(name))
        .is_some_and(|p| p.ag)
    {
        5
    } else {
        7
    }
}

/// `AircraftClass::Can_Enter_Cell` (`0x004196B0`) as an object's own +1AC
/// inside the repair/damage occupant pass. The answer is the one owner's,
/// [`Simulation::aircraft_can_enter_cell`]; the pass itself ignores it (an
/// Aircraft's kind 2 always takes the first pass's damage at `0x00487A3B`,
/// and the neighbour pass admits only Foot objects), so only the read's
/// effects matter here. Canonical lookups reproduce the shared-dummy stamps.
/// The remaining unproved locomotor precondition refuses with an error:
/// - Fly, Jumpjet, Rocket and Teleport have the constant-false `+A0`
///   (`0x004B6630`) the neighbour pass relies on; other families are
///   unproved. See LIVE_REPAIR_AIRCRAFT_RECEIVER.md and
///   locomotor_at_coord.json.
///
/// The Team arm (`0x004196BC..0x004196DE`) uses the shared Team6EC300
/// waypoint owner before the other Aircraft admission gates.
fn aircraft_entry(
    live: &EntryReadContext<'_>,
    entity: &GameEntity,
    cell: Cell,
) -> Result<u8, String> {
    if entity.locomotor.as_ref().is_none_or(|l| {
        !matches!(
            l.active_kind(),
            LocomotorKind::Fly
                | LocomotorKind::Jumpjet
                | LocomotorKind::Rocket
                | LocomotorKind::Teleport
        )
    }) {
        return Err("repair Aircraft requires a proved constant-false AtCoord family".into());
    }
    Ok(live.sim.aircraft_can_enter_cell(entity.stable_id(), cell))
}

fn terrain_impassable(
    live: &EntryReadContext<'_>,
    object: CellObjectMember,
    cell: Cell,
) -> Result<bool, String> {
    let CellObjectMember::Terrain(id) = object else {
        unreachable!()
    };
    let terrain = live
        .sim
        .terrain_objects
        .objects()
        .get(&id)
        .ok_or("repair retired Terrain")?;
    let ty = live
        .rules
        .terrain_object_type_case_insensitive(live.sim.interner.resolve(terrain.type_ref()))
        .ok_or("repair missing TerrainType")?;
    // TerrainClass::Can_Enter_Cell @ 0x0071C4D0: the first foundation cell
    // that is not clear for a typeless, houseless build refuses (7).
    let p = live.coord(cell);
    let speed = if ty.water_bound {
        SpeedType::Float
    } else {
        SpeedType::Track
    };
    for offset in crate::rules::foundation::foundation_cell_offsets(&ty.foundation) {
        let selected = live
            .terrain()
            .native_cell_identity((p.0.wrapping_add(offset.0), p.1.wrapping_add(offset.1)));
        if !crate::sim::build_site::is_clear_to_build(
            live.sim,
            live.rules,
            live.registry,
            selected,
            Some(speed),
            None,
            None,
            None,
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}
/// BuildingClass::Can_Enter_Cell @ 0x00449440: a marked building that can
/// undeploy tests the one cell, otherwise its type's CanPlaceAt from the
/// cell; either for the building's own house. `cells` is the query that
/// resolved `cell`; `None` is the simulation's own.
fn building_impassable(
    live: &EntryReadContext<'_>,
    e: &GameEntity,
    obj: &ObjectType,
    cell: Cell,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> Result<bool, String> {
    let admitted = if obj.undeploys_into.is_some() && e.lifecycle.cell_marked() {
        crate::sim::build_site::is_clear_to_build(
            live.sim,
            live.rules,
            live.registry,
            cell,
            crate::sim::build_site::building_speed_type(obj),
            Some(obj),
            Some(e.owner()),
            cells,
        )
    } else {
        let origin = cells.map_or_else(|| live.coord(cell), |cells| cells.coord(cell));
        crate::sim::build_site::can_place_building_at(
            live.sim,
            live.rules,
            live.registry,
            obj,
            origin,
            Some(e.owner()),
            cells,
        )
    };
    Ok(!admitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impassable(
        live: &EntryReadContext<'_>,
        object: CellObjectMember,
        cell: Cell,
    ) -> Result<bool, String> {
        classify_entry(
            live,
            object,
            cell,
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
        )
        .map(|code| code == 7)
    }

    #[test]
    fn aircraft_entry_answers_as_the_owner_after_the_team_arm() {
        for kind in [
            LocomotorKind::Fly,
            LocomotorKind::Jumpjet,
            LocomotorKind::Rocket,
            LocomotorKind::Teleport,
        ] {
            let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture();
            let id = sim
                .spawn_object("HORNET", "Americans", 17, 15, 0, &rules)
                .unwrap();
            let mut entity = sim.substrate.entities.get(id).unwrap().clone();
            let mut object = rules.object("HORNET").unwrap().clone();
            object.locomotor = kind;
            entity.locomotor =
                Some(crate::sim::movement::locomotor::LocomotorState::from_object_type(&object, 0));
            // The answer is the one Aircraft owner's; canonical lookups
            // reproduce the shared-dummy stamps, so a dummy cell is readable.
            let probe = |sim: &mut Simulation| {
                aircraft_entry(
                    &EntryReadContext {
                        sim,
                        rules: &rules,
                        registry: None,
                        facts: None,
                    },
                    &entity,
                    Cell::Dummy,
                )
            };
            sim.session.set_game_mode(
                crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
            );
            assert_eq!(
                probe(&mut sim),
                Ok(sim.aircraft_can_enter_cell(id, Cell::Dummy))
            );
            sim.session.set_game_mode(
                crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
            );
            assert_eq!(probe(&mut sim), Ok(0));
            let script_id = sim.intern("REPAIR_WAYPOINT_EFFECT");
            sim.team_script_vm
                .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
                    id: script_id,
                    actions: vec![crate::sim::team_script_vm::TeamScriptAction {
                        action_id: 3,
                        argument: 0,
                    }],
                    source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                });
            sim.team_script_vm.create_team(
                entity.owner(),
                script_id,
                vec![id],
                sim.session.binary_frame() as i32,
            );
            assert!(sim.member_team_is_moving_outside_playfield(id));
            assert_eq!(
                probe(&mut sim),
                Ok(0),
                "the shared Team waypoint receiver runs before the mode gate"
            );
        }
    }

    #[test]
    fn usable_area_admission_ignores_game_mode_and_nested_escape_restores_it() {
        let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture();
        let mut mover = GameEntity::test_default(90, "ENGINEER", "Americans", 15, 15);
        mover.set_owner_for_test(sim.intern("Americans"));
        mover.set_type_ref_for_test(sim.intern("ENGINEER"));
        mover.set_category_for_test(EntityCategory::Infantry);
        mover.set_in_playfield(true);
        sim.substrate.entities.insert(mover);
        sim.playfield_bounds = Some(
            crate::map::playfield::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 1, 1),
        );
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 15));
        let probe = |sim: &Simulation| {
            sim.foot_entry_receiver(90, &rules, None, None)
                .unwrap()
                .can_enter(
                    cell,
                    crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
                )
        };
        let rng_before = [
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        ];
        // Original51BF90 has no A8B238 read. Executed counter0/mode0/1
        // controls both return7 for this otherwise-clear unusable rim.
        let mode = sim.session.game_mode();
        for nonzero in [false, true] {
            sim.session.set_game_mode(
                crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(nonzero),
            );
            assert_eq!(
                probe(&sim),
                Ok(7),
                "mode {nonzero} cannot bypass Infantry51C13A..51C161"
            );
        }
        sim.session.set_game_mode(mode);
        sim.with_object_placement_scope(|sim| {
            assert_eq!(probe(sim), Ok(0));
            sim.with_object_placement_scope(|sim| assert_eq!(probe(sim), Ok(0)));
            assert_eq!(probe(sim), Ok(0), "nested return retains the outer bracket");
        });
        assert_eq!(
            probe(&sim),
            Ok(7),
            "outer return restores ordinary admission"
        );
        assert_eq!(
            [
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            ],
            rng_before,
        );
    }

    #[test]
    fn infantry_target_lookup_obeys_gates_and_retains_cell_identity() {
        let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
        let hut = sim
            .spawn_object("CABHUT", "Americans", 16, 15, 0, &rules)
            .unwrap();
        let infantry = sim
            .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules)
            .unwrap();
        // Supplied query coordinates isolate the effectful51C3B1 boundary.
        sim.substrate
            .entities
            .get_mut(hut)
            .unwrap()
            .position
            .set_rx_for_test(60);
        sim.substrate
            .entities
            .get_mut(hut)
            .unwrap()
            .position
            .set_ry_for_test(60);
        let probe = |sim: &mut Simulation| {
            let live = EntryReadContext {
                sim,
                rules: &rules,
                registry: Some(&registry),
                facts: None,
            };
            infantry_target_admission(
                &live,
                live.sim.substrate.entities.get(infantry).unwrap(),
                rules.object("ENGINEER").unwrap(),
                hut,
                &live.blocker(hut).unwrap().0,
                live.terrain().native_cell_identity((16, 15)),
            )
            .unwrap()
                == InfantryTargetAdmission::DirectTarget
        };
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .stamp_dummy_cell_requested_coord(7, 8);
        sim.mission_assign_exact(
            infantry,
            crate::sim::mission::MissionId::from_known(MissionType::Move),
            sim.session.binary_frame(),
        )
        .unwrap();
        assert!(!probe(&mut sim));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (7, 8)
        );
        sim.mission_assign_exact(
            infantry,
            crate::sim::mission::MissionId::from_known(MissionType::Capture),
            sim.session.binary_frame(),
        )
        .unwrap();
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::Object { id: hut }));
        assert!(probe(&mut sim));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (7, 8),
            "exact object match precedes map lookup"
        );
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::Cell { rx: 61, ry: 61 }));
        assert!(
            probe(&mut sim),
            "two absent coordinates denote the same retained native Dummy Cell"
        );
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (60, 60)
        );
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .navigation
            .set_nav_com_for_test(None);
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .attack_target
            .set_for_test(Some(crate::sim::combat::TargetKind::Entity(hut)));
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .stamp_dummy_cell_requested_coord(7, 8);
        assert!(probe(&mut sim));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (60, 60),
            "attack target is checked after the map lookup"
        );
    }

    #[test]
    fn slave_deposit_skip_keeps_later_building_refusal_and_raw_history() {
        let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture();
        let hut = sim
            .spawn_object("CABHUT", "Americans", 16, 15, 0, &rules)
            .unwrap();
        let slave = sim
            .spawn_object("ENGINEER", "Americans", 15, 15, 0, &rules)
            .unwrap();
        sim.substrate.entities.get_mut(slave).unwrap().slave =
            crate::sim::slave_manager::SlaveLink::for_test(Some(hut), Vec::new());
        let slav = sim.intern("SLAV");
        let manager = |slaves: &[u64]| {
            Some(crate::sim::slave_manager::SlaveManager::new(
                slav,
                slaves.iter().copied().map(Some),
                0,
                0,
                0,
            ))
        };
        sim.substrate.entities.get_mut(hut).unwrap().slave_manager = manager(&[slave]);
        sim.substrate.raw_cell_occupation.mark_ground(16, 15, 0x20);
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((16, 15));
        let probe = |sim: &mut Simulation| {
            let live = EntryReadContext {
                sim,
                rules: &rules,
                registry: Some(&registry),
                facts: None,
            };
            impassable(&live, CellObjectMember::Entity(slave), cell).unwrap()
        };
        assert!(
            !probe(&mut sim),
            "master membership admits this deposit cell"
        );
        assert_ne!(
            sim.substrate.raw_cell_occupation.ground_bits(16, 15) & 0x20,
            0,
            "receiver clears only a local latch"
        );
        sim.substrate.entities.get_mut(hut).unwrap().slave_manager = manager(&[]);
        assert!(probe(&mut sim), "empty manager cannot skip the blocker");
        sim.substrate.entities.get_mut(hut).unwrap().slave_manager = manager(&[slave]);
        // Supplied overlapping Building list tests native continuation, not
        // ordinary construction legality or a retail repair footprint scene.
        let mut later = GameEntity::test_default_of_category(
            100,
            "CABHUT",
            "Americans",
            16,
            15,
            EntityCategory::Structure,
        );
        later.set_owner_for_test(sim.intern("Americans"));
        later.set_type_ref_for_test(sim.intern("CABHUT"));
        sim.substrate.entities.insert(later);
        sim.substrate.occupancy.add(
            16,
            15,
            100,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        assert!(
            probe(&mut sim),
            "true6B0880 continues into the later hard blocker"
        );
    }
}
