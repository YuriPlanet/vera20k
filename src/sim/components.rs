//! Shared data structs used as fields of GameEntity and components.rs.
//!
//! These are plain data types with no behavior. Game logic lives in systems
//! (movement, combat, etc.). The render loop reads GameEntity fields to
//! determine what to draw and where.
//!
//! ## Design notes
//! - Position stores isometric cell coords only. Where an entity is *drawn* is
//!   `render::locomotor_visual`'s business, derived on read — sim/ writes no
//!   screen coordinates.
//!
//! ## Dependency rules
//! - Part of sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::sim::cell_kernel::{CellCoordinate, WorldCoordinate, cell_center};
use crate::sim::intern::InternedId;
use crate::util::fixed_math::SimFixed;

pub use crate::sim::movement::ground_pose::Position;
pub use crate::sim::movement::navcom::NavigationState;

// MissionCom, MissionLeaf and the private GameEntity body own retained
// construction and sale state.
pub(crate) use crate::sim::building_construction::BuildingDown;
#[cfg(any(test, feature = "test-support"))]
pub use crate::sim::building_construction::BuildingUp;

/// A NavCom-family pointer: NavCom (`Foot+0x5A4`), its auxiliary slot
/// (`+0x5A0`), SuspendedNavCom (`+0x5A8`) and the NavQueue and navigation-list
/// entries.
///
/// In gamemd each is an untagged `AbstractClass*`. The Foot setter
/// (`FootClass::Set_Destination_Internal @ 0x004D94B0`) stores its argument
/// as given (`0x004D9510`), and readers ask the pointee for its kind or
/// compare pointers: `AircraftClass::Mission_Enter @ 0x00419C80` compares
/// NavCom with `+0x6CC` (`0x00419D25`) and calls NavCom's What_Am_I
/// (virtual `+0x2C`, `0x00419D2F`) to test for a Building (6).
///
/// So there is one representation per pointee kind. A Cell is named by its
/// cell. An Object, a Techno or a Terrain, is named by its stable ID; every
/// object store draws from one ID namespace (`Simulation::allocate_stable_id`,
/// and snapshot restore rejects an ID that two registries claim), so the ID
/// alone says which store holds the object and what kind it is.
///
/// Derived coordinate equality compares cells by coordinates. Consumers that
/// require native allocation identity (including Aircraft AI's dummy clear)
/// use `TargetKind::cell_identity` after conversion; see the remaining
/// `Per_Cell_Process` residual in `movement::per_cell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NavTargetRef {
    /// A `CellClass*`, by its cell.
    Cell { rx: u16, ry: u16 },
    /// An `ObjectClass*`, by stable ID: a Techno in the entity store or a
    /// Terrain object.
    Object { id: u64 },
}

impl NavTargetRef {
    pub fn cell(rx: u16, ry: u16) -> Self {
        Self::Cell { rx, ry }
    }

    pub fn object(id: u64) -> Self {
        Self::Object { id }
    }

    /// Compare retained Abstract pointers without querying or restamping Map.
    /// Distinct missing-coordinate tokens name the one CellClass Dummy;
    /// packed fixed-stride aliases also name the same allocated Cell.
    pub(crate) fn same_pointer(
        self,
        other: Self,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) -> bool {
        match (self, other, terrain) {
            (Self::Cell { rx: ax, ry: ay }, Self::Cell { rx: bx, ry: by }, Some(terrain)) => {
                terrain.native_fixed_cell_index(ax as i16, ay as i16)
                    == terrain.native_fixed_cell_index(bx as i16, by as i16)
            }
            _ => self == other,
        }
    }

    /// The object this names; a Cell names none.
    pub(crate) const fn object_id(self) -> Option<u64> {
        match self {
            Self::Object { id } => Some(id),
            Self::Cell { .. } => None,
        }
    }
}

/// Persistent Foot path state. The FootClass constructor 0x004D31E0 anchors
/// both timers at the current frame (0x4D3320, 0x4D335B), stores +64C = 10
/// (0x4D332C) and +6B7 = 0 (0x4D3451). Set_Destination_Internal 0x004D94B0
/// rewrites the timers and latch at 0x4D96C2..0x4D9707 for every accepted
/// setter, including a null destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootPathRuntime {
    /// Foot+640/+648, including the native frame anchor.
    movement_timer: crate::sim::timer::CdTimer,
    /// Foot+668/+670.
    blocked_timer: crate::sim::timer::CdTimer,
    /// Foot+6B7.
    path_blocked: bool,
    /// Foot+64C, a dword decremented only while nonzero.
    retries_left: u32,
    /// Foot+68A: the pending path-failure sound byte. Ordinary construction
    /// clears it (0x004D33B4); raw Load (0x004103CD) and the no-init Foot
    /// constructor (0x004D3540) preserve the exact byte, including 255.
    /// No ordinary gameplay writer that arms it has been established.
    /// Evidence: `tools/spatial_oracle/foot_scold_latch.{py,json,md}`.
    #[serde(default)]
    scold_latch: u8,
}

impl Default for FootPathRuntime {
    fn default() -> Self {
        Self::at_frame(0)
    }
}

impl FootPathRuntime {
    /// `Foot+0x64C`'s reload value, the literal 10 every writer stores: the
    /// constructor (`0x004D332C`), Drive and Ship after a found route
    /// (`0x004B3285`, `0x006A28D5`), Hover (`0x00516BCD`) and Walk
    /// (`0x0075B2E2`, `0x0075BA27`).
    ///
    /// `Process_Movement` reads `[ECX+0x64C]` at `0x004B2DC8`, and on `> 0`
    /// decrements and stores it back (`0x004B2DD2`/`0x004B2DD3`) before
    /// continuing; on `<= 0` it clears the drive coord, calls
    /// `FootClass::Stop_Moving` plus vtable `+0x480`/`+0x484`, and plays the
    /// blocked voice at `Foot+0x68A`. So it *is* decremented and it *does* end
    /// the move at zero.
    ///
    /// Recorded difference: **what decrements it.** Native decrements it on every
    /// pass through the generic blocked label `LAB_004B3282` — reached from
    /// `Is_Cell_In_Playfield == 0`, from code 3, from `code != 6` and from two
    /// code-6 sub-failures — and the same label is where the literal 10 is stored.
    /// VERA decrements per failed repath instead. The escalation clock is a
    /// separate record: `Foot+0x668` = frame, `Foot+0x66C`, and `Foot+0x670` =
    /// `Rules+0x1768`, stored only on the `Foot+0x6B7 == 0` transition of the code-2
    /// arm — which contains no `Scatter_Objects` call at all. Trigger: a unit
    /// blocked long enough to exhaust the counter. Player effect: the give-up point
    /// arrives after a different number of ticks than retail's. Frequency: every
    /// traffic jam, many times a minute once a base has armour queuing. Downstream
    /// risk: the two clocks are separate fields and must stay separate.
    pub(crate) const RETRY_RELOAD: u32 = 10;

    pub const fn at_frame(frame: u32) -> Self {
        Self {
            movement_timer: crate::sim::timer::CdTimer::started(frame as i32, 0),
            blocked_timer: crate::sim::timer::CdTimer::started(frame as i32, 0),
            path_blocked: false,
            retries_left: Self::RETRY_RELOAD,
            scold_latch: 0,
        }
    }

    /// Find_Path's urgency on the code-2 ladders (Walk 0x0075B979, Drive/Ship
    /// 0x4B36BC..0x4B36EF): 2 once the latched blockage timer has expired,
    /// else 1.
    pub(crate) fn code2_urgency(&self, frame: u32) -> u8 {
        if self.path_blocked && self.blocked_timer.expired(frame as i32) {
            2
        } else {
            1
        }
    }

    /// `Foot+0x640`/`+0x648`, the movement timer.
    pub(crate) const fn movement_timer(&self) -> crate::sim::timer::CdTimer {
        self.movement_timer
    }

    /// `Foot+0x668`/`+0x670`, the blockage timer.
    pub(crate) const fn blocked_timer(&self) -> crate::sim::timer::CdTimer {
        self.blocked_timer
    }

    /// `Foot+0x6B7`, the blockage latch.
    pub const fn path_blocked(&self) -> bool {
        self.path_blocked
    }

    /// The code-2 refusal's latch (Walk `0x0075B8A9..0x0075B979`, Drive/Ship
    /// `0x004B3656..0x004B368D`, Hover's code 2): the first refusal stores
    /// `Foot+0x6B7` and starts the BlockagePathDelay grace (`+0x668`); a
    /// latched blockage keeps its running grace.
    pub(crate) fn latch_blockage(&mut self, frame: u32, delay: i32) {
        if !self.path_blocked {
            self.path_blocked = true;
            self.start_blocked(frame, delay);
        }
    }

    /// Clear the blockage latch (`Foot+0x6B7 = 0`), keeping the grace timer:
    /// progress, a found path or a new command ends the blockage.
    pub(crate) fn clear_blockage(&mut self) {
        self.path_blocked = false;
    }

    /// `Set_Destination_Internal` (`0x004D94B0`) tail `0x004D96C2..0x004D9707`
    /// for every accepted destination: `+0x6B7 = 0`, `+0x668 = (frame,
    /// Rules+1768)`, `+0x640 = (frame, 0)`. It never writes `+0x64C`.
    pub(crate) fn accept_destination(&mut self, frame: u32, blockage_delay: i32) {
        self.start_movement(frame, 0);
        self.start_blocked(frame, blockage_delay);
        self.path_blocked = false;
    }

    /// A fixture's blockage latch.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_path_blocked_for_test(&mut self, blocked: bool) {
        self.path_blocked = blocked;
    }

    /// `Foot+0x64C`, the path retries left.
    pub(crate) const fn retries_left(&self) -> u32 {
        self.retries_left
    }

    /// Reload the path retries (`Foot+0x64C`) to [`Self::RETRY_RELOAD`]: a
    /// found path (the no-head Process FindPath success, Walk `0x0075B2E2`)
    /// restores the full budget.
    pub(crate) fn reload_retries(&mut self) {
        self.retries_left = Self::RETRY_RELOAD;
    }

    /// A fixture's path retries.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_retries_left_for_test(&mut self, retries: u32) {
        self.retries_left = retries;
    }

    /// Spend one path retry.
    pub(crate) fn decrement_retries_left(&mut self) {
        self.retries_left -= 1;
    }

    /// A fixture's movement timer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_movement_timer_for_test(&mut self, timer: crate::sim::timer::CdTimer) {
        self.movement_timer = timer;
    }

    /// A fixture's blockage timer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_blocked_timer_for_test(&mut self, timer: crate::sim::timer::CdTimer) {
        self.blocked_timer = timer;
    }

    /// Native Foot checksum 0x004DBCFE reads the byte without normalizing it.
    pub(crate) const fn scold_latch_raw(&self) -> u8 {
        self.scold_latch
    }

    /// Walk 0x0075B085..0x0075B0B0 tests nonzero, then clears unconditionally.
    /// Callers decide whether that branch requests a sound before clearing.
    pub(crate) fn clear_scold_latch(&mut self) -> bool {
        std::mem::replace(&mut self.scold_latch, 0) != 0
    }

    /// Supply native retained inputs without inventing a gameplay arming edge.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_scold_latch_for_test(&mut self, raw: u8) {
        self.scold_latch = raw;
    }

    /// Original Foot timers retain the binary frame; repeated Process calls
    /// must not consume time (Drive4B3607, Ship6A2C56, Walk75B979).
    pub(crate) fn start_movement(&mut self, frame: u32, duration: i32) {
        self.movement_timer = crate::sim::timer::CdTimer::started(frame as i32, duration);
    }

    pub(crate) fn start_blocked(&mut self, frame: u32, duration: i32) {
        self.blocked_timer = crate::sim::timer::CdTimer::started(frame as i32, duration);
    }
}

/// Integer world coordinate triplet used by DriveLocomotion state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DriveCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl DriveCoord {
    /// Cell (rx, ry)'s centre ([`cell_center`]) at height `z`.
    pub fn cell(rx: u16, ry: u16, z: i32) -> Self {
        let cell = CellCoordinate {
            x: i32::from(rx),
            y: i32::from(ry),
        };
        cell_center(cell, z).into()
    }
}

impl From<WorldCoordinate> for DriveCoord {
    fn from(point: WorldCoordinate) -> Self {
        Self {
            x: point.x,
            y: point.y,
            z: point.z,
        }
    }
}

/// The most direction words Find_Path's ordinary copy installs
/// (`0x004D3E82..0x004D3EA1`): AStar returns the whole route, and a Foot
/// requests the next window when this one runs short, so a long trip adapts
/// to blockers found on the way.
pub(crate) const FIND_PATH_WINDOW_WORDS: usize = 24;

/// Foot-owned path replay. Native shifts a 24-dword queue on consumption;
/// Rust retains the consumed prefix with an explicit cursor. Consumers must
/// interpret the remaining suffix, not vector emptiness, as the native queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootPathQueue {
    /// Foot+5E0: octants0..7, explicit tube direction8, native -1 terminator.
    /// Find_Path4D3E98 writes it; Drive/Ship movement consumes the same owner
    /// memory, independently of which locomotor is currently installed.
    #[serde(default)]
    directions: Vec<u8>,
    #[serde(default)]
    cursor: u16,
    /// Foot+558. Fresh acceptance writes this reference (4B4618/6A3C47);
    /// chain consumption preserves it (4B1DF7/6A143A).
    #[serde(default)]
    reference_cell: Option<(i16, i16)>,
}

impl FootPathQueue {
    /// `Foot+0x5E0`, every direction word, consumed prefix included.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn directions(&self) -> &[u8] {
        &self.directions
    }

    /// The index of the first unconsumed word.
    pub(crate) fn cursor(&self) -> u16 {
        self.cursor
    }

    /// `Foot+0x558`, the reference cell.
    pub(crate) fn reference_cell(&self) -> Option<(i16, i16)> {
        self.reference_cell
    }

    /// Write the native `-1` terminator at `slot`, when there is a word there:
    /// the NavCom trim cuts the queue short.
    pub(crate) fn terminate_at(&mut self, slot: usize) {
        if let Some(word) = self.directions.get_mut(slot) {
            *word = u8::MAX;
        }
    }

    /// `Path[0] = -1`: every word consumed, the empty queue.
    pub(crate) fn exhaust(&mut self) {
        self.cursor = self.directions.len().min(u16::MAX as usize) as u16;
    }

    /// `Foot+0x558`, the cell the remaining directions start from.
    pub(crate) fn rebase(&mut self, reference_cell: (i16, i16)) {
        self.reference_cell = Some(reference_cell);
    }

    /// The queue `FootClass::Find_Path @ 0x004D3920` installs from a found
    /// route (`0x004D393A..0x004D4003`): the directions from `reference`
    /// through `path` from `first_destination` on, unconsumed. The ordinary
    /// zero-prefix copy at `0x004D3E82..0x004D3EA1` takes at most
    /// [`FIND_PATH_WINDOW_WORDS`] words of the whole AStar route; tube jumps
    /// count as one word. The Vec's end represents a short route's
    /// terminator without retaining unused native backing words.
    pub(crate) fn install(
        &mut self,
        reference: (u16, u16),
        path: &[(u16, u16)],
        first_destination: usize,
    ) {
        let mut from = (reference.0 as i16, reference.1 as i16);
        self.directions.clear();
        for &destination in path
            .iter()
            .skip(first_destination)
            .take(FIND_PATH_WINDOW_WORDS)
        {
            self.directions.push(direction_from_step(from, destination));
            from = (destination.0 as i16, destination.1 as i16);
        }
        self.cursor = 0;
        self.reference_cell = Some((reference.0 as i16, reference.1 as i16));
    }

    /// Fresh acceptance (`0x004B4618`/`0x006A3C47`): `endpoint` becomes the
    /// reference and `consumed_directions` words are popped.
    pub(crate) fn accept(&mut self, endpoint: (i16, i16), consumed_directions: usize) {
        self.reference_cell = Some(endpoint);
        self.consume(consumed_directions);
    }

    /// Accepted chain `0x004B1DF7`/`0x006A143A` and the tube entries (Walk
    /// `0x0075B3E0..0x0075B3EB`, Drive `0x004B135D..0x004B1373`, Ship
    /// `0x006A0A25..0x006A0A3B`) pop the queue without rewriting `Foot+0x558`.
    pub(crate) fn consume(&mut self, consumed_directions: usize) {
        let cursor = usize::from(self.cursor)
            .saturating_add(consumed_directions)
            .min(self.directions.len());
        self.cursor = cursor.min(u16::MAX as usize) as u16;
    }

    /// A fixture's cursor.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_cursor_for_test(&mut self, cursor: u16) {
        self.cursor = cursor;
    }

    /// A fixture's reference cell.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_reference_cell_for_test(&mut self, reference_cell: Option<(i16, i16)>) {
        self.reference_cell = reference_cell;
    }

    /// A fixture's direction word at `slot`, when there is one.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_direction_for_test(&mut self, slot: usize, direction: u8) {
        if let Some(word) = self.directions.get_mut(slot) {
            *word = direction;
        }
    }

    /// A fixture's direction words.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_directions_for_test(&mut self, directions: Vec<u8>) {
        self.directions = directions;
    }

    /// A fixture's queue.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(
        directions: Vec<u8>,
        cursor: u16,
        reference_cell: Option<(i16, i16)>,
    ) -> Self {
        Self {
            directions,
            cursor,
            reference_cell,
        }
    }

    /// `start` followed by the cells `words` step through from it. A tube
    /// word (8) names no adjacent cell and ends the walk.
    fn walk_words(start: (i16, i16), words: &[u8]) -> Vec<(i16, i16)> {
        let mut cell = start;
        words
            .iter()
            .map_while(|&direction| {
                let (dx, dy) =
                    *crate::util::direction::DIRECTION_DELTAS.get(usize::from(direction))?;
                cell = (
                    cell.0.wrapping_add(dx as i16),
                    cell.1.wrapping_add(dy as i16),
                );
                Some(cell)
            })
            .collect()
    }

    /// The cells the unpaid words step through, starting from the cell the head
    /// word steps out of.
    ///
    /// Native shifts the 24-dword `Foot+5E0` queue left as it consumes, so index
    /// 0 always steps out of the mover's own cell. Rust keeps the paid prefix
    /// behind a cursor instead, which leaves the stored reference (`Foot+558`) as
    /// a valid origin only where its owner refreshes it: `walk_host`'s completed
    /// step rewrites it from the object's position after every paid step, and
    /// `hover_process` rewrites it per cell. The chain and tube pops
    /// (`track_host`, `tube_movement`) preserve it, as
    /// [`FootPathQueue::consume`] records, so on
    /// those paths the reference stays at the accepted endpoint while the cursor
    /// moves on and a suffix walked from it falls a cell further behind the mover
    /// per paid step. The anchor is therefore the caller's to supply: pass the
    /// object's position cell, the same cell movement applies the head word to.
    ///
    /// A tube word (8) names no adjacent cell and ends the walk, as in
    /// [`Self::walk_words`]. Diagnostics and tests.
    pub fn remaining_cells_from(&self, head: (i16, i16)) -> Vec<(i16, i16)> {
        Self::walk_words(head, self.remaining_directions())
    }

    /// The reference cell followed by the unpaid cells walked from it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn route_cells(&self) -> Vec<(u16, u16)> {
        self.reference_cell
            .into_iter()
            .chain(self.remaining_cells_from(self.reference_cell.unwrap_or((0, 0))))
            .map(|(x, y)| (x as u16, y as u16))
            .collect()
    }

    /// `start` followed by the cells every word (consumed or not) steps
    /// through from it: the route one Find_Path installed from `start`, while
    /// the queue still holds that install's words.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn installed_cells(&self, start: (u16, u16)) -> Vec<(u16, u16)> {
        std::iter::once((start.0 as i16, start.1 as i16))
            .chain(Self::walk_words(
                (start.0 as i16, start.1 as i16),
                &self.directions,
            ))
            .map(|(x, y)| (x as u16, y as u16))
            .collect()
    }

    /// Native queue emptiness tests the unconsumed head, not retained history.
    pub fn remaining_directions(&self) -> &[u8] {
        let suffix = &self.directions[usize::from(self.cursor).min(self.directions.len())..];
        &suffix[..suffix
            .iter()
            .position(|&direction| direction == u8::MAX)
            .unwrap_or(suffix.len())]
    }

    /// Drive4B224F/Ship6A1899 overwrites the live queue head with -1 after
    /// terminal PerCell2, preserving the backing suffix and reference cell.
    pub(crate) fn clear_live_head(&mut self) {
        let cursor = usize::from(self.cursor).min(self.directions.len());
        if cursor == self.directions.len() {
            self.directions.push(u8::MAX);
        } else {
            self.directions[cursor] = u8::MAX;
        }
    }
}

/// Foot-owned speed and retained crush flag, shared by every locomotor instance.
///
/// SetSpeedFraction4D3710 writes Foot+578/+57C; GetCurrentSpeed4DB1A0 reads
/// that fraction live (`movement::owner_current_speed`); nothing caches its
/// result. Drive4AF540/Ship69EC50 constructors and DriveEND4AF930 do
/// not own or reset it. Keep this outside both class payloads so a synchronous
/// callback can replace a locomotor without replacing the owner's speed.
/// Original executable witnesses: tools/spatial_oracle/foot_speed_owner.json.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootSpeedState {
    /// Foot `+0x578`. The Foot constructor zeroes it (`0x004D327F`/`0x004D328B`,
    /// ported by `Default`); after that only `FootClass::SetSpeedFraction @
    /// 0x004D3710` writes it: [`Self::set_speed_fraction`] for a fixed-point
    /// request and [`Self::set_speed_fraction_native_bits`] for a native double.
    applied_fraction: SimFixed,
    /// Foot+580, initialized to exactly 1.0 at4D3292/4D329B. Retain the
    /// native bits: pickup refuses even the immediate neighbors of 1.0.
    /// Every speed query reads it (GetCurrentSpeed multiplies it in). Its only
    /// native writer is the Speed crate (`accept_speed_crate`), reached from
    /// `CellClass::PickupCrate` (`crates::pickup`) when a Foot commits to a
    /// crate cell; saved and hashed (v181). With Ares this is the permanent
    /// Ext+90 baseline; the separately owned derived Foot+580 is below.
    crate_multiplier: crate::util::native_x87::NativeF64Bits,
    /// Ares100460A0's derived Foot+580, distinct from its permanent
    /// TechnoExt+90 crate baseline above. None denotes an unchanged baseline.
    /// The attachment owner updates it synchronously after native mutations;
    /// snapshot restore validates it against that owner's ordered vector.
    #[cfg(feature = "ares")]
    #[serde(default)]
    attached_speed: Option<crate::util::native_x87::NativeF64Bits>,
    /// Foot+6B5, constructor0 at4D3444. Drive's straight/CrusherAll
    /// pre-entry arm4B1A2F sets it; Unit's admitted overlay tail73B067
    /// clears it. Both the Track speed prefix and RockingUpdate read it.
    /// It survives class replacement and ordinary movement/Stop calls.
    crush_slowdown: bool,
}

impl Default for FootSpeedState {
    fn default() -> Self {
        Self {
            applied_fraction: crate::util::fixed_math::SIM_ZERO,
            crate_multiplier: crate::util::native_x87::NativeF64Bits::ONE,
            #[cfg(feature = "ares")]
            attached_speed: None,
            crush_slowdown: false,
        }
    }
}

impl FootSpeedState {
    pub fn crush_slowdown(&self) -> bool {
        self.crush_slowdown
    }

    /// Drive's straight/CrusherAll pre-entry arm (`0x004B1A2F`) sets
    /// `Foot+0x6B5`.
    pub(crate) fn arm_crush_slowdown(&mut self) {
        self.crush_slowdown = true;
    }

    /// Unit's admitted overlay tail (`0x0073B067`) clears `Foot+0x6B5`.
    pub(crate) fn clear_crush_slowdown(&mut self) {
        self.crush_slowdown = false;
    }

    /// A fixture's crush slowdown.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_crush_slowdown_for_test(&mut self, active: bool) {
        self.crush_slowdown = active;
    }

    pub fn crate_multiplier(&self) -> crate::util::native_x87::NativeF64Bits {
        self.crate_multiplier
    }

    /// Live Foot+580. Crate eligibility continues to read the permanent
    /// baseline; GetCurrentSpeed reads this derived value under Ares.
    pub fn effective_multiplier(&self) -> crate::util::native_x87::NativeF64Bits {
        #[cfg(feature = "ares")]
        if let Some(multiplier) = self.attached_speed {
            return multiplier;
        }
        self.crate_multiplier
    }

    #[cfg(feature = "ares")]
    pub(crate) fn set_attached_speed(
        &mut self,
        multiplier: crate::util::native_x87::NativeF64Bits,
    ) {
        self.attached_speed = (multiplier != self.crate_multiplier).then_some(multiplier);
    }

    /// Foot `+0x578`, the applied speed fraction.
    pub fn applied_fraction(&self) -> SimFixed {
        self.applied_fraction
    }

    /// `FootClass::SetSpeedFraction @ 0x004D3710` for a fixed-point request:
    /// at least 1 stores 1, at most 0 stores 0, and anything between is
    /// stored as given ([`Self::stored_speed_fraction`]).
    pub(crate) fn set_speed_fraction(&mut self, fraction: SimFixed) {
        self.applied_fraction = fraction.clamp(
            crate::util::fixed_math::SIM_ZERO,
            crate::util::fixed_math::SIM_ONE,
        );
    }

    /// The double `FootClass::SetSpeedFraction @ 0x004D3710` stores at Foot
    /// `+0x578` for a requested one: at least 1.0, +infinity included, stores
    /// 1.0 (`0x004D3714..0x004D3735`); at most 0, negative zero included, or
    /// NaN stores +0 (`0x004D373C..0x004D375D`); anything between is stored
    /// as given. Original executable witnesses, signed zeros, NaNs,
    /// infinities and denormals included:
    /// `tools/spatial_oracle/foot_speed_owner.json`.
    pub(crate) fn stored_speed_fraction(
        requested: crate::util::native_x87::NativeF64Bits,
    ) -> crate::util::native_x87::NativeF64Bits {
        use crate::util::native_x87::NativeF64Bits;
        const INFINITY_BITS: u64 = 0x7ff0_0000_0000_0000;
        let bits = requested.bits();
        if bits >> 63 != 0 || bits == 0 || bits > INFINITY_BITS {
            // A negative value, a zero of either sign, or NaN.
            NativeF64Bits::POSITIVE_ZERO
        } else if bits >= NativeF64Bits::ONE.bits() {
            NativeF64Bits::ONE
        } else {
            requested
        }
    }

    /// `FootClass::SetSpeedFraction @ 0x004D3710` for a locomotor that computes
    /// its fraction as a native double (the Jumpjet's `Process`): the double
    /// [`Self::stored_speed_fraction`] keeps.
    ///
    /// The stored double becomes `SimFixed` by truncation, from its bits, so
    /// the fraction's readers compare against the truncated thresholds
    /// ([`Self::above_tenth`], [`Self::above_eight_tenths`]). A double within
    /// 2^-16 above a threshold would read as not above it. The Jumpjet's speed
    /// is always a whole number k (it steps by the constructor's 2.0 up and
    /// 3.0 down: stock spells `JumpJetAccel=`, which the case-sensitive reader
    /// never sees, and clamps to its integer cap C and 0), so k/C above 0.1 is
    /// at least 0.1 + 1/(10C), outside that window for any C up to 6553, and
    /// above 0.8 for any C up to 13107; k/C exactly 0.1 or 0.8 divides with
    /// truncation to just below it, as native reads it.
    pub fn set_speed_fraction_native_bits(&mut self, bits: u64) {
        use crate::util::native_x87::NativeF64Bits;
        let stored = Self::stored_speed_fraction(NativeF64Bits::from_bits(bits)).bits();
        self.applied_fraction = if stored == NativeF64Bits::ONE.bits() {
            crate::util::fixed_math::SIM_ONE
        } else {
            // 0 <= value < 1: floor(value * 2^16) from the significand.
            let exponent = ((stored >> 52) & 0x7ff) as i32;
            let mantissa = stored & ((1 << 52) - 1);
            let significand = if exponent == 0 {
                mantissa
            } else {
                mantissa | (1 << 52)
            };
            let shift = 1075 - i64::from(exponent.max(1)) - 16;
            SimFixed::from_bits(if shift >= 64 {
                0
            } else {
                (significand >> shift) as i32
            })
        };
    }

    /// Foot `+0x578` above 0.1 (`0x007E3860`): the Infantry fire error's
    /// moving gate (`0x0051C9B8`) and the sequencer's default arm
    /// (`0x00520D45`). 0.1 lies between `SimFixed` raw 6553 and 6554; the
    /// truncated threshold keeps a fraction that truncated to 6553 below it.
    /// Original boundary witnesses: `tools/spatial_oracle/infantry_fire_speed.json`.
    pub(crate) fn above_tenth(&self) -> bool {
        self.applied_fraction > SimFixed::ONE / SimFixed::from_num(10)
    }

    /// Foot `+0x578` above 0.8 (`0x007EB5C8`): the Jumpjet infantryman's Fly
    /// over Hover (`0x0052123C`). floor(0.8 * 2^16) = 52428.
    pub(crate) fn above_eight_tenths(&self) -> bool {
        self.applied_fraction > SimFixed::from_bits(52_428)
    }

    /// Cell48303A..483072: an already-modified Foot never stacks this effect.
    /// Class/radius eligibility belongs to the pickup effect caller.
    pub fn accept_speed_crate(
        &mut self,
        multiplier: crate::util::native_x87::NativeF64Bits,
    ) -> bool {
        use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF64Bits};
        if self.crate_multiplier != NativeF64Bits::ONE {
            return false;
        }
        self.crate_multiplier = X::store_f64_masked_chop(X::mul(
            X::load_f64(self.crate_multiplier),
            X::load_f64(multiplier),
        ));
        true
    }
}

pub use crate::sim::movement::track_process::TrackProgress;

/// Constructor row for a generic AnimClass-like runtime spawn.
///
/// This preserves the fields passed to `AnimClass::Constructor` separately from
/// presentation conveniences such as cached frame count and wall-clock frame
/// delay, so parity-sensitive code can inspect the original constructor surface.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AnimClassSpawnDescriptor {
    /// AnimType/SHP type interned ID.
    pub type_name: InternedId,
    /// World cell containing the constructor coordinate.
    pub rx: u16,
    pub ry: u16,
    /// Sub-cell constructor coordinate in leptons.
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
    /// Height level for the constructor coordinate.
    pub z: u8,
    /// Constructor `delay` argument, in native logic frames.
    pub delay: u16,
    /// Signed constructor `loop` argument.
    pub loop_count: i32,
    /// Constructor draw flags argument.
    pub draw_flags: u32,
    /// Constructor `ZAdjust` argument.
    pub z_adjust: i32,
    /// Constructor reverse argument.
    pub reverse: bool,
    /// AnimClass `+0x196`: draw through the owning cell's palette/light path.
    #[serde(default)]
    pub use_cell_drawer: bool,
    /// AnimClass `+0x197`: marks the instance as terrain-attached.
    #[serde(default)]
    pub terrain_attached: bool,
    /// Instance draw-state bytes supplied by the native producer.
    pub draw_runtime: crate::sim::anim_class::AnimDrawRuntime,
    /// The object the producer attaches the new Anim to right after the
    /// constructor (`AnimClass::SetOwnerObject @ 0x00424B50`), if any.
    #[serde(default)]
    pub owner: Option<u64>,
}

impl AnimClassSpawnDescriptor {
    pub fn new(
        type_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        z: u8,
    ) -> Self {
        Self {
            type_name,
            rx,
            ry,
            sub_x,
            sub_y,
            z,
            delay: 0,
            loop_count: 1,
            draw_flags: 0,
            z_adjust: 0,
            reverse: false,
            use_cell_drawer: false,
            terrain_attached: false,
            draw_runtime: crate::sim::anim_class::AnimDrawRuntime::default(),
            owner: None,
        }
    }
}

/// Test observation of EVERY due dump gate of `UnitClass::Mission_Unload @
/// 0x0073D630` state 3 (`HarvesterDumpRate × 900 <= unit+0xF8`,
/// `0x0073E355..0x0073E374`), including the final gate that finds no cargo.
/// The refinery dock commits the gate's effects itself
/// (`building_anim::begin_refinery_unload_gate`, `end_refinery_unload_empty`):
/// the refinery smoke burst (`BuildingClass` vtable+0x468 → `0x00459900`,
/// fired first on each gate at `0x0073E37E`), SpecialAnim slot 10 only while
/// none is live (`building+0x584 == NULL`, `0x0073E384`), and the cut of the
/// running SpecialAnim on the empty gate (`ClearAnimSlot(0xA)`, `0x0073E530`).
/// The frame tail drops the event.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BaleDepositEvent {
    /// Refinery stable_id where the bale was deposited.
    pub building_id: u64,
    /// Sim tick when this event was emitted (for ordering / debugging).
    pub tick: u64,
    /// One StorageClass slot drained on this gate (credits were paid;
    /// `0x0073E4A2..0x0073E4DA`).
    pub drained: bool,
    /// The gate found no cargo (`FindFirstNonEmptySlot == -1`,
    /// `0x0073E4DC..0x0073E534`): state 3 → 4 and the SpecialAnim is cut.
    pub empty: bool,
}

/// Test observation of the tank-bunker walls rising (install) or falling
/// (teardown). The bunker link sets the wall anim slots itself
/// (`building_anim::set_bunker_wall_slots`); `damaged` is the `…Damaged`
/// variant choice it passed. The frame tail drops the event.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct BunkerWallAnimEvent {
    /// Bunker building stable_id whose walls are animating.
    pub building_id: u64,
    /// `true` = walls rising (install), `false` = walls falling (teardown).
    pub up: bool,
    /// The building was at or below ConditionYellow when the event fired, so
    /// the walls use the `…Damaged` SpecialAnim variant.
    pub damaged: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::movement::DriveLocomotionRuntime;

    #[test]
    fn test_types_are_send_sync() {
        // GameEntity fields must be Send + Sync for future multithreaded sim ticks.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Position>();
        assert_send_sync::<crate::sim::combat::Health>();
        assert_send_sync::<BuildingUp>();
        assert_send_sync::<crate::sim::movement::locomotor::LocomotorState>();
        assert_send_sync::<NavigationState>();
        assert_send_sync::<DriveLocomotionRuntime>();
    }

    /// `FootClass::SetSpeedFraction @ 0x004D3710` executed on the original
    /// bytes: the double it stores for each requested one, signed zeros,
    /// NaNs, infinities and denormals included, and that double's truncation
    /// to `SimFixed`.
    #[test]
    fn speed_fraction_setter_matches_the_original() {
        use crate::util::native_x87::NativeF64Bits;
        let cases: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/foot_speed_owner.json",
        ))
        .unwrap();
        let bits = |row: &serde_json::Value, hex: &str, float: &str| {
            row[hex].as_str().map_or_else(
                || row[float].as_f64().unwrap().to_bits(),
                |hex| u64::from_str_radix(hex, 16).unwrap(),
            )
        };
        let mut setter_rows = 0;
        for case in &cases {
            let requested = bits(&case["input"], "requested_bits", "requested");
            let stored = bits(&case["output"], "applied_bits", "applied");
            let requested_double = NativeF64Bits::from_bits(requested);
            assert_eq!(
                FootSpeedState::stored_speed_fraction(requested_double).bits(),
                stored,
                "{case}"
            );
            let mut speed = FootSpeedState::default();
            speed.set_speed_fraction_native_bits(requested);
            // The stored double lies in [0, 1], so scaling by 2^16 is exact.
            let truncated = SimFixed::from_bits((f64::from_bits(stored) * 65536.0) as i32);
            assert_eq!(speed.applied_fraction(), truncated, "{case}");
            setter_rows += usize::from(case["input"]["family"] == "setter");
        }
        assert_eq!((cases.len(), setter_rows), (25, 13));
    }

    #[test]
    fn drive_locomotion_hash_changes_when_runtime_state_changes() {
        use std::hash::{Hash, Hasher};

        fn hash_drive(drive: &DriveLocomotionRuntime) -> u64 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            drive.hash(&mut hasher);
            hasher.finish()
        }

        let drive_a = DriveLocomotionRuntime::default();
        let drive_b = DriveLocomotionRuntime::default()
            .with_destination_for_test(Some(DriveCoord::cell(45, 40, 0)))
            .with_track_for_test(TrackProgress::for_test(-1, -1, false, 6));

        assert_ne!(hash_drive(&drive_a), hash_drive(&drive_b));
    }
}

/// The direction word of one route step; a non-adjacent step is the tube
/// direction.
fn direction_from_step(from: (i16, i16), to: (u16, u16)) -> u8 {
    let dx = i32::from(to.0 as i16) - i32::from(from.0);
    let dy = i32::from(to.1 as i16) - i32::from(from.1);
    crate::util::direction::direction_from_delta(dx, dy)
        .unwrap_or(crate::util::direction::TUBE_STEP_DIRECTION)
}
