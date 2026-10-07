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
//! ends in the Techno tail `0x006F5090`. Fly has no caller.

use super::ground_pose::position_world_coord;
use super::locomotor::MovementLayer;
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::lifecycle_request::{LifecycleRequest, UninitReason};
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{self, RadioMessage, RadioPayload};
use crate::sim::world::{FrameAdvanceError, Simulation};

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
    /// The virtual call: the Unit or Infantry override, else the Foot body.
    /// Returns whether the Infantry override changed a bridge.
    pub(crate) fn per_cell_process(
        &mut self,
        id: u64,
        reason: PerCellReason,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, FrameAdvanceError> {
        match self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.category)
        {
            Some(EntityCategory::Unit) => {
                self.unit_per_cell_process(id, reason, rules, registry);
                Ok(false)
            }
            Some(EntityCategory::Infantry) => {
                self.infantry_per_cell_process(id, reason, rules, registry)
            }
            Some(_) => {
                self.foot_per_cell_process(id, reason, rules, registry);
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
            if building.category != EntityCategory::Structure { return None; }
            let at = super::ground_pose::object_get_coords(unit, self.resolved_terrain.as_ref());
            let cell = ((at.x /256) as u16, (at.y /256) as u16);
            let dock = super::building_dock_cell(&self.substrate.entities, contact, Some(id),
                self.resolved_terrain.as_ref(), rules, &self.interner)?;
            if cell != dock { return None; }
            let restore = self.object_type(building.type_ref(), rules).is_some_and(|object| object.unit_repair)
                // Literal GUID7E9A40 is Hover (4A582742), not Drive
                // (4A582741 at7E9A30). Native ctor/GetClassID controls pin it.
                && unit.locomotor.as_ref().is_some_and(|locomotor| locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Hover)
                && unit.navigation.nav_com.is_none();
            let aimed = matches!(unit.navigation.nav_com,
                Some(crate::sim::components::NavTargetRef::Entity { id: nav }
                    | crate::sim::components::NavTargetRef::Object { id: nav }
                    | crate::sim::components::NavTargetRef::Building { id: nav }) if nav == contact);
            (restore || aimed).then_some((contact, restore))
        });
        if let Some((contact, restore)) = object_dock {
            if restore && let Some(unit) = self.substrate.entities.get_mut(id) {
                // Unit73A4E9 restores only the raw NavCom; unlike the Foot
                // setter this does not clear NavComAux or change movement.
                unit.navigation.nav_com =
                    Some(crate::sim::components::NavTargetRef::Building { id: contact });
            }
            self.foot_per_cell_process(id, PerCellReason::Arrival, Some(rules), registry);
            crate::sim::radio::transmit_to_contact(
                self,
                id,
                crate::sim::radio::RadioMessage::DockNow,
                Some(rules),
            );
            if let Some(unit) = self.substrate.entities.get_mut(id)
                && let Some(locomotor) = unit.locomotor.as_mut()
            {
                locomotor.power_off();
            }
            return true;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.dock_entered_with.is_none()
            || entity.mission.effective()
                != crate::sim::mission::MissionId::from_known(MissionType::Enter)
        {
            return false;
        }
        let Some(contact) = entity.radio_contacts.slot(0) else {
            return false;
        };
        let (x, y) = (entity.position.rx, entity.position.ry);
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
    ) {
        // Unit739EC0 invokes the MCV receiver before normal crush/Foot tail.
        if let Some(rules) = rules {
            crate::sim::mcv_deploy::per_cell_process(self, id, rules, registry);
        }
        if !self.track_survives(id) {
            return;
        }
        // 0x0073A31F..0x0073A5EA, before the Ready/Commence below: a tethered
        // unit on Enter arriving north-adjacent to its dock sends DOCK_NOW.
        if let Some(rules) = rules
            && reason == PerCellReason::Arrival
        {
            // The object-destination docking arm73A31F..A547 completes
            // Foot's tail and returns before the later factory clearance.
            if self.unit_dock_now(id, rules, registry) {
                return;
            }
            self.unit_per_cell_factory_clearance(id, rules, registry);
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
                    .is_some_and(|miner| miner.unload_active)
            });
        if let Some(rules) = rules.filter(|_| promote) {
            self.mission_host_promote(id, self.session.binary_frame, rules);
        }
        // 0x0073ACD7..0x0073ADC4: a harvester (or weeder) off Enter/Unload
        // drops a refinery (weeder) contact at every track end.
        if let Some(rules) = rules
            && reason == PerCellReason::Arrival
        {
            crate::sim::miner::per_cell_release_dock_contact(self, rules, id);
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let at = (entity.position.rx, entity.position.ry);
        let layer = if entity.on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let mut cursor = self
            .substrate
            .occupancy
            .get(at.0, at.1)
            .and_then(|list| list.first_on_layer(layer));
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
                crusher.regular_crusher,
                crusher.omni_crusher,
            );
            let kills = super::bump_crush::select_crush_victims(
                &[victim],
                &self.substrate.entities,
                id,
                &self.house_alliances,
                &self.interner,
                (coord.x, coord.y),
                capability,
                self.session.binary_frame,
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
                let owner = self.substrate.entities.get(id).map(|entity| entity.owner());
                if let Some(entity) = self.substrate.entities.get_mut(victim) {
                    entity.health.current = 0;
                }
                self.record_the_kill(
                    victim,
                    Some(id),
                    owner,
                    crate::sim::combat::KillCallback::Terminal,
                    rules,
                );
                self.apply_lifecycle_request_with_rules(
                    LifecycleRequest::Uninit {
                        stable_id: victim,
                        reason: UninitReason::Crush,
                    },
                    rules,
                    registry,
                );
            } else {
                self.apply_lifecycle_request(LifecycleRequest::Uninit {
                    stable_id: victim,
                    reason: UninitReason::Crush,
                });
            }
        }
        // 0x0073B08F tests only IsAlive (`+0x90`) before the Foot body.
        if self.per_cell_owner_alive(id) {
            self.foot_per_cell_process(id, reason, rules, registry);
        }
    }

    /// Unit739EC0's tethered arrival chapter73A7D2..73AADB, before its
    /// Ready/Commence checkpoint. The human continuation calls the existing
    /// archive destination, FootStop4DF0D0 and UnitScatter743A50 owners.
    /// Source: anytown_damage/unit_unlimbo radio receipt and original Unit
    /// listing; the composed factory-Unload control binds the null-NavCom arm.
    fn unit_per_cell_factory_clearance(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        if unit.dock_entered_with.is_none() {
            return;
        }
        let nav_com = unit.navigation.nav_com;
        let nav_techno = match nav_com {
            Some(
                NavTargetRef::Entity { id }
                | NavTargetRef::Object { id }
                | NavTargetRef::Building { id },
            ) => crate::sim::radio::as_techno(self, id),
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
        let at_destination = nav_com.is_none()
            || super::navcom::nav_targets_same_receiver(
                nav_com,
                NavTargetRef::cell(cell.0, cell.1),
            );
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
            && contact
                .and_then(|contact| self.substrate.entities.get(contact))
                .filter(|contact| contact.category == EntityCategory::Structure)
                .and_then(|contact| self.object_type(contact.type_ref(), rules))
                .is_some_and(|object| object.weapons_factory)
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
        );
        if reply != crate::sim::radio::RadioResponse::Queued {
            // RESIDUAL:73AAF7..73AB66's non23 replies include harvester
            // archive/Harvest and Scatter work. Refinery/service departure is
            // a separate lifecycle; no factory-specific substitute runs here.
            return;
        }
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        let nav_com = unit.navigation.nav_com;
        if nav_com.is_some()
            && !super::navcom::nav_targets_same_receiver(
                nav_com,
                NavTargetRef::cell(cell.0, cell.1),
            )
        {
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
                self.session.binary_frame,
                &LiveReadyInputProvider { rules },
            );
            return;
        }
        if unit.slave_manager.is_some() {
            // RESIDUAL:73A9D4 invokes the existing SlaveManager6B0CC0 owner.
            // The current delivery adapter starts its hunt earlier; its exact
            // arrival integration remains a manager-lifecycle dependency.
            return;
        }
        let producer = contact
            .and_then(|contact| self.substrate.entities.get(contact))
            .filter(|contact| contact.category == EntityCategory::Structure);
        let human = self
            .houses
            .get(&unit.owner())
            .map(|house| house.is_controlled_by_human(self.session.game_mode_nonzero));
        if human.is_none()
            || (human == Some(false)
                && producer
                    .and_then(|producer| self.object_type(producer.type_ref(), rules))
                    .is_some_and(|object| object.weapons_factory))
        {
            // RESIDUAL:73AA19's House500200 placement/spreading owner is
            // unported. It draws Scenario RNG for armed computer units,
            // queues Move, Commences, archives its post and queues AreaGuard.
            // A missing House cannot stand in for Human50B730.
            return;
        }
        let archive = unit.archive_target();
        if let Some(archive) = archive
            .filter(|archive| Some(*archive) != nav_com.map(crate::sim::combat::TargetKind::from))
        {
            self.set_unit_destination(id, NavTargetRef::from(archive), rules, true);
        } else {
            if let Some(unit) = self.substrate.entities.get_mut(id) {
                super::navcom::foot_stop_moving(unit);
            }
            //73AAC8..73AADB: no QueueMission, Process or EnterIdle. Unit's
            // null Scatter receiver cannot produce the Infantry error path.
            let _ = self.scatter_null(
                id,
                super::scatter::ScatterFlags::new(true, false),
                rules,
                registry,
            );
        }
        // RESIDUAL:73AB6C..73ABD0 rereads the first ground Building, NavCom,
        // and distinct counts+5BC/+598 before Scatter(NULL,1,1). NavQueue
        // represents+598; vector+5AC/count+5BC is not represented. Its full
        // producer/consumer/save/expiry chain is required before this fallback
        // can be ported; SuspendedNavCom(+5A8) is not that vector. The composed
        // fresh human control publishes NavCom in the first Scatter and skips
        // this second call, so no invented count or factory marker is needed.
    }

    /// `InfantryClass::Per_Cell_Process @ 0x00519630`: for reason 2 its
    /// Engineer building receiver (`0x00519948..0x0051A02E`), tethered radio8
    /// release (`0x0051A7F8..0x0051A812`), then the Foot body for a live owner
    /// (`0x0051A9EB` tests only IsAlive, `+0x90`).
    ///
    /// RESIDUAL: the override's other reason-2 arms (other Capture branches, Eaten, Enter,
    /// the transport and C4 receivers, `0x00519675..0x0051A9E8`) are not
    /// dispatched from here. Some have ports with their own owners and
    /// callers (`capture_manager`, `passenger`, `world_orders`); native runs
    /// them inside this call, before the Foot body, and several return
    /// without it. Trigger: an infantry arriving on a mission cell. Risk:
    /// same-frame order between those receivers and the Foot body.
    fn infantry_per_cell_process(
        &mut self,
        id: u64,
        reason: PerCellReason,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, FrameAdvanceError> {
        let entry = match rules {
            Some(rules) if reason == PerCellReason::Arrival => {
                self.infantry_per_cell_engineer_entry(id, rules, registry)?
            }
            _ => Default::default(),
        };
        if !entry.return_before_foot {
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
                    .is_some_and(|entity| entity.dock_entered_with.is_some())
            {
                radio::transmit_to_contact(self, id, RadioMessage::RequestClearance, rules);
            }
            if self.per_cell_owner_alive(id) {
                self.foot_per_cell_process(id, reason, rules, registry);
            }
        }
        Ok(entry.bridge_state_changed)
    }

    fn per_cell_owner_alive(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.lifecycle.object_alive)
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
    /// RESIDUAL: the cell tag events (`0x004D8978..0x004D8D99`), the
    /// `+0x83` / `0x00586360` step (`0x004D8DA7..0x004D8DF5`), the
    /// planning-waypoint upkeep (`0x004D8DFF..0x004D8F1D`) and the Techno
    /// tail's `+0x420` call, tag event 0x22, `+0x198` call and cell
    /// `0x00486920` are not ported. Trigger: every arrival. Effect: cell-tag
    /// triggers and planning waypoints do not fire from movement. Risk: maps
    /// with cell triggers.
    pub(super) fn foot_per_cell_process(
        &mut self,
        id: u64,
        reason: PerCellReason,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if reason != PerCellReason::Arrival {
            return;
        }
        if let Some(rules) = rules {
            self.refresh_unit_sensor_at_per_cell(id, rules);
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
            self.per_cell_range_stop(id, rules, registry);
        }
        // `0x006F5090`'s head lets a held Temporal target go.
        self.temporal_release_if_warping(id);
        self.promote_entity_playfield_membership_after_move(id);
    }

    /// The range stop (`0x004D882F..0x004D896E`): the class
    /// `Set_Destination(NULL, 1)` (vt `+0x480`), then `+0x5E0 = -1`, which
    /// runs even when the class refuses. See
    /// tools/spatial_oracle/walk_percell_stop.{py,json,meta.json}.
    ///
    /// A Unit takes only the `OpenTopped=` arm here; the pursuit stage's
    /// in-range halt (`world_orders.rs`) stands in for its InRange arm, tested
    /// each frame. Every infantryman takes Infantry `0x0051AA40`, a Rocketeer
    /// touching down in range included.
    fn per_cell_range_stop(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(category) = self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.category)
        else {
            return;
        };
        if category == EntityCategory::Unit
            && !self
                .substrate
                .entities
                .get(id)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .is_some_and(|obj| obj.open_topped)
        {
            return;
        }
        if !self.foot_per_cell_range_stop(id, rules, registry) {
            return;
        }
        match category {
            EntityCategory::Unit => {
                self.set_unit_null_destination(id, Some(rules), registry);
            }
            EntityCategory::Infantry => {
                // With no head and no destination left, the next Walk
                // Process takes its idle tail and retires the adapter.
                self.set_infantry_null_destination(id, Some(rules), registry);
            }
            _ => {}
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            // Keep the backing suffix/cursor/reference; this is one DWORD.
            entity.navigation.path_replay.clear_live_head();
        }
    }
}
