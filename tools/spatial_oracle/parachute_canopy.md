# Attached parachute canopy draw component

`parachute_canopy.py` records original PARACH construction, owner attachment,
animation advancement and complete canopy drawing on eight declared surface
inputs. Configure `VERA20K_GAMEMD_EXE` to an owned retail executable whose
directory contains the stock archives, then run from the repository root:

```sh
python -m tools.spatial_oracle.parachute_canopy --write
python -m tools.spatial_oracle.parachute_canopy --check
```

`--write` deliberately accepts a freshly executed reference. The default and
`--check` compare without writing. Neither import nor `--help` executes a VM.
Execution errors or failed prerequisites are failures; the runner never switches
to a supplied frame, output pixel, successful placement answer or fallback draw.

## Inputs and executed owners

Two fresh inherited TMP/palette/House VMs have real physical E1 and PARACH type
readers, original Infantry and Walk construction, and the selected E1
weapon/projectile/warhead reader prerequisites. The inherited GAPOWR supplies
House/map/drawing setup only and is never drawn on the measured surfaces.
Immutable MIX/INI resource caches are file transports; original archive and
physical INI parsing are outside this boundary.

The owner is supplied already admitted, alive, marked, nonlimbo and airborne at
`(2688,5248,104)` or `(2688,5248,232)`. These are two fixed component inputs,
not samples of owner descent. Original `Object::Paradrop`'s complete admitted
suffix `5F5A9D..5F5B3E` constructs PARACH, calls `SetOwner424B50`, and obtains
coordinates, remap and cell lighting through the real owner virtuals. Actual
stored and owner-resolved coordinates, `Anim+FC` light and `Anim+100` instance
Z adjustment are recorded separately. No type or instance draw field is fitted
to a measured raster.

Each VM draws the actual constructor frame0, then calls original AnimAI423AC0
until frame20 is reached, bounded to512 visits. Only its declared global clock
advances; no frame, timer, rate, loop or source-image field is assigned. Complete
Logic/MainTick and owner/body AI do not run. Original policy setters, Logic
counter and Throttle timing owners establish the declared FPS/detail prior;
actual host performance is not measured by this component.

Every whole DrawIfVisible422C70 → Object5F4B10 → Anim422CA0 → Surface4AED70
starts with camera `(-384,380)`, viewport/clip160×160, RGB565 background `BDF7`
and A127. Each frame gets two independent complete destination Z inputs:

- `empty`: Z65535 everywhere.
- `left_half_zero`: Z0 at every x<80 and Z65535 elsewhere, on every row.

The split is selected independently of source frames, crop, projected points or
observed pixels. Original selected Convert, decoder, row walker and raster bodies
execute. No host pixel or blending formula supplies an expected output.

## Retained evidence

`parachute_canopy.json` contains the semantic reader/attachment/AI states, actual
draw arguments, selected palette identities and leaves, store coverage, and full
before/after Color/Z/A planes. Planes use decimal `count:word` pairs in row-major
uint16 order. Every plane must cover25,600 words. The projection checks that the
whole-draw and sole Surface-call planes are identical before removing duplicates
and encoding their words as text.

`parachute_canopy.raw.json.gz` retains the complete sanitized observations before
that lossless projection. Publication excludes allocator pointers, raw resource
snippets, lexical INI sections and private file paths. Sidecars describe executed
owners, transports, component priors and exclusions. `finish_vectors` checks all
declared native setup/transport sources before and after generation; source
identities are never persisted. Expected evidence records native outputs, never
an executable identity.

## Boundaries

Object Mark5F5850 and Display registration4A9720 remain recorded return1 sinks.
This corpus therefore establishes neither retained membership/order nor body
composition. Shared `display_anim_owner`/`display_non_entity` controls establish
their generic original mechanisms separately; they are not a joined PARACH
ordering receipt.

Whole Paradrop playfield/navigation admission, Unlimbo/SetLocation, aircraft
flight/cargo departure, owner descent and landed canopy playout are excluded.
The ordinary stock producer/lifecycle regression documented in
[paradrop_coordinates.md](paradrop_coordinates.md) covers the Rust command path
separately. It does not turn these fixed-height component inputs into an original
whole-flight comparison. Terrain/body occlusion and full-scene pixel parity also
require separate original draws and retained ordering evidence.

## Rust rendering regressions

With `RA2_DIR` pointing at the stock game and
`VERA20K_REQUIRE_RETAIL_ASSETS=1`, run:

```sh
cargo test --locked -p vera20k --lib retail_parachute_ -- --ignored --nocapture --test-threads=1
```

The native-plane test constructs an E1 and applies the same declared admitted
Location before constructing its real owner-attached PARACH. Original Infantry
Unlimbo's subcell choice is outside this component boundary. Actual SHP emission
runs first; the shared Anim traversal then emits the canopy at its own retained
parent. The test isolates that unmodified piece and compares all25,600 color,
Z and A words for each of the eight native rows on both BGRA and RGBA sRGB
targets. One source-derived camera translation accounts for VERA's shared world
projection origin; no per-sprite point or depth is fitted to the reference.

The separate interpolation test checks consecutive declared owner poses, an
intermediate render fraction and owner-identity invalidation through the actual
emitter and lowerer. These are presentation-cache regressions, not original
interpolated-pixel or ownership-transfer measurements. The older House-remap
GPU control remains part of the same command.

The complete ordinary American command/flight/landing/playout test remains in
[paradrop_coordinates.md](paradrop_coordinates.md). These rendering tests do not
replace it. The saved native component uses unchanged flat-cell lighting;
retaining native Anim+FC across later relighting is an existing separate gap.
