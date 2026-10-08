# Ordinary mobile first-reveal admission

`shroud.py --entity-reveal` executes the original Unit, Infantry and Aircraft
DrawIfVisible entries, preserving their actual vtables and `.text` bytes. It stops
at their actual DrawIt entries; it does not replace those bodies with Python.
`entity_reveal.json` and its provenance sidecar record the executed outputs and
the executable identity.

```sh
python -B -m tools.procedural_drawing_oracle.shroud --entity-reveal --check
```

The existing Shroud/Rally fixture owns map cells, native projection startup,
surface and stack setup. Existing `crate_speed_effect.VTABLES` owns the original
class bindings: Unit `7F5C70`, Infantry `7EB058`, Aircraft `7E22A4`. Their
`+104` entries are Unit `73B0B0` and common Object `5F4B10`; their `+114` body
entries are respectively `73CEC0`, `518F90`, `4144B0`. Native retained viewport
writer `6D5F60..6D5F8D` executes before each class's controls.

All nine class/raw-anchor contrasts (`Cell+12C = 0/08/18`) reach DrawIt, without
reading the prepared anchor cell or entering `586360`, `487950`, or `5865E0`.
Three redraw-not-ready, three limbo and three off-camera controls refuse entry.
Three dead and three unmarked *retained-member* controls still enter: the common
wrapper does not own Display cleanup. These are supplied malformed members, not
claims that ordinary dead objects remain registered. Geometry, clip and visited
entry points are saved per row. Partial08 is a raw-byte control, not an executed
exploration producer or a prescribed Rust visibility state.

The production `tactical_entity_admission` helper is called by both VXL and SHP
builders. Its drawing purpose now preserves lifecycle, transport and DrawState
(cloak/sensor/warp) admission while leaving ordinary anchor shroud to the shared
per-pixel ABuffer. Screen-selection purpose continues to require exploration.
`shroud_pop_in_mobile_admission_matches_original_class_entries` compares twelve
applicable native controls: each class's clear/unrevealed anchor, limbo and
malformed dead retained-member case. Redraw and camera controls belong to the
existing scheduling/builders rather than new state in this helper.

Caller/lifecycle reading establishes the active route: Tactical
`6D8F55..6D909E` dispatches through the common virtual slot; its optional map
query is the original constant-false YR `5865E0`, not an anchor exploration read.
Display `4A9720` inserts according to the object's layer; Conceal `5F4D30`
removes through `4A9770` before setting limbo. Discovery `6F4960` handles
notifications/mission/power and does not remove objects from Display. These
surrounding callers are instruction-established, not an executed full scenario.

The measured admission/projection boundaries perform no RNG draws, timer writes
or detach calls; native redraw-ready state is consumed. Retail class constructors,
full Display scanning, temporal target ownership, transport lifecycle, special
cloak/warp/disguise producers and raster output are outside these controls.
Native shared-SHP `706389..7063EB` has a separate packed special-tint policy;
ordinary Rust DrawState currently supplies no invulnerability tint. This gate
correction does not claim special-tint or whole rendered mobile parity.
In particular Infantry5190D1 calls487950 and5190DA clears its special tint
argument before continuing to draw; admission independence does not establish
that the full DrawIt body has no fog-dependent pixel operations. An unconfirmed hidden-FX
compatibility-composition risk remains outside these ordinary controls.
