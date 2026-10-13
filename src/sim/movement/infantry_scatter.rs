//! `InfantryClass::Scatter @ 0x0051D0D0`: the gates every call shares, the
//! null-coordinate arm, and the away-from-a-coordinate selection the damage
//! receiver, DeploySlaves and the garrison eject use. Class entry and
//! navigation keep their owners.
use super::bump_crush::scatter_movement_speed;
use super::infantry_action::DO_UNDEPLOY;
use super::infantry_entry::InfantryEntryArgs;
use super::scatter::{ScatterFlags, mission_permits_scatter};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

/// Whether the Infantry class setter (`0x0051AA40`) is represented for this
/// infantryman: his active locomotor is Walk, Teleport or Jumpjet, whose
/// `Move_To` its Foot tail reaches.
fn represented_infantry_destination(actor: &crate::sim::game_entity::GameEntity) -> bool {
    actor.category == crate::map::entities::EntityCategory::Infantry
        && actor
            .locomotor
            .as_ref()
            .is_some_and(|loco| match loco.kind {
                LocomotorKind::Walk | LocomotorKind::Teleport => true,
                LocomotorKind::Jumpjet => loco.jumpjet_runtime().is_some(),
                _ => false,
            })
}

/// The Doing values of the deploy family, as Scatter (`0x0051D0E3..0x0051D0F5`)
/// and the Infantry setter (`0x0051AA60..0x0051AA7E`) test them.
const DEPLOY_DOINGS: std::ops::RangeInclusive<i32> = 0x1B..=0x1E;

/// What `InfantryClass::Scatter` reads between its deploy arm and its
/// coordinate test (`0x0051D162..0x0051D220`).
#[derive(Clone, Copy, Debug)]
pub(super) struct InfantryScatterFacts {
    /// Retained Doing `+0x6C4`, read after the deploy arm.
    pub(super) doing: i32,
    /// The active locomotor's `Is_Moving` (`+0x10`).
    pub(super) moving: bool,
    /// The current mission's MissionControl `Scatter=`.
    pub(super) mission_scatter: bool,
    /// Type `Fraidycat=` (`+0xEBF`).
    pub(super) fraidycat: bool,
    /// Techno Target (`+0x2B4`).
    pub(super) has_target: bool,
    /// `[CombatDamage] PlayerScatter` (Rules `+0x17ED`).
    pub(super) player_scatter: bool,
    /// `HasWeaponAbility(SCATTER)` at the current rank.
    pub(super) scatter_ability: bool,
    /// The owner is controlled by a human (`0x0050B730`).
    pub(super) human: bool,
    /// Foot Team `+0x5D4` is set.
    pub(super) in_team: bool,
}

/// `0x0051D16E..0x0051D220`, after the deploy arm. A moving locomotor drops
/// `forced` (`0x0051D172`). An unforced call then needs the mission's
/// `Scatter=`, a Fraidycat type or no Target, and an interruptible Doing
/// (`-1` and 31 skip the table at `0x007EAF7C`). A still-forced call passes;
/// otherwise a human owner without PlayerScatter, SCATTER or no-kidding needs
/// a Team, and every unforced path needs `Fraidycat=`. `None` for a Doing
/// outside the native table.
///
/// Evidence: tools/spatial_oracle/infantry_damage_scatter.{py,json,meta.json}
/// (false/false) and tools/infantry_scatter_oracle.py (true/true).
pub(super) fn infantry_scatter_gates_admit(
    facts: &InfantryScatterFacts,
    flags: ScatterFlags,
) -> Option<bool> {
    let forced = flags.forced && !facts.moving;
    if !forced && (!facts.mission_scatter || (!facts.fraidycat && facts.has_target)) {
        return Some(false);
    }
    if !crate::rules::infantry_sequence::scatter_allowed_by_doing(facts.doing)? {
        return Some(false);
    }
    if forced {
        return Some(true);
    }
    let owner_gated =
        !facts.player_scatter && !facts.scatter_ability && !flags.no_kidding && facts.human;
    Some(facts.fraidycat && !(owner_gated && !facts.in_team))
}

impl Simulation {
    /// `InfantryClass::Scatter`'s gates before its coordinate
    /// (`0x0051D0DD..0x0051D220`). A deploy-family Doing takes
    /// `Do_Action(Undeploy, 0, 0)` (`0x0051D103..0x0051D10D`) when forced and
    /// no-kidding, then continues; otherwise a human owner refuses it
    /// (`0x0051D115..0x0051D148`). Then [`infantry_scatter_gates_admit`] on
    /// the Doing the action left. Nothing here draws RNG.
    ///
    pub(super) fn infantry_scatter_admitted(
        &mut self,
        id: u64,
        flags: ScatterFlags,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let infantry = self
            .substrate
            .entities
            .get(id)
            .ok_or("Scatter lost its infantryman")?;
        let doing = infantry
            .mission_leaf
            .as_infantry()
            .ok_or("Scatter requires an Infantry Doing")?
            .doing();
        let human = self.owner_is_human(infantry.owner());
        if DEPLOY_DOINGS.contains(&doing) {
            if flags.forced && flags.no_kidding {
                self.infantry_do_action(id, DO_UNDEPLOY, false, rules, frame_effects)?;
            } else if human {
                return Ok(false);
            }
        }
        let infantry = self
            .substrate
            .entities
            .get(id)
            .ok_or("Scatter lost its infantryman")?;
        let object = self
            .object_type(infantry.type_ref(), rules)
            .ok_or("Scatter requires an Infantry type")?;
        // Is_Moving only matters to a forced call; the query has no effect.
        let moving = flags.forced
            && super::motion_query::is_moving(infantry)
                .ok_or("Scatter requires an active locomotor Is_Moving")?;
        let facts = InfantryScatterFacts {
            doing: infantry
                .mission_leaf
                .as_infantry()
                .ok_or("Scatter requires an Infantry Doing")?
                .doing(),
            moving,
            mission_scatter: mission_permits_scatter(infantry, rules),
            fraidycat: object.fraidycat,
            has_target: infantry.attack_target.is_some(),
            player_scatter: rules.general.player_scatter,
            scatter_ability: crate::sim::combat::veterancy::has_weapon_ability(
                crate::sim::combat::veterancy::rank_from_u16(infantry.veterancy()),
                object,
                crate::rules::object_type::Ability::Scatter,
            ),
            human,
            in_team: self.team_script_vm.team_for_member(id).is_some(),
        };
        infantry_scatter_gates_admit(&facts, flags)
            .ok_or_else(|| String::from("Scatter has an invalid Doing"))
    }

    /// `InfantryClass::Scatter` with a null coordinate (`0x0051D2D9..`):
    /// - the fallback direction first ([`null_start_direction`], with its
    ///   one draw);
    /// - then the nearby passable cell. When found: `SetDestination(cell, 1)`
    ///   and the locomotor's Process at once (`0x0051D43F..0x0051D478`);
    /// - otherwise the eight neighbours from that direction, then
    ///   `Queue_Mission(Move)` and `SetDestination(cell, 1)`
    ///   (`0x0051D487..0x0051D6E0`), with no Process.
    ///
    /// Evidence: tools/spatial_oracle/hut_scatter (found cell) and the null
    /// rows of tools/spatial_oracle/infantry_source_scatter (NullCell answer)
    /// execute the original body.
    ///
    /// [`null_start_direction`]: super::scatter_cell::null_start_direction
    ///
    /// Answers whether the immediate Process changed bridge state.
    pub(super) fn infantry_scatter_null(
        &mut self,
        id: u64,
        flags: ScatterFlags,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        if !self.infantry_scatter_admitted(id, flags, rules, frame_effects)? {
            return Ok(false);
        }
        let infantry = self
            .substrate
            .entities
            .get(id)
            .ok_or("Scatter lost its infantryman")?;
        let coord = super::foot_coordinate::current_coordinate(infantry);
        let start = super::scatter_cell::null_start_direction(
            (coord.x, coord.y),
            infantry.body_facing.current(self.session.binary_frame),
            &mut self.scenario_rng,
        );
        if let Some(cell) = self.scatter_nearby_cell(id, rules) {
            return self.infantry_scatter_destination(
                id,
                cell,
                rules,
                registry,
                true,
                frame_effects,
            );
        }
        let Some(destination) = self.infantry_scatter_neighbor(id, start, rules, registry)? else {
            return Ok(false);
        };
        if let Some(infantry) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                infantry,
                MissionId::from_known(MissionType::Move),
            );
        }
        self.infantry_scatter_destination(id, destination, rules, registry, false, frame_effects)
    }

    /// `InfantryClass::Scatter` with a real coordinate, after
    /// [`Self::infantry_scatter_admitted`]: the away arm
    /// (`0x0051D226..0x0051D2D4`, `0x0051D487..0x0051D6E0`). A found neighbour
    /// takes `Queue_Mission(Move, 0)` and `SetDestination(cell, 1)`; no
    /// locomotor Process runs on this arm. DeploySlaves passes the owner's
    /// centre with (1,1) (`0x006B0667`); the garrison eject passes the
    /// building's centre; the Infantry damage receiver passes its source's
    /// Location with (0,0) (`0x00518C56`).
    pub(crate) fn infantry_scatter_from(
        &mut self,
        id: u64,
        source: (i32, i32),
        flags: ScatterFlags,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        if !self.infantry_scatter_admitted(id, flags, rules, frame_effects)? {
            return Ok(());
        }
        let Some(destination) =
            self.select_infantry_scatter_away_from(id, source, rules, registry)?
        else {
            return Ok(());
        };
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(MissionType::Move),
            );
        }
        self.infantry_scatter_destination(id, destination, rules, registry, false, frame_effects)
            .map(|_| ())
    }

    /// The away-from-a-coordinate direction of `InfantryClass::Scatter`
    /// (`0x0051D258..0x0051D2D4`): the heading away from `source` rounded to
    /// an octant plus `RandomRanged(0, 4) - 2`, then the eight-neighbour
    /// search.
    fn select_infantry_scatter_away_from(
        &mut self,
        id: u64,
        source: (i32, i32),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<(u16, u16)>, String> {
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        let current = super::foot_coordinate::current_coordinate(infantry);
        let start = super::scatter_cell::source_start_direction(
            (current.x, current.y),
            source,
            &mut self.scenario_rng,
        );
        self.infantry_scatter_neighbor(id, start, rules, registry)
    }

    /// The eight-neighbour search both Scatter arms share
    /// (`0x0051D487..0x0051D6BA`), from the navigation cell: only numeric entry
    /// zero is legal, and a soft obstruction is not a passable destination.
    /// Evidence: tools/spatial_oracle/infantry_scatter_entry.{py,json,meta.json}.
    ///
    /// Missing map state keeps headless fixtures compatible: no cell is found.
    fn infantry_scatter_neighbor(
        &mut self,
        id: u64,
        start: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<(u16, u16)>, String> {
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        let current = super::foot_coordinate::current_coordinate(infantry);
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(None);
        };
        let Some(bounds) = self.playfield_bounds else {
            return Ok(None);
        };
        let navigation = super::foot_coordinate::navigation_coordinate(infantry, Some(terrain))?;
        let seed = ((navigation.x / 256) as i16, (navigation.y / 256) as i16);
        let on_bridge = infantry.on_bridge;
        let destination =
            super::scatter_cell::select_neighbor(seed, start, |candidate, direction| {
                let terrain = self.resolved_terrain.as_ref().expect("retained map cells");
                let cells = NativeCellQuery::canonical(terrain);
                // Preserve the retained Cell identity across the height-aware
                // playfield lookup and Object5F5F00's current-cell lookup. A dummy
                // identity is shared and can change its coordinate between reads.
                let cell = cells.lookup(candidate);
                if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (i32::from(candidate.0), i32::from(candidate.1)),
                    Some(bounds),
                    Some(terrain),
                ) {
                    return Ok(None);
                }
                let height =
                    super::ground_pose::query_object_cell_height(&cells, current, on_bridge);
                if self
                    .infantry_can_enter(
                        id,
                        cell,
                        InfantryEntryArgs {
                            direction,
                            height,
                            previous_cell: None,
                        },
                        rules,
                        registry,
                    )?
                    .is_nonzero()
                {
                    return Ok(None);
                }
                Ok::<_, String>(Some(super::scatter_cell::preferred_surface(
                    self.resolved_terrain.as_ref().expect("retained map cells"),
                    candidate,
                )))
            })?;
        Ok(destination.map(|destination| (destination.0 as u16, destination.1 as u16)))
    }

    /// Scatter's `SetDestination(cell, 1)` (`vt+0x480`): the Infantry setter
    /// (`0x0051AA40`) for a Walk, Teleport or Jumpjet man. `process_now` runs
    /// the active locomotor's Process at once, as the found-cell arm does for
    /// every locomotor (`0x0051D45B..0x0051D478`).
    fn infantry_scatter_destination(
        &mut self,
        id: u64,
        cell: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        process_now: bool,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let infantry = self
            .substrate
            .entities
            .get(id)
            .ok_or("Scatter lost its infantryman")?;
        let kind = infantry.locomotor.as_ref().map(|loco| loco.active_kind());
        let speed = scatter_movement_speed(infantry, Some(rules), &self.interner, &self.houses);
        self.assign_infantry_destination(
            id,
            NavTargetRef::cell(cell.0, cell.1),
            speed,
            rules,
            registry,
            frame_effects,
        )?;
        if !process_now
            || !matches!(
                kind,
                Some(LocomotorKind::Walk | LocomotorKind::Teleport | LocomotorKind::Jumpjet)
            )
        {
            return Ok(false);
        }
        // 0x0051D478 is an immediate locomotor invocation, without another
        // object AI, mission timer, global animation tick or frame-tail
        // deletion.
        #[cfg(test)]
        if answered_process::answer(id) {
            return Ok(false);
        }
        if kind == Some(LocomotorKind::Walk) && self.path_grid().is_none() {
            return Err(String::from("Scatter Process requires navigation"));
        }
        let outcome = self
            .process_active_locomotor(
                id,
                Some(rules),
                crate::sim::world::ObjectAiCtx {
                    overlay_registry: registry,
                    effects: frame_effects,
                    ..Default::default()
                },
            )
            .map_err(|error| format!("Scatter Process failed: {error:?}"))?;
        Ok(outcome.bridge_state_changed())
    }
}

/// Test-only answer for the immediate locomotor Process (`0x0051D478`), for
/// replays of oracles that answer the Walk Process slot (`0x0075AC80`) as
/// not moving: while installed, each Process is recorded and its body does
/// not run.
#[cfg(test)]
pub(crate) mod answered_process {
    use std::cell::RefCell;

    thread_local! {
        static PROCESSED: RefCell<Option<Vec<u64>>> = const { RefCell::new(None) };
    }

    pub(crate) fn install() {
        PROCESSED.with(|seam| *seam.borrow_mut() = Some(Vec::new()));
    }

    /// The objects whose Process was answered, in call order.
    pub(crate) fn finish() -> Vec<u64> {
        PROCESSED.with(|seam| seam.borrow_mut().take().unwrap_or_default())
    }

    pub(super) fn answer(id: u64) -> bool {
        PROCESSED.with(|seam| {
            seam.borrow_mut()
                .as_mut()
                .map(|processed| processed.push(id))
                .is_some()
        })
    }
}

impl Simulation {
    /// The Infantry setter's first test (`0x0051AA49..0x0051AA7E`): a human
    /// owner's (`0x0050B730`) infantryman whose Doing is in the deploy family
    /// refuses the destination before any write.
    fn infantry_setter_refuses(&self, actor: &crate::sim::game_entity::GameEntity) -> bool {
        self.owner_is_human(actor.owner())
            && actor
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| DEPLOY_DOINGS.contains(&leaf.doing()))
    }

    /// Whether the non-null Infantry51AA40 destination owner represents this
    /// receiver. This checks port coverage, not native admission.
    pub(crate) fn infantry_setter_receiver(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate.entities.get(id).is_some_and(|actor| {
            self.object_type(actor.type_ref(), rules).is_some()
                && represented_infantry_destination(actor)
        })
    }

    /// Pure dependency availability for the represented 51AA40 owner below.
    /// This neither evaluates4834A0 admission nor performs its Dummy lookup;
    /// the caller can validate a transaction before the actual class call.
    pub(crate) fn infantry_destination_inputs_available(
        &self,
        id: u64,
        requested: NavTargetRef,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        use crate::map::cell_index::NativeCellIdentity;
        use crate::rules::locomotor_type::{LocomotorKind, SpeedType};

        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(object) = self.object_type(actor.type_ref(), rules) else {
            return false;
        };
        if !represented_infantry_destination(actor) || actor.infantry.is_none() {
            return false;
        }
        if actor.mission_leaf.as_infantry().is_none() {
            return false;
        }
        if self.infantry_setter_refuses(actor) {
            return true;
        }
        // Non-cell +4C projections read retained state only. The shared Foot
        // owner validates active Tube/Jumpjet payloads; Building's owner
        // validates its type and, for a Bunker, the requester. Cell+4C cannot
        // return an error and is left to the actual call's lookup order.
        if !matches!(requested, NavTargetRef::Cell { .. })
            && super::navcom::nav_target_coordinate(
                requested,
                Some(id),
                &self.substrate.entities,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            )
            .is_err()
        {
            return false;
        }
        let loco = actor.locomotor.as_ref().expect("represented receiver");
        // Every represented Teleport request reaches its native resolver,
        // including the first request before IsMoving becomes true.
        if loco.active_kind() == LocomotorKind::Teleport {
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                return false;
            };
            let Ok(point) = super::navcom::nav_target_coordinate(
                requested,
                Some(id),
                &self.substrate.entities,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            ) else {
                return false;
            };
            let (rx, ry) = ((point.x / 256) as i16, (point.y / 256) as i16);
            //718B70 reaches51BF90 at the selected target, or at NULL XYZ
            //after slot refusal. Check only retained input availability here;
            //canonical Dummy lookup, admission and RNG keep their native order.
            for (x, y) in [(rx, ry), (0, 0)] {
                let cell = terrain
                    .native_fixed_cell_index(x, y)
                    .map_or(NativeCellIdentity::Dummy, NativeCellIdentity::Real);
                let overlay = self.cell_overlay(cell).overlay_id;
                if overlay.is_some_and(|id| registry.and_then(|r| r.flags(id)).is_none()) {
                    return false;
                }
            }
        }
        let Some(moving) = super::motion_query::is_moving(actor) else {
            return false;
        };
        if !moving {
            return true;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        // 51ABA2 resolves the current Cell even for Winged. Availability is
        // established without issuing native_cell_identity (which stamps
        // the shared Dummy) or reading occupation/admission.
        if object.speed_type == SpeedType::Winged {
            return true;
        }
        let xyz = super::foot_coordinate::current_coordinate(actor);
        let (cell, cost) =
            match terrain.native_fixed_cell_index((xyz.x / 256) as i16, (xyz.y / 256) as i16) {
                Some(index) => (
                    NativeCellIdentity::Real(index),
                    terrain.cells()[index]
                        .speed_costs
                        .cost_for_speed_type(object.speed_type),
                ),
                None => (NativeCellIdentity::Dummy, None),
            };
        let wall = match self.cell_overlay(cell).overlay_id {
            Some(overlay) => {
                let Some(flags) = registry.and_then(|registry| registry.flags(overlay)) else {
                    return false;
                };
                flags.wall
            }
            None => false,
        };
        terrain.native_cell_flags(cell) & 0x100 != 0 || wall || cost.is_some()
    }

    /// The Infantry class setter `vt+0x480(target, 1)` (`0x0051AA40`) for a
    /// Walk, Teleport or Jumpjet infantryman: Infantry51AA40 -> Foot4D94B0 ->
    /// the active locomotor's Move_To (Walk75ACB0, Teleport `0x00718100`, or
    /// Jumpjet through [`Self::jumpjet_foot_destination`]; the setter never
    /// reads `Teleporter=`). Every represented Infantry destination comes
    /// here: Scatter (`0x0051D6E0`), the slave manager's sends, team scripts,
    /// orders, the mission effects, the Jumpjet host's own re-targets and
    /// FindPath's redirect callbacks. In order:
    /// - a human owner's deploy Doing refuses before any write
    ///   (`0x0051AA49..0x0051AA7E`);
    /// - a `JumpJet=` type (`+0xD94`) whose destination's GetCoords (`+0x48`)
    ///   lies in his own cell returns before any write
    ///   (`0x0051AA84..0x0051AB2E`, [`Self::jumpjet_destination_in_own_cell`]);
    /// - the mutual `+0x2A8` link clears (`0x0051AB34`); only the dormant
    ///   `DirectRocker=` warhead raises it, and VERA keeps none;
    /// - while the locomotor `Is_Moving` and his cell passes `0x004834A0`,
    ///   Stop_Driver (`0x0051ABD1`) runs: for a `JumpJet=` type always, for
    ///   another unless he is on Attack with this NavCom already;
    /// - the human same-reference prone DoAction7 (`0x0051ABD7..0x0051AC1F`)
    ///   uses the existing action owner;
    /// - on Enter (current or queued) without a radio contact the transport
    ///   radio arm runs and the path head stays (`0x0051AC25..0x0051AD17`);
    ///   otherwise the path head clears;
    /// - Foot's setter (`0x0051B1D2`).
    ///
    /// Non-cell targets retain their reference and read the receiver's +4C
    /// navigation coordinate, including a Foot's committed head. No Process
    /// runs until the ordinary object turn.
    /// Native comparisons: infantry_scatter_destination.{py,json,meta.json}
    /// and anytown_damage/foot_missions.{py,json,meta.json}; ordinary original
    /// input/event/class reissues are in walk_first_path.json `gi_reissue`.
    /// The two `JumpJet=` arms were read from the instructions, not executed.
    ///
    /// RESIDUAL: the composed `JumpJet=` re-order has not been executed
    /// natively. Its Scenario draws are Stop_Driver's Jumpjet Stop
    /// (`0x0054B4D0`: FNPC, then its own Move_To's placement through
    /// `0x0054D6D0`/`0x004ACA10`), then the order's Move_To placement
    /// (`0x0054B1C0`). Each leaf runs against the original
    /// in `jumpjet_coordinates`; their order is the call order `0x0051ABD1`
    /// then `0x0051B1D2`, and the arm's two tests are the Walk-checked
    /// `0x004834A0` port and the Jumpjet `Is_Moving` the same corpus pins.
    /// Trigger: a re-order of a flying Rocketeer, a cruise re-target or a
    /// team's BalloonHover resend. Effect if misread: the Scenario stream
    /// shifts for every later draw. Frequency: most orders to an airborne
    /// Rocketeer. Risk: RNG drift against native, not a visible movement
    /// difference.
    ///
    /// RESIDUAL: the Enter radio arm is not ported. With a Techno destination
    /// (`0x0040DD70`) it queues him on a transport already in radio contact
    /// (QueueUpToEnter `+0x500`, ArchiveTarget cleared, NULL destination) or
    /// sends RADIO_HELLO and, on ROGER, message 0xE, returning before Foot's
    /// setter when that answers 0xE. VERA writes the object through Foot's
    /// setter either way. Trigger: an infantryman's boarding order
    /// (EnterTransport, Enter 7) or an Enter re-target. Frequency: every
    /// infantry boarding. Risk: boarding a busy transport or one that turns
    /// him away differs from native.
    ///
    /// RESIDUAL: a `JumpJet=` infantryman's Walk piggyback is not modelled.
    /// The setter swaps the Jumpjet of one that is not moving for a Walk
    /// (`0x0051AE5A..0x0051B1CA`) unless `0x005221D0` says fly or the type has
    /// `HoverAttack=` (`+0x390`), ends the piggyback when it says fly, and
    /// pushes the destination onto NavQueue's front for a walking one
    /// (`0x0051AD1F..0x0051AE57`). `0x005221D0` says fly when airborne; on
    /// the ground it says walk for a reachable hop of one cell, for two or
    /// three cells in the playfield whose zone cost is at most 7, and in two
    /// house and cell cases (`0x0053A130`, `0x00484AE0`). VERA flies every
    /// hop. Trigger: a grounded `JumpJet=` infantry without `HoverAttack=`
    /// given a short hop. Effect: it lifts off instead of walking. Frequency:
    /// dormant in retail `rulesmd.ini`, where each `JumpJet=` infantry type
    /// also sets `HoverAttack=` (production reader:
    /// `retail_jumpjet_infantry_all_hover_attack`); a map or mode layer could
    /// clear it. Risk: a mod's walking jumpjet infantry fly.
    ///
    /// Returns false for the other unmigrated class setters.
    /// DirectRocker reciprocal links, retained fire particles and the
    /// Unit-produced +6AC latch still lack their production owners here.
    pub(crate) fn assign_infantry_destination(
        &mut self,
        id: u64,
        requested: NavTargetRef,
        speed: SimFixed,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Infantry destination lost actor")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Infantry destination requires type")?;
        if !represented_infantry_destination(actor) {
            return Ok(false);
        }
        if self.infantry_setter_refuses(actor) {
            return Ok(true);
        }
        let jumpjet_type = object.jumpjet;
        if jumpjet_type && self.jumpjet_destination_in_own_cell(actor, requested) {
            return Ok(true);
        }
        let kind = actor
            .locomotor
            .as_ref()
            .map(|l| l.kind)
            .expect("represented receiver");
        let human = self.owner_is_human(actor.owner());
        if !self.infantry_destination_inputs_available(id, requested, rules, registry) {
            return Err("Infantry destination requires available class inputs".into());
        }
        let speed_type = object.speed_type;
        let type_allows_up = !object.fraidycat && !object.cyborg;
        let moving = super::motion_query::is_moving(actor) == Some(true);
        // 51ABA2 invokes the shared Cell leaf BEFORE the JumpJet and
        // Attack/same-NavCom tests. The current physical coordinate owns this
        // lookup.
        if moving && self.infantry_destination_current_cell_clear(id, speed_type, registry)? {
            let actor = self.substrate.entities.get(id).unwrap();
            if jumpjet_type
                || actor.mission.current().raw() != 1
                || !super::navcom::nav_targets_same_receiver(actor.navigation.nav_com, requested)
            {
                self.infantry_stop_driver(id, rules, registry, frame_effects)?;
            }
        }
        // 51ABD7..51AC1F: the human same-reference prone request is an
        // unforced DoAction(Up,0,0), before the class path-head write. Its
        // sequence/action admission remains with the sole Do_Action owner.
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("destination lost infantry after Stop_Driver")?;
        if human
            && type_allows_up
            && super::navcom::nav_targets_same_receiver(actor.navigation.nav_com, requested)
            && actor.infantry.as_ref().is_some_and(|state| state.is_prone)
        {
            self.infantry_do_action(id, 7, false, rules, frame_effects)?;
        }
        super::movement_commands::clear_destination_path_head(
            self.substrate
                .entities
                .get_mut(id)
                .ok_or("destination lost infantry after DoAction")?,
        );
        if !self.begin_foot_destination(id, true, Some(rules), registry, frame_effects) {
            return Ok(true);
        }
        match kind {
            LocomotorKind::Teleport => {
                // Foot tail: NavCom, then Teleport Move_To, then the accepted
                // timers whatever it answered (`0x004D96C2..0x004D9707`).
                let frame = self.session.binary_frame;
                let actor = self.substrate.entities.get_mut(id).unwrap();
                super::navcom::publish_nav_com(actor, requested);
                // Infantry51AA40 -> Foot4D94B0 samples destination +4C before
                // Teleport718100. An Enter carries the vehicle, not a Cell alias.
                // Original object coordinate/resolver controls:
                // infantry_teleport_destination.json transport_rows.
                let point = super::navcom::nav_target_coordinate(
                    requested,
                    Some(id),
                    &self.substrate.entities,
                    self.resolved_terrain.as_ref(),
                    Some((rules, &self.interner)),
                )?;
                let accepted =
                    self.teleport_move_to(id, point, rules, false, registry, frame_effects)?;
                let actor = self.substrate.entities.get_mut(id).unwrap();
                super::DestinationTiming::from_rules(frame, Some(rules)).accept(actor);
                Ok(accepted)
            }
            LocomotorKind::Jumpjet => {
                Ok(self.jumpjet_foot_destination(id, requested, speed, rules))
            }
            _ => {
                let coord = super::navcom::nav_target_coordinate(
                    requested,
                    Some(id),
                    &self.substrate.entities,
                    self.resolved_terrain.as_ref(),
                    Some((rules, &self.interner)),
                )?;
                super::prepare_walk_destination(
                    &mut self.substrate.entities,
                    id,
                    (requested, coord),
                    speed,
                    self.resolved_terrain.as_ref(),
                    super::DestinationTiming::from_rules(self.session.binary_frame, Some(rules)),
                );
                Ok(true)
            }
        }
    }

    /// The Infantry setter's JumpJet own-cell test (`0x0051AA84..0x0051AB2E`):
    /// his GetCoords (`+0x48`) and the destination's, each truncated to its
    /// cell (signed leptons / 256, `0x0042D470`), name the same cell. A Cell
    /// answers its centre, an object its own GetCoords.
    fn jumpjet_destination_in_own_cell(
        &self,
        actor: &crate::sim::game_entity::GameEntity,
        requested: NavTargetRef,
    ) -> bool {
        let terrain = self.resolved_terrain.as_ref();
        let cells = terrain.map(NativeCellQuery::canonical);
        let here = super::ground_pose::object_get_coords(actor, terrain);
        let Some(there) = super::ground_pose::target_get_coords(
            crate::sim::combat::TargetKind::from(requested),
            &self.substrate.entities,
            cells.as_ref(),
        ) else {
            return false;
        };
        let cell = |coord: crate::sim::components::DriveCoord| (coord.x / 256, coord.y / 256);
        cell(here) == cell(there)
    }

    /// [`Self::assign_infantry_destination`] at the mover's own
    /// movement speed (`GetCurrentSpeed`, as an ordinary Move order).
    pub(crate) fn set_infantry_destination(
        &mut self,
        id: u64,
        requested: NavTargetRef,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let speed = {
            let infantry = self
                .substrate
                .entities
                .get(id)
                .ok_or("destination lost infantry actor")?;
            scatter_movement_speed(infantry, Some(rules), &self.interner, &self.houses)
        };
        self.assign_infantry_destination(id, requested, speed, rules, registry, frame_effects)
    }

    /// The Infantry class setter `vt+0x480(NULL, 1)` (`0x0051AA40`), every
    /// infantryman's whatever his locomotor:
    /// - a human owner's deploy action (Doing 27..30) refuses before any
    ///   write (`0x0051AA49..0x0051AA7E`), and the call answers false;
    /// - the mutual `+0x2A8` link (`0x0051AB34`) clears; only the dormant
    ///   `DirectRocker=` warhead raises it, and VERA keeps none;
    /// - the path head clears unless an Enter lacks a radio contact
    ///   (`0x0051AC25..0x0051AD17`, `clear_destination_path_head`);
    /// - Foot's null arm follows (`0x0051B1D2`,
    ///   [`Self::foot_null_destination`]): Walk's Stop (`0x0075ADA0`) keeps
    ///   a paid head and consumes a pending Deploy through owner +0x54C, a
    ///   moving Jumpjet's re-targets the cell under him, a Teleport's drops
    ///   an armed warp.
    ///
    /// NavQueue is not touched. The scheduling adapter is the caller's. The
    /// Jumpjet Stop's failed search uses the caller's overlay registry for
    /// synchronous damage. Walk's virtual class callers
    /// dispatch through [`Self::assign_null_destination`].
    pub(crate) fn set_infantry_null_destination(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        if self.infantry_setter_refuses(actor) {
            return false;
        }
        super::movement_commands::clear_destination_path_head(
            self.substrate
                .entities
                .get_mut(id)
                .expect("same setter actor"),
        );
        self.foot_null_destination(id, rules, registry, frame_effects);
        true
    }

    /// 51AB73..51ABA7 passes (SpeedType,1,0,-1,Normal,-1,true) to4834A0.
    /// This is not Infantry CanEnter: ignore the five infantry bits, retain
    /// vehicles, choose the deck on a structural bridge, and reject all walls.
    fn infantry_destination_current_cell_clear(
        &self,
        id: u64,
        speed: crate::rules::locomotor_type::SpeedType,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        use super::locomotor::MovementLayer;
        use crate::map::cell_index::NativeCellIdentity;
        use crate::rules::locomotor_type::{MovementZone, SpeedType};
        use crate::sim::cell_rect::{
            IsClearToMoveRequest, IsClearToMoveResult, evaluate_is_clear_to_move,
        };
        use crate::sim::occupancy::RawCellKey;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("scatter lost current cell owner")?;
        let coord = super::foot_coordinate::current_coordinate(actor);
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("scatter setter requires map cells")?;
        let cell = terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
        if speed == SpeedType::Winged {
            return Ok(true);
        }
        let raw = &self.substrate.raw_cell_occupation;
        let key = RawCellKey::from_native(terrain, cell);
        let (level, cost) = match cell {
            NativeCellIdentity::Real(index) => {
                let c = &terrain.cells()[index];
                (
                    i16::from(c.level as i8),
                    c.speed_costs.cost_for_speed_type(speed),
                )
            }
            NativeCellIdentity::Dummy => (
                i16::from(terrain.shared_cell_dummy().snapshot().level),
                None,
            ),
        };
        let overlay = self.cell_overlay(cell).overlay_id;
        let mut input = IsClearToMoveRequest {
            speed_type: speed,
            movement_zone: MovementZone::Normal,
            requested_zone: None,
            actual_zone: 0,
            base_level: level,
            has_bridge: terrain.native_cell_flags(cell) & 0x100 != 0,
            requested_level: None,
            is_bridge: true,
            ground_occupation_bits: raw.bits_at(key, MovementLayer::Ground),
            deck_occupation_bits: raw.bits_at(key, MovementLayer::Bridge),
            ignore_infantry: true,
            ignore_vehicles: false,
            land_passable: true,
            is_wall_overlay: false,
            wall_allows_crusher: false,
        };
        if !matches!(
            evaluate_is_clear_to_move(input),
            IsClearToMoveResult::Clear { .. }
        ) {
            return Ok(false);
        }
        if let Some(overlay) = overlay {
            input.is_wall_overlay = registry
                .and_then(|r| r.flags(overlay))
                .ok_or("scatter setter overlay lacks registered type")?
                .wall;
        }
        // Bridge land rows cannot refuse; walls still can. Do not invent a
        // Clear land speed for a dummy whose native row is not represented.
        if !input.has_bridge && !input.is_wall_overlay {
            input.land_passable =
                cost.ok_or("scatter setter requires current cell speed row")? != 0;
        }
        Ok(matches!(
            evaluate_is_clear_to_move(input),
            IsClearToMoveResult::Clear { .. }
        ))
    }
}

#[cfg(test)]
#[path = "infantry_scatter_tests.rs"]
mod tests;
