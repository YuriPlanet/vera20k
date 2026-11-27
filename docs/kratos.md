# Kratos compatibility

VERA20K is starting a Rust-native port of [Kratos PP](https://github.com/ChrisLv-CN/KratosPP),
the C++ extension for Yuri's Revenge. Like Ares it is in every build, the release build too,
and the default Auto setting enables it when a game's data calls for it.

## The Kratos switch

Kratos plays in a game whose rules, art, mode or map hold a key that only a Kratos reader reads
(`src/rules/kratos_switch.rs`). The prone firing reader uses the final infantry Image after rules, mode and map overrides. Its ART reads
travel with the processed rules; the answer is decided once, when the Rules are projected, and
stored with them, so a save, a replay and every player in an online match reach it again from
the same files. The log names the deciding key, e.g.
`Kratos plays in this game: [GI] PrimaryCrawlingFLH is a Kratos key`, so a modder can see why.

A key that a stock, Ares or Phobos reader also reads in that section does not turn Kratos on:
Kratos readers parse with those readers, as Phobos' do, and a key two extensions read turns
neither on. Retail data holds no Kratos key, so a retail game plays as the original.

`RuleSet::kratos()` is the one answer sim and presentation read. A Kratos key that no ported
piece reads yet does not turn the switch on, so adding a key alone never changes a game.

## The Kratos setting

Kratos PP has no master switch: installing `Kratos.dll` plays it, its default values included.
VERA has no DLL, so the Options screen's Extensions page carries a launch setting,
`[VERA] Kratos=Auto|On|Off` in VERA.INI:

- **Auto**, the default, lets the game's data decide (the keys above).
- **On** plays Kratos in every game, so a mod that only relies on Kratos' default values, such
  as the prone `Z=20`, gets them without writing a key.
- **Off** plays none: a Kratos key turns nothing on, so a Kratos mod reads as the original game.

A mod can also declare `[Extensions] Kratos=yes` in its `mod.ini`. That makes this launch play
Kratos, as if it shipped the extension; with the player's setting Off VERA refuses the mod and
says how to turn Kratos back on. The setting is read at start, like the Ares one, and the rules
a game loads carry it, so a save, a replay and every peer reach the same answer.

## Prone firing offset

Kratos PP v0.2.4 changes the muzzle of infantry whose final rules leave `Fearless=no`.
Use these three keys in the ART section named by the infantry type's final `Image=`.
Each can enable Auto.

```ini
[GI]
PrimaryCrawlingFLH=20,0,20
ElitePrimaryCrawlingFLH=30,0,20
SecondaryCrawlingFLH=0,0,20
```

The three numbers are forward, lateral and vertical offsets in leptons. While standing,
the infantry uses its named primary or secondary fire coordinate. While prone, an
omitted coordinate defaults to the matching standing offset with height 20. Elite rank
selects the elite offsets even when the unit still uses its ordinary gun.

For the elite secondary weapon, v0.2.4 reads `EliteSecondaryFireFLH` for both stances.
Giving that key an explicit height also sets the prone height. If omitted, the prone
offset uses `SecondaryFireFLH` with height 20. The similarly named
`EliteSecondaryCrawlingFLH` has no effect in this release. Only weapon slots 0 and 1
use these rules. [Upstream ART reader](https://github.com/ra2diy/KratosPP/blob/v0.2.4/src/Ext/TechnoType/CrawlingFLHData.h#L19-L39)

VERA resolves the offset for each shooter when calculating its projectile, muzzle
animation and positioned report sound. Infantry sharing a type can stand and crawl
independently. [Upstream stance and rank selection](https://github.com/ra2diy/KratosPP/blob/v0.2.4/src/Ext/TechnoType/CrawlingFLH.cpp#L17-L90)

## Anti-air-only projectile

`AAOnly=yes` on a `[ProjectileType]` section (the bullet) refuses every shot at a target that
is not in the air:

```ini
[MissileAA]
AA=yes
AAOnly=yes
```

Kratos runs the check at its `TechnoClass_CanFire` hook (`0x6FC339`, whose refusal returns
`0x6FCB7E` = ILLEGAL): the shot stops when the target is not in the air, the firer's
`LandTargeting` is not `1` (`Land_Not_OK`, which the original refuses on land) and the
projectile has `AA=yes`. Without `AA=yes` the key does nothing, as in Kratos
([source](https://github.com/ra2diy/KratosPP/blob/v0.2.4/src/Ext/TechnoType/Status/CanFireCheck.cpp)).

VERA reads the key into `ProjectileType::aa_only` and answers it in `sim::combat::fire_error`,
beside the Phobos and Ares arms of the same hook. It is one flag on a type the original already
has, so it lives in that type's reader instead of a module of its own.

## Weapon money gate

`NoMoneyNoTalk` on a `[WeaponType]` keeps the shot from starting until the firing house holds
that much money; a negative value refuses while the house holds more than its magnitude:

```ini
[GoldShell]
NoMoneyNoTalk=1000
```

`DontNeedMoney=yes` on the same section skips the test. The value is read every time the shot
is asked for, so a house that earns the money can fire on the next attempt.

Kratos runs it in `TechnoStatus::CanFire_CheckWeapon` at its `TechnoClass::CanFire` hook
(`0x6FC339`, refusal `0x6FCB7E` = ILLEGAL), and its AE can supply the value instead
(`src/Ext/TechnoType/Status/CanFireCheck.cpp`). VERA reads the two weapon keys inside
`kratos_read` and answers them in `sim::combat::fire_error` beside the other arms of that
hook. The AttachEffect that supplies a value is not ported: only the weapon's own key counts.

## Frozen types

`Wreck=yes` on a `[TechnoType]` marks a type as an unusable wreck: it never moves, never looks
for a target and never spins its turret.

```ini
[HTNK_HULK]
Wreck=yes
```

In Kratos the key is one of three sources of a per-techno `Freezing` byte
(`src/Ext/TechnoType/Status/Freeze.cpp`), next to a live `Freeze` script and a
`VectorFreezeActive` latch. `Freezing` then clears the techno's targets and stops it every
update (`TechnoStatus::OnUpdate_Freeze`), answers yes to `TechnoClass::IsUnderEMP` (`0x70EFD0`),
and makes the engine's locomotion and facing hooks refuse to advance it (`0x4CCBB3`, `0x54AED0`,
`0x6622E0`, and the turret-spin arm at `0x736A95`).

VERA derives the status from the type in `rules::kratos_freeze`, which is exact for `Wreck`: a
wreck is frozen for as long as it exists, so no per-unit flag or save field is needed. The
per-unit sources will arrive with their own owners and add a runtime input there rather than a
second status. Three owners read it today:

- `world::object_turn` — the ground and air locomotor Process return before doing any work, so
  the object keeps its coordinate, heading and pending route;
- `world::techno_ai::target_scan` — the object neither scans for a target nor retaliates;
- `movement::turret::facing_update` — a `TurretSpins` type skips the spin arm, exactly as the
  `0x736A95` hook's jump to `0x736AD5` does, so the ordinary idle path still runs.

RESIDUAL: `TechnoClass::IsUnderEMP` is not gated, so code that asks that one predicate directly
(EMP sparkles, deploy, navcom) still sees the object as operable. The movement arms above cover
every place the original refuses to advance a frozen techno, which is what the key is for.

## Original project and authors

VERA20K implements the behavior in Rust. It does not load `Kratos.dll` and does not redistribute
Kratos binaries; each ported piece is translated from the Kratos source named beside it.

This product includes software developed by the Kratos Authors.

[Kratos PP](https://github.com/ChrisLv-CN/KratosPP) is the C++ port of **DPKratos**, created by
**双杀步枪 (ChrisLv_CN)** (developer), with **艾木魁 (M.Kenosis)** (design) and
**冲冲冲14782** (testing). It builds on **Xkein**'s
[DynamicPatcher](https://github.com/Xkein/YRDynamicPatcher), and credits the Ares and Phobos
developers. Kratos PP is licensed under the LGPLv3
([license](https://github.com/YuriPlanet/vera20k/blob/main/LICENSES/KratosPP.txt)), which
VERA20K's GPLv3 combined work keeps.

The features ported here originate with:

- Infantry firing offsets (`CrawlingFLH`) -
  **双杀步枪 (ChrisLv_CN)**, `src/Ext/TechnoType/CrawlingFLH.cpp` and `CrawlingFLHData.h`.
- Anti-air-only projectiles (`AAOnly`) -
  **双杀步枪 (ChrisLv_CN)**, `src/Ext/TechnoType/Status/CanFireCheck.cpp`.
- Weapon money gate (`NoMoneyNoTalk`) -
  **双杀步枪 (ChrisLv_CN)**, `src/Ext/TechnoType/Status/CanFireCheck.cpp`.
- Frozen types (`Wreck`) -
  **双杀步枪 (ChrisLv_CN)**, `src/Ext/TechnoType/Status/Freeze.cpp` and
  `src/Hooks/TechnoExtHook.cpp`.

## VERA20K porting credits

The Rust ports are individual VERA20K contributions; GitHub usernames name the merged pull
requests that carried each chain:

- **1372704087** — the Kratos switch, the prone firing offset (`CrawlingFLH`), the
  anti-air-only projectile (`AAOnly`), the weapon money gate (`NoMoneyNoTalk`) and frozen types
  (`Wreck`).

简体中文：[Kratos 扩展支持](zh-CN/kratos.md) · [扩展支持与原作者](zh-CN/credits.md).

## Adding a piece

A ported piece reads its keys inside `kratos_read`, which marks those reads as a Kratos
reader's; the switch then counts a key that no other reader found. The keys, their defaults
and their clamps come from Kratos PP and from `gamemd.exe` where Kratos hooks it, not from a
list; the address of the native site goes next to the Rust. A game without the key keeps the
native path.

### One piece, one module

Put a piece’s keys, defaults and decision in `src/rules/<ext>_<feature>.rs`, with
tests in `<ext>_<feature>_tests.rs`. Its reader follows the corresponding Kratos
`Read` method. The simulation uses the decision where it needs that value.

For example, `kratos_crawling_flh.rs` implements `Ext/TechnoType/CrawlingFLHData.h`.
Its reader fills `WeaponOffsets.crawling` and `WeaponOffsets.elite_crawling`.
`get_weapon_record` asks a borrowed view for `flh(slot, elite, prone)`. Keeping both
fields in their original order preserves the player-save layout, which records
field names and types as well as their order.

Type-level data belongs in the shared `RuleSet` so all units of a type use the
same offsets at 20,000 units.
