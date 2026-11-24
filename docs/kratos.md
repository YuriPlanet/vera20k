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

- Infantry firing offsets (`CrawlingFLH`) -
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
