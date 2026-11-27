# VeraX extension

VeraX is VERA20K's own first-party extension namespace, separate from the Ares, Phobos and
NPatch extensions. Build it explicitly:

```sh
cargo run --release --bin vera20k --features verax
```

The `verax` feature currently compiles Skirmish map hot reload only.
The Ctrl+D smart command (formerly AutoLoad) is now a first-party convenience
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
but the behaviour is no longer gated by `verax`. The implementation is
`src/app/input/smart_command.rs`.

## Map hot reload

Without this extension the Skirmish map list is scanned once at startup, so a map copied into the
game directory is only visible after restarting. A VeraX build rescans the directory each time the
Skirmish Choose Map dialog opens (`SessionClass::ScanMultiplayerMapFiles` @ `0x00699980` again), so
the new loose `.pkt`, `.yrm` or `.yro` map and its rows appear without a restart. Rows already
listed are not duplicated, and the current selection keeps following the same map file when the
rescan moves its index.

This is not a directory watcher: the list changes on the next open of the chooser. Network lobbies
keep their launch snapshot, because the map list is local and every peer must still compare the same
catalog.

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
  `verax` feature.
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

`--features verax` compiles `rules::VERAX_ENABLED`. VeraX-only readers take their keys and
commands under `#[cfg(feature = "verax")]`. AutoLoad's distribution and range are VeraX input
policy; its ordinary Enter orders and boarding admission reuse the native-derived engine owners
(`FootClass::ClickedAction` at `0x004D74E0`, `UnitClass::Receive_Radio` at `0x00737430`).
VeraX is orthogonal to the other extensions: it combines with Ares and NPatch, which every build
holds and whose game data switches them on ([Ares](ares.md), [NPatch](npatch.md)), and with the
`phobos` build feature (`--features phobos,verax`). The compatibility rule and the pattern for two
extensions touching one native site are in
[NPatch compatibility](npatch.md#cross-extension-compatibility).

简体中文：[扩展支持与原作者](zh-CN/credits.md)。
