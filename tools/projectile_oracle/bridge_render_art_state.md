# Retained Bullet ART and image-load state

This extends the ordinary Cannon input witness with **22 control sequences / 52
layer rows**, constructor snapshots, and **54 physically referenced retail
projectile types** through three present rules sources. It executes original
BulletType constructor46BBC0 and full reader46BEE0, including ObjectType5F92D0,
integer/bool/string readers, Animation FindOrAllocate428B80 and the original SHP
filename/header consumer. It supplies no VERA-derived scalar expectations.

The binary SHA-256 and Unicorn versions are in
[the sidecar](bridge_render_art_state.meta.json); exact results and file hashes
are in [the corpus](bridge_render_art_state.json). The inherited physical asset
provenance and extraction instructions are in [bridge_render_inputs.md](bridge_render_inputs.md).

## Established read order

1. ObjectType5F92F8..5F933B reads Image into its 25-byte buffer using the previous
   text as default. Its ART reads retain Theater(+22C), NewTheater(+237) and
   Voxel(+236). For a non-Voxel Bullet,5F963A..5F964D calls LoadImage5F9070.
2. Bullet46C1CC..46C1ED reads Image again, this time with an empty default.
   Trailer(ReadString128/FindOrAllocate), SpawnDelay(signed ReadInt), inverse
   Rotates and Flat are guarded by the resulting nonempty text.
3. AnimLow/High/Rate at46C37E..46C3D5 are unguarded. Each uses the zero-extended
   retained byte as default and stores only AL. AnimPalette46C3DB..46C3F2 uses
   the retained bool. These reads still use the base Object-selected ART section:
   the original INI cache keys its lookup by the Image buffer's pointer, not by
   the now changed text at that pointer.
4. At46C3F8..46C406, !Inviso calls LoadImage again, including an empty Image.
   That attempts `.SHP`, then `.GHP`; failed lookup replaces the old +A4 pointer
   with null. Inviso suppresses only this suffix load. The separate Voxel asset
   loader at5F8110 is a declared substitution, not a model-binding proof.

Thus current Image text cannot be used as the SHP binding authority. Every
original5F9070 entry records its attempted text/Theater/NewTheater descriptor;
actual +A4 is separately read and reverse-mapped to supplied physical allocations.
A constructor can contain its type ID as Image while still having no image load
or pointer. The production atlas consumes the retained last-attempt descriptor,
then resolves real bytes; it does not infer a read from constructor text.

Physical controls execute ordinary120MM → omitted Image → second omission →
explicit missing → recovery. Visible types lose their SHP on the first omission.
For Inviso, the first omission can retain the base-prefix120MM load while current
Image is empty; the next omission clears it. An invalid Inviso value retains
true through the original bool reader. A type named120MM with no authored Image
can load its base-prefix SHP but still finish with empty Image and Inviso=true.

## Cache lifetime and supplied boundaries

Original ReadTypeData679A5D..679A82 sweeps AnimTypes before BulletTypes. Retail's
registered D has no ART section, but its original ReadINI427D00 still calls
INI::ClearCache526B00 at427D13 before returning false. Each fixture executes
that concrete predecessor; it does **not** manually reset the ART cache.
Within the Bullet reader the cache reuse described above remains unchanged.
This is a bounded active-retail predecessor, not execution of the full global
RulesClass loop. Custom stacks lacking any preceding Animation/other ART read
still require the shared INI-cache lifetime mechanism; these rows do not prove
that larger mechanism or infer a different default from an isolated stale cache.

Physical-loader-style lexical strings build supplied signed-CRC INI indexes.
Empty/whitespace values and entryless sections are omitted before those indexes
are created. INI file/archive walking itself is not executed. The fixed ART
snapshot and RULESMD → absent LANGRULE → MPBattleMD → exact Hills inputs retain
their physical source hashes. Duplicate required ART sections would be explicitly
excluded; none occur in the current selected54-type set. Reference discovery
from physical Projectile keys is a supplied selection, not native registry
traversal. Only120MM.SHP is installed in this small asset fixture: other families'
scalar ART reads are established, but missing foreign image files cannot prove
their successful model/SHP bindings or rendering.

The filename controls enter LoadImage5F9070 with AlternateArcticArt(+211)
and its changed-image byte(+212) false. Its earlier Snow-specific Image mutation
is outside the filename claim. A physical lexical lookup across all54 selected
projectile IDs and their selected image sections finds no AlternateArcticArt key
in RULESMD, MPBattleMD, Hills or fixed ARTMD. This gate is dormant for those retail
inputs, not unreachable for custom data. The generic Arctic Image mutation and
its retained-state restoration require their own Object image-loading chain.

Allocation/delete/TLS/default Color storage and archive callbacks use the
existing input harness boundaries. No reader/default/scalar/reference result is
replaced. LoadVoxel5F8110 returns immediately after the relevant scalar reads;
its globals, models and drawing are outside this witness. Trailer identity,
sentinel and SpawnDelay reads are covered; trailer emission/lifecycle is not.

## Production ownership and checks

`native_processing::ProcessedType` privately retains `ProjectileArtState` across
all passes and registry handoff. `RuleSet::from_processed_rules` projects that
state once. Late `install_art_data` no longer rereads Projectile ART. The atlas
uses the last image-load descriptor and keeps its normal Inviso/Voxel draw gates.
The configuration hash includes these effective fields (version8), including
retained Inviso, to reject a restore against different frame/launch/binding
inputs. Nonprojectile late ART consumers retain their existing owner.

The Rust checks are in:

- [projectile_art_tests.rs](../../src/rules/projectile_art_tests.rs): all52 control
  rows through cumulative source layers and process-registry handoff, constructor
  state, actual registered retail types, configuration-hash/restore mismatch.
- [sprite_atlas_projectile_tests.rs](../../src/render/sprite_atlas_projectile_tests.rs):
  original physical120MM bytes through the actual asset loader/atlas binding,
  including no-read constructor, successful/failed loads, frame count and canvas.
- [projectile_type.rs](../../src/rules/projectile_type.rs): late ART changes cannot
  overwrite the already retained Bullet state. Retail FireAt fixtures now use
  the fixed-ART processing constructor before their other-family merge.

Native regeneration and independent `--check` pass. On the canonical focused
candidate recorded in the chain receipt,
all three retained ART tests pass, including39 registered production types out
of54 physically referenced native types. The atlas group passes6 with its actual
retail-archive test ignored; all40 full native shape cases pass in both GPU
formats, including the controlled AnimPalette path through production reading.
Logs: `/tmp/bridge-projectile-cpu-{art,atlas}.log` and
`/tmp/bridge-projectile-gpu-shape-v2.log`. The later full retail suite and live simulation restore pass before the renderer
remedy. The later renderer remedy passes dense512-pair stress and all18 GPU correctness
checks. Final CPU/Clippy also pass on the unchanged renderer source:9544 passed,
0 failed,157 ignored; Clippy exits0 with953 warnings. Same-source release load,
live-flight/collapse/debris continuation and ordinary visible restore checks also
pass within the earlier-source chain receipt. The single fresh critic completed
with no blocking defects; its two accepted cleanups are implemented. Reviewed
CPU/Clippy (efbb1320…), actual Hills assets and40-shape checks pass.
Integrated8ade15f8 CPU/Clippy (9552/0/157), actual assets and focused GPU checks
pass; fresh integrated release and visible restoration also pass. The single
critic pass is complete. See the chain receipt for the separate candidate hashes and bounds. These bounded comparisons do not
establish complete Object loading or whole-scene parity.

Root saved and read back retained-reader plates46BEE0,427D00,5F9070 and renamed
5F9070 `ObjectTypeClass__LoadSHPImage`; receipt
`/tmp/bridge-projectile-ghidra-art.json` preserves the AlternateArcticArt bound.

```sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted/bridge-projectile-inputs \
VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
python tools/projectile_oracle/bridge_render_art_state.py --check
```

The development extraction is `/tmp/bridge-projectile-input-assets`; the shared
native-oracle Python is `~/Documents/vera20k-dev/.venv/bin/python`.
Root independently reproduced the check in
`/tmp/bridge-projectile-root-native-art-check.log`. The original input/draw
corpus fingerprints are unchanged by this separate extended witness.
