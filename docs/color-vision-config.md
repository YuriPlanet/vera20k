# Color-vision accessibility: final scope

> Final scope, implemented. This is both the specification and the feature
> doc (linked from `docs/index.md`). Earlier drafts recorded alternatives and
> open questions; they are resolved here. See "Implementation status" for the
> shipped shape and "Residuals" for what still needs validation.

All of this is a VERA-only presentation extension. gamemd has no equivalent,
so there is no native function/address to cite and the native-oracle rules for
gamemd-derived numeric kernels do not apply. Nothing reaches `sim/`, the
network, a save or a replay; two clients may differ without a desync.

## Decision

Ship **one drop-down in Options and one whole-viewport daltonization filter on
the final composed frame.** The choice takes effect on the next frame. The
filter runs last in the frame, after the game world, the sidebar, the UI, the
menus and every overlay have been drawn, so the entire screen is recoloured
together. `Off` is the default and short-circuits the pass.

No strength control, no CVD-safe palette, no shape redundancy, no shared
runtime config file, no Machado matrices. The reasons are below.

## The control

One `Setting::List` on the `Display` page, persisted as
`[VERA] ColorVisionMode=<int>`.

| Row | `ini_value` | Matrix | Meaning |
|---|---|---|---|
| `Off` | 0 | identity | retail image (default) |
| `Not sure? (red-green)` | 1 | deuteranopia | the common red-green correction, recommended |
| `Red-green, green type` | 2 | deuteranopia | deuteranopia / deuteranomaly |
| `Red-green, red type` | 3 | protanopia | protanopia / protanomaly |
| `Blue-yellow` | 4 | tritanopia | tritan defects |

`Not sure? (red-green)` and `Red-green, green type` are the **same matrix**.
The first is a labelling shortcut for players who cannot name their
deficiency, not a fifth filter; its own row says which correction it uses.
Unknown or out-of-range INI values fall back to `Off`, so a hand-edited
profile never enables a filter the build does not know.

The status-line help must fit on one line in every shipped language
(`shell_text::every_status_help_fits_the_status_line_in_every_language`), so
the shipped text carries only what a player needs before flipping the switch:
it applies immediately (unlike `HouseColorStrength`'s "the next battle uses
it"), hues shift rather than the original image being restored, and `Off`
returns to retail. "Not sure? (red-green)" says what `Generic` does; the
blue-yellow model's low reliability and rarity (< 0.01% of men) are recorded
here and left out of the one-line status text. The Settings screen is itself
drawn through the filter, so it doubles as the live preview.

## Coverage: why no strength control

Severity was considered because roughly three quarters of people with a red-
green deficiency are colour-*weak*, not colour-*blind*: their cone response
curves are too close together, not missing. A full-strength dichromacy matrix
is calibrated for dichromats and over-corrects them.

**Product decision: the shipped Team Color Strength (`[VERA]`
`HouseColorStrength=`, VERA PR #3453) already serves that population in
battle.** Boosting saturation and value of the eight house colours addresses
exactly the "differences are compressed" problem that anomalous trichromats
have, without moving any hue. Adding a severity slider would put a second,
harsher correction on top of a gentler one that already works; it would also
need colour-blind playtesting to calibrate, which a normal-vision maintainer
cannot do. So the filter stays binary per type.

The two features are complementary, not redundant, and this is what makes the
pair complete:

| Need | Served by |
|---|---|
| Tell eight teams apart in battle (colour-weak) | band boost (#3453, shipped) |
| Read *all* colour-coded information on screen (colour-blind) | final-frame filter (this spec) |

Band boost touches only the 16-shade `[Colors]` ramps and their consumers
(sprite atlas, voxel ramp texture, radar dots, target lines, sidebar rows).
The filter touches every pixel, including health strips, selection brackets,
ore, superweapon indicators, lasers, menu accents and the Options screen
itself. A colour-weak player who only needs team identity never has to enable
the filter at all.

Residual, recorded but not acted on: band boost does not help colour-weak
players read *non-team* UI colours. If that turns out to matter in playtesting,
the fix is a milder preset of this same filter, not a second mechanism.

## Matrix data (compile-ready)

Simulation shows a normal viewer what a CVD viewer already sees; it does not
help that viewer. Daltonization routes the lost error into a channel the
deficient eye can still separate:

```
M_correct = M_sim + M_shift * (I - M_sim)
```

Reference inputs from open-northland PR #45 (RGB-space simplified ColorJack /
Brettel form, dichromacy only), offered for reuse without attribution:

```
SIM.protanopia    = [0.567, 0.433, 0; 0.558, 0.442, 0; 0, 0.242, 0.758]
SIM.deuteranopia  = [0.625, 0.375, 0; 0.700, 0.300, 0; 0, 0.300, 0.700]
SIM.tritanopia    = [0.950, 0.050, 0; 0, 0.433, 0.567; 0, 0.475, 0.525]
SHIFT_RED_GREEN   = [0, 0, 0; 0.7, 1, 0; 0.7, 0, 1]
SHIFT_BLUE_YELLOW = [1, 0, 0.7; 0, 1, 0.7; 0, 0, 0]
```

Evaluating the formula gives the constants to compile (verified with a small
script; every row sums to 1, so the white point is preserved):

```
CORRECT.deuteranopia = [ 0.625000,  0.375000,  0.000000;
                         0.262500,  0.737500,  0.000000;
                         0.262500, -0.262500,  1.000000]
CORRECT.protanopia   = [ 0.567000,  0.433000,  0.000000;
                         0.303100,  0.696900,  0.000000;
                         0.303100, -0.303100,  1.000000]
CORRECT.tritanopia   = [ 1.000000, -0.332500,  0.332500;
                         0.000000,  0.667500,  0.332500;
                         0.000000,  0.475000,  0.525000]
```

`Off` is the identity matrix. Clamp the transformed RGB to 0..1; leave alpha
untouched.

**These matrices operate on gamma-encoded sRGB values, not linear light.**
PR #45 applied them through an SVG `feColorMatrix` with
`color-interpolation-filters="sRGB"`, and VERA's final frame is an sRGB
target. Read the encoded channel bytes (via the unorm sibling view on native,
see below), apply the matrix to those bytes, and write them back. No
sRGB<->linear round trip. This is also why Machado (2009) is *not* used here:
its matrices are defined for linear-light RGB and its real value is the
continuous `severity` parameter and CIEDE2000 measurement, neither of which
is in scope. Adopting it would add a conversion requirement without a benefit.

The earlier "two disagreeing matrix models" concern is resolved by scope, not
by choosing a winner: the dE00 measurements from #3453 belong to the
CVD-safe-palette chain, which is not being built. If that chain is ever
revived, it must be calibrated separately and must **not** reuse these filter
matrices as its selection model.

## Implementation map

### Setting, enum, UI

| Concern | Location / template |
|---|---|
| Enum shape (`ALL`, `label()`, `choices()`, `ini_value()`, `from_ini_value()`) | `src/ui/main_menu_dialogs/options.rs:318-377` (`HealthBarMode`) |
| Drop-down id | `options.rs:454-464` (`LauncherComboId`) |
| Page placement | `options.rs:1484-1498` (`Category::Display`) |
| Event + value field + `select_*()` | `options.rs:683-725`, `:745-835`, `:1350-1404` |
| Label/help constants | `options.rs:59-141` |
| Caption + status help + popup hit + selected row + face accessor | `src/ui/main_menu_dialogs/options/shell.rs:1072-1080`, `:935-948`, `:981-986`, `:784-790`, `:697-700` |
| Combo face text + open-list rows | `src/app/frontend/skirmish_shell_render/launcher_options.rs:247-257`, `:315-332` |
| Profile field / default / read / write | `src/app/persistence/options_profile.rs:173-177`, `:336`, `:582-586`, `:717-796` |
| **Fixed-size array** | `options_profile.rs:363-371`: `vera: [...; 31]`, key appended last |
| Event chain | `src/app/persistence/options/launcher.rs:24-66`, `:101-148`, `:157-234`; `src/app/shell_main_menu.rs:86-242` |
| Per-frame accessor | `src/app/state.rs:62-66` (`health_bar_mode()` is the template) |
| i18n | `src/i18n/vera20k.*.ini` (10 files); 7 new keys (label, help, 5 rows); `VERA:` prefix; `src/i18n/mod.rs:1426-1432` fails if a language is missing one |

Two non-obvious constraints found while checking the code:

- Append the new key **at the end** of the `vera` array. The test
  `options_profile.rs:1583-1588` asserts a contiguous run of the first ten
  `[VERA]` lines; inserting in the middle breaks it.
- Add the new `Setting::List` to the `every` array in
  `options.rs:2172-2238` (`every_control_is_on_exactly_one_page`), otherwise
  that test fails. There is no golden profile file; the two
  `formatted_sections()` tests derive their expectations from the array.

Put the enum in `src/ui/` alongside `HealthBarMode` (it owns its i18n labels),
and the matrix constants in `src/render/` beside the pass. `rules/` is core and
this is a pure presentation concern; do not put a table there. Keep one owner
for the mode: the profile field, read every frame through one accessor.

### Final-frame pass

| Concern | Location / template |
|---|---|
| New files | `src/render/color_vision.rs`, `src/render/color_vision.wgsl` |
| Module declaration | `src/render/mod.rs:22-111` |
| Pass owner / construction | `src/render/gpu.rs`: `GpuContext` owns a `ColorVisionFilter`; no `RendererState` field, no resize wiring — the pass recreates its snapshot when the frame extent or format changes |
| Insertion point | `src/app/frame.rs` after `timer.end_frame`, before `queue.submit` / `output.present`, plus every other direct present path (see below) |
| Full-screen pass template | `src/render/chrono_screen.rs` (shader_source + encoded views), `src/render/depth_shift.rs` (minimal), `src/render/scaled_copy.rs:404-475` (pipeline + dual-shader test) |
| Same-texture read/write | `src/render/screen_presentation.rs:12-60` |
| sRGB helpers | `src/render/webgl.rs`: `srgb_wgsl()` (shared `srgb_encode` / `srgb_decode` from `palette_light.wgsl`) |

Requirements for the pass:

- One full-screen triangle (`draw(0..3)`), `texture_2d<f32>` + `textureLoad`,
  a single uniform holding the 3x3 matrix. No storage buffers, no compute, no
  reinterpret views — the WebGL2 fallback has none of those
  (`src/render/webgl.rs:1-14`).
- `copy_texture_to_texture(output.texture -> scratch)` first, then render to
  `output.texture` with the scratch bound; wgpu forbids reading and writing
  the same texture in one pass. Both surface paths expose `COPY_SRC`, the
  browser's offscreen `WebPresent` texture included (`web_present.rs:30-33`).
- **One shader for both builds.** The surface texture has no unorm sibling
  view (`gpu.rs:424` sets `view_formats: vec![]`), so both paths bind
  ordinary sRGB views: hardware decodes on load, the shader calls
  `srgb_encode`, applies the matrix, clamps, then `srgb_decode` for the
  store. Stored bytes match on native and WebGL2, with no `encoded_format`
  or `fallback_*` pair needed.
- Skip the whole pass when the mode is `Off`: no copy, no draw, no uniform
  write. That keeps the retail image byte-identical and the default cost zero.
- Validate the single assembled shader through naga, following
  `scaled_copy.rs:457-475`.

**Why this insertion point covers the UI.** The game UI is drawn into
`composition_view` inside `render_game` (phase 4-6 of
`src/app/presentation/render/mod.rs`), but menus, score screens, performance
overlay, status text, screen shake and the software cursor are drawn directly
onto `output.texture` in `frame.rs:1008-1464`. `scaled_copy.wgsl` is only a
zoom/sidebar resampler used mid-frame and is skipped entirely at 100% zoom
(`src/app/renderer_state.rs:62-95` uses a plain `copy_texture_to_texture`).
Only a pass at the very end of `frame.rs` sees every pixel.

**Every present path uses the same filter.** The map-load progress screen is
repainted synchronously, outside the frame loop: `pump/render.rs`
(`present_blank`, `present_native_loading`), `pump.rs`
(`present_saved_game_loading`) and `frontend/startup_splash.rs`
(`render_and_present`) each acquire a frame, submit and call `output.present`
themselves. A filter attached only to `frame.rs` therefore showed uncorrected
colours for the whole load. `GpuContext` owns the filter and publishes the matrix once per frame
(`AppState::color_vision_matrix`); all five present paths call
`gpu.encode_color_vision(...)` before their `submit`. Startup publishes the
persisted mode before the first splash, since no `AppState` exists yet, and
`Off` still skips all GPU work. The test
`every_direct_present_path_encodes_the_shared_filter` fails if a new direct
`output.present` path forgets the call.

**Screenshots and captures stay unfiltered.** All readbacks
(`frame.rs:1471-1576`) are encoded earlier in the same command buffer than
the filter's draw, so they observe the uncorrected frame. No screenshot code
needs changing.

**Browser builds need no second path.** In wasm, `GpuFrame.texture` is
already swapped for the offscreen sRGB `WebPresent` texture
(`src/render/gpu.rs:594-599`); `WebPresent::present` reads its encoded bytes
through a unorm view and copies them to the unorm canvas afterwards. The
filter writes sRGB-encoded bytes into that same texture, so presentation is
unchanged.

## Tests and acceptance

Focused tests through production code, with boundaries rather than sweeps:

1. `Off` publishes no matrix (the pass is skipped); with no lost information
   `correction(I, shift)` stays the identity, and each of the three modes
   expands to the compiled `CORRECT` constants above, with row sums equal to 1.
   A pure VERA-only function needs no native oracle or golden.
2. `[VERA] ColorVisionMode=` round-trips through `apply_ini` /
   `transform_settings`, and unknown values (`7`, `-1`, non-numeric) clamp to
   `Off` while preserving unowned keys.
3. The assembled WGSL (shared sRGB helpers + pass body) parses and validates
   through naga; one source covers native and WebGL2 alike.
4. A source guard asserts every production `output.present(` path calls
   `encode_color_vision`, so a new synchronous repaint cannot silently drop
   the filter (the loading-screen regression above).

Objective acceptance target from #3453's measurements (Machado 2009 +
CIEDE2000, `dE00 < 10` reads as the same colour): raise the worst pairs —
DarkSky/Magenta 2.7 under deuteranopia, DarkGreen/Orange 1.4 under
protanopia, DarkGreen/DarkSky 7.7 under tritanopia — above 10 under the
matching model. A normal-vision reviewer cannot judge how far the hue shift stays
comfortable, so **final sign-off requires one playtest by a
colour-vision-deficient player**; dE00 is a proxy, not a substitute. Also confirm the extra full-screen pass is not a measurable
frame-cost regression at 20,000 units before treating it as free.

## Implementation status

All of it is VERA-only presentation code:

- New: `src/render/color_vision.rs` (compile-time matrices, the pass, four
  tests) and `src/render/color_vision.wgsl` (one full-screen triangle, fed the
  shared sRGB helpers from `palette_light.wgsl`).
- The filter lives on `GpuContext` (`src/render/gpu.rs`), the one owner every
  frame-output path shares. `app/frame.rs` publishes the mode at the top of
  `render_frame` and encodes it before `submit`; the four synchronous loading
  / splash present paths encode the same published matrix.
- Settings: `ColorVisionMode` enum + `Setting::List` on the Display page,
  `[VERA] ColorVisionMode=`, the launcher event chain, a per-frame accessor
  in `app/state.rs`, and 7 new `VERA:ColorVision*` keys in all 10 language
  files. `docs/index.md` links this file as a feature doc.
- Map-load repaints bypass the frame loop, so all five present paths now
  share `GpuContext::encode_color_vision`, pinned by a source guard.

Suggested validation: `cargo check`; the four focused tests listed above;
`cargo test -p vera20k --lib` for regressions elsewhere; a `--profile fast`
native build for a visual check. Game screenshots and capture readbacks are
encoded before the filter and stay uncorrected, so compare the screen itself.

Still unverified: a wasm/WebGL2 build, an in-game image comparison, a
20,000-unit frame-cost benchmark, and a playtest by a colour-vision-deficient
player.

## Residuals

- The matrices, the dE00 figures and the prevalence estimates come from the
  two PR descriptions and published population data, not a re-run against
  VERA's retail palette in this repository. These are dichromacy matrices
  applied to a colour-blind audience; they are deliberately not tuned for
  anomalous trichromacy, which the shipped band boost serves.
- A matrix cannot invent information that is not there. Where a colour pair
  fully collapses, it can pull the pair apart but the result may look like
  neither original colour. Severe cases still need a non-colour cue
  (minimap dot shapes, build-cell marks); #3453's follow-up names it and it
  is explicitly **out of this scope**, not deleted as an idea.
- CVD-safe palettes (#3453's other follow-up) are out of scope for the same
  reason as the severity control: the shipped band boost covers the
  in-battle team-identity need. If revived, they need their own reproducible
  dE00 selection evidence.
