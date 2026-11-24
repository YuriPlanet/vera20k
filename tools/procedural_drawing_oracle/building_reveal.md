# Building reveal admission

`shroud.py --building-reveal` executes original `6D9920` and `43CEA0` over one
prepared retail GAPILE Building and records whether the base DrawIt runs.

```sh
python -B -m tools.procedural_drawing_oracle.shroud --building-reveal --check
```

| Prepared input | Original calls base DrawIt |
| --- | --- |
| Both top and center unexplored | yes |
| Only center explored | yes |
| Only top explored | yes |
| Both explored | yes |
| `Building+6E7=1` (fog snapshot state) | no |
| Same, Armageddon enabled | yes |
| Inactive / limbo / supplied offscreen rectangle | no, each |

Exploration does not gate drawing. `Building+6E7` is the optional FogOfWar
snapshot flag set by `4D0EF0`, not an unexplored-anchor flag. The tint slice
`706389..7063EB` clears the packed special tint when the center cell is shrouded
(`7063D7`) and keeps the separate brightness.
`ordinary_building_admission_matches_executed_native_controls` in
`src/app/presentation/instances/helpers.rs` reads this corpus. The `455C20`
rectangle is supplied and `43D290` is a recording terminal; enabled FogOfWar,
slopes, bridges and pixels are not covered.
