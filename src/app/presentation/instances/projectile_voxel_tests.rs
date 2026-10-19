//! Stock voxel Bullets through retained Display admission and the body builder.
//! Kirov coordinates are supplied at the render boundary; submarine tests
//! enter through ordinary normal/elite Attack and live Unit FireAt.
use super::*;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectilePayload, ProjectileSpawn, ProjectileTarget,
    ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
};

#[test]
fn stock_normal_submarine_attack_has_a_visible_flat_voxel_torpedo() {
    stock_submarine_attack_has_a_visible_flat_voxel_torpedo(false);
}

#[test]
fn stock_elite_submarine_attack_has_a_visible_flat_voxel_torpedo() {
    stock_submarine_attack_has_a_visible_flat_voxel_torpedo(true);
}

/// Unit741340 -> Techno6FDD50 -> Bullet Fire468670/GetLayer468B90 ->
/// Object5F4B10/Bullet468090/voxel46B0C0. Scene/rank/visibility are supplied;
/// the weapon, projectile, velocity, retained layer and pose are production.
pub(crate) fn launched_stock_submarine_torpedo(
    elite: bool,
) -> Option<(
    crate::rules::ruleset::RuleSet,
    crate::assets::asset_manager::AssetManager,
    crate::sim::world::Simulation,
    u64,
)> {
    use crate::sim::combat::TargetKind;
    use crate::sim::combat::combat_weapon::WeaponSlot;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::world::{FrameEffects, TickLane};

    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return None;
    };
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("Battle rules already opened the retail install");
    let mut rules = battle.rules;
    // Keep the stock bubble Trailer's constructor/Logic asset closure active.
    // Voxel body bytes below enter through the existing model/cache owner.
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    let sub = rules.object("SUB").unwrap();
    assert_eq!(sub.primary(), Some("SubTorpedo"));
    assert_eq!(sub.elite_primary(), Some("SubTorpedoE"));
    let weapon_name = if elite { "SubTorpedoE" } else { "SubTorpedo" };
    let weapon = rules.weapon(weapon_name).unwrap();
    assert_eq!(weapon.projectile.as_deref(), Some("Torpedo"));
    let kind = rules.projectile("Torpedo").unwrap();
    assert_eq!(kind.image.as_deref(), Some("SUBT"));
    assert_eq!(kind.voxel_image_load(), Some("SUBT"));
    assert!(kind.voxel && kind.flat && !kind.inviso && !kind.shadow);
    assert_eq!(kind.trailer.as_deref(), Some("BBBLELRG"));
    assert_eq!(kind.color_scheme(), 1, "native absent Color -> LightGold");
    assert!(
        rules
            .art()
            .anim_runtime_config("BBBLELRG")
            .unwrap()
            .raw_shp_frame_count
            .is_some_and(|count| count > 0),
        "stock bubbles must bind physical SHP bounds"
    );
    let model = crate::assets::vxl_file::VxlFile::from_bytes(
        assets
            .get_ref("SUBT.VXL")
            .expect("physical retail SUBT.VXL"),
    )
    .unwrap();
    let hva = crate::assets::hva_file::HvaFile::from_bytes(
        assets
            .get_ref("SUBT.HVA")
            .expect("physical retail SUBT.HVA"),
    )
    .unwrap();
    assert_eq!(model.limbs.len(), 1);
    let limb = &model.limbs[0];
    assert_eq!(limb.name, "DUMMY01");
    assert_eq!([limb.size_x, limb.size_y, limb.size_z], [13, 3, 3]);
    assert_eq!(limb.voxels.len(), 97);
    assert_eq!((hva.frame_count, hva.section_count), (1, 1));
    assert_eq!(hva.section_names, [limb.name.clone()]);

    let mut sim = crate::sim::arena_fixture::rival_water_arena(&rules, 31, 32);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolve_rule_animation_lists(&rules);
    let source = sim
        .spawn_object("SUB", "Americans", 10, 10, 0, &rules)
        .expect("supplied human submarine enters through legal Water Unlimbo");
    let target = sim
        .spawn_object("LCRF", "Russians", 16, 10, 0, &rules)
        .expect("hostile stock landing craft enters through legal Water Unlimbo");
    if elite {
        sim.substrate
            .entities
            .get_mut(source)
            .unwrap()
            .veterancy
            .set_elite();
    }
    let owner = sim.substrate.entities.get(source).unwrap().owner();
    let target_owner = sim.substrate.entities.get(target).unwrap().owner();
    assert!(sim.houses.get(&owner).unwrap().is_human);
    assert_ne!(owner, target_owner);
    for cell in [(10, 10), (16, 10)] {
        assert!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .unwrap()
                .land_type()
                == crate::rules::terrain_rules::LandType::Water.as_index()
        );
    }
    // Supplied scene visibility; no target, mission, timer or cloak override.
    sim.fog.mark_visible_for_owner(owner, 16, 10);
    sim.fog.mark_visible_for_owner(target_owner, 10, 10);
    sim.session.current_house = Some(owner);
    sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::Attack {
            attacker_id: source,
            target_id: target,
        },
    ));
    for _ in 0..180 {
        let commands = sim.take_due_commands();
        let output = sim
            .advance_app_frame(
                &commands,
                Some(&rules),
                None,
                33,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary submarine Attack frame");
        assert!(output.tick.frame_committed);
        let Some(shot) = output
            .fire_events
            .iter()
            .find(|shot| shot.attacker_id == source)
        else {
            continue;
        };
        assert_eq!(sim.interner.resolve(shot.weapon_id), weapon_name);
        assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
        assert_eq!(shot.target, TargetKind::Entity(target));
        assert!(sim.substrate.entities.get(source).unwrap().health.current() > 0);
        assert!(sim.substrate.entities.get(target).unwrap().health.current() > 0);
        let (&id, bullet) = sim
            .projectiles
            .iter()
            .find(|(_, bullet)| bullet.source_id == source)
            .expect("real SubTorpedo FireAt admits a live Bullet before impact");
        assert_eq!(bullet.payload.installed_weapon(), Some(shot.weapon_id));
        assert_eq!(
            sim.interner
                .resolve(bullet.payload.projectile_type().unwrap()),
            "Torpedo"
        );
        assert!(bullet.in_logic_vector);
        let order = NativeDisplayOrder::from_display(sim.display_layers());
        let parent = order
            .object_draw(id, crate::render::tactical_draw_plan::SpriteEncoding::Voxel)
            .expect("the real Flat Bullet retains Display membership");
        assert_eq!(
            parent.layer.0,
            crate::sim::world::display_layers::DisplayLayer::SURFACE.index()
        );
        return Some((rules, assets, sim, id));
    }
    panic!("ordinary stock {weapon_name} Attack must launch within its live cadence");
}

fn stock_submarine_attack_has_a_visible_flat_voxel_torpedo(elite: bool) {
    let Some((rules, assets, mut sim, id)) = launched_stock_submarine_torpedo(elite) else {
        return;
    };
    let kind = rules.projectile("Torpedo").unwrap();
    let bullet = sim.projectiles.get(id).unwrap();
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let parent = order
        .object_draw(id, crate::render::tactical_draw_plan::SpriteEncoding::Voxel)
        .unwrap();
    let mut cache = crate::render::unit_pose_cache::VxlPoseFrameCache::default();
    let before_draw = sim.state_hash();
    let matrix = crate::render::vxl_raster::projectile_voxel_matrix(bullet.velocity)
        .expect("actual launched velocity provides the native voxel pose");
    cache.begin_frame();
    assert_eq!(cache.fill_test_page().page, 0);
    let body = cache
        .render_matrix(
            &assets,
            "SUBT",
            matrix,
            0,
            true,
            crate::render::vxl_raster::VxlLighting::Diffuse,
        )
        .expect("physical SUBT VXL/HVA/VPL must rasterize before the draw assertion");
    assert!(body.pixel_size.iter().all(|&size| size > 0.0));
    assert!(
        body.page > 0,
        "actual torpedo survives a full preceding pose page"
    );
    let (x, y) =
        absolute_leptons_to_screen(bullet.position.x, bullet.position.y, bullet.position.z);
    let object = projectile_draw_instance(
        bullet,
        kind,
        "Torpedo",
        HouseColorIndex(8),
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
    .expect("an actual in-view stock Flat torpedo must draw its physical voxel body");
    assert_eq!(object.parent.layer, parent.layer);
    assert_eq!(object.parent.policy.render_z, RenderZPolicy::ReadOnly);
    assert_eq!(object.pieces.len(), 1, "voxel body has no SHP shadow");
    let piece = &object.pieces[0];
    assert_eq!(piece.target, ObjectTexture::UnitPosePage(body.page));
    assert_eq!(piece.render_z, RenderZPolicy::ReadOnly);
    assert_eq!(piece.instance.size, body.pixel_size);
    assert_eq!(piece.instance.draw_state.remap_row, 1);
    assert_eq!(
        piece.instance.palette_light,
        PaletteLight::new([1000; 3], 53, 1000, true)
    );
    // Original46B1B9 reads file-backed81AF00=-65; Flat46B1F2 uses
    // gradient0 and flags2800 select a Z-read/no-Z-write Convert leaf.
    assert_eq!(piece.instance.z_adjust, -65.0);
    assert_eq!(piece.instance.z_gradient, 0);
    assert_eq!(sim.state_hash(), before_draw, "body building is read-only");
    // Keep the real trajectory/target/Trailer live until ordinary impact and
    // retirement remove this shot. The elite burst may admit another Bullet.
    for _ in 0..180 {
        let commands = sim.take_due_commands();
        sim.advance_app_frame(
            &commands,
            Some(&rules),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("ordinary torpedo flight/impact frame");
        if sim.projectiles.get(id).is_none() {
            assert!(
                NativeDisplayOrder::from_display(sim.display_layers())
                    .object_draw(id, crate::render::tactical_draw_plan::SpriteEncoding::Voxel)
                    .is_none(),
                "retirement removes the tracked torpedo's draw membership"
            );
            return;
        }
    }
    panic!("the tracked torpedo must retire through ordinary flight/impact");
}

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
        assert_eq!(object.pieces[0].target, ObjectTexture::UnitPosePage(0));
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
        assert!(sim.retire_non_entity_object(id, crate::sim::world::UninitContext::default()));
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
        Some(sim.interner.intern(weapon.projectile.as_deref().unwrap())),
        weapon.bright,
    )
    .with_installed_weapon(Some(sim.interner.intern(weapon_name)));
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
