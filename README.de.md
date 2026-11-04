<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K Titelbild" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <strong>Deutsch</strong> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
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

**Wir brauchen mehr Maintainer.** Wenn du beim Review von PRs helfen, neue Mitwirkende
unterstützen oder dich um einen Teil der Engine kümmern möchtest, sag uns auf
[Discord](https://discord.gg/kmjRUn5m5F) oder in der QQ-Gruppe (1097193563) Bescheid.

**Hab keine Scheu, einen PR zu öffnen.** Er muss nicht perfekt sein; wir helfen dir beim Rest.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K: Gefechtseinstellungen und eine Szene aus dem Spiel" width="100%">

## Projektziele

1. Spielmechanik, Grafik und Atmosphäre des ursprünglichen Red Alert 2: Yuri's Revenge bewahren.
2. Größere Schlachten ermöglichen: bis zu **30 Spieler** und **20.000 Einheiten** auf 500×500-Karten.
3. Bekannte RTS-Funktionen integrieren und einige, die es noch nie gab.
4. Integrierter Mehrspieler-Client

Die [Roadmap](https://github.com/orgs/YuriPlanet/projects/1) zeigt die aktuellen Prioritäten und zukünftigen Pläne.

## Herunterladen und spielen

Lade den Installer für Windows, macOS oder Linux von der
[Release-Seite](https://github.com/YuriPlanet/vera20k/releases) herunter. Er findet dein
Yuri's Revenge und richtet VERA20K in einem neuen Ordner ein.

Wie du Einzelspieler-Gefechte oder Kampagnen von YR über einen CnCNet-Client startest, steht in der
[Offline-Client-Einrichtung](integrations/cncnet/README.md).

## Mitmachen

Hilfe ist überall willkommen, und diese Bereiche brauchen sie am meisten. Jeder Bereich
verlinkt auf seine Seite in der Roadmap, mit der laufenden Arbeit und dem nächsten Schritt:

- **[Spielmechanik und Kampf](https://github.com/YuriPlanet/vera20k/issues/3830):** Einheitenverhalten, Schaden und zuverlässige Gefechte.
- **[Leistung](https://github.com/YuriPlanet/vera20k/issues/3831):** Bildrate und Reaktionsfähigkeit in großen Schlachten und auf älterer Hardware.
- **[Mehrspielermodus](https://github.com/YuriPlanet/vera20k/issues/3832):** Online-Partien beitreten, starten und am Laufen halten sowie zuverlässiges Koop-Spiel.
- **[Oberfläche und Audio](https://github.com/YuriPlanet/vera20k/issues/3833):** Menüs, Texte, Übersetzungen und Ton.
- **[Kampagnen](https://github.com/YuriPlanet/vera20k/issues/3834):** Missionen lassen sich starten, aber sie bis zum Ende durchzuspielen braucht noch Arbeit: Ziele, Trigger, geskriptete Übergänge, Speichern/Laden und der Wechsel zur nächsten Mission.
- **[Mod-Kompatibilität](https://github.com/YuriPlanet/vera20k/issues/3835):** Funktionen aus Ares und Phobos, ohne die Standardwerte des Originalspiels zu ändern.
- **[Testen](https://github.com/YuriPlanet/vera20k/issues?q=is%3Aopen%20label%3Aparity):** VERA20K neben dem Original spielen und melden, was sich anders spielt.

Mitwirkende können Verantwortung für Bereiche der Engine übernehmen und haben dabei viel
Spielraum für Änderungen, die VERA20K voranbringen.

Die Maintainer helfen beim Review, bei der Integration und beim Mergen. Dein Beitrag wird
dir vollständig zugeschrieben.

Diskutiert wird auf [Discord](https://discord.gg/kmjRUn5m5F) und in der QQ-Gruppe (1097193563).

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

Die Kartenkapazität wird ohne Build-Schalter automatisch gewählt: Die Summe von `Size`-Breite und -Höhe darf bis zu 1024 betragen.

VERA20K spielt sich standardmäßig wie das Originalspiel. Jede andere neue Funktion, auch Erweiterungen
aus Mods wie Ares, muss ausgeschaltet bleiben, bis sie auf einem dieser Wege aktiviert wird:

- **Eine VERA-Spieleroption** unter **Options**, gespeichert im Abschnitt `[VERA]` in `VERA.INI`.
- **Eine Build-Option**, z. B. `--features ares` für die unterstützten [Ares-Erweiterungen](docs/ares.md).
- **Ein INI-Schlüssel**, den eine Mod oder eine Karte setzt.

Einstellungen für Ton, Grafik und Steuerung werden in `VERA.INI` gespeichert. Die Spielregeln
stammen aus den Lobby-Einstellungen, Mods oder Karten.

## Unterstützen

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="Unterstütze VERA20K über PayPal" height="23"></a>

Alle eingegangenen Spenden werden
gemäß dem [TIBTRE-Beitragsverzeichnis](https://github.com/YuriPlanet/tibtre-ledger) an
Mitwirkende verteilt.

Auszahlungen an Mitwirkende: PayPal, China UnionPay, Alipay, Vipps, SEPA oder Bitcoin.

## Danksagung und Rechtliches

Danke an OpenRA, XCC Mixer, das ModEnc-Wiki, Project Perfect Mod, EA für die Veröffentlichung
des Quellcodes von Command & Conquer und Red Alert unter der GPL, World-Altering Editor,
Final Alert, YRpp, Ares, Phobos und viele andere.

Copyright © 2026 Halvor Nikolai Rusten und die Mitwirkenden an VERA20K.
Lizenziert unter der [GPLv3](LICENSE-GPL) mit [zusätzlichen Bedingungen](LEGAL.md#additional-terms).
Dieses Repository enthält keine Spieldateien.
Command & Conquer und Red Alert sind Marken von Electronic Arts Inc. Die Screenshots und das
App-Symbol, der rote Stern aus Red Alert 2, zeigen Spielgrafiken, die Electronic Arts gehören.
VERA20K steht in keiner Verbindung zu Electronic Arts und wird nicht von Electronic Arts
unterstützt.

**Lies den [rechtlichen Hinweis](LEGAL.md), bevor du VERA20K weitergibst, verkaufst oder forkst.**

## Dokumentation

- [WebAssembly-Vorschau](docs/wasm.md): ein optionaler Einzelspieler-Build, der im Browser läuft.
- [INI-Profile](docs/ini-roots.md): Regel-, Grafik- und KI-Dateien auswählen.
- [Ares](docs/ares.md), [Phobos](docs/phobos.md) und [NPatch](docs/npatch.md): Unterstützung der Erweiterungen und ihre Grenzen.
- [Engine-Überblick](docs/index.md): Aufbau des Codes und wie die Teile der Engine zusammenspielen.
- [Entwicklerwerkzeuge](tools/README.md): Builds, Tests und Untersuchung der Spieldaten.
- [Kartentests](tools/map_observation.md): geskriptete Szenarien, Screenshots und Beobachtungen von Partien.
- [Vergleiche mit dem Original](tools/native_oracle.md): Verhalten mit dem Originalspiel vergleichen.
- [Ghidra-Arbeitsablauf](docs/ghidra-workflow.md): die ursprüngliche Engine untersuchen.

Übersetzung von [README.md](README.md); bitte mit der englischen Version synchron halten.
