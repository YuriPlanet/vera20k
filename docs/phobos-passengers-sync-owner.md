# Passenger owner sync (`Passengers.SyncOwner` / `.RevertOnExit`)

A transport can re-house its cargo when its own house changes and put each
passenger back on the house it held when it boarded. Two `Nullable<bool>`
TechnoType keys on the transport (`Ext/TechnoType/Body.cpp:958-959`, fields
`Ext/TechnoType/Body.h:202-203`, constructors unset) fall back through
`Nullable<bool>::Get` to two `[General]` `Valueable<bool>` globals
(`Ext/Rules/Body.cpp:467-468`, fields `Ext/Rules/Body.h:377-378`,
constructors `:882-883`): `Passengers.SyncOwner=false`,
`Passengers.SyncOwner.RevertOnExit=true`.

```ini
[General]
Passengers.SyncOwner=false              ; Valueable<bool>
Passengers.SyncOwner.RevertOnExit=true  ; Valueable<bool>

[SOMETRANSPORT]
Passengers.SyncOwner=                   ; Nullable<bool>, falls back to [General]
Passengers.SyncOwner.RevertOnExit=      ; Nullable<bool>, falls back to [General]
```

## Native behavior and hooks (`Ext/Techno/Hooks.Transport.cpp:1-149`)

- `TechnoClass_EnterTransport @ 0x71067B`, at `TechnoClass::AddPassenger`'s
  (`0x00710670`) head before the Cargo add: with SyncOwner **and**
  RevertOnExit, the entering passenger's current house is stored on its
  `FootExt::OriginalPassengerOwner` (`Ext/Foot/Body.h:18`, serialized at
  `Ext/Foot/Body.cpp:943`). Boarding itself changes no house — stock
  boarding is an allied admission and the record is what later arms restore.
- `TechnoClass_ChangeHouse_Passenger_SyncOwner @ 0x701881`, the join point
  right after the Techno owner-change tail's `UpdateReveal` (`0x0070187B`)
  and before the Building/Foot tails: on the transport's house change, with
  SyncOwner set, every cargo member takes the transport's new owner
  (`SetOwningHouse(pOwner, false)`, walking `Passengers` head to tail). Each
  member's own owner change runs in full, so a member that is itself a
  loaded transport syncs its hold.
- `FootClass_LeaveTransport @ 0x4DE722`, inside
  `FootClass::RemoveFirstPassenger` (`0x004DE710`, the vehicle/aircraft
  departure and death-escape wrapper; a building's cargo arms pop through
  `CargoClass` alone and see no hook): with SyncOwner + RevertOnExit and a
  recorded house, the leaving passenger takes it back under the
  `IsOwnerChangeFromRevertOnExit` latch (`Ext/Foot/Body.h:27`, not
  serialized, `Ext/Foot/Body.cpp:952`). The record itself stays standing.
- `UnitClass_ReceiveDamage_Cargo_SyncOwner @ 0x737F80`, 6 bytes at the head
  of `UnitClass::ReceiveDamage`'s passenger block
  (`0x00737F80..0x007381B6`), before the stock open-topped clear
  (`0x00737F80..0x00737F92`) and before Ares survivor handling: every
  dying-transport passenger that still holds a record takes it back, with
  no latch and no record clear.
- `TechnoClass_ThreatEvals_OpenToppedOwner @ 0x6F8FD7 / 0x6F7EC2 / 0x6F89F4
  / 0x6FA33C` (one body, four sites): when the rider's `Transporter` type
  has SyncOwner, the sites skip the stock computation of the mind
  controller's house over the rider's `OpenTopped` transport — the rider
  then evaluates targets for its own house, which the sync has already made
  the transport's. The stock condition is the shared
  controller-house read the four sites compute in
  `0x006F7EA2..0x006F7ED8`, `0x006F89D1..0x006F8A0A`,
  `0x006F8FA8..0x006F8FD7` and `0x006FA30C..0x006FA342` (VERA:
  `combat_weapon::controlled_transport_house`, whose consumers are
  Evaluate_Candidate, the cell walk, `Greatest_Threat`'s global walk and
  `AI_Update`'s allied-target drop), so the skip is exact and no condition
  is left unported.

The latch's consumer is `TechnoClass_Captured_UpdateTracking @ 0x7015C9`'s
AutoDeath block (`Ext/House/Hooks.cpp:225`): with
`AutoDeath.OnOwnerChange.IgnoreRevertOnExit` (key
`Ext/TechnoType/Body.cpp`, global `Ext/Rules/Body.cpp:630`) and the latch
set, the revert-induced owner change assigns `ShouldBeDead = false` and
skips the direction arms; the transporter gate's `!latch` term
(`:253-258`) lets a reverted passenger die from the change that synced it
onto the transport earlier. The AttachEffect
`DiscardOn.OwnerChange.IgnoreRevertOnExit` chain also reads the latch and
still sees `false` there (residual of `sim::auto_death`).

## VERA20K port

- The board-time record is the phobos-gated
  `GameEntity::phobos_original_passenger_owner` field, set inside the
  `TechnoClass::AddPassenger` port (`sim::passenger::add_passenger_voiced`)
  for every head passenger. The revert latch is
  `phobos_revert_on_exit_latch`.
- The sync runs in the ChangeHouse owner (`sim::world::change_owner_impl`)
  at the `0x701881` slot, after the `0x0070187B` tail and before the
  Building/Foot tails, on limbo and placed transports alike (the limbo skip
  `0x007017C0` rejoins the tail at the hook site).
- The exit revert runs in `sim::passenger::depart_cargo_head` for the
  Vehicle, LandedAircraft and DeathEscape routes — the ones that model
  `FootClass::RemoveFirstPassenger` — before placement, so a refused exit
  re-attaches the passenger already reverted, as native's re-add does.
- The dying-cargo revert runs at the head of
  `Simulation::release_dying_unit_passengers`, before the open-topped clear
  and before Ares survivor handling.
- All four owner changes go through VERA's one owner-change path
  (`change_owner_with_rules`), the established mapping of native
  `SetOwningHouse`.
- Every read and arm is Phobos-only; with the keys unset
  (SyncOwner `false` by default) nothing changes.

## Residuals

- The sync and reverts use the full ChangeHouse ceremony per passenger
  where native `SetOwningHouse(house, false)` is the lighter tracking-swap
  path (same mapping VERA's Ares raid and garrison ports use); a
  reverted passenger whose new house is defeated is not refused.
- Native records `OriginalPassengerOwner` at the AddPassenger head before
  the Cargo add; VERA records before the same add, so a Cargo add that
  fails an invariant (debug-asserted, never observed) would leave a stale
  record on a non-passenger.
- The `Abductor=true` exclusion the Phobos docs mention is an Ares
  abduction-path property; VERA has no abductor chain yet.

## Tests

`sim::passenger::sync_owner_tests` drives `board_passenger`,
`change_owner_with_rules`, `depart_cargo_head` and the production receive
damage kill: the capture sync, the exit revert and its `.RevertOnExit=no`
variant, the multi-member sync with a stock control, and the dying
transport restoring board-time houses before the escape.
