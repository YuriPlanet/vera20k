# Packed object source and shadow prerequisites

Native static byte evidence uses the Steam gamemd.exe SHA256
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.
Every JSON is the existing `python -m tools.native_inspect` owner's deterministic
packet, containing exact requested range, original identity and decoder limits.
To reproduce, select that original image as `VERA20K_GAMEMD_EXE`, then use each
packet's `request.command`, `request.address`, and `request.bytes`. No original
code is rewritten. Ghidra 8089, explicit gamemd.exe, supplied function boundaries
and decompilation leads; original instructions establish the claims here.

## VXL shadow admission

Unit DrawVoxelBody73B470 blits its final parent via virtual+55C at73C1A5 before
its shadow suffix. Sinking+3CD at73C1D2 returns before that suffix at73C1FC.
The ordinary shadow calls Foot4DB0D0 at73C5C4. Foot reads the current ILoco
interface+674 and calls virtual+20 at4DB0FB; only AL1 admits the shadow call.
Ship's constructor-installed vtable7F2D8C+20 and Drive7E7EB0+20 both contain
55ABE0. Its original five bytes B001C20400 returnAL1 without state access.
The prior locomotor receipt logs/packed-shroud-locomotor records constructor
bindings; this packet records the exact tables/leaf again for the selected slot.

Techno706BD0 rejects raw cloak state+220!=0 at706BDD..706BE5, then actual type
virtual+84's NoShadow+D98 at706BF3..706BFB. It does not ask VisualCharacter.
StartCloaking703799 writes rawstate1, and7037A3 writes progress0. That initialized
state draws an ordinary body because VisualCharacter703860 returns0 for a
nonfully-cloaked state with progress<=0, but still suppresses the VXL shadow.
Therefore a shadow gate inferred only from translucency FX_CLOAK is wrong at
that transition on the selected stock SUB VXL path. DLPH uses SHP and has a
different shadow caller, as the production ART reader test below establishes. This conclusion rests on control flow, not a whole-frame capture.
The shared VERA VXL emission consumer is instances/units.rs::emit_unit_shadow_sprite,
used by Composite and split routes; its native-mask preparation must share the
same admission decision. Existing fixture vehicle_shadow_active_locomotor_selects_native_or_legacy_companion
is the focused regression entry. No GPU/Cargo validation occurred in this receipt.

## NoShadow native reader

TechnoType710AF0 retains its receiver inESI and zerosEBX at710B00;71160D seeds
+D98 withBL=false. ReadINI712170 retains its type inEBP and its passed INI inESI.
The body selects the object's exact-case section at type+24. At715087 it loads
the current+D98 byte as the default,71508E pushes literal8436E0 `NoShadow`,
715095 calls identity524EC0 on the section pointer,71509D calls ReadBool5295F0,
and7150A2 storesAL. No clamp or key-specific post-read pass is observed.
ReadBool529762..5297A5 uppercases the stored value's first byte and returnsfalse
for0/F/N, true for1/T/Y, or its supplied prior default. Physical INI parsing and
rules chronology are owned by src/rules/mod.rs and the production Rules owner:
RULESMD, optionalLANGRULE, selectedmode, selectedmap. The existing
IniSection::read_bool folds these passes, preserving malformed/absent defaults.
Do not substitute ARTM[D] or RA2 baseRULES. Nearby Rust identity and focused
reader checks are rules/object_type.rs::tests::{no_shadow_reader_retains_native_prior_value_across_rules_layers,
retail_cloaked_naval_and_vehicle_shadow_type_inputs}. Their execution is separate
from this static evidence. Retail positive controls DNOA/DNOB are NoShadow=yes;
SUB/DLPH/MTNK expectfalse, subject to actual production-reader test result.
The same test reads actual ARTMD through ArtRegistry and resolves the production
metadata route: SUB Voxel=yes, DLPH Voxel=no. These are different body callers.

## SHP shadow is a different caller

SharedSHP705E00 checks the same type+D98 at706052..70605C, and clears its
shadow-suffix admission byte when set. VisualCharacter branch0/1 draws the body,
then, if the suffix byte remains set, draws frame+totalFrameCount/2 with selector
bits2/4 cleared and shadowbit1. Relevant instructions706458..7064CF show that
suffix; chars2/3/4 only draw the body. The SHP decision must not be replaced by
the VXL raw-state gate. Current instances/shp.rs emits main body only and contains
no native shadow companion. That pre-existing SHP shadow geometry/frame mechanism
is a separate missing chain, not proof that this new packed body is pixel-equivalent
as a whole original object including its shadow.

## Source composition scope

UnitModel::load uses production Turret/TurretCount admission and obtains one body
plus admitted gun pairs. For stock SUB, the retail reader test checks Voxel=yes,
no turret and no indexed guns; its production Composite route calls the existing
composite_parts with one VxlSprite, whose composition just copies that raster.
Thus split body/turret relative-depth merging is not a prerequisite on this
selected stock SUB source path. This is conditional on the actual test's result
and ordinary pose; it is not a proof of the voxel raster itself. DLPH has
Voxel=no and is drawn by SHP, so the missing SHP shadow companion above prohibits
a whole-DLPH rendered parity claim. It is not covered by the single-VXL argument.

For a turreted unit, instances/units.rs emits retained split images and the packed
stencil currently overlays them in draw order before one blend. The native Unit
source cache draws to its off-screen parent and final73B140 blits once. Existing
unit_atlas::composite_vxl_layers explicitly marks native relative part matrices and
final surface-blit order UNCHECKED. No native execution or retained depth proof in
this investigation establishes the correct overlap pixel for independently facing
body/turret/barrel. Treat this as an unresolved separate source-geometry dependency
for any expanded turreted-cloak claim, not as established error nor certified parity.

Existing translucent_blitter_a.json initializes fresh native memory and executes
one leaf per case. `overlap_sources` means three neighboring pixels within one
leaf row; it is not a sequential multiple-parent original framebuffer golden.

The shadow admission and NoShadow reader do not draw RNG, write gameplay timers
or detach objects. StartCloaking does initialize the progress timer and can call
virtual+150; that lifecycle remains with the existing cloak owner. Cache allocation
and full shadow raster/lifetime are outside these gate readings. Static sweeps and
C function labels do not establish exhaustive aliases or all game paths.
