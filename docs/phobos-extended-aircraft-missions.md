# Extended aircraft missions

Phobos's `ExtendedAircraftMissions` rework of the aircraft mission machinery.
Two `[General]` keys switch it on and set the unlandable damage; six
`[AircraftType]` keys tune it per type. This page records what VERA reads,
which hook behaviours are ported, and the residuals.

## Keys

| Layer | Key | Type | Default |
| --- | --- | --- | --- |
| `[General]` | `ExtendedAircraftMissions` | bool master switch | `no` |
| `[General]` | `ExtendedAircraftMissions.UnlandDamage` | int | `0` |
| `[AircraftType]` | `ExtendedAircraftMissions` | nullable bool, overrides the global master | unset |
| `[AircraftType]` | `ExtendedAircraftMissions.SmoothMoving` | nullable bool | unset |
| `[AircraftType]` | `ExtendedAircraftMissions.EarlyDescend` | nullable bool | unset |
| `[AircraftType]` | `ExtendedAircraftMissions.RearApproach` | nullable bool | unset |
| `[AircraftType]` | `ExtendedAircraftMissions.FastScramble` | nullable bool | unset |
| `[AircraftType]` | `ExtendedAircraftMissions.UnlandDamage` | nullable int | unset |

Readers: `Ext/Rules/Body.cpp:191-192` with the constructor defaults in
`Ext/Rules/Body.h:97-98`, and `Ext/AircraftType/Body.cpp:30-35` with the
`Nullable` fields in `Ext/AircraftType/Body.h:21-26`. The keys live in
`AircraftTypeExt`, so a non-aircraft section that repeats them carries no value
(`src/rules/object_type.rs`, gated on `ObjectCategory::Aircraft`).

Resolution follows the hooks (`Ext/Aircraft/Hooks.cpp:513`, `:596`, `:701`,
`:671`, `:1090`, `:773`, `:1049`): the type's master is
`type.Master.Get([General] ExtendedAircraftMissions)`; each of `SmoothMoving`,
`EarlyDescend`, `RearApproach` and `FastScramble` is `key.Get(master)`; and
`UnlandDamage` is `key.Get([General] ExtendedAircraftMissions.UnlandDamage)`,
which bypasses the master switch. Absent, blank, `none` and `<default>` all
leave the type key unset, so the fallback applies
(`src/rules/phobos_extended_aircraft_missions.rs`,
`RuleSet::extended_aircraft_missions_for`).

## Ported behaviours

### Unlandable damage (`0x414DA8`)

`AircraftClass_Update_UnlandableDamage`
(`Ext/Aircraft/Hooks.cpp:735-794`) runs after `FootClass::Update` for an alive,
`AirportBound`, non-airstrike aircraft (`IsALoaner` is not represented and stays
false). While the aircraft `IsInAir` (vt+0x54, VERA's `is_high_flying`) and it
holds no `DockNowHeadingTo` (`Aircraft+0x6CC`, VERA's `TechnoAmmo::dock`;
`:762`):

- `UnlandDamage > 0`: every fourth frame, selected by
  `(CurrentFrame - LastFireBulletFrame + UniqueID) & 3 == 0`, it takes that much
  damage from `[CombatDamage] C4Warhead=` with no attacker
  (`ReceiveDamage(&damage, 0, C4Warhead, nullptr, true, false, nullptr)`).
  VERA applies the same receiver event with `ignore_defenses` set and the second
  call flag clear (`src/sim/world/techno_ai.rs`,
  `aircraft_unlandable_damage_step`).
- `UnlandDamage < 0` and the master switch on: `Crash(nullptr)`. A negative
  value with the master off stays stock.

### Fast scramble (`0x4425B6`)

`BuildingClass_ReceiveDamage_NoDestroyLink`
(`Ext/Aircraft/Hooks.cpp:1072-1093`) rewrites `BuildingClass::ReceiveDamage`'s
destroy-link loop (`0x442511..0x4425EE`). The loop kills every linked object
within 0x100 leptons, and every linked object of a `Helipad=`, with
`Strength * 10` C4 damage; a farther contact of a non-helipad gets RUN_AWAY
(`0x44259D`). For a helipad's linked aircraft whose type resolves
`ExtendedAircraftMissions.FastScramble`, Phobos takes the RUN_AWAY arm instead:
the aircraft is released by the dying building rather than destroyed with it
(`src/sim/world/lifecycle.rs`, `building_now_dead_contacts`).

### Smooth moving (`0x4168C7`, `0x416A0A`)

`AircraftClass_Mission_Move_SmoothMoving` (`Ext/Aircraft/Hooks.cpp:495-530`,
sites `0x4168C7` in state 2 and `0x416A0A` in state 4) replaces
`AircraftClass::Mission_Move`'s own-cell arrival test. It keeps the stock test
for a team member, an airstrike or loaner, a non-`AirportBound` type and a
type whose `ExtendedAircraftMissions.SmoothMoving` resolves off (`:503-514`).
Otherwise it measures `Point2D::DistanceFrom` from the aircraft's Location to
the NavCom coordinate (`:516`): farther than
`max(SlowdownDistance / 2, turningRadius)` continues the move
(`0x416908`/`0x416A47`); closer, it calls
`TechnoClass::TryNextPlanningTokenNode @ 0x006385C0` and enters idle mode
unless the type's master resolves off or the queued mission is Area Guard,
then returns through `0x416AC0` without writing Mission+0xBC. `turningRadius`
is `GetTurningRadius` (`:485-492`): `Speed / |ROT.Raw * 2pi / 65536|`,
truncated, and 0 when the rate is at most 1e-10 radians.

VERA owns the answer in `src/sim/aircraft/move_mission.rs` (`move_visit`,
`SmoothArrival`) and its world query in `src/sim/world/aircraft_move.rs`
(`smooth_arrival`); `turning_radius` lives in `src/sim/aircraft/mod.rs`.

### Rear approach (`0x4CF190`)

`FlyLocomotionClass_FlightUpdate_SetPrimaryFacing`
(`Ext/Aircraft/Hooks.cpp:574-651`, replacing `0x004CF190..0x004CF29A`) makes
an aircraft whose type resolves `ExtendedAircraftMissions.RearApproach` curve
in from the landing side. With the primary facing unlocked and a `Destination`
equal to the aircraft's `DockNowHeadingTo` or `SpawnOwner` (`:614`), it shifts
the destination by `max(SlowdownDistance / 256, turningRadius / 128)` cells
along `Unsorted::AdjacentCoord` at the landing face
(`landingDir.GetFacing<8>(4)`, one cell toward the far side of the pad,
`:629-630`). When the bearing to the destination differs from the landing
direction by at least 12288 raw, it adds `AdjacentCoord[(landingFace + 2) &
7]` or `[(landingFace - 2) & 7]` before scaling (`:633-634`); otherwise it
scales by `min(cellCounts, horizontalDistance / 724 or / 512)` (`:635-636`).
The primary facing then points at the offset destination, or at the landing
direction when the aircraft is already there (`:643-646`); this arm does not
take the stock strafe hold.

VERA owns the block in `Simulation::fly_rear_approach` and
`aircraft_returning_to_dock` (`src/sim/world/fly_process.rs`), with the
`ADJACENT_COORD` table (`Unsorted::AdjacentCoord @ 0x0089F6D8`) and
`turning_radius` above.

### Early descend (`0x4CF3D0`)

`FlyLocomotionClass_FlightUpdate_SetFlightLevel`
(`Ext/Aircraft/Hooks.cpp:653-716`, replacing `0x004CF3D0..0x004CF4D2`) keeps
the stock fast-descent arm for an elevating Fly within 0x300 of its
destination, including the bridge correction (`:675-693`). For a
non-HunterSeeker aircraft whose type resolves
`ExtendedAircraftMissions.EarlyDescend`, it then starts the slow descent
within `SlowdownDistance` of a destination that is the aircraft's
`DockNowHeadingTo` or `SpawnOwner`, provided that, when `RearApproach` also
resolves on, the current facing is within 16384 raw of the landing direction
(`:696-702`). The target height is `(FlightLevel - destinationHeight) *
distance / SlowdownDistance + destinationHeight`, where `destinationHeight`
is the Fly destination's height above its floor plus one; otherwise the
aircraft holds `FlightLevel` (`:704-710`). VERA owns it in
`Simulation::fly_early_descend_level` (`src/sim/world/fly_process.rs`).

## Residuals

Each residual names its trigger, effect and frequency. The stock behaviour is
untouched; the hooks fire only for a mod that turns the relevant key on.

- **Waypoint paths for aircraft** (`AircraftTypeClass_CanUseWaypoint`, vtable
  `0x7E2908`, `Ext/Aircraft/Hooks.cpp:478-483`). Trigger: the master resolves
  on and a player plots an aircraft onto a planning node. Effect: Phobos lets
  the type use waypoints; VERA's `planning_selection_refusal`
  (`src/sim/world/planning.rs`) still refuses every aircraft, so no path
  exists and SmoothMoving's `TryNextPlanningTokenNode`/Area-Guard continuation
  cannot fire; only the distance arrival on an ordinary Move order is visible.
  Frequency: every aircraft waypoint order in a master-on mod. Risk: medium.
- **`IsALoaner` is always false** (`AircraftTypeExt::IsALoaner` is read at
  `Ext/AircraftType/Body.cpp:40` but not represented). Trigger: a type with
  `IsALoaner=yes`. Effect: SmoothMoving and Unlandable damage apply to
  aircraft Phobos excludes at their loaner gates. Frequency: only a mod that
  sets the key. Risk: low.
- **Rear approach without a dock** (`Ext/Aircraft/Hooks.cpp:608-647`).
  Trigger: `RearApproach` resolves on for an aircraft that is not returning to
  its dock or spawner, in particular a strafing aircraft within 768 leptons
  whose desired heading differs by more than 8192 raw. Effect: the hook's else
  arm sets the primary facing without the stock strafe hold; VERA's
  `fly_rear_approach` answers `None` there and keeps the stock hold, so the
  heading stays where Phobos would turn it. Frequency: mods with the key and
  strafing aircraft. Risk: low.
- **Unlandable damage, redirect and Area-Guard arms**
  (`Ext/Aircraft/Hooks.cpp:735-794`). Trigger: master on (the redirect and the
  dock search) or an Area-Guarding aircraft holding a `DockNowHeadingTo` and a
  `MissionStatus`. Effect: the hook redirects an aircraft whose `ArchiveTarget`
  lies farther than `GetGuardRange(1) * 1.1` (clear Target, set destination to
  the archive), calls `TryNearestDockBuilding(&Dock, 0, 0)`, and with the
  switch off refuses positive damage when that search answers; the
  Area-Guarding arm clears the archive and enters idle. VERA ports only the
  dock gate, so it can damage an aircraft Phobos would send to a dock first,
  does not pull a distant Area-Guard aircraft back to its archive, and leaves
  the Area-Guarding one hovering. Frequency: master-on mods or a positive
  `UnlandDamage`; the retail defaults (`no`, `0`) are dormant. Risk: medium.
- **Idle no-crash** (`:1039-1066`, sites `0x4179F7` and `0x417B82` in
  `AircraftClass::Enter_Idle_Mode`). Trigger: an `AirportBound` aircraft with no
  dock that would crash on entering idle, not airstrike/loaner, with
  `UnlandDamage >= 0`. Effect: instead of crashing, an unteamed aircraft not
  already Area-Guarding at an archive gets the nearest friendly airfield cell
  as destination and archive target and queues Area Guard; a team member just
  skips the crash. VERA's `enter_idle_mode` (`src/sim/aircraft/idle_entry.rs`)
  still crashes it. Frequency: AirportBound aircraft that run out of docks in a
  Phobos build; default `UnlandDamage=0` means the hook is active whenever a
  Phobos build runs, so this is the widest residual on this page.
- **The master switch's other hooks stay stock.** Trigger: the master resolves
  on. Effect: `AircraftClass_SetArchiveTarget_Wrapper`
  (`0x41AB09..0x41ACB9`), `AircraftClass_Mission_AreaGuard`
  (`0x41A96C`/`0x41A982`), `AircraftClass_GreatestThreat` (vtable `0x7E2668`),
  `TechnoClass_MouseOverObject_EnableGuardObject` (`0x7001B0`),
  `AircraftTypeClass_CanAttackMove` (`0x7E290C`), the attack-move holds
  (`0x6FA68B`, `0x4DF3BA`, `0x4DF42A`; see [attack move](phobos-attack-move.md)),
  the attack continuation (`0x418CD1`) and
  `FlyLocomotionClass_StateUpdate_NoLanding` (`0x4CE42A`) are not ported, so
  those mission behaviours remain stock. Frequency: mods enabling the master.
  Risk: medium.

## Tests

- `rules::phobos_extended_aircraft_missions` — the two globals and six type
  keys through the production `RuleSet` reader: defaults, the type master
  override, per-key fallbacks, `UnlandDamage` bypassing the master, and the
  aircraft-only category gate.
- `sim::aircraft::unland_damage_tests` — a positive value hurts an airborne
  airport-bound aircraft in five-point hits every fourth frame, a docked one
  takes none, and a negative value with the master on crashes it.
- `sim::aircraft::dock_cycle_tests::a_fast_scramble_aircraft_takes_off_instead_of_dying_with_its_helipad`
  — the same killing hit destroys a normal docked ORCA and releases a
  `FastScramble` one.
- `sim::world::aircraft_move::tests::smooth_moving_arrives_by_distance_instead_of_cell`
  — a NavCom one cell away keeps the stock own-cell test moving but takes the
  SmoothMoving distance arrival, which leaves Mission+0xBC unchanged.
- `sim::world::fly_process::tests::rear_approach_offsets_the_landing_destination`
  — the primary facing aims at the pad with the key off and three cells west
  and south of it with the key on.
- `sim::world::fly_process::tests::early_descend_starts_the_slow_descent_within_slowdown_distance`
  — a returning aircraft holds its flight level with the key off and descends
  to 768 of 1500 leptons with the key on.
