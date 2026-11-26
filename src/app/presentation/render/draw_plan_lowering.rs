//! Safe lowering from native-shaped tactical plans to existing GPU buffers.
//!
//! The simulation supplies retained Display order. Builders attach pieces to
//! those parent slots before lowering them to atlas draw runs.

use crate::render::batch::SpriteInstance;
use crate::render::surface_line::SurfaceDraw;
use crate::render::tactical_draw_plan::{
    BlitPolicy, BuildingOwnedPlan, BuildingPiece, BuildingPieceKind, DrawId, ObjectDraw,
    RenderZPolicy, SpriteEncoding, TacticalDrawInput, TacticalDrawPlan, TacticalLayer,
};
use crate::util::lookup_hash::{LookupMap, LookupSet};

/// One GPU sprite that remains owned by a single building during lowering.
///
/// Keeping the atlas page alongside the sprite lets the render submission
/// preserve `BuildingClass::Draw` piece order without inferring it from floats.
#[derive(Clone)]
pub(crate) struct PlannedBuildingPieceInstance {
    pub kind: BuildingPieceKind,
    pub z_bias: i32,
    pub policy: BlitPolicy,
    pub target: ObjectTexture,
    pub instance: SpriteInstance,
}

/// Texture/pipeline identity retained after native object ordering is decided.
///
/// Atlas identity is deliberately data carried by a draw piece, never a sort
/// key. Parent order comes exclusively from the Display draw order
/// ([`NativeDisplayOrder`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectTexture {
    /// Ordered DSurface calls, fenced into this parent's native draw slot.
    SurfaceLines(usize),
    /// Temporary System62E280 Spotlight, in this parent's body slot.
    CombatLight(usize),
    /// Initial Building Extras status, sharing the late run's instance range.
    Status(usize),
    /// Per-piece native destination edit; never a texture-only coalesced draw.
    TerrainShp(crate::render::terrain_draw::TerrainPiece),
    /// An atlas SHP whose source mask performs a native destination edit.
    /// The atlas page is payload, never an ordering key.
    SpriteAtlasEdit(usize, crate::render::terrain_draw::TerrainPiece),
    UnitAtlasPage(usize),
    UnitPosePage(usize),
    ShpPage(usize),
}

/// One already-resolved sprite owned by one retained Display parent object.
#[derive(Clone)]
pub(crate) struct ObjectPieceInstance {
    pub target: ObjectTexture,
    /// Which depth pipeline draws it (`RenderZPolicy::None` passthrough,
    /// `ReadOnly` Z-tested, `ReadWrite` Z-tested and written).
    pub render_z: RenderZPolicy,
    pub instance: SpriteInstance,
}

/// A native body that edits the destination without a source sprite.
pub(crate) enum ObjectBodyDraw {
    Surface(SurfaceDraw),
    Spotlight {
        glow: Option<crate::render::combat_light::CombatLightDrawRecord>,
        beams: [Option<SurfaceDraw>; 2],
    },
    CombatLight(crate::render::combat_light::CombatLightDrawRecord),
}

/// Where one planned parent's sprites lie in its plan's buffers
/// ([`PlannedSprites`]). Only a plan makes one, so a span always indexes the
/// buffers of the plan that holds its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PieceSpan {
    start: usize,
    end: usize,
}

impl PieceSpan {
    fn range(self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

/// A planned parent's sprites: ordinary pieces, or a building's own pieces,
/// which the tactical plan orders by kind inside its slot.
#[derive(Clone, Copy, Debug)]
enum ParentSprites {
    Object(PieceSpan),
    Building(PieceSpan),
}

/// The sprites of every parent of one plan, each parent's in a contiguous
/// span. One buffer per frame instead of a list per drawn object: the plan is
/// rebuilt every render frame for every drawn unit, bullet and particle.
#[derive(Default)]
pub(crate) struct PlannedSprites {
    pieces: Vec<ObjectPieceInstance>,
    building_pieces: Vec<PlannedBuildingPieceInstance>,
}

impl PlannedSprites {
    fn object(&mut self, pieces: impl IntoIterator<Item = ObjectPieceInstance>) -> ParentSprites {
        let start = self.pieces.len();
        self.pieces.extend(pieces);
        ParentSprites::Object(PieceSpan {
            start,
            end: self.pieces.len(),
        })
    }

    fn building(
        &mut self,
        pieces: impl IntoIterator<Item = PlannedBuildingPieceInstance>,
    ) -> ParentSprites {
        let start = self.building_pieces.len();
        self.building_pieces.extend(pieces);
        ParentSprites::Building(PieceSpan {
            start,
            end: self.building_pieces.len(),
        })
    }

    /// `sprites` of another plan, copied into these buffers.
    fn copy_of(&mut self, from: &Self, sprites: ParentSprites) -> ParentSprites {
        match sprites {
            ParentSprites::Object(span) => self.object(from.pieces[span.range()].iter().cloned()),
            ParentSprites::Building(span) => {
                self.building(from.building_pieces[span.range()].iter().cloned())
            }
        }
    }
}

/// The end of a plan's piece buffer, lent to one parent's builder
/// ([`PlannedObjects::object_with`]): it appends that parent's pieces and
/// cannot reach the pieces planned before them.
pub(crate) struct PieceSink<'a> {
    pieces: &'a mut Vec<ObjectPieceInstance>,
}

impl Extend<ObjectPieceInstance> for PieceSink<'_> {
    fn extend<I: IntoIterator<Item = ObjectPieceInstance>>(&mut self, pieces: I) {
        self.pieces.extend(pieces);
    }
}

/// One frame's draw plan: the Display parents the class builders planned, in
/// the order they planned them, and the sprites each one's draw owns.
/// [`lower_object_instances`] restores the Display order.
#[derive(Default)]
pub(crate) struct PlannedObjects {
    objects: Vec<PlannedObjectInstance>,
    sprites: PlannedSprites,
}

impl PlannedObjects {
    /// Plans `parent`, drawing `pieces` in this order, and hands it back for
    /// its other draws.
    pub(crate) fn object(
        &mut self,
        parent: ObjectDraw,
        pieces: impl IntoIterator<Item = ObjectPieceInstance>,
    ) -> &mut PlannedObjectInstance {
        let sprites = self.sprites.object(pieces);
        self.push(parent, sprites)
    }

    /// Plans `parent` with the pieces `draw` appends, in that order, and
    /// hands it back for its other draws.
    pub(crate) fn object_with(
        &mut self,
        parent: ObjectDraw,
        draw: impl FnOnce(&mut PieceSink<'_>),
    ) -> &mut PlannedObjectInstance {
        let start = self.sprites.pieces.len();
        draw(&mut PieceSink {
            pieces: &mut self.sprites.pieces,
        });
        let span = PieceSpan {
            start,
            end: self.sprites.pieces.len(),
        };
        self.push(parent, ParentSprites::Object(span))
    }

    /// Plans the building `parent` with its own `pieces`, which stay in its
    /// slot ([`BuildingOwnedPlan`]), and hands it back for its other draws.
    pub(crate) fn building(
        &mut self,
        parent: ObjectDraw,
        pieces: impl IntoIterator<Item = PlannedBuildingPieceInstance>,
    ) -> &mut PlannedObjectInstance {
        let sprites = self.sprites.building(pieces);
        self.push(parent, sprites)
    }

    fn push(&mut self, parent: ObjectDraw, sprites: ParentSprites) -> &mut PlannedObjectInstance {
        let index = self.objects.len();
        self.objects.push(PlannedObjectInstance {
            parent,
            sprites,
            body_draw: None,
            tail_draws: Vec::new(),
            brackets: None,
            status: None,
            tether_redraws: Vec::new(),
            drawn_by_tether: false,
            static_tree: false,
        });
        &mut self.objects[index]
    }

    /// Plans `object` of another plan, whose sprites `from` holds
    /// ([`Self::into_parts`]).
    pub(crate) fn push_from(&mut self, from: &PlannedSprites, object: PlannedObjectInstance) {
        let sprites = self.sprites.copy_of(from, object.sprites);
        self.objects
            .push(PlannedObjectInstance { sprites, ..object });
    }

    /// The parents, and the buffers that hold their sprites.
    pub(crate) fn into_parts(self) -> (Vec<PlannedObjectInstance>, PlannedSprites) {
        (self.objects, self.sprites)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, PlannedObjectInstance> {
        self.objects.iter()
    }

    /// The parents, for the draws assigned to them after their sprites
    /// (brackets, Extras, tether redraws).
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, PlannedObjectInstance> {
        self.objects.iter_mut()
    }

    /// Drops the parents `keep` refuses. Their sprites stay in the buffers,
    /// unreferenced, until the plan is dropped.
    pub(crate) fn retain(&mut self, keep: impl FnMut(&PlannedObjectInstance) -> bool) {
        self.objects.retain(keep);
    }

    /// `object`'s ordinary pieces; empty for a building.
    pub(crate) fn pieces(&self, object: &PlannedObjectInstance) -> &[ObjectPieceInstance] {
        match object.sprites {
            ParentSprites::Object(span) => &self.sprites.pieces[span.range()],
            ParentSprites::Building(_) => &[],
        }
    }

    /// The parents, each with its own sprites, for a test to read or plan
    /// again.
    #[cfg(test)]
    pub(crate) fn into_owned(self) -> Vec<OwnedPlannedObject> {
        self.objects
            .into_iter()
            .map(|object| {
                let (pieces, building_pieces) = match object.sprites {
                    ParentSprites::Object(span) => {
                        (self.sprites.pieces[span.range()].to_vec(), None)
                    }
                    ParentSprites::Building(span) => (
                        Vec::new(),
                        Some(self.sprites.building_pieces[span.range()].to_vec()),
                    ),
                };
                OwnedPlannedObject {
                    object,
                    pieces,
                    building_pieces,
                }
            })
            .collect()
    }
}

/// Tests' form of a planned parent, which holds its own sprites: what a test
/// builds by hand or reads back ([`PlannedObjects::into_owned`]). A list of
/// them plans again in its order (`PlannedObjects: From<Vec<_>>`).
#[cfg(test)]
pub(crate) struct OwnedPlannedObject {
    object: PlannedObjectInstance,
    pub pieces: Vec<ObjectPieceInstance>,
    pub building_pieces: Option<Vec<PlannedBuildingPieceInstance>>,
}

#[cfg(test)]
impl OwnedPlannedObject {
    pub(crate) fn object(parent: ObjectDraw, pieces: Vec<ObjectPieceInstance>) -> Self {
        let mut plan = PlannedObjects::default();
        plan.object(parent, pieces);
        plan.into_owned().remove(0)
    }

    pub(crate) fn building(parent: ObjectDraw, pieces: Vec<PlannedBuildingPieceInstance>) -> Self {
        let mut plan = PlannedObjects::default();
        plan.building(parent, pieces);
        plan.into_owned().remove(0)
    }
}

/// The planned parent's own fields (`parent`, `status`, `tail_draws` and the
/// rest), as a test reads and sets them.
#[cfg(test)]
impl std::ops::Deref for OwnedPlannedObject {
    type Target = PlannedObjectInstance;

    fn deref(&self) -> &PlannedObjectInstance {
        &self.object
    }
}

#[cfg(test)]
impl std::ops::DerefMut for OwnedPlannedObject {
    fn deref_mut(&mut self) -> &mut PlannedObjectInstance {
        &mut self.object
    }
}

#[cfg(test)]
impl From<Vec<OwnedPlannedObject>> for PlannedObjects {
    fn from(owned: Vec<OwnedPlannedObject>) -> Self {
        owned.into_iter().collect()
    }
}

#[cfg(test)]
impl FromIterator<OwnedPlannedObject> for PlannedObjects {
    fn from_iter<I: IntoIterator<Item = OwnedPlannedObject>>(owned: I) -> Self {
        let mut plan = Self::default();
        for parent in owned {
            let sprites = match parent.building_pieces {
                Some(pieces) => plan.sprites.building(pieces),
                None => plan.sprites.object(parent.pieces),
            };
            plan.objects.push(PlannedObjectInstance {
                sprites,
                ..parent.object
            });
        }
        plan
    }
}

/// One native Display registration and every sprite its display call owns.
/// Its plan ([`PlannedObjects`]) holds the sprites.
pub(crate) struct PlannedObjectInstance {
    pub parent: ObjectDraw,
    sprites: ParentSprites,
    pub body_draw: Option<ObjectBodyDraw>,
    /// Destination stores the parent's draw call makes after its sprites, such
    /// as the target laser at the end of `UnitClass::Draw_It` (`0x0073D43A`).
    pub tail_draws: Vec<SurfaceDraw>,
    pub brackets: Option<crate::app::presentation::selection_brackets::BuildingBracketLines>,
    pub status: Option<crate::app::presentation::ui_overlays::TechnoExtras>,
    /// The contacts a building draws again at the start of its Display slot,
    /// in radio order ([`assign_tether_redraws`]).
    ///
    /// [`assign_tether_redraws`]: crate::app::presentation::instances::assign_tether_redraws
    pub tether_redraws: Vec<DrawId>,
    /// A Unit that draws only in its tether building's slot.
    pub drawn_by_tether: bool,
    /// A live Terrain object whose type is not `IsAnimated`. The Display loop
    /// skips it (`Tactical_ObjectRenderingLoop @ 0x006D8DB0` draws RTTI 0x24
    /// only when `IsAnimated` (type+0x2B3) or crumbling (+0xCD) is set);
    /// `0x006D97D0` draws it instead, walking Ground backward, from
    /// `Tactical_layer_building_overlays @ 0x006D3AC0` (`0x006D44D3`), before
    /// the building bases ([`ObjectLayerPass::static_tree_runs`]).
    pub static_tree: bool,
}

impl PlannedObjectInstance {
    /// A building parent, whose body opportunity counts even with no pixels.
    pub(crate) fn is_building(&self) -> bool {
        matches!(self.sprites, ParentSprites::Building(_))
    }
}

/// One contiguous GPU-buffer run sharing a texture/pipeline binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObjectDrawRun {
    pub target: ObjectTexture,
    pub render_z: RenderZPolicy,
    /// Destination-dependent material is composed once per retained parent.
    /// Opaque runs leave this empty and retain cross-parent atlas batching.
    pub packed_parent: Option<DrawId>,
    pub start: u32,
    pub count: u32,
}

/// The second Extras sweep (6D9789), in retained Display parent order.
/// Lines use live Z; ordinary health stores never read or write it. Native
///6F5190 submits each parent's clock/wrench/rank before its health and pips.
pub(crate) enum ObjectExtrasRun {
    Lines {
        #[cfg(test)]
        parent: DrawId,
        lines: std::ops::Range<usize>,
        batch: crate::render::terrain_draw::SurfaceLineBatch,
    },
    Status {
        #[cfg(test)]
        parent: DrawId,
        bomb_clock: Option<u32>,
        wrench: Option<u32>,
        /// Ares POWEROFF marker (10049FE0), after the wrench; `None` unless
        /// the Rules were read the Ares way (`techno_extras_for_parent`).
        power_off: Option<u32>,
        rank: Option<u32>,
        /// The rank's custom insignia SHP; `None` binds the stock PIPS strip.
        rank_insignia: Option<String>,
        background: Option<u32>,
        shield_background: Option<u32>,
        fills: std::ops::Range<u32>,
        scale: std::ops::Range<u32>,
        self_heal: Option<u32>,
        group_rectangles: std::ops::Range<u32>,
        group_text: std::ops::Range<u32>,
        /// Phobos digital display text: per-entry backing/glyph ranges in
        /// list order (rectangles, then glyphs).
        digital_text: Vec<(std::ops::Range<u32>, std::ops::Range<u32>)>,
        /// Phobos digital display custom-SHP glyphs: catalog index and the
        /// uploaded instance index, in emission order.
        digital_shapes: Vec<(u32, u32)>,
        talk_bubble: Option<u32>,
    },
}

/// One parent's fixed-pixel health draw after wheel resampling.
pub(crate) struct FrameHealthBar {
    pub background: Option<u32>,
    pub shield_background: Option<u32>,
    pub fills: std::ops::Range<u32>,
}

/// One native layer output: exact integer parent order plus flat GPU instances.
#[derive(Default)]
pub(crate) struct ObjectLayerPass {
    pub instances: Vec<SpriteInstance>,
    pub runs: Vec<ObjectDrawRun>,
    /// Building6D9920 draws bases before rally6D4648. Base and later dynamic
    /// Display runs share one flat instance upload; each range draws once.
    pub building_base_runs: Vec<ObjectDrawRun>,
    /// Static trees ([`PlannedObjectInstance::static_tree`]) in reverse
    /// Ground order: `0x006D97D0` counts down from the last member
    /// (`0x006D9805`, `0x006D98FA`/`0x006D98FF` DEC/JNS). They draw once,
    /// before `building_base_runs`, from the same instance upload.
    pub static_tree_runs: Vec<ObjectDrawRun>,
    /// Native6D913C/6D9153 lines and Particle body points, in parent order.
    pub surface_draws: Vec<Vec<crate::render::surface_line::SurfaceDraw>>,
    /// Frame-local uploads of `surface_draws`, prepared after the shared A plane.
    pub line_batches: Vec<crate::render::terrain_draw::SurfaceLineBatch>,
    pub light_records: Vec<crate::render::combat_light::CombatLightDrawRecord>,
    /// Derived offset into this frame's single light upload; assigned on prepare.
    pub light_record_base: usize,
    /// Native6D9789 repeats Extras in the same five retained layer orders.
    pub extras_lines: Vec<crate::render::surface_line::SurfaceLine>,
    pub extras_instances: Vec<SpriteInstance>,
    pub extras_runs: Vec<ObjectExtrasRun>,
    /// Wheel-zoom health commands removed from both world Extras visits.
    /// Indices still refer to the single `extras_instances` upload.
    pub frame_health: Vec<FrameHealthBar>,
    /// Aligned with `instances`; retained for executable ordering checks.
    #[cfg(test)]
    pub owners: Vec<DrawId>,
}

/// Frame-local draw order of the Display members
/// (`Simulation::display_draw_order`): the simulation owns registration, and
/// the presentation sorts Ground off the simulation's frame. Built once for
/// each presentation frame (`PresentationFrame::display_order`) for every
/// Display member and presentation anim and only looked up, so it takes the
/// lookup hasher, sized up front.
#[derive(Clone, Debug, Default)]
pub(crate) struct NativeDisplayOrder {
    positions: LookupMap<DrawId, (TacticalLayer, u64)>,
    ordered: Vec<(crate::sim::world::display_layers::DisplayLayer, u64)>,
}

impl NativeDisplayOrder {
    /// `sim`'s draw order under `rules` (`Simulation::picture_draw_order`),
    /// each presentation anim where an ordinary anim of its submission would
    /// stand. They hold no Display slot, so [`Self::ordered`] leaves them
    /// out: every other draw there is registered.
    pub(crate) fn from_world(
        sim: &crate::sim::world::Simulation,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> Self {
        let mut order = Self::ranked(sim.picture_draw_order(rules)).0;
        let display = sim.display_layers();
        order
            .ordered
            .retain(|&(_, id)| display.layer_of(id).is_some());
        order
    }

    /// The order of `ordered`, every Display member with its layer in draw
    /// order.
    #[cfg(test)]
    pub(crate) fn from_ordered(
        ordered: Vec<(crate::sim::world::display_layers::DisplayLayer, u64)>,
    ) -> Self {
        Self::ranked(ordered).0
    }

    /// [`Self::from_ordered`] and each layer's member count.
    fn ranked(
        ordered: Vec<(crate::sim::world::display_layers::DisplayLayer, u64)>,
    ) -> (Self, [u64; 5]) {
        let mut positions = LookupMap::with_capacity_and_hasher(ordered.len(), Default::default());
        let mut rank = [0u64; 5];
        for &(layer, id) in &ordered {
            let next = &mut rank[usize::from(layer.index())];
            positions.insert(id, (TacticalLayer(layer.index()), *next));
            *next += 1;
        }
        (Self { positions, ordered }, rank)
    }

    /// Every Display member with its layer, in draw order.
    pub(crate) fn ordered(&self) -> &[(crate::sim::world::display_layers::DisplayLayer, u64)] {
        &self.ordered
    }

    /// The order of a bare `display`, Ground as it holds its members.
    #[cfg(test)]
    pub(crate) fn from_display(display: &crate::sim::world::display_layers::DisplayLayers) -> Self {
        Self::from_ordered(display.members_in_order().collect())
    }

    #[cfg(test)]
    pub(crate) fn new(ids: &[DrawId]) -> Self {
        Self {
            positions: ids
                .iter()
                .enumerate()
                .map(|(rank, &id)| (id, (TacticalLayer(2), rank as u64)))
                .collect(),
            ordered: ids
                .iter()
                .map(|&id| (crate::sim::world::display_layers::DisplayLayer::GROUND, id))
                .collect(),
        }
    }

    /// `id`'s place in the Display walk, `None` for an id no layer holds:
    /// ordering by it is the order of [`Self::ordered`], with each
    /// presentation anim in its place (`Simulation::picture_draw_order`).
    pub(crate) fn position(&self, id: DrawId) -> Option<(TacticalLayer, u64)> {
        self.positions.get(&id).copied()
    }

    pub(crate) fn object_draw(&self, id: DrawId, encoding: SpriteEncoding) -> Option<ObjectDraw> {
        let policy = match encoding {
            SpriteEncoding::Terrain => BlitPolicy::z_none(encoding),
            _ => BlitPolicy::z_read(encoding),
        };
        let &(layer, display_order) = self.positions.get(&id)?;
        Some(ObjectDraw {
            id,
            layer,
            display_order,
            policy,
        })
    }
}

/// Restore the Display draw order ([`NativeDisplayOrder`]) after the
/// class-specific builders. Native Tactical6D8F39 reads members in order;
/// sprite depth does not participate. Building pieces remain contiguous in
/// their slot.
pub(crate) fn lower_object_instances(plan: PlannedObjects) -> [ObjectLayerPass; 5] {
    let (entries, sprites) = plan.into_parts();
    // Side tables of the frame's Display parents, keyed by parent id. They
    // are only filled and then read by key in plan order, never walked, so
    // they take the lookup hasher: a B-tree shifted the large status values
    // on every insert, each frame for every drawn object.
    let parents = entries.len();
    let mut ordinary = LookupMap::with_capacity_and_hasher(parents, Default::default());
    let mut buildings = LookupMap::default();
    let mut brackets = LookupMap::default();
    let mut status = LookupMap::with_capacity_and_hasher(parents, Default::default());
    let mut body_draws = LookupMap::default();
    let mut tail_draws = LookupMap::default();
    let mut tether_redraws = LookupMap::default();
    let mut drawn_by_tether = LookupSet::default();
    let mut static_trees = LookupSet::default();
    let mut inputs = Vec::with_capacity(entries.len());

    for entry in entries {
        let id = entry.parent.id;
        if !entry.tether_redraws.is_empty() {
            tether_redraws.insert(id, entry.tether_redraws);
        }
        if entry.drawn_by_tether {
            drawn_by_tether.insert(id);
        }
        if entry.static_tree {
            static_trees.insert(id);
        }
        if let Some(draw) = entry.body_draw {
            assert!(
                matches!(entry.sprites, ParentSprites::Object(span) if span.range().is_empty()),
                "one Display body uses sprites or one surface operation"
            );
            assert!(body_draws.insert(id, draw).is_none());
        }
        if !entry.tail_draws.is_empty() {
            assert!(tail_draws.insert(id, entry.tail_draws).is_none());
        }
        if let Some(lines) = entry.brackets {
            assert!(brackets.insert(id, lines).is_none());
        }
        if let Some(strip) = entry.status {
            assert!(status.insert(id, strip).is_none());
        }
        match entry.sprites {
            ParentSprites::Building(span) => {
                // A building's pieces carry their own index as local id. This
                // list stays one Vec per building: `TacticalDrawPlan` owns it
                // and stable-sorts it by piece kind.
                let planned = sprites.building_pieces[span.range()]
                    .iter()
                    .enumerate()
                    .map(|(index, piece)| BuildingPiece {
                        id: index as DrawId,
                        kind: piece.kind,
                        z_bias: piece.z_bias,
                        policy: piece.policy,
                    })
                    .collect();
                assert!(
                    buildings.insert(id, span).is_none(),
                    "each Display parent must be emitted once"
                );
                inputs.push(TacticalDrawInput::Building(BuildingOwnedPlan {
                    parent: entry.parent,
                    pieces: planned,
                }));
            }
            ParentSprites::Object(span) => {
                assert!(
                    ordinary.insert(id, span).is_none(),
                    "each Display parent must be emitted once"
                );
                inputs.push(TacticalDrawInput::Object(entry.parent));
            }
        }
    }

    let plan = TacticalDrawPlan::build(inputs);
    let mut layers: [ObjectLayerPass; 5] = std::array::from_fn(|_| ObjectLayerPass::default());
    // Each layer's static trees, drawn after its walk; one list for every layer.
    let mut static_tree_pieces = Vec::new();
    for layer in plan.object_layers {
        let lowered = &mut layers[usize::from(layer.layer.0)];
        for entry in layer.entries {
            let owner = entry.object().id;
            let is_building = matches!(
                entry,
                crate::render::tactical_draw_plan::LayerEntry::Building(_)
            );
            let lines = brackets.remove(&owner);
            let strip = status.remove(&owner).map(|strip| {
                let bomb_clock = strip.bomb_clock.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                let wrench = strip.wrench.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                let power_off = strip.power_off.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                let rank = strip.rank.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                let background = strip.background.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                let shield_background = strip.shield_background.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                let start = lowered.extras_instances.len() as u32;
                lowered.extras_instances.extend(strip.fills);
                let end = lowered.extras_instances.len() as u32;
                lowered.extras_instances.extend(strip.scale);
                let scale_end = lowered.extras_instances.len() as u32;
                let self_heal = strip.self_heal.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                let rectangles_start = lowered.extras_instances.len() as u32;
                lowered.extras_instances.extend(strip.group.rectangles);
                let rectangles_end = lowered.extras_instances.len() as u32;
                lowered.extras_instances.extend(strip.group.text);
                let text_end = lowered.extras_instances.len() as u32;
                // VERA's Convenience Help rows follow Phobos' DigitalDisplay
                // text: the same GAME.FNT material, drawn in list order.
                let mut digital_text =
                    Vec::with_capacity(strip.digital_text.len() + strip.info_text.len());
                for text in strip.digital_text.into_iter().chain(strip.info_text) {
                    let rectangles_start = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.extend(text.rectangles);
                    let rectangles_end = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.extend(text.text);
                    let text_end = lowered.extras_instances.len() as u32;
                    digital_text.push((rectangles_start..rectangles_end, rectangles_end..text_end));
                }
                let digital_shapes = strip
                    .digital_shapes
                    .into_iter()
                    .map(|(catalog, instance)| {
                        let index = lowered.extras_instances.len() as u32;
                        lowered.extras_instances.push(instance);
                        (catalog, index)
                    })
                    .collect();
                let talk_bubble = strip.talk_bubble.map(|instance| {
                    let index = lowered.extras_instances.len() as u32;
                    lowered.extras_instances.push(instance);
                    index
                });
                ObjectExtrasRun::Status {
                    #[cfg(test)]
                    parent: owner,
                    bomb_clock,
                    wrench,
                    power_off,
                    rank,
                    rank_insignia: strip.rank_insignia,
                    background,
                    shield_background,
                    fills: start..end,
                    scale: end..scale_end,
                    self_heal,
                    group_rectangles: rectangles_start..rectangles_end,
                    group_text: rectangles_end..text_end,
                    digital_text,
                    digital_shapes,
                    talk_bubble,
                }
            });
            // Clock6F524E, wrench6F5340 and rank6F5382 precede Building lines
            //6F5396..5E10; health6F5E27 follows. Split that prefix when lines
            // fence it, retaining one upload for both native Extras visits.
            let mut split_prefix = None;
            if lines.as_ref().is_some_and(|lines| !lines.extras.is_empty()) {
                if let Some(ObjectExtrasRun::Status {
                    bomb_clock,
                    wrench,
                    power_off,
                    rank,
                    rank_insignia,
                    fills,
                    ..
                }) = &strip
                    && (bomb_clock.is_some()
                        || wrench.is_some()
                        || rank.is_some()
                        || power_off.is_some())
                {
                    split_prefix = Some(lowered.extras_runs.len());
                    lowered.extras_runs.push(ObjectExtrasRun::Status {
                        #[cfg(test)]
                        parent: owner,
                        bomb_clock: *bomb_clock,
                        wrench: *wrench,
                        power_off: *power_off,
                        rank: *rank,
                        rank_insignia: rank_insignia.clone(),
                        background: None,
                        shield_background: None,
                        fills: fills.start..fills.start,
                        scale: fills.start..fills.start,
                        self_heal: None,
                        group_rectangles: fills.start..fills.start,
                        group_text: fills.start..fills.start,
                        digital_text: Vec::new(),
                        digital_shapes: Vec::new(),
                        talk_bubble: None,
                    });
                }
            }
            if let Some(lines) = lines {
                if let Some(index) = split_prefix.filter(|_| is_building) {
                    push_surface_draws(
                        lowered,
                        lines
                            .behind
                            .into_iter()
                            .map(crate::render::surface_line::SurfaceDraw::Line),
                    );
                    push_initial_status(lowered, index);
                    push_surface_draws(
                        lowered,
                        lines
                            .extras
                            .iter()
                            .copied()
                            .map(crate::render::surface_line::SurfaceDraw::Line),
                    );
                } else {
                    let mut before = lines.behind;
                    before.extend_from_slice(&lines.extras);
                    push_surface_draws(
                        lowered,
                        before
                            .into_iter()
                            .map(crate::render::surface_line::SurfaceDraw::Line),
                    );
                }
                let start = lowered.extras_lines.len();
                lowered.extras_lines.extend(lines.extras);
                let end = lowered.extras_lines.len();
                if start != end {
                    lowered.extras_runs.push(ObjectExtrasRun::Lines {
                        #[cfg(test)]
                        parent: owner,
                        lines: start..end,
                        batch: Default::default(),
                    });
                }
            }
            if let Some(ObjectExtrasRun::Status {
                bomb_clock,
                wrench,
                power_off,
                rank,
                rank_insignia,
                background,
                shield_background,
                fills,
                scale,
                self_heal,
                group_rectangles,
                group_text,
                digital_text,
                digital_shapes,
                talk_bubble,
                ..
            }) = strip
            {
                let bomb_clock = bomb_clock.filter(|_| split_prefix.is_none());
                let wrench = wrench.filter(|_| split_prefix.is_none());
                let power_off = power_off.filter(|_| split_prefix.is_none());
                let marker = power_off.is_some();
                let rank = rank.filter(|_| split_prefix.is_none());
                if bomb_clock.is_some()
                    || wrench.is_some()
                    || marker
                    || rank.is_some()
                    || background.is_some()
                    || shield_background.is_some()
                    || !fills.is_empty()
                    || !scale.is_empty()
                    || self_heal.is_some()
                    || !group_rectangles.is_empty()
                    || !group_text.is_empty()
                    || !digital_text.is_empty()
                    || !digital_shapes.is_empty()
                    || talk_bubble.is_some()
                {
                    if is_building {
                        // Tactical6D9153 calls Building Extras before its body;
                        //6D9789 calls it again later. One frame-local strip is
                        // replayed twice, never produced or uploaded twice.
                        push_initial_status(lowered, lowered.extras_runs.len());
                    }
                    lowered.extras_runs.push(ObjectExtrasRun::Status {
                        #[cfg(test)]
                        parent: owner,
                        bomb_clock,
                        wrench,
                        power_off,
                        rank,
                        rank_insignia,
                        background,
                        shield_background,
                        fills,
                        scale,
                        self_heal,
                        group_rectangles,
                        group_text,
                        digital_text,
                        digital_shapes,
                        talk_bubble,
                    });
                }
            }
            if let Some(body) = body_draws.remove(&owner) {
                match body {
                    ObjectBodyDraw::Surface(draw) => {
                        push_surface_draws(lowered, std::iter::once(draw))
                    }
                    ObjectBodyDraw::Spotlight { glow, beams } => {
                        if let Some(record) = glow {
                            push_combat_light(lowered, record);
                        }
                        push_surface_draws(lowered, beams.into_iter().flatten());
                    }
                    ObjectBodyDraw::CombatLight(record) => {
                        push_combat_light(lowered, record);
                    }
                }
            }
            // Display6D916C's43DA80 opens with the tether redraw (43DAA0), after
            // this building's Extras and before its voxel turret. It repeats
            // each contact's own draw, sprites and tail stores alike, whichever
            // of the two Display slots comes first: neither slot takes the
            // contact's entries out of their tables.
            for contact in tether_redraws.remove(&owner).unwrap_or_default() {
                let Some(&span) = ordinary.get(&contact) else {
                    continue;
                };
                for piece in &sprites.pieces[span.range()] {
                    let run = push_object_piece(lowered, contact, piece);
                    append_object_run(&mut lowered.runs, run);
                }
                if let Some(tails) = tail_draws.get(&contact) {
                    push_surface_draws(lowered, tails.iter().copied());
                }
            }
            let refused = drawn_by_tether.contains(&owner);
            match entry {
                crate::render::tactical_draw_plan::LayerEntry::Object(_) => {
                    let span = *ordinary
                        .get(&owner)
                        .expect("planned Display object must retain its sprites");
                    if static_trees.contains(&owner) {
                        static_tree_pieces.push((owner, span));
                        continue;
                    }
                    // Unit73B0B0 refuses the body to a tethered Unit; its
                    // building's redraw draws it.
                    if !refused {
                        for piece in &sprites.pieces[span.range()] {
                            let run = push_object_piece(lowered, owner, piece);
                            append_object_run(&mut lowered.runs, run);
                        }
                    }
                }
                crate::render::tactical_draw_plan::LayerEntry::Building(building) => {
                    let span = buildings
                        .remove(&owner)
                        .expect("planned building must retain its owned sprites");
                    let pieces = &sprites.building_pieces[span.range()];
                    for planned in building.pieces {
                        let piece = usize::try_from(planned.id)
                            .ok()
                            .and_then(|index| pieces.get(index))
                            .expect("planned building piece must retain its sprite");
                        let piece = ObjectPieceInstance {
                            target: piece.target,
                            render_z: piece.policy.render_z,
                            instance: piece.instance,
                        };
                        let run = push_object_piece(lowered, owner, &piece);
                        // Building6D9920 ->43CEA0(...,0) draws43D290's base
                        // before rally6D4648. Display6D916C passes1 instead:
                        //43CEA0 dispatches only the voxel turret43DA80 there.
                        let runs = if planned.kind == BuildingPieceKind::PoweredOrActiveOverlay {
                            &mut lowered.runs
                        } else {
                            &mut lowered.building_base_runs
                        };
                        append_object_run(runs, run);
                    }
                }
            }
            if let Some(draws) = tail_draws.get(&owner).filter(|_| !refused) {
                push_surface_draws(lowered, draws.iter().copied());
            }
        }
        for (owner, span) in static_tree_pieces.drain(..).rev() {
            for piece in &sprites.pieces[span.range()] {
                let run = push_object_piece(lowered, owner, piece);
                append_object_run(&mut lowered.static_tree_runs, run);
            }
        }
    }
    layers
}

/// FoggedObjectClass::DrawAll4D1890 has its own sorted index, outside
/// Display. Lower in supplied order using the same primitive GPU writers.
pub(crate) fn lower_fogged_object_pieces(
    entries: impl IntoIterator<Item = (DrawId, Vec<ObjectPieceInstance>)>,
) -> ObjectLayerPass {
    let mut pass = ObjectLayerPass::default();
    for (id, pieces) in entries {
        for piece in &pieces {
            let run = push_object_piece(&mut pass, id, piece);
            append_object_run(&mut pass.runs, run);
        }
    }
    pass
}

fn push_combat_light(
    pass: &mut ObjectLayerPass,
    record: crate::render::combat_light::CombatLightDrawRecord,
) {
    let index = pass.light_records.len();
    pass.light_records.push(record);
    pass.runs.push(ObjectDrawRun {
        target: ObjectTexture::CombatLight(index),
        render_z: RenderZPolicy::None,
        packed_parent: None,
        start: pass.instances.len() as u32,
        count: 0,
    });
}

fn push_surface_draws(
    pass: &mut ObjectLayerPass,
    draws: impl IntoIterator<Item = crate::render::surface_line::SurfaceDraw>,
) {
    // Adjacent surface-only bodies share immutable Z. Keeping their ordered
    // stores in one batch avoids a snapshot/pass pair per Spark, while any
    // sprite, status or light command remains a destination/order fence.
    if let Some(ObjectDrawRun {
        target: ObjectTexture::SurfaceLines(index),
        ..
    }) = pass.runs.last()
    {
        pass.surface_draws[*index].extend(draws);
        return;
    }
    let index = pass.surface_draws.len();
    pass.surface_draws.push(draws.into_iter().collect());
    pass.runs.push(ObjectDrawRun {
        target: ObjectTexture::SurfaceLines(index),
        render_z: RenderZPolicy::ReadOnly,
        packed_parent: None,
        start: pass.instances.len() as u32,
        count: 0,
    });
}

fn push_initial_status(pass: &mut ObjectLayerPass, index: usize) {
    pass.runs.push(ObjectDrawRun {
        target: ObjectTexture::Status(index),
        render_z: RenderZPolicy::None,
        packed_parent: None,
        start: pass.instances.len() as u32,
        count: 0,
    });
}

#[cfg(test)]
pub(crate) fn lower_ground_object_instances(plan: impl Into<PlannedObjects>) -> ObjectLayerPass {
    let mut layers = lower_object_instances(plan.into());
    std::mem::take(&mut layers[2])
}

fn push_object_piece(
    pass: &mut ObjectLayerPass,
    owner: DrawId,
    piece: &ObjectPieceInstance,
) -> ObjectDrawRun {
    // Unit73B140 composites its opaque source cache once; SHP705E00 selects
    // the packed leaves through Convert490E50. Source pieces retain their
    // parent here so hull/turret/barrel are not blended separately. Native
    // selectors and coverage: procedural_drawing_oracle/translucent_blitter_a.md.
    let packed_body = matches!(
        piece.target,
        ObjectTexture::UnitAtlasPage(_)
            | ObjectTexture::UnitPosePage(_)
            | ObjectTexture::ShpPage(_)
            | ObjectTexture::TerrainShp(crate::render::terrain_draw::TerrainPiece::Body)
    ) && piece.instance.draw_state.native_selector_bits() & 6 != 0
        && piece.instance.draw_state.fx_flags & crate::render::draw_state::FX_SHADOW == 0;
    // Anim42389E can select the no-Z shadow leaf496250. Its nonzero index
    // stencil uses the existing packed-half operation, not Terrain's Z-tested
    // edit pipeline. Retain the parent so it also fences adjacent edit spans.
    let packed_shadow = matches!(
        piece.target,
        ObjectTexture::SpriteAtlasEdit(_, crate::render::terrain_draw::TerrainPiece::Shadow)
    ) && piece.render_z == RenderZPolicy::None;
    let packed_parent = (packed_body || packed_shadow).then_some(owner);
    let start = pass.instances.len() as u32;
    let run = ObjectDrawRun {
        target: piece.target,
        render_z: piece.render_z,
        packed_parent,
        start,
        count: 1,
    };
    pass.instances.push(piece.instance);
    #[cfg(test)]
    pass.owners.push(owner);
    run
}

fn append_object_run(runs: &mut Vec<ObjectDrawRun>, next: ObjectDrawRun) {
    if let Some(run) = runs.last_mut().filter(|run| {
        run.target == next.target
            && run.render_z == next.render_z
            && run.packed_parent == next.packed_parent
            && run.start + run.count == next.start
    }) {
        run.count += next.count;
    } else {
        runs.push(next);
    }
}

#[cfg(all(test, tests_app_presentation_render))]
mod tests {
    use super::*;
    use crate::render::draw_state::DrawState;
    use crate::render::tactical_draw_plan::{BlitPolicy, RenderZPolicy, SpriteEncoding};

    #[test]
    fn selected_brackets_follow_display_parents_and_fence_body_batching() {
        use crate::app::presentation::selection_brackets::BuildingBracketLines;
        use crate::render::surface_line::{SurfaceLine, SurfaceLineBlend};
        let line = |color| SurfaceLine {
            from: [0, 0],
            to: [8, 0],
            z_adjust: [14, 14],
            blend: SurfaceLineBlend::Replace(color),
        };
        let mut plan = PlannedObjects::default();
        let mut parent = |id, display_order, selected| {
            let parent = plan.object(
                ObjectDraw {
                    id,
                    layer: TacticalLayer(2),
                    display_order,
                    policy: BlitPolicy::z_read(SpriteEncoding::Rle),
                },
                [ObjectPieceInstance {
                    target: ObjectTexture::ShpPage(0),
                    render_z: RenderZPolicy::ReadOnly,
                    instance: SpriteInstance::default(),
                }],
            );
            if selected {
                parent.brackets = Some(BuildingBracketLines {
                    behind: vec![line(id as u16)],
                    extras: vec![line(id as u16 + 1)],
                });
            }
        };
        // Storage/ID order differs from Display. Original6D913C/9153 precede
        // this parent's body;6D9789 repeats Extras in that same Display order.
        parent(1, 2, true);
        parent(500, 1, false);
        parent(900, 0, true);
        let pass = lower_ground_object_instances(plan);
        assert_eq!(pass.owners, [900, 500, 1]);
        assert_eq!(
            pass.surface_draws,
            [
                vec![SurfaceDraw::Line(line(900)), SurfaceDraw::Line(line(901))],
                vec![SurfaceDraw::Line(line(1)), SurfaceDraw::Line(line(2))]
            ]
        );
        assert_eq!(pass.extras_lines, [line(901), line(2)]);
        assert!(matches!(&pass.extras_runs[..], [
            ObjectExtrasRun::Lines { parent: 900, lines: a, .. },
            ObjectExtrasRun::Lines { parent: 1, lines: b, .. },
        ] if *a == (0..1) && *b == (1..2)));
        assert!(matches!(
            pass.runs[0].target,
            ObjectTexture::SurfaceLines(0)
        ));
        assert_eq!((pass.runs[1].start, pass.runs[1].count), (0, 2));
        assert!(matches!(
            pass.runs[2].target,
            ObjectTexture::SurfaceLines(1)
        ));
        assert_eq!((pass.runs[3].start, pass.runs[3].count), (2, 1));
    }

    /// A parent's tail stores (the target laser at the end of
    /// `UnitClass::Draw_It`, `0x0073D43A`) follow its own sprites in its
    /// Display slot and precede the next parent's.
    #[test]
    fn tail_draws_follow_their_parents_sprites() {
        let store = |word| SurfaceDraw::OpaqueRect {
            rect: [0, 0, 1, 1],
            word,
        };
        let mut plan = PlannedObjects::default();
        let mut parent = |id, display_order, tail_draws: Vec<SurfaceDraw>| {
            let piece = || ObjectPieceInstance {
                target: ObjectTexture::ShpPage(0),
                render_z: RenderZPolicy::ReadOnly,
                instance: SpriteInstance::default(),
            };
            plan.object(
                ObjectDraw {
                    id,
                    layer: TacticalLayer(2),
                    display_order,
                    policy: BlitPolicy::z_read(SpriteEncoding::Rle),
                },
                [piece(), piece()],
            )
            .tail_draws = tail_draws;
        };
        parent(8, 1, Vec::new());
        parent(7, 0, vec![store(1), store(2)]);
        let pass = lower_ground_object_instances(plan);
        assert_eq!(pass.owners, [7, 7, 8, 8]);
        assert_eq!(pass.surface_draws, [vec![store(1), store(2)]]);
        assert_eq!(
            pass.runs
                .iter()
                .map(|run| (run.target, run.start, run.count))
                .collect::<Vec<_>>(),
            [
                (ObjectTexture::ShpPage(0), 0, 2),
                (ObjectTexture::SurfaceLines(0), 2, 0),
                (ObjectTexture::ShpPage(0), 2, 2),
            ]
        );
    }

    #[test]
    fn selected_nonrookie_building_keeps_rank_between_behind_and_extras_lines() {
        use crate::app::presentation::{
            selection_brackets::BuildingBracketLines, ui_overlays::TechnoExtras,
        };
        use crate::render::surface_line::{SurfaceLine, SurfaceLineBlend};
        let line = |color| SurfaceLine {
            from: [0, 0],
            to: [8, 0],
            z_adjust: [14, 14],
            blend: SurfaceLineBlend::Replace(color),
        };
        let mut plan = PlannedObjects::default();
        let parent = plan.building(
            ObjectDraw {
                id: 42,
                layer: TacticalLayer(2),
                display_order: 0,
                policy: BlitPolicy::z_read(SpriteEncoding::Rle),
            },
            [PlannedBuildingPieceInstance {
                kind: BuildingPieceKind::Body,
                z_bias: 0,
                policy: BlitPolicy::z_read(SpriteEncoding::Rle),
                target: ObjectTexture::ShpPage(0),
                instance: SpriteInstance::default(),
            }],
        );
        parent.brackets = Some(BuildingBracketLines {
            behind: vec![line(10)],
            extras: vec![line(11)],
        });
        parent.status = Some(TechnoExtras {
            bomb_clock: None,
            wrench: None,
            rank: Some(SpriteInstance::default()),
            background: None,
            shield_background: None,
            fills: vec![SpriteInstance::default(); 15],
            scale: Vec::new(),
            group: Default::default(),
            talk_bubble: None,
            self_heal: None,
            ..Default::default()
        });
        let pass = lower_ground_object_instances(plan);
        // Native6D9920 draws the base before rally. Display6D913C Behind,
        //6D9153 Extras:6F5382 rank,6F5396..5E10 selected Building lines,
        //6F5E27 health, then the dynamic turret.6D9789 repeats rank->Extras
        // lines->health, using the same uploaded instances.
        assert_eq!(
            pass.surface_draws,
            [
                vec![SurfaceDraw::Line(line(10))],
                vec![SurfaceDraw::Line(line(11))]
            ]
        );
        assert_eq!(pass.extras_instances.len(), 16);
        assert!(matches!(&pass.extras_runs[..], [
            ObjectExtrasRun::Status { parent: 42, rank: Some(0), background: None, fills: a, .. },
            ObjectExtrasRun::Lines { parent: 42, lines, .. },
            ObjectExtrasRun::Status { parent: 42, rank: None, background: None, fills: b, .. },
        ] if a.is_empty() && *lines == (0..1) && *b == (1..16)));
        assert_eq!(
            pass.runs.iter().map(|run| run.target).collect::<Vec<_>>(),
            [
                ObjectTexture::SurfaceLines(0),
                ObjectTexture::Status(0),
                ObjectTexture::SurfaceLines(1),
                ObjectTexture::Status(2),
            ]
        );
        assert_eq!(
            pass.building_base_runs
                .iter()
                .map(|run| run.target)
                .collect::<Vec<_>>(),
            [ObjectTexture::ShpPage(0)]
        );
    }

    /// Convenience Help's rows take a range of the status run, as Phobos'
    /// DigitalDisplay text does; uploaded without one, nothing drew them.
    #[test]
    fn convenience_help_rows_reach_the_status_runs_text_ranges() {
        use crate::app::presentation::ui_overlays::{StatusTextOverlayInstances, TechnoExtras};
        let mut plan = PlannedObjects::default();
        let parent = plan.building(
            ObjectDraw {
                id: 7,
                layer: TacticalLayer(2),
                display_order: 0,
                policy: BlitPolicy::z_read(SpriteEncoding::Rle),
            },
            [],
        );
        parent.status = Some(TechnoExtras {
            info_text: vec![StatusTextOverlayInstances {
                rectangles: vec![SpriteInstance::default()],
                text: vec![SpriteInstance::default(); 2],
            }],
            ..Default::default()
        });
        let pass = lower_ground_object_instances(plan);
        assert!(matches!(&pass.extras_runs[..], [
            ObjectExtrasRun::Status { digital_text, .. },
        ] if *digital_text == [(0..1, 1..3)]));
    }

    #[test]
    fn static_trees_leave_display_and_draw_backward_before_the_bases() {
        use crate::render::terrain_draw::TerrainPiece;
        let piece = |target, render_z| ObjectPieceInstance {
            target,
            render_z,
            instance: SpriteInstance::default(),
        };
        let mut plan = PlannedObjects::default();
        let mut parent = |id, static_tree| {
            plan.object(
                ObjectDraw {
                    id,
                    layer: TacticalLayer(2),
                    display_order: id,
                    policy: BlitPolicy::z_read(SpriteEncoding::Terrain),
                },
                [
                    piece(
                        ObjectTexture::TerrainShp(TerrainPiece::Body),
                        RenderZPolicy::ReadWrite,
                    ),
                    piece(
                        ObjectTexture::TerrainShp(TerrainPiece::Shadow),
                        RenderZPolicy::ReadWrite,
                    ),
                ],
            )
            .static_tree = static_tree;
        };
        // Ground order 1, 2, 3; 2 stands for an animated tree or a unit.
        parent(1, true);
        parent(2, false);
        parent(3, true);
        let pass = lower_ground_object_instances(plan);
        let owners = |runs: &[ObjectDrawRun]| -> Vec<DrawId> {
            runs.iter()
                .flat_map(|run| run.start..run.start + run.count)
                .map(|index| pass.owners[index as usize])
                .collect()
        };
        // 0x006D97D0 counts down; each tree draws its body, then its shadow.
        assert_eq!(owners(&pass.static_tree_runs), [3, 3, 1, 1]);
        assert_eq!(
            pass.static_tree_runs
                .iter()
                .map(|run| run.target)
                .collect::<Vec<_>>(),
            [
                ObjectTexture::TerrainShp(TerrainPiece::Body),
                ObjectTexture::TerrainShp(TerrainPiece::Shadow),
                ObjectTexture::TerrainShp(TerrainPiece::Body),
                ObjectTexture::TerrainShp(TerrainPiece::Shadow),
            ]
        );
        // 0x006D8DB0 skips them: only the Display member remains.
        assert_eq!(owners(&pass.runs), [2, 2]);
    }

    #[test]
    fn native_non_entity_lifetimes_reach_their_layers_without_family_or_page_sorting() {
        use crate::sim::world::display_layers::{DisplayLayer, DisplayLayers};
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/display_non_entity.json",
        ))
        .unwrap();
        for row in rows {
            let actors = row["input"]["actors"].as_array().unwrap();
            let mut keys: Vec<_> = row["queries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|query| query["key"].as_i64().map(|key| key as i32))
                .collect();
            let mut display = DisplayLayers::default();
            for (step, expected) in row["input"]["steps"]
                .as_array()
                .unwrap()
                .iter()
                .zip(row["after_steps"].as_array().unwrap())
            {
                let actor = step["actor"].as_u64().map(|id| id as usize);
                match step["op"].as_str().unwrap() {
                    "submit" => {
                        let actor = actor.unwrap();
                        let layer = DisplayLayer::from_index(
                            row["queries"][actor]["layer"].as_u64().unwrap() as u8,
                        );
                        display.submit(actor as u64 + 1, layer);
                    }
                    "remove" => {
                        display.remove(actor.unwrap() as u64 + 1);
                    }
                    "coordinates" => {
                        keys[actor.unwrap()] = Some(
                            (step["xyz"][0].as_i64().unwrap() as i32)
                                .wrapping_add(step["xyz"][1].as_i64().unwrap() as i32),
                        );
                    }
                    // VERA's simulation keeps Ground unordered; the picture
                    // sorts it at once (`Simulation::ground_draw_order`).
                    "sort" => {}
                    other => panic!("unexpected native step {other}"),
                }
                // Load must preserve the retained order even when it is only
                // partly sorted. Removed actors remain in storage below.
                let display: DisplayLayers = crate::util::save_codec::deserialize(
                    &crate::util::save_codec::serialize(&display).unwrap(),
                )
                .unwrap();
                // Ground by key, as `Simulation::ground_draw_order` sorts it.
                // Equal keys by id here: this checks that lowering keeps the
                // order it is handed, not how ties are broken.
                let ground_key = |id: u64| (keys[id as usize - 1].unwrap_or(i32::MIN), id);
                let mut ordered: Vec<_> = display.members_in_order().collect();
                // Stable: the other layers keep their members' order.
                ordered.sort_by_key(|&(layer, id)| {
                    let key = if layer == DisplayLayer::GROUND {
                        ground_key(id)
                    } else {
                        (0, 0)
                    };
                    (layer.index(), key)
                });
                let order = NativeDisplayOrder::from_ordered(ordered);
                let mut entries = PlannedObjects::default();
                for (index, actor) in actors.iter().enumerate().rev() {
                    let id = index as u64 + 1;
                    let Some(parent) = order.object_draw(id, SpriteEncoding::Plain) else {
                        continue;
                    };
                    let pieces: Vec<_> = if actor["kind"] == "bullet" {
                        [
                            crate::render::terrain_draw::TerrainPiece::Shadow,
                            crate::render::terrain_draw::TerrainPiece::Body,
                        ]
                        .into_iter()
                        .map(|piece| {
                            marked_piece(
                                ObjectTexture::SpriteAtlasEdit(index % 2, piece),
                                id as u32,
                            )
                        })
                        .collect()
                    } else {
                        vec![marked_piece(ObjectTexture::ShpPage(index % 2), id as u32)]
                    };
                    entries.object(parent, pieces);
                }
                let lowered = lower_object_instances(entries);
                for (index, pass) in lowered.iter().enumerate() {
                    let mut members: Vec<u64> = expected[index]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|value| value.as_u64().unwrap())
                        .collect();
                    if index == usize::from(DisplayLayer::GROUND.index()) {
                        members.sort_by_key(|&actor| ground_key(actor + 1));
                    }
                    let owners: Vec<_> = members
                        .iter()
                        .flat_map(|&value| {
                            let actor = value as usize;
                            std::iter::repeat_n(
                                actor as u64 + 1,
                                if actors[actor]["kind"] == "bullet" {
                                    2
                                } else {
                                    1
                                },
                            )
                        })
                        .collect();
                    assert_eq!(
                        pass.owners, owners,
                        "{}: {step}, layer{index}",
                        row["input"]["name"]
                    );
                }
            }
        }
    }

    #[test]
    fn native_ground_history_reaches_entity_picking_and_atlas_lowering() {
        use crate::app::presentation::instances::tactical_entity_encounter_order;
        use crate::app::presentation::render::draw_plan_lowering::{
            NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjects,
            lower_ground_object_instances,
        };
        use crate::render::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;
        use crate::sim::world::display_layers::DisplayLayer;
        use crate::util::fixed_math::SimFixed;

        // Original instructions produce a partially sorted vector after each
        // 551A30 call and reach the sorted order after three. VERA's
        // simulation keeps Ground unordered and the picture sorts it at once
        // (`Simulation::ground_draw_order`, a sim-scale deviation): picking and
        // drawing see the order native settles on as soon as members move.
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/crate_ground_membership.json",
        ))
        .unwrap();
        let row = rows
            .iter()
            .find(|row| row["input"]["name"] == "one_adjacent_sort_pass_per_call")
            .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let type_id = sim.intern("E1");
        for (i, actor) in row["input"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                i as u64 + 1,
                10,
                10,
                0,
                0,
                owner,
                crate::sim::combat::Health::new(100),
                type_id,
                crate::map::entities::EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.position.set_sub_x_for_test(SimFixed::from_num(
                128 + actor["delta"][0].as_i64().unwrap(),
            ));
            entity.position.set_sub_y_for_test(SimFixed::from_num(128));
            sim.entities_mut_for_test().insert(entity);
        }
        sim.set_logic_order_for_test(vec![4, 3, 2, 1]);
        let settled = row["after_steps"].as_array().unwrap().last().unwrap()["layers"][2].clone();
        let mut moved = false;
        for (step, observed) in row["input"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["after_steps"].as_array().unwrap())
        {
            let id = step["actor"].as_u64().map(|index| index + 1);
            match step["op"].as_str().unwrap() {
                "submit" => sim.submit_object_display_for_test(id.unwrap(), DisplayLayer::GROUND),
                "coordinates" => {
                    moved = true;
                    let entity = sim.entity_mut_for_test(id.unwrap()).unwrap();
                    entity.position.set_sub_x_for_test(SimFixed::from_num(
                        step["xyz"][0].as_i64().unwrap() - 2560,
                    ));
                    entity.position.set_sub_y_for_test(SimFixed::from_num(
                        step["xyz"][1].as_i64().unwrap() - 2560,
                    ));
                }
                "sort" => {
                    sim.advance_tick(&[], None, None, None, 50);
                }
                op => panic!("unexpected {op}"),
            }
            // Between the moves the order follows whichever keys moved so far.
            if moved && step["op"] == "coordinates" {
                continue;
            }
            let native = if moved {
                &settled
            } else {
                &observed["layers"][2]
            };
            let expected: Vec<_> = native
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_u64().unwrap() + 1)
                .collect();
            assert_eq!(
                tactical_entity_encounter_order(&sim, None),
                expected,
                "pick: {step}"
            );
            let order = NativeDisplayOrder::new(&sim.ground_draw_order(None));
            let mut entries = PlannedObjects::default();
            for id in (1..=4).rev() {
                if let Some(parent) = order.object_draw(id, SpriteEncoding::Plain) {
                    entries.object(
                        parent,
                        [ObjectPieceInstance {
                            target: ObjectTexture::ShpPage(id as usize % 2),
                            render_z: RenderZPolicy::ReadOnly,
                            instance: Default::default(),
                        }],
                    );
                }
            }
            assert_eq!(
                lower_ground_object_instances(entries).owners,
                expected,
                "draw: {step}"
            );
        }
    }

    fn marked_piece(target: ObjectTexture, marker: u32) -> ObjectPieceInstance {
        ObjectPieceInstance {
            target,
            render_z: RenderZPolicy::ReadOnly,
            instance: SpriteInstance {
                draw_state: DrawState {
                    fx_flags: marker,
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }

    fn plain_parent(order: &NativeDisplayOrder, id: DrawId) -> ObjectDraw {
        order
            .object_draw(id, SpriteEncoding::Plain)
            .expect("registered parent")
    }

    #[test]
    fn building_base_and_dynamic_draws_follow_native_phases_and_ground_order() {
        // Original6D9920 walks Ground's8A0394 vector, selects Building RTTI6,
        // and calls43CEA0/43D290 before rally6D4648. Mobile bodies and the
        // independently registered slot Anims remain in the later6D8DB0 pass.
        let order = NativeDisplayOrder::new(&[10, 20, 30, 40]);
        let mut plan = PlannedObjects::default();
        let building = |plan: &mut PlannedObjects, id, pieces: &[(BuildingPieceKind, u32)]| {
            plan.building(
                plain_parent(&order, id),
                pieces
                    .iter()
                    .map(|&(kind, marker)| PlannedBuildingPieceInstance {
                        kind,
                        z_bias: 0,
                        policy: if kind == BuildingPieceKind::PoweredOrActiveOverlay {
                            BlitPolicy::z_read(SpriteEncoding::Voxel)
                        } else {
                            BlitPolicy::opaque(SpriteEncoding::Plain)
                        },
                        target: if kind == BuildingPieceKind::PoweredOrActiveOverlay {
                            ObjectTexture::UnitAtlasPage(7)
                        } else {
                            ObjectTexture::ShpPage(0)
                        },
                        instance: marked_piece(ObjectTexture::ShpPage(0), marker).instance,
                    }),
            );
        };
        building(&mut plan, 40, &[(BuildingPieceKind::Body, 40)]);
        plan.object(
            plain_parent(&order, 30),
            [marked_piece(ObjectTexture::ShpPage(0), 30)],
        );
        building(
            &mut plan,
            20,
            &[
                (BuildingPieceKind::Bib, 22),
                (BuildingPieceKind::Shadow, 21),
                (BuildingPieceKind::Body, 20),
                (BuildingPieceKind::PoweredOrActiveOverlay, 29),
            ],
        );
        plan.object(
            plain_parent(&order, 10),
            [marked_piece(ObjectTexture::UnitAtlasPage(0), 10)],
        );
        let pass = lower_ground_object_instances(plan);
        let early_markers: Vec<_> = pass
            .building_base_runs
            .iter()
            .flat_map(|run| run.start..run.start + run.count)
            .map(|index| pass.instances[index as usize].draw_state.fx_flags)
            .collect();
        assert_eq!(early_markers, [20, 21, 22, 40]);
        let display_markers: Vec<_> = pass
            .runs
            .iter()
            .flat_map(|run| run.start..run.start + run.count)
            .map(|index| pass.instances[index as usize].draw_state.fx_flags)
            .collect();
        assert_eq!(display_markers, [10, 29, 30]);
        assert_eq!(pass.owners, [10, 20, 20, 20, 20, 30, 40]);
    }

    /// Display6D916C's43DA80 redraws an unloading building's tethered
    /// contacts (43DAA0) after its Extras and before its voxel turret, and
    /// Unit73B0B0 refuses the tethered Unit its own slot, laser included.
    /// The redraw does not depend on which slot comes first; a contact the
    /// Unit side does not refuse draws twice, and a refused one no building
    /// redraws does not draw.
    #[test]
    fn an_unloading_building_draws_its_tethered_contacts_in_its_own_slot() {
        let laser = |marker: i32| {
            SurfaceDraw::Line(crate::render::surface_line::SurfaceLine {
                from: [marker, 0],
                to: [marker, 8],
                z_adjust: [0, 0],
                blend: crate::render::surface_line::SurfaceLineBlend::Replace(0),
            })
        };
        let order = NativeDisplayOrder::new(&[10, 20, 30, 40, 50, 60]);
        let mut plan = PlannedObjects::default();
        let unit = |plan: &mut PlannedObjects, id, refused| {
            let unit = plan.object(
                plain_parent(&order, id),
                [marked_piece(ObjectTexture::UnitAtlasPage(0), id as u32)],
            );
            unit.tail_draws = vec![laser(id as i32)];
            unit.drawn_by_tether = refused;
        };
        let factory = |plan: &mut PlannedObjects, id, contacts: Vec<DrawId>| {
            plan.building(
                plain_parent(&order, id),
                [
                    (BuildingPieceKind::Body, id as u32),
                    (BuildingPieceKind::PoweredOrActiveOverlay, id as u32 + 1),
                ]
                .into_iter()
                .map(|(kind, marker)| PlannedBuildingPieceInstance {
                    kind,
                    z_bias: 0,
                    policy: if kind == BuildingPieceKind::Body {
                        BlitPolicy::opaque(SpriteEncoding::Plain)
                    } else {
                        BlitPolicy::z_read(SpriteEncoding::Voxel)
                    },
                    target: ObjectTexture::ShpPage(0),
                    instance: marked_piece(ObjectTexture::ShpPage(0), marker).instance,
                }),
            )
            .tether_redraws = contacts;
        };
        // 10 is drawn by 20, 40 by 30 (40's slot comes later), 50 is redrawn
        // by 30 without refusing its own slot, and 60 refuses with no redraw.
        unit(&mut plan, 10, true);
        factory(&mut plan, 20, vec![10]);
        factory(&mut plan, 30, vec![40, 50]);
        unit(&mut plan, 40, true);
        unit(&mut plan, 50, false);
        unit(&mut plan, 60, true);
        let pass = lower_ground_object_instances(plan);
        let display_markers: Vec<_> = pass
            .runs
            .iter()
            .flat_map(|run| run.start..run.start + run.count)
            .map(|index| pass.instances[index as usize].draw_state.fx_flags)
            .collect();
        assert_eq!(display_markers, [10, 21, 40, 50, 31, 50]);
        assert_eq!(
            pass.surface_draws,
            [
                vec![laser(10)],
                vec![laser(40)],
                vec![laser(50)],
                vec![laser(50)]
            ]
        );
    }

    #[test]
    fn packed_material_retains_parent_across_all_native_sprite_sources() {
        let order = NativeDisplayOrder::new(&[1, 2, 3, 4, 5]);
        let packed = |target, marker, bits| {
            let mut piece = marked_piece(target, marker);
            piece.instance.draw_state.fx_params[1] = bits as f32;
            piece
        };
        let mut plan = PlannedObjects::default();
        plan.object(
            plain_parent(&order, 2),
            [packed(ObjectTexture::UnitAtlasPage(0), 2, 4)],
        );
        plan.object(
            plain_parent(&order, 1),
            [
                packed(ObjectTexture::UnitAtlasPage(0), 11, 4),
                packed(ObjectTexture::UnitAtlasPage(1), 12, 4),
                packed(ObjectTexture::UnitPosePage(0), 13, 4),
            ],
        );
        plan.object(
            plain_parent(&order, 3),
            [packed(ObjectTexture::ShpPage(0), 3, 12)],
        );
        plan.object(
            plain_parent(&order, 4),
            [packed(ObjectTexture::ShpPage(0), 4, 2)],
        );
        plan.object(
            plain_parent(&order, 5),
            [packed(
                ObjectTexture::TerrainShp(crate::render::terrain_draw::TerrainPiece::Body),
                5,
                4,
            )],
        );
        let pass = lower_ground_object_instances(plan);
        assert_eq!(pass.owners, [1, 1, 1, 2, 3, 4, 5]);
        assert_eq!(
            pass.runs
                .iter()
                .map(|run| run.packed_parent)
                .collect::<Vec<_>>(),
            [
                Some(1),
                Some(1),
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5)
            ],
            "atlas coalescing must not merge distinct destination composites"
        );
    }

    #[test]
    fn gsi_13_03_equal_tree_unit_building_use_registration_not_atlas() {
        let order = NativeDisplayOrder::new(&[20, 30, 10]);
        let mut plan = PlannedObjects::default();
        plan.object(
            order.object_draw(10, SpriteEncoding::Terrain).unwrap(),
            [marked_piece(ObjectTexture::ShpPage(1), 10)],
        );
        plan.building(
            order.object_draw(30, SpriteEncoding::Plain).unwrap(),
            [PlannedBuildingPieceInstance {
                kind: BuildingPieceKind::Body,
                z_bias: 0,
                policy: BlitPolicy::opaque(SpriteEncoding::Plain),
                target: ObjectTexture::ShpPage(0),
                instance: marked_piece(ObjectTexture::ShpPage(0), 30).instance,
            }],
        );
        plan.object(
            plain_parent(&order, 20),
            [marked_piece(ObjectTexture::UnitAtlasPage(9), 20)],
        );
        let pass = lower_ground_object_instances(plan);

        assert_eq!(pass.owners, [20, 30, 10]);
        assert_eq!(
            pass.runs.iter().map(|run| run.target).collect::<Vec<_>>(),
            [ObjectTexture::UnitAtlasPage(9), ObjectTexture::ShpPage(1),]
        );
        assert_eq!(
            pass.building_base_runs
                .iter()
                .map(|run| run.target)
                .collect::<Vec<_>>(),
            [ObjectTexture::ShpPage(0)]
        );
    }

    #[test]
    fn gsi_13_03_building_pieces_remain_contiguous_in_parent_slot() {
        let order = NativeDisplayOrder::new(&[1, 2, 3]);
        let building_piece = |kind, target, marker| PlannedBuildingPieceInstance {
            kind,
            z_bias: 0,
            policy: BlitPolicy::opaque(SpriteEncoding::Plain),
            target,
            instance: marked_piece(target, marker).instance,
        };
        let mut plan = PlannedObjects::default();
        plan.object(
            plain_parent(&order, 3),
            [marked_piece(ObjectTexture::UnitAtlasPage(0), 3)],
        );
        plan.building(
            plain_parent(&order, 2),
            [
                building_piece(
                    BuildingPieceKind::PoweredOrActiveOverlay,
                    ObjectTexture::UnitAtlasPage(2),
                    23,
                ),
                building_piece(BuildingPieceKind::Body, ObjectTexture::ShpPage(1), 22),
                building_piece(
                    BuildingPieceKind::Shadow,
                    ObjectTexture::SpriteAtlasEdit(
                        0,
                        crate::render::terrain_draw::TerrainPiece::Shadow,
                    ),
                    24,
                ),
                building_piece(BuildingPieceKind::Bib, ObjectTexture::ShpPage(0), 21),
            ],
        );
        plan.object(
            plain_parent(&order, 1),
            [marked_piece(ObjectTexture::ShpPage(2), 1)],
        );
        let pass = lower_ground_object_instances(plan);

        assert_eq!(pass.owners, [1, 2, 2, 2, 2, 3]);
        assert_eq!(
            pass.instances
                .iter()
                .map(|instance| instance.draw_state.fx_flags)
                .collect::<Vec<_>>(),
            [1, 22, 24, 21, 23, 3]
        );
    }

    /// A whole frame's mix, handed over out of Display order: a static tree,
    /// a unit with turret pieces and a laser, an unloading factory and the
    /// unit it redraws, an anim, a bullet, a plain building, and an Air
    /// bullet and aircraft. Every sprite reaches its run list and place in
    /// it, piece by piece.
    #[test]
    fn a_mixed_frame_lowers_every_piece_in_display_order() {
        use crate::sim::world::display_layers::DisplayLayer;
        let laser = SurfaceDraw::OpaqueRect {
            rect: [0, 0, 1, 1],
            word: 2,
        };
        let order = NativeDisplayOrder::from_ordered(
            (1..=9)
                .map(|id| (DisplayLayer::GROUND, id))
                .chain([(DisplayLayer::AIR, 20), (DisplayLayer::AIR, 21)])
                .collect(),
        );
        let mut plan = PlannedObjects::default();
        let object = |plan: &mut PlannedObjects, id, markers: &[u32]| {
            plan.object(
                plain_parent(&order, id),
                markers
                    .iter()
                    .map(|&marker| marked_piece(ObjectTexture::ShpPage(0), marker)),
            );
        };
        let building = |plan: &mut PlannedObjects, id, pieces: &[(BuildingPieceKind, u32)]| {
            plan.building(
                plain_parent(&order, id),
                pieces
                    .iter()
                    .map(|&(kind, marker)| PlannedBuildingPieceInstance {
                        kind,
                        z_bias: 0,
                        policy: BlitPolicy::opaque(SpriteEncoding::Plain),
                        target: ObjectTexture::ShpPage(1),
                        instance: marked_piece(ObjectTexture::ShpPage(1), marker).instance,
                    }),
            );
        };
        fn last(plan: &mut PlannedObjects) -> &mut PlannedObjectInstance {
            plan.objects.last_mut().expect("just planned")
        }
        object(&mut plan, 21, &[211, 210]);
        object(&mut plan, 8, &[80]);
        building(&mut plan, 9, &[(BuildingPieceKind::Body, 90)]);
        object(&mut plan, 5, &[50, 51]);
        last(&mut plan).drawn_by_tether = true;
        object(&mut plan, 20, &[200]);
        object(&mut plan, 7, &[70]);
        last(&mut plan).static_tree = true;
        object(&mut plan, 2, &[20, 21, 22]);
        last(&mut plan).tail_draws = vec![laser];
        object(&mut plan, 6, &[61, 60]);
        building(
            &mut plan,
            4,
            &[
                (BuildingPieceKind::PoweredOrActiveOverlay, 43),
                (BuildingPieceKind::Bib, 42),
                (BuildingPieceKind::Body, 40),
                (BuildingPieceKind::Shadow, 41),
            ],
        );
        last(&mut plan).tether_redraws = vec![5];
        object(&mut plan, 3, &[30]);
        object(&mut plan, 1, &[10, 11]);
        last(&mut plan).static_tree = true;
        let layers = lower_object_instances(plan);
        let markers = |pass: &ObjectLayerPass, runs: &[ObjectDrawRun]| -> Vec<(DrawId, u32)> {
            runs.iter()
                .flat_map(|run| run.start..run.start + run.count)
                .map(|index| {
                    let index = index as usize;
                    (
                        pass.owners[index],
                        pass.instances[index].draw_state.fx_flags,
                    )
                })
                .collect()
        };
        let ground = &layers[2];
        assert_eq!(
            markers(ground, &ground.runs),
            [
                (2, 20),
                (2, 21),
                (2, 22),
                (3, 30),
                (5, 50),
                (5, 51),
                (4, 43),
                (6, 61),
                (6, 60),
                (8, 80)
            ]
        );
        assert_eq!(
            markers(ground, &ground.building_base_runs),
            [(4, 40), (4, 41), (4, 42), (9, 90)]
        );
        assert_eq!(
            markers(ground, &ground.static_tree_runs),
            [(7, 70), (1, 10), (1, 11)]
        );
        assert_eq!(ground.surface_draws, [vec![laser]]);
        let air = &layers[3];
        assert_eq!(markers(air, &air.runs), [(20, 200), (21, 211), (21, 210)]);
        assert!(
            [0, 1, 4]
                .into_iter()
                .all(|layer| layers[layer].instances.is_empty())
        );
    }
}
