# Ordinary animation translucency

From the checkout, with the original game archives beside the configured
`gamemd.exe` and the dependencies in `tools/requirements-test.txt` installed:

```powershell
$env:VERA20K_GAMEMD_EXE = 'C:/your-retail-install/gamemd.exe'
python -m tools.spatial_oracle.anim_translucency --check
```

The default is a comparison without writes. `--write` deliberately regenerates
`anim_translucency.json` and its provenance sidecar for review. Import and
`--help` do not read the executable or run a VM. The payload contains native
observations, not executable identities or retail SHP/palette source bytes.

The physical `NAFLAK` → `FlakWeapon` → `FlakWH` → `SMKPUFF` references establish
a stock producer. The fixture transports the selected physical FlakWH strings
from RULESMD, optional loose LANGRULE, and MPBattleMD through the existing
cached-INI owner. The original Warhead constructor/reader (`75CEC0`, `75D3A0`)
and `SelectAnim48A4F0` choose SMKPUFF from supplied damage 40, land -1 and XYZ.
Original AnimType factory/constructor/ART/image readers (`428B80`, `427530`,
`427D00`, `427B50`) consume the unchanged physical SMKPUFF ART and SHP.
No authored Translucency override or replacement image is used.

Original `Anim421EA0` and `Start424CE0` construct one ordinary animation;
`AI423AC0` advances it to stages 0, 5, 9 and 13. Each sample records the actual clock,
cumulative AI visits and runtime state. No frame or ramp state is assigned to
reach a sample. Full `DrawIt422CA0` then calls `4AED70`; the original Convert
selector, SHP decoder, clipping, row walker and selected pixel routine execute.
The physical type has both `Translucent=yes` and `Translucency=50`: its observed
stage-dependent selector is the relevant result, rather than treating the
explicit 50 as a constant alpha.

The three fresh VMs differ only in their supplied initial surfaces:

| Case | RGB565 background | A word | Z word |
| --- | --- | --- | --- |
| `stock_black` | 0 | 127 | 65535 |
| `stock_midtone` | 48631 (`0xBDF7`) | 127 | 65535 |
| `stock_dim_a` | 48631 (`0xBDF7`) | 63 | 65535 |

Each of the twelve draws starts from those uniform planes. The full 160×160
final color plane is `color_words_rle`: pairs `[count, word]` in row-major order,
including unchanged margins and transparent holes. Expanding all runs must
produce exactly 25,600 unsigned 16-bit RGB565 words. The generator checks the
encoding against the observed bytes. `z_unchanged` and `a_unchanged` assert full
byte equality, independently of the recorded zero Z/A write counts; their
complete expected output planes are the corresponding uniform input words.
No expected color comes from a Python or Rust blend formula. The actual Surface
call retains its flags, point, clip, depth adjustment, gradient, intensity,
selected source frame/crop and reached pixel routines.

The shared `tmp_clear_render`/`bridge_shadow_render` runtime supplies original
palette and native surface infrastructure. Physical ANIM.PAL is expanded and
its actual Convert tables are constructed by the existing palette owner,
independently from PALETTE.PAL. Supplied prerequisites include RGB565 format
globals, empty/spare registry storage, a flat sparse cell neighborhood,
daylight intensity 1000, detail 2, game speed 4, Scenario seed 0, clock 1000,
XYZ (2688,5248,0), loop 1, delay 0 and constructor flags 0x600. Original discovery
geometry startup establishes the shroud/projection divisors. DrawIt receives
point (80,70) and clip (0,0,160,160) as component inputs. This constructor row
uses ZAdjust 0; it does not execute the combat constructor's 0x2600/-15 row.

This fixture opts into original RGB565 mask initialization at
`4BAA73..4BAB06`, with EDX 3/EAX 0/ESI 2 and the declared format globals, before
the full palette/Convert constructors. The original stores produce half and
quarter masks; those constructors populate their pixel objects. Each case
records the startup writes, live globals `8A0DE8/EA`, Convert fields
`+180/+184`, and each translucent selector's actual pixel-object mask at `+C`.
The masks are not assigned to an already constructed Convert or blitter.
Original text remains unchanged across startup. This bounded format startup
does not execute the complete DirectDraw/DSurface creation path. This opt-in
does not change the shared helpers' default setup for other callers.

Inherited heap/delete/TLS/platform and immutable named-file transports remain
declared boundaries. Mark `5F5850` and Display registration `4A9720` are recorded
success sinks; they do not supply a frame, draw, decoder, selector or pixel
answer. The generator requires the original reader/constructor/AI/DrawIt call
chain, completed Surface and selector returns, executed raster routines and no
draw-time RNG entries. It checks the original text range and the exact listed
original class/surface and observed raster-vtable ranges for mutation.

The Rust consumer is
`anim_translucency_gpu_tests::retail_smkpuff_effect_builder_matches_original_translucency`.
It starts from the real Anim constructor/AI and physical indexed atlas, then
uses the production Display-effect builder, draw-plan lowering and GPU replay.
Native execution establishes the selected component's RGB565 output; the GPU
test separately establishes agreement through that Rust path. This does not
execute an original fired shot, Bullet flight/impact, full physical INI/MIX
loader, Display traversal or whole map. Building-slot temporal/inherited state,
owner-remapped canopies and occluded-depth controls are outside this corpus;
unchanged Z at initial 65535 does not establish that Z is never read. This
consumer covers ordinary free Anim bodies with native palette metadata. The
separate [populated Building-slot comparison](building_anim_translucency.md)
owns temporal notice/release evidence and the slot material transport. That
transport is narrower: it requires an ordinary Anim with nonzero selector bits,
an indexed source, palette metadata and an inherited material that is default
except for its remap row. Neither corpus establishes composition with inherited
cloak or displacement. The separate tiled, bouncing, meteor, RING1 and
owner-remapped free-animation paths retain their previous material handling.

Run the actual-emitter comparison separately, with extracted retail INIs in
the ignored `ini/` directory and `RA2_DIR` pointing to the retail archives:

```powershell
$env:VERA20K_REQUIRE_RETAIL_INI = '1'
$env:VERA20K_REQUIRE_RETAIL_ASSETS = '1'
cargo test --locked -p vera20k --lib retail_smkpuff_effect_builder_matches_original_translucency -- --ignored --nocapture --test-threads=1
```

It compares both BGRA and RGBA sRGB targets, including opaque controls,
transparent holes and untouched margins. Rendering must leave simulation
state and RNG unchanged. The separate
[slot guide](building_anim_translucency.md) lists its command-produced lifecycle,
actual slot-emitter GPU comparison and compatibility boundaries; those are not
validation results of this free-animation test.

A manual gameplay check is to let a stock Flak Cannon fire on hostile aircraft
and watch its smoke puffs fade over both dark ground and lighter terrain. That
is a useful whole-scene check separate from this bounded comparison; the oracle
command does not perform it.
