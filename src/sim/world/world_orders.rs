//! Order-intent tick systems for the Simulation.
//!
//! Handles automatic target acquisition for attack-move and guard orders
//! (pre-combat), and resuming movement after combat ends (post-combat).
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use std::collections::BTreeSet;

use super::{GroundMove, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat;
use crate::sim::components::OrderIntent;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::direction_tables::CELL_DELTAS;
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

/// Interior PerCell519948..519B58 result, before Engineer/hut type checks.
/// The one-cell Undeploy target enters a separate conversion receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfantryPerCellBuildingAdmission {
    MissionNotAdmitted,
    NoMatchingGroundBuilding,
    UndeployBuilding,
    GroundBuilding(u64),
}

/// The native Engineer object-action arm. Input and cursor consumers share
/// this result rather than independently guessing from relation or health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EngineerBuildingAction {
    Capture,
    Damage,
    Repair(bool),
    EnterHospital,
    EnterGrinder,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EngineerEntryResult {
    pub(crate) bridge_state_changed: bool,
    pub(crate) return_before_foot: bool,
}

#[cfg(test)]
#[path = "bridge_engineer_admission_tests.rs"]
mod bridge_engineer_admission_tests;

impl Simulation {
    pub(crate) fn house_is_human_player(&self, owner: InternedId) -> bool {
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

    /// Unit/Infantry/Aircraft virtual+A0, original Techno700C40 IsControllable. This is
    /// producer-side control admission; Event/lifecycle admission is separate.
    /// House50B6F0, bunker link, Spawned, paralysis, warp, slave ownership and
    /// launched-missile count remain with their existing authoritative owners.
    ///
    /// Ordinary Infantry +504 EMP and +1C8 Robot-offline start zero in the
    /// native constructor. EMPulseApply4C575E admits RTTI1/2, not Infantry15;
    /// Robot House writers scan all Techno but compare type pointers with
    /// PowersUnit+40C, read by7132DF..713309 through UnitType7480D0.
    /// Its UnitType pointer cannot match an ordinary InfantryType; the other
    /// established writer is UnitPerCell739F59. This is not a generic EMP or
    /// Robot implementation and makes no unreachable claim for50E1C0.
    /// Unit+6D8 death-frame and Robot/EMP lifecycle producers remain separate
    /// residuals; ordinary live Unit controls use their constructor -1/0/0.
    /// Arbitrary authored native raw-save bytes are outside this producer.
    /// See engineer_bridge_cursor_caller.md for executed gate controls/bounds.
    pub(crate) fn techno_player_controllable(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        if actor.category == EntityCategory::Structure
            || !self.house_is_human_player(actor.owner())
            || actor.bunker_link.installed_in().is_some()
        {
            return false;
        }
        let Some(object) = self.object_type(actor.type_ref(), rules) else {
            return false;
        };
        if object.spawned
            || actor.is_paralyzed(self.session.binary_frame)
            || actor.is_warped_out()
            || actor.is_warping_in()
            || actor.slave.owner().is_some()
        {
            return false;
        }
        if let Some(manager) = actor.spawn_manager.as_ref() {
            let count =
                manager.count_launched_missiles(&self.substrate.entities, rules, &self.interner)
                    as i32;
            if count > 0 && count < object.spawns_number {
                return false;
            }
        }
        true
    }

    /// Object-route Engineer action in Infantry51E49E..51E637. Ordinary
    /// action/threshold vectors: engineer_repair_joined; hut caller/display
    /// vectors: engineer_bridge_cursor_caller; shared geometric query:
    /// bridge_repair_query. Repair(false) is terminal action32.
    /// Admission belongs to the input producer, before a delayed mission event.
    pub(crate) fn engineer_building_action(
        &self,
        actor_id: u64,
        target_id: u64,
        rules: &RuleSet,
    ) -> Option<EngineerBuildingAction> {
        let actor = self.substrate.entities.get(actor_id)?;
        if actor.category != EntityCategory::Infantry
            || !self.object_type(actor.type_ref(), rules)?.engineer
            || !self.house_is_human_player(actor.owner())
        {
            return None;
        }
        let target = self.substrate.entities.get(target_id)?;
        if target.category != EntityCategory::Structure {
            return None;
        }
        let object = self.object_type(target.type_ref(), rules)?;
        if object.is_1x1_with_undeploy() || !object.repairable {
            return None;
        }
        if !object.bridge_repair_hut {
            let allied = crate::map::houses::is_allied_with(
                &self.house_alliances,
                self.interner.resolve(actor.owner()),
                self.interner.resolve(target.owner()),
            );
            let passive_occupation = self
                .houses
                .get(&target.owner())
                .is_some_and(|house| house.multiplay_passive)
                && object.can_be_occupied
                && !target.is_warped_out();
            if !allied && !passive_occupation {
                if !object.capturable {
                    return None;
                }
                //51E5C6 compares threshold < health ratio (C0), not the
                //opposite. Equality returns Capture; masked NaN keeps C0=1.
                use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering};
                return Some(
                    if matches!(
                        X87::compare(
                            X87::load_f32(rules.general.engineer_capture_level),
                            target.health.ratio(object.strength),
                        ),
                        MaskedX87Ordering::Less | MaskedX87Ordering::Unordered
                    ) {
                        EngineerBuildingAction::Damage
                    } else {
                        EngineerBuildingAction::Capture
                    },
                );
            }
            //ConditionGreen is the native forced1.0, not an authored key.
            if object.hospital
                && !actor
                    .health
                    .is_full(self.object_type(actor.type_ref(), rules)?.strength)
            {
                return Some(EngineerBuildingAction::EnterHospital);
            }
            let full = matches!(
                target.health.compare_ratio(object.strength, 1.0),
                crate::util::native_x87::MaskedX87Ordering::Equal
                    | crate::util::native_x87::MaskedX87Ordering::Unordered
            );
            return Some(if !full {
                EngineerBuildingAction::Repair(true)
            } else if object.grinding {
                EngineerBuildingAction::EnterGrinder
            } else {
                EngineerBuildingAction::Repair(false)
            });
        }
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            target,
            self.resolved_terrain.as_ref(),
        );
        //51E52F..51E547: signed truncation by256 then packed-word narrowing.
        Some(EngineerBuildingAction::Repair(
            crate::sim::world::bridge_orchestrator::bridge_hut_can_repair(
                self,
                ((coord.x / 256) as u16, (coord.y / 256) as u16),
            ),
        ))
    }

    fn announce_bridge_repair(
        &mut self,
        owner: InternedId,
        cell: (u16, u16),
        building_cell: (u16, u16),
    ) {
        let radar = self.house_is_human_player(owner).then(|| {
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

    /// Remaining Aircraft/Structure compatibility host. Units and Infantry
    /// acquire in their own Techno targeting slot, before locomotion and fire.
    pub(crate) fn tick_order_intents_pre_combat(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
    ) {
        for id in self.substrate.entities.keys_sorted() {
            if turn_suppressed.contains(&id)
                || self.substrate.entities.get(id).is_none_or(|entity| {
                    matches!(
                        entity.category,
                        EntityCategory::Unit | EntityCategory::Infantry
                    )
                })
            {
                continue;
            }
            self.acquire_order_intent_target_one(id, rules, overlay_registry);
        }
    }

    /// Existing OrderIntent acquisition decision, shared by the Unit/Infantry
    /// targeting slots and the remaining compatibility host.
    /// Native caller ordering: TechnoAI6FA65A..6FA695 reaches retained
    /// AttackMove acquisition before Foot locomotion and Unit7365E1 or
    /// Infantry51BF59 fire.
    ///
    /// RESIDUAL: this preserves VERA's legacy AttackMove/Guard semantics. It
    /// scans every eligible object visit with mask1/2, without the native
    /// targeting-timer/4DF3A0 engagement gates or scanner jitter draw. Native
    /// +5C4/+5C8/+5CC/+5D1 setup, resume and pointer-expiry state is not yet
    /// represented; the existing OrderIntent is serialized but not hashed. Trigger:
    /// player AttackMove/Guard with no held target; effect: acquisition and
    /// subsequent fire/resume cadence can differ. Evidence for that separate
    /// mechanism: tools/spatial_oracle/foot_attack_move.py and target_scan.rs.
    pub(super) fn acquire_order_intent_target_one(
        &mut self,
        attacker_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(attacker_id) else {
            return;
        };
        // A warped object's missions do not run, nor does a Health-0 wreck's
        // (MissionClass5B30A7). Keep the existing legacy decision unchanged.
        if entity.order_intent.is_none()
            || entity.attack_target.is_some()
            || entity.ai_frozen()
            || !super::techno_ai::mission_handlers_run(self, attacker_id)
        {
            return;
        }
        let scan_mask = combat::scan_mission_for(entity);
        let Some(target_sid) = self.greatest_threat_represented(
            rules,
            overlay_registry,
            attacker_id,
            scan_mask,
            None,
            combat::acquire_best_target_for_entity,
        ) else {
            return;
        };
        let _ = self.assign_target_represented(
            attacker_id,
            Some(combat::TargetKind::Entity(target_sid)),
            Some(rules),
        );
    }

    /// Post-combat: entities with an OrderIntent but no active attack or movement
    /// resume their patrol/guard movement toward the original goal. The resume
    /// coords stay on `OrderIntent` — the `mission` substrate has no goal field
    /// yet (Slice-8 follow-up); only the busy-signalling role moved off it.
    #[cfg(test)]
    pub(crate) fn tick_order_intents_post_combat(&mut self, rules: Option<&RuleSet>) {
        // This compatibility fixture has no live overlay registry.
        self.tick_order_intents_post_combat_except(rules, &BTreeSet::new(), None);
    }

    pub(crate) fn tick_order_intents_post_combat_except(
        &mut self,
        rules: Option<&RuleSet>,
        turn_suppressed: &BTreeSet<u64>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        // Without a published grid no order resumes (air resumes included).
        if self.path_grid().is_none() {
            return;
        }
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
                    || crate::sim::movement::air_movement::fly_moving(entity)
                    || entity.ai_frozen()
                    || !super::techno_ai::mission_handlers_run(self, id)
                {
                    continue;
                }
                match intent {
                    OrderIntent::AttackMove { goal_rx, goal_ry }
                        if (entity.position.rx, entity.position.ry) != (goal_rx, goal_ry) =>
                    {
                        resumes.push((id, goal_rx, goal_ry));
                    }
                    _ => {}
                }
            }
        }

        for (stable_id, goal_rx, goal_ry) in resumes {
            let (speed, is_air) = self
                .substrate
                .entities
                .get(stable_id)
                .map(|e| {
                    // A resumed order re-queries the getter like any other.
                    let obj = rules.and_then(|r| self.object_type(e.type_ref(), r));
                    let air: bool = e
                        .locomotor
                        .as_ref()
                        .is_some_and(|l| l.layer == MovementLayer::Air);
                    (
                        crate::sim::movement::order_speed(e, obj, rules, &self.houses),
                        air,
                    )
                })
                .unwrap_or((ra2_speed_to_leptons_per_second(4), false));

            if is_air {
                let _ =
                    self.issue_air_cell_destination(stable_id, (goal_rx, goal_ry), speed, rules);
            } else {
                let _ = self.issue_ground_move(
                    GroundMove {
                        entity_id: stable_id,
                        target: (goal_rx, goal_ry),
                        speed,
                        queue: false,
                        speed_type: None,
                        owner_blocks: false,
                        object_destination: None,
                    },
                    rules,
                    overlay_registry,
                );
            }
        }
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
        if !(self.owner_is_human(old_owner) || self.owner_is_human(new_owner)) {
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

    /// Original519948 uses Mission+184 (5B3040), then Building+80, then the
    /// first GROUND Building47C520, then NavCom/attack identity. This is the
    /// ordinary active-frame prefix; native A8E9A0=false loading/termination
    /// queries are not represented by OccupancyGrid. Native cases and bounds:
    /// tools/spatial_oracle/engineer_repair_admission.{py,json,meta.json}.
    fn infantry_per_cell_building_admission(
        &self,
        infantry: &crate::sim::game_entity::GameEntity,
        rules: &RuleSet,
    ) -> InfantryPerCellBuildingAdmission {
        use crate::sim::components::NavTargetRef;
        if !matches!(
            infantry.mission.effective().known(),
            Some(MissionType::Capture | MissionType::AreaGuard | MissionType::Patrol)
        ) {
            return InfantryPerCellBuildingAdmission::MissionNotAdmitted;
        }
        let nav_object = match infantry.navigation.nav_com {
            Some(
                NavTargetRef::Entity { id }
                | NavTargetRef::Object { id }
                | NavTargetRef::Building { id },
            ) => Some(id),
            _ => None,
        };
        //519987 tests Techno RTTI bit1, not cell_marked. Building+80 is
        //457620 ->465D40;5199B8 also requires Building RTTI6 before the
        //separate5199A6 conversion/destination continuation can proceed,
        //which must not be treated as ordinary bridge repair. Stock CABHUT
        //has no UndeploysInto. One-cell building conversion remains a separate mechanism; ordinary
        //Engineer entry below shares this admitted ground-building identity.
        if let Some(target) = nav_object.and_then(|id| self.substrate.entities.get(id))
            && target.category == EntityCategory::Structure
            && self
                .object_type(target.type_ref(), rules)
                .is_some_and(|object| object.is_1x1_with_undeploy())
        {
            return InfantryPerCellBuildingAdmission::UndeployBuilding;
        }
        let cell = (infantry.position.rx, infantry.position.ry);
        let Some(building_id) =
            self.substrate
                .occupancy
                .first_building_on_layer(cell.0, cell.1, MovementLayer::Ground)
        else {
            return InfantryPerCellBuildingAdmission::NoMatchingGroundBuilding;
        };
        if nav_object == Some(building_id)
            || infantry.attack_target.as_ref().is_some_and(|attack| {
                matches!(attack.target, crate::sim::combat::TargetKind::Entity(id) if id == building_id)
            })
        {
            InfantryPerCellBuildingAdmission::GroundBuilding(building_id)
        } else {
            InfantryPerCellBuildingAdmission::NoMatchingGroundBuilding
        }
    }

    /// Ordinary Infantry PerCell2 engineer receiver. The active object cursor
    /// owns removal/next-object cadence; there is no second sorted repair pass.
    pub(crate) fn infantry_per_cell_engineer_entry(
        &mut self,
        engineer_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<EngineerEntryResult, super::FrameAdvanceError> {
        let Some(engineer) = self.substrate.entities.get(engineer_id) else {
            return Ok(EngineerEntryResult::default());
        };
        if engineer.category != EntityCategory::Infantry {
            return Ok(EngineerEntryResult::default());
        }
        let InfantryPerCellBuildingAdmission::GroundBuilding(building_id) =
            self.infantry_per_cell_building_admission(engineer, rules)
        else {
            return Ok(EngineerEntryResult::default());
        };
        //519B3E's optional hut Tag event1 precedes this type gate. The live
        //Tag receiver remains a separate required chain; ordinary Hills has
        //no attached hut/Engineer Tag.519B58 tests Engineer before hut type.
        if !self
            .object_type(engineer.type_ref(), rules)
            .is_some_and(|t| t.engineer)
        {
            return Ok(EngineerEntryResult::default());
        }
        let cell = (engineer.position.rx, engineer.position.ry);
        let owner = engineer.owner();
        let Some(building) = self.substrate.entities.get(building_id) else {
            return Ok(EngineerEntryResult::default());
        };
        let Some(object) = self.object_type(building.type_ref(), rules) else {
            return Ok(EngineerEntryResult::default());
        };
        if !object.bridge_repair_hut {
            let allied = crate::map::houses::is_allied_with(
                &self.house_alliances,
                self.interner.resolve(owner),
                self.interner.resolve(building.owner()),
            );
            //519D6E's passive-occupation repair branch has no warp gate.
            let repair = allied
                || (object.can_be_occupied
                    && self
                        .houses
                        .get(&building.owner())
                        .is_some_and(|house| house.multiplay_passive));
            let full = building.health.current == object.strength;
            let refused_capture = building.mission.current().known() == Some(MissionType::Selling)
                || building.is_warped_out();
            let capturable = object.capturable;
            let native_type_index = rules
                .type_array_index(
                    crate::rules::object_type::ObjectCategory::Infantry,
                    self.interner.resolve(engineer.type_ref()),
                )
                .unwrap_or(-1);
            if (repair && full) || (!repair && capturable && refused_capture) {
                //519FB9/519EBB/519EFC: SetDestination(NULL,true), then
                //Scatter(real Building GetCoords,true,true), immediate return
                //before FootPerCell. The real-coordinate arm does not Process.
                let coord = crate::sim::movement::ground_pose::object_get_coords(
                    building,
                    self.resolved_terrain.as_ref(),
                );
                self.assign_null_destination(engineer_id, Some(rules), None);
                self.infantry_scatter_from(
                    engineer_id,
                    (coord.x, coord.y),
                    crate::sim::movement::ScatterFlags::new(true, true),
                    rules,
                    registry,
                )
                .map_err(|cause| {
                    super::FrameAdvanceError::bridge_repair(
                        self.session.tick,
                        self.session.binary_frame,
                        engineer_id,
                        cause,
                    )
                })?;
                return Ok(EngineerEntryResult {
                    return_before_foot: true,
                    ..Default::default()
                });
            }
            let mut changed = false;
            if repair {
                crate::sim::production::engineer_repair(self, rules, building_id);
            } else if capturable {
                //519DA1's MultiEngineer damage/C4Warhead branch remains a
                //separate required numeric/damage chain; stock ordinary single
                //Engineer admission (session option false) reaches this arm.
                //Attached Building/Engineer Tag events and transfer likewise
                //remain the trigger mechanism's synchronous residual.
                if let Some(old_house) = self.houses.get_mut(&building.owner()) {
                    old_house.notify_building_capture(); //519F4F, Trigger event3 latch
                }
                self.announce_engineer_capture(building_id, owner, rules);
                self.change_owner_with_rules(building_id, owner, rules, registry);
                if let Some(building) = self.substrate.entities.get_mut(building_id) {
                    building.record_infantry_capture_type(native_type_index);
                }
                changed = self.scatter_building_infantry(building_id, rules, registry)?;
            }
            //519EAACapturable=false consumes too.519FF0 repair and519F9A
            //capture join Tag event48 then virtualUnInit, with no Foot tail.
            self.uninit_with_context(
                engineer_id,
                super::UninitContext::new(Some(rules), registry),
            );
            return Ok(EngineerEntryResult {
                bridge_state_changed: changed,
                return_before_foot: true,
            });
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
        self.expire_infantry_bridge_hut_targets(building_id, rules, registry);
        changed |= self.scatter_building_infantry(building_id, rules, registry)?;
        // Attached Tag6E53A0 remains a separate synchronous receiver boundary.
        self.uninit_with_context(
            engineer_id,
            super::UninitContext::new(Some(rules), registry),
        );
        Ok(EngineerEntryResult {
            bridge_state_changed: changed,
            return_before_foot: true,
        })
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
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
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
            // to the target building. The walk reaches it because the building
            // is the NavCom, which the Infantry +1AC admits (`0x0051C2D3..`).
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
                continue; // walk-up still in progress
            }

            // Claim the plant.
            // RESIDUAL: native's planter then re-arms (`GameEntity::rearm_timer`)
            // with `GetROF(1)` (`InfantryClass::PerCellProcess
            // 0x0051A60B..0x0051A65F`), and so does a second planter entering an
            // already charged building (`0x0051A546..0x0051A5A0`), which VERA's
            // walk-up leaves hovering outside. Outside FireAt the burst index is
            // below `Burst=`, so `combat::rof::get_rof` takes its mid-burst arm:
            // one Scenario `RandomRanged(3, 5)` (an empty slot 1 returns 1
            // without a draw, `0x006FCFD4`). Trigger: every C4 plant. Effect: the
            // planter can fire up to 5 frames early, and one draw per plant is
            // missing. Belongs to the C4 plant mechanism.
            if let Some(b) = self.substrate.entities.get_mut(target_id) {
                b.pending_c4_detonation = Some(PendingC4Detonation {
                    timer: crate::sim::timer::CdTimer::started(
                        self.session.binary_frame as i32,
                        rules.c4_delay_ticks as i32,
                    ),
                    source_entity_id: Some(attacker_id),
                });
            }

            // Native PerCell51A60D does not request Attack Doing4; its
            // 0x004DF0D0 only zeroes NavCom and its auxiliary slot, so Walk
            // keeps its destination and head and its Process runs on.
            // RESIDUAL: the arm's Uncloak, ROF and forced Scatter
            // (0x0051D0D0) tail is its own mechanism. Infantry presentation
            // reads the retained Doing owner.
            if let Some(a) = self.substrate.entities.get_mut(attacker_id) {
                movement::foot_stop_moving(a);
                a.movement_target = None;
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

            if !pending.timer.expired(self.session.binary_frame as i32) {
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
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
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
        if !pending.timer.expired(self.session.binary_frame as i32) || health <= 0 {
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

    /// Post-detonation: any attacker that was on the destroyed building's
    /// cell with `c4_plant` targeting this building scatters one cell in a
    /// deterministic direction derived from the current tick. Matches gamemd
    /// `Mission_Enter` post-detonation block:
    /// `uVar13 = (tick >> 12 + 1) >> 1 & 7` → 1 of 8 directions via
    /// the direction-delta tables.
    ///
    /// Also clears each attacker's `c4_plant`.
    fn queue_c4_post_detonation_scatter(&mut self, dead_building_id: u64) {
        // Mirror the native-frame bit-twiddle: `(frame >> 12 + 1) >> 1 & 7`.
        // C operator precedence: `>>` is left-to-right at same level, so
        // this evaluates as `(((frame >> 12) + 1) >> 1) & 7`.
        let dir: usize = ((((self.session.binary_frame >> 12) + 1) >> 1) & 7) as usize;
        let (dx, dy) = CELL_DELTAS[dir];
        let (dx, dy) = (dx as i16, dy as i16);

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
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
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
            let _ = attacker_id; // the hut survives: nothing is credited
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
    /// - Aircraft, whose native mission handlers steer them
    ///   ([`crate::sim::aircraft::dispatch_mission`])
    /// - Deployed-fire infantry (locked while deployed)
    /// - Entities inside transports
    /// - Dying entities, and Health-0 wrecks, which run no mission
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
    pub(crate) fn tick_attack_pursuit(&mut self, rules: &RuleSet) {
        self.tick_attack_pursuit_with_overlay_registry(rules, None, &BTreeSet::new());
    }

    pub(crate) fn tick_attack_pursuit_with_overlay_registry(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
    ) {
        // Without a published grid pursuit decides nothing this tick.
        if self.path_grid().is_none() {
            return;
        }

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
            // The native mission owns the approaches VERA ports. InRange does
            // not clear its paid head or retained destination; FireAt can run
            // while the locomotor continues toward the chosen cell or the
            // Target.
            if self.mission_owns_approach(id, rules) {
                continue;
            }
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            let Some(attack) = entity.attack_target.as_ref() else {
                continue;
            };

            // Skip filters — see "Skips" doc above. The approach is Mission_
            // Attack's, which a Health-0 wreck does not run
            // (`MissionClass::AI @ 0x005B30A7`).
            if entity.dying || !super::techno_ai::mission_handlers_run(self, id) {
                continue;
            }
            if entity.passively_acquired_target {
                continue;
            }
            // `FootClass::Mission_Guard @ 0x004D5070` has no approach call
            // (vt+0x53C): an object committed to Guard fires at what it holds
            // from where it stands, whoever assigned it. The represented
            // Area Guard approaches use their mission owner above; ordinary
            // object pursuit remains a held movement residual. AttackMove
            // retains its separate order-intent adapter until that migration.
            if entity.mission.current().known() == Some(crate::sim::mission::MissionType::Guard)
                && entity.order_intent.is_none()
            {
                continue;
            }
            if entity.category == EntityCategory::Structure {
                continue;
            }
            if entity.category == EntityCategory::Aircraft {
                continue;
            }
            if entity.is_deployed() {
                continue;
            }
            // A passenger never walks: an open-topped transport's rider has
            // its chase off (`+0x82`, `0x004D5782`) and a destination refused
            // (`0x004D94D7`); its reach is `open_transport_reach_step`.
            if entity.passenger_role.is_inside_transport() {
                continue;
            }

            // Ground Infantry's native approach helper4D5690 returns at
            // 4D5A07..18 while NavCom is present. An accepted Walk path/head
            // continues through Process; FootPerCell(mode2)4D882F..896E makes
            // the range-stop decision after completion.
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
            let target_pos =
                combat::resolve_target_coords(&attack.target, &self.substrate.entities);
            let Some((trx, try_, _tsx, _tsy)) = target_pos else {
                continue;
            };

            // Resolve the weapon using the shared helper. None means the
            // selected slot names no weapon. Legality is not asked here, as
            // Approach_Target asks none: the fire routine's GetFireError and
            // the 16-frame check decide what happens to the target.
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
            // (`FootClass::Approach_Target @ 0x004D5690`, vt+0x53C,
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

            // `FootClass::Per_Cell_Process @ 0x004D885C`: an `OpenTopped=`
            // mover chasing a Foot target, on a mission the range stop admits,
            // stops only on entering a cell, by distance
            // (`foot_per_cell_range_stop`), never by InRange. A loaded Battle
            // Fortress drives on to its GIs' M60 range. On any other mission
            // (VERA's Guard and AttackMove orders among them) the stop never
            // fires, and the halt below stands in as for every other mover.
            let open_topped_chase = matches!(
                attack.target,
                combat::TargetKind::Entity(target_id)
                    if self.substrate.entities.get(target_id).is_some_and(|target| {
                        target.category != EntityCategory::Structure
                    })
            ) && movement::range_stop_admits(entity)
                && self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|obj| obj.open_topped);

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
                    // Out of range, no current pursuit — issue a path. A
                    // mover on Teleport warps to this cell (the target's
                    // own): see `teleport_move_to`'s residual.
                    actions.push(PursuitAction::IssueMove {
                        entity_id: id,
                        goal: (trx, try_),
                    });
                }
                // else: existing pursuit movement is still running; let it continue.
            } else if entity.movement_target.is_some() && !open_topped_chase {
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
                    let _issued = self.issue_ground_move(
                        GroundMove {
                            entity_id,
                            target: goal,
                            speed: info.speed,
                            queue: false,
                            speed_type: Some(info.speed_type),
                            owner_blocks: true,
                            object_destination: None,
                        },
                        Some(rules),
                        overlay_registry,
                    );
                    // No-op if A* fails — pursuit retries next tick.
                }
                PursuitAction::ClearMovement { entity_id } => {
                    let Some(e) = self.substrate.entities.get_mut(entity_id) else {
                        continue;
                    };
                    // A prior null destination (for example Stop) still
                    // leaves a paid Walk head active until its completion.
                    if !e.locomotor.as_ref().is_some_and(|loco| {
                        loco.kind == crate::rules::locomotor_type::LocomotorKind::Walk
                            && loco.step_head().is_some()
                    }) {
                        e.movement_target = None;
                    }
                    // The range stop in `FootClass::Per_Cell_Process`
                    // (`0x004D8920..0x004D8968`): under Rescue, Area Guard,
                    // Attack or Hunt with an empty NavQueue it calls the class
                    // SetDestination(NULL, 1), so a vehicle that stops to fire
                    // holds no NavCom. GetFireError's NavCom tests (U7..U10)
                    // would otherwise keep refusing a spark, flame, drain or
                    // temporal weapon. Infantry and open-topped units take
                    // the per-cell owner's stop (`movement/per_cell.rs`).
                    // RESIDUAL: the native stop runs from Per_Cell_Process,
                    // at cell entry; this pass runs it on the frame the
                    // target comes in range, wherever the vehicle is in its
                    // cell (other native stop paths unchecked). It then
                    // brakes into its head from there, and a turretless one
                    // turns to fire only once stopped (`0x00736FE1`), so its
                    // first shot comes an unmeasured number of frames off
                    // native. Every in-range stop of a pursuing vehicle.
                    // A cruising Jumpjet takes no such stop: Jumpjet calls
                    // Per_Cell_Process only from its descent (`0x0054C8F0`),
                    // and Foot's approach (`0x004D5690`) instead sends it to a
                    // cell in range, where it arrives. For it this halt stands
                    // in for that arrival, which never reaches the setter's
                    // BalloonHover arm (`0x00741983`); through the arm, a
                    // Floating Disc on its laser would keep flying at its
                    // target and switch to Attack.
                    if e.category == EntityCategory::Unit && movement::range_stop_admits(e) {
                        let jumpjet = e.locomotor.as_ref().is_some_and(|loco| {
                            loco.active_kind()
                                == crate::rules::locomotor_type::LocomotorKind::Jumpjet
                        });
                        if jumpjet {
                            self.unit_null_destination_past_balloon_arm(
                                entity_id,
                                Some(rules),
                                None,
                            );
                        } else {
                            self.set_unit_null_destination(entity_id, Some(rules), None);
                        }
                    }
                }
                PursuitAction::DropTargetAndMovement { entity_id } => {
                    // Foot4D5730 dispatches virtual+3C8 on Sticky refusal,
                    // then the class NULL destination (0x004D573E).
                    let _ = self.assign_target_represented(entity_id, None, Some(rules));
                    self.assign_null_destination(entity_id, Some(rules), None);
                    if let Some(e) = self.substrate.entities.get_mut(entity_id) {
                        e.movement_target = None;
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
