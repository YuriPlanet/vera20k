# FV numbered FLH and two burst tails

`ifv_fire_coord.py/json/meta.json` execute original `gamemd.exe`.
The saved metadata pins Unicorn 2.1.4 and physical input hashes. The fixture
imports the common type/map preparation from `guided_step.py`; there are no
`/tmp` imports, patched original instructions, Rust values or calculated goldens.

Run with the original executable configured as described in
[the oracle guide](../native_oracle.md), plus an extracted input directory
containing `RULESMD.INI`, `ARTMD.INI`, `MPBattleMD.ini`, `Hills.map` and the
physical assets required by `bridge_render_inputs.BulletReader`:

```sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted-inputs \
python -m tools.projectile_oracle.ifv_fire_coord --check
```

`--write` regenerates reviewed evidence; ordinary validation uses `--check`.
The recorded extraction has no `LANGRULE.INI`; the ordered reader skips that
absent layer. ART is a fixed cache, not a rules layer. Physical INI file/archive
loading is supplied; original scalar readers and consumers execute.

## Native evidence

The fixture constructs real UnitType7470D0, including inherited TechnoType710AF0
and default BurstDelay0..3 values `-1` at74726D..74727F. This allocation is large
enough for UnitType+E48; the older `ifv_launch` base-type fixture must not be
extended to GetROF without that prerequisite. Partial physical FV readers run
in RULESMD→MPBattleMD→Hills order, including BurstDelay747B03..747B49. The
physical result retains four `-1` delays, TurretCount4, WeaponCount17 and empty
Gunner slot0→HoverMissile. Unit constructor slice735678..735691 executes the
empty-gunner assignment70DC70; the whole source Unit constructor is supplied.

ART reader715B10 calls HasTurrets717880, which tests `TurretCount(+808)>0`.
It does not test IsGattling. Positive WeaponCount bounds the numbered loop:

1. sprintf `Weapon%d` and `%sFLH` for one-based slot names.
2. ReadCoord529CA0 with the current normal triple as default; store at
   TechnoType+89C+1C*index.
3. Read numbered barrel/lock fields.
4. Format `EliteWeapon%dFLH`; ReadCoord's default is that normal triple,
   and the result stores at+A98+1C*index.

FV `Weapon1FLH` becomes `[64,48,180]`. Missing elite FLH inherits its normal
slot. Eight saved controls cover absence, exact-case keys, signed decimal wrap,
trailing third-number text, explicit elite values, WeaponCount0, ordinary
PrimaryFireFLH and numbered index2. ReadCoord uses a64-byte buffer, trims it,
and scans `%d,%d,%d`. Nonempty incomplete scans expose old ABI argument words;
Rust deliberately retains the supplied default for that malformed mod input.
This residual is explicit in `IniSection::read_coord3` and is not claimed as
native equivalence.

GetFLH6F3AD0 calls GetWeapon70E140 with the native index. Elite FLH is selected
only if the elite weapon pointer is nonnull; otherwise the normal weapon
record supplies FLH. FireAt calls GetFLH at6FE268 before Bullet allocation;
the muzzle coordinate at stack+44 is shared by Bullet launch, Report and muzzle
Anim. Burst parity is read from the retained pre-shot source+3B8 value. Odd
shots negate the lateral component before transformation.

The supplied flat source is `[2688,5248,800]`, body heading0, turret heading
`0x3fff`. Original Drive constructor4AF540, Drive DrawMatrix4AFF60, the base
heading matrix55A730, GetFLH matrix arithmetic and integer conversion execute.
The live transform yields:

| Shot | Burst index before | Native muzzle | Bullet ID | Native rearm | Next index |
|---|---:|---|---:|---|---:|
| 1 | 0 | `[2751,5200,980]` | 22 | frame0, duration4 | 1 |
| 2 | 1 | `[2752,5295,980]` | 23 | frame4, duration50 | 0 |

The source pose is supplied, not fitted to Rust. IDs21→22→23 belong only to
this fixture's partial prefix. They are not whole-scenario IDs.

For each shot, actual FireAt6FE4F2..6FF43F executes Bullet COM activation,
original factory6C5090, constructor466380/AssignUniqueID46643F, launch and
GetROF6FCFA0. FireAt increments burst at6FF27C before GetROF, writes its rearm
timer, then stores signed burst remainder at6FF2C5. ScenarioSeed31 gives:

- First shot: ranged3..5 sees raw2026076499 (rejected) then2287577493, returns4.
- Second shot: ranged0..2 sees raw2225548056, returns0; physical ROF50 gives50.

The transcript records each raw transition and before/after RNG state. There
is no reached Anim, ParticleSystem, Smudge or Wave constructor in that executed
tail. Original FLH produces no ID/RNG draw. The sound registry is empty;
Report lookup/playback itself is not certified. ObjectUnlimbo and Display
admission are supplied, as is the Windows InterlockedIncrement boundary.

## Rust integration and bounds

`ArtEntry` owns numbered normal/elite arrays. `ArtEntry::weapon_flh` applies
TurretCount, WeaponCount and the selected native index; `fire_coord` checks the
actual elite weapon binding and passes live barrel/body headings to the existing
shared transform. The common fire-coordinate result remains the source of
Bullet, report and muzzle positions. Aircraft callers explicitly request index0.

Regression tests are `sim::combat::fire_coord::tests::`:

- `numbered_flh_readers_and_slot_bounds_match_original` compares all8 controls.
- `retail_empty_fv_burst_flh_matches_original_flat_drive` parses retail rules/art
  through production readers and compares both native positions.
- `numbered_weapon_index_and_elite_weapon_binding_reach_fire_coordinate`
  protects index2 and null-elite fallback in the production caller.

The production dispatch regression
`sim::combat::ifv_fireat_tests::empty_fv_two_shot_fireat_matches_native_muzzles_ids_rearm_and_rng`
sets the declared source pose, Scenario ID21, Seed31 and frames0/4, then calls
`world_fire::emit_admitted_fire` for each shot, using a supplied admission
receipt at the same post-gate boundary as native6FE4F2. Physical HoverMissile
Range is6 (1536leptons), MinimumRange1 (256); the prior Range5 description was
incorrect. This comparison does not execute ordinary admission or establish
acceptance for its supplied target/pose. It compares both launch
origins and shared fire-event coordinates, native Bullet receipts22/23, the
rearm/burst outputs and complete Scenario RNG buffers, with no extra muzzle
Anim ID and no main-stream draws. The stable handles remain independent. It
calls the existing firing owner; it does not substitute launch or GetROF logic.

Root reported PASS for all9 FLH tests and the two-shot production dispatch
comparison. The leaf author ran no Cargo.
The existing transform's documented arbitrary-coordinate precision and slope
residuals remain. This selected fixture establishes the two flat FV coordinates,
not every Drive pose. ART Image/WeaponCount changes across rules passes are not
newly certified by the final fixed-art metadata projection. The selected physical
FV uses the same image/slot definition across the inspected layers.

The host invokes shot2 when shot1's returned timer duration elapses. Native Unit
AI scheduling, source construction/admission, intervening Bullet/world AI,
post6FF43F effects, full scenario prefix, impact and retirement are outside this
fixture. [The separate launch corpus](ifv_launch.md) executes full Bullet AI;
its supplied origin and source boundary differ, so the corpora must not be
presented as one contiguous whole-world execution.

## Spawn launch: GetFLH base and SpawnManagerClass::AI case 0

`spawn_launch` (the JSON's `spawn_launch` object) uses the same command and inputs.
It constructs real UnitTypes V3, DRED and BSUB (7470D0). It finds or allocates
their weapons (772FA0). For each rules layer, in RULESMD→MPBattleMD→Hills order,
it runs the full Weapon reader 772080 and the shared TechnoType rules slices.
It then runs the ART reader from HasTurrets 715B10 through the SecondSpawnOffset
store (71602E..71605B) on the physical ARTMD sections. Native reads:

- FLH: V3 PrimaryFireFLH `-160,0,75`; DRED `30,43,92`; BSUB `225,65,0` and
  SecondaryFireFLH `0,0,-40`.
- SecondSpawnOffset: BSUB `-70,0,0`; zeros for the others.
- Weapons: V3Launcher Burst1 Spawner; DredLauncher Burst2 Spawner; BoomerTorpedo
  Burst2; CruiseLauncher Burst2 Spawner.

Each owner is supplied Unit state at `[2688,2688,416]`:

- Primary and Secondary facing 0x2000, flags 4, frame 0, rookie.
- The original Drive (V3) or Ship (DRED, BSUB) constructor runs. Draw_Matrix
  4AFF60 or 69F670 executes inside GetFLH.

GetFLH 6F3AD0, called with a base:

| Control | Owner | Index | Burst | Base | Native |
|---|---|---:|---:|---|---|
| retail SecondSpawnOffset | BSUB | 1 | 1 | `-70,0,0` | `[2639,2737,376]` |
| zero base | BSUB | 1 | 1 | `0,0,0` | `[2688,2688,376]` |
| supplied lateral base | DRED | 0 | 1 | `0,25,0` | `[2757,2714,508]` |
| supplied lateral base | DRED | 0 | 0 | `0,25,0` | `[2662,2619,508]` |

The lateral rows show that the base joins the FLH (6F3B37..6F3B58) before the
odd-burst mirror (6F3C82).

Each case-0 row runs from 6B73C4 to the child's Unlimbo call at 6B7505, then
the burst reset block 6B757A..6B758F. The supplied manager and slot memory:

- The slot's IsSpawnedMissile is 1.
- The owner's burst index starts at 1.
- Rules+548 CMislType is a supplied pointer identity. Native compares the
  pointer only.

| Owner | Slot | GetFLH index, base, burst | Unlimbo coordinate | Dir | Burst after |
|---|---:|---|---|---:|---:|
| V3 | 0 | 0, `0,0,0`, 1 | `[2575,2801,501]` | 32 | 1 |
| DRED | 0 | 0, `0,0,0`, 0 | `[2679,2637,518]` | 32 | 0 |
| DRED | 1 | 0, `0,0,0`, 1 | `[2739,2697,518]` | 32 | 0 |
| BSUB | 0 | 1, `0,0,0`, 0 | `[2648,2648,386]` | 32 | 0 |
| BSUB | 1 | 1, `-70,0,0`, 1 | `[2599,2697,386]` | 32 | 0 |

Several parts are not executed:

- The child's Unlimbo.
- The CMislType V3TAKOFF anim block 6B750B..6B7579. The BSUB rows resume at
  6B757A instead.
- The rest of AI and its scheduling.

Hooks reject any RNG draw, ID allocation, allocation or AnimClass constructor;
none is reached. Coverage is one flat pose and heading. It has no slope tilt
(the GSI-08.04 residual), no turret and no elite weapon.

Rust regressions:

- `sim::combat::fire_coord::tests::retail_flh_base_matches_original_get_flh`
  compares the GetFLH rows through the production readers.
- `sim::spawn_manager_tests::retail_missiles_keep_their_launch_coordinate_from_unlimbo`
  runs the production spawn manager on the same pose. It compares each child's
  Location and exact Z, the owner's rounded direction and its final burst index
  with the case-0 rows. It also checks the production readers against the
  native type and weapon reads.
