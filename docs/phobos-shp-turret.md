# SHP vehicle turret files (`<name>TUR.SHP`)

A non-voxel `VehicleType` with `Turret=yes` may carry its turret as a separate
SHP file, `<image>TUR.SHP`. Phobos detects it by file existence
(`UnitTypeClass_ReadINI_TurretShape @ 0x747A2E`,
`Ext/TechnoType/Hooks.cpp:142-168`), so the mechanism has no type-level INI key
of its own; the one rules key in scope is `[General] ArtImageSwap`.

```ini
[General]
ArtImageSwap=false                          ; boolean, default no
```

The file name is `<name>TUR.SHP`, where `<name>` is the art `Image=` override
when `ArtImageSwap=yes`, otherwise the type's own image
(`Phobos.INI.cpp:262`). An absent file keeps the native body-slice turret.

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x747A2E` | `UnitTypeClass_ReadINI_TurretShape` | For `!Voxel && Turret` only: build `"%sTUR.SHP"` from the ART section's `Image=` when `ArtImageSwap`, else from the type's `ImageFile`; `FileSystem::LoadSHPFile` installs it as `UnitTypeExt::TurretShape`. `Ext/TechnoType/Hooks.cpp:142-168`. |
| `0x73C7AC` | `UnitClass_DrawAsSHP_DrawTurret_TintFix` | Draws the turret at `SecondaryFacing`; `StartFrame = TurretShape ? 0 : WalkFrames * Facings`, and the shape switches to `TurretShape` when present. `Ext/Unit/Hooks.DrawIt.cpp:21-52`. |
| `0x73CCE1` | `UnitClass_DrawAsSHP_TurretShape` (`Ext/TechnoType/Hooks.MatrixOp.cpp:307`) | The matrix-op sibling of the same `StartFrame` choice: `TurretShape` substitutes frame 0 for the body block. |
| `0x73CCF4` | `UnitClass_DrawSHP_FacingsB_TurretShape` | Same `StartFrame` rule on the second SHP draw path (`:57-69`). |
| `0x52D21F` | `Phobos.INI.cpp:256-262` | `Phobos::Config::ArtImageSwap = pINI_RULESMD->ReadBool([General], "ArtImageSwap", false)` — a rulesmd `[General]` key, not a client-INI one. |

## Where VERA20K owns it

- `rules::phobos_globals::PhobosGlobals::art_image_swap` reads
  `[General] ArtImageSwap` (default no).
- `RuleSet::unit_turret_shp_id` / `unit_turret_shp_file` derive
  `<image>TUR` for a `Vehicle` with `has_turret` whose art entry is not
  `Voxel=yes`.
- `rules::animation_sequence::SequenceSet::shp_vehicle_turret` holds the
  turret layout; `build_shp_vehicle_sequences` (from
  `rules::shp_vehicle_sequence`) fills it for non-voxel turret UnitTypes with
  `start_frame = WalkFrames * Facings`, `frame_count = Facings`,
  `facing_multiplier = 1`.
- `sim::animation::resolve_shp_vehicle_turret_frame` resolves the turret
  image: frame 0 when the turret shape is bound, otherwise the body block.
- `rules::shp_vehicle_sequence::shp_vehicle_turret_shape_file` is the single
  resolution owner: it takes [`RuleSet::unit_turret_shp_file`]'s name (type
  gate, `ArtImageSwap` redirect) and answers only when
  `AssetManager::contains` — the `CCFileClass__IsAvailable_MixThenRaw` port
  (`assets/asset_manager.rs:1144`), i.e. registered MIX then startup loose
  catalog — finds the file. That is native `FileSystem::LoadSHPFile` returning
  a shape; nothing else decides whether a type has a turret file.
- `render::sprite_atlas` calls that owner per non-voxel turreted UnitType,
  decodes the file it answers and inserts its frames under the derived stem;
  an absent file binds nothing. It also requests the body's
  `WalkFrames * Facings` turret block explicitly (`shp_vehicle_turret`), which
  need not overlap a `Stand`/`Walk` sequence when `StandingFrames=0`.
- `rules::art_data::ArtData::turret_offset` is the full `[X, Y, Z]` triple,
  read through the Phobos `PartialVector3D<int>` scan: retail's bare `-20`
  fills X, an authored triple fills all three. `shp_turret_offset_screen`
  rotates the `(X, Y)` pair by `turret-bearing - body-bearing`, keeps `Z`, and
  projects all three through `absolute_leptons_to_screen`
  (`Ext/Unit/Hooks.DrawIt.cpp:38-49`).
- `app::presentation::instances::shp` emits the SHP unit turret piece after
  the body, from the bound turret shape or the body slice, at the turret
  facing; it is skipped for dying and disguised units.

## Name-source decision

`unit_turret_shp_id` names the base exactly as native `0x747A2E` chooses it:
`[General] ArtImageSwap=yes` takes the ART `Image=` redirect, otherwise the
type's own `ImageFile` (`ObjectType.image`, defaulting to the type ID). The
two bases can differ when a type sets ART `Image=`, which is exactly the case
`ArtImageSwap` exists for.

VERA's body SHP load still applies the ART `Image=` redirect unconditionally,
so a type with an ART override and `ArtImageSwap=no` can draw its body from the
redirected file while the turret probes `<ImageFile>TUR.SHP`. Native behaves the
same way at `0x747A2E`; the body side's `ArtImageSwap` handling is a separate,
pre-existing gap (see residuals).

## Key location (`[General]`, not a client INI key)

Some task write-ups name `ArtImageSwap` as a `[Phobos]` client-INI option. The
native reader is `0x52D21F` (`Phobos.INI.cpp:256-262`), and it reads
`pINI_RULESMD`'s `[General] ArtImageSwap` with default `false`. VERA already
reads exactly that value through `PhobosGlobals::art_image_swap`
(`rules/phobos_globals.rs:777`, read `:2977`), so this chain adds no second
reader — a second owner for one bool would only fork the decision.

## Phobos quirks kept

- Only a non-voxel `Vehicle` with `Turret=yes` gets a turret file; voxel units
  draw their VXL turret and other SHP units have no turret.
- The turret SHP starts at frame 0 and uses one image per **body** facing
  (the body frame table's octant slot); a body with a non-eight facing count
  keeps its own block size.
- Without the file, the body SHP's `WalkFrames * Facings` block is the turret
  source and the body and turret share one SHP.
- `[General] ArtImageSwap` is a `Valueable<bool>` (default no), so a later
  layer can turn it off again.
- Detection is asset-gated: rules only derive the name;
  `shp_vehicle_turret_shape_file` adds the `AssetManager::contains` test and the
  atlas binds what it answers, so an absent file falls back to the body slice.
- VERA previously drew no separate SHP unit turret at all; this chain adds
  the native body-slice turret piece for every non-voxel `Turret=yes` Unit,
  so existing SHP turret types gain the turret the original engine draws.

## RNG draws

None; file probing and frame selection read no RNG.

## Residuals (trigger · impact · frequency · risk)

- **Z adjust and shadow.** The turret piece now applies the native -32 px
  `zAdjust` on top of the body's computed value and still emits no turret
  shadow, matching the `Game::bDrawShadow=false` exchange around the turret
  `Draw_A_SHP` (`Ext/Unit/Hooks.DrawIt.cpp:51-53`). Trigger: every SHP turret.
  Frequency: modded turret units. Risk: none beyond the shared body gradient;
  the body keeps its own `0`/`-16` adjust.
- **Palette context.** Turret entries use the unit body's `Legacy` palette
  context; a type whose body uses another scheme is not covered. Frequency:
  authored mods. Risk: palette.
- **Late file addition.** The atlas collects new object pairs incrementally;
  a `<image>TUR.SHP` that appears after the type was already covered is not
  picked up until a full atlas rebuild. Frequency: map reload. Risk: no
  turret until reload.
- **Body SHP ignores `ArtImageSwap`.** VERA resolves the unit body through the
  ART `Image=` redirect regardless of the global, while native keeps body and
  turret naming on `ImageFile` until `ArtImageSwap=yes`. Trigger: a type with
  ART `Image=` and the default global. Frequency: types with an ART override;
  retail dormant. Risk: body and turret can come from different stems, and the
  body may load the redirected file where native loads `ImageFile`.
- **Aircraft.** The native hook is `UnitTypeClass` only; aircraft are
  excluded by the `Vehicle` category gate even though the shared SHP sequence
  builder serves them.
- **Draw path reads the atlas, not the owner.** `ShpInputs` / `SimResources`
  carry no `AssetManager`, so the draw code cannot call
  `shp_vehicle_turret_shape_file` itself. It asks whether the atlas bound the
  stem (`turret_shape_present`) and hands that answer to
  `resolve_shp_vehicle_turret_frame`. Trigger: every SHP turret draw.
  Frequency: always. Risk: the two can only disagree when the atlas skipped the
  type, and then the draw falls back to the body slice either way.
- **Draw-path coverage.** The name, existence and frame tests stage a loose
  `<image>TUR.SHP` and run without retail art; the atlas binding and the turret
  draw are compiled but not verified against retail SHP art in this checkout
  (no `ini/`).
