# VeraX extension

VeraX is VERA20K's own first-party extension namespace, separate from the Ares, Phobos and
NPatch compatibility capabilities. Every build holds it; the game's own data decides whether it
plays.

## The switch

VeraX plays in a game whose Rules hold VeraX's own key:

```ini
[VeraX]
Enabled=1
```

The key is read from the game's Rules layers (RULESMD.INI, LANGRULE.INI, the selected mode INI
and a map's own rules pass) by `rules::verax_switch`, and the answer is held on the RuleSet
(`RuleSet::verax`). The value is read by the game's own boolean reader
(`CCINIClass::ReadBool` 0x005295F0): `1`, `T` or `Y` turns VeraX on, `0`, `F` or `N` turns it
off, and an absent key leaves it off. Retail data holds no such key, so a retail game plays as
gamemd, and a mod or map that ships the key plays VeraX in every build.

The switch currently turns on Skirmish map hot reload only.
The Ctrl+D smart command (formerly AutoLoad) is a first-party convenience
feature in every build. The translucent-fog and high-terrain building lift
options and the foot-path overlay in "Origins and related first-party work"
are adjacent VERA work, not VeraX features.

## AutoLoad (moved out of VeraX)

Ctrl+D began as VeraX AutoLoad: it issued ordinary Enter orders that balanced
selected infantry and vehicles into nearby transports. It has since been
promoted to a default-build, single-player-only convenience hotkey: it picks
one of nine context actions from the whole selection (transport load,
garrison, engineer repair/capture, C4 sabotage, spy infiltration, Ivan bombs,
Tesla-coil charge, tank bunker entry and bio-reactor feeding), each emitting
ordinary commands and gated by the engine's own read-only admission. The mode
is chosen from the most specific capability shared by every selected unit;
mixed groups do nothing except transport loading. `HotkeyCommand::AutoLoad`
and the `AutoLoad=` profile binding are retained for save/replay compatibility,
but the behaviour is no longer gated by the VeraX switch. The implementation is
`src/app/input/smart_command.rs`.

## Map hot reload

Without this extension the Skirmish map list is scanned once at startup, so a map copied into the
game directory is only visible after restarting. A game that turns VeraX on rescans the directory
each time the Skirmish Choose Map dialog opens (`SessionClass::ScanMultiplayerMapFiles` @
`0x00699980` again), so the new loose `.pkt`, `.yrm` or `.yro` map and its rows appear without a
restart. Rows already listed are not duplicated, and the current selection keeps following the same
map file when the rescan moves its index.

This is not a directory watcher: the list changes on the next open of the chooser. Network lobbies
keep their launch snapshot, because the map list is local and every peer must still compare the same
catalog.

The chooser asks the game's startup Rules (RULESMD.INI, LANGRULE.INI and the selected mode INI),
because no map is chosen yet: a mod that ships `[VeraX] Enabled=1` turns the reload on, and a
map's own key is read with the rest of that map's Rules once the map is loaded.

## Origins and related first-party work

VeraX does not bundle third-party binaries or source. Its ideas and the
related presentation work came from:

- **AutoLoad** follows the vehicle-loading idea shown in a video by Bilibili
  user [335956233](https://space.bilibili.com/335956233). VERA implements its
  own allocation, range and command policy; the creator's binary and source are
  not used and are not needed to reproduce the observed behavior.
- **Translucent fog** is VERA's own idea, not copied from an extension. It is a
  client graphics option (`[graphics] translucent_fog=true`, off by default for
  the retail look) that keeps terrain visible through fog while concealing
  unseen enemy objects, enemy projectiles and walls. It is not gated by the
  VeraX switch.
- **High-terrain building lift** is VERA's own client graphics option
  (`[graphics] high_terrain_building_lift=true`, off by default to match the
  original). From terrain level 15 up, the original game draws a building 1-2 px
  below its tile row, and the tiles hide that sliver of its base. The option draws
  the building at its tile row instead. It changes only the picture.
- **Foot path steps** is a diagnostic overlay rather than an extension feature.
  In `dev-overlays` builds, `Ctrl+Shift+O` draws the remaining path cells and
  step labels for selected moving units (`src/app/diagnostics/debug_overlays.rs`).
  Release builds (`--no-default-features`) do not include it. Its presentation
  follows a video by Bilibili user [1995852436](https://space.bilibili.com/1995852436);
  no third-party binary or source is used.

## Namespace

`[VeraX] Enabled=` is VeraX's own key, read from the game's Rules. VeraX-only behaviour reads it
through `RuleSet::verax` and does nothing when it is off; there is no build flag and no build
constant, so every build holds the same code and the same game plays the same everywhere.
AutoLoad's distribution and range are VeraX input
policy; its ordinary Enter orders and boarding admission reuse the native-derived engine owners
(`FootClass::ClickedAction` at `0x004D74E0`, `UnitClass::Receive_Radio` at `0x00737430`).
VeraX is orthogonal to the other extensions: it combines with Ares, which every build
holds ([Ares](ares.md)), and with the `phobos` and `npatch` build options (`--features
phobos,npatch`). The compatibility rule and the pattern for two
extensions touching one native site are in
[NPatch compatibility](npatch.md#cross-extension-compatibility).

简体中文：[扩展支持与原作者](zh-CN/credits.md)。
