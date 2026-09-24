use super::*;
use crate::sim::timer::CdTimer;
use serde_json::Value;

/// A sparse row merged over the corpus defaults, as the oracle's `resolve`
/// does: groups merge per key (cell verdicts one level deeper) and weapons
/// per slot (null is an empty slot, at least two slots).
fn resolve(defaults: &Value, row: &Value) -> Value {
    let mut full = defaults.clone();
    let full_map = full.as_object_mut().unwrap();
    for (key, given) in row.as_object().unwrap() {
        match key.as_str() {
            "name" | "covers" => {}
            "weapons" => {
                let base = &defaults["weapons"][0];
                let given = given.as_array().unwrap();
                let slots: Vec<Value> = (0..given.len().max(2))
                    .map(|slot| match given.get(slot) {
                        Some(Value::Null) => Value::Null,
                        other => {
                            let mut weapon = base.clone();
                            if let Some(Value::Object(fields)) = other {
                                for (field, value) in fields {
                                    weapon[field] = value.clone();
                                }
                            }
                            weapon
                        }
                    })
                    .collect();
                full_map.insert(key.clone(), Value::Array(slots));
            }
            _ => match given {
                Value::Object(fields) => {
                    let group = full_map.get_mut(key).unwrap();
                    for (field, value) in fields {
                        match (&mut group[field], value) {
                            (Value::Object(cell), Value::Object(update)) => {
                                for (name, part) in update {
                                    cell.insert(name.clone(), part.clone());
                                }
                            }
                            (slot, value) => *slot = value.clone(),
                        }
                    }
                }
                value => {
                    full_map.insert(key.clone(), value.clone());
                }
            },
        }
    }
    full
}

fn int(value: &Value) -> i32 {
    match value {
        Value::Bool(flag) => i32::from(*flag),
        value => value.as_i64().unwrap() as i32,
    }
}

fn flag(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Null => false,
        value => value.as_i64().unwrap() != 0,
    }
}

/// A binary64 input: a JSON number, or "0x" and its 16 hex digits of bits.
fn float(value: &Value) -> f64 {
    match value {
        Value::String(bits) => {
            f64::from_bits(u64::from_str_radix(bits.trim_start_matches("0x"), 16).unwrap())
        }
        value => value.as_f64().unwrap(),
    }
}

fn link(value: &Value) -> Link {
    match value.as_str() {
        None => Link::None,
        Some("target") => Link::Target,
        Some(_) => Link::Other,
    }
}

fn cell(value: &Value) -> CellFacts {
    CellFacts {
        land_type: int(&value["land_type"]),
        flags: int(&value["flags"]) as u32,
    }
}

/// A CDTimer's time left (`+0` start, `+8` duration): the stopped sentinel -1
/// keeps the duration.
fn time_left(start: &Value, left: &Value, frame: i32) -> i32 {
    CdTimer::from_raw(int(start), int(left)).remaining(frame)
}

/// vt+0x184 (`0x005B3040`) through the mission owner.
fn effective_mission(current: &Value, queued: &Value) -> i32 {
    let mut state = crate::sim::mission::MissionCom::at_frame(0);
    state.apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
        current: crate::sim::mission::MissionId::from_raw(int(current)),
        queued: crate::sim::mission::MissionId::from_raw(int(queued)),
        suspended: crate::sim::mission::MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
    });
    state.effective().raw()
}

fn weapon(slot: &Value, input: &Value) -> Option<WeaponFacts> {
    if slot.is_null() {
        return None;
    }
    let warhead = &input["warhead"];
    let target_type = &input["target_type"];
    let verses = float(&warhead["verses"][usize::try_from(int(&target_type["armor"])).unwrap()]);
    let projectile = &input["projectile"];
    Some(WeaponFacts {
        damage: int(&slot["damage"]),
        ambient_damage: int(&slot["ambient_damage"]),
        burst: int(&slot["burst"]),
        range: int(&slot["range"]),
        use_fire_particles: flag(&slot["use_fire_particles"]),
        use_spark_particles: flag(&slot["use_spark_particles"]),
        omni_fire: flag(&slot["omni_fire"]),
        is_railgun: flag(&slot["is_railgun"]),
        is_sonic: flag(&slot["is_sonic"]),
        spawner: flag(&slot["spawner"]),
        decloak_to_fire: flag(&slot["decloak_to_fire"]),
        fire_while_moving: flag(&slot["fire_while_moving"]),
        drain_weapon: flag(&slot["drain_weapon"]),
        fire_in_transport: flag(&slot["fire_in_transport"]),
        area_fire: flag(&slot["area_fire"]),
        is_mag_beam: flag(&slot["is_mag_beam"]),
        warhead: Some(WarheadFacts {
            mind_control: flag(&warhead["mind_control"]),
            ivan_bomb: flag(&warhead["ivan_bomb"]),
            parasite: flag(&warhead["parasite"]),
            temporal: flag(&warhead["temporal"]),
            is_locomotor: flag(&warhead["is_locomotor"]),
            psychedelic: flag(&warhead["psychedelic"]),
            bomb_disarm: flag(&warhead["bomb_disarm"]),
            // `FCOMP` then `TEST AH,0x40`: C3 is set for equal and unordered.
            verses_zero: verses == 0.0 || verses.is_nan(),
        }),
        projectile: ProjectileFacts {
            aa: flag(&projectile["aa"]),
            ag: flag(&projectile["ag"]),
            rot: int(&projectile["rot"]),
        },
    })
}

fn class(name: &str) -> FirerClass {
    match name {
        "unit" => FirerClass::Unit,
        "infantry" => FirerClass::Infantry,
        "aircraft" => FirerClass::Aircraft,
        "building" => FirerClass::Building,
        other => panic!("class {other}"),
    }
}

fn target_kind(name: &str) -> FireTargetKind {
    match name {
        "none" => FireTargetKind::None,
        "unit" => FireTargetKind::Unit,
        "infantry" => FireTargetKind::Infantry,
        "aircraft" => FireTargetKind::Aircraft,
        "building" => FireTargetKind::Building,
        "cell" => FireTargetKind::Cell,
        "object" => FireTargetKind::Object,
        other => panic!("target kind {other}"),
    }
}

/// The plain fields of one resolved row.
fn facts(input: &Value) -> FireFacts {
    let class = class(input["class"].as_str().unwrap());
    let frame = int(&input["frame"]);
    let f = &input["firer"];
    let ft = &input["firer_type"];
    let t = &input["target"];
    let tt = &input["target_type"];
    let spawns = f["spawn_slots"].as_array().map(|slots| {
        // `0x006B7D30` and `0x006B7D80` over the SpawnManager's items.
        let state = |slot: &Value| int(&slot["state"]);
        SpawnCounts {
            not_regenerating: slots.iter().filter(|slot| state(slot) != 7).count() as i32,
            launched_missiles: slots
                .iter()
                .filter(|slot| {
                    state(slot) == 1
                        || (state(slot) == 2
                            && slot["child"].is_object()
                            && !flag(&slot["child"]["in_limbo"])
                            && flag(&slot["child"]["missile_spawn"]))
                })
                .count() as i32,
        }
    });
    FireFacts {
        class,
        frame,
        weapon_index: int(&input["weapon_index"]),
        firer: FirerFacts {
            enslaved: flag(&f["enslaved"]),
            warped_out: flag(&f["warped_out"]),
            warping_in: flag(&f["warping_in"]),
            robot_offline: flag(&f["robot_offline"]),
            sinking: flag(&f["sinking"]),
            berserk: flag(&f["berserk"]),
            falling: flag(&f["falling"]),
            in_open_transport: flag(&f["in_open_transport"]),
            transporter: match f["transporter"].as_str() {
                None => Transporter::None,
                Some("target") => Transporter::Target,
                Some("plain") => Transporter::Plain,
                Some("warped_out") => Transporter::WarpedOut,
                Some("nested") => Transporter::Nested,
                Some(other) => panic!("transporter {other}"),
            },
            on_bridge: flag(&f["on_bridge"]),
            z: int(&f["z"]),
            emp_remaining: int(&f["emp_remaining"]),
            death_frame_counter: int(&f["death_frame_counter"]),
            fire_particles_live: flag(&f["fire_particles_live"]),
            spark_particles_live: flag(&f["spark_particles_live"]),
            railgun_particles_live: flag(&f["railgun_particles_live"]),
            wave_live: flag(&f["wave_live"]),
            rearming: time_left(&f["rearm_start"], &f["rearm_left"], frame) != 0,
            burst_index: int(&f["burst_index"]),
            ammo: int(&f["ammo"]),
            cloak_state: int(&f["cloak_state"]),
            current_weapon: int(&f["current_weapon"]),
            draining_me: flag(&f["draining_me"]),
            drain_target: link(&f["drain_target"]),
            locomotor_target: link(&f["locomotor_target"]),
            temporal: match f["temporal"].as_str() {
                None => None,
                Some("idle") => Some(Link::None),
                Some(other) => Some(link(&Value::from(other))),
            },
            owner_is_human: flag(&f["owner_is_human"]),
            // Foot `0x004DE770` (SETNE); a Building's vt+0x380 is false.
            paralyzed: class.is_foot()
                && time_left(&f["paralysis_start"], &f["paralysis_left"], frame) != 0,
            spawns,
            primary_facing: int(&f["primary_facing"]) as u16,
            secondary_facing: int(&f["secondary_facing"]) as u16,
            magnetron_lifted: flag(&f["magnetron_lifted"]),
            navcom: flag(&f["navcom"]),
            // I4: `FCOMP 0.1` then `TEST AH,0x41`; NaN is not above.
            moving_faster_than_tenth: float(&f["speed_fraction"]) > 0.1,
            rocker_link: flag(&f["rocker_link"]),
            deploying: flag(&f["deploying"]) || flag(&f["undeploying"]),
            tethered: flag(&f["tethered"]),
            radio_link: match f["radio_link"].as_str() {
                None => RadioLink::None,
                Some("building") => RadioLink::Building,
                Some(_) => RadioLink::Other,
            },
            firing_sequence: flag(&f["firing_sequence"]),
            turret_rotation_latch: flag(&f["turret_rotation_latch"]),
            firing_frame: int(&f["firing_frame"]),
            sequence: int(&f["sequence"]),
            paradrop_payload: flag(&f["paradrop_payload"]),
            has_passenger: flag(&f["has_passenger"]),
            effective_mission: effective_mission(&f["mission"], &f["queued_mission"]),
            delayed_fire_counter: int(&f["delayed_fire_counter"]),
            turret_upgrade: flag(&f["turret_upgrade"]),
        },
        firer_type: FirerTypeFacts {
            natural: flag(&ft["natural"]),
            pushy: flag(&ft["pushy"]),
            land_targeting: int(&ft["land_targeting"]),
            mobile_fire: flag(&ft["mobile_fire"]),
            turret_count: int(&ft["turret_count"]),
            turret: flag(&ft["turret"]),
            is_gattling: flag(&ft["is_gattling"]),
            hunter_seeker: flag(&ft["hunter_seeker"]),
            balloon_hover: flag(&ft["balloon_hover"]),
            jumpjet: flag(&ft["jumpjet"]),
            organic: flag(&ft["organic"]),
            deploy_to_fire: flag(&ft["deploy_to_fire"]),
            small_visceroid: flag(&ft["small_visceroid"]),
            large_visceroid: flag(&ft["large_visceroid"]),
            facings: int(&ft["facings"]),
            firing_sync_frame: [
                int(&ft["firing_sync_frame"][0]),
                int(&ft["firing_sync_frame"][1]),
            ],
            jumpjet_turn: flag(&ft["jumpjet_turn"]),
            can_be_occupied: flag(&ft["can_be_occupied"]),
            can_occupy_fire: flag(&ft["can_occupy_fire"]),
            emp_pulse_cannon: flag(&ft["emp_pulse_cannon"]),
            turret_anim_is_voxel: flag(&ft["turret_anim_is_voxel"]),
        },
        target: TargetFacts {
            kind: target_kind(t["kind"].as_str().unwrap()),
            bomb: flag(&t["bomb"]),
            in_limbo: flag(&t["in_limbo"]),
            on_bridge: flag(&t["on_bridge"]),
            z: int(&t["z"]),
            mission: int(&t["mission"]),
            // `0x0041BF40` (SETG): a stopped timer with negative time is off.
            iron_curtained: time_left(&t["iron_curtain_start"], &t["iron_curtain_left"], frame) > 0,
            draining_me: flag(&t["draining_me"]),
            warped_out: flag(&t["warped_out"]),
            chrono_warp_latch: flag(&t["chrono_warp_latch"]),
            bunkered: flag(&t["bunkered"]),
            sinking: flag(&t["sinking"]),
            docked: flag(&t["docked"]),
            health: int(&t["health"]),
            parasite_lock_until: int(&t["parasite_lock_until"]),
            deploying: flag(&t["deploying"]) || flag(&t["undeploying"]),
            rocked_by_another: t["rocker_link"].as_str() == Some("other"),
            land_type: int(&t["land_type"]),
        },
        target_type: TargetTypeFacts {
            strength: int(&tt["strength"]),
            drainable: flag(&tt["drainable"]),
            berserk_friendly: flag(&tt["berserk_friendly"]),
            unnatural: flag(&tt["unnatural"]),
            immune_to_psionics: flag(&tt["immune_to_psionics"]),
            spawned: flag(&tt["spawned"]),
            balloon_hover: flag(&tt["balloon_hover"]),
            jumpjet: flag(&tt["jumpjet"]),
            organic: flag(&tt["organic"]),
            is_simple_deployer: flag(&tt["is_simple_deployer"]),
            non_vehicle: flag(&tt["non_vehicle"]),
            // The fixture's BuildingType is not a 1x1 undeployer.
            building_vehicle: false,
        },
    }
}

/// The oracle's substituted queries: the row's verdicts, logged by name.
struct RowQuery<'a> {
    input: &'a Value,
    weapons: Vec<Option<WeaponFacts>>,
    calls: Vec<&'static str>,
}

impl RowQuery<'_> {
    fn verdict(&mut self, name: &'static str) -> &Value {
        self.calls.push(name);
        &self.input["verdicts"][name]
    }
}

impl FireQuery for RowQuery<'_> {
    fn weapon(&mut self, index: i32) -> Option<WeaponFacts> {
        usize::try_from(index)
            .ok()
            .and_then(|slot| self.weapons.get(slot).copied().flatten())
    }
    fn in_range(&mut self) -> bool {
        flag(self.verdict("in_range"))
    }
    fn naval_selector(&mut self) -> i32 {
        int(self.verdict("naval_selector"))
    }
    fn visual_state(&mut self) -> i32 {
        int(self.verdict("visual_state"))
    }
    fn high_flying(&mut self) -> bool {
        flag(self.verdict("high_flying"))
    }
    fn low_flying(&mut self) -> bool {
        flag(self.verdict("low_flying"))
    }
    fn firer_high_flying(&mut self) -> bool {
        flag(self.verdict("firer_high_flying"))
    }
    fn target_layer(&mut self) -> i32 {
        int(self.verdict("target_layer"))
    }
    fn target_cell(&mut self) -> Option<CellFacts> {
        Some(cell(self.verdict("target_cell")))
    }
    fn firer_cell(&mut self) -> FirerCell {
        match self.verdict("firer_cell") {
            Value::Null => FirerCell::None,
            Value::String(_) => FirerCell::Target,
            value => FirerCell::Cell(cell(value)),
        }
    }
    fn sensor(&mut self) -> bool {
        flag(self.verdict("sensor"))
    }
    fn allied(&mut self) -> bool {
        flag(self.verdict("allied"))
    }
    fn bridge_for_firing(&mut self) -> bool {
        flag(self.verdict("bridge_for_firing"))
    }
    fn can_infect(&mut self) -> bool {
        flag(self.verdict("can_infect"))
    }
    fn can_capture(&mut self) -> bool {
        flag(self.verdict("can_capture"))
    }
    fn deploy_cell_ok(&mut self) -> bool {
        flag(self.verdict("deploy_cell_ok"))
    }
    fn locomotor_moving(&mut self) -> bool {
        flag(self.verdict("locomotor_moving"))
    }
    fn target_locomotor_moving(&mut self) -> bool {
        flag(self.verdict("target_locomotor_moving"))
    }
    fn locomotor_can_fire(&mut self) -> FireError {
        match int(self.verdict("locomotor_can_fire")) {
            0 => FireError::Ok,
            7 => FireError::Moving,
            other => panic!("Can_Fire {other}"),
        }
    }
    fn jumpjet_locomotor(&mut self) -> bool {
        flag(self.verdict("jumpjet_locomotor"))
    }
    fn fighter(&mut self) -> bool {
        flag(self.verdict("fighter"))
    }
    fn direction_to_target(&mut self) -> u16 {
        int(self.verdict("direction_to_target")) as u16
    }
    fn turret_direction(&mut self) -> u16 {
        int(self.verdict("turret_direction")) as u16
    }
    fn occupants(&mut self) -> i32 {
        int(self.verdict("occupants"))
    }
    fn operational(&mut self) -> bool {
        // `0x004555D0` runs natively in the oracle; only its house power
        // ratio is substituted.
        let input = self.input;
        let f = &input["firer"];
        let ft = &input["firer_type"];
        let frame = int(&input["frame"]);
        let facts = crate::sim::power_system::OperationalFacts {
            online: flag(&f["online"]),
            tesla_chargers: int(&f["tesla_chargers"]),
            emp_remaining: int(&f["emp_remaining"]),
            health: int(&f["health"]),
            powered: flag(&ft["powered"]),
            power_drain: int(&ft["power_drain"]),
            powered_special: flag(&ft["powered_special"]),
            owner_outage: time_left(&f["owner_blackout_start"], &f["owner_blackout_left"], frame)
                != 0
                || flag(&f["owner_drained_power_source"]),
            needs_engineer: flag(&ft["needs_engineer"]),
            has_engineer: flag(&f["has_engineer"]),
            effective_mission: effective_mission(&f["mission"], &f["queued_mission"]),
        };
        crate::sim::power_system::is_operational_for_output(&facts, || {
            float(self.verdict("power_fraction")) < 1.0
        })
    }
}

/// `tools/spatial_oracle/fire_error.json`: the original class entries run
/// with each row's fields, the substituted queries logged in call order.
/// Every row's code and query log must match. Building GetWeapon's own
/// occupant query (`occupants_via_get_weapon`, `0x004526F0` through
/// vt+0x400) belongs to weapon lookup, which the corpus fixes to the type's
/// own slots, so it is left out of the comparison.
#[test]
fn original_fire_error_rows() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/fire_error.json"
    ))
    .unwrap();
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1191);
    let mut failures = Vec::new();
    for row in rows {
        let input = resolve(&corpus["defaults"], &row["input"]);
        let facts = facts(&input);
        let mut query = RowQuery {
            input: &input,
            weapons: input["weapons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|slot| weapon(slot, &input))
                .collect(),
            calls: Vec::new(),
        };
        // T61 reads only the low byte of `check_range`.
        let check_range = match &input["check_range"] {
            Value::Bool(flag) => *flag,
            value => value.as_i64().unwrap() & 0xFF != 0,
        };
        let code = get_fire_error(&facts, &mut query, check_range) as i32;
        let native_calls: Vec<&str> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call.as_str().unwrap())
            .filter(|call| *call != "occupants_via_get_weapon")
            .collect();
        if code != int(&row["code"]) || query.calls != native_calls {
            failures.push(format!(
                "{}: code {} vs native {}, calls {:?} vs native {:?}",
                row["input"]["name"], code, row["code"], query.calls, native_calls
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} rows differ:\n{}",
        failures.len(),
        rows.len(),
        failures.join("\n")
    );
}

/// Retail `rulesmd.ini` through the production reader: the type flags and the
/// weapon flag GetFireError reads land on the stock types that author them.
#[test]
fn retail_fire_error_flags() {
    let Some((rules_ini, _)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let rules = crate::rules::ruleset::RuleSet::from_ini(&rules_ini).unwrap();
    let object = |name: &str| rules.object(name).unwrap_or_else(|| panic!("{name}"));
    // T14: the dogs and animals never attack Brutes, Yuri Prime, Mummies or
    // the Boomer.
    for natural in [
        "ADOG", "DOG", "YADOG", "YDOG", "COW", "ALL", "POLARB", "JOSH", "CAML",
    ] {
        assert!(object(natural).natural, "{natural}");
    }
    for unnatural in ["BRUTE", "YURIPR", "MUMY", "BSUB"] {
        assert!(object(unnatural).unnatural, "{unnatural}");
    }
    assert!(!object("E1").natural);
    // T13: berserk units spare the Chaos Drone.
    assert!(object("CAOS").berserk_friendly);
    // I6: only the Rocketeer turns in flight.
    assert!(object("LUNR").jumpjet_turn);
    // U8: only the Floating Disc's drain beam holds fire while moving.
    assert!(!rules.weapon("DiskDrain").unwrap().fire_while_moving);
    assert!(rules.weapon("DiskLaser").unwrap().fire_while_moving);
    // U7's key is unset in retail: every type keeps the constructor's yes.
    assert!(object("MTNK").mobile_fire);
}
