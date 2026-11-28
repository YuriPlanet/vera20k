# Passenger deletion (Soylent)

A transport type can eat the passengers it carries: one passenger disappears
after a countdown, optionally paying its owner's House. Phobos's affiliated
`PassengerDeletionTypeClass` (`Ext/TechnoType/Body.cpp:1389-1399`,
`New/Type/Affiliated/PassengerDeletionTypeClass.cpp`) exists only when a pass
sets a positive rate or a cost-derived one.

```ini
[TRUCK]                              ; TechnoType
PassengerDeletion.Rate=0             ; integer, default 0
PassengerDeletion.Rate.SizeMultiply=yes
PassengerDeletion.UseCostAsRate=no
PassengerDeletion.CostMultiplier=1.0
PassengerDeletion.CostRateCap=       ; integer, unset = no cap
PassengerDeletion.AllowedHouses=all
PassengerDeletion.DontScore=no
PassengerDeletion.Soylent=no
PassengerDeletion.SoylentMultiplier=1.0
PassengerDeletion.SoylentAllowedHouses=enemies
PassengerDeletion.DisplaySoylent=no
PassengerDeletion.DisplaySoylentToHouses=all
PassengerDeletion.DisplaySoylentOffset=0,0
PassengerDeletion.ReportSound=
PassengerDeletion.Anim=
PassengerDeletion.UnderEMP=no
```

## Behavior

`TechnoExt::EatPassengers` (`Ext/Techno/Body.Update.cpp:261-448`) runs from
`OnEarlyUpdate` after the AttachEffect update and before
`CheckDeathConditions`. In limbo, and under EMP or deactivation unless
`UnderEMP=yes`, the countdown pauses by moving its start forward one frame per
visit.

Otherwise it picks the **last** passenger in CargoClass list order whose House
the transport may target through `AllowedHouses`. With no such passenger the
clock stops. The first eligible visit starts the clock at
`Cost * CostMultiplier` (capped by `CostRateCap`) when `UseCostAsRate`, else at
`Rate` scaled by `int(Size + 0.5)` when the passenger is larger than 1. When the
clock runs out the passenger leaves the cargo list and:

- the `ReportSound` plays at the transport, and one Scenario draw picks an
  `Anim` type to create there with the transport as its owner;
- with `Soylent` and a `SoylentAllowedHouses` match, the transport's House
  receives the passenger's full refund plus each of its own passengers, its
  parasite and its hijacker, each times `SoylentMultiplier`; zero pays nothing.
  `DisplaySoylent` then queues the flying money label, filtered per client by
  `DisplaySoylentToHouses`, the transport's visibility and
  `DisplaySoylentOffset` (see `sim::flying_text`). The offset is a Phobos
  `Valueable<Point2D>`: `Parser<int, 2>` trims each token, accepts the
  `$`/`h` spellings and writes a one-token read into X only;
- a parasite eating the passenger is suppressed for 50 frames and released;
- a `Gunner=yes` transport re-picks its list tail as the gunner;
- the passenger's own cargo dies with `DontScore` choosing no credited
  attacker, and the passenger is uninitialized.

`PassengerDeletion.Rate=0` explicitly removes the feature again in a later
pass (`CanParse`'s reset arm).

## Where VERA20K owns it

Rules live in `rules::phobos_passenger_deletion`. The clock is
`GameEntity::passenger_deletion_timer`; `sim::passenger_deletion` owns the
visits, deletion, refunds and the gunner/parasite calls. `DigitalDisplay`'s
`PassengerKill` reads the clock (`Body.Visuals.cpp:687-698`).

## Native anchors

The timer this feature drives lives in `TechnoExt::EatPassengers`
(`Ext/Techno/Body.Update.cpp:261..435`), reached from the techno's own `TechnoClass::AI` visit
(`0x6F9E50`) through `TechnoExt::OnEarlyUpdate` — there is no dedicated hook for the keys, and the
destruction it registers is the stock one.

## Residuals

- A type change through `Simulation::convert_techno_type` stops a running
  countdown only when the new type has a PassengerDeletion config with
  `Rate <= 0`, as native's `UpdateTypeData` does; a new type with no config
  keeps the clock.
- An `Anim` type with `MakeInfantry=`/`CreateUnit=` re-selects its own owner
  kind in native; this port installs the transport's House and applies the
  remap, the same limitation the existing combat-explosion anim owner records.
  The anim does attach to the transport through `SetOwnerObject`
  (`ownedObject=true`) and carries it as its invoker.
- The building-transport `RecheckPower` write is modelled as
  `PowerState::invalidate(false)`, the existing House+5778 reassessment flag.
- The flying label's `DisplaySoylentToHouses` check runs per client against the
  local player, which is the single-player equivalent of native's
  `HouseClass::CurrentPlayer`.
- `CanParse` runs per rules pass in Phobos. VERA reads the merged layer state,
  so a later `UseCostAsRate=no`/`Rate=0` pair can keep a feature an earlier
  `UseCostAsRate=yes` created where Phobos would drop it. Scalar keys alone
  fold identically.
