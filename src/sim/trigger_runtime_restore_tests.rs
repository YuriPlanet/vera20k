//! Native126/6E30C0 -> House50D440 and Foot4DF510 prerequisites.
//! Steam SHA2563e81a617..08600; new control flow is instruction-derived.
//! Existing constructors, coordinate/FNPC kernels and Unlimbo keep their
//! executable comparisons. Supplied-nearby tests assert caller arguments,
//! and do not claim the supplied answer as native placement output.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry, ruleset::RuleSet};
use crate::sim::intern::InternedId;
use crate::sim::movement::{fresh_oracle_seam, ground_pose};
use crate::sim::world::UninitContext;

type Fixture = (Simulation, RuleSet, OverlayTypeRegistry, [InternedId; 3]);

fn fixture() -> Fixture {
    super::tests::event_ingress_fixture()
}

fn install(sim: &mut Simulation, rules: &RuleSet, value: i32) -> usize {
    super::tests::install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nRESTORE=Country0,<none>,Restore,0,1,1,1,0\n\
             [Events]\nRESTORE=1,8,0,0\n\
             [Actions]\nRESTORE=2,126,0,{value},0,0,0,0,Z,56,0,9,0,0,0,0,A\n\
             [Tags]\nRESTORETAG=0,Restore,RESTORE\n"
        ),
    );
    let tag_type = sim
        .trigger_runtime
        .definitions
        .tag_type("RESTORETAG")
        .unwrap();
    let tag = sim.shared_trigger_tag(tag_type);
    sim.trigger_runtime.tag(tag).unwrap().triggers[0]
}

fn record(sim: &mut Simulation, owner: InternedId, kind: &str, at: (i16, i16)) {
    let ty = sim.interner.intern(kind);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .retain_initial_techno(ty, at);
}

fn spawn(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    kind: &str,
    at: (u16, u16),
) -> u64 {
    let name = sim.interner.resolve(owner).to_owned();
    sim.spawn_object_at_height(kind, &name, at.0, at.1, 0, 0, rules)
        .unwrap()
}

fn construct(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, kind: &str) -> u64 {
    let name = sim.interner.resolve(owner).to_owned();
    sim.construct_object_limbo_at_height(kind, &name, 0, 0, 0, 0, rules, FrameEffects::default())
        .unwrap()
}

fn active(sim: &Simulation) -> Vec<(u64, String, InternedId, (i16, i16))> {
    sim.substrate
        .entities
        .iter_sorted()
        .filter(|(_, entity)| entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo())
        .map(|(id, entity)| {
            let coord = ground_pose::position_world_coord(&entity.position);
            (
                id,
                sim.interner.resolve(entity.type_ref()).to_owned(),
                entity.owner(),
                (
                    crate::util::lepton::lepton_to_cell_packed(coord.x),
                    crate::util::lepton::lepton_to_cell_packed(coord.y),
                ),
            )
        })
        .collect()
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

fn apply(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    live: Option<usize>,
    kind: i32,
    value: i32,
    incoming: InternedId,
) -> bool {
    let ini = IniFile::from_str(&format!("[Actions]\nA=1,{kind},0,{value},0,0,0,0,Z\n"));
    let entry = crate::map::actions::parse_actions(&ini)["A"].entries()[0].clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: live,
            incoming_house: Some(incoming),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        Some(registry),
        FrameEffects::default(),
    )
}

#[test]
fn live126_recreates_all_four_initial_categories_after_real_cleanup_in_record_order() {
    for saved_house in [false, true] {
        let (mut sim, rules, registry, houses) = fixture();
        let original = [
            ("CABHUT", (15, 15)),
            ("TAGUNIT", (16, 15)),
            ("ENGINEER", (17, 14)),
            ("HORNET", (17, 16)),
        ];
        let ids: Vec<_> = original
            .iter()
            .map(|&(kind, at)| spawn(&mut sim, &rules, houses[0], kind, at))
            .collect();
        sim.retain_initial_house_technos();
        assert_eq!(sim.houses[&houses[0]].initial_techno_record_count(), 4);
        for &id in &ids {
            sim.uninit_with_context(id, UninitContext::new(Some(&rules), Some(&registry)));
        }
        sim.flush_pending_delete();
        assert!(active(&sim).is_empty());
        if saved_house {
            let bytes = crate::util::save_codec::serialize(&sim.houses[&houses[0]]).unwrap();
            sim.houses.insert(
                houses[0],
                crate::util::save_codec::deserialize(&bytes).unwrap(),
            );
        }
        install(&mut sim, &rules, 0);
        sim.session.binary_frame = 16;
        sim.raise_house_tags(
            houses[0],
            Some(&rules),
            Some(&registry),
            FrameEffects::default(),
        );
        let restored = active(&sim);
        assert_eq!(restored.len(), 4);
        for ((id, kind, owner, cell), (expected, at)) in restored.iter().zip(original) {
            assert!(!ids.contains(id));
            assert_eq!(kind, expected);
            assert_eq!(*owner, houses[0]);
            assert_eq!(*cell, (at.0 as i16, at.1 as i16));
        }
        assert!(sim.trigger_runtime.locals_set.contains(&9));
        assert_eq!(
            sim.houses[&houses[0]].initial_techno_record_count(),
            4,
            "restoration does not append history"
        );
    }
}

#[test]
fn restore126_requires_live_context_and_uses_country_or_retained_house() {
    for (value, retained, have_live, selected) in [
        (0, None, true, Some(0)),
        (7, None, true, Some(1)),
        (8997, Some(2), true, Some(2)),
        (8997, None, true, None),
        (-1, Some(2), true, None),
        (999, None, true, None),
        (0, None, false, None),
    ] {
        let (mut sim, rules, registry, houses) = fixture();
        for (owner, at) in houses.into_iter().zip([(15, 15), (16, 15), (17, 14)]) {
            record(&mut sim, owner, "TAGUNIT", at);
        }
        let live = install(&mut sim, &rules, value);
        sim.trigger_runtime.triggers[live]
            .as_mut()
            .unwrap()
            .retained_house = retained.map(|i| houses[i]);
        let before_rng = rng(&sim);
        assert_eq!(
            apply(
                &mut sim,
                &rules,
                &registry,
                have_live.then_some(live),
                126,
                value,
                houses[1]
            ),
            selected.is_some()
        );
        let restored = active(&sim);
        if let Some(index) = selected {
            assert_eq!(restored.len(), 1);
            assert_eq!(restored[0].2, houses[index]);
        } else {
            assert!(restored.is_empty());
            assert_eq!(rng(&sim), before_rng);
        }
    }
}

#[test]
fn refused_placement_continues_with_next_record_and_following_trigger_action() {
    let (mut sim, rules, registry, houses) = fixture();
    let block = spawn(&mut sim, &rules, houses[1], "CABHUT", (15, 15));
    record(&mut sim, houses[0], "ENGINEER", (16, 15));
    record(&mut sim, houses[0], "TAGUNIT", (15, 15));
    install(&mut sim, &rules, 0);
    sim.session.binary_frame = 8;
    sim.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    let restored = active(&sim);
    assert_eq!(restored.len(), 2);
    assert!(restored.iter().any(|row| row.0 == block));
    assert!(
        restored
            .iter()
            .any(|row| row.1 == "ENGINEER" && row.2 == houses[0])
    );
    assert!(!restored.iter().any(|row| row.1 == "TAGUNIT"));
    assert!(sim.trigger_runtime.locals_set.contains(&9));
    assert!(
        sim.substrate
            .entities
            .values()
            .any(
                |entity| sim.interner.resolve(entity.type_ref()) == "TAGUNIT"
                    && !entity.lifecycle.object_alive()
            )
    );
}

#[test]
fn restore126_always_constructs_a_new_unit_even_when_the_original_is_present() {
    let (mut sim, rules, registry, houses) = fixture();
    let old = spawn(&mut sim, &rules, houses[0], "TAGUNIT", (15, 15));
    sim.substrate
        .entities
        .get_mut(old)
        .unwrap()
        .health
        .set_for_test(7);
    sim.retain_initial_house_technos();
    install(&mut sim, &rules, 0);
    sim.session.binary_frame = 8;
    sim.raise_house_tags(
        houses[0],
        Some(&rules),
        Some(&registry),
        FrameEffects::default(),
    );
    let restored = active(&sim);
    assert_eq!(restored.len(), 2);
    let new = restored.iter().find(|row| row.0 != old).unwrap();
    assert_eq!(new.1, "TAGUNIT");
    assert_eq!(new.3, (15, 15));
    assert_eq!(sim.substrate.entities.get(old).unwrap().health.current(), 7);
    assert_eq!(
        sim.substrate.entities.get(new.0).unwrap().health.current(),
        rules.object("TAGUNIT").unwrap().strength
    );
    assert_eq!(sim.houses[&houses[0]].initial_techno_record_count(), 1);
}

#[test]
fn restore130_still_repairs_only_existing_buildings() {
    let (mut sim, rules, registry, houses) = fixture();
    let building = spawn(&mut sim, &rules, houses[0], "CABHUT", (15, 15));
    let unit = spawn(&mut sim, &rules, houses[0], "TAGUNIT", (16, 15));
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set_for_test(7);
    sim.substrate
        .entities
        .get_mut(unit)
        .unwrap()
        .health
        .set_for_test(7);
    sim.retain_initial_house_technos();
    let live = install(&mut sim, &rules, 0);
    assert!(apply(
        &mut sim,
        &rules,
        &registry,
        Some(live),
        130,
        0,
        houses[1]
    ));
    assert_eq!(active(&sim).len(), 2);
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .health
            .current(),
        rules.object("CABHUT").unwrap().strength
    );
    assert_eq!(
        sim.substrate.entities.get(unit).unwrap().health.current(),
        7
    );
}

#[test]
fn foot_and_building_clearance_submit_the_same_native_no_bridge_nearby_arguments() {
    use crate::rules::locomotor_type::{MovementZone, SpeedType};
    use crate::sim::find_nearby_cell::NearbyFootprint;
    use fresh_oracle_seam::FreshCallRecord;
    for building in [false, true] {
        let (mut sim, rules, registry, houses) = fixture();
        let old = spawn(&mut sim, &rules, houses[1], "TAGUNIT", (15, 15));
        let coord =
            ground_pose::position_world_coord(&sim.substrate.entities.get(old).unwrap().position);
        let created = construct(
            &mut sim,
            &rules,
            houses[0],
            if building { "CABHUT" } else { "TAGUNIT" },
        );
        // Supplied callee answer; expected arguments come from458CE0..458D2D
        // and4DF71A..4DF76A, including literal0 at458CF8/4DF732.
        fresh_oracle_seam::install_nearby_only(vec![Some((16, 15))]);
        let placed = if building {
            sim.place_building_with_cleared_foundation(
                created,
                coord,
                0,
                &rules,
                Some(&registry),
                FrameEffects::default(),
            )
        } else {
            sim.place_foot_with_cleared_cell(
                created,
                coord,
                0,
                &rules,
                Some(&registry),
                FrameEffects::default(),
            )
        };
        let (records, unused) = fresh_oracle_seam::finish();
        assert_eq!(placed, Some(created));
        assert_eq!(unused, 0);
        let calls: Vec<_> = records
            .into_iter()
            .filter(|record| matches!(record, FreshCallRecord::NearbyCell { .. }))
            .collect();
        assert_eq!(
            calls,
            [FreshCallRecord::NearbyCell {
                seed: (15, 15),
                speed_type: SpeedType::Track,
                zone: None,
                movement_zone: MovementZone::Normal,
                bridge_aware_zone: false,
                footprint: NearbyFootprint::SINGLE,
                allow_bridge_cells: false,
                check_height: false,
                check_occupancy: false,
                target_cell: None,
                reject_any_overlay: false,
            }]
        );
        assert!(
            sim.substrate
                .entities
                .get(old)
                .unwrap()
                .lifecycle
                .object_alive()
        );
        assert!(
            !sim.substrate
                .entities
                .get(old)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
        assert_eq!(
            active(&sim).iter().find(|row| row.0 == old).unwrap().3,
            (16, 15)
        );
        assert_eq!(sim.substrate.entities.get(old).unwrap().owner(), houses[1]);
    }
}

#[test]
fn failed_foot_unlimbo_retires_new_object_and_restores_old_raw_location() {
    let (mut sim, rules, registry, houses) = fixture();
    let old = spawn(&mut sim, &rules, houses[1], "TAGUNIT", (15, 15));
    let coord =
        ground_pose::position_world_coord(&sim.substrate.entities.get(old).unwrap().position);
    // Aircraft4196B0, already covered by the original class-entry owner:
    // mode0 + owned-by-current-house + unmapped destination refuses +D8.
    sim.session.game_mode_nonzero = false;
    let created = construct(&mut sim, &rules, houses[0], "HORNET");
    sim.substrate
        .entities
        .get_mut(created)
        .unwrap()
        .discovery
        .set_owned_by_current_house_for_test(true);
    assert!(!sim.fog.is_cell_revealed(houses[0], 15, 15));
    assert!(
        sim.place_foot_with_cleared_cell(
            created,
            coord,
            0,
            &rules,
            Some(&registry),
            FrameEffects::default()
        )
        .is_none()
    );
    assert!(
        !sim.substrate
            .entities
            .get(created)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    let restored = sim.substrate.entities.get(old).unwrap();
    assert!(!restored.lifecycle.in_limbo());
    assert_eq!(ground_pose::position_world_coord(&restored.position), coord);
}

#[test]
fn a_building_blocks_foot_clearance_before_the_new_object_unlimbo() {
    let (mut sim, rules, registry, houses) = fixture();
    let block = spawn(&mut sim, &rules, houses[1], "CABHUT", (15, 15));
    let coord =
        ground_pose::position_world_coord(&sim.substrate.entities.get(block).unwrap().position);
    let created = construct(&mut sim, &rules, houses[0], "TAGUNIT");
    assert!(
        sim.place_foot_with_cleared_cell(
            created,
            coord,
            0,
            &rules,
            Some(&registry),
            FrameEffects::default()
        )
        .is_none()
    );
    let new = sim.substrate.entities.get(created).unwrap();
    assert!(
        new.lifecycle.object_alive() && new.lifecycle.in_limbo(),
        "caller50D507 owns this early-refusal UnInit"
    );
    assert_eq!(
        sim.substrate.entities.get(block).unwrap().category(),
        EntityCategory::Structure
    );
    assert!(
        sim.substrate
            .entities
            .get(block)
            .unwrap()
            .lifecycle
            .object_alive()
    );
}
