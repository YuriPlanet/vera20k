//! Pure tactical draw planning before GPU atlas submission.
//!
//! Simulation Display vectors own object order. This planner reassembles their
//! positions after class-specific builders and preserves building piece order.

use std::cmp::Ordering;

/// Opaque identifier retained by render planning and its eventual GPU lowering.
pub type DrawId = u64;

/// Coarse `LayerClass` bucket. Lower layers draw first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TacticalLayer(pub u8);

/// Render-Z behavior shared by SHP, VXL, and terrain lowering paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderZPolicy {
    /// Submit without reading or writing render Z.
    None,
    /// Write a nearer opaque source pixel to the render-Z target.
    ReadWrite,
    /// Read render Z without mutating it.
    ReadOnly,
    /// Read and write render Z while using an alpha-capable blitter.
    AlphaReadWrite,
}

/// Source representation consumed by the eventual blitter or GPU pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteEncoding {
    Plain,
    Rle,
    Voxel,
    Terrain,
}

/// Typed policy carrier for the native blitter-family boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlitPolicy {
    pub encoding: SpriteEncoding,
    pub render_z: RenderZPolicy,
    pub translucent: bool,
}

impl BlitPolicy {
    pub const fn opaque(encoding: SpriteEncoding) -> Self {
        Self {
            encoding,
            render_z: RenderZPolicy::ReadWrite,
            translucent: false,
        }
    }

    pub const fn translucent(encoding: SpriteEncoding, render_z: RenderZPolicy) -> Self {
        Self {
            encoding,
            render_z,
            translucent: true,
        }
    }

    /// Opaque blit that tests render Z per pixel without writing it: every
    /// non-building object (`TechnoClass_DrawSHP` a9 = 0 -> flags `0x2E00`,
    /// VXL cache blits `0x2800`; leaves `0x00494b60` / `0x00497fd0`).
    pub const fn z_read(encoding: SpriteEncoding) -> Self {
        Self {
            encoding,
            render_z: RenderZPolicy::ReadOnly,
            translucent: false,
        }
    }

    /// Opaque blit that neither reads nor writes render Z (VERA passthrough
    /// for draws whose native Z behaviour is not yet traced).
    pub const fn z_none(encoding: SpriteEncoding) -> Self {
        Self {
            encoding,
            render_z: RenderZPolicy::None,
            translucent: false,
        }
    }
}

/// Families drawn during the fixed per-cell pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellDrawKind {
    Terrain,
    Smudge,
    FlatOverlay,
    WallOverlay,
    PrimaryCellObject,
}

/// One already-cell-traversed item. Its order is preserved exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellDraw {
    pub id: DrawId,
    pub kind: CellDrawKind,
    pub policy: BlitPolicy,
}

/// Fixed cell-pass families from back to front.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellPass {
    pub terrain: Vec<CellDraw>,
    pub smudges: Vec<CellDraw>,
    /// Flat overlays and walls keep their original cell traversal order.
    pub overlays: Vec<CellDraw>,
    pub primary_objects: Vec<CellDraw>,
}

impl CellPass {
    fn push(&mut self, draw: CellDraw) {
        match draw.kind {
            CellDrawKind::Terrain => self.terrain.push(draw),
            CellDrawKind::Smudge => self.smudges.push(draw),
            CellDrawKind::FlatOverlay | CellDrawKind::WallOverlay => self.overlays.push(draw),
            CellDrawKind::PrimaryCellObject => self.primary_objects.push(draw),
        }
    }
}

/// An object already admitted to an authoritative Display vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectDraw {
    pub id: DrawId,
    pub layer: TacticalLayer,
    /// Position in the retained Display vector, including its sort history.
    pub display_order: u64,
    pub policy: BlitPolicy,
}

/// Building-owned pieces stay grouped inside their parent's object draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingPieceKind {
    BuildupOrSpecial,
    Body,
    Bib,
    PoweredOrActiveOverlay,
    SplitBack,
    SplitFront,
}

/// A fixed-order building-owned blit. `z_bias` is interpreted during lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingPiece {
    pub id: DrawId,
    pub kind: BuildingPieceKind,
    pub z_bias: i32,
    pub policy: BlitPolicy,
}

/// The parent remains globally sortable; its owned pieces never escape this group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildingOwnedPlan {
    pub parent: ObjectDraw,
    pub pieces: Vec<BuildingPiece>,
}

/// One entry in a retained Display layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerEntry {
    Object(ObjectDraw),
    Building(BuildingOwnedPlan),
}

impl LayerEntry {
    pub fn object(&self) -> ObjectDraw {
        match self {
            Self::Object(object) => *object,
            Self::Building(building) => building.parent,
        }
    }
}

/// Layer output in retained Display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectLayerPass {
    pub layer: TacticalLayer,
    pub entries: Vec<LayerEntry>,
}

/// One pure input to the tactical draw planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TacticalDrawInput {
    Cell(CellDraw),
    Object(ObjectDraw),
    Building(BuildingOwnedPlan),
}

/// Presentation plan lowered from the simulation-owned Display vectors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TacticalDrawPlan {
    pub cell_pass: CellPass,
    pub object_layers: Vec<ObjectLayerPass>,
}

impl TacticalDrawPlan {
    /// Reassemble class-specific draws in retained Display order. The sim owns
    /// Submit4A9720 and the single adjacent Ground pass551A30. Tactical6D8F19
    /// walks each vector forward; a fresh full Y-sort here would erase history.
    pub fn build(inputs: impl IntoIterator<Item = TacticalDrawInput>) -> Self {
        let mut plan = Self::default();
        let mut entries = Vec::new();

        for input in inputs {
            match input {
                TacticalDrawInput::Cell(draw) => plan.cell_pass.push(draw),
                TacticalDrawInput::Object(object) => entries.push(LayerEntry::Object(object)),
                TacticalDrawInput::Building(building) => {
                    let mut building = building;
                    building
                        .pieces
                        .sort_by_key(|piece| building_piece_order(piece.kind));
                    entries.push(LayerEntry::Building(building))
                }
            }
        }

        // Atlas emission order cannot replace the retained Display order.
        entries.sort_by(native_layer_order);
        for entry in entries {
            let object = entry.object();
            if let Some(last) = plan
                .object_layers
                .last_mut()
                .filter(|last| last.layer == object.layer)
            {
                last.entries.push(entry);
            } else {
                plan.object_layers.push(ObjectLayerPass {
                    layer: object.layer,
                    entries: vec![entry],
                });
            }
        }

        plan
    }
}

fn building_piece_order(kind: BuildingPieceKind) -> u8 {
    match kind {
        BuildingPieceKind::BuildupOrSpecial => 0,
        BuildingPieceKind::Bib => 1,
        BuildingPieceKind::SplitBack => 2,
        BuildingPieceKind::Body => 3,
        BuildingPieceKind::PoweredOrActiveOverlay => 4,
        BuildingPieceKind::SplitFront => 5,
    }
}

fn native_layer_order(left: &LayerEntry, right: &LayerEntry) -> Ordering {
    let left = left.object();
    let right = right.object();
    left.layer
        .cmp(&right.layer)
        .then_with(|| left.display_order.cmp(&right.display_order))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPAQUE_SHP: BlitPolicy = BlitPolicy::opaque(SpriteEncoding::Plain);

    fn object(id: DrawId, layer: u8, display_order: u64) -> ObjectDraw {
        ObjectDraw {
            id,
            layer: TacticalLayer(layer),
            display_order,
            policy: OPAQUE_SHP,
        }
    }

    #[test]
    fn cell_pass_keeps_native_family_order_and_cell_traversal_order() {
        let plan = TacticalDrawPlan::build([
            TacticalDrawInput::Cell(CellDraw {
                id: 4,
                kind: CellDrawKind::WallOverlay,
                policy: OPAQUE_SHP,
            }),
            TacticalDrawInput::Cell(CellDraw {
                id: 1,
                kind: CellDrawKind::Terrain,
                policy: OPAQUE_SHP,
            }),
            TacticalDrawInput::Cell(CellDraw {
                id: 2,
                kind: CellDrawKind::Smudge,
                policy: OPAQUE_SHP,
            }),
            TacticalDrawInput::Cell(CellDraw {
                id: 3,
                kind: CellDrawKind::FlatOverlay,
                policy: OPAQUE_SHP,
            }),
        ]);

        assert_eq!(
            plan.cell_pass
                .terrain
                .iter()
                .map(|draw| draw.id)
                .collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(
            plan.cell_pass
                .smudges
                .iter()
                .map(|draw| draw.id)
                .collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(
            plan.cell_pass
                .overlays
                .iter()
                .map(|draw| draw.id)
                .collect::<Vec<_>>(),
            [4, 3]
        );
    }

    #[test]
    fn object_layers_preserve_display_positions_across_builder_emission_order() {
        let plan = TacticalDrawPlan::build([
            TacticalDrawInput::Object(object(1, 2, 9)),
            TacticalDrawInput::Object(object(2, 2, 2)),
            TacticalDrawInput::Object(object(3, 1, 1)),
            TacticalDrawInput::Object(object(4, 2, 1)),
            TacticalDrawInput::Object(object(5, 2, 0)),
        ]);

        assert_eq!(plan.object_layers.len(), 2);
        assert_eq!(plan.object_layers[0].layer, TacticalLayer(1));
        assert_eq!(
            plan.object_layers[1]
                .entries
                .iter()
                .map(|entry| entry.object().id)
                .collect::<Vec<_>>(),
            [5, 4, 2, 1]
        );
    }

    #[test]
    fn building_pieces_remain_grouped_inside_global_parent_order() {
        let building = BuildingOwnedPlan {
            parent: object(20, 2, 1),
            pieces: vec![
                BuildingPiece {
                    id: 21,
                    kind: BuildingPieceKind::Bib,
                    z_bias: -1,
                    policy: OPAQUE_SHP,
                },
                BuildingPiece {
                    id: 22,
                    kind: BuildingPieceKind::Body,
                    z_bias: 0,
                    policy: OPAQUE_SHP,
                },
            ],
        };
        let plan = TacticalDrawPlan::build([
            TacticalDrawInput::Object(object(10, 2, 0)),
            TacticalDrawInput::Building(building),
            TacticalDrawInput::Object(object(30, 2, 2)),
        ]);

        let entries = &plan.object_layers[0].entries;
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.object().id)
                .collect::<Vec<_>>(),
            [10, 20, 30]
        );
        let LayerEntry::Building(building) = &entries[1] else {
            panic!("building stays grouped in the parent slot");
        };
        assert_eq!(
            building
                .pieces
                .iter()
                .map(|piece| piece.kind)
                .collect::<Vec<_>>(),
            [BuildingPieceKind::Bib, BuildingPieceKind::Body]
        );
        let LayerEntry::Building(building) = &entries[1] else {
            panic!("building parent must retain its owned draw group");
        };
        assert_eq!(
            building
                .pieces
                .iter()
                .map(|piece| piece.id)
                .collect::<Vec<_>>(),
            [21, 22]
        );
    }

    #[test]
    fn alpha_z_policy_is_explicit_for_future_shared_blitter_lowering() {
        let policy = BlitPolicy::translucent(SpriteEncoding::Rle, RenderZPolicy::AlphaReadWrite);
        assert_eq!(policy.encoding, SpriteEncoding::Rle);
        assert_eq!(policy.render_z, RenderZPolicy::AlphaReadWrite);
        assert!(policy.translucent);
    }
}
