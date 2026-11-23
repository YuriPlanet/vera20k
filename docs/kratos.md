# Kratos compatibility

VERA20K is starting a Rust-native port of [Kratos PP](https://github.com/ChrisLv-CN/KratosPP),
the C++ extension for Yuri's Revenge. Like Ares it is in every build, the release build too,
and a game's own data switches it on; there is no build feature and no launch setting.

## The Kratos switch

Kratos plays in a game whose rules, art, mode or map hold a key that only a Kratos reader reads
(`src/rules/kratos_switch.rs`). ART is read while the processing passes run, so those keys
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

`InfantryClass::Crawling` (`+0x6DB`) changes where a man's shot starts. Kratos
(`src/Ext/TechnoType/CrawlingFLH.cpp`) keeps four ART keys on the type's Image:

```ini
[GI]
PrimaryCrawlingFLH=20,0,20
ElitePrimaryCrawlingFLH=20,0,20
SecondaryCrawlingFLH=0,0,20
EliteSecondaryCrawlingFLH=0,0,20
```

- A crawling man fires weapon slot 0 or 1 from the matching crawling coordinate instead of
  `PrimaryFireFLH`/`SecondaryFireFLH` (the elite pair at elite rank). Standing, he keeps the
  stock coordinate.
- Each crawling coordinate defaults to that slot's fire coordinate with `Z` 20
  (`CrawlingFLHData::Read`), so a type that sets none still moves the prone muzzle down.
- `Fearless` infantry are skipped: `CrawlingFLH::Setup` disables itself for them.
- Only the original two slots move; Kratos patches `GetWeapon(0)` and `GetWeapon(1)`.

VERA reads the keys in `rules::weapon_offsets` and applies them in `sim::combat::combat_weapon`
(`get_weapon_record`), so the projectile's start, its muzzle animation and its report sound all
use the same coordinate. A game Kratos does not play keeps gamemd's coordinate.

## Original project and authors

VERA20K implements the behavior in Rust. It does not load `Kratos.dll` and does not redistribute
Kratos binaries; each ported piece is translated from the Kratos source named beside it.

This product includes software developed by the Kratos PP author.

[Kratos PP](https://github.com/ChrisLv-CN/KratosPP) is the C++ port of **DPKratos**, created by
**双杀步枪 (ChrisLv_CN)** (developer), with **艾木魁 (M.Kenosis)** (design) and
**冲冲冲14782** (testing). It builds on **Xkein**'s
[DynamicPatcher](https://github.com/Xkein/YRDynamicPatcher), and credits the Ares and Phobos
developers. Kratos PP is licensed under the LGPLv3
([license](https://github.com/YuriPlanet/vera20k/blob/main/LICENSES/KratosPP.txt)), which
VERA20K's GPLv3 combined work keeps.

The features ported here originate with:

- Prone firing offset (`PrimaryCrawlingFLH` and the other three keys) -
  **双杀步枪 (ChrisLv_CN)**, `src/Ext/TechnoType/CrawlingFLH.cpp` and `CrawlingFLHData.h`.

## VERA20K porting credits

The Rust ports are individual VERA20K contributions; GitHub usernames name the merged pull
requests that carried each chain:

- **1372704087** — the Kratos switch and the prone firing offset (`CrawlingFLH`).

简体中文：[Kratos 扩展支持](zh-CN/kratos.md) · [扩展支持与原作者](zh-CN/credits.md).

## Adding a piece

A ported piece reads its keys inside `kratos_read`, which marks those reads as a Kratos
reader's; the switch then counts a key that no other reader found. The keys, their defaults
and their clamps come from Kratos PP and from `gamemd.exe` where Kratos hooks it, not from a
list; the address of the native site goes next to the Rust. A game without the key keeps the
native path.
