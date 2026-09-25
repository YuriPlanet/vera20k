//! Minimal AI opponent — produces deterministic commands via the same Command API as players.
//!
//! The AI uses a simple priority-based decision loop each tick:
//! 1. Deploy MCV if one exists but no construction yard
//! 2. Build power when low or missing
//! 3. Build refinery for economy
//! 4. Build barracks and war factory for unit production
//! 5. Queue infantry and vehicles continuously
//! 6. Place ready buildings near existing base
//! 7. Send attack waves toward the nearest enemy base periodically
//!
//! All decisions are deterministic (uses SimRng). AI commands are injected
//! into the same command stream as player commands, so replays stay valid.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::object_type::{FactoryType, ObjectCategory};
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{CellRect, CellRectOccupancyContext, check_occupancy_rect};
use crate::sim::command::{Command, CommandEnvelope, QueueMode};
use crate::sim::intern::InternedId;
use crate::sim::pathfinding::PathGrid;
use crate::sim::production;
use crate::sim::world::Simulation;

/// How often (in native frames) the AI evaluates its build/production decisions.
const AI_THINK_INTERVAL_FRAMES: u32 = 8;

/// How often (in native frames) the AI sends an attack wave.
const AI_ATTACK_INTERVAL_FRAMES: u32 = 225;

/// Minimum native frame before the AI sends its first attack.
const AI_FIRST_ATTACK_FRAME: u32 = 150;

/// Maximum units to send per attack wave.
const AI_ATTACK_WAVE_SIZE: usize = 8;

/// Per-AI-owner persistent state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiPlayerState {
    /// House/owner name this AI controls.
    pub owner: InternedId,
    /// Native frame when the last attack wave was sent.
    pub last_attack_frame: u32,
    /// Whether MCV deploy has been attempted.
    pub mcv_deployed: bool,
}

impl AiPlayerState {
    pub fn new(owner: InternedId) -> Self {
        Self {
            owner,
            last_attack_frame: 0,
            mcv_deployed: false,
        }
    }
}

/// Run one AI decision cycle for all AI players. Returns commands to inject.
pub fn tick_ai(
    sim: &Simulation,
    ai_players: &mut [AiPlayerState],
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    height_map: &BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Vec<CommandEnvelope> {
    let mut commands: Vec<CommandEnvelope> = Vec::new();
    let execute_tick = sim.session.tick.saturating_add(1);
    let current_frame = sim.session.binary_frame;

    for ai in ai_players.iter_mut() {
        // A house defeated this tick issues no commands at all (not even ready-
        // building placement). gamemd evaluates each house's defeat before its AI
        // manage/produce step; Phase 8 runs check_defeat before this loop, so the
        // flag is current. Without this gate the defeat reorder would be a no-op.
        if crate::sim::house_state::house_state_for_owner_id(&sim.houses, ai.owner)
            .is_some_and(|h| h.is_defeated)
        {
            continue;
        }

        let owner_str = sim.interner.resolve(ai.owner);
        // Only think every N native frames to avoid spamming.
        if !current_frame.is_multiple_of(AI_THINK_INTERVAL_FRAMES) {
            // Still check for building placement every frame.
            place_ready_buildings(
                sim,
                ai,
                rules,
                path_grid,
                height_map,
                overlay_registry,
                execute_tick,
                &mut commands,
            );
            continue;
        }

        // 1. Deploy MCV if needed.
        if !ai.mcv_deployed {
            if let Some(cmd) = try_deploy_mcv(sim, owner_str, rules, execute_tick) {
                commands.push(cmd);
                ai.mcv_deployed = true;
                continue; // Wait for next think cycle.
            }
        }

        // 2. Build structures in priority order.
        // A ConYard is any structure with UndeploysInto= set (data-driven from rules.ini).
        let has_conyard = has_conyard_dynamic(sim, owner_str, rules);
        if !has_conyard {
            continue; // No conyard — can't do anything.
        }

        let building_queued = has_active_building_queue(sim, owner_str, rules);
        if !building_queued {
            if let Some(cmd) = decide_next_building(sim, owner_str, rules, execute_tick) {
                commands.push(cmd);
            }
        }

        // 3. Queue units from barracks and war factory.
        queue_units(sim, owner_str, rules, execute_tick, &mut commands);

        // 4. Place ready buildings.
        place_ready_buildings(
            sim,
            ai,
            rules,
            path_grid,
            height_map,
            overlay_registry,
            execute_tick,
            &mut commands,
        );

        // 5. Send attack waves.
        if current_frame >= AI_FIRST_ATTACK_FRAME
            && current_frame.wrapping_sub(ai.last_attack_frame) >= AI_ATTACK_INTERVAL_FRAMES
        {
            let attack_cmds = send_attack_wave(sim, owner_str, rules, execute_tick);
            if !attack_cmds.is_empty() {
                ai.last_attack_frame = current_frame;
                commands.extend(attack_cmds);
            }
        }
    }

    commands
}

/// Try to find an undeployed MCV and issue a deploy command.
///
/// An MCV is any unit with `DeploysInto=` set in rules.ini (data-driven).
fn try_deploy_mcv(
    sim: &Simulation,
    owner: &str,
    rules: &RuleSet,
    execute_tick: u64,
) -> Option<CommandEnvelope> {
    for entity in sim.substrate.entities.values() {
        if !sim
            .interner
            .resolve(entity.owner())
            .eq_ignore_ascii_case(owner)
        {
            continue;
        }
        if entity.dying || entity.lifecycle.in_limbo {
            continue;
        }
        let is_deployable: bool = sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|obj| obj.deploys_into.is_some());
        if is_deployable {
            let owner_id = sim.interner.get(owner)?;
            return Some(CommandEnvelope::new(
                owner_id,
                execute_tick,
                Command::DeployMcv {
                    entity_id: entity.stable_id(),
                },
            ));
        }
    }
    None
}

/// Check if the owner has any ConYard-class structure (one with UndeploysInto= set).
fn has_conyard_dynamic(sim: &Simulation, owner: &str, rules: &RuleSet) -> bool {
    has_owned_structure_matching(sim, owner, |type_id| {
        rules
            .object(type_id)
            .is_some_and(|object| object.undeploys_into.is_some())
    })
}

fn has_owned_structure_matching<F>(sim: &Simulation, owner: &str, mut matches: F) -> bool
where
    F: FnMut(&str) -> bool,
{
    sim.substrate.entities.values().any(|e| {
        !e.dying
            && !e.lifecycle.in_limbo
            && e.category == EntityCategory::Structure
            && sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
            && matches(sim.interner.resolve(e.type_ref()))
    })
}

fn has_refinery_structure(sim: &Simulation, owner: &str, rules: &RuleSet) -> bool {
    has_owned_structure_matching(sim, owner, |type_id| rules.is_refinery_type(type_id))
}

fn has_power_support_structure(sim: &Simulation, owner: &str, rules: &RuleSet) -> bool {
    has_owned_structure_matching(sim, owner, |type_id| {
        rules
            .object_case_insensitive(type_id)
            .is_some_and(|object| object.power > 0)
    })
}

fn has_factory_structure(
    sim: &Simulation,
    owner: &str,
    rules: &RuleSet,
    factory_type: FactoryType,
) -> bool {
    has_owned_structure_matching(sim, owner, |type_id| {
        rules.factory_type(type_id) == Some(factory_type)
    })
}

/// Check if the AI has an active (non-empty) building/defense production queue.
fn has_active_building_queue(sim: &Simulation, owner: &str, rules: &RuleSet) -> bool {
    let view = production::queue_view_for_owner(sim, rules, owner);
    view.iter().any(|item| {
        matches!(
            item.queue_category,
            production::ProductionCategory::Building | production::ProductionCategory::Defense
        )
    })
}

/// Decide which building to queue next, following a priority order.
fn decide_next_building(
    sim: &Simulation,
    owner: &str,
    rules: &RuleSet,
    execute_tick: u64,
) -> Option<CommandEnvelope> {
    let owner_id = sim.interner.get(owner)?;
    let options = production::build_options_for_owner(sim, rules, owner);

    // Priority 1: Power support if we have none or power balance is negative.
    let has_power = has_power_support_structure(sim, owner, rules);
    let (produced, drained) = production::power_balance_for_owner(sim, rules, owner);
    if !has_power || produced < drained {
        if let Some(type_id) = find_buildable_matching(&options, rules, &sim.interner, |object| {
            object.category == ObjectCategory::Building && object.power > 0
        }) {
            return Some(make_queue_cmd(owner_id, type_id, execute_tick));
        }
    }

    // Priority 2: Refinery if we have none.
    if !has_refinery_structure(sim, owner, rules) {
        if let Some(type_id) = find_buildable_refinery(&options, rules, &sim.interner) {
            return Some(make_queue_cmd(owner_id, type_id, execute_tick));
        }
    }

    // Priority 3: Infantry producer if we have none.
    if !has_factory_structure(sim, owner, rules, FactoryType::InfantryType) {
        if let Some(type_id) = find_buildable_matching(&options, rules, &sim.interner, |object| {
            object.category == ObjectCategory::Building
                && object.factory == Some(FactoryType::InfantryType)
        }) {
            return Some(make_queue_cmd(owner_id, type_id, execute_tick));
        }
    }

    // Priority 4: Vehicle producer if we have none.
    if !has_factory_structure(sim, owner, rules, FactoryType::UnitType) {
        if let Some(type_id) = find_buildable_matching(&options, rules, &sim.interner, |object| {
            object.category == ObjectCategory::Building
                && object.factory == Some(FactoryType::UnitType)
        }) {
            return Some(make_queue_cmd(owner_id, type_id, execute_tick));
        }
    }

    // Priority 5: Second refinery for faster economy.
    let refinery_count = count_refineries(sim, owner, rules);
    if refinery_count < 2 {
        if let Some(type_id) = find_buildable_refinery(&options, rules, &sim.interner) {
            return Some(make_queue_cmd(owner_id, type_id, execute_tick));
        }
    }

    // Priority 6: Extra power if needed.
    if produced < drained + 100 {
        if let Some(type_id) = find_buildable_matching(&options, rules, &sim.interner, |object| {
            object.category == ObjectCategory::Building && object.power > 0
        }) {
            return Some(make_queue_cmd(owner_id, type_id, execute_tick));
        }
    }

    None
}

/// Queue infantry and vehicle production if queues are empty.
fn queue_units(
    sim: &Simulation,
    owner: &str,
    rules: &RuleSet,
    execute_tick: u64,
    commands: &mut Vec<CommandEnvelope>,
) {
    let Some(owner_id) = sim.interner.get(owner) else {
        return;
    };
    let current_queue = production::queue_view_for_owner(sim, rules, owner);
    let options = production::build_options_for_owner(sim, rules, owner);

    queue_combat_lane_if_empty(
        &current_queue,
        &options,
        ObjectCategory::Infantry,
        production::ProductionCategory::Infantry,
        owner_id,
        execute_tick,
        commands,
    );

    // HouseClass owns independent Primary_ForVehicles (+0x53B4) and
    // Primary_ForShips (+0x53B8) factory lanes. A live object/tail in one must
    // neither suppress the other nor be duplicated by its sibling's vacancy.
    for category in [
        production::ProductionCategory::Vehicle,
        production::ProductionCategory::Ship,
    ] {
        queue_combat_lane_if_empty(
            &current_queue,
            &options,
            ObjectCategory::Vehicle,
            category,
            owner_id,
            execute_tick,
            commands,
        );
    }
}

fn queue_combat_lane_if_empty(
    current_queue: &[production::QueueItemView],
    options: &[production::BuildOption],
    object_category: ObjectCategory,
    queue_category: production::ProductionCategory,
    owner_id: InternedId,
    execute_tick: u64,
    commands: &mut Vec<CommandEnvelope>,
) {
    if current_queue
        .iter()
        .any(|item| item.queue_category == queue_category)
    {
        return;
    }
    if let Some(type_id) = pick_combat_unit(options, object_category, queue_category) {
        commands.push(make_queue_cmd(owner_id, type_id, execute_tick));
    }
}

/// Pick a combat unit to build from the available options.
/// Prefers units with weapons (Primary != None) and reasonable cost.
fn pick_combat_unit(
    options: &[production::BuildOption],
    object_category: ObjectCategory,
    queue_category: production::ProductionCategory,
) -> Option<InternedId> {
    let mut candidates: Vec<&production::BuildOption> = options
        .iter()
        .filter(|o| {
            o.enabled
                && o.object_category == object_category
                && o.queue_category == queue_category
                && o.cost > 0
        })
        .collect();
    // Sort by cost (cheapest first for fast army buildup).
    candidates.sort_by_key(|o| o.cost);
    // Pick the first (cheapest) available.
    candidates.first().map(|o| o.type_id)
}

/// Place any ready buildings near the AI's existing base.
fn place_ready_buildings(
    sim: &Simulation,
    _ai: &AiPlayerState,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    height_map: &BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    execute_tick: u64,
    commands: &mut Vec<CommandEnvelope>,
) {
    let owner = sim.interner.resolve(_ai.owner);
    let ready = production::ready_buildings_for_owner(sim, rules, owner);
    if ready.is_empty() {
        return;
    }

    // The ordinary approximation remains isolated from the native naval fast
    // path. A missing average structure center must not gate Naval=yes, whose
    // only origin authority is the House primary/alternate packed cell.
    let ordinary_base_center = find_base_center(sim, owner);
    let owner_id = sim.interner.get(owner);

    for ready_building in &ready {
        let type_id = ready_building.type_id;
        let type_id_str = sim.interner.resolve(type_id);
        let object = rules.object(type_id_str);
        let foundation = object.map(|obj| obj.foundation.as_str()).unwrap_or("1x1");
        let (fw, fh) = production::foundation_dimensions(foundation);

        let placement = if object.is_some_and(|object| object.naval) {
            owner_id.and_then(|owner_id| {
                crate::sim::naval_base_placement::find_naval_base_placement(
                    sim, rules, owner_id, path_grid,
                )
            })
        } else {
            ordinary_base_center.and_then(|(center_rx, center_ry)| {
                find_placement_cell(
                    sim,
                    rules,
                    owner,
                    type_id_str,
                    center_rx,
                    center_ry,
                    fw,
                    fh,
                    path_grid,
                    height_map,
                    overlay_registry,
                )
            })
        };
        if let (Some(owner_id), Some((rx, ry))) = (owner_id, placement) {
            commands.push(CommandEnvelope::new(
                owner_id,
                execute_tick,
                Command::PlaceReadyBuilding {
                    owner: owner_id,
                    type_id,
                    rx,
                    ry,
                },
            ));
        }
    }
}

/// Send an attack wave: gather idle military units and attack-move toward enemy.
fn send_attack_wave(
    sim: &Simulation,
    owner: &str,
    rules: &RuleSet,
    execute_tick: u64,
) -> Vec<CommandEnvelope> {
    let mut commands: Vec<CommandEnvelope> = Vec::new();

    // Find nearest enemy structure as attack target.
    let Some(target) = find_nearest_enemy_structure(sim, owner) else {
        return commands;
    };

    // Gather idle military units (no MovementTarget, not harvesters).
    let mut idle_units: Vec<(u64, u16, u16)> = Vec::new();
    for entity in sim.substrate.entities.values() {
        if !sim
            .interner
            .resolve(entity.owner())
            .eq_ignore_ascii_case(owner)
        {
            continue;
        }
        if entity.dying || entity.lifecycle.in_limbo {
            continue;
        }
        if !matches!(
            entity.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) {
            continue;
        }
        if production::is_harvester_type(rules, sim.interner.resolve(entity.type_ref())) {
            continue;
        }
        // Check if unit has no movement target (idle).
        if entity.movement_target.is_none() {
            idle_units.push((entity.stable_id(), entity.position.rx, entity.position.ry));
        }
    }
    idle_units.sort_by_key(|(sid, _, _)| *sid);

    // Send up to WAVE_SIZE units.
    let owner_id = match sim.interner.get(owner) {
        Some(id) => id,
        None => return commands,
    };
    for (entity_id, _, _) in idle_units.into_iter().take(AI_ATTACK_WAVE_SIZE) {
        commands.push(CommandEnvelope::new(
            owner_id,
            execute_tick,
            Command::AttackMove {
                entity_id,
                target_rx: target.0,
                target_ry: target.1,
                queue: false,
            },
        ));
    }

    if !commands.is_empty() {
        log::info!(
            "AI [{}] sending attack wave: {} units toward ({}, {})",
            owner,
            commands.len(),
            target.0,
            target.1
        );
    }

    commands
}

/// Find the center of the AI's base (average position of owned structures).
fn find_base_center(sim: &Simulation, owner: &str) -> Option<(u16, u16)> {
    let mut sum_x: i64 = 0;
    let mut sum_y: i64 = 0;
    let mut count: i64 = 0;
    for entity in sim.substrate.entities.values() {
        if !entity.dying
            && !entity.lifecycle.in_limbo
            && entity.category == EntityCategory::Structure
            && sim
                .interner
                .resolve(entity.owner())
                .eq_ignore_ascii_case(owner)
        {
            sum_x += i64::from(entity.position.rx);
            sum_y += i64::from(entity.position.ry);
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    Some((
        u16::try_from(sum_x / count).unwrap_or(u16::MAX),
        u16::try_from(sum_y / count).unwrap_or(u16::MAX),
    ))
}

/// Find the nearest enemy structure to attack.
fn find_nearest_enemy_structure(sim: &Simulation, owner: &str) -> Option<(u16, u16)> {
    let base_center = find_base_center(sim, owner)?;
    let mut best: Option<(u32, u16, u16)> = None;

    for entity in sim.substrate.entities.values() {
        if entity.dying || entity.lifecycle.in_limbo {
            continue;
        }
        if entity.category != EntityCategory::Structure {
            continue;
        }
        let e_owner = sim.interner.resolve(entity.owner());
        if e_owner.eq_ignore_ascii_case(owner) {
            continue;
        }
        // Skip neutral/civilian houses.
        let up = e_owner.to_ascii_uppercase();
        if matches!(
            up.as_str(),
            "NEUTRAL" | "SPECIAL" | "CIVILIAN" | "GOODGUY" | "BADGUY"
        ) {
            continue;
        }

        let dx = entity.position.rx as i64 - base_center.0 as i64;
        let dy = entity.position.ry as i64 - base_center.1 as i64;
        let dist_sq = (dx * dx + dy * dy) as u32;
        match best {
            Some((d, _, _)) if dist_sq >= d => {}
            _ => best = Some((dist_sq, entity.position.rx, entity.position.ry)),
        }
    }
    best.map(|(_, rx, ry)| (rx, ry))
}

/// Spiral outward from a center cell to find a valid building placement.
fn find_placement_cell(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    center_rx: u16,
    center_ry: u16,
    fw: u16,
    fh: u16,
    path_grid: Option<&PathGrid>,
    height_map: &BTreeMap<(u16, u16), u8>,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Option<(u16, u16)> {
    // Standard overlay walls get their candidates from the AI base-perimeter
    // scan, not the ordinary building-site helper's reservation phase. The
    // shared placement preview below remains the final wall-cell predicate.
    let requires_base_site_reservation = !rules.object(type_id).is_some_and(|object| object.wall);

    // Try placement in a spiral pattern around the base center.
    let max_radius: i32 = 12;
    for r in 0..=max_radius {
        let min_x = (center_rx as i32 - r).max(0);
        let max_x = (center_rx as i32 + r).min(511);
        let min_y = (center_ry as i32 - r).max(0);
        let max_y = (center_ry as i32 + r).min(511);

        // Only check the perimeter of this ring.
        for x in min_x..=max_x {
            for y in [min_y, max_y] {
                if requires_base_site_reservation
                    && !ai_base_reservation_candidate_ok(sim, rules, owner, type_id, x, y, fw, fh)
                {
                    continue;
                }
                let preview = production::placement_preview_for_owner_with_overlays(
                    sim,
                    rules,
                    owner,
                    type_id,
                    x as u16,
                    y as u16,
                    path_grid,
                    height_map,
                    overlay_registry,
                );
                if preview.as_ref().is_some_and(|p| p.valid) {
                    return Some((x as u16, y as u16));
                }
            }
        }
        for y in (min_y + 1)..max_y {
            for x in [min_x, max_x] {
                if requires_base_site_reservation
                    && !ai_base_reservation_candidate_ok(sim, rules, owner, type_id, x, y, fw, fh)
                {
                    continue;
                }
                let preview = production::placement_preview_for_owner_with_overlays(
                    sim,
                    rules,
                    owner,
                    type_id,
                    x as u16,
                    y as u16,
                    path_grid,
                    height_map,
                    overlay_registry,
                );
                if preview.as_ref().is_some_and(|p| p.valid) {
                    return Some((x as u16, y as u16));
                }
            }
        }
    }
    None
}

fn ai_base_reservation_candidate_ok(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    x: i32,
    y: i32,
    width: u16,
    height: u16,
) -> bool {
    let Some(owner_id) = sim.interner.get(owner) else {
        return false;
    };
    let Some(house_index) = sim.base_reservation_house_index(owner_id) else {
        return false;
    };
    let Some(building_type) = rules.object(type_id) else {
        return false;
    };
    let extra = i32::from(building_type.protect_with_wall || building_type.wants_extra_space);
    let border = rules.ai_base_spacing.wrapping_add(extra);
    let twice_border = border.wrapping_mul(2);
    let candidate_x = x as i16 as i32;
    let candidate_y = y as i16 as i32;
    let candidate = CellRect::new(
        candidate_x.wrapping_sub(border),
        candidate_y.wrapping_sub(border),
        i32::from(width).wrapping_add(twice_border),
        i32::from(height).wrapping_add(twice_border),
    );

    if !check_occupancy_rect(CellRectOccupancyContext {
        native_cells: None,
        rect: candidate,
        reservation_arg: house_index,
        reservations: Some(&sim.substrate.base_reservations),
        occupancy: Some(&sim.substrate.occupancy),
        entities: Some(&sim.substrate.entities),
        terrain_object_cells: Some(&sim.production.terrain_object_cells),
        resolved_terrain: sim.resolved_terrain.as_ref(),
        overlay_grid: sim.overlay_grid.as_ref(),
        playfield_bounds: sim.playfield_bounds,
    }) {
        return false;
    }

    // FUN_50B760's ordinary active game path requires connectivity to this
    // house's reservation network. Simulation has no verified raw game-mode-0
    // analogue, so no shortcut is synthesized here.
    let spacing = rules.ai_base_spacing;
    let max_extra = spacing.wrapping_mul(2);
    sim.substrate.base_reservations.has_reservation_inclusive(
        sim.resolved_terrain.as_ref(),
        x.wrapping_sub(spacing).wrapping_sub(1),
        y.wrapping_sub(spacing).wrapping_sub(1),
        x.wrapping_add(i32::from(width)).wrapping_add(max_extra),
        y.wrapping_add(i32::from(height)).wrapping_add(max_extra),
        house_index,
    )
}

fn find_buildable_matching<F>(
    options: &[production::BuildOption],
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    mut matches: F,
) -> Option<InternedId>
where
    F: FnMut(&crate::rules::object_type::ObjectType) -> bool,
{
    options.iter().find_map(|option| {
        let object = rules.object(interner.resolve(option.type_id))?;
        (option.enabled && matches(object)).then_some(option.type_id)
    })
}

fn find_buildable_refinery(
    options: &[production::BuildOption],
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<InternedId> {
    find_buildable_matching(options, rules, interner, |object| {
        object.category == ObjectCategory::Building && object.refinery
    })
}

fn count_refineries(sim: &Simulation, owner: &str, rules: &RuleSet) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| {
            !e.dying
                && e.category == EntityCategory::Structure
                && sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
                && rules.is_refinery_type(sim.interner.resolve(e.type_ref()))
        })
        .count()
}

/// Create a QueueProduction command envelope.
fn make_queue_cmd(owner: InternedId, type_id: InternedId, execute_tick: u64) -> CommandEnvelope {
    CommandEnvelope::new(
        owner,
        execute_tick,
        Command::QueueProduction {
            owner,
            type_id,
            mode: QueueMode::Append,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use super::*;
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::Health;
    use crate::sim::miner::{MinerState, ResourceType};
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::pathfinding::PathGrid;

    fn modded_ai_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MODHARV\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=MODCYRD\n\
             1=MODPOWR\n\
             2=FAKEREF\n\
             3=MODPROC\n\
             4=MODBARR\n\
             5=MODFACT\n\
             [MODCYRD]\n\
             Name=Mod Construction Yard\n\
             Foundation=2x2\n\
             Factory=BuildingType\n\
             UndeploysInto=MODMCV\n\
             Strength=1000\n\
             Cost=3000\n\
             TechLevel=1\n\
             Owner=Americans\n\
             BaseNormal=yes\n\
             [MODPOWR]\n\
             Name=Mod Power Support\n\
             Foundation=2x2\n\
             Power=200\n\
             Strength=800\n\
             Cost=600\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [FAKEREF]\n\
             Name=Fake Refinery\n\
             Foundation=3x3\n\
             Strength=800\n\
             Cost=900\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [MODPROC]\n\
             Name=Mod Ore Processor\n\
             Foundation=3x3\n\
             Refinery=yes\nDockUnload=yes\n\
             FreeUnit=MODHARV\n\
             Strength=900\n\
             Cost=900\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [MODBARR]\n\
             Name=Mod Infantry Node\n\
             Foundation=2x2\n\
             Factory=InfantryType\n\
             Strength=700\n\
             Cost=500\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [MODFACT]\n\
             Name=Mod Vehicle Node\n\
             Foundation=3x2\n\
             Factory=UnitType\n\
             Strength=900\n\
             Cost=800\n\
             TechLevel=1\n\
             Owner=Americans\n\
             ExitCoord=512,256,0\n\
             [MODHARV]\n\
             Name=Mod Harvester\n\
             Harvester=yes\n\
             Dock=MODPROC\n\
             Speed=6\n\
             Strength=600\n\
             Cost=1400\n\
             TechLevel=1\n\
             Owner=Americans\n",
        ))
        .expect("modded AI rules should parse")
    }

    fn spawn_structure(
        sim: &mut Simulation,
        sid: u64,
        owner: &str,
        type_id: &str,
        rx: u16,
        ry: u16,
    ) {
        let owner_id = sim.interner.intern(owner);
        let type_id_interned = sim.interner.intern(type_id);
        let mut ge = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            sid,
            rx,
            ry,
            0,
            0,
            owner_id,
            Health { current: 1000 },
            type_id_interned,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(ge);
        if sim.substrate.next_stable_object_id <= sid {
            sim.substrate.next_stable_object_id = sid + 1;
        }
    }

    #[test]
    fn test_ai_player_state_new() {
        let mut interner = crate::sim::intern::StringInterner::new();
        let owner_id = interner.intern("Russians");
        let state = AiPlayerState::new(owner_id);
        assert_eq!(state.owner, owner_id);
        assert_eq!(state.last_attack_frame, 0);
        assert!(!state.mcv_deployed);
    }

    #[test]
    fn tick_ai_skips_defeated_house() {
        // A house flagged is_defeated must issue NO command (the Phase-8 defeat
        // gate). Baseline: an undeployed MCV makes a live house emit DeployMcv,
        // so the empty result for the defeated house proves the gate fired.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\
             [AircraftTypes]\n\
             [VehicleTypes]\n\
             0=TSTMCV\n\
             [BuildingTypes]\n\
             0=TSTCYRD\n\
             [TSTMCV]\n\
             Name=Test MCV\n\
             DeploysInto=TSTCYRD\n\
             Speed=6\n\
             Strength=1000\n\
             TechLevel=1\n\
             Owner=Americans\n\
             [TSTCYRD]\n\
             Name=Test ConYard\n\
             Foundation=2x2\n\
             UndeploysInto=TSTMCV\n\
             Strength=1000\n\
             TechLevel=1\n\
             Owner=Americans\n",
        ))
        .expect("rules parse");
        let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

        // Build a sim holding one undeployed MCV for "Americans".
        let build_sim = || {
            let mut sim = Simulation::new();
            let owner_id = sim.interner.intern("Americans");
            let mcv_type = sim.interner.intern("TSTMCV");
            let mut ge = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
                1,
                5,
                5,
                0,
                0,
                owner_id,
                Health { current: 1000 },
                mcv_type,
                EntityCategory::Unit,
                0,
                5,
                false,
            );
            ge.lifecycle.in_limbo = false;
            sim.substrate.entities.insert(ge);
            (sim, owner_id)
        };

        // No house registered -> not defeated -> the MCV is deployed.
        let (sim, owner_id) = build_sim();
        let mut ai = vec![AiPlayerState::new(owner_id)];
        let live = tick_ai(&sim, &mut ai, &rules, None, &height_map, None);
        assert_eq!(live.len(), 1, "a live AI house deploys its MCV");
        assert!(matches!(live[0].payload, Command::DeployMcv { .. }));

        // Defeated house -> the gate skips it -> no commands.
        let (mut sim, owner_id) = build_sim();
        let mut house =
            crate::sim::house_state::HouseState::new(owner_id, 0, None, false, 10_000, 10);
        house.is_defeated = true;
        sim.houses.insert(owner_id, house);
        let mut ai = vec![AiPlayerState::new(owner_id)];
        let defeated = tick_ai(&sim, &mut ai, &rules, None, &height_map, None);
        assert!(
            defeated.is_empty(),
            "a defeated AI house must issue no command"
        );
    }

    #[test]
    fn gsi_04_07_ai_ready_wall_uses_authoritative_overlay_command_path() {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=GACNST\n\
             1=GAWALL\n\
             [OverlayTypes]\n\
             0=GASAND\n\
             1=CYCL\n\
             2=GAWALL\n\
             [GACNST]\n\
             Strength=1000\n\
             Foundation=2x2\n\
             Factory=BuildingType\n\
             BaseNormal=yes\n\
             [GASAND]\n\
             Wall=yes\n\
             Armor=wood\n\
             Strength=100\n\
             [CYCL]\n\
             [GAWALL]\n\
             Wall=yes\n\
             Armor=concrete\n\
             Strength=300\n\
             Cost=100\n\
             TechLevel=1\n\
             Foundation=1x1\n\
             Adjacent=0\n\
             GuardRange=5\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("AI wall rules");
        let art = ArtRegistry::from_ini(&IniFile::from_str("[GAWALL]\nToOverlay=GAWALL\n"));
        rules.merge_art_data(&art);
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let path_grid = PathGrid::new(32, 32);
        let height_map = BTreeMap::new();
        let mut sim = Simulation::new();
        sim.session.binary_frame = 1;
        sim.session.map_width = 32;
        sim.session.map_height = 32;
        sim.overlay_grid = Some(OverlayGrid::new(32, 32));
        spawn_structure(&mut sim, 1, "Americans", "GACNST", 10, 10);
        let owner = sim.interner.intern("Americans");
        sim.session.house_order.push(owner);
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 10_000, 10),
        );
        let conyard_profile = rules.object("GACNST").expect("ConYard profile");
        let conyard_foundation = conyard_profile.foundation.clone();
        let conyard_spacing = conyard_profile
            .base_reservation_spacing
            .expect("BaseNormal ConYard reservation spacing");
        let conyard = sim.substrate.entities.get_mut(1).expect("spawned ConYard");
        conyard.foundation = conyard_foundation;
        conyard.base_reservation_spacing = Some(conyard_spacing);
        let wall_type = sim.interner.intern("GAWALL");
        assert!(production::enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            "GAWALL"
        ));
        let wall_category =
            production::category_for_object(rules.object("GAWALL").expect("wall profile"));
        assert!(
            sim.production
                .factory_shadow
                .test_arm_ready(owner, wall_category)
        );
        assert!(!production::tick_production(
            &mut sim,
            &rules,
            &height_map,
            Some(&path_grid),
        ));
        assert_eq!(
            sim.production.ready_by_owner.get(&owner),
            Some(&VecDeque::from([wall_type])),
        );

        let mut ai = vec![AiPlayerState::new(owner)];
        let commands = tick_ai(
            &sim,
            &mut ai,
            &rules,
            Some(&path_grid),
            &height_map,
            Some(&registry),
        );
        let (rx, ry) = match commands.as_slice() {
            [
                CommandEnvelope {
                    payload:
                        Command::PlaceReadyBuilding {
                            owner: command_owner,
                            type_id,
                            rx,
                            ry,
                        },
                    ..
                },
            ] => {
                assert_eq!(*command_owner, owner);
                assert_eq!(*type_id, wall_type);
                (*rx, *ry)
            }
            other => panic!("ready wall should emit one placement command, got {other:?}"),
        };

        let entity_count = sim.substrate.entities.len();
        let tick = sim.advance_tick(
            &commands,
            Some(&rules),
            &height_map,
            Some(&path_grid),
            Some(&registry),
            67,
        );
        assert_eq!(tick.executed_commands, 1);
        assert!(!tick.spawned_entities);
        assert_eq!(sim.substrate.entities.len(), entity_count - 1);
        assert!(!sim.production.ready_by_owner.contains_key(&owner));
        let wall = sim
            .overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(rx, ry);
        assert_eq!(wall.overlay_id, registry.id_for_name("GAWALL"));
        assert_eq!(wall.wall_owner, Some(owner));
    }

    #[test]
    fn test_find_buildable_matching_positive_power_uses_rules_data() {
        let rules = modded_ai_rules();
        let mut interner = crate::sim::intern::StringInterner::new();
        let modpowr_id = interner.intern("MODPOWR");
        let options = vec![production::BuildOption {
            type_id: modpowr_id,
            display_name: "Mod Power Support".to_string(),
            cost: 600,
            object_category: ObjectCategory::Building,
            queue_category: production::ProductionCategory::Building,
            enabled: true,
            reason: None,
        }];
        assert_eq!(
            find_buildable_matching(&options, &rules, &interner, |object| object.power > 0),
            Some(modpowr_id)
        );
    }

    #[test]
    fn test_find_buildable_matching_factory_uses_rules_data() {
        let rules = modded_ai_rules();
        let mut interner = crate::sim::intern::StringInterner::new();
        let modbarr_id = interner.intern("MODBARR");
        let options = vec![production::BuildOption {
            type_id: modbarr_id,
            display_name: "Mod Infantry Node".to_string(),
            cost: 500,
            object_category: ObjectCategory::Building,
            queue_category: production::ProductionCategory::Building,
            enabled: true,
            reason: None,
        }];
        assert_eq!(
            find_buildable_matching(&options, &rules, &interner, |object| {
                object.factory == Some(FactoryType::InfantryType)
            }),
            Some(modbarr_id)
        );
    }

    #[test]
    fn test_find_buildable_matching_no_match_when_disabled() {
        let rules = modded_ai_rules();
        let mut interner = crate::sim::intern::StringInterner::new();
        let modpowr_id = interner.intern("MODPOWR");
        let options = vec![production::BuildOption {
            type_id: modpowr_id,
            display_name: "Mod Power Support".to_string(),
            cost: 600,
            object_category: ObjectCategory::Building,
            queue_category: production::ProductionCategory::Building,
            enabled: false,
            reason: None,
        }];
        assert_eq!(
            find_buildable_matching(&options, &rules, &interner, |object| object.power > 0),
            None
        );
    }

    #[test]
    fn test_make_queue_cmd() {
        let mut interner = crate::sim::intern::StringInterner::new();
        let owner_id = interner.intern("Americans");
        let type_id = interner.intern("MODPOWR");
        let cmd = make_queue_cmd(owner_id, type_id, 10);
        assert_eq!(cmd.owner, owner_id);
        assert_eq!(cmd.execute_tick, 10);
        assert!(matches!(
            cmd.payload,
            Command::QueueProduction {
                owner: cmd_owner,
                type_id: cmd_type,
                ..
            } if cmd_owner == owner_id && cmd_type == type_id
        ));
    }

    fn combat_option(
        interner: &mut crate::sim::intern::StringInterner,
        id: &str,
        queue_category: production::ProductionCategory,
        cost: i32,
    ) -> production::BuildOption {
        production::BuildOption {
            type_id: interner.intern(id),
            display_name: id.to_string(),
            cost,
            object_category: ObjectCategory::Vehicle,
            queue_category,
            enabled: true,
            reason: None,
        }
    }

    fn queued_combat_item(
        interner: &mut crate::sim::intern::StringInterner,
        id: &str,
        queue_category: production::ProductionCategory,
    ) -> production::QueueItemView {
        production::QueueItemView {
            type_id: interner.intern(id),
            display_name: id.to_string(),
            queue_category,
            state: production::BuildQueueState::Building,
            remaining_ms: 500,
            total_ms: 1_000,
        }
    }

    #[test]
    fn ai_active_ship_lane_is_not_duplicated_and_empty_vehicle_lane_queues_land() {
        let mut interner = crate::sim::intern::StringInterner::new();
        let owner = interner.intern("Americans");
        let dest = combat_option(
            &mut interner,
            "DEST",
            production::ProductionCategory::Ship,
            900,
        );
        let active_ship = vec![queued_combat_item(
            &mut interner,
            "DEST",
            production::ProductionCategory::Ship,
        )];

        let mut commands = Vec::new();
        queue_combat_lane_if_empty(
            &active_ship,
            std::slice::from_ref(&dest),
            ObjectCategory::Vehicle,
            production::ProductionCategory::Ship,
            owner,
            8,
            &mut commands,
        );
        assert!(
            commands.is_empty(),
            "an active Ship lane must not receive another Ship tail"
        );

        let mtnk = combat_option(
            &mut interner,
            "MTNK",
            production::ProductionCategory::Vehicle,
            700,
        );
        let options = vec![dest, mtnk.clone()];
        queue_combat_lane_if_empty(
            &active_ship,
            &options,
            ObjectCategory::Vehicle,
            production::ProductionCategory::Vehicle,
            owner,
            8,
            &mut commands,
        );
        assert!(matches!(
            commands.as_slice(),
            [CommandEnvelope {
                payload: Command::QueueProduction { type_id, .. },
                ..
            }] if *type_id == mtnk.type_id
        ));
    }

    #[test]
    fn ai_active_vehicle_lane_does_not_block_empty_ship_lane() {
        let mut interner = crate::sim::intern::StringInterner::new();
        let owner = interner.intern("Americans");
        let active_vehicle = vec![queued_combat_item(
            &mut interner,
            "MTNK",
            production::ProductionCategory::Vehicle,
        )];
        let dest = combat_option(
            &mut interner,
            "DEST",
            production::ProductionCategory::Ship,
            900,
        );
        let mut commands = Vec::new();

        queue_combat_lane_if_empty(
            &active_vehicle,
            std::slice::from_ref(&dest),
            ObjectCategory::Vehicle,
            production::ProductionCategory::Vehicle,
            owner,
            8,
            &mut commands,
        );
        assert!(
            commands.is_empty(),
            "Vehicle lane must not select a Ship option"
        );
        queue_combat_lane_if_empty(
            &active_vehicle,
            std::slice::from_ref(&dest),
            ObjectCategory::Vehicle,
            production::ProductionCategory::Ship,
            owner,
            8,
            &mut commands,
        );
        assert!(matches!(
            commands.as_slice(),
            [CommandEnvelope {
                payload: Command::QueueProduction { type_id, .. },
                ..
            }] if *type_id == dest.type_id
        ));
    }

    #[test]
    fn test_find_buildable_refinery_matches_rules_flag() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             0=FAKEREF\n\
             1=MODPROC\n\
             [FAKEREF]\n\
             Name=Fake Refinery\n\
             [MODPROC]\n\
             Refinery=yes\nDockUnload=yes\n",
        ))
        .expect("rules should parse");
        let mut interner = crate::sim::intern::StringInterner::new();
        let fakeref_id = interner.intern("FAKEREF");
        let modproc_id = interner.intern("MODPROC");
        let options = vec![
            production::BuildOption {
                type_id: fakeref_id,
                display_name: "Fake Refinery".to_string(),
                cost: 1000,
                object_category: ObjectCategory::Building,
                queue_category: production::ProductionCategory::Building,
                enabled: true,
                reason: None,
            },
            production::BuildOption {
                type_id: modproc_id,
                display_name: "Mod Refinery".to_string(),
                cost: 1200,
                object_category: ObjectCategory::Building,
                queue_category: production::ProductionCategory::Building,
                enabled: true,
                reason: None,
            },
        ];

        assert_eq!(
            find_buildable_refinery(&options, &rules, &interner),
            Some(modproc_id)
        );
    }

    #[test]
    fn test_count_refineries_ignores_ref_like_names_without_flag() {
        let rules = modded_ai_rules();
        let mut sim = Simulation::new();
        spawn_structure(&mut sim, 1, "Americans", "FAKEREF", 10, 10);
        spawn_structure(&mut sim, 2, "Americans", "MODPROC", 14, 10);

        assert!(has_refinery_structure(&sim, "Americans", &rules));
        assert_eq!(count_refineries(&sim, "Americans", &rules), 1);
    }

    #[test]
    fn test_has_factory_structure_uses_factory_flag() {
        let rules = modded_ai_rules();
        let mut sim = Simulation::new();
        spawn_structure(&mut sim, 1, "Americans", "MODBARR", 10, 10);
        spawn_structure(&mut sim, 2, "Americans", "MODFACT", 15, 10);

        assert!(has_factory_structure(
            &sim,
            "Americans",
            &rules,
            FactoryType::InfantryType
        ));
        assert!(has_factory_structure(
            &sim,
            "Americans",
            &rules,
            FactoryType::UnitType
        ));
    }

    #[test]
    fn test_decide_next_building_uses_custom_power_and_factory_flags() {
        let rules = modded_ai_rules();
        let mut sim = Simulation::new();
        // Pre-intern all type names so decide_next_building can find them.
        let modpowr_id = sim.interner.intern("MODPOWR");
        let modproc_id = sim.interner.intern("MODPROC");
        let modbarr_id = sim.interner.intern("MODBARR");
        let modfact_id = sim.interner.intern("MODFACT");
        spawn_structure(&mut sim, 1, "Americans", "MODCYRD", 10, 10);

        let first = decide_next_building(&sim, "Americans", &rules, 1).expect("power build");
        assert!(matches!(
            first.payload,
            Command::QueueProduction { type_id, .. } if type_id == modpowr_id
        ));

        spawn_structure(&mut sim, 2, "Americans", "MODPOWR", 14, 10);
        let second = decide_next_building(&sim, "Americans", &rules, 2).expect("refinery build");
        assert!(matches!(
            second.payload,
            Command::QueueProduction { type_id, .. } if type_id == modproc_id
        ));

        spawn_structure(&mut sim, 3, "Americans", "MODPROC", 18, 10);
        let third =
            decide_next_building(&sim, "Americans", &rules, 3).expect("infantry factory build");
        assert!(matches!(
            third.payload,
            Command::QueueProduction { type_id, .. } if type_id == modbarr_id
        ));

        spawn_structure(&mut sim, 4, "Americans", "MODBARR", 22, 10);
        let fourth =
            decide_next_building(&sim, "Americans", &rules, 4).expect("vehicle factory build");
        assert!(matches!(
            fourth.payload,
            Command::QueueProduction { type_id, .. } if type_id == modfact_id
        ));
    }

    #[test]
    #[ignore = "WIP: modded refinery harvest cycle not yet landed"]
    fn test_modded_refinery_cycle_uses_custom_flags_end_to_end() {
        let rules = modded_ai_rules();
        let mut sim = Simulation::new();
        let grid = PathGrid::new(64, 64);
        let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

        spawn_structure(&mut sim, 1, "Americans", "MODCYRD", 10, 10);

        assert!(production::enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            "MODPROC"
        ));
        for _ in 0..240 {
            let _ = sim.advance_tick(&[], Some(&rules), &height_map, Some(&grid), None, 33);
            if !production::ready_buildings_for_owner(&sim, &rules, "Americans").is_empty() {
                break;
            }
        }
        let modproc_id = sim
            .interner
            .get("MODPROC")
            .expect("MODPROC should be interned");
        assert_eq!(
            production::ready_buildings_for_owner(&sim, &rules, "Americans")
                .into_iter()
                .map(|building| building.type_id)
                .collect::<Vec<_>>(),
            vec![modproc_id]
        );

        assert!(production::place_ready_building_without_overlays(
            &mut sim,
            &rules,
            "Americans",
            "MODPROC",
            14,
            10,
            Some(&grid),
            &height_map,
        ));

        let refinery_sid = sim
            .substrate
            .entities
            .values()
            .find_map(|e| {
                (sim.interner
                    .resolve(e.owner)
                    .eq_ignore_ascii_case("Americans")
                    && sim.interner.resolve(e.type_ref) == "MODPROC"
                    && e.category == EntityCategory::Structure)
                    .then_some(e.stable_id)
            })
            .expect("placed refinery should exist");

        let miner_sid = sim
            .substrate
            .entities
            .values()
            .find_map(|e| {
                (sim.interner
                    .resolve(e.owner)
                    .eq_ignore_ascii_case("Americans")
                    && sim.interner.resolve(e.type_ref) == "MODHARV"
                    && e.category == EntityCategory::Unit)
                    .then_some(e.stable_id)
            })
            .expect("free harvester should spawn");

        assert_eq!(count_refineries(&sim, "Americans", &rules), 1);
        assert!(has_refinery_structure(&sim, "Americans", &rules));
        let credits_before_unload = production::credits_for_owner(&sim, "Americans");

        // Whole-multiple of the ore base (120) so the cell drains cleanly.
        // The production overlay seeder stores `(frame+1) * base`, so a
        // sub-density-level leftover never occurs on real maps.
        crate::sim::tiberium::test_support::place_stock_amount(
            &mut sim,
            (19, 12),
            ResourceType::Ore,
            120,
        );

        let mut saw_harvest = false;
        let mut saw_return = false;
        let mut saw_dock_or_unload = false;
        let mut saw_dock_reservation = false;
        let mut saw_unload = false;
        let mut saw_home_refinery = false;

        // Needs enough ticks for: harvest + return + unload.
        // Unload alone: up to 20 bales × 57 ticks/bale = 1140 ticks at 60Hz.
        for _ in 0..2000 {
            let _ = sim.advance_tick(&[], Some(&rules), &height_map, Some(&grid), None, 33);

            let entity = sim
                .substrate
                .entities
                .get(miner_sid)
                .expect("miner entity should exist");
            let miner = entity.miner.as_ref().expect("miner component should exist");
            match entity.miner_state().expect("miner cursor") {
                MinerState::Harvest => saw_harvest = true,
                MinerState::ReturnToRefinery => saw_return = true,
                MinerState::Dock => saw_dock_or_unload = true,
                _ => {}
            }
            // The War Miner unloads in its native Unload mission.
            if entity.mission.current().known() == Some(crate::sim::mission::MissionType::Unload) {
                saw_dock_or_unload = true;
                saw_unload = true;
            }
            if miner.home_refinery == Some(refinery_sid) {
                saw_home_refinery = true;
            }

            if crate::sim::miner::miner_dock::has_contact(&sim, refinery_sid, miner_sid) {
                saw_dock_reservation = true;
            }

            if saw_unload
                && production::credits_for_owner(&sim, "Americans") > credits_before_unload
                && saw_home_refinery
            {
                break;
            }
        }

        let miner = sim
            .substrate
            .entities
            .get(miner_sid)
            .and_then(|e| e.miner.as_ref())
            .expect("miner component should exist");

        assert!(saw_harvest, "miner should harvest ore");
        assert!(saw_return, "miner should return to refinery");
        assert!(
            saw_dock_or_unload,
            "miner should dock or unload at the refinery"
        );
        assert!(
            saw_dock_reservation,
            "refinery dock reservation should be used"
        );
        assert!(saw_unload, "miner should reach unload state");
        assert!(
            saw_home_refinery,
            "miner should complete unloading at the refinery"
        );
        assert!(
            production::credits_for_owner(&sim, "Americans") > credits_before_unload,
            "unloading should increase owner credits"
        );
        assert!(
            crate::sim::tiberium::test_support::bales_at(&sim, 19, 12) == 0,
            "the single ore bale should be consumed during harvesting"
        );
        assert_eq!(miner.home_refinery, Some(refinery_sid));
        assert_eq!(count_refineries(&sim, "Americans", &rules), 1);
    }

    #[test]
    fn gsi_04_05_reservation_ai_requires_free_candidate_then_same_house_near() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[AI]\nAIBaseSpacing=1\n\
             [BuildingTypes]\n0=NORMAL\n1=PROTECTED\n2=SPACIOUS\n\
             [NORMAL]\nFoundation=2x2\n\
             [PROTECTED]\nFoundation=2x2\nProtectWithWall=yes\n\
             [SPACIOUS]\nFoundation=2x2\nWantsExtraSpace=yes\n",
        ))
        .expect("AI reservation rules");
        let normal = rules.object("NORMAL").unwrap();
        assert!(!normal.protect_with_wall);
        assert!(!normal.wants_extra_space);
        assert!(!normal.wall, "ProtectWithWall is not Wall identity");
        assert!(rules.object("PROTECTED").unwrap().protect_with_wall);
        assert!(rules.object("SPACIOUS").unwrap().wants_extra_space);
        let mut sim = Simulation::new();
        sim.session.map_width = 64;
        sim.session.map_height = 64;
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -128,
            off_100: -128,
            off_104: 256,
            off_108: 256,
        });
        let owner = sim.interner.intern("Americans");
        let other = sim.interner.intern("Russians");
        sim.session.house_order.extend([owner, other]);

        assert!(
            !ai_base_reservation_candidate_ok(&sim, &rules, "Americans", "NORMAL", 10, 10, 2, 2),
            "isolated candidate has no same-house network"
        );
        sim.substrate.base_reservations.reserve(None, 8, 10, 1);
        assert!(
            !ai_base_reservation_candidate_ok(&sim, &rules, "Americans", "NORMAL", 10, 10, 2, 2),
            "another house's nearby bit is not connectivity"
        );
        sim.substrate.base_reservations.reserve(None, 8, 10, 0);
        assert!(ai_base_reservation_candidate_ok(
            &sim,
            &rules,
            "Americans",
            "NORMAL",
            10,
            10,
            2,
            2
        ));
        assert!(
            !ai_base_reservation_candidate_ok(&sim, &rules, "Americans", "PROTECTED", 10, 10, 2, 2),
            "ProtectWithWall expands b from 1 to 2, so (8,10) blocks first phase"
        );
        assert!(
            !ai_base_reservation_candidate_ok(&sim, &rules, "Americans", "SPACIOUS", 10, 10, 2, 2),
            "WantsExtraSpace uses the same one-cell extra border"
        );

        sim.substrate.base_reservations.reserve(None, 9, 10, 0);
        assert!(
            !ai_base_reservation_candidate_ok(&sim, &rules, "Americans", "NORMAL", 10, 10, 2, 2),
            "normal b=1 first-phase CheckOccupancy includes (9,10)"
        );
        sim.substrate.base_reservations.clear(None, 9, 10, 0);

        sim.substrate.base_reservations.reserve(None, 10, 10, 0);
        assert!(
            !ai_base_reservation_candidate_ok(&sim, &rules, "Americans", "NORMAL", 10, 10, 2, 2),
            "CheckOccupancy rejects same-house overlap before the near test"
        );
        sim.substrate.base_reservations.clear(None, 10, 10, 0);
        assert!(ai_base_reservation_candidate_ok(
            &sim,
            &rules,
            "Americans",
            "NORMAL",
            10,
            10,
            2,
            2
        ));
    }
}
