<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K Titelbild" width="100%">

<p align="center" dir="ltr">
  <a href="README.vi.md" lang="vi">Tiếng Việt</a> · <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.zh-TW.md" lang="zh-TW">繁體中文</a> · <strong>Deutsch</strong> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Linux (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Bibliothekstests unter Linux" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Windows (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Bibliothekstests unter Windows" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://raster.shields.io/badge/dynamic/xml.png?url=https://badgen.net/discord/online-members/kmjRUn5m5F&amp;query=substring-after%28/%2A/%40aria-label%2C%20%27%3A%20%27%29&amp;label=VERA20K%20Discord&amp;style=flat&amp;color=5865F2&amp;logo=discord&amp;logoColor=white&amp;cacheSeconds=3600" alt="Discord beitreten" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/VERA20K_QQ-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ-Gruppe (VERA20K)" height="23" align="middle">
  <img src="https://img.shields.io/badge/RA20K.com_QQ-319832195-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ-Gruppe (RA20K.com)" height="23" align="middle">
</p>

# VERA20K

VERA20K ist eine neue Engine für Red Alert 2: Yuri's Revenge. Sie spielt sich wie das Original, mit
denselben Einheiten, Karten und demselben Aussehen, und läuft unter Windows, macOS und Linux. Sie
ist für größere Schlachten mit bis zu 20.000 Einheiten auf einer Karte gebaut. Offline-Gefechte bieten
bis zu 120 Spielerplätze, LAN- und Online-Partien bis zu 30.

VERA20K wird von Spielern für Spieler gemacht, und die Spieler haben das letzte Wort, wohin sich
das Projekt entwickelt.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K: Gefechtseinstellungen und eine Szene aus dem Spiel" width="100%">

## Herunterladen und spielen

1. Installiere Red Alert 2: Yuri's Revenge über Steam oder die EA app.
2. Lade VERA20K für dein System von der [Release-Seite](https://github.com/YuriPlanet/vera20k/releases) herunter.
3. Öffne die heruntergeladene Datei. Sie findet dein Spiel und richtet VERA20K in einem eigenen
   Ordner ein.

Kommst du nicht weiter? Schau auf der [Hilfeseite](https://vera20k.org/de/help) nach.

## Projektziele

1. Spielmechanik, Grafik und Atmosphäre des ursprünglichen Red Alert 2: Yuri's Revenge bewahren.
2. Größere Schlachten auf 500×500-Karten ermöglichen, mit bis zu **20.000 Einheiten**, **120 Spielerplätzen** in Offline-Gefechten und **30** in LAN- und Online-Partien.
3. Bekannte RTS-Funktionen integrieren und einige, die es noch nie gab.
4. Integrierter Mehrspieler-Client

## Mitmachen

**Geschätzte Pull Requests** — Refactoring, Performance, Wegfindung, UI/UX, Zufallskartengenerator.

Mitwirkende können Verantwortung für Bereiche der Engine übernehmen und haben dabei viel
Spielraum für Änderungen, die VERA20K voranbringen.

Die Maintainer helfen beim Review, bei der Integration und beim Mergen. Dein Beitrag wird
dir vollständig zugeschrieben.

Admins: [Halvor Nikolai Rusten](https://github.com/hrusten), [风雨声](https://github.com/996666925), [Kironix](https://github.com/EncoreAlpha), [Randerion(HaoJun0823)](https://github.com/HaoJun0823), [libi](https://github.com/libi), [zzzzzzh123](https://github.com/zzzzzzh123), [八音盒](https://github.com/1372704087)

Diskutiert wird auf [Discord](https://discord.gg/kmjRUn5m5F) und in der QQ-Gruppe (1097193563).

## Architektur

Die Engine besteht aus drei Hauptteilen:

- **Simulation:** Verwaltet Spielzustand und Spiellogik.
- **Rendering:** Zeichnet Gelände, Einheiten und Gebäude mit den Originalgrafiken auf der GPU.
- **Anwendung:** Übernimmt Eingaben, Menüs, Audio und Plattformintegration und verbindet die Systeme.

Diese Trennung unterstützt einen deterministischen Spielablauf.

Die Pfeile zeigen den Datenfluss:

![Die Anwendung sendet Befehle an die Simulation, die den Weltzustand zurückgibt; die Anwendung sendet Zeichendaten an das Rendering.](docs/images/architecture.de.png)

## Kompilieren und starten

Du brauchst:

- **Rust und Git:** [Rust](https://rustup.rs/) 1.97 oder neuer, installiert mit rustup, und
  [Git](https://git-scm.com/).
- **Build-Werkzeuge:** unter Windows die C++-Build-Tools von Visual Studio, deren Installation
  der Rust-Installer anbietet; unter macOS `xcode-select --install`; unter Debian und Ubuntu
  `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Bearbeite config.toml und setze ra2_dir auf deinen Spielordner, bevor du das Spiel startest:
cargo run --release --bin vera20k
```

## So arbeiten wir

Der Großteil des Codes wird von KI-Programmieragenten geschrieben. Sie untersuchen die
ursprüngliche Engine mit Ghidra, übertragen dann ihr Verhalten nach Rust und prüfen es mit
[Vergleichswerkzeugen](tools/native_oracle.md) und Spieltests. Die Agenten folgen
[AGENTS.md](AGENTS.md).

## Anpassung

Du kannst eigene Mods, Karten, Musik und mehr hinzufügen. Sie gehören in den VERA20K-Ordner:
unter Windows der Installationsordner, unter macOS `~/Library/Application Support/VERA20k` und
unter Linux `~/.local/share/VERA20k`.

VERA20K verwendet standardmäßig die Spielregeln von Yuri's Revenge. Unter **Options** kannst du
Ton, Grafik und Steuerung anpassen oder VERA-Funktionen einschalten. Das Spiel speichert deine
Einstellungen in `VERA.INI`. Die VERA-spezifischen Optionen stehen im Abschnitt `[VERA]`.

- **Mods:** Unter **Options → Mods** installiert **Add Mod** eine heruntergeladene Mod, und **New Mod** beginnt eine eigene. Jede Mod bekommt einen eigenen Ordner in `Mods`.
- **Karten:** Lege `.map`-, `.mpr`- oder `.yrm`-Dateien in `Maps`, um sie im Gefecht zu spielen. Online heruntergeladene Karten landen ebenfalls dort.
- **Musik:** Lege WAV- oder AUD-Dateien in `musics` und füge dort eine `music.ini` mit `enable=yes` unter `[Music]` hinzu. Dann laufen deine Lieder in Partien neben der Musik des Spiels; mit `ExclusiveMode=yes` nur deine.
- **HD-Grafik:** Ein [HD-Grafikpaket](docs/hd-art.md) wird wie eine Mod installiert und ausgewählt. Schalte danach **HD art** unter **Options** ein.
- **Sprachen:** Eine Datei `vera20k.<language>.ini` fügt eine Sprache hinzu oder ändert die Texte des Spiels. Siehe [Übersetzen](src/i18n/README.md).

Lobby-Einstellungen, Mods und Karten können die Regeln einer Partie ändern. Mods und Karten können
über ihre INI-Dateien unterstützte Funktionen von [Ares](docs/ares.md), [Phobos](docs/phobos.md) und [Kratos](docs/kratos.md)
aktivieren. Eine Mod kann Ares auch aktivieren, indem sie `ares.mix` mitliefert. Einige
Phobos-Funktionen erfordern weiterhin eine mit `--features phobos` kompilierte Spielversion.

Wie du deine eigene Mod erstellst, zeigt die [Modding-Anleitung](docs/modding.md).

## Unterstützen

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="Unterstütze VERA20K über PayPal" height="23"></a>

Alle eingegangenen Spenden werden
gemäß dem [TIBTRE-Beitragsverzeichnis](https://github.com/YuriPlanet/tibtre-ledger) an
Mitwirkende verteilt.

Auszahlungen an Mitwirkende:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## Medien

<p>
  <a href="https://www.youtube.com/@cncvietnam-huydeptrai"><img src="docs/images/cnc-vietnam.webp" width="64" height="64" align="left" alt="Command &amp; Conquer Vietnam (YouTube)"></a>
  <a href="https://www.youtube.com/@cncvietnam-huydeptrai">Command &amp; Conquer Vietnam</a> stellt VERA20K vor.<br>
  <a href="https://www.youtube.com/watch?v=njpQRCTZ_MY">1 gegen 29 Brutal-KIs auf Arctic Circle 30</a><br>
  <a href="https://www.youtube.com/watch?v=7r_z_7w0InA&amp;t=1440s">Survival Fortress 5.0</a><br clear="all">
</p>

<p>
  <a href="https://www.youtube.com/@haojunyang2337"><img src="docs/images/haojun0823.webp" width="64" height="64" align="left" alt="HaoJun Yang"></a>
  <a href="https://www.youtube.com/@haojunyang2337">HaoJun Yang</a> stellt VERA20K vor.<br>
  <a href="https://www.youtube.com/watch?v=ucz-EiMeW88">Red Alert 2 in VERA20K 0.41</a><br clear="all">
</p>

## Karteneditoren

<p>
  <a href="https://github.com/996666925/ra2-map-editor"><img src="https://avatars.githubusercontent.com/u/52450772?s=128" width="64" height="64" align="left" alt="996666925"></a>
  <a href="https://github.com/996666925/ra2-map-editor">YR Map Editor</a><br>
  Erstelle Karten für Yuris Rache und teste sie in VERA20K.<br clear="all">
</p>

## Danksagung und Rechtliches

Danke an OpenRA, XCC Mixer, das ModEnc-Wiki, Project Perfect Mod, EA für die Veröffentlichung
des Quellcodes von Command & Conquer und Red Alert unter der GPL, World-Altering Editor,
Final Alert, YRpp, Ares, Phobos und viele andere.

Copyright © 2026 Halvor Nikolai Rusten und die Mitwirkenden an VERA20K.
Lizenziert unter der [GPLv3](LICENSE-GPL) mit [zusätzlichen Bedingungen](LEGAL.md#additional-terms).
Die Spieldateien für Red Alert 2: Yuri's Revenge
stammen aus deiner eigenen Installation.
Command & Conquer und Red Alert sind Marken von Electronic Arts Inc. Die Screenshots und das
App-Symbol, der rote Stern aus Red Alert 2, zeigen Spielgrafiken, die Electronic Arts gehören.
VERA20K steht in keiner Verbindung zu Electronic Arts und wird nicht von Electronic Arts
unterstützt.

This product includes software developed by the [Ares Contributors](LICENSES/Ares.txt).

This product includes software developed by the [Phobos Developers](docs/phobos.md#original-project-and-authors).

**Lies den [rechtlichen Hinweis](LEGAL.md), bevor du VERA20K weitergibst, verkaufst oder forkst.**

## Dokumentation

- [WebAssembly-Vorschau](docs/wasm.md): ein optionaler Einzelspieler-Build, der im Browser läuft.
- [Modding-Anleitung](docs/modding.md): was Mods können und was nicht und worin sich VERA20K vom Originalspiel unterscheidet.
- [INI-Profile](docs/ini-roots.md): Regel-, Grafik- und KI-Dateien auswählen.
- [Ares](docs/ares.md), [Phobos](docs/phobos.md) und [NPatch](docs/npatch.md): Unterstützung der Erweiterungen und ihre Grenzen.
- [Engine-Überblick](docs/index.md): Aufbau des Codes und wie die Teile der Engine zusammenspielen.
- [Entwicklerwerkzeuge](tools/README.md): Builds, Tests und Untersuchung der Spieldaten.
- [Kartentests](tools/map_observation.md): geskriptete Szenarien, Screenshots und Beobachtungen von Partien.
- [Vergleiche mit dem Original](tools/native_oracle.md): Verhalten mit dem Originalspiel vergleichen.
- [Ghidra-Arbeitsablauf](docs/ghidra-workflow.md): die ursprüngliche Engine untersuchen.

Übersetzung von [README.md](README.md); bitte mit der englischen Version synchron halten.
