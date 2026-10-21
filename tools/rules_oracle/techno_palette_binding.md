# Reached TechnoType Palette bindings

Scope: the legal original common path with one successful custom palette
name, including SHP body/MAKE and the original Building DestroyAnim consumer.
Ares multi-name rendering is a later, separate change.

## Native control flow established

Select the retail executable with `VERA20K_GAMEMD_EXE` and inspect the
following ranges through the existing `tools.native_inspect` owner. These
commands provide static control-flow evidence, not executed goldens or a
rendered-parity claim. No static byte packet is retained in the repository.

```sh
python -m tools.native_inspect disasm 0x711680 --bytes 0x6
python -m tools.native_inspect disasm 0x7116a7 --bytes 0x6
python -m tools.native_inspect disasm 0x712170 --bytes 0x2a
python -m tools.native_inspect disasm 0x71582b --bytes 0x4b
python -m tools.native_inspect disasm 0x717820 --bytes 0x20
python -m tools.native_inspect disasm 0x6276aa --bytes 0xd
python -m tools.native_inspect disasm 0x705d70 --bytes 0x35
python -m tools.native_inspect disasm 0x441d25 --bytes 0x42
python -m tools.native_inspect disasm 0x45fe50 --bytes 0x20
python -m tools.native_inspect disasm 0x679a84 --bytes 0x24
python -m tools.native_inspect disasm 0x465432 --bytes 0xc
python -m tools.native_inspect disasm 0x7e45d4 --bytes 0x4
python -m tools.native_inspect disasm 0x626f45 --bytes 0x20
```

- Constructor stores711680 and7116A7 clear group DF0 and name DD0.
- ObjectType5F92F8..5F933B reads Rules Image into25 bytes with the old Image
  as default. AbstractType410A7F admits only an existing Rules type section;
  TechnoType712194 skips to716132 on failure. A bounded decode of the whole
  common reader712170..716138 has no other branch crossing the Palette read.
-7157B5 chooses the retained Image section of fixed ART.71582B..71584B
  reads exact Palette, capacity32, default empty (889F64), into DD0;
 715852 calls717820 on the same type.717820..71783E keeps DF0 when the
  new name is empty. A nonempty name stores6263D0's answer, including null
  when627590 cannot load the palette (6276AA..6276B6).
- ReadTypeData679A84..679AA5 visits live BuildingTypes through vtable+64;
  actual Building vtable7E4570+64 is45FE50, which calls712170 at45FE6B.
  Registry672660 uses4653C0;465432..46543D returns the existing type pointer.
  Full_Init6876AC resets types once, reset668A27 processes root rules,
  optional LANGRULE668B05 follows, and68774F subsequently processes map
  rules. Every Process calls ReadTypeData at668EF0. No intervening type reset
  occurs on this path. Thus an admitted later Image change can retain the
  same type's earlier group. A missing [Type] pass does not read it at all.
- Techno GetRemapColour705D70 reads DF0 of the actual type, while DrawSHP
 705EC7 gives TerrainPalette precedence. Body and MAKE use the existing
  atlas path and share the retained request.
- Original Building441D25..441D61 tests DF0 before assigning DestroyAnim
  the owner's custom scheme. A request whose file is missing must not admit
  that remap. The existing immutable effect-asset catalog now records this
  successfully loaded singleton capability; the atlas and catalog call the
  same Rules resource resolver and Palette decoder.

No new arithmetic, RNG draw, timer write or detach behavior is introduced.
Existing Palette::from_bytes owns the native byte conversion and its existing
execution goldens. This change ports reader/control-flow/lifecycle only.

## Ownership and practical regressions

NativeRulesRegistryState -> ProcessedType.techno_references owns the Image
and last nonempty palette request across admitted passes and registry handoff.
Destructive reset reconstructs this per-type owner. ObjectType holds its
read-only projection; installing ART cannot reread/rebind it. There is no
separate global requested-name owner. The single public Rules resource resolver
selects the first retained name whose palette actually loads, and returns both
name and decoded palette. Body/MAKE only apply it to a type requesting that
same name. EffectAssetCatalog is rebuilt from the active assets/theater and
retains the successful name, None before binding; DestroyAnim checks the same
per-type eligibility. A failed request cannot shadow or inherit a different
type's valid group. Missing or invalid palette data uses the existing ordinary
body palette fallback. The resolver is public because the client atlas consumes
it across the core crate boundary.

Focused Rust regressions (execution results must be reported separately):

- `palette_binding_survives_image_changes_handoff_and_asset_refresh`: production
  Rules layers, fixed ART, serialized registry handoff, omitted type section,
  empty final Image Palette, valid/missing theater assets and type reset.
- `palette_binding_requires_a_reached_type_read_and_replaces_nonempty_names`:
  constructor-only entry, later nonempty replacement, literal `none` retained,
  and ART installation that differs from the fixed read snapshot.
- `custom_palette_projects_through_the_rules_image`: production reader fixture.
- `a_custom_palette_building_gives_its_destroy_anim_the_palette_scheme`:
  actual synthetic palette asset admitted through bind_effect_assets.
- `palette_binding_missing_file_keeps_destroy_anim_unremapped`: retained
  request plus absent file leaves the original DestroyAnim remap unset.
- `palette_binding_failed_request_does_not_shadow_or_inherit_loaded_group`:
  earlier A requests a missing palette, B requests lib, map changes A to empty
  Palette ART, and only LIBTEM.PAL exists. The public body/MAKE resolver must
  select lib; actual A/B DestroyAnim producers must deny/admit the remap.

## Explicit residuals

The compatibility policy keeps the first successfully loaded retained group.
This supports the legal original path with only one successful name, including
other failed requests, but does not render several successful custom groups.
It is **not** original626DA0's cache cardinality/abort mechanism: the original
aborts for multiple loaded groups, while this policy retains its first one.
Multiple successful historical names, case/cache ordering, native group cache
retirement/rebuild717840 and missing-file cache entries remain unported.
Trigger: cross-pass or multi-type distinct requests; effect: cache count,
rejection or scheme choice can differ; frequency on official maps is unknown.
This change does not certify these unsupported multiple-name histories, add
Ares behavior, or claim an entire palette mechanism is complete. No official
map or current compatibility scenario has been shown to require the specific
Image-custom-to-empty transition. Rendering/GPU output still needs the normal
production retail-map validation before publication.

Registry serialization includes the new retained binding. GameSnapshot's
existing automatic snapshot-layout description/hash accounts for this field;
no manual version bump is needed. This change does not promise compatibility
with older binary saves.
