# Per-type SHP building palettes

With the `ares` build feature, each reached TechnoType ART `Palette=` request
can load its own palette for SHP bodies and building `Buildup=` frames. A
non-Ares build retains the original compatibility policy: only types requesting
the first successfully loaded custom group can use that group. A failed request
cannot shadow a valid request or inherit its colors in either build.

The fixed ART section is the type's retained Rules `Image=`, not an ART image
redirect. The native reader retains the last nonempty request across reached
Rules passes, including an Image change whose new ART Palette is empty. An
omitted Rules type section does not read it. `Palette=` is a name without
extension or theater suffix; `Palette=custom` in a temperate scenario resolves
`customtem.pal`. The existing capacity-32 native name reader and PAL decoder
are reused. Missing/invalid assets select the existing ordinary palette fallback.
`TerrainPalette=yes` keeps precedence for the building body and MAKE.

## Ownership and lifecycle

The original [reached binding mechanism](../tools/rules_oracle/techno_palette_binding.md)
owns retention: NativeRulesRegistryState projects the binding into the existing
ObjectType. Its `custom_palette_name()` is the sole request source; ART install
and atlas refresh do not reread or infer it from the final Image. The shared
`RuleSet::load_extra_building_palette` resolves a requested name, or selects the
first successfully loaded retained name when the caller supplies None. Its
optional requested-name argument admits distinct groups only with Ares enabled.
There is one file-lookup/decoder implementation across core and client.

The existing effect-asset catalog retains the successfully loaded names admitted
by that resolver. Original DestroyAnim remapping requires the source type's
loaded group; Ares assigns GetRemapColour for every DestroyAnim, using the
ordinary house conversion when that type has no loaded custom group. Neither
path consults the source building's TerrainPalette flag. The animation freezes
the house and selected palette identity when the producer assigns its remap,
so it remains correct after the building is removed. These identities are
serialized and hashed with the animation, and admitted group names participate
in the existing rules compatibility hash.

Initial atlas construction and runtime refresh use the same requested-name
resolver. A refresh-local name cache avoids decoding a shared palette repeatedly;
it also caches failed lookups for that refresh. The existing sprite atlas owns
all resulting palette rows. Refresh preserves resident rows, sprite rectangles
and index planes; a new scenario creates a new atlas and asset catalog. Resources
are immutable for an active scenario; hot-reloading files inside an unchanged
atlas is not supported.

Palette storage grows beyond the former fixed 256-row allocation, without
truncating the existing u32 sprite row identity. On texture replacement all
registered rows are uploaded and resident index pages bind the replacement
texture through the existing Batch renderer. The actual device 2D texture limit
is checked before installing a refresh: an oversized refresh reports an error
and retains the old atlas, while an oversized initial load fails explicitly.
This remains a hardware ceiling, not an unlimited-palette claim.

The wgpu27 resource model binds concrete texture views through a
[BindGroup](https://docs.rs/wgpu/27.0.1/wgpu/struct.BindGroup.html).
Replacing the row texture therefore rebuilds resident page bindings; retaining
row numbers alone cannot redirect an old binding to the new storage.
The selected height is bounded by the actual device
[`max_texture_dimension_2d`](https://docs.rs/wgpu-types/27.0.1/wgpu_types/struct.Limits.html#structfield.max_texture_dimension_2d),
not by a presumed adapter limit.

## Native evidence and boundaries

The original instruction identities, active callers and reproduction commands
are documented in the [binding reference](../tools/rules_oracle/techno_palette_binding.md).
TechnoType71582B..715852 reads ART Palette with capacity0x20 and calls717820.
Nonempty names bind6263D0's group, including null for a failed file; an empty
read skips the group store. Palette loader627590 constructs theater-specific
filenames. GetRemapColour705D70 reads the actual type's group, while
DrawSHP705EC7 gives TerrainPalette precedence.

Ares hook715857 resumes at715876, removing the original successful-group
limit without replacing that reader or draw consumer.
[Pinned Ares source](https://github.com/Ares-Developers/Ares/blob/4f1d929920aca31924c6cd4d3dfa849daa65252a/src/Misc/Bugfixes.cpp#L76).
Ares441D25 resumes at441D37 (upstream bug231), bypassing the original DF0
test at441D2F. The instructions at441D37..441D41 still call GetRemapColour
and assign Anim+ D4. GetRemapColour705D82..705D9A reads the type's loaded
group; its null-group path705DA1..705DF3 selects the ordinary house conversion.
The subsequent optional palette-name copy441D47..441D61 remains reachable.
Bib, active and super animations, and voxel palette extensions remain outside
this body/MAKE and DestroyAnim chain.

These are body/caller and installed-hook control-flow findings. No new native
arithmetic, RNG draws, timer writes or detach calls are introduced. PAL byte
conversion remains with the existing tested decoder. The earlier original
multiple-name cache/count/history residual remains: this implementation does
not reproduce cache retirement717840, historical-name cardinality, or original
abort behavior for several successful groups in a non-Ares build. It also does
not claim that two distinct historic names bound to one type at different
times retain all original cached objects. The selected path is current retained
body/MAKE and producer-frozen DestroyAnim bindings, including empty-read
retention and missing-file fallback. Native SAV interoperability remains outside
scope: the existing native custom-group load can select scheme zero, whereas
VERA's own snapshot retains the selected house and group.

## Focused regressions

Execution results must be reported separately; the following are regression
coverage, not a whole-scene retail/native pixel-parity certification.

- `ares_palette_requests_use_retained_names_and_resolve_each_loaded_group`:
  production Rules layers and fixed ART, empty final Image Palette retention,
  separate successful files, missing file, and feature-disabled admission.
- `building_palette_groups_survive_refresh_and_palette_texture_growth`:
  ignored GPU regression using the production reader, atlas and Batch shader.
  It loads a late second body/MAKE pair, missing file and TerrainPalette, grows
  the real atlas row texture, then draws a real sprite whose source row exceeds
  255 (the second custom palette ON, TerrainPalette OFF). GPU readback compares
  it with the same production sprite at a low row; OFF also asserts that the
  second body reuses the already-resident Unit fallback row.
  A retained old page is also drawn after rebind and compared with a fresh atlas.
  Surviving animations using the same SHP and house but different groups request
  separate keys after a snapshot round trip, refresh without any donor building,
  and draw against their respective palette rows.
  This test requires a wgpu adapter and explicit `--ignored` execution.
- Existing reached-binding and missing-file regressions remain controls.
  `a_custom_palette_building_gives_its_destroy_anim_the_palette_scheme` and
  `palette_binding_failed_request_does_not_shadow_or_inherit_loaded_group`
  retain stock behavior OFF and assert the unconditional Ares assignment ON.
  `destroy_anim_retains_each_loaded_palette_and_house_fallback_after_save`
  uses real fatal receivers and source retirement, distinct groups, a missing
  file, an ordinary building, TerrainPalette, resource hashing and serialization.
  `retail_statue_bakes_against_the_theater_lib_palette` checks the existing
  retail resource path and requires local retail assets.

The MAKE coverage uses generic synthetic resources. No particular mod's assets,
loader special cases, client configuration or paths are part of this feature.
