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
    pub target: GroundTexture,
    pub instance: SpriteInstance,
}

/// Texture/pipeline identity retained after native object ordering is decided.
///
/// Atlas identity is deliberately data carried by a draw piece, never a sort
/// key. Parent order comes exclusively from the retained Display vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroundTexture {
    OverlayAtlas,
    /// Per-piece native destination edit; never a texture-only coalesced draw.
    TerrainStatic(crate::render::terrain_draw::TerrainPiece),
    UnitAtlasPage(usize),
    UnitTransitionPage(usize),
    ShpPage(usize),
}

/// One already-resolved sprite owned by one Ground-layer parent object.
pub(crate) struct GroundPieceInstance {
    pub target: GroundTexture,
    /// Which depth pipeline draws it (`RenderZPolicy::None` passthrough,
    /// `ReadOnly` Z-tested, `ReadWrite` Z-tested and written).
    pub render_z: RenderZPolicy,
    pub instance: SpriteInstance,
}

/// One native Ground-layer registration and every sprite its display call owns.
pub(crate) struct PlannedGroundObjectInstance {
    pub parent: ObjectDraw,
    pub pieces: Vec<GroundPieceInstance>,
    pub building_pieces: Option<Vec<PlannedBuildingPieceInstance>>,
}

impl PlannedGroundObjectInstance {
    pub(crate) fn object(parent: ObjectDraw, pieces: Vec<GroundPieceInstance>) -> Self {
        Self {
            parent,
            pieces,
            building_pieces: None,
        }
    }

    pub(crate) fn building(parent: ObjectDraw, pieces: Vec<PlannedBuildingPieceInstance>) -> Self {
        Self {
            parent,
            pieces: Vec::new(),
            building_pieces: Some(pieces),
        }
    }
}

/// One contiguous GPU-buffer run sharing a texture/pipeline binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GroundDrawRun {
    pub target: GroundTexture,
    pub render_z: RenderZPolicy,
    pub start: u32,
    pub count: u32,
}

/// Live Layer-2 output: exact integer parent order plus flat GPU instances.
#[derive(Default)]
pub(crate) struct GroundObjectPass {
    pub instances: Vec<SpriteInstance>,
    pub runs: Vec<GroundDrawRun>,
    /// Aligned with `instances`; retained for executable ordering checks.
    #[cfg(test)]
    pub owners: Vec<DrawId>,
}

/// Frame-local lookup derived solely from the retained Ground Display vector.
/// The sim owns all registration and sorting; atlas builders only read ranks.
pub(crate) struct NativeGroundOrder {
    positions: BTreeMap<DrawId, u64>,
}

impl NativeGroundOrder {
    pub(crate) fn new(ids: &[DrawId]) -> Self {
        Self {
            positions: ids
                .iter()
                .enumerate()
                .map(|(rank, &id)| (id, rank as u64))
                .collect(),
        }
    }

    pub(crate) fn object_draw(&self, id: DrawId, encoding: SpriteEncoding) -> Option<ObjectDraw> {
        let policy = match encoding {
            SpriteEncoding::Terrain => BlitPolicy::z_none(encoding),
            _ => BlitPolicy::z_read(encoding),
        };
        Some(ObjectDraw {
            id,
            layer: TacticalLayer(2),
            display_order: *self.positions.get(&id)?,
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

/// Restore the retained Ground vector order after the class-specific builders.
/// Native Tactical6D8F39 reads members in order; neither sprite depth nor a new
/// GetYSort query participates. Building pieces remain contiguous in their slot.
pub(crate) fn lower_ground_object_instances(
    entries: Vec<PlannedGroundObjectInstance>,
) -> GroundObjectPass {
    let mut ordinary = BTreeMap::new();
    let mut buildings = BTreeMap::new();
    let mut inputs = Vec::with_capacity(entries.len());

    for entry in entries {
        let id = entry.parent.id;
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
                                GroundPieceInstance {
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
                "each Ground parent must be emitted once"
            );
            inputs.push(TacticalDrawInput::Building(BuildingOwnedPlan {
                parent: entry.parent,
                pieces: planned,
            }));
        } else {
            assert!(
                ordinary.insert(id, entry.pieces).is_none(),
                "each Ground parent must be emitted once"
            );
            inputs.push(TacticalDrawInput::Object(entry.parent));
        }
    }

    let plan = TacticalDrawPlan::build(inputs);
    let mut lowered = GroundObjectPass::default();
    for layer in plan.object_layers {
        if layer.layer != TacticalLayer(2) {
            continue;
        }
        for entry in layer.entries {
            let owner = entry.object().id;
            match entry {
                crate::render::tactical_draw_plan::LayerEntry::Object(_) => {
                    for piece in ordinary
                        .remove(&owner)
                        .expect("planned Ground object must retain its sprites")
                    {
                        push_ground_piece(&mut lowered, owner, piece);
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
                        push_ground_piece(&mut lowered, owner, piece);
                    }
                }
            }
        }
    }
    lowered
}

fn push_ground_piece(pass: &mut GroundObjectPass, owner: DrawId, piece: GroundPieceInstance) {
    #[cfg(not(test))]
    let _ = owner;
    let start = pass.instances.len() as u32;
    if let Some(run) = pass.runs.last_mut().filter(|run| {
        run.target == piece.target
            && run.render_z == piece.render_z
            && run.start + run.count == start
    }) {
        run.count += 1;
    } else {
        pass.runs.push(GroundDrawRun {
            target: piece.target,
            render_z: piece.render_z,
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
    fn native_ground_history_reaches_entity_picking_and_atlas_lowering() {
        use crate::app::presentation::instances::tactical_entity_encounter_order;
        use crate::app::presentation::render::draw_plan_lowering::{
            GroundPieceInstance, GroundTexture, NativeGroundOrder, PlannedGroundObjectInstance,
            lower_ground_object_instances,
        };
        use crate::render::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;
        use crate::sim::world::display_layers::DisplayLayer;
        use crate::util::fixed_math::SimFixed;

        // Original instructions produce a partially sorted vector after each
        // 551A30 call. Rendering must preserve every intermediate history.
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../../tools/spatial_oracle/crate_ground_membership.json"
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
                crate::sim::components::Health { current: 100 },
                type_id,
                crate::map::entities::EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.position.sub_x = SimFixed::from_num(128 + actor["delta"][0].as_i64().unwrap());
            entity.position.sub_y = SimFixed::from_num(128);
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
                    entity.position.sub_x =
                        SimFixed::from_num(step["xyz"][0].as_i64().unwrap() - 2560);
                    entity.position.sub_y =
                        SimFixed::from_num(step["xyz"][1].as_i64().unwrap() - 2560);
                }
                "sort" => {
                    sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 50);
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
            let order = NativeGroundOrder::new(sim.display_layers().members(DisplayLayer::GROUND));
            let entries = (1..=4)
                .rev()
                .filter_map(|id| {
                    Some(PlannedGroundObjectInstance::object(
                        order.object_draw(id, SpriteEncoding::Plain)?,
                        vec![GroundPieceInstance {
                            target: GroundTexture::ShpPage(id as usize % 2),
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

    fn marked_piece(target: GroundTexture, marker: u32) -> GroundPieceInstance {
        GroundPieceInstance {
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

    fn plain_parent(order: &NativeGroundOrder, id: DrawId) -> ObjectDraw {
        order
            .object_draw(id, SpriteEncoding::Plain)
            .expect("registered parent")
    }

    #[test]
    fn gsi_13_03_far_tree_unit_near_tree_share_one_integer_ground_order() {
        let order = NativeGroundOrder::new(&[10, 20, 30]);
        let pass = lower_ground_object_instances(vec![
            PlannedGroundObjectInstance::object(
                order.object_draw(30, SpriteEncoding::Terrain).unwrap(),
                vec![marked_piece(GroundTexture::OverlayAtlas, 30)],
            ),
            PlannedGroundObjectInstance::object(
                plain_parent(&order, 20),
                vec![marked_piece(GroundTexture::UnitAtlasPage(3), 20)],
            ),
            PlannedGroundObjectInstance::object(
                order.object_draw(10, SpriteEncoding::Terrain).unwrap(),
                vec![marked_piece(GroundTexture::OverlayAtlas, 10)],
            ),
        ]);

        assert_eq!(pass.owners, [10, 20, 30]);
    }

    #[test]
    fn gsi_13_03_equal_tree_unit_building_use_registration_not_atlas() {
        let order = NativeGroundOrder::new(&[20, 30, 10]);
        let building = PlannedGroundObjectInstance::building(
            order.object_draw(30, SpriteEncoding::Plain).unwrap(),
            vec![PlannedBuildingPieceInstance {
                kind: BuildingPieceKind::Body,
                z_bias: 0,
                policy: BlitPolicy::opaque(SpriteEncoding::Plain),
                target: GroundTexture::ShpPage(0),
                instance: marked_piece(GroundTexture::ShpPage(0), 30).instance,
            }],
        );
        let pass = lower_ground_object_instances(vec![
            PlannedGroundObjectInstance::object(
                order.object_draw(10, SpriteEncoding::Terrain).unwrap(),
                vec![marked_piece(GroundTexture::OverlayAtlas, 10)],
            ),
            building,
            PlannedGroundObjectInstance::object(
                plain_parent(&order, 20),
                vec![marked_piece(GroundTexture::UnitAtlasPage(9), 20)],
            ),
        ]);

        assert_eq!(pass.owners, [20, 30, 10]);
        assert_eq!(
            pass.runs.iter().map(|run| run.target).collect::<Vec<_>>(),
            [
                GroundTexture::UnitAtlasPage(9),
                GroundTexture::ShpPage(0),
                GroundTexture::OverlayAtlas,
            ]
        );
    }

    #[test]
    fn gsi_13_03_terrain_leaves_fixed_overlay_and_enters_ground_once() {
        let fixed = lower_cell_instances(vec![cell(7, false)]);
        let order = NativeGroundOrder::new(&[8]);
        let ground = lower_ground_object_instances(vec![PlannedGroundObjectInstance::object(
            order.object_draw(8, SpriteEncoding::Terrain).unwrap(),
            vec![marked_piece(GroundTexture::OverlayAtlas, 8)],
        )]);

        assert_eq!(fixed.len(), 1);
        assert_eq!(ground.owners, [8]);
        assert_eq!(ground.instances.len(), 1);
    }

    #[test]
    fn gsi_13_03_building_pieces_remain_contiguous_in_parent_slot() {
        let order = NativeGroundOrder::new(&[1, 2, 3]);
        let building_piece = |kind, target, marker| PlannedBuildingPieceInstance {
            kind,
            z_bias: 0,
            policy: BlitPolicy::opaque(SpriteEncoding::Plain),
            target,
            instance: marked_piece(target, marker).instance,
        };
        let pass = lower_ground_object_instances(vec![
            PlannedGroundObjectInstance::object(
                plain_parent(&order, 3),
                vec![marked_piece(GroundTexture::UnitAtlasPage(0), 3)],
            ),
            PlannedGroundObjectInstance::building(
                plain_parent(&order, 2),
                vec![
                    building_piece(
                        BuildingPieceKind::PoweredOrActiveOverlay,
                        GroundTexture::UnitAtlasPage(2),
                        23,
                    ),
                    building_piece(BuildingPieceKind::Body, GroundTexture::ShpPage(1), 22),
                    building_piece(BuildingPieceKind::Bib, GroundTexture::ShpPage(0), 21),
                ],
            ),
            PlannedGroundObjectInstance::object(
                plain_parent(&order, 1),
                vec![marked_piece(GroundTexture::OverlayAtlas, 1)],
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
