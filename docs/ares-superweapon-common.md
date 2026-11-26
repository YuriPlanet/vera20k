# Ares superweapon common keys, Battery, Protect and SonarPulse

On `Base-for-modders`, a superweapon can use these keys and the added Ares
`Type=` names with Ares Off. Install any assets named by the definition in
the mod's archives. See the [stock redirects](ares-superweapon-types.md#the-redirect).

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
  Ares games also clamp the stage at 54 (`0x100354C0`) and let computer Houses
  toggle a drain like human ones.
- The 3.0p1 SetOnHold hook (`0x6CB4D0`, `0x10036190`) converts a running
  drain into paused charge using the remaining drain time and the type's ratio.
  Releasing the hold normally restarts from the instance's recharge time,
  including a scenario override. The same timer behavior applies to an authored
  `UseChargeDrain=yes` type using the stock rule path.
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
- `SW.Group` selects the types Ares's script action 55 fires (3.0p1
  `0x006EFC70` → `0x1003AD70` → `0x1003A970`): the
  first `SW.AITargeting=IronCurtain` type whose group equals the argument.
- `EVA.Impatient` plays on a press of a cameo that cannot fire; `EVA.SelectTarget`
  replaces the stock targeting line.
- `Message.Detected`, `.Ready`, `.Launch`, `.Activate` print at the detection,
  readiness, launch and UnitDelivery arrival hooks, in the Ares colour rules.
- `Message.CannotFire` defaults to `MSG:CannotFire` (3.0p1 `0x100323A3`). It
  supplies the manual refusal when `SW.UseAITargeting` is active.
- `Text.Preparing`, `.Hold`, `.Ready`, `.Charging`, `.Active` replace the cameo
  status labels of every type (`0x100356A0`); an empty label shows none.
- `SidebarPalette` names the palette the type's SHP cameo draws with. The
  first `~~~` expands to the active theater's extension, such as `SNO`
  (3.0p1 `0x10077210`). An unavailable palette falls back to CAMEO.PAL.

## Battery, Protect and SonarPulse

`Type=Battery` is a charge-drain power with no target (`Action=None` and
`UseChargeDrain` are forced). While draining, its House counts `Battery.Power` as
auxiliary power, its `Battery.KeepOnline` building types work without power and its
`Battery.Overpower` types are overpowered; any active battery keeps radar buildings
working on low power. Clicking it off, the drain running out, a hold or loss
deactivates it. The 3.0p1 Activate entry (`0x1006FBD0`) requires a current grant.

`Type=Protect` is an Iron Curtain with its own duration, a `SW.Range` rectangle or
radius (default 3x3), `SW.AffectsHouse`/`SW.AffectsTarget` filters, the type's
`StartSound=`, a `SpecialSound=` fade (`Protect.PlayFadeSoundTime`) and an optional
`Protect.PowerOutage` blackout of its owner. The same handler accepts
`Type=IronCurtain` and `Type=ForceShield` as described in the
[redirect rules](ares-superweapon-types.md#iron-curtain-and-force-shield).
An omitted `Protect.Duration` uses `[CombatDamage] IronCurtainDuration`.
Force Shield uses its own `[General]` defaults and the receiver uses
`IronCurtain.Modifier` or `ForceShield.Modifier` according to the protection kind.
Authored ranges beyond the engine's cell-grid extent have no area effect.

`Type=SonarPulse` restarts the affected Technos' sonar timer with the longer of
its time left and `SonarPulse.Delay` (default 60) and starts uncloaking any that
are cloaked; while the timer runs they cannot cloak. It reaches the `SW.Range`
area (default radius 10), or every Techno with a negative range, which also
needs no target. By default it affects enemies on water only.

The `Stealth`, `MultiMissile` and `LightningRandom` modes use the 3.0p1
AI target picker. The DLL returns no cell for `IronCurtain` in this picker
(`0x100390E0`, mode `0xF`). Team script action 55 supplies the target for
that mode. See [trigger events](ares-triggers.md) and
[superweapon cursors](ares-cursors.md).
