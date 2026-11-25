//! Build: placement, walls, repairs.
//!
//! The first owner this manager takes is **selling a structure the house cannot
//! hold**. The wish list asks for it in one line (`出售保不住的建筑`) and nothing
//! in the crate did it: a house that watches a refinery burn down to its last hit
//! point gets nothing back, while a player sells it and keeps a third of the cost.
//!
//! Walls and repairs are the next phases and stay out of this one; the registry
//! names this manager so each phase has somewhere to land.
//!
//! Everything here is a house intent, not a unit order: selling is the button on
//! a building, so it goes through `Demands::intents` and `Intent::SellBuilding`,
//! which already exists and already reaches `Command::SellBuilding`.

use super::{Ctx, Demands, Manager};
use crate::profile::Roles;
use vera20k_core::sim::control::intent::Intent;

/// How much of its health a structure keeps before the house gives up on it.
///
/// A fifth. Higher and the house sells structures that would have survived the
/// fight — measured on the 1v1 lane the base is rarely attacked at all, so a
/// loose threshold would sell perfectly good buildings; lower and the structure
/// is destroyed before the sale lands, because a sale takes effect on the next
/// frame and a structure at a sliver is one shot from gone.
const SELL_FRACTION: f32 = 0.2;

/// How close an enemy has to be before "low health" also means "about to die".
///
/// Health alone is not the question. A structure at nineteen percent behind a
/// wall of defences is being repaired faster than it is being hurt; the same
/// structure with an enemy standing next to it is lost. Twelve cells is the
/// distance a tank covers in a few seconds.
const SELL_THREAT_CELLS: i32 = 12;

#[derive(Debug, Default)]
pub struct Build {
    /// Structures already asked to be sold, so the order is not repeated every
    /// term while the sale is in flight.
    ///
    /// A sale takes effect on the next frame, and a manager that re-asks each
    /// term is the high-frequency order stream the wish list records as a defect:
    /// `高频发布指令以至于单位无法开火`.
    selling: std::collections::BTreeSet<u64>,
}

impl Manager for Build {
    fn propose(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        self.sell_what_cannot_be_held(ctx, out);
    }
}

impl Build {
    /// Sell a structure that is nearly dead **and** has an enemy next to it.
    ///
    /// Both halves are required. The wish list's other item for this manager is
    /// `建筑不能一股脑堵死`; the related defect is a house that sells good
    /// buildings because it panicked, and that is what the second half prevents.
    fn sell_what_cannot_be_held(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        let enemies: Vec<(u16, u16)> = ctx
            .obs
            .visible_enemy_entities()
            .map(|enemy| enemy.cell)
            .collect();

        let mut alive = std::collections::BTreeSet::new();
        for unit in ctx.obs.own_entities() {
            let Some(ty) = ctx.obs.object_type(ctx.rules, unit.type_id) else {
                continue;
            };
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if !profile.roles.contains(Roles::STRUCTURE) {
                continue;
            }
            alive.insert(unit.entity_id);
            if self.selling.contains(&unit.entity_id) {
                continue;
            }
            // A structure with no health left to read is one the projection does
            // not describe yet; selling it would be selling on a guess.
            let Some(health) = ctx.obs.health(unit.entity_id) else {
                continue;
            };
            let Some(max) = ctx.obs.max_health(ctx.rules, unit.type_id) else {
                continue;
            };
            if max <= 0 {
                continue;
            }
            let fraction = health as f32 / max as f32;
            if fraction > SELL_FRACTION {
                continue;
            }
            let threatened = enemies
                .iter()
                .any(|cell| cell_distance(*cell, unit.cell) <= SELL_THREAT_CELLS);
            if !threatened {
                continue;
            }
            self.selling.insert(unit.entity_id);
            out.intents.push(Intent::SellBuilding {
                entity_id: unit.entity_id,
            });
            log::info!(
                "build: selling {} {} at {:?} health={}/{}",
                unit.entity_id,
                ty.id,
                unit.cell,
                health,
                max
            );
        }
        // A structure that died or was sold is no longer in flight.
        self.selling.retain(|id| alive.contains(id));
    }
}

/// Chebyshev distance in cells, the map's own metric.
fn cell_distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    let dx = (i32::from(a.0) - i32::from(b.0)).abs();
    let dy = (i32::from(a.1) - i32::from(b.1)).abs();
    dx.max(dy)
}
