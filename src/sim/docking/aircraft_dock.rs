//! Aircraft ammo tracking and airfield docking system.
//!
//! Aircraft with finite `Ammo=` (from rules.ini) deplete ammo on each weapon
//! fire. When ammo reaches 0, the aircraft auto-returns to the nearest
//! helipad/airfield owned by the same player, descends onto its assigned
//! pad cell, reloads, and re-launches.
//!
//! Multi-pad airfields (GAAIRC, AMRADR: `NumberOfDocks=4`) allocate pad
//! indices via [`AirfieldDocks::try_reserve`] (first-empty-slot scan).
//! Aircraft descends to the per-pad cell computed by
//! [`crate::sim::docking::pad_geometry::pad_cell_for`].
//!
//! Two FSMs coexist:
//! - `tick_aircraft_docks` (this module) handles aircraft *without* an
//!   `AircraftMission` (legacy ammo state machine).
//! - `tick_aircraft_missions` ([`crate::sim::aircraft`]) handles aircraft
//!   *with* an `AircraftMission::Docking`/`DockedIdle`.
//!
//! Both call into `AirfieldDocks` for pad allocation.
//!
//! Uses the two-phase snapshot pattern from `building_dock.rs` and follows
//! the own-house refinery ranking in `miner_system.rs` (`find_docking_bay`).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/components, sim/air_movement,
//!   sim/docking/pad_geometry.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::AirMovePhase;
use crate::sim::world::Simulation;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Per-entity aircraft ammo and docking state.
///
/// Present only on aircraft with `Ammo= >= 0` in rules.ini.
/// Entities with `Ammo=-1` (unlimited, the default) have `None`.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub struct AircraftAmmo {
    /// Current ammo count. 0 = depleted, triggers auto-return.
    pub current: i32,
    /// Maximum ammo (from `Ammo=` in rules.ini).
    pub max: i32,
    /// Current docking/reload lifecycle phase. None = normal flight.
    pub dock_phase: Option<AircraftDockPhase>,
    /// Stable ID of the target helipad/airfield building.
    pub target_airfield: Option<u64>,
    /// Pad index assigned by `AirfieldDocks::try_reserve` for the current
    /// dock attempt. Set when transitioning from WaitForDock → Descending;
    /// cleared on launch / no-airfield-available.
    #[serde(default)]
    pub target_pad: Option<u32>,
    /// Ticks remaining until the next ammo point is restored.
    pub reload_timer: u32,
    /// Cooldown ticks before re-scanning for a helipad (prevents per-tick scans).
    pub rescan_cooldown: u16,
}

impl AircraftAmmo {
    /// Create a new ammo tracker with full ammo.
    pub fn new(max_ammo: i32) -> Self {
        Self {
            current: max_ammo,
            max: max_ammo,
            dock_phase: None,
            target_airfield: None,
            target_pad: None,
            reload_timer: 0,
            rescan_cooldown: 0,
        }
    }
}

/// Docking lifecycle phases for aircraft returning to an airfield.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AircraftDockPhase {
    /// Flying toward the target airfield.
    ReturnToBase,
    /// At/near the airfield, re-probing each tick for a free dock slot
    /// (no wait queue — see [`AirfieldDocks`]).
    WaitForDock,
    /// Dock slot reserved, descending to land.
    Descending,
    /// On the ground, reloading ammo.
    Reloading,
    /// Fully reloaded, ascending to resume flight.
    Launching,
}

// ---------------------------------------------------------------------------
// Multi-slot dock reservations for airfields
// ---------------------------------------------------------------------------

/// Pad-aware multi-slot dock reservation manager for airfields.
///
/// Each airfield has `NumberOfDocks` pads. `slots[airfield]` is a
/// `Vec<Option<u64>>` indexed by pad index (vec length = NumberOfDocks).
/// First-empty-slot allocation mirrors the original engine: arrivals receive
/// pads 0, 1, 2, ... in scan order.
///
/// There is **no wait queue**. The original engine stores no ordered wait-list
/// for dock admission: a saturated airfield admits nobody, and each waiting
/// aircraft independently re-probes (`WaitForDock` re-calls [`Self::try_reserve`]
/// every tick). Whichever probe hits a free slot first wins — emergent
/// timing/iteration order, NOT a FIFO and NOT a distance sort. Releasing a pad
/// simply empties it; the next probe claims it.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AirfieldDocks {
    /// Per-airfield occupancy: pad_index → occupant aircraft (None = empty).
    /// Vec length equals `NumberOfDocks` for the airfield.
    slots: BTreeMap<u64, Vec<Option<u64>>>,
    /// Derived lookup rebuilt from saved slots: aircraft → (airfield, pad_index).
    #[serde(skip)]
    aircraft_to_pad: BTreeMap<u64, (u64, u32)>,
}

impl<'de> serde::Deserialize<'de> for AirfieldDocks {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct SavedReservations {
            slots: BTreeMap<u64, Vec<Option<u64>>>,
        }
        let saved = <SavedReservations as serde::Deserialize>::deserialize(deserializer)?;
        let mut aircraft_to_pad = BTreeMap::new();
        for (&airfield, pads) in &saved.slots {
            for (index, &aircraft) in pads.iter().enumerate() {
                let Some(aircraft) = aircraft else { continue };
                let index = u32::try_from(index).map_err(serde::de::Error::custom)?;
                if let Some(previous) = aircraft_to_pad.insert(aircraft, (airfield, index)) {
                    return Err(serde::de::Error::custom(format!(
                        "aircraft {aircraft} reserves both {previous:?} and ({airfield}, {index})"
                    )));
                }
            }
        }
        Ok(Self {
            slots: saved.slots,
            aircraft_to_pad,
        })
    }
}

impl AirfieldDocks {
    /// Whether there are no registered airfields (including empty registrations).
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The reverse lookup is derived; only canonical slot state enters the hash.
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if !self.slots.is_empty() {
            b"airfield-reservations-v170".hash(hasher);
            self.slots.hash(hasher);
        }
    }

    /// Register an airfield with its pad count.
    /// Called lazily when an aircraft first tries to dock. Idempotent.
    fn ensure_registered(&mut self, airfield_sid: u64, num_pads: u32) {
        self.slots
            .entry(airfield_sid)
            .or_insert_with(|| vec![None; num_pads as usize]);
    }

    /// Try to reserve a pad slot for `aircraft_sid` at `airfield_sid`.
    ///
    /// Returns `Some(pad_index)` if a pad was assigned, `None` if all pads are
    /// full. There is no queue: a refused aircraft stays in `WaitForDock` and
    /// re-probes next tick (see [`AirfieldDocks`]).
    ///
    /// First-empty-slot policy: scans pad indices left-to-right and returns
    /// the first index whose slot is `None`.
    pub fn try_reserve(
        &mut self,
        airfield_sid: u64,
        aircraft_sid: u64,
        num_pads: u32,
    ) -> Option<u32> {
        self.ensure_registered(airfield_sid, num_pads);

        // Already docked here? Return existing pad index (idempotent).
        if let Some((af, pad)) = self.aircraft_to_pad.get(&aircraft_sid)
            && *af == airfield_sid
        {
            return Some(*pad);
        }

        // A refused transfer retains the existing reservation. Once a target
        // slot is available, retire the old slot before publishing the new one.
        let index = self
            .slots
            .get(&airfield_sid)
            .expect("registered above")
            .iter()
            .position(Option::is_none)?;
        self.release(aircraft_sid);
        self.slots.get_mut(&airfield_sid).expect("registered above")[index] = Some(aircraft_sid);
        let pad = index as u32;
        self.aircraft_to_pad
            .insert(aircraft_sid, (airfield_sid, pad));
        Some(pad)
    }

    /// Release the aircraft's pad — the slot simply becomes empty. No waiter is
    /// promoted (the original engine sends no release-time notification; the
    /// next probe claims the slot).
    pub fn release(&mut self, aircraft_sid: u64) {
        let Some((airfield_sid, pad_index)) = self.aircraft_to_pad.remove(&aircraft_sid) else {
            return;
        };
        if let Some(pads) = self.slots.get_mut(&airfield_sid)
            && let Some(slot) = pads.get_mut(pad_index as usize)
        {
            *slot = None;
        }
    }

    /// Check if an airfield has at least one free pad. Read-only probe.
    pub fn has_free_slot(&self, airfield_sid: u64, num_pads: u32) -> bool {
        match self.slots.get(&airfield_sid) {
            Some(pads) => pads.iter().any(|s| s.is_none()),
            None => num_pads > 0, // Not yet registered = all pads free.
        }
    }

    /// Look up which (airfield, pad_index) this aircraft is parked on.
    #[cfg(test)]
    pub fn pad_for(&self, aircraft_sid: u64) -> Option<(u64, u32)> {
        self.aircraft_to_pad.get(&aircraft_sid).copied()
    }

    /// Remove dead airfields and occupants while maintaining the derived index.
    /// This performs no cache rebuild/allocation in the per-tick cleanup path.
    pub fn cleanup_dead(&mut self, alive: &BTreeSet<u64>) {
        let reverse = &mut self.aircraft_to_pad;
        self.slots.retain(|airfield, pads| {
            let keep_airfield = alive.contains(airfield);
            for slot in pads {
                if let Some(aircraft) = *slot {
                    if !keep_airfield || !alive.contains(&aircraft) {
                        reverse.remove(&aircraft);
                        *slot = None;
                    }
                }
            }
            keep_airfield
        });
    }
}

// ---------------------------------------------------------------------------
// Find nearest airfield
// ---------------------------------------------------------------------------

/// Chebyshev distance squared between two cell coordinates.
fn cell_dist_sq(ax: u16, ay: u16, bx: u16, by: u16) -> u32 {
    let dx = (ax as i32 - bx as i32).unsigned_abs();
    let dy = (ay as i32 - by as i32).unsigned_abs();
    dx * dx + dy * dy
}

/// Find the nearest same-owner airfield/helipad that accepts this aircraft type.
///
/// Checks: alive, same owner, `UnitReload=yes` or `Helipad=yes`, and the
/// aircraft's `Dock=` list includes the building's type_ref.
/// Returns `(stable_id, dock_cell_rx, dock_cell_ry)`.
fn find_nearest_airfield(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    aircraft_type_id: InternedId,
    from: (u16, u16),
) -> Option<(u64, u16, u16)> {
    let aircraft_obj = rules.object(sim.interner.resolve(aircraft_type_id))?;
    let dock_list = &aircraft_obj.dock;
    if dock_list.is_empty() {
        return None;
    }

    let mut best: Option<(u64, u16, u16, u32)> = None;

    for entity in sim.substrate.entities.values() {
        if entity.category != EntityCategory::Structure {
            continue;
        }
        if entity.health.current == 0 || entity.dying || entity.lifecycle.in_limbo {
            continue;
        }
        if entity.owner() != owner {
            continue;
        }
        let entity_type_str = sim.interner.resolve(entity.type_ref());
        let Some(obj) = rules.object(entity_type_str) else {
            continue;
        };
        if !obj.unit_reload && !obj.helipad {
            continue;
        }
        // Aircraft's Dock= list must include this building's type_ref.
        if !dock_list
            .iter()
            .any(|d| d.eq_ignore_ascii_case(entity_type_str))
        {
            continue;
        }
        // Use building center as dock cell.
        let (w, h) = crate::sim::production::foundation_dimensions(&obj.foundation);
        let dock_rx = entity.position.rx + w / 2;
        let dock_ry = entity.position.ry + h / 2;
        let dist = cell_dist_sq(from.0, from.1, dock_rx, dock_ry);

        if best.is_none() || dist < best.unwrap().3 {
            best = Some((entity.stable_id(), dock_rx, dock_ry, dist));
        }
    }

    best.map(|(sid, rx, ry, _)| (sid, rx, ry))
}

// ---------------------------------------------------------------------------
// Tick system
// ---------------------------------------------------------------------------

/// Chebyshev distance between two cell coordinates.
fn cell_distance(ax: u16, ay: u16, bx: u16, by: u16) -> u32 {
    let dx = (ax as i32 - bx as i32).unsigned_abs();
    let dy = (ay as i32 - by as i32).unsigned_abs();
    dx.max(dy)
}

/// Rescan cooldown in ticks before retrying helipad search after failure.
const RESCAN_COOLDOWN_TICKS: u16 = 60;

/// Advance aircraft ammo and docking systems for all entities.
///
/// Called once per tick from `advance_tick()`, after combat and building docks.
pub fn tick_aircraft_docks(sim: &mut Simulation, rules: &RuleSet) {
    // Phase 1: Cleanup dead/dying entities from dock reservations. Dying
    // entities remain in `sim.entities` through the death animation, but their
    // airfield-pad reservation must release immediately so other aircraft can
    // dock without waiting through the death anim (matches building_dock/miner).
    let alive: BTreeSet<u64> = sim
        .substrate
        .entities
        .values()
        .filter(|e| !e.dying)
        .map(|e| e.stable_id())
        .collect();
    sim.production.airfield_docks.cleanup_dead(&alive);

    // Phase 2: Snapshot all aircraft with aircraft_ammo.
    struct AircraftSnap {
        id: u64,
        owner: InternedId,
        type_ref: InternedId,
        rx: u16,
        ry: u16,
        current_ammo: i32,
        max_ammo: i32,
        dock_phase: Option<AircraftDockPhase>,
        target_airfield: Option<u64>,
        reload_timer: u32,
        rescan_cooldown: u16,
        has_attack_target: bool,
        air_phase: Option<AirMovePhase>,
        has_movement_target: bool,
    }

    let snapshots: Vec<AircraftSnap> = sim
        .substrate
        .entities
        .values()
        .filter_map(|e| {
            // A Dying aircraft corpse must not run the dock/ammo state machine
            // (auto-return moves, reload, pad reservation) before the drain.
            if e.dying {
                return None;
            }
            let ammo = e.aircraft_ammo.as_ref()?;
            // Skip aircraft managed by the mission system.
            if e.aircraft_mission.is_some() {
                return None;
            }
            let air_phase = crate::sim::movement::air_movement::fly_mission_phase(
                e,
                sim.resolved_terrain.as_ref(),
            );
            Some(AircraftSnap {
                id: e.stable_id(),
                owner: e.owner(),
                type_ref: e.type_ref(),
                rx: e.position.rx,
                ry: e.position.ry,
                current_ammo: ammo.current,
                max_ammo: ammo.max,
                dock_phase: ammo.dock_phase,
                target_airfield: ammo.target_airfield,
                reload_timer: ammo.reload_timer,
                rescan_cooldown: ammo.rescan_cooldown,
                has_attack_target: e.attack_target.is_some(),
                air_phase,
                has_movement_target: e.movement_target.is_some(),
            })
        })
        .collect();

    if snapshots.is_empty() {
        return;
    }

    // Phase 3: Process each aircraft through the ammo/dock state machine.
    struct AircraftMutation {
        id: u64,
        new_dock_phase: Option<Option<AircraftDockPhase>>, // Some(None) = clear
        new_target_airfield: Option<Option<u64>>,
        /// Some(Some(pad)) = set pad_index; Some(None) = clear; None = leave alone.
        new_target_pad: Option<Option<u32>>,
        new_reload_timer: Option<u32>,
        new_rescan_cooldown: Option<u16>,
        restore_ammo: i32,
        clear_attack_target: bool,
        set_air_phase: Option<AirMovePhase>,
        air_move_to: Option<(u16, u16)>,
        clear_movement: bool,
    }

    let reload_ticks = rules.general.reload_rate_ticks;
    let mut mutations: Vec<AircraftMutation> = Vec::new();

    for snap in &snapshots {
        let mut m = AircraftMutation {
            id: snap.id,
            new_dock_phase: None,
            new_target_airfield: None,
            new_target_pad: None,
            new_reload_timer: None,
            new_rescan_cooldown: None,
            restore_ammo: 0,
            clear_attack_target: false,
            set_air_phase: None,
            air_move_to: None,
            clear_movement: false,
        };

        match snap.dock_phase {
            None => {
                // Trigger auto-return when ammo depleted and not attacking.
                if snap.current_ammo <= 0 && !snap.has_attack_target {
                    if snap.rescan_cooldown > 0 {
                        m.new_rescan_cooldown = Some(snap.rescan_cooldown.saturating_sub(1));
                        mutations.push(m);
                        continue;
                    }
                    if let Some((af_sid, af_rx, af_ry)) = find_nearest_airfield(
                        sim,
                        rules,
                        snap.owner,
                        snap.type_ref,
                        (snap.rx, snap.ry),
                    ) {
                        m.new_dock_phase = Some(Some(AircraftDockPhase::ReturnToBase));
                        m.new_target_airfield = Some(Some(af_sid));
                        m.clear_attack_target = true;
                        m.air_move_to = Some((af_rx, af_ry));
                    } else {
                        // No airfield available — set cooldown and retry later.
                        m.new_rescan_cooldown = Some(RESCAN_COOLDOWN_TICKS);
                    }
                }
            }

            Some(AircraftDockPhase::ReturnToBase) => {
                // Verify target airfield still exists.
                let airfield_ok = snap.target_airfield.and_then(|af_sid| {
                    let af = sim.substrate.entities.get(af_sid)?;
                    if af.health.current == 0 || af.dying {
                        return None;
                    }
                    let obj = sim.object_type(af.type_ref(), rules)?;
                    let (w, h) = crate::sim::production::foundation_dimensions(&obj.foundation);
                    Some((af_sid, af.position.rx + w / 2, af.position.ry + h / 2))
                });

                match airfield_ok {
                    Some((_af_sid, dock_rx, dock_ry)) => {
                        let dist = cell_distance(snap.rx, snap.ry, dock_rx, dock_ry);
                        if dist <= 2 {
                            m.new_dock_phase = Some(Some(AircraftDockPhase::WaitForDock));
                            m.clear_movement = true;
                        } else if !snap.has_movement_target {
                            // Re-issue move if we lost movement target.
                            m.air_move_to = Some((dock_rx, dock_ry));
                        }
                    }
                    None => {
                        // Airfield destroyed — find another.
                        sim.production.airfield_docks.release(snap.id);
                        if let Some((af_sid, af_rx, af_ry)) = find_nearest_airfield(
                            sim,
                            rules,
                            snap.owner,
                            snap.type_ref,
                            (snap.rx, snap.ry),
                        ) {
                            m.new_target_airfield = Some(Some(af_sid));
                            m.air_move_to = Some((af_rx, af_ry));
                        } else {
                            // No airfield — hover and rescan later.
                            m.new_dock_phase = Some(None);
                            m.new_target_airfield = Some(None);
                            m.new_target_pad = Some(None);
                            m.new_rescan_cooldown = Some(RESCAN_COOLDOWN_TICKS);
                        }
                    }
                }
            }

            Some(AircraftDockPhase::WaitForDock) => {
                let Some(af_sid) = snap.target_airfield else {
                    m.new_dock_phase = Some(None);
                    m.new_target_pad = Some(None);
                    mutations.push(m);
                    continue;
                };
                let max_slots = sim
                    .substrate
                    .entities
                    .get(af_sid)
                    .and_then(|af| sim.object_type(af.type_ref(), rules))
                    .map(|obj| obj.dock_contact_capacity())
                    .unwrap_or(1);

                if let Some(pad_index) = sim
                    .production
                    .airfield_docks
                    .try_reserve(af_sid, snap.id, max_slots)
                {
                    m.new_dock_phase = Some(Some(AircraftDockPhase::Descending));
                    m.set_air_phase = Some(AirMovePhase::Descending);
                    m.clear_movement = true;
                    m.new_target_pad = Some(Some(pad_index));

                    // Re-target descent toward the assigned per-pad cell so
                    // multi-pad airfields visibly spread occupants. Falls back
                    // to whatever was previously targeted (building center)
                    // when the building has no DockingOffset%d in art.
                    if let Some((px, py)) = sim.substrate.entities.get(af_sid).and_then(|af| {
                        let obj = sim.object_type(af.type_ref(), rules)?;
                        let foundation =
                            crate::sim::production::foundation_dimensions(&obj.foundation);
                        obj.pads.get(pad_index as usize).map(|pad| {
                            crate::sim::docking::pad_geometry::pad_cell_for(
                                (af.position.rx, af.position.ry),
                                foundation,
                                pad,
                            )
                        })
                    }) {
                        m.air_move_to = Some((px, py));
                    }
                }
                // Otherwise keep waiting (pad busy, aircraft remains in WaitForDock).
            }

            Some(AircraftDockPhase::Descending) => {
                // Wait for air_movement to bring altitude to 0 (Landed).
                if snap.air_phase == Some(AirMovePhase::Landed) {
                    m.new_dock_phase = Some(Some(AircraftDockPhase::Reloading));
                    m.new_reload_timer = Some(reload_ticks);
                }
            }

            Some(AircraftDockPhase::Reloading) => {
                let timer = snap.reload_timer.saturating_sub(1);
                if timer == 0 {
                    // Restore one ammo point.
                    m.restore_ammo = 1;
                    let new_ammo = snap.current_ammo + 1;
                    if new_ammo >= snap.max_ammo {
                        // Fully reloaded — launch.
                        m.new_dock_phase = Some(Some(AircraftDockPhase::Launching));
                        m.set_air_phase = Some(AirMovePhase::Ascending);
                        m.new_target_pad = Some(None); // pad released
                        // Release dock slot.
                        sim.production.airfield_docks.release(snap.id);
                    } else {
                        m.new_reload_timer = Some(reload_ticks);
                    }
                } else {
                    m.new_reload_timer = Some(timer);
                }
            }

            Some(AircraftDockPhase::Launching) => {
                // Wait for air_movement to reach cruising altitude.
                if snap.air_phase == Some(AirMovePhase::Cruising) {
                    m.new_dock_phase = Some(None);
                    m.new_target_airfield = Some(None);
                }
            }
        }

        mutations.push(m);
    }

    // Phase 4: Apply mutations.
    for m in &mutations {
        if let Some(entity) = sim.substrate.entities.get_mut(m.id) {
            if let Some(ref mut ammo) = entity.aircraft_ammo {
                if let Some(new_phase) = m.new_dock_phase {
                    ammo.dock_phase = new_phase;
                }
                if let Some(new_af) = m.new_target_airfield {
                    ammo.target_airfield = new_af;
                }
                if let Some(new_pad) = m.new_target_pad {
                    ammo.target_pad = new_pad;
                }
                if let Some(new_timer) = m.new_reload_timer {
                    ammo.reload_timer = new_timer;
                }
                if let Some(new_cooldown) = m.new_rescan_cooldown {
                    ammo.rescan_cooldown = new_cooldown;
                }
                ammo.current = (ammo.current + m.restore_ammo).min(ammo.max);
            }
            if m.clear_attack_target {
                entity.attack_target = None;
            }
            if let Some(phase) = m.set_air_phase {
                let level = rules
                    .object(sim.interner.resolve(entity.type_ref()))
                    .map_or(rules.general.flight_level, |o| {
                        o.flight_level(rules.general.flight_level)
                    });
                if let Some(loco) = entity.locomotor.as_mut() {
                    match phase {
                        AirMovePhase::Ascending => loco.begin_fly_takeoff(level),
                        AirMovePhase::Descending => loco.begin_fly_landing(),
                        _ => unreachable!("docking only requests takeoff or landing"),
                    }
                }
            }
            if m.clear_movement {
                entity.movement_target = None;
            }
        }
    }

    // Phase 5: Issue air move commands (must be done after mutations to avoid
    // borrow conflicts with entities).
    let air_moves: Vec<(u64, u16, u16)> = mutations
        .iter()
        .filter_map(|m| m.air_move_to.map(|(rx, ry)| (m.id, rx, ry)))
        .collect();
    for (id, rx, ry) in air_moves {
        // No FASTER stage: the fly locomotor never calls the
        // `FootClass::GetCurrentSpeed` slot (`veterancy::locomotor_consults_current_speed`).
        let speed = sim
            .substrate
            .entities
            .get(id)
            .and_then(|e| {
                let obj = sim.object_type(e.type_ref(), rules)?;
                Some(crate::util::fixed_math::ra2_speed_to_leptons_per_second(
                    obj.speed.max(1),
                ))
            })
            .unwrap_or(crate::util::fixed_math::SimFixed::from_num(8));
        sim.issue_air_cell_destination(id, (rx, ry), speed, Some(rules));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_transfer_is_atomic_and_releases_the_old_slot() {
        let mut docks = AirfieldDocks::default();
        assert_eq!(docks.try_reserve(100, 1, 1), Some(0));
        assert_eq!(docks.try_reserve(200, 2, 1), Some(0));
        assert_eq!(docks.try_reserve(200, 1, 1), None);
        assert_eq!(docks.pad_for(1), Some((100, 0)));
        assert!(!docks.has_free_slot(100, 1));
        docks.release(2);
        assert_eq!(docks.try_reserve(200, 1, 1), Some(0));
        assert_eq!(docks.pad_for(1), Some((200, 0)));
        assert_eq!(docks.try_reserve(100, 3, 1), Some(0));
        docks.release(1);
        assert_eq!(docks.pad_for(3), Some((100, 0)));
        assert!(docks.has_free_slot(200, 1));
    }

    #[test]
    fn restore_derives_lookup_and_rejects_ambiguous_slot_ownership() {
        let mut docks = AirfieldDocks::default();
        docks.try_reserve(100, 1, 2);
        docks.try_reserve(100, 2, 2);
        let wire = serde_json::to_value(&docks).unwrap();
        assert_eq!(wire, serde_json::json!({"slots": {"100": [1, 2]}}));
        let mut restored: AirfieldDocks =
            bincode::deserialize(&bincode::serialize(&docks).unwrap()).unwrap();
        assert_eq!(restored.pad_for(2), Some((100, 1)));
        restored.release(2);
        assert_eq!(restored.try_reserve(100, 3, 2), Some(1));
        for wire in [
            serde_json::json!({"slots": {"100": [1, 1]}}),
            serde_json::json!({"slots": {"100": [1], "200": [1]}}),
        ] {
            let error = serde_json::from_value::<AirfieldDocks>(wire).unwrap_err();
            assert!(error.to_string().contains("aircraft 1 reserves both"));
        }
    }

    #[test]
    fn dead_airfield_cleanup_removes_live_aircraft_reverse_entries() {
        let mut docks = AirfieldDocks::default();
        docks.try_reserve(100, 1, 2);
        docks.try_reserve(200, 2, 2);
        docks.cleanup_dead(&[1, 2, 200].into_iter().collect());
        assert_eq!(docks.pad_for(1), None);
        assert_eq!(docks.pad_for(2), Some((200, 0)));
        assert_eq!(docks.try_reserve(200, 1, 2), Some(1));
        docks.cleanup_dead(&[200].into_iter().collect());
        assert_eq!(docks.pad_for(1), None);
        assert_eq!(docks.pad_for(2), None);
        assert!(docks.has_free_slot(200, 2));
    }

    #[test]
    fn airfield_docks_basic_reserve() {
        let mut docks = AirfieldDocks::default();
        // 2-pad airfield: first two aircraft get pads 0 and 1.
        assert_eq!(docks.try_reserve(100, 1, 2), Some(0));
        assert_eq!(docks.try_reserve(100, 2, 2), Some(1));
        // 3rd aircraft refused — no queue.
        assert_eq!(docks.try_reserve(100, 3, 2), None);
    }

    #[test]
    fn airfield_release_does_not_pin_freed_pad_index() {
        // Releasing a pad does NOT auto-promote a waiter; the slot just empties.
        // A re-probe then takes the lowest free pad index (first-free scan),
        // not "the pad the releaser vacated".
        let mut docks = AirfieldDocks::default();
        docks.try_reserve(100, 11, 4); // pad 0
        docks.try_reserve(100, 12, 4); // pad 1
        docks.try_reserve(100, 13, 4); // pad 2
        docks.try_reserve(100, 14, 4); // pad 3
        assert_eq!(
            docks.try_reserve(100, 15, 4),
            None,
            "full — refused, no queue"
        );
        docks.release(11); // free pad 0
        docks.release(12); // free pad 1
        // No auto-promotion: 15 holds no pad until it re-probes.
        assert_eq!(docks.pad_for(15), None);
        // Re-probe takes the lowest free index (0), not the last-freed pad (1).
        assert_eq!(docks.try_reserve(100, 15, 4), Some(0));
    }

    #[test]
    fn airfield_full_waiter_admitted_by_probe_not_fifo() {
        // No FIFO: the waiter that re-probes FIRST after a slot frees wins it,
        // regardless of arrival order. 15 probed before 16, but 16 probes first
        // after the slot opens, so 16 wins it.
        let mut docks = AirfieldDocks::default();
        docks.try_reserve(100, 11, 4); // pad 0
        docks.try_reserve(100, 12, 4); // pad 1
        docks.try_reserve(100, 13, 4); // pad 2
        docks.try_reserve(100, 14, 4); // pad 3
        assert_eq!(docks.try_reserve(100, 15, 4), None);
        assert_eq!(docks.try_reserve(100, 16, 4), None);
        docks.release(13); // free pad 2
        assert_eq!(
            docks.try_reserve(100, 16, 4),
            Some(2),
            "first re-probe wins"
        );
        assert_eq!(
            docks.try_reserve(100, 15, 4),
            None,
            "no slot left for later probe"
        );
    }

    #[test]
    fn airfield_docks_release_preserves_other_reservations() {
        let mut docks = AirfieldDocks::default();
        docks.try_reserve(100, 1, 2); // pad 0
        docks.try_reserve(100, 2, 2); // pad 1
        assert_eq!(docks.try_reserve(100, 3, 2), None); // full — refused
        docks.release(1); // free pad 0
        // No auto-promotion: 3 holds no pad until it re-probes.
        assert_eq!(docks.pad_for(3), None);
        assert_eq!(docks.pad_for(2), Some((100, 1)), "2 unaffected");
        // 3 re-probes and takes the freed pad 0.
        assert_eq!(docks.try_reserve(100, 3, 2), Some(0));
    }

    #[test]
    fn airfield_docks_cleanup_dead() {
        let mut docks = AirfieldDocks::default();
        docks.try_reserve(100, 1, 2); // pad 0
        docks.try_reserve(100, 2, 2); // pad 1
        assert_eq!(docks.try_reserve(100, 3, 2), None); // full — refused
        let alive: BTreeSet<u64> = [100, 2, 3].into_iter().collect();
        docks.cleanup_dead(&alive);
        // Aircraft 1 died — pad 0 freed, but #3 is NOT auto-promoted.
        assert_eq!(docks.pad_for(1), None);
        assert_eq!(docks.pad_for(3), None);
        // #3 re-probes and takes the freed pad 0.
        assert_eq!(docks.try_reserve(100, 3, 2), Some(0));
    }

    #[test]
    fn airfield_docks_idempotent_reserve() {
        let mut docks = AirfieldDocks::default();
        assert_eq!(docks.try_reserve(100, 1, 2), Some(0));
        assert_eq!(docks.try_reserve(100, 1, 2), Some(0), "still pad 0");
    }

    #[test]
    fn airfield_docks_four_pad_allocation_order() {
        // GAAIRC has 4 pads. First 4 aircraft get pads 0..3 in arrival order.
        let mut docks = AirfieldDocks::default();
        assert_eq!(docks.try_reserve(100, 11, 4), Some(0));
        assert_eq!(docks.try_reserve(100, 12, 4), Some(1));
        assert_eq!(docks.try_reserve(100, 13, 4), Some(2));
        assert_eq!(docks.try_reserve(100, 14, 4), Some(3));
        // 5th refused — no queue.
        assert_eq!(docks.try_reserve(100, 15, 4), None);
    }

    #[test]
    fn airfield_docks_single_pad_helipad() {
        // Helipads (NAHPAD/GAHPAD) have NumberOfDocks=1.
        let mut docks = AirfieldDocks::default();
        assert_eq!(docks.try_reserve(200, 21, 1), Some(0));
        assert_eq!(docks.try_reserve(200, 22, 1), None, "full — refused");
        docks.release(21);
        // 22 re-probes after the pad frees.
        assert_eq!(docks.try_reserve(200, 22, 1), Some(0));
    }

    #[test]
    fn airfield_docks_pad_assignment_is_deterministic() {
        // Replay/lockstep correctness: identical inputs produce identical
        // pad assignments across two independent runs.
        let mut run_a = AirfieldDocks::default();
        let mut run_b = AirfieldDocks::default();
        for ac in [11_u64, 12, 13, 14] {
            let pa = run_a.try_reserve(100, ac, 4);
            let pb = run_b.try_reserve(100, ac, 4);
            assert_eq!(pa, pb, "aircraft {} got same pad in both runs", ac);
        }
    }

    #[test]
    fn aircraft_ammo_new() {
        let ammo = AircraftAmmo::new(3);
        assert_eq!(ammo.current, 3);
        assert_eq!(ammo.max, 3);
        assert!(ammo.dock_phase.is_none());
        assert!(ammo.target_pad.is_none());
    }
}
