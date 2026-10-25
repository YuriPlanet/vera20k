# CABHUT consumers of ordinary FV bridge collapse

`probe.py` extends the existing `paid_world.Paid` owner. The shared native
Navigation owner freshly constructs all four physical Anytown states and checks
their Cells, planes and ordered graphs before this witness selects `damaged`.
No VM, movement, firing, damage or repair algorithm is duplicated.

After the FV command at frame 1, original type constructor/readers and the full
Neutral House constructor admit the two authored CABHUT rows at `(85,58)` and
`(89,51)` through Building map reader `44F820`, constructor `43B740` and Unlimbo
`440580`. Both original `43FB20` Building AI receivers run in live Logic order.
The first Bullet collapses the span at frame 32; the second impacts at 35.
After both effects drain, the existing admitted `573540` repair boundary runs.

The executed original packet establishes:

- The three added constructor draws advance Scenario RNG; shots occur at
  frames 1/5. Hut IDs are 234/235, Bullets 236/237 and effects 238/239.
- All 104 Building AI visits leave both huts alive at health 2000 and consume
  no hut-owned RNG.
- Collapse calls `575EE0` and `70D4A0`. The latter scans hut235, hut234, FV231;
  only FV receives `4D8F80` and `6FCDB0(NULL)`. The complete hut bytes remain
  unchanged. No hut controller `574000`/`574C20` or CellTag event runs.
- The only new effects are two `H2O_EXP3` Anims. Repair changes overlay232 to215
  and consumes MapGen Next values 1229352179, 1781224790, 2424954917; every hut
  byte remains unchanged and no additional object is constructed.

This is a separate composition. Corrected original Drive CRT startup now
precedes the FV constructor through the shared Paid owner. The complete literal
hut gameplay projection remains identical: every hut byte, RNG word, callback,
shot, impact and repair field above is unchanged. Raw FV timers/residuals and
redundant frame-7 movement calls change as detailed in the [parent package](../README.md).
The active source is `hut_joined_drive_crt.json.gz` and the active projection is
`hut_drive_crt_vectors.json`. The prior packet and projection remain immutable
superseded evidence; the same generator entrypoints target the corrected files. Huts join **after** the FV command; this does not establish ScenarioLoad
population order. Other Buildings, Units, Infantry, initial ambient Anims, live
Terrain AI, global Scenario/House phases and audio remain excluded. The source
uses the inherited supplied human House; Neutral is the only fully constructed
House registry entry. Repair starts at its admitted receiver, without claiming
a new Engineer admission or C4 lifecycle. Existing fixture OS/asset transports
remain explicit; the additional Interlocked transport preserves its original
single-threaded count/return semantics. Original `.text` is checked unchanged.

## Inputs and reproduction

Use the parent FV package's retail input environment and its existing
`VERA20K_ANYTOWN_INPUTS`, `VERA20K_SHRAPNEL_INPUTS` and
`VERA20K_PROJECTILE_RENDER_ASSETS` directories. Supply `CTBHUT.SHP` through
`VERA20K_FV_HUT_ASSETS`; when unset, it is read from `VERA20K_ANYTOWN_INPUTS`.
Its SHA256 must be
`945216ab1eba7e34e6785fd54e916d096a6df4dee1df1052094e0b68f02630ff`
(retail `ra2.mix` → `temperat.mix`, 6080 bytes). The existing stock-CSF reader
reads `langmd.mix`/`RA2MD.CSF` from the configured retail installation; only its
two required cached strings feed original `734E60` lookup. No retail assets,
heap caches or machine-specific paths are included here.

From the repository root:

```sh
python -m tools.spatial_oracle.fv_cell_attack.huts.probe --check --fresh-world
python -m tools.spatial_oracle.fv_cell_attack.huts.projection --check
```

`--fresh-world` is accepted for clarity; every native replay rebuilds the
physical worlds. The probe uses the existing compressed packet publisher with
the local `promotion.json` guard. Even `--write` rejects a changed native
payload. Publication hashes copied lexical INI fields through the parent FV
publication owner and retains every executed state, RNG value and ordered trace.
`projection.py` reuses the paid projection's raw decoders and selects literal
native fields without recomputing gameplay.

`original_execution.meta.json` preserves the original execution's scope,
boundaries and payload identity.
The previous `hut_joined.meta.json` and `receipt.json` remain historical records
of the original relocation, not validation of corrected Drive startup.
