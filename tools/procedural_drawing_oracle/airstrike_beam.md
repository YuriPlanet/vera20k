# Boris Tactical airstrike beam

`airstrike_beam.json` is produced by the original executable, using the existing
Rally memory surface, native PE loader and native-fixture RNG seed owner. No
gameplay instructions are patched.

Run `python -m tools.procedural_drawing_oracle.airstrike_beam --write` with
`RA2_DIR` or `VERA20K_GAMEMD_EXE` pointing at a supported executable. Without
`--write` the command compares current original output with this corpus.
No extracted assets are needed: coordinates, types, poses, manager fields,
RGB565 destination and A/Z planes are explicitly prepared component inputs.
Retail configuration/lifecycle remain separate production-reader tests.

Eighteen cases execute complete705860, projection6D2140, color RNG65C7E0 and
actual4BEAC0 pixel bodies. They cover r190/255/256/270, direction, height,
coincident endpoints, clipping, A0/127/mixed, Z rejection, and original opaque
surface calls before and after the beam. Eleven additional rows execute the
actual Tactical6D47FB..6D48FA branch, including Infantry523250/6F3AD0 GetFLH,
Building447AC0 GetCoords, the original sin/cos table lookups and supplied OS
milliseconds. They cover missing/borrowed manager, missing/nonbuilding target,
active true/false and the unsigned clock boundary. This is not a whole Scenario.

Native call evidence:6D47FB requires the object's own Airstrike manager and a
Building target, irrespective of busy/active;6D48D4 gets weapon0's current FLH,
6D4846 gets foundation-center coordinates without TargetCoordOffset/roof height.
The clock wobble truncates AFTER adding each building coordinate.705860 requests
Main886B88 ranged190..270 once, does not inspect IsLine/IsHouseColor/IsLaser,
and emits four segments with three intensity-mixed lines each. Depth starts at
-32-AdjustForZ, and705B15/705C57 accumulate i*deltaZ/4 onto the preceding depth.
When endY<startY it first draws a small opaque red cross. This is neither
LaserDraw nor a separate shadow line.

Consumers: app command/wobble/RNG tests, an explicitly enabled production GPU
readback test (both RGBA/BGRA formats and unchanged depth), and the core retail
sortie/coordinate query regression. The GPU test exercises the shared Mix mode0
and opaque mode4 owner, including action-rectangle conversion in the overlap
control. Native cameras plus15 compensate VERA's common world-row convention.
