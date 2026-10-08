# Packed translucent object composition

The existing [blitter oracle](blitter_a.py) owns this extension:

```sh
python -m tools.procedural_drawing_oracle.blitter_a --translucent --check
```

`translucent_blitter_a.json` contains executed original instructions from the
Steam `gamemd.exe`, SHA256
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.
It does not execute a whole original scene. Its metadata records the original
image, source identities, execution policy and supplied inputs.

## Active object paths

Unit primary vtable `7F5C70+55C` points to final composite `73B140`.
The body creates selector `2800` at `73B217`, calls its real visual-character
virtual `+68` at `73B21C`, and selects `2802`, `2804`, or `280A/280C` at
`73B22C..73B249`. The latter alternatives depend on retained signed progress
`Techno+224`. Warp-out/in virtuals `+1D4/+1D8` at `73B25D/73B26B` read
`+270/+271` through `70C5B0/70C5C0` and add bit4. Actual final composition calls
`Convert490B90` at `73B2BE` and the row walker `4373B0` at `73B377`, `73B3E5`
or `73B446`. The Unit's source-cache path `73B470 ->706640 ->706ED0` is a
separate prerequisite; reading its temporary surface alone cannot establish
the final destination blend.

Infantry vtable `7EB058+50C` points to argument-forwarding thunk `41C090`,
which calls shared SHP `705E00` at `41C0E0`. Infantry `518F90` calls that virtual
at `519286/5195E2`; the later direct `4AED70` call also draws ancillary SHP
sources and is not by itself the main object-body chain. Shared SHP `705E00`
selects ordinary translucency bits2/4, then `+800`, native row/depth flags and
`+600`; `4AED70` selects `Convert490E50`. **SHP's cloak character4 route does
not add bit8**: `705E45..705E56` chooses2 or4 from retained progress. Unit and
voxel character4 do add bit8. A shared CPU visual-character owner must not
silently impose the Unit material choice on SHP consumers.

The fixture executes both actual selectors. File-backed globals
`81DC24/81DC28` contain `3000`, so the Unit final `2804` route is `+A4`, not
the alternate `+78` selected when that global mask is zero.

| Selector bits | SHP read slot / leaf | Unit read slot / leaf |
| --- | --- | --- |
| 2 | `+148 /4988C0` | `+A8 /495250` |
| 4 | `+144 /4986D0` | `+A4 /4950C0` |
| 6 | `+140 /4984D0` | `+A0 /494F20` |
| 10 | `+154 /498ED0` | `+B4 /495730` |
| 12 | `+150 /498CD0` | `+B0 /495590` |

The fixture also executes actual `+4000` depth-writing selector controls.
Those controls establish the pixel leaves; they do not assert that Unit's
ordinary final whole-body compositor uses depth-writing flags. With bit4000,
both selectors choose their normal depth-writing2/4 leaves even when bit8 is
present. The JSON's `selectors` rows preserve actual flags, selected slot,
vtable and leaf rather than inferring those outputs in Python.

## Pixel ownership and sampling

The RGB565 masks execute `4BAA73..4BAB06`, followed by original Convert
`48EB56..48EB73` stores. Half-mask is `7BEF`; quarter-mask is **`39E7`**.
Convert constructor `48EBF0` establishes the selected object's palette pointer
at `+4`, A lookup pointer at `+8`, mask at `+C`, and vtable. Full allocator
scaffolding is outside this fixture; its constructor-derived slot objects are
supplied, and original selectors and leaves remain unchanged.

For SHP half blending, `4987FB` reads A, `498800` selects the lookup result,
`498817` reads the resulting palette word, and `49881B` reads destination.
`49881E..498828` shifts/masks the two packed words and adds them;
`49882F` stores the result. Source holes and failed strict Z admission leave
destination intact. The corresponding Unit half leaf is `4950C0`.
Original palette generation, intensity generation and carry behavior determine
the recorded words; there is no Python blend implementation in this corpus.

Stock `light27`, A2, source1, brightness1000 over existing destination `F940`
produces `3840`, `78A0`, `A8C0` for bits2/4/6 respectively. These are executed
leaf outputs. They demonstrate that the native operation retains destination
components under dark source light. A blanket A2 discard would not reproduce
the composition. ColorScheme special source indices and N1 also prohibit
assuming every source at A2 has a black palette word.

Bit8 changes destination reads. In Unit half leaf `495590`, `49563A` loads the
supplied signed word offset, `495641` reads `[destination+offset*2]`, then
`49564E` stores at the original destination. It shades source through A and
the palette before combining it with that neighbor. Offset units are packed
16-bit surface words, not normalized texture coordinates or screen rows.

Negative offsets can read writes earlier in the same scanline. Executed
`scheme53`, A127, bits12, offset-1, sources `[1,240,1]`, destination `F940`
records `DCC3,CBE4,C365`. The preserved ordered memory trace shows pixel1
reading pixel0's newly stored `DCC3`, then pixel2 reading pixel1's new `CBE4`.
An immutable snapshot of the entire object cannot implement that feedback.
The corpus includes signed offsets -1/+1 and a supplied eight-word row pitch
-8/+8 with explicit sentinels. These offsets are leaf controls, not claims
about producer output or native framebuffer pitch.

## Visual-character and offset producers

`cloak_transition` executes real Unit vtables, original Drive constructor
`4AF540`, actual Drive virtual `55ABC0`, `Foot4DA4E0 ->Techno703860`, and
Unit selector body `73B21F..73B259`. The original math uses signed progress
`+224`, signed Rules `+628`, x87 `FIDIV`, original double256 at `7E1710`,
and `ftol7C5F00`. It records the scaled integer at `703A94`, returned
character, final flags and offset. Stage controls include9 and12 as independent
supplied values; retail settings still require the production INI reader.

Inputs cover stages0/1/9/12/13/-1/-9, progress -1,0 through the stage boundary,
and `INT_MAX`, with current-house-owned byte `+41A` and force argument0/1.
The byte is native object ownership, not an independently invented cloak
progress flag. The fixture also executes state0 and fully cloaked state2
for the current-house-owned active-screen route. Fully cloaked owned screen
draw returns character3; forced-null-house input returns5. Allied/sensor
visibility branches and their full detection lifecycle are outside these
controls.

Stage0 is not equivalent to clamping to1: actual progress1/stage0 returns
scaled0 and character1. Negative stage-9/progress1 returns -28 and character1.
These executed exceptional controls document the native result; whether a
production reader admits those inputs remains a separate evidence question.

The Rust finite arithmetic uses signed integer truncation. For positive i32
progress `p` and nonzero signed i32 stages `s`, `256*p` is below `2^39`.
Under the fixture's native `0E7F` control word (53-bit precision, truncate),
division error after scaling is below `2^(-13)/abs(s)`, while a noninteger
`256*p/s` is at least `1/abs(s)` from an integer. An integer result has an
exactly representable intermediate `p/s`. Thus truncation cannot cross an
integer boundary here; the signed64 result is narrowed to low32 with wrap.
Stage zero follows the separately executed masked-invalid low32-zero result.
This argument is bounded by that native precision/control policy.

Shadow caller reading establishes `Unit73C5C4 -> Foot4DB0D0 -> Techno706BD0`:
`706BDD` requires raw cloak state `+220 == 0`; `706BF3` requires
TechnoType `+D98` NoShadow to be false. `StartCloaking703799` with progress
`+224 == 0` can still return visual character zero, so presentation character
alone cannot replace the raw-state shadow gate. These are instruction-level
caller references, not an executed complete shadow lifecycle.

`cloak_offsets` executes full `70BE50` with original secondary Unit vtable
`7F5C54+10 ->410220`, which reads existing native identity at primary object
`+10`, plus original `ftol7C5F00` over raw float `+24C`. The instructions add
the signed results with 32-bit wrap, divide by400 and return signed remainder.
There is no coordinate, framebuffer pitch, frame counter or RNG input in this
body. `+24C` initializes to zero at Techno constructor `6F2D62`; a literal
field scan found no other Techno writer, which does not prove absence of
indirect writes or save-load changes. The supplied fields' lifecycle is not
executed. Offset controls preserve actual raw f32 bytes and cover identity
wrap and negative remainders.

The pixel leaves, visual-character body and offset body perform no RNG draws,
timer writes or detach calls in the exercised paths. The Drive constructor
initializes its own fields, including its current-frame timer seed, as fixture
setup; that does not advance gameplay cloak state. Cloaking/uncloaking tick
timers, transitions and detach remain with the existing simulation owner.

## Coverage and comparisons

The executed payload contains20 selector rows,6136 single-call pixel cases,54
sequential parent replay controls,256
visual-character controls and14 offset controls. The JSON cases use
native-generated N1/N27/N53 plain, LightConvert and
ColorScheme palette/LUT profiles, A0/1/2/63/127/255, indices1/240,
brightness1000/1500, raw destination0/F940/FFFF, native-produced source color
boundaries and destination channel boundaries. `prior_colors`, `prior_depths`,
`colors` and `depths` are directly read from the emulated image memory.
Representative `accesses` rows preserve original read/write ordering; byte
offsets for destination accesses are relative to the passed destination pointer.

The ordinary [blitter_a.json](blitter_a.json) corpus retains its original
payload and command mode. The extended generator's Steam replay produced
byte-identical ordinary JSON with SHA256
`d4d7b5145f5157defbd3a273ed2a3be1b1e8b04d2bb19a445840ff31e82f3e8a`.
This extension is native execution evidence for
isolated producer/selector/leaf coverage. Production Rust tests and GPU
comparisons must separately cite their actual results; this file is not a
claim of whole-game cloak or rendered-frame parity.

## Sequential parent replay controls

`replay_controls` separately preserves54 executed original controls; the existing
6136 `cases` are unchanged. Each replay control calls its actual selected native
leaf three times on one machine with the same retained destination and depth array.
It records `replay_count=3`, `replay_native_leaf`, initial inputs, ordered accesses
and final native `colors`/`depths`. The source retains its zero-index hole and
initial strict depth-rejected third pixel. These are three successive parent
composites, not three source parts precombined into one parent.

The36 primary controls cover SHP/voxel bits2/4/6, read/write Z, A0/2/127 and
brightness1000 using native LightConvert N27. Another18 read-Z controls use
brightness1500 with the same route/selector/A combinations. Repeated read-Z
parents can consume earlier parent destination writes; write-Z controls expose
the equal-candidate rejection after a successful first write. Native outputs
are read only after all calls; no Python blend or expected-value calculation
is introduced. These controls test overlap replay semantics for dependency
scheduling, not independent whole-scene source geometry or all material lifecycles.
