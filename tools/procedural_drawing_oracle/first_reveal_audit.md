# Ordinary first-reveal admission audit

Scope: remove incorrectly delayed *whole-object* drawing at the unexplored shroud
frontier, through existing shared production owners. This extends local building
and scenery repairs across the remaining mobile routes; enabled-FogOfWar waves are a separate
research finding with an unported framebuffer renderer prerequisite. It
is not a claim of exhaustive pixel parity, complete FogOfWar snapshot lifecycle,
special-effect parity or future/unimplemented renderers.

| Family | Production owner and remaining admission | Evidence for this correction |
|---|---|---|
| Building bodies, bibs, turrets and attached drawing | `instances/shp.rs` through `helpers::tactical_entity_admission(Drawing)`; lifecycle/DrawState, atlas and camera admission remain | Executed ordinary Building6D9920/43CEA0 controls in [building_reveal](building_reveal.md); prior local repair |
| Vehicles and aircraft, VXL bodies/shadows/turrets | `instances/units.rs` through the same drawing helper; retained Display, transport/limbo, cloak/sensors and projection remain | Original class-entry controls in [entity_reveal](entity_reveal.md); removed the remaining mobile anchor gate |
| Infantry and SHP mobile bodies | `instances/shp.rs` through the same drawing helper | Same native Infantry5F4B10/518F90 controls; shared correction does not depend on type name |
| Static Terrain trees and props | `instances/overlays.rs` Terrain loop; live/Logic/Display/frame/camera remain | Executed6D97D0/71CC50/71C1B0 controls in [terrain_reveal](../spatial_oracle/terrain_reveal.md); prior local repair |
| Cell overlays: rocks, walls, resources, crates, low bridges | `instances/overlays.rs::build_overlay_instances_inner`; its input has no fog state; identity/frame/camera remain | Ordinary47FB90/47F6A0 route read; physical low-bridge executed controls and retail rock reader/render witnesses in [scenery evidence](../spatial_oracle/validation/scenery-reveal/README.md) |
| High-bridge body/shadow/railings | Bridge builders consume retained/resolved cells and physical atlas without exploration input | Existing native bridge raster corpus and [bridge admission controls](../spatial_oracle/bridge_shadow_render.shroud-admission.md); no second anchor gate found |
| TMP terrain/cliffs and smudges | `render/terrain_instances.rs`, `render/smudge.rs`; viewport/live art admission, no exploration input | Production source audit; no gate change or new full-scene native pixel claim |
| AnimClass, bullets, parachutes and supported particles | Retained lifecycle/Display/coordinates/frame/atlas paths in `instances/overlays.rs`, `projectiles.rs`, `particles.rs` | Production source audit found no analogous whole-anchor exploration gate; no new effect raster parity claim |
| Supported wave types0/3 | `fire_effects::build_weapon_wave_visuals` -> overlay geometry -> world draw pass -> shared ABuffer | Original75F9F0/5865E0 executed16 endpoint/flag controls in [wave_admission](wave_admission.md); ordinary flag-clear path already ungated, flag-set fix held for complete framebuffer renderer |

The terrain/TMP, overlay, bridge and unit atlases bind their required art independently
of exploration; the ordinary tactical draw plan consumes retained Display layers rather
than building an exploration-only object list. The shared world fragment path samples
tactical A at the actual screen pixel. Camera, live registration, atlas/frame validity
and depth remain separate admission/ordering concerns.

These knowledge-dependent consumers retain their gates:

- Screen selection, cursor/action targeting, selection brackets and information overlays.
  `cell_visibility_for_local_owner` is still called by input/cursor/commands; it is not
  dead rendering code to delete. Mobile drawing must not expose hidden selection.
- Radar/minimap dots and terrain knowledge publication.
- Positioned sound admission in `building_anim.rs`; its shrouded closure belongs to audio.
- PixelFX sparkles: original6D7840 itself tests Cell+12C bit10 and487950, so this is a
  real native admission gate, not evidence that every fog check should be removed.
- FogOfWar building snapshot lifecycle and special packed-tint/cloak/warp policies.
  Their ownership and behavior are not replaced with ordinary body admission.

No new retail tuning keys, simulation state or authority are introduced. Shared
DrawState remains the special-effect owner; simulation still owns lifecycle/Display,
positions, waves, RNG and timers. The changed Rust draw decisions are read-only and
perform no RNG draws, timer writes or detach calls. New native boundary controls record
those coverage limits beside their results.

Residuals: native sonic/magnetic framebuffer distortion is still approximated by
existing white polygon geometry; wave types1/2 and several specialized renderers are
not implemented. Those can affect their own appearance, but are not repaired by
inventing new pixels for this admission change. Enabled FogOfWar snapshot producers,
arbitrary slope/TMP/scenery destruction cases and20k-unit performance are not certified
by the ordinary first-reveal samples. Existing scenery captures demonstrate individual
ordinary tree/bridge/rock routes, not every art variant or the unidentified shoreline
cluster in the user's video.

Current local candidate validation: the native-backed mobile test failed on the
pre-fix source. All four shroud oracle modes replay successfully on the enrolled
Steam executable. Final strict-retail library tests:9705passed,0failed,239ignored.
Final `cargo clippy -p vera20k --lib`:exit0; existing warning output retained in
local logs. The sim field ratchet remains2505/2505. Serial DX12 release output
comparison validates eight sealed bundles: step110 changes only708 vehicle pixels
before anchor exploration; step111, fully hidden and fully clear controls match
exactly. Checked inputs, clocks, complete observations and simulation fingerprints
match per pair; see [mobile runtime evidence](validation/mobile-reveal/README.md).
One fresh [read-only review](validation/mobile-reveal/review.md) found no confirmed
ordinary-material defect or blocker. It retained an unconfirmed hidden-FX outline
risk in compatibility packed composition (cloak/warp/otherFX with stock A=2),
requiring a separate native material mechanism rather than an invented cutoff.
No special-material or whole-scene native pixel parity is claimed.
The enabled-FogOfWar wave chain remains open with the prerequisites and residual
in [wave_admission](wave_admission.md).
