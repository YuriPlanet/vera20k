//! Safe lowering from native-shaped tactical plans to existing GPU buffers.
//!
//! The simulation supplies retained Display order. Builders attach pieces to
//! those parent slots before lowering them to atlas draw runs.

use std::collections::BTreeMap;

use crate::render::batch::SpriteInstance;
use crate::render::tactical_draw_plan::{
    BlitPolicy, BuildingOwnedPlan, BuildingPiece, BuildingPieceKind, CellDraw, CellDrawKind,
    DrawId, ObjectDraw, RenderZPolicy, SpriteEncoding, TacticalDrawInput, TacticalDrawPlan,
    TacticalLayer,
};

/// A cell-pass instance paired with the metadata needed by `YR TacticalClass::Draw`.
pub(crate) struct PlannedCellInstance {
    pub draw: CellDraw,
    pub instance: SpriteInstance,
}

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
    /// Ordered DSurface calls before this parent's body, with no sprite data.
    SurfaceLines(usize),
    /// Initial Building Extras status, sharing the late run's instance range.
    Status(usize),
    OverlayAtlas,
    /// Per-piece native destination edit; never a texture-only coalesced draw.
    TerrainShp(crate::render::terrain_draw::TerrainPiece),
    /// Bullet body/shadow: signed native Z comparison, destination edits,
    /// and no depth write. The atlas page is payload, never an ordering key.
    ProjectileShp(usize, crate::render::terrain_draw::TerrainPiece),
    UnitAtlasPage(usize),
    UnitPosePage(usize),
    ShpPage(usize),
}

/// One already-resolved sprite owned by one retained Display parent object.
pub(crate) struct ObjectPieceInstance {
    pub target: ObjectTexture,
    /// Which depth pipeline draws it (`RenderZPolicy::None` passthrough,
    /// `ReadOnly` Z-tested, `ReadWrite` Z-tested and written).
    pub render_z: RenderZPolicy,
    pub instance: SpriteInstance,
}

/// One native Display registration and every sprite its display call owns.
pub(crate) struct PlannedObjectInstance {
    pub parent: ObjectDraw,
    pub pieces: Vec<ObjectPieceInstance>,
    pub building_pieces: Option<Vec<PlannedBuildingPieceInstance>>,
    pub brackets: Option<crate::app::presentation::selection_brackets::BuildingBracketLines>,
    pub status: Option<crate::app::presentation::ui_overlays::TechnoExtras>,
}

impl PlannedObjectInstance {
    pub(crate) fn object(parent: ObjectDraw, pieces: Vec<ObjectPieceInstance>) -> Self {
        Self {
            parent,
            pieces,
            building_pieces: None,
            brackets: None,
            status: None,
        }
    }

    pub(crate) fn building(parent: ObjectDraw, pieces: Vec<PlannedBuildingPieceInstance>) -> Self {
        Self {
            parent,
            pieces: Vec::new(),
            building_pieces: Some(pieces),
            brackets: None,
            status: None,
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
///6F5190 submits each parent's wrench/rank before its health border and pips.
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
        wrench: Option<u32>,
        rank: Option<u32>,
        background: Option<u32>,
        fills: std::ops::Range<u32>,
        scale: std::ops::Range<u32>,
        self_heal: Option<u32>,
        group_rectangles: std::ops::Range<u32>,
        group_text: std::ops::Range<u32>,
    },
}

/// One native layer output: exact integer parent order plus flat GPU instances.
#[derive(Default)]
pub(crate) struct ObjectLayerPass {
    pub instances: Vec<SpriteInstance>,
    pub runs: Vec<ObjectDrawRun>,
    /// Native6D913C/6D9153 calls, fenced into their Display parent's slot.
    pub before_lines: Vec<Vec<crate::render::surface_line::SurfaceLine>>,
    /// Frame-local uploads of `before_lines`, prepared after the shared A plane.
    pub line_batches: Vec<crate::render::terrain_draw::SurfaceLineBatch>,
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

/// Lower a complete cell family while preserving the existing GPU instance type.
///
/// `TacticalDrawPlan` owns the family ordering; this adapter only maps ordered
/// IDs back to the existing instances. Duplicate IDs are rejected at the source.
#[cfg(test)]
pub(crate) fn lower_cell_instances(entries: Vec<PlannedCellInstance>) -> Vec<SpriteInstance> {
    lower_cell_instances_with_policy(entries).0
}

/// Keep the fixed-cell policy beside each lowered instance. An overlay's
/// native SHP depth behavior must survive lowering without sorting walls into
/// a separate batch or moving the still unsupported slope-shape draws.
pub(crate) fn lower_cell_instances_with_policy(
    entries: Vec<PlannedCellInstance>,
) -> (Vec<SpriteInstance>, Vec<RenderZPolicy>) {
    let mut instances = BTreeMap::new();
    let inputs = entries.into_iter().map(|entry| {
        assert!(
            instances.insert(entry.draw.id, entry.instance).is_none(),
            "each planned cell instance must have a unique draw id"
        );
        TacticalDrawInput::Cell(entry.draw)
    });
    let plan = TacticalDrawPlan::build(inputs);
    let mut ordered = Vec::with_capacity(instances.len());
    let mut render_z = Vec::with_capacity(instances.len());
    for draw in plan
        .cell_pass
        .terrain
        .iter()
        .chain(&plan.cell_pass.smudges)
        .chain(&plan.cell_pass.overlays)
        .chain(&plan.cell_pass.primary_objects)
    {
        render_z.push(draw.policy.render_z);
        ordered.push(
            instances
                .remove(&draw.id)
                .expect("plan entry must resolve to its existing GPU instance"),
        );
    }
    (ordered, render_z)
}

/// Restore the retained Display vector order after the class-specific builders.
/// Native Tactical6D8F39 reads members in order; neither sprite depth nor a new
/// GetYSort query participates. Building pieces remain contiguous in their slot.
pub(crate) fn lower_object_instances(entries: Vec<PlannedObjectInstance>) -> [ObjectLayerPass; 5] {
    let mut ordinary = BTreeMap::new();
    let mut buildings = BTreeMap::new();
    let mut brackets = BTreeMap::new();
    let mut status = BTreeMap::new();
    let mut inputs = Vec::with_capacity(entries.len());

    for entry in entries {
        let id = entry.parent.id;
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
                ObjectExtrasRun::Status {
                    #[cfg(test)]
                    parent: owner,
                    wrench,
                    rank,
                    background,
                    fills: start..end,
                    scale: end..scale_end,
                    self_heal,
                    group_rectangles: rectangles_start..rectangles_end,
                    group_text: rectangles_end..text_end,
                }
            });
            // Wrench6F5340 and rank6F5382 precede selected Building lines
            //6F5396..5E10; health6F5E27 follows. Split that prefix when lines
            // fence it, retaining one upload for both native Extras visits.
            let mut split_prefix = None;
            if lines.as_ref().is_some_and(|lines| !lines.extras.is_empty()) {
                if let Some(ObjectExtrasRun::Status {
                    wrench,
                    rank,
                    fills,
                    ..
                }) = &strip
                    && (wrench.is_some() || rank.is_some())
                {
                    split_prefix = Some(lowered.extras_runs.len());
                    lowered.extras_runs.push(ObjectExtrasRun::Status {
                        #[cfg(test)]
                        parent: owner,
                        wrench: *wrench,
                        rank: *rank,
                        background: None,
                        fills: fills.start..fills.start,
                        scale: fills.start..fills.start,
                        self_heal: None,
                        group_rectangles: fills.start..fills.start,
                        group_text: fills.start..fills.start,
                    });
                }
            }
            if let Some(lines) = lines {
                if let Some(index) = split_prefix.filter(|_| is_building) {
                    push_before_lines(lowered, lines.behind);
                    push_initial_status(lowered, index);
                    push_before_lines(lowered, lines.extras.clone());
                } else {
                    let mut before = lines.behind;
                    before.extend_from_slice(&lines.extras);
                    push_before_lines(lowered, before);
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
                wrench,
                rank,
                background,
                fills,
                scale,
                self_heal,
                group_rectangles,
                group_text,
                ..
            }) = strip
            {
                let wrench = wrench.filter(|_| split_prefix.is_none());
                let rank = rank.filter(|_| split_prefix.is_none());
                if wrench.is_some()
                    || rank.is_some()
                    || background.is_some()
                    || !fills.is_empty()
                    || !scale.is_empty()
                    || self_heal.is_some()
                    || !group_rectangles.is_empty()
                    || !group_text.is_empty()
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
                        wrench,
                        rank,
                        background,
                        fills,
                        scale,
                        self_heal,
                        group_rectangles,
                        group_text,
                    });
                }
            }
            match entry {
                crate::render::tactical_draw_plan::LayerEntry::Object(_) => {
                    for piece in ordinary
                        .remove(&owner)
                        .expect("planned Display object must retain its sprites")
                    {
                        push_object_piece(lowered, owner, piece);
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
                        push_object_piece(lowered, owner, piece);
                    }
                }
            }
        }
    }
    layers
}

fn push_before_lines(
    pass: &mut ObjectLayerPass,
    lines: Vec<crate::render::surface_line::SurfaceLine>,
) {
    let index = pass.before_lines.len();
    pass.before_lines.push(lines);
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

fn push_object_piece(pass: &mut ObjectLayerPass, owner: DrawId, piece: ObjectPieceInstance) {
    // Unit73B140 composites its opaque source cache once; SHP705E00 selects
    // the packed leaves through Convert490E50. Source pieces retain their
    // parent here so hull/turret/barrel are not blended separately. Native
    // selectors and coverage: procedural_drawing_oracle/translucent_blitter_a.md.
    let packed_parent = (matches!(
        piece.target,
        ObjectTexture::UnitAtlasPage(_)
            | ObjectTexture::UnitPosePage(_)
            | ObjectTexture::ShpPage(_)
            | ObjectTexture::TerrainShp(crate::render::terrain_draw::TerrainPiece::Body)
    ) && piece.instance.draw_state.native_selector_bits() & 6 != 0
        && piece.instance.draw_state.fx_flags & crate::render::draw_state::FX_SHADOW == 0)
        .then_some(owner);
    let start = pass.instances.len() as u32;
    if let Some(run) = pass.runs.last_mut().filter(|run| {
        run.target == piece.target
            && run.render_z == piece.render_z
            && run.packed_parent == packed_parent
            && run.start + run.count == start
    }) {
        run.count += 1;
    } else {
        pass.runs.push(ObjectDrawRun {
            target: piece.target,
            render_z: piece.render_z,
            packed_parent,
            start,
            count: 1,
        });
    }
    pass.instances.push(piece.instance);
    #[cfg(test)]
    pass.owners.push(owner);
}

pub(crate) fn cell_draw_kind(is_wall: bool) -> CellDrawKind {
    if is_wall {
        CellDrawKind::WallOverlay
    } else {
        CellDrawKind::FlatOverlay
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
            pass.before_lines,
            [vec![line(900), line(901)], vec![line(1), line(2)]]
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

    #[test]
    fn building_health_reuses_late_ranges_before_each_parent_body() {
        use crate::app::presentation::{
            selection_brackets::BuildingBracketLines, ui_overlays::TechnoExtras,
        };
        let parent = |id, rank| {
            let mut parent = PlannedObjectInstance::building(
                ObjectDraw {
                    id,
                    layer: TacticalLayer(2),
                    display_order: rank,
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
                behind: vec![],
                extras: vec![],
            });
            parent.status = Some(TechnoExtras {
                wrench: None,
                rank: None,
                background: None,
                fills: vec![SpriteInstance::default(); 15],
                scale: Vec::new(),
                group: Default::default(),
                self_heal: None,
            });
            parent
        };
        let pass = lower_ground_object_instances(vec![parent(1, 1), parent(900, 0)]);
        assert_eq!(pass.owners, [900, 1]);
        assert_eq!(pass.extras_instances.len(), 30);
        assert!(matches!(&pass.extras_runs[..], [
            ObjectExtrasRun::Status { parent:900, fills:a, .. },
            ObjectExtrasRun::Status { parent:1, fills:b, .. },
        ] if *a == (0..15) && *b == (15..30)));
        assert_eq!(
            pass.runs.iter().map(|run| run.target).collect::<Vec<_>>(),
            [
                ObjectTexture::SurfaceLines(0),
                ObjectTexture::Status(0),
                ObjectTexture::OverlayAtlas,
                ObjectTexture::SurfaceLines(1),
                ObjectTexture::Status(1),
                ObjectTexture::OverlayAtlas,
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
            wrench: None,
            rank: Some(SpriteInstance::default()),
            background: None,
            fills: vec![SpriteInstance::default(); 15],
            scale: Vec::new(),
            group: Default::default(),
            self_heal: None,
        });
        let pass = lower_ground_object_instances(vec![parent]);
        // Native6D913C Behind,6D9153 Extras:6F5382 rank,6F5396..5E10
        // selected Building lines,6F5E27 health, then the body.6D9789 repeats
        // rank->Extras-lines->health, using the same uploaded instances.
        assert_eq!(pass.before_lines, [vec![line(10)], vec![line(11)]]);
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
                ObjectTexture::OverlayAtlas,
            ]
        );
    }

    #[test]
    fn late_extras_interleave_rank_health_and_brackets_by_display_parent() {
        use crate::app::presentation::{
            selection_brackets::BuildingBracketLines, ui_overlays::TechnoExtras,
        };
        use crate::render::surface_line::{SurfaceLine, SurfaceLineBlend};
        let parent = |id, rank| {
            PlannedObjectInstance::object(
                ObjectDraw {
                    id,
                    layer: TacticalLayer(2),
                    display_order: rank,
                    policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                },
                vec![],
            )
        };
        let mut first = parent(901, 0);
        first.status = Some(TechnoExtras {
            wrench: None,
            rank: Some(SpriteInstance {
                position: [0.0, 0.0],
                ..Default::default()
            }),
            background: Some(SpriteInstance {
                position: [1.0, 0.0],
                ..Default::default()
            }),
            fills: vec![SpriteInstance {
                position: [2.0, 0.0],
                ..Default::default()
            }],
            scale: vec![SpriteInstance {
                position: [3.0, 0.0],
                ..Default::default()
            }],
            group: Default::default(),
            self_heal: None,
        });
        let mut middle = parent(3, 1);
        middle.brackets = Some(BuildingBracketLines {
            behind: vec![],
            extras: vec![SurfaceLine {
                from: [0, 0],
                to: [8, 0],
                z_adjust: [14, 14],
                blend: SurfaceLineBlend::Replace(65535),
            }],
        });
        let mut last = parent(2, 2);
        last.status = Some(TechnoExtras {
            wrench: None,
            rank: None,
            background: None,
            fills: vec![SpriteInstance {
                position: [4.0, 0.0],
                ..Default::default()
            }],
            scale: Vec::new(),
            group: Default::default(),
            self_heal: None,
        });
        let pass = lower_ground_object_instances(vec![last, middle, first]);
        assert!(matches!(&pass.extras_runs[..], [
            ObjectExtrasRun::Status { parent: 901, rank: Some(0), background: Some(1), fills: a, scale: s, .. },
            ObjectExtrasRun::Lines { parent: 3, .. },
            ObjectExtrasRun::Status { parent: 2, rank: None, background: None, fills: b, .. },
        ] if *a == (2..3) && *s == (3..4) && *b == (4..5)));
        assert_eq!(
            pass.extras_instances
                .iter()
                .map(|s| s.position[0])
                .collect::<Vec<_>>(),
            [0.0, 1.0, 2.0, 3.0, 4.0]
        );
        let empty = lower_ground_object_instances(vec![parent(901, 0)]);
        assert!(empty.extras_runs.is_empty());
        assert!(empty.extras_instances.is_empty());
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
                let display: DisplayLayers =
                    bincode::deserialize(&bincode::serialize(&display).unwrap()).unwrap();
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
                                    ObjectTexture::ProjectileShp(index % 2, piece),
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
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("E1");
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
            sim.entities_mut().insert(entity);
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
                "submit" => sim.submit_object_display(id.unwrap(), DisplayLayer::GROUND, None),
                "coordinates" => {
                    let entity = sim.entities_mut().get_mut(id.unwrap()).unwrap();
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

    fn cell(id: DrawId, is_wall: bool) -> PlannedCellInstance {
        PlannedCellInstance {
            draw: CellDraw {
                id,
                kind: cell_draw_kind(is_wall),
                policy: BlitPolicy::translucent(SpriteEncoding::Terrain, RenderZPolicy::None),
            },
            instance: SpriteInstance {
                draw_state: DrawState {
                    fx_flags: id as u32,
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }

    #[test]
    fn cell_lowering_keeps_walls_in_the_fixed_overlay_family() {
        let lowered = lower_cell_instances(vec![cell(4, false), cell(3, true), cell(2, false)]);
        assert_eq!(
            lowered
                .iter()
                .map(|instance| instance.draw_state.fx_flags)
                .collect::<Vec<_>>(),
            [4, 3, 2]
        );
    }

    #[test]
    fn cell_lowering_preserves_mixed_depth_policies_beside_their_instances() {
        let mut wall = cell(3, true);
        wall.draw.policy.render_z = RenderZPolicy::ReadWrite;
        wall.instance.z_adjust = -62.0;
        wall.instance.z_gradient = 2;
        let (lowered, policies) =
            lower_cell_instances_with_policy(vec![cell(4, false), wall, cell(2, false)]);
        assert_eq!(
            policies,
            [
                RenderZPolicy::None,
                RenderZPolicy::ReadWrite,
                RenderZPolicy::None
            ]
        );
        assert_eq!(
            lowered
                .iter()
                .map(|i| i.draw_state.fx_flags)
                .collect::<Vec<_>>(),
            [4, 3, 2]
        );
        assert_eq!((lowered[1].z_adjust, lowered[1].z_gradient), (-62.0, 2));
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
    fn only_native_body_selectors_split_opaque_atlas_batching() {
        use crate::render::draw_state::FX_SHADOW;

        let order = NativeDisplayOrder::new(&[1, 2, 3, 4]);
        let mut rgba_alpha = marked_piece(ObjectTexture::UnitPosePage(0), 2);
        rgba_alpha.instance.alpha = 0.5;
        rgba_alpha.instance.draw_state.fx_params[0] = 0.25;
        let mut shadow = marked_piece(ObjectTexture::UnitPosePage(0), 3);
        shadow.instance.draw_state.fx_params[1] = 4.0;
        shadow.instance.draw_state.fx_flags |= FX_SHADOW;
        let mut terrain = marked_piece(
            ObjectTexture::TerrainShp(crate::render::terrain_draw::TerrainPiece::Shadow),
            4,
        );
        terrain.instance.draw_state.fx_params[1] = 4.0;
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                plain_parent(&order, 1),
                vec![marked_piece(ObjectTexture::UnitPosePage(0), 1)],
            ),
            PlannedObjectInstance::object(plain_parent(&order, 2), vec![rgba_alpha]),
            PlannedObjectInstance::object(plain_parent(&order, 3), vec![shadow]),
            PlannedObjectInstance::object(plain_parent(&order, 4), vec![terrain]),
        ]);
        assert_eq!(pass.runs.len(), 2);
        assert_eq!(pass.runs[0].count, 3);
        assert!(pass.runs.iter().all(|run| run.packed_parent.is_none()));
    }

    #[test]
    fn gsi_13_03_far_tree_unit_near_tree_share_one_integer_ground_order() {
        let order = NativeDisplayOrder::new(&[10, 20, 30]);
        let pass = lower_ground_object_instances(vec![
            PlannedObjectInstance::object(
                order.object_draw(30, SpriteEncoding::Terrain).unwrap(),
                vec![marked_piece(ObjectTexture::OverlayAtlas, 30)],
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 20),
                vec![marked_piece(ObjectTexture::UnitAtlasPage(3), 20)],
            ),
            PlannedObjectInstance::object(
                order.object_draw(10, SpriteEncoding::Terrain).unwrap(),
                vec![marked_piece(ObjectTexture::OverlayAtlas, 10)],
            ),
        ]);

        assert_eq!(pass.owners, [10, 20, 30]);
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
            [
                ObjectTexture::UnitAtlasPage(9),
                ObjectTexture::ShpPage(0),
                ObjectTexture::OverlayAtlas,
            ]
        );
    }

    #[test]
    fn gsi_13_03_terrain_leaves_fixed_overlay_and_enters_ground_once() {
        let fixed = lower_cell_instances(vec![cell(7, false)]);
        let order = NativeDisplayOrder::new(&[8]);
        let ground = lower_ground_object_instances(vec![PlannedObjectInstance::object(
            order.object_draw(8, SpriteEncoding::Terrain).unwrap(),
            vec![marked_piece(ObjectTexture::OverlayAtlas, 8)],
        )]);

        assert_eq!(fixed.len(), 1);
        assert_eq!(ground.owners, [8]);
        assert_eq!(ground.instances.len(), 1);
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
                    building_piece(BuildingPieceKind::Bib, ObjectTexture::ShpPage(0), 21),
                ],
            ),
            PlannedObjectInstance::object(
                plain_parent(&order, 1),
                vec![marked_piece(ObjectTexture::OverlayAtlas, 1)],
            ),
        ]);

        assert_eq!(pass.owners, [1, 2, 2, 2, 3]);
        assert_eq!(
            pass.instances
                .iter()
                .map(|instance| instance.draw_state.fx_flags)
                .collect::<Vec<_>>(),
            [1, 21, 22, 23, 3]
        );
    }
}
