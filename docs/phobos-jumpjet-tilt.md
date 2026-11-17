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

## Residuals

- Shadow pose/key chain: the lifted Unit shadow reuses the body's
  resolved `JumpjetTilt` pose (`UnitClass_DrawAsVXL_Shadow` hook `0x73C47A`,
  `Ext/TechnoType/Hooks.MatrixOp.cpp:729-773`): the shadow pose, shadow part
  and frame-local pose key carry the same roll/pitch, and the shadow matrix
  arm reuses the crash rotation product. The shadow keeps its own
  `Shadow_Point` anchor; the `Jumpjet` crash-rocking, `FlyNormal` and
  `Rocket` poses keep the stock shadow policy.
- Shadow stencil raster still untilted: the frame-local shadow template is
  still sampled from the baked untilted stencil; no runtime caller yet hands
  the shadow render params the tilted `body_tilt`, so the Fresh/Retained
  template is not reprojected frame-by-frame through the tilt matrix and the
  high-flight unmasked entry is not tilted. Trigger: a drawn Unit shadow while
  the tilt is active. Effect: the visible stencil keeps the untilted baked
  outline while the body pitches and rolls. Frequency: every frame such a
  unit casts a shadow in view. Risk: visual mismatch only; simulation and
  the body draw are unaffected.
