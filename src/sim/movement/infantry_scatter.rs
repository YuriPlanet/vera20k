//! Live source-aware Infantry51D0D0 selection. The damage receiver commits
//! the destination before fear; class entry and navigation keep their owners.
use super::bump_crush::{
    InfantryDamageScatter, infantry_damage_scatter_admitted, scatter_movement_speed,
};
use super::infantry_entry::InfantryEntryArgs;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

impl Simulation {
    /// 51D52B..51D6BA: only numeric entry zero is legal. A soft obstruction
    /// is not a passable scatter destination. Evidence: original class calls
    /// in tools/spatial_oracle/infantry_scatter_entry.{py,json,meta.json}.
    ///
    /// Missing map state retains headless fixture compatibility; malformed live
    /// class state is an error, never a manufactured native refusal. NULL-source
    /// FNPC and additional class-setter branches remain separate continuations.
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
        let human = self
            .houses
            .get(&infantry.owner())
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero));
        if !infantry_damage_scatter_admitted(
            infantry,
            rules,
            human,
            &self.team_script_vm,
            &self.interner,
        ) {
            return Ok(None);
        }
        self.select_infantry_scatter_away_from(id, source, rules, registry)
    }

    /// The away-from-a-coordinate arm of `InfantryClass::Scatter @
    /// 0x0051D0D0` once its gates have passed (`0x0051D258..0x0051D6BA`):
    /// the heading away from `source` rounded to an octant plus
    /// `RandomRanged(0, 4) - 2`, then the eight-neighbour search from the
    /// navigation cell. Shared by the damage receiver and the forced
    /// Scatter (`scatter_infantry_forced_from`).
    pub(crate) fn select_infantry_scatter_away_from(
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
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(None);
        };
        let Some(bounds) = self.playfield_bounds else {
            return Ok(None);
        };
        let navigation = super::foot_coordinate::navigation_coordinate(infantry, Some(terrain))?;
        let seed = ((navigation.x / 256) as i16, (navigation.y / 256) as i16);
        let on_bridge = infantry.on_bridge;
        let speed = scatter_movement_speed(infantry, Some(rules), &self.interner);
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
}

impl Simulation {
    /// The Infantry class setter `vt+0x480(cell, 1)` (`0x0051AA40`) for a
    /// Walk infantryman: Infantry51AA40 -> Foot4D94B0 -> Walk75ACB0. Reached
    /// from Scatter (`0x0051D6E0`) and from the slave manager's sends. The
    /// human same-destination/prone DoAction7 arm (`0x0051ABD7..`) needs the
    /// requested cell to already be the NavCom: the damage caller requires
    /// Fraidycat, and the slave sends replace a missing or different NavCom.
    /// The Cell target cannot enter the Techno dock arm. No Process runs until
    /// the ordinary object turn.
    /// Native comparisons: infantry_scatter_destination.{py,json,meta.json}.
    ///
    /// Returns false for the still-unmigrated non-Walk/JumpJet class setters.
    /// DirectRocker reciprocal links, lifted-unit release, retained fire particles
    /// and the Unit-produced +6AC latch still lack their production owners here.
    pub(crate) fn assign_infantry_walk_cell_destination(
        &mut self,
        id: u64,
        scatter: InfantryDamageScatter,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::components::NavTargetRef;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("scatter destination lost actor")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("scatter destination requires type")?;
        if object.jumpjet
            || !actor
                .locomotor
                .as_ref()
                .is_some_and(|l| l.kind == LocomotorKind::Walk)
        {
            return Ok(false);
        }
        let human = self
            .houses
            .get(&actor.owner())
            .is_some_and(|h| h.is_controlled_by_human(self.session.game_mode_nonzero));
        if human
            && actor
                .mission_leaf
                .as_infantry()
                .is_some_and(|l| (27..=30).contains(&l.doing()))
        {
            return Ok(true);
        }
        let requested = NavTargetRef::cell(scatter.destination.0, scatter.destination.1);
        let moving = actor.locomotor.as_ref().and_then(|l| l.walk_is_moving()) == Some(true);
        // 51ABA2 invokes the shared Cell leaf BEFORE the Attack/same-NavCom
        // exception. The current physical coordinate owns this lookup.
        if moving
            && self.infantry_destination_current_cell_clear(id, object.speed_type, registry)?
        {
            let actor = self.substrate.entities.get(id).unwrap();
            if actor.mission.current().raw() != 1 || actor.navigation.nav_com != Some(requested) {
                self.infantry_stop_driver(id, rules, registry)?;
            }
        }
        super::movement_commands::clear_destination_path_head(
            self.substrate.entities.get_mut(id).unwrap(),
        );
        if !self.begin_foot_destination(id, true, rules) {
            return Ok(true);
        }
        super::prepare_walk_cell_destination(
            &mut self.substrate.entities,
            id,
            scatter.destination,
            scatter.speed,
            self.resolved_terrain.as_ref(),
            super::DestinationTiming::from_rules(self.session.binary_frame, Some(rules)),
        );
        Ok(true)
    }

    /// [`Self::assign_infantry_walk_cell_destination`] at the mover's own
    /// movement speed (`GetCurrentSpeed`, as an ordinary Move order).
    pub(crate) fn set_infantry_cell_destination(
        &mut self,
        id: u64,
        cell: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let speed = {
            let infantry = self
                .substrate
                .entities
                .get(id)
                .ok_or("cell destination lost actor")?;
            scatter_movement_speed(infantry, Some(rules), &self.interner)
        };
        self.assign_infantry_walk_cell_destination(
            id,
            InfantryDamageScatter {
                destination: cell,
                speed,
            },
            rules,
            registry,
        )
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
