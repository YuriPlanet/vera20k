# Building reveal admission and center tint controls

The separate `--building-reveal` mode extends the existing Shroud/Rally native
fixture; ordinary `shroud.json` generation and its saved corpus are unchanged.
`building_reveal.json` and its metadata record the executed outputs and the
executable identity.

```sh
python -B -m tools.procedural_drawing_oracle.shroud --building-reveal --check
```

## Executed coverage

Whole original `6D9920` scans one prepared Building (`WhatAmI()==6`) and calls
original `43CEA0`. The native `455C20` geometry leaf is replaced by a supplied
rectangle with its RET4 convention; native `43D290` is a recording terminal
with RET8. Clip admission, object coordinates and projection execute natively.
The original executable text stays byte-identical. These controls establish
base-draw admission, not building asset pixels or the complete display lifecycle.

| Prepared input | Original calls base DrawIt |
| --- | --- |
| Both top and center unexplored | yes |
| Only center explored | yes |
| Only top explored | yes |
| Both explored | yes |
| `Building+6E7=1` (fog snapshot state) | no |
| Same, Armageddon enabled | yes |
| Inactive / limbo / supplied offscreen rectangle | no, each |

Inputs use actual native retail GAPILE Foundation reads via the existing
`rally.stock_type_inputs` owner. Original `447AC0`, including foundation
dimension helpers `45EC90/45ECA0`, produces center `[2944,5376,0]` from supplied
object coordinates `[2688,5248,0]`. Original `41BEA0` derives top cell `[10,20]`
and center cell `[11,21]`. No Rust-derived center is used. Original Map startup
`561710/5617A0/5617C0/5617E0` computes height divisor `ABDE88=104`.

The eight tint controls execute original `706389..7063EB`, actual center virtual
`447AC0`, `Cell::IsShrouded` at `487950`, and coordinate helper `586360`.
Prepared tint zero remains zero. Prepared nonzero RGB565 `F800` becomes zero
when the center cell is unexplored and remains `F800` when the center is explored,
independently of the top cell. EBP light intensity remains 1000 in every control.
The fragment stops before rasterization; it does not execute the special tint
producer or certify colored pixels.

## Instruction-established state and limits

`6D9920` tests `Building+6E7` at its building draw admission, independently of
cell exploration. The Building constructor `43B740` initializes it to zero
at `43B984`. FoggedObject creation `4D0EF0` sets it to one at `4D10D5`; cleanup
`4D1650` resets the surviving Building at `4D17EC`. Cell snapshot creation
`486A70` first requires Scenario special flag `1000`. This is the optional
FogOfWar snapshot lifecycle, not an unexplored foundation-anchor flag.
The controls prepare that state; they do not execute its lifecycle. Enabled
FogOfWar and save/load lifecycle coverage remain separate.

At `7063D7`, native `705E00` clears its argument-11 packed special tint when
the actual center cell is shrouded. It preserves the separate EBP brightness
and passes both independently to native `4AED70`. Clearing map-light RGB is
therefore not equivalent. Current Rust `DrawState::for_entity` explicitly
leaves invulnerability absent; `shp_body_tint` consumes map lighting. The
ordinary route has no nonzero packed special tint to suppress. A future
invulnerability/berserk tint implementation must apply the proven center-cell
gate at that tint's owner; its missing producer and pixel behavior remain a
residual rather than a new map-lighting rule.

Prepared cell flags, zero terrain level, synthetic nonzero tint and supplied
geometry bound this comparison. It does not establish all foundations,
slopes/bridges, reveal traversal, observers, enabled FogOfWar, complete native
loader/placement, or final GPU/SHP pixel parity. No RNG/timer/detach operation
is passed in the executed admission/tint slices; the excluded snapshot lifecycle
is not covered by that statement.

The harness executes the shared native Map startup sequence to initialize
`ABDE88` instead of substituting a host arithmetic result.

## Rust consumer and checks

`src/app/presentation/instances/helpers.rs::tactical_entity_admission` owns both
draw and screen-selection admission. SHP and voxel builders request drawing;
the screen encounter-order composer requests selection. Ordinary buildings
bypass the anchor exploration predicate only for drawing. Native Object+74
inactive and limbo controls remain excluded; mobile policy is unchanged. This
does not implement the optional FoggedObject lifecycle described above.

`ordinary_building_admission_matches_executed_native_controls` compares four
visibility and two lifecycle inputs with this saved executable corpus, using
its native top/center cells and object coordinates. It deliberately excludes
prepared fog snapshots and geometry clipping. The separate
`building_hidden_anchor_is_drawn_without_entering_screen_selection` regression
failed before the fix and passes afterwards through retained Display membership
and the screen-selection composer. These are Rust regressions and bounded
admission comparisons, not final building-pixel parity.

## Rust production observation

The [production profile](../map_observation.building-reveal.example.json) loads
retail AnyTown `XMP03T4.MAP` (MIX entry `-854728974`, payload SHA-256
`7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`).
Starting local MTNK1374 receives an ordinary Move toward `(44,87)`;
neutral CANEWY20 building1349 is at `(46,87)`. A zero-step discovery established
these handles before scheduling the command. Its physical art reader reports
Foundation3x5 and `CTNEWY20.SHP` 260x228,8 frames; the ordinary theater loader
and SHP builder consume the retail assets in the captures.

To reproduce from the checkout's normal release build:

```sh
env -u RA2_DIR python -m tools.map_observation --profile "$PWD/tools/map_observation.building-reveal.example.json" --contract "$PWD/src/app/diagnostics/tactical_capture/contract.v2.json" --cwd "$PWD" --output "$PWD/logs/new-building-reveal"
```

This establishes production integration and a visible pre-anchor reveal on
one retail building route. It does not compare a native whole-game frame,
certify every foundation/height/observer, or cover enabled FogOfWar and colored
effects. No rendering or simulation timer/RNG/state writer was added.
