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
//! ## Native contract (verified this session)
//!
//! - Dispatched once per tick from `TechnoClass::AI_Update`
//!   (`decompile_function 0x006F9E50`: `if (this+0x2D0) (*(vtable+0x5C))()`),
//!   after the mission dispatch and the self-heal/power block. It is **not**
//!   dispatched from `UnitClass::AI` — see the module note in
//!   `sim/world/unit_post.rs`.
//! - `SpawnManagerClass::AI` (`decompile_function 0x006B7230`) self-gates on an
//!   update timer: 20 frames for the first pass, 10 frames thereafter. All slot
//!   work and the manager-mode block run only on those frames.
//! - `CountAliveSpawns` (`decompile_function 0x006B7D30`) counts every slot
//!   whose state is not `Regenerating`. The parent's fire gate uses it.
//! - The manager makes **two** separate missile tests, and they are not the
//!   same test. The per-slot `IsMissileSpawn` flag comes from comparing the
//!   resolved `Spawns=` type against `[General] V3RocketType/DMislType/
//!   CMislType` (see `rules::missile_spawn`); the retreat-versus-return
//!   decision in the Launching arm reads the **child type's own
//!   `MissileSpawn=`** (`childType+0xD68`). The two sets coincide in stock YR,
//!   so stock behaviour is identical either way; both are modelled because a
//!   mod can separate them.
//!
//! Background: `docs/research/SPAWN_MANAGER_CLASS_GHIDRA_REPORT.md`,
//! `docs/research/ROCKET_LOCOMOTION_CLASS_GHIDRA_REPORT.md`.
//!
//! ## Deliberately not parsed
//!
//! `Spawned=` (`TechnoTypeClass+0xD54`) is a live flag with four verified
//! consumers — `search_instructions operand_pattern=0xd54]` finds it read by
//! `AircraftClass::Is_Cell_Free_For_Landing` (two sites),
//! `TechnoClass::GetFireError` (`0x006FC67B`),
//! `TechnoClass::Set_ArchiveTarget` and `TechnoClass::IsIdleForAutoTarget`.
//! None of those four behaviours are implemented by this slice, and the
//! auto-target one lives in the techno-AI host rather than here, so the key is
//! left unparsed rather than parsed-and-ignored. Whoever wires the first of
//! those consumers should add it then.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/world, sim/combat, sim/movement, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use serde::{Deserialize, Serialize};

use crate::rules::missile_spawn::MissileFamily;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::intern::InternedId;
use crate::sim::world::{Simulation, UninitContext};

/// Frames the manager waits before its very first AI pass
/// (`UpdateTimer.Duration = 0x14` at construction).
const FIRST_UPDATE_DELAY_FRAMES: u32 = 20;
/// Frames between AI passes once the manager has run at least once.
const UPDATE_PERIOD_FRAMES: u32 = 10;
/// Per-launch delay written to the manager's reload timer when the *parent*
/// type does not set `MissileSpawn=`. No stock YR parent sets it, so this is
/// the only branch reachable in stock play.
const LAUNCH_DELAY_FRAMES: u32 = 20;
/// Per-launch delay when the parent type sets `MissileSpawn=yes`. Unreachable
/// in stock YR; kept because the branch is data-driven, not gated.
const LAUNCH_DELAY_FRAMES_MISSILE_PARENT: u32 = 9;
/// Height difference (leptons) under which a returning child counts as docked.
const DOCK_HEIGHT_EPSILON_LEPTONS: i32 = 0x14;

/// Native `RateTimerClass` pair: an anchor frame plus a duration.
///
/// `start_frame == None` models the native `-1` sentinel ("never anchored"),
/// in which case the timer is due only when its duration is zero. Otherwise the
/// timer is due once `duration` frames have elapsed since the anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpawnTimer {
    pub start_frame: Option<u32>,
    pub duration: u32,
}

impl SpawnTimer {
    /// A timer that is already due (native `{ -1, 0 }`).
    pub const fn ready() -> Self {
        Self {
            start_frame: None,
            duration: 0,
        }
    }

    /// Anchor at `frame` for `duration` frames.
    pub const fn armed(frame: u32, duration: u32) -> Self {
        Self {
            start_frame: Some(frame),
            duration,
        }
    }

    /// Native expiry test: remaining time has reached zero.
    pub fn due(self, now: u32) -> bool {
        match self.start_frame {
            None => self.duration == 0,
            Some(start) => now.wrapping_sub(start) >= self.duration,
        }
    }
}

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
    /// 3 — aircraft child has been recalled and is flying home.
    ReturningToDock,
    /// 4 — aircraft child is over the parent, waiting to touch down.
    LandingAtDock,
    /// 6 — child is docked in limbo, reloading.
    Reloading,
    /// 7 — slot has no child; rebuilding one after `SpawnRegenRate` frames.
    Regenerating,
}

/// One pool slot (native `SpawnControl`, 0x18 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpawnSlot {
    /// Stable id of the child, or `None` while regenerating.
    pub spawn: Option<u64>,
    pub state: SpawnSlotState,
    pub timer: SpawnTimer,
    /// Set when the pool's child type is one of the three hardcoded rocket
    /// families. Drives the launch stationary-gate and the kamikaze path.
    pub is_missile_spawn: bool,
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
    pub spawn_type: InternedId,
    /// Which hardcoded rocket family the child belongs to, if any.
    pub missile_family: Option<MissileFamily>,
    /// `SpawnRegenRate=` in frames.
    pub regen_rate: u32,
    /// `SpawnReloadRate=` in frames.
    pub reload_rate: u32,
    /// `PauseFrames + TiltFrames` for this pool's missile family, cached at
    /// construction so the per-tick machine never needs the RuleSet. Zero for
    /// aircraft pools, which never enter `KamikazeWait`.
    pub kamikaze_wait_frames: u32,
    pub slots: Vec<SpawnSlot>,
    /// Gates the whole AI pass (20 frames, then 10).
    pub update_timer: SpawnTimer,
    /// Gates launches across the pool, not per slot.
    pub reload_timer: SpawnTimer,
    pub current_target: Option<TargetKind>,
    pub queued_target: Option<TargetKind>,
    pub mode: SpawnManagerMode,
}

impl SpawnManagerState {
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

    /// `SpawnManagerClass::SetTarget` (`0x006B7B90`): a target that differs
    /// from the live one is queued, never written straight through. The next AI
    /// pass promotes it.
    pub fn set_target(&mut self, target: Option<TargetKind>) {
        if target != self.current_target {
            self.queued_target = target;
        }
    }

    /// `SpawnManagerClass::ClearAllTargets` (`0x006B7BB0`): drop both targets
    /// and fall back to `Idle`.
    fn clear_all_targets(&mut self) {
        self.current_target = None;
        self.queued_target = None;
        self.mode = SpawnManagerMode::Idle;
    }

    /// Promote the queued target, as every state branch does before reading
    /// `current_target`.
    fn promote_queued_target(&mut self) {
        if self.queued_target.is_some() {
            self.current_target = self.queued_target.take();
        }
    }

    fn slot_index_of(&self, child_id: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.spawn == Some(child_id))
    }
}

/// Build the manager state for a freshly constructed parent.
///
/// Mirrors `TechnoClass::Init_Managers` (`0x006F3FF4`): the manager exists iff
/// the type's `Spawns=` resolves to a real object type. The slot vector is
/// created here; the world-owned constructor transaction materialises every
/// child before the parent attempts Unlimbo.
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
    let slots = (0..obj.spawns_number.max(0) as usize)
        .map(|_| SpawnSlot {
            spawn: None,
            // Slots enter Regenerating with an already-due timer so the
            // world-owned constructor transaction can fill them. Native fills
            // them in the manager constructor.
            state: SpawnSlotState::Regenerating,
            timer: SpawnTimer::ready(),
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
        update_timer: SpawnTimer::armed(frame, FIRST_UPDATE_DELAY_FRAMES),
        reload_timer: SpawnTimer::ready(),
        current_target: None,
        queued_target: None,
        mode: SpawnManagerMode::Idle,
    })
}

/// Materialise every empty slot of a freshly constructed parent.
///
/// Native `SpawnManagerClass`'s constructor creates the whole pool up front
/// (`CreateObject` + `Limbo` per slot) so `CountAliveSpawns` is already full
/// when the parent's first placement attempt or fire attempt runs.
pub fn commit_spawn_manager_pool(sim: &mut Simulation, owner_id: u64, rules: &RuleSet) {
    let Some(slot_count) = manager_field(sim, owner_id, |m| m.slots.len()) else {
        return;
    };
    for slot_index in 0..slot_count {
        let empty =
            manager_field(sim, owner_id, |m| m.slots[slot_index].spawn.is_none()).unwrap_or(false);
        if empty {
            regenerate_child(sim, rules, owner_id, slot_index);
        }
    }
}

/// Run every live spawn manager for this tick.
///
/// Placed immediately after the combat phase so a parent that set its spawn
/// target through `Spawner=yes` this tick is seen by its own manager in the
/// same tick — the ordering `TechnoClass::AI_Update` gives natively
/// (Mission_Dispatch → Fire_At → SetTarget, then the SpawnManager dispatch).
pub fn tick_spawn_managers(
    sim: &mut Simulation,
    rules: &RuleSet,
    order: &[u64],
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    let frame = sim.session.binary_frame;
    for &owner_id in order {
        let has_manager = sim
            .substrate
            .entities
            .get(owner_id)
            .is_some_and(|e| e.spawn_manager.is_some() && e.lifecycle.object_alive);
        if !has_manager {
            continue;
        }
        tick_one_manager(sim, rules, owner_id, frame, overlay_registry);
    }
}

fn tick_one_manager(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    frame: u32,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    // Update-timer gate. Everything below runs only on a manager frame.
    {
        let Some(entity) = sim.substrate.entities.get_mut(owner_id) else {
            return;
        };
        let Some(manager) = entity.spawn_manager.as_mut() else {
            return;
        };
        if !manager.update_timer.due(frame) {
            return;
        }
        manager.update_timer = SpawnTimer::armed(frame, UPDATE_PERIOD_FRAMES);
    }

    // Reap children that no longer exist before the slot walk, so a slot whose
    // missile already detonated is seen as regenerating this pass. Native gets
    // this through `TechnoClass::PointerExpired` at the moment of death.
    reap_expired_spawns(sim, owner_id, frame);

    let slot_count = sim
        .substrate
        .entities
        .get(owner_id)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| m.slots.len())
        .unwrap_or(0);
    for slot_index in 0..slot_count {
        step_slot(sim, rules, owner_id, slot_index, frame);
    }

    step_manager_mode(sim, rules, owner_id, frame, overlay_registry);
}

/// `SpawnManagerClass::PointerExpired` for the child-death case: a slot whose
/// child is gone drops to `Regenerating` with the regen timer armed.
fn reap_expired_spawns(sim: &mut Simulation, owner_id: u64, frame: u32) {
    let mut expired: Vec<usize> = Vec::new();
    if let Some(manager) = sim
        .substrate
        .entities
        .get(owner_id)
        .and_then(|e| e.spawn_manager.as_ref())
    {
        for (index, slot) in manager.slots.iter().enumerate() {
            if slot.state == SpawnSlotState::Regenerating {
                continue;
            }
            let alive = slot.spawn.is_some_and(|child| {
                sim.substrate
                    .entities
                    .get(child)
                    .is_some_and(|c| c.lifecycle.object_alive && !c.dying)
            });
            if !alive {
                expired.push(index);
            }
        }
    }
    if expired.is_empty() {
        return;
    }
    let Some(manager) = sim
        .substrate
        .entities
        .get_mut(owner_id)
        .and_then(|e| e.spawn_manager.as_mut())
    else {
        return;
    };
    let regen_rate = manager.regen_rate;
    for index in expired {
        let slot = &mut manager.slots[index];
        slot.spawn = None;
        slot.state = SpawnSlotState::Regenerating;
        slot.timer = SpawnTimer::armed(frame, regen_rate);
    }
}

fn step_slot(sim: &mut Simulation, rules: &RuleSet, owner_id: u64, slot_index: usize, frame: u32) {
    let Some(slot) = sim
        .substrate
        .entities
        .get(owner_id)
        .and_then(|e| e.spawn_manager.as_ref())
        .and_then(|m| m.slots.get(slot_index))
        .cloned()
    else {
        return;
    };

    match slot.state {
        SpawnSlotState::ReadyDocked => step_ready_docked(sim, rules, owner_id, slot_index, frame),
        SpawnSlotState::KamikazeWait => {
            if slot.timer.due(frame) {
                // The missile is on its own now; free the slot to regenerate.
                let regen_rate = manager_field(sim, owner_id, |m| m.regen_rate).unwrap_or(0);
                with_slot(sim, owner_id, slot_index, |slot| {
                    slot.spawn = None;
                    slot.state = SpawnSlotState::Regenerating;
                    slot.timer = SpawnTimer::armed(frame, regen_rate);
                });
            }
        }
        SpawnSlotState::InFlight => step_in_flight(sim, rules, owner_id, slot_index),
        SpawnSlotState::ReturningToDock => step_returning(sim, rules, owner_id, slot_index),
        SpawnSlotState::LandingAtDock => step_landing(sim, rules, owner_id, slot_index, frame),
        SpawnSlotState::Reloading => {
            if slot.timer.due(frame) {
                restore_docked_child(sim, rules, owner_id, slot_index);
            }
        }
        SpawnSlotState::Regenerating => {
            if slot.timer.due(frame) {
                regenerate_child(sim, rules, owner_id, slot_index);
            }
        }
    }
}

/// State 0 → 2: launch one child at the manager's current target.
fn step_ready_docked(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    slot_index: usize,
    frame: u32,
) {
    let Some((target, reload_due, mode, is_missile_slot, child_id)) = sim
        .substrate
        .entities
        .get(owner_id)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| {
            (
                m.current_target,
                m.reload_timer.due(frame),
                m.mode,
                m.slots[slot_index].is_missile_spawn,
                m.slots[slot_index].spawn,
            )
        })
    else {
        return;
    };
    let Some(target) = target else { return };
    if !reload_due || mode == SpawnManagerMode::Returning {
        return;
    }
    let Some(child_id) = child_id else { return };

    let Some(owner) = sim.substrate.entities.get(owner_id) else {
        return;
    };
    // Missile slots only launch from a fully stationary parent — the native
    // gate calls ILocomotor::Is_Moving and Is_Moving_Now on the owner. Aircraft
    // slots skip it, so Hornets launch from a moving Carrier.
    if is_missile_slot && (owner.movement_target.is_some() || owner.facing_target.is_some()) {
        return;
    }
    // A deployed parent does not launch (native reads owner+0x6AD).
    if owner.deploy_state.is_some() {
        return;
    }

    let owner_type = sim.interner.resolve(owner.type_ref()).to_string();
    let launch_rx = owner.position.rx;
    let launch_ry = owner.position.ry;
    let launch_z = owner.position.z;
    let launch_facing = owner.facing;
    let owner_veterancy = owner.veterancy;
    let parent_missile_spawn = rules
        .object(&owner_type)
        .map(|o| o.missile_spawn)
        .unwrap_or(false);

    let launch_delay = if parent_missile_spawn {
        LAUNCH_DELAY_FRAMES_MISSILE_PARENT
    } else {
        LAUNCH_DELAY_FRAMES
    };

    let missile_family = manager_field(sim, owner_id, |m| m.missile_family).flatten();

    // Promote before the launch so the child receives the freshest target, and
    // resolve the impact cell BEFORE anything is placed in the world. If the
    // target cannot resolve there is nothing to launch at, and committing the
    // slot anyway would leave a revealed child with no flight state that no
    // later pass can reach — a permanent prop on the launcher's cell.
    // **VERA-internal:** native cannot reach this state, because a target that
    // dies is dropped from the manager by `PointerExpired` in the same tick
    // rather than up to one manager period later.
    let launch_target = if is_missile_slot {
        with_manager(sim, owner_id, |m| m.promote_queued_target());
        let promoted = manager_field(sim, owner_id, |m| m.current_target)
            .flatten()
            .unwrap_or(target);
        match resolve_target_cell(sim, promoted) {
            Some(cell) => Some((promoted, cell)),
            None => return,
        }
    } else {
        None
    };

    // Place the child in the world at the launcher.
    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
        child.position.rx = launch_rx;
        child.position.ry = launch_ry;
        child.position.z = launch_z;
        child.facing = launch_facing;
    }
    let revealed = matches!(
        sim.reveal(child_id),
        crate::sim::world::RevealOutcome::Revealed { .. }
    );
    if !revealed {
        return;
    }

    if let Some((_, target_cell)) = launch_target {
        launch_missile_child(
            sim,
            rules,
            owner_id,
            child_id,
            target_cell,
            missile_family,
            owner_veterancy,
        );
    } else {
        // Native case 0's aircraft arm does NOT send the child at the wing
        // target. It assigns a cell adjacent to the owner (direction 0) and
        // mission 2, so a freshly launched Hornet climbs off the deck and holds
        // station. Only the manager's Launching block, once every slot is
        // committed, issues `Assign_Destination(CurrentTarget)` — that is what
        // makes the wing go out together instead of peeling off one per launch.
        hold_child_over_owner(sim, rules, owner_id, child_id, ADJACENT_DIR_ON_LAUNCH);
    }

    with_manager(sim, owner_id, |m| {
        m.reload_timer = SpawnTimer::armed(frame, launch_delay);
        m.slots[slot_index].state = SpawnSlotState::InFlight;
    });
}

/// State 2 for aircraft slots: keep the child pointed at the live target, or
/// send it home when the target is gone.
fn step_in_flight(sim: &mut Simulation, rules: &RuleSet, owner_id: u64, slot_index: usize) {
    let Some((is_missile_slot, child_id)) = manager_field(sim, owner_id, |m| {
        (
            m.slots[slot_index].is_missile_spawn,
            m.slots[slot_index].spawn,
        )
    }) else {
        return;
    };
    if is_missile_slot {
        return;
    }
    let Some(child_id) = child_id else { return };
    with_manager(sim, owner_id, |m| m.promote_queued_target());
    let target = manager_field(sim, owner_id, |m| m.current_target).flatten();
    match target {
        // Still holding formation over the parent — native re-issues the same
        // owner-relative cell (direction 4) every pass while the wing waits.
        Some(_) => hold_child_over_owner(sim, rules, owner_id, child_id, ADJACENT_DIR_WHILE_HELD),
        None => {
            recall_child_to_owner(sim, rules, owner_id, child_id);
            with_slot(sim, owner_id, slot_index, |slot| {
                slot.state = SpawnSlotState::LandingAtDock;
            });
        }
    }
}

/// State 3: the child is flying home. Out of ammo or targetless → start the
/// landing approach; otherwise push it back at the target.
fn step_returning(sim: &mut Simulation, rules: &RuleSet, owner_id: u64, slot_index: usize) {
    let Some(child_id) = manager_field(sim, owner_id, |m| m.slots[slot_index].spawn).flatten()
    else {
        return;
    };
    with_manager(sim, owner_id, |m| m.promote_queued_target());
    let target = manager_field(sim, owner_id, |m| m.current_target).flatten();
    if child_ammo(sim, child_id) == 0 || target.is_none() {
        recall_child_to_owner(sim, rules, owner_id, child_id);
        with_slot(sim, owner_id, slot_index, |slot| {
            slot.state = SpawnSlotState::LandingAtDock;
        });
    } else if let Some(target) = target {
        assign_child_move(sim, child_id, target);
    }
}

/// State 4: over the parent. Same cell and close enough in height → limbo the
/// child and start the reload timer.
fn step_landing(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    slot_index: usize,
    frame: u32,
) {
    let Some(child_id) = manager_field(sim, owner_id, |m| m.slots[slot_index].spawn).flatten()
    else {
        return;
    };
    with_manager(sim, owner_id, |m| m.promote_queued_target());
    let target = manager_field(sim, owner_id, |m| m.current_target).flatten();
    if child_ammo(sim, child_id) >= 1 && target.is_some() {
        // Rearmed and still needed — go back out.
        if let Some(target) = target {
            assign_child_move(sim, child_id, target);
        }
        with_slot(sim, owner_id, slot_index, |slot| {
            slot.state = SpawnSlotState::ReturningToDock;
        });
        return;
    }

    // Native compares the child's and the owner's 2D coords and requires the
    // Z gap to be under 0x14 leptons. VERA keeps aircraft altitude on the
    // locomotor rather than in `position.z`, so the gap is the child's own
    // altitude above the shared cell.
    let docked = match (
        sim.substrate.entities.get(owner_id),
        sim.substrate.entities.get(child_id),
    ) {
        (Some(owner), Some(child)) => {
            child.position.rx == owner.position.rx
                && child.position.ry == owner.position.ry
                && child
                    .locomotor
                    .as_ref()
                    .map(|l| l.altitude.to_num::<i32>())
                    .unwrap_or(0)
                    < DOCK_HEIGHT_EPSILON_LEPTONS
        }
        _ => false,
    };
    if docked {
        sim.techno_limbo_with_rules(child_id, rules);
        let reload_rate = manager_field(sim, owner_id, |m| m.reload_rate).unwrap_or(0);
        with_slot(sim, owner_id, slot_index, |slot| {
            slot.state = SpawnSlotState::Reloading;
            slot.timer = SpawnTimer::armed(frame, reload_rate);
        });
    } else {
        recall_child_to_owner(sim, rules, owner_id, child_id);
    }
}

/// State 6 → 0: reload restores the child's health from `Strength=` and its
/// ammo from `Ammo=`.
fn restore_docked_child(sim: &mut Simulation, rules: &RuleSet, owner_id: u64, slot_index: usize) {
    let Some(child_id) = manager_field(sim, owner_id, |m| m.slots[slot_index].spawn).flatten()
    else {
        return;
    };
    let child_type = sim
        .substrate
        .entities
        .get(child_id)
        .map(|c| sim.interner.resolve(c.type_ref()).to_string());
    if let Some(obj) = child_type.as_deref().and_then(|name| rules.object(name))
        && let Some(child) = sim.substrate.entities.get_mut(child_id)
    {
        // Native state 6 writes Health and retained estimated health from the
        // child type's `Strength=`, and Ammo from the child type's `Ammo=`.
        child.health.current = obj.strength;
        child.estimated_health.reset(child.health.current);
        if let Some(ammo) = child.aircraft_ammo.as_mut() {
            ammo.current = ammo.max;
        }
    }
    with_slot(sim, owner_id, slot_index, |slot| {
        slot.state = SpawnSlotState::ReadyDocked;
        slot.timer = SpawnTimer::ready();
    });
}

/// State 7 → 0: build a new child into limbo and hand it to the slot.
fn regenerate_child(sim: &mut Simulation, rules: &RuleSet, owner_id: u64, slot_index: usize) {
    let Some((spawn_type, missile_family)) =
        manager_field(sim, owner_id, |m| (m.spawn_type, m.missile_family))
    else {
        return;
    };
    let Some((owner_house, rx, ry, z, facing)) =
        sim.substrate.entities.get(owner_id).map(|owner| {
            (
                sim.interner.resolve(owner.owner()).to_string(),
                owner.position.rx,
                owner.position.ry,
                owner.position.z,
                owner.facing,
            )
        })
    else {
        return;
    };
    let type_name = sim.interner.resolve(spawn_type).to_string();
    let Some(child_id) =
        sim.construct_object_limbo_at_height(&type_name, &owner_house, rx, ry, facing, z, rules)
    else {
        return;
    };
    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
        // Native writes the parent back-pointer into the child at +0x2D4; kill
        // credit and the "don't self-RTB" gate both read it.
        child.spawn_owner_id = Some(owner_id);
    }
    with_slot(sim, owner_id, slot_index, |slot| {
        slot.spawn = Some(child_id);
        slot.is_missile_spawn = missile_family.is_some();
        slot.state = SpawnSlotState::ReadyDocked;
        slot.timer = SpawnTimer::ready();
    });
}

/// The manager-level machine, run after every slot has been stepped.
fn step_manager_mode(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    frame: u32,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    let Some(mode) = manager_field(sim, owner_id, |m| m.mode) else {
        return;
    };
    match mode {
        SpawnManagerMode::Idle => {
            with_manager(sim, owner_id, SpawnManagerState::promote_queued_target);
            let Some(target) = manager_field(sim, owner_id, |m| m.current_target).flatten() else {
                return;
            };
            // gamemd-derived: `SpawnManagerClass::AI` @ 0x006B7230 mode 0
            // promotes +0x6C to +0x68, then Unit's vslot +0x3AC reaches
            // `TechnoClass::CanFireAtTarget` @ 0x006F7780. A false result calls
            // `ClearAllTargets` @ 0x006B7BB0 and returns before Launching.
            let target_is_legal = sim.resolved_terrain.as_ref().is_some_and(|terrain| {
                crate::sim::combat::can_fire_at_target(
                    &sim.substrate.entities,
                    rules,
                    &sim.interner,
                    owner_id,
                    &target,
                    terrain,
                    Some(&sim.house_alliances),
                    &crate::sim::combat::line_of_fire::LineOfFireInputs {
                        overlay_grid: sim.overlay_grid.as_ref(),
                        overlay_registry,
                        alliances: Some(&sim.house_alliances),
                    },
                )
            });
            if !target_is_legal {
                with_manager(sim, owner_id, SpawnManagerState::clear_all_targets);
                return;
            }
            with_manager(sim, owner_id, |m| m.mode = SpawnManagerMode::Launching);
        }
        SpawnManagerMode::Launching => {
            let Some(states) = manager_field(sim, owner_id, |m| {
                m.slots.iter().map(|s| s.state).collect::<Vec<_>>()
            }) else {
                return;
            };
            if manager_field(sim, owner_id, |m| m.current_target)
                .flatten()
                .is_none()
            {
                with_manager(sim, owner_id, |m| m.clear_all_targets());
                return;
            }
            // Wait until every slot is either out or rebuilding.
            let all_committed = states
                .iter()
                .all(|s| matches!(s, SpawnSlotState::InFlight | SpawnSlotState::Regenerating));
            if !all_committed {
                return;
            }

            let mut cleared_targets = false;
            let Some((kamikaze_frames, slot_count)) =
                manager_field(sim, owner_id, |m| (m.kamikaze_wait_frames, m.slots.len()))
            else {
                return;
            };
            for index in 0..slot_count {
                let Some((state, is_missile_family, child_slot)) =
                    manager_field(sim, owner_id, |m| {
                        (
                            m.slots[index].state,
                            m.slots[index].is_missile_spawn,
                            m.slots[index].spawn,
                        )
                    })
                else {
                    continue;
                };
                if state != SpawnSlotState::InFlight {
                    continue;
                }
                // Native uses TWO different tests here, not one:
                //   * retreat-vs-return is decided by the CHILD TYPE's own
                //     `MissileSpawn=` (`childType+0xD68`), and
                //   * once on the retreat path, kamikaze-wait-vs-immediate-regen
                //     is decided by the slot's `IsMissileSpawn` flag, which
                //     comes from the hardcoded V3Rocket/DMisl/CMisl family test.
                // The two sets coincide in stock YR, so behaviour is identical;
                // they are kept apart because a mod can separate them.
                let child_missile_spawn = child_slot
                    .and_then(|child| sim.substrate.entities.get(child))
                    .map(|child| sim.interner.resolve(child.type_ref()).to_string())
                    .and_then(|name| rules.object(&name))
                    .map(|obj| obj.missile_spawn)
                    .unwrap_or(is_missile_family);
                if child_missile_spawn {
                    // The child has left the launcher for good.
                    cleared_targets = true;
                    if is_missile_family {
                        with_slot(sim, owner_id, index, |slot| {
                            slot.state = SpawnSlotState::KamikazeWait;
                            slot.timer = SpawnTimer::armed(frame, kamikaze_frames);
                        });
                    } else {
                        // No kamikaze window: the slot starts regenerating now.
                        let regen_rate =
                            manager_field(sim, owner_id, |m| m.regen_rate).unwrap_or(0);
                        with_slot(sim, owner_id, index, |slot| {
                            slot.spawn = None;
                            slot.state = SpawnSlotState::Regenerating;
                            slot.timer = SpawnTimer::armed(frame, regen_rate);
                        });
                    }
                } else {
                    let child = manager_field(sim, owner_id, |m| m.slots[index].spawn).flatten();
                    let target = manager_field(sim, owner_id, |m| m.current_target).flatten();
                    if let (Some(child), Some(target)) = (child, target) {
                        assign_child_move(sim, child, target);
                    }
                    with_slot(sim, owner_id, index, |slot| {
                        slot.state = SpawnSlotState::ReturningToDock;
                    });
                }
            }
            with_manager(sim, owner_id, |m| {
                if cleared_targets {
                    m.clear_all_targets();
                }
                m.mode = SpawnManagerMode::Returning;
            });
        }
        SpawnManagerMode::Returning => {
            let Some(any_out) = manager_field(sim, owner_id, |m| {
                m.slots.iter().any(|s| {
                    matches!(
                        s.state,
                        SpawnSlotState::ReturningToDock | SpawnSlotState::LandingAtDock
                    )
                })
            }) else {
                return;
            };
            if !any_out {
                with_manager(sim, owner_id, |m| m.mode = SpawnManagerMode::Idle);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn manager_field<T>(
    sim: &Simulation,
    owner_id: u64,
    f: impl FnOnce(&SpawnManagerState) -> T,
) -> Option<T> {
    sim.substrate
        .entities
        .get(owner_id)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(f)
}

fn with_manager(sim: &mut Simulation, owner_id: u64, f: impl FnOnce(&mut SpawnManagerState)) {
    if let Some(manager) = sim
        .substrate
        .entities
        .get_mut(owner_id)
        .and_then(|e| e.spawn_manager.as_mut())
    {
        f(manager);
    }
}

fn with_slot(
    sim: &mut Simulation,
    owner_id: u64,
    slot_index: usize,
    f: impl FnOnce(&mut SpawnSlot),
) {
    if let Some(slot) = sim
        .substrate
        .entities
        .get_mut(owner_id)
        .and_then(|e| e.spawn_manager.as_mut())
        .and_then(|m| m.slots.get_mut(slot_index))
    {
        f(slot);
    }
}

/// Cell a manager target currently occupies, or `None` when an entity target
/// no longer resolves.
fn resolve_target_cell(sim: &Simulation, target: TargetKind) -> Option<(u16, u16)> {
    match target {
        TargetKind::Cell(rx, ry) => Some((rx, ry)),
        TargetKind::Entity(id) => sim
            .substrate
            .entities
            .get(id)
            .map(|t| (t.position.rx, t.position.ry)),
    }
}

fn child_ammo(sim: &Simulation, child_id: u64) -> i32 {
    sim.substrate
        .entities
        .get(child_id)
        .and_then(|c| c.aircraft_ammo.as_ref())
        .map(|a| a.current)
        // No finite-ammo tracking means unlimited; native reads Ammo=-1 the
        // same way and never sends such a child home to rearm.
        .unwrap_or(i32::MAX)
}

fn assign_child_attack(sim: &mut Simulation, child_id: u64, target: TargetKind) {
    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
        child.attack_target = Some(match target {
            TargetKind::Entity(id) => crate::sim::combat::AttackTarget::new(id),
            TargetKind::Cell(rx, ry) => crate::sim::combat::AttackTarget::for_cell(rx, ry),
        });
        if let Some(mission) = child.aircraft_mission.as_mut() {
            *mission = crate::sim::aircraft::AircraftMission::Attack {
                sub_state: 0,
                has_fired: false,
                is_strafe: false,
            };
        }
    }
}

/// Native state 3/4 issue `Assign_Destination(target)` + `Assign_Mission(Move)`
/// on an aircraft child; the child's own mission machine then flies it and
/// fires. VERA's aircraft attack mission owns both halves, so both map to the
/// same assignment.
fn assign_child_move(sim: &mut Simulation, child_id: u64, target: TargetKind) {
    assign_child_attack(sim, child_id, target);
}

/// Adjacent-cell direction native case 0 uses when a child first launches.
const ADJACENT_DIR_ON_LAUNCH: u8 = 0;
/// Adjacent-cell direction native case 2 re-issues while the wing forms up.
const ADJACENT_DIR_WHILE_HELD: u8 = 4;

/// Park an aircraft child on a cell next to its parent with no attack order.
///
/// Native assigns the adjacent CellClass as the child's *target* and mission 2;
/// for an aircraft that reads as "fly there", not "shoot the ground". VERA's
/// aircraft would force-fire a cell target, so the hold is expressed as an air
/// move with the attack order cleared. **VERA-internal; the native
/// cell-as-target encoding is not reproduced, only its observable effect.**
fn hold_child_over_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    child_id: u64,
    direction: u8,
) {
    let Some((owner_rx, owner_ry)) = sim
        .substrate
        .entities
        .get(owner_id)
        .map(|o| (o.position.rx, o.position.ry))
    else {
        return;
    };
    let (rx, ry) = adjacent_cell(owner_rx, owner_ry, direction);
    let speed = child_air_speed(sim, rules, child_id);
    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
        child.attack_target = None;
        if let Some(mission) = child.aircraft_mission.as_mut() {
            *mission = crate::sim::aircraft::AircraftMission::Move { sub_state: 0 };
        }
    }
    sim.issue_air_cell_destination(child_id, (rx, ry), speed, Some(rules));
}

/// The eight-direction cell step native uses for the owner-relative hold cell.
fn adjacent_cell(rx: u16, ry: u16, direction: u8) -> (u16, u16) {
    const OFFSETS: [(i32, i32); 8] = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    let (dx, dy) = OFFSETS[(direction & 7) as usize];
    (
        (rx as i32 + dx).max(0) as u16,
        (ry as i32 + dy).max(0) as u16,
    )
}

/// No FASTER stage: spawned children fly, and neither `FlyLocomotionClass` nor
/// the rocket controller calls the `FootClass::GetCurrentSpeed` vtable slot
/// (`veterancy::locomotor_consults_current_speed`).
fn child_air_speed(
    sim: &Simulation,
    rules: &RuleSet,
    child_id: u64,
) -> crate::util::fixed_math::SimFixed {
    sim.substrate
        .entities
        .get(child_id)
        .map(|c| sim.interner.resolve(c.type_ref()).to_string())
        .and_then(|name| rules.object(&name))
        .map(|obj| crate::util::fixed_math::ra2_speed_to_leptons_per_second(obj.speed.max(1)))
        .unwrap_or(crate::util::fixed_math::SimFixed::from_num(8))
}

/// Point an aircraft child back at its parent and clear its attack order.
fn recall_child_to_owner(sim: &mut Simulation, rules: &RuleSet, owner_id: u64, child_id: u64) {
    let Some((rx, ry)) = sim
        .substrate
        .entities
        .get(owner_id)
        .map(|o| (o.position.rx, o.position.ry))
    else {
        return;
    };
    let speed = child_air_speed(sim, rules, child_id);
    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
        child.attack_target = None;
        if let Some(mission) = child.aircraft_mission.as_mut() {
            *mission = crate::sim::aircraft::AircraftMission::Move { sub_state: 0 };
        }
    }
    sim.issue_air_cell_destination(child_id, (rx, ry), speed, Some(rules));
}

/// Hand a launched missile child to the rocket locomotor with the impact
/// payload attached.
///
/// **DRIFT (largely closed 2026-08-03) — missile flight arc.** The original
/// record here predicted "the arc can be replaced in place by porting
/// `RocketLocomotionClass` behind the same `RocketState` handoff" — that is
/// exactly what happened: the foundations merge landed the six-phase machine
/// (`ILoco::Process 0x006622C0` — ignition → tilt → ascent → cruise →
/// terminal → secondary) in `rocket_movement`, and this launch path now rides
/// it via `RocketFlightParameters::legacy`. What REMAINS open, recorded here:
/// `legacy()` uses default acceleration/altitude/tilt constants rather than
/// the per-family `*Acceleration`/`*Altitude`/`*LazyCurve`/`*TurnRate` table
/// values (still unparsed — floats whose fixed-point form belongs to whoever
/// wires the table), and the DMisl vertical raise is not selected. Silhouette
/// nuance only; impact cell, damage and warhead are exact, and flight duration
/// feeds nothing (the regen clock is Rules `PauseFrames + TiltFrames`).
///
/// **DRIFT — launch position and effects.** Native reads the muzzle through
/// `GetFLH` and offsets the launch Z by +10 leptons; the Boomer additionally
/// subtracts a hardcoded XY offset and spawns an underwater smoke anim. VERA
/// launches from the parent's own cell centre with no Z offset, no CMisl
/// offset (`DAT_0084009c`/`DAT_008400a0` values **UNCHECKED**) and no smoke.
/// Sub-cell visual only; no gameplay input reads it.
fn launch_missile_child(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    child_id: u64,
    target_cell: (u16, u16),
    family: Option<MissileFamily>,
    owner_veterancy: u16,
) {
    let Some(family) = family else { return };
    let params = rules.missile_spawn.params(family);
    let (target_rx, target_ry) = target_cell;
    let (origin_rx, origin_ry) = match sim.substrate.entities.get(child_id) {
        Some(c) => (c.position.rx, c.position.ry),
        None => return,
    };
    let child_type = sim
        .substrate
        .entities
        .get(child_id)
        .map(|c| sim.interner.resolve(c.type_ref()).to_string());
    // The six-phase rocket machine runs in LEPTONS per second (its ascent
    // altitude, acceleration and terminal constants are lepton-domain, and its
    // own attach test uses a 300-scale speed), so the raw INI `Speed=` goes
    // through the leptons/s conversion. Two prior unit bugs on this exact line:
    // the raw value (one cell per frame), then cells/s into the lepton-domain
    // machine (~256x too slow — the missile never finished its ascent).
    let speed = child_type
        .as_deref()
        .and_then(|name| rules.object(name))
        .map(|o| crate::util::fixed_math::ra2_speed_to_leptons_per_second(o.speed.max(1)))
        .unwrap_or_else(|| crate::util::fixed_math::ra2_speed_to_leptons_per_second(15));
    let warhead_id = sim.interner.intern(params.warhead_for(owner_veterancy));
    let payload = crate::sim::movement::rocket_movement::RocketPayload {
        warhead: warhead_id,
        damage: params.damage_for(owner_veterancy),
        firer_id: owner_id,
    };
    crate::sim::movement::rocket_movement::attach_rocket_state_with_payload(
        &mut sim.substrate.entities,
        child_id,
        (origin_rx, origin_ry),
        (target_rx, target_ry),
        speed,
        Some(payload),
    );
}

/// One missile impact, queued for the combat phase to resolve.
///
/// `RocketLocomotion::Detonate` (`0x00663030`) selects the warhead by missile
/// family and elite flag and then calls the engine's shared area-damage
/// routine — the same one the ordinary warhead, bomb, nuke and lightning paths
/// call. It does NOT have a private damage applicator. VERA's equivalent shared
/// path is combat's damage → death → despawn pipeline, so the detonation
/// records itself here and combat expands it; reimplementing the damage
/// application in this module produced targets that could be damaged but never
/// killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissileDetonation {
    pub rx: u16,
    pub ry: u16,
    /// Interned warhead section name (family + elite selected at launch).
    pub warhead: InternedId,
    pub damage: i32,
    /// Launcher stable id — kill credit and retaliation.
    pub firer_id: u64,
    /// House the missile carried, for the area-damage owner argument.
    pub owner: InternedId,
}

/// Consume every missile that reached its target during this tick's movement
/// pass: queue its impact for the combat phase and remove the missile.
///
/// The queue is drained inside `tick_combat_*` in the same tick, before the
/// damage phase, so the impact runs the identical damage → `dead_entities` →
/// `handle_entity_deaths` → despawn sequence as any ordinary bullet. Death
/// weapons, crew ejection, explosion anims, smudges and kill credit all come
/// from that shared path rather than from this module.
///
/// **DRIFT — within-tick timing.** Native applies the damage inline in the
/// locomotor pass, so a unit killed by an impact never gets a combat pass that
/// tick; here the damage lands a phase later, so a unit that will die to the
/// impact still fires once. Trigger: every missile impact that kills.
/// Player effect: one extra shot from a doomed unit, sub-tick. Frequency:
/// every killing impact. Downstream risk: none beyond the extra shot — it is
/// the same phase-batching VERA already applies to all combat damage, and the
/// alternative (a private applicator in this module) is what let targets sit
/// at zero health.
pub fn detonate_missiles(sim: &mut Simulation, detonated: &[u64]) {
    for &missile_id in detonated {
        let Some((rx, ry, payload, owner)) = sim.substrate.entities.get(missile_id).and_then(|e| {
            e.rocket_state
                .as_ref()
                .map(|r| (r.target_rx, r.target_ry, r.payload, e.owner()))
        }) else {
            continue;
        };
        if let Some(payload) = payload {
            sim.pending_missile_detonations.push(MissileDetonation {
                rx,
                ry,
                warhead: payload.warhead,
                damage: payload.damage,
                firer_id: payload.firer_id,
                owner,
            });
        }
        sim.uninit(missile_id);
    }
}

/// Tear down a parent's whole pool.
///
/// `SpawnManagerClass::Kill_All_Spawns` (`decompile_function 0x006B7100`,
/// re-read 2026-08-03) walks the slots backwards, skips any already in
/// `Regenerating`, and has exactly three arms:
///
/// - **`ReadyDocked` / `Reloading`** — call the child's destroy slot
///   (`vtable+0xF8`), null the slot pointer, arm the regen timer.
/// - **`KamikazeWait`** — a missile that has *already left the launcher*:
///   remove it from the retreat list, then call the same destroy slot. The
///   in-flight missile dies with its launcher; the salvo does **not** land.
/// - **everything else** (`InFlight` / `ReturningToDock` / `LandingAtDock`,
///   i.e. the aircraft states) — push the child onto the global retreat list so
///   it keeps flying toward the last target. This is the "Hornets keep going
///   after the Carrier sinks" behaviour.
///
/// The regen duration is `SpawnRegenRate` when the owner is dead
/// (`owner.Health < 1 || !owner.IsAlive`) and **zero** when it is still alive —
/// so an ownership change or a deploy rebuilds the pool on the next AI pass
/// rather than after a full regen wait.
///
/// VERA has no global retreat list, so the aircraft arm releases the child
/// (clears `spawn_owner_id`) and leaves it flying instead of re-issuing a
/// destination each tick. **VERA-internal; the retreat list's per-tick
/// re-issue and its `HP = 1` marking are UNCHECKED.**
///
/// This routine never touches `CurrentTarget`/`QueuedTarget`; the caller
/// decides. `Simulation::uninit` pairs it with `ClearAllTargets`, matching the
/// owner arm of `SpawnManagerClass::PointerExpired`, while
/// `Simulation::change_owner` calls only this one. The *target* arm of the same
/// native routine also clears targets and is a separate path entirely — see
/// [`notify_pointer_expired`].
///
/// Native callers of this routine, and which are wired here:
/// - `SpawnManagerClass::PointerExpired(owner)` — WIRED, via
///   `Simulation::uninit`, together with the `ClearAllTargets` it pairs with.
/// - `TechnoClass::ChangeOwner` (`0x0070157E`) — WIRED, via
///   `Simulation::change_owner`; this is the mind-control path.
/// - `TemporalClass::InitiateWarp` (`0x0071AF39`) — **not wired.** A
///   chrono-warped V3/Dreadnought keeps its pool across the warp. Fires only
///   when a Chrono Legionnaire targets one of the five spawner units.
/// - `TechnoClass::PerformDeploy` (`0x00710021`) — **not wired.** No stock
///   spawner unit sets `DeploysInto=`, so this arm is unreachable in stock YR.
/// - `FootClass::StopFiring` → `FUN_006fcd40` — **not wired**, and gated on
///   `owner+0x6AD` which was not traced. UNCHECKED.
/// - The destructor — covered by the `uninit` hook.
/// `SpawnManagerClass::PointerExpired` (`decompile_function 0x006B7C60`) for
/// one listening manager, minus the owner arm.
///
/// The native routine is an else-if chain and the **first** arm is the target
/// arm — this is the only mechanism in the engine that drops a destroyed
/// target. `SetTarget` writes `QueuedTarget` only, and the AI's promote is
/// `if (QueuedTarget != 0) CurrentTarget = QueuedTarget`, so a queued NULL is
/// never promoted and `Assign_Target(NULL)` alone cannot clear a live target.
///
/// ```text
/// if      (expired == CurrentTarget) { CurrentTarget = 0;
///                                      if (QueuedTarget == 0) ClearAllTargets(); }
/// else if (expired == QueuedTarget)  { QueuedTarget = 0; }
/// else if (expired is a slot child)  { slot.Spawn = 0; slot.State = 7;
///                                      slot.Timer = SpawnRegenRate; }
/// else if (expired == Owner)         { Kill_All_Spawns(); ClearAllTargets(); }
/// ```
///
/// Without the target arm a Carrier whose wing scores a kill keeps the corpse
/// as `CurrentTarget` forever: the manager cycles Returning → Idle → Launching
/// and sends the whole wing back out at nothing. The Hornets never fire, so
/// their ammo never reaches zero, so the recall condition never fires and they
/// hover until the player issues a fresh order.
///
/// The owner arm is handled at `Simulation::uninit`, which calls
/// [`kill_all_spawns`] and [`clear_all_spawn_targets`] directly.
///
/// **Slot-arm difference.** Native guards the slot arm with an *alive-child*
/// test — `child+0x6C > 0 && child+0x6CA == 0 && node+0x14 != 1`, i.e. health
/// above zero, not already on the retreat list, and not a missile slot.
/// `+0x6C` is Health, proven by state 6 writing `childType+0xA0` (`Strength=`)
/// into `+0x6C`/`+0x70`. The guard exists because native delivers
/// `PointerExpired` for *living* children too, on limbo — a Hornet docking
/// would otherwise expire its own slot. Omitting it is safe here only because
/// `Simulation::techno_limbo` does not run the expiry broadcast, so this
/// function is never called for a child that is merely being limboed.
/// Recorded, not implemented.
pub fn notify_pointer_expired(sim: &mut Simulation, listener_id: u64, expired_id: u64) {
    if listener_id == expired_id {
        // The owner arm; `Simulation::uninit` already ran it.
        return;
    }
    let Some((current, queued, slot_index, regen_rate)) = sim
        .substrate
        .entities
        .get(listener_id)
        .and_then(|e| e.spawn_manager.as_ref())
        .map(|m| {
            (
                m.current_target,
                m.queued_target,
                m.slot_index_of(expired_id),
                m.regen_rate,
            )
        })
    else {
        return;
    };
    let expired_target = TargetKind::Entity(expired_id);

    if current == Some(expired_target) {
        let queued_empty = queued.is_none();
        with_manager(sim, listener_id, |m| {
            m.current_target = None;
            if queued_empty {
                m.clear_all_targets();
            }
        });
        return;
    }
    if queued == Some(expired_target) {
        with_manager(sim, listener_id, |m| m.queued_target = None);
        return;
    }
    if let Some(index) = slot_index {
        let frame = sim.session.binary_frame;
        with_slot(sim, listener_id, index, |slot| {
            slot.spawn = None;
            slot.state = SpawnSlotState::Regenerating;
            slot.timer = SpawnTimer::armed(frame, regen_rate);
        });
    }
}

/// `SpawnManagerClass::ClearAllTargets` (`0x006B7BB0`) as a standalone call.
///
/// Kept separate from [`kill_all_spawns`] because the two are separate native
/// calls: the owner-expired path makes both, the ownership-change path makes
/// only the kill.
///
/// **Omission recorded.** The native routine is not a pure field reset: before
/// zeroing the two targets and the mode, it walks the slots and, for any
/// state-2 slot whose child type sets `MissileSpawn=`, pushes that child onto
/// the retreat list and expires the slot. VERA does that walk nowhere.
///
/// There are three callers of the native clear, not two. On the two that reach
/// this public function — `Simulation::uninit` and the Launching arm — the
/// omission is inert: no stock aircraft-flavoured child sets `MissileSpawn=`,
/// and a missile-flavoured slot has already left state 2 for `KamikazeWait`.
/// The third is the target arm of [`notify_pointer_expired`], and there it is
/// **not** inert: a Dreadnought or Boomer slot can still be `InFlight` while it
/// waits out the 20-frame inter-launch delay. Native frees that slot at
/// target-death; VERA frees it later, when the missile impacts, so the regen
/// clock starts late by the residual flight time. Trigger: a Dreadnought or
/// Boomer target dying to other damage inside that window. Player effect: the
/// next salvo arrives slightly later. Frequency: occasional in naval fights.
/// Downstream risk: none — it shifts one timer, feeds nothing else.
pub fn clear_all_spawn_targets(sim: &mut Simulation, owner_id: u64) {
    with_manager(sim, owner_id, |m| m.clear_all_targets());
}

pub fn kill_all_spawns(sim: &mut Simulation, owner_id: u64) {
    kill_all_spawns_with_context(sim, owner_id, UninitContext::default());
}

pub(crate) fn kill_all_spawns_with_context(
    sim: &mut Simulation,
    owner_id: u64,
    context: UninitContext<'_>,
) {
    let Some((slots, regen_rate)) = sim.substrate.entities.get(owner_id).and_then(|e| {
        e.spawn_manager
            .as_ref()
            .map(|m| (m.slots.clone(), m.regen_rate))
    }) else {
        return;
    };
    let owner_alive = sim
        .substrate
        .entities
        .get(owner_id)
        .is_some_and(|o| o.health.current >= 1 && o.lifecycle.object_alive && !o.dying);
    let regen_duration = if owner_alive { 0 } else { regen_rate };
    let frame = sim.session.binary_frame;

    // The native body is entirely inside `if (state != 7)`, timer write
    // included: a slot already regenerating keeps its running countdown and is
    // not touched at all. Nothing in this routine reads or writes the manager's
    // targets either — `ClearAllTargets` is a separate call that only the
    // owner-expired path makes alongside this one.
    for (index, slot) in slots.into_iter().enumerate() {
        if slot.state == SpawnSlotState::Regenerating {
            continue;
        }
        if let Some(child_id) = slot.spawn {
            match slot.state {
                // Docked, reloading, or a missile still inside its post-launch
                // tilt window: all three take the destroy slot.
                SpawnSlotState::ReadyDocked
                | SpawnSlotState::Reloading
                | SpawnSlotState::KamikazeWait => {
                    sim.uninit_with_context(child_id, context);
                }
                // Aircraft already out: released, not destroyed.
                SpawnSlotState::InFlight
                | SpawnSlotState::ReturningToDock
                | SpawnSlotState::LandingAtDock => {
                    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
                        child.spawn_owner_id = None;
                    }
                }
                SpawnSlotState::Regenerating => unreachable!("skipped above"),
            }
        }
        with_slot(sim, owner_id, index, |slot| {
            slot.spawn = None;
            slot.state = SpawnSlotState::Regenerating;
            slot.timer = SpawnTimer::armed(frame, regen_duration);
        });
    }
}
