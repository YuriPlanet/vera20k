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
//! - Some types here (Facing, VoxelModel, etc.) are legacy wrappers
//!   kept for any remaining call sites. The canonical data lives in GameEntity fields.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/ (EntityCategory type).
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// World position in isometric cell coordinates plus sub-cell lepton offset.
///
/// RA2 uses leptons as its spatial unit (256 leptons = 1 cell). We store the
/// cell coordinate (rx, ry) plus a sub-cell lepton offset (sub_x, sub_y) to
/// get sub-cell precision without overflowing SimFixed on large maps.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Position {
    /// Isometric cell X coordinate.
    pub rx: u16,
    /// Isometric cell Y coordinate.
    pub ry: u16,
    /// Elevation level (0 = ground). Each level is 15px visual offset.
    pub z: u8,
    /// Exact native ObjectClass coordinate Z, in signed leptons.
    ///
    /// Ground movement retains ramp height at the native height-write points;
    /// Drive/Ship residual XY interpolation can retain an earlier sampled Z.
    /// TubeMovement also retains its signed interpolation remainder after exit.
    /// Neither coordinate can be reconstructed from the coarse `z` level or
    /// indiscriminately replaced with the current surface on idle ticks.
    /// None is the legacy coarse/independent-altitude representation.
    #[serde(default)]
    pub exact_z_leptons: Option<i32>,
    /// Sub-cell lepton offset X (0..256). 128 = cell center.
    /// Provides sub-cell precision for smooth movement and accurate range checks.
    pub sub_x: SimFixed,
    /// Sub-cell lepton offset Y (0..256). 128 = cell center.
    pub sub_y: SimFixed,
}

/// Facing direction (0â€“255, RA2 convention).
///
/// 0 = north, 64 = east, 128 = south, 192 = west.
/// Used for sprite/voxel rotation and movement direction.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Facing(pub u8);

/// Independent turret facing direction (0–255, RA2 convention).
///
/// Only present on entities with `Turret=yes` in rules.ini (e.g., tanks, War Miner).
/// The turret rotates independently from the body: it tracks attack targets,
/// and returns to body facing when idle.
/// 0 = north, 64 = east, 128 = south, 192 = west.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct TurretFacing(pub u8);

/// Signed actual ObjectClass health (+0x6C in gamemd.exe).
///
/// Live ObjectType::strength owns the cap/ratio denominator. EstimatedHealth
/// is independent state; writes to this field do not implicitly update it.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Health {
    pub current: i32,
}

impl Health {
    /// ObjectClass::GetHealthRatio (gamemd.exe 0x005F5C60..0x005F5C7F).
    /// Signed loads and PC53/chop division; zero Strength keeps the native
    /// masked infinity/NaN result for the caller's actual comparison/conversion.
    pub fn ratio(self, strength: i32) -> crate::util::native_x87::MaskedX87Value {
        use crate::util::native_x87::MaskedX87Chop53 as X87;
        X87::div(X87::load_i32(self.current), X87::load_i32(strength))
    }

    /// Compare the native ratio without converting masked values to a host float.
    pub fn compare_ratio(
        self,
        strength: i32,
        threshold: f64,
    ) -> crate::util::native_x87::MaskedX87Ordering {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        X87::compare(
            self.ratio(strength),
            X87::load_f64(NativeF64Bits::from_bits(threshold.to_bits())),
        )
    }
}

/// Vision radius in grid cells used for fog/shroud reveal.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Vision {
    /// Reveal/visibility radius in cells.
    pub range_cells: u16,
}

/// Marker component: this entity is rendered as a VXL voxel model.
///
/// Vehicles and aircraft use voxel models. The render loop loads the
/// corresponding VXL+HVA files and renders them via the software rasterizer.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct VoxelModel;

/// Marker component: this entity is rendered as a SHP 2D sprite.
///
/// Infantry and buildings use SHP sprites. Not yet wired to rendering â€”
/// will be implemented when SHP sprite batching is added.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SpriteModel;

/// Which category this entity belongs to (unit, infantry, structure, aircraft).
///
/// Wraps the map::entities::EntityCategory enum so it can be used as an ECS component.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Category(pub EntityCategory);

/// Infantry sub-cell position (0–4).
///
/// RA2 uses sub-cell spots 2, 3, 4 — up to 3 infantry per cell, each at a
/// different sub-position. Only meaningful for infantry entities.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SubCell(pub u8);

/// Veterancy level: 0 = rookie, 100 = veteran, 200 = elite.
///
/// Affects unit stats (damage, armor, speed bonuses) and visual indicators.
///
/// This is the RANK PROJECTION of `GameEntity::veterancy_raw`, which is the
/// authoritative running accumulator. gamemd-derived:
/// `TechnoClass::Record_The_Kill @ 0x00702D40` awards the victim's cost —
/// zeroed between allies, doubled for a veteran victim, tripled for an elite
/// one — and `VeterancyClass::Add @ 0x0074FF50` divides it by the killer's own
/// cost times `[General] VeteranRatio=`, clamping at `VeteranCap=`. The rank
/// tests are `>= 1.0` for veteran and `>= 2.0` for elite. A Grizzly promotes on
/// its third rookie Rhino and goes elite on its fifth.
///
/// The rank effects live in `sim::combat::veterancy`: the full 18-token
/// ability arrays, `HasWeaponAbility`, the ROF/FIREPOWER/FASTER/SIGHT
/// multipliers, self-heal, the promotion announcement and the elite flash
/// timer, and the `Record_The_Kill` recipient chain.
///
/// RESIDUAL (GSI-08.12) — the elite flash is not DRAWN. `elite_flash_frames`
/// counts down as native's `+0xF0` does, but VERA draws no rank chevrons or
/// flash at all (`DrawVeterancyPips @ 0x0070A990` has no presentation
/// counterpart). Trigger: every promotion. Player effect: a promoted unit
/// shows no chevron and a new elite does not blink. Frequency: continuous.
/// Downstream risk: none — presentation only.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Veterancy(pub u16);

/// Marker component: this building is being repaired (spending credits to heal).
///
/// Added by the ToggleRepair command. Removed when health is full or credits run out.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Repairing;

/// A building's construction animation (BState 0), set by
/// `BuildingClass::Begin_Mode(0)` (`0x00447780`) from the type's control and
/// stepped once per frame by `BuildingClass::UpdateAnimation` (`0x004509D0`);
/// the stepping lives in `sim::building_construction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildupStage {
    /// The type's construction control (`Type+0xF04`): first frame, frame
    /// count, rate (`RuleSet::buildup_control`).
    pub control: [i32; 3],
    /// The stage (`+0xF8`): the Buildup frame drawn.
    pub stage: i32,
    /// The stage rate (`+0x10C`); a wrap re-derives it from the control.
    pub rate: i32,
    /// The stage timer (`+0x100..+0x108`), counting down one `rate`.
    pub timer: crate::sim::timer::CdTimer,
}

/// Where a building's Construction mission (`0x12`) stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConstructionMission {
    /// Queued behind no current mission (a human player's placement, a
    /// deploy): the ready byte commences it (`0x0043FE27` once BState is not
    /// 0, or `0x0043FF91`).
    Queued,
    /// Current with its mission timer due: the next dispatch is its first
    /// visit (`Mission_Construction` status 0).
    Due,
    /// Visited (status 1): each visit completes on the ready byte.
    Watching,
}

/// A building building up after its placement or deploy
/// (`sim::building_construction`): its construction animation, its BState,
/// its Construction mission, and `+0x6DD`, the byte set when the animation
/// lands on its last frame. The render draws `anim.stage` from the Buildup
/// SHP while BState is 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildingUp {
    /// The construction animation (BState 0); each `Begin_Mode(0)` restarts
    /// it from the control.
    pub anim: BuildupStage,
    /// BState (`+0x534`) is 1, the idle control (`{0, 1, 0}`, the
    /// BuildingType constructor's for every retail type), instead of 0.
    pub idle: bool,
    /// `Begin_Mode(1)` is queued (`+0x538`), applied at the end of the next
    /// Update (`0x0043FFB4`): a human player's placement, whose factory
    /// sends the building OVER_OUT (`0x004FB4A6` -> `0x0043CD01`).
    pub idle_queued: bool,
    pub mission: ConstructionMission,
    /// `+0x6DD`, the animation-complete (ready-to-commence) byte.
    pub done: bool,
    /// The first frame with an Update: the frame after a placement (placed
    /// after that frame's Logic pass), a deployed building's creation frame.
    pub first_frame: i32,
}

/// A building packing up into its `UndeploysInto=` unit through the Selling
/// mission's UndeploysInto arm (`BuildingClass::Sell @ 0x00449C30`,
/// `sim::building_construction`): stage 0 and 1 visits, then the
/// construction animation played again (drawn in reverse) until `+0x6DD`,
/// when the building converts (`Simulation::finish_undeploy`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildingDown {
    /// The construction animation from stage 1's `Begin_Mode(0)`; before
    /// that the building shows its idle frames.
    pub anim: BuildupStage,
    /// `BuildingClass::Sell`'s stage (`+0xBC`): 0, 1, or 2 (waiting).
    pub sell_stage: u8,
    /// The frame the Selling mission commenced; its visits start the frame
    /// after.
    pub commenced_frame: i32,
    /// `+0x6DD`, the animation-complete byte.
    pub done: bool,
    /// Started by the player's undeploy order, VERA's stand-in for the retail
    /// cell click (`0x004436F0`), which always sets an ArchiveTarget before
    /// the sale: such a pack-up never takes the archive-less stage-0x17 exit.
    pub player_order: bool,
    /// Unit type to spawn when animation completes (e.g., "AMCV").
    pub spawn_type: InternedId,
    /// Owner of the unit to spawn.
    pub spawn_owner: InternedId,
    /// Map position where the mobile unit will appear.
    pub spawn_rx: u16,
    pub spawn_ry: u16,
    /// Height level for the spawned unit.
    pub spawn_z: u8,
    /// Whether the entity was selected (transfer selection to spawned unit).
    pub was_selected: bool,
}

/// Marker component: this entity is currently selected by the player.
///
/// Added/removed dynamically via `world.insert_one()` / `world.remove_one()`.
/// The render loop queries for `Selected` to draw selection indicators.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Selected;

/// Movement path target â€” entity is moving along a computed A* path.
///
/// Attached by `issue_move_command()` when a unit is ordered to move.
/// The movement system (`tick_movement`) advances the entity along the path
/// each tick. Removed automatically when the entity reaches its destination.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MovementTarget {
    /// Sequence of (rx, ry) cells from current position to goal (inclusive).
    pub path: Vec<(u16, u16)>,
    /// Spatial layer for each path step. Matches `path.len()`.
    pub path_layers: Vec<MovementLayer>,
    /// Index of the next cell to move toward in the path.
    /// Starts at 1 (index 0 is the current position).
    pub next_index: usize,
    /// Maximum movement speed in leptons per second (from rules.ini Speed= value).
    /// 256 leptons = 1 cell. Fixed-point for deterministic multiplayer.
    pub speed: SimFixed,
    /// Actual speed this tick — ramps from 0 toward `speed` via acceleration,
    /// and brakes down near the destination. If no ramping data is set (accel=0),
    /// the movement system falls back to using `speed` directly.
    pub current_speed: SimFixed,
    /// Fraction of max speed gained per tick during acceleration (AccelerationFactor=).
    pub accel_factor: SimFixed,
    /// Fraction of max speed lost per tick during braking (DeaccelerationFactor=).
    pub decel_factor: SimFixed,
    /// Lepton distance from destination at which braking begins (SlowdownDistance=).
    pub slowdown_distance: SimFixed,
    /// Direction vector toward next cell in leptons: `dx_cells * 256`, `dy_cells * 256`.
    /// Recomputed when `next_index` advances. Cardinal = (±256, 0) or (0, ±256),
    /// diagonal = (±256, ±256).
    pub move_dir_x: SimFixed,
    /// Y component of the direction vector (see `move_dir_x`).
    pub move_dir_y: SimFixed,
    /// Lepton distance from current cell center to next cell center.
    /// 256 for cardinal moves, ~362 for diagonal. Used to normalize advancement.
    pub move_dir_len: SimFixed,
    /// Cell where this mover was last refused by a wall it could shoot.
    ///
    /// VERA-only scaffolding, and a recorded DRIFT. Native evaluates the cell
    /// twice inside ONE `DriveLocomotionClass::Process_Movement` call: the 4/5
    /// arm at `0x004B3ADB` drops `Path[0]`, stamps an already-expired timer and
    /// tail-recurses at `0x004B4552` with `arg2 = 0`, which repaths and
    /// re-evaluates immediately; the Override at `0x004B3BE9` fires only on that
    /// second refusal. VERA issues its repath through `handle_blocked_tick` and
    /// takes the second refusal on a later tick, so it needs to remember which
    /// cell was refused. Cleared on any non-wall refusal, and naturally reset
    /// when a new order replaces this target.
    #[serde(default)]
    pub wall_refusal_cell: Option<(u16, u16)>,
    /// Ultimate destination — preserved across 24-step segment replanning.
    /// When a path segment is exhausted before reaching this goal, the movement
    /// system auto-replans from the current position. `None` for short paths
    /// or test-only movement targets that don't need segmented replanning.
    pub final_goal: Option<(u16, u16)>,
    /// Group ID for formation speed sync. When set, the movement system caps
    /// this unit's speed to the slowest member of the group (deep_113 line 451).
    pub group_id: Option<u32>,
    /// When true, the movement tick skips terrain cost passability checks
    /// for cell entry. Used by `issue_direct_move` to let harvesters walk
    /// onto ore cells that are terrain-blocked for their SpeedType.
    pub ignore_terrain_cost: bool,
    /// When true, the movement tick skips PathGrid walkability checks for
    /// cell entry. Used by dock-sequence direct moves where the harvester
    /// must traverse the refinery foundation footprint (cells marked
    /// blocked by `block_building_footprint`). Does NOT bypass entity
    /// occupancy checks — other movers still collide.
    #[serde(default)]
    pub bypass_grid: bool,
    /// A Rust route adapter owns this route (`issue_direct_move`, component
    /// fixtures): its cells were never published to Foot+5E0 and it names no
    /// locomotor destination, so a Drive/Ship Unit keeps the pass lane while it
    /// is pending. Native routes (the setter's empty scheduling adapter and
    /// Find_Path's install) leave it false, so emptying their Foot+5E0 head
    /// never reroutes the Unit through the adapter lane.
    #[serde(default)]
    pub adapter_route: bool,
}

/// Native-like navigation target reference.
///
/// In gamemd this is an `AbstractClass*`. Phase 1 needs cell targets for normal
/// Drive move commands, while the entity variant gives action lines and later
/// destination paths a shared shape without claiming those paths are complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NavTargetRef {
    Cell { rx: u16, ry: u16 },
    Entity { id: u64 },
    Object { id: u64 },
    Building { id: u64 },
}

impl NavTargetRef {
    pub fn cell(rx: u16, ry: u16) -> Self {
        Self::Cell { rx, ry }
    }

    pub fn object(id: u64) -> Self {
        Self::Object { id }
    }

    pub fn building(id: u64) -> Self {
        Self::Building { id }
    }
}

/// FootClass-style owner navigation fields.
///
/// `nav_com` is the owner destination. It must stay separate from
/// `MovementTarget`, which is only the active execution path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NavigationState {
    /// Foot+55C is retained lifecycle history, not current occupancy.
    #[serde(default)]
    pub neighbor_state: crate::sim::cell_neighbors::FootNeighborState,
    /// Foot timers, retry count and blockage latch survive path retirement.
    #[serde(default)]
    pub path_runtime: FootPathRuntime,
    /// Foot-owned direction replay and reference cell (+5E0/+558), shared by
    /// every locomotor instance. Retirement must not discard the owner queue.
    /// Native chain tails reload the owner at Drive4B1DF7 / Ship6A143A.
    #[serde(default)]
    pub path_replay: FootPathQueue,
    #[serde(default)]
    pub nav_com_aux: Option<NavTargetRef>,
    #[serde(default)]
    pub nav_com: Option<NavTargetRef>,
    #[serde(default)]
    pub suspended_nav_com: Option<NavTargetRef>,
    #[serde(default)]
    pub nav_queue: Vec<NavTargetRef>,
    /// Drive path execution reached the destination cell, but the owner
    /// no-active-track arrival clear has not run yet.
    #[serde(default)]
    pub pending_arrival_clear: bool,
}

/// Persistent Foot path state. The FootClass constructor 0x004D31E0 anchors
/// both timers at the current frame (0x4D3320, 0x4D335B), stores +64C = 10
/// (0x4D332C) and +6B7 = 0 (0x4D3451). Set_Destination_Internal 0x004D94B0
/// rewrites the timers and latch at 0x4D96C2..0x4D9707 for every accepted
/// setter, including a null destination, so this owner outlives MovementTarget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootPathRuntime {
    /// Foot+640/+648, including the native frame anchor.
    pub movement_timer: crate::sim::timer::CdTimer,
    /// Foot+668/+670.
    pub blocked_timer: crate::sim::timer::CdTimer,
    /// Foot+6B7.
    pub path_blocked: bool,
    /// Foot+64C, a dword decremented only while nonzero.
    pub retries_left: u32,
}

impl Default for FootPathRuntime {
    fn default() -> Self {
        Self::at_frame(0)
    }
}

impl FootPathRuntime {
    pub const fn at_frame(frame: u32) -> Self {
        Self {
            movement_timer: crate::sim::timer::CdTimer::started(frame as i32, 0),
            blocked_timer: crate::sim::timer::CdTimer::started(frame as i32, 0),
            path_blocked: false,
            retries_left: 10,
        }
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
    const CELL_LEPTONS: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
    const CELL_CENTER: i32 = crate::util::lepton::CELL_CENTER_LEPTON_I32;

    pub fn cell(rx: u16, ry: u16, z: i32) -> Self {
        Self {
            x: i32::from(rx) * Self::CELL_LEPTONS + Self::CELL_CENTER,
            y: i32::from(ry) * Self::CELL_LEPTONS + Self::CELL_CENTER,
            z,
        }
    }
}

/// Foot-owned path replay. Native shifts a 24-dword queue on consumption;
/// Rust retains the consumed prefix with an explicit cursor. Consumers must
/// interpret the remaining suffix, not vector emptiness, as the native queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootPathQueue {
    /// Foot+5E0: octants0..7, explicit tube direction8, native -1 terminator.
    /// Find_Path4D3E98 writes it; Drive/Ship movement consumes the same owner
    /// memory, independently of which locomotor is currently installed.
    #[serde(default)]
    pub directions: Vec<u8>,
    #[serde(default)]
    pub cursor: u16,
    /// Foot+558. Fresh acceptance writes this reference (4B4618/6A3C47);
    /// chain consumption preserves it (4B1DF7/6A143A).
    #[serde(default)]
    pub reference_cell: Option<(i16, i16)>,
}

impl FootPathQueue {
    /// Native queue emptiness tests the unconsumed head, not retained history.
    pub(crate) fn remaining_directions(&self) -> &[u8] {
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

/// Foot-owned applied speed, shared by every installed locomotor instance.
///
/// SetSpeedFraction4D3710 writes Foot+578/+57C; GetCurrentSpeed4DB1A0 reads
/// that fraction. Drive4AF540/Ship69EC50 constructors and DriveEND4AF930 do
/// not own or reset it. Keep this outside both class payloads so a synchronous
/// callback can replace a locomotor without replacing the owner's speed.
/// Original executable witnesses: tools/spatial_oracle/foot_speed_owner.json.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootSpeedState {
    pub applied_fraction: SimFixed,
    /// Existing Rust adapter cache of GetCurrentSpeed, not a native field.
    /// Its producers use the movement request's adjusted type speed. The full
    /// Process host must query live owner/type modifiers at native call sites.
    pub cached_current_speed: i32,
    /// Foot+580, initialized to exactly 1.0 at4D3292/4D329B. Retain the
    /// native bits: pickup refuses even the immediate neighbors of 1.0.
    /// Every speed query reads it (GetCurrentSpeed multiplies it in).
    /// RESIDUAL: its only native writer is the Speed crate
    /// (`accept_speed_crate`), and crate pickup (Cell481A00 selection, removal,
    /// effect dispatch) has no production caller yet, so it stays exactly 1.0.
    /// Trigger: a Foot entering a crate cell. Effect: no crate is consumed and
    /// no crate effect applies. Frequency: crate maps/options. Risk: saved and
    /// hashed (v181) state that cannot differ from the constructor value until
    /// the pickup receiver lands.
    crate_multiplier: crate::util::native_x87::NativeF64Bits,
}

impl Default for FootSpeedState {
    fn default() -> Self {
        Self {
            applied_fraction: crate::util::fixed_math::SIM_ZERO,
            cached_current_speed: 0,
            crate_multiplier: crate::util::native_x87::NativeF64Bits::ONE,
        }
    }
}

impl FootSpeedState {
    pub(crate) fn crate_multiplier(&self) -> crate::util::native_x87::NativeF64Bits {
        self.crate_multiplier
    }

    /// `FootClass::SetSpeedFraction @ 0x004D3710` for a locomotor that computes
    /// its fraction as a native double (the Jumpjet's `Process`): at least 1.0,
    /// +infinity included, stores 1.0 (`0x004D3714`); at most 0, or NaN, stores
    /// 0 (`0x004D373C`); anything between is stored as given.
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
    pub(crate) fn set_speed_fraction_native_bits(&mut self, bits: u64) {
        const ONE_BITS: u64 = 0x3ff0_0000_0000_0000;
        const INFINITY_BITS: u64 = 0x7ff0_0000_0000_0000;
        let negative = bits >> 63 != 0;
        let exponent = ((bits >> 52) & 0x7ff) as i32;
        let mantissa = bits & ((1 << 52) - 1);
        self.applied_fraction = if negative || bits << 1 == 0 || bits > INFINITY_BITS {
            // A negative value, a zero of either sign, or NaN.
            crate::util::fixed_math::SIM_ZERO
        } else if bits >= ONE_BITS {
            crate::util::fixed_math::SIM_ONE
        } else {
            // 0 < value < 1: floor(value * 2^16) from the significand.
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
    pub(crate) fn accept_speed_crate(
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

/// ShipLocomotion-owned destination, committed head, and target speed state.
///
/// Ships share the ordinary TurnTrack/RawTrack curves and target fraction
/// with Drive, but do not own Drive's
/// tube, forced-track, or raw-occupation state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShipLocomotionRuntime {
    #[serde(default)]
    pub destination: Option<DriveCoord>,
    #[serde(default)]
    pub head_to: Option<DriveCoord>,
    #[serde(default)]
    pub track: TrackProgress,
    /// Ship+63, independent of head XYZ; admission6A05FC reads this byte.
    #[serde(default)]
    pub track_valid: bool,
    /// Native class+62: last eligible Process observation of body rotation.
    /// Do_Turn and track-point facing updates do not write this latch.
    #[serde(default)]
    pub turn_latched: bool,
    #[serde(default)]
    pub target_speed_fraction: SimFixed,
    #[serde(default)]
    pub occupation_head_to: Option<DriveOccupationFootprint>,
    #[serde(default)]
    pub occupation_handoff: Option<DriveOccupationFootprint>,
}

/// Drive-owned 16-bit facing target and first-movement gate.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct DriveTurnState {
    #[serde(default)]
    pub target_direction: Option<u8>,
    #[serde(default)]
    pub target_facing_16: Option<u16>,
    #[serde(default)]
    pub rate_timer: u16,
    #[serde(default)]
    pub first_movement_allowed: bool,
}

/// One active Drive/Ship locomotor's retained track selector, signed cursor,
/// short-track choice and residual (+58/+5C/+60/+4C). Curve geometry and a
/// temporary Process_Track call must not own serialized copies of this state.
/// Native evidence: tools/spatial_oracle/locomotor_track_cursor.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TrackProgress {
    pub turn_index: i32,
    /// Next-to-consume cursor. Drive constructor4AF5A6/4AF5A9 and Ship
    /// constructor69ECB6/69ECB9 initialize selector/cursor to -1;
    /// fresh/forced acceptance and completion retirement instead store zero.
    pub cursor: i32,
    pub reversed: bool,
    pub residual: i32,
}

impl Default for TrackProgress {
    fn default() -> Self {
        Self {
            turn_index: -1,
            cursor: -1,
            reversed: false,
            residual: 0,
        }
    }
}

/// Drive-owned occupation mark installed ahead of the live object-list cell.
///
/// The cell list remains tied to the unit's committed coordinates. This record
/// persists the independent head-to mark so the transient per-cell occupation
/// index can be rebuilt after loading a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DriveOccupationFootprint {
    pub rx: u16,
    pub ry: u16,
    pub layer: MovementLayer,
}

/// DriveLocomotion-owned destination/head-to state.
///
/// Native can clear destination,
/// head-to, and active track state at different points in the lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DriveLocomotionRuntime {
    #[serde(default)]
    pub destination: Option<DriveCoord>,
    #[serde(default)]
    pub head_to: Option<DriveCoord>,
    #[serde(default)]
    pub turn: DriveTurnState,
    #[serde(default)]
    pub track: TrackProgress,
    /// Drive+65, seeded true at constructor4AF5BB. Native4B4BE0/4B4BF0
    /// disable/enable END while Foot Find_Path removes a Team membership.
    /// No production Rust writer models that synchronous pair yet.
    #[serde(default = "drive_end_permitted_default")]
    pub end_permitted: bool,
    #[serde(default)]
    pub track_valid: bool,
    /// Native class+62: last eligible Process observation of body rotation.
    /// Do_Turn and track-point facing updates do not write this latch.
    #[serde(default)]
    pub turn_latched: bool,
    #[serde(default)]
    pub target_speed_fraction: SimFixed,
    /// Head-to vehicle-occupation mark, independent from CellClass object-list
    /// membership. Ordinary flat Drive installs one mark for its accepted next
    /// cell before any paid track point is consumed.
    #[serde(default)]
    pub occupation_head_to: Option<DriveOccupationFootprint>,
    /// Forward RawTrack handoff mark. A turning curve comes to rest on its head
    /// cell but *passes through* an intermediate one, and the original claims
    /// both: `Apply_Track_Occupation_Mode` applies the same mode to the track's
    /// `+0x0C` handoff point — transformed around the stored head — before it
    /// applies it to the supplied head coordinate. Without this the cell a
    /// turning mover is about to drive through looks free to every other mover.
    #[serde(default)]
    pub occupation_handoff: Option<DriveOccupationFootprint>,
}

fn drive_end_permitted_default() -> bool {
    true
}

impl Default for DriveLocomotionRuntime {
    fn default() -> Self {
        Self {
            destination: None,
            head_to: None,
            turn: DriveTurnState::default(),
            track: TrackProgress::default(),
            end_permitted: true,
            track_valid: false,
            turn_latched: false,
            target_speed_fraction: SIM_ZERO,
            occupation_head_to: None,
            occupation_handoff: None,
        }
    }
}

/// Default acceleration/deceleration values — zero means no ramping,
/// movement system falls back to using `speed` directly.
impl Default for MovementTarget {
    fn default() -> Self {
        Self {
            path: Vec::new(),
            path_layers: Vec::new(),
            next_index: 0,
            speed: SIM_ZERO,
            current_speed: SIM_ZERO,
            accel_factor: SIM_ZERO,
            decel_factor: SIM_ZERO,
            slowdown_distance: SIM_ZERO,
            move_dir_x: SIM_ZERO,
            move_dir_y: SIM_ZERO,
            move_dir_len: SIM_ZERO,
            final_goal: None,
            group_id: None,
            ignore_terrain_cost: false,
            bypass_grid: false,
            wall_refusal_cell: None,
            adapter_route: false,
        }
    }
}

impl MovementTarget {
    pub fn layer_at(&self, index: usize) -> MovementLayer {
        debug_assert_eq!(
            self.path.len(),
            self.path_layers.len(),
            "path/path_layers length mismatch: {} vs {}",
            self.path.len(),
            self.path_layers.len()
        );
        self.path_layers
            .get(index)
            .copied()
            .unwrap_or(MovementLayer::Ground)
    }
}

/// Marker component: this entity currently occupies a bridge deck cell.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct BridgeOccupancy {
    pub deck_level: u8,
}

/// Persistent high-level order state that survives transient combat/movement components.
///
/// This keeps intent like attack-move or guard alive while systems temporarily
/// add/remove `MovementTarget` and `AttackTarget`.
///
/// Slice 6: the "is this unit busy?" signalling role moved to the `mission`
/// substrate (`mission::verb::get_current_mission`/`is_busy`). What remains here
/// is the data `MissionType` cannot encode — the AttackMove goal / Guard anchor
/// coords and the transport `Unloading` flag. Retiring this enum entirely waits
/// on a goal field landing on the mission/nav substrate (a later slice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OrderIntent {
    /// Move toward a destination but auto-acquire enemies along the way.
    AttackMove { goal_rx: u16, goal_ry: u16 },
    /// Hold position and auto-acquire nearby enemies.
    Guard { anchor_rx: u16, anchor_ry: u16 },
    /// Transport is actively unloading passengers one per tick.
    Unloading,
}

/// Which part of a multi-part voxel model an entity/atlas entry represents.
///
/// Non-turret units use `Composite` (body+turret+barrel baked together).
/// Turret units store Body/Turret/Barrel separately so the turret can
/// be drawn at a different facing than the body at render time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum VxlLayer {
    /// All parts composited into one sprite (for units without independent turret).
    Composite,
    /// Body only (hull/chassis).
    Body,
    /// Turret only ({IMAGE}TUR.VXL).
    Turret,
    /// Barrel only ({IMAGE}BARL.VXL).
    Barrel,
    /// Ground shadow of the main voxel: each section's occupied (x, y) columns
    /// flattened onto z = 0 and shifted by the shadow light vector, always at
    /// motion frame 0. See `render::vxl_raster::render_vxl_shadow`.
    Shadow,
}

/// Per-entity voxel HVA animation state.
///
/// Attached to voxel entities that cycle through HVA animation frames at runtime.
/// Used for harvesting miners (arm/turret animation), and potentially other voxel
/// units with multi-frame HVA files. The render loop reads `frame` to select
/// the correct pre-rendered atlas sprite.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct VoxelAnimation {
    /// Current HVA frame index (0-based).
    pub frame: u32,
    /// Total number of HVA animation frames.
    pub frame_count: u32,
    /// Reached native frames accumulated since last image advance.
    pub elapsed_frames: u16,
    /// Reached native frames per image. 0 = no auto-advance.
    pub frame_delay: u16,
    /// Whether the animation is currently playing (cycling frames).
    pub playing: bool,
}

impl VoxelAnimation {
    /// Create a new VoxelAnimation in stopped state.
    pub fn new(frame_count: u32, frame_delay: u16) -> Self {
        Self {
            frame: 0,
            frame_count,
            elapsed_frames: 0,
            frame_delay,
            playing: false,
        }
    }
}

/// Harvest overlay animation state for the oregath.shp ore-gathering sprite.
///
/// Attached to harvester entities (HARV, CMIN). Shows the visual "sucking up ore"
/// animation as an SHP overlay on top of the VXL body when actively harvesting.
/// Uses the effect palette (anim.pal), independent of house colors.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct HarvestOverlay {
    /// Current animation frame (0..14, 15 frames per facing direction).
    pub frame: u16,
    /// Whether the overlay is currently visible and animating.
    pub visible: bool,
    /// Reached native frames accumulated since last image advance.
    pub elapsed_frames: u16,
}

/// Tracks the last entity that dealt damage to this entity.
///
/// Used for retaliation: when an idle unit takes damage, it automatically
/// attacks the source. Still subject to Verses gates (0%/1% block retaliation).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct LastAttacker {
    /// Stable entity ID of the attacker that dealt the most recent damage.
    pub attacker: u64,
}

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
        }
    }
}

/// Emitted by the refinery dock state machine for EVERY due dump gate of
/// `UnitClass::Mission_Unload @ 0x0073D630` state 3 (`HarvesterDumpRate × 900
/// <= unit+0xF8`, `0x0073E355..0x0073E374`), including the final gate that finds
/// no cargo. The authoritative master-frame tail consumes it to spawn the
/// refinery smoke burst (`BuildingClass` vtable+0x468 → `0x00459900`, fired
/// first on each gate at `0x0073E37E`), start SpecialAnim slot 10 only while
/// none is live (`building+0x584 == NULL`, `0x0073E384`), and cut the running
/// SpecialAnim on the empty gate (`ClearAnimSlot(0xA)`, `0x0073E530`) before
/// the returned state hash. The queue itself is transient and serde-skipped.
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

/// Emitted by the tank-bunker lifecycle when the walls rise (install) or fall
/// (teardown). The authoritative frame tail creates the bunker's SpecialAnim
/// overlays — document order within `kind == Special` decides the pair: 0/1 =
/// walls-up, 2/3 = walls-down. `damaged` selects the `…Damaged` art variant when
/// the building was at/below ConditionRed health at emit time. The queue is
/// transient; its resulting overlay component participates in the frame hash.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct BunkerWallAnimEvent {
    /// Bunker building stable_id whose walls are animating.
    pub building_id: u64,
    /// `true` = walls rising (install), `false` = walls falling (teardown).
    pub up: bool,
    /// Building was at/below ConditionRed when the event fired — use the
    /// `…Damaged` SpecialAnim variant.
    pub damaged: bool,
}

/// Per-attacker walk-up intent for the C4 plant mission.
///
/// Mirrors gamemd's SEAL/Tanya/PTROOP behavior: while this is `Some`, the
/// unit pathfinds toward the target building. On arrival at the target's
/// cell, `tick_c4_plants` claims the plant by setting
/// `PendingC4Detonation` on the building. This state is cleared when the
/// player retasks the unit (Move/Stop) or when the target is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct C4PlantState {
    pub target_building_id: u64,
}

/// Body rocking state for voxel-bodied units.
///
/// Tracks spring-damped roll/pitch angles driven by weapon impacts and EMP
/// wobble. Drive/Ship slope interpolation is locomotor-owned state and is
/// intentionally independent from this optional component.
///
/// Optional component on `GameEntity` — present on vehicles, ships, and
/// voxel-bodied buildings; `None` for infantry and SHP-bodied buildings, and
/// for aircraft until `FootClass::Crash` sets their spin rates.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct RockingState {
    /// Roll angle, rad. Positive sign matches AngleRotatedSideways convention.
    pub angle_sideways: SimFixed,
    /// Pitch angle, rad. Positive sign matches AngleRotatedForwards convention.
    pub angle_forwards: SimFixed,
    /// Roll angular velocity, rad/tick.
    pub vel_sideways: SimFixed,
    /// Pitch angular velocity, rad/tick.
    pub vel_forwards: SimFixed,
    /// If true, integrate without damping (EMP wobble, naval continuous rocking).
    pub is_ship_rocking: bool,
}

impl RockingState {
    /// Tilt-renderer deadband — both angles below this snap to zero and the unit
    /// renders via the static atlas path.
    pub const DEADBAND: SimFixed = SimFixed::lit("0.00002");

    /// Returns true when the body-rocking transform is neutral.
    #[cfg(test)]
    pub fn is_neutral(&self) -> bool {
        !self.is_ship_rocking
            && self.angle_sideways.abs() <= Self::DEADBAND
            && self.angle_forwards.abs() <= Self::DEADBAND
    }
}

/// Shared per-building C4 / PostMortem detonation timer.
///
/// Native BuildingClass uses one latch and timer triple for both an infantry
/// C4 plant and a qualifying `CausesDelayKill` fatal hit. Once elapsed, the
/// building's own Update fires a forced C4Warhead receiver packet with damage
/// equal to its current HP.
///
/// IronCurtain/ForceShield entry cancels this state. Normal targets keep an
/// expired latch if the forced receiver unexpectedly leaves them alive;
/// BridgeRepairHut owns the separate consume-and-clear branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PendingC4Detonation {
    /// Native signed Building timer start frame (`+0x528`). `-1` means the
    /// duration is already a remaining-duration value.
    pub start_frame: i32,
    /// Native signed duration (`+0x530`), preserved without clamping.
    pub duration_frames: i32,
    /// Retained source-object identity (`+0x540`). Fresh PostMortem arms leave
    /// this null; shortening an infantry C4 timer preserves its source.
    pub source_entity_id: Option<u64>,
}

impl PendingC4Detonation {
    /// Native signed remaining-time calculation shared by the shorten test,
    /// Building Update expiry, and deterministic checksum.
    #[inline]
    pub fn remaining_at(self, current_frame: i32) -> i32 {
        if self.start_frame == -1 {
            return self.duration_frames;
        }
        let elapsed = current_frame.wrapping_sub(self.start_frame);
        if elapsed < self.duration_frames {
            self.duration_frames.wrapping_sub(elapsed)
        } else {
            0
        }
    }

    #[inline]
    pub fn is_expired_at(self, current_frame: i32) -> bool {
        self.remaining_at(current_frame) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;

    #[test]
    fn test_position_creation() {
        let pos: Position = Position {
            rx: 30,
            ry: 40,
            z: 0,
            exact_z_leptons: None,
            sub_x: crate::util::lepton::CELL_CENTER_LEPTON,
            sub_y: crate::util::lepton::CELL_CENTER_LEPTON,
        };
        assert_eq!(pos.rx, 30);
        assert_eq!(pos.ry, 40);
    }

    #[test]
    fn test_types_are_send_sync() {
        // GameEntity fields must be Send + Sync for future multithreaded sim ticks.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Position>();
        assert_send_sync::<Facing>();
        assert_send_sync::<TurretFacing>();
        assert_send_sync::<Health>();
        assert_send_sync::<Vision>();
        assert_send_sync::<VoxelModel>();
        assert_send_sync::<SpriteModel>();
        assert_send_sync::<Category>();
        assert_send_sync::<SubCell>();
        assert_send_sync::<Veterancy>();
        assert_send_sync::<MovementTarget>();
        assert_send_sync::<BridgeOccupancy>();
        assert_send_sync::<OrderIntent>();
        assert_send_sync::<BuildingUp>();
        assert_send_sync::<Selected>();
        assert_send_sync::<LastAttacker>();
        assert_send_sync::<VoxelAnimation>();
        assert_send_sync::<HarvestOverlay>();
        assert_send_sync::<crate::sim::movement::locomotor::LocomotorState>();
        assert_send_sync::<NavigationState>();
        assert_send_sync::<DriveLocomotionRuntime>();
    }

    #[test]
    fn drive_coord_cell_uses_center_leptons() {
        let coord = DriveCoord::cell(45, 40, 0);
        assert_eq!(coord.x, 45 * 256 + 128);
        assert_eq!(coord.y, 40 * 256 + 128);
        assert_eq!(coord.z, 0);
    }

    #[test]
    fn nav_target_ref_has_cell_object_and_building_shapes() {
        assert_eq!(
            NavTargetRef::cell(45, 40),
            NavTargetRef::Cell { rx: 45, ry: 40 }
        );
        assert_eq!(NavTargetRef::object(7), NavTargetRef::Object { id: 7 });
        assert_eq!(NavTargetRef::building(9), NavTargetRef::Building { id: 9 });
    }

    #[test]
    fn drive_locomotion_default_is_inert() {
        let drive = DriveLocomotionRuntime::default();
        assert_eq!(drive.destination, None);
        assert_eq!(drive.head_to, None);
        let navigation = NavigationState::default();
        assert!(navigation.path_replay.directions.is_empty());
        assert_eq!(navigation.path_replay.cursor, 0);
        assert_eq!(drive.turn.target_direction, None);
        assert_eq!(drive.track.turn_index, -1);
        assert_eq!(drive.track.cursor, -1);
        assert!(!drive.track_valid);
        assert!(!drive.track.reversed);
        assert_eq!(drive.target_speed_fraction, SIM_ZERO);
        let owner_speed = FootSpeedState::default();
        assert_eq!(owner_speed.applied_fraction, SIM_ZERO);
        assert_eq!(owner_speed.cached_current_speed, 0);
        assert_eq!(drive.track.residual, 0);
    }

    #[test]
    fn drive_locomotion_serde_defaults_missing_fields() {
        let drive: DriveLocomotionRuntime = serde_json::from_str("{}").expect("deserialize drive");
        assert_eq!(drive, DriveLocomotionRuntime::default());
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
        let mut drive_b = DriveLocomotionRuntime::default();
        drive_b.destination = Some(DriveCoord::cell(45, 40, 0));
        drive_b.turn.target_facing_16 = Some(0x4000);
        drive_b.track.residual = 6;

        assert_ne!(hash_drive(&drive_a), hash_drive(&drive_b));
    }

    #[test]
    fn c4_state_types_are_send_sync_copy() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<C4PlantState>();
        assert_send_sync::<PendingC4Detonation>();
        // Compile-time Copy assertion via fn-bound:
        fn _assert_copy<T: Copy>() {}
        _assert_copy::<C4PlantState>();
        _assert_copy::<PendingC4Detonation>();
    }

    #[test]
    fn test_category_wraps_entity_category() {
        let cat: Category = Category(EntityCategory::Unit);
        assert_eq!(cat.0, EntityCategory::Unit);
    }

    #[test]
    fn rocking_default_is_neutral() {
        let r = RockingState::default();
        assert!(r.is_neutral());
    }

    #[test]
    fn rocking_active_angle_is_not_neutral() {
        let mut r = RockingState::default();
        r.angle_sideways = SimFixed::lit("0.01");
        assert!(!r.is_neutral());
    }

    #[test]
    fn rocking_within_deadband_is_neutral() {
        let mut r = RockingState::default();
        // SimFixed precision is ~1.5e-5, so 1e-5 rounds to 0; pick a value
        // strictly between the smallest representable nonzero and DEADBAND.
        // DEADBAND is 2e-5; SIM_EPSILON is ~1.5e-5 — exactly one delta below.
        r.angle_sideways = SimFixed::DELTA;
        assert!(r.is_neutral());
    }

    #[test]
    fn rocking_ship_rocking_is_not_neutral() {
        let mut r = RockingState::default();
        r.is_ship_rocking = true;
        assert!(!r.is_neutral());
    }
}
