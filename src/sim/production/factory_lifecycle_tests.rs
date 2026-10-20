//! Production lifecycle scenarios: registry decisions must finish their held
//! object graph, accounting and successor work before returning to the caller.

use super::tests::spawn_structure;
use super::{
    ProductionCategory, cancel_production_for_owner, dispatch_production_changes_for_tests,
    enqueue_by_type,
};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{intern::InternedId, rng::SimRng, world::Simulation};

/// Begin4FA41C clears the local category prediction gate before a failed
/// FindFactory. The resulting presentation fact must not depend on a real
/// Factory, nor be published for a foreign House. The strip request itself
/// belongs to the client and is tested there.
#[test]
fn refused_begin_still_publishes_the_local_prediction_gate_transition() {
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{FrameEffects, LifecycleOutput};

    let rules = super::tests::basic_infantry_rules();
    let mut sim = Simulation::with_seed(0x4FA4_1C00);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 1000, 10));
    sim.session.set_current_house_for_test(Some(owner));
    let before_wallet = sim.houses[&owner].economy().clone();
    assert!(!enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        FrameEffects::default(),
        false
    ));
    assert!(
        matches!(sim.lifecycle_outputs.as_slice(), [LifecycleOutput::ProductionBeginAttempted {
        owner: reached, category: ProductionCategory::Infantry, naval: false,
    }] if *reached == owner)
    );
    assert!(sim.production.factories().is_empty());
    assert!(sim.entities().is_empty());
    assert_eq!(*sim.houses[&owner].economy(), before_wallet);

    sim.lifecycle_outputs.clear();
    sim.session.set_current_house_for_test(None);
    assert!(!enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        FrameEffects::default(),
        false
    ));
    assert!(sim.lifecycle_outputs.is_empty());
}

/// Ordinary paid DEST from an already admitted Allied base. The water patch,
/// completed buildings, running Idle animation and wallet are scene priors; payment, held identity,
/// PLACE, producer missions and animation retirement all use the frame API.
/// Original House4FB563 -> Building443C60/444187 and Unload44D880 ->
/// Production8 expiry44E8F0 establish this lifecycle (gamemd SHA2561cdd).
#[test]
fn paid_destroyer_runs_shipyard_production_animation_through_idle_recovery() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::mission::MissionType;
    use crate::sim::runtime::{SimResources, SimRuntime};
    use crate::sim::world::{FrameEffects, TickLane};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
    else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = retail.rules;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").expect("retail temperate assets");
    rules
        .bind_scheduler_anim_assets(
            &["GAYARD_A".into(), "GAYARD_C".into(), "GAYARD_D".into()],
            &assets,
            "tem",
            "TEMPERATE",
        )
        .expect("production animation bounds come from physical retail SHPs");
    let mut sim = Simulation::with_seed(0x4441_7500);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let water = super::tests::water_terrain(64, 64);
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for y in 24..48 {
        for x in 24..48 {
            *terrain.cell_mut(x, y).unwrap() = water.cell(x, y).unwrap().clone();
        }
    }
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(owner), true, 10_000, 10);
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    let yard = 3;
    for (id, name, x, y) in [
        (1, "GACNST", 5, 10),
        (2, "GAPOWR", 14, 10),
        (yard, "GAYARD", 30, 30),
    ] {
        spawn_structure(&mut sim, &rules, id, "Americans", name, x, y);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(rules.object(name).unwrap().strength);
        sim.remove_entity_occupancy(id);
        super::tests::stamp_type_foundation(&mut sim, &rules, id);
        sim.add_entity_occupancy(id);
        assert!(sim.register_live_object(id));
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    // The raw admitted-building fixture skips map-load GrandOpening. Supply
    // its already-running Idle through the real slot/Anim constructor owner;
    // no production animation, dispatch/completion or payment is armed here.
    sim.set_building_anim_slot(yard, 18, false, false, 0, &rules)
        .expect("the completed shipyard's physical Idle asset is bound");
    let destroyer = sim.interner.get("DEST").unwrap();
    let cost = sim.cost_of(owner, rules.object("DEST").unwrap(), &rules);
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            ..SimResources::empty()
        },
    };
    for _ in 0..8 {
        runtime
            .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
            .expect("settle the admitted shipyard through ordinary visits");
    }
    let sim = &runtime.simulation;
    assert!(
        super::build_options_for_owner(sim, &runtime.resources.rules, "Americans")
            .iter()
            .any(|option| option.type_id == destroyer && option.enabled),
        "the actual retail prerequisite owners admit DEST before the order"
    );
    let idle = sim
        .substrate
        .entities
        .get(yard)
        .unwrap()
        .building_art
        .anim_slots()[18]
        .expect("stock idle animation is live before production");
    assert_eq!(
        sim.interner.resolve(sim.anim(idle).unwrap().type_id),
        "GAYARD_A"
    );
    let mut product = None;
    let mut production = None;
    let mut delivered = false;
    let mut recovered = false;
    for step in 1..=2_000 {
        let commands = if step == 1 {
            vec![CommandEnvelope::new(
                owner,
                1,
                Command::QueueProduction { type_id: destroyer },
            )]
        } else {
            Vec::new()
        };
        runtime
            .advance_frame(&commands, 67, TickLane::Ordinary, FrameEffects::default())
            .expect("ordinary paid ship production frame");
        let sim = &runtime.simulation;
        let id = *product.get_or_insert_with(|| held_id(sim, owner, ProductionCategory::Ship));
        let ship = sim
            .substrate
            .entities
            .get(id)
            .unwrap_or_else(|| panic!("the original paid DEST{id} must survive step{step}"));
        assert_eq!(ship.type_ref(), destroyer);
        let producer = sim.substrate.entities.get(yard).unwrap();
        if !delivered && !ship.lifecycle.in_limbo() {
            delivered = true;
            assert!(ship.lifecycle.cell_marked());
            assert_eq!(producer.mission.current().known(), Some(MissionType::Guard));
            assert_eq!(producer.mission.queued().known(), Some(MissionType::Unload));
            assert!(producer.radio_contacts.is_empty());
            assert_eq!(ship.tether.partner(), None);
            assert_eq!(sim.houses[&owner].economy().spent_credits(), cost);
            assert_eq!(sim.houses[&owner].economy().credits(), 10_000 - cost);
        }
        if let Some(anim) = producer.building_art.anim_slots()[8] {
            let retained = *production.get_or_insert(anim);
            assert_eq!(
                anim, retained,
                "one Production animation per delivered ship"
            );
            assert_eq!(
                sim.interner.resolve(sim.anim(anim).unwrap().type_id),
                "GAYARD_D"
            );
            assert!(producer.building_art.anim_slots()[18].is_none());
            assert!(
                sim.anim(idle).is_none(),
                "the original Idle was scalar-deleted"
            );
        } else if let Some(production) = production {
            let restored = producer.building_art.anim_slots()[18]
                .expect("normal Production expiry restores Idle");
            assert_ne!(restored, idle);
            assert_eq!(
                sim.interner.resolve(sim.anim(restored).unwrap().type_id),
                "GAYARD_A"
            );
            assert!(
                sim.anim(production).is_none(),
                "expired Production is retired"
            );
            assert_eq!(producer.mission.current().known(), Some(MissionType::Guard));
            assert!(producer.radio_contacts.is_empty());
            recovered = true;
            break;
        }
    }
    assert!(
        delivered && production.is_some() && recovered,
        "paid identity must reach Unload, live Production and natural Idle recovery"
    );
}

pub(super) fn manager_rules() -> RuleSet {
    RuleSet::from_ini(&manager_ini()).expect("factory manager fixture")
}

fn manager_ini() -> IniFile {
    IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n[InfantryTypes]\n0=E1\n1=SLAV\n\
         [VehicleTypes]\n0=MTNK\n1=SMIN\n\
         [AircraftTypes]\n0=HORN\n1=ORCA\n\
         [BuildingTypes]\n0=GAPILE\n1=GAWEAP\n2=GACNST\n3=YAREFN\n4=GATECH\n5=GAAIRC\n6=GAPOWR\n\
         [E1]\nName=GI\nCost=200\nStrength=100\nArmor=flak\nSpeed=4\nSight=5\nTechLevel=1\nOwner=Americans\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [SLAV]\nStrength=125\nSpeed=4\nStorage=4\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [MTNK]\nName=Tank\nCost=700\nStrength=300\nArmor=heavy\nSpeed=6\nSight=6\nTechLevel=1\nOwner=Americans\nSpawns=HORN\nSpawnsNumber=3\nSpawnRegenRate=600\nSpawnReloadRate=25\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [SMIN]\nCost=900\nStrength=2000\nSpeed=3\nTechLevel=1\nOwner=Americans\nPrerequisite=GATECH\nEnslaves=SLAV\nSlavesNumber=2\nSlaveRegenRate=500\nSlaveReloadRate=25\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [HORN]\nStrength=75\nSpeed=14\nAmmo=1\n\
         [ORCA]\nCost=1000\nStrength=200\nSpeed=8\nTechLevel=1\nOwner=Americans\n\
         [GAPILE]\nOwner=Americans\nFactory=InfantryType\n\
         [GAWEAP]\nOwner=Americans\nFactory=UnitType\n\
         [GACNST]\nOwner=Americans\nFactory=BuildingType\nConstructionYard=yes\n\
         [YAREFN]\nCost=1000\nStrength=2000\nFoundation=1x1\nTechLevel=1\nOwner=Americans\nEnslaves=SLAV\nSlavesNumber=2\nSlaveRegenRate=500\nSlaveReloadRate=25\n\
         [GAPOWR]\nCost=800\nStrength=500\nFoundation=1x1\nTechLevel=1\nOwner=Americans\n\
         [GATECH]\nStrength=500\n\
         [GAAIRC]\nOwner=Americans\nFactory=AircraftType\nHelipad=yes\n",
    )
}

fn world(seed: u64) -> (Simulation, RuleSet, InternedId) {
    world_with(manager_rules(), seed)
}

fn world_with_creation_cue(seed: u64) -> (Simulation, RuleSet, InternedId) {
    let mut ini = manager_ini();
    ini.merge(&IniFile::from_str(
        "[SoundList]\n0=FixtureCreated\n[FixtureCreated]\nSounds=fixture\n\
         [E1]\nCreateSound=FixtureCreated\n",
    ));
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&ini);
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds);
    world_with(rules, seed)
}

fn world_with(rules: RuleSet, seed: u64) -> (Simulation, RuleSet, InternedId) {
    let mut sim = Simulation::with_seed(seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 50_000, 10,
        ));
    spawn_structure(&mut sim, &rules, 1, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, &rules, 2, "Americans", "GAPILE", 14, 10);
    spawn_structure(&mut sim, &rules, 3, "Americans", "GACNST", 18, 10);
    spawn_structure(&mut sim, &rules, 4, "Americans", "GATECH", 22, 10);
    (sim, rules, owner)
}

/// Original Record_Last_Built4FB6B0 controls supply actor/type layouts and
/// existing Counter capacity as priors. This compares its four last-type slots,
/// the existing aggregate built total and all three complete RNG states.
/// Place_Production, CreateSound, House+246/timer and a full production world
/// are outside this comparison.
#[test]
fn record_last_built_matches_native_class_slots_score_gate_and_rng() {
    use crate::rules::object_type::ObjectCategory;
    use crate::sim::house_state::HouseState;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_build_type_events.json",
    ))
    .expect("original built-type controls");
    let native = &corpus["record_last_built"];
    assert_eq!(
        native["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    let rows = native["controls"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let categories = [
        (ObjectCategory::Building, "building"),
        (ObjectCategory::Vehicle, "vehicle"),
        (ObjectCategory::Infantry, "infantry"),
        (ObjectCategory::Aircraft, "aircraft"),
    ];
    let built_total = |state: &serde_json::Value| {
        state["built_counters"]
            .as_object()
            .unwrap()
            .values()
            .map(|counter| counter["total"].as_u64().unwrap())
            .sum::<u64>()
    };
    for dont_score in [false, true] {
        // Names are explicit corpus constructor inputs. Apply DontScore through
        // the actual retail startup/mode/map reader, rather than mutating types.
        let patch = categories
            .into_iter()
            .map(|(_, name)| {
                let type_name = native["type_constructor_defaults"][name]["name"]
                    .as_str()
                    .unwrap();
                format!(
                    "[{type_name}]\nDontScore={}\n",
                    if dont_score { "yes" } else { "no" }
                )
            })
            .collect::<String>();
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
            &IniFile::from_str(&patch),
        ) else {
            return;
        };
        let rules = &retail.rules;
        for (category, name) in categories {
            let row = rows
                .iter()
                .find(|row| {
                    row["category"].as_str() == Some(name)
                        && row["dont_score"].as_bool() == Some(dont_score)
                })
                .expect("one native control for each category and DontScore value");
            let type_prior = &native["type_constructor_defaults"][name];
            let type_name = type_prior["name"].as_str().unwrap();
            let native_type_index =
                i32::try_from(row["native_type_index"].as_i64().unwrap()).unwrap();
            assert_eq!(type_prior["array_index"], row["native_type_index"]);
            let ty = rules.object(type_name).unwrap();
            assert_eq!(ty.category, category);
            assert_eq!(ty.dont_score, dont_score);
            assert_eq!(
                rules.type_array_index(category, type_name),
                Some(native_type_index)
            );

            let mut sim = Simulation::with_seed(0);
            sim.intern_rule_type_ids(rules);
            sim.resolve_type_handles(rules);
            let owner = sim.interner.intern("Americans");
            sim.houses
                .register(HouseState::new(owner, 0, None, true, 0, 10));
            let product = sim
                .construct_object_limbo_at_height(
                    type_name,
                    "Americans",
                    0,
                    0,
                    0,
                    0,
                    rules,
                    crate::sim::world::FrameEffects::default(),
                )
                .expect("the comparison's supplied actor/type prior");
            let object = super::factory_lifecycle::BuiltObject::capture(&sim, product);
            for (slot, slot_name) in categories {
                assert_eq!(
                    i64::from(sim.houses[&owner].last_built_type_index(slot)),
                    row["before"]["last_built"][slot_name].as_i64().unwrap(),
                    "{name}, DontScore={dont_score}, before {slot_name}"
                );
            }
            assert_eq!(
                u64::from(sim.houses[&owner].stats().built()),
                built_total(&row["before"])
            );
            let before = &row["observation"]["rng_before"];
            sim.scenario_rng =
                SimRng::from_native_state_hex_for_test(before["scenario"].as_str().unwrap());
            sim.main_rng =
                SimRng::from_native_state_hex_for_test(before["main"].as_str().unwrap()).into();
            sim.mapgen_rng =
                SimRng::from_native_state_hex_for_test(before["mapgen"].as_str().unwrap());

            super::factory_lifecycle::record_last_built(&mut sim, rules, owner, object);

            for (slot, slot_name) in categories {
                assert_eq!(
                    i64::from(sim.houses[&owner].last_built_type_index(slot)),
                    row["after"]["last_built"][slot_name].as_i64().unwrap(),
                    "{name}, DontScore={dont_score}, after {slot_name}"
                );
            }
            assert_eq!(
                u64::from(sim.houses[&owner].stats().built()),
                built_total(&row["after"]),
                "{name}, DontScore={dont_score}"
            );
            let after = &row["observation"]["rng_after"];
            for (stream, state) in [
                ("scenario", sim.scenario_rng.native_state_hex()),
                ("main", sim.main_rng.native_state_hex()),
                ("mapgen", sim.mapgen_rng.native_state_hex()),
            ] {
                assert_eq!(
                    state,
                    after[stream].as_str().unwrap(),
                    "{name}, DontScore={dont_score}, {stream} RNG"
                );
            }
        }
    }
}

#[test]
fn record_last_built_enforces_map_authored_negative_limit() {
    use crate::sim::house_state::HouseState;

    // Original4FB755 increments the Infantry/E2 produced item; the signed
    // CanBuild4F808F..4F80C8 comparison then returns No, not AtLimit. This
    // supported map override is deliberately different from stock E2 rules.
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &IniFile::from_str("[E2]\nBuildLimit=-1\nDontScore=no\n"),
    ) else {
        return;
    };
    let rules = &retail.rules;
    let e2 = rules.object("E2").expect("retail Conscript");
    assert_eq!(e2.build_limit, -1);
    assert!(!e2.dont_score);
    let mut sim = Simulation::with_seed(0x4FB6_B000);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(owner, 1, None, true, 1000, 10));
    assert_eq!(
        super::can_build::can_build(&sim, rules, owner, e2, true, true),
        super::can_build::CanBuild::Yes
    );
    assert!(!super::can_build::check_build_limit(&sim, rules, owner, e2));

    let product = sim
        .construct_object_limbo_at_height(
            "E2",
            "Russians",
            0,
            0,
            0,
            0,
            rules,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("supplied valid product for the shared Record call");
    let object = super::factory_lifecycle::BuiltObject::capture(&sim, product);
    super::factory_lifecycle::record_last_built(&mut sim, rules, owner, object);
    assert_eq!(sim.houses[&owner].stats().built(), 1);
    assert_eq!(
        super::can_build::can_build(&sim, rules, owner, e2, true, true),
        super::can_build::CanBuild::No,
        "a successful produced item consumes the lifetime limit"
    );
    assert!(super::can_build::check_build_limit(&sim, rules, owner, e2));

    // The existing ownership and destruction owners move current tracking,
    // never this House's successful-production history. In particular the
    // recipient's owned E2 does not consume its own negative lifetime limit.
    let other = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(other, 0, None, true, 1000, 10));
    sim.change_owner_with_rules(
        product,
        other,
        rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(sim.entities().get(product).unwrap().owner(), other);
    assert_eq!(
        super::can_build::can_build(&sim, rules, owner, e2, true, true),
        super::can_build::CanBuild::No
    );
    assert_eq!(
        super::can_build::can_build(&sim, rules, other, e2, true, true),
        super::can_build::CanBuild::Yes
    );
    sim.discard_constructed_limbo(
        product,
        Some(rules),
        crate::sim::world::FrameEffects::default(),
    );
    assert!(sim.entities().get(product).is_none());
    assert_eq!(sim.houses[&owner].stats().built(), 1);
    assert_eq!(sim.houses[&other].stats().built(), 0);
    assert_eq!(
        super::can_build::can_build(&sim, rules, owner, e2, true, true),
        super::can_build::CanBuild::No
    );
}

/// Direct RecordLastBuilt control-flow regression, separate from successful
/// delivery. Original4FB6B0 admits even limbo/nonhuman products; DontScore
/// gates accounting but not the type override/class fallback or1FC writer.
#[test]
fn record_last_built_uses_class_fallbacks_type_overrides_and_the_existing_tech_owner() {
    use crate::rules::sound_ini::SoundRegistry;
    use crate::sim::world::{FrameEffects, SimSoundEvent};

    let mut ini = manager_ini();
    ini.merge(&IniFile::from_str(
        "[SoundList]\n0=TypeCue\n1=UnitCue\n2=InfantryCue\n3=AircraftCue\n\
         [TypeCue]\nSounds=type\n[UnitCue]\nSounds=unit\n\
         [InfantryCue]\nSounds=infantry\n[AircraftCue]\nSounds=aircraft\n\
         [AudioVisual]\nCreateUnitSound=UnitCue\nCreateInfantrySound=InfantryCue\n\
         CreateAircraftSound=AircraftCue\n[E1]\nCreateSound=TypeCue\nDontScore=yes\n\
         [GAPOWR]\nCreateSound=TypeCue\n",
    ));
    let sounds = SoundRegistry::from_ini(&ini);
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds);
    for (name, expected) in [
        ("MTNK", Some("UnitCue")),
        ("SLAV", Some("InfantryCue")),
        ("ORCA", Some("AircraftCue")),
        ("E1", Some("TypeCue")),
        ("GAPOWR", Some("TypeCue")),
        ("GACNST", None),
    ] {
        let (mut sim, _, owner) = world_with(rules.clone(), 0x4FB7_FF00);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_is_human_for_test(false);
        assert!(sim.session.current_house().is_none());
        let product = sim
            .construct_object_limbo_at_height(
                name,
                "Americans",
                7,
                9,
                0,
                2,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(
            sim.substrate
                .entities
                .get(product)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
        let object = super::factory_lifecycle::BuiltObject::capture(&sim, product);
        sim.sound_events.clear();
        let house = sim.houses.get_mut(&owner).unwrap();
        house.take_tech_recheck();
        assert!(!house.tech_recheck_pending());
        super::factory_lifecycle::record_last_built(&mut sim, &rules, owner, object);
        let actual = sim
            .sound_events
            .iter()
            .map(|event| {
                let SimSoundEvent::VocAt {
                    sound_id,
                    audible_to,
                    ..
                } = event
                else {
                    panic!("unexpected RecordLastBuilt output {event:?}");
                };
                assert!(audible_to.is_none());
                sound_id.as_str()
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected.into_iter().collect::<Vec<_>>(), "{name}");
        assert_eq!(sim.houses[&owner].stats().built(), u32::from(name != "E1"));
        assert!(
            sim.houses[&owner].tech_recheck_pending(),
            "native4FB804, {name}"
        );
    }
}

/// A stock, settled barracks prior for ordinary paid production and SELL.
/// The real theater SHP binder supplies sale control; this fixture never
/// arms a Factory, completion flag or BuildingDown stage.
pub(super) fn retail_barracks_sale_runtime(
    second_barracks: bool,
    radar: bool,
    initial_credits: i32,
) -> Option<(crate::sim::runtime::SimRuntime, InternedId, u64)> {
    use crate::sim::house_state::HouseState;
    use crate::sim::runtime::{SimResources, SimRuntime};

    let mut rules =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?.rules;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE");
    rules.bind_building_buildup_assets(&assets, "TEMPERATE");
    assert!(rules.has_buildup("NAHAND"));
    let mut sim = Simulation::with_seed(0x5091_4000);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let owner = sim.interner.intern("Russians");
    let mut house = HouseState::new(owner, 0, Some(owner), true, initial_credits, 10);
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    let mut buildings = vec![("NACNST", 5, 10), ("NAHAND", 14, 10), ("NAPOWR", 23, 10)];
    if radar {
        buildings.push(("NARADR", 30, 10));
    }
    if second_barracks {
        buildings.push(("NAHAND", 14, 20));
    }
    for (index, (name, x, y)) in buildings.into_iter().enumerate() {
        let id = index as u64 + 1;
        spawn_structure(&mut sim, &rules, id, "Russians", name, x, y);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(rules.object(name).unwrap().strength);
        sim.remove_entity_occupancy(id);
        super::tests::stamp_type_foundation(&mut sim, &rules, id);
        sim.add_entity_occupancy(id);
        assert!(
            sim.register_live_object(id),
            "the supplied live building takes ordinary visits"
        );
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    Some((
        SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                ..SimResources::empty()
            },
        },
        owner,
        2,
    ))
}

fn paid_infantry_with_backup() -> Option<(crate::sim::runtime::SimRuntime, InternedId, u64, u64)> {
    use crate::sim::world::{FrameEffects, TickLane};
    let (mut runtime, owner, barracks) = retail_barracks_sale_runtime(true, false, 50_000)?;
    for _ in 0..8 {
        runtime
            .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
    }
    for _ in 0..2 {
        assert!(enqueue_by_type(
            &mut runtime.simulation,
            &runtime.resources.rules,
            "Russians",
            "E2",
            FrameEffects::default(),
            false,
        ));
    }
    for _ in 0..100 {
        if runtime.simulation.houses[&owner].economy().spent_credits() > 0 {
            break;
        }
        runtime
            .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
    }
    let sim = &runtime.simulation;
    let backup = sim
        .entities()
        .iter_sorted()
        .find(|(id, entity)| *id != barracks && sim.interner.resolve(entity.type_ref()) == "NAHAND")
        .map(|(id, _)| id)
        .unwrap();
    Some((runtime, owner, barracks, backup))
}

/// House509240..50924D keeps the paid object and FIFO when a factory still
/// exists, but suspends it if none is online. The Temporal state here is a
/// supplied callback-boundary prior; the app capture exercises its CLEG source.
#[test]
fn offline_backup_preserves_paid_infantry_with_system_hold() {
    use crate::sim::temporal::TemporalState;
    let Some((mut runtime, owner, barracks, backup)) = paid_infantry_with_backup() else {
        return;
    };
    let sim = &mut runtime.simulation;
    sim.entities_mut()
        .get_mut(backup)
        .unwrap()
        .set_temporal_for_test(TemporalState::warped_by_for_test(backup));
    let category = ProductionCategory::Infantry;
    let before = sim.production.factories().view(owner, category).unwrap();
    assert!(before.is_running());
    let progress = before.progress;
    assert!(progress > 0 && progress < super::PRODUCTION_STEPS);
    let product = before.object.unwrap().entity_id;
    let order = before.construction_order();
    let wallet = sim.houses[&owner].economy().credits();
    let spent = sim.houses[&owner].economy().spent_credits();

    sim.techno_limbo_with_rules(barracks, &runtime.resources.rules, None);

    let held = sim.production.factories().view(owner, category).unwrap();
    assert!(
        !held.is_running(),
        "native50924D suspends the paid head when only an offline backup remains"
    );
    assert_eq!(held.progress, progress);
    assert_eq!(held.object.unwrap().entity_id, product);
    assert_eq!(held.construction_order(), order);
    assert_eq!(held.queue.len(), 1);
    for offset in 1..=12 {
        let sim = &mut runtime.simulation;
        sim.production
            .factories_mut()
            .step_all(&mut sim.houses, sim.session.binary_frame() + offset);
    }
    let sim = &runtime.simulation;
    let held = sim.production.factories().view(owner, category).unwrap();
    assert_eq!(held.progress, progress);
    assert_eq!(held.object.unwrap().entity_id, product);
    assert_eq!(sim.houses[&owner].economy().credits(), wallet);
    assert_eq!(sim.houses[&owner].economy().spent_credits(), spent);
}

/// A real Temporal owner supplies the online transition, but not a queue
/// callback. Capture449284 supplies later automatic recovery. Save/load must
/// retain +71 so the otherwise identical user hold refuses that recovery.
#[test]
fn offline_backup_recovery_distinguishes_saved_system_and_user_holds() {
    use crate::sim::house_state::HouseState;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::{FrameEffects, SnapshotProcessState};

    let mut hold_hashes = Vec::new();
    for user_hold in [false, true] {
        let Some((mut runtime, owner, barracks, backup)) = paid_infantry_with_backup() else {
            return;
        };
        let rules = &runtime.resources.rules;
        let sim = &mut runtime.simulation;
        let other = sim.interner.intern("Opponent");
        let mut house = HouseState::new(other, 1, Some(owner), false, 50_000, 10);
        house.project_country_mults(rules, &sim.interner);
        sim.houses.register(house);
        assert!(sim.houses.order().contains(&owner));
        let attacker = sim
            .spawn_object("CLEG", "Opponent", 12, 20, 0, rules)
            .unwrap();
        let captured = sim
            .spawn_object("NAHAND", "Opponent", 28, 20, 0, rules)
            .unwrap();
        if user_hold {
            assert!(super::suspend_production(
                sim,
                "Russians",
                ProductionCategory::Infantry
            ));
        }
        sim.temporal_initiate_warp(attacker, Some(backup), rules, None, FrameEffects::default());
        assert!(!sim.entities().get(backup).unwrap().building_online());
        sim.techno_limbo_with_rules(barracks, rules, None);
        sim.temporal_let_go(attacker);
        assert!(sim.entities().get(backup).unwrap().building_online());
        let held = sim
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        assert!(
            !held.is_running(),
            "release alone supplies no native queue callback"
        );
        let product = held.object.unwrap().entity_id;
        let progress = held.progress;
        let order = held.construction_order();
        let credits = sim.houses[&owner].economy().credits();
        let spent = sim.houses[&owner].economy().spent_credits();

        // Match Scenario load's native reseed while retaining process RNG and
        // exercising the production save/restore owner, not raw native SAV.
        sim.scenario_rng = SimRng::new(0);
        hold_hashes.push(sim.state_hash());
        let bytes = GameSnapshot::save(sim, 0, rules.simulation_config_hash(), "offline-backup", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.retain_process_state_for_load(SnapshotProcessState::Live(sim));
        restored.restore_after_snapshot_load().unwrap();
        restored.resolve_type_handles(rules);
        restored.rebuild_caches_after_load(
            sim.resolved_terrain.as_ref().unwrap().clone(),
            sim.terrain_speed_config.clone(),
            rules,
        );
        assert!(restored.rebuild_dynamic_navigation(rules));
        assert_eq!(restored.state_hash(), sim.state_hash());

        for branch in [&mut *sim, &mut restored] {
            branch.change_owner_with_rules(captured, owner, rules, None, FrameEffects::default());
            let factory = branch
                .production
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .unwrap();
            assert_eq!(
                factory.is_running(),
                !user_hold,
                "only a system hold auto-recovers"
            );
            assert_eq!(factory.progress, progress);
            assert_eq!(factory.object.unwrap().entity_id, product);
            assert_eq!(factory.construction_order(), order);
            assert_eq!(factory.queue.len(), 1);
            assert_eq!(branch.houses[&owner].economy().credits(), credits);
            assert_eq!(branch.houses[&owner].economy().spent_credits(), spent);
            if user_hold {
                assert!(enqueue_by_type(
                    branch,
                    rules,
                    "Russians",
                    "E2",
                    FrameEffects::default(),
                    false
                ));
                let factory = branch
                    .production
                    .factories()
                    .view(owner, ProductionCategory::Infantry)
                    .unwrap();
                assert!(
                    factory.is_running(),
                    "ordinary same-type selection resumes the original head"
                );
                assert_eq!(factory.object.unwrap().entity_id, product);
                assert_eq!(factory.queue.len(), 1);
            }
        }
        assert_eq!(restored.state_hash(), sim.state_hash());
    }
    assert_ne!(
        hold_hashes[0], hold_hashes[1],
        "automatic-recovery eligibility is simulation state"
    );
}

/// SellBack44717D/44718F changes mission only. Isolated stage0 has no
/// UpdateFactoryQueue509140 or House1FC request; native loss cleanup starts
/// in the completing Limbo445E14. Protect the first subsequent Factory sweep
/// before any crew/reverse-buildup work could trigger another mechanism.
#[test]
fn selling_last_retail_barracks_keeps_paid_head_before_map_exit() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::mission::MissionType;
    use crate::sim::world::{FrameEffects, TickLane};

    for successor in ["E2", "FLAKT"] {
        let Some((mut runtime, owner, barracks)) =
            retail_barracks_sale_runtime(false, successor == "FLAKT", 50_000)
        else {
            return;
        };
        for _ in 0..8 {
            runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
        }
        assert!(!runtime.simulation.houses[&owner].tech_recheck_pending());
        let conscript = runtime.simulation.interner.get("E2").unwrap();
        assert!(
            super::build_options_for_owner(
                &runtime.simulation,
                &runtime.resources.rules,
                "Russians"
            )
            .iter()
            .any(|option| option.type_id == conscript && option.enabled)
        );
        let frame = u64::from(runtime.simulation.session.binary_frame()) + 1;
        let successor = runtime.simulation.interner.get(successor).unwrap();
        assert!(
            super::build_options_for_owner(
                &runtime.simulation,
                &runtime.resources.rules,
                "Russians"
            )
            .iter()
            .any(|option| option.type_id == successor && option.enabled)
        );
        let commands = [conscript, successor].map(|type_id| {
            CommandEnvelope::new(owner, frame, Command::QueueProduction { type_id })
        });
        runtime
            .advance_frame(&commands, 67, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        for _ in 0..100 {
            if runtime.simulation.houses[&owner].economy().spent_credits() > 0 {
                break;
            }
            runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
        }
        assert!(runtime.simulation.houses[&owner].economy().spent_credits() > 0);
        let product = held_id(&runtime.simulation, owner, ProductionCategory::Infantry);
        let factory = runtime
            .simulation
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        assert!(factory.progress < super::PRODUCTION_STEPS);
        assert_eq!(factory.queue.len(), 1);
        assert!(!runtime.simulation.houses[&owner].tech_recheck_pending());
        assert!(super::can_sell_building(
            &runtime.simulation,
            &runtime.resources.rules,
            barracks,
        ));
        let frame = u64::from(runtime.simulation.session.binary_frame()) + 1;
        runtime
            .advance_frame(
                &[CommandEnvelope::new(
                    owner,
                    frame,
                    Command::SellBuilding {
                        entity_id: barracks,
                    },
                )],
                67,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        assert_eq!(
            runtime
                .simulation
                .entities()
                .get(barracks)
                .unwrap()
                .mission
                .effective()
                .known(),
            Some(MissionType::Selling),
            "the ordinary SELL was accepted"
        );
        // Native Scenario load deliberately reseeds this stream to zero. Give
        // both branches the same explicit pre-save state, then exercise the live
        // process retention/restore seam used by the app, not native SAV parsing.
        runtime.simulation.scenario_rng = SimRng::new(0);
        let rules = &runtime.resources.rules;
        let bytes = crate::sim::snapshot::GameSnapshot::save(
            &runtime.simulation,
            0,
            rules.simulation_config_hash(),
            "barracks-sale",
            0,
        );
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(
            &runtime.simulation,
        ));
        restored.restore_after_snapshot_load().unwrap();
        restored.resolve_type_handles(rules);
        restored.rebuild_caches_after_load(
            runtime
                .simulation
                .resolved_terrain
                .as_ref()
                .unwrap()
                .clone(),
            runtime.simulation.terrain_speed_config.clone(),
            rules,
        );
        assert!(restored.rebuild_dynamic_navigation(rules));
        assert_eq!(
            runtime.simulation.hash_owner_diagnostics_for_test(),
            restored.hash_owner_diagnostics_for_test(),
            "mid-sale snapshot owners"
        );
        assert_eq!(
            restored.state_hash(),
            runtime.simulation.state_hash(),
            "mid-sale snapshot"
        );
        let mut restored = crate::sim::runtime::SimRuntime {
            simulation: restored,
            resources: crate::sim::runtime::SimResources {
                rules: rules.clone(),
                ..crate::sim::runtime::SimResources::empty()
            },
        };
        let spent_at_sale = runtime.simulation.houses[&owner].economy().spent_credits();
        let mut sale_visits = 0;
        for _ in 0..2048 {
            runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            restored
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
            assert_eq!(
                runtime.simulation.state_hash(),
                restored.simulation.state_hash(),
                "sale continuation"
            );
            let on_map = runtime
                .simulation
                .entities()
                .get(barracks)
                .is_some_and(|entity| !entity.lifecycle.in_limbo());
            if !on_map {
                break;
            }
            sale_visits += 1;
            let factory = runtime
                .simulation
                .production
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .expect(
                    "native has not run the queue-loss callback while sale only changed mission",
                );
            assert_eq!(factory.object.unwrap().entity_id, Some(product));
            assert_eq!(factory.queue.len(), 1);
            assert!(
                runtime
                    .simulation
                    .entities()
                    .get(product)
                    .unwrap()
                    .lifecycle
                    .in_limbo()
            );
        }
        assert!(sale_visits > 1, "real Buildup and stage1 crew were visited");
        assert!(
            runtime.simulation.houses[&owner].economy().spent_credits() > spent_at_sale,
            "the paid factory keeps charging during reverse buildup"
        );
        assert!(
            !runtime.simulation.entities().contains(barracks),
            "ordinary sale completed"
        );
        assert!(
            !runtime.simulation.entities().contains(product),
            "held product scalar destruction"
        );
        assert!(
            runtime
                .simulation
                .production
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .is_none()
        );
        // Original stockNAHAND451330/70ADA0 control: one crew and refund250.
        // AddCredits refunds the paid head independently; its spent ledger is retained.
        assert_eq!(
            runtime.simulation.houses[&owner].economy().credits(),
            50_250
        );
    }
}

/// Execute the callback boundary separately from sale crew/animation. The
/// native factory_loss sale controls use this same declared boundary so all
/// RNG streams and identity allocation can be compared without conflating
/// legitimate stage1 crew construction with a forbidden queued constructor.
#[test]
fn retail_barracks_sale_callbacks_reject_successor_or_keep_surviving_producer() {
    use crate::sim::world::{FrameEffects, TickLane, UninitContext};

    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/factory_sale_queue.json",
    ))
    .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/factory_sale_queue.meta.json",
    ))
    .unwrap();
    assert_eq!(
        metadata["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let rows = native["controls"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let successor = row["successor"].as_str().unwrap();
        let second_barracks = row["surviving_barracks"].as_bool().unwrap();
        let Some((mut runtime, owner, barracks)) =
            retail_barracks_sale_runtime(second_barracks, successor == "FLAKT", 50_000)
        else {
            return;
        };
        let wallet = row["initial"]["credits"].as_i64().unwrap() as i32
            + row["initial"]["spent"].as_i64().unwrap() as i32;
        runtime
            .simulation
            .houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut()
            .set_credits_for_test(wallet);
        for _ in 0..8 {
            runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
        }
        let successor_type = runtime.simulation.interner.get(successor).unwrap();
        let options = super::build_options_for_owner(
            &runtime.simulation,
            &runtime.resources.rules,
            "Russians",
        );
        assert!(
            options
                .iter()
                .any(|option| option.type_id == successor_type && option.enabled),
            "real prerequisites for {successor}"
        );
        for name in ["E2", successor] {
            assert!(enqueue_by_type(
                &mut runtime.simulation,
                &runtime.resources.rules,
                "Russians",
                name,
                FrameEffects::default(),
                false
            ));
        }
        for _ in 0..100 {
            if runtime.simulation.houses[&owner].economy().spent_credits() > 0 {
                break;
            }
            runtime
                .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
        }
        let product = held_id(&runtime.simulation, owner, ProductionCategory::Infantry);
        let before = runtime
            .simulation
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        let progress = before.progress;
        assert!(progress > 0 && progress < super::PRODUCTION_STEPS);
        let factory_order = before.construction_order();
        assert!(super::production_sell::sell_back(
            &mut runtime.simulation,
            &runtime.resources.rules,
            barracks,
            super::SellOrder::Player,
            None,
            FrameEffects::default()
        ));
        let sim = &mut runtime.simulation;
        assert!(!sim.houses[&owner].tech_recheck_pending());
        let rng = (
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let identity = sim.substrate.next_stable_object_id;
        let credits = sim.houses[&owner].economy().credits();
        let paid = sim.houses[&owner].economy().spent_credits();
        let before_native = &row["after_selling"];
        let after_native = &row["after_limbo"];
        assert_eq!(
            i64::from(credits),
            before_native["credits"].as_i64().unwrap()
        );
        assert_eq!(i64::from(paid), before_native["spent"].as_i64().unwrap());
        assert_eq!(
            u64::from(progress),
            before_native["stage"].as_u64().unwrap()
        );
        assert_eq!(before_native["queue"], serde_json::json!([successor]));
        assert_eq!(row["constructor_count"], 0);
        assert_eq!(row["identity_delta"], 0);
        assert_eq!(row["rng"]["request_count"], 0);
        assert_eq!(row["rng"]["raw_advance_count"], 0);
        assert_eq!(row["rng"]["unchanged"], true);
        assert_eq!(row["rng"]["before_sha256"], row["rng"]["after_sha256"]);
        let context = UninitContext::with_rules(&runtime.resources.rules);
        sim.techno_limbo_with_context(barracks, context);
        assert_eq!(
            sim.substrate.next_stable_object_id, identity,
            "{successor}: no StartNext constructor before admission"
        );
        assert_eq!(
            (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng,
            "callback boundary consumes no RNG: {successor}"
        );
        assert_eq!(
            i64::from(sim.houses[&owner].economy().credits()),
            after_native["credits"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(sim.houses[&owner].economy().spent_credits()),
            after_native["spent"].as_i64().unwrap()
        );
        assert_eq!(
            sim.entities().contains(product),
            !after_native["product_disposed"].as_bool().unwrap()
        );
        assert_eq!(
            sim.production
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .is_some(),
            after_native["factory_present"].as_bool().unwrap()
        );
        if second_barracks {
            let factory = sim
                .production
                .factories()
                .view(owner, ProductionCategory::Infantry)
                .unwrap();
            assert_eq!(factory.construction_order(), factory_order);
            assert_eq!(factory.object.unwrap().entity_id, Some(product));
            assert_eq!(factory.queue.len(), 1);
            assert_eq!(sim.houses[&owner].economy().credits(), credits);
        } else {
            assert!(!sim.entities().contains(product));
            assert!(
                sim.production
                    .factories()
                    .view(owner, ProductionCategory::Infantry)
                    .is_none()
            );
            assert_eq!(sim.houses[&owner].economy().credits(), credits + paid);
        }
        // Repeated native BuildingLimbo still runs509140; no duplicate refund
        // or destruction, even though ObjectConceal itself is now a no-op.
        let after = (
            sim.houses[&owner].economy().credits(),
            sim.houses[&owner].economy().spent_credits(),
            sim.substrate.next_stable_object_id,
        );
        sim.techno_limbo_with_context(barracks, context);
        assert_eq!(
            (
                sim.houses[&owner].economy().credits(),
                sim.houses[&owner].economy().spent_credits(),
                sim.substrate.next_stable_object_id
            ),
            after,
            "repeated callback, {successor}"
        );
    }
}

/// Ordinary paid BORIS through the bound frame API, with an explicit supplied
/// live Soviet tech base and flat-map prior. No completion/payment/exit state is
/// armed by the test. Original4FB4B7 ->4FB6B0 ->4FB7FF reads the delivered
/// object's Location after successor promotion, and ignores the local-house
/// notification gate. Reader/resource evidence: House creation-cue audit,
/// original gamemd SHA2561cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
#[test]
fn paid_boris_delivery_requests_its_creation_cue_once_at_the_delivered_location() {
    use crate::rules::object_type::ObjectCategory;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::runtime::{SimResources, SimRuntime};
    use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let mut sim = Simulation::with_seed(0x4FB6_B000);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let owner = sim.interner.intern("Russians");
    let mut house = HouseState::new(owner, 0, Some(owner), true, 50_000, 10);
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.register(house);
    sim.session.set_current_house_for_test(Some(owner));
    for (index, (name, x)) in [
        ("NACNST", 5),
        ("NAHAND", 14),
        ("NAPOWR", 23),
        ("NAPOWR", 29),
        ("NAPOWR", 35),
        ("NATECH", 41),
    ]
    .into_iter()
    .enumerate()
    {
        let id = index as u64 + 1;
        spawn_structure(&mut sim, &rules, id, "Russians", name, x, 10);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .health
            .set_for_test(rules.object(name).unwrap().strength);
        sim.remove_entity_occupancy(id);
        super::tests::stamp_type_foundation(&mut sim, &rules, id);
        sim.add_entity_occupancy(id);
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let boris = sim.interner.get("BORIS").unwrap();
    let conscript = sim.interner.get("E2").unwrap();
    for name in ["BORIS", "E2"] {
        assert!(
            super::build_options_for_owner(&sim, &rules, "Russians")
                .iter()
                .any(|option| sim.interner.resolve(option.type_id) == name && option.enabled),
            "the real reader/prerequisite owners admit {name}"
        );
    }
    let cost = sim.cost_of(owner, rules.object("BORIS").unwrap(), &rules);
    assert!(cost > 0);
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            ..SimResources::empty()
        },
    };
    let mut product = None;
    let mut cues = Vec::new();
    let mut delivered = false;
    for tick in 1..=4_000 {
        let commands = if tick == 1 {
            [boris, conscript]
                .map(|type_id| CommandEnvelope::new(owner, 1, Command::QueueProduction { type_id }))
                .to_vec()
        } else {
            Vec::new()
        };
        let output = runtime
            .advance_frame(&commands, 67, TickLane::Ordinary, FrameEffects::default())
            .expect("ordinary paid production frame");
        for event in output.sound_events {
            if let SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } = event
                && sound_id == "BorisCreated"
            {
                assert!(audible_to.is_none(), "native has no local-House gate");
                cues.push((rx, ry, sub_x, sub_y, world_z_leptons));
            }
        }
        let sim = &runtime.simulation;
        let id = *product.get_or_insert_with(|| held_id(sim, owner, ProductionCategory::Infantry));
        let entity = sim.substrate.entities.get(id).expect("same retained BORIS");
        assert_eq!(entity.type_ref(), boris);
        if !entity.lifecycle.in_limbo() {
            delivered = true;
            break;
        }
        assert!(
            cues.is_empty(),
            "completion/held objects cannot play creation"
        );
    }
    assert!(delivered, "ordinary funded BORIS exits its barracks");
    let sim = &runtime.simulation;
    let product = product.unwrap();
    let position = sim.substrate.entities.get(product).unwrap().position;
    let promoted = held_id(sim, owner, ProductionCategory::Infantry);
    assert_ne!(promoted, product, "the FIFO successor has a fresh identity");
    let successor = sim.substrate.entities.get(promoted).unwrap();
    assert_eq!(successor.type_ref(), conscript);
    assert!(successor.lifecycle.in_limbo());
    assert_eq!(sim.houses[&owner].economy().spent_credits(), cost);
    assert_eq!(
        sim.houses[&owner].economy().available_money(),
        50_000 - cost
    );
    assert_eq!(sim.houses[&owner].stats().built(), 1);
    assert_eq!(
        sim.houses[&owner].last_built_type_index(ObjectCategory::Infantry),
        runtime
            .resources
            .rules
            .type_array_index(ObjectCategory::Infantry, "BORIS")
            .unwrap()
    );
    assert_eq!(
        cues,
        [(
            position.rx(),
            position.ry(),
            position.sub_x(),
            position.sub_y(),
            crate::sim::movement::ground_pose::position_world_coord(&position).z
        )],
        "the completed BORIS, not the promoted E2, owns one creation request"
    );
    for _ in 0..8 {
        let output = runtime
            .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(
            !output.sound_events.iter().any(|event| {
                matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "BorisCreated")
            }),
            "later publication/frames do not replay creation"
        );
    }
}

/// Use physical stock Battle rules through the scenario reader, including its
/// mode/map passes, and the three original-byte controls documented in
/// tools/sidebar_oracle/factory_loss.md. The full bound runtime must preserve
/// Factory-before-House cancellation, wallet/statistics and held-object cleanup.
/// Healthy preplaced buildings are synthetic priors from `spawn_structure`,
/// with retail health/foundations applied. Removal reaches real UnInit/first
/// Limbo and the runtime; building admission/construction and map loading are
/// outside this fixture.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_prerequisite_loss_refunds_after_the_factory_charge_sweep() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/factory_loss.json",
    ))
    .expect("original factory-loss controls");
    for row in native["controls"].as_array().unwrap() {
        let loss = row["loss"].as_str().unwrap();
        let wallet = row["wallet"].as_i64().unwrap() as i32;
        let lost_building = match loss {
            "GATECH" => 4,
            "GAWEAP" => 1,
            other => panic!("unknown native loss {other}"),
        };
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &retail.rules;
        assert_eq!(
            rules.object("MGTK").unwrap().prerequisite,
            ["GAWEAP", "GATECH"].map(|name| crate::rules::prerequisite::Prerequisite::Building(
                rules.building_ids.iter().position(|id| id == name).unwrap() as i32,
            ))
        );
        assert!(rules.object("GATECH").unwrap().factory_type().is_none());
        let mut sim = Simulation::with_seed(0xfac7_1307);
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(64),
        );
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.set_input_delay_ticks(0);
        let owner = sim.interner.intern("Americans");
        sim.session.set_current_house_for_test(Some(owner));
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 50_000, 10,
            ));
        for (index, name) in ["GAWEAP", "GAPILE", "GACNST", "GATECH", "GAPOWR", "GAPOWR"]
            .into_iter()
            .enumerate()
        {
            let id = index as u64 + 1;
            spawn_structure(
                &mut sim,
                rules,
                id,
                "Americans",
                name,
                10 + index as u16 * 5,
                10,
            );
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .health
                .set_for_test(rules.object(name).unwrap().strength);
            sim.remove_entity_occupancy(id);
            super::tests::stamp_type_foundation(&mut sim, rules, id);
            sim.add_entity_occupancy(id);
        }
        assert!(enqueue_by_type(
            &mut sim,
            rules,
            "Americans",
            "MGTK",
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let held = held_id(&sim, owner, ProductionCategory::Vehicle);
        for _ in 0..1000 {
            sim.advance_tick(&[], Some(rules), None, None, 67);
            if sim
                .production
                .factories()
                .view(owner, ProductionCategory::Vehicle)
                .unwrap()
                .progress
                >= 1
            {
                break;
            }
        }
        let vehicle = sim
            .production
            .factories()
            .view(owner, ProductionCategory::Vehicle)
            .unwrap();
        assert_eq!(
            u64::from(vehicle.progress),
            row["before"]["factories"]["MGTK"]["stage"]
                .as_u64()
                .unwrap()
        );
        let balance = sim
            .production
            .factories_mut()
            .test_factory_mut(owner, ProductionCategory::Vehicle)
            .unwrap()
            .balance;
        assert_eq!(
            i64::from(balance),
            row["before"]["factories"]["MGTK"]["balance"]
                .as_i64()
                .unwrap()
        );
        assert!(enqueue_by_type(
            &mut sim,
            rules,
            "Americans",
            "E1",
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .economy_mut()
            .set_credits_for_test(wallet);
        assert_eq!(
            i64::from(sim.houses[&owner].economy().spent_credits()),
            row["before"]["spent"].as_i64().unwrap()
        );
        assert_eq!(
            sim.houses[&owner].tech_recheck_pending(),
            row["before"]["dirty"].as_bool().unwrap()
        );
        let frame = sim.session.binary_frame() as i32;
        for category in [ProductionCategory::Vehicle, ProductionCategory::Infantry] {
            sim.production
                .factories_mut()
                .test_factory_mut(owner, category)
                .unwrap()
                .step_timer
                .start(frame, 0);
        }
        // Native BuildingLimbo445D99 sets House+1FC for either loss, but
        // only a Factory= building reaches UpdateFactoryQueue445E14. The
        // prerequisite-only loss refunds at the local event tail, after all
        // factories tried to charge (Logic55B66A -> House4F92E2 -> 647260).
        sim.uninit_with_rules(lost_building, rules);
        assert_eq!(
            sim.houses[&owner].tech_recheck_pending(),
            row["after_removal"]["dirty"].as_bool().unwrap()
        );
        let mut resources = crate::sim::runtime::SimResources::empty();
        resources.rules = retail.rules;
        let mut runtime = crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        };
        let mut output = runtime
            .advance_frame(
                &[],
                67,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("bound production loss frame");
        let admitted = output.take_admitted_commands();
        let event_kinds: Vec<_> = admitted
            .iter()
            .map(|command| {
                assert_eq!(command.owner, owner);
                assert_eq!(command.execute_tick, output.tick.tick);
                match command.payload {
                    crate::sim::command::Command::CancelProductionByType { type_id, all } => {
                        assert_eq!(runtime.simulation.interner.resolve(type_id), "MGTK");
                        if all { 0x2E } else { 0x10 }
                    }
                    ref other => panic!("unexpected loss event {other:?}"),
                }
            })
            .collect();
        assert_eq!(
            event_kinds,
            row["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect::<Vec<_>>()
        );
        let crate::sim::runtime::SimRuntime {
            simulation: mut sim,
            resources: _,
        } = runtime;
        assert_gone(&sim, held, &[]);
        assert!(
            sim.production
                .factories()
                .view(owner, ProductionCategory::Vehicle)
                .is_none(),
            "native cancellation releases the empty factory and held Techno"
        );
        let expected = &row["after_events"];
        assert!(expected["factories"]["MGTK"].is_null());
        let infantry = sim
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .unwrap();
        assert_eq!(
            u64::from(infantry.progress),
            expected["factories"]["E1"]["stage"].as_u64().unwrap(),
            "{loss}/wallet{wallet}: a prerequisite refund cannot fund an earlier factory visit"
        );
        let balance = sim
            .production
            .factories_mut()
            .test_factory_mut(owner, ProductionCategory::Infantry)
            .unwrap()
            .balance;
        assert_eq!(
            i64::from(balance),
            expected["factories"]["E1"]["balance"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(sim.houses[&owner].economy().credits()),
            expected["credits"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(sim.houses[&owner].economy().spent_credits()),
            expected["spent"].as_i64().unwrap()
        );
        assert_eq!(
            sim.houses[&owner].tech_recheck_pending(),
            expected["dirty"].as_bool().unwrap()
        );
    }
}

#[test]
fn prerequisite_loss_events_preserve_player_copies_cleanup_and_snapshot_replay() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
    use crate::sim::runtime::{SimResources, SimRuntime};
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::{FrameEffects, TickLane};

    let (mut sim, rules, owner) = world(0xfac7_1308);
    for name in ["SMIN", "SMIN", "MTNK"] {
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            name,
            FrameEffects::default(),
            false,
        ));
    }
    let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
    let child_ids = children(&sim, parent);
    for _ in 0..40 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert!(!sim.houses[&owner].tech_recheck_pending());
    sim.uninit_with_rules(4, &rules);
    assert!(sim.houses[&owner].tech_recheck_pending());
    // Full native Scenario load reseeds its saved stream to zero. Isolate the
    // recheck roundtrip from that intentional transition, and retain the live
    // process cursors through the same in-scenario load seam as the app.
    sim.scenario_rng = SimRng::new(0);
    let bytes = GameSnapshot::save(&sim, 0, rules.simulation_config_hash(), "factory-loss", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    restored
        .restore_after_snapshot_load()
        .expect("restore held-object references");
    restored.resolve_type_handles(&rules);
    assert!(
        restored.houses[&owner].tech_recheck_pending(),
        "a saved loss cannot consume the pending recheck"
    );
    assert_eq!(restored.state_hash(), sim.state_hash(), "saved loss state");

    let header = ReplayHeader {
        version: 1,
        tick_hz: 15,
        seed: sim.session.seed(),
        pixel_conversion_bounds: sim.session.pixel_conversion_bounds(),
        map_name: "factory-loss".into(),
        rules_hash: rules.simulation_config_hash(),
    };
    let mut resources = SimResources::empty();
    resources.rules = rules;
    let mut live = SimRuntime {
        simulation: sim,
        resources,
    };
    let mut replay_resources = SimResources::empty();
    replay_resources.rules = manager_rules();
    let mut replay = SimRuntime {
        simulation: restored,
        resources: replay_resources,
    };
    let type_id = live.simulation.interner.get("SMIN").unwrap();
    let input = CommandEnvelope::new(
        owner,
        live.simulation.session.tick() + 1,
        Command::CancelProductionByType {
            type_id,
            all: false,
        },
    );
    let mut output = live
        .advance_frame(
            &[input.clone()],
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .expect("loss and player cancel frame");
    let admitted_commands = output.take_admitted_commands();
    assert_eq!(
        admitted_commands,
        [
            input.clone(),
            input,
            CommandEnvelope::new(
                owner,
                output.tick.tick,
                Command::CancelProductionByType { type_id, all: true }
            )
        ],
        "the player's identical ABANDON and the strip's ABANDON both reach the event tail"
    );
    assert_eq!(output.tick.executed_commands, 3);
    assert_gone(&live.simulation, parent, &child_ids);
    assert!(!live.simulation.houses[&owner].tech_recheck_pending());
    let head = held_id(&live.simulation, owner, ProductionCategory::Vehicle);
    assert_eq!(
        live.simulation.interner.resolve(
            live.simulation
                .substrate
                .entities
                .get(head)
                .unwrap()
                .type_ref()
        ),
        "MTNK"
    );
    assert!(
        live.simulation
            .production
            .factories()
            .view(owner, ProductionCategory::Vehicle)
            .unwrap()
            .queue
            .is_empty()
    );
    let mut log = ReplayLog::new(header);
    log.record_tick(output.tick.tick, admitted_commands, output.tick.state_hash);
    let hashes =
        ReplayRunner::run_runtime(&mut replay, &log, 67).expect("recorded production replay");
    assert_eq!(hashes, [Some(live.simulation.state_hash())]);
    assert_gone(&replay.simulation, parent, &child_ids);
    let mut next = live
        .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
        .expect("post-loss frame");
    assert!(
        next.take_admitted_commands().is_empty(),
        "the consumed dirty flag emits no repeated cancellation"
    );
}

/// Use physical stock Battle rules through the scenario reader, including its
/// mode/map passes. This is a production regression, not a native timing golden.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_superweapon_construction_survives_revalidation() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for (building, country, yard, tech, plant) in [
        ("GACSPH", "Americans", "GACNST", "GATECH", "GAPOWR"),
        ("GAWEAT", "Americans", "GACNST", "GATECH", "GAPOWR"),
        ("NAIRON", "Russians", "NACNST", "NATECH", "NAPOWR"),
        ("NAMISL", "Russians", "NACNST", "NATECH", "NAPOWR"),
        ("YAGNTC", "YuriCountry", "YACNST", "YATECH", "YAPOWR"),
        ("YAPPET", "YuriCountry", "YACNST", "YATECH", "YAPOWR"),
    ] {
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::test_flat_ground_grid(64),
        );
        sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        });
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let owner = sim.interner.intern(country);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 50_000, 10,
            ));
        for (index, name) in [yard, tech, plant, plant, plant].into_iter().enumerate() {
            let id = index as u64 + 1;
            spawn_structure(
                &mut sim,
                rules,
                id,
                country,
                name,
                10 + index as u16 * 5,
                10,
            );
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .health
                .set_for_test(rules.object(name).unwrap().strength);
            // The compact helper marks one cell. Install the stock foundation
            // through the occupancy boundary before running whole-world ticks.
            sim.remove_entity_occupancy(id);
            super::tests::stamp_type_foundation(&mut sim, rules, id);
            sim.add_entity_occupancy(id);
        }
        let object = rules.object(building).unwrap();
        assert_eq!(object.build_limit, 1, "{building}: stock limit");
        let category = super::category_for_object(object);
        assert!(
            super::build_options_for_owner(&sim, rules, country)
                .iter()
                .any(|option| sim.interner.resolve(option.type_id) == building && option.enabled),
            "{building}: the stock prerequisites offer an enabled cameo"
        );
        let type_id = sim.interner.get(building).unwrap();
        let command = crate::sim::command::CommandEnvelope::new(
            owner,
            1,
            crate::sim::command::Command::QueueProduction { type_id },
        );
        sim.advance_tick(&[command], Some(rules), None, None, 67);
        let head = held_id(&sim, owner, category);
        for _ in 0..20_000 {
            sim.advance_tick(&[], Some(rules), None, None, 67);
            let view = sim
                .production
                .factories()
                .view(owner, category)
                .unwrap_or_else(|| panic!("{building}: construction was cancelled"));
            assert_eq!(view.object.unwrap().entity_id, Some(head));
            if view.progress == super::PRODUCTION_STEPS {
                break;
            }
        }
        let view = sim.production.factories().view(owner, category).unwrap();
        assert_eq!(
            view.progress,
            super::PRODUCTION_STEPS,
            "{building}: reaches placement readiness"
        );
        assert!(
            sim.substrate
                .entities
                .get(head)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
        let factories = sim.production.factories().clone();
        let wallet = sim.houses[&owner].economy().clone();
        let next_identity = sim.substrate.next_stable_object_id;
        // House4FA5A8/4FA628 admits this same completed product through the
        // held-build limit exception. Resume refuses without writes, so this
        // is a relink, not another copy (factory_hold_resume.json/.md).
        assert!(
            enqueue_by_type(
                &mut sim,
                rules,
                country,
                building,
                crate::sim::world::FrameEffects::default(),
                false,
            ),
            "{building}: same completed request is admitted"
        );
        assert_eq!(
            *sim.production.factories(),
            factories,
            "{building}: complete resume is inert"
        );
        assert_eq!(
            *sim.houses[&owner].economy(),
            wallet,
            "{building}: no charge"
        );
        assert_eq!(
            sim.substrate.next_stable_object_id, next_identity,
            "{building}: no new object"
        );
        assert_eq!(held_id(&sim, owner, category), head);
        let preview = super::placement_preview_for_owner_with_overlays(
            &sim, rules, country, building, 10, 14, None,
        )
        .unwrap();
        assert_eq!(
            preview.reason, None,
            "{building}: clear site beside its yard"
        );
        let place = crate::sim::command::CommandEnvelope::new(
            owner,
            sim.session.tick() + 1,
            crate::sim::command::Command::PlaceReadyBuilding {
                type_id,
                rx: 10,
                ry: 14,
            },
        );
        sim.advance_tick(&[place], Some(rules), None, None, 67);
        assert!(
            !sim.substrate
                .entities
                .get(head)
                .unwrap()
                .lifecycle
                .in_limbo(),
            "{building}: the completed object is placed without replacement"
        );
        assert!(sim.production.factories().view(owner, category).is_none());
        assert!(
            !enqueue_by_type(
                &mut sim,
                rules,
                country,
                building,
                crate::sim::world::FrameEffects::default(),
                false,
            ),
            "{building}: placement does not allow a second copy"
        );
    }
}

/// Positive limits use the shared CanBuild factory-held exception for all four
/// classes; cancellation must still release tracking and lost producers cancel.
#[test]
fn limited_heads_survive_revalidation_without_bypassing_limits_or_cleanup() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Americans\n[InfantryTypes]\n0=I\n[VehicleTypes]\n0=T\n\
         [AircraftTypes]\n0=A\n[BuildingTypes]\n0=B\n1=YARD\n2=BARRACKS\n3=WAR\n4=AIR\n\
         [I]\nOwner=Americans\nTechLevel=1\nBuildLimit=1\nCost=200\nStrength=100\nSpeed=4\n\
         [T]\nOwner=Americans\nTechLevel=1\nBuildLimit=1\nCost=500\nStrength=100\nSpeed=4\n\
         [A]\nOwner=Americans\nTechLevel=1\nBuildLimit=1\nCost=500\nStrength=100\nSpeed=8\n\
         [B]\nOwner=Americans\nTechLevel=1\nBuildLimit=1\nCost=500\nStrength=100\nFoundation=1x1\n\
         [YARD]\nOwner=Americans\nFactory=BuildingType\n\
         [BARRACKS]\nOwner=Americans\nFactory=InfantryType\n\
         [WAR]\nOwner=Americans\nFactory=UnitType\n\
         [AIR]\nOwner=Americans\nFactory=AircraftType\n",
    ))
    .unwrap();
    for (name, producer) in [("B", "YARD"), ("I", "BARRACKS"), ("T", "WAR"), ("A", "AIR")] {
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 50_000, 10,
            ));
        spawn_structure(&mut sim, &rules, 1, "Americans", producer, 10, 10);
        let category = super::category_for_object(rules.object(name).unwrap());
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            name,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let head = held_id(&sim, owner, category);
        super::update_factory_queue(
            &mut sim,
            &rules,
            owner,
            category,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            held_id(&sim, owner, category),
            head,
            "{name}: own held object remains admissible"
        );
        assert!(!enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            name,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        assert!(cancel_production_for_owner(
            &mut sim,
            &rules,
            "Americans",
            crate::sim::production::ProductionCancellation::Type {
                type_id: name,
                naval: false
            },
            false,
            crate::sim::world::FrameEffects::default(),
        ));
        assert!(!sim.substrate.entities.contains(head));
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            name,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let replacement = held_id(&sim, owner, category);
        sim.uninit_with_rules(1, &rules);

        assert!(!sim.substrate.entities.contains(replacement));
        assert!(sim.production.factories().view(owner, category).is_none());
    }
}

pub(super) fn held_id(sim: &Simulation, owner: InternedId, category: ProductionCategory) -> u64 {
    sim.production
        .factories()
        .view(owner, category)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap()
}

pub(super) fn children(sim: &Simulation, parent: u64) -> Vec<u64> {
    let ids: Vec<_> = if let Some(manager) = sim
        .substrate
        .entities
        .get(parent)
        .unwrap()
        .spawn_manager
        .as_ref()
    {
        manager
            .slots()
            .iter()
            .map(|slot| slot.spawn().unwrap())
            .collect()
    } else {
        sim.substrate
            .entities
            .get(parent)
            .unwrap()
            .slave_manager
            .as_ref()
            .map(|manager| manager.slaves().collect())
            .unwrap_or_default()
    };
    for &id in &ids {
        let child = sim
            .substrate
            .entities
            .get(id)
            .expect("held manager child remains represented");
        assert!(child.lifecycle.in_limbo() && !child.lifecycle.cell_marked());
        assert!(
            child.spawn_owner_id.owner() == Some(parent) || child.slave.owner() == Some(parent)
        );
    }
    ids
}

fn counts(sim: &Simulation, owner: InternedId) -> (u32, u32) {
    sim.owned_object_counts(owner)
}

fn assert_gone(sim: &Simulation, parent: u64, child_ids: &[u64]) {
    assert!(!sim.substrate.entities.contains(parent));
    for &child in child_ids {
        assert!(!sim.substrate.entities.contains(child));
    }
}

pub(super) fn assert_constructor_words(sim: &Simulation, parent: u64, expected: &mut SimRng) {
    for id in std::iter::once(parent).chain(children(sim, parent)) {
        let word = (expected.next_u32() & 0xffff) as u16;
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .techno_ctor_random_word(),
            word
        );
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn manager_factory_cancellation_finishes_graph_accounting_and_promotion() {
    for parent_type in ["MTNK", "SMIN"] {
        let (mut sim, rules, owner) = world(0xfac7_0010);
        let before = counts(&sim, owner);
        let mut expected = sim.scenario_rng.clone();
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            parent_type,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        assert_eq!(child_ids.len(), if parent_type == "MTNK" { 3 } else { 2 });
        assert_constructor_words(&sim, parent, &mut expected);
        assert_eq!(
            counts(&sim, owner),
            (before.0, before.1 + 1 + child_ids.len() as u32)
        );
        let allocated = sim.substrate.next_stable_object_id;
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            parent_type,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        assert_eq!(sim.substrate.next_stable_object_id, allocated);
        // ABANDON removes the queued copy first (`0x004FAAEE`), then the active build.
        assert!(cancel_production_for_owner(
            &mut sim,
            &rules,
            "Americans",
            crate::sim::production::ProductionCancellation::Type {
                type_id: parent_type,
                naval: false
            },
            false,
            crate::sim::world::FrameEffects::default(),
        ));
        assert_eq!(held_id(&sim, owner, ProductionCategory::Vehicle), parent);
        assert_eq!(children(&sim, parent), child_ids);
        assert_eq!(sim.houses[&owner].economy().credits(), 50_000);
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());

        assert!(cancel_production_for_owner(
            &mut sim,
            &rules,
            "Americans",
            crate::sim::production::ProductionCancellation::Type {
                type_id: parent_type,
                naval: false
            },
            false,
            crate::sim::world::FrameEffects::default(),
        ));
        assert_gone(&sim, parent, &child_ids);
        assert_eq!(counts(&sim, owner), before);
        assert_eq!(sim.substrate.next_stable_object_id, allocated);
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        assert_eq!(
            sim.houses[&owner].produced_count(
                crate::rules::object_type::ObjectCategory::Vehicle,
                rules
                    .type_array_index(
                        crate::rules::object_type::ObjectCategory::Vehicle,
                        parent_type
                    )
                    .unwrap(),
            ),
            0,
            "cancelling a held product never records successful production"
        );
        assert!(
            sim.production
                .factories()
                .view(owner, ProductionCategory::Vehicle)
                .is_none()
        );

        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            parent_type,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        assert_constructor_words(&sim, parent, &mut expected);
        let successor_type = if parent_type == "MTNK" {
            "SMIN"
        } else {
            "MTNK"
        };
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            successor_type,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        assert!(cancel_production_for_owner(
            &mut sim,
            &rules,
            "Americans",
            crate::sim::production::ProductionCancellation::Type {
                type_id: parent_type,
                naval: false
            },
            false,
            crate::sim::world::FrameEffects::default(),
        ));
        assert_gone(&sim, parent, &child_ids);
        let successor = held_id(&sim, owner, ProductionCategory::Vehicle);
        assert!(successor > parent);
        assert_constructor_words(&sim, successor, &mut expected);
        assert_eq!(
            counts(&sim, owner),
            (
                before.0,
                before.1 + 1 + children(&sim, successor).len() as u32
            )
        );
        assert_eq!(
            sim.production
                .factories()
                .view(owner, ProductionCategory::Vehicle)
                .unwrap()
                .progress,
            0
        );
    }
}

/// A Slave Miner leaves its war factory as a tank does: ExitObject's land
/// arm tethers it to the factory and starts no hunt. Unit PerCell starts
/// the hunt when the miner clears the factory (`0x0073A9CA..0x0073A9D9`,
/// `per_cell_factory_clearance_starts_a_slave_miners_hunt`).
#[test]
fn a_produced_slave_miner_leaves_tethered_without_hunting_yet() {
    let mut ini = manager_ini();
    ini.merge(&IniFile::from_str("[GAWEAP]\nWeaponsFactory=yes\n"));
    let (mut sim, rules, owner) = world_with(RuleSet::from_ini(&ini).unwrap(), 0xfac7_0020);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_archive_target(Some(crate::sim::combat::TargetKind::Cell(30, 30)));
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "SMIN",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let produced = held_id(&sim, owner, ProductionCategory::Vehicle);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, ProductionCategory::Vehicle)
    );
    let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
    sim.install_fixture_path_grid(Some(&grid));
    assert!(dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let entity = sim.substrate.entities.get(produced).unwrap();
    assert!(!entity.lifecycle.in_limbo(), "SMIN delivered");
    assert_eq!(entity.tether.partner(), Some(1));
    assert_eq!(
        entity.slave_manager.as_ref().unwrap().state(),
        crate::sim::slave_manager::ManagerState::Ready,
        "the hunt waits for the factory clearance"
    );
}

/// Each factory keeps its own rally point (its ArchiveTarget, `+0x218`), and
/// `BuildingClass::ExitObject_Main @ 0x00443C60` hands the leaving object its
/// own factory's: an infantryman walks to the barracks' rally although the
/// owner's last rally click was on the war factory.
#[test]
fn produced_infantry_retains_its_barracks_rally_until_exit_handoff() {
    let (mut sim, rules, owner) = world(0xfac7_0021);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    for (factory, rally) in [(2, (30, 30)), (1, (40, 12))] {
        let command = crate::sim::command::Command::SetRally {
            rx: rally.0,
            ry: rally.1,
            producer_ids: vec![factory],
        };
        assert!(sim.apply_command("Americans", &command, Some(&rules)));
    }
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let produced = held_id(&sim, owner, ProductionCategory::Infantry);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, ProductionCategory::Infantry)
    );
    let grid = crate::sim::pathfinding::PathGrid::new(64, 64);
    sim.install_fixture_path_grid(Some(&grid));
    assert!(dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    let entity = sim.substrate.entities.get(produced).unwrap();
    assert!(!entity.lifecycle.in_limbo(), "E1 delivered");
    // Nonzero rally: TechnoUnlimbo's InfIdle establishes Nav, then Exit
    // saves it back to Archive444CE7 before assigning the exit444D11.
    // A stock NavNULL prior follows the separate skip arm. FootEnterIdle
    // consumes this retained Archive later (520F92 -> 51CBA0 -> 4D82B0).
    assert_eq!(entity.rally_cell(), Some((30, 30)));
    assert!(
        entity.navigation.nav_com().is_some(),
        "the initial exit is paid first"
    );
    assert_ne!(
        entity.navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(30, 30))
    );
    assert!(entity.has_live_contact_with(2));
    assert_eq!(entity.tether.partner(), Some(2));
}

/// `TechnoClass::ChangeOwner @ 0x007014A0` clears the ArchiveTarget
/// (`0x0070151A`), so a captured factory loses its rally point.
#[test]
fn a_captured_factory_loses_its_rally_point() {
    let (mut sim, rules, _) = world(0xfac7_0022);
    let command = crate::sim::command::Command::SetRally {
        rx: 40,
        ry: 12,
        producer_ids: vec![1],
    };
    assert!(sim.apply_command("Americans", &command, Some(&rules)));
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().rally_cell(),
        Some((40, 12))
    );
    let russians = sim.interner.intern("Russians");
    sim.change_owner_with_rules(
        1,
        russians,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(sim.substrate.entities.get(1).unwrap().rally_cell(), None);
}

#[test]
fn ready_manager_cancel_refunds_disposes_and_constructs_one_successor() {
    let (mut sim, rules, owner) = world(0xfac7_0011);
    let before = counts(&sim, owner);
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "YAREFN",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let parent = held_id(&sim, owner, ProductionCategory::Building);
    let child_ids = children(&sim, parent);
    assert_eq!(child_ids.len(), 2);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, ProductionCategory::Building)
    );
    super::publish_production_changes(&mut sim, &rules);
    assert_eq!(
        crate::sim::production::ready_buildings_for_owner(&sim, &rules, "Americans").len(),
        1
    );
    let mut expected = sim.scenario_rng.clone();
    let credits = sim.houses[&owner].economy().credits();
    // A PRODUCE of the type waiting finished takes Begin_Production's resume
    // branch (0x004FA5A8..0x004FA5C4), which the build start refuses at stage 54
    // (0x004C9ECD). House ignores that refusal and still admits/relinks:
    // nothing is queued or charged (factory_hold_resume.json).
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "YAREFN",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    assert_eq!(held_id(&sim, owner, ProductionCategory::Building), parent);
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Building)
            .unwrap()
            .queue
            .is_empty()
    );
    assert_eq!(sim.houses[&owner].economy().credits(), credits);
    // A different queued type does not intercept cancellation of the ready head.
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GAPOWR",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    assert!(cancel_production_for_owner(
        &mut sim,
        &rules,
        "Americans",
        crate::sim::production::ProductionCancellation::Type {
            type_id: "YAREFN",
            naval: false
        },
        false,
        crate::sim::world::FrameEffects::default(),
    ));
    assert_gone(&sim, parent, &child_ids);
    assert_eq!(sim.houses[&owner].economy().credits(), credits + 1000);
    assert_eq!(counts(&sim, owner), (before.0 + 1, before.1));
    assert!(
        crate::sim::production::ready_buildings_for_owner(&sim, &rules, "Americans").is_empty()
    );
    let successor = held_id(&sim, owner, ProductionCategory::Building);
    assert!(successor > parent);
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(successor).unwrap().type_ref()),
        "GAPOWR"
    );
    assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Building)
            .unwrap()
            .progress,
        0
    );
}

/// A build whose prerequisite leaves the map is abandoned by its strip's
/// ABANDON (`recalculate_strips`). The queued build it promotes starts at
/// that frame (StartNextQueued runs Begin_Production, whose build start arms
/// the timer), so it first steps one rate later.
#[test]
fn prerequisite_revalidation_disposes_manager_and_promoted_build_steps_a_rate_later() {
    let (mut sim, rules, owner) = world(0xfac7_0012);
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "SMIN",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
    let child_ids = children(&sim, parent);
    for _ in 0..40 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    let spent = {
        let factory = sim
            .production
            .factories_mut()
            .test_factory_mut(owner, ProductionCategory::Vehicle)
            .unwrap();
        assert!(factory.progress > 0 && factory.progress < 54);
        let balance = factory.balance;
        sim.cost_of(owner, rules.object("SMIN").unwrap(), &rules) - balance
    };
    let before = sim.houses[&owner].economy().credits();
    let mut expected = sim.scenario_rng.clone();
    // The producing factory remains; only the active type loses its
    // prerequisite, which its house stops counting on the map.
    sim.uninit_with_rules(4, &rules);
    let promotion_frame = sim.session.binary_frame();
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_gone(&sim, parent, &child_ids);
    assert_eq!(sim.houses[&owner].economy().credits(), before + spent);
    let successor = held_id(&sim, owner, ProductionCategory::Vehicle);
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(successor).unwrap().type_ref()),
        "MTNK"
    );
    assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Vehicle)
            .unwrap()
            .progress,
        0
    );
    let (rate, timer) = {
        let factory = sim
            .production
            .factories_mut()
            .test_factory_mut(owner, ProductionCategory::Vehicle)
            .unwrap();
        (factory.step_rate_frames, factory.step_timer)
    };
    assert_eq!(timer.start_frame(), promotion_frame as i32);
    assert!(rate > 1);
    let progress = |sim: &Simulation| {
        sim.production
            .factories()
            .view(owner, ProductionCategory::Vehicle)
            .unwrap()
            .progress
    };
    for _ in 1..rate {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        assert_eq!(progress(&sim), 0);
    }
    assert_eq!(sim.houses[&owner].economy().credits(), before + spent);
    sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(progress(&sim), 1);
    assert!(sim.houses[&owner].economy().credits() < before + spent);
}

#[test]
fn missing_barracks_retains_completed_infantry_and_queued_successor() {
    let (mut sim, rules, owner) = world_with_creation_cue(0xfac7_0013);
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let held = held_id(&sim, owner, ProductionCategory::Infantry);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, ProductionCategory::Infantry)
    );
    // Supply an absent producer without invoking destruction's separate
    // AbandonProduction owner. HousePlace4FB520 returns before ExitObject.
    for structure in 1..=4 {
        sim.substrate.entities.remove(structure);
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .tracking_mut()
        .set_buildings_for_test(0);
    let credits = sim.houses[&owner].economy().credits();
    let rng = (
        sim.main_rng.logical_state(),
        sim.scenario_rng.logical_state(),
        sim.mapgen_rng.logical_state(),
    );
    let allocated = sim.substrate.next_stable_object_id;
    assert!(!dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert_eq!(held_id(&sim, owner, ProductionCategory::Infantry), held);
    let factory = sim
        .production
        .factories()
        .view(owner, ProductionCategory::Infantry)
        .unwrap();
    assert!(factory.complete_object().is_some());
    assert_eq!(factory.progress, super::PRODUCTION_STEPS);
    assert_eq!(factory.queue.len(), 1);
    let object = sim.substrate.entities.get(held).unwrap();
    assert!(object.lifecycle.in_limbo() && !object.lifecycle.cell_marked());
    assert_eq!(sim.houses[&owner].economy().credits(), credits);
    assert_eq!(sim.substrate.next_stable_object_id, allocated);
    assert!(
        !sim.sound_events.iter().any(|event| {
            matches!(event, crate::sim::world::SimSoundEvent::VocAt { sound_id, .. }
            if sound_id == "FixtureCreated")
        }),
        "missing producer retains the paid head without creation output"
    );
    assert_eq!(
        (
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state(),
            sim.mapgen_rng.logical_state()
        ),
        rng
    );
}

#[test]
fn active_cancel_without_house_does_not_create_refund_account() {
    for all in [false, true] {
        let (mut sim, rules, owner) = world(0xfac7_0014);
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            "SMIN",
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        sim.houses.remove_for_test(&owner);
        let rng = sim.scenario_rng.logical_state();
        let cancelled = cancel_production_for_owner(
            &mut sim,
            &rules,
            "Americans",
            crate::sim::production::ProductionCancellation::Type {
                type_id: "SMIN",
                naval: false,
            },
            all,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(cancelled);
        assert_gone(&sim, parent, &child_ids);
        assert!(!sim.houses.contains_key(&owner));
        assert_eq!(sim.scenario_rng.logical_state(), rng);
    }
}

#[test]
fn factory_loss_revalidation_disposes_parent_and_children_before_returning() {
    for parent_type in ["MTNK", "SMIN"] {
        let (mut sim, rules, owner) = world(0xfac7_0016);
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            parent_type,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            parent_type,
            crate::sim::world::FrameEffects::default(),
            false,
        ));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        for _ in 0..40 {
            sim.advance_tick(&[], Some(&rules), None, None, 67);
        }
        let spent = {
            let factory = sim
                .production
                .factories_mut()
                .test_factory_mut(owner, ProductionCategory::Vehicle)
                .unwrap();
            assert!(factory.progress > 0 && factory.progress < 54);
            let balance = factory.balance;
            sim.cost_of(owner, rules.object(parent_type).unwrap(), &rules) - balance
        };
        let before = sim.houses[&owner].economy().credits();
        let allocated = sim.substrate.next_stable_object_id;
        let rng = sim.scenario_rng.logical_state();
        sim.uninit_with_rules(1, &rules);
        assert_gone(&sim, parent, &child_ids);
        assert_eq!(sim.houses[&owner].economy().credits(), before + spent);
        assert_eq!(sim.owned_object_counts(owner).1, 0);
        assert_eq!(sim.substrate.next_stable_object_id, allocated);
        assert_eq!(sim.scenario_rng.logical_state(), rng);
        assert!(
            sim.production
                .factories()
                .view(owner, ProductionCategory::Vehicle)
                .is_none()
        );
    }
}

/// A finished object held for delivery goes with its factory. At a building's
/// kill `BuildingClass::Detach_All(1) @ 0x0044EBF0` abandons its own factory
/// and, for a Construction Yard, every production no other factory can build,
/// finished or not (AbandonProduction `0x004C9FF0` refunds what was paid and
/// deletes the object). With the only Construction Yard gone, the completed
/// building is refunded, deleted and untracked.
#[test]
fn a_ready_building_goes_with_the_last_construction_yard() {
    let (mut sim, rules, owner) = world(0xfac7_0019);
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GAPOWR",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let held = held_id(&sim, owner, ProductionCategory::Building);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, ProductionCategory::Building)
    );
    super::publish_production_changes(&mut sim, &rules);
    assert_eq!(
        crate::sim::production::ready_buildings_for_owner(&sim, &rules, "Americans").len(),
        1
    );
    let tracked = sim.houses[&owner].tracking().buildings();
    let credits = sim.houses[&owner].economy().credits();

    // With a Construction Yard standing, the ready building waits.
    super::update_factory_queue(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Building,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(sim.substrate.entities.contains(held));

    sim.uninit_with_rules(3, &rules);
    assert!(!sim.substrate.entities.contains(held));
    assert_eq!(sim.houses[&owner].tracking().buildings(), tracked - 1);
    assert_eq!(sim.houses[&owner].economy().credits(), credits + 800);
    assert!(
        crate::sim::production::ready_buildings_for_owner(&sim, &rules, "Americans").is_empty()
    );
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Building)
            .is_none()
    );
}

/// A 900-credit tank buildable from a war factory, and an Industrial Plant type
/// (`UnitsCostBonus=.75`) that is not yet on the map, for a house with no money.
fn plant_world() -> (Simulation, RuleSet, InternedId) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=HTNK\n\
         [BuildingTypes]\n0=NAWEAP\n1=NAINDP\n\
         [HTNK]\nOwner=Russians\nCost=900\nStrength=400\nSpeed=5\nTechLevel=1\n\
         [NAWEAP]\nOwner=Russians\nFactory=UnitType\n\
         [NAINDP]\nStrength=1000\nFoundation=1x1\nFactoryPlant=yes\nUnitsCostBonus=.75\n",
    ))
    .expect("FactoryPlant fixture");
    let mut sim = Simulation::with_seed(0xc057_0f01);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        32,
    ));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Russians");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    spawn_structure(&mut sim, &rules, 1, "Russians", "NAWEAP", 10, 10);
    (sim, rules, owner)
}

fn spawn_plant(sim: &mut Simulation, rules: &RuleSet) {
    sim.spawn_object("NAINDP", "Russians", 20, 20, 0, rules)
        .expect("the Industrial Plant unlimbos");
}

/// With no money the tank still starts (`HouseClass::CanBuild @ 0x004F7870` has no
/// money check) owing its house's Cost_Of (`0x004C9DE1`), which a live Industrial
/// Plant discounts, and the sidebar offers it at that price, not greyed.
#[test]
fn a_build_starts_without_money_owing_its_cost_of() {
    let (mut sim, rules, owner) = plant_world();
    spawn_plant(&mut sim, &rules);
    let options = super::build_options_for_owner(&sim, &rules, "Russians");
    let tank = options
        .iter()
        .find(|option| sim.interner.resolve(option.type_id) == "HTNK")
        .expect("the tank is offered");
    assert!(tank.enabled);
    assert_eq!(tank.cost, 675);
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Russians",
        "HTNK",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    for _ in 0..30 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    let factory = sim
        .production
        .factories_mut()
        .test_factory_mut(owner, ProductionCategory::Vehicle)
        .unwrap();
    assert_eq!(factory.balance, 675);
    assert!(factory.on_hold && factory.progress == 0);
}

/// The cancel refund is the Cost_Of at cancel time less the Balance still owed
/// (`0x004CA029..0x004CA046`). An Industrial Plant built after the first step
/// lowers the Cost_Of below that Balance, so the cancel takes money.
#[test]
fn a_cancel_refunds_the_cost_of_at_cancel_time() {
    let (mut sim, rules, owner) = plant_world();
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Russians",
        "HTNK",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    crate::sim::credit_income::add_credits(&mut sim, owner, 1000);
    let progress = |sim: &mut Simulation| {
        let factory = sim
            .production
            .factories_mut()
            .test_factory_mut(owner, ProductionCategory::Vehicle)
            .unwrap();
        (factory.progress, factory.balance)
    };
    for _ in 0..2000 {
        if progress(&mut sim).0 > 0 {
            break;
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    // The first step pays 900 / 53.
    assert_eq!(progress(&mut sim), (1, 884));
    spawn_plant(&mut sim, &rules);
    let before = sim.houses[&owner].economy().credits();
    assert!(cancel_production_for_owner(
        &mut sim,
        &rules,
        "Russians",
        crate::sim::production::ProductionCancellation::Type {
            type_id: "HTNK",
            naval: false
        },
        false,
        crate::sim::world::FrameEffects::default(),
    ));
    assert_eq!(sim.houses[&owner].economy().credits(), before + 675 - 884);
}

/// Exit4440BC calls FreeRadio65ADC0: stock one-slot barracks return1 while
/// occupied. Human House4FB57F..4FB62A sees Building+524NULL and refunds,
/// scalar-destroys the held GI and starts the FIFO successor. Body/caller
/// evidence: basic-factory-output-research native-radio-free-65adc0.json and
/// native-house-place-4fb0e0.json. This is an owner regression with supplied
/// paid-ready priors, not a comparison of the native charging cadence.
#[test]
fn occupied_barracks_radio_refunds_discards_and_promotes_one_gi() {
    let (mut sim, rules, owner) = world_with_creation_cue(0xfac7_0023);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let category = ProductionCategory::Infantry;
    let producer = 2;
    let cost = sim.cost_of(owner, rules.object("E1").unwrap(), &rules);
    assert!(cost > 0);

    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let first = held_id(&sim, owner, category);
    // Supply the paid-completion wallet and Balance0 through their existing
    // owners. The actual constructor, publication, PLACE and HELLO2/9 tail run.
    let economy = sim.houses.get_mut(&owner).unwrap().economy_mut();
    assert!(
        economy.available_money() >= cost,
        "the wallet covers the build"
    );
    economy.spend_money(cost);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );
    assert!(dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert!(sim.pending_command_snapshot().is_empty());
    let first_entity = sim.substrate.entities.get(first).unwrap();
    assert!(!first_entity.lifecycle.in_limbo() && first_entity.in_logic_vector.is_member());
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| {
                matches!(event, crate::sim::world::SimSoundEvent::VocAt { sound_id, .. }
            if sound_id == "FixtureCreated")
            })
            .count(),
        1,
        "the first successful GI has one creation request"
    );
    let first_contacts = first_entity.radio_contacts.clone();
    assert_eq!(first_contacts.slot(0), Some(producer));
    assert_eq!(first_entity.tether.partner(), Some(producer));
    let producer_entity = sim.substrate.entities.get(producer).unwrap();
    let producer_contacts = producer_entity.radio_contacts.clone();
    assert_eq!(producer_contacts.capacity(), 1);
    assert_eq!(producer_contacts.slot(0), Some(first));
    assert_eq!(producer_contacts.first_free(), None);
    assert_eq!(producer_entity.tether.partner(), Some(first));
    assert!(
        sim.production
            .factories()
            .building_factory(producer)
            .is_none()
    );

    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        crate::sim::world::FrameEffects::default(),
        false,
    ));
    let refused = held_id(&sim, owner, category);
    assert_ne!(refused, first);
    // This completed second head and post-payment wallet are supplied test
    // priors. No live GI turn is invented to keep the first real link occupied.
    let economy = sim.houses.get_mut(&owner).unwrap().economy_mut();
    assert!(
        economy.available_money() >= cost,
        "the wallet covers the build"
    );
    economy.spend_money(cost);
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );
    assert_eq!(
        sim.production
            .factories_mut()
            .test_factory_mut(owner, category)
            .unwrap()
            .balance,
        0
    );
    super::publish_production_changes(&mut sim, &rules);
    let pending = sim.pending_command_snapshot();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].owner, owner);
    assert!(matches!(
        pending[0].payload,
        crate::sim::command::Command::PlaceProducedMobile {
            category: ProductionCategory::Infantry
        }
    ));
    assert_eq!(held_id(&sim, owner, category), refused);
    assert!(
        sim.substrate
            .entities
            .get(refused)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    let completed = sim.production.factories().view(owner, category).unwrap();
    assert!(completed.complete_object().is_some());
    assert_eq!(completed.queue.len(), 1);
    let credits = sim.houses[&owner].economy().credits();
    let spent = sim.houses[&owner].economy().spent_credits();
    let refund = sim.cost_of(owner, rules.object("E1").unwrap(), &rules);
    let prior_ids: Vec<_> = sim
        .substrate
        .entities
        .iter_sorted()
        .map(|(id, _)| id)
        .collect();
    let event_start = sim.sound_events.len();
    let main = sim.main_rng.logical_state();
    let mapgen = sim.mapgen_rng.logical_state();
    let mut expected_scenario = sim.scenario_rng.clone();

    assert!(!dispatch_production_changes_for_tests(
        &mut sim, &rules, None
    ));
    assert!(sim.pending_command_snapshot().is_empty());
    assert!(
        !sim.substrate.entities.contains(refused),
        "scalar cleanup is synchronous"
    );
    assert_eq!(sim.houses[&owner].economy().credits(), credits + refund);
    assert_eq!(sim.houses[&owner].economy().spent_credits(), spent);
    let factory = sim.production.factories().view(owner, category).unwrap();
    let successor = factory.object.unwrap().entity_id.unwrap();
    assert!(successor > refused);
    assert_eq!(factory.progress, 0);
    assert!(factory.queue.is_empty());
    let new_ids: Vec<_> = sim
        .substrate
        .entities
        .iter_sorted()
        .map(|(id, _)| id)
        .filter(|id| !prior_ids.contains(id))
        .collect();
    assert_eq!(
        new_ids,
        vec![successor],
        "exactly one FIFO successor constructs"
    );
    let successor_entity = sim.substrate.entities.get(successor).unwrap();
    assert!(successor_entity.lifecycle.in_limbo() && !successor_entity.lifecycle.cell_marked());
    assert!(!successor_entity.in_logic_vector.is_member());
    let first_entity = sim.substrate.entities.get(first).unwrap();
    assert_eq!(first_entity.radio_contacts, first_contacts);
    assert_eq!(first_entity.tether.partner(), Some(producer));
    let producer_entity = sim.substrate.entities.get(producer).unwrap();
    assert_eq!(producer_entity.radio_contacts, producer_contacts);
    assert_eq!(producer_entity.tether.partner(), Some(first));
    assert!(
        sim.production
            .factories()
            .building_factory(producer)
            .is_none()
    );
    assert!(!sim.sound_events[event_start..].iter().any(|event| matches!(
        event,
        crate::sim::world::SimSoundEvent::UnitComplete { owner: receiver, .. } if *receiver == owner
    )));
    assert!(
        !sim.sound_events[event_start..].iter().any(|event| {
            matches!(event, crate::sim::world::SimSoundEvent::VocAt { sound_id, .. }
            if sound_id == "FixtureCreated")
        }),
        "refunded/disposed head and newly promoted limbo head play no creation cue"
    );
    assert_constructor_words(&sim, successor, &mut expected_scenario);
    assert_eq!(sim.main_rng.logical_state(), main);
    assert_eq!(sim.mapgen_rng.logical_state(), mapgen);
}

/// Bounded production loop: the declared lab's startup pool has one infantry,
/// so its fallback grants an otherwise unbuildable type without an override.
/// Existing owners perform capture, payment, save restoration and loss ABANDON.
#[test]
fn secret_lab_fallback_capture_paid_queue_snapshot_and_loss_share_the_existing_owners() {
    use crate::sim::house_state::HouseState;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::{FrameEffects, SnapshotProcessState};

    let mut ini = manager_ini();
    ini.merge(&IniFile::from_str(
        "[BuildingTypes]\n7=LAB\n\
        [LAB]\nStrength=100\nCapturable=yes\nNeedsEngineer=yes\nSecretLab=yes\n\
        [General]\nSecretInfantry=E1\n[E1]\nTechLevel=-1\n[Clear]\nBuildable=yes\n",
    ));
    let (mut sim, rules, owner) = world_with(RuleSet::from_ini(&ini).unwrap(), 0x68c0_5001);
    let other = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(other, 1, Some(other), false, 0, 10));
    // Ordinary building Unlimbo needs buildable Clear and a cell inside
    // the supplied Size diamond. This also publishes its native threat cache.
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let lab = sim
        .spawn_object("LAB", "Russians", 30, 20, 0, &rules)
        .unwrap();
    sim.assign_secret_lab_production(&rules);
    let fallback = sim
        .substrate
        .entities
        .get(lab)
        .unwrap()
        .secret_lab_fallback()
        .unwrap();
    assert_eq!(sim.interner.resolve(fallback.1), "E1");
    // Native Begin_Production takes already-admitted events and checks the
    // build limit, so the pre-capture gate belongs to the sidebar/Select owner.
    assert!(
        !super::production_tech::build_option_for_owner(&sim, &rules, "Americans", "E1")
            .unwrap()
            .enabled
    );
    sim.change_owner_with_rules(lab, owner, &rules, None, FrameEffects::default());
    assert!(
        super::production_tech::build_option_for_owner(&sim, &rules, "Americans", "E1")
            .unwrap()
            .enabled
    );
    assert_eq!(sim.houses[&owner].base_projection().secret_labs(), [lab]);
    assert!(
        sim.houses[&other]
            .base_projection()
            .secret_labs()
            .is_empty()
    );
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        FrameEffects::default(),
        false
    ));
    assert!(enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "E1",
        FrameEffects::default(),
        false
    ));
    let product = held_id(&sim, owner, ProductionCategory::Infantry);
    for _ in 0..20 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert_eq!(held_id(&sim, owner, ProductionCategory::Infantry), product);
    let progress = sim
        .production
        .factories()
        .view(owner, ProductionCategory::Infantry)
        .unwrap()
        .progress;
    assert!(
        progress > 0 && progress < 54,
        "actual paid work: {progress}"
    );
    assert!(sim.houses[&owner].economy().spent_credits() > 0);
    let bytes = GameSnapshot::save(&sim, 0, rules.simulation_config_hash(), "secret-lab", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_process_state_for_load(SnapshotProcessState::Live(&sim));
    restored.restore_after_snapshot_load().unwrap();
    restored.resolve_type_handles(&rules);
    assert_eq!(
        restored
            .substrate
            .entities
            .get(lab)
            .unwrap()
            .secret_lab_fallback(),
        Some(fallback)
    );
    assert_eq!(
        restored.houses[&owner].base_projection().secret_labs(),
        [lab]
    );
    assert_eq!(
        held_id(&restored, owner, ProductionCategory::Infantry),
        product
    );
    assert!(
        super::production_tech::build_option_for_owner(&restored, &rules, "Americans", "E1")
            .unwrap()
            .enabled
    );
    let credits = restored.houses[&owner].economy().credits();
    let spent = restored.houses[&owner].economy().spent_credits();
    restored.change_owner_with_rules(lab, other, &rules, None, FrameEffects::default());
    assert!(
        restored.houses[&owner]
            .base_projection()
            .secret_labs()
            .is_empty()
    );
    assert_eq!(
        restored.houses[&other].base_projection().secret_labs(),
        [lab]
    );
    assert_eq!(
        restored
            .substrate
            .entities
            .get(lab)
            .unwrap()
            .secret_lab_fallback(),
        Some(fallback)
    );
    assert!(
        !super::production_tech::build_option_for_owner(&restored, &rules, "Americans", "E1")
            .unwrap()
            .enabled
    );
    // The loss recheck reaches ordinary strip cancellation. Both duplicate
    // requests are removed and the held identity is destroyed, not delivered.
    for _ in 0..2 {
        restored.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert!(!restored.substrate.entities.contains(product));
    assert!(
        restored
            .production
            .factories()
            .view(owner, ProductionCategory::Infantry)
            .is_none()
    );
    assert!(restored.houses[&owner].economy().credits() >= credits);
    assert!(restored.houses[&owner].economy().spent_credits() >= spent);
    assert_eq!(restored.houses[&owner].economy().credits(), 50_000);
    restored.uninit_with_rules(lab, &rules);
    assert!(
        restored.houses[&other]
            .base_projection()
            .secret_labs()
            .is_empty()
    );
}
