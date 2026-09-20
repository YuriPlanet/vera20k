//! Verb-driven retasking on [`Simulation`] — the single funnel the player-command
//! sites use to queue a fresh mission.
//!
//! The mission write is the native event-execute shape: synchronized command
//! execution queues the selected mission with `commence_now = 0`
//! (`EventClass` execute, Queue callsite `0x004C73B9=dynamic/0`); the
//! per-object AI host promotes it via Ready→Commence on its next update. The
//! legacy `Option<T>` machines stay the behavior drivers; the per-site field
//! clears (`attack_target`/`order_intent`/`dock_state`/`c4_plant`/
//! `capture_target`/aircraft dock phase) stay inline at the call site — the
//! sites cancel different field subsets, so they cannot be folded into a fixed
//! teardown without diverging.

use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;

/// Which dock-reservation teardown a retasking command performs.
///
/// This governs **only** the three reservation helpers — it is the one part of
/// the per-command teardown that is a closed, enumerable set. The variant for
/// each site is the exact subset that site cancels today:
///
/// | site | variant | cancels |
/// |---|---|---|
/// | Move, Stop | `All` | depot + aircraft RTB/wait + docked-idle |
/// | RepairAtDepot | `Depot` | depot reservation only |
/// | Attack | `AircraftOnly` | aircraft RTB/wait + docked-idle (NOT depot) |
/// | ForceAttack, ForceAttackCell, AttackMove | `IdleOnly` | docked-idle only |
/// | EnterTransport, PlantC4, CaptureBuilding | `None` | nothing |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockTeardown {
    /// Depot + aircraft RTB/wait + docked-idle (Move, Stop).
    All,
    /// Depot reservation only (RepairAtDepot).
    Depot,
    /// Aircraft RTB/wait + docked-idle, but NOT the depot reservation (Attack).
    AircraftOnly,
    /// Docked-idle helipad release only (ForceAttack, ForceAttackCell, AttackMove).
    IdleOnly,
    /// No dock reservation touched (EnterTransport, PlantC4, CaptureBuilding).
    None,
}

impl Simulation {
    /// Run the dock-reservation subset selected by `teardown`. Each branch calls
    /// the exact reservation helpers the corresponding command sites call today.
    pub(crate) fn run_dock_teardown(&mut self, id: u64, teardown: DockTeardown) {
        match teardown {
            DockTeardown::All => {
                self.cancel_depot_dock(id);
                self.cancel_aircraft_dock(id);
                self.release_docked_idle(id);
            }
            DockTeardown::Depot => {
                self.cancel_depot_dock(id);
            }
            DockTeardown::AircraftOnly => {
                self.cancel_aircraft_dock(id);
                self.release_docked_idle(id);
            }
            DockTeardown::IdleOnly => {
                self.release_docked_idle(id);
            }
            DockTeardown::None => {}
        }
    }

    /// Retask `id` onto a fresh `mission`: run the dock teardown, then queue
    /// the mission through the exact authority with `commence_now = 0` — the
    /// native event-execute shape (Queue `0x004C73B9=dynamic/0`). Promotion to
    /// `current` happens at the per-object AI host's Ready→Commence. Used by
    /// every player command site (Move, Stop, Attack, ForceAttack,
    /// ForceAttackCell, AttackMove, RepairAtDepot, EnterTransport, PlantC4,
    /// CaptureBuilding).
    pub fn queue_mission_with_teardown(
        &mut self,
        id: u64,
        mission: MissionType,
        teardown: DockTeardown,
    ) {
        self.run_dock_teardown(id, teardown);
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

    /// The same retask, plus the Override-archive clear that belongs to the
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
    /// Six commands `command_uses_megamission` also counts — Guard, MinerReturn,
    /// EjectBunker, UnloadPassengers, HarvestCell, ToggleInfantryDeploy — write
    /// their missions outside this funnel entirely and so still get no clear.
    /// Pre-existing, not narrowed by the split. Trigger: one of those issued to
    /// a unit already carrying an Override archive. Player effect: a later
    /// Restore hands back a destination or target the order should have
    /// cancelled. Frequency: Guard and MinerReturn are common orders, but the
    /// archive has to be live for it to matter. Downstream risk: none — each is
    /// one call away from the same helper.
    ///
    /// Without the clear on the sites that DO need it, a unit Overridden by a
    /// blocked step or by the retaliation at 0x00702B41, then retasked, then
    /// losing its new target, marched back to the destination the player had
    /// cancelled or re-latched the cancelled target.
    pub fn queue_megamission_with_teardown(
        &mut self,
        id: u64,
        mission: MissionType,
        teardown: DockTeardown,
    ) {
        // The radio break precedes the Queue (`0x004C72E8..0x004C7342` run
        // before `0x004C73B9`).
        crate::sim::miner::miner_dock::break_for_retask(self, id);
        self.queue_mission_with_teardown(id, mission, teardown);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.suspended_attack_target = None;
            if entity.category != crate::map::entities::EntityCategory::Structure {
                entity.navigation.suspended_nav_com = None;
            }
        }
    }

    /// **Open: what a miner does when a Move order it was given finishes.**
    ///
    /// The carried-over finding said "a miner retasked onto Move or Attack
    /// still resumes harvesting when the order ends; retail does not". Read off
    /// the binary, that is only half right, and the half it gets wrong is the
    /// common case. `UnitClass`'s arrival hook has a dedicated harvester arm,
    /// and for a **player-controlled** miner arriving from a Move it decides on
    /// the arrival cell's own contents:
    ///
    /// - arrival cell carries the miner's ore kind → `Queue_Mission(Harvest)`;
    /// - it does not → `Queue_Mission(Guard)`;
    /// - and before either, `Assign_Target(NULL)` + `Set_Destination(NULL, true)`.
    ///
    /// Two earlier exits skip the decision entirely: a path that still has
    /// steps left, and a miner already on (or already queued onto) Harvest.
    ///
    /// So "send the miner to that other ore field" — much the commonest miner
    /// order — legitimately resumes harvesting in retail, and VERA's current
    /// behaviour is right there. The genuinely wrong case is the *park* order:
    /// pulling a miner out of a raid or onto a non-ore cell leaves retail's on
    /// Guard and VERA's back at work.
    ///
    /// Not closed here, deliberately. Closing it needs (a) the harvest dispatch
    /// to decline a Move-mission miner — that gate lives in `sim/miner`, not in
    /// this module — and (b) an ore-kind test on the arrival cell threaded into
    /// the Move handler's arrival arm. Landing (a) without (b) would turn every
    /// "move the miner to that ore field" order into a permanent stop, which is
    /// worse than the current drift.
    ///
    /// Frequency of the residual: park/retreat orders on miners — a few times
    /// per match for an active player, zero for a passive one.
    ///
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
    pub fn commit_stop_miner_guard(&mut self, id: u64) {
        let is_stoppable_miner = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.category == crate::map::entities::EntityCategory::Unit
                && entity.miner.is_some()
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
