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

- `NoSelfGuardArea` (the stock self-click Area Guard arm is not ported) and
  `DamageSparksParticleSystems`; the rest is in part 2 below.
- A `Missile.Weapon` without a `Projectile=`: Ares dereferences the null
  BulletType; VERA detonates it without one.
- A zero-`Strength=` type: Ares' smoke and spark gates compare a NaN health
  ratio as not damaged, where the stock tests VERA keeps count it as damaged.

## Part 2: self heal, falls, transports and type roles

```ini
[MyTank]                         ; TechnoType
SelfHealing.Rate=                ; minutes, else [General] RepairRate
SelfHealing.Amount=1             ; .RookieAmount, .VeteranAmount, .EliteAmount
SelfHealing.Max=100%             ; .RookieMax ...; heals up to Strength * Max + 1
SelfHealing.CombatDelay=0        ; frames a damaging hit holds the pulse off
FallRate.Parachute=1             ; FallRate step under a canopy
FallRate.NoParachute=1           ; free-fall step (stock gravity is 1.4)
FallRate.ParachuteMax=           ; else [General] ParachuteMaxFallRate
FallRate.NoParachuteMax=         ; else [General] NoParachuteMaxFallRate
AirRate=0                        ; airborne body frame every AirRate frames
NoManualUnload=no                ; the player cannot unload it
NoManualEnter=no                 ; the player cannot order passengers in
IsDesolator=                     ; DESO: yes; the deployed radiation shim
IsDesolator.RadDependant=        ; DESO: yes; read, no 3.0 consumer
IsCow=                           ; COW: yes; the biased idle roll
IsBomb=no                        ; a paradropped Foot lands as a bomb
Drain.Local=no                   ; a drained plant loses only Drain.Amount
Drain.Amount=0                   ; > 0 caps the drain, <= 0 floors it at -n
ReloadAmount=1                   ; now also a building's, on its reload clock
```

## Self heal

GetSelfHealAmount (10044E80) replaces the eligibility virtual (hook 0x70BE80)
and its `+1` in AI_Update (hook 0x6FA743): nothing while the combat timer
(TechnoExt+0x5C) runs, the stock SelfHealing/SELF_HEAL gate, a global
`Frame % max(ftol(rate * 900), 1)` (so a zero rate pulses every frame
instead of faulting), and up to `SelfHealing.%sAmount` while Health is below
`clamp(ftol(Strength * SelfHealing.%sMax) + 1, 1, Strength)`. A crashing or
sinking techno skips both its own and the House pulses (1004B820). The
ReceiveDamage aftermath (1004AB50, hook 0x702819) starts the timer for a
nonzero result of an initially positive hit. The stock pulse heals above
Strength, so its native replays (`signed_health_tests`) run in default builds
only.

## Falls and transports

ObjectClass_Update_MaxFallRate (10049BA0, hook 0x5F3FB2) steps every falling
Techno by its `FallRate.*` (default 1, also free fall) and selects the arm by
the Parachute pointer (+0x88) rather than +84; an object without a TechnoType
(a flaming guy) steps 1 either way; the stock free-fall replay
(`drop_as_bomb_oracle_tests`) runs in default builds only. `AirRate` decides an airborne Foot's body counter alone (100493D0, hook
0x4DA8B2). `NoManualUnload` leaves a Unit transport's and an Aircraft's self
click with no action (1004BEF0, 0x740031; 10048A90, 0x417DD2) and refuses an
unbunkered transport in CanDeploySlashUnload (10049E70, 0x700EEC);
`NoManualEnter` refuses the passenger rows (1004BEA0, 0x74031A; 100176E0,
0x51ED8E).

## Type roles

`IsDesolator` replaces `ImmuneToRadiation=` in the deployed Guard shim
(100497C0/100497F0, hooks 0x5215F9/0x52138C) and the `DESO` ID in infantry
Mission_Unload (10049790, 0x51F76D); `IsCow` replaces the `COW` ID in the
idle roll (10049740, 0x51CE9A), whose EMP arm takes no bias and leaves the
roll register holding the frame. `IsBomb` raises IsABomb at the paradrop
canopy (10047C60, 0x5F5ADD). A `Drain.Local` plant or drainer subtracts the
limited drain from the House output in place of zeroing it, which then never
stays negative (10024690/10024760, hooks 0x508D32/0x508D4A). A building
other than a Hospital or Armory reloads through TechnoClass::Reload (100478E0,
hook 0x43FE8E), so `ReloadAmount` and the reload clock reach it where the
stock visit refills an empty magazine at once.

## Where VERA20K owns part 2

- `rules/ares_techno_type.rs`: every key above (`self_healing`, `fall_rate`,
  `movement`, `roles`).
- `sim/combat/veterancy.rs` (`ares_self_heal_amount`), `sim/world/techno_ai.rs`
  (the pulse), `sim/vehicle_theft/driver_kill.rs` (the combat timer).
- `sim/object_fall.rs`, `sim/game_entity.rs`, `sim/movement/parachute_descent.rs`:
  the fall step and IsBomb.
- `sim/animation.rs`: AirRate; `sim/deploy.rs`, `app/input/context_order.rs`:
  NoManualUnload; `sim/world/world_orders.rs`: NoManualEnter.
- `sim/world/techno_ai/mission_handlers.rs`, `sim/deploy.rs`: IsDesolator;
  `sim/infantry.rs`: IsCow; `sim/power_system.rs`: Drain.Local;
  `sim/world/techno_ai/building_missions.rs`: the building reload.

## Presentation keys

- `VoiceAirstrikeAttack`, `VoiceAirstrikeAbort` (NullableIdx<VocClass>):
  the first aircraft's, then the designator's, replace the Rules airstrike
  voices (`sim/airstrike.rs`, 10048D60/10048DE0, hooks 0x41D940/0x41D5AE).
- `EnemyUIName`: a building's tooltip name for a player its owner does not
  count as an ally, unless it spied the building (`app/input/tooltips.rs`,
  10015DA0, hook 0x459ED0).
- `RadialIndicatorRadius`: a building's GetSensorRange answer
  (`ObjectType::building_sensor_range`, 10048E60, hook 0x4566B0).
- Art `CameoPalette`: the SHP cameo's palette when it loads
  (`render/sidebar_cameo_atlas.rs`, 1005E390, hook 0x6A9A2A).

## Not covered in part 2

These keys are read (`ResidualTypeRules` in `rules/ares_techno_type.rs`) and
nothing consumes them:

- `Carryall.Allowed`, `Carryall.SizeLimit`: VERA has no Carryall tote
  (hooks 0x417D75, 0x416E37); retail has no carryall.
- `Assaulter.Level`: the stock `Assaulter=` arm of CanBeOccupiedBy (hook
  0x457DB7) is not ported.
- `DigIn`, `DigOut`, `DigInSound`, `DigOutSound` are now consumed
  ([ares-subterranean.md](ares-subterranean.md)).
- `Convert.Deploy` and `InitialPayload.Types`/`.Nums` are now consumed
  ([ares-transports.md](ares-transports.md)).
- `CanBeReversed`, `ReversedAs` and `Unsellable` are now consumed
  ([ares-reverse-engineering.md](ares-reverse-engineering.md)).
- `FakeOf`: only the selection's value readout reads it
  (TacticalSelection 0x731D90, hook 0x731E08), which VERA does not draw.
- `EVA.Lost` is consumed by the unit-lost cue (FootClass_Destroyed
  10049080, hook 0x4D98C0; buildings 10013F10, hook 0x44D760;
  [ares-warhead-residuals.md](ares-warhead-residuals.md)).
- `NoSelfGuardArea` (10049710, hook 0x51E748): the healer infantry's
  GuardArea over itself (0x51E710..0x51E748) is not ported.
- `RadialIndicatorRadius` of non-building technos (0x41BE80) and the
  `~~~` theater placeholder in `CameoPalette` (the cameo atlas is not
  built per theater).
- The infantry `NoManualEnter` hook also admits Ares tunnels, not ported.
- The `HunterSeeker.*` speeds and proximities, and the RulesExt
  `HunterSeeker*` defaults, need the unported hunter flight (hooks
  0x4CCB84..0x4D8D95).
