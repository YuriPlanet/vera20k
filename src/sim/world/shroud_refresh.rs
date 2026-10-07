//! FootAI4DA692..4DA7AA: admitted high-flying sight refresh before Process.
//! Per-viewer clocks preserve the original local-player admission boundary;
//! House/view-cache reconciliation never advances these clocks or emits events.
use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::{pathfinding::PathGrid, vision};

impl Simulation {
    /// The settings a single object's reveal (`TechnoClass::UpdateReveal @
    /// 0x0070AF50` and its callers) reads: the stored playfield byte gates
    /// it, and the rules' sight keys and the game's fog shape it.
    pub(crate) fn sight_reveal_config(&self, rules: Option<&RuleSet>) -> vision::VisionConfig {
        vision::VisionConfig {
            require_playfield_membership: true,
            veteran_sight: rules.map_or(0.0, |r| r.general.veteran_sight),
            leptons_per_sight_increase: rules.map_or(0, |r| r.general.leptons_per_sight_increase),
            reveal_by_height: rules.is_none_or(|r| r.general.reveal_by_height),
            fog_of_war: self.session.game_options.fog_of_war,
        }
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
        let viewers = vision::direct_reveal_viewers(&self.fog, entity.owner(), &self.interner);
        let due: Vec<_> = viewers
            .into_iter()
            //4DA6B4 loads SOURCE House before4F9A50; reverse-only alliance
            //does not admit this event or mutate this viewer's countdown.
            .filter(|&viewer| {
                crate::map::houses::is_allied_with(
                    &self.house_alliances,
                    self.interner.resolve(entity.owner()),
                    self.interner.resolve(viewer),
                )
            })
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
        let height_grid = if config.reveal_by_height {
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
