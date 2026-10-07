# YR Wave draw admission at an unexplored boundary

`shroud.py --wave-admission` executes original `WaveClass::DrawIt` instructions
at `0x0075F9F0` to the declared pre-raster call boundaries for type 0
(`0x0075FA47`, consumer `0x0075FA90`) and type 3 (`0x0075FA5C`, consumer
`0x007602E0`). `wave_admission.json` is the executable reference;
`wave_admission.meta.json` pins the executable, emulator, producer and payload.
The enrolled Steam executable has SHA256
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.

```sh
python -B -m tools.procedural_drawing_oracle.shroud --wave-admission --check
```

The existing bridge fixture owns the PE, stack and supplied map setup; the
shared native runner checks that every execution reaches its declared boundary.
No native instruction or callee result is substituted. The 16 controls cross
Scenario bit `0x1000` clear/set, types 0/3, and each combination of raw endpoint
Cell `+0x12C` words `0`/`0x18`. The raw words are prepared negative controls,
not a native reveal traversal and not assertions about Rust visibility flags.

All 16 executions reach the raster call. With the Scenario bit set, the original
endpoint helper at `0x005865E0` executes once; its actual bytes are
`32 c0 c2 04 00` (`xor al,al; ret 4`). The false result branches to the type
dispatch before the second endpoint query. Consequently this active YR draw
path cannot hide a whole wave according to either endpoint's explored state.
The current Rust `Wave::visible_through_fog` predicate can reject both
unrevealed endpoints when FogOfWar is enabled; substituting exploration for
this constant-false native helper is incorrect. With the ordinary retail
FogOfWar=false setting, this predicate always admits and does not cause the
reported ordinary first-reveal pop-in. This evidence is research for the
separate enabled-FogOfWar wave drawing chain, not a completed Rust wave fix.

Original `TechnoClass::Fire_At` has direct constructor calls at
`0x006FF470` and `0x006FF647`. Constructor `0x0075E950` stores vtable
`0x007F6BF4` at `0x0075EA3F`; its `+0x104` slot is `ObjectClass::DrawIfVisible`
`0x005F4B10`, and `+0x114` is this `0x0075F9F0` draw body. The common object
wrapper dispatches the latter after its camera/rectangle admission. These
original byte/caller bindings establish the active route independently of
Ghidra's function names. This fixture starts at DrawIt and does not execute
the constructor, full common wrapper or Display traversal.

The gate instructions perform no RNG draws, timer writes or detach calls.
Every row preserves the supplied Wave object bytes and a Scenario RNG prefix;
constructor/AI/raster lifecycle effects are outside this boundary. The references
cover ordinary endpoint coordinates on a flat prepared map; they do not certify
native distortion pixels, full weapon/retail map binding or complete Wave parity.

Production wave instances use `draw_passthrough_range`, the world camera
binding with the shared tactical ABuffer. Its fragment shader resolves the
source with `tactical_a_at`. This is compatibility source shading, not native
framebuffer distortion: `palette_light.wgsl` multiplies the unindexed white
source by A/127; stock unrevealed A is2 rather than0, so white lines can leave
faint pixels. This is a source-established risk, not a captured native/Rust
wave scene comparison. Removing the whole-effect gate before the destination
sampling renderer exists can expose that residual under enabled FogOfWar.

The existing white polygon edge renderer approximates native framebuffer
sampling and channel modulation; types1/2 are not emitted. Native75FA90 and
7602E0 dispatch per-pixel75EDF0/760190 over sampled destination words. Those
raster producers/consumers need their own complete mechanism, executed numeric
references and production GPU evidence. The ordinary first-reveal candidate
therefore leaves the wave renderer and its existing predicate unchanged, and
retains this mismatch explicitly rather than inventing another white-source
visibility approximation. Trigger: enabled FogOfWar with both endpoints
unexplored; effect: entire supported wave rejected until endpoint exploration;
frequency: dormant with the ordinary retail flag clear; downstream risk:
incorrect effect visibility or exposed compatibility lines if changed alone.
