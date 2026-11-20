//! Native multiplayer start positions: the MapClass cell-array start bounds,
//! the Gather_Start_Positions vector (with its deficient-start search), and
//! the Battle, Cooperative and quick-coop callbacks that assign houses to it.

use super::normalized_launch_slots;
use crate::map::map_file::{MapFile, MapHeader};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::waypoints::Waypoint;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
#[cfg(any(test, feature = "test-support"))]
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::find_nearby_cell::{
    NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell, map_owned_radius_cap,
};
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::skirmish_launch::{LaunchStartPosition, SkirmishLaunchSession};
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct NativeStartBounds {
    pub min_rx: u16,
    pub min_ry: u16,
    pub width: u16,
    pub height: u16,
}

impl NativeStartBounds {
    /// Construct the post-Resize cell-array rectangle directly from parsed
    /// `[Map] Size`, before Fill has produced a resolved terrain grid.
    pub fn from_map_header(header: &MapHeader) -> Option<Self> {
        // Keep pre-Fill admission with the map owner's selected grid capacity.
        if !header.fits_cell_grid() {
            return None;
        }
        let extent = header.width.checked_add(header.height)?;
        let extent = u16::try_from(extent).ok()?;
        (extent > 1).then_some(Self {
            min_rx: 1,
            min_ry: 1,
            width: extent - 1,
            height: extent - 1,
        })
    }

    /// gamemd-derived: active YR start placement is bounded by the MapClass
    /// CELL-ARRAY rect, never by `LocalSize=`. `MapClass::Resize @ 0x00565C10` writes
    /// `MapClass+0x124..0x130` as `(1, 1, SizeW+SizeH-1, SizeW+SizeH-1)`, and
    /// both `ScenarioClass__Gather_Start_Positions @ 0x00688380` (deficient
    /// seed block `0x00688528..0x0068857C`) and
    /// `Try_Unlimbo_Object_At_Or_Near_Cell @ 0x00688ED0` (fallback probe clamp)
    /// read exactly those four fields. Playable-area
    /// acceptance is the separate isometric-diamond predicate
    /// (`cell_rect::cell_is_in_playfield_height_aware`); `LocalSize=` feeds that test's
    /// band constants only, never an axis-aligned bound. Treating LocalSize
    /// as a cell rectangle here previously rejected most authored starts and
    /// clamped every displaced MCV onto the false edge.
    pub fn from_session(sim: &Simulation, terrain: &ResolvedTerrainGrid) -> Self {
        // `session.map_width/height` hold the canonical cell-array extent
        // (~SizeW+SizeH), so the native rect side is that extent minus one.
        // Sessions without header data (headless fixtures) fall back to the
        // terrain grid extent — VERA-internal, same shape.
        let (extent_x, extent_y) = if sim.session.map_width() != 0 && sim.session.map_height() != 0
        {
            (sim.session.map_width(), sim.session.map_height())
        } else {
            (terrain.width(), terrain.height())
        };
        Self {
            min_rx: 1,
            min_ry: 1,
            width: extent_x.saturating_sub(1),
            height: extent_y.saturating_sub(1),
        }
    }

    /// The inclusive test native walks make against `+0x124..+0x130`:
    /// `left <= x <= left + width`, and the same for y. Both CABHUT tails
    /// walk inside it (repair `0x005738B1`, death `0x005744F7..0x00574531`).
    pub(crate) fn contains(self, at: (i16, i16)) -> bool {
        let (left, top) = (i32::from(self.min_rx), i32::from(self.min_ry));
        (left..=left + i32::from(self.width)).contains(&i32::from(at.0))
            && (top..=top + i32::from(self.height)).contains(&i32::from(at.1))
    }

    pub(super) fn max_rx(self) -> u16 {
        self.min_rx.saturating_add(self.width.saturating_sub(1))
    }

    pub(super) fn max_ry(self) -> u16 {
        self.min_ry.saturating_add(self.height.saturating_sub(1))
    }

    pub(crate) fn clamp(self, rx: i32, ry: i32) -> (u16, u16) {
        (
            rx.clamp(i32::from(self.min_rx), i32::from(self.max_rx())) as u16,
            ry.clamp(i32::from(self.min_ry), i32::from(self.max_ry())) as u16,
        )
    }
}

/// Build the native multiplayer-start vector before assigning houses.
///
/// Only waypoint indices below the computed target count are examined. When
/// authored starts are deficient, each retry consumes the two asymmetric
/// ranged draws and runs the 8x8 nearby-passable search; invalid searches do
/// not advance the vector and have no artificial retry cap.
#[cfg(any(test, feature = "test-support"))]
pub fn native_gather_start_positions(
    waypoints: &HashMap<u32, Waypoint>,
    participant_count: usize,
    terrain: &ResolvedTerrainGrid,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    bounds: NativeStartBounds,
    playfield_bounds: Option<PlayfieldBounds>,
    map_size_height: Option<i32>,
    binary_frame: u32,
    rng: &mut SimRng,
) -> Vec<Waypoint> {
    gather_start_positions_with_search(
        waypoints,
        participant_count,
        8,
        bounds,
        rng,
        |seed_rx, seed_ry| {
            find_nearby_start_rect(
                terrain,
                occupancy,
                playfield_bounds,
                map_size_height,
                binary_frame,
                seed_rx,
                seed_ry,
            )
        },
    )
}

/// Run Gather in the exact pre-Fill cell lifetime: MapClass has resized and
/// normalized Size/LocalSize, but every CellClass still has constructor
/// defaults and no Iso/overlay/Terrain/Techno/occupation authority exists.
pub(crate) fn native_gather_pre_fill_start_positions(
    waypoints: &HashMap<u32, Waypoint>,
    required_start_count: usize,
    map: &MapFile,
    rng: &mut SimRng,
) -> Option<Vec<Waypoint>> {
    let bounds = NativeStartBounds::from_map_header(&map.header)?;
    // VERA's explicitly declared 9–30-start maps use the same map-capacity
    // reader as setup and retained loading metadata. Participant count alone
    // must not discard a high authored start chosen by a small roster.
    let authored_slot_limit = crate::map::waypoints::multiplayer_start_slot_limit(
        crate::map::waypoints::skirmish_player_capacity(&map.ini),
    );
    Some(gather_start_positions_with_search(
        waypoints,
        required_start_count,
        authored_slot_limit,
        bounds,
        rng,
        |seed_rx, seed_ry| find_nearby_pre_fill_start_rect(&map.header, seed_rx, seed_ry),
    ))
}

fn gather_start_positions_with_search(
    waypoints: &HashMap<u32, Waypoint>,
    participant_count: usize,
    authored_slot_limit: u32,
    bounds: NativeStartBounds,
    rng: &mut SimRng,
    mut find_nearby: impl FnMut(u16, u16) -> Option<(u16, u16)>,
) -> Vec<Waypoint> {
    let authored_prefix = (0..authored_slot_limit)
        .take_while(|index| waypoints.contains_key(index))
        .count();
    let target_count = authored_prefix.max(participant_count);
    let mut starts = Vec::with_capacity(target_count);

    for index in 0..target_count as u32 {
        if let Some(waypoint) = waypoints.get(&index) {
            starts.push(*waypoint);
        }
    }

    while starts.len() < target_count {
        // gamemd-derived: active YR `ScenarioClass__Gather_Start_Positions
        // @ 0x00688380`, seed block `0x00688528..0x0068857C`: the first draw
        // `RandomRanged(0, rect.h - 10)` lands on the Y axis
        // (`+ 10 + rect.y`), the second `RandomRanged(10, rect.w - 10)` on the
        // X axis (`+ rect.x`). The rect is the MapClass cell-array rect (see
        // `NativeStartBounds::from_session`), which is square, so a transposed
        // mapping is RNG-neutral — but the seeded POINT mirrors across the
        // diagonal, so the axes are kept exactly as native writes them.
        let y_span = u32::from(bounds.height.saturating_sub(10));
        let x_high = u32::from(bounds.width.saturating_sub(10));
        let seed_ry = rng
            .next_range_u32_inclusive(0, y_span)
            .wrapping_add(10)
            .wrapping_add(u32::from(bounds.min_ry)) as u16;
        let seed_rx = rng
            .next_range_u32_inclusive(10, x_high)
            .wrapping_add(u32::from(bounds.min_rx)) as u16;

        let Some((rx, ry)) = find_nearby(seed_rx, seed_ry) else {
            continue;
        };
        starts.push(Waypoint {
            index: starts.len() as u32,
            rx,
            ry,
        });
    }

    starts
}

fn find_nearby_pre_fill_start_rect(
    header: &MapHeader,
    seed_rx: u16,
    seed_ry: u16,
) -> Option<(u16, u16)> {
    let playfield_bounds = crate::map::playfield::PlayfieldBounds::from_map_header(header);
    let query = NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            speed_type: SpeedType::Track,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::new(
            i32::from(DEFICIENT_START_RECT_W),
            i32::from(DEFICIENT_START_RECT_H),
        ),
        allow_bridge_cells: true,
        check_height: false,
        require_burrowable: false,
        burrowing_contents: None,
        check_occupancy: false,
        radius_cap: map_owned_radius_cap(playfield_bounds.base, header.height as i32),
        target_cell: None,
        path_grid: None,
        resolved_terrain: None,
        overlay_grid: None,
        occupancy: None,
        entities: None,
        zone_grid: None,
        playfield_bounds: Some(playfield_bounds),
    };
    find_nearby_passable_cell((i32::from(seed_rx), i32::from(seed_ry)), &query, 0)
}

/// Exact deficient-start adapter over the shared MapClass FNPC mechanism.
///
/// The caller's `8,8` are top-left CellRect dimensions, not a search radius.
/// Radius comes from the separately retained MapClass Size pair, and a missing
/// Size/playfield authority rejects the query instead of approximating it.
// gamemd-derived: `ScenarioClass::Gather_Start_Positions @ 0x00688380`, call
// `0x006885B5`, passes `(Track, -1, Normal, bridge-aware=0, 8x8,
// reject-overlay=0, height=0, obstacle=0, allow-bridge=1, null-reference,
// param15=0, final-occupancy=0)` to
// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20`.
#[cfg(any(test, feature = "test-support"))]
pub fn find_nearby_start_rect(
    terrain: &ResolvedTerrainGrid,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    playfield_bounds: Option<PlayfieldBounds>,
    map_size_height: Option<i32>,
    binary_frame: u32,
    seed_rx: u16,
    seed_ry: u16,
) -> Option<(u16, u16)> {
    let bounds = playfield_bounds?;
    let size_height = map_size_height?;
    let query = NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            speed_type: SpeedType::Track,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::new(
            i32::from(DEFICIENT_START_RECT_W),
            i32::from(DEFICIENT_START_RECT_H),
        ),
        allow_bridge_cells: true,
        check_height: false,
        require_burrowable: false,
        burrowing_contents: None,
        check_occupancy: false,
        radius_cap: map_owned_radius_cap(bounds.base, size_height),
        target_cell: None,
        path_grid: None,
        resolved_terrain: Some(terrain),
        overlay_grid: None,
        // CellRect passability reads the native occupation plane even though the
        // caller disables the later, distinct CheckOccupancy rectangle gate.
        occupancy: Some(occupancy),
        entities: None,
        zone_grid: None,
        playfield_bounds: Some(bounds),
    };
    find_nearby_passable_cell(
        (i32::from(seed_rx), i32::from(seed_ry)),
        &query,
        binary_frame,
    )
}

/// Retain the native start-table ownership and resolved placement for one
/// stock-offline assignment callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeStartAssignment {
    pub placements: Vec<(usize, Waypoint)>,
    pub start_table: Vec<Option<usize>>,
}

#[cfg(any(test, feature = "test-support"))]
pub fn native_assign_launch_starts(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    rng: &mut SimRng,
) -> NativeStartAssignment {
    let preassignment = native_preassign_launch_start_table(session, starts.len());
    native_assign_launch_starts_from_preassignment(session, starts, &preassignment, rng)
}

fn launch_start_requests(session: &SkirmishLaunchSession) -> Vec<LaunchStartPosition> {
    normalized_launch_slots(session)
        .iter()
        .map(|slot| slot.start_position)
        .collect()
}

pub(crate) fn native_preassign_launch_start_table(
    session: &SkirmishLaunchSession,
    start_count: usize,
) -> Vec<Option<usize>> {
    let requested = launch_start_requests(session);
    let mut explicit_owner = vec![None; start_count];
    for (slot, request) in requested.iter().enumerate() {
        let LaunchStartPosition::Position(index) = request else {
            continue;
        };
        if let Some(owner) = explicit_owner.get_mut(usize::from(*index)) {
            *owner = Some(slot);
        }
    }
    explicit_owner
}

pub(super) fn native_assign_launch_starts_from_preassignment(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    preassignment: &[Option<usize>],
    rng: &mut SimRng,
) -> NativeStartAssignment {
    if starts.is_empty() {
        return NativeStartAssignment {
            placements: Vec::new(),
            start_table: Vec::new(),
        };
    }

    assert_eq!(starts.len(), preassignment.len());
    let requested = launch_start_requests(session);
    let mut explicit_owner = preassignment.to_vec();

    // Battle +0x84 builds its occupied-byte array from the complete
    // Scenario+0x1180 table before its HouseClass pass begins. The selected
    // mode's +0x80 callback populated that table in HouseClass order, so
    // duplicate explicit starts have already resolved last-writer-wins.
    let mut occupied: Vec<bool> = explicit_owner.iter().map(Option::is_some).collect();
    let mut assigned = vec![None; requested.len()];

    let slots = normalized_launch_slots(session);
    let shares_bases = session.shares_bases();
    let mut observer_reserved_first = false;
    let mut observer_claim_complete = false;
    // Battle5D6C70 visits House order; chooser5D6890 shares an observer's
    // viewpoint without reserving another start. ABFDD4/5 distinguish a first
    // observer reservation from an observer joining an already occupied view.
    for (slot, assigned_start) in assigned.iter_mut().enumerate() {
        let explicit = explicit_owner
            .iter()
            .rposition(|owner| *owner == Some(slot));
        // VERA: where players share bases, a House whose start a later House
        // also asked for starts there too, beside it, its units on the cells
        // nearby. The table keeps the last owner, as gamemd's does.
        let beside = match requested[slot] {
            LaunchStartPosition::Position(start)
                if shares_bases
                    && explicit.is_none()
                    && !slots[slot].observer
                    && usize::from(start) < starts.len() =>
            {
                Some(usize::from(start))
            }
            _ => None,
        };
        let first_occupied = explicit_owner.iter().position(Option::is_some);
        let mut write_owner = true;
        let start_index = if let Some(explicit) = explicit {
            explicit
        } else if let Some(start) = beside {
            write_owner = false;
            start
        } else if let Some(first) = first_occupied {
            if observer_reserved_first && !observer_claim_complete && slots[slot].observer {
                // VERA: where only AI play, every observer comes before
                // them; a later one shares the first one's view and leaves
                // its reserved start to the next House, which plays.
                write_owner = false;
                first
            } else if observer_reserved_first && !observer_claim_complete {
                observer_claim_complete = true;
                first
            } else if slots[slot].observer {
                observer_reserved_first = true;
                observer_claim_complete = true;
                write_owner = false;
                first
            } else {
                choose_battle_automatic_start(starts, &occupied, rng)
            }
        } else {
            observer_reserved_first = slots[slot].observer;
            observer_claim_complete = false;
            choose_battle_automatic_start(starts, &occupied, rng)
        };
        if write_owner {
            occupied[start_index] = true;
            explicit_owner[start_index] = Some(slot);
        }
        *assigned_start = Some(starts[start_index]);
    }

    NativeStartAssignment {
        placements: assigned
            .into_iter()
            .enumerate()
            .filter_map(|(slot, start)| start.map(|start| (slot, start)))
            .collect(),
        start_table: explicit_owner,
    }
}

/// Cooperative's custom start callback partitions authored positions into a
/// human prefix and an AI suffix. The explicit Scenario start table reserves
/// all of its entries before HouseClass iteration; automatic human placement
/// draws within the prefix and probes forward, while the remaining houses
/// take the first free suffix entry without a draw.
#[cfg(any(test, feature = "test-support"))]
pub fn native_assign_cooperative_launch_starts(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    human_start_spots: usize,
    rng: &mut SimRng,
) -> NativeStartAssignment {
    let preassignment = native_preassign_launch_start_table(session, starts.len());
    native_assign_cooperative_starts_from_preassignment(
        session,
        starts,
        &preassignment,
        human_start_spots,
        session
            .humans
            .iter()
            .filter(|human| !human.is_observer())
            .count(),
        rng,
    )
}

pub(super) fn native_assign_cooperative_starts_from_preassignment(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    preassignment: &[Option<usize>],
    human_start_spots: usize,
    human_house_count: usize,
    rng: &mut SimRng,
) -> NativeStartAssignment {
    if starts.is_empty() {
        return NativeStartAssignment {
            placements: Vec::new(),
            start_table: Vec::new(),
        };
    }

    assert_eq!(starts.len(), preassignment.len());
    let requested = launch_start_requests(session);
    let mut explicit_owner = preassignment.to_vec();

    let mut occupied: Vec<bool> = explicit_owner.iter().map(Option::is_some).collect();
    let human_start_spots = human_start_spots.min(starts.len());
    let mut assigned = vec![None; requested.len()];

    for (slot, assigned_start) in assigned.iter_mut().enumerate() {
        let explicit = explicit_owner.iter().position(|owner| *owner == Some(slot));
        let start_index = if let Some(explicit) = explicit {
            explicit
        } else {
            let occupied_count = occupied.iter().filter(|used| **used).count();
            if occupied_count < human_house_count && human_start_spots != 0 {
                let mut candidate =
                    rng.next_range_u32_inclusive(0, human_start_spots as u32 - 1) as usize;
                while occupied[candidate] {
                    candidate = (candidate + 1) % human_start_spots;
                }
                candidate
            } else {
                (human_start_spots..starts.len())
                    .find(|candidate| !occupied[*candidate])
                    .or_else(|| occupied.iter().position(|used| !*used))
                    .expect("participant count cannot exceed gathered starts")
            }
        };
        occupied[start_index] = true;
        explicit_owner[start_index] = Some(slot);
        *assigned_start = Some(starts[start_index]);
    }

    NativeStartAssignment {
        placements: assigned
            .into_iter()
            .enumerate()
            .filter_map(|(slot, start)| start.map(|start| (slot, start)))
            .collect(),
        start_table: explicit_owner,
    }
}

/// WOL Quick Co-op's start callback, `ScenarioClass__AssignStartingPoints`
/// (`0x005EE9D0..0x005EEBAA`). It snapshots `used` from the Scenario start
/// table once, then makes two passes over the Houses in order: humans
/// (`+0x1EC`) keep the last table entry naming them or pick with the human
/// flag; then every AI picks with no table check, so a preset AI start stays
/// reserved but unused. The `[Basic] Official` argument only reaches Gather,
/// which never reads it. It writes no node or AI-slot start back.
pub(super) fn native_assign_quick_coop_starts_from_preassignment(
    session: &SkirmishLaunchSession,
    starts: &[Waypoint],
    preassignment: &[Option<usize>],
    rng: &mut SimRng,
) -> NativeStartAssignment {
    if starts.is_empty() {
        return NativeStartAssignment {
            placements: Vec::new(),
            start_table: Vec::new(),
        };
    }

    assert_eq!(starts.len(), preassignment.len());
    let slots = normalized_launch_slots(session);
    let mut start_table = preassignment.to_vec();
    let mut used: Vec<bool> = start_table.iter().map(Option::is_some).collect();
    let mut assigned = vec![None; slots.len()];

    for human in [true, false] {
        for (slot, normalized) in slots.iter().enumerate() {
            if normalized.is_human != human {
                continue;
            }
            let preset = if human {
                start_table.iter().rposition(|owner| *owner == Some(slot))
            } else {
                None
            };
            let start_index = preset.unwrap_or_else(|| {
                let picked = choose_quick_coop_start(starts, &used, human, rng);
                used[picked] = true;
                start_table[picked] = Some(slot);
                picked
            });
            assigned[slot] = Some(starts[start_index]);
        }
    }

    NativeStartAssignment {
        placements: assigned
            .into_iter()
            .enumerate()
            .filter_map(|(slot, start)| start.map(|start| (slot, start)))
            .collect(),
        start_table,
    }
}

/// AssignStartingPoints' picker `0x005EE6F0`, with `k` the used starts. No
/// start used: a human draws `RandomRanged(0, N-1)` on the Scenario RNG; an
/// AI takes the farthest rule, which is the first start. Two used and an AI:
/// draw `r = RandomRanged(0, N-3)` and take the `(r+1)`-th free start in
/// index order. One used: the free start nearest the used one. Otherwise the
/// free start farthest from the used ones; ties keep the lowest index.
pub(super) fn choose_quick_coop_start(
    starts: &[Waypoint],
    used: &[bool],
    human: bool,
    rng: &mut SimRng,
) -> usize {
    let used_count = used.iter().filter(|used| **used).count();
    if used_count == 0 && human {
        return rng.next_range_u32_inclusive(0, starts.len() as u32 - 1) as usize;
    }
    if used_count == 2 && !human {
        let r = rng.next_range_u32_inclusive(0, starts.len() as u32 - 3) as usize;
        if let Some(free) = (0..starts.len()).filter(|index| !used[*index]).nth(r) {
            return free;
        }
    }
    // Each distance is truncated to 16 bits before the 32-bit sum.
    let distance_sum = |candidate: usize| -> i32 {
        (0..starts.len())
            .filter(|other| used[*other])
            .map(|other| i32::from(native_start_distance(starts[candidate], starts[other]) as i16))
            .sum()
    };
    let free = (0..starts.len()).filter(|index| !used[*index]);
    let picked = if used_count == 1 {
        // Strict `<`, so the first minimum wins.
        free.fold(None::<(usize, i32)>, |best, candidate| {
            let sum = distance_sum(candidate);
            match best {
                Some((_, best_sum)) if sum >= best_sum => best,
                _ => Some((candidate, sum)),
            }
        })
    } else {
        // Strict `>` from -1, so ties keep the lowest index.
        free.fold(None::<(usize, i32)>, |best, candidate| {
            let sum = distance_sum(candidate);
            match best {
                Some((_, best_sum)) if sum <= best_sum => best,
                _ => Some((candidate, sum)),
            }
        })
    };
    picked
        .expect("participant count cannot exceed gathered starts")
        .0
}

pub fn choose_battle_automatic_start(
    starts: &[Waypoint],
    occupied: &[bool],
    rng: &mut SimRng,
) -> usize {
    let used_count = occupied.iter().filter(|used| **used).count();
    if used_count == 0 {
        return rng.next_range_u32_inclusive(0, starts.len() as u32 - 1) as usize;
    }

    let free: Vec<usize> = occupied
        .iter()
        .enumerate()
        .filter_map(|(index, used)| (!*used).then_some(index))
        .collect();
    let distance_sum = |candidate: usize| -> i32 {
        occupied
            .iter()
            .enumerate()
            .filter(|(_, used)| **used)
            .map(|(other, _)| native_start_distance(starts[candidate], starts[other]))
            .sum()
    };
    let mut iter = free.into_iter();
    let mut selected = iter
        .next()
        .expect("participant count cannot exceed gathered starts");
    let mut selected_sum = distance_sum(selected);
    for candidate in iter {
        let sum = distance_sum(candidate);
        if sum > selected_sum {
            selected = candidate;
            selected_sum = sum;
        }
    }
    selected
}

pub fn native_start_distance(left: Waypoint, right: Waypoint) -> i32 {
    let dx = i32::from((left.rx as i16).wrapping_sub(right.rx as i16));
    let dy = i32::from((left.ry as i16).wrapping_sub(right.ry as i16));
    let squared_distance = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    let root = sqrt_approx_f32(X87Chop53::load_i32(squared_distance))
        .expect("signed squared start distance stays in the verified finite x87 domain");
    f32::from_bits(root.bits()) as i32
}

const DEFICIENT_START_RECT_W: u16 = 8;
const DEFICIENT_START_RECT_H: u16 = 8;
