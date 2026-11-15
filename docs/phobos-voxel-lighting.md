# Phobos voxel lighting keys

Three `[AudioVisual]` keys touch the voxel lighting model. They differ a lot in
what it takes to reproduce them.

## `UseFixedVoxelLighting=` — ported

`Valueable<bool>`, constructor **no** (`Ext/Rules/Body.cpp:368`,
`Ext/Rules/Body.h:273,793`). The Blinn-Phong voxel initializer
(`VXL_Init_BlinnPhong @ 0x00753D00`) computes its halfway vector from the
model-space light plus a **secondary light vector** that stock leaves at
`{0, 0, 1}` — the viewer term. Phobos hooks the six bytes at `0x753D86`
(`VoxelCalcNormals_NullAdditionalVector`) and writes `{0, 0, 0}` instead when
the key is on, so the halfway vector becomes the light alone and every normal's
specular page moves.

VERA: `render::vxl_normals::VoxelLight { world, fixed }` is the draw's lighting
input; `blinn_phong_pages` uses `fixed` in place of the stock `+1`
(`Fpu::load_i32(if fixed { 0 } else { 1 })`), so the stock path stays
bit-identical to the native goldens. `VxlRenderParams.voxel_light` carries it
into `VxlLighting::pages`; `UnitRasterWork` stores the rules' choice
(`VoxelLight::for_rules`) for the whole atlas bake, because voxel sprites are
rasterized there rather than per draw. `diffuse_pages` ignores the flag — that
path has no viewer term, so the native hook cannot move it.

## `VoxelShadowLightSource=` — not a port target

Phobos comments out its read (`Ext/Rules/Body.cpp:353`), its `Process`
(`:947`) and its whole apply block (`:1200-1207`, *"doesn't really impact
anything from my testing"*). The shipped Phobos never reads or applies it, so
VERA must not either; the key is recorded in the scratch scanner's
compile-disabled list with that citation.

## `VoxelLightSource=` — open residual

`Nullable<Vector3D<float>>`, unset constructor (`Ext/Rules/Body.cpp:352`,
`Ext/Rules/Body.h:271,780`). When authored, `ReplaceVoxelLightSources`
(`Body.cpp:1189-1207`) normalizes the vector and sets

```
Game::VoxelLightSource = Matrix3D::VoxelDefaultMatrix * normalized(source)
```

then destroys the voxel caches. `Game::VoxelLightSource` (`0x00887470`) is the
world light the initializer reads; VERA already reproduces the *startup* value
of that global bit-exactly as `vxl_normals::YR_WORLD_LIGHT`
(`VXL_LightDirection_Setup @ 0x00754C00` with the retail angle, executed bits
from `tools/voxel_oracle/lighting.py`).

What is missing is `Matrix3D::VoxelDefaultMatrix`: it is **not** a static
constant — `0x754C00` builds its matrix on the stack (`0x5AE860` construct,
`0x5AF080` angle setup) before `0x5AFB80` produces the light, so the matrix has
to be captured from that execution rather than read out of `.rdata`. The next
step is to extend `tools/voxel_oracle/lighting.py` to dump the matrix object
after `0x5AF080` (asserting `M * v_angle == YR_WORLD_LIGHT`), store its
executed bits as a golden, and feed `VoxelLight.world` from
`normalize(authored)` through it. `VoxelLight` already carries the `world`
field for exactly that, and `VoxelLight::for_rules` is the one place the
rules-derived light is built.

## Tests

`render::vxl_normals::tests` pins the flag's effect (stock and fixed pages
differ, diffuse pages do not) and the rules-to-light selection, including the
`[AudioVisual]`-only spelling. `asset_tools::verb_ini` splits its bare-type
expectations per feature, because the Phobos constructor starts from
`[JumpjetControls]`'s `RulesClass` values (3 / 30 / 0.25) where stock keeps
4 / 14 / 2.0. The native golden tests (`vxl_native`, 59 green) keep the stock
path bit-exact.
