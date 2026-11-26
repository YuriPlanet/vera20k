# Optional Phobos warhead decloaking

Build with `--features phobos` to choose per warhead whether a cloaked object
it damages starts to uncloak.

```ini
[General]                     ; rulesmd.ini
DecloakDamagedTargets=true    ; boolean, default yes

[SOMEWARHEAD]                 ; a [Warheads] entry
DecloakDamagedTargets=        ; boolean, defaults to [General]
```

The warhead key is a Phobos `Nullable<bool>`: `yes` and `no` override the
global, while an empty, `none` or invalid value leaves it to `[General]`.

## Native sites

- `TechnoClass::ReceiveDamage @ 0x00701900` calls virtual `+0xFC`
  (`0x00703850`, `StartUncloaking(0)`) at `0x0070281D` for every result that
  is not NowDead. Phobos `TechnoClass_ReceiveDamage_Decloak` (`0x00702819`)
  replaces that call and resumes at `0x00702823`: only a warhead whose value
  is yes uncloaks the object, and a warhead with no Phobos extension skips it.
- Phobos `ShieldClass::ReceiveDamage` uncloaks the shield's owner when the
  shield takes a positive hit after its `ReceivedDamage` clamps and the warhead
  allows it, before the native receiver body runs. A hit the shield fully
  absorbs still surfaces the owner; a heal never does.
- When Ares plays in a `phobos` build, Ares's aftermath at `0x00702819` skips the native uncloak
  for a hit that leaves the object unaffected, so a fully absorbed hit
  surfaces the owner only through the shield.

Source: `src/Ext/Techno/Hooks.ReceiveDamage.cpp`,
`src/New/Entity/ShieldClass.cpp`, `src/Ext/WarheadType/Body.cpp` and
`src/Ext/Rules/Body.cpp`, by Starkku.

## Where VERA20K owns it

- `rules/warhead_type.rs`: the per-warhead `DecloakDamagedTargets`.
- `rules/ruleset.rs`: `[General] DecloakDamagedTargets` and
  `RuleSet::decloak_damaged_targets_for`, which falls back from the warhead to
  the global.
- `sim/shield.rs`: `ShieldReceipt::shield_damage_positive`.
- `sim/combat/world_receiver.rs`: both uncloak calls, through
  `start_uncloaking_for_damage`.
