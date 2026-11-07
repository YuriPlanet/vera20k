# Optional Ares weapon visuals

Build with `--features ares` to let a weapon colour its radiation beam, electric
bolt and Sonic/Magnetron wave, reverse a wave against chosen target classes and
fire a detached railgun.

```ini
[MyRadBeam]                      ; rulesmd.ini WeaponType section
IsRadBeam=yes
Beam.Color=255,0,128             ; else ChronoBeamColor (temporal) or RadColor
Beam.Duration=15                 ; frames
Beam.Amplitude=40.0
Beam.IsHouseColor=no             ; yes keeps the firing House's colour

[MyBolt]
IsElectricBolt=yes
Bolt.Color1=255,255,255          ; first outer, second outer, centre strand
Bolt.Color2=0,0,255
Bolt.Color3=128,128,255
Bolt.ParticleSystem=SparkSys     ; `none` spawns none; unset: DefaultSparkSystem

[MySonic]
IsSonic=yes
Wave.Color=0,0,64                ; three ints, added per channel
Wave.Intensity=0,256,256         ; three ints, per-channel brightening
Wave.IsHouseColor=no             ; yes: the owner's House colour as Wave.Color
Wave.ReverseAgainstVehicles=no   ; default IsMagBeam
Wave.ReverseAgainstAircraft=no
Wave.ReverseAgainstBuildings=no
Wave.ReverseAgainstInfantry=no
Wave.ReverseAgainstOthers=no

[MyRail]
IsDetachedRailgun=yes            ; railgun line and system on every shot
```

Default builds ignore every key above and keep the stock drawing and wave rules.

## Wave rules in Ares builds

Every Sonic and Magnetron wave is built through Ares's `CreateWave`, so these hold
for retail weapons too:

- The constructor receives the launch source first and the target second.
- A wave keeps tracking while its target stays in range: a Magnetron asks the
  owner's InRange for the slot that fired it, other waves compare the weapon's
  `Range` with the owner-target 3D distance over the square root of two (the
  stock rule was a fixed 2172 leptons). The beam is drawn from that slot's FLH.
- A weapon with `AmbientDamage` damages the cells the previous update recorded,
  before the wave updates, for every wave type; the damage uses the fired
  weapon, not the owner's current primary. A weapon without it deals none.

## Where VERA20K owns it

- `rules/ares_weapon.rs`: the keys (WeaponTypeExt constructor `100548B0`,
  `LoadFromINIFile 10055520`); `Bolt.ParticleSystem` is found by the ordered
  processor in `rules/native_processing.rs`.
- `sim/combat/radiation_beam.rs`: SpawnRadBeam's Ares hooks
  (`10056010`/`10056080`, gamemd `0x6FD64A`/`0x6FD79C`); presentation resolves the
  House colour (+0x56F9).
- `sim/combat/electric_bolt.rs`, `render/electric_bolt.rs`: CreateBolt and the
  bolt colour and particle hooks (`10056350`, `100561F0`..`100562C0`).
- `sim/combat/world_fire.rs`: detached railguns (`1004D0C0`/`1004D0F0`).
- `sim/world/mod.rs`, `sim/world/techno_ai.rs`, `sim/wave.rs`: CreateWave
  (`10055160`), WaveClass_Update (`100571A0`), Update_Beam (`10057290`) and
  DamageCell (`10056FA0`); the owner's wave slot is TechnoExt+8 on the entity.
- `app/presentation/fire_effects.rs`, `render/wave_geometry.rs`,
  `render/terrain_surface_lines.wgsl`: GetWaveColorData (`10055270`) and
  ModifyWaveColor (`10055920`) as surface-line mode 11, and the 89999 table
  clamp (`100570B0`).

## Not ported

`Wave.IsLaser` and `Wave.IsBigLaser` (Tiberian Sun laser waves, WaveClass types
1 and 2) are not read: their geometry (`Draw_NonMagnetic`'s per-type corner
table and 1.05 endpoint factor) and `Draw_Laser 0x7609E0` have no Rust port yet,
so such a weapon fires without its laser.
