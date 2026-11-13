# Optional Ares superweapon common keys, Battery, Protect and SonarPulse

Requires an explicit `--features ares` build. Default builds do not read or execute
these extensions; see [build capability](ares.md).

```ini
[SuperWeaponTypes]
0=PowerCell
1=TeamShield

[PowerCell]
Type=Battery
RechargeTime=2
Battery.Power=150
Battery.KeepOnline=GAPOWR
SW.ChargeToDrainRatio=0.5
Money.DrainAmount=-25
Money.DrainDelay=60
SW.Unstoppable=yes
SW.AlwaysGranted=yes

[TeamShield]
Type=Protect
RechargeTime=3
Protect.Duration=450
Protect.PowerOutage=300
SW.Range=5,5
SW.AffectsHouse=team
Message.Launch=MSG:ShieldUp
Text.Ready=TXT_RELEASE
```

The ports follow the installed Ares 3.0 bodies cited beside the Rust (`SWTypeExt`
reader `0x10033370` and its hooks); no Ares DLL is loaded.

## Common SWTypeExt keys

- `SW.ChargeToDrainRatio` replaces `[General] ChargeToDrainRatio=` for the type in
  the charge-drain ClickFire (`0x10035130`) and the cameo AnimStage (`0x10035420`).
  Ares builds also clamp the stage at 54 (`0x100354C0`) and let computer Houses
  toggle a drain like human ones.
- `Money.DrainAmount` / `Money.DrainDelay` transact while a drain runs, every
  `DrainDelay` frames left on its timer; an unaffordable amount ends the drain
  (`0x100358A0`). A human House hears `EVA.InsufficientFunds` and sees
  `Message.InsufficientFunds` on every client.
- `SW.Unstoppable` refuses a sidebar click on a draining type (`0x10034DA0`).
- `SW.AlwaysGranted` grants an available type without a provider building
  (`0x10038F10`); the availability gates still apply. Grants under low power hold
  only `IsPowered=yes` types.
- The House Super pass is Ares's (hook `0x50AF10`, `0x10039450`): a provider
  powers its Supers only online, not under EMP and operated, and charges them
  only when also not warped out and not building up or selling (`0x10038F10`).
  An `IsPowered=yes` type is held without a powering provider or on low power,
  any other type without a charging one, which the stock pass never holds.
- `SW.CreateRadarEvent` adds the type-13 radar event to a new type's launch
  (`0x10032ED0`).
- `SW.Group` selects the types Ares's script action 55 fires (`0x10039EF0`): the
  first `SW.AITargeting=IronCurtain` type whose group equals the argument.
- `EVA.Impatient` plays on a press of a cameo that cannot fire; `EVA.SelectTarget`
  replaces the stock targeting line.
- `Message.Detected`, `.Ready`, `.Launch`, `.Activate` print at the detection,
  readiness, launch and UnitDelivery arrival hooks, in the Ares colour rules.
- `Text.Preparing`, `.Hold`, `.Ready`, `.Charging`, `.Active` replace the cameo
  status labels of every type (`0x100356A0`); an empty label shows none.
- `SidebarPalette` names the palette the type's SHP cameo draws with; a file that
  does not load keeps CAMEO.PAL (hooks `6A9948`/`6A9A2A`). The `~~~` theater
  substitution is not applied.

## Battery, Protect and SonarPulse

`Type=Battery` is a charge-drain power with no target (`Action=None` and
`UseChargeDrain` are forced). While draining, its House counts `Battery.Power` as
auxiliary power, its `Battery.KeepOnline` building types work without power and its
`Battery.Overpower` types are overpowered; any active battery keeps radar buildings
working on low power. Clicking it off, the drain running out, a hold or loss
deactivates it.

`Type=Protect` is an Iron Curtain with its own duration, a `SW.Range` rectangle or
radius (default 3x3), `SW.AffectsHouse`/`SW.AffectsTarget` filters, the type's
`StartSound=`, a `SpecialSound=` fade (`Protect.PlayFadeSoundTime`) and an optional
`Protect.PowerOutage` blackout of its owner.

`Type=SonarPulse` restarts the affected Technos' sonar timer with the longer of
its time left and `SonarPulse.Delay` (default 60) and starts uncloaking any that
are cloaked; while the timer runs they cannot cloak. It reaches the `SW.Range`
area (default radius 10), or every Techno with a negative range, which also
needs no target. By default it affects enemies on water only.

## Not included

`Type=DropPod` (needs the DropPod locomotor), `NoCursor`/`Cursor`
(Ares `[MouseCursors]` cursor types), `Light.*`, `Message.Abort`
and the retail-type redirects (`IronCurtain`/`ForceShield` through Protect and the
other stock handlers) remain outside this port, as do the AI's Stealth and
IronCurtain target pickers (a computer House does not fire SonarPulse or
Protect on its own). The trigger events raised on activation and deactivation
are in [ares-triggers.md](ares-triggers.md).
