# Phobos dot-key families (package 45 batch)

Twelve small families ported on this branch (task 45, G2-G6 in the batch
commit and G1's three area/scatter keys in the two follow-up commits). Per-key
Phobos reads cite `Ext/...` relative to the Phobos source root; consumers
cite the hook address.

## CrashSpin.Multiplier

`TechnoType` `Valueable<float>`, constructor `1.0f`
(`Ext/TechnoType/Body.cpp:1072`, `Body.h:303`). Hook `0x4DECBB`
(`Ext/Techno/Hooks.Misc.cpp:978-999`): both crash spin rates scale by the
multiplier after the native floor, and a value `<= 0` replaces the whole
block, so the three Scenario draws do not happen. VERA: `ObjectType::
phobos_crash_spin_multiplier` (non-building), folded into `sim/world/crash.rs`
`spin_rate` before the f32 store chop; the stock `CrashSpin=no` Ares gate is
unchanged. Tests: `crash::phobos_spin_multiplier_tests` (exact f32 doubling)
and the zero-draw fixture test.

## Cloning.Powered

`BuildingType` `Valueable<bool>`, constructor `true`
(`Ext/BuildingType/Body.cpp:197`). `BuildingExt::KickOutClone`
(`Ext/Building/Body.cpp:476-486`, reached from the replaced clone-loop hook
`0x4449FB`): a dead, limboed, warping-out or powered-down vat exits no clone;
`Powered=no` restores the vanilla unconditioned exit. VERA:
`ObjectType::phobos_cloning_powered` and the guard in
`production_queue::exit_cloning_vat_copies` (the Ares clone path already folds
the power latch through `building_operational_state`). Test:
`a_powered_down_vat_exits_no_clone_unless_powered_is_no`.

## BuildingRadioLink.SyncOwner

`BuildingType` `Nullable<bool>` + `[General]` global default `true`
(`Ext/BuildingType/Body.cpp:257`, `Ext/Rules/Body.cpp:509`). Hook `0x4485DB`
(`Ext/Building/Hooks.cpp:1013-1018`): `no` SkipGameCodes the docked
take-along segment of `BuildingClass::ChangeOwner` (`0x00448559..0x004486C8`),
so a docked Foot keeps its owner and stays docked; the dismiss arm sits before
the hook. VERA: `ObjectType::phobos_building_radio_link_sync_owner`,
`PhobosGlobals::building_radio_link_sync_owner`, gate in
`world/mod.rs::take_docked_along_on_owner_change`.

## Damage.DealtByInvoker / Damage.ApplyOncePerLoop (AnimType)

`Ext/AnimType/Body.cpp:96-97`; the replaced anim damage body hook `0x42453E`
(`Ext/Anim/Hooks.cpp:40-149`) with the `[CombatDamage]
AnimDamage.DealtByInvoker` global fallback (`Ext/Rules/Body.cpp:570`,
default `false`). `DealtByInvoker` makes the stored invoker the attacker (a
techno `OwnerObject` when unset) and the invoker house (else the invoker's
owner, else the anim's house) the damage house; `ApplyOncePerLoop` lands
`round(Damage) x` the terrain multiplier only on the loop's first frame
(`max(Damage.Delay-1, 1)`). VERA: `AnimTypeRuntimeConfig` fields
(`rules/art_data.rs`), the attacker/house fold and the first-frame gate in
`anim_class.rs::anim_frame_damage`; the stock accumulate and Ares delay bodies
keep their native conversions.

The `Damage.ApplyFirepowerMult` fold is implemented (cached on the first
damage round, `anim_damage_applies_the_invokers_firepower_multiplier`); the
hook's `IsBuildingAnim -> ParentBuilding` fallback is covered by the anim's
`owner_entity` (a building anim stores its building there); and the `IsInert`
reject is the retained `no_effects` flag (trigger action 41), now checked
before the accumulator as the replaced body does. RESIDUALS: Phobos's replaced
body folds the stock damage branches through `std::round` while VERA keeps the
native/Ares conversion; the erase-anim owner house is propagated only through
the invoker path.

## FallingDownDamage.AllowEMP / .Water (+ the ratio model)

`[CombatDamage] FallingDownDamage` (`double`, default `1.0`) and
`.AllowEMP` (`bool`, default `true`, `Ext/Rules/Body.cpp:227-228`); TechnoType
`Nullable`s for the same three names (`Ext/TechnoType/Body.cpp:1140-1142`).
Hook `0x5F4021` (`Ext/Techno/Hooks.cpp:1014-1090`): an EMP-locked unit whose
`AllowEMP` refuses keeps the vanilla full-current-Health packet; otherwise the
ratio is the type's `.Water` on a bridge-less water/beach cell, else the
type's `FallingDownDamage`, else the global — negative takes that fraction of
the current Health, at most 1.0 that fraction of `Strength`, above 1.0
absolute, each truncated into the packet. VERA: globals in
`rules/combat_damage.rs`, type keys on `ObjectType`, the three-tier packet in
`parachute_descent.rs::phobos_falling_damage` with `landing_in_water`. The
`raise_falling_down` residual note (`game_entity.rs`) stands. TEST DEBT: the
landing receiver commit needs a map fixture; the tier tests were removed
pending it.

## Wake.Grapple / Wake.Sinking (and the native Wake=)

TechnoType `Nullable<AnimTypeClass*>` (`Ext/TechnoType/Body.cpp:1068-1069`),
falling back through the native per-type `Wake=` (now read on `ObjectType`)
to `[General] Wake=`; Sinking replaces the wake on both sinking sites
(`Ext/Techno/Hooks.Misc.cpp:404-422`), Grapple reads the **predator's** type
on the parasite pull (hook `0x629FA3`). VERA: the two spawn sites
(`sim/world/sinking.rs`, `sim/combat/parasite_grapple.rs::grapple_wakes`, now
owner-aware) resolve the chain with unknown names treated as null pointers,
and every override name is bound in `rules/effect_asset_catalog.rs`.

## WarpAway (TechnoType) / WarpOutWeapon

`WarpAway` is the time-erase anim list (`ValueableVector<AnimTypeClass*>`,
`Ext/TechnoType/Body.cpp:861`; `TechnoTypeExt` applies to every TechnoType, so
buildings read it too), not the teleport departure: hook `0x71A8BD`
(`Ext/Techno/Hooks.cpp:1476-1490`) replaces the `[General] WarpAway=` single
anim with `CreateRandomAnim`'s one Scenario pick owned by the target, ahead of
the Ares warhead `Temporal.WarpAway=`. `WarpOutWeapon` (`Read<true>`,
`Body.cpp:872`) detonates at the departing object with itself as target and
owner on an ordinary self-teleport departure, before the WarpOut anim
(`Ext/TechnoType/Hooks.Teleport.cpp:22-32`, hook `0x7193F6`); the
Chronosphere's chronoshift departure fires nothing. VERA:
`ObjectType::phobos_warp_away` (unknown names filtered before the draw) in
`temporal.rs::temporal_erase`; `WarpInWeaponRules::warp_out_weapon` with its
`FindOrAllocate` reference and `teleport_movement::apply_warp_out_weapon`
called from `object_turn.rs`. RESIDUALS: the erase anim's owner house has no
VERA spawn parameter; an all-unknown list falls back instead of drawing a null
pick.

## ChargeTurret.Delays

`WeaponType` `ValueableVector<int>` (`Ext/WeaponType/Body.cpp:135`).
`SetChargeTurretDelay` (`Ext/Techno/Hooks.Firing.cpp:936-976`, hooked at both
FireAt rearm sites `0x6FE4A4`/`0x6FF29E`): the just-fired burst shot's item
(`CurrentBurstIndex - 1` when `Burst > 1`, clamped to the last item)
overrides the saved `+2F8` charge denominator and starts the ext
`ChargeTurretTimer`; an item `<= 0` keeps the native value. The real rearm
timer keeps the native duration, and the turret progress prefers the ext
timer until it expires (`Ext/Techno/Hooks.cpp:241-244`). The native
`CurrentBurstIndex` has already been incremented for the shot at both rearm
sites (`0x6FF27C`, `0x6FE492`), so the item is the retained VERA index before
`complete_shot`. VERA:
`PhobosWeaponExtras::charge_turret_delays`,
`GameEntity::phobos_set_charge_turret_delay`,
`TurretSelection::phobos_charge_turret_timer` and the `update_charge_turret`
read; both FireAt rearm sites (including the DiskLaser early return) call the
setter. Tests:
`phobos_charge_turret_delays_override_the_progress_and_expire`,
`each_burst_shot_picks_its_own_charge_turret_delay_item`.

## IvanIconVisibility (+ IvanBomb.Visibility)

`[AudioVisual] IvanIconVisibility=` (`AffectedHouse`, default `Owner`,
`Ext/Rules/Body.cpp:448`) with the per-weapon `IvanBomb.Visibility=`
override (`Ext/WeaponType/Body.cpp:103`), consumed by the bomb-icon
visibility gate (hook `0x438D44`, `Ext/WeaponType/Hooks.Bomb.cpp:38-77`):
each house sees an attached bomb when its relation to the planter matches, or
through a `BombSight` detector whose owner's relation matches and which
stands within `(BombSight << 8)` of the carrier. VERA: the `seen_by` mask in
`bomb_list_update` recomputes per house through the shared
`can_target_house`, with the planting weapon's override read off
`Bomb::weapon` (now carried under `phobos` too). RESIDUAL: the hook's
`IsObserver` spectator arm has no VERA model.

## Scanner keys closed by this batch

`CrashSpin.Multiplier`, `Cloning.Powered`, `BuildingRadioLink.SyncOwner`,
`Damage.DealtByInvoker`, `Damage.ApplyOncePerLoop`, `FallingDownDamage`,
`FallingDownDamage.AllowEMP`, `FallingDownDamage.Water`, `Wake`,
`Wake.Grapple`, `Wake.Sinking`, `WarpAway`, `WarpOutWeapon`,
`ChargeTurret.Delays`, `IvanBomb.Visibility`, `CellSpread.Cylinder`,
`Temporal.ApplyMultiplier`, `Temporal.ApplyVersus`,
`Arcing.AllowElevationInaccuracy` — re-run `phobos_chain_analysis_v2.py`
after integrating.

## Temporal.ApplyMultiplier / .ApplyVersus (G1.3)

WarheadType `Nullable<bool>` pair (`Ext/WarheadType/Body.cpp:190-191`,
`Body.h:61-62`) over the `[CombatDamage]` globals (default `false`,
`Ext/Rules/Body.cpp:583-584`). `WarpPerStep::TemporalClassFake::
_GetWarpPerStep` (`Ext/Techno/Hooks.cpp:2338-2392`, LJMP `0x71AB10` replaces
`GetWarpPerStep`/`SumChainDamage`): each chain member's per-step warp starts
at its weapon's raw `Damage=`; `Multiplier` rescales by the attacker's
firepower multiplier (`(int)` truncation); the target-dependent factors — the
armor division, the deployed/crawling factor, the Verses fold and the final
product — need a live target and `warpPerStep > 0`; `Versus` multiplies the
deployed (`Damage.Deployed=`, the Ares warhead key) or crawling
(`ProneDamage=`) factor and folds the packet through the Verses kernel with
distance `0` (`MapClass::GetTotalDamage`). Every extension also needs a
resolved warhead pointer. VERA: `WarheadType::phobos_temporal_apply_*` +
`PhobosGlobals::temporal_apply_*`, the per-member fold in
`temporal.rs::temporal_warp_per_step` (chain walker moved to `&mut`).
RESIDUALS: `GetCurrentArmorMultiplier` is approximated by
`attach_effect_armor_multiplier`; the deployed factor reads the Ares combat
rules (1.0 in a phobos-only build); `GetCurrentFirepowerMultiplier` is VERA's
crit aggregate.

## CellSpread.Cylinder (G1.1)

WarheadType `Valueable<bool>`, default `false` (`Ext/WarheadType/Body.cpp:408`,
`Body.h:232,503`). The six `MapClass::DamageArea` distance hooks
(`Ext/WarheadType/Hooks.cpp:63-137`) replace the detonation Z with the
victim's, so every ground/deck, airborne and terrain distance test keeps only
the planar separation, and the building center record's two-Level vertical
allowance sees a zero difference (always admits). VERA: `WarheadType::
phobos_cell_spread_cylinder`, resolved once onto the `AoECollector` and
threaded through the three `push_*_aoe_damage` distance sites; the
`AffectsUnderground` walk's per-victim distance uses the planar arm when the
key is set (`world_receiver.rs::apply_underground_area_damage`), and the
warhead's per-target `DetonateOnOneUnit` pass carries the flag into
`getCellSpreadItemsExt` (`crit.rs::detonation_targets`), where it alters the
ground-Structure and aircraft distances. RESIDUAL: the underground walk visits
VERA's underground-layer entities rather than the native
`ScenarioExt::UndergroundTracker` membership (same set by construction).

## Arcing.AllowElevationInaccuracy (G1.2)

BulletType `Nullable<bool>` (`Ext/BulletType/Body.cpp:53`) over the
`[CombatDamage]` global (default **`true`**, `Ext/Rules/Body.cpp:477`). Hook
`0x6FF008` (`Ext/Bullet/Hooks.cpp:513-531`) and the EMP cannon site `0x44D46E`
(`:534-551`): for an `Arcing=yes` bullet with the key off,
`BulletExt::ApplyArcingFix` (`Ext/Bullet/Body.cpp:454-478`) recomputes the
launch velocity from the scattered target — `distanceCoords = target -
source`, `horizontalDistance = |dX,dY|`, `lobber = Lobber || (int)
horizontalDistance < dZ`, `speedMult = 0.45 / 0.68 / 1.0`, `speed = speedMult
* sqrt(horizontalDistance * gravity * 1.2)`, then `vX/vY = d * speed /
horizontalDistance` and `vZ = (dZ + vZ) * mult + gravity * horizontalDistance
/ (2 * speed)`; a degenerate horizontal or speed sets `vZ = speed` alone.
VERA: `ProjectileType::phobos_arcing_allow_elevation_inaccuracy` +
`PhobosGlobals::arcing_allow_elevation_inaccuracy`, the fold in
`launch.rs::apply_arcing_fix` applied at the end of `fireat_launch` (the
velocity is stored to binary64 bits, folded, and handed to
`ProjectileVelocity::from_native`); the world_fire caller resolves the key
per projectile, and the EMP cannon's status 2 (`0x44D46E`,
`building_missile.rs::emp_shot`) folds the same kernel over the vt+0x300
source and the target cell's ground coordinate. Test:
`launch::arcing_fix_tests` (aims at the scattered delta, degenerate guard,
lobber branch) and
`building_missile::arcing::the_emp_shell_folds_the_arcing_fix_when_elevation_inaccuracy_is_refused`.
RESIDUAL: the homing pass-through keeps the stock velocity (the corpus tests
pin it).

## Regression tests

- `778ba3ed8`: `phobos_charge_turret_delays_override_the_progress_and_expire`,
  `a_powered_down_vat_exits_no_clone_unless_powered_is_no`,
  `crash::phobos_spin_multiplier_tests::the_multiplier_scales_the_whole_double_product`,
  `phobos_crash_spin_zero_multiplier_skips_the_spin_and_its_draws`.
- `955a2394c`: `launch::arcing_fix_tests::the_fix_aims_at_the_scattered_delta`.
- Close-out reader tests for G1 (all `#[cfg(feature = "phobos")]`):
  `the_g1_combat_damage_gates_construct_and_read_from_their_own_section`
  (`rules/phobos_globals_tests.rs`),
  `the_g1_warhead_gates_default_and_keep_an_explicit_no` (`rules/warhead_type.rs`),
  `the_arcing_elevation_inaccuracy_nullable_reads_yes_no_and_unset`
  (`rules/projectile_type.rs`). The three cover constructor defaults, the
  `[CombatDamage]` section ownership, and the `Nullable` `Some(false)` state
  that lets a warhead refuse a `yes` global.
- Package 45 critic follow-up: `each_burst_shot_picks_its_own_charge_turret_delay_item`
  (`sim/combat/burst_no_delay_tests.rs`),
  `warp_per_step_needs_a_resolved_warhead_for_the_extension`
  (`sim/temporal_tests.rs`),
  `the_emp_shell_folds_the_arcing_fix_when_elevation_inaccuracy_is_refused`
  (`sim/world/techno_ai/building_missile.rs`).

TEST DEBT carried forward (no production-path integration test yet):
`Damage.DealtByInvoker` / `Damage.ApplyOncePerLoop` (the ares-style anim
damage test is `cfg(not(phobos))`), the wake overrides, warp-away/warp-out,
the ivan icon mask, radio-link sync-owner, and the falling-damage tier
packet (a map fixture is needed to drive the landing receiver commit). The
G1 `Temporal.ApplyMultiplier` firepower walk has a production test; its
armor/versus target factors still need an AttachEffect armor fixture, and the
`CellSpread.Cylinder` planar arms have reader-level coverage only.
