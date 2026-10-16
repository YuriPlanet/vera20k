# Building Construction → Grand_Opening

Original reference: active-retail `gamemd.exe`, SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
The generator checks every executable PE section before and after execution;
original instructions are never patched.

`building_construction.py` retains its original primitive route/slice corpus.
Additive `--joined` produces `building_construction_joined.json` and its explicit
assumptions, substituted boundaries and source identities in the matching meta.
The joined payload SHA-256 is
`62e31e63c49350afa9be3ffefafe158a7ae4423a898c3614e80b0506b7889ae3`.
The three Rust replay hash changes have a separate
819-boundary composition receipt (`tools/spatial_oracle/building_construction_replay/README.md`, in git at 1b1a8b43e);
their observed Rust pins are not native goldens.

## Coverage

The joined corpus uses original Rules/BuildingType/AnimType/MissionControl
constructors and readers, exact-case selected INI strings in native caches and
actual extracted retail SHP bytes. It compares GACNST deployment, GAPOWR/GAPILE
player placement, GAPOWR computer placement, damaged GAPOWR placement and an
explicit HasStupidGuardMode=false control through construction completion C and
the next operational edge C+1. Controls independently execute seven original
FindFactory5F7900 cases and two InitializeFactoryPrimary448070 cases.

Original Construction449A50, first-contact radio65ACB0, whole
Grand_Opening445F80, slot allocation451890, Anim constructor421EA0,
Unlimbo/Start424CE0, live AI423AC0 and destruction4228E0 execute. Retained type
pointer+0xC8 is separate from attached Object+0xCC; comparisons join pointers
through original Abstract IDs. The bounded live vector rereads its count, so
appended Anim objects visit in the creating frame.

Every compared frame retains both FacingClass direction words, body and queued body, Stage/timer, mission/status/
cadence, ready, placement/operational sample, all animation slots and runtime,
IDs/coordinates/order, and complete RNG states or their byte digests. The five
stock routes consume no RNG. The cleared HasStupidGuardMode control executes
Guard's ranged(0,2) rejection at C+1: raw2026076499 is rejected and raw2287577493
is accepted. Before/after full RNG buffers and both inline advances are retained.

The full BuildingType constructor/reader exports `+ED8=0` separately from
`DeployFacing +EDC=128`. Construction449AFE resets both direction words to zero
for these stock types. Helipad is read from `+16CB`, as Grand_Opening4463C0 does.
All six creation snapshots and284 subsequent frames retain the facing words.
The shared Anim constructor also compares134 assigned-identity Bouncer entries
against the existing158 original launch goldens, including full RNG state and
body bits. Other launch rows preserve directly supplied doubles; the134 use the
production ART reader without `%f` changing those doubles.

Construction sound uses the original registry/default/reader: native default-1,
stock Construction=Dummy, absent per-type BuildupSound. Original sound event
update/watch/release execute with Dummy's no-sample input. The Building sound
handle is released on completion and before destructor removal; device/sample
mixing and global pool scheduling remain presentation boundaries.

## Explicit limits

Prior admitted Building/House/map/country state, actor vectors and drawing/Logic
membership are supplied. The generator runs Building header43FB20..43FC39,
UpdateAnimation, pre/post ready, mission and queued-body slices, followed by
appended Anim AI. Full native Building constructor/Unlimbo, full Techno common
AI, repair/factory AI and full Logic scheduler are excluded. Four presentation
calls, native allocator/delete, lexical cache and requested-file IO are supplied
boundaries. Extracted MK assets establish bytes, not production theater mount
precedence or native pixel parity. Stock keys leave free unit, cash, healing,
purifier, helipad and other specialized opening mechanisms inactive.

Rust `sim::world::techno_ai::building_missions::opening_oracle_tests` compares the same
bounded join through the existing owners and production retail readers. The
separate `building_construction_tests` runs eighteen original routes and five
mission slices through ordinary object AI. These checks do not certify a whole
building, a full native scenario or excluded specialized branches.

## Reproduction

Use original retail executable and the existing asset extractor. Set
`VERA20K_BUILDING_CONSTRUCTION_ASSETS` to the directory containing the extracted
requested SHP files; the generator records filenames and hashes.

```sh
python -m tools.spatial_oracle.building_construction --check
python -m tools.spatial_oracle.building_construction --joined --check
VERA20K_REQUIRE_RETAIL_INI=1 VERA20K_REQUIRE_RETAIL_ASSETS=1 \
  python -m tools.cargo_run -- test -p vera20k-core --lib sim::world::techno_ai::building_missions::construction_tests::
VERA20K_REQUIRE_RETAIL_INI=1 VERA20K_REQUIRE_RETAIL_ASSETS=1 \
  python -m tools.cargo_run -- test -p vera20k-core --lib sim::world::techno_ai::building_missions::opening_oracle_tests::
```

The ordinary release-map production receipt (`tools/spatial_oracle/building_construction.production.json`, in git at 1b1a8b43e)
and [profile](../map_observation.building-opening.example.json) exercise normal
DeployMcv, QueueProduction and PlaceReadyBuilding orders on retail Battle/AnyTown.
The yard opens at step43, the power plant at701 and the barracks at1152. The
power plant replaces its operational animation at702, both crane animations
expire, and a subsequently queued GI exits the barracks at1269. These are observed
production values, not native goldens; the multi-House live vector skips one
barracks visit at1115. A repeat matches all1450 retained steps, fingerprints and
final Metal GPU bytes. The final corrected v2 frame was inspected; the native facing correction changes
the retained deterministic hash while the transition observations and GPU bytes
remain unchanged. Production behavior and
bounded native comparisons remain separate evidence levels; no object certificate
is issued.
