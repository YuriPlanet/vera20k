//! Verb-driven retasking on [`Simulation`] — the single funnel the player-command
//! sites use to queue a fresh mission.
//!
//! The mission write is the native event-execute shape: synchronized command
//! execution queues the selected mission with `commence_now = 0`
//! (`EventClass` execute, Queue callsite `0x004C73B9=dynamic/0`); the
//! per-object AI host promotes it via Ready→Commence on its next update. The
//! legacy `Option<T>` machines stay the behavior drivers; the per-site field
//! clears (`attack_target`/`order_intent`/`c4_plant`) stay inline at the call
//! site — the sites cancel different field subsets, so they cannot be folded
//! into a fixed teardown without diverging.

use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{self, RadioMessage};
use crate::sim::world::Simulation;

impl Simulation {
    /// Accepted MEGAMISSION's shared prefix, EventClass4C72E8..4C7380.
    /// Untethered actors break slot zero (vt27465ACB0 at4C72F8). Tethered
    /// actors do so only for a natively alive DockUnload Building, then clear
    /// tether+418 at4C7342. Health is not part of that contact test.
    /// Pending entry+500 clears at4C7353, before Team removal and Queue.
    /// Original executable controls: building_repair.depot_service.json,
    /// pending_entry_lifecycle (declared Event slices, full Scenario RNG).
    pub(crate) fn begin_megamission_retask(
        &mut self,
        id: u64,
        mission: MissionType,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let tethered = entity.dock_entered_with.is_some();
        let foot = matches!(
            entity.category,
            crate::map::entities::EntityCategory::Unit
                | crate::map::entities::EntityCategory::Infantry
                | crate::map::entities::EntityCategory::Aircraft
        );
        let break_tether = tethered
            && entity
                .radio_contacts
                .slot(0)
                .and_then(|contact| self.substrate.entities.get(contact))
                .filter(|contact| {
                    contact.is_object_alive()
                        && contact.category == crate::map::entities::EntityCategory::Structure
                })
                .zip(rules)
                .and_then(|(contact, rules)| self.object_type(contact.type_ref(), rules))
                .is_some_and(|object| object.dock_unload);
        if !tethered || break_tether {
            radio::transmit_to_contact(self, id, RadioMessage::Break, rules);
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            if break_tether {
                entity.dock_entered_with = None;
            }
            entity.set_pending_entry(None);
        }
        // Native4C735D..4C7380: Foot membership removal without an idle
        // order, except Unload. The existing Team owner checks membership.
        if foot && mission != MissionType::Unload {
            self.leave_team(id, true, rules);
        }
    }

    /// `TechnoClass::ResetOrdersToGuard` (`vt+0x3D0` = `0x0070F850`, no
    /// class override): the class setter `vt+0x480(0, 1)` (`0x0070F859`),
    /// class `vt+0x3C8(0)` (`0x0070F865`), ArchiveTarget (`+0x218`)
    /// cleared (`0x0070F871`), then `Assign_Mission(Guard)` (`0x0070F87B`).
    /// Callers: the capture reset (`0x00471EA5`)
    /// and the slave manager (the deploy hand-off `0x006B0D10` and the
    /// release `0x006B0BC5`).
    pub(crate) fn reset_orders_to_guard(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let now = self.session.binary_frame;
        self.assign_null_destination(id, Some(rules), None);
        let _ = self.assign_target_represented(id, None, Some(rules));
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.movement_target = None;
            entity.set_archive_target(None);
        }
        let _ = self.mission_assign_exact(id, MissionId::from_known(MissionType::Guard), now);
    }

    /// Retask `id` onto a fresh `mission`: queue the mission through the
    /// exact authority with `commence_now = 0` — the
    /// native event-execute shape (Queue `0x004C73B9=dynamic/0`). Promotion to
    /// `current` happens at the per-object AI host's Ready→Commence. Used by
    /// the shared MegaMission command funnel (Move, Attack, ForceAttack,
    /// ForceAttackCell, AttackMove, Guard, RepairAtDepot, EnterTransport, PlantC4,
    /// CaptureBuilding). Stop has its own no-queue Event6 route above.
    pub fn queue_order_mission(&mut self, id: u64, mission: MissionType) {
        let now = self.session.binary_frame;
        // A missing receiver performs no write (same as the native event path
        // skipping a dead target); Queue's own guards decide the rest.
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(mission),
            0,
            now,
            &EntityReadyInputProvider,
        );
    }

    /// The same retask, plus the archive clears that belong to the
    /// MEGAMISSION event specifically.
    ///
    /// Immediately after its `Queue_Mission` at 0x004C73B9,
    /// `EventClass::Execute`'s MEGAMISSION arm empties the archives:
    /// `MOV [EDI+0x5A8],0` (SuspendedNavCom) at 0x004C73C7 on the Foot arm the
    /// `TEST byte [EDI+0x14],0x4` at 0x004C73BF selects, and
    /// `MOV [EDI+0x2B8],0` (SuspendedTarCom) at 0x004C73D7 on the join — which
    /// the non-Foot arm also reaches, via `0x004C7440: XOR EBP,EBP;
    /// JMP 0x004C73D1`. `SuspendedMission` itself is deliberately left alone, so
    /// a later Restore reinstates the old selector with a NULL target and NULL
    /// destination.
    /// After the Slave Manager reset, the ordinary Foot arm clears
    /// ArchiveTarget through TechnoClass70C610 (`0x004C7448..0x004C7451`),
    /// before the class target and destination setters. The executed GI
    /// `archive_A_B` control in `walk_first_path.json` records that order.
    /// Raw event mission 11 takes the earlier AreaGuard arm instead.
    ///
    /// This is NOT the shared funnel's business, because not every player order
    /// is a MEGAMISSION. Stop is its own opcode — `StopCommandClass::Execute`
    /// pushes 6 at 0x00730EE7, and the table at 0x004C8114 routes 6 to
    /// 0x004C74CB. That arm DOES queue a mission in one case, the ore miner on
    /// Harvest or Return: `PUSH EDI; PUSH 5; CALL [EDX+0x1E8]` at 0x004C7685
    /// (EDI is the function's zero register there) then Commence at 0x004C7696,
    /// which `commit_stop_miner_guard` below implements. What it does not do,
    /// anywhere in 0x004C74CB-0x004C76BB, is
    /// store to `+0x2B8` or `+0x5A8`. Clearing there would leave a unit parked
    /// after a Stop where retail resumes its archived move on the next Restore.
    ///
    /// MinerReturn and HarvestCell use the shared pre-queue prefix,
    /// but their legacy mission paths still bypass the post-Queue archive
    /// clears here. EjectBunker and dormant aircraft passenger Unload also
    /// bypass the prefix. These pre-existing command routes can Restore a
    /// cancelled archive; their complete DTO/mission migration is separate
    /// from ordinary Unit depot servicing.
    ///
    /// Without the clear on the sites that DO need it, a unit Overridden by a
    /// blocked step or by the retaliation at 0x00702B41, then retasked, then
    /// losing its new target, marched back to the destination the player had
    /// cancelled or re-latched the cancelled target.
    pub fn queue_megamission(
        &mut self,
        id: u64,
        mission: MissionType,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        self.begin_megamission_retask(id, mission, rules);
        self.queue_order_mission(id, mission);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.suspended_attack_target = None;
            if entity.category != crate::map::entities::EntityCategory::Structure {
                entity.navigation.suspended_nav_com = None;
            }
        }
        // `0x004C73E1..0x004C73EA`: any order but Attack takes a Slave Miner
        // off its hunt (`sim::slave_manager`).
        if mission != MissionType::Attack
            && let Some(rules) = rules
        {
            self.reset_slave_manager(id, rules);
        }
        // Event4C7446 tests the Foot receiver; non-Foot actors skip this
        // call. Raw mission11 uses AreaGuard's separate Archive assignment
        // at4C7430. AttackMove29 still uses VERA's deferred OrderIntent
        // adapter rather than Foot4DF0E0's raw-event mapping; preserve that
        // adapter here pending its recorded migration (#852/#1023). This
        // exception is not native raw29 behavior, which clears the Archive.
        if !matches!(mission, MissionType::AreaGuard | MissionType::AttackMove)
            && let Some(entity) = self.substrate.entities.get_mut(id)
            && matches!(
                entity.category,
                crate::map::entities::EntityCategory::Unit
                    | crate::map::entities::EntityCategory::Infantry
                    | crate::map::entities::EntityCategory::Aircraft
            )
        {
            entity.set_archive_target(None);
        }
    }

    /// The ore-miner arm of the Stop command.
    ///
    /// Retail's IDLE event handler ends with, for a `UnitClass` carrying the
    /// ore-miner type flag whose committed mission is `Harvest` or `Return`:
    /// `Queue_Mission(Guard, 0); Commence();` — an unconditional promotion, not
    /// the readiness-gated one the per-object AI host performs. So the miner is
    /// on Guard with a zero dispatch delay on the same tick, and the Harvest
    /// handler stops being dispatched for it.
    ///
    /// Everything else Stop touches is left alone; this is the only mission
    /// write retail's Stop performs on any object.
    ///
    /// The flag is the type's `Harvester=` (`+0xE0E`, `0x004C766B`): every
    /// VERA miner but the Slave Miner, whose Miner component is only an
    /// order marker (`sim::slave_manager`).
    pub fn commit_stop_miner_guard(&mut self, id: u64) {
        let is_stoppable_miner = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.category == crate::map::entities::EntityCategory::Unit
                && entity.is_harvester()
                && matches!(
                    entity.mission.current().known(),
                    Some(MissionType::Harvest) | Some(MissionType::Return)
                )
        });
        if !is_stoppable_miner {
            return;
        }
        let now = self.session.binary_frame;
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Guard),
            0,
            now,
            &EntityReadyInputProvider,
        );
        let _ = self.mission_commence_exact(id, now);
    }
}
