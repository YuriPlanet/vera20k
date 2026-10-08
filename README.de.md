<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k Titelbild" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <strong>Deutsch</strong> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter macOS (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Bibliothekstests unter macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Linux (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Bibliothekstests unter Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Letzter Testlauf der Bibliothek unter Windows (manuell gestartet)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Bibliothekstests unter Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Discord beitreten" height="20" align="middle"></a>
</p>

# VERA20k

Red Alert 2: Yuri's Revenge — in Rust neu entwickelt, für große Mehrspielerschlachten.

VERA20k ist eine Neuimplementierung der ursprünglichen Engine, `gamemd.exe`. Sie verwendet
die originalen Spieldateien, also brauchst du eine eigene Kopie von Red Alert 2: Yuri's Revenge. Das Spiel
ist in *Command & Conquer The Ultimate Collection* auf
[Steam](https://store.steampowered.com/bundle/39394/) und bei
[EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc) erhältlich.

VERA20k wird von Spielern für Spieler gemacht, und die Spieler haben das letzte Wort, wohin sich
das Projekt entwickelt.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k: Gefechtseinstellungen und eine Szene aus dem Spiel" width="100%">

## Projektziele

1. Spielmechanik, Grafik und Atmosphäre des ursprünglichen Red Alert 2: Yuri's Revenge bewahren.
2. Größere Schlachten ermöglichen: bis zu **30 Spieler** und **20.000 Einheiten** auf größeren Karten.
3. Neue RTS-Funktionen integrieren.
4. Integrierter Mehrspieler-Client

## Aktueller Stand

**Frühe Entwicklungsphase.** Lokale Gefechte gegen eine einfache KI sind unter Windows
spielbar. Originalkarten und zufällig generierte Karten, Menüs, Basisbau, Rohstoffernte,
Kämpfe sowie Speichern und Laden sind vorhanden, aber es gibt noch viel zu verbessern
und fertigzustellen.

Mehrspielermodus, Kampagnen und die ursprüngliche KI fehlen noch. Flugzeuge,
Gedankenkontrolle, Brücken und mehrere Waffen und Effekte brauchen noch Arbeit.
Schlachten mit 30 Spielern und 20.000 Einheiten haben wir bisher nicht demonstriert.

## Kompilieren und starten

Du brauchst Rust 1.88 oder neuer, eine Grafikkarte mit Vulkan, DirectX 12 oder Metal und
eine installierte Version des Spiels. VERA20k wurde bereits unter Windows, Linux und macOS gespielt.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Bearbeite config.toml und setze ra2_dir auf deinen Spielordner, bevor du das Spiel startest:
cargo run --release --bin vera20k
```

Verwende zum Spielen `--release`; Debug-Builds sind zu langsam. In
[CONTRIBUTING.md](CONTRIBUTING.md#set-up) findest du Hinweise zur Einrichtung auf den
einzelnen Plattformen und zum Ausführen der Tests.

## So arbeiten wir

Der Großteil des Codes wird von KI-Programmieragenten geschrieben, die ich anleite.
Wir untersuchen die ursprüngliche Engine mit Ghidra, übertragen dann ihr Verhalten
nach Rust und prüfen es mit [Vergleichswerkzeugen](tools/native_oracle.md) und Spieltests.
Unsere Arbeitsregeln stehen in [AGENTS.md](AGENTS.md).

## Mitmachen

Hilfe ist willkommen. Du kannst Code schreiben, das Spiel testen, die Dokumentation
verbessern oder es neben dem Original spielen und uns sagen, was sich falsch anfühlt.
Du brauchst keine Erfahrung mit Reverse Engineering, um mitzuhelfen.

Lies [CONTRIBUTING.md](CONTRIBUTING.md), schau dir die
[Aufgaben für den Einstieg](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue)
an oder sag auf [Discord](https://discord.gg/kmjRUn5m5F) Hallo.
Die [Architekturübersicht](https://yuriplanet.github.io/vera20k/de/) erklärt, wie die Engine aufgebaut ist.

## Danksagung und Rechtliches

Danke an OpenRA, XCC Mixer, das ModEnc-Wiki, Project Perfect Mod, EA für die Veröffentlichung
des Quellcodes von Command & Conquer und Red Alert unter der GPL, World-Altering Editor,
Final Alert, YRpp, Ares, Phobos und viele andere.

Lizenziert unter der [GPLv3](LICENSE-GPL). Dieses Repository enthält keine Spieldateien.
Command & Conquer und Red Alert sind Marken von Electronic Arts Inc. Die Screenshots zeigen
Spielgrafiken, die Electronic Arts gehören. VERA20k steht in keiner Verbindung zu Electronic Arts
und wird nicht von Electronic Arts unterstützt.

Übersetzung von [README.md](README.md); bitte mit der englischen Version synchron halten.
