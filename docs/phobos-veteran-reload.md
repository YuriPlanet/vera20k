# Reload speed on promotion (`VeteranReload`, `VeteranEmptyReload`)

Two `Nullable<double>` multipliers that shorten (or lengthen) a promoted
object's reload clock, gated on the two Phobos rank abilities.

## Keys

| Key | Type | Default |
|---|---|---|
| `VeteranReload=` | `Nullable<double>` | unset |
| `VeteranEmptyReload=` | `Nullable<double>` | unset |

Read: `TechnoTypeExt::LoadFromINI` (`Ext/TechnoType/Body.cpp:1062-1063`),
declarations `Ext/TechnoType/Body.h:293-294`, constructor `Body.h:725-726`.
VERA reads them in `src/rules/phobos_techno_extras.rs`, owner
`ObjectType::phobos_techno_extras`.

`VeteranReload=` falls back to `[General] VeteranReload` (`Ext/Rules/Body.cpp:388`,
constructor `Ext/Rules/Body.h:808`, default `1.0`); `VeteranEmptyReload=` to
`[General] VeteranEmptyReload` (`:389`, `Body.h:809`, unset). Both live on
`RuleSet::phobos_globals`.

## Behaviour

`ScaleReloadDurationForVeterancy` (`Ext/Techno/Hooks.Firing.cpp:1180-1193`),
called from the two duration hand-offs in `StartReloading`: `0x6FB0CF` for the
empty-clip arm and `0x6FB14C` for the ordinary one (`:1196-1218`).

```cpp
if (duration <= 0 || !TechnoExt::HasAdditionalAbility(pThis, ability)) return duration;
const double multiplier = ability == AdditionalAbility::EmptyReload
    ? pTypeExt->VeteranEmptyReload.Get(Global()->VeteranEmptyReload.Get(Global()->VeteranReload))
    : pTypeExt->VeteranReload.Get(Global()->VeteranReload);
return Math::max(1, GeneralUtils::SafeMultiply(duration, multiplier));
```

`HasAdditionalAbility` (`Ext/Techno/Body.cpp:175-190`) needs a veteran or elite
rank, reads the veteran tier for a veteran, and the union of both tiers for an
elite. The bits come from `ReadAdditionalAbilities` (`Body.cpp:25-51`), which
re-reads the stock `VeteranAbilities=`/`EliteAbilities=` lists for the five
Phobos names — VERA already owns that bitset as
`ObjectType::phobos_abilities` (`crate::rules::phobos_crit::PhobosAbilityFlags`).

`SafeMultiply(int, double)` (`Utilities/GeneralUtils.cpp:321-331`) clamps the
double product into `i32` before `static_cast<int>`, which truncates toward
zero.

VERA's owner is `GameEntity::start_reloading_ammo` (`src/sim/game_entity.rs`):
it computes the stock duration per arm and then, under the `phobos` feature,
scales it through `scale_reload_for_veterancy`.

## Residuals

- **Reload clock re-arm points.** Native hooks the two `StartReloading`
  instructions; VERA scales the single value both arms store. Every caller
  reaches `start_reloading_ammo` through `shared_ammo::start_reloading`, so no
  reload clock is armed without the scale.
- **`ReloadIncrement`.** The ordinary arm's duration already includes the
  `ReloadIncrement` term before the scale, which is what native's `EAX` holds
  at `0x6FB14C`.

## Tests

- `src/rules/phobos_techno_extras_tests.rs` — the parse group.
- `src/rules/phobos_globals_tests.rs` — `the_board_queue_and_veteran_reload_globals`.
