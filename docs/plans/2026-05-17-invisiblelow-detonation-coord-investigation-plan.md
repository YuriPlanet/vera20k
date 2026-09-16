# InvisibleLow Detonation CoordStruct - Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass. Execute it by
> running `/re-investigate InvisibleLow detonation CoordStruct` with this plan
> loaded as context, OR dispatch the function inventory in batches and synthesize.

**Topic:** `InvisibleLow` / `Inviso=yes` bullet detonation CoordStruct for GI-style small-arms impacts.
**Scope Size:** Medium - 22 functions, 14 primary INI keys.
**Est. Effort:** ~5-7 hours of `/re-investigate` work.
**Prior Research:** Partial. Several HIGH-confidence BulletClass reports exist, but they leave exact `InvisibleLow` impact CoordStruct edge cases and stale-doc corrections to verify.
**Expected Output:** research document at [docs/research/INVISIBLELOW_DETONATION_COORDSTRUCT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INVISIBLELOW_DETONATION_COORDSTRUCT_GHIDRA_REPORT.md).
**Next Pipeline Step:** `/write-plan` directly if the report resolves all CoordStruct rules; otherwise a smaller follow-up `/plan-investigation` for the unresolved branch.

---

## 1. Goal

Determine exactly how gamemd.exe computes the CoordStruct used to detonate `Inviso=yes` / `InvisibleLow` bullets, especially GI `[M60]` and `[Para]` shots. The report must answer which coordinate is passed to warhead `AnimList` creation for normal entity targets, force-fire cells, walls, cliffs/elevation, bridge/on-bridge targets, and special building-target branches.

The output should be implementation-ready but not a verbatim port: Rust can keep a clean projectile model as long as the observable impact anim, damage, and obstacle behavior match.

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| `docs/fidelity-checks/2026-05-17-gi-small-arms-warhead-impact-placement.md` | Rust trace plus binary spot checks for GI `PIFFPIFF` placement | MEDIUM-HIGH | Confirms sub-cell effect payload fix, but not full `BulletClass` raycast / wall / cliff / building override behavior. |
| [BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md) | End-to-end BulletType parse, `BulletClass::Fire`, Inviso path, proximity, detonation-position corrections | HIGH overall | Explicitly flags stale older claims and leaves `BulletClass::Fire` uninit stack slots / exact edge behavior worth targeted verification. |
| [BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md) | Bullet allocation/init/fire launch path | HIGH | Generic bullet launch; does not fully enumerate `InvisibleLow` edge cases by target kind. |
| [BULLET_CLASS_AI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_CLASS_AI_GHIDRA_REPORT.md) | Main BulletClass AI loop and bounce/proximity consumers | HIGH | Mostly in-flight bullets; Inviso bullets short-circuit in `Fire`, but BounceCheck helpers still matter for shared wall/cliff behavior. |
| [BULLETCLASS_TRAJECTORY_AND_HOMING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_TRAJECTORY_AND_HOMING.md) | Trajectory, homing, proximity, BounceCheck | HIGH for covered functions | Contains stale wording about close-target handling as direct `ReceiveDamage`; consolidated report says vtable+0xA4 is CoordStruct output. Verify conflict. |
| [BULLET_CLASS_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_CLASS_LAYOUT_GHIDRA_REPORT.md) | BulletClass fields and BulletType pointers | HIGH | Layout only; needs consumer confirmation for this specific path. |
| [BULLETTYPECLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETTYPECLASS_GHIDRA_REPORT.md) | BulletType ReadINI offsets/defaults | HIGH | Relevant keys parsed; no runtime CoordStruct behavior. |
| [WARHEAD_DETONATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEAD_DETONATE_GHIDRA_REPORT.md) | Warhead/Bullet detonation dispatch and AnimList creation | HIGH with corrections | Good for consuming CoordStruct, not for producing it. |
| [ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md) | AnimClass constructor sites, including impact anims and muzzle anims | HIGH | Confirms passed CoordStruct is used; does not compute bullet impact CoordStruct. |
| [BRIDGE_OBJECT_ONBRIDGE_EXTRA_WRITERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_OBJECT_ONBRIDGE_EXTRA_WRITERS_GHIDRA_REPORT.md) | Object/Bullet OnBridge runtime writers | HIGH for hit classification | `TechnoClass::Fire_At @ 0x006FF0B0` copies OnBridge for `Inviso=yes` bullets; needs inclusion in bridge impact coordinate story. |
| [WEAPONTYPECLASS_VERIFICATION_AND_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WEAPONTYPECLASS_VERIFICATION_AND_CONSUMERS_GHIDRA_REPORT.md) | WeaponType parse and `Fire_At` consumers | HIGH | Useful to verify `Projectile=`/`Warhead=` handoff into BulletClass. |

**Conflicts between reports:**

- [BULLETCLASS_TRAJECTORY_AND_HOMING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_TRAJECTORY_AND_HOMING.md) describes close-target branches as direct `ReceiveDamage`.
- [BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md) corrects that: vtable+0xA4 is `GetCoords_OutputParam`, producing a detonation CoordStruct. This investigation must re-check the live binary and mark the older wording stale or conditional.

## 3. Function Inventory

| # | Phase | Address | Current Name | Scope Reason | Depth Target | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|--------------|----------------|
| 1 | 1 | `0x006FDD50` | `TechnoClass::Fire_At` | Primary weapon fire pipeline: selects weapon, allocates/initializes BulletClass, calls `Fire`, and has `Inviso` on-bridge propagation. | FULL for bullet creation/params; LIGHT for unrelated visual branches | Low |
| 2 | 1 | `0x0046B050` | `BulletClass::Allocate` | Factory called from `Fire_At`; verifies constructor/init parameter order and default bullet state. | MEDIUM | Low |
| 3 | 1 | `0x004664C0` | `BulletClass::Init` | Writes BulletType, Target, Owner/Firer, Damage, Warhead, Bright, and runtime fields consumed by `Fire`/detonation. | FULL | Low |
| 4 | 1 | `0x00468670` | `BulletClass::Fire` | Core `Inviso=yes` launch path: target coords, FlakScatter+Inviso, `FUN_005880A0`, ground-height snap, fallback cell. | FULL | Low |
| 5 | 1 | `0x005880A0` | `FUN_005880A0` | Inviso raycast / cell-line helper that returns the impact CoordStruct or sentinel. Must decode wall/building/firer-house behavior. | FULL | Medium - comments mention invisible projectile legacy; verify YR-active flags |
| 6 | 1 | `0x00468D80` | `BulletClass::BulletDetonation` | Starts from `BulletClass.Location`, then may override detonation CoordStruct through target CoordStruct helpers before warhead detonate. | FULL | Low |
| 7 | 1 | `0x004690B0` | `WarheadTypeClass::Detonate` / Bullet detonate body | Consumes final CoordStruct for AnimList, smudge, damage, shrapnel/airburst branches. | MEDIUM - CoordStruct consumption only | Medium - airburst/shrapnel branches include TS-era traps |
| 8 | 1 | `0x0041BDD0` | `GetCoords_OutputParam` | Corrected vtable+0xA4 helper; writes target CoordStruct into caller-provided output. Critical stale-doc conflict. | FULL | Low |
| 9 | 2 | `0x004CC100` | `FUN_004CC100` | Fallback cell helper called when `FUN_005880A0` returns sentinel in `BulletClass::Fire`. | FULL | Medium - may be rarely hit in stock YR |
| 10 | 2 | `0x004CC360` | `FUN_004CC360` | Wall/cliff/obstacle predicate called by BounceCheck; likely sibling logic to Inviso blocker checks. | FULL | Medium - must verify which BulletType flags gate it |
| 11 | 2 | `0x00468BB0` | `BulletClass::BounceCheck` | Consumes `SubjectToCliffs`, `SubjectToWalls`, `FlakScatter`, `AA`, and ground collision logic. Needed to compare shared obstacle semantics. | MEDIUM | Low |
| 12 | 2 | `0x004E1130` | `ProximityDetector::Set` | `BulletClass::Fire` always arms it after launch. For Inviso, verify whether it is inert or still affects detonation timing. | MEDIUM | Low |
| 13 | 2 | `0x004E11F0` | `ProximityDetector::Check` | Confirms non-Inviso/in-flight detonation timing and which parts are irrelevant to `InvisibleLow`. | LIGHT | Low |
| 14 | 2 | `0x005F6360` | `FUN_005F6360` | Distance/proximity helper used in detonation-position override branches. Need exact distance units and thresholds. | MEDIUM | Low |
| 15 | 2 | `0x005F65A0` | `ObjectClass::GetCoords` | Standard target CoordStruct source for entities/buildings/ground objects. | MEDIUM | Low |
| 16 | 2 | `0x00578080` | `CellClass::GetGroundHeight` wrapper | `BulletClass::Fire` sets Inviso impact Z to ground height at impact position. Verify units and bridge interaction. | MEDIUM | Medium - bridge deck vs ground-only behavior matters |
| 17 | 2 | `0x0047B3A0` | `CellClass::GetGroundHeight` inner | Height interpolation details for sub-cell CoordStructs and elevated terrain. | MEDIUM | Medium - TS bridge/fog code nearby |
| 18 | 2 | `0x0047C520` | `Look_up_building_in_cell` | Used by `FUN_005880A0` to detect blocking buildings along a ray. Need object-list ordering and house/foundation gates. | MEDIUM | Low |
| 19 | 2 | `0x005657A0` | `MapClass::Get_CellClass` | Converts cell coords to `CellClass*`; needed for out-of-bounds/sentinel and blocker lookup. | LIGHT | Low |
| 20 | 3 | `0x0046BEE0` | `BulletTypeClass::ReadINI` | Verify `InvisibleLow` flags: `Inviso`, `SubjectToCliffs`, `SubjectToElevation`, `SubjectToWalls`, `AA/AG`, `ROT`, `Arcing`. | LIGHT | Low |
| 21 | 3 | `0x005206B0` | `InfantryClass::Fire_At_Target` | GI-specific fire-frame gate and call into `Fire_At`; confirms standard infantry path reaches BulletClass. | LIGHT | Low |
| 22 | 3 | `0x00736DF0` | `UnitClass::Fire_At_Target` | Generic unit caller for cross-checking non-infantry `InvisibleLow` users and target-kind handling. | LIGHT | Low |

**Phase 1 checkpoint:** Pause after functions #1-#8. If `FUN_005880A0` turns out to be only building/wall line tracing and not terrain/elevation, revise Phase 2 before continuing.

## 4. Detail Checklist

- **CoordStruct sources:** source FLH/firer coords, target coords, force-fire cell coords, fallback cell coords, `BulletClass.Location`, `GetCoords_OutputParam` overrides.
- **CoordStruct units:** confirm X/Y are leptons, Z is leptons, cell derivation uses `/ 256`, and how sub-cell remainders are handled.
- **Magic numbers:** `0x100`, `0x80`, `0x20`, `0x2A`, `0x80`, `0x7FFFFFFF`, sentinel globals (`DAT_00ABDC10/14/18`, `DAT_0089DE30/34/38`), map width `0x200`, map cell max checks.
- **Bit flags and offsets:** BulletType `+0x294 Airburst`, `+0x296 SubjectToCliffs`, `+0x297 SubjectToElevation`, `+0x298 SubjectToWalls`, `+0x29D Floater`, `+0x29E Inviso`, `+0x2A0 Ranged`, `+0x2A2 Arcing`, `+0x2A3 FlakScatter`, `+0x2A4 AA`, `+0x2AC Cluster`, `+0x2F0 Arm`.
- **Target-kind branches:** infantry, vehicle, building, on-bridge object, force-fire cell, invalid/dead target, aircraft/airborne target.
- **Obstacle branches:** wall overlay/building-as-wall, upward cliffs, terrain elevation, out-of-bounds cell, same-source-target cell, same X or same Y special loops.
- **House/firer gates:** `FUN_005880A0` receives firer house at `param_4`; decode exactly how same-house/allied blockers are ignored or included.
- **Ground height:** verify whether impact Z uses ground-only or bridge deck, and whether `OnBridge` copied in `Fire_At` changes detonation Z or only classification.
- **Detonation override:** verify every call to target vtable+0xA4 in `BulletClass::BulletDetonation`; record condition thresholds and target class tests.
- **Damage vs anim ordering:** confirm whether damage application and AnimList creation receive the same final CoordStruct.
- **Randomness:** for GI `InvisibleLow`, verify no FlakScatter/random offset applies. For other `Inviso=yes` projectiles, document which random scatter is outside GI scope.
- **INI defaults:** confirm `*md` patch priority and base RA2 fallback for all relevant keys.

## 5. INI Keys in Scope

| Key | Section | Default / GI Value | Suspected Purpose | Currently Parsed in Rust? |
|-----|---------|--------------------|-------------------|----------------------------|
| `Projectile` | `[M60]`, `[Para]`, other weapons | `InvisibleLow` | Selects BulletType for shot. | Yes |
| `Warhead` | `[M60]`, `[Para]` | `SA`, `SSA` | Selects warhead and AnimList. | Yes |
| `Speed` | `[M60]`, `[Para]` | `100` | Passed to bullet init; may not matter for Inviso. | Yes |
| `Anim` | `[M60]`, `[Para]` | `MGUN-*` | Muzzle flash, separate from impact. | Yes / app-side presentation |
| `AnimList` | `[SA]`, `[SSA]` | `PIFFPIFF,PIFFPIFF` | Impact puff spawned by warhead detonation. | Yes |
| `Bullets` | `[SA]`, `[SSA]` | `yes` | Warhead flag; verify if detonation branch cares. | Partial / verify |
| `ProneDamage` | `[SA]`, `[SSA]` | 70%, 80% | Damage modifier, not coordinate behavior. | Yes |
| `Inviso` | `[InvisibleLow]` | `yes` | Enables instant invisible impact path in `BulletClass::Fire`. | Yes |
| `Image` | `[InvisibleLow]` | `none` | Skips projectile sprite. | Yes |
| `SubjectToCliffs` | `[InvisibleLow]` | `yes` | Bullet can be stopped by upward cliffs / cliff path. | Parsed, not fully simulated |
| `SubjectToElevation` | `[InvisibleLow]` | `yes` | Height range/path behavior; also range bonus in Rust. | Parsed, partial |
| `SubjectToWalls` | `[InvisibleLow]` | `yes` | Bullet can be stopped by walls/blocking buildings. | Parsed, not fully simulated |
| `AA` / `AG` | projectile sections | default AG true; GI ground | Target category filter. | Yes |
| `ROT`, `Arcing`, `Cluster`, `Airburst`, `Ranged`, `Arm`, `FlakScatter` | projectile sections | mostly unset/false for `InvisibleLow` | Confirm irrelevant for GI or document conditional branches. | Parsed, partial |

## 6. Caller & Integration Map

| Caller Address | Calls Into | When Invoked | Should Executor Decompile? |
|----------------|------------|--------------|----------------------------|
| `0x005206B0` | `Fire_At` via infantry fire decision | GI/E1 attack animation reaches firing frame | LIGHT |
| `0x00736DF0` | `Fire_At` via unit fire decision | Generic vehicle/unit fire path | LIGHT |
| `0x006FDD50` | `BulletClass::Allocate`, `Init`, `Fire` | Every standard weapon shot after fire gates pass | YES - Phase 1 |
| `0x00468670` | `FUN_005880A0`, `CellClass::GetGroundHeight`, `ProximityDetector::Set` | Bullet launch into world; for `Inviso=yes`, computes instant impact | YES - Phase 1 |
| `0x00468D80` | `WarheadTypeClass::Detonate`, target coord helpers | Bullet detonation tick or immediate detonation path | YES - Phase 1 |
| `0x004690B0` | `AnimClass::Constructor`, damage/smudge/special warhead logic | Final warhead detonation | MEDIUM - CoordStruct consumer only |

Rust integration notes:

- `src/rules/projectile_type.rs` parses the relevant projectile flags, including `subject_to_cliffs`, `subject_to_elevation`, `subject_to_walls`, and `inviso`.
- `src/sim/combat/mod.rs` currently resolves `TargetKind` into `(rx, ry, sub_x, sub_y)` and now preserves sub-cell effect placement, but it does not compute a BulletClass-style `InvisibleLow` impact raycast.
- `src/sim/combat/in_range.rs` uses `subject_to_elevation` for range/height handling; verify whether this matches binary range behavior separately from detonation CoordStruct.
- `src/sim/combat/combat_weapon.rs` uses projectile `AA`/`AG` target filters.
- `src/app_instances/overlays.rs` now renders world effects through lepton projection, so sim only needs to produce the correct game-space CoordStruct.

Out of scope for this investigation:

- Full visible projectile movement for rockets, shells, arcing weapons, or homing missiles.
- Muzzle flash FLH/G1 placement; that was a separate fix and should remain unchanged.
- Full warhead special effect behavior except where it consumes or mutates detonation CoordStruct.

## 7. TS-Legacy Risk Register

- **Airburst/shrapnel/cluster branches:** active for specific YR weapons but not GI `InvisibleLow`; do not let these branches expand the scope beyond CoordStruct production.
- **`SubjectToElevation` wording:** INI comment says height bonus and no effect on homing projectiles; runtime may also affect path over varying terrain. Verify exact YR-active consumers.
- **Bridge ground height:** several docs warn `GetGroundHeight` returns ground-only, while object `OnBridge` state is separate. Confirm `InvisibleLow` detonation on bridge targets before assuming bridge deck Z.
- **Older ReceiveDamage wording:** treat [BULLETCLASS_TRAJECTORY_AND_HOMING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_TRAJECTORY_AND_HOMING.md) close-target wording as suspect until `0x00468D80` and `0x0041BDD0` are re-verified.
- **Non-GI `Inviso=yes` variants:** `Invisible`, `InvisibleMedium`, `InvisibleHigh`, `InvisibleAll`, `Psychic`, `FlakProj`, and Tesla/comet projectiles may share flags but have different `ROT`, `AA`, `AG`, or `FlakScatter`; document only enough to avoid misapplying GI findings.

## 8. Current Rust Implementation Surface

| File | Current Surface | Notes |
|------|-----------------|-------|
| `src/rules/projectile_type.rs` | Parses ProjectileType flags and defaults. | Data is present for future BulletClass-style resolver. |
| `src/rules/weapon_type.rs` | Parses `Projectile=`, `Warhead=`, `Speed=`, `Anim=`. | Weapon carries enough references to choose projectile behavior. |
| `src/rules/ruleset.rs` | Loads projectile sections referenced by weapons. | `InvisibleLow` is available through `rules.projectile`. |
| `src/sim/combat/mod.rs` | Instant-hit combat applies damage immediately and emits warhead effects at resolved target coords. | Missing actual Inviso raycast/override CoordStruct computation. |
| `src/sim/combat/in_range.rs` | Uses projectile `subject_to_elevation` in range logic. | Needs comparison with binary if detonation research finds separate elevation rules. |
| `src/sim/combat/combat_weapon.rs` | Uses projectile `AA`/`AG` to select valid weapon. | Good target-category filter baseline. |
| `src/app_fire_effects.rs` | App-side fire events for muzzle/report presentation. | Should stay presentation-only; do not move projectile detonation logic here. |
| `src/app_instances/overlays.rs` | Projects `WorldEffect` by lepton coordinates. | Ready to consume exact detonation CoordStruct from sim. |

## 9. Deferred Open Questions

1. Does `FUN_005880A0` stop on walls, buildings, cliffs, or only blocking buildings with specific type flags?
2. What exact CoordStruct is returned for force-fire on empty ground when source and target are on different elevation levels?
3. What does the sentinel return from `FUN_005880A0` mean, and how does `FUN_004CC100` choose the fallback cell?
4. Does `CellClass::GetGroundHeight` in `BulletClass::Fire` account for bridge decks or only terrain ground?
5. Does `TechnoClass::Fire_At` copy target `OnBridge` state for `InvisibleLow` only into bullet metadata, or does that metadata alter the detonation CoordStruct?
6. For building targets, exactly which classes/flags trigger the vtable+0xA4 detonation-position override?
7. Are target sub-cell positions used directly, or does the target's `GetCoords` return a class-specific anchor such as foundation center, turret coords, or body center?
8. Is damage applied before or after the final detonation CoordStruct override, and do AnimList and damage use the same coordinate?
9. Which stock YR `InvisibleLow` users besides GI exercise wall/cliff/elevation branches in normal play?

## 10. Execution Strategy

**Single-session `/re-investigate` with a Phase 1 checkpoint.**

Phase 1 (#1-#8) should produce the main answer for GI-style shots. Phase 2 (#9-#19) fills in wall/cliff/elevation/fallback and height edge cases. Phase 3 (#20-#22) verifies INI defaults and caller context. If Phase 1 discovers that `FUN_005880A0` is larger than expected or fans out into many terrain helpers, split the blocker/raycast helper into a follow-up investigation before continuing.

## 11. Success Criteria

The executed research document must:

- State the exact final CoordStruct source for normal infantry, vehicle, building, and force-fire cell targets.
- State how `InvisibleLow` wall, cliff, elevation, and out-of-bounds cases alter the CoordStruct.
- Resolve the stale `ReceiveDamage` vs `GetCoords_OutputParam` conflict with live binary evidence.
- Identify which findings are active in standard YR and which are conditional on non-GI projectile flags.
- Cite addresses for every HIGH-confidence claim.
- Produce implementation guidance for a Rust sim-side `Inviso` impact resolver that stays independent of render/ui/audio/sidebar/net.

## Sources

- Ghidra sampled during planning:
  - `0x00468670` `BulletClass::Fire` - live decompile confirmed `Inviso` branch calls `FUN_005880A0`, sets ground height, and calls `SetCoords`.
  - `0x005880A0` `FUN_005880A0` - live decompile confirmed line-walk logic, building lookup, firer-house parameter, and sentinel return.
  - `0x00468D80` `BulletClass::BulletDetonation` - live decompile confirmed default `Location` CoordStruct and target CoordStruct override calls.
  - `0x00468BB0` `BulletClass::BounceCheck` - live decompile confirmed `SubjectToCliffs` / `SubjectToWalls` call into `FUN_004CC360`.
  - `0x0041BDD0`, `0x006FDD50`, `0x004666E0` - assembly start/context sampled.
- Docs searched:
  - `docs/fidelity-checks/2026-05-17-gi-small-arms-warhead-impact-placement.md`
  - [docs/research/BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_PROJECTILE_SYSTEM_CONSOLIDATED_REPORT.md)
  - [docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md)
  - [docs/research/BULLET_CLASS_AI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_CLASS_AI_GHIDRA_REPORT.md)
  - [docs/research/BULLETCLASS_TRAJECTORY_AND_HOMING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_TRAJECTORY_AND_HOMING.md)
  - [docs/research/BULLET_CLASS_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_CLASS_LAYOUT_GHIDRA_REPORT.md)
  - [docs/research/BULLETTYPECLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETTYPECLASS_GHIDRA_REPORT.md)
  - [docs/research/WARHEAD_DETONATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEAD_DETONATE_GHIDRA_REPORT.md)
  - [docs/research/ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_SPAWN_PATHS_GHIDRA_REPORT.md)
  - `docs/research/BRIDGE_OBJECT_ONBRIDGE_EXTRA_WRITERS_GHIDRA_REPORT.md`
  - [docs/research/WEAPONTYPECLASS_VERIFICATION_AND_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WEAPONTYPECLASS_VERIFICATION_AND_CONSUMERS_GHIDRA_REPORT.md)
- INI files checked:
  - `ini/rulesmd.ini`
  - `ini/rules.ini`
  - `ini/artmd.ini`
  - `ini/art.ini`
- Related plans:
  - `docs/plans/2026-05-10-warhead-detonation-smudge-spawn-plan.md`
  - `docs/plans/2026-05-17-ggi-guardian-gi-investigation-plan.md`
