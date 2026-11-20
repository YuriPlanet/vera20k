# Convert.ResetMindControl

A type change can rebuild the object's capture manager from the new type's
weapons.

```ini
[FOOT]                                ; TechnoType
Convert.ResetMindControl=             ; Nullable<bool>, unset -> [General]

[General]
Convert.ResetMindControl=false        ; Valueable<bool>, default false
```

Reader `Ext/TechnoType/Body.cpp:1017`, declaration
`Ext/TechnoType/Body.h:252`, constructor unset `:684`. Global
`Ext/Rules/Body.cpp:222`, default `false` (`Ext/Rules/Body.h:666`).

## Behavior

Phobos `FootExt::UpdateTypeData`'s capture-manager arm
(`Ext/Foot/Body.cpp:470-540`), run at the end of `TechnoExt::ConvertToType`.
The gate reads the OLD type's key (type value, else the `[General]` global).
When enabled it scans every `TechnoTypeClass::MaxWeapons` slot's current
weapon for a `MindControl=` warhead and derives:

- `maxCapture` = the highest `Damage=` among those weapons (0 when none);
- `infiniteCapture` = any weapon whose `InfiniteMindControl=` is set (a
  weapon key, `rules::weapon_type`).

Then:

- `maxCapture > 0` and a manager exists: when not infinite and the
  controlled count exceeds `maxCapture`, free the newest nodes down to the
  limit (`CaptureManagerClass::FreeUnit` order), then store
  `MaxControlNodes = maxCapture` and `InfiniteMindControl`;
- `maxCapture > 0` and no manager: create one with those values;
- `maxCapture == 0` and a manager exists: `FreeAll` and delete it;
- otherwise nothing changes.

## Native anchors

The convert pair is `InfantryClass_DoingAI_DeployConvert` — deploy `0x520B3E`, undeploy `0x520B99` — and
the flag reset this key adds is at `0x520E75` (`Ext/Infantry/Hooks.cpp:19,36,53`).

## Where VERA20K owns it

`rules::phobos_convert.reset_mind_control` reads the type key; the global
lives in `rules::phobos_globals`. `convert_techno_type`
(`sim/game_entity/type_conversion.rs`) captures the old type's effective
flag before the swap and calls `phobos_reset_mind_control_on_convert`, which
scans the new type's weapon slots through `combat_weapon::get_weapon` and
drives `CaptureManagerState`. The manager adopts the limits through
`phobos_reset_for_new_type` (the node list stays in place so each
`free_unit` restores the victim's original house and removes its nodes) or
is created/deleted with the existing owners.

RESIDUAL: the same native `UpdateTypeData` block also rebuilds the Temporal,
Airstrike, Parasite and BombSight classes and releases a stale Locomotor
target; those are separate mechanisms and are not part of this key's chain.
Trigger: converting between types that differ in them; effect: the object
keeps the old type's class instances.

RESIDUAL: the conversion body is the Ares-shaped `ConvertToType` in both
builds (see `docs/phobos-convert-health.md`).
