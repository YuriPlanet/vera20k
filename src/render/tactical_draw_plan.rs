//! Pure tactical draw planning before GPU atlas submission.
//!
//! This models the ordering authority only. The app layer lowers the resulting
//! plan to atlas/page buffers; it does not replace wgpu or asset decoding.

use std::cmp::Ordering;

/// Opaque identifier retained by render planning and its eventual GPU lowering.
pub type DrawId = u64;

/// Integer tactical coordinate in native world units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacticalCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

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

/// An object submitted to the depth-sorted `LayerClass` equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectDraw {
    pub id: DrawId,
    pub layer: TacticalLayer,
    pub coord: TacticalCoord,
    /// Class-owned adjustment, such as an anim's YSortAdjust.
    pub y_sort_adjust: i32,
    /// Registration order resolves equal native Y-sort keys.
    pub registration_order: u64,
    pub policy: BlitPolicy,
}

impl ObjectDraw {
    /// `ObjectClass::GetYSort`: X + Y, then a caller-supplied class adjustment.
    pub fn y_sort_key(self) -> i32 {
        self.coord
            .x
            .wrapping_add(self.coord.y)
            .wrapping_add(self.y_sort_adjust)
    }
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

/// One entry in a depth-sorted layer.
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

/// Layer output after native-shaped integer sorting.
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

/// Pure tactical render authority, modeled after `YR TacticalClass::Draw`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TacticalDrawPlan {
    pub cell_pass: CellPass,
    pub object_layers: Vec<ObjectLayerPass>,
}

impl TacticalDrawPlan {
    /// Build fixed cell passes plus stable `LayerClass` object ordering.
    /// RESIDUAL (GSI-13.12) — `render/tactical_compat.rs`, pass 1's named
    /// suspect, is not an ordering path at all. The legacy world-effect bypass
    /// pass 2 named is gone: bridge explosions are `AnimClass` objects and enter
    /// this planner through their layer.
    /// - **`Submit_Object @ 0x004A9720` sorts only layer 2.** VERA Y-sorts every
    ///   layer, so any two objects sharing another layer can swap against
    ///   retail.
    /// - Trigger: any frame with two objects in a non-ground layer.
    /// - Player effect: one sprite draws in front of or behind another it
    ///   should not.
    /// - Frequency: bounded by that condition rather than continuous.
    /// - Downstream risk: restricting the Y-sort to layer 2 shifts the render
    ///   goldens.
    ///
    /// The owner-attached anim half of this row is CLOSED — GSI-05.12 removed
    /// the short-circuit that used to keep burning-building fires out of this
    /// planner. `AnimClass::GetLayer @ 0x00424CB0` forces layer 2 for any anim
    /// carrying an owner, so `anim_render_destination` routes them through
    /// `anim_object_draw` into the same `ground_objects` vector as buildings and
    /// units, and they sort here on the same key.
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

        // LayerClass uses ObjectClass::GetYSort with registration-order ties.
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
        .then_with(|| left.y_sort_key().cmp(&right.y_sort_key()))
        .then_with(|| left.registration_order.cmp(&right.registration_order))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPAQUE_SHP: BlitPolicy = BlitPolicy::opaque(SpriteEncoding::Plain);

    fn object(id: DrawId, layer: u8, x: i32, y: i32, adjust: i32, registration: u64) -> ObjectDraw {
        ObjectDraw {
            id,
            layer: TacticalLayer(layer),
            coord: TacticalCoord { x, y, z: 0 },
            y_sort_adjust: adjust,
            registration_order: registration,
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
    fn object_layers_use_integer_ysort_then_registration_order() {
        let plan = TacticalDrawPlan::build([
            TacticalDrawInput::Object(object(1, 2, 300, 200, 0, 9)),
            TacticalDrawInput::Object(object(2, 2, 100, 400, 0, 2)),
            TacticalDrawInput::Object(object(3, 1, 999, 999, 0, 1)),
            TacticalDrawInput::Object(object(4, 2, 200, 300, 0, 1)),
            TacticalDrawInput::Object(object(5, 2, 200, 300, 32, 0)),
        ]);

        assert_eq!(plan.object_layers.len(), 2);
        assert_eq!(plan.object_layers[0].layer, TacticalLayer(1));
        assert_eq!(
            plan.object_layers[1]
                .entries
                .iter()
                .map(|entry| entry.object().id)
                .collect::<Vec<_>>(),
            [4, 2, 1, 5]
        );
    }

    #[test]
    fn building_pieces_remain_grouped_inside_global_parent_order() {
        let building = BuildingOwnedPlan {
            parent: object(20, 2, 100, 100, 0, 1),
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
            TacticalDrawInput::Object(object(10, 2, 50, 50, 0, 0)),
            TacticalDrawInput::Building(building),
            TacticalDrawInput::Object(object(30, 2, 200, 200, 0, 2)),
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
