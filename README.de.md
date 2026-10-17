<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k Titelbild" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <strong>Deutsch</strong> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter macOS (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Bibliothekstests unter macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Linux (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Bibliothekstests unter Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Windows (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Bibliothekstests unter Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Discord beitreten" height="20" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_Gruppe-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ-Gruppe" height="20" align="middle">
</p>

# VERA20k

VERA20k ist eine von der Community getragene Neuimplementierung der ursprünglichen Engine,
`gamemd.exe`. Sie verwendet die originalen Spieldateien, also brauchst du eine eigene Kopie von
Red Alert 2: Yuri's Revenge.

VERA20k wird von Spielern für Spieler gemacht, und die Spieler haben das letzte Wort, wohin sich
das Projekt entwickelt.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k: Gefechtseinstellungen und eine Szene aus dem Spiel" width="100%">

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

- **Das Spiel:** Red Alert 2: Yuri's Revenge.
- **Rust und Git:** die aktuelle stabile Version von [Rust](https://rustup.rs/), installiert mit
  rustup, und [Git](https://git-scm.com/).
- **Build-Werkzeuge:** unter Windows die C++-Build-Tools von Visual Studio, deren Installation
  der Rust-Installer anbietet; unter macOS `xcode-select --install`; unter Debian und Ubuntu
  `sudo apt install build-essential libasound2-dev pkg-config`.
- **Grafik:** eine Grafikkarte mit Vulkan, DirectX 12 oder Metal.

VERA20k wurde bereits unter Windows, Linux und macOS gespielt.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Bearbeite config.toml und setze ra2_dir auf deinen Spielordner, bevor du das Spiel startest:
cargo run --release --bin vera20k
```

Schreib `ra2_dir` mit normalen Schrägstrichen, etwa `C:/Games/RA2`. Verwende zum Spielen
`--release`; Debug-Builds sind zu langsam. Das Log liegt in `logs/ra2.log`.

## So arbeiten wir

Der Großteil des Codes wird von KI-Programmieragenten geschrieben. Sie untersuchen die
ursprüngliche Engine mit Ghidra, übertragen dann ihr Verhalten nach Rust und prüfen es mit
[Vergleichswerkzeugen](tools/native_oracle.md) und Spieltests. Die Agenten folgen
[AGENTS.md](AGENTS.md).

## Anpassung

Optionale Funktionen (z. B. aus Ares) sollen sich über INI-Dateien konfigurieren und,
wo sinnvoll, im Client ein- und ausschalten lassen.

## Mitmachen

Hilfe ist willkommen. Du kannst Code schreiben, die Engine refaktorieren, das Spiel testen,
Ideen einbringen oder es neben dem Original spielen und uns sagen, was sich falsch anfühlt.
Öffne einen PR, und wir kümmern uns um den Rest.
Diskutiert wird auf [Discord](https://discord.gg/kmjRUn5m5F) und in der QQ-Gruppe (1097193563).

Die Spiellogik liegt in `src/sim/`, das Rendering in `src/render/` und Menüs und Eingabe in
`src/app/` und `src/ui/`; die Python-Werkzeuge in `tools/` brauchst du nicht.

Zum Testen deiner Änderungen im Spiel starte `cargo run --profile fast --bin vera20k`. Das ist
optimiert wie `--release`, baut aber nur neu, was du geändert hast, sodass du schneller wieder
im Spiel bist. Miss die Leistung mit `--release`.

Beiträge stehen wie das übrige Projekt unter der GPLv3; ein CLA gibt es nicht.

## Danksagung und Rechtliches

Danke an OpenRA, XCC Mixer, das ModEnc-Wiki, Project Perfect Mod, EA für die Veröffentlichung
des Quellcodes von Command & Conquer und Red Alert unter der GPL, World-Altering Editor,
Final Alert, YRpp, Ares, Phobos und viele andere.

Lizenziert unter der [GPLv3](LICENSE-GPL). Dieses Repository enthält keine Spieldateien.
Command & Conquer und Red Alert sind Marken von Electronic Arts Inc. Die Screenshots zeigen
Spielgrafiken, die Electronic Arts gehören. VERA20k steht in keiner Verbindung zu Electronic Arts
und wird nicht von Electronic Arts unterstützt.

Übersetzung von [README.md](README.md); bitte mit der englischen Version synchron halten.
