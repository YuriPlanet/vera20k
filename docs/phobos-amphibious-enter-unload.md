# Amphibious enter and unload

Two transport keys let a hull take or leave passengers on water. Both are
`Nullable<bool>` on the transport type and fall back to a `[General]` switch.

```ini
[General]
AmphibiousEnter=yes
AmphibiousUnload=yes

[LCRF]                  ; a Unit/Building transport type
AmphibiousEnter=no      ; this hull still refuses boarding over water
AmphibiousUnload=yes
```

## Keys

- `[TechnoType] AmphibiousEnter=` (`Ext/TechnoType/Body.cpp:1107`,
  `Body.h:342`) → `[General] AmphibiousEnter=`, default `no`
  (`Ext/Rules/Body.cpp:197`, `Body.h:654`).
- `[TechnoType] AmphibiousUnload=` (`:1108`, `Body.h:343`) →
  `[General] AmphibiousUnload=`, default `no`
  (`Ext/Rules/Body.cpp:198`, `Body.h:655`).

## Native evidence

- Enter: the `CanEnterNow` rewrite of `0x73758A` rejects a transport whose
  `GetCell()->LandType` is Water unless the effective key is set
  (`Ext/Techno/Hooks.Transport.cpp:231-253`); the DOCKING arm
  `UnitClass_ReceiveCommand` (`0x73796B`) does the same
  (`:613-628`).
- Unload, water branch: `CanUnloadNow` (rewrite of `0x4835D5`/`0x74004B`)
  replaces `LandType != Water` with the head passenger's own
  `GroundType[land].Cost[SpeedType] != 0` when the key is set
  (`:382-388`), consumed at `0x73D769` (`:660-669`).
- Unload, positions: hooks `0x73D7AB` (`:671-689`) and `0x73DAD8`
  (`:716-728`) replace the hard-coded `SpeedType::Wheel`/`MovementZone::Normal`
  in the nearby-cell and leave-position searches with the head passenger's own;
  `0x740C9C` (`:700-714`) does the same for the exit-octant speed check.
- Deploy/unload admission: `0x70107A` (`:646-657`) lets a transport unload on
  water when the key is set, and `0x7400B5` (`:632-644`) relaxes the
  low-cost land refusal for the same key. Water itself already passes both
  tests.

## Where VERA20K owns it

- `rules::phobos_globals::PhobosGlobals::amphibious_enter` / `_unload`: the
  `[General]` reads (`Ext/Rules/Body.cpp:195-196`), both `false` by default.
- `rules::object_type::ObjectType::amphibious_enter` / `_unload`: the single
  resolution owner, `type.unwrap_or(global)`, with a `false` answer when the
  `phobos` feature is off. Every consumer calls it, so the precedence is
  defined once.
- Enter (`0x73758A`, `0x73796B`): `sim::radio::receive::unit_docking`'s
  DOCKING water branch and `unit_can_enter_radio`'s CAN_LOAD gate; the
  click/cursor admission is `sim::passenger::can_enter_transport`.
- Unload predicate (`0x4835D5`/`0x74004B`, `0x73D769`):
  `sim::transport_unload::transport_can_unload_now`, shared by state 0 and the
  `NoQueueUpToUnload` one-frame loop.
- Unload positions: `pick_exit_octant` (`0x740C9C`), `eject_head_passenger`'s
  leave-position search (`0x73DAD8`), and `unit_mission_unload` state 0's
  find-unload-position seed (`0x73D7AB`, passenger `SpeedType`/`MovementZone`
  in place of `Wheel`/`Normal`).
- Cursor gates: `sim::deploy::Simulation::can_deploy_slash_unload`
  (`0x70107A`) and `sim::world::Simulation::unit_self_deploy_site_refused`
  (`0x7400B5`).
- Loco `Mission::Unload` guards `0x4B08EF`/`0x69FFB6`:
  `sim::movement::track_turn::track_movement_gates` — a keyless stock-bug fix,
  not gated on either key.

## Tests

`rules::phobos_amphibious_tests::the_override_falls_back_to_the_global`,
`sim::radio::receive::tests::an_amphibious_enter_transport_takes_a_passenger_on_water`,
`sim::transport_unload::tests::an_amphibious_unload_puts_the_passenger_out_on_water`,
`sim::transport_unload::tests::the_mouse_over_and_deploy_slash_gates_agree_with_the_predicate`,
`sim::deploy_tests::amphibious_unload_admits_a_human_transport_on_water`,
`sim::transport_unload::tests::amphibious_unload_with_no_land_cell_takes_the_cannot_unload_tail`.

## Residuals

- `0x7400B5`'s water arm already passes in VERA; only the keyed land arm is
  changed, so the observable result is the same. Its `1%` land-cost test reads
  the rules `[Clear]`-family row (`rules/terrain_rules.rs`), and the key off
  keeps stock's refusal.
- The `MovementZone` half of `0x73D7AB`/`0x73DAD8` is carried into
  `PassabilityArgs`, but a search with no required zone answers the same
  either way, so only the `SpeedType` half is observable at a boundary cell.
