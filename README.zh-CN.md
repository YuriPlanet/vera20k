<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K 横幅" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <strong>简体中文</strong> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="最近一次手动运行的 macOS 库测试"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="macOS 库测试" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="最近一次手动运行的 Linux 库测试"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Linux 库测试" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="最近一次手动运行的 Windows 库测试"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows 库测试" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="加入 Discord" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ%E7%BE%A4-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ 群" height="23" align="middle">
</p>

# VERA20K

VERA20K 是一个由社区主导、对原版引擎 `gamemd.exe` 进行重写的项目。它使用原版游戏文件，因此你需要自行准备一份《红色警戒2：尤里的复仇》。

VERA20K 由玩家打造、为玩家而做，项目的方向由玩家说了算。

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K 遭遇战设置界面和游戏画面" width="100%">

## 项目目标

1. 保留原版《红色警戒2：尤里的复仇》的玩法、画面和氛围。
2. 支持更大规模的战斗：在 500×500 的地图上，容纳最多 **30 名玩家**和 **20,000 个单位**。
3. 加入 RTS 游戏中已有的功能，以及一些前所未见的功能。
4. 内置多人游戏客户端

## 当前进度

**积极开发中。**

- **遭遇战：** Windows、Linux 和 macOS 上均可游玩，支持原版地图和随机地图、
  基地建设、采矿、战斗以及存档和读档。
- **战役：** 任务已可启动，支持尚不完整。
- **AI：** 部分原版 AI 已能处理基地建设和脚本攻击。
- **持续完善：** 游戏行为与原版的一致性、画面和性能。
- **多人模式：** 当前源码构建已在 macOS 和 Linux 上支持局域网和在线对战。
- **规模：** 连续两场在线对战通过了功能验证，每场均有 30 个独立客户端进程和超过 20,000 个活跃的可移动单位。
  测试中的 Linux 客户端共用虚拟机；这不代表已实现流畅的实时性能。

## 下载与游玩

从[发布页面](https://github.com/YuriPlanet/vera20k/releases)下载适用于 Windows、macOS 或 Linux 的安装程序。它会找到你的《尤里的复仇》，并把 VERA20K 安装到一个新文件夹。

如需通过 CnCNet 客户端启动 YR 单人遭遇战或战役，请参阅[离线客户端设置](integrations/cncnet/README.md)。

## 参与贡献

欢迎帮忙。你可以编写代码、重构引擎、测试游戏、提出想法，或对照原版游玩，告诉我们哪些地方感觉不对。
直接提交 PR 即可，剩下的交给我们。

贡献者可以负责引擎的某些部分，并有很大的自由度去做推动 VERA20K 发展的改动。

维护者会协助审查、集成和合并。你的贡献会完整保留你的署名。

讨论在 [Discord](https://discord.gg/kmjRUn5m5F) 和 QQ 群（1097193563）进行。

## 设置

声音和操作设置请使用 **Options**，VERA 专属功能请使用 **Vera Options**。
玩家偏好、键盘快捷键和大厅选项保存在 `VERA.INI` 中：

- **Windows 安装版：** 与 `vera20k.exe` 位于同一目录。
- **macOS：** `~/Library/Application Support/VERA20k/VERA.INI`。
- **Linux：** `~/.local/share/VERA20k/VERA.INI`，或你的 XDG 数据目录。

`config.toml` 用于指定游戏目录，也可用来选择另一个设置目录。

在 **Vera Options** 中启用 **Mouse Wheel Zoom**，或在 `VERA.INI` 中设置 `[VERA] MouseWheelZoom=yes`（默认为 `no`）。

如果 `VERA.INI` 不存在，VERA 会创建该文件，并导入一次现有偏好。如果没有旧设置，
该文件初始为空，VERA 会使用内置默认值，直到设置被保存。
原有的 `RA2MD.INI` 和 `KeyboardMD.ini` 只用于读取并导入，绝不会被覆盖。

`VERARULES.INI` 位于同一文件夹，用于可选的游戏规则调整。初始内容是已注释的示例，任务和地图规则仍优先生效。
有关编辑和覆盖这些规则的方法，请参阅[个人规则](docs/personal-rules.md)。

## 架构

引擎分为三个主要部分：

- **模拟层：** 负责游戏状态和逻辑。
- **渲染层：** 为保留原版画面风格而构建的全新 GPU 渲染管线，使用原版游戏资源。
- **应用层：** 处理输入、菜单、音频和平台集成，并连接各个系统。

这种分离有助于保持游戏行为的确定性。

箭头表示数据流向：

![应用层向模拟层发送指令并接收世界状态，再向渲染层发送绘制数据。](docs/images/architecture.zh-CN.png)

## 编译与运行

你需要：

- **Rust 和 Git：** 通过 rustup 安装的 [Rust](https://rustup.rs/) 1.97 或更高版本，以及
  [Git](https://git-scm.com/)。
- **构建工具：** Windows 上需要 Visual Studio 的 C++ 生成工具，Rust 安装程序会提示安装；macOS 上运行
  `xcode-select --install`；Debian 和 Ubuntu 上运行 `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`。

VERA20K 已在 Windows、Linux 和 macOS 上运行游玩过。

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# 运行前，编辑 config.toml，将 ra2_dir 设为你的游戏文件夹：
cargo run --release --bin vera20k
```

## 我们如何开发

大部分代码由 AI 编程助手编写。它们用 Ghidra 研究原版引擎，再将其行为移植到 Rust，
通过[对比工具](tools/native_oracle.md)和实际游玩进行检查。AI 助手遵循 [AGENTS.md](AGENTS.md)。

<!-- Chinese-only section: intentionally absent from README.md and the other translations. -->
## 致中国玩家和开发者

VERA20K 的开发者都是你们的朋友。我们希望把庞大的中国红警玩家群体与世界各地的玩家联系在一起，让大家可以一起对战。

我们非常希望中国的开发者和玩家加入进来，和我们一起开发、一起游玩！
欢迎在 GitHub 上[提交 issue](https://github.com/YuriPlanet/vera20k/issues)，或者来 [Discord](https://discord.gg/kmjRUn5m5F) 或 QQ 群（群号 1097193563）找我们。
用中文写 issue 也完全没问题。

## 自定义

默认情况下，VERA20K 的玩法与原版游戏一致。每项新功能（包括 Ares 等模组的扩展）都必须默认关闭，
只能通过以下方式之一开启：

- **VERA 玩家选项**，位于 **Options → Vera Options**，保存在 `VERA.INI` 的 `[VERA]` 节下。
- **构建选项**，例如用 `--features ares` 编译受支持的 [Ares 扩展](docs/ares.md)。
- **INI 键**，由模组或地图设置。

声音、画面和操作设置保存在 `VERA.INI` 中。游戏规则由大厅设置、模组或地图决定。

## 支持项目

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="通过 PayPal 支持 VERA20K" height="23"></a>

所有收到的捐款都将根据
[TIBTRE 贡献账本](https://github.com/YuriPlanet/tibtre-ledger)分配给贡献者。

贡献者收款方式：

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## 致谢与法律声明

感谢 OpenRA、XCC Mixer、ModEnc wiki、Project Perfect Mod、EA 以 GPL 许可证发布的
《命令与征服》和《红色警戒》源码、World-Altering Editor、Final Alert、YRpp、Ares、Phobos，
以及其他为此做出贡献的项目和社区。

版权所有 © 2026 Halvor Nikolai Rusten 及 VERA20K 贡献者。本项目采用 [GPLv3](LICENSE-GPL) 许可证。本仓库不包含游戏文件。
Command & Conquer 和 Red Alert 是 Electronic Arts Inc. 的商标，截图和应用图标（《红色警戒2》的红星）中的游戏美术资源归 Electronic Arts 所有。
VERA20K 与 Electronic Arts 无隶属关系，也未获其认可。

## 文档

- [编译与运行](#编译与运行)：环境要求和从源码构建。
- [INI 配置方案](docs/ini-roots.md)：选择规则、美术和 AI 文件。
- [Ares](docs/ares.md)、[Phobos](docs/phobos.md) 和 [NPatch](docs/npatch.md)：扩展的支持情况与限制。
- [引擎概览](docs/index.md)：代码结构以及引擎各部分如何协同工作。
- [开发者工具](tools/README.md)：构建、测试和资源检查。
- [地图测试](tools/map_observation.md)：脚本化场景、截图和对局观察。
- [原版对比](tools/native_oracle.md)：将行为与原版游戏进行对比。
- [Ghidra 工作流程](docs/ghidra-workflow.md)：分析原版引擎。

本文件译自 [README.md](README.md)，请与英文版保持同步。
