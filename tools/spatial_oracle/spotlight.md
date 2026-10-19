# BuildingLight trigger control and detection

`BuildingLightClass` is an ObjectClass child, separate from the temporary
`SpotLightClass` used by drawing. Each `spotlight.meta.json` sidecar identifies
its source retail binary and replay command; the motion and drawing corpora
were generated on different supported retail builds.

- `435820` constructs ObjectClass, enters the `8B4190` class array, derives two
  endpoints from the parent's raw coordinates and FacingCurrent `4C93D0`, places
  the child and explicitly appends Logic after successful ObjectUnlimbo. RTTI
  `4370B0` returns 19; vtable `7E3AD0`. Constructor-array index modulo two becomes
  the retained sweep direction; deletion compacts the array without changing it.
- ObjectUnlimbo `5F4EC0` queries the coordinate at `5F4F34` only outside scenario
  placement scope. This class's entry predicate `4264C0` returns zero, its type
  getter `4369E0` returns null, and inherited Mark `5F5850` has no Techno cell
  footprint. It stays an independent AIR Display/Logic object.
- Execute52 `6DEFBC..6DF04E` traverses live Building order and matches the
  incoming live Trigger through Tag `6E5380`, with health/alive/marked/limbo,
  HasSpotlight and child-pointer gates. It has no House filter. Any match returns
  true. SetBehavior `436BE0` stores the full signed scalar and resets only angle.
  Mode3 scans x-major neighboring ground lists for the first strictly nearest
  enemy Unit/Infantry; an empty scan preserves its previous target and rate.
- AI `4361D0` updates the independent pose before detection. Mode1 oscillates;
  mode2 increments angle by `Speed*4`, subtracting one turn only on `>TAU`;
  mode3 checks target liveness/range and calls shared Lerp `75F540` with float
  0.25. Modes zero/unknown preserve position. A missing/dead parent calls the
  shared ObjectUnInit/deferred-delete owner.
- `436641..4368D4` scans only mode1, a live operational Building parent and a
  nonnull Tag. It visits surrounding ground lists, accepting enemy Unit/Infantry
  without an extra health/alive test. The strict distance bound is
  `Radius+30+ftol(bucket*5.973333333333333)`. The raised event35 carries the
  **parent Building**, zero cell, null source and unforced input, once after the
  scan. HasOccurred35's discrete gate requires raised35 and clear `A8ED6B`.
- PointerExpired `436A00` clears matching parent/target for either control.
  BuildingPointerExpired `44E932..44E93A` likewise clears its `+600` child
  pointer. Building Limbo owns immediate deletion; orphan AI uses the common
  pending Object ticket. Both directions participate in Rust reference cleanup.

## Rules

Rules constructor `6660F3..66613F` initializes MovementRadius=2000,
LocationRadius=1000, Speed=0.05, Acceleration=0.005, Angle=20.0, Radius=175.
ReadGeneral `671748..671816` reads these exact-case `Spotlight*` keys in that
order, through ReadInt `5276D0` / ReadDouble `5283D0`, with current defaults and
no clamp. The retained Process owner preserves them across rules layers and
numeric reset. Retail's authored speed/acceleration/angle are `.015`, `.0025`,
`.5`; the double reader first parses binary32. BuildingType constructor
`45DFDA` clears HasSpotlight; `45FEE5..45FF0D` reads its current byte as default.
The production retail-reader tests establish these inputs separately from the
explicit inputs supplied to the numeric oracle.

## Drawing and shared owners

Draw `435BE0` derives two cone edges through the shared table Asin `4CAD80`,
MatrixRotateZ `5AF1A0` and TransformPoint `5AFB80`. World-coordinate controls stop
at the projection boundary; the Tactical calls are declared argument sinks.
The temporary SpotLight constructor's argument16 is a scale, **not flags**.
Draw `5FF8FF` first indexes stage table `83358C`: stages80..89 select surfaces
64..73 and bypass ordinary scale multiplication. Initialize `5FF5AA..5FF727`
creates disks with peak128 minus six per ordinal and base radius plus ordinal,
then copies even source rows into 256x128 masks. The shared packed RGB565 light
renderer consumes these surfaces in the child's retained Display slot.

The following two lines call DSurface `4BBCA0`, which brightens each old channel
by `channel + ((channel*amount)>>8)`, saturates and repacks. It reads unsigned Z,
uses the shared clipped three-axis walk, samples no A and never writes Z for
this caller. Unknown stages beyond the known90-byte table refuse a draw.
The production surface-line upload/shader owns that operation;
there is no separate searchlight blend pipeline or parent-position fallback.

## Reproduction and limits

From the checkout with a supported retail installation:

```sh
RA2_DIR=/path/to/retail python -m tools.spatial_oracle.spotlight
RA2_DIR=/path/to/retail python -m tools.procedural_drawing_oracle.spotlight
```

`--write` regenerates the consumed controls. They cover five constructor
headings, five motion traces through frame160, three cone geometries, twenty
mask surfaces at two supplied radii, four complete brightening-line store
walks and final pixels, and two complete type16 light draws. The production GPU
tests compare the uncut lines and type16 draws in both supported color formats.
These controls do not establish whole-scenario parity.

The shared clipper intentionally retains its documented nearest-f64/native-chop
edge residual. Three uncut store walks compare exactly; the selected clipped
walk bounds its one-row/Z difference without changing the original golden.

Native lost-target mode3 `436321` submits uninitialized stack coordinates after
resetting mode1. Rust retains the last coordinate for that transition. Invalid
authored zero bucket divisors return zero; invalid negative beam widths refuse
geometry instead of indexing an invalid trig entry. These are explicit safety
policies. Nonfinite authored motion inputs or a failed float conversion preserve
the last drawable pose and skip movement/detection while retaining ordinary
parent-expiry cleanup. The
retained INI values are not clamped; exceptional native floating-state propagation
is outside the finite motion kernel's coverage. The renderer keys its mask cache
by effective Rules radius; exact
native graphics-cache retention across scenario replacement is not established.
Native parent byte `+6E7` has its established constructor-zero state here;
foreign native-save bytes are not imported.
