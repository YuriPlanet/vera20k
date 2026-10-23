<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K Titelbild" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <strong>Deutsch</strong> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter macOS (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Bibliothekstests unter macOS" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Linux (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Bibliothekstests unter Linux" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Windows (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Bibliothekstests unter Windows" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Discord beitreten" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_Gruppe-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ-Gruppe" height="23" align="middle">
</p>

# VERA20K

VERA20K ist eine von der Community getragene Neuimplementierung der ursprünglichen Engine,
`gamemd.exe`. Sie verwendet die originalen Spieldateien, also brauchst du eine eigene Kopie von
Red Alert 2: Yuri's Revenge.

VERA20K wird von Spielern für Spieler gemacht, und die Spieler haben das letzte Wort, wohin sich
das Projekt entwickelt.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K: Gefechtseinstellungen und eine Szene aus dem Spiel" width="100%">

## Projektziele

1. Spielmechanik, Grafik und Atmosphäre des ursprünglichen Red Alert 2: Yuri's Revenge bewahren.
2. Größere Schlachten ermöglichen: bis zu **30 Spieler** und **20.000 Einheiten** auf 500×500-Karten.
3. Bekannte RTS-Funktionen integrieren und einige, die es noch nie gab.
4. Integrierter Mehrspieler-Client

## Aktueller Stand

**In aktiver Entwicklung.**

- **Gefechte:** Unter Windows, Linux und macOS spielbar, mit Originalkarten und zufällig
  generierten Karten, Basisbau, Rohstoffernte, Kämpfen sowie Speichern und Laden.
- **Kampagnen:** Missionen lassen sich starten; die Unterstützung bleibt unvollständig.
- **KI:** Teile der ursprünglichen KI übernehmen den Basisbau und skriptgesteuerte Angriffe.
- **Laufende Arbeiten:** Spielverhalten, Grafik und Leistung.
- **Mehrspielermodus:** Netzwerkspiel ist noch nicht verfügbar.
- **Größenordnung:** Schlachten mit 30 Spielern und 20.000 Einheiten wurden noch nicht demonstriert.

## Herunterladen und spielen

Lade den Installer für Windows, macOS oder Linux von der
[Release-Seite](https://github.com/YuriPlanet/vera20k/releases) herunter. Er findet dein
Yuri's Revenge aus [Steam](https://store.steampowered.com/app/2229850/) oder der
[EA app](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection)
und richtet VERA20K in einem neuen Ordner ein. Dort kommen künftig auch Mods und neue Inhalte hin,
sodass die Spieldateien nie verändert werden.

Wie du Einzelspieler-Gefechte oder Kampagnen von YR über einen CnCNet-Client startest, steht in der
[Offline-Client-Einrichtung](integrations/cncnet/README.md).

## Einstellungen

Nutze **Options** für Ton und Steuerung und **Vera Options** für VERA-spezifische Funktionen.
Spielereinstellungen, Tastenkürzel und Lobby-Einstellungen werden in `VERA.INI` gespeichert:

- **Windows-Installer:** neben `vera20k.exe`.
- **macOS:** `~/Library/Application Support/VERA20k/VERA.INI`.
- **Linux:** `~/.local/share/VERA20k/VERA.INI` oder in deinem XDG-Datenordner.

`config.toml` legt den Spielordner fest und kann einen anderen Einstellungsordner vorgeben.

Wenn `VERA.INI` fehlt, erstellt VERA die Datei und importiert vorhandene Einstellungen einmalig.
Ohne alte Einstellungen bleibt die Datei zunächst leer; VERA verwendet die integrierten
Standardwerte, bis Einstellungen gespeichert werden. Die Originaldateien `RA2MD.INI` und
`KeyboardMD.ini` werden nur für den Import gelesen und nie überschrieben.

`VERARULES.INI` im selben Ordner enthält optionale Änderungen am Spielverhalten. Die Datei
enthält zunächst auskommentierte Beispiele; die Regeln der Mission oder Karte bleiben maßgeblich.
Zum Bearbeiten und Überschreiben siehe [persönliche Regeln](docs/personal-rules.md).

## Architektur

Die Engine besteht aus drei Hauptteilen:

- **Simulation:** Verwaltet Spielzustand und Spiellogik.
- **Rendering:** Eine völlig neue GPU-Rendering-Pipeline mit den ursprünglichen Spieldaten,
  gebaut, um das ursprüngliche Erscheinungsbild zu bewahren.
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
  `sudo apt install build-essential libasound2-dev pkg-config`.

VERA20K wurde bereits unter Windows, Linux und macOS gespielt.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Bearbeite config.toml und setze ra2_dir auf deinen Spielordner, bevor du das Spiel startest:
cargo run --release --bin vera20k
```

Der Build belegt mehrere GB in `target/`.

## So arbeiten wir

Der Großteil des Codes wird von KI-Programmieragenten geschrieben. Sie untersuchen die
ursprüngliche Engine mit Ghidra, übertragen dann ihr Verhalten nach Rust und prüfen es mit
[Vergleichswerkzeugen](tools/native_oracle.md) und Spieltests. Die Agenten folgen
[AGENTS.md](AGENTS.md).

## Anpassung

VERA20K spielt sich standardmäßig wie das Originalspiel. Jede neue Funktion, auch Erweiterungen
aus Mods wie Ares, muss ausgeschaltet bleiben, bis sie auf einem dieser Wege aktiviert wird:

- **Eine VERA-Spieleroption** unter **Options → Vera Options**, gespeichert im Abschnitt `[VERA]` in `VERA.INI`.
- **Eine Build-Option**, z. B. `--features ares` für die unterstützten [Ares-Erweiterungen](docs/ares.md).
- **Eine Lobby-Option im Spiel**, die für jede Partie gewählt wird.
- **Ein INI-Schlüssel**, den eine Mod oder eine Karte setzt.

Einstellungen für Ton, Grafik und Steuerung werden in `VERA.INI` gespeichert. Die Spielregeln
stammen aus den Lobby-Einstellungen, Mods oder Karten.

## Mitmachen

Hilfe ist willkommen. Du kannst Code schreiben, die Engine refaktorieren, das Spiel testen,
Ideen einbringen oder es neben dem Original spielen und uns sagen, was sich falsch anfühlt.
Öffne einen PR, und wir kümmern uns um den Rest.

Diskutiert wird auf [Discord](https://discord.gg/kmjRUn5m5F) und in der QQ-Gruppe (1097193563).

## Unterstützen

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="Unterstütze VERA20K über PayPal" height="23"></a>

Alle eingegangenen Spenden werden
gemäß dem [TIBTRE-Beitragsverzeichnis](https://github.com/YuriPlanet/tibtre-ledger) an
Mitwirkende verteilt.

Auszahlungen an Mitwirkende:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## Danksagung und Rechtliches

Danke an OpenRA, XCC Mixer, das ModEnc-Wiki, Project Perfect Mod, EA für die Veröffentlichung
des Quellcodes von Command & Conquer und Red Alert unter der GPL, World-Altering Editor,
Final Alert, YRpp, Ares, Phobos und viele andere.

Lizenziert unter der [GPLv3](LICENSE-GPL). Dieses Repository enthält keine Spieldateien.
Command & Conquer und Red Alert sind Marken von Electronic Arts Inc. Die Screenshots zeigen
Spielgrafiken, die Electronic Arts gehören. VERA20K steht in keiner Verbindung zu Electronic Arts
und wird nicht von Electronic Arts unterstützt.

Übersetzung von [README.md](README.md); bitte mit der englischen Version synchron halten.
