# Phobos landing anim selection (`LandingAnim`, `DefaultLandingAnim*`)

The stock `FlyLocomotionClass::ProcessLanding` effect names a hardcoded
`DROPLAND` for a dropship and `CARYLAND` for a carryall
(`src/sim/world/fly_landing.rs`). Phobos replaces that choice with
`TechnoExt::GetLandingAnim` and widens the effect through three hooks around
the two native anim-index reads:

| Native | Phobos | Effect |
|---|---|---|
| `0x4CEB59` | `Hooks.cpp:2206-2214` | `CL = LandingAnim.isset() || DefaultLandingAnim != null` before the `IsDropship` branch, so any aircraft plays a landing anim while either key is set |
| `0x4CEB7E` | `Hooks.cpp:2216-2221` | dropship arm anim index = `GetLandingAnim` |
| `0x4CEC31` | `Hooks.cpp:2223-2228` | carryall arm anim index = `GetLandingAnim` |

`GetLandingAnim` (`Ext/Techno/Hooks.cpp:2178-2201`) resolves, in order:

1. the aircraft type's `LandingAnim` (`Ext/AircraftType/Body.cpp:41`, already
   read as `object_type.phobos_landing_anim`);
2. for an `IsDropship` type, `[AudioVisual] DefaultLandingAnim.Dropship`, else
   the literal `DROPLAND`;
3. for a `Carryall` type, `[AudioVisual] DefaultLandingAnim.Carryall`, else
   the literal `CARYLAND`;
4. otherwise `[AudioVisual] DefaultLandingAnim` (null by default).

The three globals are read in `src/rules/phobos_globals.rs`
(`Ext/Rules/Body.cpp:602-604`, constructors `Body.h:1003-1005`). The plain key
is a `Valueable<AnimTypeClass*>`: an absent key or `<default>` keeps the
previous pointer, and `none`/`<none>` resolve to null. The two variants are
`Nullable` and also clear on `<default>`. Names are stored unresolved; the
consumer rejects any name absent from the anim catalog, so a typo behaves
like the native failed `Find`: the value stays unset and the class fallback
(`DROPLAND`/`CARYLAND`) applies.

## Behaviour details

- A carryall or dropship keeps its class variant even when the plain
  `DefaultLandingAnim` is set; the plain key is only read for aircraft that
  are neither.
- The plain key widens the landing effect to ordinary aircraft (`0x4CEB59`);
  the two variant keys alone do not, because `force` tests only the type key
  and the plain global.
- A type `LandingAnim` overrides every global and also forces the effect.

## Tests

`src/sim/world/fly_landing.rs` tests, through the production reader and the
real landing callback on oracle row 1 (landing, effect latch clear):

- `phobos_landing_anim_choice_reaches_the_landing_effect` (plain none, type
  key, plain global, carryall fallback, carryall ignores the plain global,
  carryall variant)
- `the_dropship_landing_anim_precedence` (dropship variant, type key first,
  plain global, `DROPLAND` fallback)
- `the_landing_anim_globals_default_and_read_from_audio_visual` in
  `src/rules/phobos_globals_tests.rs` (defaults, authored names, `<default>`
  and `none` handling)

## Residuals

- VERA stores declared anim names without resolving them during the rules
  read; the landing consumer rejects names the anim catalog lacks, so the
  visible result matches the native failed lookup. Frequency: rules authoring
  error only.
- The landing anim's draw flags and lifetime are the existing stock spawn
  path's (`loop_count 1`, flags `0x600`); Phobos changes only the type name.
