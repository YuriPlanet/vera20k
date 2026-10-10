//! Supplied-state comparisons with original Foot mission/scan execution.
//!
//! The evidence owner is `tools/spatial_oracle/anytown_damage/foot_missions.py`
//! (`mission --foot-missions --check`). Unit743190/Infantry51E140 dispatch to
//! Foot4D9920 and Techno6F8DF0 without replacing their bodies or vtables.
//! The fixture adopts the observed constructor/Unlimbo state, three complete
//! RNG streams and separate Techno, Logic and Display lists. It is not a Rust
//! replay of native construction, map loading or zone initialization. These
//! rows supply cropped map/zone state; this comparison supplies entity lists
//! and occupancy, with no claim about a whole-map connectivity producer.
//! Timer comparisons cover the represented start/duration fields; the native
//! timer's auxiliary stack/storage word is outside this state projection.

use crate::sim::movement::DriveLocomotionRuntime;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::{Value, json};

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_targeting::greatest_threat_for_entity;
use crate::sim::combat::greatest_threat::{
    HOUSE_SELECTS_OWN_COEFFICIENTS, ThreatCoefficients, ThreatReference, calculate_threat_score,
};
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::{DriveCoord, Health, NavTargetRef};
use crate::sim::game_entity::{GameEntity, InfantryRuntime};
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionTimer};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::occupancy::CellListInsertion;
use crate::sim::rng::SimRng;
use crate::sim::world::display_layers::DisplayLayer;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::MaskedX87Chop53;

#[path = "area_guard_oracle_tests.rs"]
mod area_guard_oracle_tests;

pub(in crate::sim::world::techno_ai) fn oracle() -> &'static Value {
    static CORPUS: OnceLock<Value> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let value: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anytown_damage/foot_missions.json",
        ))
        .unwrap();
        assert_eq!(
            value["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
        value
    })
}

pub(in crate::sim::world::techno_ai) fn signed(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

pub(in crate::sim::world::techno_ai) fn xyz(value: &Value) -> [i32; 3] {
    std::array::from_fn(|index| signed(&value[index]))
}

fn hex_double(value: f64) -> String {
    value
        .to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn rules_receipt(rules: &RuleSet) -> Value {
    json!({
        "guard_mode_stray": rules.general.guard_mode_stray,
        "idle_action_frequency_bits": hex_double(rules.general.idle_action_frequency),
    })
}

fn sections_ini(sections: &Value) -> IniFile {
    let mut text = String::new();
    for (section, entries) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in entries.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    IniFile::from_str(&text)
}

/// Physical RULESMD and the single ARTMD snapshot go through the production
/// layered/type/ART readers. Later passes project the original saved selected
/// mode/map keys; their native readers retain E1/MTNK/weapon values. This does
/// not stand in for archive-backed map or whole ScenarioLoad validation.
pub(in crate::sim::world::techno_ai) fn retail_rules() -> Option<RuleSet> {
    rules_with_native_reader_context(false, "handler_rules_after_physical")
}

/// The earlier oracle deliberately left several already-allocated weapon
/// objects unread. Transport that declared raw-store/read boundary through
/// the production INI/type readers; never rewrite private live type fields or
/// select a range to make a handler result match.
pub(super) fn legacy_rules(array: &str) -> Option<RuleSet> {
    let context = oracle()["weapon_reader_receipts"]["reader_contexts"][array]
        .as_str()
        .unwrap_or_else(|| panic!("native reader context missing for {array}"));
    rules_with_native_reader_context(true, context)
}

fn retained_reader_ini(ini: IniFile, unread_weapons: bool, constructor_idle: bool) -> IniFile {
    if !unread_weapons && !constructor_idle {
        return ini;
    }
    let omitted = oracle()["weapon_reader_receipts"]["omitted_sections"]
        .as_array()
        .unwrap();
    let mut text = String::new();
    for name in ini.section_names() {
        if unread_weapons && omitted.iter().any(|section| section == name) {
            continue;
        }
        text.push_str(&format!("[{name}]\n"));
        for (key, value) in ini.section(name).unwrap().raw_entries() {
            // Original665650 constructor state survives until the actual
            // AudioVisual66B3E4..66B40B reader, independently of weapons.
            if constructor_idle && name == "AudioVisual" && key == "IdleActionFrequency" {
                continue;
            }
            text.push_str(&format!("{key}={value}\n"));
        }
    }
    IniFile::from_str(&text)
}

fn rules_with_native_reader_context(unread_weapons: bool, context: &str) -> Option<RuleSet> {
    let native = oracle();
    let constructor_idle = match context {
        "handler_rules_before_physical" => true,
        "handler_rules_after_physical" => false,
        context => panic!("unsupported native AV reader context {context}"),
    };
    let rules_bytes = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini")?;
    let art_bytes = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini")?;
    assert_eq!(
        crate::util::sha256::sha256_hex(&rules_bytes),
        native["inputs"]["layers"][0]["sha256"].as_str().unwrap()
    );
    assert_eq!(
        crate::util::sha256::sha256_hex(&art_bytes),
        native["inputs"]["art_sha256"].as_str().unwrap()
    );
    let mut layers = RulesLayerStack::new(retained_reader_ini(
        IniFile::from_bytes(&rules_bytes).unwrap(),
        unread_weapons,
        constructor_idle,
    ));
    for index in 1..native["inputs"]["layers"].as_array().unwrap().len() {
        let pass = &native["inputs"]["layers"][index];
        if pass["sha256"].is_null() {
            assert_eq!(pass["file"], "LANGRULE.INI");
            continue;
        }
        let mut sections = json!({});
        for entry in pass["source_lines"].as_array().unwrap() {
            let section = entry["section"].as_str().unwrap();
            let key = entry["key"].as_str().unwrap();
            if sections.get(section).is_none() {
                sections[section] = json!({});
            }
            sections[section][key] = entry["value"].clone();
        }
        if let Some(read) = native["mission_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|read| read["file"] == pass["file"])
            .and_then(|read| read["inputs"].as_object())
        {
            for (section, entries) in read {
                sections[section] = entries.clone();
            }
        }
        let kind = if index == 2 {
            RulesLayerKind::GameMode
        } else {
            RulesLayerKind::Scenario
        };
        layers.push(
            kind,
            retained_reader_ini(sections_ini(&sections), unread_weapons, constructor_idle),
        );
    }
    let art = IniFile::from_bytes(&art_bytes).unwrap();
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    assert_eq!(
        rules.object("MTNK").unwrap().strength,
        signed(&native["inputs"]["strength"])
    );
    assert_eq!(
        rules.object("E1").unwrap().strength,
        signed(&native["e1_ctor"]["health"])
    );
    assert_eq!(
        rules.weapon("M60").unwrap().range.to_num::<i32>() * 256,
        signed(&native["mission_inputs"][0]["e1_range"])
    );
    assert_eq!(
        rules_receipt(&rules),
        native["rules_reader_receipts"][context]
    );
    assert_weapon_reader_projection(
        &rules,
        &native["weapon_reader_receipts"][if unread_weapons { "before" } else { "after" }],
        "E1",
    );
    Some(rules)
}

/// Named type identity and gameplay fields only: native allocation pointers,
/// image storage/padding and reader-callback ABI are retained in the evidence,
/// not inferred from Rust allocation. Null children remain null references.
fn assert_weapon_reader_projection(rules: &RuleSet, expected: &Value, family: &str) {
    use crate::sim::combat::combat_weapon::{weapon_for_index, weapon_range};
    use crate::sim::combat::threat_range::threat_range_leptons;

    let object = rules.object(family).unwrap();
    assert_eq!(expected["range_entry"], "0x7012c0");
    assert_eq!(expected["get_weapon_entry"], "0x70e140");
    assert_eq!(expected["rng_unchanged"], true);
    for slot in expected["slots"].as_array().unwrap() {
        let tier = match slot["tier"].as_str().unwrap() {
            "normal" => 0,
            "elite" => 200,
            tier => panic!("unrepresented native weapon tier {tier}"),
        };
        let index = signed(&slot["index"]);
        let name = weapon_for_index(object, tier, index).map(|(name, _)| name);
        let receipt = &slot["weapon"];
        if receipt.is_null() {
            assert_eq!(name, None, "native type slot {slot}");
            continue;
        }
        assert_eq!(name, receipt["name"].as_str(), "native type slot {slot}");
        let weapon = rules.weapon(name.unwrap()).unwrap();
        for (actual, field) in [
            (weapon.range_leptons, "range_leptons"),
            (weapon.minimum_range_leptons, "minimum_range_leptons"),
            (weapon.damage, "damage"),
            (weapon.rof, "rof"),
            (weapon.burst, "burst"),
            (weapon.speed, "speed"),
        ] {
            assert_eq!(actual, signed(&receipt[field]), "{}: {field}", weapon.id);
        }
        let projectile = &receipt["projectile"];
        assert_eq!(
            weapon.projectile.as_deref(),
            projectile["name"].as_str(),
            "{}: native projectile reference",
            weapon.id
        );
        if let Some(name) = weapon.projectile.as_deref() {
            let actual = rules.projectile(name).unwrap();
            for (actual, field) in [
                (actual.aa, "aa"),
                (actual.ag, "ag"),
                (actual.arcing, "arcing"),
                (actual.inviso, "inviso"),
                (actual.shadow, "shadow"),
                (actual.flat, "flat"),
                (actual.rotates, "inverse_rotates"),
                (actual.voxel, "voxel"),
                (actual.theater, "theater"),
                (actual.new_theater, "new_theater"),
            ] {
                assert_eq!(
                    actual,
                    projectile[field].as_bool().unwrap(),
                    "{name}: {field}"
                );
            }
            for (actual, field) in [
                (actual.subject_to_cliffs, "subject_to_cliffs"),
                (actual.subject_to_elevation, "subject_to_elevation"),
                (actual.subject_to_walls, "subject_to_walls"),
            ] {
                assert_eq!(actual, projectile[field] != 0, "{name}: {field}");
            }
        }
        let warhead = &receipt["warhead"];
        assert_eq!(
            weapon.warhead.as_deref(),
            warhead["name"].as_str(),
            "{}: native warhead reference",
            weapon.id
        );
        if let Some(name) = weapon.warhead.as_deref() {
            let actual = rules.warhead(name).unwrap();
            assert_eq!(
                actual
                    .verses_f64
                    .map(|value| format!("{:016x}", value.to_bits())),
                std::array::from_fn::<_, 11, _>(|index| {
                    warhead["verses_bits"][index].as_str().unwrap().to_owned()
                }),
                "{name}: native Verses doubles"
            );
            assert_eq!(
                hex_double(actual.prone_damage_f64),
                warhead["prone_damage_bits"],
                "{name}: ProneDamage"
            );
            assert_eq!(actual.wall, warhead["wall"] != 0, "{name}: Wall");
            assert_eq!(actual.wood, warhead["wood"] != 0, "{name}: Wood");
        }
    }
    let row = oracle()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["family"] == family)
        .unwrap();
    let mut fixture = SuppliedFootFixture::new(row, rules);
    for control in expected["range_controls"].as_array().unwrap() {
        let rank = match control["veterancy_bits"].as_str().unwrap() {
            "00000000" => 0,
            "00000040" => 200,
            bits => panic!("unrepresented native veterancy input {bits}"),
        };
        fixture
            .sim
            .substrate
            .entities
            .get_mut(fixture.actor)
            .unwrap()
            .set_veterancy_rank(rank);
        let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
        let ranges = std::array::from_fn(|index| {
            weapon_range(
                actor,
                object,
                index as i32,
                &fixture.sim.substrate.entities,
                rules,
                &fixture.sim.interner,
            )
        });
        assert_eq!(
            ranges,
            std::array::from_fn::<_, 2, _>(|index| signed(&control["weapon_range_leptons"][index])),
            "{}: original7012C0",
            control["name"]
        );
        assert_eq!(
            std::array::from_fn::<_, 3, _>(|mode| {
                threat_range_leptons(object, mode as i32, ranges)
            }),
            xyz(&control["threat_range_leptons"]),
            "{}: original707E60 modes0/1/2",
            control["name"]
        );
        assert_rng(
            &fixture.sim,
            &row["rng_before"],
            "native range getters draw no RNG",
        );
    }
}

pub(super) fn assert_rng(sim: &Simulation, expected: &Value, name: &str) {
    for (stream, rng) in [
        ("scenario", &sim.scenario_rng),
        ("main", &sim.main_rng),
        ("mapgen", &sim.mapgen_rng),
    ] {
        assert_eq!(rng.logical_view().words.len(), 250, "{name}: {stream}");
        assert_eq!(
            serde_json::to_value(rng).unwrap(),
            expected[stream],
            "{name}: full {stream} RNG"
        );
    }
}

/// Native pointers are transported to monotonic constructor ORDINALS, rather
/// than pointer magnitude. Native Logic and Display keep independent orders.
/// The literal Foot rows call only the existing entity mask/latch owner: no
/// test port of Foot's scanner or alternate class dispatch is introduced.
pub(in crate::sim::world::techno_ai) struct SuppliedFootFixture {
    pub(in crate::sim::world::techno_ai) sim: Simulation,
    pub(in crate::sim::world::techno_ai) actor: u64,
    pointers: BTreeMap<String, u64>,
    cells: BTreeMap<String, (u16, u16)>,
}

fn supplied_target(
    objects: &BTreeMap<String, u64>,
    cells: &BTreeMap<String, (u16, u16)>,
    pointer: &Value,
) -> Option<TargetKind> {
    let pointer = pointer.as_str().unwrap();
    if pointer == "0x0" {
        None
    } else if let Some(&id) = objects.get(pointer) {
        Some(TargetKind::Entity(id))
    } else if let Some(&(x, y)) = cells.get(pointer) {
        Some(TargetKind::Cell(x, y))
    } else {
        panic!("unrepresented native target {pointer}");
    }
}

impl SuppliedFootFixture {
    /// Transport a fresh native VM's constructor roles onto this fixture's
    /// existing stable identities. Resolve every old binding before replacing
    /// the table, so coincident pointer values cannot change a later role.
    pub(super) fn remap_native_objects(&mut self, bindings: &[(&Value, &Value)]) {
        let mapped = bindings
            .iter()
            .map(|(fresh, original)| {
                let id = self.id(original).expect("native role is nonnull");
                (
                    fresh.as_str().expect("native pointer string").to_owned(),
                    id,
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            mapped.len(),
            bindings.len(),
            "native role pointers are distinct"
        );
        assert_eq!(
            mapped.len(),
            self.pointers.len(),
            "transport every constructor role"
        );
        self.pointers = mapped;
    }

    pub(in crate::sim::world::techno_ai) fn id(&self, pointer: &Value) -> Option<u64> {
        let pointer = pointer.as_str().unwrap();
        if pointer == "0x0" {
            None
        } else {
            Some(
                *self
                    .pointers
                    .get(pointer)
                    .unwrap_or_else(|| panic!("unrepresented native object {pointer}")),
            )
        }
    }

    pub(super) fn target(&self, pointer: &Value) -> Option<TargetKind> {
        supplied_target(&self.pointers, &self.cells, pointer)
    }

    pub(super) fn nav(&self, pointer: &Value) -> Option<NavTargetRef> {
        self.target(pointer).map(NavTargetRef::from)
    }

    pub(in crate::sim::world::techno_ai) fn new(row: &Value, rules: &RuleSet) -> Self {
        let native = oracle();
        let input = &row["input"];
        let setup = &native["setup"];
        let registration = &native["greatest_threat_registration"];
        let lists = if row["registration_before"].is_object() {
            assert_eq!(
                input["registration_context"],
                "greatest_threat_registration.after"
            );
            assert_eq!(row["registration_before"], registration["after"]);
            let liveness = row["candidate_liveness_before"].as_array().unwrap();
            assert_eq!(
                liveness
                    .iter()
                    .map(|actor| &actor["pointer"])
                    .collect::<Vec<_>>(),
                row["registration_before"]["techno"]["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                "native supplied lifecycle order follows the actual Techno list"
            );
            &row["registration_before"]
        } else if input["greatest_threat"].is_object() || input["navigation_control"].is_object() {
            &registration["after"]
        } else {
            &registration["before"]
        };
        let pointers: BTreeMap<String, u64> = lists["techno"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(ordinal, pointer)| (pointer.as_str().unwrap().to_owned(), ordinal as u64 + 1))
            .collect();
        // OriginalRim allocates the supplied Geometry cells in its input
        // order (CELLS+ordinal*0x200). anytown_geometry.inputs supplies y47..61,
        // x82..93. This is identity transport, not a map lookup/height port.
        let cells = (47_u16..62)
            .flat_map(|y| (82_u16..94).map(move |x| (x, y)))
            .enumerate()
            .map(|(ordinal, cell)| (format!("0x{:x}", 0x4000_0000 + ordinal * 0x200), cell))
            .collect::<BTreeMap<_, _>>();
        let actor_pointer = &setup[if input["family"] == "E1" {
            "e1"
        } else {
            "source"
        }];
        let actor = pointers[actor_pointer.as_str().unwrap()];
        let mut sim = Simulation::with_seed(input["scenario_seed"].as_u64().unwrap());
        sim.session.binary_frame = 1;
        sim.session.game_mode_nonzero = true;
        // Use the persistence owner to adopt the complete supplied original
        // cursor, including pre-fixture Main/MapGen history; no seed fitting.
        sim.scenario_rng =
            serde_json::from_value::<SimRng>(row["rng_before"]["scenario"].clone()).unwrap();
        sim.main_rng = serde_json::from_value::<SimRng>(row["rng_before"]["main"].clone()).unwrap();
        sim.mapgen_rng =
            serde_json::from_value::<SimRng>(row["rng_before"]["mapgen"].clone()).unwrap();
        let friendly = sim.interner.intern("SuppliedFriendlyHouse");
        let enemy = sim.interner.intern("SuppliedEnemyHouse");
        let country = sim.interner.intern("Americans");
        // Original fixture supplies House human bytes and country links, then
        // executes4F643B..4F6455. Whole House construction remains excluded.
        sim.houses.insert(
            friendly,
            HouseState::new(friendly, 0, Some(country), true, 0, 10),
        );
        sim.houses
            .insert(enemy, HouseState::new(enemy, 0, Some(country), true, 0, 10));
        let source_pose = native["greatest_threat_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["family"] == "MTNK")
            .unwrap()["before"]["position"]
            .clone();
        let mut placed_cells = BTreeMap::new();
        let mut supplied_poses = BTreeMap::new();
        for pointer in lists["techno"]["actors"].as_array().unwrap() {
            let pointer_text = pointer.as_str().unwrap();
            let id = pointers[pointer_text];
            // Constructor ordinals use the shared Rust namespace/cursor as
            // well as its entity store, so this supplied world can be saved.
            // Native Abstract IDs are transported separately by consumers.
            assert_eq!(sim.allocate_stable_id(), id);
            let extra = registration["placements"]
                .as_array()
                .unwrap()
                .iter()
                .find(|placement| placement["actor"] == *pointer);
            let infantry = pointer == &setup["e1"]
                || extra.is_some_and(|placement| placement["label"] == "E1");
            let type_name = if infantry { "E1" } else { "MTNK" };
            let object = rules.object(type_name).unwrap();
            let placed = if pointer == &setup["source"] {
                &source_pose
            } else if pointer == &setup["e1"] {
                &native["e1_placement"]["xyz"]
            } else if pointer == &setup["victim"] {
                &native["victim_placement"]["xyz"]
            } else if pointer == &setup["candidate"] {
                &native["candidate_placement"]["xyz"]
            } else {
                &extra.unwrap()["xyz"]
            };
            let placed = xyz(placed);
            placed_cells.insert(id, ((placed[0] / 256) as u16, (placed[1] / 256) as u16));
            let mut pose = placed;
            if id == actor {
                pose = xyz(&row["before"]["position"]);
            } else if pointer == &setup["victim"] && input["archive_xyz"].is_array() {
                pose = xyz(&input["archive_xyz"]);
            } else if pointer == &setup["candidate"] && input["candidate_xyz"].is_array() {
                pose = xyz(&input["candidate_xyz"]);
            }
            let supplied_lifecycle =
                row["candidate_liveness_before"]
                    .as_array()
                    .and_then(|controls| {
                        controls
                            .iter()
                            .find(|control| control["pointer"] == *pointer)
                    });
            if let Some(control) = supplied_lifecycle {
                pose = xyz(&control["xyz"]);
                if id == actor {
                    assert_eq!(pose, xyz(&row["before"]["position"]));
                }
            }
            supplied_poses.insert(id, pose);
            let hostile = pointer == &setup["candidate"] || extra.is_some();
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                (placed[0] / 256) as u16,
                (placed[1] / 256) as u16,
                4,
                0x80,
                if hostile { enemy } else { friendly },
                Health {
                    current: object.strength,
                },
                sim.interner.intern(type_name),
                if infantry {
                    EntityCategory::Infantry
                } else {
                    EntityCategory::Unit
                },
                0,
                object.sight as u16,
                !infantry,
            );
            entity.position.sub_x = SimFixed::from_num(placed[0] % 256);
            entity.position.sub_y = SimFixed::from_num(placed[1] % 256);
            entity.position.exact_z_leptons = Some(placed[2]);
            entity.lifecycle.in_limbo = if pointer == &setup["candidate"] {
                input["candidate_live"] != true
            } else if let Some(extra) = extra {
                input["greatest_threat"]["extra_live"][extra["label"].as_str().unwrap()] != true
            } else {
                false
            };
            if let Some(control) =
                row["inactive_candidate_readback"]
                    .as_array()
                    .and_then(|controls| {
                        controls
                            .iter()
                            .find(|control| control["pointer"] == *pointer)
                    })
            {
                // Preserve both explicitly supplied lifecycle bytes. Native
                //6F7D98 rejects InLimbo; the earlier scratch observation of
                //a limbo-only hit came from prototype_restore, not this row.
                entity.lifecycle.in_limbo = control["limbo"] != 0;
                entity.lifecycle.object_alive = control["alive"] != 0;
            }
            if let Some(control) = supplied_lifecycle {
                assert_eq!(
                    control["house"],
                    setup[if hostile { "enemy" } else { "house" }],
                    "recorded native House identity for {pointer_text}"
                );
                // These receipts occur after prototype_restore and the row's
                // explicit controls. Do not infer extra-candidate liveness
                // from the earlier scan arrays or outside-row scratch writes.
                entity.lifecycle.in_limbo = control["limbo"] != 0;
                entity.lifecycle.object_alive = control["alive"] != 0;
            }
            entity.lifecycle.cell_marked = true;
            entity.in_playfield = true;
            entity.locomotor = Some(LocomotorState::from_object_type(object, 0));
            if infantry {
                entity.infantry = Some(InfantryRuntime::new());
            }
            if id == actor {
                let before = &row["before"];
                entity.mission.apply_test_fixture(MissionTestFixture {
                    current: MissionId::from_raw(signed(&before["mission"])),
                    suspended: MissionId::NONE,
                    queued: MissionId::from_raw(signed(&before["queued"])),
                    movement_bypass_latch: 0,
                    handler_state: signed(&before["status"]) as u32,
                    mission_start_frame: 0,
                    ai_counter: signed(&before["visit"]) as u32,
                    dispatch_timer: MissionDispatchTimer::from_raw(
                        signed(&before["dispatch"][0]),
                        signed(&before["dispatch"][1]),
                    ),
                });
                let target = |pointer: &Value| supplied_target(&pointers, &cells, pointer);
                entity.set_archive_target(target(&before["archive"]));
                entity.attack_target =
                    target(&before["target"]).map(|target| AttackTarget { target });
                // The oracle writes raw NavCom without Move_To. A supplied
                // nonnull NavCom therefore does not arm Walk's moving byte.
                entity.navigation.nav_com = target(&before["nav"]).map(NavTargetRef::from);
                if input["navigation_control"].is_object() {
                    entity.navigation.nav_com_aux = target(&before["aux"]).map(NavTargetRef::from);
                    entity.navigation.nav_queue = before["nav_queue"]["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|pointer| NavTargetRef::from(target(pointer).unwrap()))
                        .collect();
                    entity.navigation.path_replay.directions = before["path"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|direction| signed(direction) as u8)
                        .collect();
                    entity.navigation.path_replay.reference_cell = Some((
                        signed(&before["reference_cell"][0]) as i16,
                        signed(&before["reference_cell"][1]) as i16,
                    ));
                    let path = &mut entity.navigation.path_runtime;
                    path.movement_timer = crate::sim::timer::CdTimer::from_raw(
                        signed(&before["movement_timer_words"][0]),
                        signed(&before["movement_timer_words"][2]),
                    );
                    path.blocked_timer = crate::sim::timer::CdTimer::from_raw(
                        signed(&before["blocked_timer_words"][0]),
                        signed(&before["blocked_timer_words"][2]),
                    );
                    path.path_blocked = before["blocked"] != 0;
                    path.retries_left = signed(&before["retry"]) as u32;
                }
                let head = xyz(&before["loco_head"]);
                if head != [0, 0, 0] {
                    let head = DriveCoord {
                        x: head[0],
                        y: head[1],
                        z: head[2],
                    };
                    if infantry {
                        entity.locomotor.as_mut().unwrap().set_step_head(Some(head));
                    } else {
                        assert!(
                            entity
                                .locomotor
                                .as_mut()
                                .unwrap()
                                .install_drive_state_for_test(Some(
                                    DriveLocomotionRuntime::default()
                                        .with_head_to_for_test(Some(head))
                                ))
                        );
                    }
                }
                entity.passive_scan_timer = MissionTimer::armed(
                    signed(&before["targeting_timer"][0]) as u32,
                    signed(&before["targeting_timer"][2]) as u32,
                );
                if before["scan"] != 0 {
                    entity.mark_stopped_cannot_fire();
                }
                // Foot+68D is serialized for every concrete Foot class. This
                // includes the supplied Unit raw-load control; it does not
                // establish a stock Unit producer for a nonzero byte.
                entity
                    .mission_leaf
                    .set_foot_firing_sequence(signed(&before["firing"]) as u8);
                if infantry {
                    entity
                        .mission_leaf
                        .set_infantry_doing_verified(signed(&before["doing"]))
                        .unwrap();
                    let words = before["sequence_timer_words"].as_array();
                    let word = |index| words.and_then(|w| w.get(index)).map_or(0, signed);
                    entity.install_native_stage_fixture(
                        crate::sim::stage::StageClass::from_native_fixture(
                            before["frame_f8"].as_i64().unwrap_or(0) as i32,
                            0,
                            crate::sim::timer::CdTimer::from_raw(word(0), word(2)),
                            word(3),
                            1,
                        ),
                    );
                    if let Some(words) = before["primary_facing_words"].as_array() {
                        entity.body_facing.snap(signed(&words[0]) as u16, 1);
                    }
                    entity.infantry.as_mut().unwrap().is_prone = before["prone_6db"] == 1;
                    entity.infantry.as_mut().unwrap().idle_action_timer = MissionTimer::armed(
                        signed(&before["idle_timer"][0]) as u32,
                        signed(&before["idle_timer"][2]) as u32,
                    );
                }
            }
            sim.substrate.entities.insert(entity);
        }
        if row["destination_input_readback"].is_object() {
            let before = &row["destination_input_readback"]["archive_object"];
            let archive = sim
                .substrate
                .entities
                .get_mut(pointers[setup["victim"].as_str().unwrap()])
                .unwrap();
            archive.mission.apply_test_fixture(MissionTestFixture {
                current: MissionId::from_raw(signed(&before["mission"])),
                suspended: MissionId::NONE,
                queued: MissionId::from_raw(signed(&before["queued"])),
                movement_bypass_latch: 0,
                handler_state: signed(&before["status"]) as u32,
                mission_start_frame: 0,
                ai_counter: signed(&before["visit"]) as u32,
                dispatch_timer: MissionDispatchTimer::from_raw(
                    signed(&before["dispatch"][0]),
                    signed(&before["dispatch"][1]),
                ),
            });
            archive.passive_scan_timer = MissionTimer::armed(
                signed(&before["targeting_timer"][0]) as u32,
                signed(&before["targeting_timer"][2]) as u32,
            );
            archive.on_bridge = before["on_bridge"] != 0;
            assert_eq!(before["nav"], "0x0");
            assert_eq!(before["target"], "0x0");
            assert_eq!(before["archive"], "0x0");
            assert_eq!(
                row["destination_input_readback"]["archive_locomotor"]["head"],
                json!([0, 0, 0])
            );
            assert_eq!(
                row["destination_input_readback"]["archive_locomotor"]["destination"],
                json!([0, 0, 0])
            );
        }
        // Cell membership comes from actual placement, not subsequently
        // supplied XYZ controls. Limbo controls deliberately retain Cell and
        // Display registration, just as the native oracle does.
        let logic: Vec<u64> = lists["logic"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pointer| pointers[pointer.as_str().unwrap()])
            .collect();
        for &id in &logic {
            let (rx, ry) = placed_cells[&id];
            let sub_cell = (sim.substrate.entities.get(id).unwrap().category
                == EntityCategory::Infantry)
                .then_some(2);
            sim.substrate.occupancy.add(
                rx,
                ry,
                id,
                MovementLayer::Ground,
                sub_cell,
                CellListInsertion::PrependNonBuilding,
            );
            sim.submit_object_display(id, DisplayLayer::GROUND, Some(rules));
        }
        sim.set_logic_order_for_test(logic);
        // The original row writes pose/layer controls AFTER Unlimbo. They do
        // not reinsert Display or relocate the native Cell lists. In
        // particular a NullCoord archive keeps its original Display slot.
        for (id, pose) in supplied_poses {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.position.rx = (pose[0] / 256) as u16;
            entity.position.ry = (pose[1] / 256) as u16;
            entity.position.sub_x = SimFixed::from_num(pose[0] % 256);
            entity.position.sub_y = SimFixed::from_num(pose[1] % 256);
            entity.position.exact_z_leptons = Some(pose[2]);
            if id == actor {
                entity.on_bridge = row["before"]["on_bridge"] != 0;
            }
        }
        let fixture = Self {
            sim,
            actor,
            pointers,
            cells,
        };
        for (actual, native_order, list) in [
            (
                fixture
                    .sim
                    .substrate
                    .entities
                    .values()
                    .map(GameEntity::stable_id)
                    .collect::<Vec<_>>(),
                &lists["techno"]["actors"],
                "Techno",
            ),
            (
                fixture.sim.logic_order().to_vec(),
                &lists["logic"]["actors"],
                "Logic",
            ),
            (
                fixture
                    .sim
                    .display_layers()
                    .members(DisplayLayer::GROUND)
                    .to_vec(),
                &lists["ground_display"]["actors"],
                "Ground Display",
            ),
        ] {
            let expected: Vec<u64> = native_order
                .as_array()
                .unwrap()
                .iter()
                .map(|pointer| fixture.id(pointer).unwrap())
                .collect();
            assert_eq!(actual, expected, "supplied {list} registration order");
        }
        assert_rng(
            &fixture.sim,
            &row["rng_before"],
            input["name"].as_str().unwrap(),
        );
        fixture
    }
}

fn assert_recorded_scores(fixture: &SuppliedFootFixture, rules: &RuleSet, row: &Value) -> usize {
    let name = row["input"]["name"].as_str().unwrap();
    let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
    let object = rules
        .object(fixture.sim.interner.resolve(actor.type_ref()))
        .unwrap();
    let coefficients = ThreatCoefficients::resolve(rules, object, HOUSE_SELECTS_OWN_COEFFICIENTS);
    let mut last_score = None;
    let mut compared = 0;
    for event in row["events"].as_array().unwrap() {
        if event["kind"] == "score" {
            last_score = Some(event);
        } else if event["kind"] == "score_before_vhp" {
            let score = last_score.take().unwrap();
            assert_eq!(
                score["caller"], "0x6f870b",
                "{name}: original Evaluate_Candidate score call"
            );
            assert_eq!(fixture.id(&score["this"]), Some(fixture.actor));
            let candidate = fixture
                .id(&Value::String(format!(
                    "0x{:x}",
                    score["args"][0].as_u64().unwrap()
                )))
                .unwrap();
            let actual = calculate_threat_score(
                &fixture.sim.substrate.entities,
                fixture.actor,
                candidate,
                rules,
                &fixture.sim.interner,
                None,
                Some(&fixture.sim.house_alliances),
                coefficients,
                ThreatReference::Coords(xyz(&score["anchor_xyz"])),
                None,
            )
            .unwrap();
            // Actual `_ftol` receipt at6F8710, before VHPScan and Evaluate's
            // integer modifiers/clamp. This distinguishes native [0,0,0]
            // from [0,0,1] even when the winning object is the same.
            assert_eq!(
                MaskedX87Chop53::ftol_i32_low_masked(actual),
                signed(&event["eax"]),
                "{name}: original pre-VHP score"
            );
            compared += 1;
        }
    }
    compared
}

#[test]
fn original_concrete_greatest_threat_matches_winner_latch_and_three_rng_streams() {
    let Some(rules) = legacy_rules("greatest_threat_rows") else {
        return;
    };
    let mut compared = 0;
    let mut scores_compared = 0;
    for row in oracle()["greatest_threat_rows"].as_array().unwrap() {
        let input = &row["input"];
        let scan = &input["greatest_threat"];
        if scan["entry"] != "concrete" {
            continue;
        }
        let name = input["name"].as_str().unwrap();
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        install_recorded_cell_coordinates(&mut fixture);
        scores_compared += assert_recorded_scores(&fixture, &rules, row);
        let expected = fixture.id(&Value::String(format!(
            "0x{:x}",
            row["returned_eax"].as_u64().unwrap()
        )));
        let mask = match signed(&scan["mask"]) {
            0 => ScanMission::Hunt,
            1 => ScanMission::Guard,
            2 => ScanMission::AreaGuard,
            mask => panic!("unrepresented native caller mask {mask}"),
        };
        assert_eq!(mask.literal_mask(), signed(&scan["mask"]) as u32);
        // Some([0,0,0]) remains the actual native NullCoord argument, not
        // None (which means this adapter should choose its own actor XYZ).
        let actual = fixture.sim.greatest_threat_represented(
            &rules,
            None,
            fixture.actor,
            mask,
            Some(xyz(&scan["point_xyz"])),
            greatest_threat_for_entity,
        );
        assert_eq!(actual, expected, "{name}: original class-dispatched winner");
        let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
        assert_eq!(
            actor.foot_retarget_after_stop(),
            row["after"]["scan"] != 0,
            "{name}: Foot+688"
        );
        assert_eq!(
            actor.mission.current().raw(),
            signed(&row["after"]["mission"]),
            "{name}: mission"
        );
        assert_eq!(
            actor.mission.handler_state(),
            signed(&row["after"]["status"]) as u32,
            "{name}: status"
        );
        assert_eq!(
            actor.mission.queued().raw(),
            signed(&row["after"]["queued"]),
            "{name}: queue"
        );
        assert_eq!(
            actor.mission.ai_counter(),
            signed(&row["after"]["visit"]) as u32,
            "{name}: visit counter"
        );
        assert_eq!(
            [
                actor.mission.dispatch_timer().start_frame(),
                actor.mission.dispatch_timer().delay()
            ],
            [
                signed(&row["after"]["dispatch"][0]),
                signed(&row["after"]["dispatch"][1])
            ],
            "{name}: dispatch timer"
        );
        assert_eq!(
            actor.archive_target(),
            fixture.id(&row["after"]["archive"]).map(TargetKind::Entity),
            "{name}: archive"
        );
        assert_eq!(
            actor.attack_target.as_ref().map(|target| target.target),
            fixture.id(&row["after"]["target"]).map(TargetKind::Entity),
            "{name}: TarCom is not assigned by Greatest_Threat"
        );
        assert_eq!(actor.navigation.nav_com, None, "{name}: NavCom");
        let position = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
        assert_eq!(
            [position.x, position.y, position.z],
            xyz(&row["after"]["position"]),
            "{name}: physical XYZ"
        );
        assert_eq!(
            actor.on_bridge,
            row["after"]["on_bridge"] != 0,
            "{name}: Object layer"
        );
        assert_eq!(
            actor.mission_leaf.foot_firing_sequence_latch(),
            signed(&row["after"]["firing"]) as u8,
            "{name}: Foot+68D firing latch"
        );
        if let Some(leaf) = actor.mission_leaf.as_infantry() {
            assert_eq!(
                leaf.doing(),
                signed(&row["after"]["doing"]),
                "{name}: Doing"
            );
            let timer = actor.infantry.unwrap().idle_action_timer;
            assert_eq!(
                [timer.start_frame as i32, timer.duration as i32],
                [
                    signed(&row["after"]["idle_timer"][0]),
                    signed(&row["after"]["idle_timer"][2])
                ],
                "{name}: idle timer"
            );
        }
        assert_eq!(
            [
                actor.passive_scan_timer.start_frame as i32,
                actor.passive_scan_timer.duration as i32
            ],
            [
                signed(&row["after"]["targeting_timer"][0]),
                signed(&row["after"]["targeting_timer"][2])
            ],
            "{name}: targeting timer"
        );
        assert_rng(&fixture.sim, &row["rng_after"], name);
        compared += 1;
    }
    assert_eq!(compared, 62);
    assert_eq!(scores_compared, 36);
}

#[test]
fn original_literal_foot_rows_compare_mask_and_empty_result_lifecycle_only() {
    let Some(rules) = legacy_rules("greatest_threat_rows") else {
        return;
    };
    let mut compared = 0;
    for row in oracle()["greatest_threat_rows"].as_array().unwrap() {
        if row["input"]["greatest_threat"]["entry"] != "foot" {
            continue;
        }
        let name = row["input"]["name"].as_str().unwrap();
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        let actor = fixture
            .sim
            .substrate
            .entities
            .get_mut(fixture.actor)
            .unwrap();
        let mask = row["greatest_threat_masks"]["foot_greatest"][0]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(
            actor.coerce_foot_threat_mask(mask),
            row["greatest_threat_masks"]["greatest"][0]
                .as_u64()
                .unwrap() as u32,
            "{name}: literal Foot mask"
        );
        // These original raw Foot calls lack the concrete class's category
        // bits and all return empty. Feed that native result to the existing
        // lifecycle owner, without claiming this executes the raw scan body.
        assert_eq!(row["returned_eax"], 0);
        actor.finish_foot_threat_scan(row["returned_eax"] != 0);
        assert_eq!(
            actor.foot_retarget_after_stop(),
            row["after"]["scan"] != 0,
            "{name}: empty-result clear"
        );
        assert_rng(&fixture.sim, &row["rng_after"], name);
        compared += 1;
    }
    assert_eq!(compared, 24);
}

#[test]
fn original_rules_key_controls_match_sequential_production_reads() {
    let receipt = &oracle()["rules_reader_receipts"];
    let mut layers = RulesLayerStack::new(IniFile::empty());
    let mut rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(rules_receipt(&rules), receipt["constructor"]["after"]);
    //665650 stores the IdleActionFrequency double at667574/66757E but does
    //not initialize+1724. The native canary stays supplied; Rust deliberately
    //starts it at deterministic zero instead of exposing allocation garbage.
    let retained = &receipt["constructor_retained_guard_control"];
    assert_eq!(
        retained["before"]["guard_mode_stray"],
        retained["after"]["guard_mode_stray"]
    );
    assert_ne!(
        rules.general.guard_mode_stray,
        signed(&retained["after"]["guard_mode_stray"])
    );
    for control in receipt["controls"].as_array().unwrap() {
        let name = control["name"].as_str().unwrap();
        assert_eq!(
            rules_receipt(&rules),
            control["before"],
            "{name}: current default"
        );
        layers.push(RulesLayerKind::Scenario, sections_ini(&control["sections"]));
        rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules_receipt(&rules),
            control["after"],
            "{name}: production read"
        );
    }
    assert_eq!(receipt["controls"].as_array().unwrap().len(), 10);
}

/// Adopt only the Cell+48 height/identity observations used by these handler
/// prefixes. These are native getter receipts, not values calculated by Rust.
/// Other terrain and whole-map path/zone initialization remain outside this
/// fixture; full home queries use the retail map fixture in a separate test.
pub(in crate::sim::world::techno_ai) fn install_recorded_cell_coordinates(
    fixture: &mut SuppliedFootFixture,
) {
    let stride = oracle()["world"]["zone_storage"]["stride"]
        .as_u64()
        .unwrap() as u16;
    let mut cells = (0..stride)
        .flat_map(|y| (0..stride).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y)))
        .collect::<Vec<_>>();
    for row in oracle()["rows"].as_array().unwrap() {
        for event in row["events"].as_array().unwrap() {
            if event["kind"] != "physical_getcoords" {
                continue;
            }
            let Some(&(x, y)) = fixture.cells.get(event["this"].as_str().unwrap()) else {
                continue;
            };
            let coord = xyz(&event["xyz"]);
            assert_eq!(
                [i32::from(x) * 256 + 128, i32::from(y) * 256 + 128],
                [coord[0], coord[1]]
            );
            // The observed Cell receivers are flat ground at level4;
            // this test does not fit or infer a slope from getter output.
            assert_eq!(coord[2], 416);
            cells[usize::from(y) * usize::from(stride) + usize::from(x)] =
                crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 4, false);
        }
    }
    // Original Cell486840 -> Unlimbo receipts independently establish the
    // ground under every real registered candidate. Missing this supplied
    // map state makes exact-Z416 appear airborne to5F5F40/+54, changing the
    // shared SelectWeapon/GetFireError owner before the scan can score it.
    let native = oracle();
    let source = native["greatest_threat_rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["family"] == "MTNK")
        .unwrap()["before"]["position"]
        .clone();
    let mut placements = vec![
        &source,
        &native["e1_placement"]["xyz"],
        &native["victim_placement"]["xyz"],
        &native["candidate_placement"]["xyz"],
    ];
    placements.extend(
        native["greatest_threat_registration"]["placements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|placement| &placement["xyz"]),
    );
    for placement in placements {
        let [x, y, z] = xyz(placement);
        assert_eq!(z, 416, "observed original flat Cell placement height");
        let (x, y) = ((x / 256) as u16, (y / 256) as u16);
        cells[usize::from(y) * usize::from(stride) + usize::from(x)] =
            crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 4, false);
    }
    let mut terrain =
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(stride, stride, cells);
    terrain.test_set_native_allocated_cells(&fixture.cells.values().copied().collect::<Vec<_>>());
    fixture.sim.install_resolved_terrain_for_new_map(terrain);
}

/// Use the production retail loader for the physical map and TMP owners,
/// then adopt the native harness's deliberately cropped allocation and raw
/// zone/query inputs. This does not certify either topology initialization
/// or the full map: the original harness supplied these rows after setup.
fn install_recorded_anytown_query_state(
    fixture: &mut SuppliedFootFixture,
    scene: &crate::headless_scenario::HeadlessScenario,
    row: &Value,
) {
    let name = row["input"]["name"].as_str().unwrap();
    let source = scene.sim();
    fixture.sim.playfield_bounds = source.playfield_bounds;
    fixture.sim.playfield_size_height = source.playfield_size_height;
    fixture.sim.session.map_width = source.session.map_width;
    fixture.sim.session.map_height = source.session.map_height;
    fixture.sim.overlay_grid = source.overlay_grid.clone();
    let mut terrain = source.resolved_terrain.as_ref().unwrap().clone();
    terrain.test_set_native_allocated_cells(&fixture.cells.values().copied().collect::<Vec<_>>());
    let mut observed = BTreeMap::new();
    for event in row["events"].as_array().unwrap() {
        if event["resolved_cell"].is_object() {
            let cell = &event["resolved_cell"];
            let pointer = cell["pointer"].as_str().unwrap();
            if let Some(previous) = observed.insert(pointer, cell) {
                assert_eq!(previous, cell, "{name}: unchanged native Cell receiver");
            }
        }
    }
    for (pointer, receipt) in &observed {
        let (x, y) = fixture.cells[*pointer];
        assert_eq!(
            json!([x, y]),
            receipt["cell"],
            "{name}: original Cell constructor identity"
        );
        let cell = terrain.cell_mut(x, y).unwrap();
        // Physical map/TMP data establishes these independently of Rust's
        // query result. Flags and Land are the retained native crop state,
        // rather than a claim about every production Cell Recalc writer.
        assert_eq!(
            i32::from(cell.level),
            signed(&receipt["level"]),
            "{name}: level"
        );
        assert_eq!(
            i32::from(cell.slope_type),
            signed(&receipt["slope"]),
            "{name}: physical slope"
        );
        cell.yr_cell_land_type = signed(&receipt["land"]) as u8;
        let identity = terrain.native_cell_identity((x as i16, y as i16));
        terrain.write_native_cell_flags(identity, receipt["flags"].as_u64().unwrap() as u32);
    }
    fixture.sim.install_resolved_terrain_for_new_map(terrain);
    let terrain = fixture.sim.resolved_terrain.as_ref().unwrap();
    let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(terrain);
    let mut zones = crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
        &path,
        terrain,
        &[],
        fixture.sim.map_size_diamond(),
        fixture.sim.playfield_bounds,
    );
    // Initial7 is replaced by the native fixture's supplied crop values.
    // The actual original56D230 receipts here observe2 at both source and
    // destination. Adopt through the existing raw-zone owner, not a new scan.
    for event in row["events"].as_array().unwrap() {
        if event["kind"] == "zone" {
            assert_eq!(event["result_eax"], 2, "{name}: raw crop zone premise");
        }
    }
    zones.test_supply_uniform_raw_zone_rows(2);
    fixture.sim.zone_grid = Some(zones);
    fixture.sim.install_fixture_path_grid(Some(&path));
    if row["house_return"].is_object() {
        let state = &row["house_return"];
        let bounds = fixture.sim.playfield_bounds.unwrap();
        assert_eq!(
            json!([
                bounds.base,
                fixture.sim.playfield_size_height.unwrap(),
                bounds.off_fc,
                bounds.off_100,
                bounds.off_104,
                bounds.off_108,
            ]),
            state["map_size_words"],
            "{name}: physical retail Map/LocalSize"
        );
        let terrain = fixture.sim.resolved_terrain.as_ref().unwrap();
        let dummy = &state["before_dummy"];
        terrain
            .stamp_dummy_cell_requested_coord(signed(&dummy["cell"][0]), signed(&dummy["cell"][1]));
        terrain.test_set_dummy_cell_level_slope(
            signed(&dummy["level"]) as i8,
            signed(&dummy["slope"]) as u8,
        );
        let shared = terrain.shared_cell_dummy();
        shared.write_raw_flags(dummy["flags"].as_u64().unwrap() as u32);
        shared.test_set_land_type(signed(&dummy["land"]));
        let owner = fixture
            .sim
            .substrate
            .entities
            .get(fixture.actor)
            .unwrap()
            .owner();
        let house = fixture.sim.houses.get_mut(&owner).unwrap();
        let input = &row["input"]["house_return_control"];
        house.set_base_geometry_for_test(
            Some((
                signed(&input["primary"][0]) as u16,
                signed(&input["primary"][1]) as u16,
            )),
            signed(&input["radius"]),
        );
        house.alternate_base_center = (
            signed(&input["alternate"][0]) as u16,
            signed(&input["alternate"][1]) as u16,
        );
    }
}

pub(in crate::sim::world::techno_ai) fn assert_foot_projection(
    fixture: &SuppliedFootFixture,
    row: &Value,
) {
    let name = row["input"]["name"].as_str().unwrap();
    let expected = &row["after"];
    let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
    assert_eq!(
        actor.mission.current().raw(),
        signed(&expected["mission"]),
        "{name}: mission"
    );
    assert_eq!(
        actor.mission.queued().raw(),
        signed(&expected["queued"]),
        "{name}: queued"
    );
    assert_eq!(
        actor.mission.handler_state(),
        signed(&expected["status"]) as u32,
        "{name}: status"
    );
    assert_eq!(
        actor.mission.ai_counter(),
        signed(&expected["visit"]) as u32,
        "{name}: visit"
    );
    assert_eq!(
        actor.archive_target(),
        fixture.target(&expected["archive"]),
        "{name}: archive"
    );
    assert_eq!(
        actor.attack_target.as_ref().map(|target| target.target),
        fixture.target(&expected["target"]),
        "{name}: TarCom"
    );
    assert_eq!(
        actor.navigation.nav_com,
        fixture.nav(&expected["nav"]),
        "{name}: NavCom"
    );
    assert_eq!(
        actor.foot_retarget_after_stop(),
        expected["scan"] != 0,
        "{name}: Foot+688"
    );
    assert_eq!(
        [
            actor.mission.dispatch_timer().start_frame(),
            actor.mission.dispatch_timer().delay()
        ],
        [
            signed(&expected["dispatch"][0]),
            signed(&expected["dispatch"][1])
        ],
        "{name}: dispatch timer"
    );
    assert_eq!(
        [
            actor.passive_scan_timer.start_frame as i32,
            actor.passive_scan_timer.duration as i32
        ],
        [
            signed(&expected["targeting_timer"][0]),
            signed(&expected["targeting_timer"][2])
        ],
        "{name}: passive targeting timer"
    );
    let position = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
    assert_eq!(
        [position.x, position.y, position.z],
        xyz(&expected["position"]),
        "{name}: physical XYZ"
    );
    assert_eq!(
        actor.on_bridge,
        expected["on_bridge"] != 0,
        "{name}: OnBridge"
    );
    assert_eq!(
        actor.mission_leaf.foot_firing_sequence_latch(),
        signed(&expected["firing"]) as u8,
        "{name}: Foot+68D firing"
    );
    if let Some(leaf) = actor.mission_leaf.as_infantry() {
        assert_eq!(leaf.doing(), signed(&expected["doing"]), "{name}: Doing");
        let timer = actor.infantry.as_ref().unwrap().idle_action_timer;
        assert_eq!(
            [timer.start_frame as i32, timer.duration as i32],
            [
                signed(&expected["idle_timer"][0]),
                signed(&expected["idle_timer"][2])
            ],
            "{name}: idle timer"
        );
        if expected["frame_f8"].is_number() {
            assert_eq!(
                actor.native_stage().value(),
                signed(&expected["frame_f8"]),
                "{name}: action frame"
            );
        }
        if expected["primary_facing_words"].is_array() {
            assert_eq!(
                actor.body_facing_current(1),
                signed(&expected["primary_facing_words"][0]) as u16,
                "{name}: primary facing"
            );
        }
        if expected["walk_moving"].is_object() {
            assert_eq!(
                crate::sim::movement::motion_query::is_moving(actor),
                Some(expected["walk_moving"]["value"] != 0),
                "{name}: actual Walk IsMoving"
            );
        }
    }
    assert_rng(&fixture.sim, &row["rng_after"], name);
}

fn assert_destination_projection(fixture: &SuppliedFootFixture, row: &Value) {
    let name = row["input"]["name"].as_str().unwrap();
    let expected = &row["after"];
    let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
    assert_eq!(
        actor.navigation.nav_com_aux,
        fixture.nav(&expected["aux"]),
        "{name}: Foot NavComAux"
    );
    assert_eq!(
        actor.navigation.path_replay.cursor, 0,
        "{name}: unchanged supplied path cursor"
    );
    assert_eq!(
        actor.navigation.path_replay.directions,
        expected["path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|direction| signed(direction) as u8)
            .collect::<Vec<_>>(),
        "{name}: live path head and preserved backing suffix"
    );
    assert_eq!(
        actor.navigation.path_replay.reference_cell,
        Some((
            signed(&expected["reference_cell"][0]) as i16,
            signed(&expected["reference_cell"][1]) as i16,
        )),
        "{name}: retained Foot reference Cell"
    );
    let queue_count = expected["nav_queue"]["count"].as_u64().unwrap() as usize;
    assert_eq!(
        actor.navigation.nav_queue,
        expected["nav_queue"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .take(queue_count)
            .map(|pointer| fixture.nav(pointer).unwrap())
            .collect::<Vec<_>>(),
        "{name}: Unit clear versus Infantry preserved navigation vector"
    );
    // Compare live vector entries, not C++ allocator/vtable/backing residues.
    let path = &actor.navigation.path_runtime;
    for (actual, key) in [
        (path.movement_timer, "movement_timer_words"),
        (path.blocked_timer, "blocked_timer_words"),
    ] {
        assert_eq!(
            [actual.start_frame(), actual.duration()],
            [signed(&expected[key][0]), signed(&expected[key][2])],
            "{name}: {key} authoritative start/duration"
        );
    }
    assert_eq!(
        path.path_blocked,
        expected["blocked"] != 0,
        "{name}: blocked"
    );
    assert_eq!(
        path.retries_left,
        signed(&expected["retry"]) as u32,
        "{name}: retries"
    );
    let (destination, head) = if row["input"]["family"] == "MTNK" {
        let loco = actor
            .locomotor
            .as_ref()
            .and_then(|l| l.selected_drive_runtime())
            .and_then(|r| r.retained())
            .unwrap();
        (loco.destination(), loco.head_to())
    } else {
        let loco = actor.locomotor.as_ref().unwrap();
        (loco.walk_destination(), loco.step_head())
    };
    let coord = |coord: Option<DriveCoord>| coord.map_or([0, 0, 0], |c| [c.x, c.y, c.z]);
    assert_eq!(
        coord(destination),
        xyz(&expected["locomotor"]["destination"]),
        "{name}: exact target+4C Move_To coordinates"
    );
    assert_eq!(
        coord(head),
        xyz(&expected["locomotor"]["head"]),
        "{name}: retained locomotor paid head"
    );
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(actor),
        Some(expected["locomotor"]["is_moving_al"] != 0),
        "{name}: class IsMoving after Move_To"
    );
    assert_eq!(
        actor.body_facing_current(1),
        signed(&expected["body_facing_words"][0]) as u16,
        "{name}: unchanged primary facing"
    );
    if row["house_return"].is_object() {
        let terrain = fixture.sim.resolved_terrain.as_ref().unwrap();
        let dummy = terrain.shared_cell_dummy();
        let snapshot = dummy.snapshot();
        assert_eq!(
            json!({
                "cell": [snapshot.coord.0, snapshot.coord.1],
                "flags": dummy.raw_flags(),
                "land": dummy.land_type(),
                "level": snapshot.level,
                "slope": snapshot.slope_type,
            }),
            json!({
                "cell": row["house_return"]["after_dummy"]["cell"],
                "flags": row["house_return"]["after_dummy"]["flags"],
                "land": row["house_return"]["after_dummy"]["land"],
                "level": row["house_return"]["after_dummy"]["level"],
                "slope": row["house_return"]["after_dummy"]["slope"],
            }),
            "{name}: shared dummy identity retains native query state"
        );
    }
}

fn run_recorded_handler(
    fixture: &mut SuppliedFootFixture,
    rules: &RuleSet,
    row: &Value,
    ctx: super::super::ObjectAiCtx<'_>,
) {
    let name = row["input"]["name"].as_str().unwrap();
    for event in row["events"].as_array().unwrap() {
        if event["kind"] == "distance" && fixture.id(&event["this"]) == Some(fixture.actor) {
            let pointer = Value::String(format!("0x{:x}", event["args"][0].as_u64().unwrap()));
            let target = fixture.target(&pointer).unwrap();
            assert_eq!(
                super::foot_distance_to_target(&fixture.sim, fixture.actor, target, rules),
                Some(signed(&event["result_eax"])),
                "{name}: original physical distance"
            );
        }
    }
    if row["input"]["dispatch_entry"] == true {
        super::dispatch_foot_mission(&mut fixture.sim, fixture.actor, rules, ctx);
    } else {
        let actual = if row["input"]["mission"] == 21 {
            super::evaluate_foot_rescue(&mut fixture.sim, fixture.actor, rules, ctx)
        } else {
            assert_eq!(
                row["input"]["mission"], 11,
                "{name}: recorded handler class"
            );
            super::evaluate_foot_area_guard(&mut fixture.sim, fixture.actor, rules, ctx)
        };
        assert_eq!(
            actual.delay,
            signed(&row["returned_eax"]),
            "{name}: original handler return, NavCom {:?}, moving {:?}",
            fixture
                .sim
                .substrate
                .entities
                .get(fixture.actor)
                .unwrap()
                .navigation
                .nav_com,
            crate::sim::movement::motion_query::is_moving(
                fixture.sim.substrate.entities.get(fixture.actor).unwrap()
            )
        );
    }
    assert_foot_projection(fixture, row);
}

#[test]
fn original_rescue_and_area_guard_complete_represented_handler_rows() {
    let Some(rules) = legacy_rules("rows") else {
        return;
    };
    let mut compared = 0;
    let mut approach_excluded = 0;
    let mut prefix_excluded = 0;
    let mut supplied_scan_excluded = 0;
    for row in oracle()["rows"].as_array().unwrap() {
        if row["events"].as_array().unwrap().iter().any(|event| {
            matches!(
                event["kind"].as_str(),
                Some("foot_approach" | "infantry_approach")
            )
        }) {
            approach_excluded += 1;
            continue;
        }
        if row["stop"] != "0x30000000" {
            // These original controls intentionally stop before House500200.
            // A complete Rust handler cannot be compared at that prefix alone.
            assert_eq!(row["stop"], "0x4de12d");
            prefix_excluded += 1;
            continue;
        }
        if !row["input"]["supplied_scanner_result"].is_null() {
            // This original control replaces Greatest_Threat's return at its
            // caller boundary. A live production scanner would introduce a
            // different legality/geometry premise, so this receiver test
            // cannot certify that isolated admission control.
            supplied_scan_excluded += 1;
            continue;
        }
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        install_recorded_cell_coordinates(&mut fixture);
        run_recorded_handler(
            &mut fixture,
            &rules,
            row,
            super::super::ObjectAiCtx::default(),
        );
        compared += 1;
    }
    assert_eq!(
        (
            compared,
            approach_excluded,
            prefix_excluded,
            supplied_scan_excluded
        ),
        (60, 4, 12, 6)
    );
}

#[test]
fn original_physical_retail_area_guard_idle_runs_before_cadence() {
    let Some(rules) = legacy_rules("retail_idle_rows") else {
        return;
    };
    let mut compared = 0;
    for row in oracle()["retail_idle_rows"].as_array().unwrap() {
        if row["input"]["idle_control"]["entry"] != "area_guard" {
            continue;
        }
        assert_eq!(
            hex_double(rules.general.idle_action_frequency),
            row["input"]["idle_control"]["idle_action_frequency_bits"]
        );
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        install_recorded_cell_coordinates(&mut fixture);
        run_recorded_handler(
            &mut fixture,
            &rules,
            row,
            super::super::ObjectAiCtx::default(),
        );
        compared += 1;
    }
    // The other28 direct admission/RNG controls live at their receiver in
    // sim::infantry; these6 add the native synchronous handler ordering.
    assert_eq!(compared, 6);
}

fn assert_full_retail_row_context(rules: &RuleSet, row: &Value) {
    assert_eq!(
        row["input"]["reader_context"],
        "weapon_reader_receipts.after"
    );
    let context = row["input"]["rules_context"].as_str().unwrap();
    assert_eq!(
        rules_receipt(rules),
        oracle()["rules_reader_receipts"][context],
        "new original rows declare their actual AudioVisual reader chronology"
    );
}

#[test]
fn original_full_retail_cell_leash_matches_move_idle_and_cadence_boundaries() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut compared = 0;
    for row in oracle()["retail_weapon_rows"].as_array().unwrap() {
        if row["input"]["mission"] != 11 {
            continue;
        }
        assert_full_retail_row_context(&rules, row);
        assert_eq!(row["stop"], "0x30000000");
        assert!(row["input"]["supplied_scanner_result"].is_null());
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        install_recorded_cell_coordinates(&mut fixture);
        run_recorded_handler(
            &mut fixture,
            &rules,
            row,
            super::super::ObjectAiCtx::default(),
        );
        compared += 1;
    }
    // The source points deliberately lie outside the supplied 180-Cell crop.
    // The recorded physical Cell receiver, Null/Dummy fallback and original
    // strict-distance boundary are premises, not a whole-map loading claim.
    assert_eq!(compared, 7);
}

#[test]
fn original_full_retail_rescue_matches_native_scan_hit_and_empty_prefix() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut complete_hits = 0;
    let mut empty_prefixes = 0;
    let mut scores_compared = 0;
    for row in oracle()["retail_weapon_rows"].as_array().unwrap() {
        if row["input"]["mission"] != 21 || !row["input"]["supplied_scanner_result"].is_null() {
            continue;
        }
        let name = row["input"]["name"].as_str().unwrap();
        assert_full_retail_row_context(&rules, row);
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        install_recorded_cell_coordinates(&mut fixture);
        scores_compared += assert_recorded_scores(&fixture, &rules, row);
        if row["stop"] == "0x30000000" {
            run_recorded_handler(
                &mut fixture,
                &rules,
                row,
                super::super::ObjectAiCtx::default(),
            );
            assert_destination_projection(&fixture, row);
            complete_hits += 1;
        } else {
            assert_eq!(row["stop"], "0x4de12d");
            let scan = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["kind"] == "infantry_greatest")
                .unwrap();
            assert_eq!(scan["caller"], "0x4de05c");
            assert_eq!(scan["args"][0], ScanMission::Hunt.literal_mask());
            assert_eq!(scan["result_eax"], 0);
            //4DE030 clears688 before the concrete scan. Compare its existing
            //lifecycle owner and actual-point scanner boundary; this original
            //row stops before House500200, so no home/cadence suffix is claimed.
            fixture
                .sim
                .substrate
                .entities
                .get_mut(fixture.actor)
                .unwrap()
                .clear_rescue_retarget_latch();
            let actual = fixture.sim.greatest_threat_represented(
                &rules,
                None,
                fixture.actor,
                ScanMission::Hunt,
                Some(xyz(&scan["anchor_xyz"])),
                greatest_threat_for_entity,
            );
            assert_eq!(actual, None, "{name}: original concrete scan return");
            assert_foot_projection(&fixture, row);
            assert_destination_projection(&fixture, row);
            empty_prefixes += 1;
        }
    }
    assert_eq!((complete_hits, empty_prefixes, scores_compared), (1, 1, 1));
}

#[test]
fn original_full_retail_supplied_rescue_gates_compare_distance_and_range_prerequisites() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut compared = 0;
    for row in oracle()["retail_weapon_rows"].as_array().unwrap() {
        if row["input"]["supplied_scanner_result"].is_null() {
            continue;
        }
        let name = row["input"]["name"].as_str().unwrap();
        assert_full_retail_row_context(&rules, row);
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        install_recorded_cell_coordinates(&mut fixture);
        let distance = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "math" && event["pc"] == "0x4de0db")
            .unwrap();
        assert_eq!(
            super::foot_distance_to_target(
                &fixture.sim,
                fixture.id(&row["before"]["archive"]).unwrap(),
                fixture
                    .target(&row["input"]["supplied_scanner_result"])
                    .unwrap(),
                &rules,
            ),
            Some(signed(&distance["eax"])),
            "{name}: original inline rescue distance at4DE0DB"
        );
        let range = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "threat_range")
            .unwrap();
        assert_eq!(range["caller"], "0x4de0f3");
        assert_eq!(range["args"][0], 1);
        assert_eq!(
            super::foot_threat_range(&fixture.sim, fixture.actor, &rules),
            signed(&range["result_eax"]),
            "{name}: original live7012C0/707E60 range"
        );
        assert_rng(&fixture.sim, &row["rng_after"], name);
        compared += 1;
    }
    // These controls substitute the original scan return at its caller. This
    // API exposes the live scanner, not a supplied-pick handler seam. Compare
    // the two original numeric prerequisites only; assignment/refusal in the
    // isolated3839/3840/3841 controls is not certified by this test.
    assert_eq!(compared, 3);
}

#[test]
fn original_retail_foot_stray_and_empty_rescue_home_complete_destination_transaction() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map_name = std::env::var("VERA20K_ANYTOWN_MAP").unwrap_or_else(|_| "XMP03T4.MAP".into());
    let map = crate::map::source::load_map_by_name_or_path_with_assets(&root, &map_name, &assets)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&map.source).unwrap()["source_sha256"],
        oracle()["world"]["physical_map_sha256"],
        "physical native AnyTown map bytes"
    );
    drop(map);
    drop(assets);
    let scene = crate::sim::world::bridge_test_evidence::load_anytown_concrete();
    let Some(rules) = legacy_rules("navigation_rows") else {
        return;
    };
    assert_eq!(
        oracle()["weapon_reader_receipts"]["reader_contexts"]["navigation_rows"],
        oracle()["weapon_reader_receipts"]["reader_contexts"]["empty_rescue_rows"],
        "the four appended legacy destination rows share their reader history"
    );
    let mut compared = 0;
    let mut cell_receivers = std::collections::BTreeSet::new();
    for row in oracle()["navigation_rows"]
        .as_array()
        .unwrap()
        .iter()
        .chain(oracle()["empty_rescue_rows"].as_array().unwrap())
    {
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        assert_eq!(
            rules.general.blockage_path_delay_ticks,
            signed(&row["destination_input_readback"]["rules_repath_delay"]),
            "physical native RepathDelay input"
        );
        install_recorded_anytown_query_state(&mut fixture, &scene, row);
        for event in row["events"].as_array().unwrap() {
            if event["resolved_cell"].is_object() {
                cell_receivers.insert(event["resolved_cell"]["pointer"].as_str().unwrap());
            }
        }
        run_recorded_handler(
            &mut fixture,
            &rules,
            row,
            super::super::ObjectAiCtx {
                overlay_registry: Some(&scene.runtime.resources.overlay_registry),
                ..Default::default()
            },
        );
        assert_destination_projection(&fixture, row);
        compared += 1;
    }
    assert_eq!(compared, 4);
    assert_eq!(
        cell_receivers.len(),
        13,
        "bounded original Cell query receivers"
    );
}

#[test]
fn original_initial_stationary_e1_action_enters_ready_without_rng() {
    let Some(rules) = legacy_rules("initial_action_receipt") else {
        return;
    };
    let receipt = &oracle()["initial_action_receipt"];
    let mut row = oracle()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "E1_area_guard_post")
        .unwrap()
        .clone();
    // The sequencer receipt has no mission-handler effects. Keep the
    // supplied mission/Nav/archive/timers unchanged across this direct call.
    row["after"] = row["before"].clone();
    for phase in ["before", "after"] {
        for key in [
            "doing",
            "frame_f8",
            "mission",
            "queued",
            "on_bridge",
            "position",
        ] {
            row[phase][key] = receipt[phase][key].clone();
        }
        row[phase]["primary_facing_words"] = receipt[phase]["body_facing_words"].clone();
    }
    row["input"]["name"] = json!("E1_initial_stationary_action");
    row["rng_before"] = receipt["rng_before"].clone();
    row["rng_after"] = receipt["rng_after"].clone();
    let mut fixture = SuppliedFootFixture::new(&row, &rules);
    assert_eq!(receipt["constructor"]["doing"], -1);
    assert_eq!(receipt["entry"], "0x00520AE0");
    assert_eq!(receipt["before"]["walk_moving"], 0);
    assert!(!fixture.sim.infantry_sequencer(fixture.actor, &rules));
    assert_foot_projection(&fixture, &row);
}

#[test]
fn original_ground_fireup_stage_uses_absolute_native_frames() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let base = oracle()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "E1_area_guard_post")
        .unwrap();
    let mut compared = 0;
    for case in oracle()["ground_firing_receipt"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            case["input"]["expiry_after_completed_ai_calls"].is_null()
                && !case["frames"].as_array().unwrap().is_empty()
        })
    {
        let name = case["input"]["name"].as_str().unwrap();
        let frames = case["frames"].as_array().unwrap();
        let mut row = base.clone();
        row["before"]["doing"] = json!(0);
        row["before"]["frame_f8"] = json!(0);
        let mut fixture = SuppliedFootFixture::new(&row, &rules);
        fixture.sim.session.binary_frame = frames[0]["native_frame"].as_u64().unwrap() as u32;
        fixture
            .sim
            .substrate
            .entities
            .get_mut(fixture.actor)
            .unwrap()
            .on_bridge = case["input"]["source_on_bridge"] != 0;
        let rng = (
            fixture.sim.scenario_rng.logical_state(),
            fixture.sim.main_rng.logical_state(),
            fixture.sim.mapgen_rng.logical_state(),
        );
        // Original whole51BAB0 observations transport only the stage boundary
        // here: real DoAction51D6F0 starts FireUp after that visit's Techno step.
        // This direct-owner regression excludes Mission cadence/firing/launch;
        // it does not substitute a whole native AI or shot comparison.
        assert!(
            fixture
                .sim
                .infantry_do_action(fixture.actor, 4, false, &rules)
                .unwrap()
        );
        for (index, frame) in frames.iter().enumerate() {
            fixture.sim.session.binary_frame = frame["native_frame"].as_u64().unwrap() as u32;
            if index != 0 {
                fixture
                    .sim
                    .substrate
                    .entities
                    .get_mut(fixture.actor)
                    .unwrap()
                    .tick_native_stage(fixture.sim.session.binary_frame as i32);
            }
            let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
            assert_eq!(
                actor.native_stage().value(),
                signed(&frame["after"]["frame_f8"]),
                "{name}: native absolute frame {}",
                fixture.sim.session.binary_frame
            );
        }
        assert_eq!(
            (
                fixture.sim.scenario_rng.logical_state(),
                fixture.sim.main_rng.logical_state(),
                fixture.sim.mapgen_rng.logical_state(),
            ),
            rng,
            "DoAction(random0) and the Stage primitive draw no RNG"
        );
        compared += 1;
    }
    assert_eq!(compared, 4);
}

#[test]
fn original_idle_completion_releases_doing_at_existing_completion_owner() {
    let Some(rules) = legacy_rules("retail_idle_rows") else {
        return;
    };
    let mut compared = 0;
    for row in oracle()["retail_idle_rows"].as_array().unwrap() {
        if row["input"]["idle_control"]["entry"] != "completion" {
            continue;
        }
        let mut fixture = SuppliedFootFixture::new(row, &rules);
        let action = signed(&row["before"]["doing"]);
        assert_eq!(action, signed(&row["input"]["idle_control"]["doing"]));
        assert!(
            matches!(action, 9 | 10),
            "original Idle1/Idle2 action bytes"
        );
        // The supplied native +F8/Doing enter the production sequencer.
        // It owns completion admission, facing and the new action together.
        assert!(!fixture.sim.infantry_sequencer(fixture.actor, &rules));
        assert_foot_projection(&fixture, row);
        compared += 1;
    }
    assert_eq!(compared, 2);
}

fn native_clock_fixture(sample: &Value) -> crate::sim::stage::StageClass {
    crate::sim::stage::StageClass::from_native_fixture(
        signed(&sample["value"]),
        sample["changed"].as_u64().unwrap() as u8,
        crate::sim::timer::CdTimer::from_raw(
            signed(&sample["timer"]["start"]),
            signed(&sample["timer"]["duration"]),
        ),
        signed(&sample["rate"]),
        signed(&sample["increment"]),
    )
}

/// Original scalar instruction blocks only, with caller state declared by
/// the corpus. This establishes numeric clock behavior, not whole class AI.
#[test]
fn native_stage_clock_matches_original_techno_and_building_blocks() {
    let corpus = oracle();
    let receipt = &corpus["stage_clock_receipt"];
    assert_eq!(receipt["native_text_unchanged"], true);
    assert_eq!(receipt["native_vtables_unchanged"], true);
    let rows = receipt["clock_rows"].as_array().unwrap();
    let mut visits = 0;
    for row in rows {
        let input = &row["input"];
        let family = input["family"].as_str().unwrap();
        let name = input["name"].as_str().unwrap();
        assert!(matches!(family, "techno" | "building"));
        let frames = row["frames"].as_array().unwrap();
        let mut stage = native_clock_fixture(&frames[0]["before"]);
        for frame in frames {
            let at = format!("{family}/{name}: {}", frame["native_frame"]);
            assert_eq!(
                stage,
                native_clock_fixture(&frame["before"]),
                "{at}: prior state"
            );
            stage.advance(signed(&frame["native_frame"]));
            assert_eq!(
                stage,
                native_clock_fixture(&frame["after"]),
                "{at}: original clock outputs"
            );
            assert_eq!(
                frame["rng_before"], frame["rng_after"],
                "{at}: full three native RNG states"
            );
            assert!(
                frame["callback_events"].as_array().unwrap().is_empty(),
                "{at}: no replaced class callback"
            );
            visits += 1;
        }
    }
    assert_eq!((rows.len(), visits), (36, 58));
}

/// Infantry51BC9F calls Foot4DA539/Techno6F9E50 even for a retained Die1
/// sequence. This projects its shared clock against the executed scalar
/// Stage block; it does not certify the remainder of the dying AI subset.
#[test]
fn dying_infantry_visits_shared_native_clock() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let corpus = oracle();
    let base = corpus["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "E1_area_guard_post")
        .unwrap();
    let clock = corpus["stage_clock_receipt"]["clock_rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["input"]["family"] == "techno" && row["input"]["name"] == "running_repeated_and_gap"
        })
        .unwrap();
    let frame = &clock["frames"][0];
    let mut fixture = SuppliedFootFixture::new(base, &rules);
    fixture.sim.session.binary_frame = signed(&frame["native_frame"]) as u32;
    fixture.sim.begin_infantry_death_sequence(
        fixture.actor,
        crate::sim::world::infantry_terminal::InfantryDeathSequence::Die1,
        &rules,
    );
    let actor = fixture
        .sim
        .substrate
        .entities
        .get_mut(fixture.actor)
        .unwrap();
    actor.health.current = 0;
    actor.install_native_stage_fixture(native_clock_fixture(&frame["before"]));
    fixture.sim.object_ai_visit_one(
        fixture.actor,
        Some(&rules),
        super::super::ObjectAiCtx::default(),
    );
    assert_eq!(
        *fixture
            .sim
            .substrate
            .entities
            .get(fixture.actor)
            .unwrap()
            .native_stage(),
        native_clock_fixture(&frame["after"]),
        "retained Die1 shares the executed Techno stage clock"
    );
}

/// The production save envelope must retain the native clock's full signed
/// state. Resuming each saved visit compares against the next original block
/// execution, including paused, negative, wrapping and repeated-frame inputs.
#[test]
fn saved_native_stage_resumes_original_clock_and_changes_world_hash() {
    use crate::sim::game_entity::GameEntity;
    use crate::sim::snapshot::GameSnapshot;
    let corpus = oracle();
    let rows = corpus["stage_clock_receipt"]["clock_rows"]
        .as_array()
        .unwrap();
    let mut compared = 0;
    for row in rows {
        let mut sim = Simulation::new();
        assert_eq!(sim.allocate_stable_id(), 1);
        let mut actor = GameEntity::test_default(1, "CLOCK", "Americans", 2, 2);
        let frames = row["frames"].as_array().unwrap();
        actor.install_native_stage_fixture(native_clock_fixture(&frames[0]["before"]));
        sim.substrate.entities.insert(actor);
        for frame in frames {
            let name = row["input"]["name"].as_str().unwrap();
            let at = format!("{name}: {}", frame["native_frame"]);
            let before = native_clock_fixture(&frame["before"]);
            let after = native_clock_fixture(&frame["after"]);
            let before_hash = sim.state_hash();
            let bytes = GameSnapshot::save_validated(&sim, 0, 0, &at, 0);
            sim = GameSnapshot::load_validated(&bytes, 0, 0, &sim.session.map_name)
                .unwrap()
                .sim;
            assert_eq!(
                *sim.substrate.entities.get(1).unwrap().native_stage(),
                before,
                "{at}: every retained clock field survives the save"
            );
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .tick_native_stage(signed(&frame["native_frame"]));
            assert_eq!(
                *sim.substrate.entities.get(1).unwrap().native_stage(),
                after,
                "{at}: resumed visit matches original execution"
            );
            assert_eq!(
                sim.state_hash() == before_hash,
                before == after,
                "{at}: changing the native clock changes the world checksum"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 58);
}

#[test]
fn original_do_action_restart_preserves_independent_stage_fields() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let corpus = oracle();
    let base = corpus["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "E1_area_guard_post")
        .unwrap();
    let rows = corpus["stage_clock_receipt"]["action_restart_rows"]
        .as_array()
        .unwrap();
    for native in rows {
        let name = native["input"]["name"].as_str().unwrap();
        let mut row = base.clone();
        row["before"]["doing"] = native["doing_before"].clone();
        let mut fixture = SuppliedFootFixture::new(&row, &rules);
        fixture.sim.session.binary_frame = signed(&native["input"]["native_frame"]) as u32;
        fixture
            .sim
            .substrate
            .entities
            .get_mut(fixture.actor)
            .unwrap()
            .install_native_stage_fixture(native_clock_fixture(&native["before"]));
        let before_rng = (
            fixture.sim.scenario_rng.logical_state(),
            fixture.sim.main_rng.logical_state(),
            fixture.sim.mapgen_rng.logical_state(),
        );
        let args = &native["input"]["args"];
        assert_eq!(args[2], 0, "{name}: original random-first-stage argument");
        let accepted = fixture
            .sim
            .infantry_do_action(fixture.actor, signed(&args[0]), args[1] != 0, &rules)
            .unwrap();
        assert_eq!(
            u8::from(accepted),
            native["returned_al"].as_u64().unwrap() as u8,
            "{name}: actual AL"
        );
        let actor = fixture.sim.substrate.entities.get(fixture.actor).unwrap();
        assert_eq!(
            actor.mission_leaf.as_infantry().unwrap().doing(),
            signed(&native["doing_after"]),
            "{name}: Doing"
        );
        assert_eq!(
            *actor.native_stage(),
            native_clock_fixture(&native["after"]),
            "{name}: all retained clock fields"
        );
        assert_eq!(
            native["rng_before"], native["rng_after"],
            "{name}: original full RNG states"
        );
        assert_eq!(
            (
                fixture.sim.scenario_rng.logical_state(),
                fixture.sim.main_rng.logical_state(),
                fixture.sim.mapgen_rng.logical_state()
            ),
            before_rng,
            "{name}: Rust action consumes no RNG"
        );
    }
    assert_eq!(rows.len(), 2);
}
