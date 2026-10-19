//! Retail GAPOWR embedded-SHP shadow through its production helper, lowerer
//! and normal object dispatcher. Caller Z comes from70647A/70649E; pixel
//! expectations come from the original497390 corpus. The full collector is
//! exercised separately by normal app capture, not by this component fixture.

use super::draw_plan_lowering::{
    NativeDisplayOrder, ObjectTexture, PlannedObjectInstance, lower_object_instances,
};
use super::merge_passes::draw_native_object_pass;
use crate::app::presentation::instances::{
    TacticalEntityPurpose, building_embedded_shadow_piece, observer_draw_context,
    tactical_entity_admission,
};
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::houses::{HouseColorMap, parse_house_roster};
use crate::render::batch::{BatchRenderer, DepthAxis, InstanceBufferPool, SpriteInstance};
use crate::render::native_z::{DEFAULT_Z, ZGradient, pack_z_gradient, sprite_row_z, stored_z};
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteKey, build_sprite_atlas};
use crate::render::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
use crate::render::terrain_draw::{TerrainDrawRenderer, TerrainPiece};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use crate::rules::house_colors::HouseColorIndex;
use std::collections::HashSet;

#[test]
#[ignore = "requires GPU and physical retail XMP03T4 assets; GAPOWR production shadow plan through native destination edit"]
fn retail_gapowr_embedded_shadow_uses_real_atlas_mask_and_native_leaf() {
    let root = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .expect("retail game configuration")
                .paths
                .ra2_dir
        });
    let mut scene = crate::headless_scenario::load(&root, "XMP03T4.MAP", 0x1730).unwrap();
    assert_eq!(scene.map.header.theater, "TEMPERATE");
    let owner_name = "ShadowFixture";
    let owner_id =
        scene.add_human_house(owner_name, crate::skirmish_launch::LaunchCountry::America);
    let (map_width, map_height) = {
        let terrain = scene.sim().resolved_terrain.as_ref().unwrap();
        (
            u16::try_from(terrain.width()).expect("map width fits native cell"),
            u16::try_from(terrain.height()).expect("map height fits native cell"),
        )
    };
    let building_id = {
        let (sim, rules) = (
            &mut scene.runtime.simulation,
            &scene.runtime.resources.rules,
        );
        (1..map_height.saturating_sub(1))
            .flat_map(|ry| (1..map_width.saturating_sub(1)).map(move |rx| (rx, ry)))
            .find_map(|(rx, ry)| {
                sim.spawn_object_at_height("GAPOWR", owner_name, rx, ry, 0, 0, rules)
            })
            .expect("an unoccupied XMP03T4 cell admits production GAPOWR spawn")
    };

    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    let entity = sim
        .entities()
        .get(building_id)
        .expect("spawned GAPOWR entity");
    let type_id = sim.interner.resolve(entity.type_ref());
    let owner = sim.interner.resolve(entity.owner());
    assert_eq!(type_id, "GAPOWR");
    assert_eq!(owner, owner_name);
    assert!(entity.lifecycle.object_alive());
    let object = rules.object(type_id).expect("retail GAPOWR ObjectType");
    assert!(!object.no_shadow, "retail GAPOWR leaves NoShadow unset");

    // Match the completed-building GetCurrentFrame owner used by
    // build_shp_instances; the helper receives this actual body key and entry.
    let art = rules.art().resolve_metadata_entry(type_id, &object.image);
    let occupants = entity
        .passenger_role
        .cargo()
        .map(|cargo| cargo.count())
        .unwrap_or(0);
    let body_frame = crate::sim::building_art::body_frame(
        crate::sim::building_art::BodyFrameInput {
            state: entity.building_body_state().unwrap_or(1),
            base_frame: entity.construction_stage_value(),
            laser_frame: object.laser_fence.then_some(0),
            firestorm_frame: object.firestorm_wall.then_some(0),
            gate_stages: object
                .gate
                .then_some(art.map_or(9, |entry| entry.building_gate_stages)),
            occupants: object.can_be_occupied.then_some(occupants as i32),
            tech_level: sim.object_tech_level(object),
            selling: entity.building_down(),
            buildup: [0, 1],
            ordinary: art.map_or([[0, 1]; 4], |entry| {
                entry
                    .building_body_ranges
                    .map(|[start, count, _]| [start, count])
            }),
        },
        entity.health,
        object.strength,
        rules.general.condition_yellow,
        rules.general.condition_red,
    );
    let body_frame = u16::try_from(body_frame).unwrap_or(0);

    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = crate::map::theater::load_theater(&mut assets, &scene.map.header.theater)
        .expect("activate physical temperate archive and palette");
    let mut house_colors: HouseColorMap =
        parse_house_roster(&scene.map.ini, rules.color_schemes.as_slice(), Some(rules)).color_map();
    // add_human_house creates this launch house with color index zero.
    let house_color = HouseColorIndex(0);
    house_colors.insert(owner_name.to_owned(), house_color);
    let atlas = build_sprite_atlas(
        &gpu.device,
        &gpu.queue,
        &batch,
        sim.entities(),
        &assets,
        &theater.unit_palette,
        theater.extension,
        &scene.map.header.theater,
        Some(rules),
        Some(rules.art()),
        &house_colors,
        &[],
        &HashSet::new(),
        &HashSet::new(),
        None,
        None,
        Some(&sim.interner),
    )
    .expect("production atlas for retail scenario and spawned GAPOWR");
    assert_eq!(atlas.object_shp_frame_count(type_id), Some(6));

    let body_key = ShpSpriteKey {
        palette_context: ShpPaletteContext::Legacy,
        type_id: type_id.to_owned(),
        facing: 0,
        frame: body_frame,
        house_color,
    };
    let body_entry = *atlas.get(&body_key).expect("retail body frame atlas entry");
    // The native draw decision is made for this spawned entity by the same
    // admission owner used by the tactical builder, not a test-supplied state.
    let mut observer =
        observer_draw_context(sim, entity, Some(owner_name), Some(owner_id), Some(rules));
    observer.drawn_voxel = Some(false);
    let decision = tactical_entity_admission(
        TacticalEntityPurpose::Drawing,
        entity,
        owner_name,
        Some(owner_name),
        Some(owner_id),
        &sim.fog,
        &sim.house_alliances,
        false,
        sim.session.binary_frame,
        u32::from(house_color.0) + 1,
        observer,
    )
    .expect("ordinary visible GAPOWR DrawDecision");
    assert_eq!(decision.native_visual_state, 0);

    let raw_screen = crate::render::locomotor_visual::screen_position(entity);
    let (screen_x, screen_y) =
        crate::render::locomotor_visual::building_art_anchor(raw_screen.0, raw_screen.1);
    let lift_px = crate::render::locomotor_visual::screen_lift_px(entity);
    let depth = crate::app::presentation::instances::compute_sprite_depth_params_lifted(
        DepthAxis::NONE.origin_y,
        DepthAxis::NONE.world_height,
        screen_y,
        lift_px,
    );
    let body_instance = SpriteInstance {
        position: [
            screen_x + body_entry.offset_x,
            screen_y + body_entry.offset_y,
        ],
        size: body_entry.pixel_size,
        uv_origin: body_entry.uv_origin,
        uv_size: body_entry.uv_size,
        source_palette: body_entry.source_palette,
        depth,
        draw_state: decision.state,
        ..Default::default()
    };
    let shadow = building_embedded_shadow_piece(
        &atlas,
        Some(object),
        entity,
        &body_key,
        &body_entry,
        body_instance,
        decision,
    )
    .expect("production GAPOWR embedded second-half shadow plan");
    // Techno70647A/70649E and70654F/706576 fold both -2 terms into the
    // caller's argument before CC_Draw_Shape. Do not fit the camera to an
    // adjustment read from the implementation under test.
    let expected_shadow_z = -4 - lift_px;
    assert_eq!(shadow.instance.z_adjust, expected_shadow_z as f32);
    assert_eq!(
        shadow.kind,
        crate::render::tactical_draw_plan::BuildingPieceKind::Shadow
    );
    assert_eq!(shadow.policy.render_z, RenderZPolicy::ReadWrite);
    let (page_index, piece) = match shadow.target {
        ObjectTexture::SpriteAtlasEdit(page, piece) => (page, piece),
        other => panic!("building shadow must use indexed atlas edit, got {other:?}"),
    };
    assert_eq!(piece, TerrainPiece::Shadow);
    assert_eq!(
        shadow.instance.z_gradient,
        pack_z_gradient(ZGradient::Flat, false)
    );
    assert_eq!(shadow.instance.zshape_origin, [0.0, 0.0]);

    // Read the actual selected atlas frame's index mask. A populated pixel
    // edits RGB565/Z; a zero index remains a hole in the same real frame.
    let page = &atlas.page(page_index).expect("shadow atlas page").texture;
    let source_size = [
        shadow.instance.size[0].round() as u32,
        shadow.instance.size[1].round() as u32,
    ];
    let source_origin = [
        (shadow.instance.uv_origin[0] * page.width as f32).round() as u32,
        (shadow.instance.uv_origin[1] * page.height as f32).round() as u32,
    ];
    let mask = gpu.read_uint_texels(&page.view, source_origin, source_size);
    assert_eq!(mask.len(), (source_size[0] * source_size[1]) as usize);
    let (solid_index, source_value) = mask
        .iter()
        .enumerate()
        .find(|(_, value)| **value != 0)
        .expect("retail GGPOWR second half has non-zero mask pixels");
    let hole_index = mask
        .iter()
        .position(|value| *value == 0)
        .expect("retail GGPOWR second half has transparent holes");
    assert_ne!(*source_value, 0);

    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/terrain_draw_oracle/fixtures/leaf.json",
    ))
    .unwrap();
    assert_eq!(fixture["leaves"][1], "00497390");
    let leaf = fixture["depth_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            case[0].as_i64() == Some(1)
                && case[1].as_i64() == Some(32767)
                && case[2].as_i64() == Some(0)
                && case[3].as_i64() == Some(32768)
        })
        .expect("native shadow leaf accepted, repeated, strict-Z control");
    let candidate = leaf[1].as_i64().unwrap() as i32;
    let old_z = leaf[3].as_u64().unwrap() as u16;
    let shadow_z = shadow.instance.z_adjust.round() as i32;
    let solid_y = (solid_index / source_size[0] as usize) as i32;
    let mut view = camera(source_size);
    view.camera_pos = shadow.instance.position;
    // Choose the camera from the independently established native caller Z,
    // so an incorrect producer adjustment cannot cancel out of this test.
    let screen_top_goal = DEFAULT_Z + expected_shadow_z - solid_y - candidate;
    let desired_camera_row = shadow.instance.position[1].round() as i32 - screen_top_goal;
    view.native_z_origin_y = desired_camera_row as f32 - view.camera_pos[1];
    let screen_top = shadow.instance.position[1].round() as i32
        - (view.camera_pos[1] + view.native_z_origin_y).round() as i32;
    assert_eq!(
        sprite_row_z(
            ZGradient::Flat,
            screen_top,
            source_size[1] as i32,
            shadow_z,
            solid_y,
        ),
        candidate
    );

    let parent = NativeDisplayOrder::from_display(sim.display_layers())
        .object_draw(entity.stable_id(), SpriteEncoding::Plain)
        .expect("spawned building has production Display registration");
    let layer = usize::from(parent.layer.0);
    let lowered =
        lower_object_instances(vec![PlannedObjectInstance::building(parent, vec![shadow])]);
    let ground = &lowered[layer];
    assert_eq!(ground.runs.len(), 1, "one ordered destination-edit run");
    let run = ground.runs[0];
    assert_eq!(
        run.target,
        ObjectTexture::SpriteAtlasEdit(page_index, TerrainPiece::Shadow)
    );
    assert_eq!(run.render_z, RenderZPolicy::ReadWrite);

    let size = source_size;
    let color = gpu.target(size, format);
    let color_view = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let depth_view = depth.create_view(&Default::default());
    batch.write_camera(&gpu.queue, view);
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    terrain.prepare(&gpu.device, &color, &depth_view, view);
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(
        &gpu.device,
        &gpu.queue,
        "building-shadow",
        &ground.instances,
    );
    let (buffer, count) = pool.get("building-shadow").expect("shadow instance upload");
    assert_eq!(count, 1);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    clear(
        &mut encoder,
        &color_view,
        &depth_view,
        old_z,
        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
    );
    let mut reads = Vec::with_capacity(4);
    for _ in 0..2 {
        draw_native_object_pass(
            &mut encoder,
            &color_view,
            &depth_view,
            &mut terrain,
            [0, 0, size[0], size[1]],
            &batch,
            Some((buffer, count)),
            ground,
            None,
            None,
            None,
            Some(&atlas),
            None,
            batch.default_zshape_bind_group(),
            None,
            None,
        );
        reads.extend([
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ]);
    }
    let output = gpu.finish(encoder, &reads, size);
    let solid_offset = solid_index * 4;
    assert_eq!(
        output[0][solid_offset..solid_offset + 4],
        encoded(leaf[4].as_u64().unwrap() as u16, format),
        "real GAPOWR non-zero shadow mask follows the native first RGB565 leaf"
    );
    assert_eq!(
        output[2][solid_offset..solid_offset + 4],
        encoded(leaf[6].as_u64().unwrap() as u16, format),
        "real GAPOWR non-zero shadow mask follows the native repeated RGB565 leaf"
    );
    assert_eq!(
        stored_z(f32::from_le_bytes(
            output[1][solid_offset..solid_offset + 4]
                .try_into()
                .unwrap()
        )),
        leaf[5].as_u64().unwrap() as u16,
        "first real shadow mask writes the strict native Z winner"
    );
    assert_eq!(
        stored_z(f32::from_le_bytes(
            output[3][solid_offset..solid_offset + 4]
                .try_into()
                .unwrap()
        )),
        leaf[5].as_u64().unwrap() as u16,
        "equal candidate on replay leaves the native Z winner unchanged"
    );
    assert_eq!(
        &output[2][hole_index * 4..hole_index * 4 + 4],
        &encoded(65535, format),
        "zero index remains a transparent hole"
    );
    assert_eq!(
        stored_z(f32::from_le_bytes(
            output[3][hole_index * 4..hole_index * 4 + 4]
                .try_into()
                .unwrap()
        )),
        old_z,
        "hole preserves destination Z"
    );
}
