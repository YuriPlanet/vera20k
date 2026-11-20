# Convert.Health

A Foot can convert to another type when its health ratio enters a configured
band.

```ini
[CONV]                                ; TechnoType
Convert.Health=                       ; Nullable<TechnoTypeClass*>, absent/none = off
Convert.Health.AbovePercent=-1.0      ; Valueable<double>, default -1.0
Convert.Health.BelowPercent=-1.0      ; Valueable<double>, default -1.0
```

Readers: `Ext/TechnoType/Body.cpp:1193-1197`. Declarations
`Ext/TechnoType/Body.h:433-435`, defaults `-1.0`/`-1.0`/null at `:853-855`.
A reversed pair is swapped at read time (`:1197`).

## Behavior

`FootExt::HealthAutoConvertActions` (`Ext/Foot/Body.cpp:909-927`) runs from
the `FootClass_AI` tail (`Ext/Techno/Hooks.cpp:42-47`, hook `0x4DA54E`)
after the ammo auto-convert. It does nothing when `Convert.Health` is unset
or when both bounds are negative; otherwise
`TechnoExt::IsHealthInThreshold` (`Ext/Techno/Body.cpp:752-759`) asks

```
(hp > 0 ? hp > AbovePercent : hp >= AbovePercent) && hp <= BelowPercent
```

with `hp = Health / Strength` as a double and zero/zero health always inside.
On a match the object converts through `TechnoExt::ConvertToType`.

## Where VERA20K owns it

`rules::phobos_convert_health` reads the three keys (the swap is applied
there). `Simulation::phobos_health_auto_convert`
(`sim/game_entity/type_conversion.rs`) runs from the Foot AI tail in
`sim/world/techno_ai.rs`, before the attack-move follower count ages. It
resolves the target's `DeliveryType` and calls `convert_techno_type`, the
single in-place conversion owner also used by the Ares `Convert.Deploy`
path. The threshold comparison reuses
`combat::Health::compare_ratio`, so the masked-x87 ratio semantics are the
same owner the rest of the simulation uses.

The conversion owner was Ares-gated; its stock helpers
(`Health::set_fraction`, `ConversionHealth::capture_in_place`,
`Miner::change_type_binding`, `TechnoAmmo::change_type_capacity`,
`ObjectType::secondary_turn_rate`, `LocomotorState`/`StashedLocomotor`
rebinding) are now compiled for both features.

RESIDUAL: Phobos' `ConvertToType` calls Ares' `ConvertTypeTo` when Ares is
installed and otherwise runs its own smaller fallback. VERA always runs its
port of the Ares-shaped body. In a phobos-only build that is a superset of
Phobos' fallback (it rebinds the locomotor and its caches instead of
replacing the locomotor object, and drops the Ares-only attachment/stat
recomputation). Trigger: `Convert.Health` under a phobos-only install.
Effect: the converted object keeps its locomotor instance and timers where
the Phobos fallback would have rebuilt it. Frequency: only mods using
`Convert.Health`.

Other `Convert.*` triggers are separate chains: `Convert.Undeploy` (with
the deploy-convert latches), `Convert.HumanToComputer`/`ComputerToHuman`
(owner change plus the mission fix) and `Convert.ResetMindControl` (the
capture-manager rebuild in the conversion body).
