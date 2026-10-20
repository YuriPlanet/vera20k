# CaptureManager relationship drawing

`control_links.json` contains original Techno704E40 pixels for fifteen supplied
endpoint/color/clock/clip inputs, plus twelve original GetFLH6F3AD0 negative-index
controls. `control_links.meta.json` pins the scope.

The pixel producer reuses `rally.Rally` and original BSurface storage/draw bodies;
only DirectDraw storage/locking and OS GetTickCount are supplied. The separate
GetFLH controls reuse `ifv_fire_coord.spawn_launch(include_negative_flh=True)`:
five explicit alternate-FLH triples, native DRED/Ship setup, indices-1..-6 and both
burst parities. These are prepared component inputs, not a whole native Scenario.
No gameplay instructions are patched; drawing rejects RNG calls.

Extract raw `RULESMD.INI` and `ARTMD.INI` through the production `asset extract`
tool into an external directory. Point `VERA20K_PROJECTILE_RENDER_ASSETS` at its
`extract/` child, and set `RA2_DIR` (or `VERA20K_GAMEMD_EXE`) as in native_oracle.md.
Then run `python -m tools.procedural_drawing_oracle.control_links --write`.
Without `--write` it compares with the committed corpus.

Consumers are the core negative-FLH regression, app control-link pixel test and
the existing ignored `production_action_line_gpu_matches_original_surface`
readback test, which also runs these curves at four zooms/two color formats.
Simulation timer/selection/lifecycle tests and caller instruction reading are
separate from this pixel comparison. Test cameras account for VERA's common
15-pixel world-row bias; no control-link-specific position adjustment is used.

The shared7BC2B0 clip port retains one missed pixel at(88,99) in
`interior_clip`; its CPU comparison bounds that exact residual without changing
the native golden. For that case the GPU check compares with the bounded CPU
raster, while all other control curves compare directly with native pixels.
