//! Safe lowering from native-shaped tactical plans to existing GPU buffers.
//!
//! The simulation supplies retained Display order. Builders attach pieces to
//! those parent slots before lowering them to atlas draw runs.

use std::collections::BTreeMap;

use crate::render::batch::SpriteInstance;
use crate::render::surface_line::SurfaceDraw;
use crate::render::tactical_draw_plan::{
    BlitPolicy, BuildingOwnedPlan, BuildingPiece, BuildingPieceKind, DrawId, ObjectDraw,
    RenderZPolicy, SpriteEncoding, TacticalDrawInput, TacticalDrawPlan, TacticalLayer,
};

/// One GPU sprite that remains owned by a single building during lowering.
///
/// Keeping the atlas page alongside the sprite lets the render submission
/// preserve `BuildingClass::Draw` piece order without inferring it from floats.
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
/// key. Parent order comes exclusively from the retained Display vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectTexture {
    /// Ordered DSurface calls, fenced into this parent's native draw slot.
    SurfaceLines(usize),
    /// Temporary System62E280 Spotlight, in this parent's body slot.
    CombatLight(usize),
    /// Initial Building Extras status, sharing the late run's instance range.
    Status(usize),
    OverlayAtlas,
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

/// One native Display registration and every sprite its display call owns.
pub(crate) struct PlannedObjectInstance {
    pub parent: ObjectDraw,
    pub pieces: Vec<ObjectPieceInstance>,
    pub building_pieces: Option<Vec<PlannedBuildingPieceInstance>>,
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
}

impl PlannedObjectInstance {
    pub(crate) fn object(parent: ObjectDraw, pieces: Vec<ObjectPieceInstance>) -> Self {
        Self {
            parent,
            pieces,
            building_pieces: None,
            body_draw: None,
            tail_draws: Vec::new(),
            brackets: None,
            status: None,
            tether_redraws: Vec::new(),
            drawn_by_tether: false,
        }
    }

    pub(crate) fn building(parent: ObjectDraw, pieces: Vec<PlannedBuildingPieceInstance>) -> Self {
        Self {
            parent,
            pieces: Vec::new(),
            building_pieces: Some(pieces),
            body_draw: None,
            tail_draws: Vec::new(),
            brackets: None,
            status: None,
            tether_redraws: Vec::new(),
            drawn_by_tether: false,
        }
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
        rank: Option<u32>,
        background: Option<u32>,
        fills: std::ops::Range<u32>,
        scale: std::ops::Range<u32>,
        self_heal: Option<u32>,
        group_rectangles: std::ops::Range<u32>,
        group_text: std::ops::Range<u32>,
        talk_bubble: Option<u32>,
    },
}

/// One native layer output: exact integer parent order plus flat GPU instances.
#[derive(Default)]
pub(crate) struct ObjectLayerPass {
    pub instances: Vec<SpriteInstance>,
    pub runs: Vec<ObjectDrawRun>,
    /// Building6D9920 draws bases before rally6D4648. Base and later dynamic
    /// Display runs share one flat instance upload; each range draws once.
    pub building_base_runs: Vec<ObjectDrawRun>,
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
    /// Aligned with `instances`; retained for executable ordering checks.
    #[cfg(test)]
    pub owners: Vec<DrawId>,
}

/// Frame-local lookup derived solely from the five retained Display vectors.
/// The sim owns all registration and sorting; atlas builders only read ranks.
#[derive(Default)]
pub(crate) struct NativeDisplayOrder {
    positions: BTreeMap<DrawId, (TacticalLayer, u64)>,
}

impl NativeDisplayOrder {
    pub(crate) fn from_display(display: &crate::sim::world::display_layers::DisplayLayers) -> Self {
        Self {
            positions: (0..5u8)
                .flat_map(|index| {
                    let layer = crate::sim::world::display_layers::DisplayLayer::from_index(index)
                        .expect("native Display layer index");
                    display
                        .members(layer)
                        .iter()
                        .enumerate()
                        .map(move |(rank, &id)| (id, (TacticalLayer(index), rank as u64)))
                })
                .collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(ids: &[DrawId]) -> Self {
        Self {
            positions: ids
                .iter()
                .enumerate()
                .map(|(rank, &id)| (id, (TacticalLayer(2), rank as u64)))
                .collect(),
        }
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

/// Restore the retained Display vector order after the class-specific builders.
/// Native Tactical6D8F39 reads members in order; neither sprite depth nor a new
/// GetYSort query participates. Building pieces remain contiguous in their slot.
pub(crate) fn lower_object_instances(entries: Vec<PlannedObjectInstance>) -> [ObjectLayerPass; 5] {
    let mut ordinary = BTreeMap::new();
    let mut buildings = BTreeMap::new();
    let mut brackets = BTreeMap::new();
    let mut status = BTreeMap::new();
    let mut body_draws = BTreeMap::new();
    let mut tail_draws = BTreeMap::new();
    let mut tether_redraws = BTreeMap::new();
    let mut drawn_by_tether = std::collections::BTreeSet::new();
    let mut inputs = Vec::with_capacity(entries.len());

    for entry in entries {
        let id = entry.parent.id;
        if !entry.tether_redraws.is_empty() {
            tether_redraws.insert(id, entry.tether_redraws);
        }
        if entry.drawn_by_tether {
            drawn_by_tether.insert(id);
        }
        if let Some(draw) = entry.body_draw {
            assert!(
                entry.pieces.is_empty() && entry.building_pieces.is_none(),
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
        if let Some(pieces) = entry.building_pieces {
            let mut resolved = BTreeMap::new();
            let planned = pieces
                .into_iter()
                .enumerate()
                .map(|(index, piece)| {
                    let piece_id = index as DrawId;
                    assert!(
                        resolved
                            .insert(
                                piece_id,
                                ObjectPieceInstance {
                                    target: piece.target,
                                    render_z: piece.policy.render_z,
                                    instance: piece.instance,
                                },
                            )
                            .is_none(),
                        "each building-owned piece must have a unique local id"
                    );
                    BuildingPiece {
                        id: piece_id,
                        kind: piece.kind,
                        z_bias: piece.z_bias,
                        policy: piece.policy,
                    }
                })
                .collect();
            assert!(
                buildings.insert(id, resolved).is_none(),
                "each Display parent must be emitted once"
            );
            inputs.push(TacticalDrawInput::Building(BuildingOwnedPlan {
                parent: entry.parent,
                pieces: planned,
            }));
        } else {
            assert!(
                ordinary.insert(id, entry.pieces).is_none(),
                "each Display parent must be emitted once"
            );
            inputs.push(TacticalDrawInput::Object(entry.parent));
        }
    }

    // A tether redraw repeats each contact's own draw, sprites and tail stores
    // alike, whichever of the two Display slots comes first.
    let mut redrawn: BTreeMap<DrawId, Vec<(DrawId, Vec<ObjectPieceInstance>, Vec<SurfaceDraw>)>> =
        tether_redraws
            .into_iter()
            .map(|(building, contacts)| {
                let draws = contacts
                    .into_iter()
                    .filter_map(|contact: DrawId| {
                        let pieces = ordinary.get(&contact)?.clone();
                        let tails = tail_draws.get(&contact).cloned().unwrap_or_default();
                        Some((contact, pieces, tails))
                    })
                    .collect();
                (building, draws)
            })
            .collect();
    let plan = TacticalDrawPlan::build(inputs);
    let mut layers: [ObjectLayerPass; 5] = std::array::from_fn(|_| ObjectLayerPass::default());
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
                    rank,
                    background,
                    fills: start..end,
                    scale: end..scale_end,
                    self_heal,
                    group_rectangles: rectangles_start..rectangles_end,
                    group_text: rectangles_end..text_end,
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
                    rank,
                    fills,
                    ..
                }) = &strip
                    && (bomb_clock.is_some() || wrench.is_some() || rank.is_some())
                {
                    split_prefix = Some(lowered.extras_runs.len());
                    lowered.extras_runs.push(ObjectExtrasRun::Status {
                        #[cfg(test)]
                        parent: owner,
                        bomb_clock: *bomb_clock,
                        wrench: *wrench,
                        rank: *rank,
                        background: None,
                        fills: fills.start..fills.start,
                        scale: fills.start..fills.start,
                        self_heal: None,
                        group_rectangles: fills.start..fills.start,
                        group_text: fills.start..fills.start,
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
                rank,
                background,
                fills,
                scale,
                self_heal,
                group_rectangles,
                group_text,
                talk_bubble,
                ..
            }) = strip
            {
                let bomb_clock = bomb_clock.filter(|_| split_prefix.is_none());
                let wrench = wrench.filter(|_| split_prefix.is_none());
                let rank = rank.filter(|_| split_prefix.is_none());
                if bomb_clock.is_some()
                    || wrench.is_some()
                    || rank.is_some()
                    || background.is_some()
                    || !fills.is_empty()
                    || !scale.is_empty()
                    || self_heal.is_some()
                    || !group_rectangles.is_empty()
                    || !group_text.is_empty()
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
                        rank,
                        background,
                        fills,
                        scale,
                        self_heal,
                        group_rectangles,
                        group_text,
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
            // this building's Extras and before its voxel turret.
            for (contact, pieces, tails) in redrawn.remove(&owner).unwrap_or_default() {
                for piece in pieces {
                    let run = push_object_piece(lowered, contact, piece);
                    append_object_run(&mut lowered.runs, run);
                }
                if !tails.is_empty() {
                    push_surface_draws(lowered, tails);
                }
            }
            let refused = drawn_by_tether.contains(&owner);
            match entry {
                crate::render::tactical_draw_plan::LayerEntry::Object(_) => {
                    let pieces = ordinary
                        .remove(&owner)
                        .expect("planned Display object must retain its sprites");
                    // Unit73B0B0 refuses the body to a tethered Unit; its
                    // building's redraw draws it.
                    for piece in pieces.into_iter().filter(|_| !refused) {
                        let run = push_object_piece(lowered, owner, piece);
                        append_object_run(&mut lowered.runs, run);
                    }
                }
                crate::render::tactical_draw_plan::LayerEntry::Building(building) => {
                    let mut pieces = buildings
                        .remove(&owner)
                        .expect("planned building must retain its owned sprites");
                    for planned in building.pieces {
                        let piece = pieces
                            .remove(&planned.id)
                            .expect("planned building piece must retain its sprite");
                        let run = push_object_piece(lowered, owner, piece);
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
            if let Some(draws) = tail_draws.remove(&owner).filter(|_| !refused) {
                push_surface_draws(lowered, draws);
            }
        }
    }
    layers
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
pub(crate) fn lower_ground_object_instances(
    entries: Vec<PlannedObjectInstance>,
) -> ObjectLayerPass {
    let mut layers = lower_object_instances(entries);
    std::mem::take(&mut layers[2])
}

fn push_object_piece(
    pass: &mut ObjectLayerPass,
    owner: DrawId,
    piece: ObjectPieceInstance,
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

#[cfg(test)]
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
        let parent = |id, display_order, selected| {
            let mut parent = PlannedObjectInstance::object(
                ObjectDraw {
                    id,
                    layer: TacticalLayer(2),
                    display_order,
                    policy: BlitPolicy::z_read(SpriteEncoding::Rle),
                },
                vec![ObjectPieceInstance {
                    target: ObjectTexture::OverlayAtlas,
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
            parent
        };
        // Storage/ID order differs from Display. Original6D913C/9153 precede
        // this parent's body;6D9789 repeats Extras in that same Display order.
        let pass = lower_ground_object_instances(vec![
            parent(1, 2, true),
            parent(500, 1, false),
            parent(900, 0, true),
        ]);
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
        let parent = |id, display_order, tail_draws: Vec<SurfaceDraw>| {
            let piece = || ObjectPieceInstance {
                target: ObjectTexture::OverlayAtlas,
                render_z: RenderZPolicy::ReadOnly,
                instance: SpriteInstance::default(),
            };
            let mut parent = PlannedObjectInstance::object(
                ObjectDraw {
                    id,
                    layer: TacticalLayer(2),
                    display_order,
                    policy: BlitPolicy::z_read(SpriteEncoding::Rle),
                },
                vec![piece(), piece()],
            );
            parent.tail_draws = tail_draws;
            parent
        };
        let pass = lower_ground_object_instances(vec![
            parent(8, 1, Vec::new()),
            parent(7, 0, vec![store(1), store(2)]),
        ]);
        assert_eq!(pass.owners, [7, 7, 8, 8]);
        assert_eq!(pass.surface_draws, [vec![store(1), store(2)]]);
        assert_eq!(
            pass.runs
                .iter()
                .map(|run| (run.target, run.start, run.count))
                .collect::<Vec<_>>(),
            [
                (ObjectTexture::OverlayAtlas, 0, 2),
                (ObjectTexture::SurfaceLines(0), 2, 0),
                (ObjectTexture::OverlayAtlas, 2, 2),
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
        let mut parent = PlannedObjectInstance::building(
            ObjectDraw {
                id: 42,
                layer: TacticalLayer(2),
                display_order: 0,
                policy: BlitPolicy::z_read(SpriteEncoding::Rle),
            },
            vec![PlannedBuildingPieceInstance {
                kind: BuildingPieceKind::Body,
                z_bias: 0,
                policy: BlitPolicy::z_read(SpriteEncoding::Rle),
                target: ObjectTexture::OverlayAtlas,
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
            fills: vec![SpriteInstance::default(); 15],
            scale: Vec::new(),
            group: Default::default(),
            talk_bubble: None,
            self_heal: None,
        });
        let pass = lower_ground_object_instances(vec![parent]);
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
            [ObjectTexture::OverlayAtlas]
        );
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
                        display.submit(actor as u64 + 1, layer, &|id| {
                            keys[id as usize - 1].expect("native only queries Ground keys")
                        });
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
                    "sort" => {
                        display.sort_ground_pass(&|id| {
                            keys[id as usize - 1].expect("native only queries Ground keys")
                        });
                    }
                    other => panic!("unexpected native step {other}"),
                }
                // Load must preserve the retained order even when it is only
                // partly sorted. Removed actors remain in storage below.
                let display: DisplayLayers = crate::util::save_codec::deserialize(
                    &crate::util::save_codec::serialize(&display).unwrap(),
                )
                .unwrap();
                let order = NativeDisplayOrder::from_display(&display);
                let entries = actors
                    .iter()
                    .enumerate()
                    .rev()
                    .filter_map(|(index, actor)| {
                        let id = index as u64 + 1;
                        let parent = order.object_draw(id, SpriteEncoding::Plain)?;
                        let pieces = if actor["kind"] == "bullet" {
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
                        Some(PlannedObjectInstance::object(parent, pieces))
                    })
                    .collect();
                let lowered = lower_object_instances(entries);
                for (index, pass) in lowered.iter().enumerate() {
                    let owners: Vec<_> = expected[index]
                        .as_array()
                        .unwrap()
                        .iter()
                        .flat_map(|value| {
                            let actor = value.as_u64().unwrap() as usize;
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
            NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
            lower_ground_object_instances,
        };
        use crate::render::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;
        use crate::sim::world::display_layers::DisplayLayer;
        use crate::util::fixed_math::SimFixed;

        // Original instructions produce a partially sorted vector after each
        // 551A30 call. Rendering must preserve every intermediate history.
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
        for (step, observed) in row["input"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["after_steps"].as_array().unwrap())
        {
            let id = step["actor"].as_u64().map(|index| index + 1);
            match step["op"].as_str().unwrap() {
                "submit" => {
                    sim.submit_object_display_for_test(id.unwrap(), DisplayLayer::GROUND, None)
                }
                "coordinates" => {
                    let entity = sim.entities_mut_for_test().get_mut(id.unwrap()).unwrap();
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
            let expected: Vec<_> = observed["layers"][2]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_u64().unwrap() + 1)
                .collect();
            assert_eq!(
                tactical_entity_encounter_order(&sim),
                expected,
                "pick: {step}"
            );
            let order = NativeDisplayOrder::new(sim.display_layers().members(DisplayLayer::GROUND));
            let entries = (1..=4)
                .rev()
                .filter_map(|id| {
                    Some(PlannedObjectInstance::object(
                        order.object_draw(id, SpriteEncoding::Plain)?,
                        vec![ObjectPieceInstance {
                            target: ObjectTexture::ShpPage(id as usize % 2),
                            render_z: RenderZPolicy::ReadOnly,
                            instance: Default::default(),
                        }],
                    ))
                })
                .collect();
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
        let building = |id, pieces: &[(BuildingPieceKind, u32)]| {
            PlannedObjectInstance::building(
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
                    })
                    .collect(),
            )
        };
        let pass = lower_ground_object_instances(vec![
            building(40, &[(BuildingPieceKind::Body, 40)]),
            PlannedObjectInstance::object(
                plain_parent(&order, 30),
                vec![marked_piece(ObjectTexture::ShpPage(0), 30)],
            ),
            building(
                20,
                &[
                    (BuildingPieceKind::Bib, 22),
                    (BuildingPieceKind::Shadow, 21),
                    (BuildingPieceKind::Body, 20),
                    (BuildingPieceKind::PoweredOrActiveOverlay, 29),
                ],
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 10),
                vec![marked_piece(ObjectTexture::UnitAtlasPage(0), 10)],
            ),
        ]);
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
        let unit = |id, refused| {
            let mut unit = PlannedObjectInstance::object(
                plain_parent(&order, id),
                vec![marked_piece(ObjectTexture::UnitAtlasPage(0), id as u32)],
            );
            unit.tail_draws = vec![laser(id as i32)];
            unit.drawn_by_tether = refused;
            unit
        };
        let factory = |id, contacts: Vec<DrawId>| {
            let mut factory = PlannedObjectInstance::building(
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
                })
                .collect(),
            );
            factory.tether_redraws = contacts;
            factory
        };
        // 10 is drawn by 20, 40 by 30 (40's slot comes later), 50 is redrawn
        // by 30 without refusing its own slot, and 60 refuses with no redraw.
        let pass = lower_ground_object_instances(vec![
            unit(10, true),
            factory(20, vec![10]),
            factory(30, vec![40, 50]),
            unit(40, true),
            unit(50, false),
            unit(60, true),
        ]);
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
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                plain_parent(&order, 2),
                vec![packed(ObjectTexture::UnitAtlasPage(0), 2, 4)],
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 1),
                vec![
                    packed(ObjectTexture::UnitAtlasPage(0), 11, 4),
                    packed(ObjectTexture::UnitAtlasPage(1), 12, 4),
                    packed(ObjectTexture::UnitPosePage(0), 13, 4),
                ],
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 3),
                vec![packed(ObjectTexture::ShpPage(0), 3, 12)],
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 4),
                vec![packed(ObjectTexture::ShpPage(0), 4, 2)],
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 5),
                vec![packed(
                    ObjectTexture::TerrainShp(crate::render::terrain_draw::TerrainPiece::Body),
                    5,
                    4,
                )],
            ),
        ]);
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
        let building = PlannedObjectInstance::building(
            order.object_draw(30, SpriteEncoding::Plain).unwrap(),
            vec![PlannedBuildingPieceInstance {
                kind: BuildingPieceKind::Body,
                z_bias: 0,
                policy: BlitPolicy::opaque(SpriteEncoding::Plain),
                target: ObjectTexture::ShpPage(0),
                instance: marked_piece(ObjectTexture::ShpPage(0), 30).instance,
            }],
        );
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                order.object_draw(10, SpriteEncoding::Terrain).unwrap(),
                vec![marked_piece(ObjectTexture::OverlayAtlas, 10)],
            ),
            building,
            PlannedObjectInstance::object(
                plain_parent(&order, 20),
                vec![marked_piece(ObjectTexture::UnitAtlasPage(9), 20)],
            ),
        ]);

        assert_eq!(pass.owners, [20, 30, 10]);
        assert_eq!(
            pass.runs.iter().map(|run| run.target).collect::<Vec<_>>(),
            [ObjectTexture::UnitAtlasPage(9), ObjectTexture::OverlayAtlas,]
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
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                plain_parent(&order, 3),
                vec![marked_piece(ObjectTexture::UnitAtlasPage(0), 3)],
            ),
            PlannedObjectInstance::building(
                plain_parent(&order, 2),
                vec![
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
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 1),
                vec![marked_piece(ObjectTexture::OverlayAtlas, 1)],
            ),
        ]);

        assert_eq!(pass.owners, [1, 2, 2, 2, 2, 3]);
        assert_eq!(
            pass.instances
                .iter()
                .map(|instance| instance.draw_state.fx_flags)
                .collect::<Vec<_>>(),
            [1, 22, 24, 21, 23, 3]
        );
    }
}
