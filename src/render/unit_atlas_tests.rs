use super::*;
use crate::render::vxl_raster::VxlSprite;
use crate::rules::ini_parser::IniFile;
use crate::rules::voxel_frame_catalog::draws_turret_parts;

#[test]
fn retail_jumpjet_unit_demand_includes_native_shadow_stencils() {
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("retail assets for the loaded rules");
    let object = battle.rules.object("ZEP").expect("retail Kirov");
    assert_eq!(
        object.locomotor,
        crate::rules::locomotor_type::LocomotorKind::Jumpjet
    );
    assert!(object.considered_aircraft);
    assert!(!object.no_shadow);
    assert_eq!(object.voxel_shadow_index(), 0);
    let demand = UnitAtlasDemand {
        air: BTreeSet::from(["ZEP".to_string()]),
        ..Default::default()
    };
    let (keys, _) = needed_unit_keys(&demand, &assets, Some(&battle.rules));
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/unit_shadow.json",
    ))
    .unwrap();
    let model = UnitModel::load(&assets, "ZEP", Some(&battle.rules)).unwrap();
    for case in fixture["retail"]["cases"].as_array().unwrap() {
        let caller = &fixture["callers"][case["caller"].as_u64().unwrap() as usize];
        assert!(caller["draw_admitted"].as_bool().unwrap());
        assert!(!caller["body_mask_present"].as_bool().unwrap());
        let key = UnitSpriteKey {
            type_id: "ZEP".into(),
            turret_index: 0,
            facing: vxl_raster::voxel_facing_step_u16(
                caller["input"]["primary"].as_u64().unwrap() as u16
            ) * 8,
            layer: VxlLayer::Shadow,
            frame: 0,
            slope_type: caller["input"]["slope"].as_u64().unwrap_or(0) as u8,
            barrel_pitch: 0,
        };
        assert!(keys.contains(&key), "Unit73C5C4 requires {key:?}");
        let mut grounded = key.clone();
        grounded.frame |= GROUNDED_SHADOW_FRAME_FLAG;
        assert!(
            keys.contains(&grounded),
            "ground mask must not occupy the flight stencil"
        );
        let (_, bounds) = model
            .render(&key, None, None, &mut None)
            .expect("native Kirov shadow");
        let rect = &case["rect"];
        assert_eq!(
            bounds,
            Some([0, 1, 4, 5].map(|i| rect[i].as_i64().unwrap() as i32))
        );
    }
}

/// A Magnetron's `IsLocomotor=` Jumpjet lifts any vehicle, whose shadow
/// Unit73C1D2's suffix then draws at the slope of the cell below (base
/// Shadow_Matrix 0x0055A7D0): unmasked high above it, and in the grounded
/// entry (73C468) on the ground. A Hover victim and a Drive victim over a
/// ramp both need a resident native stencil for each.
#[test]
fn retail_lifted_vehicles_keep_native_shadow_stencils_on_their_cell_slope() {
    use crate::render::foot_depth::DepthCell;
    use crate::render::unit_shadow::{UnitShadowContext, pose};
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("retail assets for the loaded rules");
    let [x, y] = [10 * 256 + 128; 2];
    for (type_id, ramp) in [("ROBO", 0), ("HTNK", 1)] {
        let demand = UnitAtlasDemand {
            ground: BTreeSet::from([type_id.to_string()]),
            ..Default::default()
        };
        let (keys, _) = needed_unit_keys(&demand, &assets, Some(&battle.rules));
        let model = UnitModel::load(&assets, type_id, Some(&battle.rules)).unwrap();
        let floor = crate::util::lepton::ground_height_leptons(0, ramp, x, y).unwrap();
        for height in [0, 500] {
            let key = pose(
                UnitShadowContext {
                    xyz: [x, y, floor + height],
                    on_bridge: false,
                    marked: true,
                    primary_facing: 0x4000,
                },
                |coord| DepthCell {
                    coord,
                    ramp,
                    ..Default::default()
                },
            )
            .unwrap()
            .key(type_id);
            assert!(keys.contains(&key), "{key:?} resident");
            let (_, bounds) = model.render(&key, None, None, &mut None).unwrap();
            assert!(bounds.is_some(), "{key:?} native stencil");
        }
    }
}

/// Aircraft DrawIt4148A0 reaches the shared Foot shadow owner. An ordinary
/// retail Harrier therefore needs a shadow resident alongside its body.
#[test]
fn retail_fly_aircraft_demand_includes_ground_shadow() {
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("retail assets for the loaded rules");
    let object = battle.rules.object("ORCA").expect("retail Harrier");
    assert_eq!(
        object.locomotor,
        crate::rules::locomotor_type::LocomotorKind::Fly
    );
    assert!(!object.no_shadow);
    let demand = UnitAtlasDemand {
        air: BTreeSet::from(["ORCA".to_string()]),
        ..Default::default()
    };
    let (keys, _) = needed_unit_keys(&demand, &assets, Some(&battle.rules));
    assert!(
        keys.iter()
            .any(|key| key.type_id == "ORCA" && key.layer == VxlLayer::Shadow),
        "Aircraft DrawIt requires the resident ground-shadow stencil"
    );
    let model = UnitModel::load(&assets, "ORCA", Some(&battle.rules)).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/aircraft_shadow.json",
    ))
    .unwrap();
    let native = &fixture["retail"]["cases"][0];
    assert_eq!(
        voxel_image_id("ORCA", Some(&battle.rules)),
        fixture["retail"]["image"].as_str().unwrap()
    );
    let key = UnitSpriteKey {
        type_id: "ORCA".into(),
        turret_index: 0,
        facing: 0,
        layer: VxlLayer::Shadow,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let (sprite, bounds) = model.render(&key, None, None, &mut None).unwrap();
    let rect: Vec<i32> = native["rect"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    assert_eq!(bounds, Some([rect[0], rect[1], rect[4], rect[5]]));
    let mut expected = vec![0u8; 65536];
    for p in native["pixels"].as_array().unwrap() {
        expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
    }
    let cropped: Vec<u8> = (rect[3]..rect[3] + rect[5])
        .flat_map(|y| {
            let start = (y * 256 + rect[2]) as usize;
            expected[start..start + rect[4] as usize].iter().copied()
        })
        .collect();
    assert_eq!(
        sprite.palette_indices, cropped,
        "production UnitModel native ORCA stencil"
    );
    for name in ["V3ROCKET", "DMISL", "CMISL"] {
        if let Some(missile) = battle.rules.object(name) {
            eprintln!(
                "retail {name}: NoShadow={} ConsideredAircraft={} ShadowIndex={}",
                missile.no_shadow,
                missile.considered_aircraft,
                missile.voxel_shadow_index()
            );
        }
    }
}

fn gsi_13_07_variant_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "\
[VehicleTypes]
0=V3
1=HORV
2=VLAD

[V3]
NoSpawnAlt=yes
UnloadingClass=HORV
Turret=no

[HORV]
Turret=yes

[VLAD]
NoSpawnAlt=yes
Turret=no
",
    ))
    .expect("NoSpawnAlt atlas rules should parse")
}

#[test]
fn gsi_13_07_atlas_variants_share_base_unloading_and_no_spawn_alt_derivation() {
    let rules = gsi_13_07_variant_rules();
    assert_eq!(
        unit_atlas_variants("V3", Some(&rules)),
        ["V3", "HORV", "V3WO"]
    );
    // Each model splits on its own `Turret=`, not on the type that draws it.
    for (model, parts) in [("V3", false), ("HORV", true), ("V3WO", false)] {
        assert_eq!(draws_turret_parts(model, Some(&rules), 0), parts, "{model}");
    }
    assert_eq!(
        unit_atlas_variants("VLAD", Some(&rules)),
        ["VLAD", "VLADWO"],
        "the suffix is derived from the actual type id, not VLAD's stale DREDWO INI comment"
    );
}

/// `AircraftClass::Draw_It` (`0x004144B0`) draws the main voxel alone, so an
/// aircraft type's `Turret=` gives its model no gun parts; a vehicle's does.
#[test]
fn only_a_vehicle_models_turret_is_a_separate_part() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "\
[VehicleTypes]
0=TANK
1=TRUCK
[AircraftTypes]
0=JET
[TANK]
Turret=yes
[TRUCK]
[JET]
Turret=yes
",
    ))
    .expect("the gun-part rules parse");
    for (model, parts) in [
        ("TANK", true),
        ("TRUCK", false),
        ("JET", false),
        ("NOTATYPE", false),
    ] {
        assert_eq!(draws_turret_parts(model, Some(&rules), 0), parts, "{model}");
    }
    assert!(!draws_turret_parts("TANK", None, 0));
}

#[test]
fn gsi_13_07_no_spawn_alt_composite_seeds_32_facings_by_17_slopes() {
    let mut needed = HashSet::new();
    insert_unit_layer_keys(&mut needed, "DREDWO", VxlLayer::Composite, 0, 1, true, &[0]);

    assert_eq!(needed.len(), 32 * 17);
    assert!(needed.iter().all(|key| {
        key.type_id == "DREDWO"
            && key.layer == VxlLayer::Composite
            && key.frame == 0
            && key.facing % 8 == 0
            && key.slope_type <= 16
    }));
    for facing in (0..=248).step_by(8) {
        for slope_type in 0..=16 {
            assert!(needed.contains(&UnitSpriteKey {
                type_id: "DREDWO".to_string(),
                turret_index: 0,
                facing,
                layer: VxlLayer::Composite,
                frame: 0,
                slope_type,
                barrel_pitch: 0,
            }));
        }
    }
}

#[test]
fn test_composite_layers_depth_correct() {
    // Body: 2x2, all at depth 1.0, palette index 10 (opaque).
    let body = VxlSprite {
        palette_indices: vec![10, 10, 10, 10],
        depth: vec![1.0, 1.0, 1.0, 1.0],
        width: 2,
        height: 2,
        offset_x: 0.0,
        offset_y: 0.0,
    };
    // Turret: 1x1 at (1,1), depth 2.0 (closer), palette index 200 — overwrites body.
    let turret = VxlSprite {
        palette_indices: vec![200],
        depth: vec![2.0],
        width: 1,
        height: 1,
        offset_x: 1.0,
        offset_y: 1.0,
    };
    let out = composite_vxl_layers(&[body.clone(), turret]);
    assert_eq!(out.width, 2);
    assert_eq!(out.height, 2);
    let idx = (out.width + 1) as usize;
    assert_eq!(out.palette_indices[idx], 200);

    // Turret behind body (depth 0.5 < body's 1.0) — body pixel wins.
    let turret_behind = VxlSprite {
        palette_indices: vec![150],
        depth: vec![0.5],
        width: 1,
        height: 1,
        offset_x: 1.0,
        offset_y: 1.0,
    };
    let out2 = composite_vxl_layers(&[body, turret_behind]);
    let idx2 = (out2.width + 1) as usize;
    // Body pixel should remain (depth 1.0 > 0.5).
    assert_eq!(out2.palette_indices[idx2], 10);
}

#[test]
fn test_pad_layer_to_union_bounds() {
    // Body at offset (0,0), 2x2, palette index 10.
    let body = VxlSprite {
        palette_indices: vec![10, 10, 10, 10],
        depth: vec![1.0; 4],
        width: 2,
        height: 2,
        offset_x: 0.0,
        offset_y: 0.0,
    };
    // Turret at offset (5,3), 1x1, palette index 200 — different origin from body.
    let turret = VxlSprite {
        palette_indices: vec![200],
        depth: vec![2.0],
        width: 1,
        height: 1,
        offset_x: 5.0,
        offset_y: 3.0,
    };

    let all_layers: Vec<&VxlSprite> = vec![&body, &turret];

    // Pad body into union bounds.
    let padded_body = pad_layer_to_union_bounds(&body, &all_layers);
    // Pad turret into union bounds.
    let padded_turret = pad_layer_to_union_bounds(&turret, &all_layers);

    // Both should have the same dimensions and offset (shared origin).
    assert_eq!(padded_body.width, padded_turret.width);
    assert_eq!(padded_body.height, padded_turret.height);
    assert!((padded_body.offset_x - padded_turret.offset_x).abs() < 0.01);
    assert!((padded_body.offset_y - padded_turret.offset_y).abs() < 0.01);

    // Union bounds: min_x=0, min_y=0, max_x=6, max_y=4 → 6x4
    assert_eq!(padded_body.width, 6);
    assert_eq!(padded_body.height, 4);
    assert!((padded_body.offset_x - 0.0).abs() < 0.01);
    assert!((padded_body.offset_y - 0.0).abs() < 0.01);

    // Body pixel at (0,0) should be opaque (palette index 10).
    assert_eq!(padded_body.palette_indices[0], 10);

    // Turret pixel at (5,3) should be opaque (palette index 200).
    let turret_pix: usize = (3 * padded_turret.width + 5) as usize;
    assert_eq!(padded_turret.palette_indices[turret_pix], 200);
}

#[test]
fn forced_overflow_plan_keeps_every_sprite_in_bounds() {
    let dimensions = vec![(6, 4); 4];
    let plan = plan_sprite_pages(&dimensions, 8).expect("forced overflow must be pageable");

    assert!(plan.page_heights.len() >= 2);
    assert_eq!(plan.placements.len(), dimensions.len());

    assert!(plan.page_width <= 8);
    assert!(plan.page_heights.iter().all(|&height| height <= 8));

    let mut seen = vec![false; dimensions.len()];
    for (placement_index, placement) in plan.placements.iter().enumerate() {
        let (width, height) = dimensions[placement.sprite_index];
        assert!(!seen[placement.sprite_index]);
        seen[placement.sprite_index] = true;
        assert!(placement.x + width <= plan.page_width);
        assert!(placement.y + height <= plan.page_heights[placement.page]);
        for other in plan.placements.iter().skip(placement_index + 1) {
            if placement.page != other.page {
                continue;
            }
            let (other_width, other_height) = dimensions[other.sprite_index];
            let separated = placement.x + width <= other.x
                || other.x + other_width <= placement.x
                || placement.y + height <= other.y
                || other.y + other_height <= placement.y;
            assert!(separated, "page placements must not overlap");
        }

        let page_width = plan.page_width as f32;
        let page_height = plan.page_heights[placement.page] as f32;
        let uv_origin = [
            placement.x as f32 / page_width,
            placement.y as f32 / page_height,
        ];
        let uv_size = [width as f32 / page_width, height as f32 / page_height];
        assert!(uv_origin.into_iter().all(f32::is_finite));
        assert!(uv_size.into_iter().all(f32::is_finite));
        assert!(uv_origin[0] >= 0.0 && uv_origin[1] >= 0.0);
        assert!(uv_origin[0] + uv_size[0] <= 1.0);
        assert!(uv_origin[1] + uv_size[1] <= 1.0);
    }
    assert!(seen.into_iter().all(|value| value));
}

#[test]
fn incremental_repack_plan_retains_old_unloading_referent() {
    let make_cached = |type_id: &str| CachedUnitSprite {
        key: UnitSpriteKey {
            type_id: type_id.into(),
            turret_index: 0,
            facing: 0,
            layer: VxlLayer::Composite,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
        },
        pixels: vec![1; 24],
        width: 6,
        height: 4,
        offset_x: 0.0,
        offset_y: 0.0,
        native_draw_bounds: Some([-4, -4, 14, 12]),
        shadow_center: None,
    };
    // CMON is CMIN's stock UnloadingClass referent. It may be absent from the
    // current live-key collector but must remain in the rendered cache.
    let cached = vec![make_cached("CMON"), make_cached("CMIN")];
    let plan = plan_cached_sprite_pages(&cached, 8).expect("both cached sprites must repack");
    let retained = plan
        .placements
        .iter()
        .map(|placement| cached[placement.sprite_index].key.type_id.as_str())
        .collect::<HashSet<_>>();

    assert_eq!(retained, HashSet::from(["CMON", "CMIN"]));
}

#[test]
fn page_plan_has_no_u8_page_cap() {
    let dimensions = vec![(8, 8); 257];
    let plan = plan_sprite_pages(&dimensions, 8).expect("257 pages must remain addressable");

    assert_eq!(plan.page_heights.len(), 257);
    assert_eq!(
        plan.placements.iter().map(|placement| placement.page).max(),
        Some(256)
    );
    assert_eq!(plan.placements.len(), dimensions.len());
}

#[test]
fn page_plan_rejects_an_individually_oversized_sprite() {
    let err = plan_sprite_pages(&[(9, 1)], 8).expect_err("oversized sprite must not be clipped");
    assert_eq!(
        err,
        UnitAtlasPackError::SpriteExceedsTextureLimit {
            sprite_index: 0,
            width: 9,
            height: 1,
            limit: 8,
        }
    );
}

#[test]
fn unit_barrel_pitches_walk_the_unlimbo_turn() {
    // The default FireAngle 8 turns 0x4000 to 0x3800: level, then one step up.
    assert_eq!(unit_barrel_pitches(8), [0, -1]);
    assert_eq!(unit_barrel_pitches(32), [0, -1, -2, -3, -4]);
    assert_eq!(unit_barrel_pitches(0), [0]);
    // Unlimbo reads the low byte: 256 stays level, 255 and -8 turn down.
    assert_eq!(unit_barrel_pitches(256), [0]);
    assert_eq!(unit_barrel_pitches(255), [0]);
    assert_eq!(unit_barrel_pitches(-8), [0, 1]);
    // Half a turn passes straight up (step -8) and on round to 16.
    let half = unit_barrel_pitches(128);
    assert_eq!(half.len(), 17);
    assert_eq!((half[8], half[9], half[16]), (-8, 23, 16));
}

#[test]
fn barrel_image_keeps_its_first_drawn_pitch() {
    let key = |type_id: &str, facing| UnitSpriteKey {
        type_id: type_id.into(),
        turret_index: 0,
        facing,
        layer: VxlLayer::Turret,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let mut pitches = BarrelImagePitches::default();
    // The first unit drawn under a key fixes its image; later ones reuse it.
    assert_eq!(pitches.pitch(&key("MTNK", 32), None, 0), 0);
    assert_eq!(pitches.pitch(&key("MTNK", 32), None, -1), 0);
    assert_eq!(pitches.pitch(&key("MTNK", 40), None, -1), -1);
    assert_eq!(pitches.pitch(&key("MTNK", 40), None, 0), -1);
    // A TurretOffset type keys the hull's facing too; each type has its own.
    assert_eq!(pitches.pitch(&key("MTNK", 32), Some(0), -1), -1);
    assert_eq!(pitches.pitch(&key("HTNK", 32), None, -1), -1);
    let mut different_turret = key("MTNK", 32);
    different_turret.turret_index = 1;
    assert_eq!(pitches.pitch(&different_turret, None, -2), -2);
    assert_eq!(pitches.pitch(&key("MTNK", 32), None, -2), 0);
    // A new scenario starts empty.
    pitches.clear();
    assert_eq!(pitches.pitch(&key("MTNK", 32), None, -1), -1);
}

/// gamemd's voxel loader names a vehicle's models from its rules `Image=`
/// and gives a vehicle without `Turret=` no gun parts (`0x005F8277`,
/// `0x005F8844`). Retail ships the Mirage Tank's (image RTNK) and the jeep's
/// anyway, and art `[BFRT]` points `Image=` at the Prism Tank, a key gamemd
/// reads for buildings but not vehicles. The loader reruns on every INI pass,
/// and a pass that skips a part keeps an earlier pass's. No retail layer after
/// `rulesmd.ini` changes these types' `Turret=`, so the processed Hills/Battle
/// rules decide.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_vehicles_load_their_native_voxel_models() {
    use crate::rules::object_type::ObjectCategory;
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    let rules = &battle.rules;
    assert_eq!(
        rules.art().resolve_effective_image_id("BFRT", "BFRT"),
        "SREF"
    );
    assert!(assets.get_ref("BFRT.VXL").is_some());
    // Each type's gun-part files, which the art redirect sent BFRT to.
    for (type_id, image, files, turret, barrel) in [
        (
            "MGTK",
            "RTNK",
            &["RTNKTUR.VXL", "RTNKBARL.VXL"][..],
            false,
            false,
        ),
        ("JEEP", "JEEP", &["JEEPTUR.VXL"][..], false, false),
        ("BFRT", "BFRT", &["SREFTUR.VXL"][..], false, false),
        (
            "HTNK",
            "HTNK",
            &["HTNKTUR.VXL", "HTNKBARL.VXL"][..],
            true,
            true,
        ),
        ("HARV", "HARV", &["HARVTUR.VXL"][..], true, false),
    ] {
        let object = rules.object(type_id).expect(type_id);
        assert_eq!(object.category, ObjectCategory::Vehicle, "{type_id}");
        assert_eq!(object.has_turret, turret, "{type_id}");
        assert_eq!(voxel_image_id(type_id, Some(rules)), image, "{type_id}");
        for file in files {
            assert!(assets.get_ref(file).is_some(), "{file}");
        }
        let model = UnitModel::load(&assets, type_id, Some(rules)).expect(type_id);
        assert_eq!(model.gun_parts(0).0.is_some(), turret, "{type_id}");
        assert_eq!(model.gun_parts(0).1.is_some(), barrel, "{type_id}");
    }
}

/// The eight retail building guns exercise B8+C0, B8-only, and C0-only
/// loader arms. The Grand Cannon's long gun is GTGCANBARL, while SAM itself
/// occupies C0 and must not be synthesized as a vehicle hull or B8 turret.
#[test]
fn retail_building_voxel_turrets_load_their_native_barrels() {
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    for (building, turret, barrel) in [
        ("GTGCAN", Some("GTGCANTUR"), Some("GTGCANBARL")),
        ("NAFLAK", Some("FLAKTUR"), None),
        ("YAREFN", Some("SMINTUR"), None),
        ("NASAM", None, Some("SAM")),
        ("NALASR", None, Some("LASER")),
        ("YAGGUN", None, Some("YAGGUN")),
        ("CAOUTP", None, Some("OUTP")),
        ("CAEAST02", None, Some("CAHEAD")),
    ] {
        let object = battle.rules.object(building).expect(building);
        assert!(object.turret_anim_is_voxel, "{building}");
        for image in turret.into_iter().chain(barrel) {
            for extension in ["VXL", "HVA"] {
                let file = format!("{image}.{extension}");
                assert!(assets.get_ref(&file).is_some(), "{file}");
            }
        }
        let model = UnitModel::load(&assets, building, Some(&battle.rules))
            .unwrap_or_else(|| panic!("{building} should load its TurretAnim model"));
        assert!(model.body.is_none(), "{building} has an SHP hull");
        assert_eq!(
            model.gun_parts(0).0.is_some(),
            turret.is_some(),
            "{building}"
        );
        assert_eq!(
            model.gun_parts(0).1.is_some(),
            barrel.is_some(),
            "{building}"
        );
        let layers = seed_layers_for(&assets, building, Some(&battle.rules));
        assert_eq!(layers.contains(&(VxlLayer::Turret, 0)), turret.is_some());
        assert_eq!(layers.contains(&(VxlLayer::Barrel, 0)), barrel.is_some());
        assert_eq!(
            layers.len(),
            usize::from(turret.is_some()) + usize::from(barrel.is_some())
        );
    }
}

#[test]
fn building_voxel_filenames_match_original_loader_execution() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/building_barrel.json",
    ))
    .unwrap();
    for case in native["loader_cases"].as_array().unwrap() {
        if case["prior"] == true || case["barrel_anim_is_voxel"] == true {
            continue;
        }
        let name = case["name"].as_str().unwrap();
        let enabled = case["turret_anim_is_voxel"].as_bool().unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0=BUILDING\n[BUILDING]\nTurretAnim={name}\nTurretAnimIsVoxel={enabled}\n"
        ))).unwrap();
        let mut requested = Vec::new();
        if let Some((turret, barrel)) = building_voxel_names("BUILDING", Some(&rules)) {
            requested.extend(
                turret
                    .into_iter()
                    .chain([barrel])
                    .map(|base| format!("{base}.VXL")),
            );
        }
        let expected: Vec<_> = case["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["event"] == "available")
            .map(|event| event["name"].as_str().unwrap().to_uppercase())
            .collect();
        assert_eq!(requested, expected, "{case}");
    }
}

#[test]
fn retail_building_gun_atlas_covers_frames_and_elevation_without_voxel_hulls() {
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    let buildings: BTreeSet<String> = battle
        .rules
        .building_ids
        .iter()
        .filter(|id| building_voxel_names(id, Some(&battle.rules)).is_some())
        .cloned()
        .collect();
    assert_eq!(buildings.len(), 8);
    let demand = UnitAtlasDemand {
        turrets: buildings,
        ..Default::default()
    };
    let (keys, frames) = needed_unit_keys(&demand, &assets, Some(&battle.rules));
    assert!(keys.iter().all(|key| key.slope_type == 0
        && key.turret_index == 0
        && matches!(key.layer, VxlLayer::Turret | VxlLayer::Barrel)));
    let cannon_barrels: Vec<_> = keys
        .iter()
        .filter(|key| key.type_id == "GTGCAN" && key.layer == VxlLayer::Barrel)
        .collect();
    assert_eq!(cannon_barrels.len(), 64);
    assert!(cannon_barrels.iter().all(|key| key.frame == 0));
    assert_eq!(
        cannon_barrels
            .iter()
            .map(|key| key.barrel_pitch)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([-1, 0])
    );
    let gattling_frames = frames[&("YAGGUN".into(), VxlLayer::Barrel, 0)];
    assert!(
        gattling_frames > 1,
        "retail Gattling Cannon has animated C0"
    );
    assert_eq!(
        keys.iter()
            .filter(|key| key.type_id == "YAGGUN")
            .map(|key| key.frame)
            .collect::<BTreeSet<_>>(),
        (0..gattling_frames).collect()
    );

    let model = UnitModel::load(&assets, "GTGCAN", Some(&battle.rules)).unwrap();
    let vpl = VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap()).unwrap();
    let mut pose = None;
    let key = UnitSpriteKey {
        type_id: "GTGCAN".into(),
        turret_index: 0,
        facing: 64,
        layer: VxlLayer::Barrel,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let (level, _) = model.render(&key, Some(&vpl), None, &mut pose).unwrap();
    let (raised, _) = model
        .render(
            &UnitSpriteKey {
                barrel_pitch: -1,
                ..key
            },
            Some(&vpl),
            None,
            &mut pose,
        )
        .unwrap();
    assert_ne!(
        (level.width, level.height, level.palette_indices),
        (raised.width, raised.height, raised.palette_indices),
        "the barrel really pitches"
    );
}

/// The IFV's four retail gun assemblies must all be resident before its
/// passenger changes. Its hull and shadow are shared across those assemblies.
#[test]
fn retail_ifv_atlas_covers_all_turrets_without_replicating_its_hull() {
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    let object = battle.rules.object("FV").expect("retail IFV");
    assert_eq!(object.turret_count, 4);
    assert!(!object.is_gattling);
    let demand = UnitAtlasDemand {
        ground: BTreeSet::from(["FV".to_string()]),
        ..UnitAtlasDemand::default()
    };
    let (keys, _) = needed_unit_keys(&demand, &assets, Some(&battle.rules));
    let count = |layer| keys.iter().filter(|key| key.layer == layer).count();
    assert_eq!(count(VxlLayer::Body), 32 * 17);
    // Flight and grounded stencils for the Magnetron's lift.
    assert_eq!(count(VxlLayer::Shadow), 2 * 32 * 17);
    assert_eq!(count(VxlLayer::Turret), 4 * 32 * 17);
    assert_eq!(
        keys.iter()
            .filter(|key| key.layer == VxlLayer::Turret)
            .map(|key| key.turret_index)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 1, 2, 3])
    );
    assert!(
        keys.iter()
            .filter(|key| matches!(key.layer, VxlLayer::Body | VxlLayer::Shadow))
            .all(|key| key.turret_index == 0)
    );
}

#[test]
fn indexed_turret_names_and_draw_admission_match_native_controls() {
    use crate::rules::voxel_frame_catalog::voxel_turret_index;
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/ifv_turret_switching.json",
    ))
    .expect("native IFV corpus");
    for row in native["draw"]["filenames"].as_array().unwrap() {
        let layer = match row["part"].as_str().unwrap() {
            "turret" => VxlLayer::Turret,
            "barrel" => VxlLayer::Barrel,
            other => panic!("unknown gun part {other}"),
        };
        assert_eq!(
            voxel_gun_basename("FV", layer, row["index"].as_i64().unwrap() as i32),
            row["native_basename"].as_str().unwrap(),
            "{row}"
        );
    }
    for row in native["draw"]["rows"].as_array().unwrap() {
        let count = row["turret_count"].as_i64().unwrap();
        let gattling = row["is_gattling"].as_bool().unwrap();
        let turret = row["turret"].as_bool().unwrap();
        let current = row["selected_index"].as_i64().unwrap() as i32;
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=FV\n[FV]\nTurretCount={count}\nIsGattling={gattling}\nTurret={turret}\n"
        )))
        .unwrap();
        assert_eq!(
            draws_turret_parts("FV", Some(&rules), current),
            row["gun_branch"].as_bool().unwrap(),
            "{row}"
        );
        if !row["result"].is_null() {
            assert_eq!(
                voxel_turret_index("FV", Some(&rules), current),
                row["result"]["selected"].as_i64().unwrap() as i32,
                "{row}"
            );
        }
    }
}

/// Byte identity is retail evidence; distinct raster outputs and unchanged
/// hull/shadow pixels are Rust regression checks, not native raster parity.
#[test]
fn retail_ifv_models_draw_four_distinct_guns_and_share_body_and_shadow() {
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    use crate::util::sha256::sha256_hex;
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/ifv_turret_switching.json",
    ))
    .unwrap();
    for row in native["physical"]["assets"].as_array().unwrap() {
        if row["present_in_extract"] == true {
            let file = row["file"].as_str().unwrap();
            let bytes = assets
                .get_ref(file)
                .unwrap_or_else(|| panic!("retail {file}"));
            assert_eq!(sha256_hex(bytes), row["sha256"].as_str().unwrap(), "{file}");
        }
    }
    let model = UnitModel::load(&assets, "FV", Some(&battle.rules)).expect("retail IFV");
    assert_eq!(model.guns.len(), 4);
    let vpl = VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap()).unwrap();
    let mut pose = None;
    let mut key = UnitSpriteKey {
        type_id: "FV".to_string(),
        turret_index: 0,
        facing: 64,
        layer: VxlLayer::Body,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let (body, body_bounds) = model.render(&key, Some(&vpl), None, &mut pose).unwrap();
    let mut guns = BTreeSet::new();
    for index in 0..4 {
        key.layer = VxlLayer::Turret;
        key.turret_index = index;
        let (sprite, bounds) = model.render(&key, Some(&vpl), None, &mut pose).unwrap();
        assert!(bounds.is_some(), "FV turret {index}");
        guns.insert((sprite.width, sprite.height, sprite.palette_indices));
    }
    assert_eq!(guns.len(), 4, "all four retail IFV turrets must differ");
    for index in [-1, 4, i32::MAX] {
        key.turret_index = index;
        assert!(model.render(&key, Some(&vpl), None, &mut pose).is_none());
    }
    key.turret_index = 0;
    key.layer = VxlLayer::Body;
    let (body_after, bounds_after) = model.render(&key, Some(&vpl), None, &mut pose).unwrap();
    assert_eq!(body.palette_indices, body_after.palette_indices);
    assert_eq!(body_bounds, bounds_after);
    key.layer = VxlLayer::Shadow;
    let (shadow, shadow_bounds) = model.render(&key, Some(&vpl), None, &mut pose).unwrap();
    for index in 1..4 {
        // Production keys canonicalize Shadow to zero. The raster itself
        // also remains independent of the selected turret's geometry.
        key.turret_index = index;
        let (other, bounds) = model.render(&key, Some(&vpl), None, &mut pose).unwrap();
        assert_eq!(shadow.palette_indices, other.palette_indices);
        assert_eq!(shadow_bounds, bounds);
    }
}

#[test]
fn indexed_model_loader_requires_turret_pairs_but_allows_absent_barrels() {
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::retail_ini_fixture::retail_assets;
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    for (name, turret, gattling, missing, bad_barrel, expected_guns) in [
        ("valid", true, false, None, false, Some(2)),
        ("no-turret-flag", false, false, None, true, Some(2)),
        (
            "missing-turret",
            true,
            false,
            Some("FVTUR1.VXL"),
            false,
            None,
        ),
        ("missing-hva", true, false, Some("FVTUR1.HVA"), false, None),
        ("bad-barrel", true, false, None, true, None),
        ("gattling", true, true, Some("FVTUR1.VXL"), false, Some(1)),
    ] {
        let directory = TestDirectory::new(&format!("indexed-voxel-{name}"));
        for file in [
            "FV.VXL",
            "FV.HVA",
            "FVTUR.VXL",
            "FVTUR.HVA",
            "FVTUR1.VXL",
            "FVTUR1.HVA",
        ] {
            if Some(file) != missing {
                std::fs::write(directory.path().join(file), assets.get_ref(file).unwrap()).unwrap();
            }
        }
        if bad_barrel {
            // A valid voxel with no companion HVA: the required-pair failure
            // is independent of which retail geometry supplies the VXL bytes.
            std::fs::write(
                directory.path().join("FVBARL1.VXL"),
                assets.get_ref("FVTUR1.VXL").unwrap(),
            )
            .unwrap();
        }
        let fixture = AssetManager::from_loose_root_for_test(directory.path());
        let art = IniFile::from_str("[FV]\nVoxel=yes\n");
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=FV\n[FV]\nTurretCount=2\nTurret={turret}\nIsGattling={gattling}\n"
        )), &art)
        .unwrap();
        rules.install_art_fixture(crate::rules::art_data::ArtRegistry::from_ini(&art));
        assert_eq!(
            UnitModel::load(&fixture, "FV", Some(&rules)).map(|model| model.guns.len()),
            expected_guns,
            "{name}"
        );
    }
}

/// The shared direct-matrix owner keeps Bullet's successful VXL/HVA pair
/// requirement separate from debris's explicit no-HVA drawing policy.
#[test]
fn kirov_direct_matrix_requires_hva_and_wraps_its_native_frame() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let model = UnitModel::load(&assets, "ZBOMB", None).expect("retail bomb model");
    let vpl = crate::assets::vpl_file::VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap())
        .unwrap();
    let matrix = crate::render::vxl_raster::projectile_voxel_matrix(
        crate::sim::projectile::ProjectileVelocity::new(0, 0, -1),
    )
    .unwrap();
    let lighting = crate::render::vxl_raster::VxlLighting::Diffuse;
    let (first, bounds) = model
        .render_matrix(matrix, 0, true, lighting, Some(&vpl))
        .unwrap();
    assert!(bounds.is_some());
    let frames = model.body.as_ref().unwrap().hva().unwrap().frame_count;
    let (wrapped, wrapped_bounds) = model
        .render_matrix(matrix, frames, true, lighting, Some(&vpl))
        .unwrap();
    assert_eq!(first.palette_indices, wrapped.palette_indices);
    assert_eq!(bounds, wrapped_bounds);
    // Explicit raw-image input without an HVA, through the same loader.
    // The typed Techno loader requires the complete pair; this is the
    // existing Bullet/debris component boundary, not a malformed typed body.
    let directory = crate::map::source::test_support::TestDirectory::new("matrix-no-hva");
    std::fs::write(
        directory.path().join("ZBOMB.VXL"),
        assets.get_ref("ZBOMB.VXL").unwrap(),
    )
    .unwrap();
    let no_hva = AssetManager::from_loose_root_for_test(directory.path());
    let model = UnitModel::load(&no_hva, "ZBOMB", None).unwrap();
    assert!(
        model
            .render_matrix(matrix, 0, true, lighting, Some(&vpl))
            .is_none(),
        "Bullet LoadVoxelAssets requires both main files"
    );
    assert!(
        model
            .render_matrix(matrix, 0, false, lighting, Some(&vpl))
            .is_some(),
        "the explicit debris no-HVA caller still uses the same raster owner"
    );
}
