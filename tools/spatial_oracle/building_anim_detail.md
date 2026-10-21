# Attached animation detail admission

`building_anim_detail.py` executes the original physical `GAPOWR_A` attached-slot draw through `AnimClass::DrawIfVisible` (`422C70`). It records the original sampled Logic FPS query, retained hysteresis, ordered admission branches and whether the whole call reaches `Surface::DrawShape`. The Building body is a separate draw.

Run from this repository, with the retail installation containing `gamemd.exe`, `ra2.mix`, `ra2md.mix` and its expansion archives:

```sh
VERA20K_GAMEMD_EXE=/path/to/YuriRevenge/gamemd.exe python -m tools.spatial_oracle.building_anim_detail --check
```

`--check` is the default and does not rewrite the reference. Use `--write` only when deliberately generating a new reference. Import and `--help` perform no native execution. The shared `finish_vectors` publisher records command/provenance and guards the declared producer/setup/helper sources against changes during a run. It publishes no executable or repository-source identity hashes. Physical resources are read locally; their SHP/palette payload bytes are not distributed in the corpus.

## Original path and controls

The setup reads physical GAPOWR rules and ART, constructs real Country, House and Building receivers, then creates slot3 through original `451890` and Anim constructor `421EA0`. Stock `GAPOWR_A` retains `DetailLevel=2`, its original frame0, image, noShadow property, light and palette ownership. The same actual slot remains alive across all samples.

Performance counters, timer and latch are supplied empty once. Original setters `55AF40`/`55AF50` install minimum15 and buffer5. For each bucket, the requested number of original Logic counter-prefix visits (`55AFB0..55AFD9`) is followed by original Throttle `55E33B`, including the native clock shift and timer. Only the OS millisecond clock and the caller-checked first-timer CRT exit registration are transported. Every FPS after startup is published by the original bucket; no FPS/latch result is written between draws. `55AF60` is queried only by the actual draw, so its latch changes retain their real encounter order.

The 16 rows comprise:

- Original-published FPS0 and FPS60, each with process Options detail0,1,2.
- Options2 with FPS14,19,20,19,15,14,60, retaining the native latch between rows.
- FPS60/Options2 with an explicitly supplied offscreen camera.
- A fresh original-published FPS14 bucket, first with the offscreen camera and then with the restored camera. This pair makes an early skipped query observable: the draw receipts retain both the actual query count and latch before/after.

Options and camera are declared component inputs; their UI producers are not emulated. No hidden/image/frame flag is forced to manufacture a refusal. A genuine ordinary AI visit follows the sequence to observe the retained animation continuing its lifecycle.

## Ordering and recorded boundary

The ordinary non-RING1 DrawIt path calls `55AF60` at `422FCC` before checking hidden+19D, process Options detail, special-hidden+199/ShouldFogRemove, and the image virtual/null gate. Thus a failed material/image preparation alone does not establish that the original query was absent. Earlier whole-call gates remain distinct: `422C70` checks delay+184, and `Object::DrawIfVisible` (`5F4B10`) performs display/projection admission before invoking DrawIt. The corpus records reached PCs and actual query calls; it does not replace these observations with a host visibility predicate. Delay/hidden/image refusal variants are not executed controls here.

Each row keeps the measured performance fields and query return/latch, ordered gate PCs, DrawIt point/clip, actual Surface-call count/arguments, observed pixel selector/leaf entries, and selected lifecycle/identity/RNG snapshots before/after. A skipped native draw is valid evidence, not a harness failure. The output omits full pixel planes: color/blend equivalence is outside this admission comparison.

The existing base TMP/palette owner supplies its unchanged format globals and runs original Convert construction. The physical type is asserted opaque for every selected Surface call. The admission comparison does not depend on the separate original-mask startup added for translucent raster comparisons, and makes no Convert-mask or pixel-color claim. This producer has no imports from another checkout or from `building_anim_translucency.py`.

The local setup mechanically reuses the established physical GAPOWR construction recipe also used by the populated-slot translucency work. It is a focused standalone setup, not a copied temporal/pixel corpus. Cached lexical INI and named-file transports, admitted alive/placed/Guard Building state, default Country and supplied human/LightGold House flags, sparse flat/revealed Cells, and the Mark/Display registration sinks remain explicit seams. Full map/House INI loading, Unlimbo, GrandOpening, Options UI, Game/Logic/Display scheduling and complete gameplay are not executed. Original text and listed class/pixel vtable ranges are checked unchanged.

## Rebuilt-engine comparison

`app::presentation::instances::shp::building_anim_detail_tests::retail_power_plant_slot_obeys_detail_without_pausing_its_ai` consumes the measured rows through the real power-plant slot presentation path. It replays one retained presentation `DetailState` with its counter/Throttle owner, selects the supplied camera/options, and compares emitted slot presence and immutable performance observations to native outputs. It does not call a second threshold query merely to inspect the latch. The ignored retail test requires the physical inputs; use its `building_anim_detail_tests` filter with the repository's retail test setup.

For a manual gameplay check, build an Allied power plant in a temperate skirmish, keep it visible, and change the application's detail option between high and low. Observe its active animation separately from the Building body; restore high detail and confirm the animation remains live. This is a suggested manual check, not a claim that interactive play was performed. The native counter history is the reproducible evidence for low sampled Logic rate and hysteresis; manual host slowdown is not a controlled substitute.
