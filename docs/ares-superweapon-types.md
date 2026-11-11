# Optional Ares handlers for the stock Chronosphere, Nuke, Dominator and Genetic Mutator

Requires an explicit `--features ares` build. Default builds do not read or execute
these extensions; see [build capability](ares.md).

```ini
[ChronoSphereSpecial]
Chronosphere.KillTeleporters=yes
Chronosphere.KillCargo=yes
SW.Range=5,5
SW.PostDependent=ChronoWarpSpecial

[CLEG]
Chronoshift.Allow=no

[NukeSpecial]
Nuke.Payload=NukePayload
Nuke.TakeOff=NUKETO
Nuke.PsiWarning=PSIWARN
Nuke.SiloLaunch=yes
SW.Damage=1500
Light.Red=120

[NUKE]
NukeFlash.Duration=45
PreImpactAnim=NUKEBALL

[PsychicDominatorSpecial]
Dominator.FireAtPercentage=50
Dominator.PermanentCapture=no
Dominator.CaptureImmuneToPsionics=no
SW.Deferment=30

[GeneticConverterSpecial]
Mutate.IgnoreCyborg=yes
Mutate.KillNatural=no
```

The ports follow the installed Ares 3.0 bodies cited beside the Rust (`Misc/SWTypes/`
handlers and the `Ext/SWType` hooks); no Ares DLL is loaded.

## The redirect

In an Ares build the stock `Type=` ChronoSphere, ChronoWarp, MultiMissile,
PsychicDominator and GeneticConverter are handed to their NewSWType handlers, on the
dispatch [the common keys](ares-superweapon-common.md) introduced: each handler's
Initialize sets its defaults on the first rules pass with the section, its LoadFromINI
reads its keys, the custom action replaces `Action=`, `PreClick`/`PostClick` come from
the handler's Flags and `PreDependent` is -1. Its Activate replaces the stock Launch
case; a refused Activate (the ChronoWarp without a ChronoSphere source, the Nuke without
a silo or payload) falls through to the stock case. The common Launch then counts the
shot, plays the type's `EVA.Activated`, settles `Money.Amount`, builds `SW.Animation`,
plays `SW.Sound`, raises the radar event and prints `Message.Launch`, each unless the
Flags skip it, and remembers the House's last Super (SWLastIndex), which a PostClick
Super uses as its source.

The other stock Types (Iron Curtain, Force Shield, the paradrops, Psychic Reveal) keep
their stock owners in this port; parts 2 and 3 below add the Spy Plane and the
Lightning Storm.

## Chronosphere

The ChronoSphere stores the cell and loops `SW.Animation` (else `ChronoPlacement=`)
`SW.AnimationHeight` (default 5) over it, shown to the owner's team; the player's click
selects `SW.PostDependent` when it names a ChronoWarp, else the first ChronoWarp. Its
Flags skip the common anim, EVA, money, radar event, message and cleanup.

The ChronoWarp warps from the House's last Super, which must be a ChronoSphere, with
that type's keys: the radar events, `Chronosphere.BlastSrc`/`.BlastDest` (else
`ChronoBlast=`/`ChronoBlastDest=`), and every Techno on its `SW.Range` (default 3x3)
admitted by `SW.AffectsHouse`/`SW.AffectsTarget` (default infantry and units) and
`Chronoshift.Allow`. Immobilized, airborne and warping Technos, `Warpable=no` ones
without `Chronosphere.AffectsUnwarpable`, curtained ones without
`.AffectsIronCurtain` and Units in a land war factory stay. `Organic=` ones die under
`.KillOrganic` (Teleporters only with `.KillTeleporters`); `.KillCargo` kills the
passengers of the others. A Foot's destination is its own coordinates moved by the
target's cell offset; its locomotor's track is forced to its own coordinates, which
leaves a Drive's destination at the source block once the warp ends, so the Unit drives
back (see part 2).

## Nuke

A charged Nuke fires from the owner's first powered `NukeSilo=` launch site within
`SW.RangeMinimum`/`.RangeMaximum` (unless a one-time grant or `Nuke.SiloLaunch=no`):
the silo's missile shows `Nuke.PsiWarning` (none: no warning) and `Nuke.TakeOff` (else
`NukeTakeOff=`). Otherwise `Nuke.Payload` drops at once over the cell. Allied clients
see the radar event; `SW.ActivationSound` (else `DigSound=`) plays. The falling nuke
uses the type's payload with `SW.Damage` and `SW.Warhead` when set.

Any bullet's impact follows its warhead's `NukeFlash.Duration` and `PreImpactAnim`
(the `NUKE` warhead defaults to 30 and NUKEBALL); a superweapon's nuke also rises to
the ground, raises its radar event and flashes for 30 frames by default unless
`Light.Enabled=no`. `PreImpactAnim.Moves` keeps the waiting bullet on its anim.

## Psychic Dominator

A charged Dominator starts its state machine after `SW.Deferment` frames:
`Dominator.FirstAnim` (else `DominatorFirstAnim=`) `Dominator.FirstAnimHeight` (750)
over the cell, `SW.ActivationSound`, `Message.Activate`; at
`Dominator.FireAtPercentage` of its frames the strike: `Dominator.Ripple`'s screen
ripple, `Dominator.SecondAnim`, GetDamage/GetWarhead (`SW.Damage`/`SW.Warhead`, else
`DominatorDamage=`/`DominatorWarhead=`) and, with `Dominator.Capture`, every Techno on
`SW.Range` (default the `DominatorCaptureRange=` spread) that the SW filters and
`.CaptureMindControlled`, `.CapturePermaMindControlled`, `.CaptureImmuneToPsionics`
admit joins the firer, permanently with `.PermanentCapture`, ringed by
`.ControlAnim`. A second launch while one runs prints `Message.Abort`
(`Msg:DominatorActive`). Its `Light.*` tints the scenario while it runs:
`Light.Enabled=no` keeps the normal light, `Light.Ambient` and `Light.Red`/`.Green`/
`.Blue` replace the Dominator light's values. A nuke flash takes its type's `Light.*`
the same way when it starts.

## Genetic Mutator

With `Mutate.Explosion` (else `MutateExplosion=`) one area hit of `SW.Damage`
(10000); otherwise every infantryman on `SW.Range` (default 3x3) takes its Strength:
`C4Warhead=` for `Natural=` ones with `Mutate.KillNatural`, the warhead for the others,
sparing `Cyborg=`/`NotHuman=` ones under `Mutate.IgnoreCyborg`/`.IgnoreNotHuman`. The
common Launch builds `SW.Animation` (else `IonBlast=`) and plays `SW.Sound` (else
`GeneticMutatorActivateSound=`).

## Not included

The nuke flash's later relights (CurrentNukeType) are not applied. The other stock redirects, `NoCursor`/`Cursor` and the Ares trigger
events remain outside this port; part 2 below lists what it adds and what still stays out.

## Part 2: building chronoshift, make-room, Spy Plane

```ini
[General]
ChronoInfantryCrush=no

[ChronoSphereSpecial]
SW.AffectsTarget=all
Chronosphere.AffectsUndeployable=yes
Chronosphere.BlowUnplaceable=no

[GAPILE]
Chronoshift.IsVehicle=yes

[HTNK]
Chronoshift.Crushable=no

[SpyPlaneSpecial]
SpyPlane.Count=2
SpyPlane.Type=SPYP
SpyPlane.Mission=SpyplaneApproach
```

**Buildings.** A building the ChronoWarp's filters admit (`SW.AffectsTarget` with
buildings, `UndeploysInto=` ones under `Chronosphere.AffectsUndeployable`, which
place like vehicles, or `Chronoshift.IsVehicle` ones under
`.ReconsiderBuildings` while units are affected) empties its Tank Bunker, unlinks
its contacts, loses its passengers under `.KillCargo`, stops being a target (bullets
lose it), counts as warped out (its AI freezes) and goes offline. `ChronoDelay`
frames after the launch it leaves the map and comes back facing as before on the
first cell around its target where it can be placed (only the target itself for a
building, ten rings for a vehicle); otherwise it returns to where it stood and, as
a vehicle or under `.BlowUnplaceable` (the default), takes its Strength as
`C4Warhead=` damage credited to the firer. A building held by a Temporal erase stays
removed.

**Making room.** A warping object kills what stands on its landing cell as before,
but a `Chronoshift.Crushable=no` object is protected like an Iron Curtained one (the
arrival dies instead), and with `[General] ChronoInfantryCrush=no` arriving infantry
dies on anything that is not infantry.

**Units drive back.** The Ares warp forces a Unit's Drive or Ship track to the
coordinates it warps from. Nothing in the Teleport states, their END or the Foot AI
resets the stashed locomotor, so once it is active again its Process paths to the
source cell and the Unit drives back unless the source is unreachable; Ares bug
report 896231 on Launchpad describes chronoshifted tanks going back. VERA keeps this.

**Spy Plane.** `Type=SpyPlane` goes to the Ares handler: a charged launch sends
`SpyPlane.Count` (1) planes of `SpyPlane.Type` (`SPYP`) in `SpyPlane.Mission`
(SpyplaneApproach) at the cell, raises no radar event and plays `EVA_SpyPlaneReady`
as its ready line.

**Unit Delivery.** A `Deliver.BaseNormal=no` building reserves no base space for the
computer's planning, and placing it does not extend the base either.

### Still not included

- EMPulse and `Type=DropPod` (part 4 adds EMPulse and the DropPod readers).
- During a building chronoshift its light stays on, a cloak generator keeps cloaking
  and `AboutToChronoshift` has no reader (Ares's own CanWarpTarget is not ported).
- An unknown `SpyPlane.Type` sends nothing (natively it reads out of range), and an
  off-map target cell is not replaced by the shared dummy cell.

## Part 3: Lightning Storm

```ini
[LightningStormSpecial]
SW.Deferment=100
SW.Damage=300
SW.Range=8
Lightning.Duration=360
Lightning.HitDelay=6
Lightning.ScatterCount=3
Lightning.Clouds=WCCLOUD1
Lightning.BoltExplosion=EXPLOLB
Lightning.RadarOutage=120
Lightning.RadarOutageAffects=all
Light.Red=40

[GAPOWR]
LightningRod=yes
LightningRod.Modifier=0.5
```

`Type=LightningStorm` goes to the Ares handler: its Initialize sets
`Lightning.ScatterCount` 1, `.DebrisMin`/`.DebrisMax` 2/4,
`.RadarOutageAffects` enemies, the EVA lines (`EVA_LightningStormReady`,
`EVA_LightningStormCreated`, `EVA_WeatherDeviceReady`) and the messages
(`TXT_LIGHTNING_STORM_APPROACHING` for Launch, `TXT_LIGHTNING_STORM` for
Activate, `Msg:LightningStormActive` for Abort). A storm that rages or counts
down refuses a launch, printing `Message.Abort` for its owner. A launch makes
the Super the current storm and starts it with `Lightning.Duration` (else
`LightningStormDuration=`) after `SW.Deferment` (else `LightningDeferment=`);
the common launch speaks `EVA.Activated` but prints no message and raises no
radar event.

While that storm is current the stock storm's Start, Process, CreateCloudBolt
and GroundStrike follow the type's keys, and every draw is `RandomRanged`:

- The countdown prints `Message.Launch` every 225 frames under
  `Lightning.PrintText` (else `LightningPrintText=`), with no EVA line.
- The start blacks out the radar of every House `Lightning.RadarOutageAffects`
  names, for `Lightning.RadarOutage` (else `LightningStormDuration=`) frames,
  prints `Message.Activate`, plays `SW.ActivationSound` (else `StormSound=`)
  and, under `SW.CreateRadarEvent`, raises a radar event. Its `Light.*`
  tints the storm's light.
- A cloud gathers over the cell every `Lightning.HitDelay` frames and a
  scattered one every `Lightning.ScatterDelay` frames, with up to
  `Lightning.ScatterCount` tries inside `SW.Range` (else a square of
  `LightningCellSpread=`; a height above 0 also limits the distance). A try
  on a cell whose building is a `LightningRod=` type, or next to one in the
  same cell, strikes that rod unless `Lightning.IgnoreLightningRod`; a try
  within `Lightning.Separation` cells of a cloud fails.
- Clouds are `Lightning.Clouds` at `Lightning.CloudHeight` over the cell (a
  negative height takes the first bolt's half image height). Strikes take
  `Lightning.Bolts`, `Lightning.Sounds`, `SW.Damage` (times a struck rod's
  `LightningRod.Modifier`) and `SW.Warhead`, explode with
  `Lightning.BoltExplosion` (for that warhead, from any weapon while the storm
  is current), and throw `Lightning.DebrisMin`..`DebrisMax` pieces of
  `Lightning.Debris`. Unset lists fall back to the `[General]` ones.

In an Ares build every storm, a trigger's too, strikes a cloud from half its
frames on and in list order; a trigger's storm otherwise keeps the stock
rules. A nuke flash with a duration below -1 never ends.

### Still not included after part 3

- The building chronoshift's light and cloak generator, `AboutToChronoshift`
  and Ares's CanWarpTarget (see part 2's list); EMPulse and DropPod are
  part 4's.
- A palette built while a storm, nuke flash or Dominator is active takes the
  stock colours, not the type's `Light.*` (hook `0x555E50`).

## Part 4: EMPulse and the DropPod readers

```ini
[SuperWeaponTypes]
0=EMPulseSpecial

[EMPulseSpecial]
Type=EMPulse
SW.MaxCount=2
SW.RangeMaximum=-1
EMPulse.Linked=no
EMPulse.TargetSelf=no
EMPulse.PulseDelay=32
EMPulse.PulseBall=PULSBALL
EMPulse.Cannons=NAEMPC

[NAEMPC]
EMPulseCannon=yes
SuperWeapon=EMPulseSpecial
Primary=EMPulseShell
```

**The stock EMP cannon.** gamemd still holds Tiberian Sun's EMP cannon as
the other arm of the Missile mission. With a firing type the building first
fires weapon 0 at the House's NukeTarget and queues Guard. An
`EMPulseCannon=` building then turns its body toward the House's EMP target
and its barrel to the shell's ballistic pitch, shows `PULSBALL` at its fire
coordinate, waits 32 frames, and fires weapon 0's shell from its turret
pivot. The shell starts at three quarters of the launch speed that gravity
gives for the distance; the arc is retried at 1.25 times that speed, else
the pitch is 0xFA00. Then a Turret= type's recoil arms and one Main draw
picks a `Report=` sound. Last, the barrel levels and the building goes back
to Guard. Retail data never gets there: only the MultiMissile launch
queues Missile, on a silo, and no retail type sets `EMPulseCannon=`.

**Type=EMPulse.** The handler starts with `SW.MaxCount` 1,
`SW.RangeMinimum` 0 and `SW.RangeMaximum` -1, the Attack cursor and no AI
mode. A launch site is a building of a type listed in `EMPulse.Cannons`
that is alive and powered, whatever it provides. Without that list it is
a providing `EMPulseCannon=` building. Each site's range is the type's, with
a negative maximum or minimum taken in whole cells from weapon 0's `Range=`
or `MinimumRange=`. Under `EMPulse.TargetSelf` no target is needed (no
action), and no site needs to be in range.
The launch sets the House's EMP target and walks the House's buildings. Up
to `SW.MaxCount` sites (negative: all) in range, or any under
`EMPulse.Linked`, each remember the Super and the cell and fire through the
Missile mission. Under `EMPulse.TargetSelf` each instead detonates weapon
0's bullet at its own coordinates. A listed building that is not an
`EMPulseCannon=` just fires weapon 0 at the cell and goes back to Guard. A
cannon shows the Super's `EMPulse.PulseBall` (none allowed) and waits its
`EMPulse.PulseDelay`. The common launch, money and trigger events 75/77
work as for the other new types.

**Type=DropPod** has its readers here; part 5 adds its launch.
`DropPod.Minimum`, `.Maximum`, `.Veterancy` (2.0) and `.Types` are read per
pass, and so are `[General] DropPodTrailer=`, `DropPodTypes=`,
`DropPodMinimum=` and `DropPodMaximum=`. Every type reference is a Find; an
unknown name is skipped. The handler sets its EVA lines and the DropPod AI
mode.

### Still not included after part 4

- `Type=DropPod`'s launch: see part 5.
- An EMP cannon whose weapon 0 is `IsLaser=` aims its barrel by vt+0x2C8's
  laser arm (`0x0043E968`); VERA uses the EMP arm. When no arc reaches the
  cell, the stock barrel takes an uninitialized word; VERA's barrel counts
  as aimed.
- A cannon without weapon 0 crashes the stock game (`0x0043E95A`); VERA
  parks it. A stock `PULSBALL` that does not exist is read out of bounds;
  VERA shows nothing. A stock weapon with no `Report=` divides by zero;
  VERA plays nothing (as Ares's hook 44D4CA does).
- An occupied cannon's turret pivot takes the unoccupied arms: the
  occupant's port is not read.
- The building constructor's barrel sets (`0x0043BA5E`, `0x0043BACB`) are
  not ported; the barrel starts where Unlimbo leaves it.
- A TargetSelf detonation that removes one of the House's buildings during
  the walk skips the next building in native (the vector shifts); VERA
  does not skip it.
- The palette `Light.*` (hook `0x555E50`) is still not applied (see part
  3).

## Part 5: the DropPod launch

```ini
[General]
DropPodTypes=E1,MTNK
DropPodMinimum=2
DropPodMaximum=4
DropPodTrailer=SMOKEY
DropPodWeapon=Vulcan2
DropPodHeight=2000
DropPodSpeed=75
DropPodAngle=0.79
DropPod=DROPPOD,DROPPOD2

[AudioVisual]
DropPodPuff=DROPEXP
AtmosphereEntry=PODRING

[DropPodSpecial]
Type=DropPod
DropPod.Types=MTNK
DropPod.Veterancy=1.0
```

`Type=DropPod` now dispatches to SW_DropPod::Activate (`10070E40`,
`sim::superweapon::drop_pod`). It refuses without types (`DropPod.Types`,
else `DropPodTypes=`) and otherwise always activates. One Scenario draw picks
the count between `DropPod.Minimum`/`.Maximum` (else the `[General]` pair);
each of three tries per unit draws a type (a BuildingType ends the drop),
creates it for the firer with at least `DropPod.Veterancy` (capped by
`VeteranCap=`), takes the nearby Wheel cell of the current cell
(`Find_Nearby_Passable_Cell @ 0x0056DC20`) and lands it there with
TechnoExt::CreateWithDroppod (`10044030`) when the cell is open to it. A
second draw then picks where the walk over that cell's eight neighbours
starts; the first one the unit may enter becomes the current cell. A unit
still in limbo is deleted.

CreateWithDroppod gives the Foot gamemd's DropPod locomotor, piggybacked
over its own (VERA keeps the pod as the Foot's `DropPodFlight`; while it is
set, FootClass::AI's Process is the pod's), publishes the cell as NavCom,
queues Guard and faces north. The pod's Move_To (`0x004B6040`) puts the Foot
`DropPodHeight=` (1500) above the ground, run back along `DropPodAngle=`
(pi/2, read clamped into [pi/8, 3pi/8]) toward +X, or toward -X when that
point is off the playfield, and shows `AtmosphereEntry=`. Its Process
(`0x004B5B70`) moves the Foot `max(height / 10 + 2, DropPodSpeed=)` (40) a
frame along the angle; every third frame a `DropPodWeapon=` strafes a point
near the cell for twice its Damage unless an ally stands there. On the
ground the Foot leaves limbo again at its Location with `DropPodPuff=` and
`DropPod=` item 0 (item 1 for a far-side entry), idles, commences its
mission and scatters; a refused Unlimbo blows 100 `C4Warhead=` and leaves it
in limbo. The stock RulesClass keys are kept only in an Ares build: no stock
path installs the DropPod locomotor.

A computer House fires it through the Ares AI's DropPod mode (PickTarget
`10038640`, case 0x13): one Scenario `RandomRanged(1, 4)` picks a zone
(north, east, south, west), `HouseClass::PickRandomCellInZone @ 0x00501AC0`
a random cell in it (`RandomRanged(radius, 2 * radius)` leptons off the
base, the radius clamped into 0x300..0x800, along a facing a raw draw puts
within 0x2000 of the zone's, clamped into the playfield), and the nearby
Foot cell there (`Find_Nearby_Passable_Cell`, 1x1, bridges allowed) is the
target unless it is empty or CanFireAt refuses it. Zone 0 of the same port
is the one the base-return search (House500200) uses.

| gamemd hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| `0x4B5EB0` | `1005C8B0` | DropPodLocomotionClass_ILocomotion_Process_Smoke | Ported: `DropPodTrailer=` (unset: SMOKEY) every 6 frames with or without a weapon. |
| `0x4B5F9E` | `1005C890` | DropPodLocomotionClass_ILocomotion_Process_Report | Ported: no report sound for a weapon without `Report=`; the Main draw stays. |
| `0x4B619F` | `1005C870` | DropPodLocomotionClass_ILocomotion_MoveTo_AtmosphereEntry | Ported: no AtmosphereEntry anim when unset. |

### Still not included after part 5

- A negative DropPod count loops about 2^32 tries natively; VERA makes none.
- CreateWithDroppod's SetDestination runs only the Foot tail (NavCom and the
  pod's Move_To), not the class setter's prologue; a `Teleporter=` Unit's
  setter arm is not taken. Its See(0, 0) repeats the Unlimbo's reveal.
- While the pod flies, Is_Moving, the layer and a mission's Move_To reach the
  Foot's own locomotor, which natively stays suspended and unasked; a Guard
  that finds a target mid-air can leave a Drive destination behind.
- A landing past the end of `DropPod=` (no item 0, or no item 1 for a
  far-side entry) reads out of bounds natively; VERA shows no pod.

