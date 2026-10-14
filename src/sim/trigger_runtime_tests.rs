use super::*;
use std::collections::HashMap;

use crate::map::actions::{ActionMap, MapAction};
use crate::map::entities::EntityCategory;
use crate::map::events::MapEvent;
use crate::map::triggers::{MapTrigger, TriggerDifficulty};
use crate::map::variable_names::{LocalVariable, LocalVariableMap};
use crate::sim::game_entity::GameEntity;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
    ProjectileTarget, TargetExpiryPolicy,
};
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{MasterFrameTestRung, Simulation, TickLane, TriggerInputs};

/// Shared native-shaped arena for synchronous Object and cell-entry consumers.
/// Rules, object construction and owner tracking use production ingress; the
/// three Houses deliberately include two different instances of Country0.
pub(super) fn event_ingress_fixture() -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    [crate::sim::intern::InternedId; 3],
) {
    let mut countries = String::from("[Countries]\n");
    for index in 0..8 {
        countries.push_str(&format!("{index}=Country{index}\n"));
    }
    for index in 0..8 {
        countries.push_str(&format!("[Country{index}]\n"));
    }
    countries.push_str(
        "[VehicleTypes]\n0=TAGUNIT\n[TAGUNIT]\nStrength=100\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    );
    let (mut sim, rules, overlays) =
        crate::sim::world::entry_test_fixture::fixture_with_rules(&countries);
    let houses = std::array::from_fn(|index| {
        let house = sim.interner.intern(&format!("House{index}"));
        let country = sim
            .interner
            .intern(if index == 1 { "Country7" } else { "Country0" });
        sim.houses.insert(
            house,
            crate::sim::house_state::HouseState::new(
                house,
                index as u8,
                Some(country),
                false,
                0,
                10,
            ),
        );
        sim.session.house_order.push(house);
        house
    });
    (sim, rules, overlays, houses)
}

pub(super) fn install_event_ingress_map(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    body: &str,
) -> crate::map::map_file::MapFile {
    let text = format!(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\n{body}"
    );
    let map = crate::map::map_file::MapFile::from_bytes(text.as_bytes()).unwrap();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.install_authored_cell_trigger_tags(&map);
    sim.classify_map_triggers(Some(rules));
    map
}

pub(super) fn spawn_event_ingress_unit(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    house: crate::sim::intern::InternedId,
    cell: (u16, u16),
) -> u64 {
    let owner = sim.interner.resolve(house).to_owned();
    sim.spawn_object_at_height("TAGUNIT", &owner, cell.0, cell.1, 0, 0, rules)
        .expect("fixture cell admits the normal production Unit constructor")
}

/// Native line controls supply a sparse 4x4 cell table and an actor at 3,4.
/// Construct a real active Unit first, then set its declared coordinate through
/// the shared Foot owner before restricting the native cell-array rectangle.
/// Retain the loaded terrain and its matching overlay/path/bridge caches; the
/// compared marker and scan slices visit only the declared 4x4 cells.
/// A bare test constructor
/// remains in limbo, so Unit PerCell correctly refuses it before the Foot tail.
pub(super) fn sparse_line_ingress_fixture() -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    [crate::sim::intern::InternedId; 3],
    u64,
) {
    let (mut sim, rules, _, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    sim.foot_set_location_marked(
        actor,
        crate::sim::components::DriveCoord {
            x: 3 * 256,
            y: 4 * 256,
            z: 0,
        },
        Some(&rules),
        None,
        FrameEffects::default(),
    );
    sim.session.map_width = 5;
    sim.session.map_height = 5;
    (sim, rules, houses, actor)
}

pub(super) fn event_ingress_damage(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
    victim: u64,
    source: u64,
    damage: i32,
    effects: FrameEffects<'_>,
) {
    let warhead = sim.interner.intern("SA");
    sim.commit_direct_damage_receiver(
        rules,
        Some(overlays),
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            victim,
            damage,
            0,
            source,
            None,
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        effects,
    );
}

#[test]
fn object_damage_tags_spring_before_techno_and_reread_the_source_house_index() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .health
        .set_for_test(80);
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nDAMAGE=Country0,SOURCE,Damage,0,1,1,1,0\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
         [Tags]\nTAG=2,Damage,DAMAGE\n\
         [Events]\nDAMAGE=1,6,0,0\nSOURCE=1,44,0,1\n\
         [Actions]\nDAMAGE=2,14,0,7,0,0,0,0,A,28,0,6,0,0,0,0,A\nSOURCE=1,28,0,44,0,0,0,0,A\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    sim.attach_object_trigger_tag(source, Some("TAG"));
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(victim)];
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        source,
        1,
        FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().health.current(),
        79
    );
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().owner(),
        houses[1]
    );
    assert_eq!(
        sim.substrate.entities.get(source).unwrap().owner(),
        houses[1]
    );
    assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 6), (tag, 44)]);
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([6, 44]));
    assert!(
        !sim.substrate
            .entities
            .get(victim)
            .unwrap()
            .ai_sell_repair
            .was_attacked_by_enemy(),
        "the Techno continuation must consume the changed, now-allied owners"
    );
}

#[test]
fn object_damage_tag_owner_change_precedes_the_hostile_hit_latch() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nDAMAGE=Country0,<none>,Damage,0,1,1,1,0\n\
         [Tags]\nTAG=2,Damage,DAMAGE\n[Events]\nDAMAGE=1,6,0,0\n\
         [Actions]\nDAMAGE=1,14,0,7,0,0,0,0,A\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        source,
        1,
        FrameEffects::default(),
    );
    let victim = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(victim.owner(), houses[1]);
    assert!(
        !victim.ai_sell_repair.was_attacked_by_enemy(),
        "capturing pre-Tag hostility would latch an enemy hit after ownership changed"
    );
}

#[test]
fn object_damage_event44_uses_source_house_index_instead_of_country_or_victim() {
    for (victim_house, source_house, expected) in [(0, 1, true), (1, 0, false)] {
        let (mut sim, rules, overlays, houses) = event_ingress_fixture();
        let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[victim_house], (15, 15));
        let source = spawn_event_ingress_unit(&mut sim, &rules, houses[source_house], (17, 15));
        install_event_ingress_map(
            &mut sim,
            &rules,
            "[Triggers]\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
             [Tags]\nTAG=2,Source,SOURCE\n[Events]\nSOURCE=1,44,0,1\n\
             [Actions]\nSOURCE=1,28,0,44,0,0,0,0,A\n",
        );
        sim.attach_object_trigger_tag(victim, Some("TAG"));
        event_ingress_damage(
            &mut sim,
            &rules,
            &overlays,
            victim,
            source,
            1,
            FrameEffects::default(),
        );
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&44),
            expected,
            "Event44 has its own source object, independent of actor and authored incoming Country"
        );
    }
}

#[test]
fn fatal_receiver_does_not_raise_surviving_damage_event44() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nDAMAGE=Country0,SOURCE,Damage,0,1,1,1,0\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
         [Tags]\nTAG=2,Damage,DAMAGE\n[Events]\nDAMAGE=1,6,0,0\nSOURCE=1,44,0,0\n\
         [Actions]\nDAMAGE=1,28,0,6,0,0,0,0,A\nSOURCE=1,28,0,44,0,0,0,0,A\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        source,
        100,
        FrameEffects::default(),
    );
    // RecordKill703000 legitimately raises victim6 with sourceNULL. It must
    // not admit the separate surviving Object6/44 pair after a fatal result.
    assert!(sim.trigger_runtime.globals_set.contains(&6));
    assert!(!sim.trigger_runtime.globals_set.contains(&44));
    assert!(
        sim.trigger_runtime
            .dispatch_trace
            .iter()
            .all(|call| call.1 != 44)
    );
    assert!(
        sim.substrate
            .entities
            .get(victim)
            .is_none_or(|entity| entity.health.current() <= 0)
    );
}

#[test]
fn entered_cell_retains_the_actual_duplicate_country_house_for_9001_actions() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
    let other = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nENTER=Country0,<none>,Enter,0,1,1,1,0\n\
         [Tags]\nTAG=2,Enter,ENTER\n[Events]\nENTER=1,1,0,-1\n\
         [Actions]\nENTER=2,14,0,9001,0,0,0,0,A,3,0,9001,0,0,0,0,A\n\
         [CellTags]\n15015=TAG\n",
    );
    sim.attach_object_trigger_tag(actor, Some("TAG"));
    sim.attach_object_trigger_tag(other, Some("TAG"));
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.substrate.entities.get(other).unwrap().owner(),
        houses[2]
    );
    assert!(sim.houses[&houses[2]].ai_activation.production);
    assert!(!sim.houses[&houses[0]].ai_activation.production);
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(actor)];
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    assert_eq!(
        sim.trigger_runtime.trigger(live).unwrap().retained_house,
        Some(houses[2])
    );
    assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 1), (tag, 59)]);
}

#[test]
fn line_arrival_rereads_later_cell_attachments_after_real_tag_cleanup() {
    let (mut sim, rules, _, actor) = sparse_line_ingress_fixture();
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nLINE=Country0,<none>,Line,0,1,1,1,0\n\
         [Tags]\nTAG=0,Line,LINE\n[Events]\nLINE=1,26,0,0\n\
         [Actions]\nLINE=1,28,0,26,0,0,0,0,A\n\
         [CellTags]\n2003=TAG\n3003=TAG\n",
    );
    let mut terrain = sim.resolved_terrain.take().unwrap();
    sim.initialize_cell_trigger_lines(
        &mut terrain,
        (1..5).flat_map(|y| (1..5).map(move |x| (x, y))),
    );
    sim.resolved_terrain = Some(terrain);
    let first = sim.trigger_cell_identity((3, 2));
    let tag = sim.trigger_runtime.cell_tags[&first];
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 26)]);
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([26]));
    assert!(
        sim.trigger_runtime.cell_tags.is_empty(),
        "repeat0 removes every attachment of the shared Tag while the row scan is active"
    );
    let later = sim.trigger_cell_identity((3, 3));
    assert_ne!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_flags(later)
            & 0x200000,
        0,
        "line bits remain authored map attributes after Tag detach"
    );
    sim.trigger_runtime.dispatch_trace.clear();
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert!(sim.trigger_runtime.dispatch_trace.is_empty());
}

/// Drive the production dispatch owner at a chosen native frame without
/// advancing unrelated simulation phases in focused action tests.
fn advance_trigger_frame(
    sim: &mut Simulation,
    current_frame: u32,
    events: &EventMap,
    actions: &ActionMap,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    waypoints: &HashMap<u32, crate::map::waypoints::Waypoint>,
) -> Vec<TriggerEffect> {
    sim.session.binary_frame = current_frame;
    sim.session.install_waypoints(waypoints);
    if sim.resolved_terrain.is_none()
        && actions.values().any(|record| {
            record
                .entries
                .iter()
                .any(|action| matches!(action.kind, 48 | 112))
        })
    {
        // Supply native Cell storage for camera actions. Unset valid waypoints
        // retain the original(0,0) cell; no diagnostic waypoint shadow is used.
        sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    }
    sim.advance_triggers(
        TriggerInputs {
            events,
            actions,
            rules,
        },
        None,
        FrameEffects::default(),
    )
}

fn flat_trigger_playfield_terrain(
    width: u16,
    height: u16,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(
        width,
        height,
        crate::map::resolved_terrain::test_clear_cell,
    )
}

fn make_trigger(
    id: &str,
    linked_trigger_id: Option<&str>,
    name: &str,
    enabled: bool,
) -> MapTrigger {
    MapTrigger {
        id: id.to_string(),
        fields: vec![
            "Neutral".to_string(),
            linked_trigger_id.unwrap_or("<none>").to_string(),
            name.to_string(),
            if enabled { "0" } else { "1" }.to_string(),
            "1".to_string(),
            "1".to_string(),
            "1".to_string(),
            "0".to_string(),
        ],
        owner: Some("Neutral".to_string()),
        linked_trigger_id: linked_trigger_id.map(|value| value.to_ascii_uppercase()),
        name: Some(name.to_string()),
        enabled,
        difficulty: TriggerDifficulty {
            easy: true,
            medium: true,
            hard: true,
        },
    }
}

fn spawn_type(sim: &mut Simulation, type_id: &str) -> u64 {
    let sid = sim.allocate_stable_id();
    let owner_id = sim.interner.intern("Neutral");
    let type_id_interned = sim.interner.intern(type_id);
    let ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        0,
        0,
        0,
        0,
        owner_id,
        crate::sim::combat::Health::new(100),
        type_id_interned,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(ge);
    sid
}

#[test]
fn create_building_type10_reaches_placement_and_house_publication() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;

    // ALL01's cannon restoration uses type10, not type9. Original
    // Read6DD5B0/Execute125/6E4200 binds the inline+54 and alphabetic+44
    // independently. This is a Rust production-chain regression; the native
    // reader outputs are pinned separately in trigger_inline_type.json.
    // Reuse the complete shared placement fixture (native tile catalog,
    // passability, overlays and usable area), rather than a terrain-only
    // camera fixture. Type/art data still enter the production readers.
    let (mut sim, rules, overlays) =
        crate::sim::world::entry_test_fixture::fixture_with_rules_and_fixed_art(
            "[BuildingTypes]\n1=GTGCAN\n[GTGCAN]\nStrength=500\n",
            &IniFile::from_str("[GTGCAN]\nFoundation=1x1\n"),
        );
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, Some(owner), false, 0, 10));
    sim.session.house_order.push(owner);
    sim.session.install_waypoints(&HashMap::from([(
        139,
        crate::map::waypoints::Waypoint {
            index: 139,
            rx: 10,
            ry: 10,
        },
    )]));
    assert!(crate::sim::build_site::can_place_building_at(
        &sim,
        &rules,
        Some(&overlays),
        rules.object("GTGCAN").unwrap(),
        (10, 10),
        Some(owner),
    ));
    assert!(sim.trigger_waypoint_coord(139).is_some());
    let ini = IniFile::from_str("[Actions]\nRESTORE=1,125,10,GTGCAN,0,0,0,0,EJ\n");
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("RESTORE")
        .unwrap()
        .entries
        .remove(0);
    let action = sim
        .trigger_runtime
        .definitions
        .resolve_action(entry, None, None);
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: Some(owner),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(&rules),
        Some(&overlays),
        FrameEffects::default(),
    ));
    let building = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.category() == EntityCategory::Structure && entity.owner() == owner)
        .expect("CreateBuilding must publish the building, not silently reject an empty type");
    assert_eq!(sim.resolve(building.type_ref()), "GTGCAN");
    assert_eq!((building.position.rx, building.position.ry), (10, 10));
    assert!(building.lifecycle.object_alive() && building.lifecycle.cell_marked);
    assert!(!building.lifecycle.in_limbo());
    assert!(
        sim.houses[&owner]
            .base_projection
            .buildings()
            .contains(&building.stable_id())
    );
}

#[test]
fn destroyed_house_predicates_match_original_independent_signed_counters() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::house_state::HouseState;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_destroyed_house_events.json",
    ))
    .unwrap();
    let mut country_ini = String::from("[Countries]\n");
    for index in 0..8 {
        country_ini.push_str(&format!("{index}=Country{index}\n"));
    }
    for index in 0..8 {
        country_ini.push_str(&format!("[Country{index}]\n"));
    }
    let rules = RuleSet::from_ini(&IniFile::from_str(&country_ini)).unwrap();
    for case in corpus["predicate_controls"].as_array().unwrap() {
        let mut sim = Simulation::new();
        let houses: Vec<_> = (0..3)
            .map(|index| {
                let owner = sim.interner.intern(&format!("House{index}"));
                let country = sim
                    .interner
                    .intern(if index == 1 { "Country7" } else { "Country0" });
                sim.houses.insert(
                    owner,
                    HouseState::new(owner, index, Some(country), false, 0, 10),
                );
                let totals = &case["inputs"]["destruction_counts"][index as usize];
                sim.houses.get_mut(&owner).unwrap().tracking.set_for_test(
                    totals[0].as_i64().unwrap() as i32,
                    &[],
                    (
                        totals[1].as_i64().unwrap() as i32,
                        totals[2].as_i64().unwrap() as i32,
                        totals[3].as_i64().unwrap() as i32,
                    ),
                    &[],
                );
                owner
            })
            .collect();
        sim.session.house_order = case["inputs"]["house_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| houses[index.as_u64().unwrap() as usize])
            .collect();
        let event_ini = IniFile::from_str(&format!(
            "[Events]\nDESTRUCTION={}\n",
            case["raw"].as_str().unwrap()
        ));
        let events = crate::map::events::parse_events(&event_ini);
        sim.initialize_map_triggers(
            &HashMap::from([(
                "DESTRUCTION".to_string(),
                make_trigger("DESTRUCTION", None, "Destruction", true),
            )]),
            &HashMap::new(),
        );
        sim.install_pending_trigger_fixture(TriggerInputs {
            events: &events,
            actions: &HashMap::new(),
            rules: Some(&rules),
        });
        let live = sim.trigger_runtime.trigger_order[0];
        let type_index = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[type_index].events[0].retained_house =
            Some(houses[1]);
        let timer_before = sim.trigger_runtime.trigger(live).unwrap().timer;
        let mut persistence = false;
        sim.session.binary_frame = 123;
        let mut evaluate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: case["inputs"]["raised"].as_i64().unwrap() as i32,
                    actor: None,
                    source: None,
                    cell: (0, 0),
                    force: false,
                },
                case["inputs"]["incoming_house"]
                    .as_u64()
                    .map(|index| houses[index as usize]),
                &mut persistence,
                Some(&rules),
                live,
            )
        };
        let result = if case["inputs"]["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(evaluate)
        } else {
            evaluate(&mut sim)
        };
        assert_eq!(result, case["result"].as_u64().unwrap() != 0, "{case}");
        assert_eq!(
            persistence,
            case["persistence"].as_u64().unwrap() != 0,
            "{case}"
        );
        assert_eq!(
            sim.trigger_runtime.trigger(live).unwrap().timer,
            timer_before
        );
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[type_index].events[0].retained_house,
            Some(houses[1]),
            "9/11 do not replace Event+54: {case}"
        );
    }
}

#[test]
fn damage_at_waypoint_reaches_the_shared_building_receiver() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::house_state::HouseState;

    // ALL01's Damage_Reactor uses +90=364. The trailing waypoint token is
    // deliberately different so it cannot accidentally supply the impact.
    // Native Execute63 -> 6E0490 -> AreaDamage489280 establishes that this
    // action damages a present building; numeric parity is tested separately.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=REACTOR\n[REACTOR]\nStrength=1000\nArmor=concrete\n\
         [CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
         [Super]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    let owner = sim.interner.intern("Neutral");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    let type_id = sim.interner.intern("REACTOR");
    let building = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        0,
        0,
        owner,
        crate::sim::combat::Health::new(769),
        type_id,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(building);
    sim.add_entity_occupancy(1);
    let mut waypoints = HashMap::new();
    waypoints.insert(
        364,
        crate::map::waypoints::Waypoint {
            index: 364,
            rx: 10,
            ry: 10,
        },
    );
    sim.session.install_waypoints(&waypoints);
    let mut definitions = RuntimeDefinitions::default();
    let typ = definitions.trigger_type("DAMAGE").unwrap();
    let ini = IniFile::from_str("[Actions]\nDAMAGE=1,63,0,364,0,0,0,0,A\n");
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("DAMAGE")
        .unwrap()
        .entries
        .remove(0);
    let action = definitions.resolve_action(entry, None, None);
    sim.trigger_runtime.definitions = definitions;
    assert!(sim.apply_live_trigger_action(
        &action,
        sim.trigger_action_context(typ, None, TagEventInput::default(), Some(&rules)),
        &mut Vec::new(),
        Some(&rules),
        None,
        FrameEffects::default(),
    ));
    assert!(sim.substrate.entities.get(1).unwrap().health.current() < 769);
}

#[test]
fn parsed_event_records_match_native_list_and_production_predicates() {
    #[derive(serde::Deserialize)]
    struct NativeCondition {
        kind: i32,
        value: i32,
        name: String,
        results: Vec<u8>,
    }
    #[derive(serde::Deserialize)]
    struct NativeRow {
        raw: String,
        conditions: Vec<NativeCondition>,
        frames: Vec<u32>,
    }
    let rows: Vec<NativeRow> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_event_records.json",
    ))
    .unwrap();
    for row in rows {
        let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[Triggers]\nEVENT=Neutral,<none>,Event,0,1,1,1,0\n\
             [Events]\nEVENT={}\n[Actions]\nEVENT=1,112,0,0,0,0,0,0,A\n",
            row.raw
        ));
        let triggers = crate::map::triggers::parse_triggers(&ini);
        let events = crate::map::events::parse_events(&ini);
        let actions = crate::map::actions::parse_actions(&ini);
        let conditions = &events["EVENT"].conditions;
        assert_eq!(conditions.len(), row.conditions.len(), "{}", row.raw);
        for (rust, native) in conditions.iter().zip(&row.conditions) {
            assert_eq!(
                (
                    rust.kind,
                    rust.value,
                    rust.type_name.as_deref().unwrap_or("")
                ),
                (native.kind, native.value, native.name.as_str()),
                "{}",
                row.raw
            );
        }
        // Timer events need the live Trigger instance port. Type-count reader
        // coverage here does not certify the existing registry/count query.
        if conditions.is_empty()
            || !row.conditions.iter().all(|c| {
                matches!(c.kind, 27 | 28 | 36 | 37 | 47) && c.results.len() == row.frames.len()
            })
        {
            continue;
        }
        for (n, frame) in row.frames.iter().enumerate() {
            let mut sim = Simulation::new();
            sim.session.binary_frame = *frame;
            sim.initialize_map_triggers(&triggers, &HashMap::new());
            sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
            sim.trigger_runtime.globals_set.insert(7);
            sim.trigger_runtime.locals_set.insert(9);
            let result = sim
                .advance_app_frame(
                    &[],
                    None,
                    None,
                    67,
                    TickLane::Ordinary,
                    Some(TriggerInputs {
                        events: &events,
                        actions: &actions,

                        rules: None,
                    }),
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            assert!(result.tick.frame_committed);
            assert_eq!(
                !result.trigger_effects.is_empty(),
                row.conditions.iter().all(|c| c.results[n] != 0),
                "{} at frame {frame}",
                row.raw
            );
        }
    }
}

#[test]
fn parsed_variable_actions_match_native_reader_dispatch_and_restore() {
    #[derive(serde::Deserialize)]
    struct NativeRow {
        raw: String,
        kind: i32,
        value: i32,
        waypoint: i32,
        initial_globals: BTreeSet<u32>,
        initial_locals: BTreeSet<u32>,
        globals: BTreeSet<u32>,
        locals: BTreeSet<u32>,
    }
    let corpus: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    // The same native reader producer also owns reference/EVA/input controls.
    // Select the legacy numeric family before deserializing its required fields.
    let rows: Vec<NativeRow> = corpus
        .into_iter()
        .filter(|row| row.get("case_kind").is_none())
        .map(|row| serde_json::from_value(row).unwrap())
        .collect();
    assert_eq!(rows.len(), 61);
    for row in rows {
        let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[Triggers]\nVARIABLE=Neutral,<none>,Variable,0,1,1,1,0\n\
             [Events]\nVARIABLE=1,47,0,0\n[Actions]\nVARIABLE=1,{}\n",
            row.raw
        ));
        let triggers = crate::map::triggers::parse_triggers(&ini);
        let events = crate::map::events::parse_events(&ini);
        let actions = crate::map::actions::parse_actions(&ini);
        let entry = &actions["VARIABLE"].entries[0];
        assert_eq!(entry.kind, row.kind, "{}", row.raw);
        assert_eq!(entry.literal_value(), Some(row.value), "{}", row.raw);
        assert_eq!(
            entry.waypoint_index.map_or(-1, |value| value as i32),
            row.waypoint,
            "{}",
            row.raw
        );
        let mut original = Simulation::new();
        original.initialize_map_triggers(&triggers, &HashMap::new());
        original.trigger_runtime.globals_set = row.initial_globals;
        original.trigger_runtime.locals_set = row.initial_locals;
        let bytes = GameSnapshot::save(&original, 0, 0, "trigger_values", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        for sim in [&mut original, &mut restored] {
            let before_rng = sim.rng_state();
            let frame = sim
                .advance_master_frame(
                    &[],
                    None,
                    None,
                    67,
                    TickLane::Ordinary,
                    Some(TriggerInputs {
                        events: &events,
                        actions: &actions,

                        rules: None,
                    }),
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap()
                .into_tick();
            assert!(frame.frame_committed);
            assert_eq!(sim.trigger_runtime.globals_set, row.globals, "{}", row.raw);
            assert_eq!(sim.trigger_runtime.locals_set, row.locals, "{}", row.raw);
            assert_eq!(
                sim.rng_state(),
                before_rng,
                "empty native Tag registry draws no RNG"
            );
        }
        assert_eq!(
            original.trigger_runtime, restored.trigger_runtime,
            "{}",
            row.raw
        );
    }
}

#[test]
fn parsed_map_trigger_flags_gate_production_frames_and_survive_restore() {
    // Use actual map text, not hand-built MapTrigger booleans. The enabled
    // and difficulty expectations are established by trigger_type_flags.json.
    let map = crate::map::map_file::MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Triggers]\n\
          ACTIVE=Neutral,<none>,Active,0,1,1,1,0\n\
          DISABLED=Neutral,<none>,Disabled,1,1,1,1,0\n\
          NO_MEDIUM=Neutral,<none>,Other difficulty,0,1,0,1,0\n\
          MISSING=Neutral,<none>,Missing flags\n\
          SHIFTED=Neutral,<none>,Empty token,,0,0,-2,0,0\n\
          [Events]\n\
          ACTIVE=1,47,0,0\nDISABLED=1,47,0,0\nNO_MEDIUM=1,47,0,0\n\
          MISSING=1,47,0,0\nSHIFTED=1,47,0,0\n\
          [Actions]\n\
          ACTIVE=1,112,0,0,0,0,0,0,A\n\
          DISABLED=1,112,0,0,0,0,0,0,B\n\
          NO_MEDIUM=1,112,0,0,0,0,0,0,C\n\
          MISSING=1,112,0,0,0,0,0,0,D\n\
          SHIFTED=1,112,0,0,0,0,0,0,E\n",
    )
    .expect("map loader accepts trigger fixture");
    let mut original = Simulation::new();
    // Supply the synthetic Tags used by this header-reader fixture, then
    // finish live definition/classification installation before saving.
    original.initialize_map_triggers(&map.triggers, &map.local_variables);
    original.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    original.install_pending_trigger_fixture(TriggerInputs {
        events: &map.events,
        actions: &map.actions,
        rules: None,
    });
    assert!(
        original
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .filter(|t| !t.enabled || !t.admits_difficulty(1))
            .map(|t| t.id.as_str())
            .eq(["DISABLED", "MISSING", "NO_MEDIUM"])
    );
    let bytes = GameSnapshot::save(&original, 0, 0, "trigger_flags", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    for sim in [&mut original, &mut restored] {
        // Terrain is a resource-derived authority restored separately from
        // GameSnapshot. Supply the same canonical fixture to both candidates.
        sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
        let frame = sim
            .advance_app_frame(
                &[],
                None,
                None,
                67,
                TickLane::Ordinary,
                Some(TriggerInputs {
                    events: &map.events,
                    actions: &map.actions,

                    rules: None,
                }),
                crate::sim::world::FrameEffects::default(),
            )
            .expect("production frame completes");
        assert!(frame.tick.frame_committed);
        assert_eq!(
            frame.trigger_effects,
            vec![
                TriggerEffect::CameraAtCoord {
                    coord: [128, 128, 0],
                    velocity: None
                },
                TriggerEffect::CameraAtCoord {
                    coord: [128, 128, 0],
                    velocity: None
                },
            ]
        );
    }
    assert_eq!(original.trigger_runtime, restored.trigger_runtime);
}

#[test]
fn change_visible_map_area_uses_native_parameter_indices_and_atoi() {
    let fields = vec![
        "991".to_string(),
        "-774".to_string(),
        "2cells".to_string(),
        "41".to_string(),
        "junk".to_string(),
        "+38tail".to_string(),
        "A".to_string(),
    ];
    assert_eq!(parse_visible_map_area(&fields), Some([2, 41, 0, 38]));
    assert_eq!(parse_visible_map_area(&fields[..5]), None);
}

#[test]
fn trigger_action_40_normalizes_and_refreshes_authority_same_frame() {
    use crate::map::map_file::MapHeader;
    use crate::map::playfield::PlayfieldBounds;
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::pathfinding::zone_map::ZONE_INVALID;

    let triggers: TriggerMap = [(
        "AREA".to_string(),
        make_trigger("AREA", None, "Change visible map area", true),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "AREA".to_string(),
        MapEvent {
            id: "AREA".to_string(),
            fields: vec![],
            conditions: vec![EventCondition {
                kind: 47,
                value: 0,
                ..Default::default()
            }],
        },
    )]
    .into_iter()
    .collect();
    // Stock all06umd-shaped payload: 40,0,0,2,41,56,38,A. With params equal
    // to chunk[1..], ParamType/Param3 are the first two zeroes and LocalSize
    // is exactly params[2..=5].
    let actions: ActionMap = [(
        "AREA".to_string(),
        MapAction {
            id: "AREA".to_string(),
            fields: vec![],
            entries: vec![ActionEntry {
                kind: 40,
                params: ["0", "0", "2", "41", "56", "38", "A"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                waypoint_index: Some(0),
            }],
        },
    )]
    .into_iter()
    .collect();
    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 80,
        height: 58,
        local_left: 2,
        local_top: 4,
        local_width: 76,
        local_height: 48,
    };
    let mut sim = Simulation::new();
    sim.install_playfield_from_map_header(&header);
    let initial_bounds = sim.playfield_bounds.expect("initial playfield");
    let mut terrain = flat_trigger_playfield_terrain(100, 100);
    terrain.recalc_playfield_attributes(initial_bounds);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    sim.resolved_terrain = Some(terrain);
    sim.rebuild_zone_grid(&path_grid);
    sim.initialize_map_triggers(&triggers, &HashMap::new());

    let before_zone = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(MovementZone::Normal))
        .expect("normal zone")
        .zone_at(50, 50, MovementLayer::Ground);
    assert_ne!(before_zone, ZONE_INVALID);
    assert!(
        !sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(50, 50)
            .unwrap()
            .outside_playfield
    );

    let tick = sim
        .advance_master_frame(
            &[],
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(TriggerInputs {
                events: &events,
                actions: &actions,

                rules: None,
            }),
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();

    assert_eq!(
        sim.playfield_bounds,
        Some(PlayfieldBounds {
            base: 80,
            off_fc: 2,
            off_100: 41,
            off_104: 56,
            // Raw 38 clips to Size bottom 17, then the six-cell margin caps 11.
            off_108: 11,
        })
    );
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    assert!(terrain.cell(50, 50).unwrap().outside_playfield);
    assert_eq!(terrain.cell(50, 50).unwrap().zone_type, zone_class::OUTSIDE);
    assert!(!terrain.cell(85, 85).unwrap().outside_playfield);
    assert_eq!(terrain.cell(85, 85).unwrap().zone_type, zone_class::GROUND);
    let zones = sim.zone_grid.as_ref().expect("zones rebuilt");
    let normal = zones.map_for(MovementZone::Normal).expect("normal zone");
    assert_eq!(normal.zone_at(50, 50, MovementLayer::Ground), ZONE_INVALID);
    assert_ne!(normal.zone_at(85, 85, MovementLayer::Ground), ZONE_INVALID);
    assert!(sim.radar_terrain_dirty_cells.is_empty());
    assert_eq!(sim.radar_terrain_dirty_generation, 0);
    assert_eq!(sim.playfield_revision, 1);
    assert_eq!(tick.state_hash, sim.state_hash());

    // FUN_006E21E0 rebuilds radar surfaces for every firing. A second writer
    // must not disappear behind the persistent per-cell dirty-list dedup gate.
    assert!(sim.change_visible_map_area([4, 40, 54, 12], None));
    assert_eq!(sim.playfield_revision, 2);
    assert!(sim.change_visible_map_area([4, 40, 54, 12], None));
    assert_eq!(sim.playfield_revision, 3);
    assert!(sim.radar_terrain_dirty_cells.is_empty());

    let bytes = GameSnapshot::save(&sim, 0, 0, "all06umd.map", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("action-40 snapshot loads")
        .sim;
    assert_eq!(restored.playfield_bounds, sim.playfield_bounds);
    assert_eq!(restored.playfield_size_height, sim.playfield_size_height);
    assert_eq!(restored.playfield_revision, sim.playfield_revision);
}

#[test]
fn techno_playfield_action_40_exact_recompute_and_mobile_reveal_callback() {
    use crate::map::map_file::MapHeader;
    use crate::map::playfield::PlayfieldBounds;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::Health;
    use crate::sim::house_state::HouseState;

    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 80,
        height: 58,
        local_left: 30,
        local_top: 20,
        local_width: 8,
        local_height: 6,
    };
    let mut sim = Simulation::new();
    sim.install_playfield_from_map_header(&header);
    let initial = sim.playfield_bounds.unwrap();
    let expanded = PlayfieldBounds::from_raw_local_size(80, 58, [2, 2, 76, 48]);
    let candidates: Vec<(u16, u16)> = (0u16..100)
        .flat_map(|ry| (0u16..100).map(move |rx| (rx, ry)))
        .filter(|&(rx, ry)| {
            expanded.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
                && !initial.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
        })
        .collect();
    let mobile_cell = candidates
        .get(candidates.len() / 2)
        .copied()
        .expect("expansion adds cells");
    let building_cell = candidates
        .iter()
        .copied()
        .max_by_key(|&(rx, ry)| {
            i32::from(rx).abs_diff(i32::from(mobile_cell.0))
                + i32::from(ry).abs_diff(i32::from(mobile_cell.1))
        })
        .expect("distant expansion cell");
    assert!(mobile_cell.0.abs_diff(building_cell.0) + mobile_cell.1.abs_diff(building_cell.1) > 10);
    let veteran_probe = [(3i32, 0i32), (-3, 0), (0, 3), (0, -3)]
        .into_iter()
        .find_map(|(dx, dy)| {
            let rx = i32::from(mobile_cell.0) + dx;
            let ry = i32::from(mobile_cell.1) + dy;
            (rx >= 0
                && ry >= 0
                && rx < 100
                && ry < 100
                && expanded.contains_height_aware_packed(rx, ry, 0, 0))
            .then_some((rx as u16, ry as u16))
        })
        .expect("effective veteran sight probe");
    // `UpdateReveal @ 0x0070AF50`: the veteran bonus is `ftol(Sight *
    // VeteranSight)` for a `SIGHT`-ability type, so the probe at distance 3
    // needs `1 * 3` and the ability — not the old additive `1 + 3`.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nVeteranSight=3\nRevealByHeight=no\n\
         [VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nVeteranAbilities=SIGHT\n",
    ))
    .expect("effective-sight action fixture");

    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, Some(owner), true, 0, 10));
    sim.fog.width = 100;
    sim.fog.height = 100;
    let mobile_type = sim.interner.intern("MTNK");
    let building_type = sim.interner.intern("GAPOWR");
    let mut mobile = GameEntity::new_at_frame_zero_for_test(
        1,
        mobile_cell.0,
        mobile_cell.1,
        0,
        0,
        owner,
        Health::new(100),
        mobile_type,
        EntityCategory::Unit,
        100,
        1,
        true,
    );
    mobile.lifecycle.set_object_alive_for_test(true);
    mobile.lifecycle.set_in_limbo_for_test(false);
    let mut building = GameEntity::new_at_frame_zero_for_test(
        2,
        building_cell.0,
        building_cell.1,
        0,
        0,
        owner,
        Health::new(100),
        building_type,
        EntityCategory::Structure,
        0,
        1,
        false,
    );
    building.lifecycle.set_object_alive_for_test(true);
    building.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(mobile);
    sim.substrate.entities.insert(building);
    sim.resolved_terrain = Some(flat_trigger_playfield_terrain(100, 100));

    assert!(sim.change_visible_map_area([2, 2, 76, 48], Some(&rules)));
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield);
    assert!(sim.substrate.entities.get(2).unwrap().in_playfield);
    assert!(
        sim.fog
            .is_cell_revealed(owner, mobile_cell.0, mobile_cell.1)
    );
    assert!(
        sim.fog
            .is_cell_revealed(owner, veteran_probe.0, veteran_probe.1),
        "0x0070ADC0 callback must use the shared veteran-adjusted sight, not raw VisionRange"
    );
    assert!(
        !sim.fog
            .is_cell_revealed(owner, building_cell.0, building_cell.1),
        "FUN_006E21E0 excludes BuildingClass from the false-to-true callback"
    );

    assert!(sim.change_visible_map_area([30, 20, 8, 6], Some(&rules)));
    assert!(!sim.substrate.entities.get(1).unwrap().in_playfield);
    assert!(!sim.substrate.entities.get(2).unwrap().in_playfield);
    assert!(sim.change_visible_map_area([2, 2, 76, 48], Some(&rules)));
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield);
    assert!(sim.substrate.entities.get(2).unwrap().in_playfield);
}

#[test]
fn time_trigger_can_center_camera_at_waypoint() {
    let triggers: TriggerMap = [(
        "TRIG_A".to_string(),
        make_trigger("TRIG_A", None, "Timer A", true),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "TRIG_A".to_string(),
        MapEvent {
            id: "TRIG_A".to_string(),
            fields: vec![
                "1".to_string(),
                "47".to_string(),
                "3".to_string(),
                "0".to_string(),
            ],
            conditions: vec![EventCondition {
                kind: 47,
                value: 3,
                ..Default::default()
            }],
        },
    )]
    .into_iter()
    .collect();
    let actions: ActionMap = [(
        "TRIG_A".to_string(),
        MapAction {
            id: "TRIG_A".to_string(),
            fields: vec![
                "1".to_string(),
                "112".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "J".to_string(),
            ],
            entries: vec![ActionEntry {
                kind: 112,
                // Runtime owns the reader-materialized +0x44 value below;
                // changing retained source text must not trigger re-decoding.
                params: vec![
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "ZZ".to_string(),
                ],
                waypoint_index: Some(9),
            }],
        },
    )]
    .into_iter()
    .collect();
    let mut sim = Simulation::new();
    sim.initialize_map_triggers(&triggers, &HashMap::new());

    assert!(
        advance_trigger_frame(&mut sim, 44, &events, &actions, None, &HashMap::new()).is_empty()
    );
    assert_eq!(
        advance_trigger_frame(&mut sim, 45, &events, &actions, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
    assert!(
        advance_trigger_frame(&mut sim, 46, &events, &actions, None, &HashMap::new()).is_empty()
    );
}

#[test]
fn master_frame_polls_triggers_before_logic_houses_commit_and_delete() {
    let triggers: TriggerMap = [(
        "TRIG_A".to_string(),
        make_trigger("TRIG_A", None, "Frame Zero", true),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "TRIG_A".to_string(),
        MapEvent {
            id: "TRIG_A".to_string(),
            fields: vec![],
            conditions: vec![EventCondition {
                kind: 47,
                value: 0,
                ..Default::default()
            }],
        },
    )]
    .into_iter()
    .collect();
    let actions: ActionMap = [(
        "TRIG_A".to_string(),
        MapAction {
            id: "TRIG_A".to_string(),
            fields: vec![],
            entries: vec![ActionEntry {
                kind: 112,
                params: vec![
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "J".to_string(),
                ],
                waypoint_index: Some(9),
            }],
        },
    )]
    .into_iter()
    .collect();
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    sim.initialize_map_triggers(&triggers, &HashMap::new());

    let tick = sim
        .advance_master_frame(
            &[],
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(TriggerInputs {
                events: &events,
                actions: &actions,

                rules: None,
            }),
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();

    assert!(tick.frame_committed);
    assert_eq!(
        sim.take_master_frame_test_trace(),
        vec![
            MasterFrameTestRung::SessionCommands,
            MasterFrameTestRung::Triggers,
            MasterFrameTestRung::TeamScript,
            MasterFrameTestRung::LogicVector,
            MasterFrameTestRung::Houses,
            MasterFrameTestRung::FrameCommit,
            MasterFrameTestRung::PendingDelete,
        ]
    );
    assert_eq!(
        sim.drain_trigger_effects(),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
}

#[test]
fn master_frame_save_load_continues_trigger_projectile_and_delete_state() {
    let triggers: TriggerMap = [(
        "TRIG_A".to_string(),
        make_trigger("TRIG_A", None, "Set Global", true),
    )]
    .into_iter()
    .collect();
    let events: EventMap = [(
        "TRIG_A".to_string(),
        MapEvent {
            id: "TRIG_A".to_string(),
            fields: vec![],
            conditions: vec![EventCondition {
                kind: 47,
                value: 0,
                ..Default::default()
            }],
        },
    )]
    .into_iter()
    .collect();
    let actions: ActionMap = [(
        "TRIG_A".to_string(),
        MapAction {
            id: "TRIG_A".to_string(),
            fields: vec![],
            entries: vec![ActionEntry {
                kind: 28,
                params: vec!["0".to_string(), "13".to_string()],
                waypoint_index: Some(0),
            }],
        },
    )]
    .into_iter()
    .collect();
    let trigger_inputs = TriggerInputs {
        events: &events,
        actions: &actions,

        rules: None,
    };

    let mut original = Simulation::new();
    original.initialize_map_triggers(&triggers, &HashMap::new());
    original
        .advance_master_frame(
            &[],
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(trigger_inputs),
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();
    assert!(original.trigger_runtime.globals_set.contains(&13));

    let projectile_id = original.allocate_stable_id();
    original.admit_projectile(
        projectile_id,
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id: crate::sim::combat::RAD_NO_ATTACKER,
            origin: ProjectileCoord::new(0, 0, 0),
            target: ProjectileTarget::Cell { rx: 4, ry: 0 },
            initial_target_position: ProjectileCoord::new(1024, 0, 0),
            payload: ProjectilePayload::new(
                40,
                crate::sim::intern::InternedId::from_index(0),
                crate::sim::intern::InternedId::from_index(0),
            ),
            speed_leptons_per_frame: 64,
            velocity: crate::sim::projectile::ProjectileVelocity::new(64, 0, 0),
            trajectory: crate::sim::projectile::ProjectileTrajectory::Straight,
            guidance: None,
            visual: crate::sim::projectile::ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    let deleted_id = spawn_type(&mut original, "GAPOWR");
    original.uninit(deleted_id);
    assert_eq!(original.projectiles.len(), 1);
    assert!(
        original
            .substrate
            .pending_delete
            .contains_object(&deleted_id)
    );

    // Native in-scenario load restarts Scenario RNG from Seed0. Continue both
    // branches from that cursor while testing unrelated trigger/lifecycle state.
    original.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_before_save = original.state_hash();
    let bytes = GameSnapshot::save(&original, 0, 0, "test_map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("snapshot loads").sim;
    restored
        .restore_after_snapshot_load()
        .expect("snapshot references and Logic membership restore");
    assert_eq!(restored.state_hash(), hash_before_save);
    assert!(restored.trigger_runtime.globals_set.contains(&13));
    assert_eq!(restored.projectiles.len(), 1);
    assert!(
        restored
            .substrate
            .pending_delete
            .contains_object(&deleted_id)
    );

    let original_tick = original
        .advance_master_frame(
            &[],
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(trigger_inputs),
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();
    let mut replay_log = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: Default::default(),
        version: 1,
        tick_hz: 15,
        seed: original.session.seed,
        map_name: original.session.map_name.clone(),
        rules_hash: 0,
    });
    replay_log.record_tick(original_tick.tick, Vec::new(), original_tick.state_hash);
    let restored_tick = restored
        .advance_master_frame(
            &[],
            None,
            None,
            67,
            TickLane::Ordinary,
            Some(trigger_inputs),
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();

    assert!(original_tick.frame_committed);
    assert!(restored_tick.frame_committed);
    assert_eq!(original.state_hash(), restored.state_hash());
    assert_eq!(original.projectiles.len(), 1);
    assert!(original.substrate.pending_delete.is_empty());

    let mut replayed = GameSnapshot::load(&bytes).expect("snapshot reloads").sim;
    replayed
        .restore_after_snapshot_load()
        .expect("replay snapshot references and Logic membership restore");
    assert_eq!(
        ReplayRunner::run_fixture_master_frame(
            &mut replayed,
            &replay_log,
            None,
            None,
            None,
            67,
            Some(trigger_inputs),
        ),
        vec![original_tick.state_hash],
    );
}

#[test]
fn trigger_runtime_latches_participate_in_state_hash() {
    let mut sim = Simulation::new();
    let before = sim.state_hash();

    sim.trigger_runtime.globals_set.insert(13);

    assert_ne!(sim.state_hash(), before);
}

#[test]
fn elapsed_time_uses_signed_current_frame_divided_by_fifteen() {
    let sim = Simulation::new();
    let one_second = EventCondition {
        kind: 47,
        value: 1,
        ..Default::default()
    };
    let zero_seconds = EventCondition {
        kind: 47,
        value: 0,
        ..Default::default()
    };

    assert!(sim.trigger_runtime.evaluate_event(&zero_seconds, 0, &sim));
    assert!(!sim.trigger_runtime.evaluate_event(&one_second, 14, &sim));
    assert!(sim.trigger_runtime.evaluate_event(&one_second, 15, &sim));
    assert!(
        !sim.trigger_runtime
            .evaluate_event(&one_second, 0x8000_0000, &sim),
        "the native frame counter is divided as a signed 32-bit value"
    );
}

/// Exercise the existing action dispatcher with the production TAction reader.
/// The original Read6DD5B0 puts a type2 token3 reference at +50; token2 is
/// metadata, even when a TriggerType named "2" happens to exist.
fn dispatch_typed_trigger_reference(kind: i32, target_enabled: bool) -> Simulation {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
        "[Actions]\nSOURCE=1,{kind},2,0DD8E18C,0,0,0,0,A\n0DD8E18C=1,28,0,7,0,0,0,0,A\n2=1,28,0,8,0,0,0,0,A\n"
    ));
    let actions = crate::map::actions::parse_actions(&ini);
    let triggers: TriggerMap = [
        (
            "SOURCE".into(),
            make_trigger("SOURCE", None, "Source", true),
        ),
        (
            "0DD8E18C".into(),
            make_trigger("0DD8E18C", None, "Intro", target_enabled),
        ),
        (
            "2".into(),
            make_trigger("2", None, "Metadata decoy", target_enabled),
        ),
    ]
    .into_iter()
    .collect();
    let mut sim = Simulation::new();
    sim.initialize_map_triggers(&triggers, &HashMap::new());
    for index in 0..sim.trigger_runtime.definitions.trigger_types.len() {
        let id = sim.trigger_runtime.definitions.trigger_types[index]
            .id
            .clone();
        let entries = actions
            .get(&id)
            .map_or_else(Vec::new, |r| r.entries.clone());
        sim.trigger_runtime.definitions.trigger_types[index].actions = entries
            .into_iter()
            .map(|a| {
                sim.trigger_runtime
                    .definitions
                    .resolve_action(a, None, None)
            })
            .collect();
    }
    for tag in 0..sim.trigger_runtime.definitions.tag_types.len() {
        sim.construct_trigger_tag(tag);
    }
    let source = sim
        .trigger_runtime
        .definitions
        .trigger_types
        .iter()
        .position(|t| t.id == "SOURCE")
        .unwrap();
    let action = sim.trigger_runtime.definitions.trigger_types[source].actions[0].clone();
    sim.apply_live_trigger_action(
        &action,
        sim.trigger_action_context(source, None, TagEventInput::default(), None),
        &mut Vec::new(),
        None,
        None,
        FrameEffects::default(),
    );
    sim
}

#[test]
fn typed_trigger_reference_force_uses_operand_instead_of_metadata() {
    let sim = dispatch_typed_trigger_reference(22, true);
    assert_eq!(sim.trigger_runtime.globals_set, [7].into_iter().collect());
}

#[test]
fn typed_trigger_reference_enable_uses_operand_instead_of_metadata() {
    let sim = dispatch_typed_trigger_reference(53, false);
    assert!(
        sim.trigger_runtime
            .triggers
            .iter()
            .flatten()
            .find(|t| sim.trigger_runtime.definitions.trigger_types[t.type_index].id == "0DD8E18C")
            .unwrap()
            .enabled
    );
    assert!(
        !sim.trigger_runtime
            .triggers
            .iter()
            .flatten()
            .find(|t| sim.trigger_runtime.definitions.trigger_types[t.type_index].id == "2")
            .unwrap()
            .enabled
    );
}

#[test]
fn typed_trigger_reference_disable_uses_operand_instead_of_metadata() {
    let sim = dispatch_typed_trigger_reference(54, true);
    assert!(
        !sim.trigger_runtime
            .triggers
            .iter()
            .flatten()
            .find(|t| sim.trigger_runtime.definitions.trigger_types[t.type_index].id == "0DD8E18C")
            .unwrap()
            .enabled
    );
    assert!(
        sim.trigger_runtime
            .triggers
            .iter()
            .flatten()
            .find(|t| sim.trigger_runtime.definitions.trigger_types[t.type_index].id == "2")
            .unwrap()
            .enabled
    );
}

#[test]
fn global_actions_can_enable_and_force_followup_trigger() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Set Global", true),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Camera", false),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "47".to_string(),
                    "1".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 47,
                    value: 1,
                    ..Default::default()
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "27".to_string(),
                    "7".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 27,
                    value: 7,
                    ..Default::default()
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions = crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(
        "[Actions]\nTRIG_A=3,28,0,7,0,0,0,0,A,53,2,TRIG_B,0,0,0,0,A,22,2,TRIG_B,0,0,0,0,A\n\
             TRIG_B=1,112,0,0,0,0,0,0,D\n",
    ));
    let mut sim = Simulation::new();
    sim.initialize_map_triggers(&triggers, &HashMap::new());

    assert_eq!(
        advance_trigger_frame(&mut sim, 15, &events, &actions, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
}

#[test]
fn linked_trigger_instances_execute_in_reverse_chain_order() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", Some("TRIG_B"), "Primary", true),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Followup", true),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "47".to_string(),
                    "1".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 47,
                    value: 1,
                    ..Default::default()
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "28".to_string(),
                    "9".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 28,
                    value: 9,
                    ..Default::default()
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "28".to_string(),
                    "0".to_string(),
                    "5".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 28,
                    params: vec![
                        "0".to_string(),
                        "5".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                    ],
                    waypoint_index: Some(0),
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "E".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "E".to_string(),
                    ],
                    waypoint_index: Some(4),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let mut sim = Simulation::new();
    sim.initialize_map_triggers(&triggers, &HashMap::new());

    assert_eq!(
        advance_trigger_frame(&mut sim, 15, &events, &actions, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
}

#[test]
fn force_trigger_bypasses_unmet_event_predicates() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Force", true),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Blocked", true),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "47".to_string(),
                    "1".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 47,
                    value: 1,
                    ..Default::default()
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "27".to_string(),
                    "99".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 27,
                    value: 99,
                    ..Default::default()
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions = crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(
        "[Actions]\nTRIG_A=1,22,2,TRIG_B,0,0,0,0,A\n\
             TRIG_B=1,112,0,0,0,0,0,0,D\n",
    ));
    let mut sim = Simulation::new();
    sim.initialize_map_triggers(&triggers, &HashMap::new());

    assert_eq!(
        advance_trigger_frame(&mut sim, 15, &events, &actions, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
}

#[test]
fn mission_announce_then_end_uses_the_player_house_and_shared_timer() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::house_state::HouseState;

    // Native67/69 change the PlayerPtr's result and existing+298 timer;
    // they do not manufacture a message or directly select a result screen.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=PlayerCountry\n1=OtherCountry\n[PlayerCountry]\n[OtherCountry]\n[AudioVisual]\nSavourDelay=.1\n",
    )).unwrap();
    let mut sim = Simulation::new();
    let player = sim.interner.intern("PlayerHouse");
    let other = sim.interner.intern("OtherHouse");
    let player_country = sim.interner.intern("PlayerCountry");
    let other_country = sim.interner.intern("OtherCountry");
    sim.houses.insert(
        player,
        HouseState::new(player, 0, Some(player_country), true, 0, 10),
    );
    sim.houses.insert(
        other,
        HouseState::new(other, 0, Some(other_country), false, 0, 10),
    );
    sim.session.house_order = vec![player, other];
    sim.session.current_house = Some(player);
    sim.session.game_mode_nonzero = false;
    let mut trigger = make_trigger("RESULT", None, "End Mission", true);
    trigger.owner = Some("OtherCountry".to_string());
    let before = sim.houses[&player].outcome_timer();
    sim.initialize_map_triggers(
        &HashMap::from([("RESULT".to_string(), trigger)]),
        &HashMap::new(),
    );
    let ini = IniFile::from_str(
        "[Events]\nRESULT=1,47,0,1\n[Actions]\nRESULT=2,67,0,0,0,0,0,0,A,69,0,0,0,0,0,0,A\n",
    );
    let events = crate::map::events::parse_events(&ini);
    let actions = crate::map::actions::parse_actions(&ini);
    let effects = advance_trigger_frame(
        &mut sim,
        15,
        &events,
        &actions,
        Some(&rules),
        &HashMap::new(),
    );
    assert!(
        effects.is_empty(),
        "results use the existing House/audio exit path"
    );
    assert!(sim.houses[&player].has_won());
    assert!(!sim.houses[&player].has_lost());
    assert!(!sim.houses[&other].has_won());
    assert!(!sim.houses[&other].has_lost());
    assert_eq!(
        sim.houses[&player].outcome_timer().remaining(15),
        before.remaining(15),
        "67 preserves duration;69 resumes it instead of starting SavourDelay"
    );
    assert!(sim.ready_outcome_for_owner(player).is_none());
    assert!(
        sim.houses
            .get_mut(&player)
            .unwrap()
            .advance_outcome_savour(15, false)
    );
    assert!(sim.ready_outcome_for_owner(player).is_some());
}

#[test]
fn local_variables_seed_and_gate_followup_triggers() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Flip Local", true),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "Uses Local", true),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "37".to_string(),
                    "2".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 37,
                    value: 2,
                    ..Default::default()
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "36".to_string(),
                    "2".to_string(),
                    "0".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 36,
                    value: 2,
                    ..Default::default()
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "56".to_string(),
                    "0".to_string(),
                    "2".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 56,
                    params: vec![
                        "0".to_string(),
                        "2".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                    ],
                    waypoint_index: Some(0),
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "G".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "G".to_string(),
                    ],
                    waypoint_index: Some(6),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let local_variables: LocalVariableMap = [(
        2,
        LocalVariable {
            index: 2,
            name: "BridgeDone".to_string(),
            initially_set: false,
        },
    )]
    .into_iter()
    .collect();
    let mut sim = Simulation::new();
    sim.initialize_map_triggers(&triggers, &local_variables);

    assert_eq!(
        advance_trigger_frame(&mut sim, 0, &events, &actions, None, &HashMap::new()),
        Vec::<TriggerEffect>::new()
    );
    assert!(sim.trigger_runtime.locals_set.contains(&2));
    assert_eq!(
        advance_trigger_frame(&mut sim, 0, &events, &actions, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
}

#[test]
fn techtype_exists_and_not_exists_query_simulation_world() {
    let triggers: TriggerMap = [
        (
            "TRIG_A".to_string(),
            make_trigger("TRIG_A", None, "Need Two Power Plants", true),
        ),
        (
            "TRIG_B".to_string(),
            make_trigger("TRIG_B", None, "No Radar", true),
        ),
    ]
    .into_iter()
    .collect();
    let events: EventMap = [
        (
            "TRIG_A".to_string(),
            MapEvent {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "60".to_string(),
                    "2".to_string(),
                    "GAPOWR".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 60,
                    value: 2,
                    type_name: Some("GAPOWR".to_string()),
                    ..Default::default()
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapEvent {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "61".to_string(),
                    "0".to_string(),
                    "GAAIRC".to_string(),
                ],
                conditions: vec![EventCondition {
                    kind: 61,
                    value: 0,
                    type_name: Some("GAAIRC".to_string()),
                    ..Default::default()
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let actions: ActionMap = [
        (
            "TRIG_A".to_string(),
            MapAction {
                id: "TRIG_A".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "L".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "L".to_string(),
                    ],
                    waypoint_index: Some(11),
                }],
            },
        ),
        (
            "TRIG_B".to_string(),
            MapAction {
                id: "TRIG_B".to_string(),
                fields: vec![
                    "1".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "M".to_string(),
                ],
                entries: vec![ActionEntry {
                    kind: 112,
                    params: vec![
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "0".to_string(),
                        "M".to_string(),
                    ],
                    waypoint_index: Some(12),
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    let mut sim = Simulation::new();
    sim.initialize_map_triggers(&triggers, &HashMap::new());
    spawn_type(&mut sim, "GAPOWR");
    spawn_type(&mut sim, "GAPOWR");

    // Native55AFB0 revisits the live list length after each Tag. Removing
    // TRIG_A shifts TRIG_B into the current slot, which the increment skips
    // until the next Logic visit; both queries must still fire exactly once.
    assert_eq!(
        advance_trigger_frame(&mut sim, 0, &events, &actions, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
    assert_eq!(
        advance_trigger_frame(&mut sim, 1, &events, &actions, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
    assert!(
        advance_trigger_frame(&mut sim, 2, &events, &actions, None, &HashMap::new()).is_empty()
    );
}

fn run_waypoint_action(
    kind: i32,
    token: Option<&str>,
    trigger_country: Option<&str>,
    sim: &mut Simulation,
    waypoints: &HashMap<u32, crate::map::waypoints::Waypoint>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Vec<TriggerEffect> {
    let mut trigger = make_trigger("WAYPOINT_ACTION", None, "Waypoint action", true);
    trigger.owner = trigger_country.map(str::to_string);
    trigger.fields[0] = trigger_country.unwrap_or("").to_string();
    let triggers: TriggerMap = [(trigger.id.clone(), trigger)].into_iter().collect();
    let events: EventMap = [(
        "WAYPOINT_ACTION".to_string(),
        MapEvent {
            id: "WAYPOINT_ACTION".to_string(),
            fields: Vec::new(),
            conditions: vec![EventCondition {
                kind: 47,
                value: 0,
                ..Default::default()
            }],
        },
    )]
    .into_iter()
    .collect();
    let mut params = vec!["0".to_string(); 6];
    if let Some(token) = token {
        params.push(token.to_string());
    }
    let actions: ActionMap = [(
        "WAYPOINT_ACTION".to_string(),
        MapAction {
            id: "WAYPOINT_ACTION".to_string(),
            fields: Vec::new(),
            entries: vec![ActionEntry {
                kind,
                params,
                waypoint_index: crate::map::actions::read_waypoint_token(token),
            }],
        },
    )]
    .into_iter()
    .collect();
    sim.initialize_map_triggers(&triggers, &HashMap::new());
    advance_trigger_frame(sim, 0, &events, &actions, rules, waypoints)
}

fn waypoint_action_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Americans]\nName=United States\n\
         [Russians]\nName=Russian Federation\n",
    ))
    .expect("waypoint-action country registry parses")
}

fn register_waypoint_action_house(
    sim: &mut Simulation,
    name: &str,
    country: Option<&str>,
    alternate_base_center: (u16, u16),
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(name);
    let country = country.map(|country| sim.interner.intern(country));
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, country, false, 0, 10);
    house.base_center = Some((40, 50));
    house.alternate_base_center = alternate_base_center;
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    owner
}

#[test]
fn camera_actions_use_alpha_tokens_and_ctor_zero_not_decimal_indices() {
    for (kind, token, waypoint, immediate) in [(48, "NZ", 389, false), (112, "AA", 26, true)] {
        let mut sim = Simulation::new();
        let waypoints = [(
            waypoint,
            crate::map::waypoints::Waypoint {
                index: waypoint,
                rx: 9,
                ry: 10,
            },
        )]
        .into_iter()
        .collect();
        assert_eq!(
            run_waypoint_action(kind, Some(token), None, &mut sim, &waypoints, None),
            vec![TriggerEffect::CameraAtCoord {
                coord: [2432, 2688, 0],
                velocity: if immediate { None } else { Some(0) }
            }]
        );
    }

    for token in [None, Some("")] {
        let mut sim = Simulation::new();
        assert_eq!(
            run_waypoint_action(112, token, None, &mut sim, &HashMap::new(), None),
            vec![TriggerEffect::CameraAtCoord {
                coord: [128, 128, 0],
                velocity: None
            }]
        );
    }
    for token in [Some("15"), Some("   ")] {
        let mut sim = Simulation::new();
        assert!(run_waypoint_action(112, token, None, &mut sim, &HashMap::new(), None).is_empty());
    }
}

#[test]
fn action_137_replays_all_three_retail_waypoint_fixtures() {
    for (token, index, cell) in [
        ("P", 15, (93, 106)),
        ("NZ", 389, (122, 135)),
        ("AA", 26, (105, 194)),
    ] {
        let mut sim = Simulation::new();
        let rules = waypoint_action_rules();
        let house = register_waypoint_action_house(&mut sim, "HouseA", Some("Americans"), (0, 0));
        let waypoints = [(
            index,
            crate::map::waypoints::Waypoint {
                index,
                rx: cell.0,
                ry: cell.1,
            },
        )]
        .into_iter()
        .collect();

        assert!(
            run_waypoint_action(
                137,
                Some(token),
                Some("americans"),
                &mut sim,
                &waypoints,
                Some(&rules),
            )
            .is_empty()
        );
        assert_eq!(sim.houses[&house].alternate_base_center, cell);
        assert_eq!(sim.houses[&house].base_center, Some((40, 50)));
    }
}

#[test]
fn action_137_uses_first_registration_order_country_match_only() {
    let mut sim = Simulation::new();
    let rules = waypoint_action_rules();
    let first = register_waypoint_action_house(&mut sim, "First", Some("Americans"), (1, 2));
    let second = register_waypoint_action_house(&mut sim, "Second", Some("AMERICANS"), (3, 4));
    let waypoints = [(
        15,
        crate::map::waypoints::Waypoint {
            index: 15,
            rx: 93,
            ry: 106,
        },
    )]
    .into_iter()
    .collect();

    run_waypoint_action(
        137,
        Some("P"),
        Some("americans"),
        &mut sim,
        &waypoints,
        Some(&rules),
    );

    assert_eq!(sim.houses[&first].alternate_base_center, (93, 106));
    assert_eq!(sim.houses[&second].alternate_base_center, (3, 4));
    assert_eq!(sim.houses[&first].base_center, Some((40, 50)));
    assert_eq!(sim.houses[&second].base_center, Some((40, 50)));
}

#[test]
fn action_137_canonicalizes_house_type_alias_and_none_owner() {
    let rules = waypoint_action_rules();
    let waypoints = [(
        15,
        crate::map::waypoints::Waypoint {
            index: 15,
            rx: 93,
            ry: 106,
        },
    )]
    .into_iter()
    .collect();

    let mut alias_sim = Simulation::new();
    let alias_house =
        register_waypoint_action_house(&mut alias_sim, "AliasHouse", Some("Americans"), (0, 0));
    run_waypoint_action(
        137,
        Some("P"),
        Some("united states"),
        &mut alias_sim,
        &waypoints,
        Some(&rules),
    );
    assert_eq!(
        alias_sim.houses[&alias_house].alternate_base_center,
        (93, 106)
    );

    let mut none_sim = Simulation::new();
    let russian =
        register_waypoint_action_house(&mut none_sim, "RussianHouse", Some("Russians"), (7, 8));
    let first_type =
        register_waypoint_action_house(&mut none_sim, "AmericanHouse", Some("Americans"), (0, 0));
    run_waypoint_action(
        137,
        Some("P"),
        Some("<none>"),
        &mut none_sim,
        &waypoints,
        Some(&rules),
    );
    assert_eq!(none_sim.houses[&russian].alternate_base_center, (7, 8));
    assert_eq!(
        none_sim.houses[&first_type].alternate_base_center,
        (93, 106)
    );
}

#[test]
fn action_137_rejects_only_exact_packed_zero_cell() {
    let rules = waypoint_action_rules();
    for cell in [(0, 17), (23, 0)] {
        let mut sim = Simulation::new();
        let house =
            register_waypoint_action_house(&mut sim, "AxisHouse", Some("Americans"), (9, 9));
        let waypoints = [(
            15,
            crate::map::waypoints::Waypoint {
                index: 15,
                rx: cell.0,
                ry: cell.1,
            },
        )]
        .into_iter()
        .collect();
        run_waypoint_action(
            137,
            Some("P"),
            Some("Americans"),
            &mut sim,
            &waypoints,
            Some(&rules),
        );
        assert_eq!(sim.houses[&house].alternate_base_center, cell);
    }
}

#[test]
fn action_137_writes_signed_waypoint_halves_as_raw_nonzero_cell() {
    let rules = waypoint_action_rules();
    let mut sim = Simulation::new();
    let house =
        register_waypoint_action_house(&mut sim, "SignedWaypointHouse", Some("Americans"), (9, 9));
    let waypoints = crate::map::waypoints::parse_waypoints(
        &crate::rules::ini_parser::IniFile::from_str("[Waypoints]\n15=-1001\n"),
    );

    run_waypoint_action(
        137,
        Some("P"),
        Some("Americans"),
        &mut sim,
        &waypoints,
        Some(&rules),
    );

    assert_eq!(
        sim.houses[&house].alternate_base_center,
        (u16::MAX, u16::MAX),
        "signed -1 quotient/remainder halves are nonzero and must be written"
    );
    assert_eq!(sim.houses[&house].base_center, Some((40, 50)));
}

#[test]
fn action_137_invalid_resolution_and_waypoints_do_not_mutate_either_base_cell() {
    fn assert_no_write(
        trigger_country: Option<&str>,
        house_country: Option<&str>,
        token: Option<&str>,
        waypoints: HashMap<u32, crate::map::waypoints::Waypoint>,
        register: bool,
        with_rules: bool,
    ) {
        let mut sim = Simulation::new();
        let rules = waypoint_action_rules();
        let house = register
            .then(|| register_waypoint_action_house(&mut sim, "Americans", house_country, (7, 8)));
        run_waypoint_action(
            137,
            token,
            trigger_country,
            &mut sim,
            &waypoints,
            with_rules.then_some(&rules),
        );
        if let Some(house) = house {
            assert_eq!(sim.houses[&house].alternate_base_center, (7, 8));
            assert_eq!(sim.houses[&house].base_center, Some((40, 50)));
        }
    }

    let valid = || {
        [(
            15,
            crate::map::waypoints::Waypoint {
                index: 15,
                rx: 93,
                ry: 106,
            },
        )]
        .into_iter()
        .collect()
    };
    assert_no_write(None, Some("Americans"), Some("P"), valid(), true, true);
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        valid(),
        true,
        false,
    );
    assert_no_write(Some("Americans"), None, Some("P"), valid(), true, true);
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        valid(),
        false,
        true,
    );
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("15"),
        valid(),
        true,
        true,
    );
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        HashMap::new(),
        true,
        true,
    );
    assert_no_write(
        Some("Americans"),
        Some("Americans"),
        Some("P"),
        [(
            15,
            crate::map::waypoints::Waypoint {
                index: 15,
                rx: 0,
                ry: 0,
            },
        )]
        .into_iter()
        .collect(),
        true,
        true,
    );
    // Matching the House name is insufficient: only HouseState.country owns
    // the native Spring/Find_By_Country_Index resolution.
    assert_no_write(
        Some("Americans"),
        Some("Russians"),
        Some("P"),
        valid(),
        true,
        true,
    );
}

#[test]
fn action_138_clears_only_first_country_match_without_token_or_rng_use() {
    let mut sim = Simulation::new();
    let rules = waypoint_action_rules();
    let first = register_waypoint_action_house(&mut sim, "First", Some("Americans"), (93, 106));
    let second = register_waypoint_action_house(&mut sim, "Second", Some("Americans"), (122, 135));
    let rng_before = sim.scenario_rng.state();

    run_waypoint_action(
        138,
        Some("present but ignored"),
        Some("AMERICANS"),
        &mut sim,
        &HashMap::new(),
        Some(&rules),
    );

    assert_eq!(sim.houses[&first].alternate_base_center, (0, 0));
    assert_eq!(sim.houses[&second].alternate_base_center, (122, 135));
    assert_eq!(sim.houses[&first].base_center, Some((40, 50)));
    assert_eq!(sim.houses[&second].base_center, Some((40, 50)));
    assert_eq!(sim.scenario_rng.state(), rng_before);

    let mut no_match = Simulation::new();
    let house =
        register_waypoint_action_house(&mut no_match, "OnlyHouse", Some("Russians"), (105, 194));
    run_waypoint_action(
        138,
        None,
        Some("Americans"),
        &mut no_match,
        &HashMap::new(),
        Some(&rules),
    );
    assert_eq!(no_match.houses[&house].alternate_base_center, (105, 194));
}

/// HouseClass::Update's Tag raise against the original block executed in
/// `tools/spatial_oracle/house_tag_raise.json`: every eighth signed frame,
/// event 8 with no object, the zero Cell, no force and no source, last Tag
/// first.
#[test]
fn house_tag_raise_matches_original_cadence_and_order() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_tag_raise.json",
    ))
    .unwrap();
    assert_eq!(cases.len(), 19);
    for case in cases {
        let frame = case["input"]["frame"].as_i64().unwrap() as i32;
        let count = case["input"]["tags"].as_i64().unwrap().max(0) as usize;
        let mut sim = Simulation::new();
        let house = sim.interner.intern("Americans");
        sim.trigger_runtime
            .house_tags
            .insert(house, (0..count).collect());
        sim.session.binary_frame = frame as u32;
        sim.raise_house_tags(house, None, None, FrameEffects::default());
        let expected: Vec<(usize, i32)> = case["raises"]
            .as_array()
            .unwrap()
            .iter()
            .map(|raise| {
                assert_eq!(
                    (
                        &raise["object"],
                        &raise["cell"],
                        &raise["force"],
                        &raise["source"]
                    ),
                    (
                        &serde_json::json!(0),
                        &serde_json::json!([0, 0]),
                        &serde_json::json!(0),
                        &serde_json::json!(0)
                    ),
                    "{case}"
                );
                (
                    raise["tag"].as_u64().unwrap() as usize,
                    raise["event"].as_i64().unwrap() as i32,
                )
            })
            .collect();
        assert_eq!(sim.trigger_runtime.dispatch_trace, expected, "{case}");
    }
}
