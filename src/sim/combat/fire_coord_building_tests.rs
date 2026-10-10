//! A building's aim directions against the original:
//! `tools/spatial_oracle/building_fire_facing.json` runs vt+0x4E8
//! (`0x0043ED40`), vt+0x308 (`0x0044D7D0`) and vt+0x2A8 (`0x00445E50`) over a
//! building with the row's Location, type flags, pixel offsets, TarCom and
//! `+0x388` facing, and records its GetCoords (`0x00447AC0`).

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::AttackTarget;
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
    entity.type_ref = sim.intern(kind);
    entity.owner = sim.intern("Americans");
    entity.category = category;
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
        sim.substrate.entities.get_mut(1).unwrap().foundation =
            input["foundation"].as_str().unwrap().to_string();
        let target = &input["target"];
        if !target.is_null() {
            let (kind, category) = match target["kind"].as_str().unwrap() {
                "unit" => ("TANK", EntityCategory::Unit),
                _ => ("TGT", EntityCategory::Structure),
            };
            place(&mut sim, 2, kind, category, &target["location"]);
            if category == EntityCategory::Structure {
                sim.substrate.entities.get_mut(2).unwrap().foundation =
                    target["foundation"].as_str().unwrap_or("1x1").to_string();
            }
        }
        let frame = sim.session.binary_frame;
        let facing = input["facing"].as_u64().unwrap() as u16;
        let building = sim.substrate.entities.get_mut(1).unwrap();
        building.body_facing = FacingClass::new(facing, 10);
        building.attack_target = (!target.is_null()).then(|| AttackTarget::new(2));

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
            .as_ref()
            .and_then(|attack| building_direction_to(&sim, &rules, building, attack.target));
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
