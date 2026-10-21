//! Production views, economy queries and changed-output publication.
//!
//! Strip readiness queues mobile PLACE for its frame's event tail; it retains the
//! factory-held identity. Factory accounting settles in factory_lifecycle.

use crate::sim::world::FrameEffects;
use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

use super::PRODUCTION_STEPS;
use super::factory_lifecycle;
use super::production_spawn::{
    ExitArm, dock_exit, exit_arm, mark_war_factory_spawn_contact, naval_exit_cell,
    unlimbo_held_naval_unit,
};
use super::production_types::*;

/// `owner`'s Available_Money, by house name; 0 for a house that does not
/// exist.
pub fn credits_for_owner(sim: &Simulation, owner: &str) -> i32 {
    sim.interner.get(owner).map_or(0, |id| {
        crate::sim::credit_income::available_money(&sim.houses, id)
    })
}

/// `owner`'s house, created as a human house holding [`STARTING_CREDITS`]
/// when the test never made one.
#[cfg(any(test, feature = "test-support"))]
pub(in crate::sim) fn house_for_test<'a>(
    sim: &'a mut Simulation,
    owner: &str,
) -> &'a mut crate::sim::house_state::HouseState {
    let key = sim.interner.intern(owner);
    sim.houses.get_or_register_for_test(key, || {
        crate::sim::house_state::HouseState::new(key, 0, None, true, STARTING_CREDITS, 10)
    })
}

pub fn power_balance_for_owner(sim: &Simulation, _rules: &RuleSet, owner: &str) -> (i32, i32) {
    // Read from cached PowerState (health-scaled output, full-rated drain).
    // Updated each tick by power_system::tick_power_states().
    let Some(owner_id) = sim.interner.get(owner) else {
        return (0, 0);
    };
    sim.power_states
        .get(&owner_id)
        .map(|state| state.effective_totals())
        .unwrap_or((0, 0))
}

/// The options the player's sidebar shows for `owner`, by category
/// ([`BuildOption::visible_in_sidebar`]).
pub fn build_options_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> Vec<BuildOption> {
    let options: Vec<BuildOption> =
        super::production_tech::all_build_options_for_owner(sim, rules, owner);

    // Diagnostic: log reason breakdown when nothing is buildable.
    let enabled_count = options.iter().filter(|o| o.enabled).count();
    if enabled_count == 0 && sim.session.tick().is_multiple_of(90) {
        let mut reason_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for opt in &options {
            let key = match &opt.reason {
                Some(BuildDisabledReason::NoFactory) => "NoFactory",
                Some(BuildDisabledReason::CannotBuild) => "CannotBuild",
                Some(BuildDisabledReason::NoReadyFactory) => "NoReadyFactory",
                Some(BuildDisabledReason::AtBuildLimit) => "AtBuildLimit",
                None => "Enabled",
            };
            *reason_counts.entry(key).or_default() += 1;
        }
        log::warn!(
            "[BUILD-DIAG] owner='{}' tick={} total_items={} reasons={:?}",
            owner,
            sim.session.tick(),
            options.len(),
            reason_counts
        );
        // Log owned structures and their factory status.
        for e in sim.substrate.entities.values() {
            if sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
                && e.category() == crate::map::entities::EntityCategory::Structure
            {
                let ts = sim.interner.resolve(e.type_ref());
                log::warn!(
                    "[BUILD-DIAG]   structure '{}' building_up={} factory_type={:?}",
                    ts,
                    e.building_up(),
                    rules.factory_type(ts)
                );
            }
        }
        // Log a few sample failures to show the exact reason per item.
        for opt in options.iter().filter(|o| !o.enabled).take(5) {
            let type_str = sim.interner.resolve(opt.type_id);
            log::warn!(
                "[BUILD-DIAG]   sample: '{}' reason={:?}",
                type_str,
                opt.reason
            );
        }
    }

    options
        .into_iter()
        .filter(BuildOption::visible_in_sidebar)
        .collect()
}

/// True if this owner has at least one buildable production option — useful
/// for picking a likely local player house in UI code.
pub fn has_build_option_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> bool {
    super::production_tech::all_build_options_for_owner(sim, rules, owner)
        .iter()
        .any(|o| o.enabled)
}

/// StripClass::AI6A8DD3 consumes Factory::HasChanged4C9C60, announces a
/// completed building, and issues mobile PLACE6A8EB8 without releasing its head.
/// The next frame's input prefix visits Strip before Logic/Factory and appends
/// PLACE after already accepted OutList events, for that frame's
/// Event4C710B -> House4FB0E0 tail. Native controls observe completion267/484
/// and PLACE268/485; admitted SetRally then PLACE inherits the new Archive.
///
/// Sidebar6A77B6 visits tabs0..3: Infantry(tab2) precedes Unit/Aircraft(tab3).
/// Within tab3, native entry-array order and saturated OutList transport remain
/// recorded residuals; one infantry factory has no cross-category ambiguity.
pub fn publish_production_changes(sim: &mut Simulation, rules: &RuleSet) {
    let mut completed = sim.production.factories_mut().take_changed_completed_keys();
    completed.sort_by_key(|(_, category)| match category {
        ProductionCategory::Building => 0,
        ProductionCategory::Defense => 1,
        ProductionCategory::Infantry => 2,
        ProductionCategory::Vehicle | ProductionCategory::Aircraft | ProductionCategory::Ship => 3,
    });
    for (owner, category) in completed {
        let Some(type_id) = sim
            .production
            .factories()
            .view(owner, category)
            .and_then(|factory| factory.object.map(|object| object.type_id))
        else {
            continue;
        };
        factory_lifecycle::publish_completion(
            &sim.interner,
            &mut sim.production,
            &mut sim.sound_events,
            rules,
            owner,
            category,
        );
        if sim.object_type(type_id, rules).is_some_and(|object| {
            object.category != crate::rules::object_type::ObjectCategory::Building
        }) {
            let _ = sim.issue_local_command(crate::sim::command::CommandEnvelope::new(
                owner,
                sim.session.tick().saturating_add(1),
                crate::sim::command::Command::PlaceProducedMobile { category },
            ));
        }
    }
}

/// Exercise the next frame's prefix publication and its event tail in focused owner tests,
/// without running unrelated world AI. The ordinary production runtime uses
/// publish_production_changes plus the world's existing scheduled tail.
#[cfg(any(test, feature = "test-support"))]
pub fn dispatch_production_changes_for_tests(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    sim.session
        .set_tick_for_test(sim.session.tick().saturating_add(1));
    sim.session
        .set_binary_frame_for_test(sim.session.binary_frame().wrapping_add(1));
    publish_production_changes(sim, rules);
    let due = sim.take_due_commands();
    sim.transfer_local_commands();
    let (_, spawned) = sim.apply_due_commands(
        &due,
        rules,
        sim.session.tick().saturating_add(1),
        overlay_registry,
        crate::sim::world::FrameEffects::default(),
    );
    spawned
}

/// BuildingClass::ExitObject_Main443C60 for one retained object.
/// Human House PLACE and AI Building::Factory_AI use this same exit owner;
/// their refund/retry/release policies remain with their respective callers.
/// A Building or Aircraft takes its own class's arm; a Unit or Infantry the
/// arm its producer's type picks ([`exit_arm`]).
pub(in crate::sim) fn exit_produced_object(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    stable_id: u64,
    exit_cell: (i16, i16),
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> crate::sim::ai_base_building::BuildingExit {
    use crate::sim::ai_base_building::{self, BuildingExit};
    use crate::sim::ai_unit_choice::UnitChoiceKind;
    let Some(entity) = sim.substrate.entities.get_mut(stable_id) else {
        return BuildingExit::Failed;
    };
    // Exit443C81 sets Techno+3D5 before RTTI, admission or Unlimbo.
    entity.in_playfield.enter();
    let product_type_id = entity.type_ref();
    let Some(product_type) = sim.object_type(product_type_id, rules) else {
        return BuildingExit::Failed;
    };
    // The native RTTI6 jump-table arm delegates to the existing building
    // exit owner. Human sentinel-cell PLACE and AI Factory_AI share this
    // dispatch instead of copying either class's exit path.
    if product_type.category == crate::rules::object_type::ObjectCategory::Building {
        return ai_base_building::exit_building(
            sim,
            rules,
            producer_id,
            stable_id,
            overlay_registry,
            effects,
        );
    }
    let Some(kind) = UnitChoiceKind::of(product_type.category) else {
        return BuildingExit::Failed;
    };
    let Some((owner_id, producer_type)) =
        sim.substrate
            .entities
            .get(producer_id)
            .and_then(|producer| {
                sim.object_type(producer.type_ref(), rules)
                    .map(|object| (producer.owner(), object))
            })
    else {
        return BuildingExit::Failed;
    };
    //444096..4440D7 skips FreeRadio65ADC0 for Hospital, Armory and WeaponsFactory.
    //The direct4440BC call accepts any NULL contact slot; stock GAPILE has1.
    // Ordinary barracks return1 while occupied; land war factories reach
    // their own native busy/alternate-receiver arm instead.
    if product_type.category != crate::rules::object_type::ObjectCategory::Aircraft
        && !producer_type.hospital
        && !producer_type.armory()
        && !producer_type.weapons_factory
        && sim
            .substrate
            .entities
            .get(producer_id)
            .is_some_and(|producer| producer.radio_contacts.first_free().is_none())
    {
        return BuildingExit::TryLater;
    }
    //443CBC/4440FB use the producer's House, even when a reinforcement
    //belongs to another House. Hospital/Armory skip this Unit/Infantry
    //prelude (4440DD..444137); the earlier Aircraft arm always runs it.
    if kind == UnitChoiceKind::Aircraft || !producer_type.hospital && !producer_type.armory() {
        ai_base_building::economy_state_machine(
            &sim.interner,
            &sim.session,
            &mut sim.scenario_rng,
            &mut sim.houses,
            &sim.power_states,
            rules,
            owner_id,
            false,
        );
        if let Some(house) = sim.houses.get_mut(&owner_id) {
            house.ai_unit_choices_mut().clear(kind);
        }
    }
    if kind == UnitChoiceKind::Aircraft {
        let airport_bound = product_type.airport_bound;
        return exit_aircraft(
            sim,
            rules,
            producer_id,
            stable_id,
            airport_bound,
            overlay_registry,
            effects,
        );
    }
    match exit_arm(producer_type) {
        ExitArm::Refinery => {
            if kind == UnitChoiceKind::Infantry {
                //444EF8..444F16: a non-Unit scatters with the empty
                //coordinate, forced=1/noKidding=0, then returns0. Its caller
                //owns deletion; constructor and Scatter RNG stay spent.
                if let Err(error) = sim.scatter(
                    stable_id,
                    None,
                    crate::sim::movement::ScatterFlags::new(true, false),
                    rules,
                    overlay_registry,
                    effects,
                ) {
                    log::warn!("refinery exit Scatter for {stable_id}: {error}");
                }
            }
            //RESIDUAL: the Unit arm444DF4..444EF5 places at GetCoords Cell
            //plus the native SW/S offsets, facing0xA0, then queues Harvest;
            //it also returns0. Ordinary retail producing factories have no
            //Refinery/Weeder flag. The reinforcement route admits Infantry
            //only, so it uses the complete non-Unit arm above. GAREFN/NAREFN
            //themselves do have retail Refinery=yes.
            BuildingExit::Failed
        }
        ExitArm::Naval => exit_naval(
            sim,
            rules,
            producer_id,
            stable_id,
            product_type.speed_type,
            overlay_registry,
            effects,
        ),
        ExitArm::Land => exit_land(
            sim,
            rules,
            producer_id,
            stable_id,
            overlay_registry,
            effects,
        ),
        arm @ (ExitArm::Infantry | ExitArm::Generic) => exit_at_dock(
            sim,
            rules,
            arm,
            producer_id,
            stable_id,
            exit_cell,
            product_type,
            overlay_registry,
            effects,
        ),
    }
}

/// ExitObject's naval arm (`0x00444175..0x00444492`): an uncontacted yard
/// requests deferred Unload before finding an exit, then successful Unlimbo
/// and rally assignment end with Mark(0), Cell center SetLocation, Mark(1).
/// Original execution on stock GAYARD/DEST and its deferred animation cleanup:
/// `tools/sidebar_oracle/factory_naval_output.md` (gamemd).
fn exit_naval(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    stable_id: u64,
    speed_type: crate::rules::locomotor_type::SpeedType,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> crate::sim::ai_base_building::BuildingExit {
    use crate::sim::ai_base_building::BuildingExit;
    // 444175/65AE30 scans every actual radio slot. This request precedes
    // FNPC and Unlimbo, so even a refused delivery leaves it queued.
    if sim
        .substrate
        .entities
        .get(producer_id)
        .is_some_and(|producer| producer.radio_contacts.is_empty())
    {
        let _ = sim.mission_queue_exact(
            producer_id,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Unload),
            0,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
    }
    let Some(cell) = naval_exit_cell(sim, producer_id, speed_type) else {
        return BuildingExit::Failed;
    };
    let Some(spawned) =
        unlimbo_held_naval_unit(sim, rules, stable_id, cell, overlay_registry, effects)
    else {
        return BuildingExit::Failed;
    };
    // 0x0044441A..0x00444439: with a rally (the producer's ArchiveTarget,
    // `+0x218`) the arm calls the class setter Assign_Destination(rally, 1)
    // (vt+0x480), then Queue_Mission(Move, 0) (vt+0x1E8).
    let rally = sim
        .substrate
        .entities
        .get(producer_id)
        .and_then(|producer| producer.rally_cell());
    if let Some((tx, ty)) = rally {
        if let Err(cause) = sim.assign_destination_represented(
            spawned,
            Some(crate::sim::components::NavTargetRef::cell(tx, ty)),
            rules,
            overlay_registry,
            effects,
        ) {
            log::warn!("Rally destination {spawned}: {cause}");
        }
        let _ = sim.mission_queue_exact(
            spawned,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
            0,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
    }
    // 44443F..44447A: the explicit outer marks repeat Cell list/raw
    // occupation effects, even though Unlimbo already placed this ship.
    sim.foot_mark_remove(spawned, rules, overlay_registry, effects);
    let coord = crate::sim::movement::target_cell_coord(
        cell.0,
        cell.1,
        sim.resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    );
    sim.foot_set_location_marked(spawned, coord, rules, overlay_registry, effects);
    sim.foot_mark_put(spawned, rules, overlay_registry, effects);
    BuildingExit::Placed
}

/// ExitObject's land arm (`0x00444492..0x004445F6`): Unlimbo at the
/// producer's ExitCoord, then HELLO and TETHER from the producer and its
/// Unload. Unit PerCell takes the object on when it clears the producer
/// (`movement::per_cell`).
fn exit_land(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    stable_id: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> crate::sim::ai_base_building::BuildingExit {
    use crate::sim::ai_base_building::BuildingExit;
    let Some(producer) = sim.substrate.entities.get(producer_id) else {
        return BuildingExit::Failed;
    };
    let producer_type = producer.type_ref();
    let producer_owner = producer.owner();
    let archive = producer.archive_target();
    //444492/70C610 publishes Techno+218 before the busy test and Unlimbo.
    //A refused alternate also publishes its own archive through this same
    //entry. Unit PerCell consumes the retained value after clearance.
    sim.substrate
        .entities
        .get_mut(stable_id)
        .expect("held product remains live")
        .set_archive_target(archive);
    let busy = sim
        .substrate
        .entities
        .get(producer_id)
        .expect("the producer remains live")
        .mission
        .effective()
        .known()
        == Some(crate::sim::mission::MissionType::Unload);
    if busy {
        //4444B3..44451F walks the selected producer's House+68 in order:
        //identical BuildingType, another receiver, effective Guard5 and
        //Building+524 NULL. It calls the first eligible receiver once;
        //its return does not trigger a search for another receiver.
        let Some(house) = sim.houses.get(&producer_owner) else {
            return BuildingExit::Failed;
        };
        let Some(alternate_id) = house
            .base_projection()
            .buildings()
            .iter()
            .copied()
            .find(|&id| {
                sim.substrate.entities.get(id).is_some_and(|candidate| {
                    candidate.type_ref() == producer_type
                        && id != producer_id
                        && candidate.mission.effective().known()
                            == Some(crate::sim::mission::MissionType::Guard)
                        && sim.production.factories().building_factory(id).is_none()
                })
            })
        else {
            return BuildingExit::TryLater;
        };
        //44451F..444552 lends and restores the existing +524 attachment.
        //A player's held object lives in the House slot, so its producer's
        //+524 can be NULL. Do not move that House factory to a building.
        let attached = sim
            .production
            .factories()
            .building_factory(producer_id)
            .is_some();
        if attached
            && !sim
                .production
                .factories_mut()
                .transfer_building_factory_attachment(producer_id, alternate_id)
        {
            return BuildingExit::Failed;
        }
        let result = exit_produced_object(
            sim,
            rules,
            alternate_id,
            stable_id,
            //444535 offers the native empty Cell to the alternate.
            (0, 0),
            overlay_registry,
            effects,
        );
        if attached {
            assert!(
                sim.production
                    .factories_mut()
                    .transfer_building_factory_attachment(alternate_id, producer_id),
                "ExitObject must restore its lent factory attachment"
            );
        }
        return result;
    }
    let producer = sim
        .substrate
        .entities
        .get(producer_id)
        .expect("the producer remains live");
    let coord = crate::sim::movement::building_exit_coordinate(
        crate::sim::movement::ground_pose::position_world_coord(&producer.position),
        sim.interner
            .object_type(producer.type_ref(), rules)
            .expect("the producer has a type"),
        || {
            crate::sim::movement::ground_pose::object_get_coords(
                producer,
                sim.resolved_terrain.as_ref(),
            )
        },
    );
    let placed = sim.with_object_placement_scope(|sim| {
        let spawned = sim.reveal_constructed_object_at_coord_with_overlay_context(
            stable_id,
            coord,
            64,
            crate::sim::world::PlacementEvidence::EvaluateMark,
            rules,
            overlay_registry,
            effects,
        )?;
        // The native successful suffix44459F..4445C9 re-marks around the
        // second coordinate write. Use the shared Mark/SetLocation owners.
        let context =
            crate::sim::world::UninitContext::new(rules, overlay_registry).with_effects(effects);
        sim.unmark_entity_remove(spawned, context);
        crate::sim::movement::ground_pose::foot_set_location(
            &mut sim.substrate.entities,
            spawned,
            coord,
            rules,
            &sim.interner,
        );
        sim.mark_entity_put(spawned, context);
        mark_war_factory_spawn_contact(sim, rules, producer_id, spawned, effects);
        //4445F0 queues producer Unload16. Its existing Building mission
        //owner opens the shared Door and forces the native exit track.
        let _ = sim.mission_queue_exact(
            producer_id,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Unload),
            0,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        Some(spawned)
    });
    if placed.is_some() {
        BuildingExit::Placed
    } else {
        BuildingExit::Failed
    }
}

/// ExitObject's Infantry (`0x0044498E..0x00444DDF`) and generic
/// (`0x00444638..0x00444984`) arms: Unlimbo at the producer's [`dock_exit`],
/// then each arm's tail. ExitObject's placement scope (A8E7AC) stays
/// incremented through the tail, until the common return (`0x00444971`,
/// also `0x00444EDE` on refusal).
fn exit_at_dock(
    sim: &mut Simulation,
    rules: &RuleSet,
    arm: ExitArm,
    producer_id: u64,
    stable_id: u64,
    exit_cell: (i16, i16),
    product_type: &crate::rules::object_type::ObjectType,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> crate::sim::ai_base_building::BuildingExit {
    use crate::sim::ai_base_building::BuildingExit;
    if arm == ExitArm::Infantry {
        // 44498E..999 precedes GetDock, including its refusal path.
        let archive = sim
            .substrate
            .entities
            .get(producer_id)
            .and_then(|producer| producer.archive_target());
        if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
            entity.set_archive_target(archive);
        }
    }
    let offered = if arm == ExitArm::Generic {
        exit_cell
    } else {
        (0, 0)
    };
    let Some(exit) = dock_exit(
        sim,
        rules,
        producer_id,
        stable_id,
        offered,
        overlay_registry,
    ) else {
        return BuildingExit::Failed;
    };
    if arm == ExitArm::Infantry {
        // The coordinate and facing `dock_exit` derived read only the
        // producer and the dock Cell (`0x00444A53..0x00444C7C`), which the
        // copies' exits leave unchanged.
        exit_cloning_vat_copies(
            sim,
            rules,
            producer_id,
            product_type.id.as_str(),
            overlay_registry,
            effects,
        );
    }
    let placed = sim.with_object_placement_scope(|sim| {
        let spawned = sim.reveal_constructed_object_at_coord_with_overlay_context(
            stable_id,
            exit.coordinate,
            exit.facing,
            crate::sim::world::PlacementEvidence::EvaluateMark,
            rules,
            overlay_registry,
            effects,
        )?;
        if arm == ExitArm::Infantry {
            infantry_exit_tail(
                sim,
                rules,
                producer_id,
                spawned,
                exit.cell,
                product_type,
                overlay_registry,
                effects,
            );
        } else {
            generic_exit_tail(
                sim,
                rules,
                producer_id,
                spawned,
                exit.cell,
                overlay_registry,
                effects,
            );
        }
        Some(spawned)
    });
    if placed.is_some() {
        BuildingExit::Placed
    } else {
        BuildingExit::Failed
    }
}

/// The Infantry arm's Cloning Vat loop (`0x004449D9..0x00444A53`), between
/// its GetDock (`0x004449AF`) and its Unlimbo (`0x00444C95`). A
/// `Factory=InfantryType` producer without `Cloning=` (`+0xEB8`, `+0x16AC`)
/// has the product's type make one object (Create_Object, vt+0x8C) for each
/// Cloning Vat of the producer's House, owned by the vat's House, and hands it
/// to that vat's ExitObject (vt+0x100) with the empty Cell. The return is
/// ignored, so a copy the vat refuses stays in limbo.
///
/// The vats are House+0xFC (count +0x108). A vat joins it at Unlimbo
/// (`0x004413F2..0x00441443`) and ChangeOwner (`0x0044903A`) and leaves it
/// through `0x004FB9B0` (`0x004FBB6D`) and ChangeOwner (`0x00448939`), the
/// same events that move a building through House+68, so House+68 filtered by
/// `Cloning=` holds the same vats in the same order.
fn exit_cloning_vat_copies(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    product_type_name: &str,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(producer) = sim.substrate.entities.get(producer_id) else {
        return;
    };
    let owner = producer.owner();
    let clones = sim
        .object_type(producer.type_ref(), rules)
        .is_some_and(|producer_type| {
            producer_type.factory_type()
                == Some(crate::rules::object_type::FactoryType::InfantryType)
                && !producer_type.cloning
        });
    if !clones {
        return;
    }
    let vat_ids = sim
        .houses
        .get(&owner)
        .map(|house| house.base_projection().buildings().to_vec())
        .unwrap_or_default();
    for vat_id in vat_ids {
        let Some((vat_owner, is_cloning_vat)) =
            sim.substrate.entities.get(vat_id).and_then(|vat| {
                let object = sim.object_type(vat.type_ref(), rules)?;
                Some((vat.owner(), object.cloning))
            })
        else {
            continue;
        };
        if !is_cloning_vat {
            continue;
        }
        let vat_owner_name = sim.interner.resolve(vat_owner).to_owned();
        // Create_Object takes only the vat's House (`0x00444A24..0x00444A2B`);
        // the new object waits in limbo for the vat's exit to place it.
        let Some(clone_id) = sim.construct_object_limbo_at_height(
            product_type_name,
            &vat_owner_name,
            0,
            0,
            0,
            0,
            rules,
            effects,
        ) else {
            continue;
        };
        let _ = exit_produced_object(
            sim,
            rules,
            vat_id,
            clone_id,
            (0, 0),
            overlay_registry,
            effects,
        );
    }
}

/// The Infantry arm's tail (`0x00444CA3..0x00444DDF`). An object with a
/// NavCom whose type is neither `JumpJet=` (`+0xD94`) nor `Teleporter=`
/// (`+0xCD4`) archives that NavCom, queues Move and takes the dock Cell as
/// its destination (`0x00444CDF..0x00444D0B`). Then a producer whose House
/// is not controlled by a human (`0x0050B730` on the producer's `+0x21C`),
/// or any producer with `Hospital=` (`+0x16C1`), posts it ([`exit_post`],
/// `0x00444D11..0x00444DB7`), and the post joins its navigation list
/// ([`Simulation::queue_navigation_list`], `0x00444DAC`). The producer then
/// sends it HELLO, and on ROGER message 9 (`0x00444DBC..0x00444DD9`).
fn infantry_exit_tail(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    spawned: u64,
    dock: (u16, u16),
    product_type: &crate::rules::object_type::ObjectType,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let nav = sim
        .substrate
        .entities
        .get(spawned)
        .and_then(|entity| entity.navigation.nav_com());
    // Unlimbo's idle mode sets the NavCom of an object leaving with a
    // rally; one leaving without it has none and skips this whole step.
    if !product_type.jumpjet
        && !product_type.teleporter
        && let Some(nav) = nav
    {
        if let Some(entity) = sim.substrate.entities.get_mut(spawned) {
            entity.set_archive_target(Some(nav.into()));
        }
        let _ = sim.mission_queue_exact(
            spawned,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
            0,
            sim.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        if let Err(cause) = sim.assign_destination_represented(
            spawned,
            Some(crate::sim::components::NavTargetRef::cell(dock.0, dock.1)),
            rules,
            overlay_registry,
            effects,
        ) {
            log::debug!("{spawned} factory exit destination: {cause}");
        }
    }
    infantry_exit_post(sim, rules, producer_id, spawned, overlay_registry, effects);
    // Shared radio owner supplies HELLO bookkeeping and the
    // literal9 receiver; neither is an invented contact write.
    if crate::sim::radio::transmit(
        sim,
        producer_id,
        spawned,
        crate::sim::radio::RadioMessage::Hello,
        crate::sim::radio::RadioPayload::default(),
        rules,
        effects,
    ) == crate::sim::radio::RadioResponse::Roger
    {
        crate::sim::radio::transmit(
            sim,
            producer_id,
            spawned,
            crate::sim::radio::RadioMessage::TetherBack,
            crate::sim::radio::RadioPayload::default(),
            rules,
            effects,
        );
    }
}

/// The Infantry tail's post (`0x00444D11..0x00444DB7`); see
/// [`infantry_exit_tail`].
fn infantry_exit_post(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    spawned: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(producer) = ExitProducer::of(sim, rules, producer_id) else {
        return;
    };
    if !producer.computer && !producer.hospital {
        return;
    }
    if let Some(post) = exit_post(sim, rules, &producer, spawned) {
        sim.queue_navigation_list(spawned, post, rules, overlay_registry, effects);
    }
}

/// What ExitObject's posts read from the producer.
struct ExitProducer {
    /// `+0x21C`, the House House500200 runs on.
    house: crate::sim::intern::InternedId,
    /// That House is not controlled by a human (`0x0050B730`); a missing
    /// House counts as human.
    computer: bool,
    /// BuildingType `Hospital=` (`+0x16C1`).
    hospital: bool,
    /// BuildingType `Factory=` (`+0xEB8`) is set.
    factory: bool,
}

impl ExitProducer {
    fn of(sim: &Simulation, rules: &RuleSet, producer_id: u64) -> Option<Self> {
        let producer = sim.substrate.entities.get(producer_id)?;
        let house = producer.owner();
        let object = sim.object_type(producer.type_ref(), rules);
        Some(Self {
            house,
            computer: sim.houses.get(&house).is_some_and(|house| {
                !house.is_controlled_by_human(sim.session.game_mode_nonzero())
            }),
            hospital: object.is_some_and(|object| object.hospital),
            factory: object.is_some_and(|object| object.has_factory()),
        })
    }
}

/// The post both ExitObject arms inline (generic `0x00444907..0x0044496C`,
/// Infantry `0x00444D34..0x00444DB7`): Queue_Mission(Area_Guard, 0), then
/// House500200 on the producer's House for the object
/// ([`Simulation::house_return_cell`]). A post that is not the zero Cell
/// (`0x0089C818`, written only by its static initializer `0x0043B0E0`) from
/// a producer with `Factory=` becomes the object's ArchiveTarget and is
/// returned; otherwise the ArchiveTarget is cleared.
fn exit_post(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer: &ExitProducer,
    spawned: u64,
) -> Option<crate::sim::components::NavTargetRef> {
    use crate::sim::mission::{MissionId, MissionType, authority::LiveReadyInputProvider};
    let _ = sim.mission_queue_exact(
        spawned,
        MissionId::from_known(MissionType::AreaGuard),
        0,
        sim.session.binary_frame(),
        &LiveReadyInputProvider { rules },
    );
    let post = sim
        .house_return_cell(producer.house, spawned)
        .filter(|_| producer.factory)
        .map(|(x, y)| crate::sim::components::NavTargetRef::cell(x, y));
    if let Some(entity) = sim.substrate.entities.get_mut(spawned) {
        entity.set_archive_target(post.map(Into::into));
    }
    post
}

/// The generic arm's tail (`0x004448CE..0x0044496C`): Queue_Mission(Move)
/// (`0x004448D6`) and the dock Cell as the object's destination (vt+0x480,
/// `0x004448F2`). A producer whose House is not controlled by a human
/// (`0x0050B730`) then posts the object ([`exit_post`]); no VERA caller
/// passes a producer without `Factory=`.
fn generic_exit_tail(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    spawned: u64,
    dock: (u16, u16),
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    use crate::sim::mission::{MissionId, MissionType, authority::LiveReadyInputProvider};
    let _ = sim.mission_queue_exact(
        spawned,
        MissionId::from_known(MissionType::Move),
        0,
        sim.session.binary_frame(),
        &LiveReadyInputProvider { rules },
    );
    if let Err(cause) = sim.assign_destination_represented(
        spawned,
        Some(crate::sim::components::NavTargetRef::cell(dock.0, dock.1)),
        rules,
        overlay_registry,
        effects,
    ) {
        log::debug!("{spawned} factory exit destination: {cause}");
    }
    if let Some(producer) = ExitProducer::of(sim, rules, producer_id)
        && producer.computer
    {
        exit_post(sim, rules, &producer, spawned);
    }
}

/// The aircraft arm of `BuildingClass::ExitObject_Main @ 0x00443C60`
/// (`0x00443CB4..`), after the production-mode step and the unit-choice
/// clear: with a contact slot free or already its own (`0x0065ADF0`,
/// `0x00443CD4`) the aircraft is unlimboed at the factory's dock coordinate
/// for it (vt+0xA8, `0x00447B20`) facing Rules PoseDir (`0x00417FD0`), its
/// Z first zeroed (`ObjectClass::SetZ @ 0x005F6060`). The factory then sends
/// it HELLO and TETHER (vt+0x278, answers ignored): HELLO links the slot,
/// TETHER tethers only an aircraft that is not `AirportBound=`
/// (`0x006F4B4B`). The aircraft moves to the dock coordinate of that slot
/// (vt+0x1B4) and takes the factory as its dock (`+0x6CC`). One that is not
/// `AirportBound=` also takes the factory's rally point (`+0x218`) as its
/// destination and queues Move. Unlimbo's own idle mode
/// (`TechnoClass::Unlimbo @ 0x006F6E2A`) picks its mission before the link.
///
/// Without a slot an `AirportBound=` aircraft fails (`0x00443D12`).
/// RESIDUAL: any other flies in from the map edge (`0x00443D18..`: a cell
/// off the house's edge with one Scenario `RandomRanged(0, height)` draw,
/// Unlimbo there, the rally point or the cell near the factory as its
/// destination, `Assign_Mission(Move)`); VERA fails it too. Dormant: every
/// buildable retail aircraft is `AirportBound=`.
fn exit_aircraft(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    stable_id: u64,
    airport_bound: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> crate::sim::ai_base_building::BuildingExit {
    use crate::sim::ai_base_building::BuildingExit;
    use crate::sim::radio::{self, RadioMessage, RadioPayload};
    let linkable = sim
        .substrate
        .entities
        .get(producer_id)
        .is_some_and(|producer| producer.radio_contacts.has_free_or(stable_id));
    if !linkable {
        return BuildingExit::Failed;
    }
    sim.set_object_z(stable_id, 0, rules, overlay_registry, effects);
    let dock_coordinate = |sim: &Simulation| {
        crate::sim::movement::building_dock_coordinate(
            &sim.substrate.entities,
            producer_id,
            Some(stable_id),
            sim.resolved_terrain.as_ref(),
            rules,
            &sim.interner,
        )
        .expect("an aircraft factory has a dock coordinate")
    };
    let placed = sim.with_object_placement_scope(|sim| {
        let spawned = sim.reveal_constructed_object_at_coord_with_overlay_context(
            stable_id,
            dock_coordinate(sim),
            rules.general.pose_dir as u8,
            crate::sim::world::PlacementEvidence::EvaluateMark,
            rules,
            overlay_registry,
            effects,
        )?;
        for message in [RadioMessage::Hello, RadioMessage::Tether] {
            radio::transmit(
                sim,
                producer_id,
                spawned,
                message,
                RadioPayload::default(),
                rules,
                effects,
            );
        }
        let pad = dock_coordinate(sim);
        sim.foot_set_location_marked(spawned, pad, rules, overlay_registry, effects);
        sim.substrate.set_aircraft_dock(spawned, Some(producer_id));
        let rally = sim
            .substrate
            .entities
            .get(producer_id)
            .and_then(|producer| producer.archive_target());
        if let Some(rally) = rally
            && !airport_bound
        {
            sim.assign_aircraft_destination(spawned, Some(rally.into()), rules, effects);
            let _ = sim.mission_queue_exact(
                spawned,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Move),
                0,
                sim.session.binary_frame(),
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
        }
        Some(spawned)
    });
    if placed.is_some() {
        BuildingExit::Placed
    } else {
        BuildingExit::Failed
    }
}

/// Build a queue snapshot for one owner, including progress metadata for UI.
///
/// Projects the player-visible build queue from the registry (the queue-of-record).
/// Per factory: the active build (head) then its FIFO tail, sorted by `(category,
/// stamp)` where stamp is the factory's construction stamp for the head (older than
/// every entry queued behind it) or a tail entry's `enqueue_order`.
///
/// `state` shares the linked FactoryView's native rate/+70/IsComplete decision
/// with sidebar drawing and input; a cash stall still has a running rate.
/// A tail is Queued and has not started; head progress is the factory's stage.
pub fn queue_view_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> Vec<QueueItemView> {
    let Some(owner_id) = sim.interner.get(owner) else {
        return Vec::new();
    };
    // (category, stamp, type_id, state, progress); progress feeds only the
    // test-support view field.
    let mut items: Vec<(ProductionCategory, u64, InternedId, BuildQueueState, u16)> = Vec::new();
    for f in sim.production.factories().iter_insertion_ordered() {
        if f.owner() != owner_id {
            continue;
        }
        if let Some(obj) = f.object() {
            let state = f.view().cameo_state();
            items.push((
                f.category(),
                f.insertion_seq(),
                obj.type_id,
                state,
                f.progress().min(PRODUCTION_STEPS),
            ));
        }
        for e in f.queue() {
            items.push((
                f.category(),
                e.enqueue_order,
                e.type_id,
                BuildQueueState::Queued,
                0,
            ));
        }
    }
    items.sort_by_key(|&(category, stamp, ..)| (category, stamp));
    items
        .into_iter()
        .map(|(queue_category, _stamp, type_id, state, _progress)| {
            let type_str = sim.interner.resolve(type_id);
            let display_name = rules
                .object(type_str)
                .and_then(|obj| obj.name.clone())
                .unwrap_or_else(|| type_str.to_string());
            QueueItemView {
                type_id,
                display_name,
                queue_category,
                state,
                #[cfg(any(test, feature = "test-support"))]
                progress: _progress,
            }
        })
        .collect()
}

/// The owner's completed buildings waiting for placement: the object of its
/// Buildings and Defenses factories once `FactoryClass::IsComplete @
/// 0x004CA130` answers, which the sidebar strip also reads to draw "Ready".
pub fn ready_buildings_for_owner(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
) -> Vec<ReadyBuildingView> {
    let Some(owner_id) = sim.interner.get(owner) else {
        return Vec::new();
    };
    [ProductionCategory::Building, ProductionCategory::Defense]
        .into_iter()
        .filter_map(|category| {
            let factory = sim.production.factories().view(owner_id, category)?;
            let type_id = factory.complete_object()?.type_id;
            let type_str = sim.interner.resolve(type_id);
            let obj = rules.object(type_str)?;
            Some(ReadyBuildingView {
                type_id,
                display_name: obj.name.clone().unwrap_or_else(|| type_str.to_string()),
                queue_category: category,
            })
        })
        .collect()
}
