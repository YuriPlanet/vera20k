//! `UnitClass::AI @ 0x007360C0`'s repair seek (`0x0073671F..0x00736817`):
//! on every 16th frame a computer house's damaged vehicle standing guard
//! drives to the nearest of its House's UnitRepair stations that answers its
//! CAN_LOAD, after archiving its own Cell as the post to return to.
//!
//! The station search is `0x00455DD0`. It admits any nonzero CAN_LOAD
//! answer, not only ROGER: a Naval Yard answers a land vehicle NEGATORY
//! (`0x0043C3F3`), so a damaged computer tank near its own Naval Yard takes
//! that yard as its station.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::object_location;
use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
use crate::util::native_x87::{X87Chop53, distance_3d_leptons, sqrt_approx_f32};

use super::{FrameEffects, Simulation};

/// `0x00455DD0`'s reach, `ftol(CellDiagonal * 20.0)`
/// (`0x00455E03..0x00455E1C`). CellDiagonal is Sqrt_Approx of `2 * 256²`
/// (static initializer `0x0043AF40`, stored as a double), and 20.0 is the
/// double at `0x007E44C8`.
fn unit_repair_reach() -> i32 {
    let diagonal = sqrt_approx_f32(X87Chop53::load_i32(2 * 256 * 256))
        .and_then(X87Chop53::load_f32)
        .expect("Sqrt_Approx of a finite square is finite");
    X87Chop53::ftol_i32_low_masked(X87Chop53::mul(diagonal, X87Chop53::load_i32(20)))
}

impl Simulation {
    /// `UnitClass::AI`'s repair seek (`0x0073671F..0x00736817`), right after
    /// its second Ready/Commence. On a frame whose counter is a multiple of
    /// 16, a Unit on Guard (5) or AreaGuard (0xB) with nothing queued, whose
    /// House no human controls (`0x0050B730`), below its type's Strength,
    /// with no ArchiveTarget unless on AreaGuard, and neither `Harvester=`
    /// (`+0xE0E`) nor `Weeder=` (`+0xE0F`), looks for a station
    /// ([`Self::nearest_unit_repair_station`]). With one, it archives its
    /// own Cell when it has no ArchiveTarget (`0x0070C610`), queues Enter
    /// without commencing (vt+0x1E8(7, 0)), drops its Target (vt+0x3C8) and
    /// takes the station as its destination (vt+0x480(station, 1)).
    pub(crate) fn unit_ai_seek_repair(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if self.session.binary_frame() & 0xF != 0 {
            return;
        }
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        let current = unit.mission.current();
        let area_guard = current == MissionId::from_known(MissionType::AreaGuard);
        if unit.category() != EntityCategory::Unit
            || !(area_guard || current == MissionId::from_known(MissionType::Guard))
            || self
                .houses
                .get(&unit.owner())
                .is_none_or(|house| house.is_controlled_by_human(self.session.game_mode_nonzero()))
            || unit.mission.queued() != MissionId::NONE
        {
            return;
        }
        let Some(object) = self.object_type(unit.type_ref(), rules) else {
            return;
        };
        if unit.health.current() >= object.strength
            || (unit.archive_target().is_some() && !area_guard)
            || object.harvester
            || object.weeder
        {
            return;
        }
        let Some(station) = self.nearest_unit_repair_station(id, rules, effects) else {
            return;
        };
        let Some(unit) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if unit.archive_target().is_none() {
            let post = TargetKind::Cell(unit.position.rx(), unit.position.ry());
            unit.set_archive_target(Some(post));
        }
        let now = self.session.binary_frame();
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Enter),
            0,
            now,
            &LiveReadyInputProvider { rules },
        );
        let _ = self.assign_target_represented(id, None, rules, registry, effects);
        let _ = self.assign_destination_represented(
            id,
            Some(NavTargetRef::object(station)),
            rules,
            registry,
            effects,
        );
    }

    /// `0x00455DD0`: of the House's UnitRepair stations
    /// ([`Self::house_unit_repair_stations`]), in list order, the first
    /// strictly nearest one that answers the Unit's CAN_LOAD (vt+0x278(0x0F))
    /// with anything but silence and lies nearer than [`unit_repair_reach`].
    /// The distance is between the two Locations (`+0x9C`), `(dx²+dz²)+dy²`
    /// through Sqrt_Approx and ftol (`0x00455E5D..0x00455EBF`).
    fn nearest_unit_repair_station(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> Option<u64> {
        let owner = self.substrate.entities.get(id)?.owner();
        let reach = unit_repair_reach();
        let mut best = None;
        let mut best_distance = i32::MAX;
        for station in self.house_unit_repair_stations(owner, rules) {
            let answer = transmit(
                self,
                id,
                station,
                RadioMessage::CanEnter,
                RadioPayload::default(),
                rules,
                effects,
            );
            if answer == RadioResponse::None {
                continue;
            }
            let (Some(unit), Some(building)) = (
                self.substrate.entities.get(id),
                self.substrate.entities.get(station),
            ) else {
                continue;
            };
            let terrain = self.resolved_terrain.as_ref();
            let from = object_location(unit, terrain);
            let to = object_location(building, terrain);
            let distance = distance_3d_leptons([to.x, to.z, to.y], [from.x, from.z, from.y]);
            if distance < best_distance && distance < reach {
                best_distance = distance;
                best = Some(station);
            }
        }
        best
    }
}

#[cfg(test)]
#[path = "unit_repair_seek_tests.rs"]
mod tests;
