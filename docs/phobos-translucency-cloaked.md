# Translucency keyframes (`Translucency=`, `Translucency.Cloaked=`)

Phobos turns both keys from one integer level into an
`Animatable<TranslucencyLevel>` track: an anim can be opaque at its first
frames, fade in, and hold a level. `Translucency.Cloaked=` lets an animation
drawn over a cloaking or cloaked Techno follow its own track.

```ini
[SOMEANIM]                            ; AnimationType / art section
End=20
Translucency=50                       ; plain key: level over the whole anim
Translucency.Interpolation=linear     ; none (default) / linear
Translucency.Keyframe0.Percentage=0    ; 0.0 .. 1.0 of End
Translucency.Keyframe0.Value=0
Translucency.Keyframe1.Percentage=1
Translucency.Keyframe1.Value=75
Translucency.Cloaked.Keyframe0.Absolute=10   ; frame 10 of End, needs End > 0
Translucency.Cloaked.Keyframe0.Value=25
```

- `Translucency.KeyframeN` is read for `N = 0, 1, …` until the first entry that
  fails; `Translucency.Keyframe.ResetValues` is the vector's reset token
  (`Utilities/TemplateDef.h:2264-2314`).
- Each entry takes `Percentage` (`Nullable<double>`, `1.0` is the last frame)
  and `Absolute` (`Nullable<int>`, only read when the anim type's `End` is
  positive; it replaces the percentage with `Absolute / End`), then `Value`
  (`Utilities/Constructs.cpp:232-242`, `Utilities/TemplateDef.h:2330-2368`).
  An entry without a usable percentage ends the walk.
- `Value` keeps only `0`, `25`, `50`, `75`; every other integer is
  `BlitterFlags::None` (`Utilities/Constructs.h:481-501`).
- With no keyframe at all, the plain key's integer becomes one entry at
  `Percentage = 0.0`. A read with `requireParsedFallback` — the `.Cloaked`
  key — adds that entry only when its own plain key parsed
  (`Utilities/TemplateDef.h:2446-2478`, `Ext/AnimType/Body.cpp:113-114`).
- Lookup is `Get(currentFrame / End)` (`Utilities/TemplateDef.h:2390-2441`):
  `lower_bound` then one step back, so a percentage below the first entry
  answers `None`. With `Interpolation=linear` and a following entry the level
  is interpolated, truncated toward zero and bucketed up to the nearest of
  `0/25/50/75` (`Utilities/Interpolation.h:57-64`,
  `Utilities/Constructs.h:464-478`): `0 → 50` halfway gives `25`, a quarter
  gives `12 → None`.
- `None` is native `BlitterFlags::None`, which means "no track answer": the
  draw falls back to the stock instance-ramp ladder rather than drawing
  opaque.

## Native body

`AnimClass_DrawIt_Translucency` (hook `0x423183`,
`Ext/Anim/Hooks.cpp:573-631`) replaces DrawIt's translucency block. It reads
`frames = End` and, when `Translucency.Cloaked` has values and the owner is a
`TechnoClass` whose `CloakState` is `Cloaking` or `Cloaked`, takes the level
from the cloaked track — the plain track is then not consulted at all. A level
of 15 or more skips the draw; otherwise the level's bits are ORed into the
blitter flags (`25 → 2`, `50 → 4`, `75 → 6`).

## VERA

`src/rules/phobos_translucency_track.rs` owns the track: `TranslucencyTrack`
(`has_values`, `keyframes`, `get`), `TranslucencyKeyframe` (percentage +
level, compared and hashed by its percentage bits so a re-read invalidates a
config-keyed cache) and `InterpolationMode`. Its `read` mirrors the native
property read; `get` mirrors `Get`, evaluating per call instead of filling
native's `KeyframeValueCache`, which only repeats the same answer.

`AnimTypeRuntimeConfig::phobos_translucency` and
`phobos_translucency_cloaked` hold the two tracks. Both are read in
`parse_anim_runtime_config` with `absoluteLength` set to the `End` the INI
read produced (before asset binding fills in a missing `End=`), while
`AnimTranslucencyInput::new` divides the frame by the draw-time
`loaded_end(shp_frame_count)` — native reads `OwnerObject()->End` at two
different times for those two numbers.

`AnimTranslucencyInput` carries the already-evaluated `translucency_level`,
`translucency_cloaked_level` and `cloaked_translucency_answers`;
`anim_translucency_selection` uses the cloaked answer when the owner is
cloaking or cloaked and the cloaked track has values, otherwise the plain
answer, and falls through to the stock ladder for `None`. A build without the
`phobos` feature keeps the plain `Translucency=` integer arm.

Callers: `src/app/presentation/instances/overlays.rs::anim_instance_flags`
passes the live owner flag, building slot anims (never cloaked) pass false, and
the sprite-atlas tests go through the same constructor.

## Tests

- `src/rules/art_data_translucency_tests.rs` —
  `keyframe_tracks_interpolate_at_the_native_truncation` (0.5 → 25, 0.25 →
  None), `absolute_keyframes_divide_by_the_animation_end` (20 of `End=40` is
  0.5, and `End=0` ignores `.Absolute`),
  `the_cloaked_track_requires_its_own_plain_key` (absent key and an entry
  without a percentage stay empty), `a_duplicate_percentage_is_dropped_not_fatal`
  (the first entry wins), and
  `interpolation_reads_the_two_tokens_and_keeps_none_otherwise`.
- `src/sim/anim_class.rs` —
  `translucency_cloaked_overrides_on_a_cloaked_owner_only` (the cloaked track
  answers 25% while Cloaking/Cloaked and 50% comes back after uncloaking) and
  `translucency_keyframes_answer_per_frame_and_keep_the_stock_fallback`
  (0/8 → None, 2/8 → 12 → None, 4/8 → 25, 8/8 → 50, cloaked 75).

## Residuals

- A duplicated or negative percentage is a `Debug::FatalErrorAndExit` in
  native (`Utilities/TemplateDef.h:2480-2524`); VERA keeps the first entry,
  drops the offending one and prints a debug-build line. One authored
  mistake then changes one anim instead of ending the process.
- `Translucent=yes`'s per-frame ramp is still resolved before the track
  (`anim_translucency_selection` returns at the ramp). Native has the hook
  above the ramp, so an anim type that sets both `Translucent=yes` and a
  `Translucency=` track draws native's track and VERA's ramp. Trigger: art
  with both keys; effect: a different translucency stage for those frames;
  frequency: rare — retail sets `Translucent=` on smoke-like anims and the
  track is a Phobos key; risk: cosmetic only, no simulation state. Moving the
  ramp behind the track would change stock draws, so it stays documented
  rather than silently reordered here.
- `Value=` outside `0/25/50/75` is not a residual: it is native
  `BlitterFlags::None`, so such an entry answers `None` and the stock ladder
  runs. Likewise a non-numeric `Translucency`/`Translucency.Cloaked` value
  parses as `0` (`TranslucencyLevel::Read` uses `ReadInteger`, C-atoi), so a
  cloaked key with junk text still counts as parsed and shadows the plain
  track — the same as native.
- `KeyframeN.Percentage` is only read through the shared Phobos `Nullable`
  grammar, so `<none>`/`none`/`<default>` and unparsable text leave it unset
  and end the walk; native's `Percentage` reuse path is unreachable here
  because VERA always builds fresh entries.
