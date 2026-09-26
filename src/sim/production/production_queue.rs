//! Production views, economy queries and completed mobile delivery.
//!
//! `tick_production()` dispatches completed factory output after the frame's
//! charge sweep. Factory-held identity and accounting settle in factory_lifecycle.

use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

use super::PRODUCTION_STEPS;
use super::factory_lifecycle::{self, enqueue_by_type};
use super::production_spawn::{
    ProductionDeliveryKind, find_helipad_for_aircraft, find_spawn_selection_for_owner_with_type,
    mark_war_factory_spawn_contact, unlimbo_held_naval_unit,
};
use super::production_tech::{
    effective_time_to_build_frames_for_type, estimated_real_time_ms, owner_matches_build_identity,
    production_category_for_object, should_use_relaxed_build_mode,
};
use super::production_types::*;

/// Set rally point for an owner's production output.
pub fn set_rally_point_for_owner(sim: &mut Simulation, owner: &InternedId, rx: u16, ry: u16) {
    if let Some(house) = sim.houses.get_mut(owner) {
        house.rally_point = Some((rx, ry));
    }
}

/// Return current rally point for owner, if one has been set.
pub fn rally_point_for_owner(sim: &Simulation, owner: &str) -> Option<(u16, u16)> {
    sim.interner
        .get(owner)
        .and_then(|id| sim.houses.get(&id))
        .and_then(|h| h.rally_point)
}

pub fn credits_for_owner(sim: &Simulation, owner: &str) -> i32 {
    sim.interner
        .get(owner)
        .and_then(|id| sim.houses.get(&id))
        .map(|h| h.economy.credits)
        .unwrap_or(STARTING_CREDITS)
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

pub(in crate::sim) fn credits_entry_for_owner<'a>(
    sim: &'a mut Simulation,
    owner: &str,
) -> &'a mut i32 {
    let key = sim.interner.intern(owner);
    // Ensure house entry exists (auto-create with defaults if missing).
    // is_human defaults to true: in real games the app loading path seeds every house
    // with its actual flag, so the only callers that hit this fallback are
    // tests / edge cases that never declared a player. Defaulting to human
    // keeps those paths from accidentally activating AI-only behavior
    // (e.g., AIVirtualPurifiers credit bonus in the deposit path).
    if !sim.houses.contains_key(&key) {
        sim.houses.insert(
            key,
            crate::sim::house_state::HouseState::new(key, 0, None, true, STARTING_CREDITS, 10),
        );
    }
    &mut sim.houses.get_mut(&key).unwrap().economy.credits
}

/// Try to enqueue a default buildable unit for `owner`.
///
/// Returns the enqueued type ID on success.
pub fn enqueue_default_unit_for_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
) -> Option<InternedId> {
    let type_id: InternedId = pick_default_buildable_unit(sim, rules, owner)?;
    let type_str = sim.interner.resolve(type_id).to_string();
    enqueue_by_type(sim, rules, owner, &type_str).then_some(type_id)
}

/// Build a production list across supported sidebar categories for an owner.
///
/// In RA2, only items the player has unlocked via the tech tree are shown.
/// Items with missing prerequisites, wrong faction, or no factory are hidden
/// entirely — only items with insufficient credits are shown greyed out.
pub fn build_options_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> Vec<BuildOption> {
    let strict: Vec<BuildOption> =
        super::production_tech::build_options_for_owner_mode(sim, rules, owner, BuildMode::Strict);

    // Diagnostic: log reason breakdown when nothing is buildable.
    let enabled_count = strict.iter().filter(|o| o.enabled).count();
    if enabled_count == 0 && sim.session.tick % 90 == 0 {
        let mut reason_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for opt in &strict {
            let key = match &opt.reason {
                Some(BuildDisabledReason::UnbuildableTechLevel) => "UnbuildableTechLevel",
                Some(BuildDisabledReason::WrongOwner) => "WrongOwner",
                Some(BuildDisabledReason::WrongHouse) => "WrongHouse",
                Some(BuildDisabledReason::ForbiddenHouse) => "ForbiddenHouse",
                Some(BuildDisabledReason::RequiresStolenTech) => "RequiresStolenTech",
                Some(BuildDisabledReason::MissingPrerequisite(_)) => "MissingPrerequisite",
                Some(BuildDisabledReason::NoFactory) => "NoFactory",
                Some(BuildDisabledReason::AtBuildLimit) => "AtBuildLimit",
                Some(BuildDisabledReason::InsufficientCredits) => "InsufficientCredits",
                Some(BuildDisabledReason::PlacementModeUnavailable) => "PlacementModeUnavailable",
                None => "Enabled",
            };
            *reason_counts.entry(key).or_default() += 1;
        }
        log::warn!(
            "[BUILD-DIAG] owner='{}' tick={} total_items={} reasons={:?}",
            owner,
            sim.session.tick,
            strict.len(),
            reason_counts
        );
        // Log owned structures and their factory status.
        for e in sim.substrate.entities.values() {
            if sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
                && e.category == crate::map::entities::EntityCategory::Structure
            {
                let ts = sim.interner.resolve(e.type_ref());
                log::warn!(
                    "[BUILD-DIAG]   structure '{}' building_up={} factory_type={:?}",
                    ts,
                    e.building_up.is_some(),
                    rules.factory_type(ts)
                );
            }
        }
        // Log a few sample failures to show the exact reason per item.
        for opt in strict.iter().filter(|o| !o.enabled).take(5) {
            let type_str = sim.interner.resolve(opt.type_id);
            log::warn!(
                "[BUILD-DIAG]   sample: '{}' reason={:?}",
                type_str,
                opt.reason
            );
        }
    }

    let visible: Vec<BuildOption> = strict
        .into_iter()
        .filter(|opt| {
            opt.enabled
                || matches!(
                    opt.reason,
                    Some(BuildDisabledReason::InsufficientCredits)
                        | Some(BuildDisabledReason::AtBuildLimit)
                )
        })
        .collect();
    let visible = dedupe_visible_build_options(visible, sim, rules, owner, &sim.interner);
    if !visible.is_empty() || !super::production_tech::prototype_fallback_enabled() {
        return visible;
    }
    dedupe_visible_build_options(
        super::production_tech::build_options_for_owner_mode(
            sim,
            rules,
            owner,
            BuildMode::PrototypeRelaxed,
        ),
        sim,
        rules,
        owner,
        &sim.interner,
    )
}

fn dedupe_visible_build_options(
    options: Vec<BuildOption>,
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    interner: &crate::sim::intern::StringInterner,
) -> Vec<BuildOption> {
    let mut deduped: Vec<BuildOption> = Vec::new();
    let mut seen: BTreeMap<(ProductionCategory, String), usize> = BTreeMap::new();

    for option in options {
        let Some(key) = build_option_sidebar_key(rules, &option, interner) else {
            deduped.push(option);
            continue;
        };

        let seen_key = (option.queue_category, key);
        if let Some(existing_idx) = seen.get(&seen_key).copied() {
            let existing = &deduped[existing_idx];
            if prefers_sidebar_variant(sim, rules, owner, &option, existing, interner) {
                deduped[existing_idx] = option;
            }
            continue;
        }

        seen.insert(seen_key, deduped.len());
        deduped.push(option);
    }

    deduped
}

fn build_option_sidebar_key(
    rules: &RuleSet,
    option: &BuildOption,
    interner: &crate::sim::intern::StringInterner,
) -> Option<String> {
    let type_str = interner.resolve(option.type_id);
    let obj = rules.object(type_str)?;
    let image_key = if obj.image.trim().is_empty() {
        obj.id.to_ascii_uppercase()
    } else {
        obj.image.to_ascii_uppercase()
    };
    Some(format!("{}:{image_key}", option.object_category as u8))
}

fn prefers_sidebar_variant(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    candidate: &BuildOption,
    existing: &BuildOption,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    sidebar_variant_rank(sim, rules, owner, candidate, interner)
        > sidebar_variant_rank(sim, rules, owner, existing, interner)
}

fn sidebar_variant_rank(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    option: &BuildOption,
    interner: &crate::sim::intern::StringInterner,
) -> (u8, u16, u8) {
    let type_str = interner.resolve(option.type_id);
    let Some(obj) = rules.object(type_str) else {
        return (0, 0, 0);
    };

    let required_house_match = obj
        .required_houses
        .iter()
        .any(|house| owner_matches_build_identity(sim, owner, house));
    let owner_specificity = u16::MAX.saturating_sub(obj.owner.len() as u16);
    let enabled = option.enabled as u8;

    (required_house_match as u8, owner_specificity, enabled)
}

/// True if this owner has at least one strictly buildable production option.
///
/// This ignores prototype-relaxed fallback and is useful for picking a likely
/// local player house in UI code.
pub fn has_strict_build_option_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> bool {
    super::production_tech::build_options_for_owner_mode(sim, rules, owner, BuildMode::Strict)
        .iter()
        .any(|o| o.enabled)
}

/// Advance production timers and spawn completed items.
pub fn tick_production(
    sim: &mut Simulation,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
) -> bool {
    tick_production_with_overlay_registry(sim, rules, height_map, path_grid, None)
}

/// Advance production timers and spawn completed items with optional native
/// tiberium context for harvester-side reduction/reseed.
pub fn tick_production_with_overlay_registry(
    sim: &mut Simulation,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    tick_production_impl(sim, rules, height_map, path_grid, overlay_registry)
}

fn tick_production_impl(
    sim: &mut Simulation,
    rules: &RuleSet,
    height_map: &BTreeMap<(u16, u16), u8>,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    // P5d: the registry is the queue-of-record + completion authority. Collect the
    // (owner, category) keys whose active build has completed (progress == 54, object held,
    // not paused), in deterministic temporal (insertion_seq) order — the SAME order
    // step_all charged in and the hash folds. The registry advances (StartNextQueued) on a
    // successful delivery (C7), not on completion alone.
    let completed_keys = sim.production.factory_shadow.completed_keys();
    if completed_keys.is_empty() {
        return false;
    }

    let mut spawned_any = false;
    for (owner_id, queue_category) in completed_keys {
        let owner_str = sim.interner.resolve(owner_id).to_string();
        // The completed-held active object's type.
        let Some(done_type) = sim
            .production
            .factory_shadow
            .view(owner_id, queue_category)
            .and_then(|v| v.object.map(|o| o.type_id))
        else {
            continue;
        };
        let done_type_str = sim.interner.resolve(done_type).to_string();
        let produced_category = rules.object(&done_type_str).map(|o| o.category);
        factory_lifecycle::publish_completion(sim, rules, owner_id, queue_category);
        if produced_category == Some(crate::rules::object_type::ObjectCategory::Building) {
            continue;
        }
        let is_vehicle =
            produced_category == Some(crate::rules::object_type::ObjectCategory::Vehicle);
        // Aircraft use helipad spawn path; other units use exit cell path.
        let is_aircraft =
            produced_category == Some(crate::rules::object_type::ObjectCategory::Aircraft);
        let spawn_cell: Option<(u16, u16)>;
        let spawn_producer_id: Option<u64>;
        let spawn_delivery: ProductionDeliveryKind;
        let helipad_airfield: Option<u64>;

        if is_aircraft {
            if let Some((af_id, rx, ry)) = find_helipad_for_aircraft(sim, rules, &owner_str) {
                spawn_cell = Some((rx, ry));
                spawn_producer_id = Some(af_id);
                spawn_delivery = ProductionDeliveryKind::Standard;
                helipad_airfield = Some(af_id);
            } else {
                // No free helipad — refund.
                factory_lifecycle::refund_failed_delivery(sim, rules, owner_id, queue_category);
                continue;
            }
        } else {
            let is_naval: bool = rules.object(&done_type_str).map_or(false, |o| o.naval);
            let spawn_selection = produced_category.and_then(|cat| {
                find_spawn_selection_for_owner_with_type(
                    sim,
                    rules,
                    &owner_str,
                    Some(&done_type_str),
                    cat,
                    path_grid,
                    is_naval,
                )
            });
            spawn_cell = spawn_selection.map(|selection| selection.cell);
            spawn_producer_id = spawn_selection.map(|selection| selection.producer_id);
            spawn_delivery = spawn_selection
                .map(|selection| selection.delivery)
                .unwrap_or(ProductionDeliveryKind::Standard);
            helipad_airfield = None;
            if spawn_cell.is_none() {
                if is_vehicle {
                    continue;
                }
                factory_lifecycle::refund_failed_delivery(sim, rules, owner_id, queue_category);
                continue;
            }
        }
        let (rx, ry) = spawn_cell.unwrap();

        let Some(stable_id) = factory_lifecycle::active_entity_id(sim, owner_id, queue_category)
        else {
            debug_assert!(
                false,
                "active Factory object must own its StartProduction entity before delivery"
            );
            continue;
        };

        let spawned = match spawn_delivery {
            ProductionDeliveryKind::NavalUnit { .. } => unlimbo_held_naval_unit(
                sim,
                rules,
                &owner_str,
                &done_type_str,
                stable_id,
                spawn_producer_id.expect("naval delivery has one selected producer"),
                (rx, ry),
                overlay_registry,
                height_map,
            ),
            ProductionDeliveryKind::Standard => {
                let z = height_map.get(&(rx, ry)).copied().unwrap_or(0);
                sim.unlimbo_held_production_object_with_unit_context(
                    stable_id,
                    spawn_producer_id.unwrap_or(stable_id),
                    rx,
                    ry,
                    64,
                    z,
                    crate::sim::world::PlacementEvidence::EvaluateMark,
                    rules,
                    overlay_registry,
                )
            }
        };
        if let Some(stable_id) = spawned {
            if let Some(producer_id) = spawn_producer_id {
                mark_war_factory_spawn_contact(sim, rules, producer_id, stable_id);
            }
            // Aircraft spawned on helipad: reserve dock slot then set
            // DockedIdle carrying the assigned pad index.
            if let Some(af_id) = helipad_airfield {
                let max_slots = sim
                    .substrate
                    .entities
                    .get(af_id)
                    .and_then(|af| {
                        let af_type = sim.interner.resolve(af.type_ref());
                        let af_obj = rules.object(af_type)?;
                        Some(af_obj.dock_contact_capacity())
                    })
                    .unwrap_or(1);
                let assigned_pad = sim
                    .reserve_airfield_pad(af_id, stable_id, max_slots)
                    .unwrap_or(0); // Fresh spawn on a single-pad helipad always wins pad 0.
                if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
                    entity.aircraft_mission =
                        Some(crate::sim::aircraft::AircraftMission::DockedIdle {
                            airfield_id: af_id,
                            pad_index: assigned_pad,
                        });
                }
            }
            // `HouseClass::Place_Production 0x004FB5C6..0x004FB644`: for a
            // human-controlled house (`this == PlayerPtr` in MP, `+0x1EC ||
            // +0x1ED` in campaign) `CreateRadarEvent(6, object cell)` gates
            // `EVA_UnitReady` — type 6 dedupes within 2 cells, so two units
            // leaving one factory in quick succession give one line. The app
            // filters the owner to the local player and admits the event on
            // that client's radar.
            if sim
                .houses
                .get(&owner_id)
                .is_some_and(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero))
            {
                sim.sound_events
                    .push(crate::sim::world::SimSoundEvent::UnitComplete {
                        owner: owner_id,
                        radar: crate::sim::radar::RadarEventRequest::new(
                            crate::sim::radar::RadarEventType::UnitReady,
                            rx,
                            ry,
                        ),
                    });
            }
            // A Slave Miner leaving its war factory starts its hunt instead of
            // taking the rally point (`sim::slave_manager`).
            let hunting = matches!(spawn_delivery, ProductionDeliveryKind::Standard)
                && sim.slave_master_leaves_factory(stable_id, rules);
            // Auto-move newly produced unit to rally point (if set).
            // Skip for aircraft docked on helipad — they wait for orders.
            if helipad_airfield.is_none() && !hunting {
                let rally = match spawn_delivery {
                    ProductionDeliveryKind::NavalUnit { producer_rally, .. } => producer_rally,
                    ProductionDeliveryKind::Standard => rally_point_for_owner(sim, &owner_str),
                };
                let naval_rally =
                    matches!(spawn_delivery, ProductionDeliveryKind::NavalUnit { .. })
                        .then_some(rally)
                        .flatten();
                if let Some((tx, ty)) = naval_rally {
                    if let Some(entity) = sim.substrate.entities.get_mut(stable_id) {
                        // BuildingClass::ExitObject_Main @ 0x0044442B calls
                        // virtual Assign_Destination(target, 1) before its
                        // deferred Queue_Mission(Move, 0). NavCom is the owner
                        // destination; immediate A* is only a Rust executor.
                        crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                            entity,
                            Some(crate::sim::components::NavTargetRef::cell(tx, ty)),
                        );
                    }
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
                if let (Some(grid), Some((tx, ty))) = (path_grid, rally) {
                    let obj = rules.object(&done_type_str);
                    let loco_mult = sim
                        .substrate
                        .entities
                        .get(stable_id)
                        .and_then(|e| e.locomotor.as_ref())
                        .map(|l| l.speed_multiplier)
                        .unwrap_or(crate::util::fixed_math::SIM_ONE);
                    // `FootClass::GetCurrentSpeed @ 0x004DB1A0`: the rally move
                    // is an ordinary move order, so a unit that leaves the
                    // factory already promoted (InitialVeteran, cloning) drives
                    // to the rally point at its FASTER speed.
                    let speed = match sim.substrate.entities.get(stable_id) {
                        Some(e) => {
                            crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
                                e,
                                obj,
                                obj.map_or(4, |o| o.speed),
                                rules.general.veteran_speed,
                            )
                        }
                        None => crate::util::fixed_math::ra2_speed_to_leptons_per_second(
                            obj.map_or(4, |o| o.speed),
                        ),
                    };
                    let speed =
                        (speed * loco_mult).max(crate::util::fixed_math::SimFixed::lit("25"));
                    let speed_type = sim
                        .substrate
                        .entities
                        .get(stable_id)
                        .and_then(|e| e.locomotor.as_ref())
                        .map(|l| l.speed_type);
                    let _ = sim.issue_ground_move(
                        grid,
                        crate::sim::world::GroundMove {
                            entity_id: stable_id,
                            target: (tx, ty),
                            speed,
                            queue: false,
                            speed_type,
                            owner_blocks: false,
                            object_destination: None,
                        },
                        overlay_registry,
                        Some(rules),
                    );
                    if naval_rally.is_some()
                        && let Some(entity) = sim.substrate.entities.get_mut(stable_id)
                    {
                        // A Ship's setter publishes the rally unchanged; the
                        // command-time adapter of the remaining locomotors
                        // (Hover) may redirect its endpoint. Restore the
                        // producer rally so that A* never owns NavCom.
                        crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                            entity,
                            Some(crate::sim::components::NavTargetRef::cell(tx, ty)),
                        );
                    }
                }
                if matches!(spawn_delivery, ProductionDeliveryKind::NavalUnit { .. })
                    && let Some(entity) = sim.substrate.entities.get_mut(stable_id)
                {
                    // Native +0x124/+0x1B4/+0x124 success tail writes the
                    // selected CellClass centre after rally/mission assignment.
                    entity.position.sub_x = crate::util::lepton::CELL_CENTER_LEPTON;
                    entity.position.sub_y = crate::util::lepton::CELL_CENTER_LEPTON;
                }
            }
            spawned_any = true;
            factory_lifecycle::release_delivered_mobile(sim, rules, owner_id, queue_category);
        } else {
            if is_vehicle {
                continue;
            }
            factory_lifecycle::refund_failed_delivery(sim, rules, owner_id, queue_category);
        }
    }

    // P5d: drop any factory left idle (delivered + empty queue) — replaces the
    // `queues_by_owner.retain` prune.
    sim.production.factory_shadow.prune_all_idle();
    spawned_any
}

/// Build a queue snapshot for one owner, including progress metadata for UI.
///
/// P5d: projects the player-visible build queue from the registry (the queue-of-record),
/// byte-identically to the retired `queues_by_owner` view. Per factory: the active build
/// (head) then its FIFO tail. Sorted by `(category, stamp)` where stamp is the head's
/// `insertion_seq` or a tail entry's `enqueue_order` — algebraically the same order as the
/// retired `(queue_category, enqueue_order)` sort (D1: insertion_seq == active enqueue_order).
///
/// `state` is DERIVED (the `BuildQueueState` field is retired): head -> Paused if `manual`,
/// else Done if complete-held (`progress >= PRODUCTION_STEPS`, the blocked-exit case that
/// persists across ticks), else Building; tail -> Queued. `remaining` is DERIVED from
/// `progress` (the retired B2 mirror: `active_total_base_frames * (54 - progress) / 54`,
/// multiply-then-divide); a queued tail item has not started, so remaining == its total.
pub fn queue_view_for_owner(sim: &Simulation, rules: &RuleSet, owner: &str) -> Vec<QueueItemView> {
    let Some(owner_id) = sim.interner.get(owner) else {
        return Vec::new();
    };
    // (category, stamp, type_id, state, remaining_base_frames, total_base_frames)
    let mut items: Vec<(
        ProductionCategory,
        u64,
        InternedId,
        BuildQueueState,
        u32,
        u32,
    )> = Vec::new();
    for f in sim.production.factory_shadow.iter_insertion_ordered() {
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
            let steps_left = PRODUCTION_STEPS.saturating_sub(f.progress.min(PRODUCTION_STEPS));
            let remaining = ((u64::from(f.active_total_base_frames) * u64::from(steps_left))
                / u64::from(PRODUCTION_STEPS)) as u32;
            items.push((
                f.category,
                f.insertion_seq,
                obj.type_id,
                state,
                remaining,
                f.active_total_base_frames,
            ));
        }
        for e in &f.queue {
            items.push((
                f.category,
                e.enqueue_order,
                e.type_id,
                BuildQueueState::Queued,
                e.total_base_frames,
                e.total_base_frames,
            ));
        }
    }
    items.sort_by_key(|&(category, stamp, ..)| (category, stamp));
    items
        .into_iter()
        .map(
            |(queue_category, _stamp, type_id, state, remaining_base, total_base)| {
                let type_str = sim.interner.resolve(type_id);
                let (display_name, remaining_frames, total_frames) = rules
                    .object(type_str)
                    .map(|obj| {
                        (
                            obj.name.clone().unwrap_or_else(|| type_str.to_string()),
                            effective_time_to_build_frames_for_type(
                                sim,
                                rules,
                                owner,
                                type_str,
                                remaining_base,
                            ),
                            effective_time_to_build_frames_for_type(
                                sim,
                                rules,
                                owner,
                                type_str,
                                total_base.max(1),
                            ),
                        )
                    })
                    .unwrap_or_else(|| (type_str.to_string(), remaining_base, total_base.max(1)));
                QueueItemView {
                    type_id,
                    display_name,
                    queue_category,
                    state,
                    remaining_ms: estimated_real_time_ms(remaining_frames, PRODUCTION_RATE_SCALE),
                    total_ms: estimated_real_time_ms(total_frames, PRODUCTION_RATE_SCALE),
                }
            },
        )
        .collect()
}

pub fn ready_buildings_for_owner(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
) -> Vec<ReadyBuildingView> {
    let owner_id = sim.interner.get(owner);
    let ready = owner_id.and_then(|id| sim.production.ready_by_owner.get(&id));
    ready
        .map(|ready| {
            ready
                .iter()
                .filter_map(|&type_id| {
                    let type_str = sim.interner.resolve(type_id);
                    let obj = rules.object(type_str)?;
                    Some(ReadyBuildingView {
                        type_id,
                        display_name: obj.name.clone().unwrap_or_else(|| type_str.to_string()),
                        queue_category: production_category_for_object(obj),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pick_default_buildable_unit(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
) -> Option<InternedId> {
    let mode = if should_use_relaxed_build_mode(sim, rules, owner) {
        BuildMode::PrototypeRelaxed
    } else {
        BuildMode::Strict
    };
    super::production_tech::build_options_for_owner_mode(sim, rules, owner, mode)
        .into_iter()
        .find(|opt| {
            opt.enabled
                && matches!(
                    opt.queue_category,
                    ProductionCategory::Infantry
                        | ProductionCategory::Vehicle
                        | ProductionCategory::Ship
                        | ProductionCategory::Aircraft
                )
        })
        .map(|opt| opt.type_id)
}
