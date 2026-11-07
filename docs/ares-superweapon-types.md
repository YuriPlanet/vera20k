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

The other stock Types (Iron Curtain, Force Shield, Lightning Storm, the paradrops, Spy
Plane, Psychic Reveal) keep their stock owners in this port.

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

The Lightning Storm's `Light.*` and the nuke flash's later relights (CurrentNukeType)
are not applied. The other stock redirects, `NoCursor`/`Cursor` and the Ares trigger
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

- Lightning Storm redirect: the `Lightning.*` keys (Duration, RadarOutage, HitDelay,
  ScatterDelay, ScatterCount, Separation, PrintText, IgnoreLightningRod, DebrisMin,
  DebrisMax, CloudHeight, BoltExplosion, RadarOutageAffects, Clouds, Bolts, Debris,
  Sounds), `LightningRod.Modifier` and the storm's `Light.*`: the stock storm runs.
- EMPulse (`EMPulse.Linked`, `.TargetSelf`, `.PulseDelay`, `.PulseBall`, `.Cannons`).
- `Type=DropPod` and the `DropPod*` RulesExt keys: it needs the DropPod locomotor
  and CreateWithDroppod.
- During a building chronoshift its light stays on, a cloak generator keeps cloaking
  and `AboutToChronoshift` has no reader (Ares's own CanWarpTarget is not ported).
- An unknown `SpyPlane.Type` sends nothing (natively it reads out of range), and an
  off-map target cell is not replaced by the shared dummy cell.
