# Damage multipliers (`Damage*Multiplier`)

A `WarheadType` block that rescales the damage packet on **each** victim it
reaches, from that pair's Houses and both objects' health. It runs inside the
receiver, not at the detonation: one area hit can scale differently for two
targets in the same cell.

## Keys

| Key | Type | Default |
|---|---|---|
| `DamageOwnerMultiplier=` | `Nullable<double>` | unset |
| `DamageAlliesMultiplier=` | `Nullable<double>` | unset |
| `DamageEnemiesMultiplier=` | `Nullable<double>` | unset |
| `DamageOwnerMultiplier.Berzerk=` | `Nullable<double>` | unset |
| `DamageAlliesMultiplier.Berzerk=` | `Nullable<double>` | unset |
| `DamageEnemiesMultiplier.Berzerk=` | `Nullable<double>` | unset |
| `DamageSourceHealthMultiplier=` | `Valueable<double>` | `0.0` |
| `DamageTargetHealthMultiplier=` | `Valueable<double>` | `0.0` |

Read: `WarheadTypeExt::LoadFromINI` (`Ext/WarheadType/Body.cpp:347-354`),
declarations `Ext/WarheadType/Body.h:192-199`, constructor `Body.h:471-478`.
VERA reads them in `src/rules/phobos_damage_multiplier.rs`, owner
`WarheadType::phobos_damage_multiplier`.

`[CombatDamage]` globals (`Ext/Rules/Body.cpp:340-347`,
`Ext/Rules/Body.h:248-255`):

| Global | Default |
|---|---|
| `DamageOwnerMultiplier=` | `1.0` |
| `DamageAlliesMultiplier=` | `1.0` |
| `DamageEnemiesMultiplier=` | `1.0` |
| `DamageOwnerMultiplier.NotAffectsEnemies=` | unset |
| `DamageAlliesMultiplier.NotAffectsEnemies=` | unset |
| `DamageOwnerMultiplier.Berzerk=` | unset |
| `DamageAlliesMultiplier.Berzerk=` | unset |
| `DamageEnemiesMultiplier.Berzerk=` | unset |

They live on `RuleSet::phobos_globals`. There is deliberately **no**
`DamageEnemiesMultiplier.NotAffectsEnemies`: the substitution only exists for
the owner and ally columns.

## Behaviour

`Ext/Techno/Hooks.ReceiveDamage.cpp:48-82`, inside
`TechnoClass_ReceiveDamage_Shield @ 0x701900`, after the per-target effects
(`:39-45`) and before the combat alert (`:86`):

1. `if (!args->IgnoreDefenses && damage)` (`:50`) — a forced hit and a zero
   packet never rescale.
2. `pSourceHouse` is the attacker's owner, else `args->SourceHouse`; a missing
   source House counts as an enemy (`:53`).
3. Column (`:56-74`):
   - not allied, or no source House → **enemies**;
   - allied and a different House → **allies**;
   - the same House → **owner**.
   A berserk attacker reads the `.Berzerk` column of that same relation.
4. Each column resolves warhead nullable → global. The berzerk chain is
   `warhead.Berzerk ?? global.Berzerk ?? (plain global)`. The owner and ally
   plain/berzerk chains substitute the global `.NotAffectsEnemies` value when
   the warhead's `AffectsEnemies=no`.
5. `if (DamageSourceHealthMultiplier && pAttacker) multiplier += value *
   attacker.HealthPercentage` (`:76-77`) and `if (DamageTargetHealthMultiplier)
   multiplier += value * target.HealthPercentage` (`:79-80`) — **added**, not
   multiplied.
6. `if (multiplier != 1.0)` (`:82`) rewrites the packet as
   `(int)(damage * multiplier)`, and a truncation to zero keeps the damage's
   sign (`:84-87`). `Math__ftol`'s indefinite low dword is zero, so a NaN or
   out-of-range product also falls back to the sign.

VERA's owner is `sim::combat::damage_multiplier::apply`, called from
`world_receiver` right after the warhead's health/veterancy/neutral/invoker
gate and before `combat_alert::raise_combat_alert` and the shield stage, which
is the native order. `receiver_call::wave_post_object_damage` applies the same
function before its shield preview so the WaveClass shared per-cell damage
keeps matching what the committing receiver leaves there.

## Residuals

- **Health fraction on a zero `Strength`.** `GetHealthPercentage` is
  `Health / Strength` with no guard, so a zero-Strength type divides to
  infinity natively and VERA reproduces that (`0/0` becomes NaN, which the
  `ftol` fallback turns into the sign). A type with `Strength=0` is already
  degenerate elsewhere; no special case is worth adding.
- **`Math__ftol` vs `static_cast<int>`.** VERA routes the product through
  `X87Chop53::ftol_f64_low_masked`, the masked-`ftol` low dword. In-range
  values are identical to a C cast; the difference only shows at the
  integer-indefinite boundary.
- **The per-target effects run before this block only when
  `ApplyPerTargetEffectsOnDetonate` is off**, exactly as natively; that key's
  own owner already models the ordering.

## Tests

- `src/rules/phobos_damage_multiplier_tests.rs` —
  `the_eight_keys_construct_unset_and_zero`, `each_column_reads_independently`,
  `a_none_spelling_clears_a_column`, `the_health_terms_are_valueable`,
  `the_relation_follows_the_native_column_order`.
- `src/rules/phobos_globals_tests.rs` —
  `the_damage_multiplier_globals_default_and_read_from_combat_damage`.
- `src/sim/combat/damage_multiplier.rs` —
  `the_enemy_column_scales_the_packet`,
  `the_owner_column_governs_a_same_house_hit`,
  `the_ally_column_governs_an_allied_hit`,
  `the_berzerk_column_overrides_the_plain_enemy_column`,
  `not_affects_enemies_swaps_the_owner_column_fallback`,
  `the_health_terms_add_to_the_multiplier`,
  `a_truncation_to_zero_keeps_the_sign`,
  `forced_and_zero_packets_skip_the_block`.
