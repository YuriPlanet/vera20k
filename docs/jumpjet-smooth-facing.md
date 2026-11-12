# Smooth Jumpjet facing

A VERA option, off by default. With it off, Jumpjet units such as the
Nighthawk turn as in the original game.

```ini
[JumpjetControls]
SmoothFacing=yes   ; default no
```

A mod, a map or your [personal rules](personal-rules.md) (`VERARULES.INI`) can
set it. In an online game every player needs the same rules, which the rules
compatibility check compares.

## What the original does

A Jumpjet keeps two headings: its body's, and its locomotor's own. While it
moves, the locomotor's heading is copied onto the body every frame, in one
step (`JumpjetLocomotionClass::Update_Coordinates_And_Altitude`,
`0x0054D67B..0x0054D692`). That gives three sudden turns:

- **First take-off:** the locomotor's heading starts as east when the unit is
  made (`Link_To_Object`, `0x0054AE19`), so the body snaps to east before it
  turns to its course.
- **Unload:** the Unload mission turns the body to its exit direction in one
  step (`Do_Turn`, `0x0054B6E0`), leaving the locomotor's heading as it was.
- **Next take-off:** the locomotor's old heading is copied back onto the body
  (`Process` runs Update at `0x0054AEF0`, before State 0 at `0x0054B046` gives
  the locomotor the body's heading).

## What the option changes

- On the ground the locomotor's heading is not copied onto the body. Take-off
  then starts from the heading the body has, and the body turns to its course
  at `JumpjetTurnRate=`.
- For an Unload, a Jumpjet's body turns to the exit direction at its `ROT=`
  instead of in one step. Passengers leave as before, by the heading the body
  has when each one gets out.

Not changed: turning in flight, and the turns of other orders that face a
Jumpjet on the ground (a simple deployer, team script action 42).

Code: `update_coordinates_and_altitude` in
`src/sim/movement/jumpjet_flight.rs` and `unit_mission_unload` in
`src/sim/transport_unload.rs`.
