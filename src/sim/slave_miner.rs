//! Slave Miner system — deploy/undeploy, slave spawn, slave harvest AI, scan correction.
//!
//! The Slave Miner (SMIN) is Yuri's harvester. Unlike War/Chrono Miners it does NOT
//! harvest directly. Instead it deploys into a refinery building (YAREFN) and spawns
//! SLAV infantry who do the actual harvesting. Slaves pick up ore bales, walk them
//! back to the deployed master, and deposit credits directly (no dock queue needed).
//!
//! ## Key behaviors (from RA2/YR)
//! - **Deploy**: SMIN vehicle → YAREFN building + spawn `SlavesNumber` (5) SLAV infantry
//! - **Undeploy**: YAREFN building → SMIN vehicle, slaves recalled/killed
//! - **Slave harvest loop**: SearchOre → MoveToOre → Harvest → ReturnToMaster → Deposit
//! - **Slave regen**: Dead slaves respawn after `SlaveRegenRate` (500) frames
//! - **Scan correction**: Deployed YAREFN periodically checks if a closer ore patch exists
//!   (SlaveMinerKickFrameDelay=150 frames, SlaveMinerScanCorrection=3 cells improvement)
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/miner, sim/miner_system, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::economy::apply_income_mult;
use crate::sim::house_state::{house_state_for_owner_mut, income_ppm_for_owner};
use crate::sim::intern::InternedId;
use crate::sim::miner::extract_bale;
use crate::sim::miner::miner_system::{
    effective_purifier_count, is_cell_path_clear_for_scan, resource_cell_present,
    search_local_resource,
};
use crate::sim::miner::{CargoBale, MinerConfig};
use crate::sim::pathfinding::PathGrid;
use crate::sim::production::credits_entry_for_owner;
use crate::sim::world::{PlacementEvidence, Simulation};

/// Deployed state of a Slave Miner (SMIN vehicle ↔ YAREFN building).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SlaveMinerMode {
    /// SMIN vehicle form — moving toward ore field to deploy.
    Mobile,
    /// SMIN → YAREFN deploy animation in progress.
    Deploying,
    /// YAREFN building form — slaves are active.
    Deployed,
    /// YAREFN → SMIN undeploy animation in progress.
    Undeploying,
}

/// Slave harvest AI state machine — one per SLAV infantry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SlaveHarvestState {
    /// Looking for nearest ore cell within scan radius of master.
    SearchOre,
    /// Walking toward the target ore cell.
    MoveToOre,
    /// Extracting bales from the ore cell.
    Harvest,
    /// Walking back to the deployed master (YAREFN).
    ReturnToMaster,
    /// Depositing cargo at the master — credits awarded immediately.
    Deposit,
    /// No ore found — idle near master.
    Idle,
}

/// ECS component for SLAV infantry — attached to slave entities.
///
/// Each slave has its own mini harvest loop: find ore near master,
/// walk to it, harvest, walk back, deposit at master, repeat.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SlaveHarvester {
    /// Stable ID of the master entity (deployed YAREFN or mobile SMIN).
    pub master_id: u64,
    /// Current harvest AI state.
    pub state: SlaveHarvestState,
    /// Cargo bales currently carried by this slave.
    pub cargo: Vec<CargoBale>,
    /// Max bales this slave can carry (Storage=4 for SLAV).
    pub capacity: u16,
    /// Countdown timer for harvest action (HarvestRate=150 frames).
    pub harvest_timer: u32,
    /// The ore cell being targeted.
    pub target_cell: Option<(u16, u16)>,
}

impl SlaveHarvester {
    /// Create a new SlaveHarvester bound to a master entity.
    pub fn new(master_id: u64, capacity: u16) -> Self {
        Self {
            master_id,
            state: SlaveHarvestState::SearchOre,
            cargo: Vec::with_capacity(capacity as usize),
            capacity,
            harvest_timer: 0,
            target_cell: None,
        }
    }

    /// True when cargo is at capacity.
    pub fn is_full(&self) -> bool {
        self.cargo.len() as u16 >= self.capacity
    }

    /// Total credit value of all bales currently carried.
    pub fn cargo_value(&self) -> u32 {
        self.cargo.iter().map(|b| b.value as u32).sum()
    }
}

/// Snapshot of one slave entity for two-phase processing.
struct SlaveSnapshot {
    entity_id: u64,
    owner: InternedId,
    rx: u16,
    ry: u16,
    harvester: SlaveHarvester,
}

/// Slave-side combined scan filter. Mirrors `miner_system::build_scan_filter`'s
/// occupancy/path-grid check; zone reachability is skipped (slaves anchor to
/// the master refinery and the slave path planner handles per-step passability).
fn build_slave_scan_filter<'a>(
    sim: &'a Simulation,
    path_grid: Option<&'a PathGrid>,
    self_id: u64,
) -> Box<dyn Fn((u16, u16)) -> bool + 'a> {
    let occupancy = &sim.substrate.occupancy;
    Box::new(move |cell: (u16, u16)| {
        is_cell_path_clear_for_scan(occupancy, path_grid, cell, self_id)
    })
}

/// Tick all slave harvesters. Called once per sim tick from resource economy.
///
/// Uses the two-phase snapshot pattern: snapshot → process → write back.
pub(super) fn tick_slave_harvesters(
    sim: &mut Simulation,
    live_order: &[u64],
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    // Phase 1: Snapshot all slave harvesters.
    let mut snapshots: Vec<SlaveSnapshot> = Vec::new();
    for &id in live_order {
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        let Some(ref sh) = entity.slave_harvester else {
            continue;
        };
        snapshots.push(SlaveSnapshot {
            entity_id: id,
            owner: entity.owner(),
            rx: entity.position.rx,
            ry: entity.position.ry,
            harvester: sh.clone(),
        });
    }

    if snapshots.is_empty() {
        return;
    }

    // Phase 2: Process each slave.
    for snap in &mut snapshots {
        process_slave(sim, rules, config, path_grid, overlay_registry, snap);
    }

    // Phase 3: Write back.
    for snap in &snapshots {
        if let Some(entity) = sim.substrate.entities.get_mut(snap.entity_id) {
            entity.slave_harvester = Some(snap.harvester.clone());
        }
    }
}

/// Process one slave through its harvest state machine.
fn process_slave(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut SlaveSnapshot,
) {
    // Check master is still alive.
    if sim
        .substrate
        .entities
        .get(snap.harvester.master_id)
        .is_none()
    {
        // Master destroyed — slave becomes idle (in real RA2, freed slaves wander).
        snap.harvester.state = SlaveHarvestState::Idle;
        return;
    }

    match snap.harvester.state {
        SlaveHarvestState::SearchOre => {
            handle_slave_search(sim, rules, config, path_grid, overlay_registry, snap)
        }
        SlaveHarvestState::MoveToOre => handle_slave_move_to_ore(snap),
        SlaveHarvestState::Harvest => {
            handle_slave_harvest(sim, rules, config, overlay_registry, snap)
        }
        SlaveHarvestState::ReturnToMaster => handle_slave_return(sim, snap),
        SlaveHarvestState::Deposit => handle_slave_deposit(sim, rules, config, snap),
        SlaveHarvestState::Idle => {
            handle_slave_idle(sim, rules, config, path_grid, overlay_registry, snap)
        }
    }
}

/// Slave searches for ore within SlaveMinerSlaveScan of the master.
fn handle_slave_search(
    sim: &Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut SlaveSnapshot,
) {
    let scan_radius: u16 = rules.general.slave_miner_slave_scan.max(1) as u16;

    // Search from the master's position (slaves harvest around their deployed base).
    let master_pos = sim
        .substrate
        .entities
        .get(snap.harvester.master_id)
        .map(|e| (e.position.rx, e.position.ry))
        .unwrap_or((snap.rx, snap.ry));

    let scan_filter = build_slave_scan_filter(sim, path_grid, snap.entity_id);
    let filter_ref: Option<&dyn Fn((u16, u16)) -> bool> = Some(&*scan_filter);
    if let Some(cell) = search_local_resource(
        sim,
        rules,
        overlay_registry,
        master_pos,
        scan_radius,
        filter_ref,
        config,
    ) {
        snap.harvester.target_cell = Some(cell);
        snap.harvester.state = SlaveHarvestState::MoveToOre;
    } else {
        snap.harvester.state = SlaveHarvestState::Idle;
    }
}

/// Slave moving toward ore. Simplified: instant arrival if at target cell.
/// Real movement is driven by the locomotor system; here we check proximity.
fn handle_slave_move_to_ore(snap: &mut SlaveSnapshot) {
    let Some(target) = snap.harvester.target_cell else {
        snap.harvester.state = SlaveHarvestState::SearchOre;
        return;
    };

    // If the slave has arrived at (or is adjacent to) the target, start harvesting.
    let dx: i32 = snap.rx as i32 - target.0 as i32;
    let dy: i32 = snap.ry as i32 - target.1 as i32;
    if dx.abs() <= 1 && dy.abs() <= 1 {
        snap.harvester.state = SlaveHarvestState::Harvest;
        snap.harvester.harvest_timer = 0;
    }
    // Movement itself is handled by the locomotor/movement system.
}

/// Slave extracting bales from the ore cell.
fn handle_slave_harvest(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut SlaveSnapshot,
) {
    let Some(cell) = snap.harvester.target_cell else {
        snap.harvester.state = SlaveHarvestState::SearchOre;
        return;
    };

    // Check if ore still exists at target.
    let has_ore = resource_cell_present(sim, rules, overlay_registry, cell);

    if !has_ore {
        // Ore depleted — search for more.
        snap.harvester.target_cell = None;
        if snap.harvester.cargo.is_empty() {
            snap.harvester.state = SlaveHarvestState::SearchOre;
        } else {
            snap.harvester.state = SlaveHarvestState::ReturnToMaster;
        }
        return;
    }

    // Harvest timer countdown.
    if snap.harvester.harvest_timer > 0 {
        snap.harvester.harvest_timer -= 1;
        return;
    }

    // Extract one bale.
    if let Some(bale) = extract_bale(sim, rules, overlay_registry, cell, config) {
        snap.harvester.cargo.push(bale);
    }

    if snap.harvester.is_full() {
        snap.harvester.state = SlaveHarvestState::ReturnToMaster;
    } else {
        // Reset harvest timer (HarvestRate from rules, stored on the ObjectType).
        // Default 150 frames at RA2's 15fps logic = 10 seconds.
        // At 15Hz sim, 1 tick = 1 RA2 game frame, so use directly.
        snap.harvester.harvest_timer = SLAVE_HARVEST_RATE_TICKS;
    }
}

/// Default harvest rate for slaves (HarvestRate=150 in rulesmd.ini).
/// In a fully data-driven version this would come from the SLAV ObjectType's
/// `harvest_rate` field. For now, use the standard value.
const SLAVE_HARVEST_RATE_TICKS: u32 = 150;

/// Slave returning to master. Check proximity to master position.
fn handle_slave_return(sim: &Simulation, snap: &mut SlaveSnapshot) {
    let Some(master) = sim.substrate.entities.get(snap.harvester.master_id) else {
        snap.harvester.state = SlaveHarvestState::Idle;
        return;
    };
    let mx: u16 = master.position.rx;
    let my: u16 = master.position.ry;

    let dx: i32 = snap.rx as i32 - mx as i32;
    let dy: i32 = snap.ry as i32 - my as i32;

    // Adjacent or on master cell → start depositing.
    if dx.abs() <= 2 && dy.abs() <= 2 {
        snap.harvester.state = SlaveHarvestState::Deposit;
    }
    // Movement handled by locomotor system.
}

/// Slave depositing cargo at master — the whole cargo is credited in one
/// visit, one call per non-empty storage slot.
///
/// gamemd: `SlaveManagerClass::AI_Update @ 0x006AFBB2..0x006AFBD2` (slave cell
/// == the return cell, slave `+0x5A4 == 0`) calls
/// `BuildingClass__DepositOreFromStorage @ 0x00522D50` with `ECX = the slave`
/// and the master building as its stack argument. That body walks the
/// SLAVE's own `StorageClass` (`LEA EBP,[ECX+0x33c]` at `0x00522D55`) slot by
/// slot (`FindFirstNonEmptySlot @ 0x006C9820`), and per slot with the MASTER's
/// owner (`[building+0x21C]`, `0x00522D75`):
///
/// ```text
/// purifiers = owner+0x538C (+ AIVirtualPurifiers[owner+0x184] when
///             owner+0x1EC == 0 and g_GameMode != 0)        // 0x00522D7B..0x00522DAE
/// amount    = GetAmount(slot)                              // whole slot, float
/// bonus_f32 = (float)purifiers * PurifierBonus * amount    // FILD/FMUL/FMUL/FSTP, 0x00522DCD..0x00522DE3
/// removed   = RemoveAmount(amount, slot)
/// if removed > 0 { Add_Tiberium_Credits(removed, slot); if bonus > 0 { Add_Tiberium_Credits(bonus, slot) } }
/// ```
///
/// `Add_Tiberium_Credits @ 0x004F9610` is one `ftol(Value × IncomeMult ×
/// amount + credits)` per call, so the purifier bonus truncates ONCE over
/// the whole slot (Medium-AI Yuri, 4 ore bales: `trunc(0.5 × 4 × 25) = 50`,
/// not `4 × trunc(12.5) = 48`). The float32 rounding of `bonus_f32` is exact
/// for every stock input (scan §2.5 bound); modded fractional
/// `PurifierBonus`/`IncomeMult` may differ by 1 credit from this integer fold.
fn handle_slave_deposit(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    snap: &mut SlaveSnapshot,
) {
    if snap.harvester.cargo.is_empty() {
        snap.harvester.state = SlaveHarvestState::SearchOre;
        return;
    }

    // The money goes to the MASTER building's owner (`param_1[0x87]`), not
    // the slave's; both are the same house in every stock path.
    let master_owner = sim
        .substrate
        .entities
        .get(snap.harvester.master_id)
        .map_or(snap.owner, |master| master.owner());
    let owner_str = sim.interner.resolve(master_owner).to_string();
    let income_ppm = income_ppm_for_owner(&sim.houses, &sim.interner, rules, &owner_str);
    let purifier_count = effective_purifier_count(sim, rules, &owner_str);
    let bonus_ppm = rules.general.purifier_bonus_ppm;

    // Slot order: `FindFirstNonEmptySlot` walks slots 0..3 ascending. VERA's
    // two resource kinds map to Riparius (ore, slot 0) and Cruentus (gems,
    // slot 1) — the harvester deposit path uses the same order.
    let cargo: Vec<CargoBale> = std::mem::take(&mut snap.harvester.cargo);
    for slot_kind in [
        crate::sim::miner::ResourceType::Ore,
        crate::sim::miner::ResourceType::Gem,
    ] {
        let bales: Vec<&CargoBale> = cargo
            .iter()
            .filter(|bale| bale.resource_type == slot_kind)
            .collect();
        if bales.is_empty() {
            continue;
        }
        let bale_count = bales.len() as i32;
        let slot_value: i32 = bales.iter().map(|bale| i32::from(bale.value)).sum();

        // First `Add_Tiberium_Credits(removed, slot)`: the whole slot, one ftol.
        let base_credits = apply_income_mult(slot_value, income_ppm);
        // Second call, `bonus > 0` only: one ftol over the whole slot.
        let bonus_credits = crate::sim::economy::purifier_bonus_credits(
            slot_value,
            purifier_count,
            bonus_ppm,
            income_ppm,
        );
        {
            let credits: &mut i32 = credits_entry_for_owner(sim, &owner_str);
            *credits = credits.saturating_add(base_credits.saturating_add(bonus_credits));
        }
        // Score `+0x54E8` term (`amount × 5.0`) per call, statistics only.
        if let Some(h) = house_state_for_owner_mut(&mut sim.houses, &owner_str, &sim.interner) {
            h.economy.add_harvested(bale_count);
            h.economy
                .add_harvested_raw(crate::sim::economy::purifier_bonus_harvested(
                    bale_count,
                    purifier_count,
                    bonus_ppm,
                ));
        }
    }

    // Storage is empty after the one visit; the next visit re-scans.
    let _ = config;
}

/// Slave idle — periodically re-scan for ore.
fn handle_slave_idle(
    sim: &Simulation,
    rules: &RuleSet,
    config: &MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    snap: &mut SlaveSnapshot,
) {
    // Try to find ore every few ticks (reuse search logic).
    let scan_radius: u16 = rules.general.slave_miner_slave_scan.max(1) as u16;
    let master_pos = sim
        .substrate
        .entities
        .get(snap.harvester.master_id)
        .map(|e| (e.position.rx, e.position.ry))
        .unwrap_or((snap.rx, snap.ry));

    let scan_filter = build_slave_scan_filter(sim, path_grid, snap.entity_id);
    let filter_ref: Option<&dyn Fn((u16, u16)) -> bool> = Some(&*scan_filter);
    if let Some(cell) = search_local_resource(
        sim,
        rules,
        overlay_registry,
        master_pos,
        scan_radius,
        filter_ref,
        config,
    ) {
        snap.harvester.target_cell = Some(cell);
        snap.harvester.state = SlaveHarvestState::MoveToOre;
    }
}

// ---------------------------------------------------------------------------
// Slave Miner deploy/undeploy
// ---------------------------------------------------------------------------

/// Configuration for the Slave Miner deploy system, derived from rules.
pub struct SlaveMinerConfig {
    /// Number of slaves to spawn on deploy (SlavesNumber=5).
    pub slaves_number: i32,
    /// Frames between slave respawns after death (SlaveRegenRate=500).
    pub slave_regen_rate: u32,
    /// Minimum frames between consecutive respawns (SlaveReloadRate=25).
    pub slave_reload_rate: u32,
    /// Frames between scan correction checks (SlaveMinerKickFrameDelay=150).
    pub kick_frame_delay: u32,
    /// Cell improvement threshold for scan correction (SlaveMinerScanCorrection=3).
    pub scan_correction: u16,
    /// Short scan radius for deployed slave miner (SlaveMinerShortScan=8).
    pub short_scan: u16,
    /// Slave scan radius — how far slaves search for ore (SlaveMinerSlaveScan=14).
    pub slave_scan: u16,
}

impl SlaveMinerConfig {
    /// Build from parsed GeneralRules.
    pub fn from_rules(rules: &RuleSet) -> Self {
        let g = &rules.general;
        Self {
            slaves_number: 5, // overridden per-object from ObjectType.slaves_number
            slave_regen_rate: g.slave_miner_kick_frame_delay.max(1), // actually SlaveRegenRate per-obj
            slave_reload_rate: 25,
            kick_frame_delay: g.slave_miner_kick_frame_delay.max(1),
            scan_correction: g.slave_miner_scan_correction.max(0) as u16,
            short_scan: g.slave_miner_short_scan.max(1) as u16,
            slave_scan: g.slave_miner_slave_scan.max(1) as u16,
        }
    }
}

/// Deploy a Slave Miner (SMIN) vehicle into its refinery form (YAREFN).
///
/// The newly constructed YAREFN first creates its own slave pool. Native
/// `PowerUp_Cleanup @ 0x006AF580` then destroys that temporary manager and
/// transfers the SMIN's existing pool, retaining every constructor RNG draw.
///
/// Returns the new YAREFN stable_id, or None if deploy failed.
pub fn deploy_slave_miner(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) -> Option<u64> {
    deploy_slave_miner_with_overlay_context(sim, stable_id, rules, None)
}

pub(crate) fn deploy_slave_miner_with_overlay_context(
    sim: &mut Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<u64> {
    // Read deploy data before mutating.
    let deploy_data = {
        let entity = sim.substrate.entities.get(stable_id)?;
        let type_str = sim.interner.resolve(entity.type_ref());
        let obj = rules.object_case_insensitive(type_str)?;
        let target_type: &str = obj.deploys_into.as_deref()?;
        // Verify target exists in rules.
        rules.object(target_type)?;
        let enslaves: String = obj.enslaves.clone()?;
        let slaves_number: i32 = obj.slaves_number.max(0);
        let owner_str = sim.interner.resolve(entity.owner()).to_string();
        Some((
            owner_str,
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            entity.facing,
            entity.selected,
            target_type.to_string(),
            crate::sim::conversion_health::ConversionHealth::capture(
                entity,
                obj,
                rules.object(target_type)?,
                crate::sim::conversion_health::ConversionKind::Unit,
            ),
            enslaves,
            slaves_number,
        ))
    }?;

    let (
        owner,
        rx,
        ry,
        z,
        _facing,
        was_selected,
        target_type,
        converted_health,
        slave_type,
        slaves_number,
    ) = deploy_data;

    // Preserve any existing manager/bindings when this is a redeploy after a
    // retail-style YAREFN -> SMIN reverse conversion.
    let existing_slave_ids = sim.production.slave_bindings.remove(&stable_id);

    // Despawn the SMIN vehicle.
    sim.uninit_with_rules(stable_id, rules);

    // Spawn the YAREFN building at the same cell.
    let new_sid: u64 = sim.spawn_object_at_height_with_overlay_context(
        &target_type,
        &owner,
        rx,
        ry,
        0,
        z,
        rules,
        overlay_registry,
    )?;

    if let Some(ge) = sim.substrate.entities.get_mut(new_sid) {
        converted_health.apply(ge);
        ge.selected = was_selected;
    }

    let slave_capacity: u16 = rules
        .object_case_insensitive(&slave_type)
        .map(|obj| obj.storage.max(1) as u16)
        .unwrap_or(4);

    let slave_ids = if let Some(existing_slave_ids) = existing_slave_ids {
        // The YAREFN constructor has already created and drawn for this pool.
        // Destroy it before installing the old manager, exactly as the native
        // brain-transplant helper does.
        let discarded = sim.discard_constructor_owned_slave_pool(new_sid);
        debug_assert!(discarded, "YAREFN constructor must own a fresh slave pool");

        let mut live_slave_ids = Vec::with_capacity(existing_slave_ids.len());
        for slave_sid in existing_slave_ids {
            if let Some(slave_entity) = sim.substrate.entities.get_mut(slave_sid) {
                slave_entity.slave_harvester = Some(SlaveHarvester::new(new_sid, slave_capacity));
                live_slave_ids.push(slave_sid);
            }
        }
        sim.production
            .slave_bindings
            .insert(new_sid, live_slave_ids.clone());
        live_slave_ids
    } else {
        // A legacy/source object without a represented manager cannot donate
        // one. Keep the manager the target constructor just created.
        sim.production
            .slave_bindings
            .get(&new_sid)
            .cloned()
            .unwrap_or_default()
    };

    // Preserve the existing Rust timing: once the building form exists, wake
    // its retained limbo slaves around it without reconstructing them.
    for (i, slave_sid) in slave_ids
        .into_iter()
        .enumerate()
        .take(slaves_number as usize)
    {
        let in_limbo = sim
            .substrate
            .entities
            .get(slave_sid)
            .is_some_and(|slave| slave.lifecycle.in_limbo);
        if !in_limbo {
            continue;
        }
        let offset_x = (i as i32 % 3) - 1;
        let offset_y = (i as i32 / 3) - 1;
        let sx = (rx as i32 + offset_x).clamp(0, u16::MAX as i32) as u16;
        let sy = (ry as i32 + offset_y).clamp(0, u16::MAX as i32) as u16;
        let _ = sim.unlimbo_held_production_object(
            slave_sid,
            sx,
            sy,
            0,
            z,
            PlacementEvidence::EvaluateMark,
            rules,
        );
    }

    Some(new_sid)
}

/// Undeploy a Slave Miner refinery (YAREFN) back into vehicle form (SMIN).
///
/// 1. Preserve the live slave manager bindings
/// 2. Despawn the YAREFN building
/// 3. Spawn the SMIN vehicle at the same cell
/// 4. Transfer slave bindings to the new SMIN entity and rewrite slave masters
///
/// Returns the new SMIN stable_id, or None if undeploy failed.
pub fn undeploy_slave_miner(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) -> Option<u64> {
    undeploy_slave_miner_with_overlay_context(sim, stable_id, rules, None)
}

pub(crate) fn undeploy_slave_miner_with_overlay_context(
    sim: &mut Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<u64> {
    // Read undeploy data.
    let undeploy_data = {
        let entity = sim.substrate.entities.get(stable_id)?;
        let type_str = sim.interner.resolve(entity.type_ref());
        let obj = rules.object_case_insensitive(type_str)?;
        let target_type: &str = obj.undeploys_into.as_deref()?;
        rules.object(target_type)?;
        let owner_str = sim.interner.resolve(entity.owner()).to_string();
        Some((
            owner_str,
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            entity.selected,
            target_type.to_string(),
            crate::sim::conversion_health::ConversionHealth::capture(
                entity,
                obj,
                rules.object(target_type)?,
                crate::sim::conversion_health::ConversionKind::Building,
            ),
        ))
    }?;

    let (owner, rx, ry, z, was_selected, target_type, converted_health) = undeploy_data;

    let slave_ids = sim.production.slave_bindings.remove(&stable_id);

    // Despawn the YAREFN building.
    sim.uninit_with_rules(stable_id, rules);

    // Spawn the SMIN vehicle.
    let new_sid: u64 = sim.spawn_object_at_height_with_overlay_context(
        &target_type,
        &owner,
        rx,
        ry,
        0,
        z,
        rules,
        overlay_registry,
    )?;

    if let Some(ge) = sim.substrate.entities.get_mut(new_sid) {
        converted_health.apply(ge);
        ge.selected = was_selected;
    }

    if let Some(slave_ids) = slave_ids {
        // The SMIN constructor's own fresh pool and its draws already exist;
        // PowerUp_Cleanup destroys that pool before transferring the YAREFN
        // manager and rewriting every surviving child's master pointer.
        let discarded = sim.discard_constructor_owned_slave_pool(new_sid);
        debug_assert!(discarded, "SMIN constructor must own a fresh slave pool");
        let mut live_slave_ids = Vec::with_capacity(slave_ids.len());
        for slave_id in slave_ids {
            if let Some(slave) = sim.substrate.entities.get_mut(slave_id) {
                if let Some(ref mut harvester) = slave.slave_harvester {
                    harvester.master_id = new_sid;
                }
                live_slave_ids.push(slave_id);
            }
        }
        sim.production
            .slave_bindings
            .insert(new_sid, live_slave_ids);
    }

    Some(new_sid)
}

// ---------------------------------------------------------------------------
// Slave regeneration
// ---------------------------------------------------------------------------

/// Tick slave regeneration for all deployed Slave Miners.
///
/// When a slave dies (removed from entity store), spawn a replacement after
/// `SlaveRegenRate` ticks. `SlaveReloadRate` is the minimum gap between spawns.
pub(super) fn tick_slave_regen(
    sim: &mut Simulation,
    live_order: &[u64],
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    // Missing/dead masters are lifecycle cleanup, not AI visitation. Remove
    // those bindings even though the master is no longer in LogicVector.
    let stale_master_ids = sim
        .production
        .slave_bindings
        .keys()
        .copied()
        .filter(|&master_id| {
            !sim.substrate
                .entities
                .get(master_id)
                .is_some_and(|master| master.lifecycle.object_alive)
        })
        .collect::<Vec<_>>();
    for master_id in stale_master_ids {
        sim.production.slave_bindings.remove(&master_id);
    }

    // Regeneration is master AI work and therefore follows authoritative
    // LogicVector order. An explicit empty vector performs no regeneration.
    let master_ids = live_order
        .iter()
        .copied()
        .filter(|master_id| sim.production.slave_bindings.contains_key(master_id))
        .collect::<Vec<_>>();

    for master_id in master_ids {
        let Some(master) = sim.substrate.entities.get(master_id) else {
            // Master died — clean up bindings.
            sim.production.slave_bindings.remove(&master_id);
            continue;
        };

        let master_type = sim.interner.resolve(master.type_ref()).to_string();
        let owner = sim.interner.resolve(master.owner()).to_string();
        let mrx: u16 = master.position.rx;
        let mry: u16 = master.position.ry;
        let mz: u8 = master.position.z;

        // Get expected slave count and slave type from rules.
        let (slave_type, max_slaves) = {
            let obj = match rules.object_case_insensitive(&master_type) {
                Some(o) => o,
                None => continue,
            };
            let st = match obj.enslaves.as_deref() {
                Some(s) => s.to_string(),
                None => continue,
            };
            (st, obj.slaves_number.max(0))
        };

        // Count living slaves.
        let slave_ids = match sim.production.slave_bindings.get(&master_id) {
            Some(ids) => ids.clone(),
            None => continue,
        };
        let alive_count: i32 = slave_ids
            .iter()
            .filter(|&&sid| sim.substrate.entities.get(sid).is_some())
            .count() as i32;

        // Remove dead slave IDs from bindings.
        let alive_ids: Vec<u64> = slave_ids
            .iter()
            .copied()
            .filter(|&sid| sim.substrate.entities.get(sid).is_some())
            .collect();

        if alive_count < max_slaves {
            // Spawn one replacement per tick (SlaveReloadRate could throttle this,
            // but for simplicity we spawn one per tick until full).
            let sx: u16 = mrx.saturating_add(1);
            let sy: u16 = mry.saturating_add(1);

            if let Some(slave_sid) = sim.spawn_object_at_height_with_overlay_context(
                &slave_type,
                &owner,
                sx,
                sy,
                0,
                mz,
                rules,
                overlay_registry,
            ) {
                let slave_capacity: u16 = rules
                    .object_case_insensitive(&slave_type)
                    .map(|obj| obj.storage.max(1) as u16)
                    .unwrap_or(4);

                if let Some(slave_entity) = sim.substrate.entities.get_mut(slave_sid) {
                    slave_entity.slave_harvester =
                        Some(SlaveHarvester::new(master_id, slave_capacity));
                }
                let mut updated_ids: Vec<u64> = alive_ids;
                updated_ids.push(slave_sid);
                sim.production.slave_bindings.insert(master_id, updated_ids);
            } else {
                sim.production.slave_bindings.insert(master_id, alive_ids);
            }
        } else if alive_ids.len() != slave_ids.len() {
            // Just clean up dead IDs.
            sim.production.slave_bindings.insert(master_id, alive_ids);
        }
    }
}

// ---------------------------------------------------------------------------
// Scan correction (Phase 7)
// ---------------------------------------------------------------------------

/// Check if a deployed Slave Miner should reposition to a closer ore patch.
///
/// Called periodically (every SlaveMinerKickFrameDelay ticks). If the nearest
/// ore from the master's position is `SlaveMinerScanCorrection` cells closer
/// than the current nearest ore to the slaves, trigger an undeploy + move.
///
/// Returns Some((rx, ry)) = cell to reposition to, None = stay put.
pub fn check_scan_correction(
    sim: &Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    master_id: u64,
) -> Option<(u16, u16)> {
    let master = sim.substrate.entities.get(master_id)?;
    let mrx: u16 = master.position.rx;
    let mry: u16 = master.position.ry;

    let short_scan: u16 = rules.general.slave_miner_short_scan.max(1) as u16;
    let correction: u16 = rules.general.slave_miner_scan_correction.max(0) as u16;
    let cfg = MinerConfig::from_rules(rules);

    let scan_filter = build_slave_scan_filter(sim, path_grid, master_id);
    let filter_ref: Option<&dyn Fn((u16, u16)) -> bool> = Some(&*scan_filter);

    // Find nearest ore from current position.
    let current_nearest = search_local_resource(
        sim,
        rules,
        overlay_registry,
        (mrx, mry),
        short_scan,
        filter_ref,
        &cfg,
    )?;

    let current_dist: u16 = manhattan_distance(mrx, mry, current_nearest.0, current_nearest.1);

    // Search the broader area (SlaveMinerLongScan) for a better patch.
    let long_scan: u16 = rules.general.slave_miner_long_scan.max(1) as u16;
    let better_ore = search_local_resource(
        sim,
        rules,
        overlay_registry,
        (mrx, mry),
        long_scan,
        filter_ref,
        &cfg,
    )?;

    let better_dist: u16 = manhattan_distance(mrx, mry, better_ore.0, better_ore.1);

    // If the improvement exceeds SlaveMinerScanCorrection, recommend repositioning.
    if current_dist > better_dist && (current_dist - better_dist) >= correction {
        Some(better_ore)
    } else {
        None
    }
}

/// Manhattan distance between two cells.
fn manhattan_distance(ax: u16, ay: u16, bx: u16, by: u16) -> u16 {
    ax.abs_diff(bx) + ay.abs_diff(by)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::miner::ResourceType;
    use crate::sim::rng::SimRng;

    #[test]
    fn slave_harvester_capacity_and_value() {
        let mut sh = SlaveHarvester::new(100, 4);
        assert!(!sh.is_full());
        assert_eq!(sh.cargo_value(), 0);

        for _ in 0..4 {
            sh.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        assert!(sh.is_full());
        assert_eq!(sh.cargo_value(), 100);
    }

    #[test]
    fn slave_harvester_state_transitions() {
        let sh = SlaveHarvester::new(1, 4);
        assert_eq!(sh.state, SlaveHarvestState::SearchOre);
    }

    #[test]
    fn undeploy_transfers_slave_manager_to_new_smin() {
        let rules = make_test_rules();
        let seed = 0x51A7_E001;
        let mut sim = Simulation::with_seed(seed);
        let mut expected = SimRng::new(seed);
        let sm_bld = sim
            .spawn_object_at_height("SMIN", "YuriCountry", 10, 10, 0, 0, &rules)
            .expect("spawn SMIN");
        for _ in 0..6 {
            let _ = expected.next_u32();
        }
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        let yarefn = deploy_slave_miner(&mut sim, sm_bld, &rules).expect("deploy to YAREFN");
        for _ in 0..6 {
            let _ = expected.next_u32();
        }
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        let slave_ids = sim
            .production
            .slave_bindings
            .get(&yarefn)
            .cloned()
            .expect("YAREFN should own slave manager");
        assert_eq!(slave_ids, vec![2, 3, 4, 5, 6]);
        for discarded_id in 8..=12 {
            assert!(sim.substrate.entities.get(discarded_id).is_none());
        }

        let smin = undeploy_slave_miner(&mut sim, yarefn, &rules).expect("undeploy to SMIN");
        for _ in 0..6 {
            let _ = expected.next_u32();
        }
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        assert_eq!(smin, 13);
        for discarded_id in 14..=18 {
            assert!(sim.substrate.entities.get(discarded_id).is_none());
        }
        assert!(sim.production.slave_bindings.get(&yarefn).is_none());
        assert_eq!(sim.production.slave_bindings.get(&smin), Some(&slave_ids));
        for slave_id in slave_ids {
            let slave = sim
                .substrate
                .entities
                .get(slave_id)
                .expect("slave remains live");
            assert_eq!(slave.slave_harvester.as_ref().unwrap().master_id, smin);
        }
    }

    /// GSI-09.01 §2.6: `BuildingClass__DepositOreFromStorage @ 0x00522D50`
    /// drains the slave's whole slot in one visit with ONE truncation of the
    /// purifier bonus. Medium-AI Yuri (`AIVirtualPurifiers[1] = 2`, bonus
    /// `.25`) returning 4 ore bales: base 100 + bonus `trunc(2 × 0.25 × 100)
    /// = 50` in a single call (the per-bale path paid `4 × trunc(12.5) = 48`
    /// over four ticks).
    #[test]
    fn slave_deposit_credits_the_whole_slot_in_one_call() {
        use crate::sim::house_state::HouseState;
        let rules = {
            use crate::rules::ini_parser::IniFile;
            let ini_str: &str = "\
[InfantryTypes]\n1=SLAV\n\
[VehicleTypes]\n1=SMIN\n\
[BuildingTypes]\n1=YAREFN\n\
[SLAV]\nStrength=125\nSpeed=3\nSlaved=yes\nStorage=4\nHarvestRate=150\n\
[SMIN]\nStrength=2000\nSpeed=3\nEnslaves=SLAV\nSlavesNumber=5\nDeploysInto=YAREFN\nResourceGatherer=yes\nResourceDestination=yes\n\
[YAREFN]\nStrength=2000\nEnslaves=SLAV\nSlavesNumber=5\nUndeploysInto=SMIN\nFoundation=3x3\n\
[General]\nPurifierBonus=.25\nAIVirtualPurifiers=4,2,0\n\
";
            RuleSet::from_ini(&IniFile::from_str(ini_str)).expect("rules parse")
        };
        let mut sim = Simulation::with_seed(0x51A7_E002);
        let yuri = sim.interner.intern("YuriCountry");
        let mut house = HouseState::new(yuri, 2, None, false, 0, 10);
        house.difficulty = crate::sim::house_state::HouseDifficulty::Normal;
        sim.houses.insert(yuri, house);
        let heights = std::collections::BTreeMap::new();
        let master = sim
            .spawn_object("YAREFN", "YuriCountry", 10, 10, 0, &rules, &heights)
            .expect("YAREFN spawns");
        let slave = sim
            .spawn_object("SLAV", "YuriCountry", 12, 12, 0, &rules, &heights)
            .expect("SLAV spawns");
        let mut harvester = SlaveHarvester::new(master, 4);
        for _ in 0..4 {
            harvester.cargo.push(CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            });
        }
        harvester.state = SlaveHarvestState::Deposit;
        let mut snap = SlaveSnapshot {
            entity_id: slave,
            owner: yuri,
            rx: 12,
            ry: 12,
            harvester,
        };
        let config = MinerConfig::default();

        handle_slave_deposit(&mut sim, &rules, &config, &mut snap);

        assert_eq!(
            sim.houses[&yuri].economy.credits, 150,
            "100 base + 50 bonus, one call"
        );
        assert!(
            snap.harvester.cargo.is_empty(),
            "the whole slot drained at once"
        );
        assert_eq!(
            sim.houses[&yuri].economy.harvested_credits,
            20 + 10,
            "score term: 4 bales × 5 + trunc(2 × 0.25 × 4 × 5)"
        );
        // A second visit with nothing aboard pays nothing and re-scans.
        handle_slave_deposit(&mut sim, &rules, &config, &mut snap);
        assert_eq!(sim.houses[&yuri].economy.credits, 150);
        assert_eq!(snap.harvester.state, SlaveHarvestState::SearchOre);
    }

    #[test]
    fn manhattan_distance_basic() {
        assert_eq!(manhattan_distance(10, 10, 13, 14), 7);
        assert_eq!(manhattan_distance(5, 5, 5, 5), 0);
        assert_eq!(manhattan_distance(0, 0, 100, 50), 150);
    }

    #[test]
    fn scan_correction_returns_none_without_entities() {
        // With no entities, check_scan_correction returns None (master not found).
        let sim = Simulation::new();
        let rules = make_test_rules();
        assert!(check_scan_correction(&sim, &rules, None, None, 999).is_none());
    }

    /// Minimal rules for slave miner tests.
    fn make_test_rules() -> RuleSet {
        use crate::rules::ini_parser::IniFile;
        let ini_str: &str = "\
[InfantryTypes]\n1=SLAV\n\
[VehicleTypes]\n1=SMIN\n\
[BuildingTypes]\n1=YAREFN\n\
[SLAV]\nStrength=125\nSpeed=3\nSlaved=yes\nStorage=4\nHarvestRate=150\n\
[SMIN]\nStrength=2000\nSpeed=3\nEnslaves=SLAV\nSlavesNumber=5\nDeploysInto=YAREFN\nResourceGatherer=yes\nResourceDestination=yes\n\
[YAREFN]\nStrength=2000\nEnslaves=SLAV\nSlavesNumber=5\nUndeploysInto=SMIN\nFoundation=3x3\n\
[General]\nSlaveMinerShortScan=8\nSlaveMinerSlaveScan=14\nSlaveMinerLongScan=48\nSlaveMinerScanCorrection=3\nSlaveMinerKickFrameDelay=150\n\
";
        let ini: IniFile = IniFile::from_str(ini_str);
        RuleSet::from_ini(&ini).expect("test rules should parse")
    }
}
