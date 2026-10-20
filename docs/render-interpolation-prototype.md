# Scene render interpolation

This local experiment compares native tick-stepped presentation with read-only
previous/current scene transform interpolation. It is not a simulation-rate increase
or an upstream parity claim. The existing ra2-mono `SnapshotPrevStateSystem`,
`PresentationInterpolationSample` and `RenderTransformInterpolation.Sample`
provided the design reference; the implementation uses VERA's client-owned
presentation state rather than adding interpolation components to simulation.

## A/B controls

- Off by default, so new profiles draw the original tick-stepped motion. The
  main menu's **Options** page has a
  **Render Interpolation** checkbox, using the same preview/profile transaction
  as VERA's other extra options. It is saved as `RenderInterpolation=yes` or
  `RenderInterpolation=no` in `[VERA]` of the active writable `RA2MD.INI` profile.
  Options acceptance and normal exit save the choice; a map load keeps it.
- Press **Ctrl+Alt+I** in a match to toggle
  immediately; the FPS overlay displays `INTERP ON` or `INTERP OFF`. The overlay
  is hidden by default: press **Ctrl+Alt+F** on any screen to show or hide it.
  `INTERP BYPASS` means requested but bypassed (pause, exact capture, or unlimited
  speed). In particular, turn game speed down from unlimited before comparing.
- Set `VERA20K_RENDER_INTERPOLATION=0` or `1` before launching to override the
  saved startup choice. The effective choice is reflected by the checkbox and
  later ordinary profile writes; unset/other values use the saved preference.
- `FPS` counts final-window presentations. `SIM ... TPS` measures committed
  simulation frames per wall-clock second, refreshed every half second. It does
  not count repeated renders as additional Logic updates. The first half second
  after loading has no measured rate yet and displays zero.
- For a clear comparison, use a timed game speed where SIM is below display FPS,
  then move a group of tanks diagonally, turn them, and enter a tank bunker.
  Switch while the same movement is in progress. Neither the switch nor this
  prototype changes the selected game speed.

## Sampling contract

The client retains two committed transforms per live Techno and dynamic Display
object and samples between them using the existing frame-pacer bucket phase /
pacing budget. Slow
simulation/render preparation does not restart presentation time at its end.
Only new Logic frames rebuild the endpoint cache. Screen projection includes
sub-cell XY and exact height; body and turret facings follow the shortest arc.
The same sample phase is used throughout the tactical scene:

| Consumer | Sampled presentation |
| --- | --- |
| Units, infantry, aircraft and building guns | Position/height, shortest-arc hull/turret/barrel facing, pitch/roll, rocket pitch, slope transition, building gun recoil and ion-blast lift |
| Unit-owned pieces | Shadows (including upstream's aircraft shadow position/facing policy), health/bracket/rank/group/status pieces and parachute canopies; height cancellation for depth follows the sampled height |
| SHP/VXL projectiles | Position/height, directional SHP frame and quaternion-sampled voxel orientation, body/shadow anchors and draw depth |
| Dynamic animations and particles | Resolved absolute owner-relative animation coordinates; independently moving smoke/gas/fire/spark particles; particle-system lights |
| Voxel debris | Position/height, shortest-arc tumble orientation and projected ground shadow |
| Moving effect geometry | Following electric-bolt sources, projectile trail samples, spotlight endpoints, sonic/magnetic polygons and airstrike designation endpoints |
| Tactical interaction and lines | Hover/click/band-box/type-box picking, tooltip/health hover, selected action/psychic/control links, planning geometry and mobile rally targets |

Existing constructors, effects, lifetimes, sprite animation counters, palette
admission, fog and Display registration/order retain their owners. Fixed-endpoint
LaserDraw and RadBeam effects deliberately keep their copied birth coordinates:
they are not owner-following effects and must not bend as the shooter moves.

The technique deliberately displays about **one logical frame behind** authority.
It clamps at the latest committed transform rather than extrapolating during a
stall. Camera movement remains independent. Pause, exact tactical captures and
unlimited game speed snap to authority. Loads, new objects, missed frame history,
type changes, bridge transitions and jumps greater than 1.5 cells discard old
endpoints. Bullets additionally allow their committed velocity-sized step so
ordinary fast projectiles are not mistaken for teleports. Warped-out, warping-in
and limbo Technos are not interpolated.

No interpolation state is serialized, hashed or written into the simulation.
No movement, facing timer, command, RNG, collision, occupancy or combat update is
changed. The Ctrl+Alt+I chord is local presentation input, not a game command.

## Prototype limits / player acceptance

- Body/turret orientation still uses the existing atlas direction precision;
  this is not arbitrary-angle rerasterization or optical-flow frame generation.
- SHP/HVA animation counters, birth/death, teleport and gameplay state changes
  remain discrete. This does not fabricate extra animation artwork or crossfade
  different SHP frames. Their moving anchors and directional frame selection do
  use presentation samples. New objects have no previous lifetime to sample.
- Radar dots remain native integer cell/bucket observations with their existing
  fog/detection/event lifecycle, not continuously moving tactical sprites.
- Draw-only geometry (planning, spotlight and following effect endpoints) keeps
  a lazy per-Logic history at its existing producer. Its first observed frame,
  identity/owner changes and missed history snap rather than invent old points.
- Both modes run on the same thread. A long simulation stall can still block a
  window presentation; interpolation does not fix CPU spikes or make 20k-unit
  performance claims. The measured hook overhead and its optimization history
  are in PR #2263's description; they cover cache/sampling hook cost, not
  whole-game FPS.
- I have entered a map and observed the interpolation in play; the smoothness
  improvement is more noticeable at slower game speeds. That live observation
  is separate from CPU probes and does not establish save/load regression or
  long-session stability. Those remain to be checked alongside pause, teleport,
  camera scrolling and response latency; builds alone do not prove them.

Focused checks: `cargo test -p vera20k --lib --no-default-features render_interpolation`
and the `fps_overlay` filter. The normal game build keeps its default features.

## Windows startup stack budget

The game binary now explicitly reserves an 8 MiB Windows stack via `build.rs`:
`/STACK:8388608` for MSVC, or `-Wl,--stack,8388608` for GNU Windows. Cargo scopes the
option to the `vera20k` binary, so resource tools, test executables, libraries and
non-Windows targets keep their existing policies. Only reserve is specified;
initial commitment is not increased (the checked MSVC build retains 4 KiB).
Unused reservation is virtual address space, not an upfront 8 MiB RAM allocation.

The default 1 MiB MSVC reserve proved insufficient for the current development
startup path. `App` contains `AppState` inline, and nested entry-point,
initialization and rules-loading frames retain large values and temporaries.
In a paired object-code probe using identical current libraries and opt-level 1,
the `App::new_spawn(options)?` arm added 223,120 bytes to the entry-point frame,
even for ordinary launches, since that frame is reserved before the mode branch.
That upstream CnCNet path arrived with the main synchronization; the first dense
interpolation-history optimization added only 928 bytes across three startup
frames, with another 48 bytes after aircraft reuse and no increase in the final
zero-height projection optimization. This is stack-budget evidence, not a claim
that an AMD driver or retail resource format became incompatible.

An independent copy of the same executable, with only its stack-header reserve
and checksum changed, reached initialization complete and the main menu at 8 MiB.
Its path/basename differed, so that observation is not a strict same-path live A/B.
The permanent build policy preserves CnCNet, interpolation, simulation and assets;
it is not a removal of either feature or a reduction of big-value copies. Build
and PE-header verification are distinct from live startup, map loading and
save/load acceptance; report only the checks actually completed for the PR.
