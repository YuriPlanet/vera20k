# RA1 multi-turret / multi-barrel (`BarrelOverTurret`, `BarrelOffset`, `Extra*`, `BurstPerTurret`)

Phobos draws a Unit with more than one turret or barrel and cycles the burst
across them. VERA ports the sim-visible part — `BurstPerTurret=` selecting the
`ExtraTurretOffset<n>=` fire-origin pivot — and records the draw/recoil
consumers as residuals.

## Keys

| Key | Type | Default |
|---|---|---|
| `BarrelOverTurret=` | `Nullable<bool>` | unset |
| `BarrelOffset=` | `int` | `0` |
| `ExtraBarrelCount=` | `int` | `0` |
| `ExtraBarrelOffset<n>=` | `int` | `0`, `n < ExtraBarrelCount` |
| `ExtraTurretCount=` | `int` | `0` |
| `ExtraTurretOffset<n>=` | `CoordStruct` | `0,0,0`, `n < ExtraTurretCount` |
| `BurstPerTurret=` | `int` | `0`, read only when `ExtraTurretCount > 0` |

Read: `UnitTypeExt::LoadFromINI` (`Ext/UnitType/Body.cpp:100-134`),
declarations `Ext/UnitType/Body.h:76-82`, constructor defaults `Body.h:135-141`.
Two clamps are load-bearing: a non-positive `ExtraBarrelCount=` /
`ExtraTurretCount=` clamps to zero and reads no offsets (`:116`, `:134`), and
`BurstPerTurret=` is read only inside the positive `ExtraTurretCount=` arm
(`:121-131`).

VERA reads the block in `src/rules/phobos_turret_barrel.rs`, owner
`ObjectType::phobos_turret_barrel`, through
`IniSection::read_phobos_int_triple` for the `CoordStruct` offsets.

## Behaviour

**Firing origin — ported.** `TechnoClass_GetFLH`'s hook
(`Ext/Techno/Hooks.Firing.cpp:1076-1084`) turns the live `CurrentBurstIndex`
into an extra-turret index for a Unit:

```cpp
const int burstPerTurret = UnitTypeExt::Fetch(pUnitType)->BurstPerTurret;
if (burstPerTurret > 0)
    turIdx = ((CurrentBurstIndex / burstPerTurret) % (burstPerTurret + 1)) - 1;
```

`GetFLHAbsoluteCoords` passes `turIdx` to
`UnitTypeExt::ApplyTurretOffsetUnit` (`Ext/UnitType/Body.cpp:5-14`), which
translates the FLH matrix by `ExtraTurretOffset<n>=` when the index is
non-negative and by the stock art `TurretOffset=` otherwise. VERA's
`sim::combat::fire_coord` owns the port:
`phobos_turret_pivot_offset` reads the stored Unit's full
`weapon_burst.index()`, applies the quotient/modulo arithmetic, and feeds the
triple to the shared `native_flh_world_delta`, so the projectile origin, muzzle
animation and report position follow the selected turret. Non-Units, a zero
`BurstPerTurret=` and `hull_space` (`allowOnTurret` off) keep the stock pivot.
Native indexes `ExtraTurretOffsets` unchecked; an index past the authored list
would read adjacent heap words, so VERA keeps the stock pivot there.

`UnitTypeExt::LoadFromINI` (`Ext/UnitType/Body.cpp:100-134`) reads the whole
block from the type's **ART** section, selected by its native `ImageFile`
(`obj.image` else `obj.id`). `art_data::ArtEntry` parses the block once;
`RuleSet::project_art_data` copies it onto `ObjectCategory::Vehicle` before the
building-only merge. `ObjectType::from_ini_section` leaves the constructor
default, so a rule-section key is never read and an `Image=` override selects
the redirected ART section.

The other two native consumers are presentation-only and stay residual:

- **Draw matrix** — `UnitClass_DrawVoxelBody`'s matrix hook
  (`Ext/TechnoType/Hooks.MatrixOp.cpp:38-914`) replaces the single-body matrix
  whenever `TurretOffset != 0,0,0 || ExtraTurretCount > 0 || ExtraBarrelCount
  > 0` (`:30-44`). It emits one body, one matrix per barrel
  (`BarrelOffset` for the stock barrel, `ExtraBarrelOffset<n>` for the rest,
  `:174`, `:205-206`, `:885`) and one per turret
  (`ExtraTurretOffset<n>` through `UnitTypeExt::ApplyTurretOffsetUnit`,
  `Ext/UnitType/Body.cpp:5-14`); `BarrelOverTurret` chooses the turret matrix
  the barrel rides. Trigger: any type with the block; effect: the drawn body
  is one barrel/turret short and barrels do not ride their turret. No
  simulation state changes.
- **Recoil** — `UnitExt::InitializeRecoilData` (`Ext/Unit/Body.cpp:382-421`)
  keeps `ExtraTurretRecoil[ExtraTurretCount]` and
  `ExtraBarrelRecoil[(ExtraBarrelCount+1)*(ExtraTurretCount+1)-1]`, each a
  `RecoilData` cloned from the type's `TurretAnimData` / `BarrelAnimData`.
  `UnitExt::RecordRecoilData` (`:423-460`) picks the slot from the burst
  index:

  ```cpp
  turretIndex = BurstPerTurret
      ? ((CurrentBurstIndex / BurstPerTurret) % (ExtraTurretCount + 1)) : 0;
  barrelIndex = (ExtraTurretCount || ExtraBarrelCount)
      ? (CurrentBurstIndex % ((ExtraBarrelCount + 1) * (ExtraTurretCount + 1))) : 0;
  ```

  and vanilla's own recoil update is skipped (`DEFINE_JUMP 0x6FA4D1 ->
  0x6FA4FB`). Trigger: a multi-turret type fires; effect: extra barrels do not
  kick. `GameEntity` keeps one `TurretRecoil`/`BarrelRecoil` pair.

Note the firing hook's modulo is `BurstPerTurret + 1` while the recoil
turretIndex uses `ExtraTurretCount + 1`; the two readers disagree upstream.
VERA copies the firing hook for the origin and repeats the recoil formula in
this residual so the future recoil owner does not use the wrong divisor.

## Residuals

- **No multi-instance draw.** VERA's voxel body is one `(model, BodyDraw)` pair
  per entity (`drawn_model_id` / `body_draw` in
  `src/app/presentation/instances/units.rs`) with a single turret split
  (`draws_turret_parts`), so the `Hooks.MatrixOp.cpp` routine has no owner to
  extend; `BarrelOverTurret`, `BarrelOffset` and the per-index draw offsets
  are unread.
- **No per-slot recoil state.** `GameEntity` retains one `TurretRecoil` /
  `BarrelRecoil` pair. The feature needs the two extra vectors, their
  `TravelPerFrame`/`TravelSoFar`/`State`/`TravelFramesLeft` fields initialized
  from the type's anim data, their own snapshot and `world_hash` words, and a
  retained burst index.
- **Unchecked native turret index.** `ApplyTurretOffsetUnit` reads
  `ExtraTurretOffsets[turIdx]` without a bounds test; when
  `BurstPerTurret > ExtraTurretCount` the quotient can address past the vector
  and native reads adjacent heap words. VERA uses the stock `TurretOffset=`
  for those indices instead. Trigger: a type authoring `BurstPerTurret=`
  larger than `ExtraTurretCount=`; effect: the muzzle/pivot differs from
  retail only for such modded types.

## Tests

- `src/rules/phobos_turret_barrel_tests.rs` — the constructor defaults, both
  count clamps, the indexed loops in index order, the `CoordStruct` triples,
  the gated `BurstPerTurret=`, and the `BarrelOverTurret=` reset.
- `src/rules/phobos_turret_barrel_tests.rs`
  `projection_reads_the_multi_turret_block_from_art` — the production ART →
  `RuleSet` projection reads every key.
- `src/rules/phobos_turret_barrel_tests.rs`
  `rules_section_keys_do_not_supply_the_multi_turret_block` — rule-section keys
  stay at the constructor defaults.
- `src/rules/phobos_turret_barrel_tests.rs`
  `the_image_override_selects_the_art_section` — `Image=` selects the ART
  section name.
- `src/sim/combat/fire_coord.rs` `burst_per_turret_cycles_the_extra_turret_pivot`
  — the stock/extras/stock pivot cycle and all three offset components.
- `src/sim/combat/fire_coord.rs`
  `a_burst_turret_index_past_the_authored_list_keeps_the_stock_pivot` — the
  unchecked-native-index safety clamp.
