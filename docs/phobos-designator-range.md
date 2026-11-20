# Optional Phobos designator range rings

Phobos is in every build; turn it on from a game's data to ring the technos that designate or inhibit
the armed superweapon. While a superweapon is armed, every current-player
techno named in its `SW.Designators` and every hostile techno named in its
`SW.Inhibitors` draws a range ring in its owning house's colour. This is a
client/presentation feature: no simulation behaviour changes.

Three switches must all be on, plus an armed superweapon:

```ini
[Phobos]                            ; VERA.INI / RA2MD.INI client profile
ShowDesignatorRange=no              ; client switch, default no (Phobos.INI.cpp:133)

[AudioVisual]                       ; rulesmd.ini
ShowDesignatorRange=yes             ; rules switch, default yes (Ext/Rules/Body.cpp:176)

[SOMESUPERWEAPON]                   ; rulesmd.ini SuperWeaponType
ShowDesignatorRange=yes             ; per-type switch, default yes (Ext/SWType/Body.cpp:283)
SW.Designators=DESIG1,DESIG2        ; own technos that designate (Ares)
SW.Inhibitors=INHIB1                ; hostile technos that inhibit (Ares)

[DESIG1]
DesignatorRange=5                   ; cells; unset falls back to Sight (Ares)
Sight=3

[INHIB1]
InhibitorRange=4                    ; cells; unset falls back to Sight (Ares)
Sight=2
```

The native consumer is
`Tactical_SuperLinesCircles_ShowDesignatorRange @ 0x6DBE74`
(`Ext/SWType/Hooks.cpp:54-97`). It overwrites the Unit Action Lines load+test
at `0x6DBE74..0x6DBE7B` and resumes at the `je 0x6DC3F3`, so the rings draw
**before** that early-out and independent of the Unit Action Lines option.
Per techno it skips only dead/limbo objects and allies other than the current
player; an own techno must be in `SW.Designators`, a hostile techno in
`SW.Inhibitors`. The radius is `DesignatorRange.Get(Sight)` for an own
designator and `InhibitorRange.Get(Sight)` for a hostile inhibitor, the centre
is `GetCenterCoords()` with `Z = GetCellFloorHeight`, the colour is the owner
house's, and the call is `Game::DrawRadialIndicator(false, true, coords,
color, radius, false, true)`.

## Where VERA20K owns it

- `app/persistence/phobos_settings.rs`: the `[Phobos]` client switch.
- `rules/phobos_globals.rs`: the `[AudioVisual]` rules switch.
- `rules/superweapon_type.rs`: the per-superweapon switch.
- `app/presentation/render/mod.rs`: combines the client and rules switches
  into one bool, mirroring the power-plant enhancer gate.
- `app/presentation/radial_indicators.rs`: the `build` arm (before the Unit
  Action Lines early-out) and `push_designator_rings`, a presentation-side
  enumerator over `sim.entities()`. It checks the armed superweapon's switch,
  reads the `SuperWeaponInfluence` lists and reuses the shared
  `RadialIndicatorDraw`/`radius_pixels` pipeline, `HouseColorMap` and the
  `ground_pose::query_ground_height` floor query.

The Ares launch-validation scan `sim/superweapon/targeting.rs::scan_influences`
is left untouched; its owner-less `RangedSite` is built for launch validation,
not presentation.

## Residuals

- The hook reads only the explicit `SW.Designators`/`SW.Inhibitors` lists; it
  ignores `SW.AnyDesignator`/`SW.AnyInhibitor` (unlike the launch scan), and
  applies none of the launch scan's health/deactivated/building-power gates.
- Centre Z is sampled through the canonical `GetCellFloorHeight @ 0x00578080`
  ground query (`ground_pose::query_ground_height`) with an isolated cell
  identity so rendering does not stamp the sim's shared dummy; absent terrain
  or an unsupported slope retains the coordinate helper's Z.
- Without `ares` the nullable `DesignatorRange`/`InhibitorRange` keys are
  unread, so every ring falls back to `Sight`.
- The client switch arrives from the imported profile; a default Phobos
  profile (`no`) draws nothing, matching VERA's current behaviour.