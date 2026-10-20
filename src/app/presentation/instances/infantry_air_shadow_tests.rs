//! The shadow under an airborne infantry (`InfantryClass::DrawIt @
//! 0x00518F90`, `0x0051945D..0x0051958A`). The atlas is synthetic; real
//! pixels come from captures.
use super::*;
use crate::map::bridge_facts::{BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL};
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainPiece;
use crate::sim::game_entity::GameEntity;
use crate::util::native_x87::adjust_for_z_standard;
use std::collections::HashMap;

fn shadow_atlas() -> SpriteAtlas {
    SpriteAtlas::from_test_entries(HashMap::from([(
        ShpSpriteKey {
            palette_context: ShpPaletteContext::GlobalAnim,
            type_id: "INFSHDW".to_string(),
            facing: 0,
            frame: 1,
            house_color: HouseColorIndex(0),
        },
        ShpSpriteEntry {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [12.0, 6.0],
            offset_x: -6.0,
            offset_y: -3.0,
            canvas_rect: [0.0, 0.0, 12.0, 6.0],
            extended: false,
            page: 3,
            source_palette: 1,
        },
    )]))
}

/// Flat level-0 ground; `bridges` are cells given `flags`.
fn terrain(bridges: &[((u16, u16), u32)]) -> ResolvedTerrainGrid {
    let mut terrain =
        crate::map::resolved_terrain::test_grid(5, 5, crate::map::resolved_terrain::test_flat_cell);
    for &((x, y), flags) in bridges {
        terrain
            .cell_mut_for_test(x, y)
            .unwrap()
            .bridge_facts
            .raw_flags = flags;
    }
    terrain
}

/// A Rocketeer at cell (2,2)'s centre, `z` leptons up.
fn rocketeer(z: i32) -> GameEntity {
    let mut entity = GameEntity::test_default_of_category(
        1,
        "JUMPJET",
        "Americans",
        2,
        2,
        EntityCategory::Infantry,
    );
    entity.position.set_exact_z_leptons_for_test(Some(z));
    entity
}

fn shadow(z: i32, bridges: &[((u16, u16), u32)]) -> Option<ObjectPieceInstance> {
    let terrain = terrain(bridges);
    let cells = NativeCellQuery::isolated(&terrain);
    infantry_air_shadow_piece(
        &shadow_atlas(),
        &rocketeer(z),
        Some(&cells),
        [100.0, 50.0],
        0.25,
    )
}

/// Frame 1 at the body point lowered by `AdjustForZ(height)`, as a read-only
/// shadow edit with gradient 0 and `a7 = -5 - AdjustForZ(Z - height)`.
fn expected_at(height: i32, z: i32) -> ([f32; 2], f32) {
    (
        [
            100.0 - 6.0,
            50.0 + adjust_for_z_standard(height) as f32 - 3.0,
        ],
        (-5 - adjust_for_z_standard(z - height)) as f32,
    )
}

#[test]
fn only_an_airborne_infantry_casts_the_infshdw_shadow() {
    assert!(shadow(0, &[]).is_none());
    let piece = shadow(500, &[]).expect("a Rocketeer 500 leptons up casts a shadow");
    assert_eq!(
        piece.target,
        ObjectTexture::SpriteAtlasEdit(3, TerrainPiece::Shadow)
    );
    assert_eq!(piece.render_z, RenderZPolicy::ReadOnly);
    assert_eq!(
        piece.instance.z_gradient,
        pack_z_gradient(ZGradient::Flat, false)
    );
    assert_eq!(piece.instance.depth, 0.25);
    assert_eq!(
        (piece.instance.position, piece.instance.z_adjust),
        expected_at(500, 500)
    );
}

/// `0x0051949C..0x005194E1`: over a structural bridge cell at deck height or
/// above, the deck comes off the height when the neighbour along the bridge
/// (north with flag `0x800`, else west) is also structural.
#[test]
fn a_shadow_over_a_bridge_falls_on_its_deck() {
    let deck = crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
    let west = ((1, 2), BRIDGE_FLAG_STRUCTURAL);
    let north = ((2, 1), BRIDGE_FLAG_STRUCTURAL);
    let along_x = ((2, 2), BRIDGE_FLAG_STRUCTURAL);
    let along_y = ((2, 2), BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO);
    let at = |z, bridges: &[_]| {
        let piece = shadow(z, bridges).unwrap();
        (piece.instance.position, piece.instance.z_adjust)
    };
    assert_eq!(at(600, &[along_x, west]), expected_at(600 - deck, 600));
    assert_eq!(at(600, &[along_y, north]), expected_at(600 - deck, 600));
    // The other axis's neighbour does not count.
    assert_eq!(at(600, &[along_x, north]), expected_at(600, 600));
    assert_eq!(at(600, &[along_y, west]), expected_at(600, 600));
    // Below the deck the height stands.
    assert_eq!(
        at(deck - 1, &[along_x, west]),
        expected_at(deck - 1, deck - 1)
    );
    // Exactly at the deck the shadow has no height left.
    assert!(shadow(deck, &[along_x, west]).is_none());
}
