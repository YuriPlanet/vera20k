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
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use serde::{Deserialize, Serialize};

use crate::rules::missile_spawn::MissileFamily;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::sim::world::{Simulation, UninitContext};
use crate::util::direction_tables::CELL_DELTAS;

/// Frames the manager waits before its very first AI pass
/// (`UpdateTimer.Duration = 0x14` at construction).
const FIRST_UPDATE_DELAY_FRAMES: i32 = 20;
/// Frames between AI passes once the manager has run at least once.
const UPDATE_PERIOD_FRAMES: i32 = 10;
/// Per-launch delay written to the manager's reload timer when the *parent*
/// type does not set `MissileSpawn=`. No stock YR parent sets it, so this is
/// the only branch reachable in stock play.
const LAUNCH_DELAY_FRAMES: u32 = 20;
/// Per-launch delay when the parent type sets `MissileSpawn=yes`. Unreachable
/// in stock YR; kept because the branch is data-driven, not gated.
const LAUNCH_DELAY_FRAMES_MISSILE_PARENT: u32 = 9;
/// Height difference (leptons) under which a returning child counts as docked.
const DOCK_HEIGHT_EPSILON_LEPTONS: i32 = 0x14;
/// Leptons a launch adds to its GetFLH Z (`ADD EAX,0xa` at `0x006B74B2`).
const LAUNCH_Z_LIFT_LEPTONS: i32 = 10;
/// X and Y a `CMislType=` launch takes off its GetFLH coordinate: the dwords
/// at `0x0084009C` and `0x008400A0` (`0x006B74C4`, `0x006B74CA`), 40 and 40 in
/// the retail bytes. Those two reads are their only references.
const CMISL_LAUNCH_OFFSET_LEPTONS: [i32; 2] = [40, 40];

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
    /// The slot's timer. A ready slot's is left paused with no time left
    /// ([`CdTimer::default`]); native restarts it at the current frame with
    /// none, which reads the same.
    pub timer: CdTimer,
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
    pub update_timer: CdTimer,
    /// Gates launches across the pool, not per slot.
    pub reload_timer: CdTimer,
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
                    .filter(|child| !child.lifecycle.in_limbo)
                    .and_then(|child| rules.object(interner.resolve(child.type_ref())))
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

    /// The tail of `SpawnManagerClass::ClearAllTargets` (`0x006B7C26..`): drop
    /// both targets and fall back to `Idle`. [`clear_all_spawn_targets`] walks
    /// the slots first.
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
            timer: CdTimer::default(),
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
        update_timer: CdTimer::started(frame as i32, FIRST_UPDATE_DELAY_FRAMES),
        reload_timer: CdTimer::default(),
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
        // A warped owner's AI_Update does not run (`GameEntity::ai_frozen`),
        // so the spawns its warp start killed wait for its release.
        let has_manager = sim.substrate.entities.get(owner_id).is_some_and(|e| {
            e.spawn_manager.is_some() && e.lifecycle.object_alive && !e.ai_frozen()
        });
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
        if !manager.update_timer.expired(frame as i32) {
            return;
        }
        manager.update_timer = CdTimer::started(frame as i32, UPDATE_PERIOD_FRAMES);
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
        step_slot(sim, rules, owner_id, slot_index, frame, overlay_registry);
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
        slot.timer = CdTimer::started(frame as i32, regen_rate as i32);
    }
}

fn step_slot(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    slot_index: usize,
    frame: u32,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
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
            if slot.timer.expired(frame as i32) {
                // The missile is on its own now; free the slot to regenerate.
                let regen_rate = manager_field(sim, owner_id, |m| m.regen_rate).unwrap_or(0);
                with_slot(sim, owner_id, slot_index, |slot| {
                    slot.spawn = None;
                    slot.state = SpawnSlotState::Regenerating;
                    slot.timer = CdTimer::started(frame as i32, regen_rate as i32);
                });
            }
        }
        SpawnSlotState::InFlight => step_in_flight(sim, rules, owner_id, slot_index),
        SpawnSlotState::ReturningToDock => step_returning(sim, rules, owner_id, slot_index),
        SpawnSlotState::LandingAtDock => {
            step_landing(sim, rules, owner_id, slot_index, frame, registry)
        }
        SpawnSlotState::Reloading => {
            if slot.timer.expired(frame as i32) {
                restore_docked_child(sim, rules, owner_id, slot_index);
            }
        }
        SpawnSlotState::Regenerating => {
            if slot.timer.expired(frame as i32) {
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
                m.reload_timer.expired(frame as i32),
                m.mode,
                m.slots[slot_index].is_missile_spawn,
                m.slots[slot_index].spawn,
            )
        })
    else {
        return;
    };
    if target.is_none() || !reload_due || mode == SpawnManagerMode::Returning {
        return;
    }
    let Some(child_id) = child_id else { return };

    let Some(owner) = sim.substrate.entities.get(owner_id) else {
        return;
    };
    // A missile slot (`SpawnControl+0x14`) launches only while the parent's
    // locomotor answers false to Is_Moving (`0x006B731E`) and Is_Moving_Now
    // (`0x006B7349`). Aircraft slots skip it, so Hornets launch from a moving
    // Carrier.
    if is_missile_slot
        && (crate::sim::movement::motion_query::is_moving(owner) == Some(true)
            || crate::sim::movement::motion_query::is_moving_now(
                owner,
                Some(crate::sim::movement::SpeedRules::new(
                    rules,
                    &sim.interner,
                    &sim.type_handles,
                    &sim.houses,
                )),
                frame,
            ))
    {
        return;
    }
    // SpawnManager6B737C reads Foot+6AD, the locomotor-swap latch,
    // independent of Unit deployment6E0..6E2.
    if owner.foot_locomotor_swap_active {
        return;
    }

    let owner_type = sim.interner.resolve(owner.type_ref()).to_string();
    let owner_level = owner.position.z;
    // The Unlimbo direction: the owner's PrimaryFacing `Current()` rounded to
    // a DirType (`0x006B74E9..0x006B74FC`).
    let launch_dir = owner.body_facing_dir(frame);
    let parent_missile_spawn = rules
        .object(&owner_type)
        .map(|o| o.missile_spawn)
        .unwrap_or(false);

    let launch_delay = if parent_missile_spawn {
        LAUNCH_DELAY_FRAMES_MISSILE_PARENT
    } else {
        LAUNCH_DELAY_FRAMES
    };

    let Some(launch) = launch_coordinate(sim, rules, owner_id, slot_index, is_missile_slot) else {
        return;
    };
    // The child's Unlimbo (vt+0xD8, `0x006B7505`) at that coordinate, staged
    // on its limbo Location for Reveal: a `MissileSpawn=` type keeps it whole
    // (`unlimbo_z`); an aircraft's Z is replaced there. Its Unlimbo snaps the
    // body to the direction (`0x006F6DAA`). The coarse level stays the owner's.
    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
        crate::sim::movement::ground_pose::put_location(&mut child.position, launch);
        child.position.z = owner_level;
        child.body_facing.snap(u16::from(launch_dir) << 8, frame);
    }
    let revealed = matches!(
        sim.reveal_entity_with_rules(child_id, rules),
        crate::sim::world::RevealOutcome::Revealed { .. }
    );
    if !revealed {
        return;
    }

    if is_missile_slot {
        launch_missile_child(sim, rules, owner_id, child_id);
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
        m.reload_timer = CdTimer::started(frame as i32, launch_delay as i32);
        m.slots[slot_index].state = SpawnSlotState::InFlight;
    });
}

/// The coordinate `SpawnManagerClass::AI` case 0 unlimbos a slot's child at
/// (`0x006B73C4..0x006B74D7`), with its write to the owner's burst index.
///
/// - A missile slot whose owner's GetWeapon(0) (vt+0x3F8, elite-aware) has
///   `Burst > 1` sets the owner's burst index (`+0x3B8`) to the slot index's
///   parity (`0x006B73CF..0x006B73F6`) and stores 0 after the child's
///   Unlimbo (`0x006B757A..0x006B7585`). Nothing in between reads it but
///   this GetFLH, so the parity goes in as GetFLH's argument and the store
///   happens here.
/// - GetFLH (vt+0xB0) is asked for weapon 0 when GetWeapon(0) is `Spawner=`
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
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: u64,
    slot_index: usize,
    is_missile_slot: bool,
) -> Option<crate::sim::components::DriveCoord> {
    use crate::sim::combat::fire_coord;

    let spawn_type = manager_field(sim, owner_id, |m| m.spawn_type)?;
    let owner = sim.substrate.entities.get(owner_id)?;
    let obj = sim.object_type(owner.type_ref(), rules)?;
    let weapon = crate::sim::combat::combat_weapon::primary_for_tier(obj, owner.veterancy())
        .and_then(|id| rules.weapon(id));
    let sets_burst = is_missile_slot && weapon.is_some_and(|weapon| weapon.burst > 1);
    let burst = if sets_burst {
        (slot_index & 1) as i32
    } else {
        owner.weapon_burst.index()
    };
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
    // Native tests the pool's spawn type pointer against Rules' CMislType
    // once (`0x006B74BC`); a type name names one type, so the name compare is
    // that test.
    if sim
        .interner
        .resolve(spawn_type)
        .eq_ignore_ascii_case(&rules.missile_spawn.cmisl.type_name)
    {
        coord.x = coord.x.wrapping_sub(CMISL_LAUNCH_OFFSET_LEPTONS[0]);
        coord.y = coord.y.wrapping_sub(CMISL_LAUNCH_OFFSET_LEPTONS[1]);
    }
    if sets_burst && let Some(owner) = sim.substrate.entities.get_mut(owner_id) {
        owner.weapon_burst.reset();
    }
    Some(coord)
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
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
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
        sim.techno_limbo_with_rules(child_id, rules, registry);
        let reload_rate = manager_field(sim, owner_id, |m| m.reload_rate).unwrap_or(0);
        with_slot(sim, owner_id, slot_index, |slot| {
            slot.state = SpawnSlotState::Reloading;
            slot.timer = CdTimer::started(frame as i32, reload_rate as i32);
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
        slot.timer = CdTimer::default();
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
                owner.body_facing_byte(sim.session.binary_frame),
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
        slot.timer = CdTimer::default();
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
            // `TechnoClass::CanFireAtTarget` @ 0x006F7780 (InRange with the
            // selected weapon). A false result calls `ClearAllTargets` @
            // 0x006B7BB0 and returns before Launching.
            let can_fire_at = sim.resolved_terrain.as_ref().is_some_and(|terrain| {
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
            if !can_fire_at {
                clear_all_spawn_targets(sim, owner_id, Some(rules), overlay_registry);
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
                clear_all_spawn_targets(sim, owner_id, Some(rules), overlay_registry);
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
                    // The child has left the launcher for good: the kamikaze
                    // tracker takes it (`0x006B7A37`) and runs 2 frames later.
                    cleared_targets = true;
                    if let Some(child) = child_slot {
                        let target = manager_field(sim, owner_id, |m| m.current_target).flatten();
                        sim.kamikaze_push(child, target, rules, overlay_registry);
                        sim.kamikaze_restart_after_push();
                    }
                    if is_missile_family {
                        with_slot(sim, owner_id, index, |slot| {
                            slot.state = SpawnSlotState::KamikazeWait;
                            slot.timer = CdTimer::started(frame as i32, kamikaze_frames as i32);
                        });
                    } else if let Some(child) = child_slot {
                        // No kamikaze window: PointerExpired (`0x006B7ACD`)
                        // frees the slot, which starts regenerating now.
                        notify_pointer_expired(sim, owner_id, child, Some(rules), overlay_registry);
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
            if cleared_targets {
                clear_all_spawn_targets(sim, owner_id, Some(rules), overlay_registry);
            }
            with_manager(sim, owner_id, |m| m.mode = SpawnManagerMode::Returning);
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

fn child_ammo(sim: &Simulation, child_id: u64) -> i32 {
    sim.substrate
        .entities
        .get(child_id)
        .and_then(|c| c.aircraft_ammo.as_ref())
        .map(|a| a.current)
        // Real Aircraft now retain their signed count, including -1. This
        // fallback applies only to incomplete/non-Aircraft compatibility data.
        .unwrap_or(i32::MAX)
}

/// `child.vt+0x3C8 Assign_Target(target)` then `vt+0x1E8 Queue_Mission(Attack,
/// 0)` (slot states 3 and 4, and the send-out, `0x006B7718`/`0x006B772C`).
/// Re-issued every manager pass, both are no-ops on a child already attacking
/// that target: Assign_Target returns on the same target (`0x006FCDCC`) and
/// Queue_Mission skips the mission already current (`0x005B35E0`). A new
/// target mid-run is Assign_Target's aircraft arm (`0x006FCE27`).
///
/// The queue goes through the mission owner: `AircraftClass::AI` pays a
/// pending release (`0x0041505E`) whenever the current mission is not Attack,
/// so a child whose mission owner never saw Attack paid its pass's ammo the
/// frame after its first bomb and was recalled.
fn assign_child_attack(sim: &mut Simulation, child_id: u64, target: TargetKind) {
    let commits = crate::sim::mission::concrete_effects::assign_target_commits(
        &sim.substrate.entities,
        Some(target),
    );
    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
        crate::sim::mission::concrete_effects::represented_assign_target_admitted(
            child,
            Some(target),
            commits,
        );
    }
    queue_child_mission(sim, child_id, crate::sim::mission::MissionType::Attack);
    if let Some(child) = sim.substrate.entities.get_mut(child_id)
        && let Some(mission) = child.aircraft_mission.as_mut()
        && !mission.is_attacking()
    {
        *mission = crate::sim::aircraft::AircraftMission::Attack { sub_state: 0 };
    }
}

/// The manager's `child.vt+0x1E8 Queue_Mission(mission, 0)` through the
/// child's mission owner (the aircraft override's guards included).
fn queue_child_mission(
    sim: &mut Simulation,
    child_id: u64,
    mission: crate::sim::mission::MissionType,
) {
    let _ = sim.mission_queue_exact(
        child_id,
        crate::sim::mission::MissionId::from_known(mission),
        0,
        sim.session.binary_frame,
        &crate::sim::mission::authority::EntityReadyInputProvider,
    );
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
    // `Queue_Mission(Move, 0)` (`0x006B7608`, `0x006B76D8`).
    queue_child_mission(sim, child_id, crate::sim::mission::MissionType::Move);
    sim.issue_air_cell_destination(child_id, (rx, ry), speed, Some(rules));
}

/// The eight-direction cell step native uses for the owner-relative hold cell.
fn adjacent_cell(rx: u16, ry: u16, direction: u8) -> (u16, u16) {
    let (dx, dy) = CELL_DELTAS[(direction & 7) as usize];
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
        .map(|c| {
            crate::sim::movement::order_speed(
                c,
                sim.object_type(c.type_ref(), rules),
                Some(rules),
                &sim.houses,
            )
        })
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
    // `Queue_Mission(Move, 0)` (`0x006B7687`, `0x006B785C`).
    queue_child_mission(sim, child_id, crate::sim::mission::MissionType::Move);
    sim.issue_air_cell_destination(child_id, (rx, ry), speed, Some(rules));
}

/// Case 0's missile tail after the child's Unlimbo (`0x006B750B..0x006B75CA`).
///
/// - A `CMislType=` pool puffs `V3TAKOFF` at the child (`[0x00840098]` names
///   it too): `AnimClass(type, &Location, 2, 1, 0x600, -10, 0)`. The
///   constructor draws only for `RandomRate=`, `IsMeteor=` or `Bouncer=`,
///   which retail `[V3TAKOFF]` leaves unset.
/// - The owner's burst index reset precedes it in [`launch_coordinate`].
/// - The queued target is promoted (`0x006B759B..0x006B75A5`), then the child
///   is sent at `CurrentTarget`: `Assign_Destination(target, 1)` (vt+0x480,
///   `AircraftClass` `0x0041AA80`), whose Foot setter hands the target's
///   coordinate to Rocket Move_To (`rocket_movement::move_to`), and
///   `Queue_Mission(Move, 0)` (vt+0x1E8).
///
/// The manager's next Launching pass hands the missile to the kamikaze
/// tracker (`sim::kamikaze`), which gives it the target's cell and the
/// Attack mission.
///
/// RESIDUAL (next chain): the missile's own mission never runs, because the
/// aircraft mission runner skips non-Fly locomotors. Natively each
/// `Mission_Move @ 0x004166E0` visit from substate 0 (and each
/// `Mission_Attack @ 0x00417FE0` epilogue) draws Scenario `RandomRanged(0,2)`,
/// and Mission_Attack's substate 1 sends the missile at the tracker's cell
/// (`Assign_Destination(FindFireLocation)`). Trigger: every launch. Effect:
/// no flight change while Rocket Move_To holds its destination, but the
/// Scenario RNG stream falls behind native from the first launch; a missile
/// whose launch dropped a high-flying target (no destination) stays on its
/// launcher instead of being re-sent at the target's cell. Risk: RNG parity
/// and that stranded missile.
fn launch_missile_child(sim: &mut Simulation, rules: &RuleSet, owner_id: u64, child_id: u64) {
    let boomer_pool = manager_field(sim, owner_id, |m| m.spawn_type).is_some_and(|spawn_type| {
        sim.interner
            .resolve(spawn_type)
            .eq_ignore_ascii_case(&rules.missile_spawn.cmisl.type_name)
    });
    if boomer_pool && let Some(child) = sim.substrate.entities.get(child_id) {
        let location = crate::sim::movement::ground_pose::position_world_coord(&child.position);
        sim.spawn_named_anim(
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
    with_manager(sim, owner_id, |m| m.promote_queued_target());
    let target = manager_field(sim, owner_id, |m| m.current_target).flatten();
    sim.assign_aircraft_attack_destination(
        child_id,
        target.map(crate::sim::components::NavTargetRef::from),
        rules,
    );
    queue_child_mission(sim, child_id, crate::sim::mission::MissionType::Move);
}

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
/// else if (expired is a slot child)  { if (child alive) return;  // see below
///                                      slot.Spawn = 0; slot.State = 7;
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
/// The owner arm is handled by `Simulation::spawn_manager_owner_expired`, which
/// calls [`kill_all_spawns_with_context`] and [`clear_all_spawn_targets`] directly.
///
/// **Slot-arm alive-child guard** (`0x006B7CDD..0x006B7CF2`): the slot is
/// kept, and nothing else happens, while `child+0x6C > 0 && child+0x6CA == 0
/// && node+0x14 != 1`, i.e. the child has Health, is not on the kamikaze
/// tracker, and the slot is not a missile slot. `+0x6C` is Health, proven by
/// state 6 writing `childType+0xA0` (`Strength=`) into `+0x6C`/`+0x70`. The
/// guard exists because `ObjectClass::Limbo` broadcasts (`0x005F4D61`), and a
/// Hornet docking through `step_landing`'s Limbo would otherwise expire its own
/// slot and strand itself in limbo. `+0x6CA` is the tracker's membership: on
/// an aircraft the constructor clears it (`0x00413D4E`) and only Push sets it
/// (`0x0054E47D`), as it appends the node; the node goes at the child's
/// pointer expiry, after the listeners (`0x00725972`), so this guard still
/// sees it.
///
/// `rules` lets the target arm's ClearAllTargets hand state-2 missiles to the
/// tracker ([`clear_all_spawn_targets`]).
pub fn notify_pointer_expired(
    sim: &mut Simulation,
    listener_id: u64,
    expired_id: u64,
    rules: Option<&RuleSet>,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    if listener_id == expired_id {
        // The owner arm; `Simulation::spawn_manager_owner_expired` already ran it.
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
        with_manager(sim, listener_id, |m| m.current_target = None);
        if queued.is_none() {
            clear_all_spawn_targets(sim, listener_id, rules, registry);
        }
        return;
    }
    if queued == Some(expired_target) {
        with_manager(sim, listener_id, |m| m.queued_target = None);
        return;
    }
    if let Some(index) = slot_index {
        let child_alive = sim
            .substrate
            .entities
            .get(expired_id)
            .is_some_and(|child| child.health.current > 0);
        let missile_slot =
            manager_field(sim, listener_id, |m| m.slots[index].is_missile_spawn).unwrap_or(false);
        if child_alive && !sim.kamikaze.contains(expired_id) && !missile_slot {
            return;
        }
        let frame = sim.session.binary_frame;
        with_slot(sim, listener_id, index, |slot| {
            slot.spawn = None;
            slot.state = SpawnSlotState::Regenerating;
            slot.timer = CdTimer::started(frame as i32, regen_rate as i32);
        });
    }
}

/// `SpawnManagerClass::ClearAllTargets` (`0x006B7BB0`).
///
/// Each slot in state 2 whose child's type sets `MissileSpawn=` (`+0xD68`)
/// hands its child to the kamikaze tracker with the live target
/// (`SpawnRetreat__Push`, `0x006B7BEB`; NULL when the target arm of
/// [`notify_pointer_expired`] has just dropped it, so the missile takes the
/// cell ahead of its facing), restarts the tracker for 2 frames
/// (`0x006B7BF0`) and expires the child through [`notify_pointer_expired`]
/// (`0x006B7C16`), which frees the slot: the Push has just tracked it. Then
/// both targets and the mode reset.
///
/// A Dreadnought or Boomer slot is still in state 2 while the manager waits
/// out the inter-launch delay, so a target that dies then frees the slot at
/// once and its missile flies on under the tracker.
///
/// Native callers: the manager AI (no target, out of range, after the
/// Launching pass), the target arm of PointerExpired, its owner arm with
/// Kill_All_Spawns, and the owner's target scan. The rules-less UnInit
/// adapters (tests and fixtures only) skip the walk.
pub(crate) fn clear_all_spawn_targets(
    sim: &mut Simulation,
    owner_id: u64,
    rules: Option<&RuleSet>,
    registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    if let Some(rules) = rules {
        let mut index = 0;
        while let Some((state, child, target)) = manager_field(sim, owner_id, |m| {
            m.slots
                .get(index)
                .map(|slot| (slot.state, slot.spawn, m.current_target))
        })
        .flatten()
        {
            index += 1;
            let Some(child) = child.filter(|_| state == SpawnSlotState::InFlight) else {
                continue;
            };
            let missile_spawn = sim
                .substrate
                .entities
                .get(child)
                .and_then(|entity| sim.object_type(entity.type_ref(), rules))
                .is_some_and(|kind| kind.missile_spawn);
            if missile_spawn {
                sim.kamikaze_push(child, target, rules, registry);
                sim.kamikaze_restart_after_push();
                notify_pointer_expired(sim, owner_id, child, Some(rules), registry);
            }
        }
    }
    with_manager(sim, owner_id, SpawnManagerState::clear_all_targets);
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
///   remove it from the kamikaze tracker (`0x006B7162`), then call the same
///   destroy slot. The in-flight missile dies with its launcher; the salvo
///   does **not** land.
/// - **everything else** (`InFlight` / `ReturningToDock` / `LandingAtDock`,
///   i.e. the aircraft states) — the slot is marked regenerating, then
///   `SpawnRetreat__Push @ 0x0054E3B0` (`0x006B71B7`) with the manager's
///   target and no timer restart. A child whose type is not `MissileSpawn=`
///   (`+0xD68`, read at `0x00714F37`) crashes there (`Crash(0)`,
///   `0x0054E3CA..0x0054E3D2`): the Hornets of a sunk Carrier fall out of the
///   sky like shot-down aircraft (`Simulation::foot_crash`), and one already
///   on the ground is refused and stays. A missile joins the tracker
///   (`sim::kamikaze`) and flies on toward the last target.
///
/// The regen duration is `SpawnRegenRate` when the owner is dead
/// (`owner.Health < 1 || !owner.IsAlive`) and **zero** when it is still alive —
/// so an ownership change or a deploy rebuilds the pool on the next AI pass
/// rather than after a full regen wait.
///
/// The pushed child's `spawn_owner_id` is cleared here; native clears the
/// child's spawn owner (`+0x2D4`) when the owner's pointer expires. Push needs
/// the rules; the rules-less UnInit adapters (tests and fixtures only) release
/// the child without it.
///
/// This routine never touches `CurrentTarget`/`QueuedTarget`; the caller
/// decides. `Simulation::spawn_manager_owner_expired` pairs it with
/// `ClearAllTargets`, matching the owner arm of
/// `SpawnManagerClass::PointerExpired`, while
/// `Simulation::change_owner` calls only this one. The *target* arm of the same
/// native routine also clears targets and is a separate path entirely — see
/// [`notify_pointer_expired`].
///
/// Native callers of this routine, and which are wired here:
/// - `SpawnManagerClass::PointerExpired(owner)` — WIRED, via
///   `Simulation::spawn_manager_owner_expired` at the killing hit's Destroy
///   broadcast and again at UnInit, together with the `ClearAllTargets` it
///   pairs with. Conceal's Destroy(1) broadcast would reach it too; VERA's
///   conceal does not run it (RESIDUAL, see `object_conceal_with_context`).
/// - `TechnoClass::ChangeOwner` (`0x0070157E`) — WIRED, via
///   `Simulation::change_owner`; this is the mind-control path.
/// - `TemporalClass::InitiateWarp` (`0x0071AF39`) — WIRED, via
///   `sim::temporal`: a chrono-warped spawner loses its pool first.
/// - `TechnoClass::ImbueLocomotor` (`0x00710021`), the Magnetron lift reached
///   only from the IsLocomotor arm of `BulletClass::DetonateAtCoord`
///   (`0x004696FB`) — **not wired**; VERA does not port that arm. A Magnetron
///   lifting a V3 Launcher reaches it.
/// - `TechnoClass::Stun` (`0x006FCD40`, entered from `FootClass::Stun @
///   0x004D5660`) — WIRED for the death arm via `Simulation::techno_death_stun`.
///   Its gate, Foot+0x6AD, is set only by `TechnoClass::ImbueLocomotor @
///   0x00710000` (a Magnetron lift), which VERA does not port.
/// - The destructor — covered by the `uninit` hook.
pub(crate) fn kill_all_spawns_with_context(
    sim: &mut Simulation,
    owner_id: u64,
    context: UninitContext<'_>,
) {
    let Some((slots, regen_rate, target)) = sim.substrate.entities.get(owner_id).and_then(|e| {
        e.spawn_manager
            .as_ref()
            .map(|m| (m.slots.clone(), m.regen_rate, m.current_target))
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
    // Backwards, as native (`0x006B7123` from the last slot, `0x006B7218`):
    // each crashing Hornet takes its three Scenario draws in that order.
    for (index, slot) in slots.into_iter().enumerate().rev() {
        if slot.state == SpawnSlotState::Regenerating {
            continue;
        }
        if let Some(child_id) = slot.spawn {
            match slot.state {
                // Docked or reloading: the destroy slot.
                SpawnSlotState::ReadyDocked | SpawnSlotState::Reloading => {
                    sim.uninit_with_context(child_id, context);
                }
                // A missile still inside its post-launch tilt window leaves
                // the tracker first.
                SpawnSlotState::KamikazeWait => {
                    sim.kamikaze.remove(child_id);
                    sim.uninit_with_context(child_id, context);
                }
                // Out: `SpawnRetreat__Push`, which crashes anything but a
                // missile.
                SpawnSlotState::InFlight
                | SpawnSlotState::ReturningToDock
                | SpawnSlotState::LandingAtDock => {
                    if let Some(child) = sim.substrate.entities.get_mut(child_id) {
                        child.spawn_owner_id = None;
                    }
                    if let Some(rules) = context.rules() {
                        sim.kamikaze_push(child_id, target, rules, context.registry());
                    }
                }
                SpawnSlotState::Regenerating => unreachable!("skipped above"),
            }
        }
        with_slot(sim, owner_id, index, |slot| {
            slot.spawn = None;
            slot.state = SpawnSlotState::Regenerating;
            slot.timer = CdTimer::started(frame as i32, regen_duration as i32);
        });
    }
}
