# Penetrates transport

Phobos lets a warhead reach the passengers of a transport it hits
(`Ext/WarheadType/Body.cpp:437-442`, `Ext/TechnoType/Body.cpp:1174-1177`;
consumer `WarheadTypeExt::ApplyPenetratesTransport`,
`Ext/WarheadType/Detonate.cpp:780-930`, called from `DetonateOnOneUnit` at
`:233`).

```ini
[WARHEAD]                               ; WarheadType
PenetratesTransport.Level=0             ; int, default 0 (disabled)
PenetratesTransport.PassThrough=1.0     ; double, default 1.0
PenetratesTransport.FatalRate=0.0       ; double, default 0.0
PenetratesTransport.DamageMultiplier=1.0
PenetratesTransport.DamageAll=false
PenetratesTransport.CleanSound=         ; ValueableIdx<VocClass>, default none

[TRANSPORT]                             ; TechnoType
PenetratesTransport.Level=              ; Nullable<int>, falls back to global
PenetratesTransport.PassThroughMultiplier=1.0
PenetratesTransport.FatalRateMultiplier=1.0
PenetratesTransport.DamageMultiplier=1.0

[CombatDamage]
PenetratesTransport.Level=10            ; global fallback, default 10
```

## Behavior

The arm runs in the bullet detonation's per-target pass, before the
shield/AE/crit blocks; Phobos passes the bullet's Health as the damage (a pure
`MapClass::DamageArea` passes 0). A warhead level at or below the transport's
effective level returns. `PassThrough` is the warhead value times the
transport's multiplier; when below 1, a `RandomDouble` above it skips the
effect. `FatalRate` likewise; when above 0, a second `RandomDouble` at or
below it makes the hit fatal (the passenger's max Strength, defenses
ignored). Otherwise the damage is
`ceil(bullet Health * warhead DamageMultiplier * transport DamageMultiplier)`.
`DamageAll=yes` damages every passenger whose own level is below the
warhead's; otherwise one `Random(0, n-1)` passenger is picked. Each damaged
passenger first takes the transport's Location
(`passenger->SetLocation(transporterCoords)`, `Detonate.cpp:813,845,865,916`),
so its death effects and corpse leave from the transport; the SetLocation
carries the passenger's own riders. An attack that leaves no passenger plays
`CleanSound` at the transport.

## Where VERA20K owns it

`rules::phobos_penetrates_transport` reads the warhead/techno keys,
`ObjectType` and `WarheadType` carry them and `phobos_globals` reads the
global level. `sim::combat::penetrates_transport` runs inside
`crit::detonate`'s per-target loop (the `DetonateOnOneUnit` port);
`crit::detonate` now carries the bullet damage. Passenger damage goes through
the shared receiver (`commit_entities`), so armor, verses and shields apply;
before that receiver call the passenger takes the transport's coordinate
through `movement::ground_pose::foot_set_location`. The regression test
`penetrates_transport_moves_damaged_passengers_to_the_transport` stages a
`TarCom` on both passengers so `ReceiveDamage`'s Scatter gate refuses and the
final Location stays the transport's.

## Native anchors

The penetration test runs inside the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) (`Ext/WarheadType/Detonate.cpp`); the passengers it walks are
`FootClass`'s own cargo, already owned by the stock transport code.

## Residuals

- The general death path already removes a dying gunner; only the
  `ReceiveGunner(new head)` half of `Gunner=` handling is missing, so a
  transport with `Gunner=yes` and at least two passengers keeps an empty
  gunner slot after a penetrating kill. Trigger: a fatal penetration whose
  first (cargo-head) passenger dies in a `Gunner=yes` Unit; effect: the
  transport keeps firing mode0 instead of the new head's `IFVMode`; risk:
  gunner transports lose their post-kill weapon.
- The receiver's under-attack events produced by passenger damage are
  dropped; nested death effects still surface through the shared effects.
- `ApplyPerTargetEffectsOnDetonate=no` keeps the existing VERA residual: the
  pass stays on the bullet entry rather than moving into ReceiveDamage.
- Passenger fixtures inject the cargo id list without the full limbo/Inside
  transport link; the production boarding state is exercised elsewhere.
