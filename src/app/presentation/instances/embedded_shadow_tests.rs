//! Shared Techno705E00 shadow decisions through native Unit draw records.
//! Synthetic atlas entries isolate admission/coordinates; physical masks and
//! the real dispatcher are checked by the retail GPU and normal-game runs.
use super::*;
use crate::app::presentation::frame::ObjectRecord;
use crate::map::resolved_terrain::{NativeCellQuery, test_flat_cell, test_grid};
use crate::render::draw_state::DrawDecision;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::sim::game_entity::GameEntity;
use std::collections::HashMap;

fn atlas() -> (SpriteAtlas, ShpSpriteKey, ShpSpriteEntry) {
    let key = ShpSpriteKey {
        palette_context: ShpPaletteContext::Legacy,
        type_id: "MGTK".into(),
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
fn infantry_embedded_shadow_matches_original_positive_and_no_shadow_controls() {
    let native =
        crate::test_fixture::json("tools/procedural_drawing_oracle/infantry_embedded_shadow.json");
    let (atlas, key, entry) = atlas();
    let entity = GameEntity::test_default_of_category(
        1,
        "CONTROL",
        "Americans",
        2,
        2,
        EntityCategory::Infantry,
    );
    for row in native["rows"].as_array().unwrap() {
        let suppressed = row["name"] == "no_shadow";
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[InfantryTypes]\n0=CONTROL\n[CONTROL]\nNoShadow={}\n",
                if suppressed { "yes" } else { "no" },
            )),
        )
        .unwrap();
        let piece = embedded_shp_shadow_piece(
            &atlas,
            rules.object("CONTROL"),
            &ObjectRecord::of_test_in(&entity, None, &rules),
            None,
            &key,
            &entry,
            SpriteInstance::default(),
            DrawDecision {
                visible: true,
                state: DrawState::default(),
                native_visual_state: 0,
            },
            BlitPolicy::z_read(SpriteEncoding::Plain),
        );
        let expected = row["draws"].as_array().unwrap().get(1);
        assert_eq!(piece.is_some(), expected.is_some(), "{}", row["name"]);
        if let (Some(piece), Some(expected)) = (piece, expected) {
            assert_eq!(
                expected["frame"].as_u64().unwrap(),
                native["input"]["original_frame_count"].as_u64().unwrap() / 2,
            );
            assert_eq!(
                piece.instance.z_adjust,
                expected["z_adjust"].as_i64().unwrap() as f32
            );
            assert_eq!(
                piece.instance.z_gradient,
                expected["gradient"].as_u64().unwrap() as u32
            );
            assert_eq!(piece.render_z, RenderZPolicy::ReadOnly);
            assert_ne!(
                expected["flags"].as_u64().unwrap() & 1,
                0,
                "native stencil draw"
            );
        }
    }
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
            &ObjectRecord::of_test_in(&entity, Some(&terrain), &rules),
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
fn unit_and_infantry_shadow_query_live_ground_and_bridge_height() {
    let terrain = test_grid(5, 5, |x, y| {
        let mut cell = test_flat_cell(x, y);
        cell.level = 4;
        cell
    });
    let cells = NativeCellQuery::isolated(&terrain);
    let decision = DrawDecision {
        visible: true,
        state: DrawState::default(),
        native_visual_state: 0,
    };
    for category in [EntityCategory::Unit, EntityCategory::Infantry] {
        let mut entity =
            GameEntity::test_default_of_category(1, "CONTROL", "Americans", 2, 2, category);
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
                embedded_shp_shadow_admitted(
                    None,
                    &ObjectRecord::of_test_in(
                        &entity,
                        Some(&terrain),
                        crate::rules::ruleset::RuleSet::empty_for_test()
                    ),
                    Some(&cells),
                    decision
                ),
                admitted,
                "{category:?}: worldZ={z}, OnBridge={bridge}"
            );
        }
    }
}

/// Stock gamemd refuses a visceroid Unit's embedded shadow before the height
/// test (`0x00705FED..0x00706007`), while the turreted branch's own shadow
/// (`0x0073C733..0x0073C7AC`) has no gate: `NoShadow=yes` stops only the
/// embedded one.
#[test]
fn a_visceroid_refuses_the_embedded_shadow_but_the_turreted_one_has_no_gate() {
    let (atlas, key, _) = atlas();
    let decision = DrawDecision {
        visible: true,
        state: DrawState::default(),
        native_visual_state: 0,
    };
    let entity =
        GameEntity::test_default_of_category(1, "MGTK", "Americans", 2, 2, EntityCategory::Unit);
    let rules_with = |keys: &str| {
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            &format!("[VehicleTypes]\n0=MGTK\n[MGTK]\n{keys}"),
        ))
        .unwrap()
    };
    for (keys, admitted) in [
        ("", true),
        ("SmallVisceroid=yes\n", false),
        ("LargeVisceroid=yes\n", false),
        ("NoShadow=yes\n", false),
    ] {
        let rules = rules_with(keys);
        let record = ObjectRecord::of_test_in(&entity, None, &rules);
        assert_eq!(
            embedded_shp_shadow_admitted(rules.object("MGTK"), &record, None, decision),
            admitted,
            "{keys:?}"
        );
    }
    let rules = rules_with("NoShadow=yes\n");
    let piece = stock_turreted_shp_shadow_piece(
        &atlas,
        crate::app::presentation::instances::foot_depth::DepthWorld {
            runtime: None,
            frame: None,
        },
        &ObjectRecord::of_test_in(&entity, None, &rules),
        &key,
        [10.0, 20.0],
        SpriteInstance::default(),
        BlitPolicy::z_read(SpriteEncoding::Plain),
    )
    .expect("the turreted branch draws the body frame's shadow half");
    assert_eq!(
        piece.target,
        ObjectTexture::SpriteAtlasEdit(1, crate::render::terrain_draw::TerrainPiece::Shadow)
    );
    assert_eq!(piece.instance.position, [10.0, 20.0]);
    assert_eq!(piece.instance.z_adjust, -2.0);
}

/// The turreted SHP branch sets Unit `+0x6D3` (`0x0073C7F2`..`0x0073CDF9`),
/// and `TechnoClass_DrawSHP` draws no embedded shadow for a Unit with it set
/// (`0x00706151..0x00706163`). A stock turreted SHP Unit therefore draws one
/// shadow, the branch's own (`0x0073C733`), under its body and turret.
/// The pieces a stock turreted SHP Unit draws: eight body frames at the
/// draw point, the 32-frame turret ring from frame 8 three pixels higher,
/// then the shadow half. Drawn as a Display member, or as the cargo a
/// Carryall draws at its body point `carrier`.
fn stock_turreted_shp_pieces(carrier: Option<[f32; 2]>) -> Vec<ObjectPieceInstance> {
    use crate::app::presentation::frame::PresentationFrame;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::runtime::{SimResources, SimRuntime};

    let art = IniFile::from_str("[TANK]\nVoxel=no\nWalkFrames=1\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\nTurret=yes\nSpeed=4\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
        ),
        &art,
    )
    .unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    assert!(!rules.ares());
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let id = sim
        .spawn_object("TANK", "Americans", 10, 20, 0, &rules)
        .unwrap();
    let runtime = SimRuntime {
        simulation: sim,
        resources: SimResources::new(rules).into(),
    };
    let frame = PresentationFrame::capture(&runtime, &Default::default(), false);
    let record = frame.object(id).unwrap();
    assert!(!record.is_voxel());
    // Eight body frames, the 32-frame turret ring from frame 8, then the
    // shadow half.
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
    let mut atlas = SpriteAtlas::from_test_entries(
        (0..80)
            .map(|frame| {
                (
                    ShpSpriteKey {
                        palette_context: ShpPaletteContext::Legacy,
                        type_id: "TANK".into(),
                        facing: 0,
                        frame,
                        house_color: HouseColorIndex(0),
                    },
                    ShpSpriteEntry {
                        page: u8::from(frame >= 40),
                        offset_y: if (8..40).contains(&frame) { -3.0 } else { 0.0 },
                        ..entry
                    },
                )
            })
            .collect(),
    );
    atlas.set_object_shp_frame_count_for_test("TANK", 80);
    let [x, y] = record.pose.screen;
    let camera = [x - 400.0, y - 300.0];
    let colors =
        crate::map::houses::HouseColorMap::from([("Americans".to_owned(), HouseColorIndex(0))]);
    let mut draws = crate::app::presentation::render::draw_plan_lowering::PlannedObjects::default();
    let input = ShpInputs {
        runtime: &runtime,
        frame: &frame,
        atlas: Some(&atlas),
        overlay_atlas: None,
        unit_atlas: None,
        voxel_images: &Default::default(),
        zshape_skips: &Default::default(),
        depth_tested_images: None,
        camera,
        render_size: [800, 600],
        zoom: 1.0,
        building_viewport: crate::app::presentation::render::tactical_world_viewport(
            camera,
            [0, 0, 800, 600],
            None,
        ),
        axis: crate::render::batch::DepthAxis {
            origin_y: 0.0,
            world_height: 10_000.0,
        },
        local_owner: Some("Americans"),
        ignore_visibility: true,
        lighting: &crate::map::lighting::CellLightGrid::new(),
        house_colors: &colors,
        building_zshape: None,
    };
    let order = frame.display_order(&runtime.simulation);
    if let Some(carrier) = carrier {
        let mut cargo = record.clone();
        cargo.on_carryall = true;
        let mut pieces = Vec::new();
        shp_visitor(&input, order)(
            ShpDraw::Carried {
                carrier,
                pieces: &mut pieces,
            },
            &cargo,
        );
        return pieces;
    }
    build_shp_instances_from_inputs(&input, &mut draws, order);
    draws
        .into_owned()
        .iter()
        .filter(|draw| draw.parent.id == id)
        .flat_map(|draw| draw.pieces.iter().cloned())
        .collect()
}

#[test]
fn a_turreted_shp_unit_draws_only_its_branch_shadow() {
    let targets: Vec<_> = stock_turreted_shp_pieces(None)
        .iter()
        .map(|piece| piece.target)
        .collect();
    let shadow =
        ObjectTexture::SpriteAtlasEdit(1, crate::render::terrain_draw::TerrainPiece::Shadow);
    assert_eq!(
        targets,
        [shadow, ObjectTexture::ShpPage(0), ObjectTexture::ShpPage(0)],
        "branch shadow, body, turret"
    );
}

/// The branch draws body and turret to a scratch surface whose rectangle
/// the Unit composite (`0x0073B140`) blits: both seed from the union of
/// their rectangles, with FootClass's Z adjust and the vertical gradient,
/// not the no-turret branch's own rectangle.
#[test]
fn a_turreted_shp_units_body_and_turret_share_the_composite() {
    let pieces = stock_turreted_shp_pieces(None);
    let [body, turret] = [&pieces[1].instance, &pieces[2].instance];
    assert_eq!(turret.position[1], body.position[1] - 3.0);
    for part in [body, turret] {
        assert_eq!(part.zshape_origin, [turret.position[1], 5.0]);
        assert_eq!(
            part.z_gradient,
            crate::render::native_z::pack_voxel_z_gradient(
                crate::render::native_z::ZGradient::Vertical,
                false
            )
        );
        assert_eq!(part.z_adjust, body.z_adjust);
    }
}

/// A Carryall draws its cargo at its own body point (`AircraftClass::Draw_It`,
/// `0x0041461D..0x00414653`). DrawIt lowers a carried Unit's body 14 pixels
/// (`0x0073CF16`), and the turreted branch raises its shadow back to that
/// point (`0x0073C753`).
#[test]
fn a_carried_shp_unit_hangs_below_its_carryall_with_its_shadow_at_it() {
    let carrier = [500.0, 200.0];
    let pieces = stock_turreted_shp_pieces(Some(carrier));
    let positions: Vec<_> = pieces.iter().map(|piece| piece.instance.position).collect();
    let [x, y] = carrier;
    assert_eq!(
        positions,
        [[x, y], [x, y + 14.0], [x, y + 14.0 - 3.0]],
        "shadow, body, turret"
    );
}
