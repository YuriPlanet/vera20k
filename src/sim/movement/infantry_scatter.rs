//! `InfantryClass::Scatter @ 0x0051D0D0`: the gates every call shares, the
//! null-coordinate arm, and the away-from-a-coordinate selection the damage
//! receiver, DeploySlaves and the garrison eject use. Class entry and
//! navigation keep their owners.
use super::bump_crush::{InfantryDamageScatter, scatter_movement_speed};
use super::infantry_entry::InfantryEntryArgs;
use super::scatter::{ScatterFlags, mission_permits_scatter};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

fn represented_infantry_destination(
    actor: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
    requested: NavTargetRef,
) -> bool {
    use crate::rules::locomotor_type::LocomotorKind;
    actor.category == crate::map::entities::EntityCategory::Infantry
        && !object.jumpjet
        && actor
            .locomotor
            .as_ref()
            .is_some_and(|loco| match loco.kind {
                LocomotorKind::Walk => true,
                LocomotorKind::Teleport => matches!(requested, NavTargetRef::Cell { .. }),
                _ => false,
            })
}

/// The Doing values of the deploy family, as Scatter (`0x0051D0E3..0x0051D0F5`)
/// and the Infantry setter (`0x0051AA60..0x0051AA7E`) test them.
const DEPLOY_DOINGS: std::ops::RangeInclusive<i32> = 0x1B..=0x1E;
/// Undeploy, the action the forced no-kidding deploy arm requests.
const DO_UNDEPLOY: i32 = 0x1F;

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
                self.infantry_do_action(id, DO_UNDEPLOY, false, rules)?;
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
    ) -> Result<bool, String> {
        if !self.infantry_scatter_admitted(id, flags, rules)? {
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
            return self.infantry_scatter_destination(id, cell, rules, registry, true);
        }
        let Some(scatter) = self.infantry_scatter_neighbor(id, start, rules, registry)? else {
            return Ok(false);
        };
        if let Some(infantry) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                infantry,
                MissionId::from_known(MissionType::Move),
            );
        }
        self.infantry_scatter_destination(id, scatter.destination, rules, registry, false)
    }

    /// `InfantryClass::Scatter` with a real coordinate, after
    /// [`Self::infantry_scatter_admitted`]: the away arm
    /// (`0x0051D226..0x0051D2D4`, `0x0051D487..0x0051D6E0`). A found neighbour
    /// takes `Queue_Mission(Move, 0)` and `SetDestination(cell, 1)`; no
    /// locomotor Process runs on this arm. Answers whether a destination was
    /// installed. DeploySlaves passes the owner's centre with (1,1)
    /// (`0x006B0667`); the garrison eject passes the building's centre.
    pub(crate) fn infantry_scatter_from(
        &mut self,
        id: u64,
        source: (i32, i32),
        flags: ScatterFlags,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        if !self.infantry_scatter_admitted(id, flags, rules)? {
            return Ok(false);
        }
        let Some(scatter) = self.select_infantry_scatter_away_from(id, source, rules, registry)?
        else {
            return Ok(false);
        };
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(MissionType::Move),
            );
        }
        self.assign_infantry_walk_destination(
            id,
            NavTargetRef::cell(scatter.destination.0, scatter.destination.1),
            scatter.speed,
            rules,
            registry,
        )
    }

    /// The Infantry damage receiver's Scatter (`Scatter(attacker, 0, 0)`) up
    /// to its selection. The receiver commits the destination before its fear
    /// callback. Only a live Infantry with a locomotor reaches it.
    pub(crate) fn select_infantry_damage_scatter(
        &mut self,
        id: u64,
        source: (i32, i32),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<InfantryDamageScatter>, String> {
        let Some(infantry) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        if infantry.category != crate::map::entities::EntityCategory::Infantry
            || infantry.dying
            || infantry.health.current == 0
            || infantry.locomotor.is_none()
        {
            return Ok(None);
        }
        if !self.infantry_scatter_admitted(id, ScatterFlags::new(false, false), rules)? {
            return Ok(None);
        }
        self.select_infantry_scatter_away_from(id, source, rules, registry)
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
    ) -> Result<Option<InfantryDamageScatter>, String> {
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
    ) -> Result<Option<InfantryDamageScatter>, String> {
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
        let speed = scatter_movement_speed(infantry, Some(rules), &self.interner, &self.houses);
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
        Ok(destination.map(|destination| InfantryDamageScatter {
            destination: (destination.0 as u16, destination.1 as u16),
            speed,
        }))
    }

    /// Scatter's `SetDestination(cell, 1)` (`vt+0x480`). Walk and Teleport men
    /// take the Infantry setter (`0x0051AA40`). A Jumpjet man takes the
    /// existing air destination owner (FNPC, placement and cached XYZ).
    /// `process_now` runs the locomotor's Process at once, as the found-cell
    /// arm does (`0x0051D478`).
    ///
    /// RESIDUAL: only a Walk man's immediate Process runs here. A Jumpjet or
    /// Teleport man advances at its ordinary object turn instead, one frame
    /// later. Trigger: such a man scattered into a found cell. Effect: its
    /// first step comes one frame later. Frequency: rare; stock Jumpjet and
    /// Teleport infantry are few.
    fn infantry_scatter_destination(
        &mut self,
        id: u64,
        cell: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        process_now: bool,
    ) -> Result<bool, String> {
        let infantry = self
            .substrate
            .entities
            .get(id)
            .ok_or("Scatter lost its infantryman")?;
        let kind = infantry.locomotor.as_ref().map(|loco| loco.active_kind());
        let speed = scatter_movement_speed(infantry, Some(rules), &self.interner, &self.houses);
        if kind == Some(LocomotorKind::Jumpjet) {
            if !self.issue_air_cell_destination(id, cell, speed, Some(rules)) {
                return Err(
                    "Scatter Jumpjet destination requires its failed-placement continuation".into(),
                );
            }
            return Ok(false);
        }
        if !self.assign_infantry_walk_destination(
            id,
            NavTargetRef::cell(cell.0, cell.1),
            speed,
            rules,
            registry,
        )? || !process_now
            || kind != Some(LocomotorKind::Walk)
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
        if self.path_grid().is_none() {
            return Err(String::from("Scatter Process requires navigation"));
        }
        let outcome = self
            .process_ground_locomotor_one(id, Some(rules), registry)
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
    /// receiver and target. This checks port coverage, not native admission.
    pub(crate) fn infantry_setter_receiver(
        &self,
        id: u64,
        requested: NavTargetRef,
        rules: &RuleSet,
    ) -> bool {
        self.substrate.entities.get(id).is_some_and(|actor| {
            self.object_type(actor.type_ref(), rules)
                .is_some_and(|object| represented_infantry_destination(actor, object, requested))
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
        if !represented_infantry_destination(actor, object, requested) || actor.infantry.is_none() {
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
        // Every represented Teleport Cell request reaches its native resolver,
        // including the first request before IsMoving becomes true.
        if loco.active_kind() == LocomotorKind::Teleport {
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                return false;
            };
            let NavTargetRef::Cell { rx, ry } = requested else {
                return false;
            };
            //718B70 reaches51BF90 at the selected target, or at NULL XYZ
            //after slot refusal. Check only retained input availability here;
            //canonical Dummy lookup, admission and RNG keep their native order.
            for (x, y) in [(rx as i16, ry as i16), (0, 0)] {
                let overlay = match terrain.native_fixed_cell_index(x, y) {
                    Some(index) => terrain.cells()[index].bridge_facts.overlay_id,
                    None => {
                        u8::try_from(terrain.shared_cell_dummy().overlay_identity_state().0).ok()
                    }
                };
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
        let (cell, overlay, cost) =
            match terrain.native_fixed_cell_index((xyz.x / 256) as i16, (xyz.y / 256) as i16) {
                Some(index) => {
                    let cell = &terrain.cells()[index];
                    (
                        NativeCellIdentity::Real(index),
                        cell.bridge_facts.overlay_id,
                        cell.speed_costs.cost_for_speed_type(object.speed_type),
                    )
                }
                None => (
                    NativeCellIdentity::Dummy,
                    u8::try_from(terrain.shared_cell_dummy().overlay_identity_state().0).ok(),
                    None,
                ),
            };
        let wall = match overlay {
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
    /// Walk or Teleport infantryman: Infantry51AA40 -> Foot4D94B0 -> the
    /// active locomotor's Move_To (Walk75ACB0 or Teleport `0x00718100`; the
    /// setter never reads `Teleporter=`). Reached from Scatter (`0x0051D6E0`),
    /// the slave manager's sends, team scripts, ordinary represented Walk
    /// and Teleport orders, and FindPath's redirect callbacks. The human
    /// same-reference prone DoAction7
    /// arm (`0x0051ABD7..0x0051AC1F`) uses the existing action owner.
    /// Non-cell targets retain their reference and read the receiver's +4C
    /// navigation coordinate, including a Foot's committed head. No Process
    /// runs until the ordinary object turn.
    /// Native comparisons: infantry_scatter_destination.{py,json,meta.json}
    /// and anytown_damage/foot_missions.{py,json,meta.json}; ordinary original
    /// input/event/class reissues are in walk_first_path.json `gi_reissue`.
    ///
    /// Returns false for the still-unmigrated Jumpjet and other class setters.
    /// DirectRocker reciprocal links, lifted-unit release, retained fire particles
    /// and the Unit-produced +6AC latch still lack their production owners here.
    pub(crate) fn assign_infantry_walk_destination(
        &mut self,
        id: u64,
        requested: NavTargetRef,
        speed: SimFixed,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        use crate::rules::locomotor_type::LocomotorKind;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Infantry destination lost actor")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Infantry destination requires type")?;
        let teleport = actor
            .locomotor
            .as_ref()
            .is_some_and(|l| l.kind == LocomotorKind::Teleport);
        // A Jumpjet infantryman's cell takes Foot's setter through the
        // Jumpjet Move_To (`0x0051B1D2` -> `0x004D94B0`), whichever route
        // gave it (the idle Archive handoff, an order, a scatter). An object
        // target of his stays unrepresented, as before.
        let jumpjet_cell = match requested {
            NavTargetRef::Cell { rx, ry }
                if actor
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.jumpjet_runtime())
                    .is_some() =>
            {
                Some((rx, ry))
            }
            _ => None,
        };
        if jumpjet_cell.is_none() && !represented_infantry_destination(actor, object, requested) {
            return Ok(false);
        }
        // The retained Teleport Move_To port still takes a Cell. Ordinary
        // Walk executes non-cell +4C below; do not snap an unported Teleport
        // object destination to that object's physical cell.
        let teleport_cell = match requested {
            NavTargetRef::Cell { rx, ry } => Some((rx, ry)),
            _ => None,
        };
        if self.infantry_setter_refuses(actor) {
            return Ok(true);
        }
        if let Some(cell) = jumpjet_cell {
            super::movement_commands::clear_destination_path_head(
                self.substrate.entities.get_mut(id).unwrap(),
            );
            return Ok(self
                .jumpjet_cell_destination(id, cell, speed, Some(rules))
                .unwrap_or(false));
        }
        let human = self.owner_is_human(actor.owner());
        if !self.infantry_destination_inputs_available(id, requested, rules, registry) {
            return Err("Infantry destination requires available class inputs".into());
        }
        let speed_type = object.speed_type;
        let type_allows_up = !object.fraidycat && !object.cyborg;
        let moving = super::motion_query::is_moving(actor) == Some(true);
        // 51ABA2 invokes the shared Cell leaf BEFORE the Attack/same-NavCom
        // exception. The current physical coordinate owns this lookup.
        if moving && self.infantry_destination_current_cell_clear(id, speed_type, registry)? {
            let actor = self.substrate.entities.get(id).unwrap();
            if actor.mission.current().raw() != 1
                || !super::navcom::nav_targets_same_receiver(actor.navigation.nav_com, requested)
            {
                self.infantry_stop_driver(id, rules, registry)?;
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
            self.infantry_do_action(id, 7, false, rules)?;
        }
        super::movement_commands::clear_destination_path_head(
            self.substrate.entities.get_mut(id).unwrap(),
        );
        if !self.begin_foot_destination(id, true) {
            return Ok(true);
        }
        if teleport {
            // Foot tail: NavCom, then Teleport Move_To, then the accepted
            // timers whatever it answered (`0x004D96C2..0x004D9707`).
            let frame = self.session.binary_frame;
            let actor = self.substrate.entities.get_mut(id).unwrap();
            super::navcom::publish_nav_com(actor, requested);
            let accepted = self.teleport_move_to(
                id,
                teleport_cell.expect("represented Teleport target is a Cell"),
                rules,
                false,
                registry,
            )?;
            let actor = self.substrate.entities.get_mut(id).unwrap();
            super::DestinationTiming::from_rules(frame, Some(rules)).accept(actor);
            return Ok(accepted);
        }
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

    /// [`Self::assign_infantry_walk_destination`] at the mover's own
    /// movement speed (`GetCurrentSpeed`, as an ordinary Move order).
    pub(crate) fn set_infantry_destination(
        &mut self,
        id: u64,
        requested: NavTargetRef,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let speed = {
            let infantry = self
                .substrate
                .entities
                .get(id)
                .ok_or("destination lost infantry actor")?;
            scatter_movement_speed(infantry, Some(rules), &self.interner, &self.houses)
        };
        self.assign_infantry_walk_destination(id, requested, speed, rules, registry)
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
        self.foot_null_destination(id, rules, registry);
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
        let (level, overlay, cost) = match cell {
            NativeCellIdentity::Real(index) => {
                let c = &terrain.cells()[index];
                (
                    i16::from(c.level as i8),
                    c.bridge_facts.overlay_id,
                    c.speed_costs.cost_for_speed_type(speed),
                )
            }
            NativeCellIdentity::Dummy => {
                let dummy = terrain.shared_cell_dummy();
                (
                    i16::from(dummy.snapshot().level),
                    u8::try_from(dummy.overlay_identity_state().0).ok(),
                    None,
                )
            }
        };
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
