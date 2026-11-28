# Sell or undeploy a building on impact (`BuildingSell`, `BuildingUndeploy`)

Two `WarheadType` arms that act on a hit building: sell it, or pack it up into
its `UndeploysInto=` unit. `BuildingSell` outranks `BuildingUndeploy`.

## Keys

| Key | Type | Default |
|---|---|---|
| `BuildingSell=` | `Valueable<bool>` | `no` |
| `BuildingSell.IgnoreUnsellable=` | `Valueable<bool>` | `no` |
| `BuildingUndeploy=` | `Valueable<bool>` | `no` |
| `BuildingUndeploy.Leave=` | `Valueable<bool>` | `no` |

Read: `WarheadTypeExt::LoadFromINI` (`Ext/WarheadType/Body.cpp:362-365`),
declarations `Ext/WarheadType/Body.h:207-210`, constructor `Body.h:486-489`.
VERA reads them in `src/rules/phobos_warhead_extras.rs`, owner
`WarheadType::phobos_extras`.

## Behaviour

`WarheadTypeExt::DetonateOnOneUnit` calls `ApplyBuildingUndeploy(pTarget)` after
`RemoveMindControl`, `ChangeOwner` and `ApplyConvert`
(`Ext/WarheadType/Detonate.cpp:213-214`). The body (`:266-352`):

1. A non-building target returns immediately.
2. `BuildingSell` **returns either way** (`:270-279`): with
   `CanBeSold() && !IsStrange()`, or unconditionally under
   `BuildingSell.IgnoreUnsellable`, it clears the archive target and calls
   `Sell(1)`; otherwise it does nothing at all and never falls through to the
   undeploy arm.
3. Otherwise a `Construction` or `Selling` mission returns (`:283-287`).
4. A type with no `UndeploysInto=`, or a `ConstructionYard` while the game mode
   forbids MCV redeploy, returns (`:291-292`).
5. The building's cell becomes the archive target, then `Sell(1)` starts the
   pack-up (`:351-353`). With `BuildingUndeploy.Leave=` the cell is instead
   chosen by a 16-direction threat weighting over armed non-allied units within
   20 cells, then moved about 14 cells along that bin's radian and snapped to a
   deployable cell (`:304-355`).

VERA's owner is `sim::combat::building_sell::apply`; the sell arm calls
`production::sell_back(SellOrder::Player)` after `set_archive_target(None)`,
and the undeploy arm calls `Simulation::undeploy_building`, whose
`can_undeploy_building_runtime` already carries the `UndeploysInto` and
`ConstructionYard`/MCVRedeploy gates. `Simulation::finish_undeploy`
(`world_spawn.rs:3233-3247`) hands the building's archive target to the new
unit as its destination (`Assign_Destination` then `Move`), so the cell the
arm records here is where the resulting unit walks to.

`BuildingUndeploy.Leave=`'s pick lives in
`sim::combat::building_sell_leave` (the pure half) plus
`building_sell::relocation_cell` (the scan and the snap):

- `threat_bin` is `DirectionClass::GetValue<4>` (`YRpp/Dir.h:44-47`), the
  rounding `phobos_jumpjet_tilt::dir_value_128` spells out for `N = 7`;
- `weighted_bins` is the triangular kernel
  `(8 - |j|) * record[(d + j) & 15]` over `j` in `-7..=7` (`:318-328`), in the
  native `int`'s wrapping arithmetic;
- `strongest_bin` is the strict-greater argmax, which keeps the earlier bin on
  a tie exactly as `if (newCosts > costs)` does, and answers `None` when no
  armed non-ally is in range so the caller draws `RandomRanged(0, 15)`;
- `leave_target_cell` is `radian = -(((dir - 4) / 16.0) * TwoPi)` then
  `X -= 14*cos`, `Y += 14*sin` (`:338-348`), with the short casts truncating
  toward zero and the `CellStruct` adds wrapping;
- the snap is `MapClass::NearByLocation(cell, UndeploysInto->SpeedType, -1,
  UndeploysInto->MovementZone, false, width + 2, height + 2, false, false,
  false, false, CellStruct::Empty, false, false)` (`:354-355`) onto
  `Find_Nearby_Passable_Cell`. Every one of that call's six booleans is
  `false`, so no passability flag needed a tie-break between the two verified
  native spellings; the `-1` zone argument resolves to the seed's own
  `GetZoneID` without the bridge lookup, the shape
  `phobos_unlimbo_detonate::search_landing_cell` already ports. Phobos keeps
  the raw offset when the snap returns `CellStruct::Empty`
  (`if (newCell != CellStruct::Empty)`), which `relocation_cell` mirrors.

`IsStrange()` is `sim::combat::crit`'s existing
`ObjectType::is_1x1_with_undeploy`, the same predicate the kill-weapon and
trajectory owners use.

## Native anchors

`BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) is the hook whose body reaches
`WarheadTypeExt::ApplyBuildingUndeploy` (`Ext/WarheadType/Detonate.cpp:217,266`). The sell and
undeploy calls that body makes are gamemd's own `BuildingClass` methods, already owned elsewhere in
VERA, so no separate address is cited for them here.

## Residuals

- **`CanBeSold()` is VERA's `production::can_sell_building`.** That function
  models `IsSellable` (category, draining, `Unsellable`, the buildup/wall
  arms); native `CanBeSold()` additionally folds in the mission test, which the
  undeploy arm re-checks separately here. A building with a live `Selling`
  mission that also passes `IsSellable` could be sold again natively where VERA
  refuses; the sell path is idempotent, so the visible difference is nil.
- **`Sell(1)` vs `SellOrder::Player`.** Phobos passes control `1` (the
  computer's sale) so the sale is not blocked by the player-facing gates VERA's
  `SellOrder::Player` models. Both end in the same refund and detach; only a
  mod that gates on the control value could tell them apart.
- **The non-`Leave` arm records no archive target.** Native calls
  `SetArchiveTarget(GetMapCoords())` either way, so the resulting unit is sent
  to its own spawn cell and picks up a `Move` mission. VERA leaves the archive
  `None`, which `finish_undeploy` reads as "stand still" — the unit lands on
  that same cell without the no-op move. A mod that keys on the `Move` mission
  being queued could tell them apart.

## Tests

- `src/rules/phobos_warhead_extras_tests.rs` — the parse group, including
  `only_the_three_primary_arms_ask_for_a_visit`.
- `src/sim/combat/building_sell_tests.rs` — the arm through the production
  detonation path: `building_sell_sells_the_hit_building`,
  `an_unsellable_building_is_left_alone_without_the_override`,
  `ignore_unsellable_overrides_the_gate`,
  `a_strange_building_needs_the_override_too`,
  `a_refused_sell_never_falls_through_to_the_undeploy_arm`,
  `building_undeploy_packs_the_building_into_its_unit`,
  `a_building_without_undeploys_into_stays`,
  `the_ordinary_undeploy_sets_no_destination`,
  `leave_relocates_the_destination_away_from_the_scan`,
  `a_non_building_target_is_ignored`,
  `a_missing_target_sets_no_destination`.
- `src/sim/combat/building_sell_leave_tests.rs` — the pure pick:
  `threat_bin_rounds_the_facing_word_to_sixteen_bins_half_up`,
  `every_bin_of_the_circle_is_reachable`,
  `a_lone_spike_weighs_a_triangle_across_the_ring`,
  `a_uniform_record_weighs_every_bin_the_same`,
  `a_runaway_cost_wraps_instead_of_panicking`,
  `a_tie_keeps_the_earlier_bin`,
  `the_argmax_follows_the_heaviest_bin`,
  `an_empty_record_hands_the_decision_to_the_die`,
  `the_four_cardinal_bins_step_fourteen_cells`,
  `consecutive_bins_rotate_a_quarter_step_apart`,
  `the_step_wraps_at_the_cell_edges`.
