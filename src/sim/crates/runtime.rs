//! Live crate slot clear, identity-specific overlay removal, and the per-tick
//! `CrateRegen` scan.
//!
//! `MapClass__UpdateCrateRegenTimers @ 0x0056BBE0` maintains the 256 persistent
//! crate slots alongside pickup removal at `0x0056C020`. `LogicClass__PerTickUpdate
//! @ 0x0055AFB0` calls it once per tick at `0x0055B65A`, after
//! `AlphaShapeClass::PurgeDisabled` and before the Tactical, Factory and House
//! callbacks, and it does nothing at all unless the game mode is nonzero and the
//! lobby Crates option byte is set.
//!
//! One ascending pass over all 256 slots expires every slot whose timer is up.
//! Each expiry runs `CrateSlot__ClearAndPreserveTimer @ 0x004A1750` — which
//! removes the crate overlay from the cell, frees the coordinate and rebases the
//! remaining duration — and then exactly one
//! `MapClass__PlaceCrateAtRandomCell @ 0x0056BD40`, the same placer scenario
//! start uses. Because the clear always frees the slot the scan is standing on,
//! the placer's lowest-free scan can only reinstall at an index at or below it:
//! a replacement is never revisited later in the same pass, and there is no
//! cascade. Several slots can still expire in one pass, each spending its own
//! placement draws in ascending slot order.
//!
//! ## Dependency rules
//! Part of `sim/` — depends on `rules/`, `map/` grid types and other `sim/`
//! modules only. Never on render/, ui/, sidebar/, audio/, net/.

use crate::map::lighting::LightingProfileUnits;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::crate_rules::CrateRules;
use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

use super::state::{CRATE_SLOT_CAPACITY, CrateSlot};
use super::{
    CrateMarkCellRef, ForcedPostPrecheckFailure, OneCrateResult, place_one_random_crate,
    resolve_crate_mark_cell,
};

/// Outcome of one `MapClass__UpdateCrateRegenTimers` pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CrateRegeneration {
    /// Slots whose timer expired and were cleared this pass.
    pub expired: u32,
    /// Replacement placer calls that took a slot, including timed ghosts.
    pub accepted: u32,
    /// Replacement calls that installed a visible overlay.
    pub visible: u32,
}

/// `CrateSlot__RemoveCrateOverlayFromCell @ 0x004A1AA0`.
///
/// Native order is `MapClass::In_Bounds @ 0x00568300`,
/// `MapClass::Get_CellClass @ 0x005657A0`, the `CellClass+0x44` identity test at
/// `0x004A1ACD..0x004A1AFE`, one screen-dirty request, and only then the two
/// field writes. The identity test is exact pointer equality against the three
/// live `RulesClass` crate images at `Rules+0xF8`, `+0xFC` and `+0x100` — an
/// unrelated overlay, or a crate overlay after the images were re-read to other
/// names, is left alone and the caller still frees the slot.
///
/// The rectangle union and `TacticalClass::DirtyScreenRect(..., force = 0)` at
/// `0x004A1B04..0x004A1BDC` are screen-space presentation; the sim-visible
/// effect is exactly `CellClass+0x44 = -1` and `CellClass+0x11E = 0`, published
/// through the OverlayGrid dirty receipt.
pub(crate) fn remove_crate_overlay_from_cell(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    cell: (i16, i16),
) -> bool {
    if !sim.map_cell_in_bounds(cell) {
        return false;
    }
    let cell_ref = resolve_crate_mark_cell(sim, cell);
    let overlay_id = match &cell_ref {
        CrateMarkCellRef::Real(rx, ry) => sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.cell(*rx, *ry).overlay_id),
        CrateMarkCellRef::Dummy(dummy) => dummy.overlay_fields().0,
    };
    let Some(overlay_id) = overlay_id else {
        return false;
    };
    if !is_live_crate_image(&rules.crate_rules, registry, overlay_id) {
        return false;
    }
    clear_crate_overlay_fields(sim, cell_ref)
}

fn clear_crate_overlay_fields(sim: &mut Simulation, cell_ref: CrateMarkCellRef) -> bool {
    match cell_ref {
        CrateMarkCellRef::Real(rx, ry) => {
            let (Some(grid), Some(terrain)) =
                (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
            else {
                return false;
            };
            grid.clear_crate_mark_fields(terrain, rx, ry)
        }
        CrateMarkCellRef::Dummy(dummy) => {
            dummy.set_overlay_fields(None, 0);
            true
        }
    }
}

/// `MapClass` pickup removal at `0x0056C020`, called by Cell481A00 before
/// replacement generation and the selected effect. Native comparisons:
/// `tools/spatial_oracle/crate_pickup.json` (removal-reaching rows).
///
/// Multiplayer clears only the first occupied slot with this coordinate. A
/// missing slot leaves even a visible crate untouched. Solo instead looks up
/// the cell directly, without the In_Bounds diamond test, and accepts any
/// OverlayType whose Crate flag is set; it never modifies runtime slots.
pub(super) fn remove_pickup_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    cell: (i16, i16),
) -> bool {
    if sim.session.game_mode_nonzero {
        let Some(index) = sim
            .crate_authority
            .slots()
            .iter()
            .position(|slot| !slot.is_empty() && (slot.cell_x, slot.cell_y) == cell)
        else {
            return false;
        };
        return clear_crate_slot(sim, rules, registry, index, sim.session.binary_frame as i32);
    }
    let cell_ref = resolve_crate_mark_cell(sim, cell);
    let overlay_id = match &cell_ref {
        CrateMarkCellRef::Real(rx, ry) => sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.cell(*rx, *ry).overlay_id),
        CrateMarkCellRef::Dummy(dummy) => dummy.overlay_fields().0,
    };
    if !overlay_id
        .and_then(|id| registry.flags(id))
        .is_some_and(|flags| flags.crate_type)
    {
        return false;
    }
    clear_crate_overlay_fields(sim, cell_ref)
}

/// Exact `Rules+0xF8`/`+0xFC`/`+0x100` pointer identity, expressed over the
/// OverlayType ids those three `RulesClass` image pointers resolve to. A rules
/// image that names no live OverlayType is a null pointer natively and can
/// never match a live cell identity.
fn is_live_crate_image(rules: &CrateRules, registry: &OverlayTypeRegistry, overlay_id: u8) -> bool {
    [
        rules.wood_crate_img.as_deref(),
        rules.crate_img.as_deref(),
        rules.water_crate_img.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|name| registry.id_for_name(name))
    .any(|id| id == overlay_id)
}

/// `CrateSlot__ClearAndPreserveTimer @ 0x004A1750`.
///
/// An already-empty coordinate returns false with no mutation at all — no
/// removal attempt and no timer touch (`0x004A1754..0x004A176F`). Otherwise the
/// overlay removal runs first, its result is discarded, and the coordinate is
/// freed unconditionally. The timer is then rebased only when a start frame is
/// live: an unexpired timer keeps `duration - elapsed`, an already-expired one
/// is zeroed at `0x004A17AB`, and a slot that is already paused at `start == -1`
/// keeps its stored duration untouched.
pub(crate) fn clear_crate_slot(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    slot_index: usize,
    current_frame: i32,
) -> bool {
    let slot = sim.crate_authority.slots()[slot_index];
    if slot.is_empty() {
        return false;
    }
    let _ = remove_crate_overlay_from_cell(sim, rules, registry, (slot.cell_x, slot.cell_y));
    let slot = sim.crate_authority.slot_mut(slot_index);
    slot.cell_x = 0;
    slot.cell_y = 0;
    if slot.start_frame != -1 {
        let elapsed = current_frame.wrapping_sub(slot.start_frame);
        slot.duration = if elapsed < slot.duration {
            slot.duration.wrapping_sub(elapsed)
        } else {
            0
        };
        slot.start_frame = -1;
    }
    true
}

/// `MapClass__UpdateCrateRegenTimers @ 0x0056BBE0`.
///
/// The two gate reads at `0x0056BBE0` and `0x0056BBEC` are the live game mode
/// and the lobby Crates option; either one clear makes the whole call a no-op,
/// so paused crates never regenerate and never spend a draw. The scan itself is
/// a fixed ascending walk of the 256 slots on the pre-increment frame counter,
/// the same one the accepted-placement timer stored as its start.
pub(crate) fn tick_crate_regeneration(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    path_grid: Option<&PathGrid>,
    lighting_profile: LightingProfileUnits,
) -> CrateRegeneration {
    let mut result = CrateRegeneration::default();
    if !sim.session.game_mode_nonzero || !sim.session.game_options.crates {
        return result;
    }
    let current_frame = sim.session.binary_frame as i32;
    for index in 0..CRATE_SLOT_CAPACITY {
        // Reload the live slot: an earlier expiry in this same pass may have
        // reinstalled a crate at or below this index.
        let slot = sim.crate_authority.slots()[index];
        if slot.is_empty() || !crate_slot_timer_expired(slot, current_frame) {
            continue;
        }
        result.expired = result.expired.wrapping_add(1);
        clear_crate_slot(sim, rules, overlay_registry, index, current_frame);
        match place_one_random_crate(
            sim,
            rules,
            overlay_registry,
            path_grid,
            lighting_profile,
            ForcedPostPrecheckFailure::None,
        ) {
            OneCrateResult::HardRejected => {}
            OneCrateResult::AcceptedGhost => result.accepted = result.accepted.wrapping_add(1),
            OneCrateResult::AcceptedVisible => {
                result.accepted = result.accepted.wrapping_add(1);
                result.visible = result.visible.wrapping_add(1);
            }
        }
    }
    result
}

/// The expiry predicate at `0x0056BC1C..0x0056BC35`.
///
/// A paused slot (`start == -1`) expires only once its stored duration has
/// already reached zero — and then it re-fires on every following tick until a
/// replacement takes the slot. A running slot expires as soon as the signed
/// elapsed frame count reaches its duration.
///
/// Native reaches the shared `TEST ECX,ECX` at `0x0056BC33` from both branches,
/// but for a running slot it is dead: `remaining == 0` would need
/// `duration == elapsed`, which the preceding `JGE` already took. The shared
/// tail is reproduced as one expression because it is the paused branch's whole
/// test, not because the running branch can reach it.
fn crate_slot_timer_expired(slot: CrateSlot, current_frame: i32) -> bool {
    let mut remaining = slot.duration;
    if slot.start_frame != -1 {
        let elapsed = current_frame.wrapping_sub(slot.start_frame);
        if elapsed >= remaining {
            return true;
        }
        remaining = remaining.wrapping_sub(elapsed);
    }
    remaining == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::map::lighting::ParsedLightingProfiles;
    use crate::sim::crates::state::CrateSlot;

    use super::super::tests::{crate_cells, crate_registry, crate_ruleset, sim_with_grid};

    fn lighting() -> LightingProfileUnits {
        ParsedLightingProfiles::default().normal
    }

    #[test]
    fn pickup_removal_matches_original_cell_pickup_outputs() {
        // Compare only rows that reach Map56C020; guard/selection/effect
        // coverage belongs to the complete pickup dispatcher, not this owner.
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/crate_pickup.json"
        ))
        .unwrap();
        let mut compared = 0;
        for row in rows {
            if !row["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e == "0056c020")
            {
                continue;
            }
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let cell = input["cell"].as_array().map_or((10, 10), |v| {
                (v[0].as_i64().unwrap() as i16, v[1].as_i64().unwrap() as i16)
            });
            let mut sim = sim_with_grid(31);
            sim.session.game_mode_nonzero = input["mode"].as_i64() != Some(0);
            sim.session.binary_frame = 100;
            sim.playfield_bounds.as_mut().unwrap().base = 10;
            sim.playfield_size_height = Some(10);
            let mut rules = crate_ruleset("");
            if input["different_image"] == true {
                rules.crate_rules.wood_crate_img = Some("SILVER".into());
            }
            let registry = crate_registry();
            let wood = registry.id_for_name("WOOD").unwrap();
            sim.overlay_grid.as_mut().unwrap().place_overlay(
                cell.0 as u16,
                cell.1 as u16,
                wood,
                input["selection"].as_u64().unwrap_or(10) as u8,
            );
            // The native fixture explicitly zeroes the slot memory.
            for (index, start, aux, duration) in [(0, 50, 777, 100), (1, 60, 888, 200)] {
                let occupied = if index == 0 {
                    input["registered"] != false
                } else {
                    input["duplicate_slot"] == true
                };
                *sim.crate_authority.slot_mut(index) = if occupied {
                    CrateSlot {
                        start_frame: start,
                        aux,
                        duration,
                        cell_x: cell.0,
                        cell_y: cell.1,
                    }
                } else {
                    CrateSlot {
                        start_frame: 0,
                        ..CrateSlot::default()
                    }
                };
            }
            let removed = remove_pickup_crate(&mut sim, &rules, &registry, cell);
            assert_eq!(
                serde_json::json!([u8::from(removed)]),
                row["removal_returns"],
                "{name}"
            );
            let actual = sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(cell.0 as u16, cell.1 as u16);
            assert_eq!(
                actual.overlay_id,
                (row["overlay"] != -1).then_some(wood),
                "{name}"
            );
            assert_eq!(
                i64::from(actual.overlay_data),
                row["selection"].as_i64().unwrap(),
                "{name}"
            );
            for (index, key) in [(0, "slot"), (1, "second_slot")] {
                let slot = sim.crate_authority.slots()[index];
                assert_eq!(
                    serde_json::json!([
                        slot.start_frame,
                        slot.aux,
                        slot.duration,
                        slot.cell_x,
                        slot.cell_y
                    ]),
                    row[key],
                    "{name} {key}"
                );
            }
            let dirty = sim
                .overlay_grid
                .as_mut()
                .unwrap()
                .take_removed_render_cells();
            assert_eq!(
                dirty.len(),
                row["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| *e == "dirty_screen")
                    .count(),
                "{name}"
            );
            compared += 1;
        }
        assert_eq!(compared, 24);
    }

    /// The three timer shapes the native predicate distinguishes, including the
    /// paused-at-zero slot that re-fires every tick.
    #[test]
    fn crate_regen_expiry_predicate_matches_native_branches() {
        let running = |start: i32, duration: i32| CrateSlot {
            start_frame: start,
            aux: 0,
            duration,
            cell_x: 3,
            cell_y: 4,
        };
        // Running: expires exactly when elapsed reaches duration.
        assert!(!crate_slot_timer_expired(running(100, 50), 149));
        assert!(crate_slot_timer_expired(running(100, 50), 150));
        assert!(crate_slot_timer_expired(running(100, 50), 151));
        // Zero-duration running slot expires on its own placement frame.
        assert!(crate_slot_timer_expired(running(100, 0), 100));
        // Paused: only a zero remainder expires, and it keeps expiring.
        assert!(!crate_slot_timer_expired(running(-1, 1), i32::MAX));
        assert!(crate_slot_timer_expired(running(-1, 0), 0));
        assert!(crate_slot_timer_expired(running(-1, 0), i32::MIN));
        // Negative duration is already past due for a running slot — this is
        // the assertion that pins the native `JGE` over an unsigned compare.
        assert!(crate_slot_timer_expired(running(100, -5), 100));
        // Elapsed is computed with wrapping subtraction across the signed
        // boundary, and the comparison stays signed.
        assert!(crate_slot_timer_expired(
            running(i32::MAX, 4),
            i32::MIN.wrapping_add(3)
        ));
        assert!(!crate_slot_timer_expired(
            running(i32::MAX, 6),
            i32::MIN.wrapping_add(3)
        ));
    }

    /// Clear frees the coordinate, rebases a live timer, and pauses the slot.
    #[test]
    fn crate_slot_clear_rebases_live_timer_and_pauses() {
        let mut sim = sim_with_grid(0x51);
        let rules = crate_ruleset("");
        let registry = crate_registry();
        *sim.crate_authority.slot_mut(2) = CrateSlot {
            start_frame: 100,
            aux: 0x1234,
            duration: 500,
            cell_x: 9,
            cell_y: 11,
        };

        assert!(clear_crate_slot(&mut sim, &rules, &registry, 2, 380));

        let slot = sim.crate_authority.slots()[2];
        assert_eq!(slot.cell_x, 0);
        assert_eq!(slot.cell_y, 0);
        assert_eq!(slot.start_frame, -1);
        assert_eq!(slot.duration, 220, "500 - (380 - 100) remaining frames");
        assert_eq!(slot.aux, 0x1234, "the auxiliary word is never rewritten");
    }

    /// An expired timer is zeroed, not carried negative, and an already-paused
    /// slot keeps its stored duration.
    #[test]
    fn crate_slot_clear_zeroes_expired_and_preserves_paused_duration() {
        let mut sim = sim_with_grid(0x52);
        let rules = crate_ruleset("");
        let registry = crate_registry();

        *sim.crate_authority.slot_mut(0) = CrateSlot {
            start_frame: 10,
            aux: 0,
            duration: 30,
            cell_x: 4,
            cell_y: 4,
        };
        assert!(clear_crate_slot(&mut sim, &rules, &registry, 0, 900));
        assert_eq!(sim.crate_authority.slots()[0].duration, 0);
        assert_eq!(sim.crate_authority.slots()[0].start_frame, -1);

        *sim.crate_authority.slot_mut(1) = CrateSlot {
            start_frame: -1,
            aux: 0,
            duration: 77,
            cell_x: 5,
            cell_y: 5,
        };
        assert!(clear_crate_slot(&mut sim, &rules, &registry, 1, 900));
        assert_eq!(
            sim.crate_authority.slots()[1].duration,
            77,
            "a paused slot keeps its remaining frames verbatim"
        );
        assert_eq!(sim.crate_authority.slots()[1].start_frame, -1);
    }

    /// An empty coordinate returns false and mutates nothing at all.
    #[test]
    fn crate_slot_clear_on_empty_slot_is_a_total_no_op() {
        let mut sim = sim_with_grid(0x53);
        let rules = crate_ruleset("");
        let registry = crate_registry();
        *sim.crate_authority.slot_mut(7) = CrateSlot {
            start_frame: 42,
            aux: 8,
            duration: 9,
            cell_x: 0,
            cell_y: 0,
        };

        assert!(!clear_crate_slot(&mut sim, &rules, &registry, 7, 1000));

        assert_eq!(
            sim.crate_authority.slots()[7],
            CrateSlot {
                start_frame: 42,
                aux: 8,
                duration: 9,
                cell_x: 0,
                cell_y: 0,
            }
        );
    }

    /// Removal accepts only the three live Rules crate images and writes exactly
    /// the two CellClass overlay fields, keeping the cell's wall owner.
    ///
    /// Both cells sit inside the `MapClass::In_Bounds @ 0x00568300` diamond
    /// (`size_width < x + y`), which removal tests before it looks at any
    /// CellClass at all.
    #[test]
    fn crate_overlay_removal_is_identity_specific_and_two_fields_wide() {
        let mut sim = sim_with_grid(0x54);
        let rules = crate_ruleset("");
        let registry = crate_registry();
        let wood = registry.id_for_name("WOOD").expect("WOOD overlay");
        let tiberium = registry.id_for_name("TIB01").expect("TIB01 overlay");

        {
            let grid = sim.overlay_grid.as_mut().expect("grid");
            grid.place_overlay(15, 15, wood, 0xFF);
            grid.place_overlay(16, 15, tiberium, 3);
            grid.cell_mut(15, 15).wall_owner = Some(crate::sim::intern::InternedId::from_index(5));
            let _ = grid.take_dirty_cells();
        }

        assert!(
            !remove_crate_overlay_from_cell(&mut sim, &rules, &registry, (16, 15)),
            "a non-crate identity is never removed"
        );
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(16, 15).overlay_id,
            Some(tiberium)
        );

        assert!(remove_crate_overlay_from_cell(
            &mut sim,
            &rules,
            &registry,
            (15, 15)
        ));
        let cleared = sim.overlay_grid.as_ref().unwrap().cell(15, 15);
        assert_eq!(cleared.overlay_id, None);
        assert_eq!(cleared.overlay_data, 0);
        assert_eq!(
            cleared.wall_owner,
            Some(crate::sim::intern::InternedId::from_index(5)),
            "removal writes CellClass+0x44 and +0x11E only"
        );
        assert_eq!(
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .take_removed_render_cells(),
            vec![(15, 15)],
            "the removed cell publishes exactly one presentation receipt"
        );
    }

    /// A cell outside the native In_Bounds diamond is refused before any
    /// CellClass read, exactly as `0x004A1AB6` does.
    #[test]
    fn crate_overlay_removal_refuses_an_out_of_bounds_cell() {
        let mut sim = sim_with_grid(0x5B);
        let rules = crate_ruleset("");
        let registry = crate_registry();
        let wood = registry.id_for_name("WOOD").expect("WOOD overlay");
        {
            let grid = sim.overlay_grid.as_mut().expect("grid");
            grid.place_overlay(5, 5, wood, 0xFF);
            let _ = grid.take_dirty_cells();
        }

        assert!(!remove_crate_overlay_from_cell(
            &mut sim,
            &rules,
            &registry,
            (5, 5)
        ));
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
            Some(wood),
            "In_Bounds rejects 5 + 5 <= size width before Get_CellClass"
        );
    }

    /// Removal that finds no matching identity still lets the caller free the
    /// slot: a ghost slot's cell never carried a crate overlay.
    #[test]
    fn crate_slot_clear_ignores_removal_failure_on_a_ghost_cell() {
        let mut sim = sim_with_grid(0x55);
        let rules = crate_ruleset("");
        let registry = crate_registry();
        *sim.crate_authority.slot_mut(0) = CrateSlot {
            start_frame: 5,
            aux: 0,
            duration: 60,
            cell_x: 12,
            cell_y: 13,
        };

        assert!(clear_crate_slot(&mut sim, &rules, &registry, 0, 20));

        assert!(sim.crate_authority.slots()[0].is_empty());
        assert_eq!(sim.crate_authority.slots()[0].duration, 45);
    }

    /// The two gates are the whole function: a zero game mode or a cleared
    /// Crates option spends no draw and expires nothing.
    #[test]
    fn crate_regen_gates_on_game_mode_and_the_crates_option() {
        let rules = crate_ruleset("");
        let registry = crate_registry();
        for (game_mode_nonzero, crates) in [(false, true), (true, false), (false, false)] {
            let mut sim = sim_with_grid(0x56);
            sim.session.game_mode_nonzero = game_mode_nonzero;
            sim.session.game_options.crates = crates;
            *sim.crate_authority.slot_mut(0) = CrateSlot {
                start_frame: -1,
                aux: 0,
                duration: 0,
                cell_x: 8,
                cell_y: 8,
            };
            let rng_before = sim.scenario_rng.state();

            let regen = tick_crate_regeneration(&mut sim, &rules, &registry, None, lighting());

            assert_eq!(regen, CrateRegeneration::default());
            assert_eq!(sim.crate_authority.slots()[0].cell_x, 8);
            assert_eq!(
                sim.scenario_rng.state(),
                rng_before,
                "a gated call draws nothing"
            );
        }
    }

    /// An unexpired slot is skipped without a draw; the expired one clears and
    /// takes exactly one replacement placer call.
    #[test]
    fn crate_regen_expires_only_due_slots_and_replaces_each_once() {
        let mut sim = sim_with_grid(0x57);
        sim.session.game_mode_nonzero = true;
        sim.session.game_options.crates = true;
        sim.session.binary_frame = 1_000;
        let rules = crate_ruleset("");
        let registry = crate_registry();

        *sim.crate_authority.slot_mut(0) = CrateSlot {
            start_frame: 900,
            aux: 0,
            duration: 5_000,
            cell_x: 6,
            cell_y: 6,
        };
        *sim.crate_authority.slot_mut(1) = CrateSlot {
            start_frame: 100,
            aux: 0,
            duration: 200,
            cell_x: 7,
            cell_y: 7,
        };
        let untouched = sim.crate_authority.slots()[0];

        let regen = tick_crate_regeneration(&mut sim, &rules, &registry, None, lighting());

        assert_eq!(regen.expired, 1, "only the due slot expires");
        assert_eq!(regen.accepted, 1, "one replacement placer call");
        assert_eq!(
            sim.crate_authority.slots()[0],
            untouched,
            "an unexpired slot is not touched"
        );
        let replaced = sim.crate_authority.slots()[1];
        assert!(
            !replaced.is_empty(),
            "the freed slot is the lowest free one"
        );
        assert_eq!(
            replaced.start_frame, 1_000,
            "the new timer starts this frame"
        );
    }

    /// The replacement lands at or below the expiring index, so one ascending
    /// pass never revisits it and never cascades.
    #[test]
    fn crate_regen_replacement_never_lands_above_the_expiring_slot() {
        let mut sim = sim_with_grid(0x58);
        sim.session.game_mode_nonzero = true;
        sim.session.game_options.crates = true;
        sim.session.binary_frame = 4_000;
        let rules = crate_ruleset("");
        let registry = crate_registry();

        for index in [0usize, 1, 2] {
            *sim.crate_authority.slot_mut(index) = CrateSlot {
                start_frame: -1,
                aux: 0,
                duration: 0,
                cell_x: 5 + index as i16,
                cell_y: 5,
            };
        }

        let regen = tick_crate_regeneration(&mut sim, &rules, &registry, None, lighting());

        assert_eq!(regen.expired, 3, "each due slot expires exactly once");
        assert_eq!(regen.accepted, 3);
        for index in 3..CRATE_SLOT_CAPACITY {
            assert!(
                sim.crate_authority.slots()[index].is_empty(),
                "no replacement may appear above the expiring indices"
            );
        }
        for index in [0usize, 1, 2] {
            assert_eq!(
                sim.crate_authority.slots()[index].start_frame,
                4_000,
                "every replacement re-armed its timer this frame"
            );
        }
    }

    /// A regenerated crate really moves on the map: the expired cell's overlay
    /// is gone and the only crate left is the one the replacement marked.
    #[test]
    fn crate_regen_removes_the_old_overlay_and_marks_the_new_cell() {
        let mut sim = sim_with_grid(0x59);
        sim.session.game_mode_nonzero = true;
        sim.session.game_options.crates = true;
        sim.session.binary_frame = 500;
        let rules = crate_ruleset("");
        let registry = crate_registry();
        let wood = registry.id_for_name("WOOD").expect("WOOD overlay");

        {
            let grid = sim.overlay_grid.as_mut().expect("grid");
            grid.place_overlay(15, 15, wood, 0xFF);
            let _ = grid.take_dirty_cells();
        }
        *sim.crate_authority.slot_mut(0) = CrateSlot {
            start_frame: -1,
            aux: 0,
            duration: 0,
            cell_x: 15,
            cell_y: 15,
        };

        let regen = tick_crate_regeneration(&mut sim, &rules, &registry, None, lighting());

        assert_eq!(regen.expired, 1);
        assert_eq!(regen.visible, 1);
        let now = sim.crate_authority.slots()[0];
        assert_eq!(now.start_frame, 500, "the replacement armed a fresh timer");
        assert_ne!(
            (now.cell_x, now.cell_y),
            (15, 15),
            "fixture precondition: the replacement drew a different cell, so the              assertions below actually distinguish removal from a no-op"
        );
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(15, 15).overlay_id,
            None,
            "the expired crate's overlay identity was erased"
        );
        assert_eq!(
            crate_cells(&sim, &registry),
            vec![(now.cell_x as u16, now.cell_y as u16)],
            "the expired crate overlay is gone and only the replacement remains"
        );
    }

    /// Retail names the same overlay for two of the three image slots
    /// (`CrateImg` and `WoodCrateImg` are both `CRATE`), and all three are
    /// accepted identities. Pin every slot, including the duplicate case the
    /// stock pointer table actually produces.
    #[test]
    fn crate_overlay_removal_accepts_every_rules_image_including_duplicates() {
        let registry = crate_registry();
        for (wood, common, water) in [
            ("WOOD", "SILVER", "WATER"),
            // The stock shape: WoodCrateImg == CrateImg == CRATE.
            ("SILVER", "SILVER", "WATER"),
        ] {
            let rules = super::super::tests::crate_ruleset_with_images(wood, common, water, "");
            for name in [wood, common, water] {
                let mut sim = sim_with_grid(0x60);
                let id = registry.id_for_name(name).expect("configured overlay");
                sim.overlay_grid
                    .as_mut()
                    .expect("grid")
                    .place_overlay(15, 15, id, 0xFF);
                assert!(
                    remove_crate_overlay_from_cell(&mut sim, &rules, &registry, (15, 15)),
                    "identity {name} is one of the three live Rules crate images"
                );
                assert_eq!(
                    sim.overlay_grid.as_ref().unwrap().cell(15, 15).overlay_id,
                    None
                );
            }
        }
    }

    /// `CrateSlot__RemoveCrateOverlayFromCell` ends at its two field writes with
    /// no `CellClass::RecalcAttributes` tail, so the cell keeps the land type
    /// the crate gave it. The erased coordinate must therefore reach
    /// presentation through the removal channel and NOT through `dirty_cells`,
    /// whose frame-tail drain re-derives land from the pristine tile.
    #[test]
    fn crate_overlay_removal_publishes_a_removal_and_never_a_recalc_dirty_cell() {
        let mut sim = sim_with_grid(0x61);
        let rules = crate_ruleset("");
        let registry = crate_registry();
        let wood = registry.id_for_name("WOOD").expect("WOOD overlay");
        {
            let grid = sim.overlay_grid.as_mut().expect("grid");
            grid.place_overlay(15, 15, wood, 0xFF);
            let _ = grid.take_dirty_cells();
            let _ = grid.take_removed_render_cells();
        }

        assert!(remove_crate_overlay_from_cell(
            &mut sim,
            &rules,
            &registry,
            (15, 15)
        ));

        let grid = sim.overlay_grid.as_mut().expect("grid");
        assert_eq!(
            grid.take_dirty_cells(),
            Vec::new(),
            "a removal must not enter the attribute-recalc queue"
        );
        assert_eq!(
            grid.take_removed_render_cells(),
            vec![(15, 15)],
            "presentation learns about the erased cell on its own channel"
        );
    }

    /// Production delivery: the ordinary `advance_tick` master frame reaches the
    /// regeneration rung, and the same tick with the Crates option off does not.
    #[test]
    fn advance_tick_reaches_crate_regeneration_only_while_crates_are_on() {
        use std::collections::BTreeMap;

        let rules = crate_ruleset("");
        let registry = crate_registry();
        let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

        let due_slot = CrateSlot {
            start_frame: -1,
            aux: 0,
            duration: 0,
            cell_x: 15,
            cell_y: 15,
        };

        let mut on = sim_with_grid(0x5C);
        on.session.game_mode_nonzero = true;
        on.session.game_options.crates = true;
        *on.crate_authority.slot_mut(0) = due_slot;
        on.advance_tick(&[], Some(&rules), &height_map, None, Some(&registry), 33);
        let regenerated = on.crate_authority.slots()[0];
        assert_ne!(
            regenerated, due_slot,
            "the master frame must reach MapClass__UpdateCrateRegenTimers"
        );
        assert_eq!(
            regenerated.start_frame, 0,
            "the rung runs on the pre-increment frame this tick observed"
        );
        assert_eq!(
            on.session.binary_frame, 1,
            "the frame counter still commits after every phase"
        );
        assert!(
            !regenerated.is_empty(),
            "the replacement took the free slot"
        );

        assert!(
            on.take_master_frame_test_trace()
                .contains(&crate::sim::world::MasterFrameTestRung::CrateRegen),
            "the rung is an observable master-frame position, not an incidental call"
        );

        let mut off = sim_with_grid(0x5C);
        off.session.game_mode_nonzero = true;
        off.session.game_options.crates = false;
        *off.crate_authority.slot_mut(0) = due_slot;
        off.advance_tick(&[], Some(&rules), &height_map, None, Some(&registry), 33);
        assert_eq!(
            off.crate_authority.slots()[0],
            due_slot,
            "a paused-crates session never regenerates"
        );
    }

    /// A paused, already-zero slot re-fires every tick until a placement takes
    /// it — the native `start == -1, duration == 0` branch.
    #[test]
    fn crate_regen_paused_zero_slot_refires_until_a_placement_succeeds() {
        let mut sim = sim_with_grid(0x5A);
        sim.session.game_mode_nonzero = true;
        sim.session.game_options.crates = true;
        let rules = crate_ruleset("");
        let registry = crate_registry();

        // Fill every slot above index 0 so the replacement can only reuse the
        // slot the scan just freed, then keep zeroing its timer.
        for index in 1..CRATE_SLOT_CAPACITY {
            *sim.crate_authority.slot_mut(index) = CrateSlot {
                start_frame: 0,
                aux: 0,
                duration: i32::MAX,
                cell_x: 1,
                cell_y: 1,
            };
        }
        *sim.crate_authority.slot_mut(0) = CrateSlot {
            start_frame: -1,
            aux: 0,
            duration: 0,
            cell_x: 9,
            cell_y: 9,
        };

        for _ in 0..3 {
            let regen = tick_crate_regeneration(&mut sim, &rules, &registry, None, lighting());
            assert_eq!(
                regen.expired, 1,
                "the paused zero-duration slot expires on every pass"
            );
            *sim.crate_authority.slot_mut(0) = CrateSlot {
                start_frame: -1,
                aux: 0,
                duration: 0,
                ..sim.crate_authority.slots()[0]
            };
        }
    }
}
