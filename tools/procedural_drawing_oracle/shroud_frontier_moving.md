# Ordinary moving AMCV sight

`shroud_frontier.py --moving` extends the existing stationary GoldSt owner.
It executes an original Move event to cell111,27 from the original AMCV at
Waypoint3 (109,27), follows actual Drive terminal arrivals into Unit739EC0,
then executes original Stop after the final arrival and retains idle state.
It does not supply poses, paths, sight, Cell counters or expected pixels.

```sh
# Configure the installed original executable/retail assets using the normal
# native-oracle environment. The inherited Mission inputs also require both
# VERA20K_SHRAPNEL_INPUTS and VERA20K_ANYTOWN_INPUTS extraction directories.
python -m tools.procedural_drawing_oracle.shroud_frontier --moving --check
```

The sidecar records the replay command. `--write` deliberately
regenerates the reference. The existing stationary command remains
`python -m tools.procedural_drawing_oracle.shroud --frontier --check`.

## Native boundary and consumers

One Mission/PaletteReader VM owns original physical GoldSt Cells, AMCV rules,
construction/Unlimbo, navigation graphs/workspace, object passes, frame commit
and deferred drain. The existing Object and Unit/Foot geometry initializers
and original Drive CRT dispatcher execute before selected source construction.
Object flight scalars matter: uninitialized zero would incorrectly admit a
ground source into Foot's periodic high-flight sight branch.

The compact reference keeps the two actual terminal4B220F reason2 arrivals,
physical crossings with old retained sight, and source/Cell states separately
before the release/update pair, after it, after Unit's border, and after full
native shroud drawing. Unit73AC52..73ACAE calls70B1D0,70AF50, then567DA0 with
current XYZ, height0 and cached radius+3. Cell108,18 distinguishes the border
from a later periodic edge refresh. Native drawing can additionally update
fog-edge cache, so before-draw and after-draw states are not interchangeable.

Current native A is drawn at each arrival. Physical Clear111,22 is drawn
through the existing Cell480350/TMP owner with all eight independent variants
per changed A; the normal production loader chooses its own variant. Stop and
idle reference the final identical A rather than repeat it. Initial A refers
to the existing stationary reference. No full frame/Cell-plane replay is kept.

The normal core test `retail_goldst_amcv_sight_follows_arrivals_then_survives_stop`
consumes event-relative source/cache and Cell ground-channel witnesses. The
client test `moving_amcv_sight_reaches_the_live_shroud_and_tmp_raster` consumes
native A and material rows through the live ShroudBuffer, MatchLighting,
physical atlas, pooled terrain draw and color/depth readback. Presentation
must preserve the simulation hash. Existing stationary coverage remains.

## Limits and effects

This is an isolated native object-pass composition, not a complete native
skirmish startup or a matched whole-frame replay. The inherited supplied
House, sparse Cell/zone registration, empty local residents/overlays, blocked
outside crop, seed0 streams and excluded global Logic phases remain explicit
in the sidecar. Absolute arrival frames differ from normal VERA startup;
event-relative observations do not certify startup RNG or whole-match timing.
The selected route reaches terminal4B220F, not accepted-chain4B1CFD. Other
Unit boarding/grinding/transport/Guard-exit arms remain separate chains.

The effects ledger records actual Scenario RNG calls, mission/movement/blocked
timer writes, occupation changes and Mark calls. The pure sight pair/border
has no own random draw or timer reload. Proper ground admission has no periodic
high-flight sight timer reload. Stop preserves a live retained footprint;
no detach/UnInit is synthesized to finish the control. Those observations do
not establish a no-RNG whole world, all detach paths, or full-function parity.
