//! FootClass-style navigation destination helpers.
//!
//! These helpers model the owner `NavCom` lifecycle (Foot+0x5A4) and the
//! locomotor Move_To and Stop_Moving calls the Foot setter makes.

use super::track_process::TrackFamily;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::FrameEffects;
use crate::util::fixed_math::SIM_ZERO;
#[cfg(test)]
use crate::util::fixed_math::SimFixed;

/// FootClass-style owner navigation fields, owned by this module: NavCom
/// (`Foot+0x5A4`), its auxiliary slot (`Foot+0x5A0`), SuspendedNavCom
/// (`Foot+0x5A8`), the NavQueue (`Foot+0x588`, count `+0x598`) and the
/// navigation list (`Foot+0x5AC`, count `+0x5BC`) with its loop flag
/// (`Foot+0x6B1`). Their writers are the native operations below: the
/// destination setter's publication, Stop_Moving, a locomotor's Move_To, the
/// mission override's save, the list's queue and send
/// (`super::nav_list`), and the clears of PointerExpired, the MEGAMISSION
/// and IDLE events and Remove_Member.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NavigationState {
    /// Foot+55C is retained lifecycle history, not current occupancy.
    #[serde(default)]
    neighbor_state: crate::sim::cell_neighbors::FootNeighborState,
    /// Foot timers, retry count and blockage latch survive path retirement.
    #[serde(default)]
    path_runtime: crate::sim::components::FootPathRuntime,
    /// Foot-owned direction replay and reference cell (+5E0/+558), shared by
    /// every locomotor instance. Retirement must not discard the owner queue.
    /// Native chain tails reload the owner at Drive4B1DF7 / Ship6A143A.
    #[serde(default)]
    path_replay: crate::sim::components::FootPathQueue,
    /// `Foot+0x5A0`, NavCom's auxiliary slot, cleared whenever NavCom is
    /// published or stopped.
    #[serde(default)]
    nav_com_aux: Option<NavTargetRef>,
    /// `Foot+0x5A4`, NavCom.
    #[serde(default)]
    nav_com: Option<NavTargetRef>,
    /// `Foot+0x5A8`, SuspendedNavCom.
    #[serde(default)]
    suspended_nav_com: Option<NavTargetRef>,
    /// `Foot+0x588` (count `+0x598`), the NavQueue, oldest first.
    #[serde(default)]
    nav_queue: Vec<NavTargetRef>,
    /// `Foot+0x5AC` (count `+0x5BC`), the navigation list, oldest first: the
    /// posts Queue_Navigation_List (`0x004DA0E0`) appends, which
    /// Handle_Navigation_List (`0x004DA030`) and Mission_Area_Guard's head
    /// (`0x004D6B23..0x004D6B8D`) send the Foot to one at a time. The shared
    /// Ghidra database calls this field `NavQueue`.
    #[serde(default)]
    nav_list: Vec<NavTargetRef>,
    /// `Foot+0x6B1`: a sent list head is appended again, so the list loops.
    #[serde(default)]
    nav_list_loops: bool,
    /// A class-setter call (vt+0x480) that Rust deferred and still owes: a
    /// mission restore's `Assign_Destination(saved, 1)` (`0x004D8F99`) made
    /// where only NavCom could be written. `Simulation::complete_pending_order`
    /// makes it at the object's next Process entry.
    #[serde(default)]
    pending_destination_setter: bool,
    /// FootClass+530, constructor positive-zero. FootUnlimbo4D72EA..4D72F4
    /// copies TechnoType+2F0 only after successful Techno placement, neighbor
    /// publication and the optional AirTracker tail. It outlives routes and
    /// locomotor instances; native getter4DC760 reads this retained double.
    #[serde(default = "default_threat_avoidance_coefficient")]
    threat_avoidance_coefficient: crate::util::native_x87::NativeF64Bits,
}

const fn default_threat_avoidance_coefficient() -> crate::util::native_x87::NativeF64Bits {
    crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO
}

impl Default for NavigationState {
    fn default() -> Self {
        Self::at_frame(0)
    }
}

impl NavigationState {
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_neighbor_state_for_test(
        &mut self,
        value: crate::sim::cell_neighbors::FootNeighborState,
    ) {
        self.neighbor_state = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_path_replay_for_test(&mut self, value: crate::sim::components::FootPathQueue) {
        self.path_replay = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_path_runtime_for_test(&mut self, value: crate::sim::components::FootPathRuntime) {
        self.path_runtime = value;
    }

    /// Foot+55C neighbor history.
    pub(crate) fn neighbor_state(&self) -> &crate::sim::cell_neighbors::FootNeighborState {
        &self.neighbor_state
    }

    pub(crate) fn neighbor_state_mut(
        &mut self,
    ) -> &mut crate::sim::cell_neighbors::FootNeighborState {
        &mut self.neighbor_state
    }

    /// The Foot path timers, retry count and blockage latch.
    pub fn path_runtime(&self) -> &crate::sim::components::FootPathRuntime {
        &self.path_runtime
    }

    pub(crate) fn path_runtime_mut(&mut self) -> &mut crate::sim::components::FootPathRuntime {
        &mut self.path_runtime
    }

    /// The Foot-owned direction replay and reference cell (+5E0/+558).
    pub fn path_replay(&self) -> &crate::sim::components::FootPathQueue {
        &self.path_replay
    }

    pub(crate) fn path_replay_mut(&mut self) -> &mut crate::sim::components::FootPathQueue {
        &mut self.path_replay
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn path_runtime_mut_for_test(&mut self) -> &mut crate::sim::components::FootPathRuntime {
        &mut self.path_runtime
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn path_replay_mut_for_test(&mut self) -> &mut crate::sim::components::FootPathQueue {
        &mut self.path_replay
    }

    /// Foot constructor-owned navigation state, including its frame anchors.
    pub(crate) fn at_frame(frame: u32) -> Self {
        Self {
            neighbor_state: Default::default(),
            path_runtime: crate::sim::components::FootPathRuntime::at_frame(frame),
            path_replay: Default::default(),
            nav_com_aux: None,
            nav_com: None,
            suspended_nav_com: None,
            nav_queue: Vec::new(),
            nav_list: Vec::new(),
            nav_list_loops: false,
            pending_destination_setter: false,
            threat_avoidance_coefficient: default_threat_avoidance_coefficient(),
        }
    }

    /// `Foot+0x5A4`, NavCom.
    pub fn nav_com(&self) -> Option<NavTargetRef> {
        self.nav_com
    }

    /// `Foot+0x5A0`, NavCom's auxiliary slot.
    pub(crate) fn nav_com_aux(&self) -> Option<NavTargetRef> {
        self.nav_com_aux
    }

    /// `Foot+0x5A8`, SuspendedNavCom.
    pub fn suspended_nav_com(&self) -> Option<NavTargetRef> {
        self.suspended_nav_com
    }

    /// `FootClass::Set_Destination`'s publication: NavComAux cleared
    /// (`0x004D94C7`), NavCom stored (`0x004D9510`, `0x004D96BC`), and an owed
    /// class-setter call ([`Self::pending_destination_setter`]) settled by the
    /// setter it stood for.
    pub(crate) fn publish_nav_com(&mut self, nav_com: Option<NavTargetRef>) {
        self.nav_com_aux = None;
        self.nav_com = nav_com;
        self.pending_destination_setter = false;
    }

    /// `FootClass::Stop_Moving @ 0x004DF0D0`: NavComAux (`0x004DF0D2`) and
    /// NavCom (`0x004DF0D8`) cleared.
    pub(crate) fn stop_moving(&mut self) {
        self.nav_com_aux = None;
        self.nav_com = None;
    }

    /// `FootClass::PointerExpired`'s NavCom arm: NavComAux (`0x004D9ABD`) and
    /// NavCom (`0x004D9AC3`) cleared when NavCom is the expiring object.
    pub(crate) fn expire_nav_com(&mut self) {
        self.nav_com_aux = None;
        self.nav_com = None;
    }

    /// The destination prologue's NavComAux clear (vt+0x47C, `0x004D94A4`),
    /// made before its refusal tests.
    pub(crate) fn clear_nav_com_aux(&mut self) {
        self.nav_com_aux = None;
    }

    /// FootClass::Set_NavComAux 0x004D94A0 stores the pointer at +0x5A0.
    /// EventClass::Execute calls it after Set_Destination (0x004C74BA).
    pub(crate) fn set_nav_com_aux(&mut self, target: Option<NavTargetRef>) {
        self.nav_com_aux = target;
    }

    /// NavCom stored alone, without the setter's NavComAux clear: the null
    /// setter's re-clear after the locomotor Stop (`0x004D96BC`), a
    /// locomotor's Move_To (Jumpjet's reached cell `0x0054B44B`, Teleport's
    /// NULL when it refuses `0x00718213`) and UnitClass::PerCellProcess's dock
    /// restore (`0x0073A4E9`).
    pub(crate) fn store_nav_com(&mut self, nav_com: Option<NavTargetRef>) {
        self.nav_com = nav_com;
    }

    /// `FootClass::Override_Mission`'s save of NavCom into SuspendedNavCom
    /// (`0x004D8F5B`).
    pub(crate) fn suspend_nav_com(&mut self) {
        self.suspended_nav_com = self.nav_com;
    }

    /// SuspendedNavCom cleared: the MEGAMISSION event (`0x004C73C7`),
    /// `TeamClass::Remove_Member` (`0x006EA9AD`) and PointerExpired
    /// (`0x004D9A09`).
    pub(crate) fn clear_suspended_nav_com(&mut self) {
        self.suspended_nav_com = None;
    }

    /// A fixture's NavCom.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_nav_com_for_test(&mut self, nav_com: Option<NavTargetRef>) {
        self.nav_com = nav_com;
    }

    /// A fixture's NavComAux.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_nav_com_aux_for_test(&mut self, nav_com_aux: Option<NavTargetRef>) {
        self.nav_com_aux = nav_com_aux;
    }

    /// A fixture's SuspendedNavCom.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_suspended_nav_com_for_test(&mut self, suspended_nav_com: Option<NavTargetRef>) {
        self.suspended_nav_com = suspended_nav_com;
    }

    /// `Foot+0x588`, the NavQueue, oldest first.
    pub fn nav_queue(&self) -> &[NavTargetRef] {
        &self.nav_queue
    }

    /// A fixture's NavQueue.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_nav_queue_for_test(&mut self, nav_queue: Vec<NavTargetRef>) {
        self.nav_queue = nav_queue;
    }

    /// Empty the NavQueue.
    pub(crate) fn clear_nav_queue(&mut self) {
        self.nav_queue.clear();
    }

    /// Take the NavQueue's oldest entry.
    pub(crate) fn pop_nav_queue_front(&mut self) -> Option<NavTargetRef> {
        (!self.nav_queue.is_empty()).then(|| self.nav_queue.remove(0))
    }

    /// Append to a fixture's NavQueue.
    #[cfg(any(test, feature = "test-support"))]
    pub fn push_nav_queue_for_test(&mut self, target: NavTargetRef) {
        self.nav_queue.push(target);
    }

    /// Keep only the NavQueue entries `keep` accepts.
    pub(crate) fn retain_nav_queue(&mut self, keep: impl FnMut(&NavTargetRef) -> bool) {
        self.nav_queue.retain(keep);
    }

    /// `Foot+0x5AC`, the navigation list, oldest first.
    pub(crate) fn nav_list(&self) -> &[NavTargetRef] {
        &self.nav_list
    }

    /// `Foot+0x6B1`, the list's loop flag.
    pub(crate) fn nav_list_loops(&self) -> bool {
        self.nav_list_loops
    }

    /// Queue_Navigation_List's list writes (`0x004DA0F1..0x004DA157`): the
    /// Foot naming itself on a list that has entries makes it loop instead
    /// of being appended; a target appended to an empty list first clears
    /// the loop.
    pub(crate) fn queue_nav_list(&mut self, target: NavTargetRef, names_self: bool) {
        if names_self && !self.nav_list.is_empty() {
            self.nav_list_loops = true;
            return;
        }
        if self.nav_list.is_empty() {
            self.nav_list_loops = false;
        }
        self.nav_list.push(target);
    }

    /// After the head is sent: the entry at index 0 is removed (`0x0045ADD0`,
    /// inlined at `0x004DA068..0x004DA094`), whatever the send left there,
    /// and a looping list appends `head` again (`0x004A7FE0`, inlined at
    /// `0x004DA096..0x004DA0D8`).
    pub(crate) fn rotate_nav_list(&mut self, head: NavTargetRef) {
        if !self.nav_list.is_empty() {
            self.nav_list.remove(0);
        }
        if self.nav_list_loops {
            self.nav_list.push(head);
        }
    }

    /// The list's DynamicVectorClass Clear (vt+0xC): `0x004DA1C0` and its
    /// inlined copies. The loop flag stays.
    pub(crate) fn clear_nav_list(&mut self) {
        self.nav_list.clear();
    }

    /// Keep only the list entries `keep` accepts.
    pub(crate) fn retain_nav_list(&mut self, keep: impl FnMut(&NavTargetRef) -> bool) {
        self.nav_list.retain(keep);
    }

    /// A fixture's navigation list and loop flag.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_nav_list_for_test(&mut self, nav_list: Vec<NavTargetRef>, loops: bool) {
        self.nav_list = nav_list;
        self.nav_list_loops = loops;
    }

    /// A deferred class-setter call is owed; see the field.
    pub(crate) fn pending_destination_setter(&self) -> bool {
        self.pending_destination_setter
    }

    /// A mission restore owes the class setter's call with the restored
    /// NavCom (`FootClass::Restore_Mission`'s vt+0x480 at `0x004D8F99`), which
    /// Rust makes at the object's next Process entry.
    pub(crate) fn owe_destination_setter(&mut self) {
        self.pending_destination_setter = true;
    }

    /// Arrival ends the order the owed call would have resumed.
    pub(crate) fn settle_destination_setter(&mut self) {
        self.pending_destination_setter = false;
    }

    /// Take the owed call for `Simulation::complete_pending_order` to make.
    pub(crate) fn take_destination_setter(&mut self) -> bool {
        std::mem::take(&mut self.pending_destination_setter)
    }

    /// A fixture's owed call.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_pending_destination_setter_for_test(&mut self, pending: bool) {
        self.pending_destination_setter = pending;
    }

    /// Retained Foot+530 input to4DC760. Team AvoidThreats overrides this
    /// value at the getter; it does not mutate the stored coefficient.
    pub(crate) const fn path_threat_coefficient(&self) -> crate::util::native_x87::NativeF64Bits {
        self.threat_avoidance_coefficient
    }

    /// Foot4DC760 reads its TeamType+F2 override without changing Foot+530.
    /// Original reader/getter controls: astar_threat_inputs.{py,json}.
    pub(crate) fn path_threat_coefficient_for_team(
        &self,
        avoids_threats: bool,
    ) -> crate::util::native_x87::NativeF64Bits {
        if avoids_threats {
            crate::util::native_x87::NativeF64Bits::ONE
        } else {
            self.path_threat_coefficient()
        }
    }

    /// Successful FootUnlimbo4D72F4's sole type-to-object copy. Failed early
    /// placement and failed Mark must retain constructor or prior Load state.
    pub(crate) fn retain_threat_avoidance_after_unlimbo(
        &mut self,
        coefficient: crate::util::native_x87::NativeF64Bits,
    ) {
        self.threat_avoidance_coefficient = coefficient;
    }
}

/// Pixels Walk Move_To lifts a bridge destination (`0x0075AD4D MOV ECX,0x3C`).
const WALK_BRIDGE_LIFT_PIXELS: i32 = 60;

/// Drive Stop_Moving 0x4AFE00 and Ship 0x69F510 clamp the class target
/// fraction to the float 0.3 at 0x7E6240 / 0x7F1308 (identical bodies).
const TRACK_STOP_TARGET_FRACTION: crate::util::native_x87::NativeF64Bits =
    crate::util::native_x87::NativeF64Bits::from_bits(0x3fd3_3333_4000_0000);

/// Accepted Walk75ACB0 destination store. This conversion is independent of
/// HeadTo/OnBridge's416: a bridge cell lifts the destination by
/// `native_pixel_height_leptons(60)` (`0x0075AD52 CALL 0x006D2120`), 414.
/// See walk_head_occupation.json destination rows, including actual Cell+4C.
pub(crate) fn set_walk_destination_coord(
    entity: &mut GameEntity,
    coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) {
    // Walk75ACBD/75ACD0/75ACE3 checks EMP and both owner warp bytes.
    // The teleport owner represents the latter; the native EMP timer still
    // lacks its production writer. There is deliberately no power gate here.
    if super::locomotor_owner::owner_is_warping(entity) {
        return;
    }
    let Some(loco) = entity
        .locomotor
        .as_mut()
        .filter(|l| l.active_kind() == LocomotorKind::Walk)
    else {
        return;
    };
    let mut coord = coord;
    if coord != (DriveCoord { x: 0, y: 0, z: 0 })
        && let Some(terrain) = terrain
    {
        let cell = terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
        if terrain.native_cell_flags(cell) & 0x100 != 0 {
            coord.z = coord
                .z
                .wrapping_add(crate::util::lepton::native_pixel_height_leptons(
                    WALK_BRIDGE_LIFT_PIXELS,
                ));
        }
    }
    loco.set_walk_destination(Some(coord));
}

/// The Foot's Locomotor (`+0x674`) is a `DriveLocomotionClass` (Move_To `0x004AFD40`).
fn is_drive_locomotor(entity: &GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| matches!(loco.active_kind(), LocomotorKind::Drive))
}

/// The Foot's Locomotor (`+0x674`) is a `ShipLocomotionClass` (Move_To `0x0069F450`).
fn is_ship_locomotor(entity: &GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| matches!(loco.active_kind(), LocomotorKind::Ship))
}

/// `CellClass::GetCoords` (`0x00486840`) of the cell `MapClass::Get_CellClass`
/// (`0x005657A0`) gives for `(rx, ry)` through `cells`, which stamps the
/// query's fallback CellClass on a miss. Z 0 for an unsupported slope
/// ([`CellRef::get_coords`]).
///
/// [`CellRef::get_coords`]: crate::sim::cell_rect::CellRef::get_coords
pub(crate) fn target_cell_coord(
    rx: u16,
    ry: u16,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> DriveCoord {
    if let Some(cells) = cells {
        return crate::sim::cell_rect::native_cell_ref(cells, cells.lookup((rx as i16, ry as i16)))
            .get_coords();
    }
    // Map-less fixtures take the centre at Z 0. Every gameplay world (app,
    // headless, replay, sim-bench) comes from a map load.
    let point = crate::sim::cell_kernel::cell_center(
        crate::sim::cell_kernel::CellCoordinate {
            x: i32::from(rx as i16),
            y: i32::from(ry as i16),
        },
        0,
    );
    DriveCoord {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

/// Cell486840 through an already-retained pointer (virtual+48/+4C).
/// Recover the allocation from the original command coordinate without a Map
/// lookup: a later miss can have moved the shared Dummy, even onto a real
/// cell's coordinate, while its own level and slope still belong to Dummy.
pub(crate) fn retained_cell_coord(
    rx: u16,
    ry: u16,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> DriveCoord {
    let Some(cells) = cells else {
        return target_cell_coord(rx, ry, None);
    };
    let identity = crate::sim::combat::TargetKind::Cell(rx, ry)
        .cell_identity(cells.terrain())
        .expect("Cell target has a Cell allocation identity");
    crate::sim::cell_rect::native_cell_ref(cells, identity).get_coords()
}

/// Shared destination-setter adjustment (Drive4AFD40 / Ship69F450). Each call
/// receives raw caller XYZ, including already elevated Infantry GetCoords.
fn adjusted_destination(
    mut coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
) -> Option<DriveCoord> {
    if coord == (DriveCoord { x: 0, y: 0, z: 0 }) {
        return None;
    }
    if let Some(terrain) = terrain {
        let rx = (coord.x / 256) as i16;
        let ry = (coord.y / 256) as i16;
        let cell = terrain.native_cell_identity((rx, ry));
        let structural = terrain.native_cell_flags(cell) & 0x100 != 0;
        if structural {
            coord.z = coord
                .z
                .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        }
    }
    Some(coord)
}

/// Native4B05D0..0638 compares the raw target XYZ before calling SetDestination.
/// The committed head is independent and remains owned by movement acceptance.
pub(super) fn refresh_drive_destination_coord(
    entity: &mut GameEntity,
    coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) -> bool {
    let Some(loco) = entity
        .locomotor
        .as_ref()
        .filter(|l| l.has_track_state(TrackFamily::Drive))
    else {
        return false;
    };
    let requested = (coord != (DriveCoord { x: 0, y: 0, z: 0 })).then_some(coord);
    if loco.track_destination(TrackFamily::Drive) == requested {
        return false;
    }
    drive_set_destination(entity, coord, terrain, binary_frame)
}

/// The cell of `BuildingClass::GetDockCoord @ 0x00447B20` for `building`, as
/// `MapClass::operator[](COORD) @ 0x00565730` looks it up: the DOCKING
/// receiver's forced-MOVE_HERE test (`0x0043C91B..0x0043C93A`) and
/// Per_Cell_Process's dock-cell test (`0x0073A3B1..0x0073A437`). For a
/// `Refinery=` type the coordinate is the foundation centre plus 128 leptons
/// east, which for the stock 4x3 foundation is the cell NW+(2,1), beside the
/// pad NW+(3,1) (tools/spatial_oracle/refinery_dock, `nav_on_dock_coord`).
pub(crate) fn building_dock_cell(
    entities: &EntityStore,
    building_id: u64,
    requester: Option<u64>,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<(u16, u16)> {
    let coord =
        building_dock_coordinate(entities, building_id, requester, terrain, rules, interner)?;
    let cell = |value: i32| u16::try_from(value / 256).ok();
    Some((cell(coord.x)?, cell(coord.y)?))
}

/// Live GetDockCoord447B20 output shared by cell projection and Building
/// service distance447E00. This remains the one retail-offset coordinate owner.
pub(crate) fn building_dock_coordinate(
    entities: &EntityStore,
    building_id: u64,
    requester: Option<u64>,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<DriveCoord> {
    let building = entities.get(building_id)?;
    let object = interner.object_type(building.type_ref(), rules)?;
    super::building_coordinate::dock_coordinate(
        super::ground_pose::object_location(building, terrain),
        super::ground_pose::object_get_coords(building, terrain),
        object,
        &building.radio_contacts,
        requester,
        || {
            let id = requester.ok_or("Bunker dock coordinate needs a requester")?;
            let entity = entities
                .get(id)
                .ok_or_else(|| format!("Dock requester {id} disappeared"))?;
            Ok(super::ground_pose::object_get_coords(entity, terrain))
        },
    )
    .ok()
}

/// Resolve the live receiver behind a non-null NavCom: the object's store and
/// kind decide its coordinate, as its own virtual does natively. A dangling ID
/// is not NULL.
pub(crate) fn nav_target_coordinate(
    target: NavTargetRef,
    requester: Option<u64>,
    world: &crate::sim::world::Simulation,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: Option<(
        &crate::rules::ruleset::RuleSet,
        &crate::sim::intern::StringInterner,
    )>,
) -> Result<DriveCoord, String> {
    let id = match target {
        NavTargetRef::Cell { rx, ry } => {
            return Ok(retained_cell_coord(
                rx,
                ry,
                terrain
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            ));
        }
        NavTargetRef::Object { id } => id,
    };
    let entities = &world.substrate.entities;
    let Some(entity) = entities.get(id) else {
        // Terrain7F522C+4C is Object4104F0, which forwards virtual+48;
        // the requester does not alter its retained coordinate.
        let cells = terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        return super::ground_pose::target_get_coords(
            crate::sim::combat::TargetKind::Entity(id),
            world,
            cells.as_ref(),
        )
        .ok_or_else(|| format!("NavCom coordinate target {id} disappeared"));
    };
    if entity.category() == crate::map::entities::EntityCategory::Structure {
        let (rules, interner) =
            rules.ok_or_else(|| format!("Building {id} navigation requires type data"))?;
        let object = interner
            .object_type(entity.type_ref(), rules)
            .ok_or_else(|| format!("Building {id} navigation type disappeared"))?;
        return super::building_coordinate::navigation_coordinate(
            super::ground_pose::object_location(entity, terrain),
            super::ground_pose::object_get_coords(entity, terrain),
            object,
            &entity.radio_contacts,
            requester,
            || {
                let id = requester.expect("Bunker only reads a non-null requester");
                let entity = entities
                    .get(id)
                    .ok_or_else(|| format!("Navigation requester {id} disappeared"))?;
                Ok(super::ground_pose::object_get_coords(entity, terrain))
            },
        );
    }
    super::foot_coordinate::navigation_coordinate(entity, terrain)
}

impl crate::sim::world::Simulation {
    /// Query the existing virtual+4C navigation owner with the actual requester.
    /// Unlike Object+48, a moving Foot can return its retained head or tube exit;
    /// Buildings retain their requester/type-dependent coordinate policy.
    /// Radar6567F4 passes a null requester before its own signed cell clamp.
    pub fn navigation_target_coordinate(
        &self,
        target: NavTargetRef,
        requester: Option<u64>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Result<DriveCoord, String> {
        nav_target_coordinate(
            target,
            requester,
            self,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        )
    }
}

/// Cell callers of the shared non-null destination owner. Object callers
/// capture their receiver's +4C through `nav_target_coordinate` instead.
pub(crate) fn set_destination_internal_cell(
    entity: &mut GameEntity,
    target: (u16, u16),
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    let coord = target_cell_coord(
        target.0,
        target.1,
        resolved_terrain
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    );
    set_destination_internal_coord(
        entity,
        NavTargetRef::cell(target.0, target.1),
        coord,
        resolved_terrain,
        binary_frame,
    );
}

/// Foot4D9510/4D9628 publishes the reference and dispatches its captured +4C
/// coordinate. Cell and object orders reach the same active locomotor owner;
/// `binary_frame` is the locomotor Move_To paralysis clock.
pub(crate) fn set_destination_internal_coord(
    entity: &mut GameEntity,
    target: NavTargetRef,
    coord: DriveCoord,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    publish_nav_com(entity, target);

    if is_drive_locomotor(entity) {
        drive_set_destination(entity, coord, resolved_terrain, binary_frame);
    } else if is_ship_locomotor(entity) {
        ship_set_destination(entity, coord, resolved_terrain, binary_frame);
    } else if super::hover::hover_move_to(entity, coord, resolved_terrain, binary_frame) {
    } else {
        set_walk_destination_coord(entity, coord, resolved_terrain);
    }
}

/// Foot4D94C7/4D9510: NavComAux cleared and the reference published, before
/// the locomotor dispatch. The Foot+0x6AC skip (4D9607) stops here, and a
/// Teleport owner's Move_To is dispatched by the Unit setter.
pub(super) fn publish_nav_com(entity: &mut GameEntity, target: NavTargetRef) {
    entity.navigation.publish_nav_com(Some(target));
}

/// Foot4D94C7/4D9510 NULL reference publication, before locomotor Stop.
pub(super) fn publish_null_nav_com(entity: &mut GameEntity) {
    entity.navigation.publish_nav_com(None);
}

fn drive_set_destination(
    entity: &mut GameEntity,
    destination: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) -> bool {
    // Drive4AFD5E calls FootIsParalyzed4DE770 before destination/map access,
    // including a zero Move_To. Refusal retains Foot's NavCom/timer writes.
    // Owner warp bytes follow at4AFD71/80; EMP/Unit+6D8 is still unported.
    // Original whole-call comparisons: tools/spatial_oracle/track_destination.
    if entity.parasite.is_paralyzed(binary_frame)
        || super::locomotor_owner::owner_is_warping(entity)
    {
        return false;
    }
    let destination = adjusted_destination(destination, terrain);
    let Some(loco) = entity.locomotor.as_mut() else {
        return false;
    };
    loco.ensure_installed_track_state();
    // Native4AFD40 writes destination only. Accepted movement owns Head_To.
    loco.store_track_destination(TrackFamily::Drive, destination)
}

/// ILocomotion +0x44 Move_To of the active Drive/Ship instance without the
/// Foot setter (Drive 0x4AFD40 / Ship 0x69F450): the outer Process's NavCom
/// reissue (0x4B09A3 / 0x6A006C) calls it directly. Paralysis and warp
/// admission belong to the shared destination setters below.
pub(super) fn track_move_to(
    entity: &mut GameEntity,
    coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    if is_drive_locomotor(entity) {
        drive_set_destination(entity, coord, terrain, binary_frame);
    } else if is_ship_locomotor(entity) {
        ship_set_destination(entity, coord, terrain, binary_frame);
    }
}

/// ILocomotion +0x48 Stop_Moving of the active Drive, Ship or Hover
/// instance, as FootStopDriver4D55C0/Unit+500 dispatches from FindPath,
/// FireAt and first-Limbo. It clears the locomotor destination; NavCom and the committed
/// head are untouched.
pub(crate) fn track_stop_moving(entity: &mut GameEntity) -> bool {
    if is_drive_locomotor(entity) {
        drive_stop_moving(entity);
    } else if is_ship_locomotor(entity) {
        ship_stop_moving(entity);
    } else {
        return super::hover::hover_stop_moving(entity);
    }
    true
}

fn drive_stop_moving(entity: &mut GameEntity) {
    let Some(loco) = entity.locomotor.as_mut() else {
        return;
    };
    loco.ensure_installed_track_state();
    // 0x4AFE00 clamps the class target fraction (+0x50), then clears only
    // the destination; the head (+0x40) may continue to its endpoint. The
    // IsTrain follower cascade has no stock type (no retail IsTrain=yes).
    if let Some(target) = loco.track_target_fraction_native(TrackFamily::Drive) {
        loco.store_track_target_fraction_native(
            TrackFamily::Drive,
            stopped_target_fraction(target),
        );
    }
    loco.store_track_destination(TrackFamily::Drive, None);
    // Foot+578 is retained here. The common Process tail4B0828 owns its
    // eventual zero, after the destination/head/path gates (track_turn).
}

fn ship_set_destination(
    entity: &mut GameEntity,
    destination: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
    binary_frame: u32,
) {
    // Ship69F46E calls FootIsParalyzed4DE770 before even a zero destination
    // write. Warp gates69F481/90 follow; Stop69F510 is a separate entry.
    if entity.parasite.is_paralyzed(binary_frame)
        || super::locomotor_owner::owner_is_warping(entity)
    {
        return;
    }
    let destination = adjusted_destination(destination, terrain);
    let Some(loco) = entity.locomotor.as_mut() else {
        return;
    };
    loco.ensure_installed_track_state();
    // Ship's Move_To slot writes only +0x30. The committed +0x3C head is
    // selected later by Process_Movement from the owner's path.
    loco.store_track_destination(TrackFamily::Ship, destination);
}

fn ship_stop_moving(entity: &mut GameEntity) {
    let Some(loco) = entity.locomotor.as_mut() else {
        return;
    };
    loco.ensure_installed_track_state();
    // Ship Stop_Moving clamps the class-owned target fraction, then clears
    // only +0x30. A committed head may continue to its track endpoint.
    if let Some(target) = loco.track_target_fraction_native(TrackFamily::Ship) {
        loco.store_track_target_fraction_native(TrackFamily::Ship, stopped_target_fraction(target));
    }
    loco.store_track_destination(TrackFamily::Ship, None);

    // OPEN Process-host correction: this preexisting adapter applies the
    // rest speed before the native Process-tail admission. FootStop4DF0D0
    // does NOT clear Foot+5E0; explicit abandonment is a separate owner call.
    if loco.track_head(TrackFamily::Ship).is_none()
        && entity.foot_speed.speed_fraction_is_positive()
    {
        entity.foot_speed.set_speed_fraction(SIM_ZERO);
    }
}

impl crate::sim::world::Simulation {
    /// Foot4D94B0 clears NavComAux before its three nonnull admission gates:
    /// a Magnetron-lifted object (`+0x6AD`), a passenger of an open transport
    /// (`+0x82`) and a bunkered vehicle (`+0x2E4`) take no destination. An
    /// admitted one lets go of any object it holds aloft (`0x004D9509`,
    /// [`Self::release_locomotor`]) before NavCom is written. Class
    /// preprocessing precedes this call; publication, locomotor dispatch and
    /// accepted timers follow it. Retained-particle cleanup still requires
    /// its missing native owner and is not implied here.
    pub(crate) fn begin_foot_destination(
        &mut self,
        id: u64,
        nonnull: bool,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let refused = nonnull
            && (entity.lift.swap_active()
                || entity.passenger_role.in_open_transport()
                || entity.bunker.link().installed_in().is_some());
        let holds = entity.lift.target().is_some();
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .clear_nav_com_aux();
        if refused {
            return false;
        }
        if nonnull && holds {
            self.release_locomotor(id, true, rules, registry, frame_effects);
        }
        true
    }

    /// Foot's `Set_Destination` with a NULL target (`0x004D94B0`), where every
    /// class setter's null arm ends: Unit's at `0x0074314F`, Infantry's at
    /// `0x0051B1D2` and Aircraft's at once (`0x0041AA8B` -> `0x0041ADAC`).
    /// - NavComAux (`0x004D94C7`) and NavCom (`0x004D9510`) clear.
    /// - A lifted object a Magnetron still holds loses both links and may end
    ///   its lift (`0x004D9518..0x004D953F`, [`ObjectSubstrate::lifted_null_destination`](crate::sim::world::ObjectSubstrate::lifted_null_destination)).
    /// - An Aircraft (What_Am_I 2) whose current (`+0xAC`) or queued
    ///   (`+0xB4`) mission is Attack and which holds a TarCom (`+0x2B4`) skips
    ///   the locomotor Stop (`0x004D9672..0x004D969C`): its Fly flies on.
    /// - Otherwise the active locomotor's `Stop_Moving` runs (`0x004D96B9`,
    ///   [`Self::locomotor_stop_moving`]) and NavCom is cleared again
    ///   (`0x004D96BC`), so a re-target's NavCom does not survive.
    /// - The timer tail (`0x004D96C2..0x004D9707`) follows either way.
    ///
    /// The caller's class prelude runs first.
    pub(crate) fn foot_null_destination(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        use crate::sim::mission::{MissionId, MissionType};
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        publish_null_nav_com(entity);
        self.substrate.lifted_null_destination(id);
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let attack = MissionId::from_known(MissionType::Attack);
        let attacking_aircraft = entity.category()
            == crate::map::entities::EntityCategory::Aircraft
            && (entity.mission.current() == attack || entity.mission.queued() == attack)
            && entity.attack_target.target().is_some();
        if !attacking_aircraft {
            self.locomotor_stop_moving(id, rules, registry, frame_effects)
                .unwrap_or_else(|cause| panic!("Foot null destination {id}: {cause}"));
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.navigation.store_nav_com(None);
            }
        }
        let timing = super::DestinationTiming::from_rules(self.session.binary_frame(), rules);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            timing.accept(entity);
        }
    }

    /// `Stop_Moving` (ILocomotion `+0x48`) of the owner's active locomotor, as
    /// Foot's null destination calls it (`0x004D96B9`) and as Foot's
    /// `Stop_Driver` is (`0x004D55C0`: the Unit and Aircraft vtable `+0x500`,
    /// and the tail of Infantry's `0x0051DAF0`):
    /// - Drive `0x004AFE00`, Ship `0x0069F510` and Hover `0x00516320`
    ///   ([`track_stop_moving`]);
    /// - Walk `0x0075ADA0` (`Simulation::walk_stop_moving`);
    /// - Jumpjet `0x0054B4D0` (`Simulation::jumpjet_stop_moving`), which
    ///   re-targets a moving owner to the passable cell nearest it;
    /// - Teleport `0x00718230`
    ///   ([`teleport_stop_moving`](super::teleport_movement::teleport_stop_moving));
    /// - Fly `0x004CCFD0` (`Simulation::fly_stop_moving`), which re-targets a
    ///   moving aircraft through its own class setter;
    /// - Rocket `0x006633C0`, an empty body (`RET 4`).
    ///
    /// The Jumpjet's needs the map and, for a failed search, the rules; the
    /// Fly's needs the rules. A world without them leaves the owner as it
    /// was.
    pub(crate) fn locomotor_stop_moving(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return Ok(());
        };
        let Some(kind) = entity.locomotor.as_ref().map(|loco| loco.active_kind()) else {
            return Ok(());
        };
        match kind {
            LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Hover => {
                track_stop_moving(entity);
            }
            LocomotorKind::Walk => self.walk_stop_moving(id, rules, frame_effects)?,
            LocomotorKind::Jumpjet => {
                if !self.jumpjet_stop_moving(id, rules, registry, frame_effects) {
                    log::debug!("Jumpjet {id} Stop_Moving lacks the map or rules");
                }
            }
            LocomotorKind::Teleport => super::teleport_movement::teleport_stop_moving(entity),
            LocomotorKind::Fly => {
                if !self.fly_stop_moving(id, rules, registry, frame_effects) {
                    log::debug!("Fly {id} Stop_Moving lacks the rules");
                }
            }
            LocomotorKind::Rocket => {}
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "track_destination_tests.rs"]
mod native_destination_tests;

/// Stop4AFEA4/69F5B4 always spills the selected value. TEST AH,1 keeps
/// Less/Unordered, and Equal/Greater selects the promoted-float floor.
fn stopped_target_fraction(
    target: crate::util::native_x87::NativeF64Bits,
) -> crate::util::native_x87::NativeF64Bits {
    use crate::util::native_x87::{
        MaskedX87Chop53 as X,
        MaskedX87Ordering::{Less, Unordered},
    };
    let target = X::load_f64(target);
    let floor = X::load_f64(TRACK_STOP_TARGET_FRACTION);
    X::store_f64_masked_chop(if matches!(X::compare(target, floor), Less | Unordered) {
        target
    } else {
        floor
    })
}
