# Missile cruise and takeoff

Phobos gives an aircraft type that flies as a rocket three keys
(`Ext/AircraftType/Body.cpp:42-44`, `AircraftTypeExt::Initialize` at `:12`;
consumers in `Ext/Techno/Hooks.cpp:2240-2325`).

```ini
[MISSILE]                        ; AircraftType
Missile.Cruise=false             ; bool, default false
Missile.TakeOffSeparation=24     ; int, default 24
Missile.TakeOffAnim=V3TAKOFF     ; AnimType, default V3TAKOFF
```

## Behavior

- `Missile.Cruise=yes` sends the rocket's tilt-state entry into the raise body
  (`RocketLocomotionClass_Process_CruiseMissileCheck` hook `0x662354` and
  `_CruiseMissileRaise` `0x662720`): a crate with `PauseFrames=0` raises
  immediately instead of tilting. The stock CMisl bool only selects the pause
  state's next state; it does not promote from the tilt.
- `Missile.TakeOffSeparation` is the takeoff trailer-timer length in frames at
  the pause smoke interval (`_CustomSmokeInterval` `0x6623FC`) and at the raise
  takeoff (`0x6624FB`); the stock value is 24.
- `Missile.TakeOffAnim` names the raise takeoff animation
  (`_CustomMissileTakeoff` `0x6624FB`, default `V3TAKOFF`; `none`/`<none>`
  means no puff). The pause and tilt puffs keep the rules block/Ares anim;
  Phobos does not hook those sites.

## Where VERA20K owns it

`rules::phobos_missile` reads the three keys into an `ObjectType` field;
`sim::movement::rocket_movement` consults the linked owner through the new
`RocketHost::phobos_missile` method: `process` ORs the type's cruise flag into
the CMisl selection, and the pause/raise states take the separation and
takeoff anim from `takeoff_settings`. `sim::world::rocket_flight`'s host
resolves the owner's type. Stock builds keep the 24-frame timer and the rules
block's anim.

## Residuals

- An unknown `Missile.TakeOffAnim` name is kept literally; native
  `AnimTypeClass::Find` would keep the previous value instead. No retail key
  names an unregistered animation.
- Ares reads the same `Missile.TakeOffAnim` key for its custom-missile paths;
  in `ares,phobos` builds the Phobos field outranks the Ares block at the
  raise site, but both read the same section key, so authored values agree.
- The host clones the small settings struct for every rocket frame.
