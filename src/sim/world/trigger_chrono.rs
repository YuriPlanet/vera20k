//! House50D6D0, called by TAction128/6E1A40: all owned ground Foots to a
//! waypoint's placement sweep. Original Steam gamemd;
//! instruction bodies via tools.native_inspect. Existing cell/entry/sub-cell
//! and Chrono owners retain their arithmetic, RNG, occupancy and cleanup.

use super::{FrameEffects, Simulation};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};

impl Simulation {
    pub(crate) fn chronoshift_house_to(
        &mut self,
        house: InternedId,
        target: (i16, i16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        if target == (0, 0) {
            return Ok(());
        }
        //50D719..50D9A1 visits the mutable Foot array backwards. One shared
        //cursor advances on blocked cells and successful warps, not refusals.
        //Native ends after offset350, inside the existing R11 table (including
        //its original duplicate). Do not rescan from the centre per member.
        let offsets = &crate::sim::combat::cell_spread::sweep(11)[..=350];
        let mut offset = 0;
        let mut index = self.substrate.entities.foot_registry_len();
        while index != 0 {
            index -= 1;
            let Some(id) = self.substrate.entities.foot_registry_at(index) else {
                continue;
            };
            let Some(actor) = self.substrate.entities.get(id) else {
                continue;
            };
            if actor.owner() != house
                || actor.passenger_role.inside_transport().is_some()
                || !matches!(
                    actor.category(),
                    EntityCategory::Unit | EntityCategory::Infantry
                )
            {
                continue;
            }
            let infantry = actor.category() == EntityCategory::Infantry;
            let cell = loop {
                let Some(&(dx, dy)) = offsets.get(offset) else {
                    return Ok(());
                };
                let cell = (target.0.wrapping_add(dx), target.1.wrapping_add(dy));
                if self.mover_can_enter(
                    id,
                    cell,
                    InfantryEntryArgs::REPAIR,
                    EntryQueryMode::CheckLocomotor,
                    rules,
                    registry,
                )? == 0
                {
                    break cell;
                }
                offset += 1;
            };
            let terrain = self
                .resolved_terrain
                .as_ref()
                .ok_or("Chronoshift needs map cells")?;
            let cells = NativeCellQuery::canonical(terrain);
            let identity = cells.lookup(cell);
            //Cell vt48 returns its own centre at its ground elevation; the
            //class ChronoWarpTo adds the structural bridge height later.
            let [x, y] = crate::sim::cell_kernel::native_cell_own_xy(identity, &cells);
            let mut coord = DriveCoord { x, y, z: 0 };
            coord.z = crate::sim::movement::ground_pose::query_ground_height(&cells, coord)?;
            if infantry {
                //50D93B passes priority0/false/false to original481180.
                //Its existing owner pays any sub-cell RNG, even if WarpTo
                //subsequently refuses. A null result is the zero coordinate.
                coord = Self::place_infantry_spot_in_cell(
                    &self.interner,
                    &mut self.scenario_rng,
                    &self.substrate,
                    &self.resolved_terrain,
                    rules,
                    identity,
                    coord,
                )?
                .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
            }
            if self.foot_chrono_warp_to(id, coord, rules, registry, effects)? {
                offset += 1;
                if offset == offsets.len() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}
