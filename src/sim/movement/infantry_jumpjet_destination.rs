//! InfantryClass::Set_Destination's choice between a Jumpjet and a walking
//! controller. The Foot keeps both complete locomotors through IPiggyback.

use super::foot_path::{chebyshev, coord_cell};
use super::{air_movement, ground_pose, motion_query};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query;
use crate::sim::components::NavTargetRef;
use crate::sim::pathfinding::zone_map::ZoneQueryCell;
use crate::sim::world::Simulation;

impl Simulation {
    /// Infantry 0x0051AD17..0x0051B1CA, after the Enter radio arm and before
    /// the Foot setter. None passes a null destination to the Foot.
    pub(super) fn prepare_jumpjet_infantry_destination(
        &mut self,
        id: u64,
        requested: NavTargetRef,
        hover_attack: bool,
        rules: &RuleSet,
    ) -> Result<Option<NavTargetRef>, String> {
        let actor = self.substrate.entities.get(id).ok_or("missing infantry")?;
        let loco = actor
            .locomotor
            .as_ref()
            .ok_or("missing infantry locomotor")?;
        let kind = loco.active_kind();
        if motion_query::is_moving(actor) == Some(true) {
            if kind != LocomotorKind::Walk {
                return Ok(Some(requested));
            }
            // 51ADBE..51AE57: the new order goes on the front. For this
            // call use GetOccupiedCellClass2 (5F6A10), or NULL on a bridge.
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .prepend_nav_list(requested);
            let here = coord_cell(self.foot_navigation_coordinate(id)?);
            let terrain = self
                .resolved_terrain
                .as_ref()
                .ok_or("Walk order needs map cells")?;
            let cells = NativeCellQuery::canonical(terrain);
            let cell = cells.lookup(here);
            let (x, y) = cells.coord(cell);
            return Ok(
                (cells.flags(cell) & 0x100 == 0).then(|| NavTargetRef::cell(x as u16, y as u16))
            );
        }

        // A fresh Walk cannot end a piggyback. HoverAttack (51AFB5) always
        // flies even when the grounded-distance query would choose walking.
        let fly = if hover_attack {
            true
        } else if kind == LocomotorKind::Jumpjet || loco.piggyback().is_some() {
            self.infantry_should_use_jumpjet(id, requested, rules)?
        } else {
            return Ok(Some(requested));
        };
        if kind == LocomotorKind::Jumpjet && !fly {
            self.try_end_piggyback(id, rules);
            let frame = self.session.binary_frame();
            if let Some(loco) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|e| e.locomotor.as_mut())
            {
                loco.begin_piggyback(LocomotorKind::Walk, frame);
            }
        } else if kind != LocomotorKind::Jumpjet && fly {
            self.try_end_piggyback(id, rules);
        }
        Ok(Some(requested))
    }

    /// InfantryClass::ShouldUseJumpjetForDestination (0x005221D0).
    /// HouseClass::IonSensitivesShouldBeOffline (0x0053A130) returns false.
    fn infantry_should_use_jumpjet(
        &self,
        id: u64,
        requested: NavTargetRef,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("missing JumpJet infantry")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("JumpJet walk decision needs map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let to = ground_pose::target_get_coords(requested.into(), self, Some(&cells))
            .ok_or("JumpJet infantry destination has no coordinates")?;
        let from = coord_cell(self.foot_navigation_coordinate(id)?);
        let to = coord_cell(to);
        if air_movement::current_fly_height(actor, Some(terrain)) > 0 {
            return Ok(true);
        }
        if self.owner_is_human(actor.owner()) && cells.is_near_tube_northwest(cells.lookup(from)) {
            return Ok(false);
        }
        let bounds = self
            .playfield_bounds()
            .ok_or("JumpJet walk decision needs playfield bounds")?;
        let size = self
            .map_size_diamond()
            .ok_or("JumpJet walk decision needs map size")?;
        let zones = self
            .zone_grid
            .as_ref()
            .ok_or("JumpJet walk decision needs movement zones")?;
        let goal_deck = cells.flags(cells.lookup(to)) & 0x100 != 0;
        let reachable = zones
            .can_reach_native(
                &cells,
                ZoneQueryCell::Copied(from),
                ZoneQueryCell::Copied(to),
                MovementZone::Infantry,
                actor.on_bridge(),
                goal_deck,
                false,
                bounds,
                size,
            )
            .ok_or("JumpJet walk decision needs native Infantry zone")?;
        if !reachable {
            return Ok(true);
        }
        let distance = chebyshev(from, to);
        if distance <= 1 {
            return Ok(false);
        }
        let in_playfield = |(x, y): (i16, i16)| {
            cell_is_in_playfield_height_aware_in_query(
                (i32::from(x), i32::from(y)),
                Some(bounds),
                Some(terrain),
                Some(&cells),
            )
        };
        if distance >= 4 || !in_playfield(from) || !in_playfield(to) {
            return Ok(true);
        }
        Ok(self.estimate_zone_cost_in_query(
            id,
            ZoneQueryCell::Copied(from),
            ZoneQueryCell::Copied(to),
            actor.on_bridge(),
            goal_deck,
            rules,
            Some(MovementZone::Infantry),
        )? > 7)
    }
}
