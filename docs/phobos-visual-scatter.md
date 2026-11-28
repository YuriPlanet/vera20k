# Visual scatter (`VisualScatter`)

A weapon can draw its laser, electric bolt or railgun beam at a random point
near the target instead of exactly at it. The shot itself is unaffected: only
the drawn effect scatters, so damage still lands where the bullet aims.

## Keys

| Key | Owner | Type | Default |
|---|---|---|---|
| `VisualScatter=` | WeaponType | `Valueable<bool>` | `no` |
| `VisualScatter.Min=` | `[AudioVisual]` | `Valueable<Leptons>` | `8` |
| `VisualScatter.Max=` | `[AudioVisual]` | `Valueable<Leptons>` | `32` |

Reads: `WeaponTypeExt::LoadFromINIFile` (`Ext/WeaponType/Body.cpp:133`, field
`Ext/WeaponType/Body.h:63`, constructed `no` `:158`); globals
`RulesExt::LoadFromINIFile` (`Ext/Rules/Body.cpp:325-326`, declarations
`Ext/Rules/Body.h:220-221`, constructors `:758-759`). VERA reads the weapon key
in `src/rules/phobos_weapon_extras.rs` and the pair in
`src/rules/phobos_globals.rs`.

## Behaviour

`TechnoClass_DrawSth_DrawToInvisoFlakScatterLocation`
(`Ext/Techno/Hooks.WeaponEffects.cpp:220-248`) hooks the three drawing helpers
`FireAt` uses:

| Hook | Helper | VERA owner |
|---|---|---|
| `0x6FD38D` | CreateLaser | `src/sim/combat/laser.rs` (`fired`) |
| `0x6FD514` | CreateEBolt | `src/sim/combat/electric_bolt.rs` (`fired`) |
| `0x6FD70D` | CreateRBeam | not drawn by VERA — residual below |

```cpp
if (pWeaponExt->VisualScatter) {
    radius = Random.RandomRanged(VisualScatter_Min, VisualScatter_Max);
    *pTargetCoords = GetRandomCoordsNear(GetTargetCoordsForFiring(pBullet), radius, false);
}
```

Two Scenario-RNG draws per fired effect (the radius, then the direction), so
the scattered endpoint stays identical across lockstep peers. The hook only
acts while `FireAtTemp::FireBullet` is set, i.e. inside `FireAt`; a shrapnel or
split-child `CreateLaser`/`CreateEBolt` call keeps the plain endpoint.

VERA applies the replacement where each effect's endpoint is decided:
`laser::fired` overwrites the returned birth's `to`, and `electric_bolt::create`
takes a `visual_scatter` flag and scatters before `emit`, because CreateEBolt
hands the same coordinate to the bolt and to its spark particle system.

## Residuals

- `MapClass::GetRandomCoordsNear` (`0x0049F420`) is the impact-animation owner
  `src/sim/combat/inviso_scatter.rs::random_direction_coord`: one raw Scenario
  draw, its low byte into the retail sine/cosine samples, the 512-cell
  fallback, Z preserved. `random_coords_near`
  (`src/sim/projectile/launch.rs`) now only carries the projectile coordinate
  shape into it, so this arm and the three trajectory arms share one formula.
- `RBeam` (`0x6FD70D`) has no VERA counterpart: the railgun's line damage and
  its particle aim exist (`src/sim/combat/railgun.rs`), but no beam is drawn,
  so there is nothing to scatter. A future railgun beam must hook the same
  scatter.
- Phobos `LaserPositionUpdate` re-tracks a laser's endpoint every frame; when a
  weapon sets both, the follow overwrites the scattered point after the first
  frame. Native has the same interaction, so VERA leaves the two features
  independent rather than suppressing the scatter.

## Tests

- `src/rules/phobos_globals_tests.rs` —
  `the_visual_scatter_bounds_default_and_read_from_audio_visual`.
- `src/sim/combat/phobos_visual_scatter_tests.rs` —
  `a_visual_scatter_laser_ends_away_from_its_real_target` (`no` keeps the
  target, `yes` stays inside the radius).
