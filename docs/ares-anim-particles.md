# Optional Ares anim and particle extensions

Build with `--features ares`. Ares 3.0's AnimTypeExt (`Ext/AnimType`),
ParticleTypeExt (`Ext/ParticleType`) and ParticleSystemExt
(`Ext/ParticleSystem`). The AnimType keys are read from the anim's ART
section by `AnimTypeExt::LoadFromINIFile` (`1000A4B0`, hook
`1000A760` at `0x4287DC`/`0x4287E9`), after the original body; the ParticleType keys from
its rules section (`100291A0`, hook `10029230` at `0x645405`). Default builds read none of
them.

Focused tests: `sim::ares_anim_type_tests`, `sim::particles::ares_gas_tests`.

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
| ParticleType `Palette` | hook `0x62D015` `10029790` | Particles draw through their type's palette | presentation: not ported |
| ParticleSystemExt: hooks `0x62FD60`, `0x62E380`, `0x62E2AD`, `0x6D9427`, `0x72590E`, `0x62DF05`, `0x62E26B`, save/load | `100290D0`, `100290B0`, `10029050`, `10029100`, `10028FD0` | Ares simulates Smoke, Spark and Railgun systems whose particle type has no LineTrail and no AlphaImage in its own records instead of ParticleClass objects, with its own Scenario draws, and draws them after the Air layer | residual: a separate mechanism (`Ext/ParticleSystem/Body.cpp`, about 1000 lines). UpdateSmoke `10027950`, UpdateSpark `10028370` and UpdateRailgun `10026E70` draw from `ScenarioClass::Instance->Random`, the synchronised stream, so the port is simulation work (its particle state saved and hashed, its draws in order), with only the after-Air draw (`0x6D9427`) on the client. Effect: those systems' particles move and fade as vanilla in VERA, and the Scenario stream differs from an Ares game after any such system updates; VERA peers stay in step, as all run the vanilla path |
