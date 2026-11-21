# Geländeregeln und Bodenbewegung in VERA20K

[English (Referenzfassung)](preset-terrain.md) · [简体中文](preset-terrain.zh-CN.md) · **Deutsch**

Diskussionsentwurf für Kartenautoren, Modder und Engine-Entwickler. Er beschreibt zwei Änderungen, die VERA20K unabhängig voneinander übernehmen kann, sowie die weitergehenden Ziele des eigenständigen Forks [VERA Mod Engine](https://github.com/YuriPlanet/vera-mod-engine).

## Theater, Karte und Spieldateien

Ein *Theater* wie `TEMPERATE` bestimmt, welche Kachelsätze eine Karte verwenden kann. Die Karte wählt das Theater und setzt Kacheln, Unterkacheln, Zellhöhen, Overlays und Objekte. Zu den Dateien der Spielinstallation gehören Geländekacheln, Paletten und Sprites. TMP-Kacheln liefern neben Bilddaten auch Angaben zu Geländeart, Höhe und Rampenform. Diese Angaben beeinflussen die Simulation. [Theater-Lader](../../src/map/theater.rs)

VERA20K könnte zunächst die Theaterdefinition vom Dateizugriff trennen. Der bestehende Lader würde die Definition aus `[TileSetNNNN]` und `[General]` liefern. Nummern, Reihenfolge und Brückenteile bleiben erhalten. Dieselbe Definition gilt für Vorschau, Spiel, Headless-Betrieb und das Laden eines Spielstands. Unabhängig davon lässt sich die Bodenbewegung so umbauen, dass Befehl, Wegsuche und ausgeführter Schritt dieselbe Fläche bezeichnen.

## Vom Fahrbefehl bis zur Brückenreparatur

Ein Mod-Autor soll einem neuen Fahrzeug ein benanntes Bewegungsprofil zuweisen können. Es legt befahrbare Geländearten, Fahrkosten und Übergänge zwischen Boden und Brückendeck fest. Der neue Typ kommt ohne Eintrag in `[VehicleTypes]` und ohne `Locomotor`-GUID aus. Alte Mods können weiterhin `SpeedType`, `MovementZone` und `Locomotor` angeben. Der Fork löst Typnamen und historische Listennummern vor Spielbeginn auf. Karte, Fabrik und Trigger verwenden denselben Fahrzeugtyp. Jede erzeugte Einheit erhält eine eigene ID.

Ein Klick auf das Brückendeck bezeichnet die Zelle **und** die Ebene. Die Bewegung muss den geplanten Ebenenwechsel an der vorgesehenen Rampe ausführen. Ein Fahrzeug auf dem Deck und eines unter der Brücke belegen unterschiedliche Flächen derselben Zelle. `ResolvedTerrainGrid` liefert die Zellgeometrie. Das Modul `world::substrate` verwaltet die Zellbelegung und die Reihenfolge der Objekte. Ändert sich die Erreichbarkeit, müssen betroffene Zonen und Wege neu geprüft werden. [Geländeraster](../../src/map/resolved_terrain.rs) · [Zellbelegung](../../src/sim/world/substrate.rs) · [Navigation](../../src/sim/world/navigation.rs)

Beschuss oder Reparatur ändern den Brückenzustand über `BridgeRuntimeState`. Beim Einsturz veröffentlicht der vorhandene Brückencode die betroffenen Zellen. Einheiten auf und unter der Brücke erleiden die vorgesehenen Folgen. Der für die Zelle in FinalAlert 2 gesetzte Tag erhält das Ereignis an der vorgesehenen Stelle im Schadensablauf. Fahrbefehle und Bauplatzprüfung berücksichtigen die neue Geometrie. Nach der Reparatur wird das Deck wieder erreichbar. Darstellung, Radar und Bauvorschau zeigen den Zustand der Simulation. [Brückenzustand](../../src/sim/bridge_state/mod.rs) · [Übernahme in die Spielwelt](../../src/sim/world/bridge_publication.rs)

## Produktvertrag des Forks

Der Fork soll eigene versionierte Presets für RA2 und YR ausliefern. Ein Ares-Preset erweitert eine bestimmte YR-Fassung. Zur Laufzeit darf der Fork die Spielinstallation ausschließlich lesend für Spieldaten wie Grafik und Ton sowie für die ausgewählte Karte öffnen. Originale Basis-INIs werden weder als lose Dateien noch als MIX-Einträge angefordert. Ein fehlendes Preset wird mit Name und Version gemeldet. Deklarierte Mod-INIs können die vorgesehene Basisschicht ersetzen. Ergänzungen des Mods und Kartenregeln greifen in der Lesereihenfolge des gewählten Spielmodus.

Spielstände, Replays und Netzwerkprotokoll des Forks erhalten eigene Versionen. Ein Spielstand muss die wirksamen Regeln und den laufenden Bewegungszustand festhalten. Replays prüfen Karte und Regelsatz. Eine Mehrspielerpartie darf nur bei übereinstimmendem Protokoll und Spielinhalt starten. Abweichungen werden im Spielraum angezeigt.

## Abnahme

- Einen Fahrbefehl über eine Brücke während der Überfahrt durch Einsturz unterbrechen. Belegung, Schaden, Zielerreichbarkeit, Bauvorschau und Kartentrigger prüfen. Nach der Reparatur erneut über die Brücke fahren.
- Vor dem Einsturz speichern und nach einem Neustart laden. Bei gleichen Eingaben müssen Wiederholung und Lockstep-Partie denselben Simulationszustand ergeben. Im Fork ab Erststart die Dateizugriffe prüfen. Erwartet werden null Anfragen nach originalen Basis-INIs und null Schreibzugriffe auf die Installation. Entfernte oder geänderte Basis-INIs müssen wirkungslos bleiben, eine ausdrücklich angegebene Kartenregel muss greifen.
- Auf gleicher Hardware zwei Release-Builds bei 20.000 Einheiten und 30 Häusern vergleichen. Karte, Zufalls-Seed und Befehle festhalten, Tickdauer (Median und 95. Perzentil), Spitzenspeicherbedarf und Endzustand erfassen.

Der Fork beginnt mit INI und INIWeaver. Eine spätere Wasm-Anbindung könnte dieselben Geländeabfragen und Aktionen nutzen. Über den Umfang der Wasm-Schnittstelle wird in libis RFC beraten. Der Vorschlag von HaoJun0823 zu Inhaltspaketen und die Antwort von libi zur schrittweisen Einführung und Weitergabe von Mods stehen im Originalthread. [Wasm-Entwurf #4653](https://github.com/YuriPlanet/vera20k/pull/4653) · [Beitrag von HaoJun0823](https://github.com/YuriPlanet/vera20k/pull/4653#issuecomment-7040998846) · [Antwort von libi](https://github.com/YuriPlanet/vera20k/pull/4653#issuecomment-7042992589)
