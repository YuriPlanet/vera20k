# Optional Ares anim and particle extensions

Build with `--features ares`. Ares 3.0's AnimTypeExt (`Ext/AnimType`),
ParticleTypeExt (`Ext/ParticleType`) and ParticleSystemExt
(`Ext/ParticleSystem`). The AnimType keys are read from the anim's ART
section by `AnimTypeExt::LoadFromINIFile` (`1000A4B0`, hook
`1000A760` at `0x4287DC`/`0x4287E9`), after the original body; the ParticleType keys from
its rules section (`100291A0`, hook `10029230` at `0x645405`). Default builds read none of
them.

Focused tests: `sim::ares_anim_type_tests`, `sim::particles::ares_gas_tests`,
`sim::particles::ares_system::tests`.

Declare animation-only weapons in the rules `[WeaponTypes]` list. Ares registers
that list before the type bodies (`0x679A15`, `1002B2FF..1002B34D`), so the
animation's `Weapon=` lookup can resolve them on that rules pass. Unregistered
names retain the previous reference, and `none` clears it. This includes healing
auras whose weapon has a zero-damage warhead. See the
[Ares animation documentation](https://ares-developers.github.io/Ares-docs/new/animations.html).

| Key or hook | Ares.dll | Reader / behaviour | Status |
| --- | --- | --- | --- |
| `MakeInfantryOwner` | `1000A4B0` | Valueable<OwnerHouseKind>, Invoker | ported earlier (`art_data`, `destruction_effects`) |
| `Damage.Delay` | `1000A4B0`; hook `0x424538` `1000AFE0` | Valueable<int>, 0. Above zero with `Damage=` at least 1, the +0x188 count (seeded 1.0 by the constructor) rises by one a frame and the whole truncated Damage (times 5 on a terrain owner) lands when it reaches the delay, then restarts at 0. Otherwise the original fraction accumulates. The DLL's conversion (`100772B0`) truncates and gives `i32::MIN` out of range; a result below 1 or a no-effects anim delivers nothing | ported: `Simulation::anim_frame_damage` |
| `Weapon` | `1000A4B0` (Find, `1000A370`); hook `0x424538` | Valueable<WeaponTypeClass*>, Find-only, null. The delivered damage detonates a bullet of the weapon (its Warhead and Bright, no owner, target the anim's cell) at the anim's GetCoords instead of the area damage: the bare `DetonateAtCoord` | ported: `AnimTypeReferences::weapon`, `ares_anim_weapon_detonation` |
| `SpawnsParticle.RangeMinimum`, `SpawnsParticle.RangeMaximum` | `1000A4B0` (`1000A5D0`); hook `0x425002` `1000AE40` | Valueable<Leptons>, 0 each. Either nonzero: each of the NumParticles particles lands `abs(RandomRanged(min, max))` leptons from GetCoords at `RandomRanged(1, 0x7FFFFFFF)/0x7FFFFFFF` of its share of the circle, keeping the anim's height above the floor (`0x00578080`); two Scenario draws per particle | ported: `Simulation::ares_spawn_particles_around` (x87 arithmetic as the DLL; residual: CRT `cos`/`sin` are libm's, a last-ulp difference can move a particle by one lepton) |
| `CustomPalette` | `1000A4B0` (`10076160`); hook `0x4232CE` `1000ACE0` | The anim draws through its own palette (art, `~~~` the theater extension) | read (`AnimTypeRuntimeConfig::ares_custom_palette`); presentation not ported (anims bind the anim/unit palettes in the atlas) |
| `0x4250C9`, `0x42511B`, `0x42513F` | `1000AD10` | Expired Flamer/Scorch fires: a Flamer anim makes one to three SmallFire/LargeFire anims (two RandomRanged(0,99) rolls); a Scorch anim under height 10 off water/beach/ice/rock makes a SmallFire attached to its owner | residual: retail sets no `SmallFire=`/`LargeFire=` and no `Flamer=yes`, so the arms make nothing (the Flamer rolls still draw); needs `MapClass::GetRandomCoordsNear` |
| `0x4242CA` | `1000B210` | A `TrailerSeperation=` below 1 spawns no trailer (the original divides by it) | equivalent: `trailer_cadence_matches` already refuses it |
| ParticleType `DamageRange` | `100291A0`; hook `0x62C23D` `100297C0` | Valueable<double>, 0. Above zero a Gas particle collects the objects of every cell within `ceil(range)` in CellSpreadEnumerator order whose Location is within `range * 256` leptons (at most 0x100), then damages each still alive with the original distance, no attacker and the system's owner House | ported: `particles::gas::damage_in_range` |
| ParticleType `Palette` | hook `0x62D015` `10029790` | Particles draw through their type's palette | presentation: not ported (also the Ares smoke records below draw through ANIM.PAL) |

## ParticleSystemExt

Ares 3.0 simulates a Smoke, Spark or Railgun system itself when its `HoldsWhat`
particle type has no `UseLineTrail` and no AlphaImage art and behaves like the system
(Ares maps a particle `BehavesLike` below 2 to `value == 0`, so a Smoke system holding
a type with no `BehavesLike` counts too). Every retail Smoke, Spark and Railgun system
qualifies. Such a system keeps two record vectors instead of
ParticleClass children and draws from the synchronised Scenario stream in Ares' own
order, so the records are simulation state: they are saved with the system
(`ParticleSystem::ares`) and folded into the world hash. Owner: `sim::particles::ares_system`.
Unlike the type keys above it is in every build: the constructor allocates the extension
whenever the game's Ares switch (`RuleSet::ares`) is on, so retail data keeps the stock
particles.

| Hook | Ares.dll | Behaviour | Status |
| --- | --- | --- | --- |
| `0x62DF05` (end of the constructor `0x0062DC50`) | `10028D10` -> `100267E0` | Picks the mode from the type's held particle | ported: `AresParticleSystemExt::for_system`, `Simulation::spawn_particle_system` |
| `0x62FD60` (AI entry, to its `ret` `0x62FE43`) | `100290D0` -> `10026C70` | `UpdateSpark`, `UpdateRailgun` or `UpdateSmoke`, then the stock lifetime count, retiring a done system only once its records are gone too | ported: `system_ai::tick_particle_system`, `ares_system::update`; the stock tail `finish_system_visit` counts the records |
| `0x62E380` (SpawnParticle entry, to `0x62E428`) | `100290B0` | Such a system constructs no ParticleClass | ported: `spawn::spawn_particle` |
| `0x62E2AD` in DrawIt `0x0062E280` | `10029050` | The one-frame light counts the Spark/Railgun records; `ParticleCap=0` gives none | ported: `combat_lights::particle_system_light` |
| `0x6D9427` after the Air layer | `10029100` -> `10026760` -> `10026430` | Draws every system's records in array order when Detail is above 0 and `RulesExt::DetailsCurrentlyEnabled`: points for sparks and railgun records (A and Z tests as the stock point), skipped on shroud under FogOfWar; the type's SHP frame `State` for smoke records | ported (client): `instances::particles::build_ares_particle_draws`, after every Air member |
| `0x62E26B`, `0x62FF20`, `0x630088`, `0x630090`, `0x6300F3` | `10028D30`, `10028E80`, `10028D70`, `10028EC0` | Frees, saves and loads the extension | ported: the field drops and serialises with its system |
| `0x72590E` in `DispatchPointerExpiredCleanup` `0x007258D0` | `10028FD0` | An expiring Particle is removed from its own system's vector and the rest of the broadcast is skipped | equivalent: only its system names a particle in VERA's listener index, and the skipped listeners (Logic, Tactical, selection, bombs, kamikaze, disk lasers) never hold one |

The updates (all on the Scenario stream, `RandomRanged(1, 0x7FFFFFFF)` scaled by
2^-31):
- `UpdateSpark` `10028370`: while `SparkSpawnFrames` lasts, one draw walks the light
  radius (`+0xF4`) by 3 within 17..41; on its last frame, or under
  `SpawnSparkPercentage` (one more draw), `RandomRanged(0, cap/2) + cap/2` sparks start
  at the system with `X/YVelocity`, `MinZVelocity + 0..ZVelocityRange` (three draws),
  `MaxEC + 0..MaxEC` lifetime (one draw) and a start colour (a draw only when both
  StartColors are set). `UpdateSparkMovement` `10028900` subtracts `Gravity` twice from
  the Z step and kills a spark below its new cell's floor, across a bridge deck, or
  within 150 leptons on a building (not IsStrange) or a `Wall=` overlay. One colour
  draw per record follows (`10026D30`).
- `UpdateRailgun` `10026E70`: on the first frame, `(int)(ParticlesPerCoord * distance)`
  records on the spiral from the target to the system, each with seven draws and a
  `MaxEC + 0..9` lifetime (and the colour draw), plus the stock laser; each frame one
  speed draw and one colour draw per record.
- `UpdateSmoke` `10027950`: follows any owner object at the constructor's offset; on
  odd frames each record takes a 1-in-4 drift draw (and two more when it hits); dead
  records with a `NextParticle` become two (lifetime, fade and two spread draws); every
  `(int)SpawnFrames` frames one record spawns (two offset draws and a lifetime draw);
  `SpawnFrames` grows by `Slowdown` until `SpawnCutoff`.

Residuals:
- `UseLineTrail=` (ARTMD.INI, the particle image's section) is not read for particle
  types and counts as false. Trigger: a mod setting it on a particle image; effect: that
  system runs Ares' simulation instead of ParticleClass; no retail image sets it.
- `SpawnDirection=` keeps whole leptons (the stock reader's representation); Ares adds
  its floats. Trigger: a fractional value; effect: sub-lepton spark directions; neither
  data set authors it.
- VERA keeps no `LaserFenceFrame` (`+0x618`), so a spark counts a `LaserFence=yes`
  building as an online section; VERA builds no fence sections yet.
- Native division traps (a zero `SpawnFrames` period, a zero StateAI period) spawn or
  advance nothing in VERA; a spark or smoke record whose cell VERA cannot evaluate
  (slope type above 20) dies.
- Presentation: smoke records draw through ANIM.PAL, not the type's Ares `Palette=`; a
  point whose colour index has no next list entry is skipped (Ares reads past the
  list); the Detail query runs after the whole Display traversal rather than between
  the Air and Top layers.
- A stock ParticleClass spark (one Ares leaves alone) still answers walls with the stock
  list rather than Ares' `Wall=` hook `0x480534` (`particles::spark`).
