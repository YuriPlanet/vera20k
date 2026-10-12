//! Order-intent tick systems for the Simulation.
//!
//! Handles automatic target acquisition for attack-move and guard orders
//! (pre-combat), and resuming movement after combat ends (post-combat).
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use crate::sim::world::FrameEffects;
use std::collections::BTreeSet;

use super::{GroundMove, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat;
use crate::sim::components::OrderIntent;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

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

/// Unit73FD7C..73FE43's healing-weapon post-processing of an object action.
/// Move is native action2: the barred move cursor still reaches the ordinary
/// object-coordinate Move receiver when the unit can move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitRepairAction {
    Repair,
    Select,
    Move,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BuildingEntryResult {
    pub(crate) bridge_state_changed: bool,
    pub(crate) return_before_foot: bool,
}

#[cfg(test)]
#[path = "bridge_engineer_admission_tests.rs"]
mod bridge_engineer_admission_tests;

#[cfg(test)]
#[path = "building_control_tests.rs"]
mod building_control_tests;

/// The infiltration arms' type test (`0x0051EE4E..0x0051EEE7`,
/// `0x0051B2BD..0x0051B330`): an `Infiltrate=` infantryman (`+0xEBE`) may
/// enter a `Spyable=` building (`+0x1576`) if it is an `Agent=` one
/// (`+0xEC4`), else a `Capturable=` one (`+0x1572`).
fn infiltrator_admits(
    infantry: &crate::rules::object_type::ObjectType,
    building: &crate::rules::object_type::ObjectType,
) -> bool {
    infantry.infiltrate
        && if infantry.agent {
            building.spyable
        } else {
            building.capturable
        }
}

impl Simulation {
    /// UnitClass::What_Action_OnObject73FD7C..73FE43, after the base self/
    /// Shift action. Reproducible original control/weapon/health/display
    /// and ClickedAction witnesses: spatial_oracle/bunker_attack_action.json
    /// repair_rows (Steam gamemd 3e81a617). This is producer-side admission;
    /// a delivered Attack event must not repeat it after cargo/health changes.
    pub(crate) fn unit_repair_action(
        &self,
        actor_id: u64,
        target_id: u64,
        issuing_house: InternedId,
        rules: &RuleSet,
    ) -> Option<UnitRepairAction> {
        let actor = self.substrate.entities.get(actor_id)?;
        let actor_type = self.object_type(actor.type_ref(), rules)?;
        if actor.category != EntityCategory::Unit
            || actor.owner() != issuing_house
            || !self.house_is_human_player(actor.owner())
            || combat::combat_weapon::weapon_damage_value(actor, actor_type, rules) >= 0
        {
            return None;
        }
        let target = self.substrate.entities.get(target_id)?;
        let target_type = self.object_type(target.type_ref(), rules)?;
        if !crate::map::houses::is_allied_with(
            &self.house_alliances,
            self.interner.resolve(actor.owner()),
            self.interner.resolve(target.owner()),
        ) {
            return Some(UnitRepairAction::Move);
        }
        // Target vt+80: Unit746400, Building457620 ->465D40, and
        // Aircraft41B910 -> Object's grounded predicate. Every GameEntity is
        // a Techno (vt+7C); Infantry's vt+80 answers false.
        let vehicle = match target.category {
            EntityCategory::Unit => !target_type.non_vehicle,
            EntityCategory::Structure => target_type.is_1x1_with_undeploy(),
            EntityCategory::Aircraft => {
                if let Some(terrain) = self.resolved_terrain.as_ref() {
                    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
                    crate::sim::movement::air_movement::is_low_flying_in_query(
                        target,
                        &cells,
                        Some((rules, &self.interner)),
                    )
                } else {
                    crate::sim::movement::air_movement::is_low_flying(
                        target,
                        None,
                        Some((rules, &self.interner)),
                    )
                }
            }
            EntityCategory::Infantry => false,
        };
        if !vehicle || target_id == actor_id {
            //73FE2F..73FE43 preserves the base action for a Building.
            return (target.category != EntityCategory::Structure)
                .then_some(UnitRepairAction::Select);
        }
        if target.category == EntityCategory::Aircraft {
            //73FDE1..73FE06: a grounded aircraft on a building is excluded.
            let coord = crate::sim::movement::ground_pose::position_world_coord(&target.position);
            let cell = self.resolved_terrain.as_ref().and_then(|terrain| {
                let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
                match cells.lookup_world(coord.x, coord.y) {
                    crate::map::cell_index::NativeCellIdentity::Real(index) => {
                        let cell = &terrain.cells()[index];
                        Some((cell.rx, cell.ry))
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => None,
                }
            });
            if cell.is_some_and(|(rx, ry)| {
                self.substrate
                    .occupancy
                    .first_building_on_layer(rx, ry, MovementLayer::Ground)
                    .is_some()
            }) {
                return None;
            }
        }
        // Rules+16F8 is the fixed 1.0 threshold, not an INI condition key.
        (!target.health.is_full(target_type.strength)).then_some(UnitRepairAction::Repair)
    }

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

    /// Virtual+A0: Building44F5C0 or Unit/Infantry/Aircraft Techno700C40.
    /// This is producer-side control admission; Event/lifecycle admission is separate.
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
        let Some(object) = self.object_type(actor.type_ref(), rules) else {
            return false;
        };
        if actor.category == EntityCategory::Structure {
            // Steam gamemd 3e81a617: Building44F5C0, complete original control
            // vectors in procedural_drawing_oracle/factory_destination.py --control.
            // EMP+504 still has no runtime producer; this is its ctor-zero subset.
            // HasRallyPoint455DA0 bypasses the ConYard and House50B730 tests.
            if object.has_rally_line() {
                return true;
            }
            if object.construction_yard
                && !(self.owner_is_human(actor.owner())
                    && self.session.game_mode_nonzero
                    && self.session.game_options.mcv_redeploy
                    && actor.mind_control.controller().is_none())
            {
                return false;
            }
            return crate::sim::production::undeploy_target(rules, &object.id).is_some();
        }
        if !self.house_is_human_player(actor.owner()) || actor.bunker_link.installed_in().is_some()
        {
            return false;
        }
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

    /// `InfantryClass::What_Action_OnObject`'s infiltration arm
    /// (`0x0051EE3B..0x0051EEEF`): the local player's (`0x0050B6F0`)
    /// `Infiltrate=` infantryman over a building its own house is not allied
    /// to (`0x004F9A90`, one-way) that takes it — `Spyable=` for an `Agent=`
    /// one, `Capturable=` for any other — turns its Attack action (5) into
    /// Capture (9). Its base action is Attack there because
    /// `TechnoClass::What_Action_OnObject` keeps Attack despite an ILLEGAL
    /// fire error for exactly those flags (`0x007004A0..0x00700531`,
    /// `0x00700548`); that function's other gates are the input producer's
    /// hover classification and modifiers. The
    /// foundation-zone search that follows (`0x0051EEFA..0x0051F04B`) only
    /// leaves its loop: the action stays 9.
    pub(crate) fn infiltrator_building_action(
        &self,
        actor_id: u64,
        target_id: u64,
        rules: &RuleSet,
    ) -> bool {
        let (Some(actor), Some(target)) = (
            self.substrate.entities.get(actor_id),
            self.substrate.entities.get(target_id),
        ) else {
            return false;
        };
        if actor.category != EntityCategory::Infantry
            || target.category != EntityCategory::Structure
            || !self.house_is_human_player(actor.owner())
        {
            return false;
        }
        let (Some(infantry), Some(building)) = (
            self.object_type(actor.type_ref(), rules),
            self.object_type(target.type_ref(), rules),
        ) else {
            return false;
        };
        infiltrator_admits(infantry, building)
            && !crate::map::houses::is_allied_with(
                &self.house_alliances,
                self.interner.resolve(actor.owner()),
                self.interner.resolve(target.owner()),
            )
    }

    /// `InfantryClass::Assign_Target`'s infiltration arm
    /// (`0x0051B2B1..0x0051B33F`), after the Techno base write: the building
    /// an `Infiltrate=` infantryman with no NavCom (`+0x5A4`) heads for when
    /// handed `target` (vt+0x480(target, 1)), if [`infiltrator_admits`] it.
    /// Unlike the click's arm ([`Self::infiltrator_building_action`]), it asks
    /// neither the owner's control nor alliance. Every path through the
    /// setter's tail then clears `+0x68E`, a re-acquisition latch VERA does
    /// not keep (`mission_handlers`, GSI-07.06).
    pub(crate) fn infiltrator_target_building(
        &self,
        actor_id: u64,
        target: Option<crate::sim::combat::TargetKind>,
        rules: &RuleSet,
    ) -> Option<u64> {
        let Some(crate::sim::combat::TargetKind::Entity(building_id)) = target else {
            return None;
        };
        let actor = self.substrate.entities.get(actor_id)?;
        let building = self.substrate.entities.get(building_id)?;
        if actor.category != EntityCategory::Infantry
            || actor.navigation.nav_com.is_some()
            || building.category != EntityCategory::Structure
        {
            return None;
        }
        infiltrator_admits(
            self.object_type(actor.type_ref(), rules)?,
            self.object_type(building.type_ref(), rules)?,
        )
        .then_some(building_id)
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
            overlay_registry,
            FrameEffects::default(),
        );
    }

    /// Post-combat: entities with an OrderIntent but no active attack or movement
    /// resume their patrol/guard movement toward the original goal. The resume
    /// coords stay on `OrderIntent` — the `mission` substrate has no goal field
    /// yet (Slice-8 follow-up); only the busy-signalling role moved off it.
    #[cfg(test)]
    pub(crate) fn tick_order_intents_post_combat(&mut self, rules: Option<&RuleSet>) {
        // This compatibility fixture has no live overlay registry.
        self.tick_order_intents_post_combat_except(
            rules,
            &BTreeSet::new(),
            None,
            crate::sim::world::FrameEffects::default(),
        );
    }

    pub(crate) fn tick_order_intents_post_combat_except(
        &mut self,
        rules: Option<&RuleSet>,
        turn_suppressed: &BTreeSet<u64>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
                let _ = self.issue_air_cell_destination(
                    stable_id,
                    (goal_rx, goal_ry),
                    speed,
                    rules,
                    frame_effects,
                );
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
                    frame_effects,
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

    /// Ordinary Infantry PerCell2 building receiver (`0x00519B17..0x0051A06E`):
    /// an Engineer's capture, repair or bridge repair, or
    /// [`Self::infantry_per_cell_infiltrator_entry`]. The active object cursor
    /// owns removal/next-object cadence; there is no second sorted repair pass.
    pub(crate) fn infantry_per_cell_building_entry(
        &mut self,
        engineer_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<BuildingEntryResult, super::FrameAdvanceError> {
        let Some(engineer) = self.substrate.entities.get(engineer_id) else {
            return Ok(BuildingEntryResult::default());
        };
        if engineer.category != EntityCategory::Infantry {
            return Ok(BuildingEntryResult::default());
        }
        let InfantryPerCellBuildingAdmission::GroundBuilding(building_id) =
            self.infantry_per_cell_building_admission(engineer, rules)
        else {
            return Ok(BuildingEntryResult::default());
        };
        //519B3E's optional hut Tag event1 precedes this type gate. The live
        //Tag receiver remains a separate required chain; ordinary Hills has
        //no attached hut/Engineer Tag.519B58 tests Engineer before hut type.
        let Some(infantry_type) = self.object_type(engineer.type_ref(), rules) else {
            return Ok(BuildingEntryResult::default());
        };
        if !infantry_type.engineer {
            return self.infantry_per_cell_infiltrator_entry(
                engineer_id,
                building_id,
                infantry_type.agent,
                rules,
                registry,
                frame_effects,
            );
        }
        let cell = (engineer.position.rx, engineer.position.ry);
        let owner = engineer.owner();
        let Some(building) = self.substrate.entities.get(building_id) else {
            return Ok(BuildingEntryResult::default());
        };
        let Some(object) = self.object_type(building.type_ref(), rules) else {
            return Ok(BuildingEntryResult::default());
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
                //519FB9/519EBB/519EFC, then an immediate return before
                //FootPerCell.
                self.turn_away_from_building(
                    engineer_id,
                    building_id,
                    rules,
                    registry,
                    frame_effects,
                )?;
                return Ok(BuildingEntryResult {
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
                self.change_owner_with_rules(building_id, owner, rules, registry, frame_effects);
                if let Some(building) = self.substrate.entities.get_mut(building_id) {
                    building.record_infantry_capture_type(native_type_index);
                }
                changed =
                    self.scatter_building_infantry(building_id, rules, registry, frame_effects)?;
            }
            //519EAACapturable=false consumes too.519FF0 repair and519F9A
            //capture join Tag event48 then virtualUnInit, with no Foot tail.
            self.uninit_with_context(
                engineer_id,
                super::UninitContext::new(Some(rules), registry).with_effects(frame_effects),
            );
            return Ok(BuildingEntryResult {
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
            frame_effects,
        )
        .map_err(|cause| {
            super::FrameAdvanceError::new(
                self.session.tick,
                self.session.binary_frame,
                engineer_id,
                cause,
            )
        })?;
        //519D17..519D36 descends Infantry's registry with +28(hut,false).
        //Clearing NavCom does not stop a retained Walk head/destination.
        self.expire_infantry_bridge_hut_targets(building_id, rules, registry, frame_effects);
        changed |= self.scatter_building_infantry(building_id, rules, registry, frame_effects)?;
        // Attached Tag6E53A0 remains a separate synchronous receiver boundary.
        self.uninit_with_context(
            engineer_id,
            super::UninitContext::new(Some(rules), registry).with_effects(frame_effects),
        );
        Ok(BuildingEntryResult {
            bridge_state_changed: changed,
            return_before_foot: true,
        })
    }

    /// The PerCell2 building receiver's arm for an infantryman that is not
    /// an Engineer (`0x00519B66` → `0x00519FF8..0x0051A06E`); both ends
    /// return before the Foot body.
    ///
    /// An `Agent=` one runs `BuildingClass::Infiltrate` with its house
    /// (`0x0051A002..0x0051A00B`, [`crate::sim::spy_infiltration`]) and
    /// leaves the game as an engineer does: the attached Tag's event 0x30,
    /// then vt+0xF8 UnInit (`0x0051A010..0x0051A02E`). Any other drops its
    /// destination (vt+0x480(NULL, 1)) and scatters from the building's
    /// GetCoords (vt+0x174(coord, 1, 1); `0x0051A03E..0x0051A061`).
    ///
    /// RESIDUAL: no Tag event is raised (`TagClass::Raise_Event`,
    /// `0x006E53A0`). The receiver gives the building's Tag event 1 ("entered
    /// by", `0x00519B3E..0x00519B53`) before its Engineer test, and an
    /// `Agent=` spy's own Tag event 0x30 after the infiltration
    /// (`0x0051A010..0x0051A025`); the live Tag dispatcher
    /// (`sim::trigger_runtime`) has no producer in Per_Cell_Process. Trigger:
    /// a map Tag on the building or on the spy. Effect: its trigger does not
    /// spring. A building or spy a house builds has no Tag.
    fn infantry_per_cell_infiltrator_entry(
        &mut self,
        infantry_id: u64,
        building_id: u64,
        agent: bool,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<BuildingEntryResult, super::FrameAdvanceError> {
        let entered = BuildingEntryResult {
            return_before_foot: true,
            ..Default::default()
        };
        if agent {
            if let Some(owner) = self
                .substrate
                .entities
                .get(infantry_id)
                .map(|infantry| infantry.owner())
            {
                self.infiltrate_building(building_id, owner, rules);
            }
            self.uninit_with_context(
                infantry_id,
                super::UninitContext::new(Some(rules), registry).with_effects(frame_effects),
            );
            return Ok(entered);
        }
        self.turn_away_from_building(infantry_id, building_id, rules, registry, frame_effects)?;
        Ok(entered)
    }

    /// How PerCell2 turns an infantryman away from a building: the occupier
    /// arm (`0x005196DD..0x00519700`) and the building receiver
    /// (`0x00519EBB`, `0x00519EFC`, `0x00519FB9`, `0x0051A03E..0x0051A061`).
    /// SetDestination(NULL, 1) (vt+0x480), then
    /// [`Self::scatter_from_building`].
    pub(crate) fn turn_away_from_building(
        &mut self,
        infantry_id: u64,
        building_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), super::FrameAdvanceError> {
        self.assign_null_destination(infantry_id, Some(rules), registry, frame_effects);
        self.scatter_from_building(infantry_id, building_id, rules, registry, frame_effects)
    }

    /// PerCell2's Scatter from a building's real GetCoords (vt+0x174(coord,
    /// 1, 1)), which does not Process. Its heading away from the building
    /// draws one Scenario `RandomRanged(0, 4)` (`0x0051D258..0x0051D2D4`).
    pub(crate) fn scatter_from_building(
        &mut self,
        infantry_id: u64,
        building_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), super::FrameAdvanceError> {
        let Some(coord) = self.substrate.entities.get(building_id).map(|building| {
            crate::sim::movement::ground_pose::object_get_coords(
                building,
                self.resolved_terrain.as_ref(),
            )
        }) else {
            return Ok(());
        };
        self.infantry_scatter_from(
            infantry_id,
            (coord.x, coord.y),
            crate::sim::movement::ScatterFlags::new(true, true),
            rules,
            registry,
            frame_effects,
        )
        .map_err(|cause| {
            super::FrameAdvanceError::new(
                self.session.tick,
                self.session.binary_frame,
                infantry_id,
                cause,
            )
        })
    }

    /// BuildingClass::Update's shared C4/PostMortem expiry tail
    /// (`0x004401D2..0x00440358`). Called from the current Structure
    /// LogicVector visit, so the forced receiver and any nested DeathWeapon
    /// complete before the next live object is visited. Once the C4Timer
    /// runs out:
    /// - A bridge repair hut (`0x00440213..0x00440331`) collapses its bridge
    ///   (`0x00574000`, or the wooden `0x00574C20`; `bridge_orchestrator`)
    ///   and lets its charge go (`+0x6DF`, C4AppliedBy `+0x540`). The hut
    ///   takes no damage.
    /// - Any other building takes its own health as `C4Warhead=` damage
    ///   from C4AppliedBy, ignoring defenses (`0x00440333..0x00440358`), and
    ///   keeps the charge.
    ///
    /// Answers whether a bridge changed.
    pub(crate) fn tick_pending_building_detonation(
        &mut self,
        building_id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some((pending, health, bridge_hut, cell)) = self
            .substrate
            .entities
            .get(building_id)
            .and_then(|building| {
                let pending = building.pending_c4_detonation?;
                let object = rules.object(self.interner.resolve(building.type_ref()))?;
                Some((
                    pending,
                    building.health.current,
                    object.bridge_repair_hut,
                    (building.position.rx, building.position.ry),
                ))
            })
        else {
            return false;
        };
        if !pending.timer.expired(self.session.binary_frame as i32) || health <= 0 {
            return false;
        }

        if bridge_hut {
            let changed = crate::sim::world::bridge_orchestrator::dispatch_bridge_collapse_from_hut_with_overlay_registry(
                self,
                rules,
                cell,
                overlay_registry,
                frame_effects,
            );
            if let Some(hut) = self.substrate.entities.get_mut(building_id) {
                hut.pending_c4_detonation = None;
            }
            return changed;
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
        self.commit_noncombat_aoe_hits(rules, overlay_registry, &[event], frame_effects);
        false
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
        self.tick_attack_pursuit_with_overlay_registry(
            rules,
            None,
            &BTreeSet::new(),
            crate::sim::world::FrameEffects::default(),
        );
    }

    pub(crate) fn tick_attack_pursuit_with_overlay_registry(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        turn_suppressed: &BTreeSet<u64>,
        frame_effects: FrameEffects<'_>,
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
            // Unit AI commences a ready queued mission (`0x00736465`/
            // `0x00736473`, Infantry `0x0051BC51`) before FootClass::AI
            // (`0x0073647B`, `0x0051BC9F`) runs the mission handler and then
            // the locomotor, so neither Mission_Attack's approach nor the
            // per-cell range stop sees that object: the Move a computer
            // crusher's damage response queues (`0x007385A1`) keeps the
            // destination it set on the object that hit it.
            // RESIDUAL: once that Move commences, this pass still takes the
            // unit, which holds the attacker as its retaliation target:
            // within range it drops the MovementTarget adapter, which the
            // track host restores toward the NavCom (`complete_pending_order`;
            // `range_stop_admits` refuses Move, so the NavCom stays), where
            // Mission_Move has no approach or stop. The charge completes.
            if entity.mission.queued() != MissionId::NONE
                && self.mission_ready_to_commence(id, rules)
            {
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
                        frame_effects,
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
                    // temporal weapon. Infantry, open-topped units and the
                    // Drive, Ship and Hover units whose mission's Approach
                    // carries them take the per-cell owner's stop
                    // (`movement/per_cell.rs`).
                    // RESIDUAL: for any other unit on these missions but a
                    // Jumpjet (a Teleport mover or a `CloseRange=` type; no
                    // armed stock unit), the native stop runs from
                    // Per_Cell_Process, at cell entry; this pass runs it on
                    // the frame the target comes in range, wherever the unit
                    // is in its cell. It then brakes into its head from
                    // there, and a turretless one turns to fire only once
                    // stopped (`0x00736FE1`).
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
                                frame_effects,
                            );
                        } else {
                            self.set_unit_null_destination(
                                entity_id,
                                Some(rules),
                                None,
                                frame_effects,
                            );
                        }
                    }
                }
                PursuitAction::DropTargetAndMovement { entity_id } => {
                    // Foot4D5730 dispatches virtual+3C8 on Sticky refusal,
                    // then the class NULL destination (0x004D573E).
                    let _ = self.assign_target_represented(
                        entity_id,
                        None,
                        Some(rules),
                        overlay_registry,
                        frame_effects,
                    );
                    self.assign_null_destination(entity_id, Some(rules), None, frame_effects);
                    if let Some(e) = self.substrate.entities.get_mut(entity_id) {
                        e.movement_target = None;
                    }
                }
            }
        }
    }
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
