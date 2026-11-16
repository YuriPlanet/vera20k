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
| `0x73CCF4` | `UnitClass_DrawSHP_FacingsB_TurretShape` | Same `StartFrame` rule on the second SHP draw path (`:57-69`). |
| `Phobos.INI.cpp:262` | `ArtImageSwap` | `[General]` bool, default `no`; gates whether the ART `Image=` redirect replaces the turret base. |

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
- `render::sprite_atlas` probes `<image>TUR.SHP` at atlas build for each
  non-voxel turreted UnitType, decodes it and inserts its frames under the
  derived stem; an absent file binds nothing. It also requests the body's
  `WalkFrames * Facings` turret block explicitly (`shp_vehicle_turret`), which
  need not overlap a `Stand`/`Walk` sequence when `StandingFrames=0`.
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
- Detection is asset-gated: rules only derive the name; the atlas binds it
  when the file exists, and an absent file falls back to the body slice.
- VERA previously drew no separate SHP unit turret at all; this chain adds
  the native body-slice turret piece for every non-voxel `Turret=yes` Unit,
  so existing SHP turret types gain the turret the original engine draws.

## RNG draws

None; file probing and frame selection read no RNG.

## Residuals (trigger · impact · frequency · risk)

- **TurretOffset.** Native `ApplyTurretOffset` (`Ext/TechnoType/Body.cpp:56`,
  Phobos `TurretOffset` FLH) is not applied; the turret is drawn at the body
  anchor. Trigger: a nonzero unit `TurretOffset`. Frequency: authored mods.
  Risk: turret sprite offset.
- **Z adjust and shadow.** Native's SHP turret draw uses zAdjust -32 and
  disables the shadow; VERA reuses the body's Z policy and emits no separate
  turret shadow. Trigger: every SHP turret. Frequency: modded turret units.
  Risk: visual layering.
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
- **Draw-path coverage.** The resolver/sequence/naming tests run without
  assets; the atlas binding and turret draw are compiled but not verified
  against retail SHP art in this checkout (no `ini/`).
