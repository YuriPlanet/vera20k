# EMPulse.*

Phobos extends Ares's `Type=EMPulse` superweapon. Two of its four keys share
a name with an Ares `SW_EMPulse` key, so VERA keeps one value per SW type and
both consumers read it.

Readers `Ext/SWType/Body.cpp:179-182`, declarations
`Ext/SWType/Body.h:109-112`, constructors `:196-201`. Consumers:
`Ext/SWType/SWHelpers.cpp:232-290`, `Ext/SWType/FireSuperWeapon.cpp:37`,
`:356-404`, `Ext/Building/Hooks.cpp:132-208`.

## Keys

| Key | Native type | Default | VERA owner |
| --- | --- | --- | --- |
| `EMPulse.WeaponIndex` | `Valueable<int>` | `0` | `AresEMPulseRules::empulse_weapon_index` |
| `EMPulse.SuspendOthers` | `Valueable<bool>` | `false` | `AresEMPulseRules::empulse_suspend_others` |
| `EMPulse.Cannons` | `ValueableVector<BuildingTypeClass*>` | empty | Ares's `AresEMPulseRules::cannons` |
| `EMPulse.TargetSelf` | `Valueable<bool>` | `false` | Ares's `AresEMPulseRules::target_self` |

`Valueable<int>` is `INI_EX::ReadInteger` and `Valueable<bool>` a plain
`ReadBool` (`Utilities/TemplateDef.h:99-113`); a failed or absent read keeps
the constructor value. Both extensions are compiled only with
`ares,phobos`: `Type=EMPulse` is Ares's superweapon, and VERA's
`SW_EMPulse::Activate` owner is `#[cfg(feature = "ares")]`. With Phobos alone
the keys are dormant, as they are in a game without Ares.

**`EMPulse.Cannons` is not a new key.** Ares `SW_EMPulse::LoadFromINI`
(`10071510`) reads it into `Cannons` and VERA's `targeting::launch_site`
already uses that list. Phobos reads the same key into its own
`SWTypeExt::EMPulse_Cannons`. VERA reuses the Ares owner instead of adding a
second field. One native difference: Ares's list is `Find` (unknown names
skipped, `src/rules/native_processing.rs:8354`), while Phobos's
`ValueableVector<BuildingTypeClass*>` specialization uses
`BuildingTypeClass::FindOrAllocate` (`Utilities/TemplateDef.h:1993-2006`),
which allocates a placeholder type for an unknown token instead of dropping
it. In VERA a placeholder has no matching building, so only
`cannons.is_empty()` could differ; that is a documented residual below.

`EMPulse.TargetSelf` is likewise Ares's `SWTypeExt` flag. Phobos reads it
again and uses it at `FireSuperWeapon.cpp:37`:

```cpp
if (static_cast<int>(pType->Type) == 28 && !pTypeExt->EMPulse_TargetSelf)
    pTypeExt->HandleEMPulseLaunch(pSW, cell);
```

So `TargetSelf=yes` skips the whole Phobos launch tail (no cannon latch, no
suspension), while `SWHelpers.cpp:272` gives `GetEMPulseCannonRange` (-1,-1).
VERA's Ares `EMPulse.TargetSelf` arm still self-detonates the cannon.

## Behavior

`SWTypeExt::HandleEMPulseLaunch` (`Ext/SWType/FireSuperWeapon.cpp:356-404`)
walks `GetEMPulseCannons` (`SWHelpers.cpp:232-268`), stamps
`BuildingExt::CurrentEMPulseSW` on up to `SW.MaxCount` of them, and with
`SuspendOthers` holds every other `Type=EMPulse` Super of the House. A hold
happens when both the firing SW's and the other SW's cannon lists are empty,
or when the two lists share a type (`:384-392`). The held types go into
`HouseExt::SuspendedEMPulseSWs` (`Ext/House/Body.h:66`) under the firing SW
type.

`BuildingClass_Mission_Missile_EMPulseSelectWeapon @ 0x44CEEC`
(`Ext/Building/Hooks.cpp:132-189`) is the status-2 hook: a non-negative
`EMPulse.WeaponIndex` fixes the weapon slot; a negative one calls
`SelectWeapon` on the cell (or its content) at the owner's `EMPTarget`.
It then resumes the suspended Supers and erases that map entry, clears
`CurrentEMPulseSW`, and stores the slot for the `GetFireCoords` FLH wrapper
(`:191-198`, jump `0x44D1F9`). `0x44D455` (`:201-208`) gives the missile's
bullet that weapon.

## In VERA

- Rules: `AresEMPulseRules` gains the two new keys with `#[cfg(feature =
  "phobos")]` fields, read in its existing `read_pass`;
  `cannons` and `target_self` stay on the same owner.
- Launch: `src/sim/superweapon/emp_pulse.rs::launch` is the single owner.
  For a non-`TargetSelf` EMPulse it calls `suspend_phobos_others` (the
  `FireSuperWeaponExt` tail at `:37`) and stamps
  `GameEntity::phobos_current_empulse_sw` on each cannon it sends to
  `MissionType::Missile`. That latch is separate from Ares's
  `TechnoExt::SuperWeapon`, which other owners read.
- House state: `HouseState::suspended_emp_pulse_sws` is a
  `BTreeMap<InternedId, Vec<InternedId>>` keyed by firing SW type, with
  `#[serde(default)]` and a non-empty-only fold in `world_hash.rs`.
- Mission: `src/sim/world/techno_ai/building_missile.rs::emp_shot` consumes
  and clears the latch, resumes the held Supers through the existing
  `SuperWeaponInstance::suspend` primitive, and uses the chosen slot for
  `get_weapon` and `fire_coordinate`. No second launch owner is introduced.

## Residuals

- **Cannon selection is Ares's, not Phobos's union.** `GetEMPulseCannons`
  accepts a listed type *or* any `EMPulseCannon=` building that
  `IsLaunchSite` admits (`SWHelpers.cpp:241-248`), while VERA's
  `targeting::launch_site` uses Ares's either/or rule (`cannons.is_empty()`
  selects one branch). A listed-type SW therefore does not also latch an
  unlisted Ares EMPulseCannon in VERA.
- **Unknown `EMPulse.Cannons` tokens.** Ares drops them; Phobos's
  `FindOrAllocate` keeps a placeholder. VERA follows Ares, so a list of only
  unknown names is empty here (`SuspendOthers` sees "no restriction") where
  Phobos would see a non-empty list.
- **`SelectWeapon` target.** Native prefers the cell's content over the cell
  itself (`Ext/Building/Hooks.cpp:143-158`); VERA passes
  `TargetKind::Cell` to `combat_weapon::select_weapon_against`. A negative
  SelectWeapon answer falls back to slot 0.
- **FLH and pitch.** Native's `GetFireCoords` wrapper and the bullet use the
  chosen slot; VERA applies it to `get_weapon` and `fire_coordinate`, while
  `emp_barrel_pitch`, `emp_launch_speed` and the turret pivot still read
  weapon 0.
- **Phobos's own index mixing.** `FireSuperWeapon.cpp:400-403` pushes the
  *firing* SW's `ArrayIndex` into the residue when the key already exists,
  but the other SW's index on the first insert, and the resume hook indexes
  `pHouse->Supers` with those values. VERA stores interned SW type IDs, which
  is the intended identity without that ambiguity.
- **Tail order for an Ares-handler EMPulse.** Native
  `FireSuperWeaponExt` runs LimboDelivery/LimboKill/Detonate, `SW.Next`,
  Convert and `SW.Link` before `HandleEMPulseLaunch`, and the activated
  message/EVA after it. VERA's Ares handler arm launches the cannons and the
  `SuspendOthers` hold from `emp_pulse::launch` before
  `finish_ares_launch`'s Phobos tail and EVA event. Trigger: an EMPulse SW
  authoring another Phobos launch-tail key; no retail SW does. Effect: a
  chained Super or conversion sees the hold one step earlier.
- **`SW.MaxCount=0`.** Native's stamp loop breaks only at `i + 1 == count`,
  so a zero count stamps every cannon; VERA's Ares loop stops at `fired >=
  count` and stamps none. Pre-existing Ares activation behavior, unchanged
  here.
- **Retail data.** No retail SW type sets these four keys; the assertions use
  the synthetic EMPulse arena.
