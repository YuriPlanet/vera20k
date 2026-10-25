//! MegaMission group-destination adjustment.
//!
//! Active `gamemd.exe` does not spread a selected group in radial rings.  For
//! each consecutive equal-action/equal-target run, `0x0064CDA0` resolves the
//! FootClass members, chooses the member nearest the group's 3D centroid as an
//! anchor, sorts by 3D distance from that anchor, and probes six cells along
//! each member's anchor-relative direction.

use std::collections::{BTreeMap, BTreeSet};

use crate::util::lepton::lepton_to_cell;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53, distance_3d_leptons, sqrt_approx_f32,
};

const MAX_CANDIDATE_PROBES: usize = 6;

/// One resolved FootClass member in its original staged-command order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupDestinationMember {
    /// Offset of this command within the staged run.
    pub command_index: usize,
    pub entity_id: u64,
    /// Native object coordinates in leptons.
    pub coord: [i32; 3],
    /// Cell returned by the member's source-cell virtual.
    pub source_cell: (i16, i16),
}

/// Read-only results of the native candidate gates.
///
/// The caller computes these in native order: playfield, zone, then
/// `Can_Enter_Cell`.  The distributor deliberately interprets the return code
/// only after the temporary-reservation and height gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateFacts {
    pub in_playfield: bool,
    pub same_zone: bool,
    pub height_band_ok: bool,
    pub can_enter_code: u8,
}

impl CandidateFacts {
    pub const fn outside_playfield() -> Self {
        Self {
            in_playfield: false,
            same_zone: false,
            height_band_ok: false,
            can_enter_code: 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupDestinationAssignment {
    pub command_index: usize,
    pub destination: (i16, i16),
}

/// Reproduce active retail `0x0064CDA0` for one already-filtered MegaMission run.
///
/// `members` must preserve staged-event order.  The callback is invoked at most
/// six times per independently assigned member and supplies the map/world gates
/// that this pure mechanism cannot own.
pub fn distribute_group_destinations<F>(
    clicked_target: (i16, i16),
    members: &[GroupDestinationMember],
    mut candidate_facts: F,
) -> Vec<GroupDestinationAssignment>
where
    F: FnMut(&GroupDestinationMember, (i16, i16)) -> CandidateFacts,
{
    if members.is_empty() {
        return Vec::new();
    }

    let centroid = centroid_3d(members);
    let mut anchor_index = members.len() - 1;
    let mut anchor_distance = distance_3d_leptons(members[anchor_index].coord, centroid);
    for index in (0..anchor_index).rev() {
        let distance = distance_3d_leptons(members[index].coord, centroid);
        if distance < anchor_distance {
            anchor_index = index;
            anchor_distance = distance;
        }
    }
    let anchor_coord = members[anchor_index].coord;
    let anchor_source_cell = members[anchor_index].source_cell;

    let mut sorted_indices = (0..members.len()).collect::<Vec<_>>();
    sorted_indices.sort_unstable_by_key(|&index| {
        (
            distance_3d_leptons(members[index].coord, anchor_coord),
            index,
        )
    });
    // The native 256-entry stage could pack index after distance*1000.
    // VERA's larger stage keeps the same ordering as a tuple and indexes the
    // unchanged same-source-cell rule instead of rescanning every later unit.
    let mut source_members = BTreeMap::<_, Vec<_>>::new();
    for (index, member) in members.iter().enumerate() {
        source_members
            .entry(member.source_cell)
            .or_default()
            .push(index);
    }

    let mut destinations = vec![clicked_target; members.len()];
    let mut assigned = vec![false; members.len()];
    let mut reserved_cells = BTreeSet::new();

    let first_index = sorted_indices[0];
    assigned[first_index] = true;
    reserved_cells.insert(clicked_target);

    for sorted_position in 1..sorted_indices.len() {
        let member_index = sorted_indices[sorted_position];
        if assigned[member_index] {
            continue;
        }
        let member = &members[member_index];
        let member_cell = (
            lepton_to_cell(member.coord[0]),
            lepton_to_cell(member.coord[1]),
        );
        let direction = (
            member_cell.0.wrapping_sub(i32::from(anchor_source_cell.0)),
            member_cell.1.wrapping_sub(i32::from(anchor_source_cell.1)),
        );
        let step = normalized_step(direction);
        let mut candidate_x = cell_center_f32(clicked_target.0);
        let mut candidate_y = cell_center_f32(clicked_target.1);
        let mut chosen = clicked_target;
        let mut saw_bad_candidate = false;

        for _ in 0..MAX_CANDIDATE_PROBES {
            candidate_x = add_stored_f32(candidate_x, step.0);
            candidate_y = add_stored_f32(candidate_y, step.1);
            let candidate = (
                stored_f32_to_i16(candidate_x),
                stored_f32_to_i16(candidate_y),
            );
            let facts = candidate_facts(member, candidate);

            if !facts.in_playfield {
                break;
            }
            if !facts.same_zone {
                saw_bad_candidate = true;
                continue;
            }
            if reserved_cells.contains(&candidate) {
                // Native keeps the most recent reserved cell as its fallback.
                chosen = candidate;
                continue;
            }
            if !facts.height_band_ok {
                break;
            }
            if facts.can_enter_code >= 4 && facts.can_enter_code != 6 {
                saw_bad_candidate = true;
                continue;
            }
            if saw_bad_candidate {
                // gamemd calls EstimateZoneCost here, ignores its return value,
                // and immediately takes the saved fallback. Rust path searches
                // carry no equivalent mutable PathfinderClass scratch state.
                break;
            }

            chosen = candidate;
            break;
        }

        destinations[member_index] = chosen;
        assigned[member_index] = true;
        reserved_cells.insert(chosen);

        // Members whose source-cell virtual returns the same cell receive the
        // same destination. This is the native reason grouped infantry can
        // share a cell; there is no category-based "three infantry" rule here.
        for &later_index in &source_members[&member.source_cell] {
            if !assigned[later_index] {
                destinations[later_index] = chosen;
                assigned[later_index] = true;
            }
        }
    }

    members
        .iter()
        .enumerate()
        .map(|(index, member)| GroupDestinationAssignment {
            command_index: member.command_index,
            destination: destinations[index],
        })
        .collect()
}

fn centroid_3d(members: &[GroupDestinationMember]) -> [i32; 3] {
    let mut sum = [0i64; 3];
    for member in members {
        for (axis, coordinate) in member.coord.into_iter().enumerate() {
            sum[axis] += i64::from(coordinate);
        }
    }
    let count = i64::try_from(members.len()).expect("an allocated group count fits i64");
    // Division still truncates toward zero. The mean remains in the original
    // i32 coordinate range even when a 20,000-member sum exceeds it.
    sum.map(|value| i32::try_from(value / count).expect("coordinate mean fits i32"))
}

fn normalized_step(direction: (i32, i32)) -> (NativeF32Bits, NativeF32Bits) {
    let dx =
        X87Chop53::store_f32(X87Chop53::load_i32(direction.0)).expect("cell delta fits finite f32");
    let dy =
        X87Chop53::store_f32(X87Chop53::load_i32(direction.1)).expect("cell delta fits finite f32");
    if direction == (0, 0) {
        return (dx, dy);
    }

    let dx_value = X87Chop53::load_f32(dx).expect("stored cell delta is finite");
    let dy_value = X87Chop53::load_f32(dy).expect("stored cell delta is finite");
    let squared = X87Chop53::add(
        X87Chop53::mul(dx_value, dx_value),
        X87Chop53::mul(dy_value, dy_value),
    );
    let magnitude_bits =
        sqrt_approx_f32(squared).expect("map-space direction stays in finite f32 range");
    let magnitude = X87Chop53::load_f32(magnitude_bits).expect("Sqrt_Approx direction is finite");

    (
        X87Chop53::store_f32(
            X87Chop53::div(dx_value, magnitude).expect("nonzero direction has nonzero magnitude"),
        )
        .expect("normalized X fits finite f32"),
        X87Chop53::store_f32(
            X87Chop53::div(dy_value, magnitude).expect("nonzero direction has nonzero magnitude"),
        )
        .expect("normalized Y fits finite f32"),
    )
}

fn cell_center_f32(cell: i16) -> NativeF32Bits {
    let half = X87Chop53::load_f64(NativeF64Bits::HALF).expect("0.5 is finite");
    X87Chop53::store_f32(X87Chop53::add(X87Chop53::load_i32(i32::from(cell)), half))
        .expect("map cell center fits finite f32")
}

fn add_stored_f32(lhs: NativeF32Bits, rhs: NativeF32Bits) -> NativeF32Bits {
    let lhs = X87Chop53::load_f32(lhs).expect("stored candidate is finite");
    let rhs = X87Chop53::load_f32(rhs).expect("stored normalized step is finite");
    X87Chop53::store_f32(X87Chop53::add(lhs, rhs)).expect("candidate probe fits finite f32")
}

fn stored_f32_to_i16(value: NativeF32Bits) -> i16 {
    let value = X87Chop53::load_f32(value).expect("stored candidate is finite");
    X87Chop53::ftol_i64(value).expect("candidate probe fits a signed integer") as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(command_index: usize, entity_id: u64, cell: (i16, i16)) -> GroupDestinationMember {
        GroupDestinationMember {
            command_index,
            entity_id,
            coord: [
                i32::from(cell.0) * 256 + 128,
                i32::from(cell.1) * 256 + 128,
                0,
            ],
            source_cell: cell,
        }
    }

    const CLEAR: CandidateFacts = CandidateFacts {
        in_playfield: true,
        same_zone: true,
        height_band_ok: true,
        can_enter_code: 0,
    };

    #[test]
    fn large_group_keeps_centroid_and_distance_priority_past_the_native_stage() {
        let mut members = (0..20_000)
            .map(|index| GroupDestinationMember {
                command_index: index,
                entity_id: index as u64,
                coord: [115_329, 115_329, 0],
                source_cell: (450, 450),
            })
            .collect::<Vec<_>>();
        members.last_mut().unwrap().coord = [115_328, 115_328, 0];
        assert_eq!(centroid_3d(&members), [115_328, 115_328, 0]);
        let mut first_probe = None;
        let assigned = distribute_group_destinations((460, 460), &members, |member, _| {
            first_probe.get_or_insert(member.command_index);
            CLEAR
        });
        // The final member is the anchor; its index must not outweigh a
        // smaller distance, as a distance*1000+index key would allow.
        assert_eq!(first_probe, Some(0));
        assert_eq!(assigned.len(), members.len());
        assert!(assigned.iter().all(|entry| entry.destination == (460, 460)));
    }

    #[test]
    fn normal_group_uses_centroid_anchor_distance_order_and_directional_probes() {
        let members = [
            member(0, 10, (10, 10)),
            member(1, 11, (12, 10)),
            member(2, 12, (10, 12)),
        ];

        let assignments = distribute_group_destinations((20, 20), &members, |_, _| CLEAR);

        assert_eq!(
            assignments,
            vec![
                GroupDestinationAssignment {
                    command_index: 0,
                    destination: (20, 20),
                },
                GroupDestinationAssignment {
                    command_index: 1,
                    destination: (21, 20),
                },
                GroupDestinationAssignment {
                    command_index: 2,
                    destination: (20, 21),
                },
            ]
        );
    }

    #[test]
    fn congested_ray_keeps_fallback_after_an_earlier_bad_probe() {
        let members = [member(0, 10, (10, 10)), member(1, 11, (12, 10))];
        let mut probes = Vec::new();

        let assignments = distribute_group_destinations((20, 20), &members, |_, candidate| {
            probes.push(candidate);
            CandidateFacts {
                can_enter_code: if candidate == (19, 20) { 5 } else { 0 },
                ..CLEAR
            }
        });

        assert_eq!(probes, vec![(19, 20), (18, 20)]);
        assert_eq!(
            assignments,
            vec![
                GroupDestinationAssignment {
                    command_index: 0,
                    destination: (20, 20),
                },
                GroupDestinationAssignment {
                    command_index: 1,
                    destination: (20, 20),
                },
            ]
        );
    }

    #[test]
    fn playfield_boundary_aborts_the_ray_on_its_first_outside_probe() {
        let members = [member(0, 10, (10, 10)), member(1, 11, (12, 10))];
        let mut probes = Vec::new();

        let assignments = distribute_group_destinations((20, 20), &members, |_, candidate| {
            probes.push(candidate);
            CandidateFacts::outside_playfield()
        });

        assert_eq!(probes, vec![(19, 20)]);
        assert_eq!(assignments[0].destination, (20, 20));
        assert_eq!(assignments[1].destination, (20, 20));
    }

    #[test]
    fn later_members_from_the_same_source_cell_share_one_assignment() {
        let members = [
            member(0, 10, (10, 10)),
            member(1, 11, (14, 10)),
            member(2, 12, (14, 10)),
            member(3, 13, (6, 10)),
        ];
        let mut evaluated_ids = Vec::new();

        let assignments = distribute_group_destinations((20, 20), &members, |member, _| {
            evaluated_ids.push(member.entity_id);
            CLEAR
        });

        assert_eq!(assignments[1].destination, (21, 20));
        assert_eq!(assignments[2].destination, (21, 20));
        assert!(!evaluated_ids.contains(&12));
    }
}
