//! Harvest mission handler — host-dispatched (AI-shell migration, the L5
//! seam grown into the real dispatch point).
//!
//! The per-object AI host (the Unit arm of `techno_ai_shell`, at the native
//! Mission_Dispatch position: after the AI-counter/promotion step, before the
//! post-mission common block) calls [`dispatch_harvest_for_object`] for every
//! live Unit. The dispatch is timer-gated and ends with the verified
//! post-handler epilogue write (host shape: `timer.start = current frame`,
//! `timer.delay = handler return` — the same write every native mission case
//! performs after its handler call).
//!
//! Authority: `MissionCom::handler_state` is the FSM cursor of record; the
//! bespoke `miner.state` field is retired. The handler decodes the cursor into
//! `MinerSnapshot::state`, runs the FSM step, and commits the cursor + the
//! dispatch delay back through the mission component.
//!
//! Cadence (native rows: tools/spatial_oracle/harvest_field.json and
//! refinery_dock.json): the harvesting and dock states, the full search
//! state and the search that finds the miner's own cell return
//! `DISPATCH_NEXT_FRAME` (per-frame); the return/finding-home state, the
//! idle state, the search state's archive and scan-hit drives and its
//! still-driving returns, and every cursor outside the native switch exit
//! through the default epilogue
//! (`ftol([Harvest] Rate × 900)` + `RandomRanged(0,2)` on the scenario
//! stream, ~14-16 frames stock); the no-ore transition into idle returns the
//! fixed 105-frame wait with no RNG draw. The Mission_Deploy state-4 dock
//! exit installs the same Rate epilogue at its own site.
//!
//! Structural residuals (native returns with no Rust dispatch equivalent):
//! the 450-frame non-harvester hold — dispatch is gated on Miner-component
//! presence, so a non-harvester never reaches the handler; and the
//! slave-host preamble (`0x0073E5E9`, HandleReturnedSlaves) — a Slave Miner
//! never reaches this handler; its slaves are `sim::slave_manager`'s.
//!
//! Guard hand-offs (`UnitClass::Mission_Harvest @ 0x0073E5E0`): the preamble
//! queues Guard when the house owns no instance of any `Dock=` type, and
//! state 4 (105 frames after a scan miss) queues Guard after moving the
//! miner off a refinery cell — both `Queue_Mission(5, 0)`, promoted by the
//! host's Ready-to-Commence step. A miner on Guard is no longer dispatched
//! here; the harvester Guard override's chrono arms
//! (`techno_ai/mission_handlers.rs`) or a player order (`Command::HarvestCell`
//! → `mission_assign_exact(Harvest)`, `Command::MinerReturn`) put it back.
//!
//! Dispatch gating residual: the host dispatches on Miner-component presence
//! plus "the committed mission is neither Guard nor Move", not strictly on
//! `current == Harvest`. Guard is gated because the Stop command force-assigns
//! it to a harvesting miner exactly where the retail IDLE event handler does,
//! and declining it here is what makes Stop actually stop the miner. Move is
//! gated because a player Move runs the Foot Move handler (`FootClass::
//! Mission_Move @ 0x004D4200`), whose arrival `Enter_Idle_Mode(0,1)` takes the
//! harvester arm of `UnitClass::Enter_Idle_Mode @ 0x00738970` — Harvest, or
//! Guard for a human whose miner stopped on non-ore land — modelled in
//! `techno_ai/mission_handlers.rs::harvester_enter_idle_mode_evaluation`; the
//! re-committed Harvest restarts this handler from state 0. An Attack retask
//! still flips `current` away from Harvest while the legacy FSM keeps driving
//! (pre-absorption behaviour, kept until the Attack arrival is modelled). The
//! strict mission-id gate becomes exact once the creation caller family
//! (roadmap Track B1) commits missions at those points.
//!
//! Depends on: `world::Simulation`, `miner::miner_system`.
//! Must NOT depend on render/ui/sidebar/audio/net (sim invariant #1).
//! Dispatch stays a `match` on the FSM cursor — no trait / dyn / vtable
//! (invariant #2).

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

use super::miner_system::{
    MinerSnapshot, build_miner_snapshot, commit_miner_snapshot, process_miner,
};
use super::{MinerKind, MinerState};

/// Host dispatch point: run one timer-gated Harvest handler step for `id`.
///
/// Called from the Unit arm of the per-object AI shell in live-object order.
/// No-op for objects that are not live, dispatchable miners, and for miners
/// whose dispatch timer is still pending.
pub(crate) fn dispatch_harvest_for_object(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &super::MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
) {
    let now = sim.session.binary_frame;
    {
        let Some(entity) = sim.substrate.entities.get(id) else {
            return;
        };
        if entity.dying {
            return;
        }
        let Some(miner) = entity.miner.as_ref() else {
            return;
        };
        if miner.kind == MinerKind::Slave {
            // `UnitClass::Mission_Harvest @ 0x0073E5E0`'s prologue
            // (`0x0073E5E9..0x0073E612`): a ResourceDestination=,
            // ResourceGatherer= type holding a slave manager runs
            // HandleReturnedSlaves, then the shared Rate epilogue
            // (`0x0073EF77`).
            let slave_master = entity.slave_manager.is_some()
                && sim
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.resource_destination && object.resource_gatherer);
            let due = entity.mission.current().known()
                == Some(crate::sim::mission::MissionType::Harvest)
                && entity.mission.dispatch_timer().due(now);
            if slave_master && due {
                sim.handle_returned_slaves(id, rules);
                let delay = sim.mission_rate_epilogue_for(
                    rules,
                    id,
                    crate::sim::mission::MissionType::Harvest,
                );
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    entity.mission.write_dispatch_epilogue(now as i32, delay);
                }
            }
            return;
        }
        // The native dispatcher routes on the committed mission id, so a miner
        // that is NOT on Harvest never reaches this handler. VERA adopts that
        // gate for the two ids it commits authoritatively:
        // - Guard: the Stop command force-assigns it to a miner on Harvest or
        //   Return, exactly where the retail IDLE event handler does, and the
        //   Guard slot is a different handler (the harvester Guard override).
        //   Declining it here is what makes Stop actually stop a miner.
        // - Move: a player Move order runs the Foot Move handler; its arrival
        //   `Enter_Idle_Mode(0,1)` (`FootClass::Mission_Move` 0x004D4242 →
        //   `UnitClass::Enter_Idle_Mode @ 0x00738970` harvester arm) queues
        //   Harvest — or Guard for a human miner parked on non-ore land — and
        //   the promoted Harvest re-enters here at state 0.
        //
        // - Enter with a repair-depot `DockState`: a player depot order
        //   (`Command::RepairAtDepot`) commits Enter(7), and native dispatches
        //   that selector through `FootClass::Mission_Enter @ 0x004D9290` —
        //   `UnitClass`'s vtable `0x007F5C70 + 0x240` holds that Foot handler,
        //   not an override — whose probe/epilogue VERA runs from the Unit
        //   Enter arm of `techno_ai/mission_handlers.rs`
        //   (`building_dock::mission_enter_dispatch`). Declining it here keeps
        //   the dispatch timer on one writer while the miner waits, is
        //   serviced and is released; the release Move's arrival puts it back
        //   on Harvest (or Guard) through the harvester idle-mode arm.
        //
        // RESIDUAL: a miner retasked onto Attack still resumes harvesting from
        // its old cursor when that order finishes (the Attack arrival is not
        // modelled); the strict `current == Harvest` gate waits on it.
        let current = entity.mission.current().known();
        if matches!(
            current,
            Some(crate::sim::mission::MissionType::Guard)
                | Some(crate::sim::mission::MissionType::Move)
        ) {
            return;
        }
        if current == Some(crate::sim::mission::MissionType::Enter) && entity.dock_state.is_some() {
            return;
        }
        // A harvester's refinery dock runs as the native Enter and Unload
        // missions (`refinery_dock`), dispatched from the Foot handler host.
        if matches!(
            current,
            Some(crate::sim::mission::MissionType::Enter)
                | Some(crate::sim::mission::MissionType::Unload)
        ) {
            return;
        }
        // Native Mission_Dispatch gate: run the handler only when the
        // dispatch timer is due (verified host shape). The strength>0 gate is
        // the bracket's IsAlive guard upstream.
        if !entity.mission.dispatch_timer().due(now) {
            return;
        }
    }
    let Some(mut snap) = build_miner_snapshot(sim, rules, id) else {
        return;
    };
    harvest_mission_step(sim, rules, config, path_grid, overlay_registry, &mut snap);
    commit_miner_snapshot(sim, &snap, now);
}

fn harvest_mission_step(
    sim: &mut Simulation,
    rules: &RuleSet,
    config: &super::MinerConfig,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    snap: &mut MinerSnapshot,
) {
    // Cursor sanity (debug-only, never hashed): the working cursor must have
    // decoded from the entity's handler state — pins the cursor round-trip the
    // substate-authority flip relies on.
    #[cfg(debug_assertions)]
    if let Some(entity) = sim.substrate.entities.get(snap.entity_id) {
        debug_assert_eq!(
            MinerState::from_cursor(entity.mission.handler_state())
                .unwrap_or(MinerState::SearchOre),
            snap.state,
            "Harvest dispatch entry: entity {} working cursor must equal the \
             decoded MissionCom.handler_state",
            snap.entity_id,
        );
    }

    process_miner(sim, rules, config, path_grid, overlay_registry, snap);
}
