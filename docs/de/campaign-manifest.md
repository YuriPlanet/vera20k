---
layout: default
title: Kampagnenserien
lang: de
---

# Kampagnenserien

[English](../campaign-manifest.md) · [简体中文](../zh-CN/campaign-manifest.md) · **Deutsch** · [العربية](../ar/campaign-manifest.md) · [Русский](../ru/campaign-manifest.md) · [Svenska](../sv/campaign-manifest.md) · [ไทย](../th/campaign-manifest.md) · [Türkçe](../tr/campaign-manifest.md)

In `campaigns/<Serie>/campaign.ini` steht, welche Karte zu jeder Mission einer Serie gehört. Dort legst du auch Regeln, Rollen, Routen und die Übergabe an die nächste Mission fest. Für den ersten Versuch reicht eine Karte.

Mit `asset campaign-check` startest du eine Prüfung ohne Raum. Zum Spielen öffnest du einen Raum für eine Person oder für mehrere Spieler mit eigenen Rollen. Für Internetspiel muss die Lobby Kampagnenräume unterstützen. Das normale Menü **Kampagne** verwendet die Kampagnenliste aus `battlemd.ini`.

[Schlüsselverzeichnis](campaign-manifest-keys.md) mit Werten, Voreinstellungen und Wirkung aller Schlüssel.

## Mit einer Mission beginnen

Speichere `campaign.ini` als UTF‑8 unter `campaigns/broken-line/`. Lege die Karte daneben unter `maps/opening.map` ab. Die Datei darf bis zu 64 KiB groß sein. Verwende relative Pfade mit `/`.

```ini
[Stage Opening]
Map=maps/opening.map
```

Eine Mission ohne Rollen übernimmt die Fraktion aus `[Basic] Player`. Sie hat einen Spielerplatz.

## Eine Mission ohne Raum prüfen

```text
asset campaign-check campaigns/broken-line/campaign.ini Opening 120 --ra2-dir "/path/to/Yuri's Revenge"
```

Gib die Missions-ID und eine positive Bildzahl an. Der Prüflauf lädt die Karte, baut das Spiel aus der Aufzeichnung erneut auf und vergleicht die Zustände Bild für Bild. Bei einem Fehler in der INI nennt das Werkzeug Datei, Abschnitt, Schlüssel, Zeile und eine Korrektur. Wenn die Dateien im Raum abweichen, prüfe auch die eingebundenen INIs.

## Rollen hinzufügen

Mit `[Roles Opening]` können mehrere Personen mitspielen. Die Fraktionsnamen stammen aus `[Houses]` auf der Karte.

```ini
[Roles Opening]
Lead=Americans
Support=Americans

[Grants Opening]
Lead.Russians=FullControl
Support.Russians=UnitsOnly
```

`Lead` und `Support` teilen sich `Americans`. `FullControl` erlaubt auch Befehle, die volle Kontrolle über `Russians` erfordern. `UnitsOnly` erlaubt taktische Befehle. Lege die Spielerzahl mit `MinPlayers` und `MaxPlayers` fest. In `[Routes Opening]` verweist beispielsweise `Coast=Finale` auf eine bereits deklarierte Mission. `[CarryOver Opening]` legt fest, welche Werte mitgenommen werden.

## Installieren und einen Raum öffnen

Lege den Serienordner unter `campaigns/<Serie>/` im Spielordner oder im aktiven Mod ab. Alle Spieler brauchen denselben Spielbuild, dieselben Regeln und dieselben Kampagnendateien. Übernimm auch die Ordnerstruktur und die von den Karten eingebundenen INI-Dateien.

1. Erstelle einen LAN-Raum und wähle die Kampagne in der Modusliste. **Standard-Mehrspieler** führt zu den üblichen Spielmodi.
2. Wähle eine Mission und den Schwierigkeitsgrad. Hat die Karte Rollen, weise in der Spalte **Rolle** jedem Mitspieler eine eigene zu. Zwei Rollen können dieselbe Fraktion steuern.
3. Starte, wenn alle bereit sind. Prüfe bei einer Startverweigerung zuerst die Dateien, die Rollenzuweisung und die Spielergrenzen der Mission.

Ein Internetraum setzt einen WOL-Lobbyserver mit `campaign_rooms` voraus. Er muss die Kampagnenoptionen auch beim Start und beim Zuschauen weitergeben. Meldet die Lobby, dass Kampagnenräume fehlen, verwende LAN. Zuschauer im Beobachtungsmodus belegen keine Rolle. Räume vor dem Start gegebenenfalls die normalen Zuschauerplätze.

Bei einer Zwischensequenz wartet das Spiel auf die Mitspieler. Nach der Mission wechselt **Andere Route** das angezeigte Ziel. **Fortsetzen** stimmt für dieses Ziel. **Wiederholen** startet die Mission mit dem Stand bei ihrem Eintritt neu. Alle Mitspieler müssen dieselbe Wahl treffen. Zuschauer stimmen nicht ab.

[Vollständiges INI-Format und Wiedergabe auf Englisch](../campaign-manifest.md) · [Netzwerkprotokoll auf Englisch](../Networking.md#multiplayer-campaign-rooms)
