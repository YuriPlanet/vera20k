//! `VoxelAnimClass::DrawIt` geometry and arms, and the production debris draw
//! chain from a death to the pose page.
use super::*;
use crate::render::vxl_raster::{BodyTilt, VxlRenderParams, render_vxl};
use crate::sim::world::display_layers::DisplayLayer;

fn screen(x: i32, y: i32, z: i32) -> [f32; 2] {
    let (x, y) = absolute_leptons_to_screen(x, y, z);
    [x, y]
}

#[test]
fn a_piece_at_a_bridge_deck_shadows_one_lepton_above_it() {
    // Object6D2140 of (2688, 5248, 1040) is native (-300, 315), plus VERA's
    // world row bias of 15 (the projectile builder's bridge-height pin).
    let over = geometry(IVec3::new(2688, 5248, 1040), 624, true);
    assert_eq!(over.body.point, [-300.0, 330.0]);
    assert_eq!(over.body.z_adjust, -2 - adjust_for_z_standard(1040));
    assert_eq!(over.shadow.point, screen(2688, 5248, 1041));
    assert_eq!(over.shadow.z_adjust, -4 - adjust_for_z_standard(1040));
    // Below the deck, or off a bridge, the shadow falls to the ground.
    let below = geometry(IVec3::new(2688, 5248, 1039), 624, true);
    assert_eq!(below.shadow.point, screen(2688, 5248, 624));
    assert_eq!(below.shadow.z_adjust, -4 - adjust_for_z_standard(624));
    let plain = geometry(IVec3::new(2688, 5248, 1040), 624, false);
    assert_eq!(plain.body, over.body);
    assert_eq!(plain.shadow, below.shadow);
}

/// A War Miner killed through the production receiver throws its tyres;
/// each draws from its Display rank as its shadow and then its body, both
/// rasterized from the retail `TIRE.VXL` at the piece's drawing matrix, in
/// its house's Convert at its cell's intensity. Retail rules and archives;
/// skipped without them.
#[test]
fn a_dying_war_miners_tyres_draw_from_admission_to_the_pose_page() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let mut sim = Simulation::with_seed(17);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        32,
    ));
    let miner = sim.substrate_mut_for_test().allocate_stable_id();
    sim.entities_mut_for_test()
        .insert(crate::sim::game_entity::GameEntity::test_default(
            miner,
            "HARV",
            "Americans",
            12,
            12,
        ));
    *sim.interner_mut_for_test() = crate::sim::intern::test_interner();
    assert!(matches!(
        sim.reveal(miner),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));
    sim.receive_own_health_c4(
        miner,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let ids = sim.voxel_anims().ids();
    // [HARV] MaxDebris=6, DebrisTypes=TIRE: this seed's budget is positive.
    assert!(!ids.is_empty(), "the death threw no tyre");
    for _ in 0..6 {
        for &id in &ids {
            sim.visit_voxel_anim(id, rules);
        }
    }

    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let air = sim.display_layers().members(DisplayLayer::AIR).to_vec();
    let first = sim.voxel_anims().get(ids[0]).unwrap().world_coord();
    let [x, y] = screen(first.x, first.y, first.z);
    let view = DebrisView {
        terrain: sim.resolved_terrain().unwrap(),
        camera: [x - 400.0, y - 300.0],
        viewport: [800.0, 600.0],
        axis: DepthAxis::NONE,
    };
    let tire =
        crate::assets::vxl_file::VxlFile::from_bytes(assets.get_ref("TIRE.VXL").unwrap()).unwrap();
    let vpl = crate::assets::vpl_file::VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap())
        .unwrap();
    // Every cell carries its own intensity, so the light names the cell.
    let intensity = |x: u16, y: u16| 500 + 20 * i32::from(x) + i32::from(y);
    let mut light = crate::map::lighting::CellLightGrid::new();
    let profile = light.profiles().default_profile_id();
    for x in 0..32 {
        for y in 0..32 {
            let top = intensity(x, y);
            light.insert_light(
                (x, y),
                crate::map::lighting::CellLight::new(
                    profile, [1000; 3], [1000; 3], 65536, 0, 0, top, top, top, top, top,
                ),
            );
        }
    }
    let house_colors = HouseColorMap::from([(
        "Americans".to_string(),
        crate::rules::house_colors::HouseColorIndex(4),
    )]);
    let mut cache = crate::render::unit_pose_cache::VxlPoseFrameCache::default();
    for &id in &ids {
        let piece = sim.voxel_anims().get(id).unwrap();
        let kind = rules.voxel_anim_type(piece.type_id());
        assert_eq!(kind.image, "TIRE");
        let location = piece.world_coord();
        let palette = piece_palette(piece, kind, &sim, &house_colors, &light);
        let (remap_row, source_palette, lit) = palette;
        assert_eq!(source_palette, 0);
        assert_eq!(remap_row, 5, "Americans' scheme 4");
        let [x, y] = [location.x, location.y].map(|lepton| (lepton / 256) as u16);
        assert_eq!(lit.0[3], intensity(x, y) as u32, "the cell's intensity");
        assert_eq!(
            (lit.0[0] >> 24, lit.0[1] >> 31),
            (53, 1),
            "a scheme Convert"
        );
        let mut rendered = None;
        let object = voxel_anim_draw_instance(
            id,
            piece,
            kind,
            palette,
            &view,
            &order,
            |image, section, m| {
                rendered = Some(m);
                cache.render_tumble(&assets, image, section, m)
            },
        )
        .expect("an admitted piece in view draws");
        assert_eq!(object.parent.layer.0, DisplayLayer::AIR.index());
        assert_eq!(
            object.parent.display_order,
            air.iter().position(|&member| member == id).unwrap() as u64
        );
        let matrix = rendered.unwrap();
        assert_ne!(matrix, glam::Mat4::IDENTITY, "six ticks tumble every tyre");
        let [shadow, body] = object.pieces.as_slice() else {
            panic!("a shadow and a body");
        };
        for drawn in [shadow, body] {
            assert_eq!(drawn.target, ObjectTexture::UnitPosePage(0));
            assert_eq!(drawn.render_z, RenderZPolicy::ReadOnly);
        }
        assert_eq!(shadow.instance.draw_state.fx_flags, FX_SHADOW);
        assert_eq!(shadow.instance.z_adjust, -4.0, "flat level-zero ground");
        assert_eq!(body.instance.draw_state.fx_flags, 0);
        assert_eq!(body.instance.draw_state.remap_row, remap_row);
        assert_eq!(body.instance.palette_light, lit);
        assert_eq!(
            body.instance.z_adjust,
            (-2 - adjust_for_z_standard(location.z)) as f32
        );
        // The body on the page is the retail model at that matrix, lit
        // diffusely, placed at the piece's projected Location.
        let sprite = render_vxl(
            &tire,
            None,
            &VxlRenderParams {
                body_tilt: Some(BodyTilt::Matrix(matrix)),
                lighting: crate::render::vxl_raster::VxlLighting::Diffuse,
                ..VxlRenderParams::default()
            },
            Some(&vpl),
        );
        let [x, y] = screen(location.x, location.y, location.z);
        assert_eq!(
            body.instance.position,
            [x + sprite.offset_x, y + sprite.offset_y]
        );
        assert_eq!(
            body.instance.size,
            [sprite.width as f32, sprite.height as f32]
        );
    }

    // Expiry removes every piece from the store and the Air layer.
    for _ in 0..200 {
        for &id in &ids {
            sim.visit_voxel_anim(id, rules);
        }
    }
    assert!(sim.voxel_anims().is_empty());
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    assert!(
        ids.iter()
            .all(|&id| order.object_draw(id, SpriteEncoding::Voxel).is_none())
    );
}

/// Bullet469DBA leaves a typed piece's owner null. Its production constructor,
/// Logic/Display admission and retail TIRE raster must still reach DrawIt;
/// this presentation fixture supplies the impact coordinate, not combat.
#[test]
fn an_unowned_retail_tyre_draws_with_the_global_plain_convert() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let tire_id = rules.voxel_anim_type_id_by_name("TIRE").unwrap();
    let kind = rules.voxel_anim_type(tire_id);
    assert!(!kind.is_tiberium);
    let mut sim = Simulation::with_seed(17);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        32,
    ));
    sim.emit_debris_voxel(
        tire_id,
        kind,
        None,
        IVec3::new(12 * 256 + 128, 12 * 256 + 128, 0),
        true,
        &mut Vec::new(),
    )
    .unwrap();
    let ids = sim.voxel_anims().ids();
    assert_eq!(ids.len(), 1);
    let id = ids[0];
    for _ in 0..6 {
        sim.visit_voxel_anim(id, rules);
    }
    let piece = sim.voxel_anims().get(id).unwrap();
    assert_eq!(piece.owner_house(), None);
    let mut light = CellLightGrid::new();
    let location = piece.world_coord();
    let cell = ((location.x / 256) as u16, (location.y / 256) as u16);
    light.insert_light(
        cell,
        crate::map::lighting::CellLight::new(
            light.profiles().default_profile_id(),
            [400; 3],
            [400; 3],
            65536,
            0,
            0,
            425,
            425,
            425,
            425,
            425,
        ),
    );
    let palette = piece_palette(piece, kind, &sim, &HouseColorMap::new(), &light);
    assert_eq!(
        palette,
        (0, 1, PaletteLight::plain(53, 1000)),
        "non-tiberium ignores cell intensity"
    );
    let mut tiberium = kind.clone();
    tiberium.is_tiberium = true; // Declared render-header control for the adjacent arm.
    assert_eq!(
        piece_palette(piece, &tiberium, &sim, &HouseColorMap::new(), &light),
        (0, 1, PaletteLight::plain(53, 425)),
    );
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let [x, y] = screen(location.x, location.y, location.z);
    let view = DebrisView {
        terrain: sim.resolved_terrain().unwrap(),
        camera: [x - 400., y - 300.],
        viewport: [800., 600.],
        axis: DepthAxis::NONE,
    };
    let mut cache = crate::render::unit_pose_cache::VxlPoseFrameCache::default();
    assert_eq!(cache.fill_test_page().page, 0);
    let object = voxel_anim_draw_instance(
        id,
        piece,
        kind,
        palette,
        &view,
        &order,
        |image, section, matrix| cache.render_tumble(&assets, image, section, matrix),
    )
    .expect("an unowned admitted tyre reaches the physical TIRE pose page");
    assert_eq!(object.parent.layer.0, DisplayLayer::AIR.index());
    let [shadow, body] = object.pieces.as_slice() else {
        panic!("shadow then body")
    };
    assert_eq!(shadow.instance.draw_state.fx_flags, FX_SHADOW);
    for drawn in [shadow, body] {
        assert!(
            matches!(drawn.target, ObjectTexture::UnitPosePage(page) if page > 0),
            "every actual TIRE part retains its overflow page"
        );
    }
    assert_eq!(body.instance.source_palette, 1);
    assert_eq!(body.instance.draw_state.remap_row, 0);
    assert_eq!(body.instance.palette_light, PaletteLight::plain(53, 1000));
    assert!(body.instance.size.iter().all(|size| *size > 0.));
}
