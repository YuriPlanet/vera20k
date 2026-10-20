# Ordinary bridge projectile render inputs

This input witness covers **MTNK → 105mm → Cannon → 120MM.SHP** on the
selected physical Hills.mmx scenario. `120mm` is also a weapon name; it is not
the ordinary MTNK weapon in this witness. Elite105mmE is not executed.

## Executed native owners

| Input | Original execution and result |
| --- | --- |
| MTNK Primary | Full TechnoType constructor710AF0, then Primary/Secondary reader7129AB..712A1D using original ReadString128 and Weapon factory772FA0/constructor771C70: 105mm and null Secondary. |
| Weapon inputs | Full Weapon ReadINI772080 on physical105mm sections: Projectile Cannon, body-reader Speed102 (physical Speed40 through474810, before postpass7729F0), Lobber false. An absent mode/map section returns false and retains values. |
| Gravity | Original constructor6674D6 stores3; AudioVisual66B3C4..66B3E4 reads physical6 through ReadInt5276D0, retained across mode/map. This is not a supplied launch-speed100 assumption. |
| Cannon | Full BulletType constructor46BBC0 and full ReadINI46BEE0, including ObjectType5F92D0 and LoadImage5F9070. Image120MM, Arcing/Shadow true; Inviso/Flat/AnimPalette/FirersPalette/Voxel false; inverse-Rotates true; AnimLow/High/Rate zero. |
| Image and frame | Original image loader requests120MM.SHP twice; unchanged physical48-byte SHP gives24×24 canvas, one frame. Full Bullet ctor466380 followed by original468000/468B90 on the read type returns frame0/Air3. Type pointer assignment is a supplied binding; no flight velocity is invented for this nonrotating type. |
| Default and animation palettes | Original Init_Game52BE61..52BFCE requests ANIM.PAL then PALETTE.PAL, copies/expands physical bytes, runs complete Convert48E740→4BBB00→Blitter_init48EBF0, and stores87F6C0/87F6C4. Both use53 rows, middle26; the original intensity420140 cache is populated and reused. |

The separate [retained ART witness](bridge_render_art_state.md) extends this
with original per-pass state, constructor admission, physical load identity and
54 referenced retail types; this original selected-input payload remains unchanged.
The Weapon postpass7729F0 is outside this input witness. ReadTypeData679A10 runs
that pass after Weapon/Bullet/Warhead bodies; it can replace ROT=0 stored Speed
from Range/Gravity. Thus102 is a body-reader value, not the final retail field.
The selected GetSpeed ROT=0 arm derives its result without using that field.

Seven full-reader controls cover defaults, inverted Rotates, boolean flags,
AnimLow/High/Rate low-byte storage, missing Image erasing the name, Image25
truncation, exact-case keys and absence of AnimType-style ART Image redirection.
Twelve original image-loader controls cover Theater and NewTheater in all six
theaters, with the earlier AlternateArcticArt(+211)/changed-image(+212) prefix
false. The Snow-specific Image mutation is not established by these controls. Native order is Temperate, Snow, Urban, Desert, NewUrban, Lunar.

The selected physical ART120MM section contains no authored scalar keys.
Constructor and retained-reader defaults establish those values. Original
native instructions, not VERA exports, establish every scalar injected into the
connected draw/flight witnesses. `load_retail_type()` and
`bridge_render_inputs_selection.initialize_weapon()` expose those native owners
to the connected corpora rather than copying VERA-interpreted fields.

## Supplied boundaries

- Physical file extraction and INIClass525A60 file loading are not executed.
  Unique selected lexical strings populate original signed-CRC INI caches.
  Scalar parsing, type factories, filename formation and SHP header consumers
  execute natively. ART is fixed; rules layers are RULESMD, absent LANGRULE,
  MPBattleMD, then the exact inner Hills.mmx INI payload.
- Operator new uses bump storage, delete is a no-op, and CRT TLS storage is
  supplied. Original7C8F5E installs the floating scanner. FPCW is0E7F.
- Techno reader prefix/admission and unrelated fields are outside the Primary
  block. The full Weapon reader has an empty sound registry and constructor-only
  AP warhead. No report, damage or full firing-chain equivalence is claimed here.
- Cannon has no Color key. Original474A90 executes with one supplied unused
  ColorScheme default entry, needed for its default-name dereference. This does
  not establish House/ColorScheme startup or FirersPalette ownership.
- Palette startup receives an RGB565 BSurface (native411630 reads bpp2), supplied
  initialized RGB565 shifts/masks, and empty spare-capacity Convert/intensity
  registries. DirectDraw surface creation/device format negotiation are not
  executed. Source palette data, both Convert constructors, shade tables and
  all blitter constructors execute unchanged. Fields named body_plain_vtable
  and body_rle_vtable in the palette diagnostic JSON are merely Convert+88/+8C;
  actual Bullet blitter selection is established by the connected draw corpus.
- Original default-palette construction alone is not rendered output proof.
  The connected `bridge_render*` draw/rowwalker/pixel witnesses own that boundary.

## Physical asset identity

`bridge_render_inputs.json` pins file lengths, SHA-256 hashes, source lines,
the complete48-byte120MM input and its palette indices.

- RULESMD is the743215-byte expandmd01.mix entry8218F9F4; fixed ARTMD is336535
  bytes. MPBattleMD is295 bytes. Hills is the144458-byte inner INI of loose
  Hills.mmx, not the menu's XHills.MAP variant. Their provenance is shared with
  [the landing reader witness](../rules_oracle/bridge_landing_inputs.md).
- Production AssetManager/asset-browser identifies120mm.shp in
  `ra2.mix → conquer.mix`, entry17EDBDA8,48 bytes. It stores one plain4×4 frame
  at(10,10), with12 nonzero and4 transparent indices within a24×24 canvas.
- PALETTE.PAL is `ra2.mix → cache.mix` entry18E978EE; ANIM.PAL is entryA64C5120.
  Both are768 bytes. For example index139 expands to gray92,92,92 in PALETTE
  and244,76,0 in ANIM. These are palette source colors, not final lit pixels.

## Reproduction and Rust connection

Provide the seven files named above in `VERA20K_PROJECTILE_RENDER_ASSETS`, using
`Hills.map` for the extracted inner INI and `120mm.shp` for the original SHP.
The default directory is `$CARGO_TARGET_DIR/asset/bridge-projectile-inputs/extract`
(or `target/asset/bridge-projectile-inputs/extract`). The development receipt
used `/tmp/bridge-projectile-input-assets` and the shared native-oracle venv.

```sh
PYTHONPATH=. python tools/projectile_oracle/bridge_render_inputs.py --check tools/projectile_oracle/bridge_render_inputs.json
PYTHONPATH=. python tools/projectile_oracle/bridge_render_inputs_palette.py --check tools/projectile_oracle/bridge_render_inputs_palette.json
```

Set VERA20K_GAMEMD_EXE or RA2_DIR as required by `tools/native_oracle.py`.
Both commands passed after corpus generation. No native instructions are patched.

`render::sprite_atlas::projectile_tests` compares the production retail rules
reader, exact projectile filename candidates, strict palette ownership, stored
frame/stencil retention and absence of frame fallback. Its ignored
`retail_hills_projectile_assets_match_original_reader_and_physical_bytes`
loads the actual scenario and archives. The canonical focused atlas group passes6 with this archive test ignored;
retained ART tests pass3 and the full40-shape comparison passes both GPU formats.
The physical Hills archive test subsequently passes when invoked separately.
See the chain receipt for the
binary hash and logs. The renderer remedy now passes the reproduced Metal stress case. Final CPU/Clippy
also pass (9544/0/157, Clippy exit0) on unchanged renderer source. Same-source
release/visible validation now passes within the recorded load/continuation/UI
bounds on the earlier release. The single critic found no blocking defects;
both accepted cleanups are implemented. Reviewed CPU/Clippy (efbb1320…), actual
Hills assets and40-shape GPU checks pass. Integrated8ade15f8 now passes9552/0/157,
Clippy exit0, actual retail assets,40-shape and640-fence GPU checks. Fresh
integrated release and visible restoration also pass. Separate identities and limits are in the
chain receipt.

## Required later House scheme chain

Original BulletConstruct466519..46653B captures source Techno+21C House+16054
into Bullet+114 when FirersPalette is set, otherwise stores−1. Current HouseState
and ProjectileVisualState do not retain that authority. Physical RULESMD has six
true native ReadBool results: JUMP, DOGJUMP, ADOGJUMP, GiantNukeUp, GiantNukeDown,
and DredMissile. DredMissile is a dormant definition in the selected data, not
an established active delivery. Cannon does not enter this path.

The production atlas registers only the explicit ANIM.PAL fallback requested
when the retained scheme is missing, preserving the prior appearance through
the same Bullet renderer. Unused per-house variants are no longer packed. An
explicitly supplied-scheme lookup remains testable without claiming live scheme
authority. It is not native FirersPalette
parity. Capturing the scheme at firing, retaining it through capture/source
deletion, save/restore and correct current-player fallback remains a required
separate House-scheme mechanism; this ordinary Cannon witness cannot close it.
