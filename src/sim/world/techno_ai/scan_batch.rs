//! Batched target scans: the scans a live object pass's targeting slots and
//! Area Guard and Hunt handlers are due to run are answered side by side when
//! the pass begins.
//!
//! Three callers of `Retaliate_And_Scan` are batched:
//! - `TechnoClass::AI_Update`'s targeting slot (`0x006FA65A..0x006FA6EE`)
//!   scans with mask 1 around the object's own coordinate once its targeting
//!   timer has run out: the passive block's Passive_Target_Acquire or, for an
//!   attack-mover, Arrival_Target_Handler (`0x004DF3A0`).
//! - `FootClass::Mission_AreaGuard` (`0x004D6AA0`) scans with mask 2 around
//!   its post once its handler runs with no target held, it can acquire one
//!   and its targeting timer has run out.
//! - `FootClass::Mission_Hunt` (`0x004D5350`) scans with mask 0 around the
//!   object's own coordinate at every handler run with no target held.
//!
//! With batching on, the live object pass first finds the objects whose turn
//! reaches Greatest_Threat this frame as the world stands, and with which
//! mask and coordinate ([`Simulation::scan_ahead`]: the Area Guard or Hunt
//! handler's scan, which runs first in a turn, else the targeting slot's),
//! and runs their Greatest_Threat (`0x006F8DF0`, through the `+0x3C4` override) on
//! worker threads against that frame-start world. At the object's turn a scan with
//! that mask and coordinate takes the answer in place of its own
//! Greatest_Threat when the object still stands as it did ([`ScannerKey`])
//! and the pick still stands as the scan saw it
//! ([`Simulation::take_batched_scan`]); otherwise it scans as usual.
//!
//! An answer's pick is held across the turns before the scanner's, as a
//! TarCom is, so what releases a TarCom releases the pick too
//! ([`Simulation::release_batched_scan_pick`]): the pointer-expiry broadcast
//! (`0x007258D0`; a kill's or a cloak's Detach_All, limbo) and the detach
//! targeting sweep (`0x0070D4A0`; an owner change, a sale). A pick whose
//! owner changed, or whose owner the scanner's house now allies or no longer
//! allies, or that GetFireError now refuses is not taken either: ILLEGAL or
//! CANT, and for the slot's scan RANGE, as its next drop step would drop a
//! passive pick that left weapon range. Area Guard and Hunt pick beyond
//! weapon range by design, so their check skips the range test and reaches
//! the class tests after it.
//!
//! Proposed sim-scale deviation from native order (update order and
//! same-frame effects), pending side-by-side play: an answered scan sees the
//! world as it stood when the pass began rather than at the scanner's turn.
//! A target that comes into reach, or a better one that appears, during the
//! turns before the scanner's is seen at its next scan (the targeting slot's
//! `[General] NormalTargetingDelay=` frames later, plus the 0..2 draw; Area
//! Guard's at its next handler run with the timer run out; Hunt's at its next
//! handler run), and
//! an estimate another scan debited during those turns does not lower this
//! scan's scores (`VHPScan=`, retail `[NASAM]` only). The scan's own effects
//! stay at the object's turn in native order: the scan frame stamp, the
//! RandomRanged(0, 2) draw, the timer, the drop step, the assignment, the
//! estimate debit and Foot+688's write. Greatest_Threat draws only for a
//! computer's disguise detection (`0x006F851C`) of a blinking candidate: an
//! answer that drew is dropped, and so is one that refused a disguised
//! candidate for want of a blink once a shot starts a blink during the pass
//! ([`Simulation::note_disguise_blink_started`]); their objects scan live.
//! Residual: a candidate that disguises or comes into reach during the pass
//! is not seen by an answer, and neither is its roll. Each answer
//! reads only the frame-start world, and its copy of the Dummy (below) starts
//! from the shared word on whichever thread it runs, so no result depends on
//! the thread count. Simulations start without batching; real matches and
//! sim-bench turn it on ([`Simulation::batch_target_scans`]).
//!
//! Lookups off the map stamp the shared Dummy, so each answer works on its
//! own copy of the Dummy's coordinate (`SharedCellDummy::detached_stamps`),
//! and an answer whose lookups changed that copy at any point is dropped and
//! its object scans live, stamping as native. Residuals: an answer whose
//! lookups stamped only the coordinate the Dummy held when the pass began is
//! kept, so its stamps never reach the shared Dummy, and a later read that
//! does not stamp first sees the last serial stamp; the turn's GetFireError
//! on the pick runs InRange's line query, whose lookups can stamp where the
//! serial scan's did not. A `DistributedFire=` scanner (retail `[AEGIS]`)
//! keeps the candidates its walk collects, so it always scans live. Aircraft
//! guard areas and hunt through their own missions (`aircraft_guard`,
//! `aircraft::hunt_mission`), deployed infantry guard areas through the
//! deploy shim (`0x00521320`), and a computer house's deployed infantryman
//! undeploys in Hunt, so none of those is foreseen.

use std::collections::BTreeSet;

use super::target_scan::{TargetingSlot, passive_acquire_gate};
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_weapon::{
    AttackerFacts, attacker_facts, controlled_transport_house, is_ally_by_object,
};
use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::fire_error_world::{FireSubject, FirerPart};
use crate::sim::combat::greatest_threat::{RetargetAfterStop, ThreatScanOutcome};
use crate::sim::combat::{ScanMission, TargetKind};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::mission::mega_mission::ArrivalStep;
use crate::sim::movement::FacingClass;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

/// This live object pass's answered scans.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ScanBatch {
    /// The pass's answers in ascending object id, each until a scan takes
    /// it. Empty between passes, so a save carries only the setting.
    #[serde(skip)]
    answers: Vec<(u64, Option<BatchedScan>)>,
    /// The picks a release reached during the pass.
    #[serde(skip)]
    released: BTreeSet<u64>,
    /// A shot started a disguise blink during the pass
    /// ([`Simulation::note_disguise_blink_started`]).
    #[serde(skip)]
    blink_started: bool,
    /// Answers scans took.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    used: usize,
}

#[derive(Debug)]
struct BatchedScan {
    /// The scan the answer is for.
    mask: ScanMission,
    scan_coord: Option<[i32; 3]>,
    key: ScannerKey,
    outcome: ThreatScanOutcome,
    /// The pick's owner, and whether the scanner's house allied it, when the
    /// scan ran.
    pick_house: Option<(InternedId, bool)>,
}

/// What Greatest_Threat reads of its own object that can change between the
/// pass's start and the object's turn; an answer applies only while all of it
/// holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScannerKey {
    owner: InternedId,
    type_ref: InternedId,
    cell: (u16, u16, u8),
    exact_z: Option<i32>,
    sub_cell: (SimFixed, SimFixed),
    body_facing: FacingClass,
    barrel_facing: Option<FacingClass>,
    mission: MissionId,
    retarget_after_stop: RetargetAfterStop,
    berserk: bool,
    /// The selection ladder's facts (`attacker_facts`): veterancy, weapon,
    /// gattling stage, deployment, transport and garrison, drain, Unload.
    facts: AttackerFacts,
    /// The house controlling the open-topped transport it rides.
    transport_controller: Option<InternedId>,
    /// Its team, and whether that team defends a base.
    team: Option<(u64, bool)>,
    /// Its house's last lost building (`House+0x54E0`).
    lost_building: Option<u64>,
    /// GetFireError's firer half (`FireSubject::firer_part`), which the
    /// scan's candidate probes read: NavCom, speed, sequence, links and the
    /// rest.
    fire: FirerPart,
}

impl ScannerKey {
    fn of(world: &Simulation, entity: &GameEntity, obj: &ObjectType, rules: &RuleSet) -> Self {
        let position = &entity.position;
        Self {
            owner: entity.owner(),
            type_ref: entity.type_ref(),
            cell: (position.rx(), position.ry(), position.z()),
            exact_z: position.exact_z_leptons(),
            sub_cell: (position.sub_x(), position.sub_y()),
            body_facing: entity.body_facing,
            barrel_facing: entity.barrel_facing,
            mission: entity.mission.current(),
            retarget_after_stop: entity.foot_retarget_after_stop,
            berserk: entity.berserk.is_active(),
            facts: attacker_facts(entity, obj),
            transport_controller: controlled_transport_house(
                &world.substrate.entities,
                rules,
                &world.interner,
                entity,
            ),
            team: world.team_script_vm.team_for_member(entity.stable_id()),
            lost_building: world
                .houses
                .get(&entity.owner())
                .and_then(|house| house.lost_building()),
            fire: FireSubject {
                world,
                rules,
                overlay_registry: None,
                fog: None,
                firer: entity,
                obj,
                target: None,
                weapon_index: 0,
            }
            .firer_part(),
        }
    }
}

impl Simulation {
    /// Answer the targeting slots' and Area Guard and Hunt handlers' scans
    /// side by side from now on; see the module docs.
    pub fn batch_target_scans(&mut self) {
        self.scan_batch.get_or_insert_with(ScanBatch::default);
    }

    /// The pass's answers, before its first turn.
    pub(in crate::sim::world) fn answer_batched_scans(
        &mut self,
        rules: Option<&RuleSet>,
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) {
        let (Some(rules), Some(terrain), Some(_)) = (
            rules,
            self.resolved_terrain.as_ref(),
            self.scan_batch.as_ref(),
        ) else {
            return;
        };
        let dummy = terrain.shared_cell_dummy();
        let world: &Simulation = self;
        // Only the Logic list's objects take a turn.
        let logic = world.substrate.logic.as_slice();
        let mut answers = crate::sim::parallel::filter_map_indices(logic.len(), |index| {
            let id = logic[index];
            let entity = world.substrate.entities.get(id)?;
            let (answer, stamped) = dummy.detached_stamps(|| {
                let obj = world.object_type(entity.type_ref(), rules)?;
                if obj.distributed_fire {
                    return None;
                }
                let (mask, scan_coord) = world.scan_ahead(id, entity, rules, overlay_registry)?;
                let outcome = world.threat_scan(rules, overlay_registry, id, mask, scan_coord);
                let pick_house = outcome
                    .target()
                    .and_then(|pick| world.substrate.entities.get(pick))
                    .map(|pick| (pick.owner(), world.scanner_allies(entity, pick.owner())));
                Some(BatchedScan {
                    mask,
                    scan_coord,
                    key: ScannerKey::of(world, entity, obj, rules),
                    outcome,
                    pick_house,
                })
            });
            answer
                .filter(|answer| !stamped && !answer.outcome.drew_scenario_rng())
                .map(|answer| (id, Some(answer)))
        });
        answers.sort_unstable_by_key(|&(id, _)| id);
        if let Some(batch) = self.scan_batch.as_mut() {
            batch.answers = answers;
        }
    }

    /// The pass is over: drop the answers no scan took.
    pub(in crate::sim::world) fn finish_batched_scans(&mut self) {
        if let Some(batch) = self.scan_batch.as_mut() {
            batch.answers.clear();
            batch.released.clear();
            batch.blink_started = false;
        }
    }

    /// A shot started a disguise blink (`UnitClass::Fire_At @ 0x0074143E`):
    /// no answer that refused a disguised candidate only for want of a blink
    /// is taken this pass, so its scan runs live and rolls as native does.
    pub(crate) fn note_disguise_blink_started(&mut self) {
        if let Some(batch) = self.scan_batch.as_mut()
            && !batch.answers.is_empty()
        {
            batch.blink_started = true;
        }
    }

    /// A release reached `pick` (see the module docs): no answer that picked
    /// it is taken this pass.
    pub(in crate::sim::world) fn release_batched_scan_pick(&mut self, pick: u64) {
        if let Some(batch) = self.scan_batch.as_mut()
            && !batch.answers.is_empty()
        {
            batch.released.insert(pick);
        }
    }

    /// The first Greatest_Threat `id`'s turn reaches this frame as the world
    /// stands, as its mask and coordinate (`None`: its own): the Area Guard
    /// or Hunt handler's (`mission_handlers::foot_area_guard_scan_ahead`,
    /// `foot_hunt_scans_ahead`), which runs before the targeting slot, else
    /// the targeting slot's (mask 1). A frozen object's turn returns at its
    /// Temporal test and a Unit's or Infantry's in a tube before its Foot
    /// body (`techno_ai`'s visit), so neither scans.
    /// The slot scans when it runs (`target_scan::targeting_slot`) and
    /// Arrival_Target_Handler scans ([`Simulation::foot_arrival_step`]) with
    /// no target left, or Passive_Target_Acquire's gate passes with none
    /// held. A held target the drop step would drop is not foreseen: its
    /// object scans live.
    fn scan_ahead(
        &self,
        id: u64,
        entity: &GameEntity,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) -> Option<(ScanMission, Option<[i32; 3]>)> {
        if entity.ai_frozen()
            || (matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            ) && entity.low_bridge_tube_state.is_some())
        {
            return None;
        }
        if let Some(post) = super::mission_handlers::foot_area_guard_scan_ahead(self, id, rules) {
            return Some((ScanMission::AreaGuard, Some(post)));
        }
        if super::mission_handlers::foot_hunt_scans_ahead(self, id, rules) {
            return Some((ScanMission::Hunt, None));
        }
        let unheld = entity.attack_target.target().is_none();
        let scans = match super::target_scan::targeting_slot(entity, self.session.binary_frame()) {
            TargetingSlot::Skip => false,
            TargetingSlot::ArrivalHandler => matches!(
                self.foot_arrival_step(id, rules, overlay_registry),
                Some(ArrivalStep::Scan { drop_tarcom, .. }) if drop_tarcom || unheld
            ),
            TargetingSlot::PassiveAcquire(mission) => {
                unheld && passive_acquire_gate(self, id, rules, mission)
            }
        };
        scans.then_some((ScanMission::Guard, None))
    }

    /// Whether `scanner`'s house allies `owner`, as the scan asks it
    /// (`HouseClass::Is_Ally_ByObject @ 0x004F9A90`).
    fn scanner_allies(&self, scanner: &GameEntity, owner: InternedId) -> bool {
        is_ally_by_object(
            Some(self.house_alliances.as_map()),
            &self.interner,
            scanner.owner(),
            owner,
        )
    }

    /// The answer to `id`'s Greatest_Threat with `mask` around `scan_coord`
    /// (`None`: its own coordinate) through the `+0x3C4` override, when this pass
    /// made one for that scan, the object still stands as it did, and its
    /// pick, if any, is unreleased, has the owner and the standing with the
    /// scanner's house it had, and GetFireError (vt+0x3C0 for SelectWeapon's
    /// weapon) refuses it neither ILLEGAL nor CANT. Only the slot's in-range
    /// scan asks it with range and is refused RANGE too, as its next drop
    /// step (`0x007098B9..0x00709912`) would drop the pick. A scan takes it
    /// once.
    pub(super) fn take_batched_scan(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        id: u64,
        mask: ScanMission,
        scan_coord: Option<[i32; 3]>,
    ) -> Option<ThreatScanOutcome> {
        let batch = self.scan_batch.as_mut()?;
        let index = batch
            .answers
            .binary_search_by_key(&id, |&(scanner, _)| scanner)
            .ok()?;
        let slot = &mut batch.answers[index].1;
        // Another scan of the object leaves the answer for its own.
        if !slot
            .as_ref()
            .is_some_and(|answer| (answer.mask, answer.scan_coord) == (mask, scan_coord))
        {
            return None;
        }
        let answer = slot.take()?;
        let released = answer
            .outcome
            .target()
            .is_some_and(|pick| batch.released.contains(&pick))
            || (batch.blink_started && answer.outcome.met_idle_disguise());
        let scanner = self.substrate.entities.get(id)?;
        let obj = self.object_type(scanner.type_ref(), rules)?;
        if released || ScannerKey::of(self, scanner, obj, rules) != answer.key {
            return None;
        }
        if let Some(pick) = answer.outcome.target() {
            let pick_house = self
                .substrate
                .entities
                .get(pick)
                .map(|pick| (pick.owner(), self.scanner_allies(scanner, pick.owner())));
            if pick_house != answer.pick_house {
                return None;
            }
            let check_range = mask == ScanMission::Guard;
            let refused = match self.selected_weapon_fire_error(
                rules,
                id,
                TargetKind::Entity(pick),
                check_range,
                overlay_registry,
            ) {
                FireError::Illegal | FireError::Cant => true,
                FireError::Range => check_range,
                _ => false,
            };
            if refused {
                return None;
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        {
            self.scan_batch.as_mut().unwrap().used += 1;
        }
        Some(answer.outcome)
    }

    /// Answers scans took.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn batched_scans_used(&self) -> usize {
        self.scan_batch.as_ref().map_or(0, |batch| batch.used)
    }
}

#[cfg(test)]
#[path = "scan_batch_tests.rs"]
mod tests;
