//! Shared Techno705E00 shadow decisions through native Unit draw records.
//! Synthetic atlas entries isolate admission/coordinates; physical masks and
//! the real dispatcher are checked by the retail GPU and normal-game runs.
use super::*;
use crate::map::resolved_terrain::{NativeCellQuery, test_flat_cell, test_grid};
use crate::render::draw_state::DrawDecision;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::sim::game_entity::GameEntity;
use std::collections::HashMap;

fn atlas() -> (SpriteAtlas, ShpSpriteKey, ShpSpriteEntry) {
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::Legacy,
        type_id: "MGTK".to_string(),
        facing: 0,
        frame: 0,
        house_color: HouseColorIndex(0),
    };
    let entry = ShpSpriteEntry {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [2.0; 2],
        offset_x: 0.0,
        offset_y: 0.0,
        canvas_rect: [0.0, 0.0, 2.0, 2.0],
        extended: true,
        page: 0,
        source_palette: 1,
    };
    let mut shadow_key = key.clone();
    shadow_key.frame = 1;
    let mut atlas = SpriteAtlas::from_test_entries(HashMap::from([
        (key.clone(), entry),
        (shadow_key, ShpSpriteEntry { page: 1, ..entry }),
    ]));
    atlas.set_object_shp_frame_count_for_test("MGTK", 2);
    (atlas, key, entry)
}

#[test]
fn unit_shadow_matches_original_type_gate_point_and_read_only_policy() {
    let original = crate::test_fixture::json("tools/spatial_oracle/mirage_disguise.json");
    let terrain = test_grid(100, 60, |x, y| {
        let mut cell = test_flat_cell(x, y);
        cell.level = 4;
        cell
    });
    let cells = NativeCellQuery::isolated(&terrain);
    let (atlas, key, entry) = atlas();
    for row in original["drawing"].as_array().unwrap() {
        let input = &row["input"];
        let mut entity = GameEntity::test_default_of_category(
            1,
            "MGTK",
            "Americans",
            input["cell"][0].as_u64().unwrap() as u16,
            input["cell"][1].as_u64().unwrap() as u16,
            EntityCategory::Unit,
        );
        entity
            .position
            .set_exact_z_leptons_for_test(Some(input["native_world_z"].as_i64().unwrap() as i32));
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[VehicleTypes]\n0=MGTK\n[MGTK]\nNoShadow={}\n",
                if row["actual_type"]["no_shadow"] == 1 {
                    "yes"
                } else {
                    "no"
                },
            )),
        )
        .unwrap();
        let draws = row["draws"].as_array().unwrap();
        let body = &draws[0];
        let state = DrawState::from_native_selector(1, (body["flags"].as_u64().unwrap() & 6) as u8);
        let piece = embedded_shp_shadow_piece(
            &atlas,
            rules.object("MGTK"),
            &entity,
            Some(&cells),
            &key,
            &entry,
            SpriteInstance {
                position: [
                    body["point"][0].as_i64().unwrap() as f32,
                    body["point"][1].as_i64().unwrap() as f32,
                ],
                draw_state: state,
                ..Default::default()
            },
            DrawDecision {
                visible: true,
                state,
                native_visual_state: 0,
            },
            BlitPolicy::z_read(SpriteEncoding::Plain),
        );
        assert_eq!(piece.is_some(), draws.len() == 2, "{}", row["name"]);
        if let Some(piece) = piece {
            let shadow = &draws[1];
            assert_eq!(
                piece.target,
                ObjectTexture::SpriteAtlasEdit(
                    1,
                    crate::render::terrain_draw::TerrainPiece::Shadow
                )
            );
            assert_eq!(piece.render_z, RenderZPolicy::ReadOnly);
            assert_eq!(shadow["flags"].as_u64().unwrap() & 0x4000, 0);
            assert_eq!(
                piece.instance.position,
                [
                    shadow["point"][0].as_i64().unwrap() as f32,
                    shadow["point"][1].as_i64().unwrap() as f32
                ]
            );
            assert_eq!(
                piece.instance.z_adjust,
                shadow["z_adjust"].as_i64().unwrap() as f32
            );
            assert_eq!(
                piece.instance.z_gradient,
                shadow["gradient"].as_u64().unwrap() as u32
            );
            assert_eq!(
                piece.instance.draw_state.native_selector_bits(),
                (shadow["flags"].as_u64().unwrap() & 6) as u32
            );
        }
    }
}

#[test]
fn unit_shadow_queries_live_ground_and_bridge_height() {
    let terrain = test_grid(5, 5, |x, y| {
        let mut cell = test_flat_cell(x, y);
        cell.level = 4;
        cell
    });
    let cells = NativeCellQuery::isolated(&terrain);
    let mut entity =
        GameEntity::test_default_of_category(1, "DRON", "Americans", 2, 2, EntityCategory::Unit);
    let decision = DrawDecision {
        visible: true,
        state: DrawState::default(),
        native_visual_state: 0,
    };
    for (z, bridge, admitted) in [
        (415, false, false),
        (416, false, true),
        (417, false, false),
        (832, false, false),
        (832, true, true),
    ] {
        entity.position.set_exact_z_leptons_for_test(Some(z));
        entity.set_on_bridge(bridge);
        assert_eq!(
            embedded_shp_shadow_admitted(None, &entity, Some(&cells), decision),
            admitted,
            "worldZ={z}, OnBridge={bridge}"
        );
    }
}
