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
    /// FNPC and the full class destination setter remain separate continuations.
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
