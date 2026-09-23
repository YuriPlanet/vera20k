<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k hero image" width="100%">

[![VERA20k Discord](https://img.shields.io/badge/VERA20k%20Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/kmjRUn5m5F)
[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue?style=for-the-badge)](LICENSE-GPL)
[![Platforms: Windows, Linux, macOS](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20Linux%20%C2%B7%20macOS-blue?style=for-the-badge)](https://github.com/YuriPlanet/vera20k/actions)

# VERA20k

**Red Alert 2: Yuri's Revenge, rebuilt in Rust.** Faithful to the original game, with a goal
of 30 players and 20,000 units.

VERA20k is an open-source reimplementation of the Yuri's Revenge engine (`gamemd.exe`). It
plays from your own copy of the game and ports the original's behavior one mechanism at a
time. Unlike [OpenRA](https://www.openra.net), which recreates the classic games on its own
engine, VERA20k aims to reproduce Yuri's Revenge itself.

> **You need the original game.** VERA20k contains no game files. EA sells Red Alert 2 and
> Yuri's Revenge in *Command & Conquer The Ultimate Collection*, on
> [Steam](https://store.steampowered.com/bundle/39394/) and from
> [EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc).

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k skirmish setup screen and in-game view" width="100%">

## Status

**Pre-alpha** (September 2026). A local skirmish on retail maps is playable on Windows against
a placeholder AI.

| Works | Partial or in progress | Not yet |
|---|---|---|
| Retail and random maps, original menus and sidebar | Aircraft attack runs and Carrier Hornets | Multiplayer (lockstep exists, no network) |
| Building, power, tech tree, selling and repair | Mind control | The original AI (a placeholder for now) |
| War, Chrono and Slave Miners | Crate pickup | Campaign and most map triggers |
| Infantry, vehicle, naval and base-defense combat | Death effects (ship sinking and a few others missing) | Chrono Legionnaire, Crazy Ivan, Magnetron and other special weapons |
| Attack dogs and Terror Drones | | Gattling spin-up, Prism chaining, Tesla charging |
| Garrisons, transports, engineers and cloaking | | Nuke, Chronosphere, Psychic Dominator, Spy Plane |
| Lightning Storm, Iron Curtain and other support powers | | Movies and credits |
| Save and load | | 30 players / 20,000 units (not demonstrated yet) |

## Quick start

You need Rust 1.88 or newer, a GPU with Vulkan, DirectX 12 or Metal, and the game installed.
It has been played on Windows, Linux and macOS, and CI builds and tests all three.

[![Windows](https://img.shields.io/github/actions/workflow/status/YuriPlanet/vera20k/windows.yml?branch=main&label=Windows&style=flat-square)](https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml)
[![Linux](https://img.shields.io/github/actions/workflow/status/YuriPlanet/vera20k/linux.yml?branch=main&label=Linux&style=flat-square)](https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml)
[![macOS](https://img.shields.io/github/actions/workflow/status/YuriPlanet/vera20k/macos.yml?branch=main&label=macOS&style=flat-square)](https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml)
[![Linux ARM](https://img.shields.io/github/actions/workflow/status/YuriPlanet/vera20k/linux-arm.yml?branch=main&label=Linux%20ARM&style=flat-square)](https://github.com/YuriPlanet/vera20k/actions/workflows/linux-arm.yml)
[![Windows ARM](https://img.shields.io/github/actions/workflow/status/YuriPlanet/vera20k/windows-arm.yml?branch=main&label=Windows%20ARM&style=flat-square)](https://github.com/YuriPlanet/vera20k/actions/workflows/windows-arm.yml)
[![Clippy](https://img.shields.io/github/actions/workflow/status/YuriPlanet/vera20k/rust.yml?branch=main&label=Clippy&style=flat-square)](https://github.com/YuriPlanet/vera20k/actions/workflows/rust.yml)

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml   # then set ra2_dir to your game folder
cargo run --release --bin vera20k    # always --release: debug builds are too slow to play
```

`cargo test -p vera20k --lib` runs the tests, no game needed. More setup details are in
[CONTRIBUTING.md](CONTRIBUTING.md#set-up).

## How it's built

The original `gamemd.exe` is the reference. Newer gameplay code cites the original function it
was ported from, and [native harnesses](tools/native_oracle.md) run the original code to check
the Rust results. Most code is written by AI coding assistants that the maintainer directs,
following the rules in [AGENTS.md](AGENTS.md).

## Contributing

Help is welcome, and you don't need reverse-engineering experience. Compare VERA20k with the
original and report differences, try it on Linux or macOS, or pick a
[good first issue](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue). Start
with [CONTRIBUTING.md](CONTRIBUTING.md), or say hi on [Discord](https://discord.gg/kmjRUn5m5F).

**Learn more:** [architecture overview](https://yuriplanet.github.io/vera20k/) ·
[native oracle](tools/native_oracle.md) · [research notes](docs/research/README.md)

## Credits and legal

Thanks to OpenRA, XCC Mixer, the ModEnc wiki, Project Perfect Mod, EA's GPL source release of
Command & Conquer and Red Alert, World-Altering Editor, Final Alert, YRpp, Ares, Phobos and
many others.

Licensed under the [GPLv3](LICENSE-GPL). This repository contains no game files. Command &
Conquer and Red Alert are trademarks of Electronic Arts Inc., and screenshots show game art
owned by Electronic Arts. VERA20k is not affiliated with or endorsed by Electronic Arts.
