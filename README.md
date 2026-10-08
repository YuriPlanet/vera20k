<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k hero image" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <strong>English</strong>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Latest macOS library test run (run manually)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="macOS library tests" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Latest Linux library test run (run manually)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Linux library tests" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Latest Windows library test run (run manually)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows library tests" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Join Discord" height="20" align="middle"></a>
</p>

<!-- Keep all translated READMEs in sync when changing the text below. -->

# VERA20k

VERA20k is a rewrite of the original engine, `gamemd.exe`. It uses the original game files,
so you'll need your own copy of Red Alert 2: Yuri's Revenge.

VERA20k is made by gamers, for gamers, and players have the final say in where it goes.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k skirmish setup screen and in-game view" width="100%">

## Project goals

1. Keep the gameplay, visuals and atmosphere of the original Red Alert 2: Yuri's Revenge.
2. Support bigger battles: up to **30 players** and **20,000 units** on larger maps.
3. Incorporate new RTS features.
4. Integrated multiplayer client

## Current status

**Early development.** Local skirmish is playable on Windows against a basic AI. Retail and
random maps, menus, base building, harvesting, combat and save/load are in place, but there's
still a lot to fix and finish.

Multiplayer, campaigns and the original AI are still missing. Aircraft, mind control, bridges
and several weapons and effects need more work. We haven't demonstrated 30-player,
20,000-unit battles yet.

## Build and run

You need Rust 1.88 or newer, a GPU with Vulkan, DirectX 12 or Metal, and the game installed.
VERA20k has been played on Windows, Linux and macOS.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Edit config.toml and set ra2_dir to your game folder before running:
cargo run --release --bin vera20k
```

Use `--release` to play; debug builds are too slow. See
[CONTRIBUTING.md](CONTRIBUTING.md#set-up) for platform setup and running the tests.

## How we work

Most of the code is written by AI coding agents that I direct. We use Ghidra to study the
original engine, then port its behavior to Rust and check it with
[comparison tools](tools/native_oracle.md) and playtesting. The working rules are in
[AGENTS.md](AGENTS.md).

## Contributing

Help is welcome. You can write code, test the game, improve the docs, or play it next to the
original and tell us what feels wrong. You don't need reverse-engineering experience to help.

Read [CONTRIBUTING.md](CONTRIBUTING.md), browse the
[good first issues](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue), or say
hi on [Discord](https://discord.gg/kmjRUn5m5F). The
[architecture overview](https://yuriplanet.github.io/vera20k/) explains how the engine fits together.

## Credits and legal

Thanks to OpenRA, XCC Mixer, the ModEnc wiki, Project Perfect Mod, EA's GPL source release of
Command & Conquer and Red Alert, World-Altering Editor, Final Alert, YRpp, Ares, Phobos and
many others.

Licensed under the [GPLv3](LICENSE-GPL). This repository contains no game files. Command &
Conquer and Red Alert are trademarks of Electronic Arts Inc., and the screenshots show game art
owned by Electronic Arts. VERA20k is not affiliated with or endorsed by Electronic Arts.
