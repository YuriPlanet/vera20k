# Jumpjet tilt

Phobos can pitch and roll an airborne jumpjet body from its own motion,
without changing how it flies (`Ext/UnitType/Body.cpp:63-67`, consumer
`Ext/TechnoType/Hooks.MatrixOp.cpp:391-523`, vtable `0x7ECD8C`).

```ini
[AudioVisual]                       ; global defaults
JumpjetTilt=false                   ; bool, default false
JumpjetTilt.ForwardAccelFactor=1.0  ; double, default 1.0
JumpjetTilt.ForwardSpeedFactor=1.0  ; double, default 1.0
JumpjetTilt.SidewaysRotationFactor=1.0 ; double, default 1.0
JumpjetTilt.SidewaysSpeedFactor=1.0 ; double, default 1.0

[TILTUNIT]                          ; UnitType, all Nullable
JumpjetTilt=                        ; unset inherits [AudioVisual]
JumpjetTilt.ForwardAccelFactor=
JumpjetTilt.ForwardSpeedFactor=
JumpjetTilt.SidewaysRotationFactor=
JumpjetTilt.SidewaysSpeedFactor=
```

## Behavior

When the stock `TiltCrashJumpjet=` arm has no rocking angle, a live Unit
whose type enables the tilt and whose jumpjet locomotor is airborne, moving
(`CurrentSpeed > 0`) and not held by a Magnetron draws with:

- a forwards pitch from `(accel * ForwardAccelFactor + speed *
  ForwardSpeedFactor) * (pi/2/4)/32`, clamped to `+/- pi/2`, on the
  `RotateY` axis;
- a sideways roll from `speed * SidewaysSpeedFactor * turn_difference *
  SidewaysRotationFactor * (pi/2/4)/(32768*32)`, clamped the same way, on
  `RotateX`, only while the locomotor facing is turning.

Each angle is quantized through `DirStruct` to 128 facings and a zero axis
is skipped, so the draw is yaw then the surviving rotations. This is a
presentation-only pose: no simulation field changes, and the crashed
`TiltCrashJumpjet=` arm keeps precedence.

## Where VERA20K owns it

`rules::phobos_jumpjet_tilt` reads the five `[AudioVisual]` globals and the
five per-type `Nullable` overrides; `ObjectType` carries the nullable type
fields and the draw resolves them against the globals
(`phobos_jumpjet_tilt_settings`). `app::presentation::instances::units`
applies the pose in `body_pose_with_angles`, and
`render::vxl_raster::BodyTilt::JumpjetTilt` renders yaw/X/Y without the
crash arm's translation. The shadow path reuses the same resolved pose:
`shadow_tilt_for_body` gates the `JumpjetTilt` arm onto the lifted shadow
pose/part/pose key, and `render::vxl_shadow` draws the shadow matrix through
the same crash rotation product. In Phobos builds a jumpjet's live rocking angles
are drawn for every type (the `TiltCrashJumpjet=` check moved to the crash
itself) and airborne they take the same no-translation rotation; stock
builds keep the flag-gated crash arm.

## Shadow stencil

`UnitClass_DrawAsVXL_Shadow` (`Ext/TechnoType/Hooks.MatrixOp.cpp:729-773`,
hook `0x73C47A`) writes the same roll/pitch into `shadowMatrix` and, once
either angle reaches 0.005 (`:733`, `:768`), invalidates `vxlIndexKey`. Native
therefore rasterizes that frame's stencil afresh instead of reusing the baked
cache entry, while `shadowCenter = *pt + loco->Shadow_Point()` (`:726-727`)
keeps the stencil's own anchor and only the sections turn around it.

VERA reproduces that request: `render::unit_atlas::UnitModel::shadow_stencil`
rasterizes the type's `ShadowIndex` or `ShadowIndices` sections through
`render::vxl_shadow`'s tilt arm, `UnitAtlas::mask_stencil` composes the body
mask over either the baked template or that re-raster, and
`render::unit_pose_cache::render_tilted_unit_shadow` places the result on
this frame's page keyed by the tilt bits. `app::presentation::instances::units`
routes an active tilt there for both masked arms (positive low flight, and the
grounded first fill when it ever carries a tilt) and for the unmasked
high-flight template, and records the draw as uncached like native's
invalidated key. `shadow_tilt_for_body` applies the same 0.005 gate, so a
smaller pose keeps the stock stencil and the stock key. The `Jumpjet`
crash-rocking, `FlyNormal` and `Rocket` poses still take the stock shadow
policy. Test `render::unit_shadow_tests::a_tilted_shadow_reprojects_its_template`
pins the raster request only: the produced template must differ from the
baked one and a pose under the gate must reproduce it byte for byte. Rendered
parity is not claimed from that CPU test; it needs the maintainer's playtest
of a tilted jumpjet in view.

## Residuals

- Shadow pose/key chain: the lifted Unit shadow reuses the body's
  resolved `JumpjetTilt` pose (`UnitClass_DrawAsVXL_Shadow` hook `0x73C47A`,
  `Ext/TechnoType/Hooks.MatrixOp.cpp:729-773`): the shadow pose, shadow part
  and frame-local pose key carry the same roll/pitch, and the shadow matrix
  arm reuses the crash rotation product. The shadow keeps its own
  `Shadow_Point` anchor; the `Jumpjet` crash-rocking, `FlyNormal` and
  `Rocket` poses keep the stock shadow policy.
- Tilted shadow matrix drops the cell slope: native post-multiplies
  `RotateX(ars)`/`RotateY(arf)` onto `shadowMatrix`, which already carries the
  slope/facing product (`Hooks.MatrixOp.cpp:768-773`), while VERA's
  `BodyTilt::JumpjetTilt` arm builds camera * yaw/X/Y without the slope
  (`render::vxl_raster`) — the same expression the body arm uses. Trigger: a
  tilted jumpjet shadow over a sloped cell. Effect: the stencil lacks the
  slope skew the native matrix would add. Frequency: only while tilting over
  sloped terrain. Risk: visual only, shared with the body draw.
