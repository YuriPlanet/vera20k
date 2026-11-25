//! Ordered ObjectClass-style lifecycle authority.
//!
//! Owns the independent Reveal, Conceal/Limbo, UnInit, LogicVector membership,
//! and pending-delete transitions.  Upper-layer work is emitted as ordered data;
//! this module never depends on render, UI, sidebar, audio, or net.
//!
//! This file holds the lifecycle types and the base-reservation rect helpers;
//! the `Simulation` transitions live in the child modules: `reveal`
//! (Unlimbo), `discovery` (discovery and the watcher seat), `placement`
//! (Mark/UnMark and base reservations), `fly_phase`, `logic_objects` (Logic
//! membership and non-entity objects), `limbo` (Conceal/Limbo), `detach`
//! (Detach_All and Stun), `pointer_expiry` and `uninit` (UnInit and pending
//! delete).

#[cfg(test)]
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::CellRect;
#[cfg(test)]
use crate::sim::combat::TargetKind;
#[cfg(test)]
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::projectile::ProjectileTarget;

#[cfg(any(test, feature = "test-support"))]
use super::substrate::DeferredDeleteTicket;
use super::{
    FrameAdvanceError, SimSoundEvent, display_layers, techno_ai_cloak, techno_unlimbo_idle_mode,
};
use super::{FrameEffects, Simulation};

mod detach;
mod discovery;
mod fly_phase;
mod limbo;
mod logic_objects;
mod placement;
mod pointer_expiry;
mod reveal;
mod uninit;

/// A building's `HasSpotlight=` fact and its owned `BuildingLightClass`.
/// Construction copies the fact, and a later Unlimbo recreates the light from
/// the copy ([`Self::stamp`]). `BuildingClass::Unlimbo @
/// 0x00441187` constructs the light after placement succeeds and
/// `BuildingClass::Limbo @ 0x00445D81` UnInits it before the remaining
/// building-count and base-node teardown, and the frame's drain deletes it;
/// the building owns one only while placed with `HasSpotlight=yes`.
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
    pub(super) fn attach(&mut self, id: u64, _: crate::sim::game_entity::NameNote) {
        self.light = Some(id);
    }
    pub(super) fn take(&mut self) -> Option<u64> {
        self.light.take()
    }
}

/// Two type facts the removal paths read, copied onto the object:
/// - `ObjectClass::Limbo` (`0x005F4D30`) dirties the AlphaImage-sized rect
///   around GetCoords when the type's AlphaImage (`+0xAC`) loaded
///   ([`LifecycleOutput::DirtyTacticalRect`]). No production path sets the
///   fact: that rect only clears the object's glow, and the tactical A plane
///   redraws whenever its glow list changes (`alpha_shape`).
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
/// Conceal6F4A40 and the silent70D990 shroud probe own the retained
/// observations. Infantry raw Save/Load
/// preserves the bytes: never reconstruct them from the restored owner.
/// Native peer CRC64DAB0 omits these actual-client bytes. In a VERA network
/// world the same owner also retains each represented human's admission and
/// the nonhuman receiver history. Those canonical fields affect shared sight
/// and discovery callbacks and therefore DO enter the shared Rust hash.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TechnoDiscoveryHistory {
    /// `+0x41A`: the current house owns the object.
    owned_by_current_house: bool,
    /// `+0x41B`: the current house has discovered it.
    discovered_by_current_house: bool,
    /// `+0x41C`: a single historical byte for all other receiver houses, not
    /// a house set.
    discovered_by_other_house: bool,
    /// LAN/WOL +41B for every represented human, in immutable human House
    /// registration order. Admission bounds the match to30 humans, fitting32
    /// bits without per-object allocations; these are not raw interner IDs.
    #[serde(default)]
    network_human_discovery: u32,
    /// Canonical nonhuman callback admission, independent of actual-local41C.
    #[serde(default)]
    network_other_discovery: bool,
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

    /// VERA watching: this process's own bytes rebuilt for a newly followed
    /// human from the history every peer keeps. `view` is that human's bit
    /// in `network_human_discovery`; the House left behind becomes another
    /// receiver.
    fn follow_view(&mut self, owned: bool, view: u32) {
        self.owned_by_current_house = owned;
        self.discovered_by_current_house = self.network_human_discovery & view != 0;
        self.discovered_by_other_house |= self.network_human_discovery & !view != 0;
    }

    fn clear_current_discovery(&mut self) {
        self.discovered_by_current_house = false;
        // A shared lifecycle clear executes in every represented native view.
        // Preserve the independent other-receiver history, as native preserves41C.
        self.network_human_discovery = 0;
    }

    pub(super) fn hash_network(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        b"network-discovery-v1".hash(hasher);
        self.network_human_discovery.hash(hasher);
        self.network_other_discovery.hash(hasher);
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
            network_human_discovery: 0,
            network_other_discovery: false,
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
/// A control of 0 changes five things inside the receiver body: it skips
/// RadioClass's contact break (`0x0065AAF0`) and the listener's
/// `CargoClass::Remove` of the object (`0x007077D8..0x007077E9`), it runs the
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
/// It holds the frame's effects handle by value, lent for this tree with
/// [`FrameEffects::reborrow`]; a nested call takes [`Self::reborrow`].
pub struct UninitContext<'a> {
    terrain: Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rules: &'a RuleSet,
    requested_facing: Option<u8>,
    effects: FrameEffects<'a>,
}

impl<'a> UninitContext<'a> {
    /// A caller's rules, with no terrain override. Receivers read the
    /// match's OverlayTypeClass table from these rules ([`Self::registry`]).
    pub const fn new(rules: &'a RuleSet) -> Self {
        Self {
            terrain: None,
            rules,
            requested_facing: None,
            effects: FrameEffects::empty(),
        }
    }

    /// Preserve the current frame's synchronous sound boundary through nested
    /// lifecycle receivers. It borrows the caller; no callback survives the
    /// enclosing frame or becomes simulation state.
    pub fn with_effects<'b>(self, effects: &'b mut FrameEffects<'_>) -> UninitContext<'b>
    where
        'a: 'b,
    {
        UninitContext {
            terrain: self.terrain,
            rules: self.rules,
            requested_facing: self.requested_facing,
            effects: effects.reborrow(),
        }
    }

    pub(crate) fn effects(&mut self) -> &mut FrameEffects<'a> {
        &mut self.effects
    }

    /// The same context for one nested call.
    pub(crate) fn reborrow(&mut self) -> UninitContext<'_> {
        UninitContext {
            terrain: self.terrain,
            rules: self.rules,
            requested_facing: self.requested_facing,
            effects: self.effects.reborrow(),
        }
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

    /// The match's OverlayTypeClass table, which its rules own, for
    /// receivers that classify a cell's overlay (an ejected occupant's
    /// Scatter entry test). gamemd reads the one global OverlayTypes vector.
    pub(crate) fn registry(&self) -> Option<&'a crate::rules::overlay_types::OverlayTypeRegistry> {
        Some(self.rules.overlay_registry())
    }

    pub(crate) const fn terrain(
        &self,
    ) -> Option<&'a crate::map::resolved_terrain::ResolvedTerrainGrid> {
        self.terrain
    }

    pub const fn rules(&self) -> &'a RuleSet {
        self.rules
    }
}

/// A fixture's context: the rules an empty INI reads to
/// ([`RuleSet::empty_for_test`]), whose overlay table is empty.
#[cfg(any(test, feature = "test-support"))]
impl Default for UninitContext<'static> {
    fn default() -> Self {
        Self::new(RuleSet::empty_for_test())
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
    /// What reaches Reveal unconverted (a building its admission accepted, or
    /// a map-less fixture) goes straight to Mark(PUT): no Unlimbo layer
    /// refuses on the playfield (Building `0x00440580..0x004415E6`, Techno
    /// `0x006F6CA0`, Object `0x005F4EC0`). A building's playfield refusal is
    /// its own Can_Enter_Cell (`0x00449440`), reached through Object
    /// Unlimbo's +1AC call (`0x005F4F3C`), whose CanPlaceAt asks
    /// Is_Clear_To_Build (`0x0047C620`; playfield test at `0x0047C878`) of
    /// each foundation cell unless the type is `PlaceAnywhere=`.
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

pub(crate) struct TargetingSweepReceipt {
    #[cfg(any(test, feature = "test-support"))]
    pub cell_detaches: Vec<crate::sim::combat::combat_aoe::CellTargetDetach>,
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
        *self.object_placement_scope_depth = self.object_placement_scope_depth.wrapping_add(1);
        let result = operation(self);
        *self.object_placement_scope_depth = self.object_placement_scope_depth.wrapping_sub(1);
        result
    }

    pub(crate) fn object_placement_scope_active(&self) -> bool {
        *self.object_placement_scope_depth != 0
    }

    /// Crate validation4A18F0 saves A8E7AC, clears it at4A190B, and
    /// restores it on both success4A1A7D and refusal4A1A8E. Its callbacks
    /// therefore use ordinary admission even inside an initial-object scope.
    /// Source: original gamemd; crate_pickup and wall-autofill callers.
    pub(crate) fn without_object_placement_scope<R>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prior = std::mem::take(&mut *self.object_placement_scope_depth);
        let result = operation(self);
        *self.object_placement_scope_depth = prior;
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
        let saved = std::mem::replace(&mut *self.buildup_bypass, true);
        let result = operation(self);
        *self.buildup_bypass = saved;
        result
    }

    pub(crate) fn buildup_bypass_active(&self) -> bool {
        *self.buildup_bypass
    }
}

pub(super) fn building_base_reservation_rect(
    rx: u16,
    ry: u16,
    foundation: &crate::rules::foundation::Foundation,
    spacing: i32,
) -> CellRect {
    let (width, height) = foundation.dimensions();
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
pub(crate) fn object_get_coords_cell(
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
    /// PlanMgr append 0x00638120 refused `owner`'s event with a message
    /// (latched with ScoldSound) that only the local house shows.
    PlanningAppendRefused {
        owner: InternedId,
        message: super::planning::PlanningAppendMessage,
    },
    /// Append 0x006383BF set AC4C90 for `owner`: if local, PlanMgr637270's
    /// next frame pass deselects all and posts `message`.
    PlanningFramePassQueued {
        owner: InternedId,
        message: super::planning::PlanningAppendMessage,
    },
    /// MPlayer_Defeated 0x004FC0B0 for `owner`, after its Defeated flag and
    /// Computer_Paranoid. The app runs PlayerPtr's local branch (beacons,
    /// command bar, chat mask: 0x004FC1BD..0x004FC23E) and, when
    /// `match_decided` (the decision at 0x004FC3DE..0x004FC6B7), reopens a
    /// command bar that branch closed (0x004FC591..0x004FC59F).
    MultiplayerHouseDefeated {
        owner: InternedId,
        match_decided: bool,
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
    /// StripAI6A8E48 -> SetSidebarTabObject734250 publishes the actual held
    /// Building on each consumed changed+complete edge. The local sidebar
    /// owns this reference's history; Factory owns the object's readiness.
    ProductionBuildingReady {
        owner: InternedId,
        category: crate::sim::production::ProductionCategory,
        stable_id: u64,
    },
    /// Local HouseAbandon4FAB74 enters Sidebar6ABAD0. Its private strip
    /// owner determines whether the cursor prefix precedes settlement (no
    /// link) or follows the first linked FactoryAbandon/destructor.
    ProductionFactoryAbandoning {
        owner: InternedId,
        category: crate::sim::production::ProductionCategory,
        construction_order: u64,
        /// StripAbandon6ABBB6 clears only an actual held Building reference.
        /// None means the nonnull/Building gate skipped the helper entirely.
        held_building: Option<u64>,
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
    /// BuildingChangeOwner448C9A..448CD6: a Construction Yard leaving a
    /// human player (IsHumanPlayer50B6F0) clears that player's current
    /// placement (Display PendingObject/Type/House) and cursor (4A8BF0).
    /// The saved retry and the ready product stay.
    ConstructionYardCaptured {
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
    /// follows this object, whatever the argument and the Deselect gate.
    /// Every Deselect of a selected object emits one too
    /// (`0x005F4503..0x005F4513`, [`Simulation::object_deselect`]): Limbo, a
    /// lift, a Temporal erase, a cloak, shroud or a logic Select's
    /// Unselect_All, after which a later Select in the same frame may select
    /// the object again.
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
            entity.seldom.kill_record.clear();
        }
    }
}

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "foot_archive_expiry_tests.rs"]
mod foot_archive_expiry_tests;

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "watcher_tests.rs"]
mod watcher_tests;

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "ares_prerequisite_lifecycle_tests.rs"]
mod ares_prerequisite_lifecycle_tests;
