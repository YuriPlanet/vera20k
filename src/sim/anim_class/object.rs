//! The animation instance: `AnimObject` with its runtime, draw and display
//! state, and the owner-relative coordinates and display sort keys derived
//! from it.

use super::{AnimId, AnimWorldCoord, flaming_guy};
use crate::rules::house_colors::HouseColorIndex;
use crate::sim::bounce::BounceState;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::util::native_x87::NativeF64Bits;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimRuntime {
    pub current_frame: i32,
    pub frame_step: i32,
    /// AnimClass +0x184, a signed int: the constructor's delay or a drawn
    /// `RandomLoopDelay=` pause.
    pub delay_remaining: i32,
    pub rate_reload: u16,
    pub frame_timer: CdTimer,
    pub loop_remaining: u8,
    pub first_ai_guard: bool,
    pub constructor_reverse: bool,
    /// Native Anim+19B: request expiry on a later AI visit (owner expiry425196).
    /// UnInit4255B0 preserves this byte. Pending deletion and retained Logic/
    /// Display membership own retirement independently.
    pub inactive: bool,
    /// Anim+19E, pause/resume425260/425270. The absolute frame timer keeps running.
    #[serde(default)]
    pub paused: bool,
    /// Anim+11A: a building's Temporal erase freezes its slot anims
    /// ([`AnimObject::set_temporal_erase`], [`AnimObject::temporal_pause`]).
    /// AI stops on it just as on +19E (`0x004244A9`); the frame timer keeps
    /// running.
    #[serde(default)]
    pub temporal_paused: bool,
}

/// Per-instance `AnimClass::DrawIt` bytes that are independent of the art
/// type. They remain serialized simulation state because native effects may
/// set them between frames; presentation only reads this resolved input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnimDrawRuntime {
    /// AnimClass `+0x19d`: unconditional draw suppression.
    pub hidden: bool,
    /// AnimClass `+0x199`, the fogged byte. DrawIt skips the anim when it is
    /// set and the type's `ShouldFogRemove=` (`+0x374`, `0x004282DC`) is too
    /// (`0x00423014..0x00423026`). Presentation ports that gate with the fog
    /// query (`instances::overlays`), so nothing sets this field.
    pub special_hidden: bool,
    /// AnimClass `+0x178`: ramp/age input used by translucency selection.
    pub translucency_ramp: u8,
    /// AnimClass `+0x119`: draw at 50%, or 75% for a `DoubleThick=` type
    /// (`AnimClass::DrawIt` reads type `+0x368` at `0x0042306B`).
    pub forced_translucent: bool,
}

/// Retained Anim instance inputs to Display, independent of vector membership.
/// Constructor422131 copies type+340 to instance+104; SetOwner424B50 tests
/// Object+74 to gate only its normal-detach re-registration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct AnimDisplayState {
    pub(super) marked_on_map: bool,
    pub(super) y_sort_adjust: i32,
}

impl AnimDisplayState {
    pub(crate) fn y_sort_adjust(&self) -> i32 {
        self.y_sort_adjust
    }
}

/// The producer-selected AnimClass+0xD4 palette conversion. A fixed scheme
/// serves CellAnim; a deploy animation retains the house selected by
/// Techno705D70 at creation. Presentation resolves that identity through the
/// match's existing HouseColorMap, never through the owner's later state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnimRemap {
    ColorScheme(HouseColorIndex),
    House(InternedId),
    /// Techno705D70's loaded `+0xDF0` group and house colour, frozen when
    /// the producer assigns the Convert. The palette name survives the
    /// source building's destruction and distinguishes simultaneous groups.
    ExtraBuildingScheme {
        house: InternedId,
        palette: InternedId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimObject {
    pub(super) stable_id: AnimId,
    pub(super) native_unique_id: i32,
    pub(super) type_id: InternedId,
    /// World leptons — but **owner-relative whenever `owner_entity` is set**,
    /// exactly as `AnimClass::SetOwnerObject @ 0x00424B50` stores it. Read it
    /// through [`Simulation::anim_absolute_coord`] for a world position; read
    /// the field directly only where native reads the stored `ObjectClass`
    /// coordinate directly: the multiplayer sync checksum
    /// (`Compute_Game_Sync_Checksum @ 0x0064DAB0` folds `+0x9c`/`+0xa0`),
    /// HideIfNoOre and post-load Tiberium conversion4261D0. All three axes
    /// are world leptons.
    pub(super) world_coord: AnimWorldCoord,
    pub(super) draw_flags: u32,
    pub(super) z_adjust: i32,
    /// Optional ConvertClass palette selected by a producer after construction
    /// (for example OverlayClass CellAnim over a Tiberium cell).
    #[serde(default)]
    pub(super) remap: Option<AnimRemap>,
    pub(super) effective_end: i32,
    pub(super) effective_loop_end: i32,
    pub(super) runtime: AnimRuntime,
    pub(super) draw_runtime: AnimDrawRuntime,
    /// Per-viewer snapshots of native Anim+19D, for a producer whose byte
    /// depends on the native client's PlayerPtr. Every ordinary hidden-byte
    /// writer replaces this with a uniform value.
    #[serde(default)]
    pub(super) viewer_hidden: Option<AnimViewerHidden>,
    /// AnimClass `+0x196`: use the containing CellClass draw/palette authority.
    #[serde(default)]
    pub(super) use_cell_drawer: bool,
    /// AnimClass `+0x197`: created from a terrain tile animation descriptor.
    #[serde(default)]
    pub(super) terrain_attached: bool,
    /// Anim+198. Constructor421FF1 clears it; TAction416E2368 sets it
    /// only after constructor-time Start. Start424CF1/424D5F, AI423AC6
    /// and UnInit4255DA suppress effects, independently of drawing/Middle.
    #[serde(default)]
    pub(super) no_effects: bool,
    /// Anim+188. Normal ctor421FA4/421FC8 seeds1; default ctor4227FD/
    ///42280F and Next424877/424885 seed0. Load425280 retains saved bytes.
    /// Binary64 keeps the reader's fractional precision and full accumulator
    /// range, including a NoEffects backlog, beyond SimFixed's16.16 range.
    /// Basic IEEE operations are deterministic; NaN stores are canonicalized.
    /// https://doc.rust-lang.org/std/primitive.f64.html
    /// Precision residual: round-to-nearest replaces native PC53/chop. Near
    /// an integer threshold a long-running fire can emit on a different visit.
    pub(super) damage_accumulator: NativeF64Bits,
    /// LogicClass membership is reconstructed from the serialized vector.
    /// ObjectClass::Save does not persist its local membership byte.
    #[serde(skip)]
    pub(super) in_logic_vector: bool,
    pub(super) owner_entity: Option<u64>,
    /// Derived reverse index of Building+55C's slot reference, rebuilt on load.
    /// Independent of both Anim+118 and the Anim+CC Object-owner attachment.
    #[serde(skip)]
    pub(super) building_slot: Option<(u64, u8)>,
    /// Anim+118: ctor421F44 clears it; Building45199B sets it. It suppresses
    /// independent drawing and admits451B40, even after the slot is cleared.
    #[serde(default)]
    pub(super) building_anim: bool,
    /// Derived reverse index of Building+5C8's damage-fire slot, rebuilt on
    /// load. It survives owner expiry until the Anim's own UnInit broadcast.
    #[serde(skip)]
    pub(super) damage_fire_slot: Option<(u64, u8)>,
    /// Anim+179: constructor421FB0 clears it; normal AI completion424B31
    /// sets it before UnInit. Building451B40 distinguishes a completed
    /// retraction from an animation removed for another reason.
    #[serde(default)]
    pub(super) completed: bool,
    pub(super) start_sound_active: bool,
    pub(super) stop_sound_id: Option<InternedId>,
    pub(super) display: AnimDisplayState,
    /// `AnimClass+0x128`: a `Bouncer=` or `IsMeteor=` chunk's BounceClass
    /// body, both built by [`anim_constructor_draws`]. Its presence is `+0x194`.
    #[serde(default)]
    pub(super) bounce: Option<BounceState>,
    /// Anim+108/+114/+19A and Object falling words for IsFlamingGuy AI.
    #[serde(default)]
    pub(super) flaming_guy: Option<flaming_guy::FlamingGuyState>,
    /// `AnimClass+0x180`, the owning House (`SetOwnerHouse @ 0x00424CA0`).
    /// Its expiry also ends the anim (`0x004251D5`). The PsiWarning
    /// visibility test reads it against the local player (`0x0043B4C0`).
    #[serde(default)]
    pub(super) owner_house: Option<InternedId>,
    /// `AnimClass+0x17C`, the bullet whose expiry ends this anim
    /// (`0x00424C90` stores it; `AnimClass::PointerExpired @ 0x00425150`
    /// clears it and UnInits the anim). The nuclear missile's warning marker
    /// rides its missile this way.
    #[serde(default)]
    pub(super) attached_bullet: Option<u64>,
    /// `AnimClass+0xCC` when it names a `TerrainClass`: a tree's fire, which
    /// only the Ares terrain fire attaches (`sim::terrain_fire`). VERA keeps
    /// a terrain owner apart from `owner_entity` and its stored coordinate
    /// absolute: a terrain object never moves, so native's owner-relative
    /// form adds the same constant back on every read.
    #[serde(default)]
    pub(super) owner_terrain: Option<u64>,
    /// VERA-only: `Some` for a presentation anim
    /// ([`AnimConstructorInput::Presentation`]), holding the layer it draws in
    /// while revealed, where an ordinary anim would hold a Display slot. A
    /// presentation anim is local, so it is never saved, compared or hashed.
    #[serde(skip)]
    pub(super) presentation: Option<Option<crate::sim::world::display_layers::DisplayLayer>>,
    /// VERA-only: a presentation anim's place in Display submission order,
    /// taken when it was last placed in its layer
    /// (`DisplayLayers::unshared_order`), where an ordinary anim's slot
    /// would hold it.
    #[serde(skip)]
    pub(super) presentation_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct AnimViewerHidden {
    pub(super) viewers: Vec<(InternedId, bool)>,
    pub(super) other_hidden: bool,
}

impl AnimObject {
    /// Whether this is a presentation anim ([`AnimConstructorInput::Presentation`]).
    pub fn is_presentation(&self) -> bool {
        self.presentation.is_some()
    }

    /// The layer a revealed presentation anim draws in; `None` for an
    /// ordinary anim, which Display places, or a concealed one.
    pub fn presentation_layer(&self) -> Option<crate::sim::world::display_layers::DisplayLayer> {
        self.presentation.flatten()
    }

    /// A presentation anim's place in Display submission order
    /// (`Simulation::picture_draw_order`).
    pub fn presentation_order(&self) -> u64 {
        self.presentation_order
    }

    /// Read the same hidden byte for a particular client without making
    /// simulation state depend on which house this process is displaying.
    pub fn hidden_for(&self, viewer: Option<InternedId>) -> bool {
        if let Some(snapshot) = &self.viewer_hidden {
            return viewer
                .and_then(|viewer| snapshot.viewers.iter().find(|(id, _)| *id == viewer))
                .map_or(snapshot.other_hidden, |(_, hidden)| *hidden);
        }
        self.draw_runtime.hidden
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn draw_runtime_mut_for_test(&mut self) -> &mut AnimDrawRuntime {
        &mut self.draw_runtime
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_building_slot_for_test(&mut self, value: Option<(u64, u8)>) {
        self.building_slot = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_in_logic_vector_for_test(&mut self, value: bool) {
        self.in_logic_vector = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_start_sound_active_for_test(&mut self, value: bool) {
        self.start_sound_active = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_terrain_attached_for_test(&mut self, value: bool) {
        self.terrain_attached = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn runtime_mut_for_test(&mut self) -> &mut AnimRuntime {
        &mut self.runtime
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn world_coord_mut_for_test(&mut self) -> &mut AnimWorldCoord {
        &mut self.world_coord
    }

    /// LogicClass membership: RegisterObject sets it (`0x0055BAC6`),
    /// UnregisterObject clears it (`0x0055BB27`). Only the logic vector's
    /// owner writes it, and a load rebuilds it from the saved vector.
    pub(crate) fn mark_logic_membership(&mut self, member: bool) {
        self.in_logic_vector = member;
    }

    /// Building43F738 moves a slot anim to its building's new coordinate;
    /// it neither reconstructs the anim nor resets its timers.
    pub(crate) fn move_with_building(&mut self, coord: AnimWorldCoord) {
        self.world_coord = coord;
    }

    /// The anim takes slot `slot` of `building` (Building+55C), with the
    /// slot config's `ZAdjust=`. The reverse index is derived from that slot.
    pub(crate) fn enter_building_slot(&mut self, building: u64, slot: u8, z_adjust: i32) {
        self.building_slot = Some((building, slot));
        self.z_adjust = z_adjust;
    }

    /// The reverse index of the building slot that holds this anim, rebuilt
    /// from Building+55C on load.
    pub(crate) fn index_building_slot(&mut self, building: u64, slot: u8) {
        self.building_slot = Some((building, slot));
    }

    /// The building slot leaves the anim: the index is cleared and returned.
    pub(crate) fn take_building_slot(&mut self) -> Option<(u64, u8)> {
        self.building_slot.take()
    }

    /// The reverse index of the Building+5C8 damage-fire slot that holds
    /// this anim, rebuilt on load.
    pub(crate) fn index_damage_fire_slot(&mut self, building: u64, slot: u8) {
        self.damage_fire_slot = Some((building, slot));
    }

    /// Drop both derived slot indexes before a rebuild, or when the
    /// building releases the anim.
    pub(crate) fn clear_slot_indexes(&mut self) {
        self.building_slot = None;
        self.damage_fire_slot = None;
    }

    /// Anim pause (`0x00425260`): only Anim+19E changes; the absolute frame
    /// timer keeps running.
    pub(crate) fn pause(&mut self) {
        self.runtime.paused = true;
    }

    /// Anim resume (`0x00425270`).
    pub(crate) fn resume(&mut self) {
        self.runtime.paused = false;
    }

    /// A building's Temporal erase on one slot anim: `0x004521C0` freezes it
    /// (+11A) and draws it translucent (+119); `0x00452210` clears both.
    ///
    /// RESIDUAL: the erase also saves the frame (+11C) and the release sets
    /// +11B, which AI (`0x00423C03..0x00423C1D`) clears once the frame comes
    /// back round to the saved one. Nothing else reads either, so neither is
    /// kept.
    pub(crate) fn set_temporal_erase(&mut self, erased: bool) {
        self.runtime.temporal_paused = erased;
        self.draw_runtime.forced_translucent = erased;
    }

    /// `0x00451B25`: a slot anim created while its building is being erased
    /// starts frozen (+11A), but opaque.
    pub(crate) fn temporal_pause(&mut self) {
        self.runtime.temporal_paused = true;
    }

    /// A building writes the anim's current frame: a slot successor keeps
    /// its predecessor's frame, and SiloDamage (`0x00450D81`) shows the
    /// storage tier.
    pub(crate) fn jump_to_frame(&mut self, frame: i32) {
        self.runtime.current_frame = frame;
    }

    /// A turret building aims its slot-9 anim (`0x004466C5..0x004466CB`,
    /// `0x00451286..0x0045128C`, `0x0044FE17..0x0044FE1D`): it writes the
    /// frame (`+0xAC`) and a zero frame step (`+0xC4`), so the stage timer
    /// keeps running without moving the frame.
    pub(crate) fn hold_at_frame(&mut self, frame: i32) {
        self.runtime.current_frame = frame;
        self.runtime.frame_step = 0;
    }

    /// Hold the current loop until the owning effect explicitly destroys it.
    /// Ares attached animation1005872F stores byte255 at Anim+195.
    pub(crate) fn hold_loops(&mut self) {
        self.runtime.loop_remaining = u8::MAX;
    }

    /// No loops remain: the anim ends after its current pass. The
    /// parachute release and the ChronoPlacement release use it.
    pub(crate) fn end_loops(&mut self) {
        self.runtime.loop_remaining = 0;
    }

    pub const fn stable_id(&self) -> AnimId {
        self.stable_id
    }

    pub const fn native_unique_id(&self) -> i32 {
        self.native_unique_id
    }

    pub const fn type_id(&self) -> InternedId {
        self.type_id
    }

    pub const fn world_coord(&self) -> AnimWorldCoord {
        self.world_coord
    }

    pub const fn draw_flags(&self) -> u32 {
        self.draw_flags
    }

    pub const fn z_adjust(&self) -> i32 {
        self.z_adjust
    }

    pub const fn effective_end(&self) -> i32 {
        self.effective_end
    }

    pub const fn effective_loop_end(&self) -> i32 {
        self.effective_loop_end
    }

    pub const fn runtime(&self) -> &AnimRuntime {
        &self.runtime
    }

    pub const fn draw_runtime(&self) -> AnimDrawRuntime {
        self.draw_runtime
    }

    pub const fn use_cell_drawer(&self) -> bool {
        self.use_cell_drawer
    }

    pub const fn terrain_attached(&self) -> bool {
        self.terrain_attached
    }

    pub const fn in_logic_vector(&self) -> bool {
        self.in_logic_vector
    }

    pub const fn owner_entity(&self) -> Option<u64> {
        self.owner_entity
    }

    pub const fn building_slot(&self) -> Option<(u64, u8)> {
        self.building_slot
    }

    pub const fn damage_fire_slot(&self) -> Option<(u64, u8)> {
        self.damage_fire_slot
    }

    pub const fn start_sound_active(&self) -> bool {
        self.start_sound_active
    }

    pub const fn stop_sound_id(&self) -> Option<InternedId> {
        self.stop_sound_id
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn display(&self) -> &AnimDisplayState {
        &self.display
    }

    pub const fn bounce(&self) -> Option<BounceState> {
        self.bounce
    }

    pub fn remap(&self) -> Option<AnimRemap> {
        self.remap
    }

    pub fn no_effects(&self) -> bool {
        self.no_effects
    }

    pub fn completed(&self) -> bool {
        self.completed
    }

    pub fn is_building_anim(&self) -> bool {
        self.building_anim
    }

    /// Building45199B..4519A2 marks the slot animation and replaces its
    /// constructor YSortAdjust with the building slot's retained value.
    /// The Anim store touch invalidates the existing Ground key cache.
    /// Native reader/store/sort evidence: building_slot_replacement.display.json.
    pub(crate) fn mark_building_anim(&mut self, y_sort: i32) {
        self.building_anim = true;
        self.display.y_sort_adjust = y_sort;
    }

    /// Ares `BuildingExt::ExtData::UpdateFirewall @ 10013280` stores its
    /// own anims in `building`'s `slot` and sets only Anim+0x118: the
    /// constructor's ZAdjust and YSortAdjust stay.
    pub(crate) fn enter_ares_firewall_slot(&mut self, building: u64, slot: u8) {
        self.building_anim = true;
        self.building_slot = Some((building, slot));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_building_expiry_fixture(&mut self, marker: bool, completed: bool) {
        self.building_anim = marker;
        self.completed = completed;
    }

    /// Minimal unattached object for tests of storage, lifecycle and checksum
    /// consumers. Runtime producers use the animation constructor below.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new_unattached_for_test(
        stable_id: AnimId,
        native_unique_id: i32,
        type_id: InternedId,
        world_coord: AnimWorldCoord,
        runtime: AnimRuntime,
    ) -> Self {
        Self {
            stable_id,
            native_unique_id,
            type_id,
            world_coord,
            draw_flags: 0,
            z_adjust: 0,
            remap: None,
            effective_end: 1,
            effective_loop_end: 1,
            runtime,
            draw_runtime: AnimDrawRuntime::default(),
            viewer_hidden: None,
            use_cell_drawer: false,
            terrain_attached: false,
            no_effects: false,
            damage_accumulator: NativeF64Bits::POSITIVE_ZERO,
            in_logic_vector: false,
            owner_entity: None,
            building_slot: None,
            building_anim: false,
            damage_fire_slot: None,
            completed: false,
            start_sound_active: false,
            stop_sound_id: None,
            display: AnimDisplayState::default(),
            bounce: None,
            flaming_guy: None,
            owner_house: None,
            attached_bullet: None,
            owner_terrain: None,
            presentation: None,
            presentation_order: 0,
        }
    }

    /// Mark the fixture's anim as a presentation anim outside the Anim array.
    #[cfg(test)]
    pub(crate) fn mark_presentation_for_test(&mut self) {
        self.presentation = Some(None);
    }

    /// The owning House (`AnimClass+0x180`).
    #[cfg(test)]
    pub fn owner_house(&self) -> Option<InternedId> {
        self.owner_house
    }

    /// The bullet whose expiry ends this anim (`AnimClass+0x17C`).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn attached_bullet(&self) -> Option<u64> {
        self.attached_bullet
    }

    /// The ids `Simulation::anim_pointer_expired` compares: the owner object
    /// (`AnimClass+0xCC`) and the attached bullet (`+0x17C`). VERA-only: feeds
    /// the pointer-expiry listener index (`ExpiryListeners`); a field that
    /// callback compares must be named here too.
    pub(crate) fn expiry_names(&self) -> [Option<u64>; 2] {
        // An anim names at most one owner at `+0xCC`.
        let owner = self.owner_entity.or(self.owner_terrain);
        [owner, self.attached_bullet]
    }

    /// Preserve the former field order; the new damage-fire reverse index is
    /// derived from already-hashed Building slots and never enters this fold.
    pub(crate) fn hash_before_display(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.stable_id.hash(hasher);
        self.native_unique_id.hash(hasher);
        self.type_id.hash(hasher);
        self.world_coord.hash(hasher);
        self.draw_flags.hash(hasher);
        self.z_adjust.hash(hasher);
        self.remap.hash(hasher);
        self.effective_end.hash(hasher);
        self.effective_loop_end.hash(hasher);
        self.runtime.hash(hasher);
        self.draw_runtime.hash(hasher);
        if let Some(snapshot) = &self.viewer_hidden {
            "AnimViewerHidden".hash(hasher);
            snapshot.hash(hasher);
        }
        self.use_cell_drawer.hash(hasher);
        self.terrain_attached.hash(hasher);
        self.in_logic_vector.hash(hasher);
        self.owner_entity.hash(hasher);
        self.building_slot.hash(hasher);
        self.start_sound_active.hash(hasher);
        self.stop_sound_id.hash(hasher);
        self.completed.hash(hasher);
        self.building_anim.hash(hasher);
        // The retained native byte affects later Start/UnInit behavior.
        // False preserves pre-existing hashes and compatibility fixtures.
        if self.no_effects {
            b"anim-no-effects-v1".hash(hasher);
            self.no_effects.hash(hasher);
        }
        if self.damage_accumulator != NativeF64Bits::ONE {
            b"anim-damage-accumulator-v1".hash(hasher);
            self.damage_accumulator.hash(hasher);
        }
        // Unset links add no bytes, preserving earlier streams.
        if self.owner_house.is_some() || self.attached_bullet.is_some() {
            b"anim-owner-house-bullet-v1".hash(hasher);
            self.owner_house.hash(hasher);
            self.attached_bullet.hash(hasher);
        }
        if let Some(state) = &self.flaming_guy {
            b"anim-flaming-guy-v1".hash(hasher);
            state.hash(hasher);
        }
        if let Some(terrain) = self.owner_terrain {
            b"anim-owner-terrain-v1".hash(hasher);
            terrain.hash(hasher);
        }
    }
}

impl std::hash::Hash for AnimObject {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        self.hash_before_display(hasher);
        self.display.hash(hasher);
    }
}

/// The owner centre (its GetCoords) an attached anim's stored coordinate is
/// relative to.
pub(super) fn anim_owner_world_coords(
    owner: &crate::sim::game_entity::GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> AnimWorldCoord {
    let centre = crate::sim::movement::ground_pose::object_get_coords(owner, terrain);
    AnimWorldCoord {
        x: centre.x,
        y: centre.y,
        z: centre.z,
    }
}

/// Anim GetYSort (422BC0 -> Object5F6BD0 -> GetCoords422BE0, plus the
/// retained instance+104): the absolute X + Y plus the adjust. An attached
/// anim stores an owner-relative coordinate, so its key is
/// [`anim_own_sort_term`] plus [`anim_owner_sort_term`] (wrapping, so the
/// terms add in any order).
pub(crate) fn anim_display_sort_key(
    anim: &AnimObject,
    entities: &crate::sim::entity_store::EntityStore,
) -> i32 {
    let owner = anim.owner_entity.and_then(|id| entities.get(id));
    anim_own_sort_term(anim).wrapping_add(owner.map_or(0, anim_owner_sort_term))
}

/// The anim's own part of its GetYSort: the stored coordinate's X + Y plus
/// the retained adjust. Type changes do not recopy the adjust.
pub(crate) fn anim_own_sort_term(anim: &AnimObject) -> i32 {
    anim.world_coord
        .x
        .wrapping_add(anim.world_coord.y)
        .wrapping_add(anim.display.y_sort_adjust())
}

/// What an attached anim's GetYSort adds for its owner: the X + Y of the
/// owner's centre ([`anim_owner_world_coords`], whose Z the key never reads).
pub(crate) fn anim_owner_sort_term(owner: &crate::sim::game_entity::GameEntity) -> i32 {
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(owner);
    x.wrapping_add(y)
}

pub(crate) fn anim_world_coords(
    anim: &AnimObject,
    entities: &crate::sim::entity_store::EntityStore,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> AnimWorldCoord {
    let Some(owner) = anim.owner_entity.and_then(|id| entities.get(id)) else {
        return anim.world_coord;
    };
    let owner = anim_owner_world_coords(owner, terrain);
    AnimWorldCoord {
        x: anim.world_coord.x.wrapping_add(owner.x),
        y: anim.world_coord.y.wrapping_add(owner.y),
        z: anim.world_coord.z.wrapping_add(owner.z),
    }
}
