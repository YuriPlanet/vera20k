//! Replays `tools/spatial_oracle/aircraft_fire_at.json`: the original
//! `0x00415EE0`, one call per row, through [`fire_at`].

use super::*;
use crate::sim::movement::air_movement::current_fly_speed;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF64Bits;
use serde_json::{Value, json};

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn coord(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(int(&value[0]), int(&value[1]), int(&value[2]))
}

fn velocity(value: &Value) -> ProjectileVelocity {
    ProjectileVelocity::from_native(std::array::from_fn(|i| {
        NativeF64Bits::from_bits(u64::from_str_radix(value[i].as_str().unwrap(), 16).unwrap())
    }))
}

fn bits(velocity: ProjectileVelocity) -> Value {
    json!(
        velocity
            .native()
            .map(|axis| format!("{:016x}", axis.bits()))
    )
}

/// Answers each call from the row and logs it as the oracle logs the native
/// call it stands for. Apparent_Speed is VERA's Fly port
/// ([`current_fly_speed`]) over the row's type speed and CurrentSpeed, which
/// the log compares with the value the original returned.
struct Recorder {
    input: Value,
    velocity: ProjectileVelocity,
    shroud_calls: i64,
    calls: Vec<Value>,
}

impl FireAtHost for Recorder {
    fn carries_passenger(&mut self) -> bool {
        self.input["passenger"].as_bool().unwrap()
    }

    fn drop_payload(&mut self) {
        self.calls.push(json!(["drop_payload"]));
    }

    fn techno_fire_at(&mut self) -> Option<AircraftShot> {
        self.calls.push(json!(["techno_fire_at"]));
        self.input["bullet"]
            .as_bool()
            .unwrap()
            .then(|| AircraftShot::new(int(&self.input["rot"]), self.velocity))
    }

    fn apparent_speed(&mut self) -> i32 {
        let fraction = SimFixed::from_num(self.input["current_speed"].as_f64().unwrap());
        let speed = current_fly_speed(int(&self.input["type_speed"]), fraction);
        self.calls.push(json!(["apparent_speed", speed]));
        speed
    }

    fn facing(&mut self) -> u16 {
        self.calls.push(json!(["facing"]));
        int(&self.input["facing"]) as u16
    }

    fn coords(&mut self) -> ProjectileCoord {
        coord(&self.input["location"])
    }

    fn target_coords(&mut self) -> ProjectileCoord {
        coord(&self.input["target_location"])
    }

    fn weapon0_speed(&mut self) -> i32 {
        self.calls.push(json!(["weapon0_speed"]));
        int(&self.input["weapon0_speed"])
    }

    fn set_velocity(&mut self, velocity: ProjectileVelocity) {
        self.velocity = velocity;
    }

    fn owner_is_player(&mut self) -> bool {
        self.calls.push(json!(["owner_is_player"]));
        self.input["player"].as_bool().unwrap()
    }

    fn location(&mut self) -> ProjectileCoord {
        coord(&self.input["location"])
    }

    fn is_shrouded(&mut self, point: ProjectileCoord) -> bool {
        self.calls
            .push(json!(["is_shrouded", [point.x, point.y, point.z]]));
        let shrouded = self.input["shrouded_at"].as_i64() == Some(self.shroud_calls);
        self.shroud_calls += 1;
        shrouded
    }

    fn reveal_area(&mut self, final_pass: bool) {
        self.calls.push(json!([
            "reveal_area",
            self.input["location"],
            self.input["sight_range"],
            i32::from(final_pass)
        ]));
    }

    fn destroy_after_firing(&mut self) -> bool {
        self.input["destroy"].as_bool().unwrap()
    }

    fn uninit(&mut self) {
        self.calls.push(json!(["uninit"]));
    }
}

/// Every row: the call log (Apparent_Speed's value included) and the
/// bullet's velocity afterwards, bit for bit: the passenger arm, a NULL
/// bullet, ROT 0 over speeds, CurrentSpeeds, facings and velocities (the
/// zero and vertical vectors the Vector3D guards seed among them), ROT 1
/// over velocities, target offsets (the Location itself among them) and
/// weapon speeds (0 included), other ROTs, each shroud probe and the
/// reveal pair, and `+0x6CA`.
#[test]
fn fire_at_matches_the_original() {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_fire_at.json",
    ))
    .expect("aircraft_fire_at.json");
    let rows = oracle["fire_at"].as_array().unwrap();
    assert_eq!(rows.len(), 320);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut host = Recorder {
            input: input.clone(),
            velocity: velocity(&input["velocity"]),
            shroud_calls: 0,
            calls: Vec::new(),
        };
        fire_at(&mut host);
        assert_eq!(json!(host.calls), row["calls"], "{name}");
        assert_eq!(bits(host.velocity), row["velocity"], "{name}");
    }
}

/// The courses retail aircraft take, through the production reader on
/// retail rulesmd.ini: the Hornet's and the Osprey's bombs are ROT 1 (the
/// course at the target, the Hornet's at `Speed=30` read as 76 leptons a
/// frame), the Harrier's, Black Eagle's and Boris MiG's missiles ROT 100
/// (FireAt's own), and `AttackingAircraftSightRange=` is 2.
#[test]
fn retail_aircraft_weapons_take_their_courses() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    for (aircraft, rot) in [
        ("HORNET", 1),
        ("ASW", 1),
        ("ORCA", 100),
        ("BEAG", 100),
        ("BPLN", 100),
    ] {
        let object = rules.object(aircraft).unwrap();
        let weapon = crate::sim::combat::combat_weapon::primary_for_tier(object, 0)
            .and_then(|weapon| rules.weapon(weapon))
            .unwrap();
        let projectile = rules
            .projectile(weapon.projectile.as_deref().unwrap())
            .unwrap();
        assert_eq!(projectile.rot, rot, "{aircraft}");
    }
    assert_eq!(rules.weapon("HornetBomb").unwrap().speed, 76);
    assert_eq!(rules.general.attacking_aircraft_sight_range, 2);
}
