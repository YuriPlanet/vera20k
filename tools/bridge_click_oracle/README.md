# Tactical coordinate inverse comparisons

The existing bridge corpus executes original `6D6590` and its bridge decision
block. Its generator and invocation remain `python -m tools.bridge_click_oracle
--check`; see also the sibling `bridge_click_state_oracle/README.md` for the
live-flag consumer. No plane command rewrites `vectors.json` or its sidecar.

## Plane XYZ output

```sh
python -m tools.bridge_click_oracle --plane --check
python -m tools.bridge_click_oracle --plane --write
```

`plane_vectors.json` contains 24 calls to the complete original `6D2280` body,
including its original `5AFB80` matrix multiply and both `7C5F00` integer
conversions on each admitted call. The shared `NativeFixture` executes the
original `6D1DC5..6D1E1E` constructor matrix block. The original image loader
and reference workflow are reused. No instructions or
function results are replaced. Output guards, return pointer and stack cleanup
are checked after each call. Metadata records the command.

Inputs are integer VERA world pixels, including its existing +15 Y row bias.
The incoming native point is `(world_x-camera_x, world_y-15-camera_y)`.
Unlike `6D6590`, `6D2280` does not subtract viewport globals `886FA0/4`:
`ProcessClickCoords692300` adds those offsets back before calling it. Controls
vary those globals independently and retain equal outputs for the same world
point. Camera controls likewise preserve the same world point through the
original body. This does not prove fractional camera/zoom extension policy.

Every case supplies the signed upper-window globals as
`window_width_top_height`: `B0CE30=width`, `B0CE2C=top`, `B0CE34=height`.
The original body rejects incoming `x >= width` or `y >= top+height`, with no
lower bound check. Most numeric controls supply `[32768,0,32768]`; separate
boundary controls supply `[640,20,480]`. On rejection the body copies the
explicit fixture markers in `supplied_invalid_xyz` from `B0CE08/0C/10`.
Those markers establish copying and admission, not native constructor defaults.
Rust arithmetic consumers should select `native_exit == "plane"`; invalid
controls concern caller admission and must not become invented normal XYZ.

`expected_xyz` and `matrix_result_f32_bits` are observed outputs, never Python
calculations. The latter are the original stored matrix result immediately
before the first integer conversion. The fixture uses existing control word
`0E7F` (53-bit precision, round toward zero). In particular world `(2006,15)`
returns `(8558,-8558,0)` with stored float bits `4605BBFF/C605BBFF`. Converting
the widened sum using a normal nearest-rounded final `f32` would produce
`8559/-8559`; the signed pair protects that native rounding boundary. The other
controls cover origin, signed pixels, cancellation, cell edges and map-sized
coordinates. A bounded corpus does not establish all possible float inputs.

The plane answer is separate from the height/bridge-sensitive packed Cell
answer produced by `6D6590`. Display tooltip `4AE5CB` takes this plane XYZ to
Map `565730` and then tests Cell `+12C & 8`. These controls execute neither
that map lookup nor tooltip admission, fog, loading, input dispatch, rendering
or gameplay. They establish a numeric dependency only.
