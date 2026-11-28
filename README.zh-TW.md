<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K 橫幅" width="100%">

<p align="center" dir="ltr">
  <a href="README.vi.md" lang="vi">Tiếng Việt</a> · <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <strong>繁體中文</strong> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="最近一次手動執行的 Linux 庫測試"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Linux 庫測試" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="最近一次手動執行的 Windows 庫測試"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Windows 庫測試" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://raster.shields.io/badge/dynamic/xml.png?url=https://badgen.net/discord/online-members/kmjRUn5m5F&amp;query=substring-after%28/%2A/%40aria-label%2C%20%27%3A%20%27%29&amp;label=VERA20K%20Discord&amp;style=flat&amp;color=5865F2&amp;logo=discord&amp;logoColor=white&amp;cacheSeconds=3600" alt="加入 Discord" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/VERA20K_QQ-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ 群 (VERA20K)" height="23" align="middle">
  <img src="https://img.shields.io/badge/RA20K.com_QQ-319832195-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ 群 (RA20K.com)" height="23" align="middle">
</p>

# VERA20K

VERA20K 是《紅色警戒2：尤里的復仇》的全新引擎。玩起來和原版一樣，單位、地圖和畫面都相同，可在 Windows、macOS 和 Linux 上執行。它支援更大規模的戰鬥，一張地圖最多可容納 20,000 個單位。離線遭遇戰最多有 120 個玩家席位，區域網路與線上對戰最多有 30 個。

VERA20K 由玩家打造、為玩家而做，專案的方向由玩家說了算。

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K 遭遇戰設定介面和遊戲畫面" width="100%">

## 下載與遊玩

1. 從 Steam 或 EA app 安裝《紅色警戒2：尤里的復仇》。
2. 從[釋出頁面](https://github.com/YuriPlanet/vera20k/releases)下載適合你系統的 VERA20K。
3. 開啟下載的檔案。它會找到你的遊戲，並把 VERA20K 安裝到單獨的資料夾。

遇到問題？請看[幫助頁面](https://vera20k.org/zh/help)。

## 專案目標

1. 保留原版《紅色警戒2：尤里的復仇》的玩法、畫面和氛圍。
2. 支援在 500×500 的地圖上進行更大規模的戰鬥，最多容納 **20,000 個單位**。離線遭遇戰最多有 **120 個玩家席位**，區域網路與線上對戰最多有 **30 個**。
3. 加入 RTS 遊戲中已有的功能，以及一些前所未見的功能。
4. 內建多人遊戲客戶端

## 參與貢獻

**我們重視的 PR** — 程式碼重構、效能改善、尋路、UI/UX、隨機地圖產生器。

貢獻者可以負責引擎的某些部分，並有很大的自由度去做推動 VERA20K 發展的改動。

維護者會協助審查、整合和合併。你的貢獻會完整保留你的署名。

管理員：[Halvor Nikolai Rusten](https://github.com/hrusten)、[风雨声](https://github.com/996666925)、[Kironix](https://github.com/EncoreAlpha)、[Randerion(HaoJun0823)](https://github.com/HaoJun0823)、[libi](https://github.com/libi)、[zzzzzzh123](https://github.com/zzzzzzh123)、[八音盒](https://github.com/1372704087)

討論在 [Discord](https://discord.gg/kmjRUn5m5F) 和 QQ 群（1097193563）進行。

## 架構

引擎分為三個主要部分：

- **模擬層：** 負責遊戲狀態和邏輯。
- **渲染層：** 使用原版遊戲的美術素材，在 GPU 上繪製地形、單位和建築。
- **應用層：** 處理輸入、選單、音訊和平臺整合，並連線各個系統。

這種分離有助於保持遊戲行為的確定性。

箭頭表示資料流向：

![應用層向模擬層傳送指令並接收世界狀態，再向渲染層傳送繪製資料。](docs/images/architecture.zh-CN.png)

## 編譯與執行

你需要：

- **Rust 和 Git：** 通過 rustup 安裝的 [Rust](https://rustup.rs/) 1.97 或更高版本，以及
  [Git](https://git-scm.com/)。
- **構建工具：** Windows 上需要 Visual Studio 的 C++ 生成工具，Rust 安裝程式會提示安裝；macOS 上執行
  `xcode-select --install`；Debian 和 Ubuntu 上執行 `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`。

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# 執行前，編輯 config.toml，將 ra2_dir 設為你的遊戲資料夾：
cargo run --release --bin vera20k
```

## 我們如何開發

大部分程式碼由 AI 程式設計助手編寫。它們用 Ghidra 研究原版引擎，再將其行為移植到 Rust，
通過[對比工具](tools/native_oracle.md)和實際遊玩進行檢查。AI 助手遵循 [AGENTS.md](AGENTS.md)。

<!-- Chinese-only section: intentionally absent from README.md and the other translations. -->
## 致中國玩家和開發者

VERA20K 的開發者都是你們的朋友。我們希望把龐大的中國紅警玩家群體與世界各地的玩家聯絡在一起，讓大家可以一起對戰。

我們非常希望中國的開發者和玩家加入進來，和我們一起開發、一起遊玩！
歡迎在 GitHub 上[提交 issue](https://github.com/YuriPlanet/vera20k/issues)，或者來 [Discord](https://discord.gg/kmjRUn5m5F) 或 QQ 群（群號 1097193563）找我們。
用中文寫 issue 也完全沒問題。

## 自定義

你可以加入自己的模組、地圖、音樂等內容。這些檔案放在 VERA20K 資料夾中：Windows 上是安裝資料夾，macOS 上是 `~/Library/Application Support/VERA20k`，Linux 上是 `~/.local/share/VERA20k`。

VERA20K 預設使用《尤里的復仇》原版規則。你可以在 **Options** 選單中調整聲音、畫面和操作設定，也可以開啟 VERA 專屬功能。遊戲會將設定儲存在 `VERA.INI` 中，其中 VERA 專屬選項位於 `[VERA]` 節。

- **模組：** 在 **Options → Mods** 中，**Add Mod** 可以安裝你下載的模組，**New Mod** 可以開始製作你自己的模組。每個模組在 `Mods` 中都有自己的資料夾。
- **地圖：** 把 `.map`、`.mpr` 或 `.yrm` 檔案放進 `Maps`，即可在遭遇戰中遊玩。線上下載的地圖也會儲存在這裡。
- **音樂：** 把 WAV 或 AUD 檔案放進 `musics`，並在其中加入 `music.ini`，在 `[Music]` 下寫入 `enable=yes`。你的音樂會在對局中與遊戲音樂一起播放；設定 `ExclusiveMode=yes` 則只播放你的音樂。
- **高畫質美術：** [高畫質美術包](docs/hd-art.md)的安裝和選擇方式與模組相同。之後在 **Options** 中開啟 **HD art**。
- **語言：** `vera20k.<language>.ini` 檔案可以加入語言或修改遊戲文字。參見[翻譯指南](src/i18n/README.md)。

大廳設定、模組和地圖都能改變對局規則。模組和地圖可以通過 INI 檔案啟用 VERA20K 支援的 [Ares](docs/ares.md)、[Phobos](docs/phobos.md) 和 [Kratos](docs/zh-CN/kratos.md) 功能。模組也可以通過附帶 `ares.mix` 來啟用 Ares。部分 Phobos 功能仍需在編譯時加上 `--features phobos`。

想製作自己的模組，可以從[模組製作指南](docs/modding.md)開始。

## 支援專案

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="通過 PayPal 支援 VERA20K" height="23"></a>

所有收到的捐款都將根據
[TIBTRE 貢獻帳本](https://github.com/YuriPlanet/tibtre-ledger)分配給貢獻者。

貢獻者收款方式：

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## 媒體

<p>
  <a href="https://www.youtube.com/@cncvietnam-huydeptrai"><img src="docs/images/cnc-vietnam.webp" width="64" height="64" align="left" alt="Command &amp; Conquer Vietnam (YouTube)"></a>
  <a href="https://www.youtube.com/@cncvietnam-huydeptrai">Command &amp; Conquer Vietnam</a> 頻道介紹了 VERA20K。<br>
  <a href="https://www.youtube.com/watch?v=njpQRCTZ_MY">Arctic Circle 30：1 對 29 個冷酷 AI</a><br>
  <a href="https://www.youtube.com/watch?v=7r_z_7w0InA&amp;t=1440s">Survival Fortress 5.0</a><br clear="all">
</p>

<p>
  <a href="https://www.youtube.com/@haojunyang2337"><img src="docs/images/haojun0823.webp" width="64" height="64" align="left" alt="HaoJun Yang"></a>
  <a href="https://www.youtube.com/@haojunyang2337">HaoJun Yang</a> 頻道介紹了 VERA20K。<br>
  <a href="https://www.youtube.com/watch?v=ucz-EiMeW88">在 VERA20K 0.41 中玩《紅色警戒 2》</a><br clear="all">
</p>

## 地圖編輯器

<p>
  <a href="https://github.com/996666925/ra2-map-editor"><img src="https://avatars.githubusercontent.com/u/52450772?s=128" width="64" height="64" align="left" alt="996666925"></a>
  <a href="https://github.com/996666925/ra2-map-editor">YR Map Editor</a><br>
  製作《尤里的復仇》地圖，並在 VERA20K 中試玩。<br clear="all">
</p>

## 致謝與法律宣告

感謝 OpenRA、XCC Mixer、ModEnc wiki、Project Perfect Mod、EA 以 GPL 許可證釋出的
《命令與征服》和《紅色警戒》原始碼、World-Altering Editor、Final Alert、YRpp、Ares、Phobos，
以及其他為此做出貢獻的專案和社群。

版權所有 © 2026 Halvor Nikolai Rusten 及 VERA20K 貢獻者。本專案採用 [GPLv3](LICENSE-GPL) 許可證，並附有[附加條款](LEGAL.md#additional-terms)。
玩家需自行提供《紅色警戒2：尤里的復仇》的遊戲檔案。
Command & Conquer 和 Red Alert 是 Electronic Arts Inc. 的商標，截圖和應用圖示（《紅色警戒2》的紅星）中的遊戲美術資源歸 Electronic Arts 所有。
VERA20K 與 Electronic Arts 無隸屬關係，也未獲其認可。

This product includes software developed by the [Ares Contributors](LICENSES/Ares.txt).

This product includes software developed by the [Phobos Developers](docs/phobos.md#original-project-and-authors).

**在分享、出售或復刻（fork）VERA20K 之前，請先閱讀[法律宣告](LEGAL.md)。**

## 文件

- [WebAssembly 預覽版](docs/wasm.zh-CN.md)：在瀏覽器中執行的可選單人版本。
- [模組製作指南](docs/modding.md)：模組能做什麼、不能做什麼，以及 VERA20K 與原版遊戲的區別。
- [INI 配置方案](docs/ini-roots.md)：選擇規則、美術和 AI 檔案。
- [Ares](docs/ares.md)、[Phobos](docs/phobos.md) 和 [NPatch](docs/npatch.md)：擴充套件的支援情況與限制。
- [引擎概覽](docs/index.md)：程式碼結構以及引擎各部分如何協同工作。
- [開發者工具](tools/README.md)：構建、測試和資源檢查。
- [地圖測試](tools/map_observation.md)：指令碼化場景、截圖和對局觀察。
- [原版對比](tools/native_oracle.md)：將行為與原版遊戲進行對比。
- [Ghidra 工作流程](docs/ghidra-workflow.md)：分析原版引擎。

本檔案譯自 [README.md](README.md)，請與英文版保持同步。
