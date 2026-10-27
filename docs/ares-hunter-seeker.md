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

This is a bounded launch and initial-acquisition port. It does not establish
complete flying hunter movement, impact behavior or reacquisition after target
loss. Stock redirected superweapons retain their existing settlement path.
No mod-specific unit names, resource readers or assets are included.
