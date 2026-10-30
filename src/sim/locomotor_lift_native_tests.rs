//! Bounded original hit -> Imbue -> class destination -> installed Jumpjet
//! comparisons. Release altitude and settled moving=false are explicit native
//! input boundaries; the retail production test owns actual flight/landing.

use super::LocomotorShotTarget;
use crate::rules::{ini_parser::IniFile, locomotor_type::LocomotorKind, ruleset::RuleSet};
use crate::sim::components::NavTargetRef;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::world::{FrameEffects, Simulation};
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/magnetron_aircraft_lift.json",
    ))
    .unwrap()
}

/// Replay the selected lexical layers used by the original readers. The
/// registries and synthetic reload pad are declared fixture scaffolding.
fn selected_rules(data: &Value) -> RuleSet {
    let mut ini = IniFile::from_str(
        "[VehicleTypes]\n0=TELE\n1=MTNK\n[AircraftTypes]\n0=ORCA\n1=BEAG\n\
         [InfantryTypes]\n0=E1\n[Warheads]\n0=LocomotorBeam\n\
         [BuildingTypes]\n0=PreparedReloadPad\n[PreparedReloadPad]\n\
         Strength=1000\nUnitReload=yes\nFoundation=1x1\n[Clear]\nBuildable=yes\n",
    );
    for layer in data["inputs"]["layers"].as_array().unwrap() {
        let Some(sections) = layer["sections"].as_object() else {
            continue;
        };
        let mut text = String::new();
        for (section, values) in sections {
            text.push_str(&format!("[{section}]\n"));
            for (key, value) in values.as_object().unwrap() {
                text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
        }
        ini.merge(&IniFile::from_str(&text));
    }
    let rules = RuleSet::from_ini(&ini).unwrap();
    for (name, native) in data["inputs"]["types"].as_object().unwrap() {
        let object = rules.object(name).unwrap();
        assert_eq!(
            object.strength, native["strength"],
            "{name}: reader strength"
        );
        assert_eq!(object.size, native["size"], "{name}: reader Size");
        if let Some(landable) = native["landable"].as_bool() {
            assert_eq!(object.landable, landable, "{name}: reader Landable");
            assert_eq!(
                object.airport_bound, native["airport_bound"],
                "{name}: reader AirportBound"
            );
        }
    }
    rules
}

struct Scene {
    sim: Simulation,
    firer: u64,
    victim: u64,
    pad: Option<u64>,
}

fn kind_name(kind: LocomotorKind) -> &'static str {
    match kind {
        LocomotorKind::Fly => "fly",
        LocomotorKind::Drive => "drive",
        LocomotorKind::Walk => "walk",
        LocomotorKind::Jumpjet => "jumpjet",
        _ => panic!("unexpected fixture locomotor {kind:?}"),
    }
}

impl Scene {
    fn new(data: &Value, row: &Value, rules: &RuleSet) -> Self {
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
        sim.intern_rule_type_ids(rules);
        let mut spawn = |name, at: &Value| {
            let x = at[0].as_i64().unwrap() as i32;
            let y = at[1].as_i64().unwrap() as i32;
            let id = sim
                .spawn_object_at_height(
                    name,
                    "Americans",
                    (x / 256) as u16,
                    (y / 256) as u16,
                    0,
                    0,
                    rules,
                )
                .unwrap();
            sim.remove_entity_occupancy(id);
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity
                .position
                .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(x % 256));
            entity
                .position
                .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(y % 256));
            entity
                .position
                .set_exact_z_leptons_for_test(Some(at[2].as_i64().unwrap() as i32));
            entity.locomotor = Some(LocomotorState::from_object_type(
                rules.object(name).unwrap(),
                0,
            ));
            // The original prepared Foot objects are live, but absent from
            // the Cell member/occupation lists at this input boundary.
            id
        };
        let firer = spawn("TELE", &data["inputs"]["firer_position"]);
        let victim = spawn(
            row["input"]["type"].as_str().unwrap(),
            &row["before"]["position"],
        );
        let pad = row["input"]["docked"].as_bool().unwrap_or(false).then(|| {
            let id = sim
                .spawn_object("PreparedReloadPad", "Americans", 10, 10, 0, rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(victim)
                .unwrap()
                .radio_contacts
                .set_slot(0, id);
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .radio_contacts
                .set_slot(0, victim);
            id
        });
        sim.resolve_type_handles(rules);
        // Match the native prepared map's actual playable diamond. The
        // source object is supplied outside it, so FNPC must choose a cell
        // inside the boundary rather than accepting the requested source cell.
        sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 16,
            off_fc: 0,
            off_100: 0,
            off_104: 32,
            off_108: 32,
        }));
        sim.set_playfield_size_height_for_test(Some(16));
        sim.scenario_rng = crate::sim::rng::SimRng::new(data["inputs"]["seed"].as_u64().unwrap());
        Self {
            sim,
            firer,
            victim,
            pad,
        }
    }

    fn object_ref(&self, id: Option<u64>) -> Value {
        id.map_or(Value::Null, |id| {
            json!({"object": if id == self.firer { "firer" }
                else if id == self.victim { "victim" }
                else if Some(id) == self.pad { "pad" }
                else { panic!("unknown fixture object {id}") }})
        })
    }

    fn assert_state(&self, expected: &Value, context: &str) {
        let plane = self.sim.substrate.entities.get(self.victim).unwrap();
        let loco = plane.locomotor.as_ref().unwrap();
        let coord = crate::sim::movement::ground_pose::position_world_coord(&plane.position);
        let nav = match plane.navigation.nav_com() {
            Some(NavTargetRef::Object { id }) => self.object_ref(Some(id)),
            Some(NavTargetRef::Cell { rx, ry }) => json!({"cell": [rx, ry]}),
            None => Value::Null,
        };
        let actual = json!({
            "position": [coord.x, coord.y, coord.z], "health": plane.health.current(),
            "nav": nav, "source": self.object_ref(plane.lift.source()),
            "target": self.object_ref(self.sim.substrate.entities.get(self.firer).unwrap().lift.target()),
            "lifted": plane.lift.swap_active(), "released": plane.lift.may_end(),
            "crashing": plane.crash.is_crashing(), "manipulated": plane.drop_credit.being_manipulated(),
            "credit": self.object_ref(plane.drop_credit.manipulated_by()),
            "active_kind": kind_name(loco.active_kind()),
            "contact": self.object_ref(plane.radio_contacts.slot(0)),
            "pad_contact": self.object_ref(self.pad.and_then(|id| self.sim.substrate.entities.get(id).unwrap().radio_contacts.slot(0))),
            "mission": plane.mission.current().raw(), "queued": plane.mission.queued().raw(),
        });
        for (key, value) in actual.as_object().unwrap() {
            assert_eq!(value, &expected[key], "{context}: {key}");
        }
        if let Some(jumpjet) = loco.jumpjet_runtime() {
            let destination = jumpjet.destination();
            assert_eq!(
                json!({
                    "destination": [destination.x, destination.y, destination.z],
                    "moving": jumpjet.moving(), "phase": jumpjet.phase(),
                    "suspended_kind": loco.piggyback().as_ref().map(|old| kind_name(old.active_kind())),
                }),
                expected["jumpjet"],
                "{context}: installed Jumpjet"
            );
        } else {
            // After END, native records the detached Jumpjet allocation too.
            // Rust destroys it; only the restored owner is compared here.
            assert!(loco.piggyback().is_none(), "{context}: restored original");
        }
    }
}

#[test]
fn magnetron_aircraft_hit_and_release_match_original_composition() {
    let data = corpus();
    let rules = selected_rules(&data);
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for row in cases {
        let name = row["input"]["name"].as_str().unwrap();
        let mut scene = Scene::new(&data, row, &rules);
        scene.assert_state(&row["before"], &format!("{name} before"));
        scene.sim.locomotor_detonation(
            scene.firer,
            LocomotorShotTarget::Object(scene.victim),
            row["input"]["strength"]
                .as_i64()
                .unwrap_or(data["inputs"]["magnetic_beam_damage"].as_i64().unwrap())
                as i32,
            LocomotorKind::Jumpjet,
            &crate::rules::jumpjet_params::JumpjetOverride::default(),
            &rules,
            None,
            FrameEffects::default(),
        );
        scene.assert_state(&row["after_hit"], &format!("{name} hit"));
        if let Some(height) = row["input"]["release_height"].as_i64() {
            scene.sim.set_object_height(
                scene.victim,
                height as i32,
                &rules,
                None,
                FrameEffects::default(),
            );
            scene.assert_state(&row["before_release"], &format!("{name} release input"));
            scene
                .sim
                .release_locomotor(scene.firer, false, &rules, None, FrameEffects::default());
            scene.assert_state(&row["after_release"], &format!("{name} release"));
            if height == 0 {
                assert!(!scene.sim.try_end_piggyback(scene.victim, &rules));
                scene.assert_state(&row["after_restore_gate"], &format!("{name} moving gate"));
                let runtime = scene
                    .sim
                    .substrate
                    .entities
                    .get_mut(scene.victim)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .jumpjet_runtime_mut()
                    .unwrap();
                *runtime = runtime.clone().with_moving_for_test(false);
                scene.assert_state(
                    &row["before_supplied_settled_restore"],
                    &format!("{name} settled input"),
                );
                assert!(scene.sim.try_end_piggyback(scene.victim, &rules));
                scene.assert_state(
                    &row["after_supplied_settled_restore"],
                    &format!("{name} settled gate"),
                );
            }
        }
        assert_eq!(
            scene.sim.scenario_rng.next_u32(),
            row["next_random"],
            "{name}: original RNG continuation"
        );
    }
}
