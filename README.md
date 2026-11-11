<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K hero image" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <strong>English</strong>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Latest macOS library test run (run manually)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="macOS library tests" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Latest Linux library test run (run manually)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Linux library tests" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Latest Windows library test run (run manually)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows library tests" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/wasm.yml?query=branch%3Amain" title="Latest WebAssembly build run (run manually)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/wasm.yml/badge.svg?branch=main" alt="WebAssembly build" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Join Discord" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ group" height="23" align="middle">
</p>

<!-- Keep all translated READMEs in sync when changing the text below. -->

# VERA20K

VERA20K is a community-led rewrite of the original engine, `gamemd.exe`. It uses the original
game files, so you'll need your own copy of Red Alert 2: Yuri's Revenge.

VERA20K is made by gamers, for gamers, and gamers have the final say in where it goes.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K skirmish setup screen and in-game view" width="100%">

## Project goals

1. Keep the gameplay, visuals and atmosphere of the original Red Alert 2: Yuri's Revenge.
2. Support bigger battles: up to **30 players** and **20,000 units** on 500×500 maps.
3. Incorporate known RTS features and some never seen before.
4. Integrated multiplayer client

See the [roadmap](https://github.com/orgs/YuriPlanet/projects/1) for current priorities
and future plans.

## Download and play

Download the installer for Windows, macOS or Linux from the
[releases page](https://github.com/YuriPlanet/vera20k/releases). It finds your copy of
Yuri's Revenge and sets up VERA20K in a new folder.

To launch single-player YR skirmishes or campaigns from a CnCNet client, see the
[offline client setup](integrations/cncnet/README.md).

## Contributing

**Don't be afraid to send a PR.** It doesn't have to be perfect; we'll help with the rest.

Help is welcome anywhere, and these areas need it most. Each links to its roadmap issue, with
the work in progress and the next step:

- **[Gameplay and combat](https://github.com/YuriPlanet/vera20k/issues/3830):** unit behavior, damage and everyday skirmish reliability.
- **[Performance](https://github.com/YuriPlanet/vera20k/issues/3831):** frame rate and responsiveness in large battles and on older hardware.
- **[Multiplayer](https://github.com/YuriPlanet/vera20k/issues/3832):** joining, starting and keeping online matches going, and co-op reliability.
- **[UI and audio](https://github.com/YuriPlanet/vera20k/issues/3833):** menus, text, [translations](src/i18n/README.md) and sound.
- **[Campaigns](https://github.com/YuriPlanet/vera20k/issues/3834):** missions launch, but playing them to the end still needs work: objectives, triggers, scripted transitions, save/load and moving on to the next mission.
- **[Mod compatibility](https://github.com/YuriPlanet/vera20k/issues/3835):** Ares and Phobos features, without changing the original game's defaults.
- **[Testing](https://github.com/YuriPlanet/vera20k/issues?q=is%3Aopen%20label%3Aparity):** play VERA20K next to the original and report what plays differently.

Contributors can take responsibility for engine areas, with broad freedom to make changes that
move VERA20K forward.

Maintainers will help with review, integration and merging. You'll keep full credit for your
contribution.

Discussion is on [Discord](https://discord.gg/kmjRUn5m5F) and in the QQ group (1097193563).

## Architecture

The engine has three main parts:

- **Simulation:** Owns gameplay state and logic.
- **Rendering:** An absolutely new GPU rendering pipeline using the original game assets, built
  to preserve the original look.
- **Application:** Handles input, menus, audio, and platform integration, connecting the systems.

This separation supports deterministic gameplay.

Arrows show data flow:

![Application sends commands to simulation, which returns world state; application sends draw data to rendering.](docs/images/architecture.png)

## Build and run

You need:

- **Rust and Git:** [Rust](https://rustup.rs/) 1.97 or later, installed with rustup, and
  [Git](https://git-scm.com/).
- **Build tools:** on Windows, the Visual Studio C++ build tools, which the Rust installer offers
  to install; on macOS, `xcode-select --install`; on Debian and Ubuntu,
  `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Edit config.toml and set ra2_dir to your game folder before running:
cargo run --release --bin vera20k
```

## How we work

Most of the code is written by AI coding agents. They use Ghidra to study the original
engine, then port its behavior to Rust and check it with [comparison tools](tools/native_oracle.md)
and playtesting. Agents follow [AGENTS.md](AGENTS.md).

## Customization

VERA20K plays like the original game by default. Map capacity is selected automatically,
without a build flag: `Size` width plus height can be up to 1024. Other new features,
including extensions from mods such as Ares, stay off until they are turned on with one of:

- **A VERA player option**, in **Options**, saved under `[VERA]` in `VERA.INI`.
- **A build option**, such as `--features ares` for the supported [Ares extensions](docs/ares.md).
- **An INI key**, set by a mod or a map.

Sound, graphics and controls are saved in `VERA.INI`. Gameplay rules come from lobby
settings, mods or maps.

The [modding guide](docs/modding.md) explains how to make your own mods.

## Support

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="Support VERA20K via PayPal" height="23"></a>

All donations received are distributed
to contributors according to the [TIBTRE contribution ledger](https://github.com/YuriPlanet/tibtre-ledger).

Payouts to contributors:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## Credits and legal

Thanks to OpenRA, XCC Mixer, the ModEnc wiki, Project Perfect Mod, EA's GPL source release of
Command & Conquer and Red Alert, World-Altering Editor, Final Alert, YRpp, Ares, Phobos and
many others.

Copyright © 2026 Halvor Nikolai Rusten and VERA20K contributors.
Licensed under the [GPLv3](LICENSE-GPL) with [additional terms](LEGAL.md#additional-terms).
This repository contains no game files. Command &
Conquer and Red Alert are trademarks of Electronic Arts Inc., and the screenshots and the app
icon, Red Alert 2's red star, show game art owned by Electronic Arts. VERA20K is not affiliated
with or endorsed by Electronic Arts.

This product includes software developed by the [Ares Contributors](LICENSES/Ares.txt).

**Read the [legal notice](LEGAL.md) before you share, sell or fork VERA20K.**

## Documentation

- [WebAssembly preview](docs/wasm.md): an opt-in single-player build that runs in a browser.
- [Modding guide](docs/modding.md): what mods can and can't do, and how VERA20K differs from the original game.
- [INI profiles](docs/ini-roots.md): choosing rules, art and AI files.
- [Ares](docs/ares.md), [Phobos](docs/phobos.md), [NPatch](docs/npatch.md) and [VeraX](docs/verax.md): extension support and limitations.
- [Engine overview](docs/index.md): code layout and how the engine fits together.
- [Developer tools](tools/README.md): builds, tests and asset inspection.
- [Map testing](tools/map_observation.md): scripted scenarios, screenshots and match observations.
- [Native comparisons](tools/native_oracle.md): comparing behavior with the original game.
- [Ghidra workflow](docs/ghidra-workflow.md): inspecting the original engine.
