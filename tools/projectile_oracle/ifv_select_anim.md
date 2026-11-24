# Original impact animation selection

`ifv_select_anim` executes 48 controls through full `SelectAnim(0x48A4F0)`.
Seventeen controls additionally enter its actual Bullet caller at `0x469AF0`
and stop after selection at `0x469BD4`. Physical HE and CombatDamage SplashList readers run;
the initialized height units at `0x89DE70` and `0x89E870` are both 104.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
~/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_select_anim.py --check
```

Each `rows[]` has `input.name`, complete prepared input, native retained
`conventional`/`em_effect`, the actual `passed` caller arguments, selected native
type name, ordered getter/RNG events and final RNG hash. `passed.object_height`
is present for caller rows. Incoming coordinates are always the Bullet's retained
XYZ, not the explosion coordinate; the joined impact corpora record both.

Damage bands include zero, 24/25, 34/35, 69/70, 104/105, 199/200 and signed-max.
The physical HE ordinary list and physical SplashList may have different orders;
native selection supplies every expected name. Water callers test height
207/208/209, live structural flags, OnBridge, and the Unit suppression gate
using original Building/Unit type getters. The Unit type comes from the original FV constructor; original Naval and
Underwater reader blocks consume explicit yes/no controls. Cell occupancy and
direct-target caller local remain supplied inputs, not Unit lifecycle proof. The selected type and null-warhead returns execute natively.

The random branch uses the original **EMEffect** key and native Scenario RNG,
independently seeded 31. No RandomAnims key is inferred. This helper corpus does
not execute AreaDamage or animation construction; `ifv_bridge_impact` establishes
post-mutation selection ordering. LightningWarhead/WeatherConBoltExplosion
special-reference behavior is outside these controls.

`dummy_land_controls` executes the full fallback Cell constructor, then native
lookup misses with explicitly supplied Land 0/2/7. Constructor/reconstruction
write Land 0; misses restamp coordinates and preserve Land. The poison values
demonstrate persistence, not a native gameplay producer for those values.

`--check` independently executes without writing. `--write` explicitly
regenerates the JSON and binary-pinned metadata. No temporary script is imported.
