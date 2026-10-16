//! Order helpers for the Simulation: click-order receivers and actions, the
//! engineer and infiltrator building entries and pending building detonation.
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use crate::sim::world::FrameEffects;

use super::{SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement::locomotor::MovementLayer;

/// Techno `+0x338`: the native InfantryType index of the infantryman that
/// captured this object. The Techno constructor stores -1 (`0x006F2EC8`). An
/// engineer's capture stores its type after the owner change
/// (`InfantryClass::PerCellProcess`, `0x00519A42` and `0x00519F94`, one per
/// capture arm; [`Self::record`]). A vehicle thief's store
/// (`InfantryClass::TryThiefVehicleCapture`, `0x00520465`) belongs to the
/// Unit hijacker mechanism, which is separate. Its Building consumers are
/// diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct CapturedInfantryType(i32);

impl CapturedInfantryType {
    /// The Techno constructor's -1.
    pub(crate) const fn constructed() -> Self {
        Self(-1)
    }

    /// The captor's native InfantryType index, or -1.
    pub(crate) const fn get(self) -> i32 {
        self.0
    }

    /// The engineer capture's store of its native InfantryType index.
    pub(crate) fn record(&mut self, native_index: i32) {
        self.0 = native_index;
    }
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
pub enum EngineerBuildingAction {
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
pub enum UnitRepairAction {
    Repair,
    Select,
    Move,
}

/// What `UnitClass::What_Action_OnObject`'s building arm
/// (`0x00740122..0x007401BC`) makes of a Select over a Building whose
/// CAN_LOAD admits the Unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingClickAction {
    /// Action 3.
    Enter,
    /// Action 0xB, at a `Grinding=` building (`+0x16AD`).
    Grind,
    /// Action 0x1F, for a `MovementZone=Fly` Unit (`Type+0x5B4` 9).
    NoEnter,
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
    /// The selected objects whose ClickedAction can still send a click
    /// order, in selection order. `ClickedAction_Object` and `_Cell` refuse a
    /// berserk object (`+0x298`) before anything else: Foot
    /// `0x004D74EC`/`0x004D7D58`, Infantry `0x0051F1D8`/`0x0051F28F` and Unit
    /// `0x007388EB`/`0x0073893D`. Aircraft's own arms
    /// (`0x00417BD0`: actions 3, 4 and 17) never see one, because a berserk
    /// aircraft's What_Action answers 0 (`0x00417CC0` -> `0x006FFEC9`, cell
    /// `0x00417F80`). Producer-side, like `unit_repair_action`: an event sent
    /// before the object went berserk still executes.
    pub fn click_order_receivers(&self, selection: &[u64]) -> Vec<u64> {
        selection
            .iter()
            .copied()
            .filter(|&id| {
                !self
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| entity.berserk.is_active())
            })
            .collect()
    }

    /// UnitClass::What_Action_OnObject73FD7C..73FE43, after the base self/
    /// Shift action. Reproducible original control/weapon/health/display
    /// and ClickedAction witnesses: spatial_oracle/bunker_attack_action.json
    /// repair_rows (Steam gamemd 3e81a617). This is producer-side admission;
    /// a delivered Attack event must not repeat it after cargo/health changes.
    pub fn unit_repair_action(
        &self,
        actor_id: u64,
        target_id: u64,
        issuing_house: InternedId,
        rules: &RuleSet,
    ) -> Option<UnitRepairAction> {
        let actor = self.substrate.entities.get(actor_id)?;
        let actor_type = self.object_type(actor.type_ref(), rules)?;
        if actor.category() != EntityCategory::Unit
            || actor.owner() != issuing_house
            || !self.house_is_human_player(actor.owner())
            || combat::combat_weapon::weapon_damage_value(
                actor,
                actor_type,
                &self.substrate.entities,
                rules,
                &self.interner,
            ) >= 0
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
        let vehicle = match target.category() {
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
            return (target.category() != EntityCategory::Structure)
                .then_some(UnitRepairAction::Select);
        }
        if target.category() == EntityCategory::Aircraft {
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

    /// `UnitClass::What_Action_OnObject`'s building arm
    /// (`0x00740122..0x007401BC`), with the base action Select: a human
    /// player's Unit (House `0x0050B6F0`) over a Building of a house allied
    /// with it (`0x004F9A90`), not being warped out (vt+0x1D4), sends CAN_LOAD
    /// (`0x0074017E`, [`can_load_query`]). Any answer but ROGER leaves Select
    /// (`None`), as a busy, unpowered or selling refinery gives a miner.
    /// Producer-side, like [`Self::unit_repair_action`].
    ///
    /// [`can_load_query`]: crate::sim::radio::receive::can_load_query
    pub fn unit_building_click_action(
        &self,
        actor_id: u64,
        target_id: u64,
        rules: &RuleSet,
    ) -> Option<BuildingClickAction> {
        let actor = self.substrate.entities.get(actor_id)?;
        let target = self.substrate.entities.get(target_id)?;
        if actor.category() != EntityCategory::Unit
            || !self.house_is_human_player(actor.owner())
            || !combat::combat_weapon::is_ally_by_object(
                Some(&self.fog.alliances),
                &self.interner,
                target.owner(),
                actor.owner(),
            )
            || target.is_warped_out()
            || target.category() != EntityCategory::Structure
            || crate::sim::radio::receive::can_load_query(self, target_id, actor_id, rules)
                != crate::sim::radio::RadioResponse::Roger
        {
            return None;
        }
        if self.object_type(actor.type_ref(), rules)?.movement_zone
            == crate::rules::locomotor_type::MovementZone::Fly
        {
            return Some(BuildingClickAction::NoEnter);
        }
        Some(if self.object_type(target.type_ref(), rules)?.grinding {
            BuildingClickAction::Grind
        } else {
            BuildingClickAction::Enter
        })
    }

    pub fn house_is_human_player(&self, owner: InternedId) -> bool {
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

    /// Native TeamAssign731060 and TeamRecall7311C0 eligibility. The assignment
    /// path reads House+0x1ED (`PlayerControl`) directly in mode zero, while a
    /// nonzero mode calls House::IsHumanPlayer50B6F0 (current-house identity).
    /// Keep this distinct from `house_is_human_player`: its mode-zero branch
    /// also admits the separate `IsHuman` flag.
    pub fn house_is_control_group_eligible(&self, owner: InternedId) -> bool {
        if self.session.game_mode_nonzero {
            self.house_is_human_player(owner)
        } else {
            self.houses
                .get(&owner)
                .is_some_and(|house| house.player_control)
        }
    }

    /// Virtual+A0: Building44F5C0 or Unit/Infantry/Aircraft Techno700C40.
    /// This is producer-side control admission; Event/lifecycle admission is separate.
    /// House50B6F0, bunker link, Spawned, paralysis, warp, slave ownership and
    /// launched-missile count remain with their existing authoritative owners.
    ///
    /// Ordinary Infantry +504 EMP starts zero in the native constructor.
    /// EMPulseApply4C575E admits RTTI1/2, not Infantry15. The deactivated
    /// latch +1C8 (700CBC) has its owner in `sim::powered_unit`.
    /// Unit+6D8 death-frame and EMP lifecycle producers remain separate
    /// residuals; ordinary live Unit controls use their constructor -1/0.
    /// Arbitrary authored native raw-save bytes are outside this producer.
    /// See engineer_bridge_cursor_caller.md for executed gate controls/bounds.
    pub fn techno_player_controllable(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(object) = self.object_type(actor.type_ref(), rules) else {
            return false;
        };
        if actor.category() == EntityCategory::Structure {
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
        if !self.house_is_human_player(actor.owner())
            || actor.bunker.link().installed_in().is_some()
        {
            return false;
        }
        if object.spawned
            || actor.parasite.is_paralyzed(self.session.binary_frame)
            || actor.is_warped_out()
            || actor.is_warping_in()
            || actor.slave.owner().is_some()
            || actor.deactivated.is_offline()
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

    /// `TechnoClass::IsActive @ 0x007010D0` (vt+0x9C, no override): not
    /// deactivated (`+0x1C8`), owned by the player's house (`0x0050B6F0`),
    /// its current weapon (vt+0x3F4) typed and not vt+0x37C. `is_armed`
    /// stands in for the weapon test; vt+0x37C (a Unit's death frames or
    /// EMP) has no VERA producer, as in [`Self::techno_player_controllable`].
    pub fn techno_is_active(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        !actor.deactivated.is_offline()
            && self.house_is_human_player(actor.owner())
            && self
                .object_type(actor.type_ref(), rules)
                .is_some_and(|object| combat::combat_weapon::is_armed(actor, object))
    }

    /// Object-route Engineer action in Infantry51E49E..51E637. Ordinary
    /// action/threshold vectors: engineer_repair_joined; hut caller/display
    /// vectors: engineer_bridge_cursor_caller; shared geometric query:
    /// bridge_repair_query. Repair(false) is terminal action32.
    /// Admission belongs to the input producer, before a delayed mission event.
    pub fn engineer_building_action(
        &self,
        actor_id: u64,
        target_id: u64,
        rules: &RuleSet,
    ) -> Option<EngineerBuildingAction> {
        let actor = self.substrate.entities.get(actor_id)?;
        if actor.category() != EntityCategory::Infantry
            || !self.object_type(actor.type_ref(), rules)?.engineer
            || !self.house_is_human_player(actor.owner())
        {
            return None;
        }
        let target = self.substrate.entities.get(target_id)?;
        if target.category() != EntityCategory::Structure {
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
    pub fn infiltrator_building_action(
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
        if actor.category() != EntityCategory::Infantry
            || target.category() != EntityCategory::Structure
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
    /// setter's tail then clears the automatic Tesla charging latch `+0x68E`.
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
        if actor.category() != EntityCategory::Infantry
            || actor.navigation.nav_com().is_some()
            || building.category() != EntityCategory::Structure
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
            .map(|e| (e.owner(), e.type_ref(), e.position.rx(), e.position.ry()))
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
        let nav_object = match infantry.navigation.nav_com() {
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
            && target.category() == EntityCategory::Structure
            && self
                .object_type(target.type_ref(), rules)
                .is_some_and(|object| object.is_1x1_with_undeploy())
        {
            return InfantryPerCellBuildingAdmission::UndeployBuilding;
        }
        let cell = (infantry.position.rx(), infantry.position.ry());
        let Some(building_id) =
            self.substrate
                .occupancy
                .first_building_on_layer(cell.0, cell.1, MovementLayer::Ground)
        else {
            return InfantryPerCellBuildingAdmission::NoMatchingGroundBuilding;
        };
        if nav_object == Some(building_id)
            || infantry.attack_target.target().is_some_and(|attack| {
                matches!(attack, crate::sim::combat::TargetKind::Entity(id) if id == building_id)
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
        if engineer.category() != EntityCategory::Infantry {
            return Ok(BuildingEntryResult::default());
        }
        let InfantryPerCellBuildingAdmission::GroundBuilding(building_id) =
            self.infantry_per_cell_building_admission(engineer, rules)
        else {
            return Ok(BuildingEntryResult::default());
        };
        //519B3E..519B53 raises the BUILDING attachment with this Infantry
        //as actor, before the Engineer type gate. Synchronous actions may
        //change either object; reload the Infantry after returning.
        let context = super::UninitContext::new(Some(rules), registry).with_effects(frame_effects);
        self.dispatch_object_tag_event(building_id, 1, Some(engineer_id), None, context);
        let Some(engineer) = self.substrate.entities.get(engineer_id) else {
            return Ok(BuildingEntryResult::default());
        };
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
        let cell = (engineer.position.rx(), engineer.position.ry());
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
            let full = building.health.current() == object.strength;
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
                //519F44 re-reads the Building Tag and raises Entered again;
                //do not deduplicate the earlier519B53 call.519F4F then reads
                //the current owner, followed by optional519F6C Tag transfer.
                self.dispatch_object_tag_event(building_id, 1, Some(engineer_id), None, context);
                if let Some(old_owner) = self
                    .substrate
                    .entities
                    .get(building_id)
                    .map(|building| building.owner())
                    && let Some(old_house) = self.houses.get_mut(&old_owner)
                {
                    old_house.notify_building_capture();
                }
                self.transfer_engineer_tag_to_building(engineer_id, building_id);
                self.announce_engineer_capture(building_id, owner, rules);
                self.change_owner_with_rules(building_id, owner, rules, registry, frame_effects);
                if let Some(building) = self.substrate.entities.get_mut(building_id) {
                    // Techno+0x338 (`0x00519F94`).
                    building
                        .capture_infantry_type_index
                        .record(native_type_index);
                }
                changed =
                    self.scatter_building_infantry(building_id, rules, registry, frame_effects)?;
            }
            //519EAACapturable=false consumes too.519FF0 repair and519F9A
            //capture join Tag event48 then virtualUnInit, with no Foot tail.
            self.dispatch_object_tag_event(engineer_id, 48, Some(engineer_id), None, context);
            self.uninit_with_context(
                engineer_id,
                super::UninitContext::new(Some(rules), registry).with_effects(frame_effects),
            );
            return Ok(BuildingEntryResult {
                bridge_state_changed: changed,
                return_before_foot: true,
            });
        }
        let building_cell = (building.position.rx(), building.position.ry());
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
        //51A025 consumption raises own Tag48 before virtual UnInit.
        self.dispatch_object_tag_event(engineer_id, 48, Some(engineer_id), None, context);
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
    /// The common caller raises the Building's Entered1 before this gate.
    /// Consumption raises the spy's own48 synchronously before UnInit.
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
            self.dispatch_object_tag_event(
                infantry_id,
                48,
                Some(infantry_id),
                None,
                super::UninitContext::new(Some(rules), registry).with_effects(frame_effects),
            );
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
                let pending = building.pending_c4_detonation.pending()?;
                let object = self.interner.object_type(building.type_ref(), rules)?;
                Some((
                    pending,
                    building.health.current(),
                    object.bridge_repair_hut,
                    (building.position.rx(), building.position.ry()),
                ))
            })
        else {
            return false;
        };
        if !pending.timer().expired(self.session.binary_frame as i32) || health <= 0 {
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
                hut.pending_c4_detonation.cancel();
            }
            return changed;
        }

        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            building_id,
            health,
            0,
            pending
                .source_entity_id()
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

#[cfg(test)]
mod click_order_receiver_tests {
    use super::*;

    /// ClickedAction refuses a berserk object (Infantry `0x0051F1D8`, Unit
    /// `0x007388EB`): only the calm objects stay, in selection order.
    #[test]
    fn click_orders_go_only_to_calm_objects() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=GI\n[VehicleTypes]\n0=MTNK\n[GI]\nStrength=125\n\
             [MTNK]\nStrength=300\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let mut spawn = |kind: &str, rx: u16| {
            sim.spawn_object(kind, "Americans", rx, 5, 0, &rules)
                .unwrap()
        };
        let (first, gassed_man, tank, gassed_tank) = (
            spawn("GI", 3),
            spawn("GI", 4),
            spawn("MTNK", 5),
            spawn("MTNK", 6),
        );
        for id in [gassed_man, gassed_tank] {
            sim.entities_mut()
                .get_mut(id)
                .unwrap()
                .berserk
                .set_active_for_test(true);
        }
        assert_eq!(
            sim.click_order_receivers(&[gassed_tank, tank, gassed_man, first]),
            [tank, first]
        );
        assert!(sim.click_order_receivers(&[gassed_man]).is_empty());
    }
}
