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
    ProductionDeliveryKind, mark_war_factory_spawn_contact, unlimbo_held_naval_unit,
};
use super::production_types::*;

/// `owner`'s Available_Money, by house name; 0 for a house that does not
/// exist.
pub fn credits_for_owner(sim: &Simulation, owner: &str) -> i32 {
    sim.interner
        .get(owner)
        .map_or(0, |id| crate::sim::credit_income::available_money(sim, id))
}

/// `owner`'s house, created as a human house holding [`STARTING_CREDITS`]
/// when the test never made one.
#[cfg(test)]
pub(in crate::sim) fn house_for_test<'a>(
    sim: &'a mut Simulation,
    owner: &str,
) -> &'a mut crate::sim::house_state::HouseState {
    let key = sim.interner.intern(owner);
    sim.houses.entry(key).or_insert_with(|| {
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
        .map(|state| (state.total_output, state.total_drain))
        .unwrap_or((0, 0))
}

/// Sum of |Power=| from TypeClass for ALL owned buildings (including under
/// construction). Used by the sidebar power bar fill curve.
pub fn theoretical_power_for_owner(sim: &Simulation, owner: &str) -> i32 {
    let Some(owner_id) = sim.interner.get(owner) else {
        return 0;
    };
    sim.power_states
        .get(&owner_id)
        .map(|state| state.theoretical_total_power)
        .unwrap_or(0)
}

/// The options the player's sidebar shows for `owner`, by category
/// ([`BuildOption::visible_in_sidebar`]).
pub fn build_options_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> Vec<BuildOption> {
    let options: Vec<BuildOption> =
        super::production_tech::all_build_options_for_owner(sim, rules, owner);

    // Diagnostic: log reason breakdown when nothing is buildable.
    let enabled_count = options.iter().filter(|o| o.enabled).count();
    if enabled_count == 0 && sim.session.tick.is_multiple_of(90) {
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
            sim.session.tick,
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
    let mut completed = sim.production.factories.take_changed_completed_keys();
    completed.sort_by_key(|(_, category)| match category {
        ProductionCategory::Building => 0,
        ProductionCategory::Defense => 1,
        ProductionCategory::Infantry => 2,
        ProductionCategory::Vehicle | ProductionCategory::Aircraft | ProductionCategory::Ship => 3,
    });
    for (owner, category) in completed {
        let Some(type_id) = sim
            .production
            .factories
            .view(owner, category)
            .and_then(|factory| factory.object.map(|object| object.type_id))
        else {
            continue;
        };
        factory_lifecycle::publish_completion(sim, rules, owner, category);
        if sim.object_type(type_id, rules).is_some_and(|object| {
            object.category != crate::rules::object_type::ObjectCategory::Building
        }) {
            sim.queue_command(crate::sim::command::CommandEnvelope::new(
                owner,
                sim.session.tick.saturating_add(1),
                crate::sim::command::Command::PlaceProducedMobile { category },
            ));
        }
    }
}

/// Exercise the next frame's prefix publication and its event tail in focused owner tests,
/// without running unrelated world AI. The ordinary production runtime uses
/// publish_production_changes plus the world's existing scheduled tail.
#[cfg(test)]
pub(crate) fn dispatch_production_changes_for_tests(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    sim.session.tick = sim.session.tick.saturating_add(1);
    sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
    publish_production_changes(sim, rules);
    let due = sim.take_due_commands();
    let (_, spawned) = sim.apply_due_commands(
        &due,
        Some(rules),
        sim.session.tick.saturating_add(1),
        overlay_registry,
        crate::sim::world::FrameEffects::default(),
    );
    spawned
}

/// BuildingClass::ExitObject_Main443C60 for one retained object.
/// Human House PLACE and AI Building::Factory_AI use this same exit owner;
/// their refund/retry/release policies remain with their respective callers.
/// GetDock/admission, Unlimbo and connected radio/mission effects run once.
pub(in crate::sim) fn exit_produced_object(
    sim: &mut Simulation,
    rules: &RuleSet,
    producer_id: u64,
    stable_id: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> crate::sim::ai_base_building::BuildingExit {
    use crate::sim::ai_base_building::{self, BuildingExit};
    use crate::sim::ai_unit_choice::UnitChoiceKind;
    let Some(entity) = sim.substrate.entities.get_mut(stable_id) else {
        return BuildingExit::Failed;
    };
    // Exit443C81 sets Techno+3D5 before RTTI, admission or Unlimbo.
    entity.in_playfield = true;
    let owner_id = entity.owner();
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
    let type_name = product_type.id.as_str();
    let Some(kind) = UnitChoiceKind::of(product_type.category) else {
        return BuildingExit::Failed;
    };
    let Some(producer_type) = sim
        .substrate
        .entities
        .get(producer_id)
        .and_then(|producer| sim.object_type(producer.type_ref(), rules))
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
    ai_base_building::economy_state_machine(sim, rules, owner_id, false);
    if let Some(house) = sim.houses.get_mut(&owner_id) {
        house.ai_unit_choices.clear(kind);
    }
    if product_type.category == crate::rules::object_type::ObjectCategory::Infantry {
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
    let Some(producer) = sim.substrate.entities.get(producer_id) else {
        return BuildingExit::Failed;
    };
    let Some(selection) = super::production_spawn::spawn_selection_at_producer(
        sim,
        rules,
        (
            producer_id,
            producer.position.rx,
            producer.position.ry,
            sim.interner.resolve(producer.type_ref()),
        ),
        Some(stable_id),
        Some(type_name),
        product_type.category,
        product_type.naval,
        overlay_registry,
    ) else {
        return BuildingExit::Failed;
    };
    let (rx, ry) = selection.cell;
    let is_unit = product_type.category == crate::rules::object_type::ObjectCategory::Vehicle;
    let land_factory = is_unit
        && sim
            .substrate
            .entities
            .get(selection.producer_id)
            .is_some_and(|producer| {
                super::production_spawn::exact_land_vehicle_exit_factory(
                    rules,
                    sim.interner.resolve(producer.type_ref()),
                )
            });
    if land_factory {
        let producer = sim
            .substrate
            .entities
            .get(selection.producer_id)
            .expect("selected producer remains live");
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
            .get(selection.producer_id)
            .expect("selected producer remains live")
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
            let Some(alternate_id) =
                house
                    .base_projection
                    .buildings()
                    .iter()
                    .copied()
                    .find(|&id| {
                        sim.substrate.entities.get(id).is_some_and(|candidate| {
                            candidate.type_ref() == producer_type
                                && id != selection.producer_id
                                && candidate.mission.effective().known()
                                    == Some(crate::sim::mission::MissionType::Guard)
                                && sim.production.factories.building_factory(id).is_none()
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
                .factories
                .building_factory(selection.producer_id)
                .is_some();
            if attached
                && !sim
                    .production
                    .factories
                    .transfer_building_factory_attachment(selection.producer_id, alternate_id)
            {
                return BuildingExit::Failed;
            }
            let result = exit_produced_object(
                sim,
                rules,
                alternate_id,
                stable_id,
                overlay_registry,
                effects,
            );
            if attached {
                assert!(
                    sim.production
                        .factories
                        .transfer_building_factory_attachment(alternate_id, selection.producer_id),
                    "ExitObject must restore its lent factory attachment"
                );
            }
            return result;
        }
    }
    let spawned = if land_factory {
        let producer = sim
            .substrate
            .entities
            .get(selection.producer_id)
            .expect("selected producer remains live");
        let coord = crate::sim::movement::building_exit_coordinate(
            crate::sim::movement::ground_pose::position_world_coord(&producer.position),
            rules
                .object(sim.interner.resolve(producer.type_ref()))
                .expect("selected producer has a type"),
            || {
                crate::sim::movement::ground_pose::object_get_coords(
                    producer,
                    sim.resolved_terrain.as_ref(),
                )
            },
        );
        sim.with_object_placement_scope(|sim| {
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
            let context = crate::sim::world::UninitContext::new(Some(rules), overlay_registry)
                .with_effects(effects);
            sim.unmark_entity_remove(spawned, context);
            crate::sim::movement::ground_pose::foot_set_location(
                &mut sim.substrate.entities,
                spawned,
                coord,
                Some(rules),
                &sim.interner,
            );
            sim.mark_entity_put(spawned, context);
            mark_war_factory_spawn_contact(sim, rules, selection.producer_id, spawned, effects);
            //4445F0 queues producer Unload16. Its existing Building mission
            //owner opens the shared Door and forces the native exit track.
            let _ = sim.mission_queue_exact(
                selection.producer_id,
                crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Unload,
                ),
                0,
                sim.session.binary_frame,
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
            Some(spawned)
        })
    } else {
        let spawned = match selection.delivery {
            ProductionDeliveryKind::NavalUnit => {
                unlimbo_held_naval_unit(sim, rules, stable_id, (rx, ry), overlay_registry, effects)
            }
            ProductionDeliveryKind::Infantry {
                coordinate,
                facing,
                exit_cell,
            } => {
                // A8E7AC stays incremented through the destination/radio tail,
                // until the common successful return444971 (also on refusal).
                sim.with_object_placement_scope(|sim| {
                    let spawned = sim.reveal_constructed_object_at_coord_with_overlay_context(
                        stable_id,
                        coordinate,
                        facing,
                        crate::sim::world::PlacementEvidence::EvaluateMark,
                        rules,
                        overlay_registry,
                        effects,
                    )?;
                    let nav = sim
                        .substrate
                        .entities
                        .get(spawned)
                        .and_then(|entity| entity.navigation.nav_com);
                    // Only a non-JumpJet/non-Teleporter with Unlimbo NavCom
                    // takes444CA3..D0B. No-rally NavNULL skips this whole arm.
                    if !product_type.jumpjet
                        && !product_type.teleporter
                        && let Some(nav) = nav
                    {
                        if let Some(entity) = sim.substrate.entities.get_mut(spawned) {
                            entity.set_archive_target(Some(nav.into()));
                        }
                        let _ = sim.mission_queue_exact(
                            spawned,
                            crate::sim::mission::MissionId::from_known(
                                crate::sim::mission::MissionType::Move,
                            ),
                            0,
                            sim.session.binary_frame,
                            &crate::sim::mission::authority::EntityReadyInputProvider,
                        );
                        if let Err(cause) = sim.assign_infantry_destination(
                            spawned,
                            crate::sim::components::NavTargetRef::cell(exit_cell.0, exit_cell.1),
                            rules,
                            overlay_registry,
                            effects,
                        ) {
                            log::debug!("Infantry {spawned} factory exit destination: {cause}");
                        }
                    }
                    // Shared radio owner supplies HELLO bookkeeping and the
                    // literal9 receiver; neither is an invented contact write.
                    if crate::sim::radio::transmit(
                        sim,
                        selection.producer_id,
                        spawned,
                        crate::sim::radio::RadioMessage::Hello,
                        crate::sim::radio::RadioPayload::default(),
                        Some(rules),
                        effects,
                    ) == crate::sim::radio::RadioResponse::Roger
                    {
                        crate::sim::radio::transmit(
                            sim,
                            selection.producer_id,
                            spawned,
                            crate::sim::radio::RadioMessage::TetherBack,
                            crate::sim::radio::RadioPayload::default(),
                            Some(rules),
                            effects,
                        );
                    }
                    Some(spawned)
                })
            }
            ProductionDeliveryKind::Standard => {
                let z = sim.terrain_cell_level(rx, ry).unwrap_or(0);
                sim.reveal_constructed_object_at_height_with_overlay_context(
                    stable_id,
                    rx,
                    ry,
                    64,
                    z,
                    crate::sim::world::PlacementEvidence::EvaluateMark,
                    rules,
                    overlay_registry,
                    effects,
                )
            }
        };
        if let Some(spawned) = spawned
            && !matches!(selection.delivery, ProductionDeliveryKind::Infantry { .. })
        {
            mark_war_factory_spawn_contact(sim, rules, selection.producer_id, spawned, effects);
        }
        spawned
    };
    let Some(spawned) = spawned else {
        return BuildingExit::Failed;
    };
    let stable_id = spawned;
    // A Slave Miner leaving its war factory starts its hunt instead of
    // taking the rally point (`sim::slave_manager`).
    let hunting = matches!(selection.delivery, ProductionDeliveryKind::Standard)
        && sim.slave_master_leaves_factory(stable_id, rules, effects);
    // Auto-move newly produced unit to rally point (if set).
    if !hunting
        && !land_factory
        && !matches!(selection.delivery, ProductionDeliveryKind::Infantry { .. })
    {
        // `ExitObject_Main @ 0x00443C60` reads the factory's own
        // ArchiveTarget (`+0x218`, the rally point) for the object
        // leaving it; the naval arm reads it after Unlimbo
        // (`0x0044441A`).
        //
        // The land WeaponsFactory's archive copy444492 and human/Harvest
        // continuation use the shared owners above and Unit PerCell. Residual
        // (instruction reading): a Unit from any other Factory=UnitType
        // building takes the generic arm444638, which never reads +0x218. It
        // queues Move (4448D6) and assigns its exit Cell (4448F2); a human
        // owner returns there (50B730), and a computer one queues Area_Guard
        // (44490F) and takes its House500200 post (70C610). This adapter sends
        // that object to the rally through the class setter instead. Trigger:
        // such a building with a rally; every retail rulesmd.ini
        // Factory=UnitType building sets WeaponsFactory, so only map or mod
        // data reaches it. Effect: the object drives to the rally rather than
        // holding its exit Cell or guarding its post.
        // The separate computer Unit House500200 post selection and its
        // Scenario RandomRanged(1,4) draw remain a Unit PerCell residual.
        let rally = sim
            .substrate
            .entities
            .get(selection.producer_id)
            .and_then(|producer| producer.rally_cell());
        if let Some((tx, ty)) = rally {
            // 0x0044441A..0x00444439: with a rally the naval arm calls the
            // class setter Assign_Destination(rally, 1) (vt+0x480), then
            // Queue_Mission(Move, 0) (vt+0x1E8).
            if let Err(cause) = sim.assign_destination_represented(
                stable_id,
                Some(crate::sim::components::NavTargetRef::cell(tx, ty)),
                Some(rules),
                overlay_registry,
                effects,
            ) {
                log::warn!("Rally destination {stable_id}: {cause}");
            }
            if matches!(selection.delivery, ProductionDeliveryKind::NavalUnit) {
                let _ = sim.mission_queue_exact(
                    stable_id,
                    crate::sim::mission::MissionId::from_known(
                        crate::sim::mission::MissionType::Move,
                    ),
                    0,
                    sim.session.binary_frame,
                    &crate::sim::mission::authority::EntityReadyInputProvider,
                );
            }
        }
        if matches!(selection.delivery, ProductionDeliveryKind::NavalUnit)
            && let Some(entity) = sim.substrate.entities.get_mut(stable_id)
        {
            // Native +0x124/+0x1B4/+0x124 success tail writes the
            // selected CellClass centre after rally/mission assignment.
            entity.position.sub_x = crate::util::lepton::CELL_CENTER_LEPTON;
            entity.position.sub_y = crate::util::lepton::CELL_CENTER_LEPTON;
        }
    }
    BuildingExit::Placed
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
    sim.set_object_z(stable_id, 0, Some(rules), overlay_registry, effects);
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
                Some(rules),
                effects,
            );
        }
        let pad = dock_coordinate(sim);
        sim.foot_set_location_marked(spawned, pad, Some(rules), overlay_registry, effects);
        sim.set_aircraft_dock(spawned, Some(producer_id));
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
                sim.session.binary_frame,
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
/// `state` is DERIVED: head -> Paused if held by the user (`manual`), else Done if
/// complete-held (`progress >= PRODUCTION_STEPS`, the blocked-exit case that persists
/// across ticks), else Building (a cash stall included); tail -> Queued. `progress` is
/// the factory's step count; a queued tail item has not started.
pub fn queue_view_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> Vec<QueueItemView> {
    let Some(owner_id) = sim.interner.get(owner) else {
        return Vec::new();
    };
    // (category, stamp, type_id, state, progress)
    let mut items: Vec<(ProductionCategory, u64, InternedId, BuildQueueState, u16)> = Vec::new();
    for f in sim.production.factories.iter_insertion_ordered() {
        if f.owner != owner_id {
            continue;
        }
        if let Some(obj) = f.object.as_ref() {
            let state = if f.manual {
                BuildQueueState::Paused
            } else if f.progress >= PRODUCTION_STEPS {
                BuildQueueState::Done
            } else {
                BuildQueueState::Building
            };
            items.push((
                f.category,
                f.insertion_seq,
                obj.type_id,
                state,
                f.progress.min(PRODUCTION_STEPS),
            ));
        }
        for e in &f.queue {
            items.push((
                f.category,
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
        .map(|(queue_category, _stamp, type_id, state, progress)| {
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
                progress,
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
            let factory = sim.production.factories.view(owner_id, category)?;
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
