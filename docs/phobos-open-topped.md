# OpenTopped per-type properties

Phobos makes the eleven `OpenTopped.*` `TechnoType` keys configurable per
transport type, with two distinct fallback chains. This page records the
native identities, the corrections to the port brief, and what VERA20K owns.

## Key inventory and fallbacks

The eleven type keys are read in one block by
`TechnoTypeExt::LoadFromINIFile` (`src/Ext/TechnoType/Body.cpp:879-889`),
declared `Nullable` at `Body.h:149-159` and constructed `{}` (unset) at
`Body.h:546-556` — there is **no hard default at the type layer**.

| Type key (`Body.cpp`) | Type | Fallback |
| --- | --- | --- |
| `OpenTopped.RangeBonus` :879 | `Nullable<int>` | YR `[CombatDamage] OpenToppedRangeBonus` (default 2 cells) |
| `OpenTopped.DamageMultiplier` :880 | `Nullable<float>` | YR `OpenToppedDamageMultiplier` (default 1.0f) |
| `OpenTopped.WarpDistance` :881 | `Nullable<int>` | YR `OpenToppedWarpDistance` (default 5 cells; stock ini 7) |
| `OpenTopped.IgnoreRangefinding` :882 | `Nullable<bool>` | `[General]` same name, default **false** |
| `OpenTopped.AllowFiringIfDeactivated` :883 | `Nullable<bool>` | same name, default **true** |
| `OpenTopped.AllowFiringIfAttackedByLocomotor` :884 | `Nullable<bool>` | same name, default **true** |
| `OpenTopped.ShareTransportTarget` :885 | `Nullable<bool>` | same name, default **true** |
| `OpenTopped.UseTransportRangeModifiers` :886 | `Nullable<bool>` | same name, default **false** |
| `OpenTopped.CheckTransportDisableWeapons` :887 | `Nullable<bool>` | same name, default **false** |
| `OpenTopped.DecloakToFire` :888 | `Nullable<bool>` | same name, default **false** |
| `OpenTopped.FireWhileMoving` :889 | `Nullable<bool>` | same name, default **true** |

The eight `[General]` globals are read by `RulesExt::LoadFromINI`
(`src/Ext/Rules/Body.cpp:455-462`), declared `Valueable<bool>` at
`Rules/Body.h:365-372` with constructor defaults `:870-877`. A `Valueable`
global has no unset state and no `none`/`<default>` clearing token. Loading
order: `RulesData_LoadBeforeTypeData @ 0x679A15` reads the globals first;
each type's `TechnoTypeExt::LoadFromINIFile` (`Body.cpp:753`) reads the type
keys later; the fallback is evaluated at the hook at runtime by `.Get(...)`.

## Corrections to the port brief (authoritative)

1. **Eleven type keys, not nine.** `Body.cpp:879-889` is one contiguous block
   of eleven reads; the brief omitted `:886 OpenTopped.UseTransportRangeModifiers`
   and `:887 OpenTopped.CheckTransportDisableWeapons`. Those two are **also**
   `[General]` globals (`Ext/Rules/Body.cpp:459-460`) — a dual identity.
2. **Two separate fallback chains.** The eight boolean type keys fall back to
   the Phobos `[General]` `Valueable<bool>` globals of the same name. The three
   numeric keys fall back to the **YR engine** `[CombatDamage]` fields
   `OpenToppedRangeBonus` / `OpenToppedDamageMultiplier` /
   `OpenToppedWarpDistance`; Phobos reads no global and sets no default for
   them. There are **no numeric keys among the eight new globals**, and the
   numeric keys must not fall back to them. `OpenTransport.FireWhileMoving`
   is a three-key AND with the carrier and weapon keys; the passenger-side
   companion keys (`Rules/Body.cpp:463-465`) are read by
   [`rules::phobos_open_transport`](phobos-open-transport.md).
3. **`DamageMultiplier` is `Nullable<float>`, not double**
   (`TechnoType/Body.h:150`). VERA reads with
   `read_phobos_nullable_double` (f64) then narrows to `f32`.
4. **`DecloakToFire` default is false.** `docs/New-or-Enhanced-Logics.md:1912`
   says default true, but the constructor `Ext/Rules/Body.h:876` is
   `{ false }`. The source wins.
5. **`IgnoreRangefinding` is not an InRange branch.** It acts on
   `TechnoClass_WeaponRange @ 0x7012C2`
   (`Ext/Techno/Hooks.WeaponRange.cpp:4-49`), which reimplements the entire
   `TechnoClass::WeaponRange` and skips clamping the carrier's effective
   range to the shortest passenger weapon. `RangeBonus` is the one that lives
   in `0x6F72D2`'s InRange distance tolerance.
6. **`WarpDistance` is not "the minimum distance the vehicle teleports" nor a
   stealth distance.** It is the passenger/vehicle-versus-target distance
   threshold in a temporal attack,
   `TemporalClass_AI_Opentopped_WarpDistance @ 0x71A82C`
   (`Ext/Techno/Hooks.Transport.cpp:185-197`), in cells (used times 256);
   beyond it the temporal class Lets Go, otherwise it erases. It is unrelated
   to `FootExt::LastWarpDistance` (TeleportLocomotion).
7. **`Hooks.TargetEvaluation.cpp` has no `OpenTopped_` hits** — that brief
   assumption is wrong. `IgnoreRangefinding` is in `Hooks.WeaponRange.cpp`;
   `ShareTransportTarget`, `RangeBonus` and `WarpDistance` are in
   `Hooks.Transport.cpp`; `UseTransportRangeModifiers` is in
   `Ext/WeaponType/Body.cpp`.

## Consumers, order, native addresses

- **`0x6FC5C7` `TechnoClass_CanFire_OpenTopped`**
  (`Ext/Techno/Hooks.Firing.cpp:439-482`), strict order:
  ① `pTransport->Deactivated && !AllowFiringIfDeactivated` → `Illegal`
  (`0x6FC86A`); ② `pTransport->Transporter` (carrier itself nested) →
  `Illegal` unconditionally, key-free; ③ take the passenger weapon;
  ④ `CheckTransportDisableWeapons && HasWeaponsDisabled(pTransport) &&
  pWeapon` → the T3 `Rearm` exit (`0x6FC0DF`, return code 3), **not**
  `Illegal` and not `OutOfRange`; ⑤ only when the
  carrier casts to `FootClass`: 5a `IsAttackedByLocomotor &&
  !AllowFiringIfAttackedByLocomotor` → `Illegal`; 5b the three-key AND being
  false runs the moving check (`BalloonHover` uses `Is_Moving_Now()`, else
  `Destination != nullptr`) → `Illegal`; ⑥ all pass → `Continue` (`0x6FC5D5`).
  VERA `FireError`: `Illegal = 5`, `Rearm = 3` (`fire_error.rs:50-63`).
- **`0x4D9510` `FootClass_SetDestination_OpenToppedFireWhileMoving`**
  (`Hooks.Transport.cpp:893-952`): on setting a destination, release each
  passenger's `LocomotorTarget` / `TemporalImUsing` when it may not fire while
  moving. VERA runs it from the admitted non-null destination in
  `movement/navcom.rs` (`phobos_open_topped_release_locks` in
  `sim/passenger.rs`): the carrier's `OpenTopped.FireWhileMoving` and the
  rider's `OpenTransport.FireWhileMoving` must both allow firing, and a link
  whose selected weapon carries `Warhead.IsLocomotor` (`Warhead.Temporal` for
  the temporal link) still goes when that weapon is `FireWhileMoving=no`.
  Tests `a_moving_open_topped_carrier_releases_a_rider_that_cannot_fire`,
  `a_locomotor_weapon_without_fire_while_moving_releases_the_link`.
- **`0x6FCA26` `TechnoClass_CanFire_RevertAresOpenTopCloakFix`**
  (`Hooks.Cloak.cpp:179-199`): `OpenTopped.DecloakToFire` true returns
  `NotApplicable(0x6FCA5E)`, skipping the Ares forced-decloak verdict; false
  keeps the native/Ares check.
- **`0x6FCD1D` `TechnoClass_CanFire_OpenTopCloakFix`**
  (`Hooks.Cloak.cpp:201-214`): `checkIfTargetInRange` and a passenger in an
  open-topped carrier with `DecloakToFire` → `pThis->Transporter->Uncloak(true)`
  (uncloaks the **carrier**, not the passenger). VERA runs it at the
  `FireError::Ok` verdict in `admit_attacker_fire`, after the
  target-in-range GetFireError call, through
  `open_topped_decloak_carrier`; the production test fires a passenger from
  a fully cloaked carrier and asserts the 2 → 3 transition only with the key.
- **`0x6FE43B` `TechnoClass_FireAt_OpenToppedDmgMult`**
  (`Hooks.Firing.cpp:714-741`): replaces FireAt's `fild
  [OpenToppedDamageMultiplier]`; `EAX = int(damage * multiplier)` →
  `0x6FE45A`. **`GetCurrentFirepowerMultiplier`**
  (`Ext/Techno/Body.cpp:208-236`) is a **separate** path, not serialized with
  FireAt; both multiply by the carrier key.
- **`0x7012C2` `TechnoClass_WeaponRange`**
  (`Hooks.WeaponRange.cpp:4-49`): `!IgnoreRangefinding && type->OpenTopped`
  clamps `result` to `min(carrier weapon range, all passenger
  FireInTransport weapon ranges)`. Used by threat/approach scanning, not the
  passenger's actual InRange fire.
- **`0x6F72D2` `TechnoClass_IsCloseEnoughToTarget_OpenTopped_RangeBonus`**
  (`Hooks.Transport.cpp:167-183`): `pThis` is the passenger, `pExt` the
  **carrier** type ext; `EAX(carrier RangeBonus.Get(YR global) + passenger
  OpenTransport.RangeBonus.Get(...))` → `0x6F72DE`, cells; VERA `<< 8`.
- **`0x710552` `TechnoClass_SetOpenTransportCargoTarget_ShareTarget`**
  (`Hooks.Transport.cpp:199-215`): `pTarget != nullptr &&
  !ShareTransportTarget` → `0x71057F` (skip); `pTarget == nullptr` (clear) is
  never intercepted.
- **`0x71A82C` `TemporalClass_AI_Opentopped_WarpDistance`**
  (`Hooks.Transport.cpp:185-197`): a passenger with a `Transporter` gets
  `EDX(carrier WarpDistance.Get(YR global OpenToppedWarpDistance))` →
  `0x71A838`; otherwise the native path continues.
- **`UseTransportRangeModifiers`**:
  `WeaponTypeExt::GetRangeWithModifiers` (`Ext/WeaponType/Body.cpp:398-416`)
  switches the modifier source from passenger to carrier. VERA already had
  this key fully ported and it is the template for the new aggregate.
- Related: `0x6F7248` `TechnoClass_InRange_WeaponRange` (ExtraRange, not this
  key); `0x71A88D` `TemporalClass_AI`; `0x679A15` / `0x679CAF` RulesData
  loading. Docs: `docs/New-or-Enhanced-Logics.md:72,82,123-124,223-224,
  1894-1943` (`:1903` three-key AND, `:1921-1923` numeric `[CombatDamage]`
  fallback, `:1937-1939` passive targeting ignores RangeBonus),
  `docs/Fixed-or-Improved-Logics.md:71,223,233,321,377,3055`.

## Where VERA20K owns it

`rules::phobos_open_topped` holds the eleven `Option` fields with
`from_section` and per-key `.Get`-equivalent accessors that take `&RuleSet`:
the three numerics fall back to `garrison_rules` / `combat_damage`, the eight
bools to `PhobosGlobals` (which reads the eight `[General]` keys). The
aggregate is a `#[cfg(feature = "phobos")]` field on `ObjectType` with a
getter and a constructor read. All consumers are `cfg`-isolated; the stock
(no-features) build keeps the YR globals and the native behavior unchanged.

Consumer owners: `sim::combat::in_range` (RangeBonus),
`sim::combat::world_fire` (DamageMultiplier through the shared
`firepower_stages`, which also feeds `GetCurrentFirepowerMultiplier`, plus
`0x6FCD1D` carrier uncloak), `sim::temporal` (WarpDistance),
`sim::combat::combat_weapon` (IgnoreRangefinding), `sim::passenger`
(ShareTransportTarget), `sim::combat::fire_error` / `fire_error_world`
(the `0x6FC5C7` gate and `0x6FCA26` revert). A shared
`sim::attach_effect::open_topped_transport_type` resolves the carrier's
`ObjectType` for a passenger.

`open_topped_use_transport_range_modifiers` remains a separate `ObjectType`
field consumed by `range_modifier_source` (untouched), so that one INI key is
read twice — once into the standalone `Option<bool>` and once into the new
aggregate — and its `[General]` global is read both into
`AttachEffectGlobals` and `PhobosGlobals`. Both use `read_bool` with the same
default (`false`), so the effective value is identical.

The three numeric overrides preserve the two distinct native overflow
semantics: `in_range.rs` keeps `i64 << 8`, `temporal.rs` keeps
`i32.wrapping_mul(256)`. The `damage/attacker.rs:52-76` x87 stage order, the
`GarrisonRules` / `CombatDamageDefaults` defaults and `from_ini`, T32/T33 and
T1..T11, and the stock clear-target semantic are unchanged.

## Verification

No retail `ini/`, `config.toml` or `RA2_DIR` exists on this machine, so the
retail-INI test fixtures (`open_topped_fire_tests.rs`, all `#[ignore]`) are
skipped. **No retail, visual or GPU verification was performed.** The port is
checked by three lightweight production-path regression tests (rules read,
per-type RangeBonus override through `mapless_in_range`, and
IgnoreRangefinding through `weapon_range`), each individually falsified
(break the implementation → that test red → restore → green) under the
`phobos`, stock (no features), and `ares,phobos` builds. The stock build
compiles with the narrow filter selecting zero tests (stock behavior
unchanged).

## Residuals (trigger · impact · frequency · risk)

- **`FireWhileMoving` `0x4D9510`** **is ported**:
  `Simulation::phobos_open_topped_release_locks` (`sim/passenger.rs`), called
  from `Simulation::begin_foot_destination` (`sim/movement/navcom.rs`) as soon
  as a non-null destination is admitted — the same `FootClass::SetDestination`
  call, before the NavCom write rather than after it. A moving `OpenTopped`
  Unit with passengers makes each passenger drop the locomotion link
  (`TechnoClass+0x1D0`-style `lift.target()`) or the temporal one
  (`TemporalImUsing`) it holds when the carrier's `OpenTopped.FireWhileMoving`,
  the passenger's `OpenTransport.FireWhileMoving` or the selected weapon's own
  `FireWhileMoving` says no. The weapon half reads `IsLocomotor=`/`Temporal=`
  on the warhead through the shared `select_weapon_against` owner. Tests:
  `sim::combat::phobos_open_transport_tests::
  a_moving_open_topped_carrier_releases_a_rider_that_cannot_fire` and
  `...::a_locomotor_weapon_without_fire_while_moving_releases_the_link`.
  The key is also consumed by the `0x6FC5C7` firing gate (5b).
- **`OpenTransport.*` companion keys**: ported by
  [`phobos-open-transport.md`](phobos-open-transport.md) (range, damage and
  the passenger `FireWhileMoving` term); the `0x4D9510` row above is the
  remaining transport-order gate they also feed.
- **`BalloonHover` `Is_Moving_Now()` variant approximated by `NavCom`** in
  the 5b moving check: trigger = a `BalloonHover=yes` open-topped carrier;
  impact = the moving check reads `Destination` rather than
  `Is_Moving_Now()`; frequency = very low (no retail open-topped
  BalloonHover type); risk = low.
- **Check ④ verdict address `0x6FC0DF`**: both the Phobos hook and the stock
  T3 rows leave through the same shared exit, which the stock corpus labels
  `Rearm` (3); VERA returns `Rearm` for the disabled-carrier gate. No
  divergence remains.
- **`crit.rs:616 current_firepower_multiplier`** is an independent path that
  folds only the occupy stage and carries no open-topped stage; not synced.
  Trigger = crit damage path; impact = no per-type open-topped multiplier in
  that path; frequency = low; risk = low (Phobos `GetCurrentFirepowerMultiplier`
  and FireAt are both covered by `firepower_stages`).
- **Double / triple INI reads of `UseTransportRangeModifiers` and
  `CheckTransportDisableWeapons`** (standalone field + aggregate +
  `AttachEffectGlobals` + `PhobosGlobals`): trigger = any rules load;
  impact = identical `read_bool` values with the same defaults, no behavioral
  divergence; frequency = always; risk = none (documented, not a defect).
- **No retail/GPU/visual verification**: trigger = this environment lacks a
  retail install; impact = no claim of visual parity; frequency = this port;
  risk = none for the simulation paths, which are corpus- and test-backed.
