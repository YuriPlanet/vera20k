//! `Per_Cell_Process`, vtable slot `+0x18C`: what an object runs on reaching a
//! cell. Every native arrival pushes reason 2: Drive (`0x004B1CFD`,
//! `0x004B220F`), Ship (`0x006A1340`, `0x006A1852`), Walk (`0x0075BE3C`),
//! Hover (`0x005146CA`, `0x00515A1C`), the Unit and Infantry tube exits
//! (`0x0073603F`, `0x0051BA9B`), Jumpjet touchdown (`0x0054C8F0`), a
//! parachute landing in `ObjectClass::AI` (`0x005F3F8D`) and Teleport
//! relocation (`0x0071971C`, `0x00719ADE`). Drive and Ship turn completion
//! pass another reason.
//!
//! The class overrides (`UnitClass` `0x00739EC0`, `InfantryClass`
//! `0x00519630`) run their own work, then `FootClass` `0x004D85D0`, which
//! ends in the Techno tail `0x006F5090`. An ordinary Fly arrival caller
//! has not been established; parachute landing is a separate Object caller.

use super::ground_pose::position_world_coord;
use super::locomotor::MovementLayer;
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{self, RadioMessage, RadioPayload};
use crate::sim::world::{FrameAdvanceError, FrameEffects, Simulation, UninitContext};

/// NavCom (`+0x5A4`) compared by pointer with `object`.
fn nav_com_is(entity: &crate::sim::game_entity::GameEntity, object: u64) -> bool {
    entity.navigation.nav_com() == Some(NavTargetRef::object(object))
}

/// The reason a `Per_Cell_Process` call passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PerCellReason {
    /// Drive/Ship turn completion: the Foot body and Techno tail skip it
    /// (`0x004D85DE`, `0x006F509D`).
    TurnComplete,
    /// Reason 2, every cell arrival.
    Arrival,
}

impl Simulation {
    /// FootClass::IsLeavingMap at 0x004DC790..4DC803 in retail gamemd.exe.
    /// Stored membership precedes the Team6EC300 gate; an IsTrain type
    /// still heading to an on-playfield NavCom must remain. The existing Map
    /// query owner retains its Dummy writes; the predicate draws nothing.
    fn foot_is_leaving_map(&self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if !entity.in_playfield()
            || self.team_script_vm.team_for_member(id).is_some()
                && !self.member_team_is_moving_outside_playfield(id)
        {
            return false;
        }
        let is_train = rules
            .and_then(|rules| self.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.is_train);
        let Some(destination) = entity.navigation.nav_com().filter(|_| is_train) else {
            return true;
        };
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        super::ground_pose::target_get_coords(destination.into(), self, cells.as_ref()).is_none_or(
            |coord| {
                !crate::sim::cell_rect::cell_is_in_playfield_leptons(
                    (coord.x, coord.y, coord.z),
                    self.playfield_bounds(),
                    self.resolved_terrain.as_ref(),
                )
            },
        )
    }

    /// The virtual call: the Unit or Infantry override, else the Foot body.
    /// Returns whether the Infantry override changed a bridge.
    pub(crate) fn per_cell_process(
        &mut self,
        id: u64,
        reason: PerCellReason,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, FrameAdvanceError> {
        match self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.category())
        {
            Some(EntityCategory::Unit) => {
                self.unit_per_cell_process(id, reason, rules, registry, effects);
                Ok(false)
            }
            Some(EntityCategory::Infantry) => {
                self.infantry_per_cell_process(id, reason, rules, registry, effects)
            }
            Some(_) => {
                self.foot_per_cell_process(id, reason, rules, registry, effects);
                Ok(false)
            }
            None => Ok(false),
        }
    }

    /// Unit739EC0 dock arrival arms73A31F..A5EA. Returns whether the
    /// object-destination arm ran its early Foot tail and completed the call.
    /// Native controls: tools/spatial_oracle/refinery_dock.json.
    pub(crate) fn unit_dock_now(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        // 73A31F..A547: an Enter7/Patrol25 object destination at its
        // GetDockCoord cell runs Foot's tail once, sends DOCK_NOW, powers
        // off the locomotor and returns before Unit Ready/crush/tail.
        let object_dock = self.substrate.entities.get(id).and_then(|unit| {
            if !matches!(unit.mission.effective().known(), Some(MissionType::Enter | MissionType::Patrol)) {
                return None;
            }
            let contact = unit.radio_contacts.slot(0)?;
            let building = self.substrate.entities.get(contact)?;
            if building.category() != EntityCategory::Structure { return None; }
            let at = super::ground_pose::object_get_coords(unit, self.resolved_terrain.as_ref());
            let cell = ((at.x /256) as u16, (at.y /256) as u16);
            let dock = super::building_dock_cell(&self.substrate.entities, contact, Some(id),
                self.resolved_terrain.as_ref(), rules, &self.interner)?;
            if cell != dock { return None; }
            let restore = self.object_type(building.type_ref(), rules).is_some_and(|object| object.unit_repair)
                // Literal GUID7E9A40 is Hover (4A582742), not Drive
                // (4A582741 at7E9A30). Native ctor/GetClassID controls pin it.
                && unit.locomotor.as_ref().is_some_and(|locomotor| locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Hover)
                && unit.navigation.nav_com().is_none();
            let aimed = unit.navigation.nav_com() == Some(NavTargetRef::object(contact));
            (restore || aimed).then_some((contact, restore))
        });
        if let Some((contact, restore)) = object_dock {
            if restore && let Some(unit) = self.substrate.entities.get_mut(id) {
                // Unit73A4E9 restores only the raw NavCom; unlike the Foot
                // setter this does not clear NavComAux or change movement.
                unit.navigation
                    .store_nav_com(Some(crate::sim::components::NavTargetRef::object(contact)));
            }
            self.foot_per_cell_process(id, PerCellReason::Arrival, Some(rules), registry, effects);
            crate::sim::radio::transmit_to_contact(
                self,
                id,
                crate::sim::radio::RadioMessage::DockNow,
                Some(rules),
                effects,
            );
            // `0x0073A52B`: ILoco+0x5C, Hover's override included.
            self.locomotor_power_off(id);
            return true;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.tether.partner().is_none()
            || entity.mission.effective()
                != crate::sim::mission::MissionId::from_known(MissionType::Enter)
        {
            return false;
        }
        let Some(contact) = entity.radio_contacts.slot(0) else {
            return false;
        };
        let (x, y) = (entity.position.rx(), entity.position.ry());
        let north = self.substrate.occupancy.first_building_on_layer(
            x,
            (y as i16).wrapping_sub(1) as u16,
            MovementLayer::Ground,
        );
        if north != Some(contact) {
            return false;
        }
        let reply = radio::transmit(
            self,
            id,
            contact,
            RadioMessage::DockNow,
            RadioPayload::default(),
            Some(rules),
            effects,
        );
        // 0x0073A5CE..0x0073A5E4: an answer other than 1 or 5 scatters the unit
        // (a refinery being sold) — RESIDUAL, module doc.
        let _ = reply;
        false
    }

    /// `UnitClass::Per_Cell_Process @ 0x00739EC0`: the Unit's own arrival
    /// work, then the Foot body (`0x0073A4FB`, `0x0073B0A0`).
    pub(super) fn unit_per_cell_process(
        &mut self,
        id: u64,
        reason: PerCellReason,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // 739ED3..739EE2: +1B8 (41BEA0) samples physical packed XY once,
        // before MCV/arrival callbacks. This is not the navigation/head cell.
        let entry_cell = self
            .substrate
            .entities
            .get(id)
            .map(|entity| super::foot_path::coord_cell(position_world_coord(&entity.position)));
        // Unit739EC0 invokes the MCV receiver before normal crush/Foot tail.
        if let Some(rules) = rules {
            crate::sim::mcv_deploy::per_cell_process(self, id, rules, registry, effects);
        }
        if !self.track_survives(id) {
            return;
        }
        // 0x0073A31F..0x0073A5EA, before the Ready/Commence below: a tethered
        // unit on Enter arriving north-adjacent to its dock sends DOCK_NOW.
        if let Some(rules) = rules
            && reason == PerCellReason::Arrival
        {
            // 0x00739F21..0x00739F59 come first: a `PoweredUnit=` type whose
            // house has no powered center goes offline off a building.
            self.powered_unit_arrival(id, rules, effects);
            //739F7D..73A234: Eaten consumes the unit before docking, boarding
            //and the Foot sight/mission tail.
            if self.foot_per_cell_eaten(id, rules, registry, effects) {
                return;
            }
            if self.unit_per_cell_absorber(id, rules, registry, effects) {
                return;
            }
            // The object-destination docking arm73A31F..A547 completes
            // Foot's tail and returns before the later factory clearance.
            if self.unit_dock_now(id, rules, registry, effects) {
                return;
            }
            // Unit73A5EA..73A7CF attaches at PerCell2, before clearance,
            // sight, Ready and Foot's tail. Drive4B20CD..4B2115 retires its
            // track before this callback.
            if self.unit_per_cell_transport(id, rules, registry, effects) {
                return;
            }
            self.unit_per_cell_factory_clearance(id, rules, registry, effects);
        }
        // 0x0073AC52..0x0073ACAE: ReReveal then UpdateReveal and the
        // current-XYZ/cached-radius+3 Map567DA0 border, after clearance and
        // before Ready/Commence. Evidence: shroud_frontier_moving.json.
        // RESIDUAL: native returns
        // before them from Guard's map exit (`0x0073AC0E..0x0073AC4F`), which
        // VERA runs elsewhere. Trigger: a unit arriving to leave the map.
        // Effect: VERA reveals one more footprint at that cell, which stays
        // revealed.
        if reason == PerCellReason::Arrival && self.per_cell_owner_alive(id) {
            self.refresh_object_sight(id, rules, 0);
            self.refresh_object_sight_border(id, UninitContext::new(rules, registry));
        }
        // Unit PerCell2 739EC0: after MCV retry, +6D1==0 admits
        // Ready(+200)73ACC2 -> Commence(+1EC)73ACD1, BEFORE full-cell
        // crush73B089 and Foot sensor/playfield tail73B0A0. The existing
        // miner.unload_active owns +6D1 (ctor7353FE, unload73DFDA).
        // This promotes only: it does not dispatch a mission handler or
        // repeat the object AI prefix. mission_host_promote retains its
        // documented unavailable locomotor/height fallback for other inputs.
        let promote = reason == PerCellReason::Arrival
            && self.substrate.entities.get(id).is_some_and(|entity| {
                !entity
                    .miner
                    .as_ref()
                    .is_some_and(|miner| miner.unload_active())
            });
        if let Some(rules) = rules.filter(|_| promote) {
            self.mission_host_promote(id, self.session.binary_frame(), rules);
        }
        // 0x0073ACD7..0x0073ADC4: a harvester (or weeder) off Enter/Unload
        // drops a refinery (weeder) contact at every track end.
        if let Some(rules) = rules
            && reason == PerCellReason::Arrival
        {
            crate::sim::miner::per_cell_release_dock_contact(self, rules, id, effects);
        }
        // 73AFD4..73B06E is common to reasons0/2 and precedes the fresh
        // +1B8/full-cell crush query73B074 and the Foot continuation.
        if let (Some(cell), Some(rules), Some(registry)) = (entry_cell, rules, registry) {
            self.unit_per_cell_overlay_crush(id, cell, rules, registry, effects);
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let at = (entity.position.rx(), entity.position.ry());
        let layer = if entity.on_bridge() {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let mut cursor = self
            .substrate
            .occupancy
            .get(at.0, at.1)
            .and_then(|list| list.first_on_layer(layer));
        let mut crushed_unit = false;
        while let Some(victim) = cursor {
            // Save successor BEFORE lifecycle can remove the current object.
            cursor = self
                .substrate
                .occupancy
                .get(at.0, at.1)
                .and_then(|list| list.next_on_layer(layer, victim));
            if victim == id {
                continue;
            }
            let Some(crusher) = self.substrate.entities.get(id) else {
                return;
            };
            let coord = position_world_coord(&crusher.position);
            let capability = super::bump_crush::CrushCapability::new(
                crusher.mover_facts.regular_crusher(),
                crusher.mover_facts.omni_crusher(),
            );
            let kills = super::bump_crush::select_crush_victims(
                &[victim],
                &self.substrate.entities,
                id,
                self.house_alliances.as_map(),
                &self.interner,
                (coord.x, coord.y),
                capability,
                self.session.binary_frame(),
            );
            if !kills.contains(&victim) {
                continue;
            }
            if let Some(rules) = rules {
                if let Some(entity) = self.substrate.entities.get(victim) {
                    super::bump_crush::emit_crush_kill_sounds_at(
                        entity,
                        (i32::from(at.0), i32::from(at.1)),
                        rules,
                        &mut self.interner,
                        &mut self.sound_events,
                    );
                }
                // The victim's vtable `+0x170` (`0x007418E5`): a Unit's
                // `0x00746D60` runs Death_Explosion (`0x00738680`), then
                // FreeAllMindControlCaptures (`0x00710460`), which is all the
                // other classes run; VERA releases captives in the victim's
                // UnInit below.
                if self
                    .substrate
                    .entities
                    .get(victim)
                    .is_some_and(|entity| entity.category() == EntityCategory::Unit)
                {
                    crushed_unit = true;
                    self.unit_death_explosion(rules, victim, &mut Vec::new());
                }
                // `UnitClass::Crush_Or_Prescatter_Cell @ 0x007416A0`: the
                // victim records its kill by the crusher (vtable `+0xE0`,
                // `0x007418F0`) and is UnInit (vtable `+0xF8`, `0x00741910`),
                // with no Health store.
                self.record_the_kill(
                    victim,
                    crate::sim::combat::KillSource::Techno(Some(id)),
                    crate::sim::combat::KillCallback::Terminal,
                    UninitContext::new(Some(rules), registry).with_effects(effects),
                );
                self.apply_lifecycle_request_with_rules(
                    LifecycleRequest::Uninit {
                        stable_id: victim,
                        reason: UninitReason::Crush,
                    },
                    rules,
                    registry,
                    effects,
                );
            } else {
                self.apply_lifecycle_request(LifecycleRequest::Uninit {
                    stable_id: victim,
                    reason: UninitReason::Crush,
                });
            }
        }
        // The loop's tail (`0x00741925..0x00741954`): any crush uncloaks the
        // crusher (vtable `+0x45C`, argument 0; no stock crusher cloaks),
        // and crushing a Unit tilts a level crusher forwards.
        if crushed_unit && let Some(crusher) = self.substrate.entities.get_mut(id) {
            crusher
                .rocking
                .get_or_insert_with(Default::default)
                .tilt_after_unit_crush();
        }
        // 0x0073B08F tests only IsAlive (`+0x90`) before the Foot body.
        if self.per_cell_owner_alive(id) {
            self.foot_per_cell_process(id, reason, rules, registry, effects);
        }
    }

    /// Unit73AFD4..73B06E. The Cell is selected from the entry sample;
    /// Sound73B04D separately samples the actor's current raw XYZ (+48).
    /// Controls: unit_per_cell_overlay.json and anytown_damage/fence_driveover.json.
    fn unit_per_cell_overlay_crush(
        &mut self,
        id: u64,
        entry_cell: (i16, i16),
        rules: &RuleSet,
        registry: &OverlayTypeRegistry,
        effects: FrameEffects<'_>,
    ) {
        use crate::map::cell_index::NativeCellIdentity;
        use crate::rules::locomotor_type::MovementZone;
        use crate::rules::object_type::Ability;
        use crate::sim::combat::veterancy::has_weapon_ability;
        use crate::sim::overlay_grid::WallPointerTarget;

        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let cell = terrain.native_cell_identity(entry_cell);
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(actor.type_ref(), rules) else {
            return;
        };
        if !(object.crusher
            || has_weapon_ability(actor.veterancy.rank_kind(), object, Ability::Crusher))
        {
            return;
        }
        let Some(flags) = self
            .cell_overlay(cell)
            .overlay_id
            .and_then(|id| registry.flags(id))
        else {
            return;
        };
        if !(flags.crushable || (flags.wall && object.movement_zone == MovementZone::CrusherAll)) {
            return;
        }
        let target = match cell {
            NativeCellIdentity::Real(index) => {
                let c = &terrain.cells()[index];
                WallPointerTarget::Real(c.rx, c.ry)
            }
            NativeCellIdentity::Dummy => WallPointerTarget::SharedDummy,
        };
        if let Some(sound) = flags.crush_sound.as_ref() {
            self.sound_events
                .emit(crate::sim::world::SimSoundEvent::wall_crushed(
                    sound.clone(),
                    &actor.position,
                ));
            //7509E0 completes before480CB0. The existing process owner may
            //draw Main RNG during cue admission/playback; queuing alone would
            //defer it across this native boundary.
            effects.flush_sound(self, Some(rules));
        }
        self.apply_wall_damage(target, -1, registry);
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor
                .rocking
                .get_or_insert_with(Default::default)
                .recoil_after_overlay_crush();
            //73B067 clears Foot+6B5 after the damage call, even when the
            //Cell refuses a non-Wall overlay. TiltsWhenCrushes is not a gate.
            actor.foot_speed.clear_crush_slowdown();
        }
    }

    /// Unit739EC0's tethered arrival chapter73A7D2..73ABD0, before its
    /// Ready/Commence checkpoint. A computer House's unit from a war factory
    /// takes its post ([`Self::unit_factory_post`]); the other continuation
    /// calls the existing archive destination, FootStop4DF0D0 and
    /// UnitScatter743A50 owners. Every arm ends in the building Scatter tail
    /// ([`Self::unit_per_cell_building_scatter`]).
    /// Source: anytown_damage/unit_unlimbo radio receipt and original Unit
    /// listing; the composed factory-Unload control binds the null-NavCom arm.
    fn unit_per_cell_factory_clearance(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        if unit.tether.partner().is_none() {
            return;
        }
        let nav_com = unit.navigation.nav_com();
        let nav_techno = match nav_com {
            Some(NavTargetRef::Object { id }) => crate::sim::radio::as_techno(self, id),
            _ => None,
        };
        let mission = unit.mission.effective().known();
        if mission == Some(MissionType::Enter)
            && (nav_techno.is_none() || nav_techno == unit.radio_contacts.slot(0))
        {
            return;
        }
        if mission == Some(MissionType::Unload) {
            return;
        }
        let position = position_world_coord(&unit.position);
        //73A859..73A889 uses signed truncation toward zero, then compares
        // the packed Cell words. Navigation's route/final goal is not read.
        let cell = (
            (position.x / 256) as i16 as u16,
            (position.y / 256) as i16 as u16,
        );
        let at_destination =
            nav_com.is_none() || nav_com == Some(NavTargetRef::cell(cell.0, cell.1));
        //Map565730 -> Cell47C520 walks only the first ground Building.
        // RESIDUAL: ScenarioActiveA8E9A0=false loading/termination and shared
        // Dummy Cell identities have no full active-map boundary here. This
        // receiver covers the ordinary retained real Cell in an active frame.
        let building =
            self.substrate
                .occupancy
                .first_building_on_layer(cell.0, cell.1, MovementLayer::Ground);
        if building.is_some() && !at_destination {
            return;
        }
        //73A8CF saves Contact0 before the synchronous transmit73A93D. The
        // radio8 owner can clear the live slot and both tether bytes inside it.
        let contact = unit.radio_contacts.slot(0);
        if !at_destination
            && contact.is_some_and(|contact| self.is_weapons_factory(contact, rules))
            && self.substrate.occupancy.first_building_on_layer(
                cell.0,
                cell.1,
                MovementLayer::Ground,
            ) == contact
        {
            return;
        }
        let reply = crate::sim::radio::transmit_to_contact(
            self,
            id,
            crate::sim::radio::RadioMessage::RequestClearance,
            Some(rules),
            effects,
        );
        if reply != crate::sim::radio::RadioResponse::Queued {
            // RESIDUAL: `0x0073AAF7..0x0073AB66` is not ported. These are the
            // replies other than Queued (0x17) from a tethered dock (a
            // refinery, service depot or bunker; a war factory answers 0x17).
            // - A Harvester or Weeder with an archive queues Harvest (0) and is
            //   sent to the archive, which then clears. Without an archive and
            //   NavCom it Scatters (NULL, 1, 0).
            // - Reply 0xA and every other Unit go straight to the building
            //   Scatter tail (`Self::unit_per_cell_building_scatter`), which
            //   every one of these arms reaches.
            // VERA returns here, leaving the departure to the dock owners.
            // Trigger: a tethered Unit's arrival in a Cell away from its dock.
            // Effect: none of this arm's Harvest, archive send or Scatter.
            // Frequency: harvester and service-depot departures; not measured.
            // Risk: a Unit left standing on a building with no orders stays
            // put where native would scatter it.
            return;
        }
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        let nav_com = unit.navigation.nav_com();
        if nav_com.is_some() && nav_com != Some(NavTargetRef::cell(cell.0, cell.1)) {
            // RESIDUAL:73A96F tests Abstract+14 bit1 before directed opcode
            //0xE at73A981. This distinct non-current-object receiver requires
            // that validity state; it is not the null/current-Cell human arm.
            return;
        }
        let Some(object) = self.object_type(unit.type_ref(), rules) else {
            return;
        };
        if object.harvester || object.weeder {
            // Type+E0E/E0F at73A9AE/73A9C2 select73AAE6. PUSH1/PUSH0xA
            // at73AAE9/73AAEB calls Queue_Mission(+1E8) at73AAEF, after
            // radio8's reciprocal cleanup and before the later Ready call.
            // Live readiness owns whether this queues or Commences Harvest;
            // the existing miner mission handler owns subsequent resource work.
            let _ = self.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Harvest),
                1,
                self.session.binary_frame(),
                &LiveReadyInputProvider { rules },
            );
        } else if unit.slave_manager.is_some() {
            //73A9CA..73A9D9: a Slave Miner's manager (+2D8) starts its hunt
            // (6B0CC0) in place of the rally or post.
            self.begin_slave_hunt(id, rules, effects);
        } else {
            // A missing House cannot stand in for Human50B730.
            let Some(human) = self
                .houses
                .get(&unit.owner())
                .map(|house| house.is_controlled_by_human(self.session.game_mode_nonzero()))
            else {
                return;
            };
            if !human && contact.is_some_and(|contact| self.is_weapons_factory(contact, rules)) {
                self.unit_factory_post(id, unit.owner(), rules, effects);
            } else if let Some(archive) = unit.archive_target().filter(|archive| {
                Some(*archive) != nav_com.map(crate::sim::combat::TargetKind::from)
            }) {
                self.set_unit_destination(id, NavTargetRef::from(archive), rules, true, effects);
            } else {
                if let Some(unit) = self.substrate.entities.get_mut(id) {
                    unit.navigation.stop_moving();
                }
                //73AAC8..73AADB: no QueueMission, Process or EnterIdle. Unit's
                // null Scatter receiver cannot produce the Infantry error path.
                let _ = self.scatter(
                    id,
                    None,
                    super::scatter::ScatterFlags::new(true, false),
                    rules,
                    registry,
                    effects,
                );
            }
        }
        self.unit_per_cell_building_scatter(id, rules, registry, effects);
    }

    /// `0x0073AB6C..0x0073ABD0`, where every arm of the clearance above
    /// ends: a Unit still on a Building (Map565730 -> Cell47C520, the first
    /// ground Building of its Location's Cell) with no NavCom, an empty
    /// navigation list (`+0x5BC`) and an empty NavQueue (`+0x598`) scatters
    /// (vt+0x174(NULL, 1, 1)).
    ///
    /// A war factory clears its product on its own foundation (location
    /// +(4,1), where the exit track ends) with a null NavCom. So a harvester
    /// or Slave Miner leaving it scatters here, and so does a computer
    /// product whose post is the zero Cell. A post or a rally sets a NavCom,
    /// and an order given during the exit is listed
    /// ([`Self::unit_factory_exit_arm`]); either skips this call. A human
    /// product without a rally has already scattered (NULL, 1, 0), so this
    /// call, with no-kidding set, only retries when that one set no NavCom.
    fn unit_per_cell_building_scatter(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        let position = position_world_coord(&unit.position);
        let cell = (
            (position.x / 256) as i16 as u16,
            (position.y / 256) as i16 as u16,
        );
        let on_building = self
            .substrate
            .occupancy
            .first_building_on_layer(cell.0, cell.1, MovementLayer::Ground)
            .is_some();
        if on_building
            && unit.navigation.nav_com().is_none()
            && unit.navigation.nav_list().is_empty()
            && unit.navigation.nav_queue().is_empty()
        {
            let _ = self.scatter(
                id,
                None,
                super::scatter::ScatterFlags::new(true, true),
                rules,
                registry,
                effects,
            );
        }
    }

    /// `0x0073AA0D..0x0073AA9C`: a computer House's unit cleared by its war
    /// factory takes a post near its base, House500200 (`0x0073AA19`,
    /// [`Self::house_return_cell`], two Scenario draws). The post queues Move
    /// (`0x0073AA48`), becomes the destination (vt+0x480, `0x0073AA65`), the
    /// mission Commences (vt+0x1EC, `0x0073AA70`), the post is archived
    /// (`0x0070C610`, `0x0073AA88`) and AreaGuard is queued (`0x0073AA96`).
    /// With no post (the packed zero Cell at `0x00B1CFB8`, which only the
    /// static initializer `0x00735320` writes) the archive clears
    /// (`0x0073AB43`).
    ///
    /// A product reaches this on the factory's Move with nothing queued, so
    /// the Queue and Commence of Move change nothing. The Ready/Commence that
    /// follows (`0x0073ACC2`, `0x0073ACD1`) then commences AreaGuard at once:
    /// Drive emptied HeadTo before this arrival call (`0x004B21ED..
    /// 0x004B2204`) and Move_To writes only the destination, so the unit is
    /// not moving (native trace: `spatial_oracle/anytown_damage/
    /// unit_unlimbo.json`, `factory_unload_rows[0]`, Ready 1 at `0x0073ACC2`).
    /// Whether a Hover or Jumpjet product is moving at its arrival call is
    /// not established; if it is, `ready_to_commence_answer`'s moving-defer
    /// residual applies.
    fn unit_factory_post(
        &mut self,
        id: u64,
        owner: crate::sim::intern::InternedId,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(post) = self.house_return_cell(owner, id) else {
            if let Some(unit) = self.substrate.entities.get_mut(id) {
                unit.set_archive_target(None);
            }
            return;
        };
        let now = self.session.binary_frame();
        let readiness = LiveReadyInputProvider { rules };
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Move),
            0,
            now,
            &readiness,
        );
        let _ =
            self.set_unit_destination(id, NavTargetRef::cell(post.0, post.1), rules, true, effects);
        let _ = self.substrate.mission_commence_exact(id, now);
        if let Some(unit) = self.substrate.entities.get_mut(id) {
            unit.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(post.0, post.1)));
        }
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::AreaGuard),
            0,
            now,
            &readiness,
        );
    }

    /// `InfantryClass::Per_Cell_Process @ 0x00519630`: for reason 2 its
    /// occupier arm (`0x00519675..0x00519739`,
    /// [`Self::infantry_per_cell_occupy`]), building receiver (an Engineer's,
    /// or a spy's infiltration, `0x00519948..0x0051A06E`), Enter arm
    /// (`0x0051A0D4..0x0051A4BF`, [`Self::infantry_per_cell_enter`]), C4 plant
    /// (`0x0051A4BF..0x0051A664`, [`Self::infantry_per_cell_c4`]), tethered
    /// radio8 release (`0x0051A7F8..0x0051A812`), then the Foot body for a
    /// live owner (`0x0051A9EB` tests only IsAlive, `+0x90`). Each arm asks
    /// for its own missions, so their order among themselves is native.
    ///
    /// The Enter arm runs before C4 and the Teleport caller's NULL
    /// destination, despite warp-in.
    /// Eaten51973C runs after the occupier arm and before ordinary Capture.
    /// RESIDUAL: other Capture branches and the C4 cell
    /// arm (`0x0051973C..0x0051A9E8`) are not dispatched from here. Some have
    /// ports with their own owners (`capture_manager`, `passenger`); native
    /// runs them before the Foot body, with some early returns, which also
    /// skip the reveal (`0x0051A8D7`). Trigger: an infantry arriving on a
    /// mission cell. Risk: same-frame receiver ordering, and one more
    /// revealed footprint for an infantryman such an arm takes in.
    fn infantry_per_cell_process(
        &mut self,
        id: u64,
        reason: PerCellReason,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, FrameAdvanceError> {
        if reason == PerCellReason::Arrival
            && let Some(rules) = rules
            && self.infantry_per_cell_occupy(id, rules, registry, effects)?
        {
            return Ok(false);
        }
        if reason == PerCellReason::Arrival
            && let Some(rules) = rules
            && self.foot_per_cell_eaten(id, rules, registry, effects)
        {
            return Ok(false);
        }
        let entry = match rules {
            Some(rules) if reason == PerCellReason::Arrival => {
                self.infantry_per_cell_building_entry(id, rules, registry, effects)?
            }
            _ => Default::default(),
        };
        if !entry.return_before_foot {
            // Teleport71971C calls here after setting warp-in, before its
            // NULL destination.
            if reason == PerCellReason::Arrival
                && let Some(rules) = rules
                && let Some(changed) = self.infantry_per_cell_enter(id, rules, registry, effects)?
            {
                return Ok(entry.bridge_state_changed || changed);
            }
            //51A4BF follows the Enter arm and precedes radio8 release.
            if reason == PerCellReason::Arrival
                && let Some(rules) = rules
                && self.infantry_per_cell_c4(id, rules, registry, effects)?
            {
                return Ok(false);
            }
            //51A7F8 reads the live Techno+418 after earlier arrival receivers;
            //51A80C sends literal8 through Contacts[0], not DOCK_NOW0x15.
            //No new alive gate precedes it;51A9EB gates only the Foot tail.
            //P2 human GAPILE output observes the original nested25/3 release,
            //with all three RNG streams unchanged by this transaction.
            if reason == PerCellReason::Arrival
                && self
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| entity.tether.partner().is_some())
            {
                radio::transmit_to_contact(
                    self,
                    id,
                    RadioMessage::RequestClearance,
                    rules,
                    effects,
                );
            }
            // 0x0051A8CB..0x0051A8EB: ReReveal then UpdateReveal, before the
            // water and rock check.
            if reason == PerCellReason::Arrival && self.per_cell_owner_alive(id) {
                self.refresh_object_sight(id, rules, 0);
            }
            if self.per_cell_owner_alive(id) {
                self.foot_per_cell_process(id, reason, rules, registry, effects);
            }
        }
        Ok(entry.bridge_state_changed)
    }

    /// What an Enter arrival finds in a cell: the first Building of its
    /// ground list (`0x0047C520`), else the first Aircraft (`0x0047EBF0`) or
    /// Unit (`0x0047EBA0`) of `layer`'s list, in that order; a
    /// `VehicleThief=` infantryman looks for the Unit first and the Building
    /// last (Infantry `0x0051A0D4..0x0051A184` and `0x0051A1BB..0x0051A219`,
    /// Unit `0x0073A5EA..0x0073A63A`).
    fn enter_candidate(
        &self,
        (rx, ry): (u16, u16),
        layer: MovementLayer,
        vehicle_thief: bool,
    ) -> Option<u64> {
        let occupancy = &self.substrate.occupancy;
        let entities = &self.substrate.entities;
        let building = || occupancy.first_building_on_layer(rx, ry, MovementLayer::Ground);
        let aircraft =
            || occupancy.first_category_on_layer(rx, ry, layer, EntityCategory::Aircraft, entities);
        let unit =
            || occupancy.first_category_on_layer(rx, ry, layer, EntityCategory::Unit, entities);
        if vehicle_thief {
            unit().or_else(aircraft).or_else(building)
        } else {
            building().or_else(aircraft).or_else(unit)
        }
    }

    /// The Enter arm of `InfantryClass::Per_Cell_Process`
    /// (`0x0051A0D4..0x0051A4BF`). On Enter (Get_Mission), what he finds in
    /// his own cell ([`Self::enter_candidate`], his OnBridge `+0x8C` naming
    /// the list) is entered when it is his NavCom or his Target (`+0x2B4`),
    /// or what a Cell NavCom holds, looked up on the bridge lists
    /// (`0x0051A186..0x0051A252`). Its Tag hears event 1 with him
    /// (`0x0051A258..0x0051A26A`). Then:
    /// - a Building that is also the first Building of his cell is sent
    ///   DOCK_NOW through his first contact, and ROGER takes him in
    ///   (`0x0051A27F..0x0051A39D`);
    /// - anything else that shares his cell (GetMapCoords, vt+0x1B8) and is
    ///   his NavCom is asked CAN_LOAD: ROGER takes him aboard
    ///   (`0x0051A3A0..0x0051A485`); any other answer drops his destination,
    ///   queues Guard and scatters him (`0x0051A488..0x0051A4BC`).
    ///
    /// Answers `Some(bridge changed)` when the arm returned before C4 and
    /// Foot's tail, `None` when the visit carries on. The Tag event can run
    /// trigger actions; nothing here draws RNG but the refusal's Scatter.
    fn infantry_per_cell_enter(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<Option<bool>, FrameAdvanceError> {
        use crate::sim::passenger::{BoardingArm, board_passenger};
        use crate::sim::radio::RadioResponse;
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        if infantry.mission.effective() != MissionId::from_known(MissionType::Enter) {
            return Ok(None);
        }
        let vehicle_thief = self
            .object_type(infantry.type_ref(), rules)
            .is_some_and(|object| object.vehicle_thief);
        let layer = if infantry.on_bridge() {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let cell = (infantry.position.rx(), infantry.position.ry());
        let Some(candidate) = self.enter_candidate(cell, layer, vehicle_thief) else {
            return Ok(None);
        };
        let nav_cell = match infantry.navigation.nav_com() {
            Some(NavTargetRef::Cell { rx, ry }) => Some((rx, ry)),
            _ => None,
        };
        let targeted = matches!(
            infantry.attack_target.target(),
            Some(crate::sim::combat::TargetKind::Entity(target)) if target == candidate
        );
        if !nav_com_is(infantry, candidate)
            && !targeted
            && nav_cell
                .and_then(|cell| self.enter_candidate(cell, MovementLayer::Bridge, vehicle_thief))
                != Some(candidate)
        {
            return Ok(None);
        }
        let context = UninitContext::new(Some(rules), registry).with_effects(effects);
        self.dispatch_object_tag_event(candidate, 1, Some(id), None, context);
        let (Some(infantry), Some(carrier)) = (
            self.substrate.entities.get(id),
            self.substrate.entities.get(candidate),
        ) else {
            return Ok(None);
        };
        let cell = (infantry.position.rx(), infantry.position.ry());
        if carrier.category() == EntityCategory::Structure {
            if self.substrate.occupancy.first_building_on_layer(
                cell.0,
                cell.1,
                MovementLayer::Ground,
            ) != Some(candidate)
                || radio::transmit_to_contact(self, id, RadioMessage::DockNow, Some(rules), effects)
                    != RadioResponse::Roger
            {
                return Ok(None);
            }
            board_passenger(
                self,
                rules,
                id,
                candidate,
                BoardingArm::InfantryBuilding,
                registry,
                effects,
            );
            return Ok(Some(false));
        }
        if (carrier.position.rx(), carrier.position.ry()) != cell
            || !nav_com_is(infantry, candidate)
        {
            return Ok(None);
        }
        if radio::transmit(
            self,
            id,
            candidate,
            RadioMessage::CanEnter,
            RadioPayload::default(),
            Some(rules),
            effects,
        ) == RadioResponse::Roger
        {
            board_passenger(
                self,
                rules,
                id,
                candidate,
                BoardingArm::InfantryTransport,
                registry,
                effects,
            );
            return Ok(Some(false));
        }
        self.assign_null_destination(id, Some(rules), registry, effects);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(MissionType::Guard),
            );
        }
        let changed = self
            .infantry_scatter_null(
                id,
                super::ScatterFlags::new(true, true),
                rules,
                registry,
                effects,
            )
            .map_err(|cause| FrameAdvanceError {
                tick: self.session.tick(),
                binary_frame: self.session.binary_frame(),
                entity_id: id,
                cause,
            })?;
        Ok(Some(changed))
    }

    /// `UnitClass::Per_Cell_Process`'s `UnitAbsorb=` arm
    /// (`0x0073A237..0x0073A31C`): on Enter, with the first Building of its
    /// cell (`0x0047C520`) as NavCom and that building `UnitAbsorb=`, the unit
    /// asks it CAN_LOAD. ROGER takes it in; any other answer runs
    /// [`Self::unit_per_cell_enter_refused`]. Either returns before the rest
    /// of the visit.
    fn unit_per_cell_absorber(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(unit) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(building) = self.substrate.occupancy.first_building_on_layer(
            unit.position.rx(),
            unit.position.ry(),
            MovementLayer::Ground,
        ) else {
            return false;
        };
        if !self
            .substrate
            .entities
            .get(building)
            .and_then(|building| self.object_type(building.type_ref(), rules))
            .is_some_and(|object| object.unit_absorb)
            || unit.mission.effective() != MissionId::from_known(MissionType::Enter)
            || !nav_com_is(unit, building)
        {
            return false;
        }
        self.unit_per_cell_board(
            id,
            building,
            crate::sim::passenger::BoardingArm::UnitAbsorber,
            rules,
            registry,
            effects,
        );
        true
    }

    /// `UnitClass::Per_Cell_Process`'s transport arm
    /// (`0x0073A5EA..0x0073A7CF`): what the unit finds in its cell
    /// ([`Self::enter_candidate`]; the object after itself in its cell list
    /// when that is itself), on Enter, sharing its GetMapCoords, in radio
    /// contact with it (`0x0065AD50`) and of a type with `Passengers=` above
    /// 0, is asked CAN_LOAD. ROGER takes it aboard; any other answer runs
    /// [`Self::unit_per_cell_enter_refused`]. Either returns before the rest
    /// of the visit.
    fn unit_per_cell_transport(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(unit) = self.substrate.entities.get(id) else {
            return false;
        };
        let cell = (unit.position.rx(), unit.position.ry());
        let layer = if unit.on_bridge() {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let mut candidate = self.enter_candidate(cell, layer, false);
        if candidate == Some(id) {
            // `0x0073A640`: As_Techno of its own next object (`+0x30`).
            candidate = self
                .substrate
                .occupancy
                .get(cell.0, cell.1)
                .and_then(|list| list.next_on_layer(layer, id));
        }
        if unit.mission.effective() != MissionId::from_known(MissionType::Enter) {
            return false;
        }
        let Some(transport) = candidate
            .filter(|&candidate| candidate != id)
            .and_then(|candidate| self.substrate.entities.get(candidate))
        else {
            return false;
        };
        if (transport.position.rx(), transport.position.ry()) != cell
            || !unit.radio_contacts.contains(transport.stable_id())
            || self
                .object_type(transport.type_ref(), rules)
                .is_none_or(|object| object.passengers <= 0)
        {
            return false;
        }
        let transport = transport.stable_id();
        self.unit_per_cell_board(
            id,
            transport,
            crate::sim::passenger::BoardingArm::UnitTransport,
            rules,
            registry,
            effects,
        );
        true
    }

    /// The Unit arms' CAN_LOAD (`0x0073A273`, `0x0073A6D1`): ROGER boards
    /// through [`crate::sim::passenger::board_passenger`], anything else runs
    /// [`Self::unit_per_cell_enter_refused`].
    fn unit_per_cell_board(
        &mut self,
        id: u64,
        carrier: u64,
        arm: crate::sim::passenger::BoardingArm,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if radio::transmit(
            self,
            id,
            carrier,
            RadioMessage::CanEnter,
            RadioPayload::default(),
            Some(rules),
            effects,
        ) == crate::sim::radio::RadioResponse::Roger
        {
            crate::sim::passenger::board_passenger(
                self, rules, id, carrier, arm, registry, effects,
            );
        } else {
            self.unit_per_cell_enter_refused(id, rules, registry, effects);
        }
    }

    /// The Unit Enter arms' refusal (`0x0073A796..0x0073A7CF`): the NULL
    /// destination (vt+0x480(0, 1)), Guard queued (vt+0x1E8(5, 0)) and a
    /// forced Scatter from no coordinate (vt+0x174(null, 1, 1)).
    fn unit_per_cell_enter_refused(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.assign_null_destination(id, Some(rules), registry, effects);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(MissionType::Guard),
            );
        }
        let _ = self.scatter(
            id,
            None,
            super::ScatterFlags::new(true, true),
            rules,
            registry,
            effects,
        );
    }

    /// The occupier arm (`0x00519675..0x00519739`): an `Occupier=` or
    /// `Assaulter=` infantryman on Capture (vt+0x184) whose NavCom is the
    /// first ground building of the cell it reached (`0x0047C520`) asks
    /// that building `CanBeOccupiedBy`. Refused, it drops its destination
    /// (vt+0x480(0, 1)) and scatters from the building's coordinate
    /// (vt+0x174(coord, 1, 1)); admitted, it goes in
    /// ([`crate::sim::passenger::add_garrison_occupant`]). Either way the
    /// call returns before the Foot body. Answers whether the arm ran.
    ///
    /// RESIDUAL: the building's attached Tag is told it was entered
    /// (`TagClass::Raise_Event(1, ...)`, `0x00519710..0x00519725`) before
    /// the infantryman goes in; this garrison ingress does not raise event 1 yet.
    /// Trigger: a
    /// map building with a Tag garrisoned this way. Effect: its "entered by"
    /// trigger does not spring. A building a house builds has no Tag.
    fn infantry_per_cell_occupy(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, FrameAdvanceError> {
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        if infantry.mission.effective().known() != Some(MissionType::Capture)
            || !self
                .object_type(infantry.type_ref(), rules)
                .is_some_and(|object| object.occupier || object.assaulter)
        {
            return Ok(false);
        }
        let Some(building) = infantry
            .navigation
            .nav_com()
            .and_then(NavTargetRef::object_id)
            .filter(|&building| {
                self.substrate
                    .entities
                    .get(building)
                    .is_some_and(|entity| entity.category() == EntityCategory::Structure)
            })
        else {
            return Ok(false);
        };
        let (rx, ry) = (infantry.position.rx(), infantry.position.ry());
        if self
            .substrate
            .occupancy
            .first_building_on_layer(rx, ry, MovementLayer::Ground)
            != Some(building)
        {
            return Ok(false);
        }
        if crate::sim::passenger::building_can_be_occupied_by(self, rules, building, id) {
            crate::sim::passenger::add_garrison_occupant(
                self, rules, id, building, registry, effects,
            );
            return Ok(true);
        }
        self.turn_away_from_building(id, building, rules, registry, effects)?;
        Ok(true)
    }

    fn per_cell_owner_alive(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.lifecycle.object_alive())
    }

    /// `FootClass::Per_Cell_Process @ 0x004D85D0` for reason 2, ending in the
    /// Techno tail `0x006F5090`; both return at once for any other reason.
    ///
    /// Ported, in native order: the sensor deposit move (`0x004D8611`,
    /// `0x004D8621`), the neighbour history (`0x004D8627..0x004D8759`), the
    /// `Sensors=` uncloak scan (`0x004D8760..0x004D882F`), the range stop
    /// (`0x004D882F..0x004D896E`) and the Techno tail's Temporal release
    /// (`0x006F50A3..0x006F50B4`) and playfield promote (`0x006F511A`).
    ///
    /// Cell-attached 1/59 run in their original `0x004D8978..0x004D8A14`
    /// slot, followed by Map line 25/26 at `0x004D8A70..0x004D8B66`.
    /// Ordinary Fly ingress remains unproven.
    /// Agent Infantry54 follows1/59 at4D8A14..4D8A6B;53 is explicitly
    /// deferred. RESIDUAL: Map zone24 (`0x004D8B66..0x004D8D99`), the
    /// `+0x83` / `0x00586360` step (`0x004D8DA7..0x004D8DF5`), the
    /// planning-waypoint upkeep (`0x004D8DFF..0x004D8F1D`) are not ported.
    /// Trigger: every arrival. Effect: zone triggers and planning waypoints
    /// do not fire from movement. Risk: maps with those trigger families.
    ///
    /// The off-playfield exit at 0x004D8D67..0x004D8DA4 asks the class IsLeavingMap
    /// after Map578460(cell,1), then UnInit and returns before the tail.
    /// Foot uses 0x004DC790; Aircraft retains its separate 0x0041B890 override.
    ///
    /// The Techno tail ends with the open-ground discovery
    /// ([`Self::discover_on_open_ground_after_cell_entry`]) and the Cell's
    /// ActivateVeins (`0x00486920`), which acts only on the veins overlay
    /// (index 0x7E) that retail maps never place: dormant, not run.
    pub(super) fn foot_per_cell_process(
        &mut self,
        id: u64,
        reason: PerCellReason,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if reason != PerCellReason::Arrival {
            return;
        }
        if let Some(rules) = rules {
            self.refresh_unit_sensor_at_per_cell(id, rules, registry);
        }
        //4D8657..4D868D compares the two coarse threat buckets and migrates
        //the cached contribution BEFORE4D870E replaces retained Foot+55C.
        if let Some(rules) = rules {
            self.spatial_threat_at_per_cell(id, rules);
        }
        self.foot_neighbors_at_per_cell(id);
        if let Some(rules) = rules {
            crate::sim::world::techno_ai_cloak::uncloak_on_sensor_neighbour_after_cell_entry(
                self, id, rules,
            );
            self.per_cell_range_stop(id, rules, registry, effects);
        }
        self.dispatch_foot_cell_tag_events(
            id,
            crate::sim::world::UninitContext::new(rules, registry).with_effects(effects),
        );
        // Foot4D8D31..4D8D53: a Foot whose structural deck disappeared
        // takes the same DropAsBomb as BlowUpBridge's deck traversal.
        let deck_gone = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.on_bridge()
                && self.resolved_terrain.as_ref().is_some_and(|terrain| {
                    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                    let at = position_world_coord(&entity.position);
                    cells.flags(cells.lookup_world(at.x, at.y)) & 0x100 == 0
                })
        });
        if deck_gone {
            self.drop_as_bomb(
                id,
                crate::sim::world::UninitContext::new(rules, registry).with_effects(effects),
            );
        }
        // Foot4D8D59: an object that died in the Foot body skips the whole
        // Techno tail.
        if !self.per_cell_owner_alive(id) {
            return;
        }
        // Retail Foot4D8D67..4D8DA4: physical packed Cell, mode1 Map
        // lookup, virtual IsLeavingMap, then the shared UnInit owner.
        let cell = self
            .substrate
            .entities
            .get(id)
            .map(|entity| super::foot_path::coord_cell(position_world_coord(&entity.position)));
        // Partial worlds without a configured Map cannot admit an exit.
        if let (Some((x, y)), Some(bounds)) = (cell, self.playfield_bounds())
            && !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(x), i32::from(y)),
                Some(bounds),
                self.resolved_terrain.as_ref(),
            )
            && self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| match entity.category() {
                    EntityCategory::Aircraft => self.aircraft_may_leave_map(id),
                    EntityCategory::Unit | EntityCategory::Infantry => {
                        self.foot_is_leaving_map(id, rules)
                    }
                    EntityCategory::Structure => false,
                })
        {
            self.uninit_with_context(
                id,
                UninitContext::new(rules, registry).with_effects(effects),
            );
            return;
        }
        // `0x006F5090`'s head lets a held Temporal target go, then caches the
        // Cell of its GetCoords (`0x006F50B9..0x006F50F6`) for the tail.
        self.temporal_release_if_warping(id);
        let cell = self
            .substrate
            .entities
            .get(id)
            .map(|entity| (entity.position.rx(), entity.position.ry()));
        //6F50FA calls the existing cloak/sensor owner before Tag34. The
        //ordinary stock arm is dormant without a CloakGenerator field;
        //custom fields retain the owner's StartCloaking/targeter response.
        if let Some(rules) = rules {
            crate::sim::world::techno_ai_cloak::sensor_reevaluate_stock_cloak(
                self, id, rules, registry,
            );
        }
        //6F5100..6F5115: attached Tag, actor=this, zero cell/source. This
        //follows earlier cell/line callbacks and precedes playfield promotion.
        self.dispatch_object_tag_event(
            id,
            34,
            Some(id),
            None,
            UninitContext::new(rules, registry).with_effects(effects),
        );
        self.promote_entity_playfield_membership_after_move(id);
        if let Some(cell) = cell {
            self.discover_on_open_ground_after_cell_entry(
                id,
                cell,
                UninitContext::new(rules, registry).with_effects(effects),
            );
        }
    }

    /// The Techno tail's discovery (`0x006F5140..0x006F516C`): an object the
    /// current house (`0x00A83D4C`) has not discovered (`+0x41B`) that enters
    /// a Cell whose ground it has open (`CellClass+0x12C` bit `0x10`, the Cell
    /// of its GetCoords) is DiscoveredBy that house (vt+0x198). VERA's
    /// shroud-refresh discovery covers Cells being revealed; this covers an
    /// object walking into ground already open.
    fn discover_on_open_ground_after_cell_entry(
        &mut self,
        id: u64,
        (rx, ry): (u16, u16),
        context: UninitContext<'_>,
    ) {
        let Some(viewer) = self.session.current_house() else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.discovery.discovered_by_current_house()
            || !self.fog.is_ground_open(viewer, rx, ry)
        {
            return;
        }
        self.discover_techno(id, viewer, context);
    }

    /// The range stop (`0x004D882F..0x004D896E`): the class
    /// `Set_Destination(NULL, 1)` (vt `+0x480`), then `+0x5E0 = -1`, which
    /// runs even when the class refuses. See
    /// tools/spatial_oracle/walk_percell_stop.{py,json,meta.json}.
    ///
    /// A Unit takes it from its locomotor's PerCell, a Jumpjet from its
    /// descent; every infantryman takes Infantry `0x0051AA40`, a Rocketeer
    /// touching down in range included.
    fn per_cell_range_stop(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(category) = self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.category())
        else {
            return;
        };
        if !self.foot_per_cell_range_stop(id, rules, registry) {
            return;
        }
        match category {
            EntityCategory::Unit => {
                self.set_unit_null_destination(id, Some(rules), registry, effects);
            }
            EntityCategory::Infantry => {
                // With no head and no destination left, the next Walk
                // Process takes its idle tail.
                self.set_infantry_null_destination(id, Some(rules), registry, effects);
            }
            _ => {}
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            // Keep the backing suffix/cursor/reference; this is one DWORD.
            entity.navigation.path_replay_mut().clear_live_head();
        }
    }
}

#[cfg(test)]
#[path = "per_cell_exit_tests.rs"]
mod exit_tests;

#[cfg(test)]
#[path = "per_cell_crush_tests.rs"]
mod crush_tests;

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;

    /// The Techno per-cell tail's discovery (`0x006F5140..0x006F516C`): an
    /// enemy arriving on ground the current house has open is discovered;
    /// over shrouded ground, with no current house, or once dead in the Foot
    /// body (`0x004D8D59`), it is not.
    #[test]
    fn an_enemy_arriving_on_open_ground_is_discovered_by_the_current_house() {
        let mut sim = Simulation::new();
        let player = sim.interner.intern("Americans");
        let enemy = sim.interner.intern("Russians");
        sim.houses
            .register(HouseState::new(player, 0, None, true, 0, 10));
        sim.houses
            .register(HouseState::new(enemy, 1, None, false, 0, 10));
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        let type_ref = sim.interner.intern("TEST");
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                1,
                5,
                5,
                0,
                0,
                enemy,
                Health::new(100),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            ));
        let arrive = |sim: &mut Simulation| {
            sim.foot_per_cell_process(
                1,
                PerCellReason::Arrival,
                None,
                None,
                FrameEffects::default(),
            );
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .discovery
                .discovered_by_current_house()
        };
        sim.fog.set_ground_bits_for_test(player, 5, 5, 0x18);
        assert!(!arrive(&mut sim), "no current house");
        sim.session.set_current_house_for_test(Some(player));
        sim.fog.set_ground_bits_for_test(player, 5, 5, 0x08);
        assert!(!arrive(&mut sim), "visible but not open");
        sim.fog.set_ground_bits_for_test(player, 5, 5, 0x18);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(false);
        assert!(!arrive(&mut sim), "a dead object skips the tail");
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(true);
        assert!(arrive(&mut sim));
    }
}
