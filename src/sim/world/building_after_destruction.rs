//! `BuildingClass::AfterDestruction @ 0x00441F60`: the rubble a destroyed
//! `LeaveRubble=yes` building leaves on its foundation, and the crate a
//! `CrateBeneath=yes` building leaves behind.
//!
//! Native callers run it right after the dying building's UnInit (`vt+0xF8`):
//! `BuildingClass::ReceiveDamage` (`0x004426A2`) when DestructionEffects
//! armed a running death timer, and `BuildingClass::Update` (`0x004400E5`)
//! when that timer expires (an `Explodes=` type or a building dying while
//! Selling, whose timer is zero). Both run it from the fatal lifecycle's
//! AfterDeathEffects stage, after `uninit_with_context`.
//!
//! Rubble: when the type's rubble art answers
//! ([`crate::rules::building_rubble`], `0x0045F160`), each foundation cell in
//! `vt+0x108(0)` order is redrawn (`CellClass::MarkForRedraw @ 0x00486E70`:
//! tactical rect and radar cell), takes overlay 0xEF with a null rubble type,
//! runs `RecalcAttributes(-1)` (`0x0047D2B0`), `AssignOrphanedCellZone`
//! (`0x0056D460`) and the local graph update (`0x00584550`), then the
//! cell-target detach sweep (`0x0070D4A0`). The origin cell (`vt+0xAC`,
//! Location minus 0x80, the north-west cell) then records the type for the
//! overlay draws.
//!
//! Crate (`0x004421A4..0x0044222D`), whether or not the rubble gate passed:
//! for `CrateBeneath=` (`+0x1767`) the origin cell (`vt+0xAC` again) goes to
//! [`crate::sim::crates::place_crate`] (`MapClass::Place_Crate @ 0x0056BEC0`)
//! with powerup 0 (Money) for `CrateBeneathIsMoney=` (`+0x1769`), else `0x14`
//! (random at pickup). The lobby Crates option is not consulted. Every
//! retail `CrateBeneath=` type also sets `LeaveRubble=`; when its rubble art
//! binds, the origin cell already holds the passable rubble overlay. Where the
//! origin is a direct candidate of the placer's nearby-cell search (it projects
//! back to itself, as on flat ground or a level plateau) the search lands
//! there and the crate commit's no-overlay check refuses it, so no crate
//! appears; where higher ground hides the origin, the search may pick a
//! neighbour instead. That outcome composes the ported owners and has not
//! been compared against native execution.
//!
//! Evidence: instruction reading of `0x00441F60..0x0044222D` and its callees
//! named above. The crate tail's draws are the placer's: one Scenario
//! `RandomRanged(0, 0x7FFFFFFE)` for the slot timer when the cell is
//! accepted. No other RNG draws, timer writes or detach calls besides the
//! cell sweep.
//!
//! RESIDUAL: the screen rect AfterDestruction dirties for the origin cell
//! (`0x00442085..0x0044219F`) is presentation; VERA's overlay upsert redraws
//! it.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use std::collections::BTreeSet;

use crate::sim::intern::InternedId;

use super::{FrameEffects, Simulation};

impl Simulation {
    /// `BuildingClass::AfterDestruction @ 0x00441F60` for a building of
    /// `type_id` whose north-west cell was `origin`.
    pub(crate) fn building_after_destruction(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        type_id: InternedId,
        origin: (u16, u16),
        effects: FrameEffects<'_>,
    ) {
        let Some(registry) = registry else {
            return;
        };
        let Some(object) = self.interner.object_type(type_id, rules) else {
            return;
        };
        if rules.building_rubble_art(&object.id).is_some() {
            self.leave_building_rubble(
                rules,
                registry,
                type_id,
                &object.foundation,
                origin,
                effects,
            );
        }
        if object.crate_beneath {
            let powerup = object
                .crate_beneath_is_money
                .then_some(crate::rules::powerups::POWERUP_MONEY as u8);
            crate::sim::crates::place_crate(
                self,
                rules,
                registry,
                (origin.0 as i16, origin.1 as i16),
                powerup,
            );
        }
    }

    /// The rubble arm (`0x00441F8A..0x004421A4`).
    fn leave_building_rubble(
        &mut self,
        rules: &RuleSet,
        registry: &OverlayTypeRegistry,
        type_id: InternedId,
        foundation: &str,
        origin: (u16, u16),
        effects: FrameEffects<'_>,
    ) {
        let cells = crate::sim::crew_survival::foundation_cells(origin.0, origin.1, foundation);
        // The building is already unmarked, so presence cannot change between
        // cells. Before the first navigation build there is nothing to publish
        // or repair ([`Simulation::publish_recalculated_cells`]).
        let navigation_installed = self.path_grid.is_some() || self.zone_grid.is_some();
        let blocked = if navigation_installed {
            super::navigation::structure_blocked_among(
                &self.substrate.entities,
                &self.interner,
                rules,
                &cells,
            )
        } else {
            BTreeSet::new()
        };
        for cell in cells {
            self.dirty_cells.mark_radar([cell]);
            self.dirty_cells.mark_tactical(cell);
            let (Some(grid), Some(terrain)) =
                (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut())
            else {
                break;
            };
            if !grid.write_rubble_fields(terrain, registry, cell.0, cell.1) {
                continue;
            }
            grid.recalculate_runtime_cell(terrain, registry, cell);
            if navigation_installed {
                self.publish_cell_and_assign_orphaned_zone(rules, cell, &blocked);
            }
            self.stop_all_targeting_cell(cell.0, cell.1, Some(rules), Some(registry), effects);
        }
        if let Some(grid) = self.overlay_grid.as_mut() {
            grid.set_rubble_building_type(origin.0, origin.1, type_id);
        }
    }
}
