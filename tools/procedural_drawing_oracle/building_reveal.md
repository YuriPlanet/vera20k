# Building reveal admission and center tint controls

The separate `--building-reveal` mode extends the existing Shroud/Rally native
fixture; ordinary `shroud.json` generation and its saved corpus are unchanged.
`building_reveal.json` and its metadata were generated from official Steam
`gamemd.exe`, SHA-256
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.

```powershell
$env:VERA20K_GAMEMD_EXE='D:\steam\steamapps\common\Command & Conquer Red Alert II\gamemd.exe'
$env:RA2_DIR='D:\steam\steamapps\common\Command & Conquer Red Alert II'
python -X utf8 -B -m tools.procedural_drawing_oracle.shroud --building-reveal --check
# --write regenerates both references; review changes before accepting them.
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

The initial tint attempt faulted because the fixture had not initialized
`ABDE88`; the retained ignored scratch receipt records that investigation.
The final harness executes the shared native Map startup sequence instead of
substituting a host arithmetic result. `--write` followed by `--check` passed.

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
and the screen-selection composer. The focused instance suite passed 89 tests,
with 3 unrelated explicit GPU/retail tests ignored. These are Rust regressions
and bounded admission comparisons, not final building-pixel parity.

The original SHROUD corpus was also replayed on Steam: its payload remains
byte-identical, SHA-256
`39a019add707e6d11e972aee22fac8404e05c1a4a1123eecb561c4ea895c8f43`.
Only its metadata now records this execution identity and updated oracle source.

## Rust production observation

The [production profile](../map_observation.building-reveal.example.json) loads
retail AnyTown `XMP03T4.MAP` (MIX entry `-854728974`, payload SHA-256
`7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e`).
Starting local MTNK1374 receives an ordinary Move toward `(44,87)`;
neutral CANEWY20 building1349 is at `(46,87)`. A zero-step discovery established
these handles before scheduling the command. Its physical art reader reports
Foundation3x5 and `CTNEWY20.SHP` 260x228,8 frames; the ordinary theater loader
and SHP builder consume the retail assets in the captures.

On RTX3050 Laptop/Dx12, the saved 54-step image
shows part of the roof, 75 and
91 show more of the building while the
read-only anchor visibility remains false. It first becomes true at92;
the 92-step image retains the already
drawn body rather than admitting its whole sprite for the first time.
All seven capture bundles passed the existing map-observation validator,
including final offline revalidation after derived previews were moved outside
the bundles' strict inventory.
The 200-step observer-off replay has identical1920000 BGRA bytes, initial/final
simulation fingerprints and final state hash15818579214581792755.

Release label `building-shroud-reveal-20261006-v1` pins binary SHA-256
`9c2d1476b62cf1bf71eda3c8a3ec659165681e97478dcd96b1001907f6a59e46`.
The receipt records its build-source
identity, actual map/GPU identity, profiles, manifest/frame hashes and endpoints.
Raw capture bundles remain at the receipt's local evidence paths. The checked
source's Rust changes were already present in this executable; the profile,
images and receipt were retained afterwards. No Rust change followed capture.
To reproduce from the checkout's normal release build:

```powershell
$env:WGPU_BACKEND='dx12'
Remove-Item Env:RA2_DIR -ErrorAction SilentlyContinue
python -m tools.map_observation --profile "$PWD/tools/map_observation.building-reveal.example.json" --contract "$PWD/src/app/diagnostics/tactical_capture/contract.v2.json" --cwd "$PWD" --output "$PWD/logs/new-building-reveal"
```

This establishes production integration and a visible pre-anchor reveal on
one retail building route. It does not compare a native whole-game frame,
certify every foundation/height/observer, or cover enabled FogOfWar and colored
effects. No rendering or simulation timer/RNG/state writer was added. The frame
timer in the receipt includes observation and presentation pacing, not GPU time.

Final strict-retail lib suite:9704 passed,239 ignored; Clippy lib passed with
existing warnings. Python map/build-wrapper suite:141 tests,1 skipped. Field
ratchet:2505/2505. The full lib run initially exposed an existing Windows/exFAT
audio test fixture deleting a still-mapped MIX; it now releases its old manager
before changing install state. Its focused test and the repeated full suite pass;
production audio loading is unchanged. The admission regression's expected
pre-fix failure and all actual logs remain in `logs/building-reveal-fix/logs/`.

One fresh read-only critic independently
replayed the native corpus, traced the changed consumers and checked the saved
production evidence. It reported no confirmed implementation defects or blocking
findings; the coverage limits above remain.
