//! Retained stock Kirov bombs must reach the same body builder as other
//! Bullets. Flight coordinates here are supplied at the render boundary;
//! actual normal/elite launches and numeric poses have separate coverage.
use super::*;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectilePayload, ProjectileSpawn, ProjectileTarget,
    ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
};

#[test]
fn admitted_stock_kirov_bomb_has_a_visible_voxel_body() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let zep = rules.object("ZEP").unwrap();
    assert_eq!(zep.primary(), Some("BlimpBomb"));
    assert_eq!(zep.elite_primary(), Some("BlimpBombE"));
    let model =
        crate::assets::vxl_file::VxlFile::from_bytes(assets.get_ref("ZBOMB.VXL").unwrap()).unwrap();
    let hva =
        crate::assets::hva_file::HvaFile::from_bytes(assets.get_ref("ZBOMB.HVA").unwrap()).unwrap();
    assert_eq!(model.limbs.len(), 1);
    assert_eq!(hva.frame_count, 1);
    let mut cache = crate::render::unit_pose_cache::VxlPoseFrameCache::default();
    for weapon_name in ["BlimpBomb", "BlimpBombE"] {
        cache.begin_frame();
        let weapon = rules.weapon(weapon_name).unwrap();
        assert_eq!(weapon.speed, 52);
        assert_eq!(rules.general.gravity, 6);
        let type_id = weapon.projectile.as_deref().unwrap();
        let kind = rules.projectile(type_id).unwrap();
        assert_eq!(type_id, "BlimpBombP");
        assert_eq!(kind.image.as_deref(), Some("ZBOMB"));
        assert!(kind.voxel && kind.vertical && !kind.inviso && !kind.flat);
        assert!(!kind.arcing && !kind.floater);
        assert_eq!(kind.acceleration, 1);
        assert_eq!(kind.voxel_image_load(), Some("ZBOMB"));
        assert_eq!(kind.color_scheme(), 1);
        let (mut sim, id) = admitted_kirov_bomb(rules, weapon_name);
        let position = sim.projectiles.get(id).unwrap().position;
        let order = NativeDisplayOrder::from_display(sim.display_layers());
        let (x, y) = absolute_leptons_to_screen(position.x, position.y, position.z);
        let object = projectile_draw_instance(
            sim.projectiles.get(id).unwrap(),
            kind,
            type_id,
            HouseColorIndex(0),
            None,
            sim.resolved_terrain.as_ref().unwrap(),
            [x - 400.0, y - 300.0],
            [800.0, 600.0],
            DepthAxis::NONE,
            &order,
            |image, matrix| {
                cache.render_matrix(
                    &assets,
                    image,
                    matrix,
                    0,
                    true,
                    crate::render::vxl_raster::VxlLighting::Diffuse,
                )
            },
        )
        .expect("an admitted in-view stock Kirov bomb must draw its voxel body");
        assert_eq!(
            object.parent.layer.0,
            crate::sim::world::display_layers::DisplayLayer::AIR.index()
        );
        assert_eq!(object.pieces.len(), 1, "the voxel branch has no SHP shadow");
        assert_eq!(object.parent.policy.render_z, RenderZPolicy::None);
        assert_eq!(object.pieces[0].render_z, RenderZPolicy::None);
        assert_eq!(object.pieces[0].target, ObjectTexture::UnitPose);
        assert_eq!(
            object.pieces[0].instance.draw_state.remap_row, 1,
            "native scheme1 is Colors entry0, not entry1"
        );
        assert_eq!(
            object.pieces[0].instance.palette_light,
            PaletteLight::new([1000; 3], 53, 1000, true)
        );
        assert_eq!(object.pieces[0].instance.z_adjust, 0.0);
        assert_eq!(object.pieces[0].instance.z_gradient, 2);
        assert!(
            object.pieces[0]
                .instance
                .size
                .iter()
                .all(|&size| size > 0.0)
        );
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/bullet_fixtures.json",
        ))
        .unwrap();
        let shot = corpus["flight"]["shots"]
            .as_array()
            .unwrap()
            .iter()
            .find(|shot| shot["weapon"] == weapon_name)
            .unwrap();
        for frame in 1..=8 {
            sim.session.binary_frame = frame;
            sim.visit_combat_tail(id, rules, None, crate::sim::world::FrameEffects::default());
            if let Some(expected) = shot["frames"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["frame"] == frame)
            {
                let bullet = sim.projectiles.get(id).unwrap();
                assert_eq!(
                    [bullet.position.x, bullet.position.y, bullet.position.z],
                    std::array::from_fn::<_, 3, _>(
                        |i| expected["candidate"][i].as_i64().unwrap() as i32
                    )
                );
                for (actual, expected) in bullet
                    .velocity
                    .native()
                    .iter()
                    .zip(expected["bits"].as_array().unwrap())
                {
                    assert_eq!(
                        actual.bits(),
                        u64::from_str_radix(expected.as_str().unwrap(), 16).unwrap()
                    );
                }
            }
        }
        assert!(sim.retire_non_entity_object(id));
        let removed = NativeDisplayOrder::from_display(sim.display_layers());
        assert!(
            projectile_draw_instance(
                sim.projectiles.get(id).unwrap(),
                kind,
                type_id,
                HouseColorIndex(0),
                None,
                sim.resolved_terrain.as_ref().unwrap(),
                [x - 400.0, y - 300.0],
                [800.0, 600.0],
                DepthAxis::NONE,
                &removed,
                |_, _| panic!("retired bullet rasterized")
            )
            .is_none()
        );
        sim.flush_pending_delete();
        assert!(sim.projectiles.get(id).is_none());
    }
}

/// Admit the original flight fixture's stock-typed bomb through the production
/// Bullet lifecycle. Native launch math is independently covered by the source
/// corpus; the retained first velocity is not rounded to integer displacement.
pub(crate) fn admitted_kirov_bomb(
    rules: &crate::rules::ruleset::RuleSet,
    weapon_name: &str,
) -> (crate::sim::world::Simulation, u64) {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/bullet_fixtures.json",
    ))
    .unwrap();
    let shot = corpus["flight"]["shots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|shot| shot["weapon"] == weapon_name)
        .unwrap();
    let point = |v: &serde_json::Value| {
        ProjectileCoord::new(
            v[0].as_i64().unwrap() as i32,
            v[1].as_i64().unwrap() as i32,
            v[2].as_i64().unwrap() as i32,
        )
    };
    let position = point(&shot["origin"]);
    let target = point(&shot["target"]);
    let weapon = rules.weapon(weapon_name).unwrap();
    let kind = rules
        .projectile(weapon.projectile.as_deref().unwrap())
        .unwrap();
    assert_eq!(i64::from(weapon.speed), shot["speed"]["input"]["speed"]);
    let mut sim = crate::sim::world::Simulation::with_seed(31);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        32,
    ));
    let payload = ProjectilePayload::new(
        weapon.damage,
        sim.interner.intern(weapon.warhead.as_deref().unwrap()),
        sim.interner.intern(weapon_name),
    );
    let id = sim.allocate_stable_id();
    let velocity = ProjectileVelocity::from_native(std::array::from_fn(|i| {
        crate::util::native_x87::NativeF64Bits::from_bits(
            u64::from_str_radix(shot["launch"]["bits"][i].as_str().unwrap(), 16).unwrap(),
        )
    }));
    sim.admit_projectile(
        id,
        ProjectileSpawn {
            native_unique_id: 1,
            line_trail: None,
            flat: kind.flat,
            source_id: 0,
            origin: position,
            target: ProjectileTarget::Cell {
                rx: (target.x / 256) as u16,
                ry: (target.y / 256) as u16,
            },
            initial_target_position: target,
            payload,
            speed_leptons_per_frame: shot["launch"]["speed"].as_u64().unwrap() as u16,
            velocity,
            trajectory: ProjectileTrajectory::Vertical {
                detonation_altitude: kind.detonation_altitude,
                acceleration: kind.acceleration,
                max_speed: weapon.speed,
            },
            guidance: None,
            visual: ProjectileVisualState::new(
                kind.anim_low as u8,
                kind.anim_high as u8,
                kind.anim_rate as u8,
            ),
            arm_frames: kind.arm,
            fuse_frames: None,
            ranged_fuse: kind.ranged,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    (sim, id)
}
