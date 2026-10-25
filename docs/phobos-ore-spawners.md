# Optional Phobos custom ore spawners

Build with `--features phobos` to read Phobos `TerrainTypeExt`'s five
`SpawnsTiberium.*` keys on a `TerrainTypes` entry. Default builds keep the
stock spawner: one density-3 cell of the source's own resource type in one of
the eight neighbours.

In `rulesmd.ini`:

```ini
[TIBTRE01]                         ; TerrainType
SpawnsTiberium=yes                 ; boolean, the stock gate
SpawnsTiberium.Type=0              ; integer, [Tiberiums] list index
SpawnsTiberium.Range=1             ; integer
SpawnsTiberium.GrowthStage=3       ; integer or min,max
SpawnsTiberium.CellsPerAnim=1      ; integer or min,max
SpawnsTiberium.Particle=           ; ParticleType
```

`TerrainTypeExt::LoadFromINI` hangs off `TerrainTypeClass_LoadFromINI @
0x0071E0A6`, the end of every `TerrainTypeClass::ReadINI @ 0x0071DEA0` pass, and
each `Valueable` read retains the field it is given when its key is absent or
cannot be parsed. A later Rules, mode or map layer therefore overrides only the
keys it names. `GrowthStage=` and `CellsPerAnim=` are
`PartialVector2D<int>`: the reader scans at most two comma-separated integers
and records how many it got, so `4,8` is the inclusive range `4..=8` and a
one-value spelling, a reversed pair or a pair whose ends are equal is a fixed
value that draws nothing. A longer list is not a choice set — the third value
and beyond are never read.

The spawner's animation midpoint (`TerrainClass_AI_Animated @ 0x0071C84D`) draws
`CellsPerAnim=`, then calls `CellClass::SpreadTiberium @ 0x00483780` with
`force=1` that many times and stops at the first call that germinates nothing.
Two Phobos hooks change that call:

- `CellClass_SpreadTiberium_TiberiumType @ 0x00483811` replaces the
  `mov [esp+0x20],0` store, which the original reaches only when the spawner's
  own cell resolves to no `TiberiumClass`. `SpawnsTiberium.Type=` therefore
  replaces the stock Riparius fallback, and a spawner standing on its own ore
  still spreads that ore: the resolved index jumps past the store at
  `0x004837FE`/`0x0048380F`.
- `CellClass_SpreadTiberium_CellSpread @ 0x0048381D` replaces the six bytes that
  load `TiberiumClass::Array`, the `Random__RandomRanged @ 0x0065C7E0(0,7)` at
  `0x00483839` and the eight-direction loop. It draws one start index over
  `AdjacentCellsInRange(Range)`, takes the first cell
  `CanTiberiumGerminate @ 0x004838E0` admits, clamps the drawn growth stage to
  `0..=NumFrames-1` and passes it as the amount to
  `IncreaseTiberium @ 0x00487190`. Like the original it returns that first
  placement result and retries no other neighbour. A cell the spawner matured at
  once also joins that resource's spread queue through
  `RegisterForSpread @ 0x00722AF0`, whose own admission and dedupe it keeps.

`Range` converts to unsigned before the enumerator clamps it to 256; negative
values therefore select that bounded maximum.

`AdjacentCellsInRange` walks YRpp's `CellSpreadEnumerator`, so a `Range=1`
spawner searches nine cells — the centre first, then ring 1 with gamemd's own
CellSpread table's offsets 2 and 4 swapped — and `Range=2` searches that table's
21. The covered set is the same disk a `CellSpread=2` warhead damages; the walk
order is not, so a Phobos spawner and a stock one pick different neighbours for
the same RNG state. The centre is a candidate and `CanTiberiumGerminate` refuses
it, as it refuses every cell a live spawner occupies.

`SpawnsTiberium.Particle=` emits one particle at the spawner's own `Location`
after that loop, whether or not any cell accepted ore. Phobos calls
`ParticleSystemClass::SpawnParticle @ 0x0062E430` on the global at `0xA8ED78`,
which `ScenarioSetup @ 0x00685040` fills with the literal `GasCloudSys` instance,
so the ore particle shares the Scenario's one shared system rather than a second
one. `ValueableIdx<ParticleTypeClass>::Read` uses `FindIndex` and never
allocates: a successful lookup replaces the reference; a failed lookup clears
it only for case-insensitive `none` / `<none>` (`INIClass::IsBlank`). An unknown
nonblank name retains the previous one.
`INI_EX::ReadString` returns zero for an absent or empty value, so neither
changes the reference.

RESIDUALS (trigger, effect, frequency):

- A `SpawnsTiberium.Type=` outside `[Tiberiums]`, when the source holds no
  ore and needs that fallback, dereferences a null `TiberiumClass::Array`
  slot natively. VERA germinates nothing, so the spawner
  emits no cell and no further spread. Only custom data reaches this.

The extension fields are retained per type in the Rules process owner, enter the
simulation configuration hash with the rest of the type, and cross the save
boundary with the retained Rules records. A save records the build capability
and cannot load in the other mode, so a Phobos spawner never replays under the
stock scan by accident.

This feature does not load Phobos DLLs or establish complete mod compatibility.
