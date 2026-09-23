//! Order-intent tick systems for the Simulation.
//!
//! Handles automatic target acquisition for attack-move and guard orders
//! (pre-combat), and resuming movement after combat ends (post-combat).
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use std::collections::BTreeSet;

use super::{SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat;
use crate::sim::components::OrderIntent;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement;
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::PathGrid;
use crate::util::fixed_math::SimFixed;
use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

/// Result of one `apply_c4_damage_to_building` call.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct C4DamageOutcome {
    /// HP reached 0; building marked dying this tick.
    pub killed_building: bool,
    /// The C4 hit a BridgeRepairHut, the hut survived, and the connected
    /// bridge collapsed. The app needs to rebuild PathGrid.
    pub bridge_state_changed: bool,
    /// The target's pending C4 marker should be cleared even though the
    /// building entity survived. Used by BridgeRepairHut dispatch.
    pub consumed_pending_marker: bool,
}

/// Result of `tick_c4_plants` across all per-tick plants + detonations.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct C4TickOutcome {
    pub destroyed_structure: bool,
    pub bridge_state_changed: bool,
}

impl Simulation {
    fn bridge_repair_notification_allowed(&self, owner: InternedId) -> bool {
        // House50B6F0: nonzero session mode compares exactly to LocalPlayer;
        // mode zero uses the two native human/control flags.
        if self.session.game_mode_nonzero {
            //50B716 loads A83D4C, the persisted native current-house input.
            //Presentation viewer binding cannot change this receiver's result.
            self.session.current_house == Some(owner)
        } else {
            self.houses
                .get(&owner)
                .is_some_and(|h| h.is_controlled_by_human(false))
        }
    }

    fn announce_bridge_repair(
        &mut self,
        owner: InternedId,
        cell: (u16, u16),
        building_cell: (u16, u16),
    ) {
        let radar = self.bridge_repair_notification_allowed(owner).then(|| {
            crate::sim::radar::RadarEventRequest::new(
                crate::sim::radar::RadarEventType::BridgeRepaired,
                cell.0,
                cell.1,
            )
        });
        self.sound_events.push(SimSoundEvent::BridgeRepaired {
            rx: building_cell.0,
            ry: building_cell.1,
            owner,
            radar,
        });
    }

    /// Pre-combat: entities with an OrderIntent but no current AttackTarget
    /// try to acquire a nearby enemy to engage.
    ///
    /// The `order_intent.is_some()` selector is retired in spirit (the busy role
    /// moves to the `mission` substrate) but kept unchanged in code: `OrderIntent`
    /// carries the AttackMove/Guard *coords* that `MissionType` cannot encode.
    /// Full retirement (a goal field on the mission/nav substrate) is a later slice.
    pub(crate) fn tick_order_intents_pre_combat(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
    ) {
        // Collect attacker candidates from EntityStore.
        let keys: Vec<u64> = self.substrate.entities.keys_sorted();
        let mut attacker_ids: Vec<u64> = Vec::new();
        for &id in &keys {
            if turn_suppressed.contains(&id) {
                continue;
            }
            if let Some(entity) = self.substrate.entities.get(id) {
                // A warped object's missions do not run (`ai_frozen`).
                if entity.order_intent.is_some()
                    && entity.attack_target.is_none()
                    && !entity.ai_frozen()
                {
                    attacker_ids.push(id);
                }
            }
        }

        for attacker_id in attacker_ids {
            let Some(scan_mask) = self
                .substrate
                .entities
                .get(attacker_id)
                .map(combat::scan_mission_for)
            else {
                continue;
            };
            let Some(target_sid) = combat::acquire_best_target_for_entity(
                &self.substrate.entities,
                &self.substrate.occupancy,
                rules,
                &self.interner,
                attacker_id,
                Some(&self.fog),
                self.resolved_terrain.as_ref(),
                self.playfield_bounds.is_some(),
                // VERA-internal entry with no single native counterpart, so it
                // keeps the passive block's mask — `1`, or `2` for a player
                // "guard this spot" order. gamemd equivalent UNCHECKED.
                scan_mask,
                self.zone_grid.as_ref(),
                combat::line_of_fire::LineOfFireInputs {
                    overlay_grid: self.overlay_grid.as_ref(),
                    overlay_registry,
                    alliances: Some(&self.fog.alliances),
                },
            ) else {
                continue;
            };
            let _ = combat::issue_attack_command(
                &mut self.substrate.entities,
                attacker_id,
                target_sid,
                Some(rules),
                &self.interner,
            );
        }
    }

    /// Post-combat: entities with an OrderIntent but no active attack or movement
    /// resume their patrol/guard movement toward the original goal. The resume
    /// coords stay on `OrderIntent` — the `mission` substrate has no goal field
    /// yet (Slice-8 follow-up); only the busy-signalling role moved off it.
    #[cfg(test)]
    pub(crate) fn tick_order_intents_post_combat(
        &mut self,
        path_grid: Option<&PathGrid>,
        rules: Option<&RuleSet>,
    ) {
        self.tick_order_intents_post_combat_with_overlay_registry(
            path_grid,
            rules,
            None,
            &BTreeSet::new(),
        );
    }

    pub(crate) fn tick_order_intents_post_combat_with_overlay_registry(
        &mut self,
        path_grid: Option<&PathGrid>,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
    ) {
        let Some(grid) = path_grid else { return };
        // Collect (stable_id, goal) for entities that need to resume movement.
        let keys: Vec<u64> = self.substrate.entities.keys_sorted();
        let mut resumes: Vec<(u64, u16, u16)> = Vec::new();
        for &id in &keys {
            if turn_suppressed.contains(&id) {
                continue;
            }
            if let Some(entity) = self.substrate.entities.get(id) {
                let intent = match entity.order_intent {
                    Some(ref i) => *i,
                    None => continue,
                };
                if entity.attack_target.is_some()
                    || entity.movement_target.is_some()
                    || entity.ai_frozen()
                {
                    continue;
                }
                match intent {
                    OrderIntent::AttackMove { goal_rx, goal_ry }
                        if (entity.position.rx, entity.position.ry) != (goal_rx, goal_ry) =>
                    {
                        resumes.push((id, goal_rx, goal_ry));
                    }
                    OrderIntent::Guard {
                        anchor_rx,
                        anchor_ry,
                    } if (entity.position.rx, entity.position.ry) != (anchor_rx, anchor_ry) => {
                        resumes.push((id, anchor_rx, anchor_ry));
                    }
                    _ => {}
                }
            }
        }

        for (stable_id, goal_rx, goal_ry) in resumes {
            let (base_speed, loco_multiplier, is_air) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|e| {
                    // `FootClass::GetCurrentSpeed @ 0x004DB1A0`: a resumed order
                    // re-queries the getter like any other, so the FASTER stage
                    // runs here too.
                    let obj = rules.and_then(|r| self.object_type(e.type_ref(), r));
                    let bs: SimFixed =
                        crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
                            e,
                            obj,
                            obj.map_or(4, |o| o.speed),
                            rules.map_or(1.0, |r| r.general.veteran_speed),
                        );
                    let lm: SimFixed = e
                        .locomotor
                        .as_ref()
                        .map(|l| l.speed_multiplier)
                        .unwrap_or(SimFixed::from_num(1));
                    let air: bool = e
                        .locomotor
                        .as_ref()
                        .is_some_and(|l| l.layer == MovementLayer::Air);
                    (bs, lm, air)
                })
                .unwrap_or((
                    ra2_speed_to_leptons_per_second(4),
                    SimFixed::from_num(1),
                    false,
                ));
            let speed: SimFixed = (base_speed * loco_multiplier).max(SimFixed::lit("25"));

            if is_air {
                let _ =
                    self.issue_air_cell_destination(stable_id, (goal_rx, goal_ry), speed, rules);
            } else {
                let blocker_neighbor_counts =
                    bump_crush::build_blocker_neighbor_counts_with_overlays(
                        &self.substrate.entities,
                        grid.width(),
                        grid.height(),
                        self.resolved_terrain.as_ref(),
                        self.overlay_grid.as_ref(),
                        overlay_registry,
                        &self.interner,
                        rules,
                    );
                let _ = movement::issue_move_command_with_layered(
                    &mut self.substrate.entities,
                    grid,
                    stable_id,
                    (goal_rx, goal_ry),
                    speed,
                    false,
                    None,
                    None,
                    self.resolved_terrain.as_ref(),
                    self.zone_grid.as_ref(),
                    None,
                    Some(&blocker_neighbor_counts),
                    self.playfield_bounds,
                    Some(&mut self.substrate.cell_occupation),
                    crate::sim::movement::DestinationTiming::from_rules(
                        self.session.binary_frame,
                        rules,
                    ),
                );
            }
        }
    }

    /// Tick engineer capture orders: check if any engineer with a capture_target
    /// has arrived adjacent to its target building. If so, transfer ownership and
    /// consume the engineer.
    ///
    /// Engineers targeting `BridgeRepairHut=yes` buildings are skipped here —
    /// they are consumed earlier in the tick by `tick_bridge_repair_orders`.
    /// This skip is defense in depth in case ordering ever changes; the
    /// original game never captures CABHUTs.
    /// Returns true if any capture occurred (triggers atlas rebuild for new owner color).
    pub(crate) fn tick_capture_orders(
        &mut self,
        rules: &RuleSet,
        turn_suppressed: &BTreeSet<u64>,
    ) -> bool {
        let mut any_captured = false;
        // Snapshot engineers with active capture targets.
        let captures: Vec<(u64, u64, InternedId)> = self
            .substrate
            .entities
            .values()
            .filter(|e| {
                e.capture_target.is_some()
                    && !e.dying
                    // A warped engineer never reaches PerCellProcess.
                    && !e.ai_frozen()
                    && !turn_suppressed.contains(&e.stable_id())
            })
            .map(|e| (e.stable_id(), e.capture_target.unwrap(), e.owner()))
            .collect();

        for (engineer_id, building_id, engineer_owner) in captures {
            // Skip BridgeRepairHut targets — repair tick handles them.
            let target_bridge_hut = self
                .substrate
                .entities
                .get(building_id)
                .and_then(|b| {
                    self.object_type(b.type_ref(), rules)
                        .map(|t| t.bridge_repair_hut)
                })
                .unwrap_or(false);
            if target_bridge_hut {
                continue;
            }

            // Check building still exists and is capturable.
            let building_ok = self
                .substrate
                .entities
                .get(building_id)
                .is_some_and(|b| b.category == EntityCategory::Structure && !b.dying);
            if !building_ok {
                // Target lost — clear capture order.
                if let Some(e) = self.substrate.entities.get_mut(engineer_id) {
                    e.capture_target = None;
                }
                continue;
            }

            // Distance check: adjacent = Chebyshev distance <= 1 cell.
            let (eng_rx, eng_ry) = self
                .substrate
                .entities
                .get(engineer_id)
                .map(|e| (e.position.rx, e.position.ry))
                .unwrap_or((0, 0));
            let (bld_rx, bld_ry) = self
                .substrate
                .entities
                .get(building_id)
                .map(|e| (e.position.rx, e.position.ry))
                .unwrap_or((0, 0));
            let dx = (eng_rx as i32 - bld_rx as i32).abs();
            let dy = (eng_ry as i32 - bld_ry as i32).abs();

            // InfantryClass::PerCellProcess turns the engineer away from a
            // building being warped (`0x00519EF2`): no capture.
            let target_warped = self
                .substrate
                .entities
                .get(building_id)
                .is_some_and(crate::sim::game_entity::GameEntity::is_warped_out);
            if dx <= 1 && dy <= 1 && !target_warped {
                self.announce_engineer_capture(building_id, engineer_owner, rules);
                // CAPTURE: the ownership chokepoint moves HouseState counts,
                // the by-owner index, and the entity owner exactly once.
                self.change_owner_with_rules(building_id, engineer_owner, rules);
                // Destroy engineer (consumed on capture).
                self.uninit_with_rules(engineer_id, rules);
                any_captured = true;
            }
        }
        any_captured
    }

    /// `BuildingClass::ChangeOwner @ 0x00448260` announce block
    /// (`0x004483C0..0x0044848F`), run before the owner swap because native
    /// reads `this->Owner` (the OLD owner) there. The engineer capture site
    /// (`InfantryClass::PerCellProcess 0x00519A27 PUSH 1`) passes
    /// announce=true. Native gates: the old or the new owner is the local
    /// player (`0x004483C6`/`0x004483D1 CALL 0x0050B6F0`) and the new
    /// owner's type is not `MultiplayPassive` (`0x004483E1 HouseType+0x1A6`).
    /// The sim has no local player, so it pre-filters on a human-controlled
    /// house on either side and the app applies the local test (equal in the
    /// single-human skirmish; VERA-internal, gamemd equivalent UNCHECKED for
    /// two-human matches, where the radar event would also be local).
    /// `NeedsEngineer=` types skip the radar event (`0x00448407`); the rest
    /// go through `CreateRadarEvent(10, cell)` (`0x00448472 MOV ECX,0xA`)
    /// whose accept result gates `EVA_BuildingCaptured` — type table row 10
    /// (`0x007F0A38`: dedup 8, blink 100, unique 0) has no uniqueness
    /// dedupe, so on stock data the radar accepts every capture. The type's
    /// `CaptureEvaEvent=` (`Type+0x1554`, `0x00448443..0x00448459`) rides
    /// along for the app's local-new-owner branch.
    pub(crate) fn announce_engineer_capture(
        &mut self,
        building_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
    ) {
        let Some((old_owner, type_ref, rx, ry)) = self
            .substrate
            .entities
            .get(building_id)
            .map(|e| (e.owner(), e.type_ref(), e.position.rx, e.position.ry))
        else {
            return;
        };
        if old_owner == new_owner {
            return;
        }
        let game_mode_nonzero = self.session.game_mode_nonzero;
        let human = |sim: &Self, house: InternedId| {
            sim.houses
                .get(&house)
                .is_some_and(|h| h.is_controlled_by_human(game_mode_nonzero))
        };
        if !(human(self, old_owner) || human(self, new_owner)) {
            return;
        }
        if self
            .houses
            .get(&new_owner)
            .is_some_and(|h| h.multiplay_passive)
        {
            return;
        }
        let (tech_building, capture_eva_event) = self
            .object_type(type_ref, rules)
            .map(|obj| (obj.needs_engineer, obj.capture_eva_event.clone()))
            .unwrap_or((false, None));
        let capture_eva_event = capture_eva_event.map(|name| self.interner.intern(&name));
        let radar = (!tech_building).then(|| {
            crate::sim::radar::RadarEventRequest::new(
                crate::sim::radar::RadarEventType::BuildingCaptured,
                rx,
                ry,
            )
        });
        self.sound_events.push(SimSoundEvent::BuildingCaptured {
            old_owner,
            new_owner,
            tech_building,
            radar,
            capture_eva_event,
        });
    }

    /// Legacy order-intent preparation: an adjacent idle engineer still needs
    /// an ordinary move into the hut. Actual repair belongs exclusively to
    /// Infantry PerCell2 inside the live object turn (519B58..519D12).
    pub(crate) fn tick_bridge_repair_orders_with_overlay_registry(
        &mut self,
        rules: &RuleSet,
        _overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
    ) -> bool {
        for id in self.substrate.entities.keys_sorted() {
            if turn_suppressed.contains(&id) {
                continue;
            }
            let Some((target, cell)) = self.substrate.entities.get(id).and_then(|e| {
                (!e.dying && !e.ai_frozen())
                    .then_some((e.capture_target?, (e.position.rx, e.position.ry)))
            }) else {
                continue;
            };
            if !self
                .substrate
                .entities
                .get(target)
                .and_then(|b| self.object_type(b.type_ref(), rules))
                .is_some_and(|t| t.bridge_repair_hut)
            {
                continue;
            }
            let Some(footprint) = self.building_entry_target_footprint(target, rules) else {
                continue;
            };
            if !footprint.contains(&cell)
                && self.adjacent_to_target_footprint(cell, &footprint)
                && !self.infantry_has_active_movement(id)
            {
                self.issue_building_enter_target_cell(id, cell, &footprint, rules);
            }
        }
        false
    }

    /// Ordinary Infantry PerCell2 engineer receiver. The active object cursor
    /// owns removal/next-object cadence; there is no second sorted repair pass.
    pub(crate) fn infantry_per_cell_bridge_repair(
        &mut self,
        engineer_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<bool, super::FrameAdvanceError> {
        use crate::rules::mission_data::MissionType;
        use crate::sim::components::NavTargetRef;
        let Some(engineer) = self.substrate.entities.get(engineer_id) else {
            return Ok(false);
        };
        if engineer.category != EntityCategory::Infantry
            || !matches!(
                engineer.mission.current().known(),
                Some(MissionType::Capture | MissionType::AreaGuard | MissionType::Patrol)
            )
            || !self
                .object_type(engineer.type_ref(), rules)
                .is_some_and(|t| t.engineer)
        {
            return Ok(false);
        }
        let cell = (engineer.position.rx, engineer.position.ry);
        let owner = engineer.owner();
        let Some(building_id) = self.substrate.occupancy.first_building_on_layer(
            cell.0,
            cell.1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
        ) else {
            return Ok(false);
        };
        let targets = matches!(engineer.navigation.nav_com,
            Some(NavTargetRef::Entity{id}|NavTargetRef::Object{id}|NavTargetRef::Building{id}) if id==building_id)
            || engineer.attack_target.as_ref().is_some_and(|attack| {
                matches!(attack.target, crate::sim::combat::TargetKind::Entity(id) if id == building_id)
            });
        if !targets {
            return Ok(false);
        }
        let Some(building) = self.substrate.entities.get(building_id) else {
            return Ok(false);
        };
        if !self
            .object_type(building.type_ref(), rules)
            .is_some_and(|t| t.bridge_repair_hut)
        {
            return Ok(false);
        }
        let building_cell = (building.position.rx, building.position.ry);
        //519BB6 supplies the ENGINEER cell;519C02 supplies building XYZ.
        //519B90/50B6F0 gates insertion itself, including radar dedup state.
        self.announce_bridge_repair(owner, cell, building_cell);
        let mut changed = crate::sim::world::bridge_orchestrator::repair_from_engineer(
            self,
            rules,
            registry,
            engineer_id,
        )
        .map_err(|cause| {
            super::FrameAdvanceError::bridge_repair(
                self.session.tick,
                self.session.binary_frame,
                engineer_id,
                cause,
            )
        })?;
        //519D17..519D36 descends Infantry's registry with +28(hut,false).
        //Clearing NavCom does not stop a retained Walk head/destination.
        self.expire_infantry_bridge_hut_targets(building_id);
        changed |= self.scatter_bridge_hut(building_id, rules, registry)?;
        // Attached Tag6E53A0 remains a separate synchronous receiver boundary.
        self.uninit_with_rules(engineer_id, rules);
        Ok(changed)
    }

    /// Tick C4 plant orders.
    ///
    /// Phase 1 (walk-up): for each entity with `c4_plant`, check if it's
    /// Chebyshev-≤-1 adjacent to the target building's anchor cell; if so
    /// and the building doesn't already have a `pending_c4_detonation`
    /// claimed by another attacker, claim it. Second attackers on an
    /// already-claimed target hover (no-op) — matches gamemd's `+0x6df`
    /// marker check.
    ///
    /// Phase 2 (detonation): for each building with `pending_c4_detonation`,
    /// if the wrapping native-frame elapsed count reaches
    /// `rules.c4_delay_ticks`, apply C4Warhead
    /// damage equal to the building's current HP. For normal buildings the
    /// pending state is not cleared if damage is nullified by IronCurtain, so
    /// it fires again next frame. When the building dies, the entity despawns
    /// and the pending state goes with it. BridgeRepairHut dispatch clears
    /// the pending marker after the bridge path runs because the hut survives.
    ///
    /// Returns the per-tick C4 outcome: `destroyed_structure` is true if any
    /// building died, and `bridge_state_changed` is true if any C4 detonation
    /// on a `BridgeRepairHut` collapsed a bridge.
    pub(crate) fn tick_c4_plants_with_overlay_registry(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
    ) -> C4TickOutcome {
        use crate::sim::components::PendingC4Detonation;
        let mut destroyed_structure = false;
        let mut bridge_state_changed = false;

        // ---- Phase 1: walk-up + plant claim ----
        // Snapshot attackers with c4_plant. Deterministic sorted order via
        // keys_sorted then look up c4_plant.
        let mut walkup: Vec<(u64, u64)> = Vec::new();
        for sid in self.substrate.entities.keys_sorted() {
            if turn_suppressed.contains(&sid) {
                continue;
            }
            if let Some(e) = self.substrate.entities.get(sid) {
                if let Some(plant) = e.c4_plant {
                    // A warped planter's mission does not run (`ai_frozen`).
                    if !e.dying && !e.ai_frozen() {
                        walkup.push((sid, plant.target_building_id));
                    }
                }
            }
        }

        for (attacker_id, target_id) in walkup {
            // Target gone or dying? Clear c4_plant.
            let target_alive = self
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|b| b.category == EntityCategory::Structure && !b.dying);
            if !target_alive {
                if let Some(e) = self.substrate.entities.get_mut(attacker_id) {
                    e.c4_plant = None;
                }
                continue;
            }

            // gamemd claims only when the infantry's current cell resolves
            // to the target building. Normal pathing stops at the blocked
            // footprint boundary, then we issue the one-cell enter move below.
            let attacker_cell = self
                .substrate
                .entities
                .get(attacker_id)
                .map(|e| (e.position.rx, e.position.ry));
            let target_footprint = self.building_entry_target_footprint(target_id, rules);
            let (Some(attacker_cell), Some(target_footprint)) = (attacker_cell, target_footprint)
            else {
                continue;
            };

            // Already claimed by another attacker?
            let already_claimed = self
                .substrate
                .entities
                .get(target_id)
                .is_some_and(|b| b.pending_c4_detonation.is_some());
            if already_claimed {
                // Second SEAL — hover, no-op. Matches gamemd's marker-set early-return.
                continue;
            }

            if !target_footprint.contains(&attacker_cell) {
                if self.adjacent_to_target_footprint(attacker_cell, &target_footprint)
                    && !self.infantry_has_active_movement(attacker_id)
                {
                    self.issue_building_enter_target_cell(
                        attacker_id,
                        attacker_cell,
                        &target_footprint,
                        rules,
                    );
                }
                continue; // walk-up or enter-cell movement still in progress
            }

            // Claim the plant.
            if let Some(b) = self.substrate.entities.get_mut(target_id) {
                b.pending_c4_detonation = Some(PendingC4Detonation {
                    start_frame: self.session.binary_frame as i32,
                    duration_frames: rules.c4_delay_ticks as i32,
                    source_entity_id: Some(attacker_id),
                });
            }

            // Drive the plant animation (FireUp = Attack sequence).
            if let Some(a) = self.substrate.entities.get_mut(attacker_id) {
                a.movement_target = None;
                if let Some(ref mut anim) = a.animation {
                    anim.switch_to(crate::sim::animation::SequenceKind::Attack);
                }
            }

            // SealPlaceBomb spatial sound. App-side dispatcher resolves to
            // `[SealPlaceBomb]` from soundmd.ini.
            if let Some(a) = self.substrate.entities.get(attacker_id) {
                self.sound_events
                    .push(crate::sim::world::SimSoundEvent::C4Planted {
                        rx: a.position.rx,
                        ry: a.position.ry,
                    });
            }
        }

        // ---- Phase 2: detonation ----
        let mut det_keys: Vec<u64> = Vec::new();
        for sid in self.substrate.entities.keys_sorted() {
            if let Some(e) = self.substrate.entities.get(sid) {
                let bridge_hut = rules
                    .object(self.interner.resolve(e.type_ref()))
                    .is_some_and(|object| object.bridge_repair_hut);
                if e.pending_c4_detonation.is_some() && !e.dying && bridge_hut {
                    det_keys.push(sid);
                }
            }
        }
        // Early-out returns before rule-handle resolution so pre-feature
        // fixtures never intern warhead names they do not exercise. They do not
        // call it; guarding here keeps them passing.
        if det_keys.is_empty() {
            return C4TickOutcome {
                destroyed_structure,
                bridge_state_changed,
            };
        }

        let c4_warhead_id = self.rule_handles().c4;
        for building_id in det_keys {
            let pending = self
                .substrate
                .entities
                .get(building_id)
                .and_then(|e| e.pending_c4_detonation);
            let Some(pending) = pending else { continue };

            if !pending.is_expired_at(self.session.binary_frame as i32) {
                continue;
            }

            // Timer elapsed — apply C4Warhead damage. Damage value = current_hp
            // for guaranteed one-shot kill (matches gamemd's
            // `&iStack_28 = this->Health` argument to TakeDamage).
            // Normal-building pending state is only cleared by despawn;
            // BridgeRepairHut returns consumed_pending_marker below.
            let dmg: i32 = self
                .substrate
                .entities
                .get(building_id)
                .map(|b| b.health.current as i32)
                .unwrap_or(0);
            if dmg <= 0 {
                continue;
            }

            // Resolve kill-credit. Attacker may have despawned — fall back to None.
            let attacker_for_credit = pending
                .source_entity_id
                .filter(|&source_id| self.substrate.entities.get(source_id).is_some());

            let outcome = self.apply_c4_damage_to_building(
                building_id,
                dmg,
                c4_warhead_id,
                attacker_for_credit,
                rules,
                overlay_registry,
            );
            bridge_state_changed |= outcome.bridge_state_changed;
            if outcome.killed_building {
                destroyed_structure = true;
                // pending_c4_detonation goes away with the entity via despawn path.
                // Trigger scatter walk-away for any attacker on this cell with
                // c4_plant pointing at this building. Matches gamemd
                // Mission_Enter post-detonation block.
                self.queue_c4_post_detonation_scatter(building_id);
            } else if outcome.consumed_pending_marker {
                if let Some(building) = self.substrate.entities.get_mut(building_id) {
                    building.pending_c4_detonation = None;
                }
                if let Some(attacker_id) = pending.source_entity_id
                    && let Some(attacker) = self.substrate.entities.get_mut(attacker_id)
                {
                    if attacker
                        .c4_plant
                        .is_some_and(|plant| plant.target_building_id == building_id)
                    {
                        attacker.c4_plant = None;
                    }
                }
            }
        }

        C4TickOutcome {
            destroyed_structure,
            bridge_state_changed,
        }
    }

    /// BuildingClass::Update's shared C4/PostMortem expiry tail. Called from
    /// the current Structure LogicVector visit, so the forced receiver and any
    /// nested DeathWeapon complete before the next live object is visited.
    pub(crate) fn tick_pending_building_detonation(
        &mut self,
        building_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some((pending, health, bridge_hut)) = self
            .substrate
            .entities
            .get(building_id)
            .and_then(|building| {
                let pending = building.pending_c4_detonation?;
                let object = rules.object(self.interner.resolve(building.type_ref()))?;
                Some((
                    pending,
                    i32::from(building.health.current),
                    object.bridge_repair_hut,
                ))
            })
        else {
            return;
        };
        if !pending.is_expired_at(self.session.binary_frame as i32) || health <= 0 {
            return;
        }

        if bridge_hut {
            // Preserve the existing bridge-specific Phase-5 consumer outside
            // this damage slice. It owns collapse, attacker cleanup, and the
            // result flag that invalidates bridge navigation for the caller.
            return;
        }

        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            building_id,
            health,
            0,
            pending
                .source_entity_id
                .unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            None,
            self.rule_handles().c4,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        self.commit_noncombat_aoe_hits(rules, overlay_registry, &[event]);
    }

    fn building_entry_target_footprint(
        &self,
        target_id: u64,
        rules: &RuleSet,
    ) -> Option<Vec<(u16, u16)>> {
        let target = self.substrate.entities.get(target_id)?;
        let obj = self.object_type(target.type_ref(), rules)?;
        // Infantry building-entry resolves through normal building cell lookup.
        // AddOccupy/RemoveOccupy only affect hidden occupancy counters.
        Some(c4_base_foundation_cells(
            target.position.rx,
            target.position.ry,
            obj.foundation.as_str(),
        ))
    }

    fn adjacent_to_target_footprint(
        &self,
        attacker_cell: (u16, u16),
        target_footprint: &[(u16, u16)],
    ) -> bool {
        target_footprint.iter().any(|&(trx, try_)| {
            let dx = (attacker_cell.0 as i32 - trx as i32).abs();
            let dy = (attacker_cell.1 as i32 - try_ as i32).abs();
            dx <= 1 && dy <= 1
        })
    }

    fn infantry_has_active_movement(&self, attacker_id: u64) -> bool {
        self.substrate
            .entities
            .get(attacker_id)
            .is_some_and(|attacker| attacker.movement_target.is_some())
    }

    fn issue_building_enter_target_cell(
        &mut self,
        attacker_id: u64,
        attacker_cell: (u16, u16),
        target_footprint: &[(u16, u16)],
        rules: &RuleSet,
    ) {
        let Some(entry_cell) = target_footprint.iter().copied().min_by_key(|&(rx, ry)| {
            let dx = (attacker_cell.0 as i32 - rx as i32).abs();
            let dy = (attacker_cell.1 as i32 - ry as i32).abs();
            (dx.max(dy), dx + dy, rx, ry)
        }) else {
            return;
        };

        let speed = self
            .resolve_move_info(attacker_id, Some(rules))
            .as_ref()
            .map(|info| info.speed)
            .unwrap_or(ra2_speed_to_leptons_per_second(4));
        let timing =
            movement::DestinationTiming::from_rules(self.session.binary_frame, rules.into());
        if movement::issue_direct_move(
            &mut self.substrate.entities,
            attacker_id,
            entry_cell,
            speed,
            timing,
        ) {
            if let Some(target) = self
                .substrate
                .entities
                .get_mut(attacker_id)
                .and_then(|attacker| attacker.movement_target.as_mut())
            {
                target.bypass_grid = true;
            }
        }
    }

    /// Post-detonation: any attacker that was on the destroyed building's
    /// cell with `c4_plant` targeting this building scatters one cell in a
    /// deterministic direction derived from the current tick. Matches gamemd
    /// `Mission_Enter` post-detonation block:
    /// `uVar13 = (tick >> 12 + 1) >> 1 & 7` → 1 of 8 directions via
    /// the direction-delta tables.
    ///
    /// Also clears each attacker's `c4_plant`.
    fn queue_c4_post_detonation_scatter(&mut self, dead_building_id: u64) {
        // 8 cardinal+ordinal directions in standard RA2 order:
        // N, NE, E, SE, S, SW, W, NW.
        const DIR_DELTAS: [(i16, i16); 8] = [
            (0, -1),  // N
            (1, -1),  // NE
            (1, 0),   // E
            (1, 1),   // SE
            (0, 1),   // S
            (-1, 1),  // SW
            (-1, 0),  // W
            (-1, -1), // NW
        ];
        // Mirror the native-frame bit-twiddle: `(frame >> 12 + 1) >> 1 & 7`.
        // C operator precedence: `>>` is left-to-right at same level, so
        // this evaluates as `(((frame >> 12) + 1) >> 1) & 7`.
        let dir: usize = ((((self.session.binary_frame >> 12) + 1) >> 1) & 7) as usize;
        let (dx, dy) = DIR_DELTAS[dir];

        let bld_cell = self
            .substrate
            .entities
            .get(dead_building_id)
            .map(|b| (b.position.rx, b.position.ry));
        let Some((brx, bry)) = bld_cell else { return };

        // Collect attackers on this cell with c4_plant on this building.
        let mut scatterers: Vec<u64> = Vec::new();
        for sid in self.substrate.entities.keys_sorted() {
            if let Some(e) = self.substrate.entities.get(sid) {
                if !e.dying
                    && e.position.rx == brx
                    && e.position.ry == bry
                    && e.c4_plant
                        .map_or(false, |p| p.target_building_id == dead_building_id)
                {
                    scatterers.push(sid);
                }
            }
        }

        for sid in scatterers {
            let target_rx = (brx as i16 + dx).max(0) as u16;
            let target_ry = (bry as i16 + dy).max(0) as u16;
            if let Some(e) = self.substrate.entities.get_mut(sid) {
                e.c4_plant = None;
            }
            // Queue a Move command for the next tick. Simpler than
            // reimplementing the pathfind call; 1-tick delay is below the
            // human-observable threshold.
            if let Some(owner) = self.substrate.entities.get(sid).map(|e| e.owner()) {
                self.queue_command(crate::sim::command::CommandEnvelope::new(
                    owner,
                    self.session.tick + 1,
                    crate::sim::command::Command::Move {
                        entity_id: sid,
                        target_rx,
                        target_ry,
                        queue: false,
                        group_id: None,
                    },
                ));
            }
        }
    }

    /// Apply one C4Warhead damage instance to a building entity. Returns
    /// `C4DamageOutcome` reporting whether the building died and whether a
    /// connected bridge collapsed (for `BridgeRepairHut` targets). Non-hut
    /// targets honor IronCurtain via the standard invulnerability check.
    /// Used by `tick_c4_plants` Phase 2.
    fn apply_c4_damage_to_building(
        &mut self,
        building_id: u64,
        damage: i32,
        warhead_id: crate::sim::intern::InternedId,
        attacker_id: Option<u64>,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> C4DamageOutcome {
        // BridgeRepairHut target: reroute the explosion into the bridge
        // collapse cascade and leave the hut at full HP. The hut never
        // takes C4 / demo-truck damage — destruction is the linked bridge
        // segment's, not the hut's. Also the right entry point for a
        // future demo-truck damage path.
        let target_bridge_hut = self
            .substrate
            .entities
            .get(building_id)
            .and_then(|b| {
                rules
                    .object(self.interner.resolve(b.type_ref()))
                    .map(|t| t.bridge_repair_hut)
            })
            .unwrap_or(false);
        if target_bridge_hut {
            let bld_center = self
                .substrate
                .entities
                .get(building_id)
                .map(|b| (b.position.rx, b.position.ry));
            let bridge_state_changed = match bld_center {
                Some(center) => {
                    crate::sim::world::bridge_orchestrator::dispatch_bridge_collapse_from_hut_with_overlay_registry(
                        self,
                        rules,
                        center,
                        overlay_registry,
                    )
                }
                None => false,
            };
            let _ = attacker_id; // hut survives — no last_attacker_id update
            return C4DamageOutcome {
                killed_building: false,
                bridge_state_changed,
                consumed_pending_marker: true,
            };
        }

        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            building_id,
            damage,
            0,
            attacker_id.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            None,
            warhead_id,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        self.commit_noncombat_aoe_hits(rules, overlay_registry, &[event]);
        if self
            .substrate
            .entities
            .get(building_id)
            .is_none_or(|b| b.dying)
        {
            C4DamageOutcome {
                killed_building: true,
                bridge_state_changed: false,
                consumed_pending_marker: false,
            }
        } else {
            C4DamageOutcome::default()
        }
    }

    /// Pre-combat: entities with an `attack_target` that's out of weapon
    /// range walk toward the target. Entities that just entered range halt
    /// their movement so the combat tick can fire from a stationary
    /// position.
    ///
    /// Range failure preserves the target; pursuit closes the gap.
    ///
    /// Skips entities that can't or shouldn't pursue:
    /// - Structures (can't move)
    /// - Aircraft (own state machine in `attack_mission.rs`)
    /// - Deployed-fire infantry (locked while deployed)
    /// - Entities inside transports
    /// - Dying entities
    /// - Objects holding a target their own scanner picked up (see below)
    /// - Objects on the **Sticky** mission, which drop the target instead
    ///
    /// **A passively-acquired target is never pursued.** The original's passive
    /// commit writes the target pointer and nothing else — no mission assign and
    /// no destination assign — and a Guard-mission unit derives any destination
    /// it does have from its OWN position, never from the target's. So an idle
    /// unit that notices an enemy fires from where it stands and stays put.
    /// Without this skip an idle base-defence force would walk off across the
    /// map, unleashed, the first time an enemy scouted past: nothing carries
    /// these units home because they have no `OrderIntent` to resume.
    #[cfg(test)]
    pub(crate) fn tick_attack_pursuit(&mut self, rules: &RuleSet, path_grid: Option<&PathGrid>) {
        self.tick_attack_pursuit_with_overlay_registry(rules, path_grid, None, &BTreeSet::new());
    }

    pub(crate) fn tick_attack_pursuit_with_overlay_registry(
        &mut self,
        rules: &RuleSet,
        path_grid: Option<&PathGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
    ) {
        let Some(grid) = path_grid else {
            return;
        };

        // Phase 1: collect pursuit decisions (read-only on entities).
        // Two action kinds: issue a new path, or clear an existing one.
        enum PursuitAction {
            IssueMove {
                entity_id: u64,
                goal: (u16, u16),
            },
            ClearMovement {
                entity_id: u64,
            },
            /// A Sticky object that cannot already shoot its target: drop the
            /// target AND the destination, and produce no pursuit cell.
            DropTargetAndMovement {
                entity_id: u64,
            },
        }

        let keys: Vec<u64> = self.substrate.entities.keys_sorted();
        let mut actions: Vec<PursuitAction> = Vec::new();

        for &id in &keys {
            if turn_suppressed.contains(&id) {
                continue;
            }
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            let Some(attack) = entity.attack_target.as_ref() else {
                continue;
            };

            // Skip filters — see "Skips" doc above.
            if entity.dying {
                continue;
            }
            if entity.passively_acquired_target {
                continue;
            }
            if entity.category == EntityCategory::Structure {
                continue;
            }
            if entity.aircraft_mission.is_some() {
                continue;
            }
            if entity.is_deployed() {
                continue;
            }
            if entity.passenger_role.is_inside_transport() {
                continue;
            }

            // Ground Infantry's native approach helper4D5690 returns at
            // 4D5A07..18 while NavCom is present. An accepted Walk path/head
            // continues through Process; FootPerCell(mode2)4D882F..896E makes
            // the range-stop decision after completion. Dropping the path
            // adapter here strands its independently retained head/raw claim.
            if entity
                .locomotor
                .as_ref()
                .is_some_and(|loco| loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk)
                && entity.navigation.nav_com.is_some()
            {
                continue;
            }

            // Resolve target coords using the same helper combat tick uses.
            // None means entity-target despawned; combat tick's target-dead
            // branch handles cleanup.
            let target_pos = combat::resolve_target_coords(
                &attack.target,
                &self.substrate.entities,
                Some(rules),
                &self.interner,
            );
            let Some((trx, try_, _tsx, _tsy)) = target_pos else {
                continue;
            };

            // Resolve the weapon using the shared helper. None means no weapon
            // can engage; combat tick will drop on its own weapon-select fail.
            let Some(weapon) = combat::pursuit_selected_weapon(
                entity,
                &attack.target,
                &self.substrate.entities,
                rules,
                &self.interner,
                self.resolved_terrain.as_ref(),
                Some(&self.house_alliances),
            ) else {
                continue;
            };

            // Range check — the SAME predicate the combat tick's fire gate
            // uses, line-of-fire walk included. The approach search
            // (`FootClass::Greatest_Threat_Scan @ 0x004D5690`, vt+0x53C,
            // reached from `Mission_Attack` 0x004D4DC0 at 0x004D4E6A) decides
            // with `TechnoClass::InRange` 0x006F7220 at 0x004D622C /
            // 0x004D6550, and `InRange` ends in the wall/cliff walk at
            // 0x006F7642. If this stage used the plain radius while the fire
            // gate ran the walk, a unit ordered to shoot across a wall would
            // halt here and then be refused the shot, standing still under a
            // live order.
            //
            // The one refusal that must NOT produce a pursuit cell is
            // `MinimumRange`: native's approach search picks a candidate
            // FARTHER out, and the target's own cell — VERA's only candidate —
            // is the worst one in that set. `HoldInsideMinimumRange` therefore
            // takes the halt arm, which is exactly what this stage did for that
            // case before the walk landed. See `PursuitRangeVerdict`.
            let verdict = combat::pursuit_in_range(
                entity,
                &attack.target,
                weapon,
                &self.substrate.entities,
                rules,
                &self.interner,
                self.resolved_terrain.as_ref(),
                // Same alliance view the fire gate hands the walk
                // (`resolve_attacker_fire` passes `fog.alliances`) and the same
                // one the sibling pre-combat scan uses, so the two stages
                // cannot disagree on the `AlliedWallTransparency` arm.
                &combat::line_of_fire::LineOfFireInputs {
                    overlay_grid: self.overlay_grid.as_ref(),
                    overlay_registry,
                    alliances: Some(&self.fog.alliances),
                },
            );

            if verdict == combat::PursuitRangeVerdict::CloseIn {
                // **Sticky never chases.** The one place the engine tells
                // Sticky apart from Guard at all — they share a mission handler
                // — is the pursuit-cell producer: when the can-fire-at query
                // fails and the object's committed mission is Sticky, it drops
                // both its target and its destination and returns "nowhere to
                // go", *before* the fallthrough that lets a Guard-family object
                // pursue. That is the whole of `[Sticky]`'s "just like guard
                // mode, but cannot move".
                //
                // Read off the RAW committed selector, as the original does —
                // not the derived reading.
                if entity.mission.current().known() == Some(MissionType::Sticky) {
                    actions.push(PursuitAction::DropTargetAndMovement { entity_id: id });
                } else if entity.movement_target.is_none() {
                    // Out of range, no current pursuit — issue a path.
                    actions.push(PursuitAction::IssueMove {
                        entity_id: id,
                        goal: (trx, try_),
                    });
                }
                // else: existing pursuit movement is still running; let it continue.
            } else if entity.movement_target.is_some() {
                // `CanFire` — halt for firing. `HoldInsideMinimumRange` halts
                // here too: closing further cannot help, and this is the arm it
                // took before the walk landed.
                actions.push(PursuitAction::ClearMovement { entity_id: id });
            }
        }

        // Phase 2: apply mutations.
        for action in actions {
            match action {
                PursuitAction::IssueMove { entity_id, goal } => {
                    let Some(info) = self.resolve_move_info(entity_id, Some(rules)) else {
                        continue;
                    };
                    let owner_str = self
                        .substrate
                        .entities
                        .get(entity_id)
                        .map(|e| self.interner.resolve(e.owner()).to_string())
                        .unwrap_or_default();
                    let (entity_blocks, entity_block_map) = bump_crush::build_entity_block_set(
                        &self.substrate.entities,
                        &owner_str,
                        &self.house_alliances,
                        &self.interner,
                        Some(rules),
                    );
                    let cost_grid = self.terrain_costs.get(&info.speed_type);
                    let blocker_neighbor_counts =
                        bump_crush::build_blocker_neighbor_counts_with_overlays(
                            &self.substrate.entities,
                            grid.width(),
                            grid.height(),
                            self.resolved_terrain.as_ref(),
                            self.overlay_grid.as_ref(),
                            overlay_registry,
                            &self.interner,
                            Some(rules),
                        );
                    let _issued = movement::issue_move_command_with_layered(
                        &mut self.substrate.entities,
                        grid,
                        entity_id,
                        goal,
                        info.speed,
                        false, // queue
                        cost_grid,
                        Some(&entity_blocks),
                        self.resolved_terrain.as_ref(),
                        self.zone_grid.as_ref(),
                        Some(&entity_block_map),
                        Some(&blocker_neighbor_counts),
                        self.playfield_bounds,
                        Some(&mut self.substrate.cell_occupation),
                        crate::sim::movement::DestinationTiming::new(
                            self.session.binary_frame,
                            rules.general.blockage_path_delay_ticks,
                        ),
                    );
                    // No-op if A* fails — pursuit retries next tick.
                }
                PursuitAction::ClearMovement { entity_id } => {
                    if let Some(e) = self.substrate.entities.get_mut(entity_id) {
                        // A prior null destination (for example Stop) still
                        // leaves a paid Walk head active until its completion.
                        if !e.locomotor.as_ref().is_some_and(|loco| {
                            loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk
                                && loco.step_head().is_some()
                        }) {
                            e.movement_target = None;
                        }
                    }
                }
                PursuitAction::DropTargetAndMovement { entity_id } => {
                    if let Some(e) = self.substrate.entities.get_mut(entity_id) {
                        // Foot4D5730 calls Assign_Target(NULL) on Sticky refusal.
                        crate::sim::mission::concrete_effects::represented_assign_target(e, None);
                        e.movement_target = None;
                        e.navigation.nav_com = None;
                    }
                }
            }
        }
    }
}

fn c4_base_foundation_cells(origin_rx: u16, origin_ry: u16, foundation: &str) -> Vec<(u16, u16)> {
    let (w, h) = crate::rules::foundation::foundation_dimensions(foundation);
    let mut cells = Vec::with_capacity(w as usize * h as usize);

    for dx in 0..w {
        for dy in 0..h {
            let rx = origin_rx as i32 + dx as i32;
            let ry = origin_ry as i32 + dy as i32;
            if rx >= 0 && rx <= u16::MAX as i32 && ry >= 0 && ry <= u16::MAX as i32 {
                cells.push((rx as u16, ry as u16));
            }
        }
    }

    cells
}

#[cfg(test)]
mod repair_notification_tests {
    use super::*;

    #[test]
    fn native_current_house_gates_repair_on_headless_restore() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let other = sim.interner.intern("Russians");
        for id in [owner, other] {
            sim.houses.insert(
                id,
                crate::sim::house_state::HouseState::new(id, 0, None, true, 0, 1),
            );
            sim.session.house_order.push(id);
        }
        sim.session.current_house = Some(owner);
        sim.session.game_mode_nonzero = true;
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "notification", 0);
        let restore = || {
            crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim
        };
        let mut restored = restore();
        assert_eq!(restored.session.current_house, Some(owner));
        restored.announce_bridge_repair(owner, (7, 8), (8, 8));
        assert!(
            matches!(
                restored.sound_events.last(),
                Some(SimSoundEvent::BridgeRepaired {
                    radar: Some(crate::sim::radar::RadarEventRequest {
                        event_type: crate::sim::radar::RadarEventType::BridgeRepaired,
                        rx: 7,
                        ry: 8,
                    }),
                    ..
                })
            ),
            "headless restore needs no app viewer bind"
        );

        let mut other_current = restore();
        other_current.session.current_house = Some(other);
        other_current.announce_bridge_repair(owner, (7, 8), (8, 8));
        assert!(
            matches!(
                other_current.sound_events.last(),
                Some(SimSoundEvent::BridgeRepaired { radar: None, .. })
            ),
            "the spatial repair sound remains even when the native radar/EVA gate refuses"
        );
        assert_eq!(restored.state_hash(), other_current.state_hash());
        assert_eq!(restored.rng_state(), other_current.rng_state());
    }

    #[test]
    fn offline_ai_does_not_take_the_human_repair_radar_dedup_slot() {
        let mut sim = Simulation::new();
        let ai = sim.interner.intern("Russians");
        let player = sim.interner.intern("Americans");
        for owner in [ai, player] {
            sim.houses.insert(
                owner,
                crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 0),
            );
        }
        sim.houses.get_mut(&player).unwrap().player_control = true;
        sim.announce_bridge_repair(ai, (7, 8), (8, 8));
        assert!(
            matches!(
                sim.sound_events.last(),
                Some(SimSoundEvent::BridgeRepaired { radar: None, .. })
            ),
            "an AI repair never reaches CreateRadarEvent, so it cannot dedupe the human's"
        );
        sim.announce_bridge_repair(player, (7, 8), (8, 8));
        assert!(matches!(
            sim.sound_events.last(),
            Some(SimSoundEvent::BridgeRepaired { radar: Some(_), .. })
        ));
    }
}
