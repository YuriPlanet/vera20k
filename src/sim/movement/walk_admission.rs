//! Walk75B59C..75BCBD: live Foot admission and the fresh-head response.
//!
//! The movement pass suspends before this owner so Foot+1AC reads the complete
//! Simulation. Its raw code is consumed here once; Clear never encounters the
//! route adapter's grid, cliff or occupancy classifiers. Native comparisons:
//! tools/spatial_oracle/walk_prehead_response (callee boundaries documented there).
//!
//! The shared Cell Scatter owner retains its existing per-occupant displacement
//! residuals. JumpJet=true Infantry's +4F4/+4F8 conversion is a separate
//! locomotor mechanism; its unrepresented continuation is reported explicitly.

use super::block_index::HeldBlockSets;
use super::foot_path::{FindPathResult, coord_cell};
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::{FootPathRequest, WalkAdmissionRequest};
use super::track_fresh::BlockingObject;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::native_xyz_distance;
use crate::sim::combat::TargetKind;
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SIM_ZERO;
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

impl Simulation {
    /// Resume the same Process at75B59C after releasing its mutable entity
    /// borrow. A retry returns the same visit for75AEC0(false)'s Find_Path;
    /// every other answer completes this fresh-head Process invocation.
    pub(crate) fn run_walk_admission_request(
        &mut self,
        request: WalkAdmissionRequest,
        held: Option<&mut HeldBlockSets>,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<FootPathRequest>, String> {
        let rules = rules.ok_or("Walk admission requires rules")?;
        let id = request.entity_id;
        #[cfg(test)]
        self.export_bridge_engineer_entry_inputs(id, rules, "walk-prehead");
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Walk admission owner")?;
        let physical = ground_pose::position_world_coord(&actor.position);
        let on_bridge = actor.on_bridge;
        //75B302 retains the original word for+1AC. Only the coordinate
        //vector uses &7 at75B5A7; an empty (-1) word therefore selects NW.
        let direction = actor
            .navigation
            .path_replay
            .remaining_directions()
            .first()
            .map_or(-1, |direction| i32::from(*direction));
        let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[(direction & 7) as usize];
        let candidate = DriveCoord {
            x: physical.x.wrapping_add(dx * 256),
            y: physical.y.wrapping_add(dy * 256),
            z: physical.z,
        };
        let packed = coord_cell(candidate);
        //75B642 tests structural100 before5F5F00's two physical-cell
        //queries, then75B687 looks up the retained packed candidate again.
        let mismatch = {
            let terrain = self
                .resolved_terrain
                .as_ref()
                .ok_or("Walk admission requires map cells")?;
            let cells = NativeCellQuery::canonical(terrain);
            let candidate_cell = cells.lookup_world(candidate.x, candidate.y);
            (cells.flags(candidate_cell) & 0x100 != 0) != on_bridge
        };
        if mismatch {
            self.substrate
                .entities
                .get_mut(id)
                .ok_or("retired Walk admission owner")?
                .runtime_bridge_transition
                .pending_mismatch = true;
        }
        let (height, cell) = {
            let cells = NativeCellQuery::canonical(self.resolved_terrain.as_ref().unwrap());
            let height = ground_pose::query_object_cell_height(&cells, physical, on_bridge);
            (height, cells.lookup(packed))
        };
        let code = self.foot_can_enter(
            id,
            cell,
            InfantryEntryArgs {
                direction,
                height,
                previous_cell: None,
            },
            rules,
            registry,
        )?;
        self.finish_walk_admission_response(request, candidate, code, held, rules, registry)
    }

    ///75B696: one response owner for the live classifier and the corpus's
    ///explicitly supplied result boundary. Neither caller repeats admission.
    #[allow(clippy::too_many_arguments)]
    fn finish_walk_admission_response(
        &mut self,
        request: WalkAdmissionRequest,
        candidate: DriveCoord,
        code: u8,
        held: Option<&mut HeldBlockSets>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<FootPathRequest>, String> {
        let id = request.entity_id;
        let packed = coord_cell(candidate);
        if code == 0 {
            let grid = self.path_grid_snapshot();
            let selection = super::walk_head::select_step_head_at(
                &mut self.substrate.entities,
                id,
                &self.substrate.occupancy,
                &mut self.substrate.raw_cell_occupation,
                self.resolved_terrain.as_ref(),
                grid.as_deref(),
                Some(rules),
                &self.interner,
                &mut self.scenario_rng,
                candidate,
            );
            let selected = match selection {
                super::walk_head::StepHead::Done(answer) => answer,
                super::walk_head::StepHead::Pending => {
                    //75C565..75C5A6: the crate question on the stored head. A
                    //false answer outside limbo resets the head to the dummy;
                    //a dead owner answers 0 without the Mark tail.
                    let head = self
                        .substrate
                        .entities
                        .get(id)
                        .and_then(|actor| actor.locomotor.as_ref())
                        .and_then(|loco| loco.step_head())
                        .ok_or("Walk head vanished before its crate question")?;
                    let picked = self.pickup_crate_at(id, coord_cell(head), Some(rules), registry);
                    let actor = self
                        .substrate
                        .entities
                        .get_mut(id)
                        .ok_or("retired Walk head owner")?;
                    let mut dead = false;
                    if !picked && !actor.lifecycle.in_limbo {
                        if let Some(loco) = actor.locomotor.as_mut() {
                            loco.set_step_head(None);
                        }
                        dead = !actor.lifecycle.object_alive;
                    }
                    !dead
                        && super::walk_head::commit_walk_head(
                            &self.substrate.entities,
                            id,
                            &mut self.substrate.raw_cell_occupation,
                            self.resolved_terrain.as_ref(),
                            grid.as_deref(),
                        )
                }
            };
            let actor = self
                .substrate
                .entities
                .get_mut(id)
                .ok_or("retired Walk head owner")?;
            if selected {
                super::walk_head::finish_fresh_head(actor, self.session.binary_frame);
            } else {
                //75BCC0..75BCD5: failed subcell selection only zeros speed.
                actor.foot_speed.set_speed_fraction(SIM_ZERO);
                actor.navigation.path_runtime.clear_scold_latch();
            }
            return Ok(None);
        }
        if code > 7 {
            return Err("Walk Foot+1AC answered outside0..7".into());
        }
        self.walk_refused_animation(id)?;
        match code {
            1 => {
                //75BB9D..75BBAE runs every ground resident's+FC before
                //testing the once-only recursive retry flag.
                let native = self.walk_lookup_cell(packed)?;
                self.uncloak_cell_contacts(native, rules)?;
                self.walk_retry_admission(request)
            }
            2 => {
                self.walk_blocked_delay(request, held, rules, registry)?;
                Ok(None)
            }
            3 => {
                //75BA0E..75BA27: ignore gate answer, then+64C=10. Neither
                //timer nor path words are reset on this response.
                crate::sim::gate_runtime::request_gate_open_for_cell(
                    self,
                    (packed.0 as u16, packed.1 as u16),
                    id,
                    rules,
                );
                self.substrate
                    .entities
                    .get_mut(id)
                    .ok_or("retired Walk gate owner")?
                    .navigation
                    .path_runtime
                    .retries_left = super::PATH_STUCK_INIT;
                Ok(None)
            }
            4 | 5 => {
                //75BA46..75BA6C resolves the blocker BEFORE retry admission.
                let native = self.walk_lookup_cell(packed)?;
                let at = self
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .native_cell_coord(native);
                let blocker = self.find_blocking_object((at.0 as u16, at.1 as u16));
                if request.allows_retry() {
                    return self.walk_retry_admission(request);
                }
                self.walk_override_blocker(id, packed, blocker, rules, registry)?;
                //75BB59..75BB90: clear path, speed0, WalkStop. NavCom is
                //changed only by an admitted Override, not this Stop itself.
                self.clear_walk_admission_path(id)?;
                let actor = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .ok_or("retired Walk stop owner")?;
                actor.foot_speed.set_speed_fraction(SIM_ZERO);
                self.walk_stop_moving(id, Some(rules))?;
                let actor = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .ok_or("Walk stop owner retired during its callback")?;
                //75BB84/75BB90: both override/Stop exits clear the byte.
                actor.navigation.path_runtime.clear_scold_latch();
                super::retain_committed_movement(actor);
                Ok(None)
            }
            6 => {
                //75B6B8..75B6C7 retains this Cell pointer across the stop
                //band's current-cell query, including the shared Dummy alias.
                let cell = self.walk_lookup_cell(packed)?;
                if request.allows_retry() {
                    return self.walk_retry_admission(request);
                }
                self.walk_scatter_or_stop(id, cell, rules, registry)?;
                Ok(None)
            }
            7 => self.walk_retry_admission(request),
            _ => unreachable!("nonzero Foot entry domain checked above"),
        }
    }

    /// The native decoder corpus starts after+1AC at75B696. It supplies
    ///only that result, preserving this exact production response body.
    #[cfg(test)]
    pub(crate) fn replay_walk_admission_response(
        &mut self,
        id: u64,
        allow_retry: bool,
        candidate: DriveCoord,
        code: u8,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<FootPathRequest>, String> {
        let request = WalkAdmissionRequest::for_response_test(
            &self.substrate.entities,
            id,
            allow_retry,
            rules,
        )
        .ok_or("missing Walk corpus receiver")?;
        self.finish_walk_admission_response(request, candidate, code, None, rules, registry)
    }

    fn walk_lookup_cell(&self, cell: (i16, i16)) -> Result<NativeCellIdentity, String> {
        Ok(self
            .resolved_terrain
            .as_ref()
            .ok_or("Walk response requires map cells")?
            .native_cell_identity(cell))
    }

    ///75B6A3 then+548: Infantry521B20 clears Doing3/6/17 only; Unit4DBA30
    ///returns without writes. This is independent of locomotor IsMoving+34.
    fn walk_refused_animation(&mut self, id: u64) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk refusal owner")?;
        actor
            .locomotor
            .as_mut()
            .ok_or("Walk refusal requires locomotor")?
            .stop_movement_animation();
        if actor.category == EntityCategory::Infantry
            && actor
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| matches!(leaf.doing(), 3 | 6 | 17))
        {
            actor
                .mission_leaf
                .set_infantry_doing_verified(-1)
                .map_err(|error| format!("{error:?}"))?;
        }
        Ok(())
    }

    fn clear_walk_admission_path(&mut self, id: u64) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk path owner")?;
        actor.clear_live_path_head();
        Ok(())
    }

    ///75B6D1/75BA76/75BBBF: clear one live path word, null the no-head
    ///coordinate, expire+640, and recurse once withargument0. Codes1/7 do
    ///nothing beyond+548 when that argument was alreadyfalse.
    fn walk_retry_admission(
        &mut self,
        request: WalkAdmissionRequest,
    ) -> Result<Option<FootPathRequest>, String> {
        if !request.allows_retry() {
            return Ok(None);
        }
        let id = request.entity_id;
        self.clear_walk_admission_path(id)?;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk retry owner")?;
        let loco = actor
            .locomotor
            .as_mut()
            .ok_or("Walk retry requires locomotor")?;
        loco.set_step_head(None);
        let destination = loco
            .walk_destination()
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        actor
            .navigation
            .path_runtime
            .start_movement(self.session.binary_frame, 0);
        Ok(Some(request.into_path_request(destination, 0)))
    }

    ///75B8A9..75B9F9. Find_Path runs only after the exact-zero+640 gate;
    ///PathDelay is written AFTER it, before the result-specific continuation.
    fn walk_blocked_delay(
        &mut self,
        request: WalkAdmissionRequest,
        held: Option<&mut HeldBlockSets>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk code2 owner")?;
        let Some(urgency) = walk_code2_gate(
            &mut actor.navigation.path_runtime,
            frame,
            rules.general.blockage_path_delay_ticks,
        ) else {
            return Ok(());
        };
        let destination = actor
            .locomotor
            .as_ref()
            .and_then(|loco| loco.walk_destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        let request = request.into_path_request(destination, urgency);
        let found = self.foot_find_path(&request, held, rules, registry)?;
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("Walk code2 owner retired during Find_Path")?
            .navigation
            .path_runtime
            .start_movement(frame, rules.general.path_delay_ticks());
        if found == FindPathResult::Failed {
            //75B9B9 reads the live destination; the failed core's+500
            //receiver may have stopped it. Unlike75AFEE, this caller returns
            //immediately after+4F4, without the no-queue distance/retry tail.
            let destination = self
                .substrate
                .entities
                .get(id)
                .and_then(|actor| actor.locomotor.as_ref())
                .and_then(|loco| loco.walk_destination())
                .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
            if self.foot_path_zone_precheck(id, destination, rules)? {
                self.walk_failed_path_receiver(id, rules)?;
            } else {
                self.assign_null_destination(id, Some(rules), None);
            }
            if let Some(actor) = self.substrate.entities.get_mut(id)
                && actor.locomotor.as_ref().is_some_and(|loco| {
                    loco.walk_destination().is_none() && loco.step_head().is_none()
                })
            {
                actor.movement_target = None;
            }
        } else {
            self.walk_short_path_receiver(id, rules)?;
        }
        Ok(())
    }

    fn walk_scatter_or_stop(
        &mut self,
        id: u64,
        cell: NativeCellIdentity,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Walk code6 owner")?;
        let physical = ground_pose::position_world_coord(&actor.position);
        let destination = actor
            .locomotor
            .as_ref()
            .and_then(|loco| loco.walk_destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        let close = native_xyz_distance(
            physical.x.wrapping_sub(destination.x),
            physical.y.wrapping_sub(destination.y),
            physical.z.wrapping_sub(destination.z),
        ) < rules.general.close_enough;
        if close && actor.radio_contacts.is_empty() && self.track_stop_band(physical, destination) {
            //75B7E1..75B828: headNULL, Stop, speed0, classNULL setter, then
            //clearFoot+5E0 even when the class setter refuses the request.
            let actor = self
                .substrate
                .entities
                .get_mut(id)
                .ok_or("retired Walk code6 owner")?;
            if let Some(loco) = actor.locomotor.as_mut() {
                loco.set_step_head(None);
            }
            self.walk_stop_moving(id, Some(rules))?;
            self.substrate
                .entities
                .get_mut(id)
                .ok_or("Walk code6 owner retired during its callback")?
                .foot_speed
                .set_speed_fraction(SIM_ZERO);
            self.assign_null_destination(id, Some(rules), None);
            self.clear_walk_admission_path(id)?;
            if let Some(actor) = self.substrate.entities.get_mut(id) {
                super::retain_committed_movement(actor);
            }
            return Ok(());
        }
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Walk scatter requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let deck = cells.flags(cell) & 0x100 != 0
            && (physical.z / GROUND_LEVEL_HEIGHT_LEPTONS
                - i32::from(cells.ground_fields(cell).0 as i8))
            .wrapping_abs()
                > 2;
        let at = cells.coord(cell);
        self.scatter_cell_contacts(at, deck, true, rules, registry)
    }

    fn walk_override_blocker(
        &mut self,
        id: u64,
        cell: (i16, i16),
        blocker: Option<BlockingObject>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        let target = match blocker {
            Some(BlockingObject::Entity(blocker)) => {
                let actor = self
                    .substrate
                    .entities
                    .get(id)
                    .ok_or("retired Walk override owner")?;
                let other = self
                    .substrate
                    .entities
                    .get(blocker)
                    .ok_or("retired Walk blocker")?;
                if crate::map::houses::is_allied_with(
                    &self.house_alliances,
                    self.interner.resolve(actor.owner()),
                    self.interner.resolve(other.owner()),
                ) {
                    return Ok(());
                }
                Some(TargetKind::Entity(blocker))
            }
            Some(BlockingObject::Terrain) => {
                return Err("Walk Override requires a Terrain target owner".into());
            }
            None => {
                let first = self.walk_lookup_cell(cell)?;
                let terrain = self.resolved_terrain.as_ref().unwrap();
                let overlay = match first {
                    NativeCellIdentity::Real(index) => terrain.cells()[index]
                        .bridge_facts
                        .overlay_id
                        .map(i32::from)
                        .unwrap_or(-1),
                    NativeCellIdentity::Dummy => {
                        terrain.shared_cell_dummy().overlay_identity_state().0
                    }
                };
                if overlay == -1 {
                    return Ok(());
                }
                let second = self.walk_lookup_cell(cell)?;
                let terrain = self.resolved_terrain.as_ref().unwrap();
                let overlay = match second {
                    NativeCellIdentity::Real(index) => {
                        terrain.cells()[index].bridge_facts.overlay_id
                    }
                    NativeCellIdentity::Dummy => {
                        u8::try_from(terrain.shared_cell_dummy().overlay_identity_state().0).ok()
                    }
                };
                let wall = overlay
                    .and_then(|overlay| registry.and_then(|registry| registry.flags(overlay)))
                    .is_some_and(|flags| flags.wall);
                if !wall {
                    return Ok(());
                }
                let target_cell = self.walk_lookup_cell(cell)?;
                let at = terrain.native_cell_coord(target_cell);
                Some(TargetKind::Cell(at.0 as u16, at.1 as u16))
            }
        };
        if let Some(target) = target {
            #[cfg(test)]
            if super::fresh_oracle_seam::substitute(
                super::fresh_oracle_seam::FreshCallRecord::Override { target },
            ) {
                return Ok(());
            }
            self.mission_override_movement_blocker(id, target, rules);
        }
        Ok(())
    }

    /// Foot4DC030 (Infantry521DD0 prefix, Unit+4F4): Hunt alone clears
    ///TarCom then calls the class NULL destination setter. No RNG or timers
    ///are introduced by this receiver; the setter owns any timer effects.
    pub(super) fn walk_failed_path_receiver(
        &mut self,
        id: u64,
        rules: &RuleSet,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Walk failed-path receiver")?;
        if actor.mission.effective().raw() == 15 {
            self.assign_target_represented(id, None, Some(rules))
                .map_err(|error| format!("{error:?}"))?;
            self.assign_null_destination(id, Some(rules), None);
        }
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Walk failed-path receiver")?;
        if actor.category == EntityCategory::Infantry
            && rules
                .object(self.interner.resolve(actor.type_ref()))
                .is_some_and(|object| object.jumpjet)
        {
            return Err(
                "Infantry+4F4 JumpJet close-navigation continuation requires its locomotor owner"
                    .into(),
            );
        }
        Ok(())
    }

    /// Infantry521EB0 / Unit41C080. Ordinary Infantry and every Unit return
    ///false; JumpJet=true Infantry has a separate short-path takeoff body.
    pub(super) fn walk_short_path_receiver(&self, id: u64, rules: &RuleSet) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Walk short-path receiver")?;
        if actor.category == EntityCategory::Infantry
            && actor.navigation.path_replay.remaining_directions().len() < 4
            && rules
                .object(self.interner.resolve(actor.type_ref()))
                .is_some_and(|object| object.jumpjet)
        {
            return Err(
                "Infantry+4F8 JumpJet short-path continuation requires its locomotor owner".into(),
            );
        }
        Ok(())
    }
}

/// Walk code 2 (0x0075B8A9..0x0075B979): the first refusal latches Foot+6B7
/// and starts the BlockagePathDelay grace (+668); a running +640 movement
/// delay waits; otherwise Find_Path runs with urgency 2 once the grace has
/// expired, else 1.
fn walk_code2_gate(
    runtime: &mut crate::sim::components::FootPathRuntime,
    frame: u32,
    blockage_delay: i32,
) -> Option<u8> {
    if !runtime.path_blocked {
        runtime.path_blocked = true;
        runtime.start_blocked(frame, blockage_delay);
    }
    if !runtime.movement_timer.expired(frame as i32) {
        return None;
    }
    Some(
        if runtime.path_blocked && runtime.blocked_timer.expired(frame as i32) {
            2
        } else {
            1
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::sim::timer::CdTimer;

    /// Original instructions 0x75B8A0..0x75B979 / 0x75C1F1, recheck with
    /// `python -m tools.infantry_scatter_oracle --check`: the grace latch and
    /// its preservation, the movement-delay gate and the urgency selection.
    /// Find_Path and the post-search PathDelay restart are outside the oracle.
    #[test]
    fn walk_code2_gate_matches_native_vectors() {
        let data: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/infantry_scatter_oracle.json",
        ))
        .unwrap();
        assert_eq!(data["source"], "unicorn/gamemd.exe");
        let cases = data["walk_code2_timers"].as_array().unwrap();
        assert_eq!(cases.len(), 30);
        let mut observed = [0usize; 3];
        for case in cases {
            let n = |key: &str| case[key].as_i64().unwrap() as i32;
            let b = |key: &str| case[key].as_bool().unwrap();
            let frame = n("frame");
            let mut runtime = crate::sim::components::FootPathRuntime::default();
            runtime.path_blocked = b("already_blocked");
            runtime.blocked_timer = CdTimer::from_raw(n("grace_start"), n("grace_duration"));
            runtime.movement_timer = CdTimer::from_raw(n("movement_start"), n("movement_duration"));
            let urgency = super::walk_code2_gate(&mut runtime, frame as u32, n("configured_grace"));
            assert_eq!(urgency.is_some(), b("repath"), "{case}");
            if let Some(urgency) = urgency {
                assert_eq!(
                    i64::from(urgency),
                    case["urgency"].as_i64().unwrap(),
                    "{case}"
                );
            }
            observed[urgency.map_or(0, usize::from)] += 1;
            assert_eq!(runtime.path_blocked, b("out_blocked"), "{case}");
            assert_eq!(
                runtime.blocked_timer.remaining(frame),
                CdTimer::from_raw(n("out_grace_start"), n("out_grace_duration")).remaining(frame),
                "{case}"
            );
        }
        assert!(observed.iter().all(|&count| count > 0), "{observed:?}");
    }
}
