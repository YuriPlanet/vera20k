use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::{
    art_data::ArtRegistry, foundation::FOUNDATION_TABLE, ini_parser::IniFile, ruleset::RuleSet,
};
use crate::sim::{
    components::NavTargetRef, game_entity::GameEntity, intern::test_interner,
    snapshot::GameSnapshot, world::Simulation,
};
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

fn rows() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/building_navigation_coordinate.json"
    ))
    .unwrap()
}

fn coord(v: &Value) -> DriveCoord {
    DriveCoord {
        x: v[0].as_i64().unwrap() as i32,
        y: v[1].as_i64().unwrap() as i32,
        z: v[2].as_i64().unwrap() as i32,
    }
}

fn center(mut c: DriveCoord, foundation: usize) -> DriveCoord {
    // Input for the +4C comparison; +48 itself has the existing235-call
    // Object::Distance_To corpus, including every native foundation.
    let f = FOUNDATION_TABLE[foundation];
    c.x = c.x.wrapping_add((i32::from(f.width) - 1) * 128);
    c.y = c.y.wrapping_add((i32::from(f.height) - 1) * 128);
    c
}

fn rules(input: &Value) -> RuleSet {
    let mut ini = format!(
        "[BuildingTypes]\n0=BUILDING\n[VehicleTypes]\n0=MTNK\n[InfantryTypes]\n0=ENGI\n[ENGI]\nEngineer=yes\nStrength=100\nSpeed=4\nLocomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n[MTNK]\nStrength=100\nSpeed=0\n[BUILDING]\nCapturable=yes\nStrength=500\nNumberOfDocks={}\n",
        input["count"].as_i64().unwrap_or(1)
    );
    for flag in input["flags"].as_array().into_iter().flatten() {
        let key = match flag.as_str().unwrap() {
            "repair" => "UnitRepair",
            "helipad" => "Helipad",
            "bunker" => "Bunker",
            "refinery" => "Refinery",
            "weeder" => "Weeder",
            _ => panic!("unknown flag"),
        };
        ini.push_str(&format!("{key}=yes\n"));
    }
    let foundation = FOUNDATION_TABLE[input["foundation"].as_u64().unwrap_or(0) as usize].name;
    let mut art = format!("[BUILDING]\nFoundation={foundation}\n");
    for (i, offset) in input["offsets"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let c = coord(offset);
        art.push_str(&format!("DockingOffset{i}={},{},{}\n", c.x, c.y, c.z));
    }
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(&art)));
    rules
}

fn contacts(input: &Value) -> Contacts {
    let slots = input["contacts"].as_array().cloned().unwrap_or_default();
    let mut contacts = Contacts::with_capacity(slots.len());
    // Reserve every slot before nulling holes so the real Contacts owner
    // preserves the native vector's sparse indices. Duplicate requester rows
    // deliberately select the first match, which is sufficient for the query.
    for (index, slot) in slots.iter().enumerate() {
        let id = if slot == 1 && !contacts.contains(1) {
            1
        } else {
            100 + index as u64
        };
        contacts.insert(id);
    }
    for (index, slot) in slots.iter().enumerate() {
        if slot == 0 {
            contacts.remove(100 + index as u64);
        }
    }
    contacts
}

#[test]
fn building_navigation_matches_all_native_dispatch_and_direction_rows() {
    let rows = rows();
    assert_eq!(rows.len(), 258);
    for row in rows {
        let input = &row["input"];
        let rules = rules(input);
        let object = rules.object("BUILDING").unwrap();
        let current = coord(&input["current"]);
        let requester = (!input["requester"].is_null()).then_some(1);
        let result = navigation_coordinate(
            current,
            center(current, input["foundation"].as_u64().unwrap_or(0) as usize),
            object,
            &contacts(input),
            requester,
            || {
                let mut c = coord(&input["requester"]);
                match input["requester_kind"].as_str() {
                    Some("building") => c = center(c, 5),
                    Some("attached_anim") => {
                        let owner = center(coord(&input["requester_owner"]), 5);
                        c.x = c.x.wrapping_add(owner.x);
                        c.y = c.y.wrapping_add(owner.y);
                        c.z = c.z.wrapping_add(owner.z);
                    }
                    _ => {}
                }
                Ok(c)
            },
        )
        .unwrap();
        assert_eq!(result, coord(&row["coordinate"]), "{input}");
    }
}

fn position(entity: &mut GameEntity, c: DriveCoord) {
    entity.position.rx = (c.x / 256) as u16;
    entity.position.ry = (c.y / 256) as u16;
    entity.position.sub_x = SimFixed::from_num(c.x % 256);
    entity.position.sub_y = SimFixed::from_num(c.y % 256);
    entity.position.exact_z_leptons = Some(c.z);
}

#[test]
fn building_navigation_capture_command_preserves_the_native_dock_coordinate() {
    use crate::sim::intern::test_intern;
    let row = rows()
        .into_iter()
        .find(|row| row["input"]["name"] == "dispatch_5_helipad")
        .unwrap();
    let input = &row["input"];
    let rules = rules(input);
    let mut sim = Simulation::with_seed(0);
    let mut building = GameEntity::test_default(2, "BUILDING", "Soviet", 5, 6);
    building.category = EntityCategory::Structure;
    building.foundation = rules.object("BUILDING").unwrap().foundation.clone();
    building.radio_contacts = contacts(input);
    building.lifecycle.in_limbo = false;
    position(&mut building, coord(&input["current"]));
    let mut engineer = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        0,
        0,
        test_intern("Americans"),
        crate::sim::components::Health { current: 100 },
        test_intern("ENGI"),
        EntityCategory::Infantry,
        0,
        5,
        true,
    );
    engineer.lifecycle.in_limbo = false;
    engineer.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::from_object_type(
            rules.object("ENGI").unwrap(),
            0,
        ),
    );
    position(&mut engineer, coord(&input["requester"]));
    sim.substrate.entities.insert(building);
    sim.substrate.entities.insert(engineer);
    sim.interner = test_interner();
    let accepted = sim.apply_command(
        "Americans",
        &crate::sim::command::Command::CaptureBuilding {
            engineer_id: 1,
            target_building_id: 2,
        },
        Some(&rules),
        None,
        &Default::default(),
    );
    assert!(accepted);
    let engineer = sim.substrate.entities.get(1).unwrap();
    assert_eq!(engineer.capture_target, Some(2));
    assert_eq!(
        engineer.navigation.nav_com,
        Some(NavTargetRef::Building { id: 2 })
    );
    assert_eq!(
        engineer.locomotor.as_ref().unwrap().walk_destination(),
        Some(coord(&row["coordinate"]))
    );
}

#[test]
fn building_navcom_reads_live_type_requester_and_radio_slots_after_restore() {
    let mut checked = 0;
    for row in rows() {
        let input = &row["input"];
        let current = coord(&input["current"]);
        let requester = (!input["requester"].is_null()).then(|| coord(&input["requester"]));
        let representable =
            |c: DriveCoord| (0..65536 * 256).contains(&c.x) && (0..65536 * 256).contains(&c.y);
        // This fixture supplies live Foot requesters; Anim +48 dispatch is
        // covered as a query input above, not a Foot AssignDestination caller.
        if !representable(current)
            || requester.is_some_and(|c| !representable(c))
            || input["requester_kind"].is_string()
        {
            continue;
        }
        let rules = rules(input);
        let mut sim = Simulation::with_seed(0);
        let mut building = GameEntity::test_default(2, "BUILDING", "Americans", 5, 6);
        building.category = EntityCategory::Structure;
        building.foundation = rules.object("BUILDING").unwrap().foundation.clone();
        building.radio_contacts = contacts(input);
        position(&mut building, current);
        sim.substrate.entities.insert(building);
        if let Some(c) = requester {
            let mut mover = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
            position(&mut mover, c);
            sim.substrate.entities.insert(mover);
        }
        // Native fixture contact pointers refer to allocated objects, even
        // when they are not the requester. Supply them for snapshot swizzling.
        let linked: Vec<_> = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .radio_contacts
            .iter_live()
            .collect();
        for id in linked {
            if sim.substrate.entities.get(id).is_none() {
                sim.substrate.entities.insert(GameEntity::test_default(
                    id,
                    "MTNK",
                    "Americans",
                    0,
                    0,
                ));
            }
        }
        sim.interner = test_interner();
        sim.substrate.next_stable_object_id = 200;
        for target in [
            NavTargetRef::Entity { id: 2 },
            NavTargetRef::Building { id: 2 },
            NavTargetRef::Object { id: 2 },
        ] {
            let before = super::super::nav_target_coordinate(
                target,
                requester.map(|_| 1),
                &sim.substrate.entities,
                None,
                Some((&rules, &sim.interner)),
            )
            .unwrap();
            assert_eq!(before, coord(&row["coordinate"]), "{input}");
        }
        if checked % 13 == 0 {
            let saved = GameSnapshot::save(&sim, 0, 0, "building destination", 0);
            let mut restored = GameSnapshot::load(&saved).unwrap().sim;
            restored.restore_after_snapshot_load().unwrap();
            assert_eq!(sim.state_hash(), restored.state_hash());
            let result = super::super::nav_target_coordinate(
                NavTargetRef::Building { id: 2 },
                requester.map(|_| 1),
                &restored.substrate.entities,
                None,
                Some((&rules, &restored.interner)),
            )
            .unwrap();
            assert_eq!(result, coord(&row["coordinate"]), "{input}");
        }
        checked += 1;
    }
    assert_eq!(checked, 136, "representable live Foot-requester cases");
}
