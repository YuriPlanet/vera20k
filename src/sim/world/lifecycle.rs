//! Ordered ObjectClass-style lifecycle authority.
//!
//! Owns the independent Reveal, Conceal/Limbo, UnInit, LogicVector membership,
//! and pending-delete transitions.  Upper-layer work is emitted as ordered data;
//! this module never depends on render, UI, sidebar, audio, or net.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{CellRect, resolve_reservation_real_cell, scan_cell_rect};
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::lifecycle_request::LifecycleRequest;
use crate::sim::occupancy::{
    BUILDING_OCCUPATION_BIT, CellListInsertion, cell_list_layer_for_entity, entity_occupancy_cells,
};
use crate::sim::projectile::ProjectileTarget;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::lepton::LEPTONS_PER_LEVEL;

use super::display_layers::DisplayLayer;
use super::expiry_listeners::{ExpiryListeners, ListenerStores};
use super::substrate::{DeferredDeleteTicket, ObjectKind};
use super::{FrameEffects, Simulation};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::houses::Houses;
use crate::sim::intern::StringInterner;
use crate::sim::projectile::ProjectileStore;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::terrain_object::TerrainObjectStore;
use crate::sim::wave::WaveStore;
use crate::sim::world::ObjectSubstrate;

/// A building's `HasSpotlight=` fact and its owned `BuildingLightClass`.
/// Construction copies the fact, and a later Unlimbo recreates the light from
/// the copy ([`Self::stamp`]). `BuildingClass::Unlimbo @
/// 0x00441187` constructs the light after placement succeeds and
/// `BuildingClass::Limbo` destroys it before the remaining building-count and
/// base-node teardown; the light exists only for a placed `HasSpotlight=yes`
/// building.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildingLight {
    spotlight_capable: bool,
    light: Option<u64>,
}
impl BuildingLight {
    pub(crate) fn stamp(&mut self, has_spotlight: bool) {
        self.spotlight_capable = has_spotlight;
    }
    pub(crate) const fn spotlight_capable(self) -> bool {
        self.spotlight_capable
    }
    pub(crate) const fn light(self) -> Option<u64> {
        self.light
    }
    pub(super) fn attach(&mut self, id: u64) {
        self.light = Some(id);
    }
    pub(super) fn take(&mut self) -> Option<u64> {
        self.light.take()
    }
}

/// Two type facts the removal paths read, copied onto the object:
/// - `ObjectClass::Limbo` (`0x005F4D30`) dirties the tactical rect of the
///   object's extent when the type's `+0xAC` pointer is set
///   ([`LifecycleOutput::DirtyTacticalRect`]). VERA keeps the fact false
///   unless a caller has positive evidence and never infers it from category
///   or render representation; no production path sets it.
/// - `Occupier=`, which `FootClass::PointerExpired` (`0x004D9960`) tests to
///   keep a capturing infantryman's NavCom on an expiring target.
///   Construction stamps it ([`Self::stamp_occupier`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RemovalFacts {
    #[serde(default)]
    dirty_rect_eligible: bool,
    #[serde(default)]
    occupier: bool,
}

impl RemovalFacts {
    /// Limbo dirties the object's rect.
    pub(crate) const fn dirty_rect_eligible(self) -> bool {
        self.dirty_rect_eligible
    }

    /// `Occupier=`.
    pub(crate) const fn occupier(self) -> bool {
        self.occupier
    }

    /// Construction's copy of `Occupier=`.
    pub(crate) fn stamp_occupier(&mut self, occupier: bool) {
        self.occupier = occupier;
    }

    /// Install a fixture's dirty-rect fact.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_dirty_rect_eligible_for_test(&mut self, value: bool) {
        self.dirty_rect_eligible = value;
    }

    /// Install a fixture's `Occupier=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_occupier_for_test(&mut self, value: bool) {
        self.occupier = value;
    }
}

/// Client-relative Techno discovery history, native +41A/+41B/+41C.
/// Constructor6F2FB5..6F2FC1 clears all three; InitManagers6F3F40 and
/// ChangeOwner70173B classify only the current owner. Discovery6F4960 and
/// Conceal6F4A40 own the two retained observations. Infantry raw Save/Load
/// preserves the bytes: never reconstruct them from the restored owner.
/// Native peer CRC64DAB0 omits them; this is also omitted from the shared
/// Rust peer hash. The separate diagnostic CRC70C270 is not that checksum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TechnoDiscoveryHistory {
    /// `+0x41A`: the current house owns the object.
    owned_by_current_house: bool,
    /// `+0x41B`: the current house has discovered it.
    discovered_by_current_house: bool,
    /// `+0x41C`: a single historical byte for all other receiver houses, not
    /// a house set.
    discovered_by_other_house: bool,
}

impl TechnoDiscoveryHistory {
    /// `+0x41A`.
    pub const fn owned_by_current_house(self) -> bool {
        self.owned_by_current_house
    }

    /// `+0x41B`.
    pub const fn discovered_by_current_house(self) -> bool {
        self.discovered_by_current_house
    }

    /// `+0x41C`. Discovery and Conceal read the field here; tests read it
    /// through this.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn discovered_by_other_house(self) -> bool {
        self.discovered_by_other_house
    }

    /// InitManagers (`0x006F3F40`) and ChangeOwner (`0x0070173B..0x00701751`)
    /// classify only the owner byte.
    pub(crate) fn classify_owner(&mut self, owned_by_current_house: bool) {
        self.owned_by_current_house = owned_by_current_house;
    }

    /// Install a fixture's owner byte.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_owned_by_current_house_for_test(&mut self, owned: bool) {
        self.owned_by_current_house = owned;
    }

    /// Install a fixture's current-house discovered byte.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_discovered_by_current_house_for_test(&mut self, discovered: bool) {
        self.discovered_by_current_house = discovered;
    }

    /// Install a fixture's other-house discovered byte.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_discovered_by_other_house_for_test(&mut self, discovered: bool) {
        self.discovered_by_other_house = discovered;
    }

    /// A fixture's three bytes.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(
        owned_by_current_house: bool,
        discovered_by_current_house: bool,
        discovered_by_other_house: bool,
    ) -> Self {
        Self {
            owned_by_current_house,
            discovered_by_current_house,
            discovered_by_other_house,
        }
    }
}

/// Independent ObjectClass lifecycle facts.
///
/// These bytes deliberately do not derive from health, store presence, cell
/// occupancy, LogicVector membership, or the Rust death-sequence state. Active
/// gamemd keeps those concerns independent, including alive objects that are in
/// limbo, active objects that are temporarily off-cell, and dead-limbo objects
/// that remain resolvable until the pending-delete drain.
///
/// IsAlive and InLimbo are private to this module, whose Reveal, Limbo and
/// UnInit write them. The ObjectClass constructor sets both
/// (`0x005F392A`, `0x005F3930`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ObjectLifecycle {
    /// `ObjectClass+0x90`, IsAlive. UnInit clears it (`0x005F6625`).
    object_alive: bool,
    /// `ObjectClass+0x81`, InLimbo: Limbo sets it (`0x005F4E9E`) and Unlimbo
    /// clears it (`0x005F4F4E`). Independent of LogicVector membership.
    in_limbo: bool,
    /// `ObjectClass+0x74`, Marked: whether Mark/cell-list insertion owns cell
    /// membership. `ObjectClass::Mark` stores it (`0x005F58F7`, `0x005F5913`)
    /// through `movement::foot_mark`, whose
    /// `ObjectSubstrate::replace_foot_marked_byte` also saves, clears and restores
    /// it around the locomotors' setters.
    cell_marked: bool,
}

impl Default for ObjectLifecycle {
    fn default() -> Self {
        Self {
            object_alive: true,
            in_limbo: true,
            cell_marked: false,
        }
    }
}

impl ObjectLifecycle {
    /// Whether Mark/cell-list insertion currently owns cell membership.
    pub const fn cell_marked(self) -> bool {
        self.cell_marked
    }

    /// `ObjectClass::Mark` (`0x005F5850`) DOWN's admission: a Limbo (`+0x81`)
    /// or already marked (`+0x74`) object is refused; otherwise `+0x74` is
    /// stored (`0x005F58F7`).
    pub(crate) fn admit_mark_down(&mut self) -> bool {
        if self.in_limbo() || self.cell_marked {
            return false;
        }
        self.cell_marked = true;
        true
    }

    /// Mark(UP)'s admission: a Limbo or unmarked object is refused; otherwise
    /// `+0x74` is cleared (`0x005F5913`).
    pub(crate) fn admit_mark_up(&mut self) -> bool {
        if self.in_limbo() || !self.cell_marked {
            return false;
        }
        self.cell_marked = false;
        true
    }

    /// Replace `+0x74` without Mark's callbacks, answering the old value.
    pub(crate) fn replace_cell_marked(&mut self, marked: bool) -> bool {
        std::mem::replace(&mut self.cell_marked, marked)
    }

    /// A fixture's marked byte.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_cell_marked_for_test(&mut self, marked: bool) {
        self.cell_marked = marked;
    }

    /// `ObjectClass+0x90`, IsAlive.
    pub const fn object_alive(self) -> bool {
        self.object_alive
    }

    /// `ObjectClass+0x81`, InLimbo.
    pub const fn in_limbo(self) -> bool {
        self.in_limbo
    }

    /// The constructor's facts for a newly built object: alive and in limbo
    /// (`0x005F392A`, `0x005F3930`), and not Marked (`0x005F394B`).
    pub(crate) fn reset_constructed(&mut self) {
        self.object_alive = true;
        self.in_limbo = true;
        self.cell_marked = false;
    }

    /// IsAlive set again on an object that took a lethal hit: PostMortem's
    /// restore (`0x00701F60`) and a ship that starts sinking (Unit
    /// `0x00737E4A`). Each also stores Health 1.
    pub(crate) fn restore_alive(&mut self) {
        self.object_alive = true;
    }

    /// Install a fixture's IsAlive.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_object_alive_for_test(&mut self, alive: bool) {
        self.object_alive = alive;
    }

    /// Install a fixture's InLimbo.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_in_limbo_for_test(&mut self, in_limbo: bool) {
        self.in_limbo = in_limbo;
    }
}

/// The dying mark: an object on the VERA-only SHP death sequence of a
/// non-Infantry object (`combat::world_receiver`), or past its UnInit until
/// the pending delete drains it. VERA-only: native has no such field. Native
/// UnInit (`ObjectClass::UnInit @ 0x005F65F0`) clears IsAlive (`+0x90`),
/// which VERA's UnInit clears as well; native has no SHP death sequence.
///
/// An infantryman's death sets no mark: his corpse keeps IsAlive at Health 0
/// and goes on through his ordinary visit (`world::infantry_death`).
///
/// Writers: [`Self::begin`] (the receiver's SHP death and UnInit) and
/// [`Self::restore`] (PostMortem).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminalLifetime {
    dying: bool,
}

impl TerminalLifetime {
    /// The object is on its SHP death sequence or past its UnInit.
    pub const fn is_dying(self) -> bool {
        self.dying
    }

    /// Mark the object dying: a non-Infantry SHP death sequence, or UnInit.
    pub(crate) fn begin(&mut self) {
        self.dying = true;
    }

    /// PostMortem's restore (`0x00701F60`): alive again at Health 1.
    pub(crate) fn restore(&mut self) {
        self.dying = false;
    }

    /// Install a fixture's dying mark.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_dying_for_test(&mut self, dying: bool) {
        self.dying = dying;
    }
}

/// The control value `DispatchPointerExpiredCleanup @ 0x007258D0` forwards to
/// every listener's `PointerExpired` slot (`vt+0x28`), i.e. the third argument
/// of `TechnoClass::PointerExpired @ 0x007077C0`.
///
/// Two production callers, and they disagree on the value:
/// * `ObjectClass__UnInit @ 0x005F65F0` dispatches with **1** at `0x005F6616` —
///   the object is going away, so every reference is dropped unconditionally.
/// * `ObjectClass::Detach_All(bool)` (vtable `+0xDC`, `0x005F5280`; the
///   `FootClass` override is `0x004D9720`) forwards its own argument, and both
///   cloak callers pass **0**: `TechnoClass::StartCloaking @ 0x00703770` and
///   the `CloakState 1 -> 2` arm of `TechnoClass::CloakingTick @ 0x006FB740`.
///
/// A control of 0 changes three things inside the receiver body: it runs the
/// `allowClear` sensor test (`0x00707994 CALL 0x004870D0`,
/// `CellClass::SensorCountForHouse`), it exempts a receiver whose own house
/// owns the expiring object from the Target clear (`0x007079B7..0x007079CB`),
/// and it skips the Techno `+0x500` / `+0x218` / CaptureManager block opened
/// at `0x00707AE7`. After that body returns, Foot's override independently
/// clears an exact ArchiveTarget match on BOTH controls (`4D99F1..4D99FC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerExpiryControl {
    /// `Detach_All(false)` — the expiring object survives.
    DetachAll,
    /// `ObjectClass::UnInit` — the expiring object is being removed.
    Uninit,
}

/// Borrowed map authority carried through one synchronous receiver lifecycle
/// tree, including nested survivor Reveal as well as UnInit. Ordinary entry
/// points use the Simulation-owned terrain; combat uses this context while
/// that same terrain is staged outside `Simulation`.
/// Native clear/repair routines always query the global MapClass: Unit clear
/// `0x00744210` (RemoveContent `0x0047EA90`, vt+0xF4), Aircraft clear
/// `0x005F6120` (vtable `0x007E22A4`), Building reservation clear `0x004561F0`,
/// and Bullet expiry `0x004684E0`. Source: active gamemd.exe bodies and vtables.
/// An absent resident field during receiver execution is not an empty map.
#[derive(Clone, Copy)]
pub struct UninitContext<'a> {
    terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules: &'a RuleSet,
    registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    requested_facing: Option<u8>,
    effects: FrameEffects<'a>,
}

impl<'a> UninitContext<'a> {
    /// A caller's rules and overlay table, with no terrain override.
    pub const fn new(
        rules: &'a RuleSet,
        registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Self {
        Self {
            terrain: None,
            rules,
            registry,
            requested_facing: None,
            effects: FrameEffects::empty(),
        }
    }

    pub const fn with_rules(rules: &'a RuleSet) -> Self {
        Self::new(rules, None)
    }

    /// Preserve the current frame's synchronous sound boundary through nested
    /// lifecycle receivers. It borrows the caller; no callback survives the
    /// enclosing frame or becomes simulation state.
    pub const fn with_effects(self, effects: FrameEffects<'a>) -> Self {
        Self { effects, ..self }
    }

    /// The effects, with this context's overlay table bound when they carry
    /// none, so a receiver reads one table whichever carrier lent it.
    pub(crate) fn effects(self) -> FrameEffects<'a> {
        self.effects.with_overlay_registry(self.registry)
    }

    /// Techno Unlimbo's direction for this synchronous reveal. The caller's
    /// raw Z stays in RevealPosition, through the existing type clamp before
    /// Mark; successful, alive Techno6F6DAA snaps the body before idle.
    pub(crate) const fn with_unlimbo_facing(self, requested_facing: Option<u8>) -> Self {
        Self {
            requested_facing,
            ..self
        }
    }

    /// The match's OverlayTypeClass table, for receivers that classify a
    /// cell's overlay (an ejected occupant's Scatter entry test).
    pub(crate) const fn with_registry(
        self,
        registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Self {
        Self { registry, ..self }
    }

    pub(crate) const fn registry(
        self,
    ) -> Option<&'a crate::rules::overlay_types::OverlayTypeRegistry> {
        self.registry
    }

    pub(crate) const fn terrain(
        self,
    ) -> Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid> {
        self.terrain
    }

    pub const fn rules(self) -> &'a RuleSet {
        self.rules
    }
}

/// A fixture's context: the rules an empty INI reads to
/// ([`RuleSet::empty_for_test`]) and no overlay table.
#[cfg(any(test, feature = "test-support"))]
impl Default for UninitContext<'static> {
    fn default() -> Self {
        Self::new(RuleSet::empty_for_test(), None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementEvidence {
    RejectedEarly,
    MarkFailed,
    MarkSucceeded,
    /// An Unlimbo request for the class admission
    /// ([`Simulation::unlimbo_class_entry`]), which fixed/runtime constructors
    /// use so a later placement rejection can retain the spent constructor.
    /// What reaches Reveal unconverted (a building, or a map-less fixture)
    /// runs VERA's playfield gate before Mark(PUT); see
    /// `reveal_position_is_in_playfield`.
    EvaluateMark,
    /// Object5F4F1B accepted a Foot's admission (Unit, Infantry or Aircraft):
    /// either its class +1AC receiver returned zero or the caller's A8E7AC
    /// scope skipped it. No playfield gate follows: no Unlimbo layer refuses
    /// on IsCellInPlayfield (`0x00578460`). Aircraft (`0x00414310`) asks it
    /// (through `0x005785F0`, at `0x00414352`) only for the Unlimbo Z,
    /// Infantry (`0x0051E06B`) only for its placement arm, and Techno only to
    /// write `+0x3D5` (`0x006F6CF9`) and clear `+0x41B` (`0x006F6EA4`); Foot
    /// (`0x004D7170`) and Object (`0x005F4EC0`) do not ask.
    /// Query-local height/list outputs never establish the object's pose.
    FootEntryAdmitted,
}

impl Simulation {
    /// A8E7AC's caller-owned bracket. Native factory444575..444979/444EE6
    /// retains it through Unlimbo, Mark and radio callbacks; nested scopes
    /// increment/decrement the same dword. SpawnSurvivors443141..443288
    /// and passenger escape738030..7381A1 retain it through Scatter and
    /// immediate Walk/cell callbacks. Session GameMode is A8B238.
    pub(crate) fn with_object_placement_scope<R>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.object_placement_scope_depth = self.object_placement_scope_depth.wrapping_add(1);
        let result = operation(self);
        self.object_placement_scope_depth = self.object_placement_scope_depth.wrapping_sub(1);
        result
    }

    pub(crate) fn object_placement_scope_active(&self) -> bool {
        self.object_placement_scope_depth != 0
    }

    /// Crate validation4A18F0 saves A8E7AC, clears it at4A190B, and
    /// restores it on both success4A1A7D and refusal4A1A8E. Its callbacks
    /// therefore use ordinary admission even inside an initial-object scope.
    /// Source: original gamemd; crate_pickup and wall-autofill callers.
    pub(crate) fn without_object_placement_scope<R>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prior = std::mem::take(&mut self.object_placement_scope_depth);
        let result = operation(self);
        self.object_placement_scope_depth = prior;
        result
    }

    /// Raise VERA's analog of native `0xA8ED6B` around `operation`: an initial
    /// building Unlimbo inside it takes Enter_Idle_Mode's Idle arm
    /// (`0x0044D6A0`) and stands finished, with no build-up. Native sets the
    /// byte only around the random map generator's GALITE pass
    /// (`0x005A91E0`); VERA raises it around a convenience spawn's Unlimbo
    /// ([`Self::spawn_object`]), whose buildings stand like a map's. Native's
    /// other readers of the byte keep the clear byte's answer.
    pub(crate) fn with_buildup_bypass<R>(&mut self, operation: impl FnOnce(&mut Self) -> R) -> R {
        let saved = std::mem::replace(&mut self.buildup_bypass, true);
        let result = operation(self);
        self.buildup_bypass = saved;
        result
    }

    pub(crate) fn buildup_bypass_active(&self) -> bool {
        self.buildup_bypass
    }
}

pub(super) fn building_base_reservation_rect(
    rx: u16,
    ry: u16,
    foundation: &str,
    spacing: i32,
) -> CellRect {
    let (width, height) = crate::rules::foundation::foundation_dimensions(foundation);
    CellRect::new(
        native_base_reservation_start(rx, spacing),
        native_base_reservation_start(ry, spacing),
        i32::from(width).wrapping_add(spacing.wrapping_mul(2)),
        i32::from(height).wrapping_add(spacing.wrapping_mul(2)),
    )
}

fn native_base_reservation_start(anchor: u16, spacing: i32) -> i32 {
    i32::from(i32::from(anchor).wrapping_sub(spacing) as i16)
}

fn packed_reservation_coord(x: i32, y: i32) -> u32 {
    u32::from(x as i16 as u16) | (u32::from(y as i16 as u16) << 16)
}

fn base_reservation_perimeter_rect(rect: CellRect) -> CellRect {
    CellRect::new(
        rect.x.wrapping_sub(1),
        rect.y.wrapping_sub(1),
        rect.width.wrapping_add(3),
        rect.height.wrapping_add(3),
    )
}

/// The "no valid cell here" sentinel `BulletClass::PointerExpired` compares its
/// truncated target cell against, at `0x0046856E` and `0x0046857C`. Both
/// `DAT_0089DDF0`/`DAT_0089DDF2` words read zero in the image, and the only
/// writer in the program — the four-instruction routine at `0x00466270` — zeroes
/// them, so the sentinel is the cell (0, 0) rather than a general off-map test.
const NULL_TARGET_CELL_SENTINEL: (i16, i16) = (0, 0);

/// Cell selected after the represented ObjectClass virtual `GetCoords` result
/// is truncated from world leptons. BuildingClass shifts its stored NW anchor
/// to the geometric foundation center before that truncation.
///
/// The `None` arm is unreachable at retail map sizes rather than by
/// construction. `position.rx`/`ry` are `u16` and the in-cell offsets stay in
/// `0..256`, but the Structure arm adds `(width - 1) * 128` leptons before
/// truncating, so a multi-cell structure at the very top of the `u16` cell range
/// would overflow and return `None`. No retail map approaches that bound.
/// Native's own guard against a bad coordinate is the (0, 0) sentinel, checked
/// at the callback instead, so this arm is not the sentinel's analogue.
pub(super) fn object_get_coords_cell(
    entity: &crate::sim::game_entity::GameEntity,
) -> Option<(u16, u16)> {
    let [world_x, world_y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    Some((
        u16::try_from(crate::util::lepton::lepton_to_cell(world_x)).ok()?,
        u16::try_from(crate::util::lepton::lepton_to_cell(world_y)).ok()?,
    ))
}

pub(super) fn building_base_reservation_repair_rect(
    rx: u16,
    ry: u16,
    foundation_width: u16,
    foundation_height: u16,
    spacing: i32,
) -> CellRect {
    let five_times_spacing = spacing.wrapping_mul(5);
    let primary_x = native_base_reservation_start(rx, spacing);
    let primary_y = native_base_reservation_start(ry, spacing);
    CellRect::new(
        primary_x.wrapping_sub(spacing),
        primary_y.wrapping_sub(spacing),
        i32::from(foundation_width).wrapping_add(five_times_spacing),
        i32::from(foundation_height).wrapping_add(five_times_spacing),
    )
}

/// A caller's placement coordinate. Reuse the Location representation so a
/// native CoordStruct can retain its exact Z independently of height levels.
pub type RevealPosition = crate::sim::components::Position;

#[derive(Debug, Clone, Copy)]
pub struct RevealRequest {
    pub position: RevealPosition,
    pub placement: PlacementEvidence,
    /// Caller-supplied result of the still-blocked native type/mode gate.
    pub logic_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealFailure {
    MissingObject,
    RejectedEarly,
    MarkFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealOutcome {
    Revealed {
        logic_registered: bool,
    },
    /// Building440580's wall arm succeeded and already UnInit'd the
    /// constructor. It remains in limbo until the shared deferred drain.
    Consumed,
    AlreadyRevealed,
    Failed(RevealFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcealOutcome {
    Concealed,
    AlreadyConcealed,
    MissingOrDead,
}

/// Release-visible lifecycle handoffs.  Consumers may be temporarily no-op,
/// but the stream preserves the verified native relative ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleOutput {
    /// Event OPTIONS4C7929..4C7939 requests GameState1. The app opens the
    /// modal after MainTick returns, through its existing menu owner.
    OptionsRequested,
    /// Event LATENCYFUDGE4C7F35..4C7F4F writes the existing network-session
    /// setting. Transport owns its persistent value, not Simulation.
    LatencyFudgeChanged {
        owner: InternedId,
        value: i32,
    },
    /// Building4456D0 attempts AddCameo before House4F92E2 recalculates.
    /// The retained client strip owns duplicate/capacity admission. Emit in
    /// House building/type order at the simulation phase that made the query.
    SidebarConstructionOption {
        owner: InternedId,
        type_id: InternedId,
    },
    /// Local House4F927C consumes +1FC before4F92E2 recalculates the retained
    /// sidebar. Sale begin and ordinary StripAI/Draw do not request this pass.
    SidebarRecalculated {
        owner: InternedId,
    },
    /// NodeDelete6380A6 captures its FIRST member successor before removal;
    ///6380EE updates only the local house retained presentation highlight.
    PlanningNodeDeleted {
        owner: InternedId,
        node_id: u64,
        next_node: Option<u64>,
    },
    /// HouseBegin4FA41C clears the category prediction gate before builder
    /// admission, including a refused Begin. The strip's request survives.
    /// Source: tools/sidebar_oracle/pending_production.md.
    ProductionBeginAttempted {
        owner: InternedId,
        category: crate::sim::production::ProductionCategory,
        naval: bool,
    },
    /// Local HouseBegin4FA6AA→Sidebar6A6140 links the reached cameo to this
    /// live Factory. Product expiry and a later rearm do not erase the link.
    ProductionFactoryLinked {
        owner: InternedId,
        type_id: InternedId,
        category: crate::sim::production::ProductionCategory,
        construction_order: u64,
    },
    /// Local HouseAbandon4FAB74 enters Sidebar6ABAD0. Its private strip
    /// owner determines whether the cursor prefix precedes settlement (no
    /// link) or follows the first linked FactoryAbandon/destructor.
    ProductionFactoryAbandoning {
        owner: InternedId,
        category: crate::sim::production::ProductionCategory,
        construction_order: u64,
    },
    /// Strip6ABBDF clears every matching link after the linked destructor,
    /// before the House cursor prefix and successor Begin. Queued-only
    /// removal and direct Factory/product expiry have neither House fact.
    ProductionFactoryUnlinked {
        owner: InternedId,
        category: crate::sim::production::ProductionCategory,
        construction_order: u64,
    },
    /// HousePlace4FB35F: actual building Unlimbo failed, after HELLO. The
    /// local input owner restores its saved transaction, not the event type.
    BuildingPlacementRetry {
        owner: InternedId,
    },
    /// HousePlace4FB328 clears active and saved placement after BuildingSlam.
    BuildingPlacementSucceeded {
        owner: InternedId,
    },
    /// HouseAbandon4FAB83/4FAB9F clears the local current cursor. The app's
    /// strip owner derives this effect from the admitted House begin/end
    /// facts, preserving its link-dependent order around product expiry.
    BuildingProductionAbandoned {
        owner: InternedId,
    },
    /// Logic55AFB0 increments the process FPS sample even on terminal frames.
    LogicVisit,
    /// Logic55B5C3 updates existing lasers before the live object visits.
    LaserUpdate {
        frame: i32,
    },
    /// Copied LaserDraw54FE60 birth; no source/target pointer to detach.
    LaserCreated(crate::sim::combat::laser::LaserBirth),
    ElectricBoltCreated(crate::sim::combat::electric_bolt::ElectricBoltBirth),
    /// SpawnRadBeam6FD620 modes0/1 copy endpoints into a Tactical-lived effect.
    RadiationBeamCreated(crate::sim::combat::radiation_beam::RadiationBeamBirth),
    /// ObjectUnlimbo5F517A/5F5207 constructs and attaches a presentation trail.
    LineTrailConstructed {
        stable_id: u64,
        style: crate::sim::projectile::ProjectileLineTrail,
    },
    /// ObjectDtor5F3D56 detaches; its fading registry entry remains alive.
    LineTrailDetached {
        stable_id: u64,
    },
    /// ObjectDetachAll5F52EF..5F5306 hands the follow camera back when it
    /// follows this object, whatever the argument and the Deselect gate. A
    /// logic Select's Unselect_All emits one per object it deselects
    /// (Deselect `0x005F4503..0x005F4513`), which a later Select in the same
    /// frame may select again.
    FollowReleased {
        stable_id: u64,
    },
    /// A Select (`TechnoClass::Select @ 0x006FBFA0`, vt+0x14C) the game made
    /// itself rather than local input: its ObjectClass::Select
    /// (`0x005F4520`) inserted the object into the client's current-objects
    /// vector (`0x00A8ECB8`), before its Selected tag event (`0x006FBFE4`).
    /// Outputs follow Select call order: the actor, each partner as
    /// `0x00637840` Selects it, and any Select a tag event starts in between.
    /// Input Selects write the app's selection ledger themselves.
    ObjectSelected {
        stable_id: u64,
    },
    /// DispatchPointerExpired7258D0 reaches navigation733160 at7259B1
    /// for a Techno abstract pointer, regardless of announce-control or Alive.
    ObjectPointerExpired {
        stable_id: u64,
    },
    RevealDisplay {
        stable_id: u64,
    },
    DisplayRemove {
        stable_id: u64,
    },
    DetachAttachedAnims {
        stable_id: u64,
    },
    DirtyTacticalRect {
        stable_id: u64,
    },
    ClearDrawnState {
        stable_id: u64,
    },
    ClearRedraw {
        stable_id: u64,
    },
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleTestEvent {
    RevealLimboCleared,
    RevealCoordinatesCommitted,
    MarkPut,
    UnlimboBodyFacingSnapped,
    UnlimboIdleMode,
    RawOccupationListLinked,
    HiddenOccupationEntered,
    BaseReservationMarked,
    RawOccupationMarked,
    CellMarked,
    BuildConstAppended {
        stable_id: u64,
    },
    BasePlanFilled {
        stable_id: u64,
    },
    RevealDisplayBoundary,
    LogicAppended,
    LogicMembershipSet,
    ConcealDeselected,
    ConcealDestroyNotifyBoundary {
        stable_id: u64,
        object_alive: bool,
        cell_marked: bool,
        resolvable: bool,
    },
    ConcealAlreadyLimboReturn {
        stable_id: u64,
        object_alive: bool,
        resolvable: bool,
    },
    BaseReservationCleared,
    RawOccupationListUnlinked,
    HiddenOccupationExited,
    RawOccupationCleared,
    ConcealUnmarked,
    ConcealDisplayBoundary,
    ConcealAnimBoundary,
    ConcealVocBoundary,
    ConcealLogicRemoved,
    ConcealDirtyTacticalRectBoundary,
    ConcealClearDrawnStateBoundary,
    ConcealLimboSet,
    ConcealClearRedrawBoundary,
    BreakSlot {
        slot: usize,
        target: Option<u64>,
    },
    BreakSenderCleared {
        target: u64,
    },
    BreakReceiverClassEffect {
        target: u64,
    },
    BreakReceiverCleared {
        target: u64,
    },
    UninitClassPre {
        stable_id: u64,
    },
    UninitRemovalNotifyBoundary {
        stable_id: u64,
        object_alive: bool,
        cell_marked: bool,
        resolvable: bool,
    },
    UninitRemovalListenerVisited {
        expired_id: u64,
        listener_id: u64,
        target_alive: bool,
        target_in_limbo: bool,
    },
    /// Native stock constructor/UnInit/scalar-dtor expiry comparison. Captures
    /// the shared dispatch boundary, including readiness and pending order.
    SmudgeExpiryBoundary {
        stable_id: u64,
        native_id: i32,
        native_cursor: u32,
        object_alive: bool,
        in_limbo: bool,
        health: i32,
        location: [i32; 3],
        pending: Vec<u64>,
        generic_objects: Vec<u64>,
        rng: [String; 3],
    },
    ProjectilePointerExpiredVisited {
        expired_id: u64,
        projectile_id: u64,
        expired_resolvable: bool,
        projectile_resolvable: bool,
        source_id: u64,
        target: ProjectileTarget,
    },
    UnfiredProjectileConstructed {
        stable_id: u64,
        native_id: i32,
        source_id: u64,
        target: ProjectileTarget,
        payload: crate::sim::projectile::ProjectilePayload,
        position: crate::sim::projectile::ProjectileCoord,
        in_logic: bool,
        in_display: bool,
    },
    TerrainExpiryBoundary {
        stable_id: u64,
        cell: (u16, u16),
        indexed: bool,
        in_logic: bool,
        live: bool,
    },
    WaveDamageReceiverSelected {
        wave_id: u64,
        target_id: u64,
        scenario_rng_state: u64,
    },
    /// The FireAt transaction has committed every receiver/effect owned by
    /// this shot, including a Wave it registered (Sonic `0x006FF470`, MagBeam
    /// `0x006FF647`); that Wave's Logic tail must not dispatch until all later
    /// pre-existing callbacks.
    CombatFireEffectsCommitted {
        attacker_id: u64,
        scenario_rng_state: u64,
    },
    UninitAliveCleared {
        stable_id: u64,
    },
    RecordKillBookkeeping {
        stable_id: u64,
    },
    DestroyRadioBreakCompleted {
        stable_id: u64,
    },
    DestroyDeselected {
        stable_id: u64,
    },
    DestroyNotifyBoundary {
        stable_id: u64,
    },
    BuildingNowDeadRunAway {
        building_id: u64,
        contact_id: u64,
    },
    /// One visited listener of the live-detach targeting sweep, in the order
    /// the sweep visited it. Recorded for every listener that was pointed at
    /// the detaching object, so a test can pin the descending walk.
    DetachTargetingSweepVisited {
        detach_id: u64,
        listener_id: u64,
        restored: bool,
        target_cleared: bool,
    },
    PendingDeleteQueued {
        stable_id: u64,
    },
    /// What a Fly `Stop_Moving` decided for aircraft `id`.
    FlyStopOrdered {
        id: u64,
        order: super::fly_orders::FlyStopOrder,
    },
    BinaryFrameCommitted,
    PendingDeleteDrainStarted,
    DeferredDeleteSelected {
        ticket: DeferredDeleteTicket,
    },
    FinalizedCommon {
        stable_id: u64,
    },
}

impl Simulation {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn trace_lifecycle_for_test(&mut self, event: LifecycleTestEvent) {
        self.lifecycle_test_events.push(event);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn lifecycle_test_events_for_test(&self) -> &[LifecycleTestEvent] {
        &self.lifecycle_test_events
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_lifecycle_test_events_for_test(&mut self) {
        self.lifecycle_test_events.clear();
    }

    /// The Location Z Object Unlimbo 0x5F4EC0 commits (SetLocation at
    /// `0x005F4FA8`), adapted from the level-based placement API. It passes its
    /// input coordinate through the type's virtual +0x6C (`0x005F4F88`):
    /// - BuildingType 0x464A70 keeps the XY and takes the floor there
    ///   (`0x578080`) as the Z, with no deck, whatever the input Z.
    /// - UnitType 0x747EB0 / InfantryType 0x5247D0 clamp that input to the
    ///   exact ground surface, whatever the locomotor, and add no bridge
    ///   offset. Authored bridge placement supplies its deck coordinate before
    ///   that clamp; translate our coarse deck request here. This coarse API
    ///   cannot recover all raw authored inputs (notably Unit input zero on
    ///   negative terrain); the report records that caller residual. See
    ///   docs/research/RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md.
    /// - AircraftType keeps the coordinate (`0x0041CF80`), whose Z
    ///   AircraftClass::Unlimbo 0x414310 set first: a `MissileSpawn=` type
    ///   keeps its input (`0x00414338`), with no floor read; one without
    ///   Techno+3D4 (`0x00414342`) inside the playfield (`0x005785F0`) takes
    ///   the floor (`0x00414361`); any other the floor plus its FlightLevel
    ///   (type virtual +0xBC, `0x00414383`). Native comparison:
    ///   tools/spatial_oracle/aircraft_unlimbo_height.json.
    ///
    /// A falling paradrop keeps its drop's Z, and a `MissileSpawn=` aircraft
    /// the Z its caller staged on its limbo Location (the spawn launch's
    /// coordinate, `spawn_manager::launch_coordinate`). Exact-coordinate input
    /// is retained independently of the legacy coarse level. `None` keeps no exact Z, for:
    /// - a tube owner, whose own state carries the height;
    /// - a `MissileSpawn=` aircraft whose caller staged no exact Z (no
    ///   production caller does);
    /// - an Aircraft whose type the rules lack (fixtures);
    /// - an object revealed without terrain (headless fixtures), except a
    ///   `MissileSpawn=` aircraft, whose Z reads no floor.
    fn unlimbo_z(
        &self,
        stable_id: u64,
        position: RevealPosition,
        context: UninitContext<'_>,
    ) -> Option<i32> {
        use crate::sim::movement::ground_pose::ground_surface_z_at;

        let xy = crate::sim::movement::ground_pose::position_world_xy(&position);
        let entity = self.substrate.entities.get(stable_id)?;
        if entity.is_falling_down() {
            // Paradrop's Unlimbo coordinate is the drop coordinate, which
            // SetLocation then commits whole (`0x005F5A50`): the falling
            // object keeps the Z its drop gave it.
            return position
                .exact_z_leptons()
                .or(entity.position.exact_z_leptons());
        }
        if entity.low_bridge_tube_state.is_some() {
            return None;
        }
        let aircraft_type = (entity.category() == EntityCategory::Aircraft)
            .then(|| {
                self.interner
                    .object_type(entity.type_ref(), context.rules())
            })
            .flatten();
        if aircraft_type.is_some_and(|object| object.missile_spawn) {
            return position
                .exact_z_leptons()
                .or(entity.position.exact_z_leptons());
        }
        let terrain = context.terrain().or(self.resolved_terrain.as_ref())?;
        let ground_z = ground_surface_z_at(xy, false, Some(terrain), None)?;
        match entity.category() {
            EntityCategory::Structure => return Some(ground_z),
            EntityCategory::Aircraft => {
                let object = aircraft_type?;
                return Some(
                    if !entity.is_mission_only()
                        && self.reveal_position_is_in_playfield(position, context)
                    {
                        ground_z
                    } else {
                        ground_z
                            .wrapping_add(object.flight_level(context.rules().general.flight_level))
                    },
                );
            }
            EntityCategory::Unit | EntityCategory::Infantry => {}
        }
        let level = terrain
            .native_fixed_cell_index((xy[0] / 256) as i16, (xy[1] / 256) as i16)
            .map_or_else(
                || terrain.shared_cell_dummy().snapshot().level,
                |index| terrain.cells()[index].level as i8,
            );
        let input_z = if let Some(input_z) = position.exact_z_leptons() {
            input_z
        } else if entity.on_bridge() && i32::from(position.z() as i8) == i32::from(level) + 4 {
            // VERA input adapter: retain the ramp remainder that the existing
            // coarse bridge-level API cannot carry. Native clamp remains max.
            ground_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS)
        } else {
            i32::from(position.z() as i8).wrapping_mul(LEPTONS_PER_LEVEL as i32)
        };
        Some(input_z.max(ground_z))
    }

    /// The object's own Location (`Object+0x9C`), the coordinate a Reveal at the
    /// current position passes on.
    fn current_reveal_position(&self, stable_id: u64) -> Option<RevealPosition> {
        self.substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.position)
    }

    /// Compatibility convenience for already-admitted current-position callers.
    /// It still executes the complete result-bearing Reveal transaction.
    #[cfg(any(test, feature = "test-support"))]
    pub fn reveal(&mut self, stable_id: u64) -> RevealOutcome {
        if self.substrate.anims.contains_key(stable_id) {
            let registered = self.reveal_anim(
                stable_id,
                crate::rules::ruleset::RuleSet::empty_for_test(),
                true,
            );
            return RevealOutcome::Revealed {
                logic_registered: registered,
            };
        }
        if self.substrate.particle_systems.contains_key(stable_id) {
            let registered = self.reveal_particle_system(
                stable_id,
                crate::rules::ruleset::RuleSet::empty_for_test(),
            );
            return RevealOutcome::Revealed {
                logic_registered: registered,
            };
        }
        let Some(position) = self.current_reveal_position(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        self.try_reveal_entity(
            stable_id,
            RevealRequest {
                position,
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
        )
    }

    /// [`Self::reveal`] for an entity, with the rules context the type-reading
    /// Unlimbo writers need (Aircraft Techno+3D4 retention, Ground sort keys).
    pub(crate) fn reveal_entity_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) -> RevealOutcome {
        self.reveal_entity_with_context(stable_id, UninitContext::with_rules(rules))
    }

    /// A nested Reveal retains the enclosing frame and receiver authorities.
    pub(crate) fn reveal_entity_with_context(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> RevealOutcome {
        let Some(position) = self.current_reveal_position(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        self.try_reveal_entity_with_context(
            stable_id,
            RevealRequest {
                position,
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            },
            context,
        )
    }

    /// `TechnoClass::Unlimbo`'s barrel elevation writes (`+0x370`,
    /// `0x006F6DC3`, `0x006F6DF5`), aimed by the object's `FireAngle=`, behind
    /// the alive gate that also guards its Added_To_Game (`0x006F6D04`).
    /// The parent retains that one pre-callback admission through its suffix;
    /// this writer only checks that the admitted object still exists.
    pub(crate) fn unlimbo_barrel_elevation(
        interner: &StringInterner,
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        stable_id: u64,
        rules: &RuleSet,
    ) {
        let Some(fire_angle) = substrate
            .entities
            .get(stable_id)
            .and_then(|entity| interner.object_type(entity.type_ref(), rules))
            .map(|object| object.fire_angle)
        else {
            return;
        };
        let frame = session.binary_frame();
        if let Some(entity) = substrate.entities.get_mut(stable_id) {
            entity.unlimbo_barrel_elevation(fire_angle, frame);
        }
    }

    /// ObjectClass::Reveal: clear limbo for the attempt, commit coordinates,
    /// Mark(PUT), expose display, then append eligible LogicClass membership.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn try_reveal_entity(
        &mut self,
        stable_id: u64,
        request: RevealRequest,
    ) -> RevealOutcome {
        self.try_reveal_entity_with_context(stable_id, request, UninitContext::default())
    }

    /// A nested receiver Reveal uses the same map authority as its enclosing
    /// destruction transaction. SellBuilding @ 0x00458060 still reaches the
    /// global MapClass membership writer in Techno Unlimbo @ 0x006F6CC0..6CFE.
    /// Source: active gamemd.exe call and instruction bodies.
    pub(crate) fn try_reveal_entity_with_context(
        &mut self,
        stable_id: u64,
        request: RevealRequest,
        context: UninitContext<'_>,
    ) -> RevealOutcome {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return RevealOutcome::Failed(RevealFailure::MissingObject);
        };
        if !entity.lifecycle.in_limbo {
            return RevealOutcome::AlreadyRevealed;
        }
        if entity.lifecycle.cell_marked || request.placement == PlacementEvidence::RejectedEarly {
            return RevealOutcome::Failed(RevealFailure::RejectedEarly);
        }
        if request.placement == PlacementEvidence::EvaluateMark
            && !self.object_placement_scope_active()
            && !self.reveal_position_is_in_playfield(request.position, context)
        {
            return RevealOutcome::Failed(RevealFailure::RejectedEarly);
        }

        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = false;
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealLimboCleared);

        // Foot4D9C60 adjusts query-local height/list pointers. OnBridge and
        // the caller's requested XYZ remain independent through Unlimbo.
        let position = request.position;
        let exact_z = self.unlimbo_z(stable_id, position, context);
        // RESIDUAL: `ObjectClass::Unlimbo` sets this Location through
        // SetLocation (vt+0x1B4 at 0x005F4FA8, before its Mark(1) at
        // 0x005F4FB4), which for a Foot also runs the OpenTopped rider tail.
        // VERA copies the parts, keeping no exact Z for a tube owner
        // (`unlimbo_z`), so it cannot go through `foot_set_location`
        // yet. Trigger: a loaded OpenTopped transport revealed away from its
        // riders. Frequency: none known in retail. Effect: the riders keep
        // their old Location.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity
                .position
                .copy_from(crate::sim::components::Position::new(
                    position.rx(),
                    position.ry(),
                    position.z(),
                    exact_z,
                    position.sub_x(),
                    position.sub_y(),
                ));
        }
        self.reposition_building_anim_slots(stable_id, context.rules());
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealCoordinatesCommitted);

        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::MarkPut);
        if request.placement == PlacementEvidence::MarkFailed {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.lifecycle.in_limbo = true;
            }
            return RevealOutcome::Failed(RevealFailure::MarkFailed);
        }

        if !self.mark_entity_put(stable_id, context) {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.lifecycle.in_limbo = true;
            }
            return RevealOutcome::Failed(RevealFailure::MarkFailed);
        }
        // Techno6F6CB1 calls Object Unlimbo, and its failed return6F6CB8
        // precedes the +3D5 establishment at6F6CFE. Failed Mark retains the
        // prior byte. Headless fixtures have no MapClass authority and retain
        // the constructor default through this existing writer.
        self.establish_entity_playfield_membership_on_unlimbo(stable_id, context);
        // Techno6F6D04/D0C tests +90 once before the synchronous sight
        // receiver. Original6F6D27..6EDE does not recheck it after callbacks;
        // carry this admission while each owner retains object-existence safety.
        let techno_alive_on_entry = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive);
        //6F6D27/6F6D61: source sight and Cell ground-frame border run
        //synchronously before Added_To_Game and the ordinary discovery tail.
        if techno_alive_on_entry {
            self.reveal_object_sight_on_unlimbo(stable_id, context);
        }
        // `TechnoClass::Unlimbo` calls Added_To_Game once placement succeeded
        // (`0x006F6D8F`), behind its alive gate (`0x006F6D04`: a dead Techno
        // returns success before it).
        if techno_alive_on_entry {
            self.house_added_to_game(stable_id, context.rules());
        }
        // Techno6F6DAA: the admitted successful Techno snaps
        // the body to this call's direction, before barrel elevation and idle.
        // Its original6F6CA0 body is retained in the shared Unlimbo evidence.
        if techno_alive_on_entry
            && let Some(facing) = context.requested_facing
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity
                .body_facing
                .snap(u16::from(facing) << 8, self.session.binary_frame());
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::UnlimboBodyFacingSnapped);
        }
        // The barrel elevation writes follow the body snap (`0x006F6DC3`).
        if techno_alive_on_entry {
            Self::unlimbo_barrel_elevation(
                &self.interner,
                &self.session,
                &mut self.substrate,
                stable_id,
                context.rules(),
            );
        }
        // TechnoClass::Unlimbo 0x006F6E2A..0x006F6E4F: Enter_Idle_Mode(1, 1),
        // Ready_To_Commence and Commence, ahead of its second mode-one query
        // and of the owner discovery below (Foot 0x004D722F, Building
        // 0x00440E5C), which observes the mission it left: a Foot's committed
        // idle mission, or a placed building's queued Construction. An
        // upgrade's Unlimbo attaches its type to the host and returns
        // (0x00440A1C..0x00440A2F) before TechnoClass::Unlimbo.
        if techno_alive_on_entry {
            #[cfg(any(test, feature = "test-support"))]
            if context.requested_facing.is_some() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::UnlimboIdleMode);
            }
            super::techno_unlimbo_idle_mode(
                self,
                stable_id,
                context.rules(),
                context.registry(),
                context.effects(),
            );
        }
        // TechnoUnlimbo6F6E65..AD runs this second mode-one query only
        // after successful Object Mark and the +90 alive gate. A failed Mark
        // must retain history. This precedes Foot4D722F's owner observation.
        // The dead Techno arm still returns success to Foot4D7184, so only
        // this writer is gated: ForceSlope, owner discovery and Sight follow.
        if techno_alive_on_entry
            && self.entity_playfield_membership_mode_one(
                stable_id,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ) == Some(false)
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.discovery.discovered_by_current_house = false;
        }
        // Techno6F6ED2/6F6EDE publishes its current threat before the Foot
        // tail. Failed Mark and the dead successful Reveal arm publish none.
        if techno_alive_on_entry {
            self.spatial_threat_after_unlimbo(stable_id, context.rules(), context.terrain());
        }
        // FootClass::Unlimbo @ 0x004D7170 dispatches active Drive/Ship
        // Force_Slope at 0x004D71A9 only after TechnoClass placement succeeds.
        // This precedes display/Logic exposure and must not run on either
        // failed placement path above.
        let reveal_slope = self.substrate.entities.get(stable_id).and_then(|entity| {
            context
                .terrain()
                .or(self.resolved_terrain.as_ref())?
                .cell(entity.position.rx(), entity.position.ry())
                .map(|cell| cell.slope_type)
        });
        if let Some(sampled_slope) = reveal_slope
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            crate::sim::movement::slope_transition::snap_after_successful_unlimbo(
                entity,
                sampled_slope,
                self.session.binary_frame(),
            );
        }
        // Ares3.0 FootPut hook4D7221 ->1004E780 follows successful
        // locomotor Put and precedes owner discovery4D722F. Declaration
        // membership alone requests the owner's existing House+1FC pass.
        let rules = context.rules();
        if crate::rules::ARES_ENABLED
            && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() != EntityCategory::Structure
                    && rules.is_ares_prerequisite_alternate(
                        entity.category().into(),
                        self.interner.resolve(entity.type_ref()),
                    ))
                .then_some(entity.owner())
            })
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        // ObjectUnlimbo5F4FB4 Mark/CellPUT has already run. Foot4D722F calls
        // +198(owner) next; only afterward Infantry51E0EF clears +41B for
        // exactly Sight=0. Never move these producers before the Mark call.
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && entity.category() != EntityCategory::Structure
        {
            self.discover_techno(stable_id, entity.owner(), context);
        }
        let high_flight = self.foot_neighbors_after_unlimbo(stable_id, context.rules);
        // Foot4D72B2/+54 requires high flight, then Type ConsideredAircraft
        // (+D96) admits AirTrackerAdd4D72DB, whatever the locomotor. Mark
        // itself never adds anything. A type the rules lack (fixtures) reads
        // +D96 as its default, Aircraft only.
        if high_flight
            && self.substrate.entities.get(stable_id).is_some_and(|e| {
                self.interner
                    .object_type(e.type_ref(), context.rules())
                    .map_or(e.category() == EntityCategory::Aircraft, |o| {
                        o.considered_aircraft
                    })
            })
        {
            Self::aircraft_tracker_add(&self.session, &mut self.substrate, stable_id);
        }
        // FootUnlimbo4D72EA..4D72F4 copies Type+2F0 into Foot+530 after
        // neighbors and optional AirTracker, including the dead-Techno success
        // arm. Both early and Mark refusals above must retain the prior value.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && matches!(
                entity.category(),
                EntityCategory::Infantry | EntityCategory::Unit | EntityCategory::Aircraft
            )
            && let Some(object) = self
                .interner
                .object_type(entity.type_ref(), context.rules())
        {
            entity
                .navigation
                .retain_threat_avoidance_after_unlimbo(object.threat_avoidance_coefficient);
        }
        // Unit737BBE..737BD2 / Aircraft414403..414417 snap Secondary after
        // Foot success, including dead-Techno success. The Techno body snap
        // above has its own alive gate and precedes idle. Diagnostic callers
        // without a direction retain the existing facing seam.
        if let Some(facing) = context.requested_facing
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Aircraft
            )
            && let Some(barrel) = entity.barrel_facing.as_mut()
        {
            barrel.snap(u16::from(facing) << 8, self.session.binary_frame());
        }
        // Unit737BF5..737C75 resets the SAME +F8 StageClass after Foot
        // success. E18/E19 are Unit-section booleans, not General references.
        // The existing owner preserves FC/110; +104 is native stack padding.
        // Original execution/RNG: anytown_damage/unit_unlimbo stage controls.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        {
            let visceroid = self
                .substrate
                .entities
                .get(stable_id)
                .and_then(|entity| {
                    self.interner
                        .object_type(entity.type_ref(), context.rules())
                })
                .is_some_and(|object| object.small_visceroid || object.large_visceroid);
            let value = if visceroid {
                self.scenario_rng.next_range_i32_inclusive(0, 29)
            } else {
                0
            };
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.restart_native_stage(
                    value,
                    self.session.binary_frame() as i32,
                    i32::from(visceroid),
                );
            }
        }
        // Aircraft4143A8 follows successful Foot Unlimbo, including the dead
        // Techno success arm. Failed placement above must not promote +3D4.
        // RESIDUAL: the class Unlimbo tails also write what VERA does not
        // write here.
        // - Concrete coordinate/height Unlimbo supplies a direction to this
        //   shared lifecycle owner. Low-level diagnostic Reveal has no
        //   direction argument and retains the existing facing seam.
        // - The Aircraft +0x6C9 latch (`0x004143F2..0x004143FC`), set when a
        //   first passenger rides at Unlimbo. Dormant: carriers take their
        //   passengers after Unlimbo, and `superweapon::paradrop` records why
        //   VERA keeps no latch for the paradrop's own writes (`0x0065E7B8`,
        //   `0x0065DCE9`).
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            let type_id = self.interner.resolve(entity.type_ref());
            entity.retain_aircraft_unlimbo_control(context.rules(), type_id);
            if entity.category() == EntityCategory::Aircraft
                && let Some(object) = context.rules().object(type_id)
            {
                let height = crate::sim::movement::air_movement::current_fly_height(
                    entity,
                    context.terrain().or(self.resolved_terrain.as_ref()),
                );
                entity.finish_aircraft_unlimbo(
                    height,
                    object.flight_level(context.rules().general.flight_level),
                    self.session.binary_frame() as i32,
                );
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.category() == EntityCategory::Infantry
            && entity.sight.is_zero()
        {
            entity.discovery.discovered_by_current_house = false;
        }
        // Then `InfantryClass::Unlimbo` marks the man's sub-cell through his
        // vt+0xF0 (`0x005217C0`) unless he was placed more than a deck above
        // the ground (`0x0051E0F6..0x0051E10E`). His Mark never writes it.
        if let Some((owner, coord)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Infantry)
            .map(|entity| {
                (
                    entity.owner(),
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position),
                )
            })
        {
            let terrain = context.terrain().or(self.resolved_terrain.as_ref());
            let ground = crate::sim::movement::ground_pose::ground_surface_z_at(
                [coord.x, coord.y],
                false,
                terrain,
                None,
            )
            .unwrap_or(coord.z);
            if coord.z <= ground.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS) {
                crate::sim::movement::walk_head::raw_at(
                    &mut self.substrate.raw_cell_occupation,
                    owner,
                    coord,
                    true,
                    terrain,
                    self.path_grid.as_deref(),
                );
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
            }
        }
        if !self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.object_alive)
        {
            return RevealOutcome::Revealed {
                logic_registered: false,
            };
        }
        Self::mark_ai_repairable_at_unlimbo(
            &self.session,
            &mut self.substrate,
            &self.houses,
            stable_id,
        );
        // Building440D07: recompute from the PRE-append House+68. Failed
        // Mark and the earlier dead arms never reach it.
        if let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(|entity| entity.owner())
        {
            self.recalculate_house_base_geometry(owner, context.rules());
        }
        // Building440D13: IncrementFactoryCount right after it.
        Self::update_house_tracking(
            &mut self.substrate,
            &mut self.houses,
            stable_id,
            |tracking, entity| tracking.increment_factory_count(entity),
        );
        // Building440D1E, after IncrementFactoryCount and before +544.
        if let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(|entity| entity.owner())
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        self.append_live_build_const(stable_id);
        Self::append_house_base_building(&self.substrate, &mut self.houses, stable_id);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            // Building440D2D, after native House building registration.
            entity.reset_building_health_sample_at_unlimbo();
        }
        self.mark_building_base_reservation_with_arg(stable_id, false, context);
        self.discover_building_owner_after_unlimbo(stable_id, context);
        self.construct_spotlight(stable_id, context.rules());
        // Unlimbo44119C clears this object's retained primary before
        //448070 asks the owning house for another live primary.
        self.production.clear_primary_factory(stable_id);
        crate::sim::production::initialize_factory_primary(self, context.rules(), stable_id);
        //440E76's virtual discovery precedes the successful BasePlan
        // tail at44159D..4415B3, including a reached first opening.
        self.fill_base_plan_from_successful_building_unlimbo(stable_id);
        Self::submit_entity_display(
            &self.interner,
            &self.terrain_objects,
            &mut self.substrate,
            &self.resolved_terrain,
            stable_id,
            context.rules,
            context.terrain(),
        );
        self.lifecycle_outputs
            .emit(LifecycleOutput::RevealDisplay { stable_id });
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::RevealDisplayBoundary);

        let logic_registered = if request.logic_eligible {
            self.register_logic_object(stable_id)
        } else {
            false
        };
        // Building4415D8: successful normal Unlimbo after opening/base-plan.
        // The early dead/failed paths do not reach this call.
        let rules = context.rules();
        if let Some((owner, category)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .and_then(|entity| {
                let object = self.object_type(entity.type_ref(), rules)?;
                Some((
                    entity.owner(),
                    crate::sim::production::factory_category_for_building(object)?,
                ))
            })
        {
            crate::sim::production::update_factory_queue(
                self,
                rules,
                owner,
                category,
                context.effects(),
            );
        }
        RevealOutcome::Revealed { logic_registered }
    }

    /// `BuildingClass::Unlimbo 0x00440B4F..0x00440B7A`, after the Techno
    /// Unlimbo and its alive gate: outside a campaign, a building of a house
    /// no human controls whose house type is not `MultiplayPassive=` becomes
    /// AI-repairable (`+0x6CB`), whatever its map line said.
    fn mark_ai_repairable_at_unlimbo(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        stable_id: u64,
    ) {
        let Some(entity) = substrate.entities.get(stable_id) else {
            return;
        };
        let game_mode_nonzero = session.game_mode_nonzero();
        if entity.category() == EntityCategory::Structure
            && game_mode_nonzero
            && houses.get(&entity.owner()).is_some_and(|house| {
                !house.is_controlled_by_human(game_mode_nonzero) && !house.multiplay_passive()
            })
            && let Some(entity) = substrate.entities.get_mut(stable_id)
        {
            entity.ai_sell_repair.make_repairable();
        }
    }

    /// Building440E5C..440E7C, after registration: owner50B730 alone
    /// admits the owner virtual+198 callback. This establishes41C for AI
    /// map buildings before a subsequent campaign reveal discovers them
    /// for PlayerPtr. Construction/radio are checked by the override.
    pub(crate) fn discover_building_owner_after_unlimbo(
        &mut self,
        id: u64,
        context: UninitContext<'_>,
    ) {
        let Some(owner) = self
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .map(|entity| entity.owner())
        else {
            return;
        };
        if self
            .houses
            .get(&owner)
            .is_some_and(|house| !house.is_controlled_by_human(self.session.game_mode_nonzero()))
        {
            self.discover_techno(id, owner, context);
        }
    }

    /// Shared TechnoDiscovered6F4960, called by CellAddContent47E9DC with
    /// PlayerPtr and FootUnlimbo4D722F with the object's owner. The queried
    /// House50B730 is the VIEWER; dirty flags and the1F4 notification belong
    /// to the object's owner at the time of the callback. The whole native
    /// capture comparison exercises this before the701735 owner swap.
    /// ObjectDiscovered5F5930 accepts a nonnull House without mutation.
    /// Live attached Tag event4 at6F4A18 enters the shared Tag dispatcher;
    /// the ordinary joined Engineer route carries NULL Tags. Missing House state
    /// is an unavailable headless boundary, not an invented native identity.
    fn record_techno_discovery(
        &mut self,
        stable_id: u64,
        viewer: InternedId,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        //6F4960..6F4972 compares the nonnull viewer against PlayerPtr;
        //there is no null-Player guard. A headless owner callback therefore
        //takes the ordinary other-house arm when no local player is bound.
        let current = Some(viewer) == self.session.current_house();
        let history = entity.discovery;
        if (current && history.discovered_by_current_house)
            || (!current && history.discovered_by_other_house)
        {
            return false;
        }
        let Some(house) = self.houses.get(&viewer) else {
            return false;
        };
        let controlled = house.is_controlled_by_human(self.session.game_mode_nonzero());
        let owner = entity.owner();
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("discovered Techno");
        if !current {
            // 6F4990 precedes base discovery and the mission inquiry.
            entity.discovery.discovered_by_other_house = true;
        }
        if !controlled
            && entity.mission.effective().known()
                == Some(crate::rules::mission_data::MissionType::Ambush)
        {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(
                    crate::rules::mission_data::MissionType::Hunt,
                ),
            );
        }
        if current {
            entity.discovery.discovered_by_current_house = true;
            //6F49DE invalidates both House bytes even for a Foot. Another
            //Building's repaired health can therefore become visible before
            //its next own health sample, through this independent writer.
            Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, true);
            if !history.owned_by_current_house {
                if !self.object_placement_scope_active() {
                    //6F4A03..6F4A18: the attachment owner resolves the Tag,
                    // including an Event48-only chain's no-match traversal.
                    self.dispatch_object_tag_event(stable_id, 4, Some(stable_id), None, context);
                }
            }
        }
        // Ares3.0's successful discovery sites6F4A1D/6F4A37 share1004E840.
        // The live owner is read after Tag callbacks, before the conditional
        //1F4 notification; repeated discovery returned before either site.
        let rules = context.rules();
        if crate::rules::ARES_ENABLED
            && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() != EntityCategory::Structure
                    && rules.is_ares_prerequisite_alternate(
                        entity.category().into(),
                        self.interner.resolve(entity.type_ref()),
                    ))
                .then_some(entity.owner())
            })
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        //6F4A1D reloads owner AFTER synchronous Tag actions.
        if current
            && !history.owned_by_current_house
            && let Some(owner) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| entity.owner())
            && let Some(house) = self.houses.get_mut(&owner)
        {
            house.notify_discovered_by_current_house();
        }
        true
    }

    /// The Techno virtual DiscoveredBy slot (+198): Foot uses6F4960;
    /// Building44D5D0 calls that same base before its own admission. A
    /// repeated base callback returns false before any Building tail.
    /// Original execution controls: trigger_action_values.py's discovery
    /// aggregate, including owner-entry/current/repeat and6E4 fast return.
    pub(crate) fn discover_techno(
        &mut self,
        id: u64,
        viewer: InternedId,
        context: UninitContext<'_>,
    ) -> bool {
        if !self.record_techno_discovery(id, viewer, context) {
            return false;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        if entity.category() != EntityCategory::Structure {
            return true;
        }
        let owner = entity.owner();
        let no_radio = entity.radio_contacts.is_empty();
        //44D5ED..44D60D records the owner's last BuildingType after shared
        //discovery admits, outside ScenarioInit, before Grand_Opening.
        //This shares House's field owner with Record_Last_Built4FB6B0 and
        //does not score a produced object. Executed: --build-type-events.
        if !self.object_placement_scope_active() {
            let type_id = self.interner.resolve(entity.type_ref());
            let type_index = context
                .rules()
                .building_type_index(type_id)
                .expect("a discovered building belongs to its Type registry");
            if let Some(house) = self.houses.get_mut(&owner) {
                house.record_built_type(
                    crate::rules::object_type::ObjectCategory::Building,
                    type_index,
                );
            }
        }
        let construction = Some(crate::rules::mission_data::MissionType::Construction);
        // Both arms ask Get_Mission (vt+0x184 = 0x005B3040: current, else
        // queued; the computer arm's at 0x0044D679), so a placed building
        // whose Unlimbo queued Construction stays shut until it is built.
        let effective_construction = entity.mission.effective().known() == construction;
        let queued_construction = entity.mission.queued().known() == construction;
        let controlled = self
            .houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero()));
        //44D632/44D664 query OWNER; shared6F4960 queries VIEWER.
        // A foreign viewer therefore does not open an AI building.
        // ScenarioInit affects the House notification writes,
        // independently of this Grand_Opening admission.
        let grand_admitted = if controlled {
            !effective_construction && !queued_construction
        } else {
            viewer == owner && !effective_construction
        };
        //44D61A precedes Grand_Opening, independent of ScenarioInit. The
        //owner's existing House tail sees this request from nested callbacks.
        self.houses.request_house_tech_tree_recheck(owner);
        if no_radio && grand_admitted {
            self.grand_opening(
                id,
                false,
                self.object_placement_scope_active(),
                context.rules(),
                context.registry(),
                context.effects(),
            );
        }
        //House246's 90-frame notification remains the separate timer residual
        //in ai_base_building; it is not consumed by the built-type predicate.
        true
    }

    /// CellAddContent47E953..47E9DC's discovery gate. In active YR the
    /// IsFogged5865E0 body always returns false. Any noncampaign CellPUT
    /// therefore calls DiscoveredBy(PlayerPtr); campaigns do so only when
    /// the height-projected Cell coordinate is unshrouded. Reuse the existing
    ///586360 and Fog authority instead of a presentation visibility cache.
    pub(crate) fn discover_cell_put_object(
        &mut self,
        id: u64,
        cell: (u16, u16),
        context: UninitContext<'_>,
    ) {
        let Some(viewer) = self.session.current_house() else {
            return;
        };
        if !self.session.game_mode_nonzero() {
            let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                return;
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            if crate::sim::vision::cell_is_shrouded(&self.fog, &cells, viewer, cell) {
                return;
            }
        }
        self.discover_techno(id, viewer, context);
    }

    /// `BuildingClass::Unlimbo @ 0x00440580` calls
    /// `FUN_0042F260 @ 0x0042F260` at `0x0044159D..0x004415B3` for
    /// successful non-human BasePlan satisfaction.
    fn fill_base_plan_from_successful_building_unlimbo(&mut self, stable_id: u64) {
        let Some((owner, type_index, packed_cell, has_undeploy_target)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() == EntityCategory::Structure
                    && entity.type_facts.base_plan_type_index() >= 0)
                    .then_some((
                        entity.owner(),
                        entity.type_facts.base_plan_type_index(),
                        crate::sim::base_plan::pack_base_plan_cell(
                            i32::from(entity.position.rx()),
                            i32::from(entity.position.ry()),
                        ),
                        entity.type_facts.base_plan_has_undeploy_target(),
                    ))
            })
        else {
            return;
        };
        let game_mode_nonzero = self.session.game_mode_nonzero();
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        if house.is_controlled_by_human(game_mode_nonzero) {
            return;
        }
        let _filled = house
            .base_plan_mut()
            .fill_successful_building(type_index, packed_cell, has_undeploy_target)
            .is_some();
        #[cfg(any(test, feature = "test-support"))]
        if _filled {
            self.trace_lifecycle_for_test(LifecycleTestEvent::BasePlanFilled { stable_id });
        }
    }

    /// `BuildingClass__Limbo @ 0x00445880` calls
    /// `FUN_0050A490 @ 0x0050A490` for BasePlan invalidation before the common
    /// Techno/Object concealment path. VERA has no map-editor runtime; every
    /// call to this lifecycle seam is therefore outside editor mode.
    fn invalidate_base_plan_from_building_limbo(
        session: &ScenarioSession,
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        stable_id: u64,
    ) {
        let Some((owner, type_index, packed_cell, is_base_defense, in_limbo)) =
            substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() == EntityCategory::Structure
                    && entity.type_facts.base_plan_type_index() >= 0)
                    .then_some((
                        entity.owner(),
                        entity.type_facts.base_plan_type_index(),
                        crate::sim::base_plan::pack_base_plan_cell(
                            i32::from(entity.position.rx()),
                            i32::from(entity.position.ry()),
                        ),
                        entity.type_facts.base_plan_is_defense(),
                        entity.lifecycle.in_limbo,
                    ))
            })
        else {
            return;
        };
        if in_limbo {
            return;
        }
        if let Some(house) = houses.get_mut(&owner) {
            house.base_plan_mut().invalidate_limbo_building(
                type_index,
                packed_cell,
                is_base_defense,
                session.game_mode_nonzero(),
            );
        }
    }

    /// Mark(PUT) through the object's vt+0x124: `FootClass::Mark`
    /// ([`Simulation::foot_mark_put`]) for Infantry, Unit and Aircraft, the
    /// building transaction below for a Structure.
    pub(crate) fn mark_entity_put(&mut self, stable_id: u64, context: UninitContext<'_>) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure {
            return self.foot_mark_put(
                stable_id,
                context.rules,
                context.registry(),
                context.effects(),
            );
        }
        if entity.lifecycle.cell_marked {
            return false;
        }
        entity.lifecycle.cell_marked = true;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(
            entity,
            context.terrain().or(self.resolved_terrain.as_ref()),
        );
        let insertion = CellListInsertion::from_category(entity.category());
        let foundation = entity.type_facts.foundation().to_owned();
        let hidden_profile = entity.type_facts.building_hidden_occupancy();
        let current_cell = (entity.position.rx(), entity.position.ry());
        let (width, height) = crate::rules::foundation::foundation_dimensions(&foundation);
        let mut intersections = Vec::with_capacity(usize::from(width) * usize::from(height));
        for dy in 0..height {
            for dx in 0..width {
                let Some(rx) = current_cell.0.checked_add(dx) else {
                    continue;
                };
                let Some(ry) = current_cell.1.checked_add(dy) else {
                    continue;
                };
                intersections.push((rx, ry));
            }
        }
        if let Some(smudge_grid) = self.smudge_grid.as_mut() {
            smudge_grid.clear_intersecting_footprints(&intersections);
        }
        Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);

        if let Some(layer) = layer {
            for &(rx, ry) in &cells {
                self.substrate
                    .occupancy
                    .add(rx, ry, stable_id, layer, None, insertion);
                self.discover_cell_put_object(stable_id, (rx, ry), context);
            }
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListLinked);
            if layer == crate::sim::movement::locomotor::MovementLayer::Ground
                && hidden_profile.is_some_and(|profile| {
                    self.substrate.hidden_occupation.enter_building(
                        current_cell,
                        &foundation,
                        profile,
                        Some((self.session.map_width(), self.session.map_height())),
                    )
                })
            {
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::HiddenOccupationEntered);
            }
            for &(rx, ry) in &cells {
                self.substrate
                    .raw_cell_occupation
                    .mark_ground(rx, ry, BUILDING_OCCUPATION_BIT);
            }
            #[cfg(any(test, feature = "test-support"))]
            if !cells.is_empty() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationMarked);
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::CellMarked);
        // Building Mark43F180 -> PlaceDown5683C0 publishes AddContent and
        // Cell Recalc47D2B0 before returning. Keep the derived path view at
        // that boundary too; the marked structures remain its authority.
        self.publish_recalculated_cells(context.rules(), &cells);
        self.substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.lifecycle.cell_marked)
    }

    /// IsCellInPlayfield (`0x00578460`, mode 1) of the Reveal position's
    /// origin Cell. Unbounded synthetic fixtures retain their historical
    /// permissive behavior. Two consumers:
    /// - an aircraft's Unlimbo Z ([`Self::unlimbo_z`]), as AircraftClass::
    ///   Unlimbo asks it through `0x005785F0` (`0x00414352`);
    /// - VERA's gate before Mark(PUT) for [`PlacementEvidence::EvaluateMark`]
    ///   outside the `0x00A8E7AC` scope.
    ///
    /// RESIDUAL (#2393): the gate has no native counterpart. No Unlimbo layer
    /// refuses on the playfield: Building (`0x00440580..0x004415E6`), Techno
    /// (`0x006F6CA0`) and Object (`0x005F4EC0`), nor the Foot layers (see
    /// [`PlacementEvidence::FootEntryAdmitted`]). With rules only a building
    /// reaches it, since a Foot's admission comes back converted. Trigger: a
    /// building revealed outside the scope whose origin Cell is outside the
    /// playfield. Effect: the placement fails here, where native's Unlimbo
    /// places it unless its caller refused first; those callers' own checks
    /// were not traced. Frequency not counted. Downstream, the caller's
    /// failure path runs instead of a placed building.
    fn reveal_position_is_in_playfield(
        &self,
        position: RevealPosition,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(bounds) = self.playfield_bounds else {
            return true;
        };
        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(position.rx()), i32::from(position.ry())),
            Some(bounds),
            context.terrain().or(self.resolved_terrain.as_ref()),
        )
    }

    pub(crate) fn base_reservation_house_index(&self, owner: InternedId) -> Option<i32> {
        // Active YR Full_Init constructs the complete HouseClass array before
        // Terrain/Techno map sections reveal objects. Scenario loading preserves
        // that order, so Reveal writes the final house-index bit immediately.
        if self.houses.is_empty() {
            return None;
        }
        let index = self
            .houses
            .index_of(owner)
            .and_then(|index| i32::try_from(index).ok());
        debug_assert!(
            index.is_some(),
            "base-reservation owner must be registered in the House array"
        );
        index
    }

    fn mark_building_base_reservation_with_arg(
        &mut self,
        stable_id: u64,
        repair_only: bool,
        context: UninitContext<'_>,
    ) -> bool {
        let Some((owner, rect)) = self.base_reservation_writer(stable_id) else {
            return false;
        };
        let Some(house_index) = self.base_reservation_house_index(owner) else {
            return false;
        };
        self.substrate.base_reservations.reserve_rect(
            context.terrain().or(self.resolved_terrain.as_ref()),
            rect,
            house_index,
        );
        if let Some(house) = self.houses.get_mut(&owner) {
            house
                .base_reservation_mut()
                .update_bounds(rect.x, rect.y, rect.width, rect.height);
        } else {
            debug_assert!(false, "base-reservation owner must have HouseState");
        }
        if !repair_only {
            Self::update_base_reservation_perimeter_after_mark(
                &self.substrate,
                &mut self.houses,
                &self.resolved_terrain,
                owner,
                house_index,
                rect,
                context,
            );
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BaseReservationMarked);
        true
    }

    fn update_base_reservation_perimeter_after_mark(
        substrate: &ObjectSubstrate,
        houses: &mut Houses,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        owner: InternedId,
        house_index: i32,
        rect: CellRect,
        context: UninitContext<'_>,
    ) {
        scan_cell_rect(base_reservation_perimeter_rect(rect), |x, y| {
            let neighbor_mask = substrate.base_reservations.house_reservation_neighbor_mask(
                context.terrain().or(resolved_terrain.as_ref()),
                x,
                y,
                house_index,
            );
            let packed = packed_reservation_coord(x, y);
            if let Some(house) = houses.get_mut(&owner) {
                match neighbor_mask {
                    0x0000_00ff => house.base_reservation_mut().remove_perimeter_cell(packed),
                    1..=0x0000_00fe => house
                        .base_reservation_mut()
                        .append_perimeter_cell_if_absent(packed),
                    0 | u32::MAX => {}
                    _ => unreachable!("neighbor mask is eight bits or the absent-center sentinel"),
                }
            }
            true
        });
    }

    fn update_base_reservation_perimeter_after_clear(
        substrate: &mut ObjectSubstrate,
        houses: &mut Houses,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        owner: InternedId,
        house_index: i32,
        rect: CellRect,
        context: UninitContext<'_>,
    ) {
        scan_cell_rect(base_reservation_perimeter_rect(rect), |x, y| {
            let neighbor_mask = substrate.base_reservations.house_reservation_neighbor_mask(
                context.terrain().or(resolved_terrain.as_ref()),
                x,
                y,
                house_index,
            );
            let packed = packed_reservation_coord(x, y);
            match neighbor_mask {
                0x0000_00ff => {
                    if let Some(house) = houses.get_mut(&owner) {
                        house.base_reservation_mut().remove_perimeter_cell(packed);
                    }
                }
                1..=0x0000_00fe => {
                    if let Some(house) = houses.get_mut(&owner) {
                        house
                            .base_reservation_mut()
                            .append_perimeter_cell_if_absent(packed);
                    }
                }
                0 | u32::MAX => {
                    if let Some(house) = houses.get_mut(&owner) {
                        house.base_reservation_mut().remove_perimeter_cell(packed);
                    }
                    // Native resolves the center again before the extra clear.
                    substrate.base_reservations.clear(
                        context.terrain().or(resolved_terrain.as_ref()),
                        x,
                        y,
                        house_index,
                    );
                }
                _ => unreachable!("neighbor mask is eight bits or the absent-center sentinel"),
            }
            true
        });
    }

    /// The rectangle of the base-reservation writer `0x00455F10`: Location's cell
    /// less `AIBaseSpacing=`, the foundation plus twice the spacing.
    fn base_reservation_writer(&self, stable_id: u64) -> Option<(InternedId, CellRect)> {
        let entity = self.substrate.entities.get(stable_id)?;
        let spacing = entity.type_facts.base_reservation_spacing()?;
        (entity.category() == EntityCategory::Structure
            && entity.lifecycle.object_alive
            && !entity.lifecycle.in_limbo
            && entity.lifecycle.cell_marked
            && cell_list_layer_for_entity(entity, self.resolved_terrain.as_ref())
                == Some(crate::sim::movement::locomotor::MovementLayer::Ground))
        .then(|| {
            (
                entity.owner(),
                building_base_reservation_rect(
                    entity.position.rx(),
                    entity.position.ry(),
                    entity.type_facts.foundation(),
                    spacing,
                ),
            )
        })
    }

    /// The base-reservation clear and neighbor repair `0x004561F0` that Building
    /// Limbo calls (`0x00445A9A`), over the same rectangle from Location's cell.
    fn clear_building_base_reservation_and_repair(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        let Some(spacing) = entity.type_facts.base_reservation_spacing() else {
            return false;
        };
        if entity.category() != EntityCategory::Structure
            || !entity.lifecycle.cell_marked
            || cell_list_layer_for_entity(
                entity,
                context.terrain().or(self.resolved_terrain.as_ref()),
            ) != Some(crate::sim::movement::locomotor::MovementLayer::Ground)
        {
            return false;
        }
        let owner = entity.owner();
        let rect = building_base_reservation_rect(
            entity.position.rx(),
            entity.position.ry(),
            entity.type_facts.foundation(),
            spacing,
        );
        let (foundation_width, foundation_height) =
            crate::rules::foundation::foundation_dimensions(entity.type_facts.foundation());
        let repair_rect = building_base_reservation_repair_rect(
            entity.position.rx(),
            entity.position.ry(),
            foundation_width,
            foundation_height,
            spacing,
        );
        let Some(house_index) = self.base_reservation_house_index(owner) else {
            return false;
        };

        self.substrate.base_reservations.clear_rect(
            context.terrain().or(self.resolved_terrain.as_ref()),
            rect,
            house_index,
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::BaseReservationCleared);

        // The native repair scan happens while this building is still linked in
        // the ground list. Each lookup selects only the first Building and calls
        // its repair-only writer immediately; identities are not deduplicated.
        scan_cell_rect(repair_rect, |x, y| {
            let neighbor_id = resolve_reservation_real_cell(
                context.terrain().or(self.resolved_terrain.as_ref()),
                x,
                y,
            )
            .and_then(|(rx, ry)| {
                self.substrate.occupancy.first_building_on_layer(
                    rx,
                    ry,
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                )
            });
            if let Some(neighbor_id) = neighbor_id
                && neighbor_id != stable_id
            {
                self.mark_building_base_reservation_with_arg(neighbor_id, true, context);
            }
            true
        });
        Self::update_base_reservation_perimeter_after_clear(
            &mut self.substrate,
            &mut self.houses,
            &self.resolved_terrain,
            owner,
            house_index,
            rect,
            context,
        );
        true
    }

    /// Mark(REMOVE) through the object's vt+0x124: `FootClass::Mark`
    /// ([`Simulation::foot_mark_remove`]) for Infantry, Unit and Aircraft, the
    /// building transaction below for a Structure.
    pub(crate) fn unmark_entity_remove(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if entity.category() != EntityCategory::Structure {
            return self.foot_mark_remove(
                stable_id,
                context.rules,
                context.registry(),
                context.effects(),
            );
        }
        if !entity.lifecycle.cell_marked {
            return false;
        }
        entity.lifecycle.cell_marked = false;
        let cells = entity_occupancy_cells(entity);
        let layer = cell_list_layer_for_entity(
            entity,
            context.terrain().or(self.resolved_terrain.as_ref()),
        );
        let foundation = entity.type_facts.foundation().to_owned();
        let hidden_profile = entity.type_facts.building_hidden_occupancy();
        let current_cell = (entity.position.rx(), entity.position.ry());
        if let Some(layer) = layer {
            for &(rx, ry) in &cells {
                self.substrate
                    .occupancy
                    .remove_on_layer(rx, ry, stable_id, layer);
            }
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationListUnlinked);
            if layer == crate::sim::movement::locomotor::MovementLayer::Ground
                && hidden_profile.is_some_and(|profile| {
                    self.substrate.hidden_occupation.exit_building(
                        current_cell,
                        &foundation,
                        profile,
                        Some((self.session.map_width(), self.session.map_height())),
                    )
                })
            {
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::HiddenOccupationExited);
            }
            for &(rx, ry) in &cells {
                self.substrate
                    .raw_cell_occupation
                    .clear_ground(rx, ry, BUILDING_OCCUPATION_BIT);
            }
            #[cfg(any(test, feature = "test-support"))]
            if !cells.is_empty() {
                self.trace_lifecycle_for_test(LifecycleTestEvent::RawOccupationCleared);
            }
        }
        // Building Mark43F180 -> PlaceUp5687F0 unlinks each Cell47EA90,
        // clears occupation453DC0 and recalculates47D2B0 before Conceal
        // resumes at5F4D73. Fatal Bullet receivers can remove Logic here,
        // so no later object/frame visit may be required to release the path.
        // Executed shared death path: tools/spatial_oracle/
        // building_death_anims_joined_stock_smudges.json (building_mark).
        // This projects current marked owners, retaining overlapping or
        // still-marked dying structures; it adds no RNG/timer/detach work.
        self.publish_recalculated_cells(context.rules(), &cells);
        true
    }

    /// ObjectClass::DropAsBomb5F4160: latch the fall before removing the
    /// old layer, clear OnBridge, then re-enter the ground list. Location,
    /// FallRate, parachute byte and locomotor order remain authoritative.
    /// Native multi-cell content hooks differ from full teardown Mark:
    /// neither building smudges nor unrelated track reservations are retired.
    /// Residual: hidden-building content callbacks beyond this relayer.
    pub(crate) fn drop_as_bomb(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let rules = context.rules;
        use crate::sim::movement::locomotor::MovementLayer;
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let cells = entity_occupancy_cells(entity);
        let sub_cell = entity.sub_cell();
        let insertion = CellListInsertion::from_category(entity.category());
        let old_layer = cell_list_layer_for_entity(entity, self.resolved_terrain.as_ref());
        let world_z = crate::sim::movement::ground_pose::object_world_z_leptons(
            entity,
            self.resolved_terrain.as_ref(),
        );
        self.substrate
            .entities
            .get_mut(stable_id)
            .unwrap()
            .begin_bomb_fall();
        let foot = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                )
            });
        if foot {
            self.foot_mark_remove(stable_id, rules, context.registry, context.effects);
        } else {
            for &(rx, ry) in &cells {
                self.substrate.occupancy.remove_on_layer(
                    rx,
                    ry,
                    stable_id,
                    old_layer.unwrap_or(MovementLayer::Ground),
                );
            }
        }
        self.substrate.display.remove(stable_id);
        let entity = self
            .substrate
            .entities
            .get_mut(stable_id)
            .expect("member remains represented");
        entity.on_bridge.store(false);
        entity.position.put_z(world_z);
        Self::submit_entity_display(
            &self.interner,
            &self.terrain_objects,
            &mut self.substrate,
            &self.resolved_terrain,
            stable_id,
            rules,
            None,
        );
        if foot {
            self.foot_mark_put(stable_id, rules, context.registry, context.effects);
        } else {
            for &(rx, ry) in &cells {
                self.substrate.occupancy.add(
                    rx,
                    ry,
                    stable_id,
                    MovementLayer::Ground,
                    sub_cell,
                    insertion,
                );
            }
        }
        //5F41D0..5F4218: Foot's active Is_Moving chooses the shared
        // locomotor occupation release or the owner's raw Location receiver.
        if let Some(entity) = self.substrate.entities.get(stable_id)
            && matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
            && entity.locomotor.is_some()
        {
            let moving = crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false);
            let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            if moving {
                self.locomotor_mark_all_occupation_bits(stable_id, false);
            } else {
                Self::object_raw_receiver_at(
                    &mut self.substrate,
                    &self.path_grid,
                    &self.resolved_terrain,
                    stable_id,
                    coord,
                    false,
                );
            }
        }
    }

    /// Fixture boundary: puts a fixture object on the map and marks it
    /// (idempotent). Fixtures are constructed in Limbo, which Mark refuses
    /// (`0x005F5854`), so this clears InLimbo first.
    #[cfg(any(test, feature = "test-support"))]
    pub fn add_entity_occupancy(&mut self, stable_id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = false;
        }
        let _ = self.mark_entity_put(stable_id, UninitContext::default());
    }

    /// Fixture boundary; common lifecycle code calls the private unmark
    /// transaction instead.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn remove_entity_occupancy(&mut self, stable_id: u64) {
        self.unmark_entity_remove(stable_id, UninitContext::default());
    }

    /// One air locomotor's Process visit: a Fly's (`0x004CCB40`,
    /// [`Self::fly_process`]) or a Jumpjet's (`world::jumpjet_cruise`). Each
    /// keeps its own AircraftTracker membership (Add `0x004134A0`, Remove
    /// `0x004135D0`, Update `0x004138C0`), and `FootClass::Limbo` removes a
    /// tracked object ([`ObjectSubstrate::release_foot_air_tracker_before_limbo`](crate::sim::world::ObjectSubstrate::release_foot_air_tracker_before_limbo)).
    pub(crate) fn tick_air_movement_with_cell_lists_one(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<crate::sim::movement::air_movement::AirMovementTickStats, super::FrameAdvanceError>
    {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::MovementLayer;

        let fly = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.lifecycle.object_alive
                    && !entity.lifecycle.in_limbo
                    && entity.locomotor.as_ref().is_some_and(|locomotor| {
                        locomotor.active_kind() == LocomotorKind::Fly
                            && locomotor.layer() == MovementLayer::Air
                    })
            });
        if fly {
            return Ok(self.fly_process(stable_id, rules, registry, effects));
        }
        let jumpjet_layer_before = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| {
                entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.active_kind() == LocomotorKind::Jumpjet)
            })
            .and_then(|_| self.entity_display_layer(stable_id, rules));
        // A cruising Jumpjet runs the native Update/State3 body
        // (`world::jumpjet_cruise`).
        let stats = match self.tick_jumpjet_cruise_one(stable_id, rules, registry, effects)? {
            // State 5's impact notice UnInits the wreck, so `Process`'s layer
            // tail finds it dead (`0x0054B16C`); the object turn commits it.
            Some(stats) if stats.impact => return Ok(stats),
            Some(stats) => stats,
            None => crate::sim::movement::air_movement::AirMovementTickStats::default(),
        };
        if let Some(before) = jumpjet_layer_before {
            Self::complete_jumpjet_display_process(
                &self.interner,
                &self.terrain_objects,
                &mut self.substrate,
                &self.resolved_terrain,
                stable_id,
                before,
                rules,
            );
        }
        Ok(stats)
    }

    /// Fly4CCB40 ->4CD2A0 after4CD600's movement/height Mark pair.
    /// Non-Landable aircraft return before the phase's OWN Mark/Display pair.
    /// Landing runs first, then rechecks takeoff; both resubmit on equal layers.
    /// Return whether the phase performed the Display transaction.
    pub(super) fn complete_fly_phase(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let admitted = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.lifecycle.object_alive
                && entity.health.current() > 0
                && entity
                    .locomotor
                    .as_ref()
                    .is_some_and(|l| l.is_powered() && l.fly_runtime().is_some())
        });
        if !admitted {
            return false;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let non_landable_level = (entity.category() == EntityCategory::Aircraft)
            .then(|| {
                self.interner
                    .object_type(entity.type_ref(), rules)
                    .filter(|object| !object.landable)
                    .map(|object| object.flight_level(rules.general.flight_level))
            })
            .flatten();
        if let Some(flight_level) = non_landable_level {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap()
                .fly_runtime_mut()
                .unwrap()
                .force_non_landable_flight(flight_level);
            return false;
        }
        if !entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .has_phase_callback()
        {
            return false;
        }
        let before = self.entity_display_layer(id, rules);
        self.foot_mark_remove(id, rules, registry, effects);
        self.substrate.display.remove(id);
        if self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.fly_runtime())
            .is_some_and(|s| s.landing())
        {
            self.apply_fly_landing_callback(id, rules, registry, effects);
        }
        if self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.fly_runtime())
            .is_some_and(|s| s.taking_off())
        {
            Self::apply_fly_takeoff_callback(
                &self.interner,
                &self.session,
                &mut self.substrate,
                &self.resolved_terrain,
                id,
                rules,
            );
        }
        let after = self.entity_display_layer(id, rules);
        if before != after {
            // `0x004CD3B6` asks about the cell under the aircraft (MapAtCoord).
            let coord = crate::sim::movement::ground_pose::position_world_coord(
                &self.substrate.entities.get(id).unwrap().position,
            );
            let below = NavTargetRef::cell((coord.x / 256) as u16, (coord.y / 256) as u16);
            if after == Some(super::display_layers::DisplayLayer::GROUND)
                && !self
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|e| e.locomotor.as_ref())
                    .and_then(|l| l.fly_runtime())
                    .is_some_and(|s| s.taking_off())
                && !self.foot_land_zone_clear(id, below, rules)
            {
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .fly_runtime_mut()
                        .unwrap()
                        .reject_landing_cell();
                    entity.on_bridge.store(false);
                }
                self.foot_mark_remove(id, rules, registry, effects);
                let height = crate::sim::movement::air_movement::current_fly_height(
                    self.substrate.entities.get(id).unwrap(),
                    self.resolved_terrain.as_ref(),
                );
                self.set_object_height(id, height.wrapping_add(10), rules, registry, effects);
                self.foot_mark_put(id, rules, registry, effects);
            } else {
                self.finish_fly_layer_transition(id, after, rules, effects);
            }
        }
        // Display4A9720 has no limbo gate, so the tail resubmits even an owner
        // the landing retry's C4 receiver just destroyed; store removal expires
        // that registration. Object Mark5F5850 refuses a Limbo owner, so the
        // cell lists never regain it.
        Self::submit_entity_display(
            &self.interner,
            &self.terrain_objects,
            &mut self.substrate,
            &self.resolved_terrain,
            id,
            rules,
            None,
        );
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| !e.lifecycle.in_limbo)
        {
            self.foot_mark_put(id, rules, registry, effects);
        }
        true
    }

    /// Admitted callback4CE680. The phase caller owns Mark/Display sequencing;
    /// the callback owns flag clearing and the two existing facing controllers.
    pub(super) fn apply_fly_takeoff_callback(
        interner: &StringInterner,
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
        rules: &RuleSet,
    ) {
        use crate::sim::movement::{air_movement, fly_height::TakeoffFacing, ground_pose};
        let entity = substrate.entities.get(id).expect("admitted Fly callback");
        let xy = ground_pose::position_world_xy(&entity.position);
        let mut height = air_movement::current_fly_height(entity, resolved_terrain.as_ref());
        //4CE696..4CE6DF: query structure even below416; bridge-normalize only
        // when not already OnBridge and high enough above a structural deck.
        if !entity.on_bridge() {
            let bridge = resolved_terrain.as_ref().is_some_and(|terrain| {
                let cell =
                    terrain.native_cell_identity(((xy[0] / 256) as i16, (xy[1] / 256) as i16));
                terrain.native_cell_flags(cell) & 0x100 != 0
            });
            if bridge && height >= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS {
                height = height.wrapping_sub(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
            }
        }
        let landing_base = crate::sim::aircraft::landing_base::landing_base(
            entity,
            &substrate.entities,
            Some((rules, interner)),
        );
        let entity = substrate
            .entities
            .get_mut(id)
            .expect("admitted Fly callback");
        entity.ensure_fly_secondary_facing();
        let state = entity
            .locomotor
            .as_mut()
            .unwrap()
            .fly_runtime_mut()
            .unwrap();
        let destination = state.destination();
        match state.complete_takeoff(height, landing_base) {
            TakeoffFacing::Unchanged => {}
            TakeoffFacing::SecondaryToPrimaryDestination => {
                let desired = entity.body_facing.destination();
                entity
                    .barrel_facing
                    .as_mut()
                    .unwrap()
                    .set(desired, session.binary_frame());
            }
            TakeoffFacing::PrimaryToDestination => {
                // Original also evaluates the zero delta; do not special-case
                // it to the current heading. The shared native table owns it.
                let desired = crate::util::direction_tables::facing16_from_delta(
                    destination.x.wrapping_sub(xy[0]),
                    destination.y.wrapping_sub(xy[1]),
                );
                entity.body_facing.set(desired, session.binary_frame());
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .fly_runtime_mut()
                    .unwrap()
                    .request_full_speed();
            }
        }
    }

    /// The kind of the object `stable_id` names, or `None` when no store
    /// holds it. Object IDs are unique across stores, so the probe order
    /// ([`ObjectKind::ALL`]) is only a cost: entities first, as most Logic
    /// slots are, and particles last, as finding one walks the systems.
    pub(crate) fn classify_object(&self, stable_id: u64) -> Option<ObjectKind> {
        ObjectKind::ALL
            .into_iter()
            .find(|&kind| self.object_store_holds(kind, stable_id))
    }

    /// Whether `kind`'s store holds `stable_id`.
    fn object_store_holds(&self, kind: ObjectKind, stable_id: u64) -> bool {
        match kind {
            ObjectKind::Entity => self.substrate.entities.contains(stable_id),
            ObjectKind::Anim => self.substrate.anims.contains_key(stable_id),
            ObjectKind::VoxelAnim => self.substrate.voxel_anims.contains_key(stable_id),
            ObjectKind::Spotlight => self.substrate.spotlights.contains_key(stable_id),
            ObjectKind::ParticleSystem => self.substrate.particle_systems.contains_key(stable_id),
            ObjectKind::Terrain => self.terrain_objects.objects().contains_key(&stable_id),
            ObjectKind::Projectile => self.projectiles.get(stable_id).is_some(),
            ObjectKind::Wave => self.waves.get(stable_id).is_some(),
            ObjectKind::Smudge => self
                .smudge_grid
                .as_ref()
                .and_then(|grid| grid.object(stable_id))
                .is_some(),
            ObjectKind::Particle => self
                .substrate
                .particle_systems
                .particle(stable_id)
                .is_some(),
        }
    }

    /// Read the per-object `in_logic_vector` membership flag for a classified
    /// object. The flag lives on the object in its own store; this is the
    /// single dispatch the registration/removal contract reads through.
    /// For a Techno, `Object+0x98`: RegisterObject sets it (`0x0055BAC6`).
    pub(super) fn logic_membership_flag(&self, stable_id: u64, kind: ObjectKind) -> bool {
        match kind {
            ObjectKind::Spotlight => self
                .substrate
                .spotlights
                .get(stable_id)
                .is_some_and(|l| l.in_logic_vector()),
            ObjectKind::Smudge | ObjectKind::Particle => false,
            ObjectKind::Anim => self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| anim.in_logic_vector()),
            ObjectKind::VoxelAnim => self
                .substrate
                .voxel_anims
                .get(stable_id)
                .is_some_and(|debris| debris.in_logic_vector()),
            ObjectKind::ParticleSystem => self
                .substrate
                .particle_systems
                .get(stable_id)
                .is_some_and(|system| system.in_logic_vector()),
            ObjectKind::Terrain => self
                .terrain_objects
                .objects()
                .get(&stable_id)
                .is_some_and(|terrain| terrain.in_logic_vector()),
            ObjectKind::Projectile => self
                .projectiles
                .get(stable_id)
                .is_some_and(|projectile| projectile.in_logic_vector()),
            ObjectKind::Wave => self
                .waves
                .get(stable_id)
                .is_some_and(|wave| wave.in_logic_vector()),
            ObjectKind::Entity => self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.in_logic_vector.is_member()),
        }
    }

    /// Write the per-object `in_logic_vector` membership flag for a classified
    /// object. The single dispatch the registration/removal contract repairs
    /// the flag through. For a Techno it is RegisterObject's set (`0x0055BAC6`)
    /// or UnregisterObject's clear (`0x0055BB27`).
    pub(super) fn set_logic_membership_flag(
        terrain_objects: &mut TerrainObjectStore,
        substrate: &mut ObjectSubstrate,
        projectiles: &mut ProjectileStore,
        waves: &mut WaveStore,
        stable_id: u64,
        kind: ObjectKind,
        member: bool,
    ) {
        match kind {
            ObjectKind::Spotlight => {
                if let Some(l) = substrate.spotlights.get_mut(stable_id) {
                    l.mark_logic_membership(member);
                }
            }
            ObjectKind::Smudge | ObjectKind::Particle => {
                assert!(!member, "{kind:?} {stable_id} never joins Logic");
            }
            ObjectKind::Anim => {
                if let Some(anim) = substrate.anims.get_mut(stable_id) {
                    anim.mark_logic_membership(member);
                }
            }
            ObjectKind::VoxelAnim => {
                if let Some(debris) = substrate.voxel_anims.get_mut(stable_id) {
                    debris.mark_logic_membership(member);
                }
            }
            ObjectKind::ParticleSystem => {
                if let Some(system) = substrate.particle_systems.get_mut(stable_id) {
                    system.mark_logic_membership(member);
                }
            }
            ObjectKind::Terrain => {
                terrain_objects.set_logic_membership(stable_id, member);
            }
            ObjectKind::Projectile => {
                if let Some(projectile) = projectiles.get_mut(stable_id) {
                    projectile.mark_logic_membership(member);
                }
            }
            ObjectKind::Wave => {
                if let Some(wave) = waves.get_mut(stable_id) {
                    wave.mark_logic_membership(member);
                }
            }
            ObjectKind::Entity => {
                if let Some(entity) = substrate.entities.get_mut(stable_id) {
                    entity.in_logic_vector.mark(member);
                }
            }
        }
    }

    /// gamemd-derived: active YR `LogicClass__RegisterObject @ 0x0055BAA0`
    /// gates on the object's membership flag, appends at the live tail, then
    /// sets the flag only after insertion succeeds.
    fn register_logic_object(&mut self, stable_id: u64) -> bool {
        let kind = self.classify_object(stable_id);
        if let Some(kind) = kind
            && self.logic_membership_flag(stable_id, kind)
        {
            return true;
        }
        let Some(kind) = kind else {
            return false;
        };
        if !kind.joins_logic() {
            return false;
        }
        if self.substrate.logic.try_push(stable_id).is_err() {
            return false;
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::LogicAppended);
        Self::set_logic_membership_flag(
            &mut self.terrain_objects,
            &mut self.substrate,
            &mut self.projectiles,
            &mut self.waves,
            stable_id,
            kind,
            true,
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::LogicMembershipSet);
        true
    }

    /// gamemd-derived: active YR `LogicClass__UnregisterObject @ 0x0055BAE0`
    /// gates removal on the object's membership flag, performs the first-match
    /// stable erase, then repairs that flag.
    fn unregister_logic_object(&mut self, stable_id: u64) -> bool {
        let Some(kind) = self.classify_object(stable_id) else {
            return false;
        };
        if !self.logic_membership_flag(stable_id, kind) {
            return false;
        }
        let _ = self.substrate.logic.remove_first(stable_id);
        Self::set_logic_membership_flag(
            &mut self.terrain_objects,
            &mut self.substrate,
            &mut self.projectiles,
            &mut self.waves,
            stable_id,
            kind,
            false,
        );
        true
    }

    /// Test-only access to the exact LogicVector helper ordering. Returns the
    /// dispatch outcome so order/gate tests can assert it directly.
    #[cfg(any(test, feature = "test-support"))]
    pub fn register_live_object(&mut self, stable_id: u64) -> bool {
        self.register_logic_object(stable_id)
    }

    /// Test-only access to the exact LogicVector helper ordering. Returns the
    /// dispatch outcome so order/gate tests can assert it directly.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn unregister_live_object(&mut self, stable_id: u64) -> bool {
        self.unregister_logic_object(stable_id)
    }

    /// Reveal, leaving out the Display submit when `submit` is false: the
    /// constructor's attach submits the Anim before anything reads Display
    /// (`spawn_anim_at_world_with_constructor`).
    pub(crate) fn reveal_anim(&mut self, stable_id: u64, rules: &RuleSet, submit: bool) -> bool {
        // A physically retained Destroy/UnInit object has already left Logic
        // and Display. Its independent19B byte can still be zero, so it cannot
        // be resurrected through Reveal before the common deferred drain.
        if self.substrate.pending_delete.contains_object(&stable_id)
            || !self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| !anim.runtime().inactive)
        {
            return false;
        }
        self.substrate.mark_anim_display(stable_id, true);
        if submit {
            self.submit_anim_display(stable_id, rules);
        }
        self.register_logic_object(stable_id)
    }

    pub(crate) fn conceal_anim(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.substrate.mark_anim_display(stable_id, false);
        self.unregister_logic_object(stable_id)
    }

    /// The BuildingLight constructor435820 explicitly joins the shared Logic owner.
    pub(super) fn register_spotlight_logic(&mut self, id: u64) {
        self.register_logic_object(id);
    }
    pub(super) fn conceal_spotlight_logic(&mut self, id: u64) {
        self.substrate.display.remove(id);
        self.unregister_logic_object(id);
    }

    /// `ObjectClass::Unlimbo` inside `VoxelAnimClass::Constructor @ 0x007493B0`
    /// reveals the piece, which is what puts it in the LogicClass vector.
    pub(crate) fn reveal_voxel_anim(&mut self, stable_id: u64) -> bool {
        if !self.substrate.voxel_anims.contains_key(stable_id) {
            return false;
        }
        // VoxelAnim VT7F6318+78 ->74A960: always Air.
        Self::submit_overhead_display(&mut self.substrate, stable_id, DisplayLayer::AIR);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn reveal_particle_system(&mut self, stable_id: u64, rules: &RuleSet) -> bool {
        if !self.substrate.particle_systems.contains_key(stable_id) {
            return false;
        }
        // ParticleSystem VT7EFB9C+78 ->62FE80: always Ground. Rules resolve
        // the existing Building peers' GetYSort adjustments during insertion.
        Self::submit_object_display(
            &self.interner,
            &self.terrain_objects,
            &mut self.substrate,
            stable_id,
            DisplayLayer::GROUND,
            rules,
        );
        self.register_logic_object(stable_id)
    }

    pub(crate) fn conceal_particle_system(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.unregister_logic_object(stable_id)
    }

    /// Particle62BB0E calls ObjectUnlimbo5F4EC0; GetLayer62D770 returns
    /// AIR (3), so Display4A9720 reaches the existing append owner5519B0.
    /// A particle is owned by its system and never joins global Logic.
    pub(crate) fn reveal_particle(substrate: &mut ObjectSubstrate, stable_id: u64) {
        Self::submit_overhead_display(substrate, stable_id, DisplayLayer::AIR);
    }

    /// Spark62ED2C (and the other System AI cleanup callers) invokes the
    /// Particle's inherited ObjectUnInit5F65F0. Transfer the unique body
    /// first, then use that common lifecycle entry; it owns expiry, Limbo,
    /// Alive and the one B0F698 queue. Physical destruction waits for725C70.
    /// Source and executed queue controls: procedural_drawing_oracle/welding.
    pub(crate) fn retire_particle(
        &mut self,
        particle: crate::sim::particles::Particle,
        rules: &RuleSet,
    ) {
        let id = self
            .substrate
            .particle_systems
            .retain_detached_particle(particle);
        self.uninit_with_context(id, UninitContext::with_rules(rules));
    }

    pub(crate) fn register_terrain_object(&mut self, stable_id: u64, rules: &RuleSet) -> bool {
        if !self
            .terrain_objects
            .objects()
            .get(&stable_id)
            .is_some_and(|terrain| terrain.is_live())
        {
            return false;
        }
        // Terrain ctor71BC76 reveals at the retained type-adjusted coordinate;
        // Object5F4260 selects Ground even on elevated cells.
        Self::submit_object_display(
            &self.interner,
            &self.terrain_objects,
            &mut self.substrate,
            stable_id,
            DisplayLayer::GROUND,
            rules,
        );
        self.register_logic_object(stable_id)
    }

    pub fn register_projectile(&mut self, stable_id: u64, flat: bool) -> bool {
        if self.projectiles.get(stable_id).is_none() {
            return false;
        }
        // Bullet Fire468B6D ->Submit; GetLayer468B90 reads type+2F7.
        let layer = if flat {
            DisplayLayer::SURFACE
        } else {
            DisplayLayer::AIR
        };
        Self::submit_overhead_display(&mut self.substrate, stable_id, layer);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn register_wave(&mut self, stable_id: u64) -> bool {
        if self.waves.get(stable_id).is_none() {
            return false;
        }
        // Wave VT7F6BF4+78 ->75F890: always Air; Submit at75F952.
        Self::submit_overhead_display(&mut self.substrate, stable_id, DisplayLayer::AIR);
        self.register_logic_object(stable_id)
    }

    pub(crate) fn unregister_non_entity_object(&mut self, stable_id: u64) -> bool {
        self.substrate.display.remove(stable_id);
        self.unregister_logic_object(stable_id)
    }

    /// Terminal Terrain/Bullet/Wave objects leave Logic immediately but retain
    /// their physical store identity until the common late delete drain.
    ///
    /// gamemd-derived: active YR `DrainDeferredFinalizationQueue @ 0x00725C70`
    /// performs scalar destruction/freeing only after the frame commit.
    pub fn retire_non_entity_object(&mut self, stable_id: u64, context: UninitContext<'_>) -> bool {
        let represented = matches!(
            self.classify_object(stable_id),
            Some(ObjectKind::Terrain | ObjectKind::Projectile | ObjectKind::Wave)
        );
        if !represented {
            return false;
        }
        if self.classify_object(stable_id) == Some(ObjectKind::Projectile) {
            // `ObjectClass::UnInit @ 0x005F65F0` broadcasts the bullet's
            // expiry before it leaves Logic; an anim riding it ends
            // (`AnimClass::PointerExpired @ 0x00425150`, `+0x17C`).
            self.notify_pointer_expired(stable_id, context);
            // ObjectLimbo5F4D61 repeats DetachAll before removing the fired
            // Bullet's membership. The later scalar destructor repeats once.
            self.notify_pointer_expired(stable_id, context);
        }
        // The same UnInit's Limbo (vt+0xD4) detaches a dying tree's custom
        // sound (`object_sound`).
        self.detach_custom_sound(stable_id);
        let _ = self.unregister_non_entity_object(stable_id);
        self.substrate.pending_delete.queue_object(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
        true
    }

    /// Ordinary Terrain71B920 lethal tail: DetachAll, ObjectUnInit and
    /// ObjectLimbo each announce expiry while the retained object is still
    /// readable and marked. Only then may TerrainLimbo unlink its cell.
    pub(crate) fn terrain_lethal_uninit(
        &mut self,
        lethal: crate::sim::terrain_object::TerrainLethalDamage,
        finalizing: &mut std::collections::BTreeSet<u64>,
        navigation_changed_cells: &mut Vec<(u16, u16)>,
        context: UninitContext<'_>,
    ) -> bool {
        // 71BB69->5F5311, 71BB73->5F6616, then 5F4D61. Repeated
        // notifications do not rematch a Bullet already repaired to Cell.
        for _ in 0..3 {
            self.notify_pointer_expired(lethal.stable_id, context);
        }
        let finalized = self.terrain_objects.finalize_lethal(
            &mut self.substrate.raw_cell_occupation,
            finalizing,
            navigation_changed_cells,
            lethal,
            self.resolved_terrain.as_mut(),
        );
        if finalized {
            let retired = self.retire_non_entity_object(lethal.stable_id, context);
            debug_assert!(retired);
        }
        finalized
    }

    /// COM Release46AFF0 -> scalar Bullet destructor46B5C0 for a temporary
    /// never-Fired Bullet. Its Limbo early-return performs no registration
    /// changes; destruction still uses the common observer/cleanup owner.
    pub(crate) fn destroy_unfired_projectile(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(bullet) = self.projectiles.get(id) else {
            return;
        };
        assert!(
            !bullet.in_logic_vector() && self.substrate.display.layer_of(id).is_none(),
            "temporary Bullet {id} must not be Fired before direct destruction"
        );
        self.substrate
            .pending_delete
            .remove(DeferredDeleteTicket::Object(id));
        self.finalize_and_remove_common(
            id,
            UninitContext::new(rules, registry).with_effects(effects),
            1,
        );
    }

    /// Open-topped cargo entry hides the passenger but then directly restores
    /// its active membership. This is deliberately not Reveal: the passenger
    /// remains limbo/unmarked while its AI stays in the live object order.
    pub(crate) fn register_open_topped_passenger(&mut self, stable_id: u64) -> bool {
        if !self.substrate.entities.contains(stable_id) {
            return false;
        }
        self.register_logic_object(stable_id)
    }

    /// Compatibility dispatch which keeps AnimClass logic-only and routes every
    /// GameEntity through the complete common Object Conceal transaction.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conceal(&mut self, stable_id: u64) -> ConcealOutcome {
        if self.substrate.anims.contains_key(stable_id) {
            return if self.conceal_anim(stable_id) {
                ConcealOutcome::Concealed
            } else {
                ConcealOutcome::AlreadyConcealed
            };
        }
        if self.substrate.particle_systems.contains_key(stable_id) {
            return if self.conceal_particle_system(stable_id) {
                ConcealOutcome::Concealed
            } else {
                ConcealOutcome::AlreadyConcealed
            };
        }
        self.object_conceal(stable_id)
    }

    /// ObjectClass::Conceal represented order. Conceal does not mutate Alive.
    #[cfg(any(test, feature = "test-support"))]
    pub fn object_conceal(&mut self, stable_id: u64) -> ConcealOutcome {
        self.object_conceal_with_context(stable_id, UninitContext::default())
    }

    /// BuildingDetach44EE7F/44EEC8 temporarily excludes only this building
    /// while the held type queries FindFactory and HouseAbandon runs. Restore
    /// the old byte after synchronous callbacks; no stored producer binding.
    pub(crate) fn with_factory_building_in_limbo<R>(
        &mut self,
        stable_id: u64,
        callback: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prior = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.lifecycle.in_limbo);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = true;
        }
        let result = callback(self);
        if let Some(prior) = prior
            && let Some(entity) = self.substrate.entities.get_mut(stable_id)
        {
            entity.lifecycle.in_limbo = prior;
        }
        result
    }

    fn object_conceal_with_context(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> ConcealOutcome {
        let Some((in_limbo, object_alive)) = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| (entity.lifecycle.in_limbo, entity.lifecycle.object_alive))
            .or_else(|| {
                self.substrate
                    .particle_systems
                    .detached_particle_lifecycle(stable_id)
                    .map(|(alive, in_limbo)| (in_limbo, alive))
            })
        else {
            return ConcealOutcome::MissingOrDead;
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = object_alive;
        // gamemd-derived: `ObjectClass::Conceal @ 0x005F4D30` tests InLimbo
        // at `0x005F4D45` and returns before Destroy on the already-limbo path.
        if in_limbo {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealAlreadyLimboReturn {
                stable_id,
                object_alive,
                resolvable: true,
            });
            return ConcealOutcome::AlreadyConcealed;
        }

        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.selection.deselect();
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDeselected);

        // gamemd-derived: active YR `ObjectClass::Conceal @ 0x005F4D30`
        // enters `ObjectClass::Detach_All(1) @ 0x005F5280` after deselection and
        // before `Mark(REMOVE)`, so the expiry broadcast observes the target
        // alive, resolvable, and still cell-marked.
        #[cfg(any(test, feature = "test-support"))]
        {
            let (object_alive, cell_marked) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|entity| (entity.lifecycle.object_alive, entity.lifecycle.cell_marked))
                .or_else(|| {
                    self.substrate
                        .particle_systems
                        .detached_particle_lifecycle(stable_id)
                        .map(|(alive, _)| (alive, false))
                })
                .unwrap_or((false, false));
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDestroyNotifyBoundary {
                stable_id,
                object_alive,
                cell_marked,
                resolvable: true,
            });
        }
        // BuildingDetach44EBF0 completes both production arms before the
        // common Object expiry broadcast (44EC01..44EF24).
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            crate::sim::production::detach_building_production(
                self,
                context.rules(),
                stable_id,
                context.effects(),
            );
            //44EED0 repeats OVER_OUT after production, independently of
            //RadioLimbo65AA91's earlier contact pass. Repeated Conceal
            //returns before this BuildingDetach arm.
            crate::sim::radio::broadcast_break(self, stable_id, context.rules(), context.effects());
        }
        // The Foot prelude (`0x004D9744`) leaves the object's team, before
        // Limbo is set, so the object still takes its idle mode.
        self.leave_team(stable_id, false, context.rules(), context.effects());
        // This Detach_All(1) (`0x005F4D61`) also visits the concealed object
        // itself, so a live spawner's Limbo runs its SpawnManager's owner arm:
        // docked children UnInit with a zero regen timer, and airborne ones
        // turn kamikaze on the wing's target (`0x006B7100`).
        self.notify_pointer_expired(stable_id, context);

        if self.unmark_entity_remove(stable_id, context) {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealUnmarked);
        }

        self.substrate.display.remove(stable_id);
        // The app consumes this projection to refresh entity/Anim atlases.
        // Particles draw from the retained Display registry directly; their
        // removal cannot introduce atlas entries and must not trigger a
        // whole-world atlas coverage scan for every child retirement.
        if self.substrate.entities.contains(stable_id) {
            self.lifecycle_outputs
                .emit(LifecycleOutput::DisplayRemove { stable_id });
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDisplayBoundary);
        self.lifecycle_outputs
            .emit(LifecycleOutput::DetachAttachedAnims { stable_id });
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealAnimBoundary);
        // ObjectConceal5F4D81 hard-stops the AmbientSound handle (+3C) and
        //5F4D89 detaches the custom-sound handle (+50): `object_sound`.
        // They are separate from Foot+544/Anim+1A0/Building+6A0.
        self.object_sound_conceal(stable_id, context.rules());
        context.effects().flush_sound(self, context.rules());
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealVocBoundary);

        if self.unregister_logic_object(stable_id) {
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealLogicRemoved);
        }

        let dirty_rect_eligible = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.removal_facts.dirty_rect_eligible());
        if dirty_rect_eligible {
            self.lifecycle_outputs
                .emit(LifecycleOutput::DirtyTacticalRect { stable_id });
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealDirtyTacticalRectBoundary);
        }
        self.lifecycle_outputs
            .emit(LifecycleOutput::ClearDrawnState { stable_id });
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealClearDrawnStateBoundary);

        // ObjectConceal5F4E98 invokes vt+0x11C here, before +81 Limbo is set
        // at5F4E9E (the same byte tested by the5F4D45 entry guard).
        self.destroy_spotlight_for_parent(stable_id, context);
        Self::techno_clear_discovery_on_conceal(
            &self.session,
            &mut self.substrate,
            &self.houses,
            stable_id,
        );
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.lifecycle.in_limbo = true;
        } else {
            // Ordinary Particle base fields are null and Mark(REMOVE) has
            // no cell footprint; the same Object5F4D30 body still owns its
            // retained Display removal and independent +81 write.
            self.substrate
                .particle_systems
                .enter_particle_limbo(stable_id);
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealLimboSet);
        self.lifecycle_outputs
            .emit(LifecycleOutput::ClearRedraw { stable_id });
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::ConcealClearRedrawBoundary);
        ConcealOutcome::Concealed
    }

    /// `TechnoClass::ClearDiscoveryOnConceal @ 0x006F4A40` (vt+0x11C): an
    /// object whose house a human does not control
    /// ([`HouseState::is_controlled_by_human`], `0x0050B730`) forgets that
    /// the current house discovered it (`+0x41B`). `+0x41A` and `+0x41C`
    /// stay. Object Conceal calls it (`0x005F4E98`), and so does a paradrop
    /// plane that took a refused passenger back (`0x00415EC1`).
    ///
    /// [`HouseState::is_controlled_by_human`]:
    /// crate::sim::house_state::HouseState::is_controlled_by_human
    pub(crate) fn techno_clear_discovery_on_conceal(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        stable_id: u64,
    ) {
        let human = substrate
            .entities
            .get(stable_id)
            .and_then(|entity| houses.get(&entity.owner()))
            .map(|house| house.is_controlled_by_human(session.game_mode_nonzero()));
        if human == Some(false)
            && let Some(entity) = substrate.entities.get_mut(stable_id)
        {
            entity.discovery.discovered_by_current_house = false;
        }
    }

    /// TechnoClass Limbo sends synchronous BREAK to every contact before the
    /// common Object Conceal transaction.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn techno_limbo(&mut self, stable_id: u64) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::default())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn techno_limbo_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> ConcealOutcome {
        self.techno_limbo_with_context(stable_id, UninitContext::new(rules, registry))
    }

    pub fn techno_limbo_with_context(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
    ) -> ConcealOutcome {
        if !self.substrate.entities.contains(stable_id) {
            return ConcealOutcome::MissingOrDead;
        }
        // Building445D8E: Recount on the first Limbo (Building4458CE skips a
        // repeated one), before the base recompute.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            Self::update_house_tracking(
                &mut self.substrate,
                &mut self.houses,
                stable_id,
                |tracking, entity| tracking.recount(entity),
            );
            // Building445D99 follows Recount and precedes Techno Limbo.
            // The non-factory prerequisite path stops here: its strip
            // cancellation is decided only after this frame's Factory sweep.
            if let Some(entity) = self.substrate.entities.get(stable_id)
                && entity.category() == EntityCategory::Structure
                && let Some(house) = self.houses.get_mut(&entity.owner())
            {
                house.request_tech_recheck();
            }
            // 0x004459AE..0x004459CA (infantry) and the unit arm after it:
            // the building's self-heal counts leave its house on the same
            // first Limbo, each clamped at zero.
            self.remove_house_self_heal(stable_id, context.rules());
        }
        // Building445DA6 precedes Techno Limbo445DDA, including its pointer
        // expiry and InLimbo write. Building4458CE skips it on repeated Limbo;
        // UnInit's earlier expiry may already have removed this list entry.
        if let Some(owner) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| {
                entity.category() == EntityCategory::Structure && !entity.lifecycle.in_limbo
            })
            .map(|entity| entity.owner())
        {
            self.recalculate_house_base_geometry(owner, context.rules());
        }
        // `InfantryClass::Limbo @ 0x0051DF10`, before FootClass::Limbo and
        // whether or not the man is already in limbo: its locomotor's
        // Stop_Movement_Animation (ILocomotion `+0xAC`, `0x0051DF30`), the
        // water state's constructor sentinel (`+0x6E8 = 2`), not prone
        // (`+0x6DB`) and Doing Ready (`+0x6C4`, stored without Do_Action).
        // A boarded man leaves his transport with these.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && entity.category() == EntityCategory::Infantry
        {
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.stop_movement_animation();
            }
            entity.mission_leaf.reset_infantry_water_state();
            if let Some(infantry) = entity.infantry.as_mut() {
                infantry.stand_up();
            }
            entity
                .mission_leaf
                .set_infantry_doing_verified(crate::sim::movement::infantry_action::DO_READY)
                .expect("Ready is in the Doing table");
        }
        // Foot4DB266 reads InLimbo once before its callback-bearing prelude.
        // A repeated call jumps4DB3AF directly to TechnoLimbo; admission
        // belongs to this caller, not the locomotor's literal +9C body.
        let first_foot_limbo = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category() != EntityCategory::Structure && !entity.lifecycle.in_limbo
            });
        Self::foot_neighbors_before_limbo(
            &self.substrate,
            &self.resolved_terrain,
            &mut self.overlay_grid,
            stable_id,
        );
        // Foot4DB260 dispatches vt+0x500 on the first Limbo (0x004DB2FB,
        // while A8E7AC is zero) before +9C(0): the same class receiver as
        //FindPath/FireAt. Infantry51DAF0 clears Doing/Walk moving state;
        //Unit/Aircraft4D55C0 calls the active locomotor Stop_Moving. This
        //includes Hover/Jumpjet Script37 returns, before occupation release.
        let rules = context.rules();
        if !self.object_placement_scope_active()
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    entity.category() != EntityCategory::Structure && !entity.lifecycle.in_limbo
                })
            && let Err(cause) =
                self.foot_stop_driver(stable_id, rules, context.registry(), context.effects())
        {
            log::debug!("Foot {stable_id} Limbo Stop_Driver: {cause}");
        }
        // 0x004DB324: only the admitted Foot prelude calls +9C(0). Keep
        // the entry admission across Stop_Driver's synchronous callbacks.
        // Native Hover5171C0 itself has no InLimbo gate and retains its head.
        if first_foot_limbo {
            self.locomotor_mark_all_occupation_bits(stable_id, false);
        }
        // FootClass::Limbo (0x004DB260) then Locks the locomotor (+0xB0) on
        // the first Limbo, so a boarded or stored man keeps no Walk
        // destination or head to resume.
        if let Some(entity) = self.substrate.entities.get_mut(stable_id)
            && !entity.lifecycle.in_limbo
            && let Some(locomotor) = entity.locomotor.as_mut()
        {
            locomotor.walk_lock();
        }
        // FootLimbo4DB353 follows the locomotor Lock and precedes the
        // air-tracker/Techno suffix. It detaches any shared Foot sound, with
        // no MoveSound latch gate or write. Unit7440B4, Infantry51DF53 and
        // Aircraft's direct vtable slot all delegate to this one Foot path.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                entity.category() != EntityCategory::Structure && !entity.lifecycle.in_limbo
            })
        {
            self.sound_events
                .emit(super::SimSoundEvent::ObjectSoundDetached { owner: stable_id });
        }
        self.substrate
            .release_foot_air_tracker_before_limbo(stable_id);
        #[cfg(feature = "ares")]
        self.limbo_ares_attach_effects(stable_id, context.rules());
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            //6F6B16 ordinary sight release precedes TypeCD1 gap removal at
            //6F6B6A; both precede Object Limbo. Repeated Limbo emits neither.
            // A `RevealToAll=` type takes the branch at `0x006F6AEF..0x006F6B06`
            // instead: ReReveal(0, 0, 1, PlayerPtr) releases its sight for the
            // current house in place of its owner's. Without a current house
            // (headless) there is no PlayerPtr to release for, and the
            // ordinary release keeps its place before the gap removal.
            let rules = context.rules();
            let reveal_to_all = self.session.current_house().is_some()
                && self
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| self.object_type(entity.type_ref(), rules))
                    .is_some_and(|object| object.reveal_to_all);
            if reveal_to_all {
                self.rereveal_sight_to_current_house(stable_id, rules);
            } else {
                self.release_object_sight_with_context(stable_id, context);
            }
            Self::remove_building_gap(
                &self.interner,
                &mut self.fog,
                &self.house_alliances,
                &mut self.substrate,
                &self.houses,
                &self.resolved_terrain,
                stable_id,
            );
            // Steam gamemd, TechnoLimbo6F6B98..6F6BA0 clears an
            // OpenTopped hull's own rider targets before HouseRemoved6F6BD1.
            // Cargo, each rider's +82 and Logic membership survive this call.
            self.open_topped_passengers_take_target(
                stable_id,
                None,
                context.rules(),
                context.registry(),
                context.effects(),
            );
            // 6F6BD1: Removed_From_Game, also only on the first Limbo.
            self.house_removed_from_game(stable_id, context.rules(), context.effects());
            // BuildingClass::Limbo 0x00445946..0x00445988: an opened
            // Helipad's docks leave its house's AirportDocks.
            let rules = context.rules();
            if let Some(entity) = self.substrate.entities.get(stable_id)
                && entity.category() == EntityCategory::Structure
                && entity.building_art.actually_placed()
                && let Some(object) = self.object_type(entity.type_ref(), rules)
                && object.helipad
            {
                let owner = entity.owner();
                if let Some(house) = self.houses.get_mut(&owner) {
                    house
                        .tracking_mut()
                        .limbo_airport_docks(object.number_of_docks);
                }
            }
        }
        //6F6C2A/2F removes the retained contribution and clears+508 before
        //ObjectConceal; its result cannot roll these Techno writes back.
        self.spatial_threat_before_limbo(stable_id, context.rules(), context.terrain());
        // BuildingClass owns this pass before the common TechnoClass Limbo can
        // clear committed type/cell facts or broadcast another expiry callback.
        Self::invalidate_base_plan_from_building_limbo(
            &self.session,
            &self.substrate,
            &mut self.houses,
            stable_id,
        );
        // FootClass::Limbo @ 0x004DB260 and BuildingClass::Limbo @
        // 0x00445880 remove the exact deposited footprint before conceal.
        self.remove_sensor_before_limbo(stable_id, context.rules(), context.registry());
        // Dead and InLimbo are independent native state. TechnoClass::Limbo
        // still reaches ObjectClass::Conceal for a stored dead object; the
        // latter's InLimbo branch alone decides whether Conceal is a no-op.
        self.clear_building_base_reservation_and_repair(stable_id, context);
        // The same TechnoClass::Limbo sound cleanup releases the turret
        // rotation loop's `+0x488` handle before the gattling report handle,
        // and re-arms its latch.
        self.turret_rotate_sound_limbo(stable_id, context.effects(), context.rules());
        // TechnoClass::Limbo releases the gattling loop and clears the report
        // latch (`0x006F6C6B`, `0x006F6C76`) ahead of its radio pass.
        self.gattling_limbo(stable_id, context);
        // RadioLimbo65AA80..65AA99 tests InLimbo before Broadcast(3).
        // CargoAdd4733B0 repeats virtual Limbo after Hospital51A35F's
        // post-conceal HELLO; breaking again loses the repair contact.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo)
        {
            crate::sim::radio::broadcast_break(self, stable_id, context.rules(), context.effects());
        }
        let outcome = self.object_conceal_with_context(stable_id, context);
        // Building445DF4..445E14 follows the base Limbo, including the
        // already-limbo branch4458CE. This is one House/category callback,
        // never a global eligibility sweep or a boolean-transition latch.
        let rules = context.rules();
        if let Some((owner, category)) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
            .and_then(|entity| {
                let object = self.object_type(entity.type_ref(), rules)?;
                Some((
                    entity.owner(),
                    crate::sim::production::factory_category_for_building(object)?,
                ))
            })
        {
            crate::sim::production::update_factory_queue(
                self,
                rules,
                owner,
                category,
                context.effects(),
            );
        }
        outcome
    }

    /// Guard UnInit's legacy uncredited-loss fallback. Routed RecordTheKill
    /// callbacks already book their immediate House statistics, including
    /// Temporal's full-health callback. Unrouted destruction sites still book
    /// only their loss here; this does not certify their missing kill callback.
    /// House quantities move through Removed_From_Game and Remove_Tracking.
    pub(crate) fn record_destruction_once(
        substrate: &mut ObjectSubstrate,
        houses: &mut Houses,
        stable_id: u64,
    ) {
        let Some((
            owner,
            category,
            already_recorded,
            destroyed,
            dont_score,
            suppress_building_loss,
        )) = substrate.entities.get(stable_id).map(|entity| {
            (
                entity.owner(),
                entity.category(),
                entity.kill_record.is_recorded(),
                // A Temporal erase leaves at full health with its kill
                // already booked (`combat::record_kill::record_kill_credit`);
                // a crashing infantryman falls at Health 1 (`0x0051BC57`)
                // after the kill that booked its loss.
                entity.health.current() == 0
                    || entity.kill_record.killer().is_some()
                    || entity.crash.is_crashing(),
                entity.tracking_facts.dont_score(),
                entity.category() == EntityCategory::Structure
                    && entity.building_loss_index() == -1,
            )
        })
        else {
            return;
        };
        if already_recorded {
            return;
        }
        if let Some(entity) = substrate.entities.get_mut(stable_id) {
            entity.kill_record.mark_fallback_recorded();
        }
        if destroyed
            && !dont_score
            // A synchronous sale Tag action can enter UnInit before the
            // outer RecordKill books its guard. The native +53C sentinel
            // still suppresses the building loss at that nested boundary.
            && !suppress_building_loss
            && let Some(house) = houses.get_mut(&owner)
        {
            house.stats_mut().record_loss(category);
        }
    }

    /// `Detach_All(all)` (vtable `+0xDC`): the class prelude, then
    /// `ObjectClass::Detach_All @ 0x005F5280`'s Deselect and its expiry
    /// announcement (`DispatchPointerExpiredCleanup @ 0x007258D0` with `all`
    /// as the control), leaving Logic, occupancy and liveness intact.
    ///
    /// Callers:
    /// * `all`: the exact-zero Destroy callback of `ObjectClass::ReceiveDamage`
    ///   (`0x005F5765..0x005F57AF`), after the kill callback and before
    ///   TechnoClass's death arm, and the Stun of a living object.
    /// * not `all`: `TechnoClass::StartCloaking @ 0x00703770` and the cloak's
    ///   1 → 2 completion (`0x006FBA98`).
    ///
    /// The class prelude:
    /// * `BuildingClass::Detach_All @ 0x0044EBF0`. With `all`, the production
    ///   prelude it shares with Conceal: abandon the separate Building
    ///   Factory, temporarily exclude the building, and cancel the House
    ///   product only without an alternative. Then RADIO OVER_OUT to every
    ///   contact. Without `all` (`0x0044EF29..0x0044EF69`), OVER_OUT to each
    ///   contact its owner is not allied with (`HouseClass::Is_Ally_ByObject
    ///   @ 0x004F9A90`), slot by slot.
    /// * `FootClass::Detach_All @ 0x004D9720`. With `all`, the object leaves
    ///   its team (`0x004D9744`) and sends OVER_OUT to contact 0. Without
    ///   `all` (`0x004D9762..0x004D9785`), contact 0 gets OVER_OUT only when
    ///   the owner is not allied with it.
    ///
    /// The Deselect (`0x005F52A8..0x005F52E9`) is unconditional with `all`.
    /// Without it, the object stays selected when its owner is the player's
    /// (`0x0050B6F0`, [`Self::house_is_human_player`]) or, for a Techno, the
    /// player senses its cell (vt+0x328, [`sensor_visible_to_player`]). Then
    /// the follow camera lets go of the object whatever the gate decided
    /// (`0x005F52EF..0x005F5306`, [`LifecycleOutput::FollowReleased`]).
    ///
    /// Other `Detach_All(false)` callers, all dormant in retail: Infantry
    /// PerCellProcess's one-cell UndeploysInto conversion (`0x00519A19`, on
    /// the Building), the vehicle thief (`0x00520423`, `0x00741868`) and the
    /// Tunnel locomotor (`0x00728F7B`, `0x00729465`). No retail building
    /// undeploys into a one-cell infantryman, no type sets `VehicleThief=`
    /// and none uses the Tunnel locomotor.
    ///
    /// The LineTrail teardown (`0x005F5284..0x005F529D`) has nothing to free:
    /// VERA constructs LineTrails only for bullets (`admit_projectile`).
    ///
    /// [`sensor_visible_to_player`]: super::techno_ai_cloak::sensor_visible_to_player
    pub(crate) fn detach_all(&mut self, stable_id: u64, all: bool, context: UninitContext<'_>) {
        let Some((category, owner)) = self
            .substrate
            .entities
            .get(stable_id)
            .map(|e| (e.category(), e.owner()))
        else {
            return;
        };
        let allied_with = |sim: &Self, contact: u64| {
            sim.substrate.entities.get(contact).is_some_and(|contact| {
                crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(sim.house_alliances.as_map()),
                    &sim.interner,
                    owner,
                    contact.owner(),
                )
            })
        };
        match category {
            EntityCategory::Structure if all => {
                // Shared BuildingDetach44EBF0 production prelude.
                crate::sim::production::detach_building_production(
                    self,
                    context.rules(),
                    stable_id,
                    context.effects(),
                );
                crate::sim::radio::broadcast_break(
                    self,
                    stable_id,
                    context.rules(),
                    context.effects(),
                );
            }
            EntityCategory::Structure => {
                let capacity = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .map_or(0, |entity| entity.radio_contacts.capacity());
                for slot in 0..capacity {
                    if let Some(contact) = self
                        .substrate
                        .entities
                        .get(stable_id)
                        .and_then(|entity| entity.radio_contacts.slot(slot))
                        .filter(|&contact| !allied_with(self, contact))
                    {
                        crate::sim::radio::transmit(
                            self,
                            stable_id,
                            contact,
                            crate::sim::radio::RadioMessage::Break,
                            crate::sim::radio::RadioPayload::default(),
                            context.rules(),
                            context.effects(),
                        );
                    }
                }
            }
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft => {
                if all {
                    // `0x004D9744`: the Foot prelude leaves the object's team
                    // first.
                    self.leave_team(stable_id, false, context.rules(), context.effects());
                }
                if let Some(contact) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .and_then(|entity| entity.radio_contacts.slot(0))
                    .filter(|&contact| all || !allied_with(self, contact))
                {
                    crate::sim::radio::transmit(
                        self,
                        stable_id,
                        contact,
                        crate::sim::radio::RadioMessage::Break,
                        crate::sim::radio::RadioPayload::default(),
                        context.rules(),
                        context.effects(),
                    );
                }
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyRadioBreakCompleted { stable_id });

        let keeps_selection = !all
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| {
                    self.house_is_human_player(entity.owner())
                        || super::techno_ai_cloak::sensor_visible_to_player(self, stable_id)
                });
        if !keeps_selection && let Some(target) = self.substrate.entities.get_mut(stable_id) {
            target.selection.deselect();
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyDeselected { stable_id });
        self.lifecycle_outputs
            .emit(LifecycleOutput::FollowReleased { stable_id });

        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::DestroyNotifyBoundary { stable_id });
        if all {
            self.notify_pointer_expired(stable_id, context);
        } else {
            self.announce_detach_all(stable_id, context);
        }
    }

    /// The Building NowDead contact loop (`BuildingClass::ReceiveDamage`,
    /// `0x00442511..0x004425F4`) over the contacts the building held before
    /// the hit: one at least 0x100 leptons from the building's `GetCoords`
    /// centre, on a building that is not a `Helipad=`, is sent RUN_AWAY (0x17),
    /// so a War Miner on a destroyed refinery's pad leaves its unload for
    /// Harvest (`radio::receive`), then clears its Unit+0x500 pending entry
    /// (`0x004425AA`). A nearer contact, or any contact of a helipad, takes
    /// `Strength * 10` C4 damage (`0x004425B6..0x004425EE`), with no source
    /// and both receiver flags set. Execute each receiver synchronously in
    /// the caller's transaction: its death effects may alter later contacts.
    /// Evidence: retail gamemd.exe instructions, inspect with
    /// `python -m tools.native_inspect disasm 0x442511 --bytes 0xf7`.
    pub(crate) fn building_now_dead_contacts(
        &mut self,
        building_id: u64,
        contacts: &[u64],
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        run: &mut crate::sim::combat::world_receiver::ReceiverRun<'_>,
    ) -> (
        crate::sim::combat::DeathEffects,
        Vec<crate::sim::combat::UnderAttackEvent>,
    ) {
        let mut death_effects = crate::sim::combat::DeathEffects::default();
        let mut under_attack_events = Vec::new();
        // `0x00442532`/`0x00442543`: both ends are GetCoords (vt+0x48).
        let Some((centre, helipad)) = self.substrate.entities.get(building_id).and_then(|b| {
            let object = self.object_type(b.type_ref(), rules)?;
            let centre = crate::sim::movement::ground_pose::object_get_coords(
                b,
                self.resolved_terrain.as_ref(),
            );
            Some((
                [
                    i64::from(centre.x),
                    i64::from(centre.y),
                    i64::from(centre.z),
                ],
                object.helipad,
            ))
        }) else {
            return (death_effects, under_attack_events);
        };
        for &contact in contacts {
            let Some(at) = self.substrate.entities.get(contact).map(|c| {
                crate::sim::movement::ground_pose::object_get_coords(
                    c,
                    self.resolved_terrain.as_ref(),
                )
            }) else {
                continue;
            };
            let d = [
                i64::from(at.x) - centre[0],
                i64::from(at.y) - centre[1],
                i64::from(at.z) - centre[2],
            ];
            // `0x00442586`: the truncated length against 0x100.
            let far = d.iter().map(|v| v * v).sum::<i64>() >= 0x100 * 0x100;
            if far && !helipad {
                #[cfg(any(test, feature = "test-support"))]
                self.trace_lifecycle_for_test(LifecycleTestEvent::BuildingNowDeadRunAway {
                    building_id,
                    contact_id: contact,
                });
                crate::sim::radio::transmit(
                    self,
                    building_id,
                    contact,
                    crate::sim::radio::RadioMessage::RunAway,
                    crate::sim::radio::RadioPayload::default(),
                    rules,
                    run.effects(),
                );
                if let Some(unit) = self.substrate.entities.get_mut(contact) {
                    // `0x004425AA`, after RUN_AWAY returns.
                    unit.pending_entry.clear();
                }
            } else {
                //4425BA gets the contact's type, not its current Health.
                //LEA/SHL at4425CC..4425D4 are wrapping i32 arithmetic.
                let Some(damage) = self.substrate.entities.get(contact).and_then(|entity| {
                    self.object_type(entity.type_ref(), rules)
                        .map(|object| object.strength.wrapping_mul(10))
                }) else {
                    continue;
                };
                let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
                let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                    contact,
                    damage,
                    0,
                    crate::sim::combat::RAD_NO_ATTACKER,
                    None,
                    warhead,
                    crate::sim::combat::ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: true,
                    },
                );
                let (nested, mut pings) = crate::sim::combat::world_receiver::commit_entities(
                    self,
                    run,
                    &[event],
                    Some(false),
                    rules,
                    overlay_registry,
                );
                death_effects.append(nested);
                under_attack_events.append(&mut pings);
            }
        }
        (death_effects, under_attack_events)
    }

    /// The Stun of TechnoClass::ReceiveDamage's death arm (`0x00702210`),
    /// after the death sounds and before the debris. Units, Infantry and
    /// Aircraft take `FootClass::Stun @ 0x004D5660`: the class setter's NULL
    /// destination (`0x004D5669`, [`Self::assign_null_destination`]), Path[0]
    /// = -1 (`0x004D5673`) and `Stop_Driver` (vtable `+0x500`), which is
    /// Foot's `0x004D55C0` for a Unit or an Aircraft, the locomotor's
    /// `Stop_Moving` ([`Self::locomotor_stop_moving`]), and Infantry's
    /// `0x0051DAF0` ([`Self::infantry_stop_driver`]). `TechnoClass::Stun @
    /// 0x006FCD40` follows: Assign_Target(NULL), a second NULL destination
    /// (`0x006FCD55`), RADIO OVER_OUT to every contact (`+0x280(3)`),
    /// SpawnManager Kill_All_Spawns `0x006B7100` and ClearAllTargets
    /// `0x006B7BB0`, Detach_All(1), and Deselect.
    ///
    /// Each accepted NULL destination reaches the locomotor's `Stop_Moving`
    /// through Foot's null arm, so a moving Jumpjet stops up to three times.
    /// A Jumpjet Unit's keeps a moving wreck flying to the cell under it and
    /// lifts a descent back into the climb, so `Process`'s crash latch
    /// engages wherever the kill found it. Unit `0x00741970` returns at once
    /// without a NavCom (its `+0x1F8` override down), so the NavCom the
    /// Stop_Driver's re-target writes is what the second NULL destination
    /// clears. A Jumpjet Infantry's placement draws on the Scenario RNG each
    /// time his moving byte is set, and his Stop_Driver's Do_Action at Health
    /// 0 re-enters Stop_Driver once more when it accepts. Native execution:
    /// `tools/spatial_oracle/jumpjet_infantry_crash`.
    ///
    /// A lifted Foot (`+0x6AD`, Magnetron-held) skips the spawn calls and
    /// Detach_All (`0x006FCD79`). On the death path the spawn calls repeat
    /// the Destroy's owner arm and find nothing left to kill. Detach_All(1) repeats the broadcast the killing hit's Destroy
    /// already made (on a re-entered death arm, `0x0070202E..0x00702035`, that
    /// was an earlier call). The walk is not repeated: every Target write
    /// since then refused the Health-0 object (the setter through
    /// `assign_target_commits`, including the Destroy walk's own Restores; the
    /// scans, orders and legacy retaliation filter Health 0 themselves), so no
    /// listener can hold it again. The Foot prelude's contact-0 OVER_OUT is
    /// covered by the OVER_OUT to every contact. A living object's Stun (a
    /// sinking hull, a retreating aircraft leaving the map) runs the walk.
    /// Native execution of an Aircraft's: `tools/spatial_oracle/fly_stop`'s
    /// Stun rows, replayed by `fly_process_tests::fly_death_stun_matches_native_rows`.
    pub(crate) fn techno_death_stun(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let category = entity.category();
        let walking_infantry = category == EntityCategory::Infantry
            && entity.locomotor.as_ref().is_some_and(|locomotor| {
                locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Walk
            });
        if matches!(
            category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        ) {
            self.assign_null_destination(
                stable_id,
                context.rules,
                context.registry,
                context.effects(),
            );
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.clear_live_path_head();
            }
            if category == EntityCategory::Infantry {
                if let Err(cause) = self.infantry_stop_driver(
                    stable_id,
                    context.rules(),
                    context.registry,
                    context.effects(),
                ) {
                    log::debug!("infantry {stable_id} Stun Stop_Driver: {cause}");
                }
            } else if let Err(cause) = self.locomotor_stop_moving(
                stable_id,
                context.rules,
                context.registry,
                context.effects(),
            ) {
                log::debug!("{stable_id} Stun Stop_Driver: {cause}");
            }
        }
        if walking_infantry {
            let _ = self.assign_target_represented(
                stable_id,
                None,
                context.rules,
                None,
                FrameEffects::default(),
            );
        } else if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
        }
        self.assign_null_destination(
            stable_id,
            context.rules,
            context.registry,
            context.effects(),
        );
        if !self.substrate.entities.contains(stable_id) {
            return;
        }
        crate::sim::radio::broadcast_break(self, stable_id, context.rules(), context.effects());
        // `0x006FCD6B..0x006FCD79`: a lifted Foot keeps its spawns, its spawns'
        // targets and its listeners.
        let lifted = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                ) && entity.lift.swap_active()
            });
        if lifted {
            if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                entity.selection.deselect();
            }
            return;
        }
        crate::sim::spawn_manager::kill_all_spawns_with_context(self, stable_id, context);
        crate::sim::spawn_manager::clear_all_spawn_targets(
            self,
            stable_id,
            context.rules(),
            context.registry(),
            context.effects(),
        );
        // A living object's Stun cannot use the Health-0 elision above: its
        // Techno6FCD9B Detach_All(1) runs, and self and other pointer-expiry
        // callbacks observe the live object. Unit737E58 repeats Stun after
        // restoring Health 1/+3CD, warp sinking stuns a living hull, and a
        // retreating aircraft leaving the map is stunned alive (Fly 4CD5D7).
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.health.current() != 0)
        {
            self.detach_all(stable_id, true, context);
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.selection.deselect();
        }
    }

    pub(crate) fn apply_lifecycle_request(
        &mut self,
        request: LifecycleRequest,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        match request {
            LifecycleRequest::Uninit {
                stable_id,
                reason: _,
            } => self.uninit_with_context(
                stable_id,
                UninitContext::new(rules, registry).with_effects(effects),
            ),
        }
    }

    fn run_represented_uninit_pre_hook(&mut self, stable_id: u64) {
        // Object UnInit5F6616 expires damage-fire owners before Building's
        // destructor43BDE0 destroys the remaining slot Anims. Do not run the
        // recovery path here: it converts coordinates and stops sounds early.
        // Ordinary21slots have no Anim owner+CC and survive this broadcast;
        // Building43BDC5 clears them only at deferred scalar destruction.
        Self::record_destruction_once(&mut self.substrate, &mut self.houses, stable_id);
        crate::sim::docking::bunker_link::break_links_on_despawn(&mut self.substrate, stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UninitClassPre { stable_id });
    }

    /// What `listener` holds that its pointer-expiry forwards can change:
    /// `name` gets each id a field [`Self::notify_entity_pointer_expired`] or
    /// a forward compares (repeats possible), and the result says whether it
    /// holds an object a forward reaches whatever expires (an airstrike
    /// manager, its parasite). A superset of each forward's own test, so
    /// passing over a listener that is not the expiring object, names nothing
    /// expiring and returns false matches visiting it. A field or forward
    /// added to the broadcast must be added here too.
    /// VERA-only: feeds the expiry-listener index (`ExpiryListeners`) so the
    /// `0x007258D0` broadcast skips entities; the listener's fields are the truth.
    pub(super) fn entity_expiry_holds(listener: &GameEntity, mut name: impl FnMut(u64)) -> bool {
        if let Some(anim) = listener.emp_sparkle() {
            name(anim);
        }
        #[cfg(feature = "ares")]
        listener.attached_effect_anims().for_each(&mut name);
        for target in [
            listener.attack_target.target(),
            listener.suspended_attack_target.target(),
            listener.archive_target(),
        ] {
            if let Some(TargetKind::Entity(id)) = target {
                name(id);
            }
        }
        listener.radio_contacts.iter_live().for_each(&mut name);
        listener
            .tesla_charge
            .chargers()
            .iter()
            .copied()
            .for_each(&mut name);
        if let Some(cargo) = listener.passenger_role.cargo() {
            cargo.passengers().iter().copied().for_each(&mut name);
        }
        let navigation = &listener.navigation;
        navigation
            .suspended_nav_com()
            .iter()
            .chain(navigation.nav_com().iter())
            .chain(navigation.nav_queue().iter())
            .chain(navigation.nav_list().iter())
            .filter_map(|target| target.object_id())
            .for_each(&mut name);
        [
            listener.passenger_role.inside_transport_id(),
            listener.pending_entry(),
            listener.spawn_owner_id.owner(),
            listener.spark_particle_system.system(),
            listener.damage_smoke_system_id.system(),
            listener.ammo().and_then(|ammo| ammo.dock()),
            listener
                .pending_c4_detonation
                .pending()
                .and_then(|pending| pending.source_entity_id()),
            listener.lift.target(),
            listener.lift.source(),
            listener.drop_credit.manipulated_by(),
            listener.building_light.light(),
        ]
        .into_iter()
        .flatten()
        .for_each(&mut name);
        listener
            .parasite
            .class()
            .and_then(|class| class.grapple_anim())
            .into_iter()
            .for_each(&mut name);
        listener.mega_mission.referenced_ids().for_each(&mut name);
        listener.distributed_fire_references().for_each(&mut name);
        // The SpawnManager, CaptureManager and TemporalClass forwards act on
        // the listener's own expiry and on the ids they hold.
        if let Some(manager) = &listener.spawn_manager {
            manager.expiry_names().for_each(&mut name);
        }
        if let Some(manager) = &listener.capture_manager {
            manager.victims().for_each(&mut name);
        }
        listener
            .temporal
            .warp_target()
            .into_iter()
            .for_each(&mut name);
        listener.airstrike.owns_manager() || listener.parasite.eater().is_some()
    }

    /// Whether the pointer-expiry forwards an entity listener receives can
    /// change anything for `expired_id`: the listener is the expiring object,
    /// or [`Self::entity_expiry_holds`] names `expired_id` or says it acts on
    /// every expiry.
    pub(super) fn entity_expiry_listener_acts(
        listener: &GameEntity,
        listener_id: u64,
        expired_id: u64,
    ) -> bool {
        let mut names = listener_id == expired_id;
        Self::entity_expiry_holds(listener, |id| names |= id == expired_id) || names
    }

    /// Each RNG stream's cursor, which every draw moves.
    #[cfg(any(test, feature = "test-support"))]
    fn rng_cursors(&self) -> [(u8, i32, i32); 3] {
        [
            self.scenario_rng.logical_state(),
            self.main_rng.logical_state(),
            self.mapgen_rng.logical_state(),
        ]
        .map(|view| (view.disabled, view.index_a, view.index_b))
    }

    /// Represented entries in global ObjectClass construction order, each
    /// with the store it lives in. Stable IDs are monotonic and never reused,
    /// so merging the separate Rust stores by ID reproduces the native
    /// registration order without walking holes left by already-finalized
    /// objects, and an ID's store never changes.
    ///
    /// gamemd-derived: active YR `ObjectClass` construction/destruction at
    /// `0x005F3900` / `0x005F3B80` maintains the listener roster in object
    /// construction order. Of the entities, it lists `entities` (ascending).
    /// Test builds visit this roster; production builds visit the listeners
    /// the expiry index finds, in the same order.
    #[cfg(any(test, feature = "test-support"))]
    fn removal_listener_order(&self, entities: Vec<u64>) -> Vec<(u64, ObjectKind)> {
        let mut listeners = Vec::new();
        for kind in ObjectKind::ALL {
            match kind {
                ObjectKind::Entity => listeners.extend(entities.iter().map(|&id| (id, kind))),
                ObjectKind::Anim => {
                    listeners.extend(self.substrate.anims.iter().map(|(&id, _)| (id, kind)));
                }
                ObjectKind::ParticleSystem => listeners.extend(
                    self.substrate
                        .particle_systems
                        .iter()
                        .map(|(&id, _)| (id, kind)),
                ),
                ObjectKind::Projectile => {
                    listeners.extend(self.projectiles.iter().map(|(&id, _)| (id, kind)));
                }
                ObjectKind::Wave => {
                    listeners.extend(self.waves.iter().map(|(&id, _)| (id, kind)));
                }
                ObjectKind::Smudge => {
                    if let Some(grid) = &self.smudge_grid {
                        listeners.extend(grid.objects().map(|(id, _)| (id, kind)));
                    }
                }
                // ObjectClass's PointerExpired (`0x005F5230`), which these
                // inherit, and Terrain's (`0x0071CFD0`), which adds only its
                // type, clear nothing these stores hold. A Spotlight's
                // (`0x00436A00`) runs apart, for every expiry.
                ObjectKind::VoxelAnim
                | ObjectKind::Particle
                | ObjectKind::Terrain
                | ObjectKind::Spotlight => {}
            }
        }
        // Each store yields its IDs in order: the run-adaptive stable sort
        // merges those runs instead of sorting from scratch.
        listeners.sort_by_key(|&(stable_id, _)| stable_id);
        debug_assert!(
            listeners.windows(2).all(|pair| pair[0].0 != pair[1].0),
            "object stable ID exists in more than one represented store"
        );
        listeners
    }

    /// The detach-time targeting sweep: release every object currently shooting
    /// at `detach_id`, which is leaving play *while still alive*.
    ///
    /// This is not the pointer-expiry broadcast. The detaching object survives —
    /// it is being sold, changing owner, teleporting, or being detached by area
    /// damage — so nothing else nulls the references pointed at it, and this is
    /// the only route by which a live-but-detached target releases its
    /// attackers. In an ordinary skirmish it fires tens of times per match:
    /// every building sale, every engineer capture, every mind-control or
    /// Psychic Beacon owner change, every Chrono Legionnaire or Chronosphere
    /// teleport.
    ///
    /// Three clauses are NOT copies of the pointer-expiry sweep and are
    /// reproduced verbatim because each is observable:
    ///
    /// 1. **Descending stable-ID iteration.** The native walk runs the global
    ///    techno vector from its last entry down to its first. When two objects
    ///    share the detaching target, the higher ID's class target/destination
    ///    setters run first. Restore does not itself dispatch mission AI.
    /// 2. **Restore runs first, with no suspended-mission pre-check.** The
    ///    expiry sweep asks whether a mission is suspended and clears the target
    ///    before restoring; this one restores unconditionally and clears after.
    /// 3. **The target clear is conditional on the Restore not having replaced
    ///    the target.** A successful Restore re-installs the archived target, and
    ///    the null-out then does not run.
    ///
    /// The native sweep has callers in area damage, building sale, owner change,
    /// building placement and teleportation. The represented owner-change,
    /// bridge-damage, wall-sale and teleport (`0x007193C7`) callers share this
    /// owner; the other caller chains remain separate migration work.
    ///
    /// After the Restore an aircraft now on Patrol restarts it
    /// (`0x0070D513..0x0070D532`, [`crate::sim::aircraft::restart_patrol_after_restore`]).
    pub(crate) fn stop_all_targeting_on_detach(
        &mut self,
        detach_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.stop_all_targeting_target(TargetKind::Entity(detach_id), rules, registry, effects);
    }

    /// Bridge success, foundation clearing and House wall sale call the same
    /// 0x0070D4A0 sweep with a CellClass pointer. This is not PointerExpired:
    /// Restore precedes the conditional target clear, in descending Techno order.
    pub(crate) fn stop_all_targeting_cell(
        &mut self,
        rx: u16,
        ry: u16,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.stop_all_targeting_target(TargetKind::Cell(rx, ry), rules, registry, effects);
    }

    fn stop_all_targeting_target(
        &mut self,
        target: TargetKind,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // VERA-only: the sweep releases a batched target scan's pick as it
        // does a TarCom (`techno_ai::scan_batch`).
        if let TargetKind::Entity(target_id) = target {
            self.release_batched_scan_pick(target_id);
        }
        let mut listeners = self.substrate.entities.keys_sorted();
        listeners.reverse();

        for listener_id in listeners {
            if let TargetKind::Entity(target_id) = target
                && self.airstrike_detach_preserves_target(listener_id, target_id)
            {
                continue;
            }
            if !self.listener_targets(listener_id, target) {
                continue;
            }

            let restored = self
                .mission_restore_represented(listener_id, rules, registry, effects)
                .expect("detach sweep listener was resolved immediately before the Restore");
            // `0x0070D513..0x0070D532`, whatever the Restore did.
            if let Some(listener) = self.substrate.entities.get_mut(listener_id) {
                crate::sim::aircraft::restart_patrol_after_restore(listener);
            }

            let target_cleared = self.listener_targets(listener_id, target);
            if target_cleared {
                self.assign_target_represented(listener_id, None, rules, registry, effects)
                    .expect("detach sweep listener remains present for the target clear");
            }

            let _ = restored;
            #[cfg(any(test, feature = "test-support"))]
            if let TargetKind::Entity(detach_id) = target {
                self.trace_lifecycle_for_test(LifecycleTestEvent::DetachTargetingSweepVisited {
                    detach_id,
                    listener_id,
                    restored,
                    target_cleared,
                });
            }
        }
        self.team_script_vm.detach_target(target.into());
    }

    /// Match the retained target identity, preserving Cell/Techno distinction.
    /// The detach sweep `0x0070D4A0`'s TarCom test `CMP [ESI+0x2B4]` (`0x0070D539`).
    fn listener_targets(&self, listener_id: u64, target: TargetKind) -> bool {
        self.substrate
            .entities
            .get(listener_id)
            .is_some_and(|listener| listener.attack_target.target() == Some(target))
    }

    /// Infantry PerCell repair519D17..519D36 calls each registered Infantry
    /// +28(hut,false) in descending construction order. The hut survives;
    /// this is not the global ObjectClass removal broadcast. Actual Infantry
    /// +28 is51AA10 -> Foot4D9960 -> Techno7077C0. Its extra +6C0 clear
    /// compares an InfantryType pointer with the hut and cannot match.
    pub(crate) fn expire_infantry_bridge_hut_targets(
        &mut self,
        hut_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // Infantry ctor517B34 appends to its type registry. Represented
        // PointerExpired receivers do not construct/delete registry entries;
        // stable construction IDs therefore preserve its descending cursor.
        let mut listeners = self.substrate.entities.keys_sorted();
        listeners.reverse();
        for listener_id in listeners {
            if !self
                .substrate
                .entities
                .get(listener_id)
                .is_some_and(|e| e.category() == EntityCategory::Infantry)
            {
                continue;
            }
            let Some(hut) = self.substrate.entities.get(hut_id) else {
                return;
            };
            let facts = (
                object_get_coords_cell(hut),
                hut.lifecycle.object_alive,
                hut.health.current(),
                hut.mission.current().known() == Some(crate::sim::mission::MissionType::Selling),
                hut.owner(),
            );
            self.notify_entity_pointer_expired(
                listener_id,
                hut_id,
                facts.0,
                facts.1,
                facts.2,
                facts.3,
                Some(facts.4),
                PointerExpiryControl::DetachAll,
                rules,
                registry,
                effects,
            );
            // Techno707B24 forwards this manager independently of control.
            crate::sim::spawn_manager::notify_pointer_expired(
                self,
                listener_id,
                hut_id,
                rules,
                registry,
                effects,
            );
        }
    }

    /// Techno70F770 and the identical PointerExpired7079D1..7A28 arm
    /// shorten the passive targeting timer (+180/+188), never weapon rearm.
    /// The native priority bracket A8E7AC suppresses both the draw and write.
    /// Original controls: infantry_deploy_action.json's reload rows.
    pub(crate) fn shorten_passive_scan_timer(&mut self, id: u64) -> bool {
        let now = self.session.binary_frame();
        if self.object_placement_scope_active()
            || self
                .substrate
                .entities
                .get(id)
                .is_none_or(|entity| entity.passive_scan.timer().remaining(now) <= 10)
        {
            return false;
        }
        let delay = self.scenario_rng.next_range_u32_inclusive(4, 8);
        self.substrate
            .entities
            .get_mut(id)
            .expect("queried passive timer owner remains present")
            .passive_scan
            .arm_timer(now, delay);
        true
    }

    /// The broadcast passes over listeners that
    /// [`Self::entity_expiry_listener_acts`] rejects, so a field compared here
    /// must be named or flagged by [`Self::entity_expiry_holds`] too. That
    /// enumeration may read only the listener: the expiry index re-reads an
    /// entity only when the store hands that entity out mutably.
    pub(super) fn notify_entity_pointer_expired(
        &mut self,
        listener_id: u64,
        expired_id: u64,
        // The expiring object's `ObjectClass::GetCoords` cell, which
        // `TechnoClass::PointerExpired` hands to `SensorCountForHouse` when it
        // computes `allowClear` on the `Detach_All` control.
        expired_get_coords_cell: Option<(u16, u16)>,
        expired_object_alive: bool,
        expired_health: i32,
        expired_is_selling: bool,
        expired_owner: Option<InternedId>,
        control: PointerExpiryControl,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(listener) = self.substrate.entities.get(listener_id) else {
            return;
        };
        let current_target_matches = listener
            .attack_target
            .target()
            .is_some_and(|target| matches!(target, TargetKind::Entity(id) if id == expired_id));
        let listener_owner = listener.owner();
        let mission_is_suspended =
            listener.mission.suspended() != crate::sim::mission::MissionId::NONE;
        // What the radio and cargo clears below would change. A listener is
        // handed out mutably only when something changes: every hand-out
        // enters the store's touch logs, and most listeners hold no
        // reference to the expiring object.
        let drops_contact =
            control == PointerExpiryControl::Uninit && listener.has_live_contact_with(expired_id);
        let drops_passenger = listener
            .passenger_role
            .cargo()
            .is_some_and(|cargo| cargo.passengers().contains(&expired_id));
        // `0x007077EE..0x007077FD`, after the radio and cargo, whatever the
        // control: a child whose SpawnOwner (`+0x2D4`) expires forgets it.
        let drops_spawn_owner = listener.spawn_owner_id.owner() == Some(expired_id);

        // `TechnoClass::PointerExpired @ 0x007077C0`, the `allowClear` local
        // (`[ESP+0x24]`): it starts true and is cancelled only on the
        // `Detach_All` control, when the expiring object is a Techno and the
        // RECEIVER's own house already holds a sensor count on that object's
        // cell — `0x00707994 CALL 0x004870D0`. The Techno half of that guard is
        // `AbstractFlags +0x14` bit 0, ORed in for every object by
        // `TechnoClass__Constructor @ 0x006F3228` and therefore true of every
        // `GameEntity`. Terrain retains the Object flags without that bit,
        // so its expiry never asks the sensor guard. `allowClear` gates both the
        // Target (`+0x2B4`) clear at `0x007079AD` and the `+0x2B8` clear at
        // `0x00707A7A`.
        let allow_clear = match control {
            PointerExpiryControl::Uninit => true,
            PointerExpiryControl::DetachAll => {
                !self.substrate.entities.contains(expired_id)
                    || !expired_get_coords_cell.is_some_and(|(rx, ry)| {
                        self.fog.has_sensor_for_house(listener_owner, rx, ry)
                    })
            }
        };
        // `0x007079B7..0x007079CB`: on the same control the Target clear is
        // additionally skipped when the expiring object's `GetOwner` (vslot
        // `+0x3C`) equals the receiver's `+0x21C`, so a diving submarine never
        // drops its own house's shooters.
        let same_owner_exempt =
            control == PointerExpiryControl::DetachAll && expired_owner == Some(listener_owner);
        let clears_current_target = current_target_matches && allow_clear && !same_owner_exempt;

        // `0x007079D1..0x00707A0D`: the re-arm sits INSIDE the guarded arm,
        // ahead of `Assign_Target(NULL)`, so a receiver that keeps its target
        // spends no Scenario draw. An already-expired timer (`elapsed >=
        // duration`) skips the block entirely, which `remaining()`'s clamp to 0
        // reproduces.
        if clears_current_target {
            self.shorten_passive_scan_timer(listener_id);
        }
        if (drops_contact || drops_passenger || drops_spawn_owner)
            && let Some(listener) = self.substrate.entities.get_mut(listener_id)
        {
            // `RadioClass::PointerExpired @ 0x0065AAC0` nulls matching sparse
            // slots in place, but ONLY on a nonzero control:
            //
            // ```
            // 0065aac1 MOV EBX,[ESP+0xc]      ; EBX = the control argument
            // 0065aae6 MOV EDX,[EAX + ECX*4]  ; slot
            // 0065aaec CMP EDX,EDI            ; slot == expired ?
            // 0065aaee JNZ 0065aafa
            // 0065aaf0 TEST BL,BL             ; control
            // 0065aaf2 JZ   0065aafa          ; control 0 skips the clear
            // 0065aaf4 MOV dword ptr [EAX],0x0
            // ```
            //
            // So `Detach_All(false)` — the dive — leaves radio contacts intact,
            // and only UnInit breaks them. A diving submarine therefore keeps
            // its naval-yard repair link and its transport link.
            if drops_contact {
                listener.clear_live_contact_with(expired_id);
            }

            // TechnoClass removes an expiring passenger from its CargoClass before
            // clearing its target/archive/manager reference family.
            if drops_passenger && let Some(cargo) = listener.passenger_role.cargo_mut() {
                let _ = cargo.disembark(expired_id);
            }
            if drops_spawn_owner {
                listener.spawn_owner_id.forget();
            }
        }

        if clears_current_target {
            self.assign_target_represented(listener_id, None, rules, registry, effects)
                .expect("expiry listener remains present");
            if mission_is_suspended {
                self.mission_restore_represented(listener_id, rules, registry, effects)
                    .expect("represented expiry restore remains available");
                // `0x00707A51..0x00707A6C`.
                if let Some(listener) = self.substrate.entities.get_mut(listener_id) {
                    crate::sim::aircraft::restart_patrol_after_restore(listener);
                }
            }
        }

        // Decide every clear from a read, then hand the listener out mutably
        // only when one applies (see `drops_contact`).
        let Some(listener) = self.substrate.entities.get(listener_id) else {
            return;
        };
        // `+0x2B8` is cleared on an exact match under `allowClear` alone — the
        // same-owner exemption applies only to `+0x2B4`.
        let clear_suspended_target = allow_clear
            && matches!(
                listener.suspended_attack_target.target(),
                Some(TargetKind::Entity(id)) if id == expired_id
            );
        // Techno707AC2..707AD7 clears eight retained effect pointers on an
        // exact match, including damage-Smoke+310. It precedes the control
        // gate707AE7 and is independent of the target's sensor/owner guards.
        let clear_damage_smoke = listener.damage_smoke_system_id.system() == Some(expired_id);
        let clear_spark_system = listener.spark_particle_system.system() == Some(expired_id);
        // Techno707AE7..707B03 clears ArchiveTarget+218 on control1. The
        // later Foot4D99F1..4D99FC clears a still-matching archive on both
        // controls, independent of sensors and same-owner target exemptions.
        // Only Unit/Infantry/Aircraft inherit that additional Foot clear.
        // Native execution: spatial_oracle/foot_archive_expiry.{json,md}.
        let foot_receiver = matches!(
            listener.category(),
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        );
        let clear_archive_target = (control == PointerExpiryControl::Uninit || foot_receiver)
            && listener.archive_target() == Some(TargetKind::Entity(expired_id));

        // FootClass clears SuspendedNavCom first, then its current/aux target,
        // and removes every matching queue entry. Cell targets are unaffected.
        //
        // `FootClass::PointerExpired @ 0x004D9960` carries its OWN copy of the
        // `allowClear` Boolean, computed by a SECOND `SensorCountForHouse`
        // call — `0x004D9A57 CALL 0x004870D0`, at the expiring object's
        // `GetCoords` cell (`0x004D9A3D` vslot `+0x48`) for the RECEIVER's house
        // (`param_1[0x87] + 0x30`) — under the same control-0 / nonnull /
        // `+0x14` bit-0 guard as the Techno body. It gates the `+0x5A0`/`+0x5A4`
        // PAIR clear below; the `+0x5A8` clear above it is unguarded. The two
        // Booleans are equal in value because both read the receiver's house at
        // the same cell, so the single `allow_clear` local models both.
        let expired_nav = NavTargetRef::object(expired_id);
        let clear_suspended_nav_com = listener.navigation.suspended_nav_com() == Some(expired_nav);
        let current_nav_matches = listener.navigation.nav_com() == Some(expired_nav);
        let retain_capture_nav = current_nav_matches
            && listener.category() == EntityCategory::Infantry
            && listener.removal_facts.occupier()
            && listener.mission.current().known()
                == Some(crate::sim::mission::MissionType::Capture)
            && expired_object_alive
            && expired_health > 0
            && !expired_is_selling;
        //4D9A0F gates the pair on current NavCom identity;4D9ABD then
        //clears both fields, irrespective of the auxiliary pointer's value.
        let clear_nav_com = current_nav_matches && !retain_capture_nav && allow_clear;
        let prune_nav_queue = listener.navigation.nav_queue().contains(&expired_nav);
        // `0x004D9B43..0x004D9B95`: every navigation list entry naming the
        // expiring object goes, before the NavQueue's.
        let prune_nav_list = listener.navigation.nav_list().contains(&expired_nav);

        // Techno707AE7..707AF5 clears private pending+500 only on control1.
        // Non-destructive DetachAll(false) retains this independent entry;
        // radio contact expiry owns the admitted visit.
        let clear_dock =
            control == PointerExpiryControl::Uninit && listener.pending_entry() == Some(expired_id);
        // Aircraft PointerExpired clears its dock (`+0x6CC`) for either
        // control (`0x0041B673..0x0041B67F`).
        let clear_airfield = listener
            .ammo()
            .is_some_and(|ammo| ammo.dock() == Some(expired_id));

        // Steam gamemd, TechnoPointerExpired7078B2..7078DB:
        // Transporter+11C survives a live hull's ordinary Limbo announcement,
        // for either control. Clear only on Object+90==0, exact Health==0 or
        // ScenarioInit(A8E7AC)!=0. Negative live HP is not this zero test.
        let clear_passenger_role = listener.passenger_role.inside_transport_id()
            == Some(expired_id)
            && (!expired_object_alive
                || expired_health == 0
                || self.object_placement_scope_active());
        let clear_c4_source = listener
            .pending_c4_detonation
            .pending()
            .as_ref()
            .is_some_and(|pending| pending.source_entity_id() == Some(expired_id));
        // `0x004D9AC9..0x004D9B43`, on either control: an attack-move's
        // MegaTarget becomes a MegaDestination at its GetCoords cell, and its
        // MegaDestination is dropped.
        let expire_mega_pointer = listener.mega_mission.references(expired_id);
        // Building44EA77 removes a charger for either control, irrespective
        // of the base target exemptions. Only later AI changes the weapon latch.
        let clear_tesla_charger = listener.tesla_charge.chargers().contains(&expired_id);
        // BuildingPointerExpired44E932..44E93A clears its owned child for
        // either control, before that storage is physically removed.
        let clear_spotlight = listener.category() == EntityCategory::Structure
            && listener.building_light.light() == Some(expired_id);
        if !(clear_suspended_target
            || clear_spark_system
            || clear_damage_smoke
            || clear_archive_target
            || clear_suspended_nav_com
            || clear_nav_com
            || prune_nav_list
            || prune_nav_queue
            || clear_dock
            || clear_airfield
            || clear_passenger_role
            || clear_c4_source
            || expire_mega_pointer
            || clear_tesla_charger
            || clear_spotlight)
        {
            return;
        }

        let listener = self
            .substrate
            .entities
            .get_mut(listener_id)
            .expect("expiry listener read above");
        if clear_suspended_target {
            listener.suspended_attack_target.clear();
        }
        if clear_damage_smoke {
            listener.damage_smoke_system_id.forget();
        }
        if clear_spark_system {
            listener.spark_particle_system.forget();
        }
        if clear_archive_target {
            listener.set_archive_target(None);
        }
        if clear_suspended_nav_com {
            listener.navigation.clear_suspended_nav_com();
        }
        if clear_nav_com {
            listener.navigation.expire_nav_com();
        }
        if prune_nav_list {
            listener
                .navigation
                .retain_nav_list(|target| *target != expired_nav);
        }
        if prune_nav_queue {
            listener
                .navigation
                .retain_nav_queue(|target| *target != expired_nav);
        }
        if clear_dock {
            // `0x00707AF5`.
            listener.pending_entry.expire(expired_id);
        }
        if clear_airfield && let Some(ammo) = listener.ammo_mut() {
            ammo.set_dock(None);
        }
        if clear_passenger_role {
            listener.passenger_role.clear_carrier_relation();
        }
        if clear_c4_source {
            listener.pending_c4_detonation.forget_source();
        }
        if expire_mega_pointer {
            listener
                .mega_mission
                .pointer_expired(expired_id, expired_get_coords_cell);
        }
        if clear_tesla_charger {
            listener.tesla_charge.pointer_expired(expired_id);
        }
        if clear_spotlight {
            listener.building_light.take();
        }
    }

    /// ObjectClass::Detach_From_All_Lists represented listener broadcast.
    ///
    /// The callback pass runs before removing the target from its store.
    /// A never-Fired Bullet is still in Limbo; ordinary Object UnInit reaches
    /// this boundary while its target is alive and marked. Listener UnInit may
    /// queue deletion, but final erasure waits for the common drain, retaining
    /// the construction-order cursor through nested callbacks.
    ///
    /// gamemd-derived: active YR `DispatchPointerExpiredCleanup @ 0x007258D0`
    /// is called directly by `ObjectClass__UnInit @ 0x005F65F0` and by
    /// `ObjectClass::Detach_All @ 0x005F5280` (the killing hit's Destroy, the
    /// virtual Conceal path and the Stun).
    /// Every listener uses the live MapClass authority, including when a receiver
    /// has temporarily lent that grid through UninitContext. The remaining arms
    /// read the object's liveness, health and mission from the same world.
    pub(crate) fn notify_pointer_expired(&mut self, expired_id: u64, context: UninitContext<'_>) {
        if !self.substrate.spotlights.contains_key(expired_id)
            && !self.substrate.entities.contains(expired_id)
            && !self.substrate.particle_systems.contains_key(expired_id)
            && self
                .substrate
                .particle_systems
                .detached_particle(expired_id)
                .is_none()
            && !self.terrain_objects.objects().contains_key(&expired_id)
            && self.projectiles.get(expired_id).is_none()
            && self
                .smudge_grid
                .as_ref()
                .and_then(|grid| grid.object(expired_id))
                .is_none()
        {
            return;
        }

        #[cfg(any(test, feature = "test-support"))]
        if let Some(tree) = self.terrain_objects.objects().get(&expired_id) {
            self.trace_lifecycle_for_test(LifecycleTestEvent::TerrainExpiryBoundary {
                stable_id: expired_id,
                cell: tree.cell(),
                indexed: self.terrain_objects.cell_index().get(&tree.cell()) == Some(&expired_id),
                in_logic: tree.in_logic_vector(),
                live: tree.is_live(),
            });
        }

        // gamemd-derived: the House pointer-expiry handler reached by this
        // synchronous broadcast stable-removes a BuildConst Building while it
        // is still alive, marked, and resolvable. Keeping the House mutation
        // at this callback boundary also covers direct UnInit/destruction;
        // Conceal's already-limbo return never reaches it.
        Self::remove_build_const_from_owner(&self.substrate, &mut self.houses, expired_id);
        Self::remove_house_base_membership(&self.substrate, &mut self.houses, expired_id);
        // `TechnoClass::PointerExpired @ 0x0070785F..0x0070792A` (and the
        // death arm of `ReceiveDamage @ 0x0070206A`): both halves of a drain
        // link drop when either object expires.
        crate::sim::credit_income::clear_drain_links_on_expiry(self, expired_id, context.rules);
        self.broadcast_pointer_expired(expired_id, PointerExpiryControl::Uninit, context);
    }

    /// `DispatchPointerExpiredCleanup @ 0x007258D0` with control **0**, as
    /// `FootClass::Crash` calls it directly (`0x004DED4F`). `Detach_All(false)`
    /// ([`Self::detach_all`]) ends with the same announcement. It runs the same
    /// `TechnoClass::PointerExpired` body as the UnInit path over the same
    /// registered-object roster. The detaching object survives, so no
    /// House-side BuildConst removal happens here.
    ///
    /// **The non-Techno listeners on that roster are control-INSENSITIVE**, read
    /// this session and no longer a residual:
    ///
    /// * `WaveClass::PointerExpired @ 0x0075F610` (vtable `0x007F6BF4 + 0x28`)
    ///   and `ParticleSystemClass::PointerExpired @ 0x0062FE90` branch on the
    ///   control only inside the inherited `ObjectClass` call.
    /// * `AnimClass`'s `+0x28` override is `0x00425150` (vtable
    ///   `0x007E3354 + 0x28`; its only xref is that slot). It too forwards the
    ///   control to `ObjectClass::PointerExpired` and takes no further branch on
    ///   it.
    /// * `BulletClass::PointerExpired @ 0x004684E0` branches on the control only
    ///   for its trailing global-vector erase; the `+0x10C` target repair that
    ///   substitutes `Get_CellClass` at the expired object's last cell — the arm
    ///   `ProjectileStore::pointer_expired` receives — is unguarded.
    /// * `ObjectClass::PointerExpired @ 0x005F5230` itself gates only the
    ///   `+0x30` chain relink on a nonzero control; VERA models no `+0x30`
    ///   chain, so it is correct by omission.
    ///
    /// So a torpedo already in flight at a diving submarine falling to the sub's
    /// last cell instead of tracking it is native behavior, not an approximation.
    ///
    /// The victim's FootClass parasite forward is control-insensitive too, so a
    /// cloaking host releases its parasite; the rules place the released owner.
    pub fn detach_all_pointer_expired(
        &mut self,
        expired_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.announce_detach_all(
            expired_id,
            UninitContext::new(rules, registry).with_effects(effects),
        );
    }

    /// [`Self::detach_all_pointer_expired`] under a caller's context.
    fn announce_detach_all(&mut self, expired_id: u64, context: UninitContext<'_>) {
        if self.substrate.entities.contains(expired_id) {
            self.broadcast_pointer_expired(expired_id, PointerExpiryControl::DetachAll, context);
        }
    }

    /// Teleport Process7193CC..7193F4 calls Bullet::LoseTarget468430 on every
    /// matching Bullet in reverse construction order, including non-Logic
    /// bullets. Stable IDs retain that construction order in the store.
    pub(super) fn lose_projectile_targets(&mut self, target_id: u64, rules: &RuleSet) {
        if !self
            .projectiles
            .iter()
            .any(|(_, bullet)| bullet.target().object_id() == Some(target_id))
        {
            return;
        }
        // Every matching Bullet queries the same unchanged object. Resolve
        // once: no RNG/timer writes occur, and repeated dummy stamps agree.
        let replacement = self.projectile_lost_target(target_id, UninitContext::new(rules, None));
        self.projectiles.lose_targets(target_id, replacement);
    }

    /// Shared target repair in LoseTarget468430 and PointerExpired468517.
    /// Both read +48, pack signed cells, then test A8E7AC, IsInAir, and the
    /// empty-cell sentinel before Map5657A0. The map owns aliasing and Dummy.
    fn projectile_lost_target(
        &self,
        target_id: u64,
        context: UninitContext<'_>,
    ) -> ProjectileTarget {
        let entity = self.substrate.entities.get(target_id);
        let xy = entity
            .map(crate::sim::movement::ground_pose::object_center_xy)
            .or_else(|| {
                self.substrate
                    .particle_systems
                    .get(target_id)
                    .map(|system| [system.coords().x, system.coords().y])
            })
            .or_else(|| {
                self.terrain_objects.objects().get(&target_id).map(|tree| {
                    let coord = tree.world_coord();
                    [coord.x, coord.y]
                })
            })
            .or_else(|| {
                self.smudge_grid
                    .as_ref()?
                    .object(target_id)
                    .map(|smudge| [smudge.location().x, smudge.location().y])
            })
            .or_else(|| {
                self.spotlight_object_coord(target_id)
                    .map(|[x, y, _]| [x, y])
            });
        let Some([x, y]) = xy else {
            return ProjectileTarget::None;
        };
        let (rx, ry) = (
            crate::util::lepton::lepton_to_cell_packed(x),
            crate::util::lepton::lepton_to_cell_packed(y),
        );
        let terrain = context.terrain().or(self.resolved_terrain.as_ref());
        if self.object_placement_scope_active()
            || entity.is_some_and(|entity| {
                crate::sim::movement::air_movement::is_high_flying(
                    entity,
                    terrain,
                    Some((context.rules(), &self.interner)),
                )
            })
            || self
                .terrain_objects
                .objects()
                .get(&target_id)
                .is_some_and(|tree| tree.is_in_air(terrain))
            || (rx, ry) == NULL_TARGET_CELL_SENTINEL
        {
            return ProjectileTarget::None;
        }
        // Real worlds query MapClass and retain its returned identity.
        ProjectileTarget::looked_up_cell(terrain, rx as u16, ry as u16)
    }

    /// Anim4's dedicated holder announcement725A16..725A4C, before Anim
    /// conceal/removal (UnInit42561F, destructor422906). It does not enter
    /// the generic Object/bomb/factory/team broadcast. This dispatches the
    /// Bullet, Parasite and Ares EMP-holder subsets; Building receivers stay at their existing boundaries,
    /// including their distinct ordinary-UnInit versus scalar slot handling.
    pub(crate) fn notify_anim_pointer_expired(&mut self, expired_id: u64) {
        if !self.substrate.anims.contains_key(expired_id) {
            return;
        }
        let holders = self
            .expiry_listeners
            .entities_reached_by(&mut self.substrate.entities, expired_id);
        for owner in holders {
            if let Some(entity) = self.substrate.entities.get_mut(owner) {
                entity.emp_anim_pointer_expired(expired_id);
                #[cfg(feature = "ares")]
                entity.attached_effect_anim_expired(expired_id);
            }
            self.substrate
                .parasite_anim_pointer_expired(owner, expired_id);
        }
        let acting = self
            .expiry_listeners
            .projectiles_reached_by(&mut self.projectiles, expired_id);
        if crate::sim::touch_log::live_read_check_enabled() {
            let expected: Vec<u64> = self
                .projectiles
                .iter()
                .filter(|(_, bullet)| bullet.expiry_names().contains(&Some(expired_id)))
                .map(|(&id, _)| id)
                .collect();
            assert_eq!(
                acting, expected,
                "Anim expiry listeners diverged from a live read"
            );
        }
        for id in acting {
            // A held Anim cannot be a Techno/terrain target or firer. The
            // expired RTTI is Anim4, so the Scalable Bullet arm stays off.
            self.projectiles
                .pointer_expired(id, expired_id, ProjectileTarget::None, false)
                .expect("indexed Bullet remains stored during Anim expiry");
        }
    }

    /// The pointer-expiry listener index, with the stores it reads.
    fn expiry_listener_index(&mut self) -> (&mut ExpiryListeners, ListenerStores<'_>) {
        (
            &mut self.expiry_listeners,
            ListenerStores {
                entities: &mut self.substrate.entities,
                anims: &mut self.substrate.anims,
                particle_systems: &mut self.substrate.particle_systems,
                projectiles: &mut self.projectiles,
                waves: &mut self.waves,
            },
        )
    }

    fn broadcast_pointer_expired(
        &mut self,
        expired_id: u64,
        control: PointerExpiryControl,
        context: UninitContext<'_>,
    ) {
        // VERA-only: a batched target scan holds its pick across the turns
        // before its scanner's as a TarCom does (`techno_ai::scan_batch`).
        self.release_batched_scan_pick(expired_id);
        // `DispatchPointerExpiredCleanup @ 0x007258D0` calls
        // `BombListClass::PointerGotInvalid @ 0x00439150` after its listener
        // loop (0x00725961) whatever the control, so a cloaking planter loses
        // its bombs' credit as a removed one does. No listener reads a planter,
        // so running it first changes nothing.
        Self::bomb_planter_expired(&mut self.substrate, &self.bombs, expired_id);
        // `TeamClass::PointerExpired @ 0x006EAE60`, each team a listener.
        self.team_script_vm
            .pointer_expired(expired_id, control == PointerExpiryControl::Uninit);
        // FactoryClass::PointerExpired4CA580 is a control-insensitive
        // listener in Dispatch7258D0. Its only mutation clears a matching
        // held product; no entity listener consumes that factory pointer.
        self.production.factories_mut().pointer_expired(expired_id);
        // `HouseClass::PointerExpired @ 0x004FB9B0`, each house a listener
        // (`0x004F5FFB`), forgets House+54E0 whatever the control.
        for house in self.houses.values_mut() {
            house.forget_lost_building(expired_id);
        }
        let Some((
            expired_target_cell,
            expired_object_alive,
            expired_health,
            expired_is_selling,
            expired_owner,
        )) = self
            .substrate
            .entities
            .get(expired_id)
            .map(|expired| {
                (
                    object_get_coords_cell(expired),
                    expired.lifecycle.object_alive,
                    expired.health.current(),
                    expired.mission.current().known()
                        == Some(crate::sim::mission::MissionType::Selling),
                    Some(expired.owner()),
                )
            })
            .or_else(|| {
                let [x, y, _] = self.spotlight_object_coord(expired_id)?;
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(y)).ok());
                // ObjectCtor5F390B supplies255; the class has no Techno owner.
                Some((cell, self.spotlight_alive(expired_id)?, 255, false, None))
            })
            .or_else(|| {
                let system = self.substrate.particle_systems.get(expired_id)?;
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(system.coords().x))
                    .ok()
                    .zip(
                        u16::try_from(crate::util::lepton::lepton_to_cell(system.coords().y)).ok(),
                    );
                // ObjectCtor5F390B..5F3919 supplies Health=255. A System
                // has no Techno flags, selling mission or Techno owner;
                // its retained coordinates and liveness are still readable
                // during ObjectLimbo's synchronous expiry announcement.
                Some((cell, system.alive(), 255, false, None))
            })
            .or_else(|| {
                let (particle, alive) = self
                    .substrate
                    .particle_systems
                    .detached_particle(expired_id)?;
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(particle.coords().x))
                    .ok()
                    .zip(
                        u16::try_from(crate::util::lepton::lepton_to_cell(particle.coords().y))
                            .ok(),
                    );
                Some((cell, alive, 255, false, None))
            })
            .or_else(|| {
                let bullet = self.projectiles.get(expired_id)?;
                let point = bullet.position();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(point.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(point.y)).ok());
                Some((cell, true, bullet.payload().base_damage, false, None))
            })
            .or_else(|| {
                let tree = self.terrain_objects.objects().get(&expired_id)?;
                let coord = tree.world_coord();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(coord.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(coord.y)).ok());
                Some((cell, tree.object_alive(), tree.health(), false, None))
            })
            .or_else(|| {
                let smudge = self.smudge_grid.as_ref()?.object(expired_id)?;
                let location = smudge.location();
                let cell = u16::try_from(crate::util::lepton::lepton_to_cell(location.x))
                    .ok()
                    .zip(u16::try_from(crate::util::lepton::lepton_to_cell(location.y)).ok());
                Some((cell, smudge.object_alive(), smudge.health(), false, None))
            })
        else {
            return;
        };

        // BuildingLight436A00 clears parent/target for either expiry control.
        // These nullable-field writes commute with the other listener clears.
        self.substrate.spotlights_pointer_expired(expired_id);

        #[cfg(any(test, feature = "test-support"))]
        if let Some(smudge) = self
            .smudge_grid
            .as_ref()
            .and_then(|grid| grid.object(expired_id))
        {
            let location = smudge.location();
            self.trace_lifecycle_for_test(LifecycleTestEvent::SmudgeExpiryBoundary {
                stable_id: expired_id,
                native_id: smudge.native_unique_id(),
                native_cursor: self.native_unique_ids.as_ref().unwrap().current_raw(),
                object_alive: smudge.object_alive(),
                in_limbo: smudge.in_limbo(),
                health: smudge.health(),
                location: [location.x, location.y, location.z],
                pending: self.substrate.pending_delete.object_ids(),
                generic_objects: self
                    .removal_listener_order(self.substrate.entities.keys_sorted())
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect(),
                rng: [
                    self.scenario_rng.native_state_hex(),
                    self.main_rng.native_state_hex(),
                    self.mapgen_rng.native_state_hex(),
                ],
            });
        }

        // `BulletClass::PointerExpired @ 0x004684E0` performs the packed
        // `MapClass::Get_CellClass @ 0x005657A0` lookup only for a matching
        // target. Its result pointer is stored at Bullet+0x10C: an allocated
        // slot therefore remains a stable Cell target, while a miss stores the
        // one shared dummy at `0x00ABDC50`. Later `BulletClass::AI @ 0x004666E0`
        // dispatches that live pointer and observes its most recent coord stamp.

        // Most listeners hold nothing this expiry changes, and the game
        // passes over them (below). The ones that do are found up front in
        // the expiry index. Production builds list only those. Test builds
        // list every listener, each with whether the index found it, and
        // check at each one's turn that a listener it left out changes
        // nothing.
        let (index, stores) = self.expiry_listener_index();
        let acting = index.reached_by(stores, expired_id);
        if crate::sim::touch_log::live_read_check_enabled() {
            let (_, stores) = self.expiry_listener_index();
            assert_eq!(
                acting,
                ExpiryListeners::asked_of_every(&stores, expired_id),
                "the expiry listeners of {expired_id} diverged from a live read"
            );
        }
        // Original4685CC gates the Bullet callback's scalable-list arm on
        // control!=0, expired RTTI8 and its current retained type. Read once,
        // before any listener runs: RuleSet/type remain immutable through this
        // broadcast, the expiring Bullet stays stored until dtor466581 (its
        // own registered subscriber is one of the Bullets), and Unit741439
        // only enrolls a fresh returned Bullet, never this old expired
        // identity. With the arm on and the Bullet enrolled, every present
        // Bullet subscriber removes one enrolled duplicate, so production
        // builds then visit every Bullet, not only those the index finds.
        // Then retain the proven lack of further matches.
        let mut scalable_expiry_pending = control == PointerExpiryControl::Uninit
            && self
                .projectiles
                .get(expired_id)
                .and_then(|bullet| bullet.payload().projectile_type())
                .and_then(|type_id| context.rules().projectile(self.interner.resolve(type_id)))
                .is_some_and(|kind| kind.scalable)
            && self.projectiles.scalable_enrolled(expired_id);
        #[cfg(any(test, feature = "test-support"))]
        let listeners: Vec<(u64, ObjectKind, bool)> = {
            let found: std::collections::BTreeSet<u64> = acting.iter().map(|&(id, _)| id).collect();
            self.removal_listener_order(self.substrate.entities.keys_sorted())
                .into_iter()
                .map(|(id, kind)| (id, kind, found.contains(&id)))
                .collect()
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let listeners: Vec<(u64, ObjectKind, bool)> = {
            let mut listeners: Vec<(u64, ObjectKind, bool)> = acting
                .into_iter()
                .map(|(id, kind)| (id, kind, true))
                .collect();
            if scalable_expiry_pending {
                listeners.extend(
                    self.projectiles
                        .iter()
                        .map(|(&id, _)| (id, ObjectKind::Projectile, false)),
                );
                // Stable: a Bullet the index found keeps its first entry.
                listeners.sort_by_key(|&(id, ..)| id);
                listeners.dedup_by_key(|&mut (id, ..)| id);
            }
            listeners
        };
        for (listener_id, kind, found) in listeners {
            // A callback may have removed a later listener; an ID never
            // moves to another store. An entity listener is read once, for
            // its presence and for whether it holds anything this expiry
            // changes.
            let mut entity_acts = false;
            let present = match kind {
                ObjectKind::Entity => {
                    self.substrate
                        .entities
                        .get(listener_id)
                        .is_some_and(|listener| {
                            entity_acts = Self::entity_expiry_listener_acts(
                                listener,
                                listener_id,
                                expired_id,
                            );
                            true
                        })
                }
                ObjectKind::Anim => self.substrate.anims.contains_key(listener_id),
                ObjectKind::ParticleSystem => {
                    self.substrate.particle_systems.contains_key(listener_id)
                }
                ObjectKind::Projectile => self.projectiles.get(listener_id).is_some(),
                ObjectKind::Wave => self.waves.get(listener_id).is_some(),
                ObjectKind::Smudge => self
                    .smudge_grid
                    .as_ref()
                    .and_then(|grid| grid.object(listener_id))
                    .is_some(),
                // Not on the roster ([`ObjectKind`]'s pointer-expiry column).
                ObjectKind::VoxelAnim
                | ObjectKind::Particle
                | ObjectKind::Terrain
                | ObjectKind::Spotlight => false,
            };
            if !present {
                continue;
            }
            #[cfg(any(test, feature = "test-support"))]
            assert!(
                !entity_acts || found,
                "listener {listener_id} came to hold {expired_id} during its expiry broadcast; \
                 production builds leave it out"
            );

            #[cfg(any(test, feature = "test-support"))]
            if control == PointerExpiryControl::Uninit {
                let (target_alive, target_in_limbo) = self
                    .substrate
                    .entities
                    .get(expired_id)
                    .map(|target| (target.lifecycle.object_alive, target.lifecycle.in_limbo))
                    .or_else(|| {
                        self.substrate
                            .particle_systems
                            .get(expired_id)
                            .map(|system| (system.alive(), system.in_limbo()))
                    })
                    .or_else(|| {
                        self.substrate
                            .particle_systems
                            .detached_particle(expired_id)
                            .map(|(_, alive)| (alive, !alive))
                    })
                    .or_else(|| {
                        self.terrain_objects
                            .objects()
                            .get(&expired_id)
                            .map(|tree| (tree.object_alive(), !tree.is_live()))
                    })
                    .or_else(|| {
                        self.smudge_grid
                            .as_ref()?
                            .object(expired_id)
                            .map(|object| (object.object_alive(), object.in_limbo()))
                    })
                    .or_else(|| {
                        self.projectiles
                            .get(expired_id)
                            .map(|bullet| (true, !bullet.in_logic_vector()))
                    })
                    .or_else(|| {
                        self.spotlight_alive(expired_id)
                            .map(|alive| (alive, !alive))
                    })
                    .unwrap_or((false, true));
                self.trace_lifecycle_for_test(LifecycleTestEvent::UninitRemovalListenerVisited {
                    expired_id,
                    listener_id,
                    target_alive,
                    target_in_limbo,
                });
            }

            if kind == ObjectKind::Entity {
                // Most entity listeners hold nothing this expiry changes, and
                // the game passes over them. Test builds visit them as well,
                // and check below that their forwards handed out no entity and
                // drew no random number.
                if !entity_acts && !cfg!(any(test, feature = "test-support")) {
                    continue;
                }
                #[cfg(any(test, feature = "test-support"))]
                let passed_over = (!entity_acts)
                    .then(|| (self.substrate.entities.hand_outs(), self.rng_cursors()));
                self.notify_entity_pointer_expired(
                    listener_id,
                    expired_id,
                    expired_target_cell,
                    expired_object_alive,
                    expired_health,
                    expired_is_selling,
                    expired_owner,
                    control,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
                // `TechnoClass::PointerExpired` forwards to the listener's
                // SpawnManager: `0x00707B24 CALL 0x006B7C60`, gated only on
                // `+0x2D0 != 0`. This is the only mechanism that drops a
                // destroyed wing target, so without it a Carrier keeps sending
                // its Hornets at a corpse. The forward sits OUTSIDE the control
                // test, so it runs on a cloak dive as well as on UnInit.
                crate::sim::spawn_manager::notify_pointer_expired(
                    self,
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
                // The CaptureManager forward — `0x00707B14 CALL 0x00471F90` —
                // sits inside the `if (control != 0)` block opened at
                // `0x00707AE7`, so a dive leaves mind-control links alone while
                // a death drops them. It changes nothing unless a node names
                // the expiring object, so only then is the listener handed out.
                if control == PointerExpiryControl::Uninit
                    && self
                        .substrate
                        .entities
                        .get(listener_id)
                        .and_then(|entity| entity.capture_manager.as_ref())
                        .is_some_and(|manager| manager.victims().any(|victim| victim == expired_id))
                    && let Some(manager) = self
                        .substrate
                        .entities
                        .get_mut(listener_id)
                        .and_then(|entity| entity.capture_manager.as_mut())
                {
                    manager.pointer_expired(expired_id);
                }
                // The TemporalClass forward (`0x00707B34` -> `0x0071AB60`)
                // follows, outside the control test like the SpawnManager's.
                self.temporal_pointer_expired(
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.effects(),
                );
                self.airstrike_pointer_expired(
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.effects(),
                );
                // Then the Magnetron links (`0x00707B64..0x00707BAC`), also
                // outside the control test.
                self.locomotor_link_pointer_expired(
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
                // Techno707BCE..707CA4 first-removes one candidate/paired
                // score and one history occurrence, outside control/allowClear.
                // This must precede the subclass Foot parasite forward.
                if let Some(entity) = self.substrate.entities.get_mut_if(listener_id, |entity| {
                    entity
                        .distributed_fire_references()
                        .any(|id| id == expired_id)
                }) {
                    entity.distributed_fire_pointer_expired(expired_id);
                }
                // FootClass::PointerExpired 0x004D998C..0x004D99CD follows the
                // Techno body: the parasite link and its forward.
                self.foot_parasite_pointer_expired(
                    listener_id,
                    expired_id,
                    context.rules(),
                    context.effects(),
                );
                #[cfg(any(test, feature = "test-support"))]
                if let Some(before) = passed_over {
                    assert_eq!(
                        before,
                        (self.substrate.entities.hand_outs(), self.rng_cursors()),
                        "the expiry of {expired_id} changed listener {listener_id}, which \
                         entity_expiry_listener_acts passes over"
                    );
                }
            } else if kind == ObjectKind::Anim {
                let changed = self.anim_pointer_expired(listener_id, expired_id, context.rules());
                debug_assert!(
                    found || !changed,
                    "the expiry of {expired_id} changed anim {listener_id}, which the expiry \
                     index passes over"
                );
            } else if kind == ObjectKind::ParticleSystem {
                let changed = self
                    .substrate
                    .particle_systems
                    .pointer_expired(listener_id, expired_id);
                debug_assert!(
                    found || !changed,
                    "the expiry of {expired_id} changed particle system {listener_id}, which the \
                     expiry index passes over"
                );
            } else if kind == ObjectKind::Projectile {
                let target_matches = self
                    .projectiles
                    .get(listener_id)
                    .is_some_and(|projectile| projectile.target().object_id() == Some(expired_id));
                let projectile_replacement_target = if target_matches {
                    self.projectile_lost_target(expired_id, context)
                } else {
                    ProjectileTarget::None
                };
                let (changed, remaining) = self
                    .projectiles
                    .pointer_expired(
                        listener_id,
                        expired_id,
                        projectile_replacement_target,
                        scalable_expiry_pending,
                    )
                    .expect("present Bullet subscriber");
                scalable_expiry_pending = remaining;
                debug_assert!(
                    found || !changed,
                    "the expiry of {expired_id} changed bullet {listener_id}, which the expiry \
                     index passes over"
                );
                #[cfg(any(test, feature = "test-support"))]
                if let Some((source_id, target)) = self
                    .projectiles
                    .get(listener_id)
                    .map(|projectile| (projectile.source_id(), projectile.target()))
                {
                    self.trace_lifecycle_for_test(
                        LifecycleTestEvent::ProjectilePointerExpiredVisited {
                            expired_id,
                            projectile_id: listener_id,
                            expired_resolvable: self.classify_object(expired_id).is_some(),
                            projectile_resolvable: true,
                            source_id,
                            target,
                        },
                    );
                }
            } else if kind == ObjectKind::Wave {
                let (owner_cleared, target_cleared) = self
                    .waves
                    .pointer_expired(listener_id, expired_id)
                    .expect("Wave listener disappeared during expiry callback");
                debug_assert!(
                    found || !(owner_cleared || target_cleared),
                    "the expiry of {expired_id} changed wave {listener_id}, which the expiry \
                     index passes over"
                );
                if owner_cleared && self.active_wave_links.get(&expired_id) == Some(&listener_id) {
                    // TechnoClass keeps the Wave link through the dying/deferred
                    // interval. Once the exact owner pointer expires, retaining
                    // this Rust projection would serialize a link whose Wave
                    // owner is now null.
                    self.active_wave_links.remove(&expired_id);
                }
            } else {
                // The one roster kind with no callback: a smudge's
                // PointerExpired clears nothing it holds.
                debug_assert_eq!(
                    kind,
                    ObjectKind::Smudge,
                    "{listener_id} has no expiry callback"
                );
            }
        }
        // The kamikaze tracker's Remove (`0x00725972`) follows the listeners
        // and the BombList; a SpawnManager's slot guard reads the membership
        // before it.
        self.kamikaze.remove(expired_id);
        //725977..725993: DiskLaser4A7900 sees every expired pointer after
        // Kamikaze and before the Techno navigation handoff. It queues a shot
        // without clearing its retained pointers or deleting copied beams.
        self.detach_disk_lasers(expired_id);
        // Dispatch7258D0's Techno-bit gate reaches733160 at7259B1 after
        // Kamikaze Remove. Executed controls: input_oracle/selection_navigation.
        // All typed GameEntity classes carry Abstract+14's Techno bit. Smudge
        // does not, so it must not produce this handoff. It precedes native
        // Tactical/Logic expiry and eventual DisplayRemove/Alive writes.
        if expired_owner.is_some() {
            self.lifecycle_outputs
                .emit(LifecycleOutput::ObjectPointerExpired {
                    stable_id: expired_id,
                });
        }
    }

    /// ObjectClass::UnInit represented ordering on the rules an empty INI
    /// reads to.  Physical removal is deferred.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn uninit(&mut self, stable_id: u64) {
        self.uninit_with_context(stable_id, UninitContext::default());
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn uninit_with_rules(&mut self, stable_id: u64, rules: &RuleSet) {
        self.uninit_with_context(stable_id, UninitContext::with_rules(rules));
    }

    pub fn uninit_with_context(&mut self, stable_id: u64, context: UninitContext<'_>) {
        self.planning_detach_actor(stable_id);
        let kind = match self.classify_object(stable_id) {
            Some(kind @ (ObjectKind::Entity | ObjectKind::Spotlight | ObjectKind::Smudge)) => kind,
            Some(ObjectKind::Particle)
                if self
                    .substrate
                    .particle_systems
                    .detached_particle(stable_id)
                    .is_some() =>
            {
                ObjectKind::Particle
            }
            // A particle still in its system retires with it; the other kinds
            // retire through their own owners, never this UnInit.
            Some(
                ObjectKind::Particle
                | ObjectKind::Anim
                | ObjectKind::VoxelAnim
                | ObjectKind::ParticleSystem
                | ObjectKind::Terrain
                | ObjectKind::Projectile
                | ObjectKind::Wave,
            )
            | None => return,
        };
        // Ordinary Particle and Smudge instances have no Techno/Foot hooks
        // or passengers. Every represented family joins the one base tail.
        if let Some(entity) = self
            .substrate
            .entities
            .get(stable_id)
            .filter(|_| kind == ObjectKind::Entity)
        {
            // `FootClass::UnInit @ 0x004DE5DD` frees a controller's captives
            // first (a crushed or otherwise removed Yuri or Mastermind).
            let foot = matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            );
            if foot && entity.capture_manager.is_some() {
                self.free_all_captures(
                    stable_id,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
            // `0x004DE5F0`: a Magnetron lets go of what it holds.
            if foot
                && self
                    .substrate
                    .entities
                    .get(stable_id)
                    .is_some_and(|entity| entity.lift.target().is_some())
            {
                self.release_locomotor(
                    stable_id,
                    true,
                    context.rules(),
                    context.registry(),
                    context.effects(),
                );
            }
            // `0x004DE604`: then the object leaves its team.
            if foot {
                self.leave_team(stable_id, false, context.rules(), context.effects());
            }

            self.run_represented_uninit_pre_hook(stable_id);
            // `ObjectClass::UnInit` kills a Techno's passengers with no attacker
            // (`0x005F660D`).
            self.kill_passengers(stable_id, None, context);

            #[cfg(any(test, feature = "test-support"))]
            {
                let (object_alive, cell_marked) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .map(|entity| (entity.lifecycle.object_alive, entity.lifecycle.cell_marked))
                    .unwrap_or((false, false));
                self.trace_lifecycle_for_test(LifecycleTestEvent::UninitRemovalNotifyBoundary {
                    stable_id,
                    object_alive,
                    cell_marked,
                    resolvable: self.substrate.entities.contains(stable_id),
                });
            }
        }
        // ObjectClass::UnInit's first statement (`0x005F65F3`) defuses a
        // carried bomb silently: sold, crushed, erased.
        Self::bomb_defuse(&mut self.substrate, &mut self.bombs, stable_id);
        self.notify_pointer_expired(stable_id, context);

        match kind {
            ObjectKind::Spotlight => {
                self.notify_pointer_expired(stable_id, context);
                self.finish_spotlight_retirement(stable_id);
            }
            ObjectKind::Smudge => {
                // Whole Smudge6B4A50 delegates to Object5F65F0. Its Mark(0)
                // writes no cell data; preserve its existing fused Conceal tail.
                self.notify_pointer_expired(stable_id, context);
                self.smudge_grid.as_mut().unwrap().finish_uninit(stable_id);
            }
            ObjectKind::Particle => {
                let _ = self.object_conceal_with_context(stable_id, context);
                self.substrate
                    .particle_systems
                    .finish_particle_retirement(stable_id);
            }
            ObjectKind::Entity => {
                let _ = self.techno_limbo_with_context(stable_id, context);
                if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
                    entity.lifecycle.object_alive = false;
                    entity.death.begin();
                }
            }
            ObjectKind::Anim
            | ObjectKind::VoxelAnim
            | ObjectKind::ParticleSystem
            | ObjectKind::Terrain
            | ObjectKind::Projectile
            | ObjectKind::Wave => unreachable!("{kind:?} {stable_id} never reaches UnInit"),
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::UninitAliveCleared { stable_id });

        // Native append has no duplicate suppression.  The drain collapses all
        // occurrences when this dead object becomes the selected ready entry.
        self.substrate.pending_delete.queue_object(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn despawn_entity(&mut self, stable_id: u64) {
        self.uninit(stable_id);
    }

    /// Particle systems stay resolvable until the ordinary common late drain.
    /// Their owned particles have already emptied before this transition.
    /// SystemAI62FDC7..62FDE7 calls ObjectLimbo5F4D30 while Alive is still
    /// set, then clears Alive and queues deletion. Limbo's literal control1
    /// dispatch clears Techno+308/+310 synchronously, before the late destructor.
    pub(crate) fn retire_particle_system(&mut self, stable_id: u64, context: UninitContext<'_>) {
        let ready = self
            .substrate
            .particle_systems
            .get(stable_id)
            .is_some_and(|system| {
                system.alive() && system.done_spawning() && system.particles().is_empty()
            });
        if !ready {
            return;
        }

        if !self
            .substrate
            .particle_systems
            .get(stable_id)
            .unwrap()
            .in_limbo()
        {
            self.notify_pointer_expired(stable_id, context);
            self.conceal_particle_system(stable_id);
        }
        self.substrate.particle_systems.enter_limbo(stable_id);
        self.substrate.pending_delete.queue_object(stable_id);
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteQueued { stable_id });
    }

    /// A queued object's IsReadyToDelete (vt+0x44), by kind. An ID no store
    /// holds is ready.
    fn pending_object_is_ready(&self, stable_id: u64) -> bool {
        let Some(kind) = self.classify_object(stable_id) else {
            return true;
        };
        match kind {
            ObjectKind::Spotlight => self.spotlight_alive(stable_id) == Some(false),
            ObjectKind::Smudge => self
                .smudge_grid
                .as_ref()
                .and_then(|grid| grid.object(stable_id))
                .is_some_and(|smudge| !smudge.object_alive()),
            ObjectKind::Entity => self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| !entity.lifecycle.object_alive),
            // Every ordinary Anim retirement Conceals before queueing. Owner
            // expiry425196 only sets19B and leaves Logic live until its own AI;
            // that byte is not ObjectUnInit5F6625's readiness state.
            ObjectKind::Anim => self
                .substrate
                .anims
                .get(stable_id)
                .is_some_and(|anim| !anim.in_logic_vector()),
            // SystemVT7EFB9C+44 -> IsReadyToDelete62FE60: done+F8 and
            // count+CC==0, independently of the ObjectAlive byte.
            ObjectKind::ParticleSystem => self
                .substrate
                .particle_systems
                .get(stable_id)
                .is_some_and(|system| system.done_spawning() && system.particles().is_empty()),
            // Particle+44 inherits Object5F6690: !ObjectAlive (+0x90). Only a
            // detached particle is queued; one still in its system is alive.
            ObjectKind::Particle => self
                .substrate
                .particle_systems
                .detached_particle(stable_id)
                .is_some_and(|(_, alive)| !alive),
            ObjectKind::Terrain => self
                .terrain_objects
                .objects()
                .get(&stable_id)
                .is_some_and(|terrain| !terrain.is_live() && !terrain.in_logic_vector()),
            ObjectKind::Projectile => self
                .projectiles
                .get(stable_id)
                .is_some_and(|projectile| !projectile.in_logic_vector()),
            ObjectKind::Wave => self
                .waves
                .get(stable_id)
                .is_some_and(|wave| !wave.in_logic_vector()),
            // Never queued: its AI's Delete verdict removes it at once
            // (`visit_voxel_anim`).
            ObjectKind::VoxelAnim => true,
        }
    }

    /// Shared physical destructor owner, used by deferred retirement and
    /// constructor-complete limbo disposal. Neither caller repeats UnInit or
    /// books a loss here. Rules stay available to synchronous expiry receivers.
    pub(super) fn finalize_and_remove_common(
        &mut self,
        stable_id: u64,
        context: UninitContext<'_>,
        game_state: i32,
    ) {
        let kind = self.classify_object(stable_id);
        if kind == Some(ObjectKind::Spotlight) {
            self.destroy_spotlight(stable_id, context);
            return;
        }
        self.destroy_spotlight_for_parent(stable_id, context);
        let foot = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() != EntityCategory::Structure);
        // Unit7357D6..7357EF asks the existing House CanBuild4F7870 before
        // leaving its Team or removing tracking. AtLimit(-1) requests the
        // existing House+1FC tech-tree refresh; no alternate counter or
        // production admission is introduced. Constructor-complete Full
        // carriers can reach this scalar path without an earlier UnInit.
        // Source: Steam gamemd, Unit destructor735780 body.
        let rules = context.rules();
        if (game_state as u8) != 0
            && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                (entity.category() == EntityCategory::Unit)
                    .then(|| self.object_type(entity.type_ref(), rules))
                    .flatten()
                    .filter(|object| {
                        crate::sim::production::can_build(
                            self,
                            rules,
                            entity.owner(),
                            object,
                            false,
                            false,
                        ) == crate::sim::production::CanBuild::AtLimit
                    })
                    .map(|_| entity.owner())
            })
        {
            self.houses.request_house_tech_tree_recheck(owner);
        }
        // Constructor-complete Unit7357F6..735809, Infantry517DF2..517E05
        // and Aircraft4140EB..4140FE leave their Team before Remove_Tracking.
        // Their gate is the low byte of GameState[A8E9A0], independently of
        // ScenarioStarted[A8ED5C]. Foot4D3597..4D35C8 repeats the same shared
        // RemoveMember6EA870 when a class destructor retained the reference.
        // Source: original gamemd;
        // executed composed action7 cleanup: spatial_oracle/team_creation.
        if foot && (game_state as u8) != 0 {
            // Ares3.0's Unit7357F6/Infantry517DF2/Aircraft4140EB hooks
            // run after the existing GameState/type/build-limit gates and
            // before LeaveTeam/RemoveTracking. Every declared alternate
            // requests a pass, even while another copy remains tracked.
            let rules = context.rules();
            if crate::rules::ARES_ENABLED
                && let Some(owner) = self.substrate.entities.get(stable_id).and_then(|entity| {
                    rules
                        .is_ares_prerequisite_alternate(
                            entity.category().into(),
                            self.interner.resolve(entity.type_ref()),
                        )
                        .then_some(entity.owner())
                })
            {
                self.houses.request_house_tech_tree_recheck(owner);
            }
            self.leave_team(stable_id, false, context.rules(), context.effects());
        }
        if kind == Some(ObjectKind::ParticleSystem) {
            // SystemDtor62E075/62E092 broadcasts control1 before its
            // delegated Limbo62E099 and before leaving object storage.
            // The ordinary done-empty AI already announced its earlier
            // live Limbo; both callbacks use the same retained-reference
            // owner instead of clearing only its producer after removal.
            self.notify_pointer_expired(stable_id, context);
            self.conceal_particle_system(stable_id);
        }
        if kind == Some(ObjectKind::Particle) {
            // ParticleDtor62D9A0: its already-limbo check does no work,
            // then62D9C8 announces again before ObjectDtor5F3B80 unregisters
            // and frees the body. No Particle-specific RNG/timer writes.
            self.notify_pointer_expired(stable_id, context);
        }
        if kind == Some(ObjectKind::Smudge) {
            // Scalar Smudge6B4FA0 sends its third removed=true expiry at
            //6B4FC6 before leaving Smudge/Object registries. Place's cells
            // survive this destructor and Building Mark(0).
            self.notify_pointer_expired(stable_id, context);
        }
        if kind == Some(ObjectKind::Terrain) {
            // Terrain destructor71B7D6 repeats expiry before leaving its
            // registry. This is later than the live lethal/UnInit callbacks.
            self.notify_pointer_expired(stable_id, context);
        }
        if kind == Some(ObjectKind::Projectile) {
            // BulletDtor466581 announces expiry before clearing construction
            // fields or leaving its registry, including never-Fired bullets.
            self.notify_pointer_expired(stable_id, context);
        }
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            // Building destructor43BD27 releases the construction SoundClass
            //(+6A0) before the delegated Techno destructor removes tracking.
            self.sound_events
                .emit(super::SimSoundEvent::ObjectSoundReleased { owner: stable_id });
            // The delegated Techno destructor also tears down the turret
            // rotation loop's `+0x488` handle; this object's own UnInit Limbo
            // released it already, and a structure that never limbos never
            // started a loop, so no second event is published here.
            // Building43BD67 broadcasts removed=true after sound release,
            // even when a limbo object never reached UnInit. The original
            // constructed-cancel controls execute this under Factory's
            // A8E7AC bracket; the ordinary late drain repeats it after UnInit.
            self.notify_pointer_expired(stable_id, context);
            // Building43BDC5 -> ClearAnimSlot451E40(-2) synchronously deletes
            // ordinary21slots, then43BDE0 destroys the distinct eight fire
            // Anims, before Techno tracking removal43BF34. Whole original
            // UnInit/drain control: building_death_anims joined evidence,
            // gamemd.
            self.clear_all_building_anim_slots(stable_id);
            self.clear_building_damage_fire_slots(stable_id, context.rules());
            // Building43BE47..43BEF0 rechecks existing grants when one of its
            // actual457630/457690 providers departs. This is a destructor
            // hold/revoke callback, not an event-time grant pass.
            let rules = context.rules();
            if (game_state as u8) != 0
                && let Some(owner) = self
                    .substrate
                    .entities
                    .get(stable_id)
                    .filter(|entity| {
                        crate::sim::superweapon::provided_supers(self, rules, entity)
                            .next()
                            .is_some()
                    })
                    .map(|entity| entity.owner())
            {
                crate::sim::superweapon::update_owned_supers(self, rules, owner);
            }
            // Active-frame Building43BEF5..43BF11 compares Health+6C with
            // the retained AI sample+544 after its slot/fire destructors.
            // A mismatch dirties power only, without publishing a sample or
            // touching radar, before Remove_Tracking43BF34. Original joined
            // building_death_anims: normal fatal0/38 versus direct0/0.
            let unsampled_owner = self.substrate.entities.get(stable_id).and_then(|entity| {
                entity
                    .building_power_health_sample()
                    .filter(|sample| *sample != entity.health.current())
                    .map(|_| entity.owner())
            });
            if (game_state as u8) != 0
                && let Some(owner) = unsampled_owner
            {
                Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
            }
        }
        // The Techno destructors (`0x0041410B`, `0x0043BF34`, `0x00517E2E`,
        // `0x00735816`) call Remove_Tracking under GameState's low-byte gate.
        // The admitted entity store contains constructor-complete concrete
        // types; the native null-Type branch is outside that representation.
        if (game_state as u8) != 0 {
            Self::update_house_tracking(
                &mut self.substrate,
                &mut self.houses,
                stable_id,
                crate::sim::house_tracking::HouseTracking::remove_tracking,
            );
        }
        // Unit73581B..73584D follows its own RemoveTracking735816 with
        // CargoRemoveFirst473430 -> child's scalarF8, before Unit expiry
        //735892 and the delegated Foot destructor. A8E7AC!=0 skips this
        //loop: PlaceMembers65E5A4's final refusal still holds that scope,
        //whereas its initial NullCell65E12B disposal does not. Ordinary
        //UnInit already drained cargo; constructor-complete Full carriers
        //can reach this scalar owner directly. Do not book a kill/UnInit or
        //use FootRemoveFirst4DE710's gunner reset on this raw Cargo pop.
        // Source: Steam gamemd, Unit destructor735780 body.
        if (game_state as u8) != 0
            && !self.object_placement_scope_active()
            && self
                .substrate
                .entities
                .get(stable_id)
                .is_some_and(|entity| entity.category() == EntityCategory::Unit)
        {
            loop {
                let passenger = self
                    .substrate
                    .entities
                    .get_mut(stable_id)
                    .and_then(|entity| entity.passenger_role.cargo_mut())
                    .and_then(|cargo| cargo.unload_first())
                    .map(|(id, _)| id);
                let Some(passenger) = passenger else {
                    break;
                };
                if self.substrate.entities.contains(passenger) {
                    self.finalize_and_remove_common(passenger, context, game_state);
                }
            }
        }
        if foot {
            // Class scalar destructors dispatch removed=true independently of
            // the active-state gate, after class tracking/Limbo and before the
            // delegated Foot cached-cell/sound tail: Unit735892,
            // Infantry517E75 and Aircraft414125 -> original7258D0. This also
            // repairs observers of a rejected constructor that never UnInits.
            self.notify_pointer_expired(stable_id, context);
        }
        // The ObjectClass destructor's defensive Defuse (`0x005F3BA6`).
        Self::bomb_defuse(&mut self.substrate, &mut self.bombs, stable_id);
        self.team_script_vm.object_deleted(stable_id);
        Self::release_house_base_tracking(&self.substrate, &mut self.houses, stable_id);
        // Selected original Foot destructor4D3632..4D366E clears only its
        // cached+564 Cell if+E0 still holds this Foot. UnInit/Limbo retain it;
        // the block sends no notification and leaves the obsolete+564 intact.
        self.substrate.clear_foot_air_slot_at_destruction(stable_id);
        // Foot destructor4D3677 Release406060 follows class Remove_Tracking
        // and Foot's cached-cell cleanup. All Foot classes share it, including
        // constructor-complete limbo disposal. No caller-local hard stop.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() != EntityCategory::Structure)
        {
            self.sound_events
                .emit(super::SimSoundEvent::ObjectSoundReleased { owner: stable_id });
        }
        // Foot's Parasite destructor6293E0 destroys its held Anim and clears
        // progress; it neither invents a release nor rewrites the host's HP.
        self.reset_parasite_grapple(stable_id, context.rules());
        // The Building destructor's Release406060 on its +0x6B4 working-sound
        // handle (43BD1A), the same most-derived destructor slot.
        Self::release_building_working_sound(
            &self.interner,
            &self.substrate,
            &mut self.sound_events,
            stable_id,
            context.rules(),
        );
        #[cfg(feature = "ares")]
        self.destroy_ares_attach_effects(stable_id, context.rules());
        // Foot4D3701 delegates to Techno6F4500 only after its own sound
        // tail. Techno6F4607 destroys the distinct voice handle via405C00,
        // before its attached/deploy Anims6F468B. Keep the disposal marker in
        // the same stream as AI visits and those Anim sound callbacks.
        if kind == Some(ObjectKind::Entity) {
            self.sound_events
                .emit(super::SimSoundEvent::UnitVoiceDestroyed { owner: stable_id });
        }
        // Techno destructor6F4667..6F4679 UnInits its Behind Anim, then
        //6F467F..6F4691 its retained deploy Anim. Pointer-expiry may already
        // have cleared either through the Anim owner.
        if let Some(anim) = self
            .substrate
            .entities
            .get_mut(stable_id)
            .and_then(|entity| entity.take_behind_anim())
        {
            self.destroy_anim_with_context(anim, context.rules());
        }
        if let Some(anim) = self
            .substrate
            .entities
            .get_mut(stable_id)
            .and_then(|entity| entity.take_deploy_anim())
        {
            self.destroy_anim_with_context(anim, context.rules());
        }
        self.lighting_sources.destroy_building_light(stable_id);
        if kind == Some(ObjectKind::Anim) {
            self.notify_anim_pointer_expired(stable_id);
            self.substrate.clear_damage_fire_anim_reference(stable_id);
            self.conceal_anim(stable_id);
            self.substrate.release_anim_owner_reference(stable_id);
            self.substrate.clear_building_anim_reference(stable_id);
        }
        // The native Object field+34 disappears with storage. This is not
        // SetTag(nullptr): the shared Tag owner preserves its native counter.
        self.trigger_runtime
            .finalize_object_trigger_tag_attachment(stable_id);
        // Display registration never outlives the object. Fly's phase tail
        // (4CD4DE) resubmits even an owner concealed earlier in the frame.
        self.substrate.display.remove(stable_id);
        self.fog.forget_sight_source(stable_id);
        self.drop_object_custom_sound(stable_id);
        let removed = match kind {
            Some(ObjectKind::Entity) => {
                let entity = self.substrate.entities.remove(stable_id);
                if entity
                    .as_ref()
                    .is_some_and(|entity| entity.category() == EntityCategory::Structure)
                {
                    // Techno+3D3 (ctor6F2F4F) dies with the Building's storage. Limbo retains it;
                    // physical disposal removes this identity without electing a
                    // replacement or altering the House's Factory queue slots.
                    self.production.clear_primary_factory(stable_id);
                }
                entity.is_some()
            }
            Some(ObjectKind::Anim) => self.substrate.anims.remove(stable_id).is_some(),
            Some(ObjectKind::ParticleSystem) => {
                let system = self.substrate.particle_systems.finalize_remove(stable_id);
                if let Some(system) = system.as_ref() {
                    // SystemDtor62E0AE..62E15D disposes retained children before its
                    // vector is freed. Ordinary AI retirement has no children left;
                    // direct disposal must still remove each individual AIR entry.
                    for particle in system.particles() {
                        self.substrate.conceal_particle(particle.stable_id());
                    }
                }
                system.is_some()
            }
            Some(ObjectKind::Particle) => self
                .substrate
                .particle_systems
                .finalize_particle(stable_id)
                .is_some(),
            Some(ObjectKind::Terrain) => self.terrain_objects.remove(stable_id).is_some(),
            Some(ObjectKind::Projectile) => {
                let removed = self.projectiles.remove(stable_id).is_some();
                if removed {
                    self.lifecycle_outputs
                        .emit(LifecycleOutput::LineTrailDetached { stable_id });
                }
                removed
            }
            Some(ObjectKind::Wave) => {
                let wave = self.waves.remove(stable_id);
                if let Some(wave) = wave.as_ref()
                    && let Some(owner_id) = wave.owner_id()
                    && self.active_wave_links.get(&owner_id) == Some(&stable_id)
                {
                    self.active_wave_links.remove(&owner_id);
                }
                wave.is_some()
            }
            Some(ObjectKind::Smudge) => self
                .smudge_grid
                .as_mut()
                .and_then(|grid| grid.finalize_remove(stable_id))
                .is_some(),
            // A spotlight returned above. VERA removes a voxel anim on its AI's
            // Delete (`visit_voxel_anim`), never through this destructor.
            Some(ObjectKind::Spotlight | ObjectKind::VoxelAnim) => false,
            // A stale deferred-delete ticket: a direct destructor already
            // removed its object (`discard_constructed_limbo` keeps a queued
            // constructor-owned child's ticket).
            None => false,
        };
        debug_assert!(
            removed || kind.is_none(),
            "{kind:?} {stable_id} left no store at its destructor"
        );
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::FinalizedCommon { stable_id });
    }

    /// The pending-delete drain of test fixtures, on the rules an empty INI
    /// reads to (see [`Self::process_pending_delete_with`]).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn process_pending_delete(&mut self) {
        self.process_pending_delete_with(
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            FrameEffects::default(),
            1,
            self.session.started(),
        );
    }

    /// Native-shaped pending-delete drain: preserve alive entries, collapse all
    /// duplicate ready IDs, and finalize each selected object exactly once.
    /// The frame's rules reach the destructors' slave release.
    ///
    /// gamemd-derived: active YR `DrainDeferredFinalizationQueue @ 0x00725C70`
    /// is reached from `Main_Tick` at `0x0055DE9F` after the frame commit; it
    /// preserves non-ready entries, collapses selected duplicates, and finalizes
    /// each selected ready object once.
    pub fn process_pending_delete_with(
        &mut self,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
        game_state: i32,
        started: bool,
    ) {
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(LifecycleTestEvent::PendingDeleteDrainStarted);
        let mut index = 0;
        while let Some(ticket) = self.substrate.pending_delete.ticket(index) {
            let ready = match ticket {
                DeferredDeleteTicket::Object(id) => self.pending_object_is_ready(id),
                DeferredDeleteTicket::Overlay(id) => self.load_objects.queued_overlay_is_ready(id),
                // Abstract410440's +44 returns true for both live families.
                DeferredDeleteTicket::Tag(_)
                | DeferredDeleteTicket::Trigger(_)
                | DeferredDeleteTicket::DiskLaser(_) => true,
            };
            if !ready {
                index += 1;
                continue;
            }
            //725C9A..725CE1 removes every selected duplicate BEFORE virtual
            // destruction, then reexamines the same live logical index.
            self.substrate.pending_delete.remove(ticket);
            #[cfg(any(test, feature = "test-support"))]
            self.trace_lifecycle_for_test(LifecycleTestEvent::DeferredDeleteSelected { ticket });
            match ticket {
                DeferredDeleteTicket::Object(id) => {
                    self.release_slave_links_at_destruction(id, rules, registry, effects);
                    self.finalize_and_remove_common(
                        id,
                        UninitContext::new(rules, registry).with_effects(effects),
                        game_state,
                    );
                }
                DeferredDeleteTicket::Overlay(id) => self.load_objects.destroy_queued_overlay(id),
                DeferredDeleteTicket::Tag(id) => {
                    self.delete_live_tag(id, rules, game_state, started)
                }
                DeferredDeleteTicket::Trigger(id) => self.trigger_runtime.delete_live_trigger(id),
                DeferredDeleteTicket::DiskLaser(id) => self.destroy_disk_laser(id),
            }
        }
        self.substrate.pending_delete.finish_drain();
    }

    /// Test compatibility only.  Production has one ordinary tail drain.
    #[cfg(any(test, feature = "test-support"))]
    pub fn flush_pending_delete(&mut self) {
        self.process_pending_delete();
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// ObjectLimbo5F4D74 removes the individual particle's retained AIR
    /// entry through DisplayRemove4A9770, independently of its system.
    pub(crate) fn conceal_particle(&mut self, stable_id: u64) {
        self.display.remove(stable_id);
    }
    /// Object5F5765 invokes RecordKill for each actual exact-zero callback.
    /// A retained Health1 hull can reach this again with a different attacker;
    /// stale attribution and the prior UnInit guard must not swallow it.
    /// The receiver then captures this hit's credit, awards experience
    /// (702FF0), and consumes the score record (703003..7031DC) before Destroy.
    pub(crate) fn begin_receiver_kill_record(&mut self, id: u64) {
        if let Some(entity) = self.entities.get_mut(id) {
            debug_assert_eq!(entity.health.current(), 0);
            entity.kill_record.clear();
        }
    }
}

#[cfg(test)]
#[path = "foot_archive_expiry_tests.rs"]
mod foot_archive_expiry_tests;

#[cfg(test)]
#[path = "ares_prerequisite_lifecycle_tests.rs"]
mod ares_prerequisite_lifecycle_tests;
