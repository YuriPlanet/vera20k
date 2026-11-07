# Optional Ares TechnoType combat and movement keys

Build with `--features ares`. Default builds ignore every key below and keep
the stock rocket, reload, crush, targeting, smoke and crash paths.

```ini
[MyRocket]                       ; rulesmd.ini AircraftType
Missile.Custom=yes               ; fly by the Missile.* block below
Missile.PauseFrames=0            ; the RocketStruct keys, all 0 by default
Missile.TiltFrames=60
Missile.PitchInitial=0.21        ; quarter turns
Missile.PitchFinal=0.5
Missile.TurnRate=0.05
Missile.RaiseRate=1              ; read as a float, truncated
Missile.Acceleration=0.4
Missile.Altitude=768
Missile.Damage=300
Missile.EliteDamage=400
Missile.BodyLength=256
Missile.LazyCurve=yes
Missile.Warhead=MyWH             ; else null: no explosion
Missile.EliteWarhead=MyEliteWH
Missile.Weapon=MyBlast           ; detonate this weapon instead of the warhead
Missile.EliteWeapon=MyEliteBlast
Missile.TakeOffAnim=V3TAKOFF     ; every AircraftType; none puffs nothing
Missile.TrailerAnim=V3TRAIL
Missile.TrailerSeparation=3      ; frames between trailer puffs

[MyTank]                         ; TechnoType
ReloadAmount=1                   ; rounds per Reload
EmptyReloadAmount=               ; rounds when empty, else ReloadAmount
NoAmmoAmount=0                   ; at or below this Ammo ...
NoAmmoWeapon=-1                  ; ... fire this weapon index
Gattling.Cycle=no                ; at the cap, restart at stage 0
CrushDamage=0                    ; what crushing this type costs the crusher
CrushDamage.Rookie=              ; .Veteran, .Elite by the victim's rank
CrushDamage.Warhead=             ; else [CombatDamage] C4Warhead
OmniCrusher.Aggressive=yes       ; no: no OmniCrusher charge
Berserk.ROFMultiplier=           ; else [CombatDamage] BerserkROFMultiplier
CanPassiveAquire.Guard=yes       ; no: a player's guarding unit never scans
CanPassiveAquire.Cloak=yes       ; no: not while it can cloak
CivilianEnemy=no                 ; yes: always a target, even Insignificant
Smoke.Anim=SGRYSMK1              ; damaged aircraft smoke (aircraft default)
Smoke.ChanceRed=10               ; percent per frame alive below red
Smoke.ChanceDead=80              ; percent per frame at zero health
DamageSparks=                    ; yes/no over the type's own (Cyborg) flag
CrashSpin=yes                    ; no: crash without the spin
PassengerTurret=no               ; yes: turret index = passenger count

[CombatDamage]
BerserkROFMultiplier=0.5
AutoRepel=no                     ; computer: attack whoever attacks an ally
PlayerAutoRepel=no               ; the same for a human player
```

## Rockets

`Missile.Custom` gives an AircraftType its own RocketStruct (TechnoTypeExt
+0x330, zeroed by the constructor 1003C710 and read by 10040740), which Rocket
Process, Move_To, Detonate and Draw_Matrix use in place of the Rules V3, DMisl
or CMisl block (hooks 0x6622E0, 0x6632F2, 0x66305A, 0x6634F6). With a
`Missile.Weapon` the impact detonates a bullet of that weapon, owner and target
the rocket, instead of the explosion, light and area damage (0x663218). A
custom type's spawn pool slots are missiles that wait its own PauseFrames plus
TiltFrames (0x6B6D60, 0x6B78F8, 0x6B7A72). Every AircraftType names its own
takeoff and trailer puffs (0x66238A, 0x662512, 0x6627E5, 0x662D85, 0x6B752E);
Initialize (1003DEC0) finds V3TAKOFF and V3TRAIL by name at the type's first
read, so an unregistered one puffs nothing.

## Ammo, gattlings and combat

Reload adds `ReloadAmount` (or `EmptyReloadAmount` to an empty count) clamped to
zero and `Ammo=` (10047A70, hook 0x6FB05B), for the Unit and Infantry reloads.
`NoAmmoWeapon` answers What_Weapon_Should_I_Use right after its open-topped arm
(10047AE0, 0x6F3410). Ares' gattling hook (1004B6C0, 0x70DEBA) makes the stage
test read the charged value, so in Ares builds every gattling steps up one
charge earlier, and restarts a `Gattling.Cycle` type at the cap. Crushing a
Techno makes the crusher receive its `CrushDamage` (1004BCE0, 0x7418AA). A
berserk firer's rearm is `ftol(ROF * multiplier)` (1004A3D0, 0x6FF28F).
`CanPassiveAquire.*` returns no target from Foot's Greatest_Threat entry for a
human player's Foot on Guard (100491E0, 0x4D9920); `CivilianEnemy` and
`AutoRepel` skip Evaluate_Candidate's passive-house and Insignificant gates
(10049D00, 0x6F826E). Aircraft smoke (10047B60, 0x415085), damage sparks
(1004B760, 0x6FACD9), the crash spin (10049050, 0x4DECAE) and the passenger
turret (1004B320, 0x6F9E50) follow their keys.

## Where VERA20K owns it

- `rules/ares_techno_type.rs`: every key above and the RulesExt globals, read
  by the ordered Rules processor (`native_processing`) and projected onto
  `ObjectType::techno_combat`; `rules/missile_spawn.rs`: the RocketStruct
  reader and `RocketTypeExt`; `RuleSet::rocket_block`, `rocket_type_ext` and
  `spawn_missile_wait_frames` choose the block.
- `sim/movement/rocket_movement.rs` and `sim/world/rocket_flight.rs`: the
  puffs and the weapon detonation; `sim/spawn_manager.rs`: the missile slots.
- `sim/ammo.rs`, `sim/game_entity.rs`: Reload; `sim/combat/combat_weapon.rs`:
  NoAmmoWeapon; `sim/combat/gattling.rs`: the cycle.
- `sim/movement/per_cell.rs`: crush damage; `sim/movement/foot_approach.rs`:
  OmniCrusher; `sim/combat/world_fire.rs`: berserk ROF;
  `sim/combat/greatest_threat.rs`: passive acquire and CivilianEnemy, with
  `sim/world/techno_ai_cloak.rs`' FootClass::IsCloakable.
- `sim/world/crash.rs`: smoke and spin; `sim/particles/spawn.rs`: sparks;
  `sim/game_entity/gunner.rs`: the passenger turret.

## Not covered

- `SelfHealing.CombatDelay`: Ares replaces the whole self-heal pulse
  (`SelfHealing.Rate/.Amount/.Max`, 10044E80, hooks 0x70BE80 and 0x6FA743),
  which is not ported.
- `FallRate.*`: Ares replaces ObjectClass::Update's fall step (10049BA0, hook
  0x5F3FB2) for every object, an integer step in place of the 1.4 free-fall
  gravity and the Parachute pointer in place of `+0x84`; not ported.
- `AirRate`, `Carryall.*`, `NoManualUnload`, `NoManualEnter`, `NoSelfGuardArea`
  (the stock self-click Area Guard arm is not ported), `DigIn`/`DigOut` and
  their sounds, `Assaulter.Level`, `DamageSparksParticleSystems`, and the
  group 6 keys (`Drain.*`, `InitialPayload.*`, `IsBomb`, `IsCow`, `IsDesolator*`,
  `Convert.Deploy`, `EnemyUIName`, `RadialIndicatorRadius`, `CameoPalette`, the
  `HunterSeeker.*` speeds).
- A `Missile.Weapon` without a `Projectile=`: Ares dereferences the null
  BulletType; VERA detonates it without one.
- A zero-`Strength=` type: Ares' smoke and spark gates compare a NaN health
  ratio as not damaged, where the stock tests VERA keeps count it as damaged.
