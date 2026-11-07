# Optional Ares line of fire, solid buildings and projectile scatter

Build with `--features ares`. Ares replaces the whole line-of-fire walk
(`0x4CC100`, `0x4CC310`, `0x4CC360`) with `AresTrajectoryHelper` and lets a
projectile stop at buildings. Default builds keep the original walk with its
geometry tail (`combat::line_of_fire`) and ignore every key below.

```ini
[GAPILL]                        ; rulesmd.ini BuildingType
SolidHeight=2                   ; levels of 256 leptons over the cell floor
                                ;   that stop shots; 0 (default) never,
                                ;   negative: the type's art Height=
SolidLevel=1                    ; the projectile SolidLevel= that pierces it

[GAPILLART]                     ; artmd.ini, the type's Image=
SolidHeight=2                   ; the Ares <= 0.9 place; rules wins

[Cannon]                        ; projectile
SubjectToBuildings=yes          ; buildings on the line block the shot
SolidLevel=1                    ; pierces enemy buildings up to this level
BallisticScatter.Min=0.5        ; cells; FireAt's scatter magnitude bounds
BallisticScatter.Max=1.5        ;   (Inaccurate=yes Arcing=yes)

[CombatDamage]
AlliedSolidTransparency=yes     ; buildings let their owner's allies through
```

## Where VERA20K owns it

- `rules/ares_bullet_type.rs`: BulletTypeExt `SubjectToBuildings`,
  `SolidLevel`, `SubjectToTrenches`, `BallisticScatter.Min/.Max`
  (`100193C0`, constructor `100192C0`), read with the projectile.
  `rules/ares_building_type.rs`: `SolidHeight` (art, then rules) and
  `SolidLevel` (`1000DD40`), and RulesExt `AlliedSolidTransparency`
  (`1002AB60`).
- `sim/combat/line_of_fire.rs`, the walk's one owner, has the Ares branch
  of each native body: `GetObstacle 1006E540` (cliff and wall tests on
  unsigned levels, a missing firer is no wall ally, the building test, no
  geometry tail), `FindFirstObstacle 1006E2B0` (the same integer stepping,
  target cell looked up before the source) and `FindFirstImpenetrableObstacle
  1006E1F0` (a breakable wall as before; otherwise the cell's building of a
  house the firer is not allied with is pierced when its `SolidLevel` is at
  most the projectile's). The building test (`CellClass::GetBuilding
  0x47C520`) skips the firer and its target, an ally under
  `AlliedSolidTransparency`, and an open gate (`0x4525F0`), and compares the
  sample's Z with `SolidHeight * 256` over `MapClass::GetCellFloorHeight
  0x578080`.
- `sim/combat/in_range.rs`: InRange's hooks `0x6F7511`/`0x6F7631`
  (`1004D120`) pass the firer and its target. Every InRange caller hands the
  live buildings in `LineOfFireInputs::buildings`.
- `sim/world/projectile_collision.rs`: `BulletClass_ShouldDetonate_Obstacle
  1001AB10` (hook `0x468BE2`): the shared probe's gate adds
  `SubjectToBuildings` (carried in the bullet's collision policy, saved and
  hashed) and passes the bullet's owner and target.
- `sim/combat/projectile_delivery.rs`: `BulletClass_SetMovement_Obstacle
  1001A940` (hook `0x4688BD`): an `Inviso=` bullet stands on the first
  obstacle between its launch and its placement, at that cell's centre, for
  FireAt's and shrapnel's bullets alike. Without Ares the original walk's
  result is overwritten.
- `sim/combat/shrapnel.rs`: `BulletClass_Shrapnel_WeaponType1/2`
  (`1001ABF0`, hooks `0x46A5B2`/`0x46AA27`) give each shrapnel fragment its
  weapon, which its detonation reads (`RadLevel=`, Ivan bombs) and which a
  `FlakScatter=` Inviso fragment's Fire placement divides by
  (`Simulation::ares_shrapnel_flak_placement`).
- `sim/combat/world_fire.rs`: `TechnoClass_Fire_BallisticScatter1/2`
  (`10019930`, `10019980`, hooks `0x6FE709`/`0x6FE7FE`) take the scatter
  draw's bounds from `BallisticScatter.Min/.Max`.

The restored source steps the walk with `(target - source) * (1.0 / steps)`;
the 3.0 DLL divides integers (`IDIV` at `1006E411..1006E42B`), which VERA
follows. The three `0x4CC1xx`/`0x4CC3xx` hooks themselves have no caller in
an Ares build: their only callers sit inside the code the hooks above skip.

## Not covered

- The firestorm arm of `0x4688BD` (`MapClass::FindFirstFirestorm`,
  `ImmolateVictim`): VERA has no active Ares firestorm wall.
- The other BulletTypeExt keys: `Parachuted` (`46867F`), `Splits`,
  `RetargetSelf`, `RetargetAccuracy`, `AirburstSpread` and `AroundTarget`
  (the airburst tail `469EBA`, `1001A090`, which Ares builds skip),
  `AnimLength` (`468000`, `1001A840`: the client draw path has no global
  frame and the atlas holds 32 facings; Ares' own facing conversion,
  `0x3FFF - ftol(angle * 65535/2pi)`, can also pick a neighbouring facing
  at bucket edges), `AttachedSystem` (`5F4FE7`) and the anim palette
  (`468379`). None is set by retail rules; retail's only `Airburst=`
  projectile (`V3AirburstP`) is fired by no weapon.
- The original walk's oracle vectors (`line_walk_vectors.json`,
  `shared_collision_vectors.json`) run in default builds only; the Ares
  bodies are covered by their own tests.
