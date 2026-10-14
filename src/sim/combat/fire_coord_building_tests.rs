//! Building aim and muzzle coordinates against original executable outputs.
//!
//! `tools/spatial_oracle/building_fire_facing.json` runs vt+0x4E8
//! (`0x0043ED40`), vt+0x308 (`0x0044D7D0`) and vt+0x2A8 (`0x00445E50`) over a
//! building with the row's Location, type flags, pixel offsets, TarCom and
//! `+0x388` facing, and records its GetCoords (`0x00447AC0`).

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::movement::FacingClass;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

fn rules(input: &Value) -> RuleSet {
    let flag = |key: &str| {
        if input[key].as_bool().unwrap_or(false) {
            "yes"
        } else {
            "no"
        }
    };
    let anim = input["turret_anim"].as_array().map_or((0, 0), |pair| {
        (pair[0].as_i64().unwrap(), pair[1].as_i64().unwrap())
    });
    let target_foundation = input["target"]["foundation"].as_str().unwrap_or("1x1");
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nGuardAreaTargetingDelay=36\n\
         [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=BLD\n1=TGT\n\
         [BLD]\nStrength=500\nFoundation={}\nTurret={}\nTurretAnimIsVoxel={}\n\
         TurretAnimX={}\nTurretAnimY={}\n\
         [TGT]\nStrength=500\nFoundation={target_foundation}\n",
        input["foundation"].as_str().unwrap(),
        flag("turret"),
        flag("voxel"),
        anim.0,
        anim.1,
    )))
    .expect("rules");
    let art = input["pixel_offset"]
        .as_array()
        .map_or(String::new(), |pair| {
            format!(
                "[BLD]\nPrimaryFirePixelOffset={},{}\n",
                pair[0].as_i64().unwrap(),
                pair[1].as_i64().unwrap()
            )
        });
    rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(&art)));
    rules
}

/// An entity at the row's Location (leptons).
fn place(sim: &mut Simulation, id: u64, kind: &str, category: EntityCategory, at: &Value) {
    let (x, y) = (at[0].as_i64().unwrap(), at[1].as_i64().unwrap());
    let mut entity =
        GameEntity::test_default(id, kind, "Americans", (x / 256) as u16, (y / 256) as u16);
    entity.set_type_ref_for_test(sim.intern(kind));
    entity.set_owner_for_test(sim.intern("Americans"));
    entity.set_category_for_test(category);
    entity.position.sub_x = SimFixed::from_num(x % 256);
    entity.position.sub_y = SimFixed::from_num(y % 256);
    sim.substrate.entities.insert(entity);
}

#[test]
fn building_aim_directions_match_the_original() {
    let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_fire_facing.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 52);
    for row in &rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = rules(input);
        let mut sim = Simulation::new();
        place(
            &mut sim,
            1,
            "BLD",
            EntityCategory::Structure,
            &input["location"],
        );
        // Construction stamps each building type's `Foundation=`.
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .type_facts
            .set_foundation_for_test(input["foundation"].as_str().unwrap().to_string());
        let target = &input["target"];
        if !target.is_null() {
            let (kind, category) = match target["kind"].as_str().unwrap() {
                "unit" => ("TANK", EntityCategory::Unit),
                _ => ("TGT", EntityCategory::Structure),
            };
            place(&mut sim, 2, kind, category, &target["location"]);
            if category == EntityCategory::Structure {
                sim.substrate
                    .entities
                    .get_mut(2)
                    .unwrap()
                    .type_facts
                    .set_foundation_for_test(
                        target["foundation"].as_str().unwrap_or("1x1").to_string(),
                    );
            }
        }
        let frame = sim.session.binary_frame;
        let facing = input["facing"].as_u64().unwrap() as u16;
        let building = sim.substrate.entities.get_mut(1).unwrap();
        building.body_facing = FacingClass::new(facing, 10);
        building
            .attack_target
            .set_for_test((!target.is_null()).then(|| TargetKind::Entity(2)));

        let building = sim.substrate.entities.get(1).unwrap();
        let obj = sim.object_type(building.type_ref(), &rules).unwrap();
        let current = building.body_facing.current(frame);
        let (flh, fire) = building_fire_facings(
            &sim,
            &FireSource::of_entity(building),
            obj,
            firer_art(&rules, obj),
            current,
        );
        let output = &row["output"];
        let coords = crate::sim::combat::resolve_target_coords(
            &TargetKind::Entity(1),
            &sim.substrate.entities,
        )
        .map(coords_xy);
        let native = &output["get_coords"];
        assert_eq!(
            coords,
            Some([0, 1].map(|axis| native[axis].as_i64().unwrap() as i32)),
            "{name} GetCoords"
        );
        let word = |key: &str| output[key].as_u64().map(|value| value as u16);
        assert_eq!(Some(fire), word("fire_facing"), "{name} vt+0x308");
        assert_eq!(Some(flh), word("turret_facing"), "{name} vt+0x2A8");
        let direction = building
            .attack_target
            .target()
            .and_then(|attack| building_direction_to(&sim, &rules, building, attack));
        assert_eq!(direction, word("direction_to"), "{name} vt+0x4E8");
    }
}

fn prism_native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_prism.json",
    ))
    .unwrap()
}

fn native_coord(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

#[test]
fn building_primary_pixel_and_selected_flh_match_original_slot_and_burst_controls() {
    let native = prism_native();
    let rows = native["laser_flh"].as_array().unwrap();
    assert_eq!(rows.len(), 16);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=BLD\n[BLD]\nStrength=500\nTurret=no\n\
             Primary=GUN\nSecondary=SUPPORT\n[GUN]\nDamage=1\n[SUPPORT]\nDamage=1\n",
        ))
        .unwrap();
        let triplet = |key: &str, fallback: [i32; 3]| -> [i32; 3] {
            input[key].as_array().map_or(fallback, |v| {
                std::array::from_fn(|axis| v[axis].as_i64().unwrap() as i32)
            })
        };
        let [forward, lateral, height] = triplet("primary_flh", [0, 0, 378]);
        let [sf, sl, sh] = triplet("secondary_flh", [0; 3]);
        let (px, py) = input["pixel_offset"].as_array().map_or((0, -4), |v| {
            (v[0].as_i64().unwrap(), v[1].as_i64().unwrap())
        });
        let dual = input["primary_dual"].as_bool().unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(&format!(
            "[BLD]\nFoundation=1x1\nPrimaryFirePixelOffset={px},{py}\n\
             PrimaryFireDualOffset={dual}\nPrimaryFireFLH={forward},{lateral},{height}\n\
             SecondaryFireFLH={sf},{sl},{sh}\n",
        ))));
        let mut sim = Simulation::new();
        place(
            &mut sim,
            1,
            "BLD",
            EntityCategory::Structure,
            &serde_json::json!([3200, 3200, 0]),
        );
        let building = sim.substrate.entities.get_mut(1).unwrap();
        building.body_facing = FacingClass::new(0x2000, 0);
        let source = FireSource::of_entity(building);
        let [forward, lateral, height] = triplet("base", [0; 3]);
        let actual = fire_coordinate(
            &sim,
            &rules,
            &source,
            rules.object("BLD").unwrap(),
            input["slot"].as_i64().unwrap() as i32,
            input["burst"].as_u64().unwrap() as u8,
            Flh {
                forward,
                lateral,
                height,
            },
        );
        assert_eq!(actual.coord, native_coord(&row["source"]), "{name}");
    }
}

fn voxel_native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_voxel_flh.json",
    ))
    .unwrap()
}

fn voxel_row<'a>(native: &'a Value, name: &str) -> &'a Value {
    native["voxel_flh"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == name)
        .unwrap_or_else(|| panic!("missing native voxel_flh row {name}"))
}

fn triple(value: &Value) -> [i32; 3] {
    std::array::from_fn(|axis| value[axis].as_i64().unwrap() as i32)
}

fn pair(value: &Value) -> [i32; 2] {
    std::array::from_fn(|axis| value[axis].as_i64().unwrap() as i32)
}

/// The reader inputs one `building_voxel_flh.json` row writes: `[BLD]`'s type
/// keys and the `[BLD]` ART section the merged FLH and pixel offset come from.
/// The row's `barrel_anim_is_voxel` byte has no reader (`+0x16C6` is the arm
/// VERA does not port) and every row leaves it clear. `primary` supplies the
/// laser weapon its consumer re-reads; these fixtures retain the same ART
/// coordinate inputs with `TurretCount=` at its default of 0.
fn voxel_rules_with(input: &Value, primary: Option<&str>) -> RuleSet {
    let flag = |key: &str| {
        if input[key].as_i64().unwrap() != 0 {
            "yes"
        } else {
            "no"
        }
    };
    let pair = |key: &str| {
        let value = input[key].as_array().unwrap();
        format!("{},{}", value[0], value[1])
    };
    let triple_text = |key: &str| {
        let value = input[key].as_array().unwrap();
        format!("{},{},{}", value[0], value[1], value[2])
    };
    let (primary_key, primary_section) = match primary {
        Some(name) => (
            format!("Primary={name}\n"),
            format!("[{name}]\nIsLaser=yes\nIsHouseColor=yes\nLaserDuration=15\nDamage=1\n"),
        ),
        None => (String::new(), String::new()),
    };
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[BuildingTypes]\n0=BLD\n\
         [BLD]\nStrength=500\nTurret=yes\nTurretAnimIsVoxel={}\n\
         TurretAnimX={}\nTurretAnimY={}\n{primary_key}{primary_section}",
        flag("turret_anim_is_voxel"),
        input["turret_anim_x"].as_i64().unwrap(),
        input["turret_anim_y"].as_i64().unwrap(),
    )))
    .expect("rules");
    rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(&format!(
        "[BLD]\nPrimaryFirePixelOffset={}\nPrimaryFireDualOffset={}\n\
         PrimaryFireFLH={}\nSecondaryFireFLH={}\n",
        pair("primary_pixel_offset"),
        flag("primary_dual"),
        triple_text("primary_flh"),
        triple_text("secondary_flh"),
    ))));
    rules
}

fn voxel_rules(input: &Value) -> RuleSet {
    voxel_rules_with(input, None)
}

/// `tools/spatial_oracle/building_voxel_flh.json` executes the original
/// `BuildingClass::GetFLH @ 0x00453840` on a building whose type carries the
/// row's `+0x16C5`/`+0x11E0`/`+0x11E4`, `+0xE44`/`+0x1764` and FLH values,
/// with its `+0x388` facing snapped to the row's heading. Every row states
/// those inputs explicitly and records the addresses the arm reached, so the
/// `voxel_disabled` control shows the original converting pixels in one row
/// and the base GetFLH arm in the other.
#[test]
fn building_voxel_turret_shot_origin_matches_the_original_flh() {
    let native = voxel_native();
    let rows = native["voxel_flh"].as_array().unwrap();
    assert_eq!(rows.len(), 13);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = voxel_rules(input);
        let mut sim = Simulation::new();
        place(
            &mut sim,
            1,
            "BLD",
            EntityCategory::Structure,
            &input["location"],
        );
        let heading = input["heading"].as_u64().unwrap() as u16;
        sim.substrate.entities.get_mut(1).unwrap().body_facing = FacingClass::new(heading, 0);
        let building = sim.substrate.entities.get(1).unwrap();
        let obj = sim.object_type(building.type_ref(), &rules).unwrap();
        let shot = fire_coordinate(
            &sim,
            &rules,
            &FireSource::of_entity(building),
            obj,
            input["weapon_index"].as_i64().unwrap() as i32,
            input["burst"].as_u64().unwrap() as u8,
            Flh::from(triple(&input["base"])),
        );
        assert_eq!(shot.coord, native_coord(&row["source"]), "{name}");
        // `offset_y` is the muzzle animation's `ZAdjust` input: the origin's Y
        // minus the object coordinate (vt+0xAC) the offset was added to.
        assert_eq!(
            shot.offset_y,
            row["source"][1].as_i64().unwrap() as i32
                - row["object_coordinate"][1].as_i64().unwrap() as i32,
            "{name} offset_y"
        );
        let getters: Vec<&str> = row["getters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|getter| getter.as_str().unwrap())
            .collect();
        assert!(getters.contains(&"0x453840"), "{name} entered GetFLH");
        let pixel_arm = pair(&input["primary_pixel_offset"]) != [0xFFFF, 0xFFFF];
        assert_eq!(
            getters.contains(&"0x6d2070"),
            pixel_arm || input["turret_anim_is_voxel"].as_i64().unwrap() != 0,
            "{name} conversion"
        );
        assert!(row["rng_unchanged"].as_bool().unwrap(), "{name} RNG");
    }
    // The arm's own displacement, as the two rows that differ only in
    // `+0x16C5` executed it, and the no-op controls beside it.
    let with = triple(&voxel_row(&native, "gtgcan_burst0")["source"]);
    let without = triple(&voxel_row(&native, "voxel_disabled")["source"]);
    assert_eq!((with[0] - without[0], with[1] - without[1]), (251, 226));
    assert_eq!(
        triple(&voxel_row(&native, "zero_pixels")["source"]),
        without
    );
    assert_eq!(triple(&voxel_row(&native, "gtgcan_burst1")["source"]), with);
}

/// The beam consumer of the same owner: `laser::fired` (FireAt's IsLaser arm
/// `0x006FF4CC` through `SpawnLaser 0x006FD210`) emits the laser's source as
/// [`fire_coordinate`]'s coordinate, and a stock voxel-turret type can fire a
/// beam (`[CAEAST02]` is `TurretAnimIsVoxel=true`, `TurretAnimY=-45` and
/// `Primary=PrismShot`, an `IsLaser=` weapon). Synthetic laser fixtures
/// use GTGCAN and YAGGUN's primary inputs and GTGCAN's secondary-slot control
/// to compare emitted beam origins with the original GetFLH outputs.
#[test]
fn a_voxel_turrets_laser_birth_uses_the_original_get_flh_coordinate() {
    let native = voxel_native();
    for (name, weapon_index) in [
        ("gtgcan_burst0", 0),
        ("yaggun_burst0", 0),
        ("gtgcan_weapon1", 1),
    ] {
        let row = voxel_row(&native, name);
        let input = &row["input"];
        let rules = voxel_rules_with(input, Some("BLDLaser"));
        let mut sim = Simulation::new();
        place(
            &mut sim,
            1,
            "BLD",
            EntityCategory::Structure,
            &input["location"],
        );
        place(
            &mut sim,
            2,
            "TGT",
            EntityCategory::Structure,
            &serde_json::json!([4096, 3200, 0]),
        );
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .type_facts
            .set_foundation_for_test("1x1".to_string());
        let heading = input["heading"].as_u64().unwrap() as u16;
        sim.substrate.entities.get_mut(1).unwrap().body_facing = FacingClass::new(heading, 0);
        sim.lifecycle_outputs.clear();
        crate::sim::combat::laser::fired(
            &mut sim,
            &rules,
            1,
            TargetKind::Entity(2),
            weapon_index,
            rules.weapon("BLDLaser").unwrap(),
            0,
        );
        let birth = sim
            .lifecycle_outputs
            .iter()
            .find_map(|output| match output {
                crate::sim::world::LifecycleOutput::LaserCreated(birth) => Some(*birth),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} emitted a beam"));
        assert_eq!(
            birth.from,
            native_coord(&row["source"]),
            "{name} beam source"
        );
    }
}

/// The occupied arm returns its muzzle port before both the primary pixel
/// and voxel terms. The primary-pixel golden executes the same native
/// conversion and GetCoords sum (453886/4538AB..4538AF) for (6,-4).
#[test]
fn an_occupied_voxel_building_keeps_its_muzzle_port_precedence() {
    let native = voxel_native();
    let input = &voxel_row(&native, "primary_pixel_dual_precedence")["input"];
    let mut rules = voxel_rules(input);
    let art = ArtRegistry::from_ini(&IniFile::from_str(
        "[BLD]\nPrimaryFireFLH=250,0,230\nPrimaryFirePixelOffset=20,30\n\
         PrimaryFireDualOffset=yes\nMuzzleFlash0=6,-4\n",
    ));
    rules.replace_art_registry_for_test(art);
    let mut sim = Simulation::new();
    place(
        &mut sim,
        1,
        "BLD",
        EntityCategory::Structure,
        &input["location"],
    );
    let building = sim.substrate.entities.get(1).unwrap();
    let obj = sim.object_type(building.type_ref(), &rules).unwrap();
    let mut source = FireSource::of_entity(building);
    source.garrison_fire_index = Some(0);
    let shot = fire_coordinate(&sim, &rules, &source, obj, 0, 0, Flh::default());
    assert_eq!(
        shot.coord,
        native_coord(&voxel_row(&native, "primary_pixel_precedence")["source"])
    );
    assert_eq!(
        shot.source_z, shot.coord.z,
        "the occupied port keeps building Z"
    );

    // An authored building may resolve no ART entry. Preserve its existing
    // occupied-shot fallback to the building coordinate; missing ART must
    // not expose the later voxel-turret displacement.
    rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str("")));
    let obj = sim.object_type(building.type_ref(), &rules).unwrap();
    let shot = fire_coordinate(&sim, &rules, &source, obj, 0, 0, Flh::default());
    assert_eq!(
        shot.coord,
        native_coord(&voxel_row(&native, "primary_pixel_precedence")["object_coordinate"]),
        "an occupied building without ART keeps its base coordinate"
    );
    assert_eq!(shot.offset_y, 0);
}

/// The corpus' stock rows are the values retail authors for the two types the
/// issue names, read through the production layers: a later layer could
/// override them, so the fixture's recorded inputs are compared with the
/// reader instead of trusted.
#[test]
fn retail_voxel_turret_inputs_match_the_native_rows() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let native = voxel_native();
    for (type_id, row_name) in [("GTGCAN", "gtgcan_burst0"), ("YAGGUN", "yaggun_burst0")] {
        let input = &voxel_row(&native, row_name)["input"];
        let obj = rules.object(type_id).unwrap();
        assert!(obj.has_turret, "{type_id}");
        assert!(obj.turret_anim_is_voxel, "{type_id}");
        assert_eq!(
            obj.turret_anim_x,
            input["turret_anim_x"].as_i64().unwrap() as i32
        );
        assert_eq!(
            obj.turret_anim_y,
            input["turret_anim_y"].as_i64().unwrap() as i32
        );
        let art_entry = firer_art(&rules, obj);
        assert_eq!(
            primary_fire_pixel_offset(art_entry),
            None,
            "{type_id} takes no alternative pixel arm"
        );
        let flh =
            art_entry
                .expect(type_id)
                .weapon_flh(obj.turret_count, obj.weapon_count, 0, false);
        assert_eq!(
            [flh.forward, flh.lateral, flh.height],
            triple(&input["primary_flh"]),
            "{type_id} merged FLH"
        );
    }
}

#[test]
fn retail_prism_main_and_support_muzzles_match_original_creation_inputs() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = prism_native();
    let rows = native["laser_birth"].as_array().unwrap();
    for name in ["production_main", "production_support"] {
        let row = rows
            .iter()
            .find(|row| row["input"]["name"] == name)
            .unwrap();
        let mut sim = Simulation::new();
        place(
            &mut sim,
            1,
            "ATESLA",
            EntityCategory::Structure,
            &row["input"]["location"],
        );
        let source = FireSource::of_entity(sim.substrate.entities.get(1).unwrap());
        // The support constructor uses its tower's primary FLH as well.
        let actual = fire_coordinate(
            &sim,
            &retail.rules,
            &source,
            retail.rules.object("ATESLA").unwrap(),
            0,
            0,
            Flh::default(),
        );
        assert_eq!(
            actual.coord,
            native_coord(&row["laser"]["source"]),
            "{name}"
        );
    }
}
