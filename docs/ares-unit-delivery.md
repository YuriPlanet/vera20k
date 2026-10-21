# Optional Ares UnitDelivery

Requires an explicit `--features ares` build. Default builds do not read or execute
these extensions; see [build capability](../README.md#customization).

```ini
[SuperWeaponTypes]
; Use the next unused list index in your mod.
0=ReinforcementSpecial

[ReinforcementSpecial]
Type=UnitDelivery
Action=Custom
IsPowered=no
RechargeTime=1
Deliver.Types=MTNK,MTNK
SW.Deferment=20
Deliver.Owner=invoker
Deliver.BaseNormal=yes
; SW.ActivationSound=SomeSound
```

Register payloads in the normal InfantryTypes, VehicleTypes, AircraftTypes or
BuildingTypes lists. A provider building or a normal trigger grant must supply
the retained superweapon instance. This extension does not grant itself.

## Provenance

Original implementation by **baixiong443**, who attributes the delivery queue,
placement selector and scenarios to their earlier ra2-mono work and contributes
this change under VERA's GPLv3. The donor repository was unavailable during
review; its source and history have not been independently verified.

The implementation uses VERA's existing state and lifecycle owners. The
[Ares UnitDelivery documentation](https://ares-developers.github.io/Ares-docs/new/superweapons/types/unitdelivery.html)
and [Ares activation source](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Misc/SWTypes/UnitDelivery.cpp)
provide compatibility references. No Ares DLL or C# simulation is loaded.

## Supported behavior

- `Deliver.Types` preserves duplicates and resolves existing, category-distinct
  types at the ordered rules read. Omitted or empty reads retain prior values;
  `none` clears the list. Unknown or ambiguous names are skipped.
- `SW.Deferment` defaults to 20 frames. Negative values use the same 20-frame
  fallback; zero delivers immediately. Pending requests freeze the payload,
  resolved owner, target, settings and deadline. They use the wrapping simulation
  frame clock and run newest-first in the global superweapon-effect phase.
- `Deliver.Owner` accepts invoker, Neutral, Special and Civilian. These resolve
  existing Houses through the shared House owner; Civilian selects the first
  matching House in House order. A missing required House refuses admission.
- Infantry, vehicles, aircraft and buildings use normal constructor, entry and
  Unlimbo owners. Nearby placement is bounded by map dimensions; each payload
  gets an attempt, and an exhausted search disposes its held constructor through
  the normal cleanup owner. Building searches center the footprint on the target.
- Ground aircraft candidates also pass the shared ground terrain/occupation
  check. Foot placement retains the chosen bridge plane, deck coordinate and
  occupancy. Existing class-specific placement rules still apply.
- Human-owned mobiles receive AreaGuard; AI-owned mobiles receive Hunt. This
  follows the published Ares 3.0/donor behavior; current Ares source uses Guard
  for humans. Buildings finish on Guard. BalloonHover/JumpJet types receive the
  shared post-placement Scatter, allowing their normal flight owner to lift off.
- `Deliver.BaseNormal=no` is an instance flag read by the common build-area
  query, including ally build-off. It survives capture and save/load and does
  not change the shared BuildingType.
- `SW.ActivationSound` resolves against the fixed SOUNDMD catalog. Unknown names
  retain a prior valid cue; an explicit `none` clears it. The positional cue is
  emitted once before payload placement when the request becomes due. The normal
  launch event occurs at acceptance, so charge and selection cleanup do not wait.

## Persistence and compatibility limits

The private pending queue and per-building flag are saved and hashed. Empty
queues/default flags add no stock hash input. Resolved extension settings enter
the rules hash. The changed positional rules, simulation and entity records change
the save layout id, so older saves are rejected.

Empty payloads, invalid targets, missing owners and unsupported charge drain
refuse before charge or effects. The type remains one-click even if PreClick or
PostClick is authored. Missing/Custom Action is normalized once to ParaDrop for
both cameo admission and cursor display. Explicit other actions retain their
normal meaning. Extension identifiers cannot enter the stock EVA table or serve
as a native PreDependent type.

Availability composes with the shared native/Ares building gates. An accepted
request remains frozen if a provider is later lost. A valid launch with no legal
placement consumes its charge and has no retry queue. One-time grants are consumed
at acceptance; their queued effect still completes once.

Placement retains the contribution's deterministic ring selector, using VERA's
portable hasher over stable simulation inputs. This is a VERA adaptation, not a
claim of Ares's exact placement order or RNG stream. Initial delivered facing is zero; Ares derives it from the cell index. Existing
constructors, infantry placement and Scatter retain their own simulation RNG draws. Full-map
search cost for large blocked payloads has not been profiled; no 20,000-unit
performance claim is made for this extension.

Money.Amount, custom AI modes, designators/inhibitors, Deliver.Buildups and generic
Ares animation/visual settings remain outside this feature. AI uses the existing
ParaDrop-style target arm. The implementation and synthetic regression scenes do
not establish full Mental Omega compatibility or executable Ares parity.
