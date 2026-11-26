# Ares UnitDelivery

`Base-for-modders` recognizes `Type=UnitDelivery` with Ares Off.

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
- `SW.Deferment` is the common superweapon read every Ares type shares. The
  3.0p1 integer parser reads `$30` as hexadecimal 48. An unreadable value
  keeps the previous layer's result. Unset and negative values wait 20 frames.
  3.0p1 `SW_UnitDelivery::Activate` (`0x100769D0`) queues the request even at
  zero delay. Each request retains its payload, Houses, target and deadline.
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
- After placement, AI-owned mobiles in AreaGuard switch to Hunt. Other missions
  retain the placement result. BalloonHover/JumpJet types receive the
  shared post-placement Scatter, allowing their normal flight owner to lift off.
- `Deliver.BaseNormal=no` is an instance flag read by the common build-area
  query, including ally build-off. It survives capture and save/load and does
  not change the shared BuildingType.
- `SW.ActivationSound` resolves against the fixed SOUNDMD catalog. Unknown names
  retain a prior valid cue; an explicit `none` clears it. The positional cue is
  emitted once at the target cell centre at Z=0 before placement. Activation text
  with `Message.FirerColor=yes` uses the firing House, including delivery to neutral
  or another House. The launch event occurs at acceptance.

## Persistence and compatibility limits

Accepted requests, including zero delay, complete at the next due Logic pass in
registration order (`0x0055AFB3` → `0x1006F150`). The pending queue stores the
firer separately from the recipient. The updated save layout rejects earlier
binary saves, and current saves resume pending requests with both Houses intact.

Empty payloads, invalid targets, missing owners and unsupported charge drain
refuse before charge or effects. The type remains one-click even if PreClick or
PostClick is authored. Missing/Custom Action is normalized once to ParaDrop for
cameo admission. The cursor is the handled type's SWTypeExt `Cursor` (Attack
by default, since `SW_UnitDelivery::Initialize` sets none) and `NoCursor`
(Disallowed); see [ares-cursors.md](ares-cursors.md). Explicit other actions retain their
normal meaning. Extension identifiers cannot enter the stock EVA table or serve
as a native PreDependent type.

Availability composes with the shared native/Ares building gates. An accepted
request remains frozen if a provider is later lost. A valid launch with no legal
placement consumes its charge and has no retry queue. One-time grants are consumed
at acceptance; their queued effect still completes once.

Placement follows PlaceUnits (`10075A60`): each payload, in `Deliver.Types` order,
asks MapClass::NearByLocation (Find_Nearby_Passable_Cell `0x56DC20`) once from the
target less half its extent. A building searches with its foundation (height with
its bib) as Track/Normal with the occupancy test (Ares' `buildable`), or Float
without it; a Foot type with its own SpeedType and MovementZone, an Aircraft as
Track/Normal, both 1x1. No zone, overlay, height or burrow test, no bridge cell and
no target cell, so the pick is the frame-counter modulo and draws no RNG; a
PlaceAnywhere building takes the anchor. One Put at the cell's coordinate, with
Ares' DirType (GetCellIndex & 7) << 5 of the cell, so the facing follows its X; a
refused Put UnInits the payload. Constructors, infantry placement and Scatter keep
their own simulation RNG draws.

3.0 reads no `Deliver.Buildups` (SW+0x4A8, default true): a building queues
Construction before its Put and builds up, is DiscoveredBy its owner and has its
ready byte (`+0x6DD`) set. A payload that is not powered (IsPowered `100454F0`) or
needs an absent operator (IsOperated `10045440`) is Deactivated, and a building's
house rechecks its tech tree.

The common [money](ares-superweapon-money.md) and
[targeting/presentation](ares-generic-warhead.md) owners also serve UnitDelivery.
Its default automatic mode is ParaDrop. The recipient determines missions:
an AI unit already in `AreaGuard` changes to `Hunt`, and other existing
missions are preserved (`0x10076E01..0x10076E19`). ChronoWarp and UnitDelivery
use separate queues in VERA, so simultaneous effects of the two types can
follow a different registration order from the DLL's shared queue.
