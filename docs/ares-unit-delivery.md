# UnitDelivery: ra2-mono port and host integration

```ini
[SuperWeaponTypes]
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

Register the payloads in their usual InfantryTypes, VehicleTypes, AircraftTypes
or BuildingTypes lists. A building providing this superweapon, or an ordinary
trigger grant, still supplies its retained House/Super instance. This extension
does not grant a superweapon just because its section exists.

## Provenance

This is a port from the MIT-licensed ra2-mono code, with the notice retained in
`src/sim/superweapon/unit_delivery.LICENSE`:

- `GameLogicManager.Cmd.Superweapon.Custom.cs`:
  `TryQueueUnitDeliverySuperweapon`, `ApplyPendingSupportDelivery`,
  `ResolveSupportSuperweaponDeferment`, `ApplyUnitDeliveryMission`, owner
  resolution, and the nearest-valid-building-anchor decision.
- `GameLogicManager.SpawnPlacement.cs`: `ResolveSupportSpawnRingOffset` and
  nearest-ring valid-candidate selection with the wrapping `seed * 397 ^ radius`
  selection rule.
- `Domains/SupportSuperweapons/SuperweaponStateTypes.cs`,
  `SuperweaponDomainService.cs` and its `RuntimeSnapshot` partial: pending
  delivery state, reverse-order due-effect processing, persistence and hashing.
- `GameLogicManagerTriggerSuperweaponTests.cs`: vehicle deferment, human
  AreaGuard, AI Hunt, blocked-site fallback and building delivery scenarios.

The code is adapted to VERA's owners, not a second C#/ECS simulation or an
imported Ares C++ implementation. The tag contract was checked against the
[Ares UnitDelivery documentation](https://ares-developers.github.io/Ares-docs/new/superweapons/types/unitdelivery.html).

## Supported behavior

- `Deliver.Types` retains duplicates and resolves registered, category-distinct
  payload identities during ordered rules processing. Missing layer members
  retain prior values; `none` clears the list. Unknown or cross-category
  ambiguous tokens are omitted rather than bound to a different object type.
- `SW.Deferment` defaults to 20 frames; zero places immediately. Negative values
  clamp to zero. Delayed entries freeze their payloads, resolved owner, target,
  placement settings and deadline. They use the simulation's wrapping binary
  frame clock, never wall time. Due entries are visited newest-first, as in
  ra2-mono's support queue, before the ordinary object visits in VERA's existing
  global superweapon effect phase.
- `Deliver.Owner` supports invoker, Neutral, Special, and the first country of
  the Civilian side. It resolves to an existing House, never constructs an
  invented owner. An absent required owner rejects the command.
- Infantry, vehicles, aircraft and buildings go through the existing constructor,
  class entry and Unlimbo owners. Placement tries the target, then legal nearby
  rings bounded by the resident map dimensions. Mobiles select among valid
  candidates in the nearest ring; buildings select the closest legal anchor,
  with perimeter order breaking ties. One payload's failure does not prevent
  later payloads from being attempted. Exhausted placement discards the held
  constructor and its tracking through the existing cleanup owner.
- Constructors, tracking, occupancy, sensors, power, building discovery and
  dirty House tails remain VERA-owned. Units receive the existing AreaGuard
  mission for human owners and Hunt for AI owners; buildings stand finished on
  Guard. Missions use the rules-aware queue/readiness owner.
- Aircraft start at ground height, unlike the original C# altitude-based
  aircraft spawn. VERA's existing flight/jumpjet behavior owns any later takeoff.
  Infantry use VERA's existing three functional spots, not ra2-mono's five-slot
  model. Naval/land placement uses the host's normal terrain restrictions.
- `Deliver.BaseNormal=no` is a per-building override checked by the common
  preview/command build-area query, including ally build-off. It does not mutate
  the shared BuildingType. It survives save/load and capture.
- `SW.ActivationSound` emits the existing positional Voc event once when the
  pending delivery is processed. The ordinary launch event is emitted at
  acceptance, so selection and charge do not wait for the delivery.

## Persistence, safety and differences

The queue, every future-affecting pending field, and the per-building override
are serialized and hashed. Empty queues and default building flags add no stock
hash input. Resolved extension settings participate in the rules configuration
hash. Snapshot version 434 advances to 435 because all three positional record
layouts change; old-version saves are rejected.

Malformed/empty payloads, invalid target cells, missing delivery owners, and
unsupported pre-click/post-click/charge-drain settings are refused before
paired-Super mutation, charge or effects. AI rejects malformed settings before
target-search RNG. A valid launch with no legal site still consumes its charge;
it has no retry-after-completion queue. One-time grants are consumed at accepted
launch, and their already-queued effect still completes once.

Placement uses VERA's pinned portable hasher over owner/Super identity, target,
issue frame, constructor ID and payload index for the C# ring selector's seed.
It is deterministic in this engine, not a claim of Ares's random stream, exact
draw count or exact placement order. Normal constructors and infantry placement
retain their existing RNG draws. This extension adds no independent mutable RNG.
Candidate checks and final placement share the host's entry owner; final
admission is rechecked rather than trusted from a cached search result.

The generic native Action table has no `Custom` entry: only UnitDelivery's
missing/Custom action falls back to the existing ParaDrop target cursor. The
extension-only dispatch tag 0x101 is outside the 12 stock slots; it is not a
gamemd Type index or a valid native PreDependent. No hardcoded EVA cue or stock
payload change is introduced. Extension types are excluded from the stock
list-slot detection EVA table even when authored in an early list position.

Not included: the other unmerged Ares contributions, `Money.Amount`, country or
building-availability extensions, custom `SW.AITargeting` modes, designators,
inhibitors, `SW.Animation` or other generic Ares presentation settings. AI uses
the existing ParaDrop-style target arm. No complete MO skill or mod is certified
by this payload mechanism; for example PaladinAidSpecial also requires other
common Ares settings.

Tests in `sim::superweapon::unit_delivery::tests` exercise production rules,
command dispatch, effect-phase timing, placement, missions and binary state
round trips with synthetic maps. They are not a live playtest, a retail loader
smoke test, an Ares executable comparison, or a full-suite result.
