//! FootAI4DA692..4DA7AA: admitted high-flying sight refresh before Process.
//! Per-viewer clocks preserve the original local-player admission boundary;
//! House/view-cache reconciliation never advances these clocks or emits events.
use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::{pathfinding::PathGrid, vision};

impl Simulation {
    /// `MapClass::Reveal @ 0x00577D90` for one house: every allocated cell
    /// becomes explored, once (`House+0x240`). Natively only `PlayerPtr`'s
    /// shroud changes; VERA keeps one plane per house, so the lobby's no-shroud
    /// option and the Reveal crate both ask for their house here.
    pub(crate) fn reveal_whole_map_for_owner(&mut self, owner: crate::sim::intern::InternedId) {
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            let cells: Vec<(u16, u16)> = terrain.iter().map(|cell| (cell.rx, cell.ry)).collect();
            self.fog.reveal_cells_for_owner(owner, cells);
        } else {
            self.fog.reveal_all_for_owner(owner);
        }
    }

    /// The settings every reveal reads (`TechnoClass::UpdateReveal @
    /// 0x0070AF50` and its callers, the Psychic Reveal): with a live map the
    /// stored playfield byte gates a Techno's, the map's `Size=` diamond
    /// bounds them, and the rules' sight keys and `AllyReveal=` shape them.
    pub(crate) fn sight_reveal_config(&self, rules: Option<&RuleSet>) -> vision::VisionConfig {
        vision::VisionConfig::new(
            rules,
            self.map_size_diamond(),
            self.playfield_bounds.is_some(),
        )
    }

    pub(super) fn refresh_high_flying_sight_before_process(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
    ) {
        let frame = self.session.binary_frame;
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        if entity.dying
            || entity.lifecycle.in_limbo
            || !matches!(
                entity.category,
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
            || !crate::sim::movement::motion_query::is_moving_now(
                entity,
                rules.map(|rules| {
                    crate::sim::movement::SpeedRules::new(
                        rules,
                        &self.interner,
                        &self.type_handles,
                        &self.houses,
                    )
                }),
                frame,
            )
        {
            return;
        }
        let high = crate::sim::movement::air_movement::is_high_flying(
            entity,
            self.resolved_terrain.as_ref(),
            rules.map(|rules| (rules, &self.interner)),
        );
        if !high {
            return;
        }
        self.fog.alliances = self.house_alliances.clone();
        //4DA6B4 loads SOURCE House before4F9A50; reverse-only alliance
        //does not admit this event or mutate this viewer's countdown.
        let viewers = vision::allied_viewers(&self.fog, entity.owner(), &self.interner);
        let due: Vec<_> = viewers
            .into_iter()
            .filter(|&viewer| {
                entity
                    .sight_refresh_timers
                    .timer(viewer)
                    .expired(frame as i32)
            })
            .collect();
        if due.is_empty() {
            return;
        }
        let config = self.sight_reveal_config(rules);
        let height_grid = if config.reveal_by_height() {
            self.path_grid().map(PathGrid::ground_height_grid)
        } else {
            None
        };
        let ability = vision::entity_has_sight_ability(entity, &self.interner, rules);
        for &viewer in &due {
            // The two reveal leaves may reject membership/zero-radius. The
            // due FootAI branch nevertheless reloads15 after its calls.
            if entity.in_playfield && !entity.passenger_role.is_inside_transport() {
                vision::refresh_entity_vision_for_viewer(
                    &mut self.fog,
                    entity,
                    &config,
                    height_grid.as_deref(),
                    ability,
                    &self.interner,
                    viewer,
                );
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            for viewer in due {
                entity.sight_refresh_timers.reload(viewer, frame);
            }
        }
    }
}
