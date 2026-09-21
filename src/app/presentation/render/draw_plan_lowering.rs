//! Safe lowering from native-shaped tactical plans to existing GPU buffers.
//!
//! Parent registrations keep their native integer keys until after one global
//! Ground order is fixed; only then are sprites lowered to atlas draw runs.

use std::collections::BTreeMap;

use crate::render::batch::SpriteInstance;
use crate::render::tactical_draw_plan::{
    BlitPolicy, BuildingOwnedPlan, BuildingPiece, BuildingPieceKind, CellDraw, CellDrawKind,
    DrawId, ObjectDraw, RenderZPolicy, SpriteEncoding, TacticalCoord, TacticalDrawInput,
    TacticalDrawPlan, TacticalLayer,
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
/// key. Equal `ObjectClass::GetYSort` values are resolved exclusively by live
/// registration order.
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

/// Stable registration lookup shared by all Ground instance builders.
pub(crate) struct NativeGroundOrder {
    registrations: BTreeMap<DrawId, u64>,
}

/// Native BuildingClass render coordinate and class-owned Y-sort adjustment.
///
/// gamemd-derived: active YR `BuildingClass::GetRenderCoords @ 0x00459EF0`
/// returns `Location.X - 128, Location.Y - 128, Location.Z` with signed wrap;
/// `BuildingClass::GetYSort @ 0x00449410` then independently adds 32 for the
/// actual ObjectType's `TurretAnimIsVoxel` byte and subtracts 16 for `Gate`.
pub(crate) fn building_ground_order_parts(
    location: TacticalCoord,
    turret_anim_is_voxel: bool,
    gate: bool,
) -> (TacticalCoord, i32) {
    let (coord, adjust) = crate::sim::movement::ground_pose::building_render_order_parts(
        crate::sim::components::DriveCoord {
            x: location.x,
            y: location.y,
            z: location.z,
        },
        turret_anim_is_voxel,
        gate,
    );
    (
        TacticalCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        },
        adjust,
    )
}

impl NativeGroundOrder {
    pub(crate) fn new(ids: &[DrawId]) -> Self {
        Self {
            registrations: ids
                .iter()
                .enumerate()
                .map(|(registration, &id)| (id, registration as u64))
                .collect(),
        }
    }

    pub(crate) fn object_draw(
        &self,
        id: DrawId,
        coord: TacticalCoord,
        encoding: SpriteEncoding,
    ) -> Option<ObjectDraw> {
        self.object_draw_with_adjust(id, coord, 0, encoding)
    }

    fn object_draw_with_adjust(
        &self,
        id: DrawId,
        coord: TacticalCoord,
        y_sort_adjust: i32,
        encoding: SpriteEncoding,
    ) -> Option<ObjectDraw> {
        // Non-building objects test render Z per pixel and never write it;
        // terrain objects (trees) keep the untraced passthrough.
        let policy = match encoding {
            SpriteEncoding::Terrain => BlitPolicy::z_none(encoding),
            _ => BlitPolicy::z_read(encoding),
        };
        Some(ObjectDraw {
            id,
            layer: TacticalLayer(2),
            coord,
            y_sort_adjust,
            registration_order: *self.registrations.get(&id)?,
            policy,
        })
    }

    pub(crate) fn building_object_draw(
        &self,
        id: DrawId,
        location: TacticalCoord,
        object_type: &crate::rules::object_type::ObjectType,
        encoding: SpriteEncoding,
    ) -> Option<ObjectDraw> {
        let (coord, y_sort_adjust) = building_ground_order_parts(
            location,
            object_type.turret_anim_is_voxel,
            object_type.gate,
        );
        self.object_draw_with_adjust(id, coord, y_sort_adjust, encoding)
    }

    /// gamemd-derived: no-owner `AnimClass::GetLayer @ 0x00424CB0` submits
    /// Ground types with `AnimClass::GetYSort @ 0x00422BC0`, which adds the
    /// instance +0x104 value initialized from AnimType `YSortAdjust`.
    pub(crate) fn anim_object_draw(
        &self,
        id: DrawId,
        coord: TacticalCoord,
        y_sort_adjust: i32,
    ) -> Option<ObjectDraw> {
        self.object_draw_with_adjust(id, coord, y_sort_adjust, SpriteEncoding::Plain)
    }

    /// gamemd-derived: `TerrainClass__Read_Map_Section @ 0x0071CA70`
    /// constructs at the cell center, and the active Terrain virtuals
    /// `GetCoords @ 0x0041BE00`, `ObjectClass::GetYSort @ 0x005F6BD0`, and
    /// `GetLayer @ 0x005F4260` feed `TerrainClass::Display @ 0x0071CC50`, which
    /// submits that exact signed X+Y key to Ground.
    pub(crate) fn terrain_object_draw(&self, id: DrawId, rx: u16, ry: u16) -> Option<ObjectDraw> {
        self.object_draw(
            id,
            TacticalCoord {
                x: i32::from(rx) * 256 + 128,
                y: i32::from(ry) * 256 + 128,
                z: 0,
            },
            SpriteEncoding::Terrain,
        )
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

/// Lower all visible Ground parents through the live native-shaped layer plan.
///
/// gamemd-derived: `SortedInsert @ 0x00551A90` inserts only before a strictly
/// smaller `ObjectClass::GetYSort @ 0x005F6BD0` key, so equal signed X+Y keys
/// retain the `DisplayClass::Submit @ 0x004A9720` registration sequence. The
/// atlas and GPU depth carried by each piece are intentionally absent from the
/// comparison.
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
    use crate::rules::ini_parser::IniFile;
    use crate::rules::object_type::ObjectType;
    use crate::rules::ruleset::RuleSet;

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

    fn plain_parent(order: &NativeGroundOrder, id: DrawId, x: i32, y: i32) -> ObjectDraw {
        order
            .object_draw(id, TacticalCoord { x, y, z: 0 }, SpriteEncoding::Plain)
            .expect("registered parent")
    }

    fn parsed_building(id: &str, fields: &str) -> ObjectType {
        let ini = IniFile::from_str(&format!(
            "[BuildingTypes]\n0={id}\n[{id}]\nFixtureOnly=1\n{fields}"
        ));
        RuleSet::from_ini(&ini)
            .expect("building fixture rules")
            .object(id)
            .expect("registered building fixture")
            .clone()
    }

    #[test]
    fn gsi_13_03_far_tree_unit_near_tree_share_one_integer_ground_order() {
        let order = NativeGroundOrder::new(&[10, 20, 30]);
        let pass = lower_ground_object_instances(vec![
            PlannedGroundObjectInstance::object(
                order.terrain_object_draw(30, 3, 3).unwrap(),
                vec![marked_piece(GroundTexture::OverlayAtlas, 30)],
            ),
            PlannedGroundObjectInstance::object(
                plain_parent(&order, 20, 640, 640),
                vec![marked_piece(GroundTexture::UnitAtlasPage(3), 20)],
            ),
            PlannedGroundObjectInstance::object(
                order.terrain_object_draw(10, 1, 1).unwrap(),
                vec![marked_piece(GroundTexture::OverlayAtlas, 10)],
            ),
        ]);

        assert_eq!(pass.owners, [10, 20, 30]);
    }

    #[test]
    fn gsi_13_03_equal_tree_unit_building_use_registration_not_atlas() {
        let order = NativeGroundOrder::new(&[20, 30, 10]);
        let normal = parsed_building("NORMAL", "");
        let building = PlannedGroundObjectInstance::building(
            order
                .building_object_draw(
                    30,
                    TacticalCoord {
                        x: 428,
                        y: 596,
                        z: 7,
                    },
                    &normal,
                    SpriteEncoding::Plain,
                )
                .unwrap(),
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
                order.terrain_object_draw(10, 1, 1).unwrap(),
                vec![marked_piece(GroundTexture::OverlayAtlas, 10)],
            ),
            building,
            PlannedGroundObjectInstance::object(
                plain_parent(&order, 20, 500, 268),
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
    fn gsi_13_03_building_render_coordinate_and_independent_key_terms_are_exact() {
        let order = NativeGroundOrder::new(&[1, 2, 3, 4]);
        let normal = parsed_building("NORMAL", "");
        let turret = parsed_building("TURRET", "TurretAnimIsVoxel=yes\n");
        let gate = parsed_building("GATE", "Gate=yes\n");
        let both = parsed_building("BOTH", "TurretAnimIsVoxel=yes\nGate=yes\n");
        let location = TacticalCoord {
            x: 2688,
            y: 5248,
            z: 13,
        };

        let normal_draw = order
            .building_object_draw(1, location, &normal, SpriteEncoding::Plain)
            .unwrap();
        let turret_draw = order
            .building_object_draw(2, location, &turret, SpriteEncoding::Plain)
            .unwrap();
        let gate_draw = order
            .building_object_draw(3, location, &gate, SpriteEncoding::Plain)
            .unwrap();
        let both_draw = order
            .building_object_draw(4, location, &both, SpriteEncoding::Plain)
            .unwrap();

        assert_eq!(
            normal_draw.coord,
            TacticalCoord {
                x: 2560,
                y: 5120,
                z: 13,
            }
        );
        assert_eq!(
            [
                normal_draw.y_sort_key(),
                turret_draw.y_sort_key(),
                gate_draw.y_sort_key(),
                both_draw.y_sort_key(),
            ],
            [7680, 7712, 7664, 7696]
        );
    }

    #[test]
    fn gsi_13_03_building_ini_flags_remain_independent() {
        let normal = parsed_building("NORMAL", "");
        let turret = parsed_building("TURRET", "TurretAnimIsVoxel=yes\n");
        let gate = parsed_building("GATE", "Gate=yes\n");
        let both = parsed_building("BOTH", "TurretAnimIsVoxel=yes\nGate=yes\n");

        assert_eq!(
            [
                (normal.turret_anim_is_voxel, normal.gate),
                (turret.turret_anim_is_voxel, turret.gate),
                (gate.turret_anim_is_voxel, gate.gate),
                (both.turret_anim_is_voxel, both.gate),
            ],
            [(false, false), (true, false), (false, true), (true, true)]
        );
    }

    #[test]
    fn gsi_13_03_building_terms_cross_terrain_boundary_before_registration_tie() {
        let order = NativeGroundOrder::new(&[1, 2, 3, 4, 5]);
        let normal = parsed_building("NORMAL", "");
        let turret = parsed_building("TURRET", "TurretAnimIsVoxel=yes\n");
        let gate = parsed_building("GATE", "Gate=yes\n");
        let both = parsed_building("BOTH", "TurretAnimIsVoxel=yes\nGate=yes\n");
        let location = TacticalCoord {
            x: 2688,
            y: 5248,
            z: 0,
        };
        let piece = |id| vec![marked_piece(GroundTexture::ShpPage(id), id as u32)];
        let pass = lower_ground_object_instances(vec![
            PlannedGroundObjectInstance::object(
                order
                    .building_object_draw(5, location, &turret, SpriteEncoding::Plain)
                    .unwrap(),
                piece(5),
            ),
            PlannedGroundObjectInstance::object(
                order
                    .building_object_draw(4, location, &both, SpriteEncoding::Plain)
                    .unwrap(),
                piece(4),
            ),
            PlannedGroundObjectInstance::object(
                order
                    .building_object_draw(3, location, &normal, SpriteEncoding::Plain)
                    .unwrap(),
                piece(3),
            ),
            PlannedGroundObjectInstance::object(
                order.terrain_object_draw(2, 10, 19).unwrap(),
                vec![marked_piece(GroundTexture::OverlayAtlas, 2)],
            ),
            PlannedGroundObjectInstance::object(
                order
                    .building_object_draw(1, location, &gate, SpriteEncoding::Plain)
                    .unwrap(),
                piece(1),
            ),
        ]);

        assert_eq!(pass.owners, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn gsi_13_03_terrain_key_is_cell_center_not_sprite_geometry_or_gpu_depth() {
        let order = NativeGroundOrder::new(&[1, 2]);
        let terrain = order.terrain_object_draw(1, 7, 9).unwrap();
        assert_eq!(
            terrain.coord,
            TacticalCoord {
                x: 7 * 256 + 128,
                y: 9 * 256 + 128,
                z: 0,
            }
        );
        let mut arbitrary = marked_piece(GroundTexture::OverlayAtlas, 1);
        arbitrary.instance.position = [-9000.0, 42_000.0];
        arbitrary.instance.size = [511.0, 3.0];
        arbitrary.instance.depth = 0.999;
        let pass = lower_ground_object_instances(vec![
            PlannedGroundObjectInstance::object(
                plain_parent(&order, 2, terrain.coord.x, terrain.coord.y + 1),
                vec![marked_piece(GroundTexture::ShpPage(3), 2)],
            ),
            PlannedGroundObjectInstance::object(terrain, vec![arbitrary]),
        ]);

        assert_eq!(pass.owners, [1, 2]);
    }

    #[test]
    fn gsi_13_03_terrain_leaves_fixed_overlay_and_enters_ground_once() {
        let fixed = lower_cell_instances(vec![cell(7, false)]);
        let order = NativeGroundOrder::new(&[8]);
        let ground = lower_ground_object_instances(vec![PlannedGroundObjectInstance::object(
            order.terrain_object_draw(8, 2, 4).unwrap(),
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
                plain_parent(&order, 3, 300, 300),
                vec![marked_piece(GroundTexture::UnitAtlasPage(0), 3)],
            ),
            PlannedGroundObjectInstance::building(
                plain_parent(&order, 2, 200, 200),
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
                plain_parent(&order, 1, 100, 100),
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
