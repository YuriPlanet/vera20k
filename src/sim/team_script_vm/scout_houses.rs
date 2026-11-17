//! Original gamemd.exe TeamClass::Script_Scout006EE800 (script action48).

use super::{FrameEffects, NearbyArgs, NearbyFootprint, PassabilityArgs, RuleSet, Simulation};
use crate::rules::locomotor_type::MovementZone;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::mission::MissionType;
use crate::sim::movement::ground_pose::position_world_coord;

impl Simulation {
    /// 6EE81D..6EEAB7 chooses an unmarked House and one of its buildings;
    /// 6EEAF6..6EEB8D marks it after the leader stops moving. The action
    /// ignores both its argument and new-step flag. No private timer/detach;
    /// CoordinateMove/AssignMissionTarget retain their shared side effects.
    pub(super) fn team_action_scout_houses(
        &mut self,
        team_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(team) = self.team_script_vm.team(team_id) else {
            return;
        };
        let owner = team.owner();
        let selected = team.scout_house;
        let mut completed = false;
        let mut mark = None;
        if let Some(peer) = selected {
            if self.team_leader(team_id, rules).is_some_and(|leader| {
                self.substrate.entities.get(leader).is_some_and(|entity| {
                    entity.mission.current().raw() != MissionType::Move as i32
                        && entity.mission.queued().raw() != MissionType::Move as i32
                })
            }) {
                mark = Some(peer);
            }
        } else {
            let Some(house) = self.houses.get(&owner) else {
                return;
            };
            let peers: Vec<_> = self
                .houses
                .order()
                .iter()
                .copied()
                .filter(|&peer| peer != owner && !house.has_scouted_house(peer))
                .collect();
            if peers.is_empty() {
                completed = true;
            } else {
                // 6EE84D/6EE93F: Scenario ranges, House then Building. The
                // common RNG preserves the no-draw single-candidate case.
                let index = self
                    .scenario_rng
                    .next_range_i32_inclusive(0, peers.len() as i32 - 1);
                let peer = peers[index as usize];
                self.team_script_vm
                    .teams
                    .get_mut(&team_id)
                    .unwrap()
                    .scout_house = Some(peer);
                // 6EE8CD..6EE935 reads BuildingClass::Array, filtering only
                // owner. Dead and limbo entries still belong to this pool.
                let buildings: Vec<_> = (0..self.substrate.entities.building_registry_len())
                    .filter_map(|index| self.substrate.entities.building_registry_at(index))
                    .filter(|&id| {
                        self.substrate
                            .entities
                            .get(id)
                            .is_some_and(|e| e.owner() == peer)
                    })
                    .collect();
                if buildings.is_empty() {
                    mark = Some(peer);
                } else {
                    let index = self
                        .scenario_rng
                        .next_range_i32_inclusive(0, buildings.len() as i32 - 1);
                    let building = self
                        .substrate
                        .entities
                        .get(buildings[index as usize])
                        .unwrap();
                    // 6EE9E1..6EEA21 uses Location, signed /256 (not GetCoords).
                    let location = position_world_coord(&building.position);
                    let seed = (
                        i32::from((location.x / 256) as i16),
                        i32::from((location.y / 256) as i16),
                    );
                    let speed = self
                        .team_leader(team_id, rules)
                        .and_then(|id| self.substrate.entities.get(id))
                        .and_then(|entity| self.object_type(entity.type_ref(), rules))
                        .map(|object| object.speed_type);
                    let cell = speed.and_then(|speed_type| {
                        self.nearby_passable_cell(
                            seed,
                            NearbyArgs::plain(
                                PassabilityArgs {
                                    speed_type,
                                    required_zone_id: None,
                                    movement_zone: MovementZone::Normal,
                                    bridge_aware_zone: false,
                                },
                                NearbyFootprint::SINGLE,
                            ),
                        )
                    });
                    let target = cell
                        .filter(|&cell| cell != (0, 0))
                        .map(|(x, y)| self.canonical_team_cell((x as i16, y as i16)));
                    self.team_assign_mission_target(team_id, target, rules, registry, effects);
                }
            }
        }
        if let Some(peer) = mark {
            if let Some((house, order)) = self.houses.get_mut_with_order(&owner) {
                house.mark_scouted_house(peer, order);
            }
            self.team_script_vm
                .teams
                .get_mut(&team_id)
                .unwrap()
                .scout_house = None;
        }
        self.team_coordinate_move(team_id, rules, registry, effects);
        // 6EEB94..6EEBC2 overwrites CoordinateMove's completion result: only
        // finding no unmarked peer at entry completes this script action.
        if let Some(team) = self.team_script_vm.teams.get_mut(&team_id) {
            team.advance_pending = completed;
        }
    }
}
