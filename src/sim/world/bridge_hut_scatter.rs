//! Synchronous BridgeRepairHut evacuation, after target expiration.
//!
//! Native Building4576F0 visits the Infantry registry and calls
//! Infantry51D0D0(NULL,true,true). The successful FNPC arm installs a Cell
//! destination and invokes only locomotor Process before the next receiver.

use super::{FrameAdvanceError, Simulation};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell,
    map_owned_radius_cap,
};
use crate::sim::movement::{self, locomotor::MovementLayer};

/// Post-DoAction31 gates of Infantry51D174..51D220 for NULL,true,true.
/// Current mission supplies Scatter; Doing is the retained Infantry leaf.
fn forced_scatter_admitted(
    doing: i32,
    moving: bool,
    mission_scatter: bool,
    fraidycat: bool,
    has_attack: bool,
) -> Option<bool> {
    if moving && (!mission_scatter || (!fraidycat && has_attack)) {
        return Some(false);
    }
    let permitted = crate::rules::infantry_sequence::scatter_allowed_by_doing(doing)?;
    Some(permitted && (!moving || fraidycat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_scatter_gates_match_original_hut_caller() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/hut_scatter.json"
        ))
        .unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 12);
        for row in &rows[..8] {
            let input = &row["input"];
            let output = &row["output"];
            let admitted = forced_scatter_admitted(
                input["doing"].as_i64().unwrap() as i32,
                input["moving"].as_bool().unwrap(),
                input["mission_scatter"].as_bool().unwrap(),
                input["fraidycat"].as_bool().unwrap(),
                input["attack"].as_bool().unwrap(),
            )
            .unwrap();
            assert_eq!(
                admitted,
                output["destination_changed"].as_bool().unwrap(),
                "{row}"
            );
            let mut rng = crate::sim::rng::SimRng::new(31);
            if admitted {
                rng.next_range_u32_inclusive(0, 4);
            }
            let state = rng.logical_state();
            assert_eq!(
                serde_json::json!([state.index_a, state.index_b]),
                output["random_indices"],
                "{row}"
            );
            if admitted {
                assert_eq!(
                    output["events"],
                    serde_json::json!([
                        "coordinate",
                        "random",
                        "coordinate",
                        ["fnpc", [10, 10], [0, 0]],
                        "destination",
                        "process"
                    ])
                );
            }
        }
    }
}

impl Simulation {
    fn hut_callback_error(&self, id: u64, cause: String) -> FrameAdvanceError {
        FrameAdvanceError::bridge_repair(self.session.tick, self.session.binary_frame, id, cause)
    }

    pub(super) fn scatter_bridge_hut(
        &mut self,
        hut: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, FrameAdvanceError> {
        let mut index = 0;
        let mut changed = false;
        // Ordinary +F8 Uninit5F65F0 retains the Infantry class entry until
        // deferred destruction (517EA2). A direct removal would compact the
        // indexed registry; incrementing the cursor still skips its successor.
        while let Some(id) = self.substrate.entities.infantry_registry_at(index) {
            index += 1;
            let coord = self
                .foot_navigation_coordinate(id)
                .map_err(|cause| self.hut_callback_error(id, cause))?;
            let terrain = self.resolved_terrain.as_ref().ok_or_else(|| {
                self.hut_callback_error(id, "hut coordinate requires live map cells".into())
            })?;
            // 457716..45772A: +4C, Map565730, first ground Building47C520.
            let cell =
                terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
            let first_building = match cell {
                NativeCellIdentity::Real(_) => {
                    let xy = terrain.native_cell_coord(cell);
                    self.substrate.occupancy.first_building_on_layer(
                        xy.0 as u16,
                        xy.1 as u16,
                        MovementLayer::Ground,
                    )
                }
                NativeCellIdentity::Dummy => None,
            };
            let e = self
                .substrate
                .entities
                .get(id)
                .expect("query does not remove listener");
            if !e.lifecycle.object_alive || first_building != Some(hut) {
                continue;
            }
            if e.navigation.nav_com.is_some_and(|target| !matches!(target,
                NavTargetRef::Entity{id} | NavTargetRef::Object{id} | NavTargetRef::Building{id} if id == hut)) {
                continue;
            }
            changed |= self.scatter_infantry_from_hut(id, rules, registry)?;
        }
        Ok(changed)
    }

    fn scatter_infantry_from_hut(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, FrameAdvanceError> {
        let e = self
            .substrate
            .entities
            .get(id)
            .expect("hut selected live listener");
        let object = self.object_type(e.type_ref(), rules).ok_or_else(|| {
            self.hut_callback_error(id, "hut Scatter requires Infantry type".into())
        })?;
        let doing = e
            .mission_leaf
            .as_infantry()
            .ok_or_else(|| {
                self.hut_callback_error(id, "hut Scatter requires Infantry Doing".into())
            })?
            .doing();
        if (28..=30).contains(&doing)
            && rules
                .animation_sequence(&object.id)
                .and_then(|set| set.get(&crate::rules::animation_sequence::SequenceKind::Undeploy))
                .is_some_and(|sequence| sequence.frame_count != 0)
        {
            return Err(self.hut_callback_error(
                id,
                "hut Scatter requires accepted DoAction31 lifetime".into(),
            ));
        }
        // 51D103 passes (31,false,false), so Doing27 cannot change through
        // its permission gate; it is rejected by the Scatter table below.
        let moving = crate::sim::movement::motion_query::is_moving(e).ok_or_else(|| {
            self.hut_callback_error(id, "hut Scatter requires active locomotor Is_Moving".into())
        })?;
        let mission_scatter = if moving {
            e.mission
                .current()
                .known()
                .and_then(|mission| rules.mission_control.entry(mission))
                .ok_or_else(|| {
                    self.hut_callback_error(
                        id,
                        "hut Scatter requires current mission control".into(),
                    )
                })?
                .scatter
        } else {
            true
        };
        // Literal third argument true bypasses PlayerScatter/SCATTER after
        // pure rank/type reads. The last force/Fraidycat gate remains.
        if !forced_scatter_admitted(
            doing,
            moving,
            mission_scatter,
            object.fraidycat,
            e.attack_target.is_some(),
        )
        .ok_or_else(|| self.hut_callback_error(id, "hut Scatter has invalid Doing".into()))?
        {
            return Ok(false);
        }
        // Stock infantry locomotors are Walk and Jumpjet. Both reach the same
        // FNPC/SetDestination(+0x480) arm below; only the immediate locomotor
        // Process (51D478 -> ILocomotion+0x40) differs per kind. The stock
        // Teleport infantryman (CLEG) cannot be a hut occupant: its warp
        // destination must be an FNPC-passable cell and the hut cell holds a
        // Building, so its Teleport MoveTo continuation is not delivered here.
        let kind = e
            .locomotor
            .as_ref()
            .map(|loco| loco.active_kind())
            .filter(|kind| matches!(kind, LocomotorKind::Walk | LocomotorKind::Jumpjet))
            .ok_or_else(|| {
                self.hut_callback_error(
                    id,
                    "admitted hut Scatter requires a Walk or Jumpjet locomotor".into(),
                )
            })?;
        let speed_type = object.speed_type;
        let on_bridge = e.on_bridge;
        // Null-threat angle only determines the later eight-neighbour fallback.
        // The successful FNPC arm does not consume it, but still draws first.
        let _fallback_direction_draw = self.scenario_rng.next_range_u32_inclusive(0, 4);
        let seed = self
            .foot_navigation_coordinate(id)
            .map_err(|cause| self.hut_callback_error(id, cause))?;
        let seed = (
            i32::from((seed.x / 256) as i16),
            i32::from((seed.y / 256) as i16),
        );
        let size = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .map(|(bounds, height)| (bounds.base, height))
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())
            .ok_or_else(|| {
                self.hut_callback_error(id, "hut FNPC requires original Map Size".into())
            })?;
        let grid = self.path_grid_snapshot();
        let destination = find_nearby_passable_cell(
            seed,
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type,
                    required_zone_id: None,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: on_bridge,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: true,
                check_height: true,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(size.0, size.1),
                // 51D39F/3A4 zero the separate target CellStruct.
                target_cell: None,
                path_grid: grid.as_deref(),
                resolved_terrain: self.resolved_terrain.as_ref(),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds,
            },
            self.session.binary_frame,
        );
        let destination = destination.ok_or_else(|| {
            self.hut_callback_error(
                id,
                "hut Scatter requires the eight-neighbour fallback after FNPC failure".into(),
            )
        })?;
        let move_info = self
            .resolve_move_info(id, Some(rules))
            .expect("selected listener");
        if kind == LocomotorKind::Jumpjet {
            // SetDestination(cell,1) reaches Jumpjet MoveTo 54B1C0 through the
            // existing air destination owner (FNPC + placement + cached XYZ).
            // Residual: native then runs one Jumpjet Process 54AEC0 (51D478)
            // immediately; the compatibility air adapter advances this actor
            // at its ordinary object turn instead, one frame later.
            if !self.issue_air_cell_destination(id, destination, move_info.speed, Some(rules)) {
                return Err(self.hut_callback_error(
                    id,
                    "hut Jumpjet destination requires its failed-placement continuation".into(),
                ));
            }
            return Ok(false);
        }
        let grid = grid
            .as_deref()
            .ok_or_else(|| self.hut_callback_error(id, "hut Process requires navigation".into()))?;
        movement::prepare_walk_cell_destination(
            &mut self.substrate.entities,
            id,
            destination,
            move_info.speed,
            self.resolved_terrain.as_ref(),
            crate::sim::movement::DestinationTiming::new(
                self.session.binary_frame,
                rules.general.blockage_path_delay_ticks,
            ),
        );
        // 51D478 is an immediate locomotor invocation, without another object
        // AI, mission timer, global animation tick, or frame-tail deletion.
        let outcome = self.process_ground_locomotor_one(id, Some(rules), Some(grid), registry)?;
        Ok(outcome.bridge_state_changed)
    }
}
