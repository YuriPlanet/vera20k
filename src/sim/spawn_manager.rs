//! `SpawnManagerClass` — the sub-unit pool carried by V3 Launcher,
//! Dreadnought, Boomer, Aircraft Carrier and Destroyer.
//!
//! A techno whose type sets `Spawns=` owns a fixed pool of children. Each slot
//! holds one child plus a small state machine; the manager itself has a second,
//! three-state machine that decides when the wing launches and when it comes
//! home. The manager issues only high-level orders (unlimbo, target, mission,
//! limbo) — flight, firing and death belong to the children's own systems.
//!
//! The parent never fires a real bullet at the target: its `Spawner=yes` weapon
//! short-circuits in the fire path and only hands the target to this manager
//! (`SpawnManagerClass::SetTarget`). Without this module those units carry a
//! `Damage=1` rangefinder weapon and nothing else, which is why a V3 Launcher
//! or Dreadnought is combat-inert until the manager exists.
//!
//! ## The original's bodies
//!
//! `SpawnManagerClass::AI @ 0x006B7230`, `PointerExpired @ 0x006B7C60`,
//! `ClearAllTargets @ 0x006B7BB0` and `Kill_All_Spawns @ 0x006B7100` are
//! ported whole over a [`SpawnHost`]: the manager's own fields and the order
//! of every call the original makes on its owner, its children and the
//! kamikaze tracker. Like the original, which re-reads `this` after each
//! call, the bodies fetch the manager from the host on every access, so a
//! PointerExpired broadcast that a call sets off (a child's Limbo or UnInit)
//! acts on the same state.
//!
//! - AI runs from its owner's `TechnoClass::AI` (`0x006FA94C..0x006FA958`,
//!   `if (+0x2D0) vt+0x5C()`), after the mission dispatch, the passive scan,
//!   the bomb, the SlaveManager and CaptureManager, the IsAlive gate
//!   (`0x006FA735`) and the cloak (`vt+0x410`, `0x006FA946`): see
//!   `world::techno_ai`. A Unit's own Fire_At_Target (`0x007365E1`) follows
//!   its `FootClass::AI` (`0x0073647B`), so the target a V3 hands its manager
//!   when it fires waits for the next AI pass.
//! - AI self-gates on an update timer: 20 frames from construction, 10
//!   thereafter. All slot work and the manager status run only then.
//! - `CountAliveSpawns` (`0x006B7D30`) counts every slot whose state is not
//!   `Regenerating`. The parent's fire gate uses it.
//! - The manager makes **two** separate missile tests. The per-slot
//!   `IsMissileSpawn` flag (`SpawnControl+0x14`) compares the pool's type
//!   against `[General] V3RocketType/DMislType/CMislType` (see
//!   `rules::missile_spawn`); the Launching pass and ClearAllTargets read the
//!   **child type's own `MissileSpawn=`** (`+0xD68`). They coincide in stock
//!   YR; both are kept because a mod can separate them.
//!
//! Native comparison: `tools/rocket_oracle/spawn_manager.py` runs the five
//! bodies (with SetTarget `0x006B7B90`) over scripted V3, Dreadnought, Boomer
//! and Carrier pools; `spawn_manager_oracle_tests` replays every step through
//! these bodies and compares the call log and the manager, its nodes, the
//! kamikaze timer, the owner's burst index and the children after each step.
//! The launch coordinate is `tools/projectile_oracle/ifv_fire_coord.json`'s
//! (`launch_coordinate`).
//!
//! ## `Spawned=`
//!
//! `Spawned=` (`TechnoTypeClass+0xD54`, `ObjectType::spawned`) has four
//! verified readers — `search_instructions operand_pattern=0xd54]` finds
//! `AircraftClass::Is_Cell_Free_For_Landing` (two sites),
//! `TechnoClass::GetFireError` (`0x006FC67B`, T36 in `combat::fire_error`),
//! `TechnoClass::Set_ArchiveTarget` and `TechnoClass::IsIdleForAutoTarget`.
//! Only GetFireError's is ported; the other three are not this slice's.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/world, sim/combat, sim/movement, rules/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::sim::world::FrameEffects;
use serde::{Deserialize, Serialize};

use crate::rules::missile_spawn::MissileFamily;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::timer::CdTimer;
use crate::sim::world::{Simulation, UninitContext};
use crate::util::direction::DIRECTION_DELTAS;

#[cfg(test)]
#[path = "spawn_manager_oracle_tests.rs"]
mod oracle_tests;
#[cfg(test)]
#[path = "spawn_manager_tests.rs"]
mod tests;

/// A spawned child's SpawnOwner, `TechnoClass+0x2D4`: the object whose
/// spawn manager holds the child. The manager's launch sets it
/// (`0x006B7947`); `TechnoClass::PointerExpired` drops it when that owner
/// expires, whatever the control (`0x007077EE..0x007077FD`,
/// [`Self::forget`]). Kill credit and the "do not self-RTB" gate read it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SpawnOwner {
    owner: Option<u64>,
}

impl SpawnOwner {
    /// The owner's stable id.
    pub(crate) const fn owner(self) -> Option<u64> {
        self.owner
    }

    /// PointerExpired's drop of an expiring owner.
    pub(crate) fn forget(&mut self) {
        self.owner = None;
    }

    /// Install a fixture's owner.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_for_test(&mut self, owner: Option<u64>) {
        self.owner = owner;
    }
}

/// Frames the manager waits before its very first AI pass
/// (`UpdateTimer.Duration = 0x14` at construction).
const FIRST_UPDATE_DELAY_FRAMES: i32 = 20;
/// Frames between AI passes once the manager has run (`0x006B7260`).
const UPDATE_PERIOD_FRAMES: i32 = 10;
/// The manager's SpawnTimer after a launch when the *owner* type does not set
/// `MissileSpawn=` (`0x006B73B9`). No stock YR parent sets it.
const LAUNCH_DELAY_FRAMES: i32 = 20;
/// The SpawnTimer when the owner type sets `MissileSpawn=yes` (`0x006B73A8`).
const LAUNCH_DELAY_FRAMES_MISSILE_PARENT: i32 = 9;
/// Height difference (leptons) under which a returning child counts as docked
/// (`0x006B77FA`).
const DOCK_HEIGHT_EPSILON_LEPTONS: i32 = 0x14;
/// Leptons a launch adds to its GetFLH Z (`ADD EAX,0xa` at `0x006B74B2`).
const LAUNCH_Z_LIFT_LEPTONS: i32 = 10;
/// X and Y a `CMislType=` launch takes off its GetFLH coordinate: the dwords
/// at `0x0084009C` and `0x008400A0` (`0x006B74C4`, `0x006B74CA`), 40 and 40 in
/// the retail bytes. Those two reads are their only references.
const CMISL_LAUNCH_OFFSET_LEPTONS: [i32; 2] = [40, 40];
/// Adjacent-cell direction of the cell a launched aircraft holds (`0x006B75F0`).
const ADJACENT_DIR_ON_LAUNCH: u8 = 0;
/// Adjacent-cell direction a waiting aircraft holds (`0x006B76BE`).
const ADJACENT_DIR_WHILE_HELD: u8 = 4;

/// Per-slot state. Native uses 0..7 with no case 5; the gap is preserved by
/// simply not having a variant for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpawnSlotState {
    /// 0 — child sits in limbo on the parent, ready to launch.
    ReadyDocked,
    /// 1 — missile has been sent at the target; the slot waits out
    /// `PauseFrames + TiltFrames` before starting to regenerate.
    KamikazeWait,
    /// 2 — child is out in the world.
    InFlight,
    /// 3 — aircraft child is attacking.
    Attacking,
    /// 4 — aircraft child is coming home to the parent.
    ComingHome,
    /// 6 — child is docked in limbo, reloading.
    Reloading,
    /// 7 — slot has no child; rebuilding one after `SpawnRegenRate` frames.
    Regenerating,
}

/// One pool slot (native `SpawnControl`, 0x18 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpawnSlot {
    /// Stable id of the child, or `None` while regenerating.
    spawn: Option<u64>,
    state: SpawnSlotState,
    /// The slot's timer (`+0x08`): the kamikaze wait, the reload and the
    /// regeneration. A ready slot keeps the timer it last ran out.
    timer: CdTimer,
    /// `+0x14`: set when the pool's child type is one of the three hardcoded
    /// rocket families. Drives the launch's owner gates and the kamikaze path.
    is_missile_spawn: bool,
}

impl SpawnSlot {
    /// The child's stable id.
    pub fn spawn(&self) -> Option<u64> {
        self.spawn
    }

    /// The slot's state.
    pub fn state(&self) -> SpawnSlotState {
        self.state
    }

    /// The slot's timer.
    pub fn timer(&self) -> CdTimer {
        self.timer
    }

    /// The pool's child is a rocket.
    pub fn is_missile_spawn(&self) -> bool {
        self.is_missile_spawn
    }

    /// A fixture's slot.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(
        spawn: Option<u64>,
        state: SpawnSlotState,
        timer: CdTimer,
        is_missile_spawn: bool,
    ) -> Self {
        Self {
            spawn,
            state,
            timer,
            is_missile_spawn,
        }
    }

    /// Install a fixture's child.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_spawn_for_test(&mut self, spawn: Option<u64>) {
        self.spawn = spawn;
    }

    /// Install a fixture's state.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_state_for_test(&mut self, state: SpawnSlotState) {
        self.state = state;
    }
}

/// Manager-level machine (native `ManagerMode` at +0x70).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpawnManagerMode {
    /// 0 — no target; nothing launches.
    Idle,
    /// 1 — a target is live and slots are being pushed out.
    Launching,
    /// 2 — everything is out; wait for the wing to come home.
    Returning,
}

/// Per-parent spawn pool state (native `SpawnManagerClass`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpawnManagerState {
    /// Interned `Spawns=` child type name.
    spawn_type: InternedId,
    /// Which hardcoded rocket family the child belongs to, if any.
    missile_family: Option<MissileFamily>,
    /// `SpawnRegenRate=` in frames.
    regen_rate: u32,
    /// `SpawnReloadRate=` in frames.
    reload_rate: u32,
    /// `PauseFrames + TiltFrames` for this pool's missile family, cached at
    /// construction so the per-tick machine never needs the RuleSet. Zero for
    /// aircraft pools, which never enter `KamikazeWait`.
    kamikaze_wait_frames: u32,
    slots: Vec<SpawnSlot>,
    /// Gates the whole AI pass (20 frames, then 10).
    update_timer: CdTimer,
    /// The manager's SpawnTimer (`+0x5C`): spaces launches across the pool.
    spawn_timer: CdTimer,
    current_target: Option<TargetKind>,
    queued_target: Option<TargetKind>,
    mode: SpawnManagerMode,
}

impl SpawnManagerState {
    /// The interned `Spawns=` child type.
    pub fn spawn_type(&self) -> InternedId {
        self.spawn_type
    }

    /// `SpawnRegenRate=` in frames.
    pub fn regen_rate(&self) -> u32 {
        self.regen_rate
    }

    /// `SpawnReloadRate=` in frames.
    pub fn reload_rate(&self) -> u32 {
        self.reload_rate
    }

    /// The pool's slots.
    pub fn slots(&self) -> &[SpawnSlot] {
        &self.slots
    }

    /// The AI pass's gate.
    pub fn update_timer(&self) -> CdTimer {
        self.update_timer
    }

    /// The SpawnTimer (`+0x5C`).
    pub fn spawn_timer(&self) -> CdTimer {
        self.spawn_timer
    }

    /// The live target.
    pub fn current_target(&self) -> Option<TargetKind> {
        self.current_target
    }

    /// The target SetTarget queued for the next AI pass.
    pub fn queued_target(&self) -> Option<TargetKind> {
        self.queued_target
    }

    /// The objects whose expiry [`pointer_expired`] acts on besides its
    /// owner's own: the live and queued target and each slot's child. The
    /// pointer-expiry index (`ExpiryListeners`) reaches the owner by these.
    pub(crate) fn expiry_names(&self) -> impl Iterator<Item = u64> + '_ {
        [self.current_target, self.queued_target]
            .into_iter()
            .filter_map(|target| match target {
                Some(TargetKind::Entity(id)) => Some(id),
                Some(TargetKind::Cell(..)) | None => None,
            })
            .chain(self.slots.iter().filter_map(SpawnSlot::spawn))
    }

    /// The ManagerMode (`+0x70`).
    pub fn mode(&self) -> SpawnManagerMode {
        self.mode
    }

    /// A fixture's idle pool of `slots` with zero rates and timers.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(spawn_type: InternedId, slots: Vec<SpawnSlot>) -> Self {
        Self {
            spawn_type,
            missile_family: None,
            regen_rate: 0,
            reload_rate: 0,
            kamikaze_wait_frames: 0,
            slots,
            update_timer: CdTimer::default(),
            spawn_timer: CdTimer::default(),
            current_target: None,
            queued_target: None,
            mode: SpawnManagerMode::Idle,
        }
    }

    /// Install a fixture's rates.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_rates_for_test(&mut self, regen_rate: u32, reload_rate: u32) {
        self.regen_rate = regen_rate;
        self.reload_rate = reload_rate;
    }

    /// Install a fixture's AI gate.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_update_timer_for_test(&mut self, update_timer: CdTimer) {
        self.update_timer = update_timer;
    }

    /// Install a fixture's current target.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_current_target_for_test(&mut self, current_target: Option<TargetKind>) {
        self.current_target = current_target;
    }

    /// Install a fixture's queued target.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_queued_target_for_test(&mut self, queued_target: Option<TargetKind>) {
        self.queued_target = queued_target;
    }

    /// Install a fixture's mode.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_mode_for_test(&mut self, mode: SpawnManagerMode) {
        self.mode = mode;
    }

    /// A fixture's slots.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slots_mut_for_test(&mut self) -> &mut Vec<SpawnSlot> {
        &mut self.slots
    }

    /// Slots whose state is not `Regenerating` — the native
    /// `CountAliveSpawns`. The parent's `Spawner=yes` fire gate uses this.
    pub fn count_alive_spawns(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.state != SpawnSlotState::Regenerating)
            .count()
    }

    /// Slots physically docked on the parent. Native
    /// `SpawnManagerClass::CountDockedSpawns` (`0x006B7D50`) counts only
    /// states 0 (`ReadyDocked`) and 6 (`Reloading`). `NoSpawnAlt` queries this
    /// at draw time; [`Self::count_alive_spawns`] belongs to the fire gate.
    pub fn count_docked_spawns(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| {
                matches!(
                    slot.state,
                    SpawnSlotState::ReadyDocked | SpawnSlotState::Reloading
                )
            })
            .count()
    }

    /// Native `0x006B7D80`, read by the Drive/Ship Process_Movement gate
    /// (0x4B272C / 0x6A1D49) before its no-queue path request: slots waiting
    /// out a kamikaze launch (state 1), plus in-flight children (state 2)
    /// outside limbo (+81) whose own type sets `MissileSpawn=` (+D68).
    pub(crate) fn count_launched_missiles(
        &self,
        entities: &crate::sim::entity_store::EntityStore,
        rules: &RuleSet,
        interner: &crate::sim::intern::StringInterner,
    ) -> usize {
        self.slots
            .iter()
            .filter(|slot| match slot.state {
                SpawnSlotState::KamikazeWait => true,
                SpawnSlotState::InFlight => slot
                    .spawn
                    .and_then(|id| entities.get(id))
                    .filter(|child| !child.lifecycle.in_limbo())
                    .and_then(|child| interner.object_type(child.type_ref(), rules))
                    .is_some_and(|kind| kind.missile_spawn),
                _ => false,
            })
            .count()
    }

    /// `SpawnManagerClass::SetTarget` (`0x006B7B90`): a target that differs
    /// from the live one is queued, never written straight through. The next AI
    /// pass promotes it.
    pub fn set_target(&mut self, target: Option<TargetKind>) {
        if target != self.current_target {
            self.queued_target = target;
        }
    }

    /// Promote the queued target, as every state branch does before reading
    /// `current_target`: `if (NewTarget) { Target = NewTarget; NewTarget = 0; }`.
    fn promote_queued_target(&mut self) -> Option<TargetKind> {
        if self.queued_target.is_some() {
            self.current_target = self.queued_target.take();
        }
        self.current_target
    }
}

/// Build the manager state for a freshly constructed parent.
///
/// Mirrors `TechnoClass::Init_Managers` (`0x006F3FF4`): the manager exists iff
/// the type's `Spawns=` resolves to a real object type. The slot vector is
/// created here; the world-owned constructor transaction materialises every
/// child before the parent attempts Unlimbo. The constructor
/// (`0x006B6C90`) writes every node timer and the SpawnTimer as
/// `{Frame, 0}` and the UpdateTimer as `{Frame, 20}`.
pub fn init_spawn_manager(
    obj: &crate::rules::object_type::ObjectType,
    rules: &RuleSet,
    interner: &mut crate::sim::intern::StringInterner,
    frame: u32,
) -> Option<SpawnManagerState> {
    let spawn_type_name = obj.spawns.as_deref()?;
    if obj.spawns_number <= 0 {
        return None;
    }
    // Native resolves `Spawns=` through the TechnoType registry; an unresolved
    // name leaves the pointer null and no manager is created.
    rules.object(spawn_type_name)?;

    let missile_family = rules.missile_spawn.family_of(spawn_type_name);
    let is_missile_spawn = missile_family.is_some();
    let frame = frame as i32;
    let slots = (0..obj.spawns_number.max(0) as usize)
        .map(|_| SpawnSlot {
            spawn: None,
            // Slots enter Regenerating so the world-owned constructor
            // transaction can fill them (`commit_spawn_manager_pool`). Native
            // fills them in the manager constructor.
            state: SpawnSlotState::Regenerating,
            timer: CdTimer::started(frame, 0),
            is_missile_spawn,
        })
        .collect();

    Some(SpawnManagerState {
        spawn_type: interner.intern(spawn_type_name),
        missile_family,
        regen_rate: obj.spawn_regen_rate,
        reload_rate: obj.spawn_reload_rate,
        kamikaze_wait_frames: missile_family
            .map(|family| rules.missile_spawn.kamikaze_wait_frames(family))
            .unwrap_or(0),
        slots,
        update_timer: CdTimer::started(frame, FIRST_UPDATE_DELAY_FRAMES),
        spawn_timer: CdTimer::started(frame, 0),
        current_target: None,
        queued_target: None,
        mode: SpawnManagerMode::Idle,
    })
}

// ---------------------------------------------------------------------------
// The original's bodies
// ---------------------------------------------------------------------------

/// Where the manager sends a child (`Assign_Destination(dest, 1)`, vt+0x480).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpawnDestination {
    /// The manager's target, or NULL.
    Target(Option<TargetKind>),
    /// `Map[owner GetMapCoords]->Adjacent_Cell(direction)` (`0x00481810`).
    Adjacent(u8),
    /// The owner itself.
    Owner,
}

/// What the manager's bodies reach outside the manager: its owner, its
/// children and the kamikaze tracker. Queries do not change the world.
/// It supplies the reads of the `SpawnManagerClass` bodies: AI (`0x006B7230`), the
/// constructor (`0x006B6C90`), PointerExpired (`0x006B7C60`), ClearAllTargets
/// (`0x006B7BB0`) and Kill_All_Spawns (`0x006B7100`).
trait SpawnHost {
    /// The frame counter (`0x00A8ED84`).
    fn frame(&self) -> i32;
    /// The manager, fetched afresh on every call.
    fn manager(&mut self) -> &mut SpawnManagerState;
    /// The manager for a read, which leaves the owner untouched: the per-frame
    /// UpdateTimer gate and every PointerExpired broadcast read it first.
    fn manager_ref(&self) -> &SpawnManagerState;
    fn is_owner(&self, id: u64) -> bool;
    /// The pool's type is Rules' `CMislType=` (`0x006B74BC`, `0x006B7513`).
    fn pool_is_cmisl(&self) -> bool;
    /// The owner's locomotor Is_Moving (ILocomotion `+0x10`, `0x006B731E`).
    fn owner_is_moving(&mut self) -> bool;
    /// The owner's locomotor Is_Moving_Now (ILocomotion `+0x80`, `0x006B7349`).
    fn owner_is_moving_now(&mut self) -> bool;
    /// A Foot owner's locomotor-swap latch (`+0x6AD`, `0x006B737C`).
    fn owner_locomotor_swap(&self) -> bool;
    /// The owner type's `MissileSpawn=` (`+0xD68`, `0x006B7392`).
    fn owner_missile_spawn(&self) -> bool;
    /// `Burst=` of the owner's GetWeapon(0) (vt+0x3F8), `None` without a
    /// WeaponType (`0x006B73DC..0x006B73E2`).
    fn owner_weapon_burst(&self) -> Option<i32>;
    /// The owner's Health (`+0x6C`).
    fn owner_health(&self) -> i32;
    /// The owner's IsAlive (`+0x90`).
    fn owner_is_alive(&self) -> bool;
    /// The owner's GetMapCoords (vt+0x1B8).
    fn owner_cell(&self) -> (i32, i32);
    /// The owner's CanFireAtTarget (vt+0x3AC, `0x006B7B43`).
    fn can_fire_at(&mut self, target: TargetKind) -> bool;
    /// The child's Ammo (`+0x2FC`).
    fn child_ammo(&self, child: u64) -> i32;
    /// The child type's `MissileSpawn=` (`+0x6C4 -> +0xD68`).
    fn child_missile_spawn(&self, child: u64) -> bool;
    /// The child's Health (`+0x6C`).
    fn child_health(&self, child: u64) -> i32;
    /// The child is on the kamikaze tracker (`+0x6CA`).
    fn child_tracked(&self, child: u64) -> bool;
    /// The child's GetMapCoords (vt+0x1B8).
    fn child_cell(&self, child: u64) -> (i32, i32);
    /// The child's Location Z less the owner's (`+0xA4`, `0x006B77F7`).
    fn child_height_over_owner(&self, child: u64) -> i32;
    /// Case 0's launch (`0x006B73FC..0x006B7505`): GetFLH with the owner's
    /// CurrentBurstIndex (`+0x3B8`), which a burst launch sets to the slot's
    /// parity first, then the child's Unlimbo (vt+0xD8) at that coordinate
    /// facing the owner.
    fn launch(&mut self, child: u64, burst_parity: Option<i32>);
    /// The owner's CurrentBurstIndex back to 0 after a burst launch
    /// (`0x006B757A..0x006B7585`).
    fn reset_owner_burst(&mut self);
    /// A `CMislType=` pool's `V3TAKOFF` at the launched child (`0x006B751B..`).
    fn takeoff_anim(&mut self, child: u64);
    fn set_destination(&mut self, child: u64, destination: SpawnDestination);
    /// `Queue_Mission(mission, 0)` (vt+0x1E8).
    fn queue_mission(&mut self, child: u64, mission: MissionType);
    /// `Assign_Target` (vt+0x3C8).
    fn assign_target(&mut self, child: u64, target: Option<TargetKind>);
    /// `Limbo` (vt+0xD4, `FootClass::Limbo`).
    fn limbo(&mut self, child: u64);
    /// Case 6's rearm: Ammo from the type's `Ammo=`, Health and the estimate
    /// from its `Strength=` (`0x006B788F..0x006B78AA`).
    fn refill(&mut self, child: u64);
    /// Case 7's `CreateObject(owner's house)` (type vt+0x8C).
    fn create_child(&mut self) -> Option<u64>;
    /// The child's SpawnOwner (`+0x2D4`) set to the owner (`0x006B7947`).
    fn adopt(&mut self, child: u64);
    /// `UnInit` (vt+0xF8).
    fn uninit(&mut self, child: u64);
    /// The kamikaze tracker's Push (`0x0054E3B0`).
    fn push(&mut self, child: u64, target: Option<TargetKind>);
    /// The callers' `{Frame, 2}` restart of the tracker's timer (`0x00ABC5F8`).
    fn restart_kamikaze(&mut self);
    /// The kamikaze tracker's Remove (`0x0054E590`).
    fn kamikaze_remove(&mut self, child: u64);
}

/// `SpawnManagerClass::AI @ 0x006B7230`.
fn ai(host: &mut impl SpawnHost) {
    let frame = host.frame();
    if !host.manager_ref().update_timer.expired(frame) {
        return;
    }
    host.manager().update_timer = CdTimer::started(frame, UPDATE_PERIOD_FRAMES);
    let mut index = 0;
    while index < host.manager_ref().slots.len() {
        visit_slot(host, index, frame);
        index += 1;
    }
    manager_status(host, frame);
}

/// The per-node switch (jump table `0x006B7B6C`).
fn visit_slot(host: &mut impl SpawnHost, index: usize, frame: i32) {
    use SpawnSlotState::*;
    let slot = host.manager_ref().slots[index].clone();
    let Some(child) = slot.spawn else {
        // Only a regenerating node is without a unit.
        if slot.state == Regenerating && slot.timer.expired(frame) {
            fill_slot(host, index);
        }
        return;
    };
    match slot.state {
        ReadyDocked => launch_slot(host, index, child, frame),
        KamikazeWait => {
            if slot.timer.expired(frame) {
                pointer_expired(host, child);
            }
        }
        InFlight => {
            if slot.is_missile_spawn {
                return;
            }
            match host.manager().promote_queued_target() {
                None => {
                    recall(host, child);
                    host.manager().slots[index].state = ComingHome;
                }
                Some(_) => {
                    host.set_destination(
                        child,
                        SpawnDestination::Adjacent(ADJACENT_DIR_WHILE_HELD),
                    );
                    host.queue_mission(child, MissionType::Move);
                }
            }
        }
        Attacking => {
            let target = host.manager().promote_queued_target();
            match target.filter(|_| host.child_ammo(child) != 0) {
                Some(target) => {
                    host.assign_target(child, Some(target));
                    host.queue_mission(child, MissionType::Attack);
                }
                None => {
                    recall(host, child);
                    host.manager().slots[index].state = ComingHome;
                }
            }
        }
        ComingHome => {
            let target = host.manager().promote_queued_target();
            match target.filter(|_| host.child_ammo(child) >= 1) {
                Some(target) => {
                    host.manager().slots[index].state = Attacking;
                    host.assign_target(child, Some(target));
                    host.queue_mission(child, MissionType::Attack);
                }
                None if host.child_cell(child) == host.owner_cell()
                    && host.child_height_over_owner(child) < DOCK_HEIGHT_EPSILON_LEPTONS =>
                {
                    host.limbo(child);
                    let manager = host.manager();
                    let reload = manager.reload_rate as i32;
                    let slot = &mut manager.slots[index];
                    slot.state = Reloading;
                    slot.timer = CdTimer::started(frame, reload);
                }
                None => recall(host, child),
            }
        }
        Reloading => {
            if slot.timer.expired(frame) {
                host.manager().slots[index].state = ReadyDocked;
                host.refill(child);
            }
        }
        Regenerating => {
            if slot.timer.expired(frame) {
                fill_slot(host, index);
            }
        }
    }
}

/// Case 0 (`0x006B72A2..0x006B760E`): with a target, the SpawnTimer run out
/// and the manager not Returning, the child launches.
fn launch_slot(host: &mut impl SpawnHost, index: usize, child: u64, frame: i32) {
    let manager = host.manager_ref();
    let missile = manager.slots[index].is_missile_spawn;
    if manager.current_target.is_none()
        || !manager.spawn_timer.expired(frame)
        || manager.mode == SpawnManagerMode::Returning
    {
        return;
    }
    // HouseClass::IonSensitivesShouldBeOffline (`0x0053A130`) answers false.
    if missile && (host.owner_is_moving() || host.owner_is_moving_now()) {
        return;
    }
    if host.owner_locomotor_swap() {
        return;
    }
    let delay = if host.owner_missile_spawn() {
        LAUNCH_DELAY_FRAMES_MISSILE_PARENT
    } else {
        LAUNCH_DELAY_FRAMES
    };
    host.manager().spawn_timer = CdTimer::started(frame, delay);
    let parity = (missile && host.owner_weapon_burst().is_some_and(|burst| burst > 1))
        .then_some((index & 1) as i32);
    host.manager().slots[index].state = SpawnSlotState::InFlight;
    host.launch(child, parity);
    if host.pool_is_cmisl() {
        host.takeoff_anim(child);
    }
    if parity.is_some() {
        host.reset_owner_burst();
    }
    if host.manager_ref().slots[index].is_missile_spawn {
        let target = host.manager().promote_queued_target();
        host.set_destination(child, SpawnDestination::Target(target));
    } else {
        host.set_destination(child, SpawnDestination::Adjacent(ADJACENT_DIR_ON_LAUNCH));
    }
    host.queue_mission(child, MissionType::Move);
}

/// Case 7's body (`0x006B78D3..0x006B7953`), also the constructor's
/// (`0x006B6D3C..0x006B6DA1`): a new child in limbo, flagged by the pool type,
/// with the owner as its SpawnOwner.
fn fill_slot(host: &mut impl SpawnHost, index: usize) {
    // RESIDUAL: case 7 assumes CreateObject succeeds and the constructor
    // leaves out a node whose CreateObject fails (`0x006B6D5A`); a VERA
    // construction that fails leaves the slot regenerating for the next pass.
    // Trigger: a `Spawns=` type VERA cannot construct (none in retail).
    let Some(child) = host.create_child() else {
        return;
    };
    let manager = host.manager();
    let missile = manager.missile_family.is_some();
    let slot = &mut manager.slots[index];
    slot.spawn = Some(child);
    slot.is_missile_spawn = missile;
    host.limbo(child);
    host.adopt(child);
    host.manager().slots[index].state = SpawnSlotState::ReadyDocked;
}

/// `Assign_Destination(owner, 1)`, `Assign_Target(NULL)` and
/// `Queue_Mission(Move, 0)` (`0x006B7663..0x006B7687`, `0x006B7838..`).
fn recall(host: &mut impl SpawnHost, child: u64) {
    host.set_destination(child, SpawnDestination::Owner);
    host.assign_target(child, None);
    host.queue_mission(child, MissionType::Move);
}

/// The manager status after the node walk (`0x006B796A..`).
fn manager_status(host: &mut impl SpawnHost, frame: i32) {
    use SpawnSlotState::*;
    match host.manager_ref().mode {
        SpawnManagerMode::Idle => {
            let Some(target) = host.manager().promote_queued_target() else {
                return;
            };
            if !host.can_fire_at(target) {
                clear_all_targets(host);
                return;
            }
            host.manager().mode = SpawnManagerMode::Launching;
        }
        SpawnManagerMode::Launching => {
            let manager = host.manager_ref();
            if manager.current_target.is_none() {
                clear_all_targets(host);
                return;
            }
            if !manager
                .slots
                .iter()
                .all(|slot| matches!(slot.state, InFlight | Regenerating))
            {
                return;
            }
            let mut pushed = false;
            let mut index = 0;
            while index < host.manager_ref().slots.len() {
                let slot = host.manager_ref().slots[index].clone();
                index += 1;
                let Some(child) = slot.spawn.filter(|_| slot.state == InFlight) else {
                    continue;
                };
                let target = host.manager_ref().current_target;
                if !host.child_missile_spawn(child) {
                    host.manager().slots[index - 1].state = Attacking;
                    host.assign_target(child, target);
                    host.queue_mission(child, MissionType::Attack);
                    continue;
                }
                pushed = true;
                host.push(child, target);
                host.restart_kamikaze();
                let manager = host.manager();
                if manager.slots[index - 1].is_missile_spawn {
                    let wait = manager.kamikaze_wait_frames as i32;
                    let slot = &mut manager.slots[index - 1];
                    slot.state = KamikazeWait;
                    slot.timer = CdTimer::started(frame, wait);
                } else {
                    pointer_expired(host, child);
                }
            }
            if pushed {
                clear_all_targets(host);
            }
            host.manager().mode = SpawnManagerMode::Returning;
        }
        SpawnManagerMode::Returning => {
            let manager = host.manager();
            if !manager
                .slots
                .iter()
                .any(|slot| matches!(slot.state, Attacking | ComingHome))
            {
                manager.mode = SpawnManagerMode::Idle;
            }
        }
    }
}

/// `SpawnManagerClass::PointerExpired @ 0x006B7C60`, an else-if chain:
///
/// ```text
/// if      (expired == Target)    { Target = 0; if (!NewTarget) ClearAllTargets(); }
/// else if (expired == NewTarget) { NewTarget = 0; }
/// else if (expired is a slot's unit, walked from the last slot)
///         { unless the unit has Health, is off the tracker and the slot is
///           not a missile slot: Unit = 0, state 7, timer {Frame, RegenRate} }
/// else if (expired == Owner)     { Kill_All_Spawns(); ClearAllTargets(); }
/// ```
///
/// The target arm is the only thing in the engine that drops a destroyed
/// target: SetTarget queues, and the promote never promotes NULL.
///
/// The slot guard (`0x006B7CDD..0x006B7CF2`) exists because Limbo broadcasts
/// (`0x005F4D61`): a docking Hornet's Limbo would otherwise expire its own
/// slot and strand it in limbo.
///
/// Every id compared here but the owner's is named by
/// [`SpawnManagerState::expiry_names`].
fn pointer_expired(host: &mut impl SpawnHost, expired: u64) {
    let frame = host.frame();
    let expired_target = Some(TargetKind::Entity(expired));
    let manager = host.manager_ref();
    if manager.current_target == expired_target {
        let manager = host.manager();
        manager.current_target = None;
        if manager.queued_target.is_none() {
            clear_all_targets(host);
        }
        return;
    }
    if manager.queued_target == expired_target {
        host.manager().queued_target = None;
        return;
    }
    if let Some(index) = manager
        .slots
        .iter()
        .rposition(|slot| slot.spawn == Some(expired))
    {
        let missile = manager.slots[index].is_missile_spawn;
        if host.child_health(expired) > 0 && !host.child_tracked(expired) && !missile {
            return;
        }
        let manager = host.manager();
        let regen = manager.regen_rate as i32;
        let slot = &mut manager.slots[index];
        slot.spawn = None;
        slot.state = SpawnSlotState::Regenerating;
        slot.timer = CdTimer::started(frame, regen);
        return;
    }
    if host.is_owner(expired) {
        kill_all_spawns(host);
        clear_all_targets(host);
    }
}

/// `SpawnManagerClass::ClearAllTargets @ 0x006B7BB0`: each slot in state 2
/// whose child's type sets `MissileSpawn=` goes to the kamikaze tracker with
/// the target (NULL when PointerExpired's target arm just dropped it, so the
/// missile takes the cell ahead of its facing), the tracker restarts for 2
/// frames and PointerExpired frees the slot; then the status and both
/// targets clear.
///
/// A Dreadnought or Boomer slot is still in state 2 while the manager waits
/// out the inter-launch delay, so a target that dies then frees the slot at
/// once and its missile flies on under the tracker.
fn clear_all_targets(host: &mut impl SpawnHost) {
    let mut index = 0;
    while index < host.manager_ref().slots.len() {
        let slot = host.manager_ref().slots[index].clone();
        index += 1;
        let Some(child) = slot
            .spawn
            .filter(|_| slot.state == SpawnSlotState::InFlight)
        else {
            continue;
        };
        if !host.child_missile_spawn(child) {
            continue;
        }
        let target = host.manager_ref().current_target;
        host.push(child, target);
        host.restart_kamikaze();
        if let Some(child) = host.manager_ref().slots[index - 1].spawn {
            pointer_expired(host, child);
        }
    }
    let manager = host.manager();
    manager.mode = SpawnManagerMode::Idle;
    manager.queued_target = None;
    manager.current_target = None;
}

/// `SpawnManagerClass::Kill_All_Spawns @ 0x006B7100`: from the last slot,
/// every slot not regenerating becomes state 7 with no child and the timer
/// `{Frame, duration}`, where the duration is 0 for a live owner (Health and
/// IsAlive) and `SpawnRegenRate` otherwise. Before that:
///
/// - **Docked or reloading** (0, 6): the child UnInits.
/// - **KamikazeWait** (1): the missile leaves the tracker (Remove) and
///   UnInits — the salvo dies with its launcher.
/// - **Out** (2, 3, 4): the kamikaze tracker's Push with the target, without
///   the restart; Push crashes a child whose type is not `MissileSpawn=`
///   (a sunk Carrier's Hornets fall like shot-down aircraft).
///
/// It never touches the targets; the PointerExpired owner arm pairs it with
/// ClearAllTargets.
fn kill_all_spawns(host: &mut impl SpawnHost) {
    use SpawnSlotState::*;
    let frame = host.frame();
    let duration = if host.owner_health() > 0 && host.owner_is_alive() {
        0
    } else {
        host.manager_ref().regen_rate as i32
    };
    for index in (0..host.manager_ref().slots.len()).rev() {
        let slot = host.manager_ref().slots[index].clone();
        match (slot.state, slot.spawn) {
            (Regenerating, _) => continue,
            (ReadyDocked | Reloading, child) => {
                host.manager().slots[index].state = Regenerating;
                if let Some(child) = child {
                    host.uninit(child);
                }
            }
            (KamikazeWait, child) => {
                if let Some(child) = child {
                    host.kamikaze_remove(child);
                }
                host.manager().slots[index].state = Regenerating;
                if let Some(child) = child {
                    host.uninit(child);
                }
            }
            (InFlight | Attacking | ComingHome, child) => {
                host.manager().slots[index].state = Regenerating;
                let target = host.manager_ref().current_target;
                if let Some(child) = child {
                    host.push(child, target);
                }
            }
        }
        let slot = &mut host.manager().slots[index];
        slot.spawn = None;
        slot.timer = CdTimer::started(frame, duration);
    }
}

// ---------------------------------------------------------------------------
// Production
// ---------------------------------------------------------------------------

/// `TechnoClass::AI`'s SpawnManager call (`0x006FA94C..0x006FA958`) for one
/// owner: `SpawnManagerClass::AI`.
pub(crate) fn spawn_manager_ai(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if let Some(mut host) = WorldSpawn::new(
        sim,
        owner_id,
        UninitContext::new(Some(rules), registry).with_effects(frame_effects),
    ) {
        ai(&mut host);
    }
}

/// Materialise every empty slot of a freshly constructed parent.
///
/// Native `SpawnManagerClass`'s constructor creates the whole pool up front
/// (CreateObject, Limbo and the SpawnOwner per slot) so `CountAliveSpawns`
/// is already full when the parent's first placement or fire attempt runs.
pub(crate) fn commit_spawn_manager_pool(
    sim: &mut Simulation,
    owner_id: u64,
    rules: &RuleSet,
    frame_effects: FrameEffects<'_>,
) {
    let Some(mut host) = WorldSpawn::new(
        sim,
        owner_id,
        UninitContext::with_rules(rules).with_effects(frame_effects),
    ) else {
        return;
    };
    for index in 0..host.manager_ref().slots.len() {
        if host.manager_ref().slots[index].spawn.is_none() {
            fill_slot(&mut host, index);
        }
    }
}

/// [`pointer_expired`] for one listening manager, from
/// `TechnoClass::PointerExpired`'s forward (`0x00707B19..0x00707B24`), which
/// sits outside the control test, so a cloak's `Detach_All(0)` reaches it as
/// a Destroy or UnInit does.
///
/// Every object joins the expiry roster in `ObjectClass::Constructor @
/// 0x005F3900` (append `0x005F3A85..0x005F3A8B`) and the announce loop
/// (`0x00725947..0x0072595F`) does not skip the announcer, so a spawner's own
/// expiry reaches the owner arm at its roster slot: at its Destroy and its
/// UnInit, a live spawner's Limbo (`0x005F4D61`) and each cloak broadcast. A
/// live owner's docked children UnInit and its slots regenerate at once
/// (`Kill_All_Spawns`); the next AI pass rebuilds them.
pub(crate) fn notify_pointer_expired(
    sim: &mut Simulation,
    listener_id: u64,
    expired_id: u64,
    rules: Option<&RuleSet>,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if let Some(mut host) = WorldSpawn::new(
        sim,
        listener_id,
        UninitContext::new(rules, registry).with_effects(frame_effects),
    ) {
        pointer_expired(&mut host, expired_id);
    }
}

/// [`clear_all_targets`] for one owner. Native callers besides the manager's
/// own bodies: the Stop event (`0x004C7634`), Stun (`0x006FCD90`, wired for
/// its death arm), Retaliate_And_Scan's CANT arm (`0x007098FB`) and
/// UnitClass::Fire_At_Target's CANT case (`0x0073705E`). The rules-less
/// UnInit adapters (tests and fixtures only) find no `MissileSpawn=` child, so
/// only the clear runs.
pub(crate) fn clear_all_spawn_targets(
    sim: &mut Simulation,
    owner_id: u64,
    rules: Option<&RuleSet>,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    if let Some(mut host) = WorldSpawn::new(
        sim,
        owner_id,
        UninitContext::new(rules, registry).with_effects(frame_effects),
    ) {
        clear_all_targets(&mut host);
    }
}

/// [`kill_all_spawns`] for one owner. Native callers, and which are wired:
/// - `SpawnManagerClass::PointerExpired(owner)` — WIRED: every broadcast of
///   the owner's own expiry reaches it at the owner's roster slot
///   ([`notify_pointer_expired`]).
/// - `TechnoClass::ChangeOwner` (`0x0070157E`) — WIRED, via
///   `Simulation::change_owner`; this is the mind-control path.
/// - `TemporalClass::InitiateWarp` (`0x0071AF39`) — WIRED, via
///   `sim::temporal`: a chrono-warped spawner loses its pool first.
/// - `TechnoClass::ImbueLocomotor` (`0x00710021`), the Magnetron lift —
///   WIRED, via `Simulation::imbue_locomotor`.
/// - `TechnoClass::Stun` (`0x006FCD40`) — WIRED via
///   `Simulation::techno_death_stun`, which a lifted Foot skips.
///
/// Push needs the rules; the rules-less UnInit adapters (tests and fixtures
/// only) skip it.
pub(crate) fn kill_all_spawns_with_context(
    sim: &mut Simulation,
    owner_id: u64,
    context: UninitContext<'_>,
) {
    if let Some(mut host) = WorldSpawn::new(sim, owner_id, context) {
        kill_all_spawns(&mut host);
    }
}

/// The world behind [`SpawnHost`]: the owner's manager and its children.
struct WorldSpawn<'a, 'c> {
    sim: &'a mut Simulation,
    owner_id: u64,
    context: UninitContext<'c>,
}

impl<'a, 'c> WorldSpawn<'a, 'c> {
    /// A host for an owner that carries a manager.
    fn new(sim: &'a mut Simulation, owner_id: u64, context: UninitContext<'c>) -> Option<Self> {
        sim.substrate
            .entities
            .get(owner_id)
            .is_some_and(|owner| owner.spawn_manager.is_some())
            .then_some(Self {
                sim,
                owner_id,
                context,
            })
    }

    fn child_type(&self, child: u64) -> Option<&crate::rules::object_type::ObjectType> {
        let rules = self.context.rules()?;
        let entity = self.sim.substrate.entities.get(child)?;
        self.sim.object_type(entity.type_ref(), rules)
    }

    fn owner_type(&self) -> Option<&crate::rules::object_type::ObjectType> {
        let rules = self.context.rules()?;
        let owner = self.sim.substrate.entities.get(self.owner_id)?;
        self.sim.object_type(owner.type_ref(), rules)
    }
}

impl SpawnHost for WorldSpawn<'_, '_> {
    fn frame(&self) -> i32 {
        self.sim.session.binary_frame() as i32
    }

    fn manager(&mut self) -> &mut SpawnManagerState {
        self.sim
            .substrate
            .entities
            .get_mut(self.owner_id)
            .and_then(|owner| owner.spawn_manager.as_mut())
            .expect("a spawn manager keeps its owner for the call")
    }

    fn manager_ref(&self) -> &SpawnManagerState {
        self.sim
            .substrate
            .entities
            .get(self.owner_id)
            .and_then(|owner| owner.spawn_manager.as_ref())
            .expect("a spawn manager keeps its owner for the call")
    }

    fn is_owner(&self, id: u64) -> bool {
        id == self.owner_id
    }

    fn pool_is_cmisl(&self) -> bool {
        self.context
            .rules()
            .is_some_and(|rules| cmisl_pool(self.sim, rules, self.manager_ref().spawn_type))
    }

    fn owner_is_moving(&mut self) -> bool {
        self.sim
            .substrate
            .entities
            .get(self.owner_id)
            .is_some_and(|owner| crate::sim::movement::motion_query::is_moving(owner) == Some(true))
    }

    fn owner_is_moving_now(&mut self) -> bool {
        let Some(rules) = self.context.rules() else {
            return false;
        };
        let sim = &*self.sim;
        sim.substrate
            .entities
            .get(self.owner_id)
            .is_some_and(|owner| {
                crate::sim::movement::motion_query::is_moving_now(
                    owner,
                    Some(crate::sim::movement::SpeedRules::new(
                        rules,
                        &sim.interner,
                        &sim.houses,
                    )),
                    sim.session.binary_frame(),
                )
            })
    }

    fn owner_locomotor_swap(&self) -> bool {
        // SpawnManager6B737C reads Foot+6AD, the locomotor-swap latch,
        // independent of Unit deployment6E0..6E2.
        self.sim
            .substrate
            .entities
            .get(self.owner_id)
            .is_some_and(|owner| owner.lift.swap_active())
    }

    fn owner_missile_spawn(&self) -> bool {
        self.owner_type().is_some_and(|kind| kind.missile_spawn)
    }

    fn owner_weapon_burst(&self) -> Option<i32> {
        let rules = self.context.rules()?;
        let owner = self.sim.substrate.entities.get(self.owner_id)?;
        crate::sim::combat::combat_weapon::primary_for_tier(
            self.owner_type()?,
            owner.veterancy.rank(),
        )
        .and_then(|id| rules.weapon(id))
        .map(|weapon| weapon.burst)
    }

    fn owner_health(&self) -> i32 {
        self.sim
            .substrate
            .entities
            .get(self.owner_id)
            .map_or(0, |owner| owner.health.current())
    }

    fn owner_is_alive(&self) -> bool {
        self.sim
            .substrate
            .entities
            .get(self.owner_id)
            .is_some_and(|owner| owner.lifecycle.object_alive() && !owner.death.is_dying())
    }

    fn owner_cell(&self) -> (i32, i32) {
        self.sim
            .substrate
            .entities
            .get(self.owner_id)
            .map_or((-1, -1), |owner| {
                (
                    i32::from(owner.position.rx()),
                    i32::from(owner.position.ry()),
                )
            })
    }

    fn can_fire_at(&mut self, target: TargetKind) -> bool {
        // Unit's vt+0x3AC is `TechnoClass::CanFireAtTarget @ 0x006F7780`
        // (InRange with the selected weapon).
        let (Some(rules), Some(terrain)) =
            (self.context.rules(), self.sim.resolved_terrain.as_ref())
        else {
            return false;
        };
        let sim = &*self.sim;
        crate::sim::combat::can_fire_at_target(
            sim,
            rules,
            &sim.interner,
            self.owner_id,
            &target,
            terrain,
            Some(sim.house_alliances.as_map()),
            &crate::sim::combat::line_of_fire::LineOfFireInputs {
                overlay_grid: sim.overlay_grid.as_ref(),
                overlay_registry: self.context.registry(),
                alliances: Some(sim.house_alliances.as_map()),
            },
        )
    }

    fn child_ammo(&self, child: u64) -> i32 {
        self.sim
            .substrate
            .entities
            .get(child)
            .and_then(|c| c.ammo())
            .map(|a| a.current())
            // Real Aircraft retain their signed count, including -1. This
            // fallback applies only to incomplete/non-Aircraft data.
            .unwrap_or(i32::MAX)
    }

    fn child_missile_spawn(&self, child: u64) -> bool {
        self.child_type(child)
            .is_some_and(|kind| kind.missile_spawn)
    }

    fn child_health(&self, child: u64) -> i32 {
        self.sim
            .substrate
            .entities
            .get(child)
            .map_or(0, |child| child.health.current())
    }

    fn child_tracked(&self, child: u64) -> bool {
        self.sim.kamikaze.contains(child)
    }

    fn child_cell(&self, child: u64) -> (i32, i32) {
        self.sim
            .substrate
            .entities
            .get(child)
            .map_or((-2, -2), |child| {
                (
                    i32::from(child.position.rx()),
                    i32::from(child.position.ry()),
                )
            })
    }

    fn child_height_over_owner(&self, child: u64) -> i32 {
        let terrain = self.sim.resolved_terrain.as_ref();
        let location_z = |id| {
            self.sim.substrate.entities.get(id).map(|object| {
                crate::sim::movement::ground_pose::object_world_z_leptons(object, terrain)
            })
        };
        // `0x006B77C7..0x006B77F7`: a wrapping DWORD subtraction, compared
        // signed with 0x14.
        location_z(child)
            .zip(location_z(self.owner_id))
            .map_or(0, |(child, owner)| child.wrapping_sub(owner))
    }

    fn launch(&mut self, child: u64, burst_parity: Option<i32>) {
        let Some(rules) = self.context.rules() else {
            return;
        };
        let frame = self.sim.session.binary_frame();
        let Some(owner) = self.sim.substrate.entities.get(self.owner_id) else {
            return;
        };
        let owner_position = owner.position;
        // The Unlimbo direction: the owner's PrimaryFacing `Current()` rounded
        // to a DirType (`0x006B74E9..0x006B74FC`).
        let launch_dir = owner.body_facing_dir(frame);
        let Some(launch) = launch_coordinate(self.sim, rules, self.owner_id, burst_parity) else {
            return;
        };
        // The child's Unlimbo (vt+0xD8, `0x006B7505`) at that coordinate,
        // staged on its limbo Location for Reveal: a `MissileSpawn=` type keeps
        // it whole (`unlimbo_z`); an aircraft's Z is replaced there. Its
        // Unlimbo snaps the body to the direction (`0x006F6DAA`). The coarse
        // level stays the owner's. The original ignores its answer.
        if let Some(child) = self.sim.substrate.entities.get_mut(child) {
            child.position.put_location(launch);
            child.position.copy_level_from(&owner_position);
            child.body_facing.snap(u16::from(launch_dir) << 8, frame);
        }
        let _ = self.sim.reveal_entity_with_rules(child, rules);
    }

    fn reset_owner_burst(&mut self) {
        if let Some(owner) = self.sim.substrate.entities.get_mut(self.owner_id) {
            owner.weapon_burst.reset();
        }
    }

    fn takeoff_anim(&mut self, child: u64) {
        // `AnimClass(V3TAKOFF, &Location, 2, 1, 0x600, -10, 0)`; `[0x00840098]`
        // names it too. The constructor draws only for `RandomRate=`,
        // `IsMeteor=` or `Bouncer=`, which retail `[V3TAKOFF]` leaves unset.
        let Some(rules) = self.context.rules() else {
            return;
        };
        let Some(location) =
            self.sim.substrate.entities.get(child).map(|child| {
                crate::sim::movement::ground_pose::position_world_coord(&child.position)
            })
        else {
            return;
        };
        self.sim.spawn_named_anim(
            rules,
            crate::rules::effect_asset_catalog::ROCKET_TAKEOFF_ANIM,
            crate::sim::anim_class::AnimWorldCoord {
                x: location.x,
                y: location.y,
                z: location.z,
            },
            2,
            crate::sim::movement::rocket_movement::PUFF_DRAW_FLAGS,
            -10,
        );
    }

    fn set_destination(&mut self, child: u64, destination: SpawnDestination) {
        let Some(rules) = self.context.rules() else {
            return;
        };
        let destination = match destination {
            // The Foot setter hands a missile's target coordinate to Rocket
            // Move_To (`rocket_movement::move_to`).
            SpawnDestination::Target(target) => {
                target.map(crate::sim::components::NavTargetRef::from)
            }
            SpawnDestination::Adjacent(direction) => {
                let Some(owner) = self.sim.substrate.entities.get(self.owner_id) else {
                    return;
                };
                let (rx, ry) = adjacent_cell(owner.position.rx(), owner.position.ry(), direction);
                Some(crate::sim::components::NavTargetRef::cell(rx, ry))
            }
            // The NavCom is the owner itself, so the child heads for the
            // owner's vt+0x4C: a moving carrier's destination, a standing
            // one's center.
            SpawnDestination::Owner => Some(TargetKind::Entity(self.owner_id).into()),
        };
        self.sim
            .assign_aircraft_destination(child, destination, rules, self.context.effects());
    }

    fn queue_mission(&mut self, child: u64, mission: MissionType) {
        let _ = self.sim.mission_queue_exact(
            child,
            crate::sim::mission::MissionId::from_known(mission),
            0,
            self.sim.session.binary_frame(),
            &crate::sim::mission::authority::EntityReadyInputProvider,
        );
    }

    fn assign_target(&mut self, child: u64, target: Option<TargetKind>) {
        let _ = self.sim.assign_target_represented(
            child,
            target,
            self.context.rules(),
            None,
            FrameEffects::default(),
        );
    }

    fn limbo(&mut self, child: u64) {
        // FootClass::Limbo and TechnoClass::Limbo do nothing for an object
        // already in limbo (`0x004DB266`, `0x006F6AC9`).
        let in_limbo = self
            .sim
            .substrate
            .entities
            .get(child)
            .is_none_or(|child| child.lifecycle.in_limbo());
        if let (false, Some(rules)) = (in_limbo, self.context.rules()) {
            self.sim
                .techno_limbo_with_context(child, self.context.requiring_rules(rules));
        }
    }

    /// Case 6's rearm (`0x006B788F..0x006B78AA`).
    fn refill(&mut self, child: u64) {
        let Some(strength) = self.child_type(child).map(|kind| kind.strength) else {
            return;
        };
        if let Some(child) = self.sim.substrate.entities.get_mut(child) {
            child.health.set(strength);
            child.estimated_health.reset(strength);
            if let Some(ammo) = child.ammo_mut() {
                ammo.refill();
            }
        }
    }

    fn create_child(&mut self) -> Option<u64> {
        let rules = self.context.rules()?;
        let spawn_type = self.manager().spawn_type;
        let owner = self.sim.substrate.entities.get(self.owner_id)?;
        let house = self.sim.interner.resolve(owner.owner()).to_string();
        let (rx, ry, z) = (owner.position.rx(), owner.position.ry(), owner.position.z());
        let facing = owner.body_facing_byte(self.sim.session.binary_frame());
        let type_name = self.sim.interner.resolve(spawn_type).to_string();
        self.sim.construct_object_limbo_at_height(
            &type_name,
            &house,
            rx,
            ry,
            facing,
            z,
            rules,
            self.context.effects(),
        )
    }

    fn adopt(&mut self, child: u64) {
        if let Some(entity) = self.sim.substrate.entities.get_mut(child) {
            entity.spawn_owner_id.owner = Some(self.owner_id);
        }
    }

    fn uninit(&mut self, child: u64) {
        self.sim.uninit_with_context(child, self.context);
    }

    fn push(&mut self, child: u64, target: Option<TargetKind>) {
        if let Some(rules) = self.context.rules() {
            self.sim.kamikaze_push(
                child,
                target,
                rules,
                self.context.registry(),
                self.context.effects(),
            );
        }
    }

    fn restart_kamikaze(&mut self) {
        self.sim.kamikaze_restart_after_push();
    }

    fn kamikaze_remove(&mut self, child: u64) {
        self.sim.kamikaze.remove(child);
    }
}

/// The eight-direction cell step native uses for the owner-relative hold cell.
fn adjacent_cell(rx: u16, ry: u16, direction: u8) -> (u16, u16) {
    let (dx, dy) = DIRECTION_DELTAS[(direction & 7) as usize];
    (
        (rx as i32 + dx).max(0) as u16,
        (ry as i32 + dy).max(0) as u16,
    )
}

/// The coordinate `SpawnManagerClass::AI` case 0 unlimbos a slot's child at
/// (`0x006B73FC..0x006B74D7`).
///
/// - GetFLH (vt+0xB0) takes the owner's burst index: the slot's parity when
///   the launch set it, else the owner's own.
/// - GetFLH is asked for weapon 0 when GetWeapon(0) is `Spawner=`
///   (`+0x131`), else weapon 1 (`0x006B742A..0x006B7436`). Its base is the
///   owner type's `SecondSpawnOffset=` while the burst index is nonzero, else
///   zero (`0x006B743B..0x006B7492`).
/// - The Z gains [`LAUNCH_Z_LIFT_LEPTONS`]; a `CMislType=` pool's X and Y
///   lose [`CMISL_LAUNCH_OFFSET_LEPTONS`] (`0x006B74B9..0x006B74D7`).
///
/// Native comparison: tools/projectile_oracle/ifv_fire_coord.json
/// `spawn_launch` runs this block on retail V3, DRED and BSUB from the
/// missile-slot test to the Unlimbo call, then the burst reset.
///
/// GetWeapon(0) with no WeaponType faults natively (`0x006B742C` reads
/// through it); VERA asks for weapon 1 then.
///
/// RESIDUAL, inherited from the GetFLH port (GSI-08.04,
/// `util::flh_transform`): no slope tilt. Trigger: a launch from a sloped
/// cell, such as a V3 on a ramp. Effect: the missile unlimbos at the
/// flat-ground FLH, a lepton or two off. Frequency: launches from slopes.
/// Risk: the launch coordinate seeds the missile's hashed Location.
fn launch_coordinate(
    sim: &Simulation,
    rules: &RuleSet,
    owner_id: u64,
    burst_parity: Option<i32>,
) -> Option<crate::sim::components::DriveCoord> {
    use crate::sim::combat::fire_coord;

    let spawn_type = sim
        .substrate
        .entities
        .get(owner_id)
        .and_then(|owner| owner.spawn_manager.as_ref())?
        .spawn_type;
    let owner = sim.substrate.entities.get(owner_id)?;
    let obj = sim.object_type(owner.type_ref(), rules)?;
    let weapon = crate::sim::combat::combat_weapon::primary_for_tier(obj, owner.veterancy.rank())
        .and_then(|id| rules.weapon(id));
    let burst = burst_parity.unwrap_or_else(|| owner.weapon_burst.index());
    let base = if burst == 0 {
        Default::default()
    } else {
        fire_coord::firer_art(rules, obj)
            .map_or_else(Default::default, |art| art.second_spawn_offset)
    };
    let fire = fire_coord::fire_coordinate(
        sim,
        rules,
        &fire_coord::FireSource::of_entity(owner),
        obj,
        if weapon.is_some_and(|weapon| weapon.spawner) {
            0
        } else {
            1
        },
        (burst & 1) as u8,
        base,
    );
    let mut coord = crate::sim::components::DriveCoord {
        x: fire.coord.x,
        y: fire.coord.y,
        z: fire.coord.z.wrapping_add(LAUNCH_Z_LIFT_LEPTONS),
    };
    if cmisl_pool(sim, rules, spawn_type) {
        coord.x = coord.x.wrapping_sub(CMISL_LAUNCH_OFFSET_LEPTONS[0]);
        coord.y = coord.y.wrapping_sub(CMISL_LAUNCH_OFFSET_LEPTONS[1]);
    }
    Some(coord)
}

/// The pool's spawn type pointer against Rules' `CMislType=`, which case 0
/// tests for the launch offset (`0x006B74BC`) and for `V3TAKOFF`
/// (`0x006B7513`); a type name names one type, so the name compare is that
/// test. It stays separate from the family the pool's flag and kamikaze wait
/// read, so a mod whose CMislType is also its V3 or DMisl type keeps both.
fn cmisl_pool(sim: &Simulation, rules: &RuleSet, spawn_type: InternedId) -> bool {
    sim.interner
        .resolve(spawn_type)
        .eq_ignore_ascii_case(&rules.missile_spawn.cmisl.type_name)
}
