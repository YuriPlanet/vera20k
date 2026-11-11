# Ares HunterSeeker launch

Requires an explicit `--features ares` build. Default builds do not recognize
`Type=HunterSeeker` or read its extension keys.

The launch handler follows installed Ares `SW_HunterSeeker::Activate`
(`0x10072850`) and its building callback (`0x10072190`). Configuration stays
with the ordered Rules processor; generated units use the existing construction,
placement, mission, target and cleanup owners.

- `HunterSeeker.Type` names an existing VehicleType. An unset/null value uses
  the firing Side's `HunterSeeker` pointer. Unknown scalar names retain an earlier
  value; the reader does not allocate a missing type.
- `HunterSeeker.Buildings` names existing BuildingTypes. An empty list uses
  `[SpecialWeapons] HSBuilding`. Buildings are visited in the house's retained
  building order. `SW.MaxCount` defaults to one and counts successful placements;
  a negative value removes that count limit. Failed constructors are discarded.
- `Action` is forced to None. Direct clicks launch immediately without a target
  cursor. The common automatic targeting mode is HunterSeeker; manual/automatic
  policy, charging and presentation use the shared superweapon owners.
- Each generated unit retains its firing house and superweapon identity for
  target acquisition and saves. Fly locomotors acquire an initial eligible
  target using the shared target setter and Scenario RNG; other locomotors'
  acquisition method remains a no-op. Harvesters receive Area Guard, others
  Attack.
- `HunterSeeker.RandomOnly`, the common effect masks and
  `HunterSeeker.Ignore` control the initial target pool. Ignore is retained
  across ordered reads and participates in the simulation configuration hash.
- Successful activation precedes extension fee settlement, following common
  `SWTypeExt::Launch` (`0x10032ED0`). A launch with no usable building still
  consumes readiness but does not pay its fee. GenericWarhead and UnitDelivery
  use that same activation/settlement boundary.
- `EVA.Ready`, `EVA.Activated` and `EVA.Detected` resolve against the fixed
  EVAMD catalog in read order. Unknown later names retain the earlier binding;
  explicit none clears unless that name is itself registered. HunterSeeker
  defaults to `EVA_HunterSeekerReady`, `EVA_HunterSeekerLaunched` and
  `EVA_HunterSeekerDetected`; unregistered defaults are silent. Ready is local,
  detection uses the existing hostile-building gate, and launch uses the
  listener's EVA voice. Its queued Ready entry is removed after common sound.
  GenericWarhead and UnitDelivery share these readers and playback consumers;
  stock redirected superweapons retain their existing announcement paths.

This is a bounded launch and initial-acquisition port, plus three hooks of
the flight (`Ext/Techno/Hooks.HunterSeeker.cpp`):

- Height step (`FlyRuntime::step_height`): a `HunterSeeker=` Fly that is not
  a Dropship climbs at `HunterSeeker.EmergeSpeed` while taking off and
  `.AscentSpeed` otherwise (1004D930, hook 0x4CDE64), and descends at
  `.DescentSpeed` (1004DA20, hook 0x4CDF54), each capped by the remaining
  distance. The TechnoType keys (Nullable<int>, `rules::ares_techno_type`)
  fall back to `[General] HunterSeekerAscentSpeed`, `DescentSpeed` and
  `EmergeSpeed` (Valueable<int>, 0, LoadAfterTypeData 1002A800).
- Landing (1004D8A0, hook 0x4CE85A, `sim::world::fly_landing`): a
  HunterSeeker never lands. Without a target its landing callback runs the
  acquisition above; on success the Fly stops landing and takes the type's
  flight level, the hunter unlinks its first radio contact, queues Attack
  and commences it.

The rest of the flight (`Ext/Techno/Hooks.HunterSeeker.cpp`, `sim::hunter_seeker`):

- Process (1004D7A0, hook 0x4CCB84, before the queued-mission check): a
  hunter drops a Techno Target that left the playfield, is iron-curtained or
  is being warped out (Assign_Target(NULL)) and, without a Target, runs the
  landing callback's reacquire block.
- Target update (1004DA90, hook 0x4CD9C8, in UpdateFlightMotion): the Fly's
  destination follows the Target's GetCoords raised to the cell floor, and
  the body and turret facings snap toward it. The tube coordinate of a
  target in a tunnel is not represented.
- Horizontal step (1004DC30, hook 0x4CF3D0): within
  `HunterSeeker.DetonateProximity` (else `[General]`) of its destination a
  hunter with a Techno Target detonates: the Target, then the hunter, take
  weapon 0's Damage and Warhead (ReceiveDamage, IgnoreDefenses), then the C4
  flash and Apply_area_damage at the hunter, and the step returns 0. Within
  `.DescendProximity` FlightLevel blends from the type's flight height down
  to the Target's Z (at least 10); farther it looks 10 cells ahead along its
  facing and climbs back to its FlightLevel over higher ground. The facing
  angle uses the engine's coordinate-facing kernel (`facing16_between`),
  which may differ from YRpp's DirStruct conversion by one binary-angle unit.
- Leaving the map (1004E050, hook 0x4D8D95, PerCellProcess's exit): the
  hunter hits its Techno Target with weapon 0 before its UnInit.

Stock redirected superweapons retain their existing settlement
path. No mod-specific unit names, resource readers or assets are included.
