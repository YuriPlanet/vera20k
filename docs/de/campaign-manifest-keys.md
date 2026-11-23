---
layout: default
title: Schlüssel in campaign.ini
lang: de
---

# Schlüssel in campaign.ini

[English](../campaign-manifest-keys.md) · [简体中文](../zh-CN/campaign-manifest-keys.md) · **Deutsch** · [العربية](../ar/campaign-manifest-keys.md) · [Русский](../ru/campaign-manifest-keys.md) · [Svenska](../sv/campaign-manifest-keys.md) · [ไทย](../th/campaign-manifest-keys.md) · [Türkçe](../tr/campaign-manifest-keys.md)

Ersetze `<id>` durch die Missions-ID. Boolesche Schlüssel akzeptieren `Yes`, `No`, `True`, `False`, `1` und `0`. Die Tabelle nennt die Standardwerte für Missionen aus dieser Datei. Ein Strich bedeutet, dass der Schlüssel fehlt. Lass unbenutzte optionale Einträge ganz weg. [Installation, Beispiele und Prüfung](campaign-manifest.md)

## `[Campaign]`

| Schlüssel | Werte | Standard | Zeitpunkt und Wirkung |
| --- | --- | --- | --- |
| `Title` | Text | Serienordner | Der Client zeigt diesen Namen in der Modusliste an, wenn er die installierten Serien einliest. Beispiel `Title=Broken Line`. |
| `Title.<locale>` | Text | `Title` | Beim Einlesen der Serienliste wählt der Client die Übersetzung für die Oberflächensprache. Beispiel `Title.de=Unterbrochene Linie`. Danach folgen die Grundsprache und `Title`. |

## `[Stage <id>]`

| Schlüssel | Werte | Standard | Zeitpunkt und Wirkung |
| --- | --- | --- | --- |
| `Map` | Relativer Pfad | Erforderlich | Die Raumprüfung und der Kartenlader öffnen die Datei im Serienordner. Beispiel `Map=maps/opening.map`. Die aufgelöste Datei muss im Serienordner liegen. |
| `MapSha256` | 64 Hexziffern | — | Bei der Kartenprüfung vergleicht das Spiel die SHA-256 der Datei mit dem angegebenen Wert. Die Spieler vergleichen ihre Kartendateien auch ohne diesen Eintrag. |
| `MinPlayers` | 1–128 | `1` | Vor dem Start müssen mindestens so viele Spieler verbunden sein. Benötigt eine Mission beide Rollen, setze `MinPlayers=2`. Zuschauer zählen nicht mit. |
| `MaxPlayers` | 1–128 | Rollen- und Raumgrenze | Die Raumzulassung begrenzt die Spielerzahl. Der kleinste Wert aus diesem Eintrag, der Rollenzahl und der Raumgrenze gilt. Ohne Rollenliste gibt es genau einen Platz. |
| `Title` | Text | `ID` | Missionsauswahl und Ergebnisdialog zeigen diesen Namen. Beispiel `Title=Durchbruch`. |
| `Title.<locale>` | Text | `Title` | Missionsauswahl und Ergebnisdialog wählen diese Übersetzung für die aktive Sprache. Beispiel `Title.de=Durchbruch`. Die Rückfallregeln entsprechen `[Campaign] Title.<locale>`. |

## `[Roles <id>]`

| Schlüssel | Werte | Standard | Zeitpunkt und Wirkung |
| --- | --- | --- | --- |
| `RoleId=HouseId` | Fraktions-ID der Karte | `[Basic] Player` für eine Person | Beim Start verbindet das Spiel jede Rolle mit einer Fraktion aus `[Houses]`. `Lead=Americans` und `Support=Americans` teilen dieselbe Hauptfraktion. Jede Rolle gehört einem Spieler. |

## `[Grants <id>]`

| Schlüssel | Werte | Standard | Zeitpunkt und Wirkung |
| --- | --- | --- | --- |
| `RoleId.HouseId` | `FullControl` / `UnitsOnly` | — | Beim Start gibt `FullControl` der Rolle taktische Befehle und Befehle mit voller Kontrolle über eine andere Fraktion. `UnitsOnly` gibt taktische Befehle. Beispiel `Lead.Russians=FullControl`. Befehle für die Hauptfraktion bleiben an die eigene gebunden. |

## `[Routes <id>]`

| Schlüssel | Werte | Standard | Zeitpunkt und Wirkung |
| --- | --- | --- | --- |
| `BranchId=StageId` | Deklarierte Missions-ID | — | Beim Lesen löst das Spiel das Ziel auf. `Coast=Finale` zeigt die deklarierte Mission `Finale` an. `Retry` darf nur auf die aktuelle Mission verweisen. Auch ohne den Eintrag verwendet **Wiederholen** den Eintrittsstand. |

## `[CarryOver <id>]`

| Schlüssel | Werte | Standard | Zeitpunkt und Wirkung |
| --- | --- | --- | --- |
| `Globals` | `Yes` / `No` | `No` | Beim Verlassen der Mission werden die globalen Triggervariablen erfasst und vor dem ersten Bild der Zielkarte wiederhergestellt. |
| `Countdown` | `Yes` / `No` | `No` | Am Ende erfasst das Spiel die Restzeit. Setze `Countdown=Yes` im Manifest und `[Basic] TimerInherit=Yes` in der Zielkarte. |
| `MapStage` | `Yes` / `No` | `No` | Am Ende erfasst das Spiel den numerischen Fortschrittswert der Karte. `MapStage=Yes` überträgt ihn an die Zielkarte. Missions-IDs kommen aus den Routen. |
| `House.<id>` | `Credits` / `Difficulty` / `Credits,Difficulty` | — | Am Ende werden Geld und/oder Schwierigkeit nach Fraktions-ID erfasst. `House.Americans=Credits,Difficulty` verlangt `Americans` auf beiden Karten. Die Zielkarte skaliert und begrenzt das Geld mit `CarryOverMoney` und `CarryOverCap`. |

## `[Rules <id>]`

| Schlüssel | Werte | Standard | Zeitpunkt und Wirkung |
| --- | --- | --- | --- |
| `Bootstrap` | `Campaign` | `Campaign` | Wählt beim Start die Fraktionen aus der Karte. Explizit `Bootstrap=Campaign`. |
| `AllowWinTags` | `RequireClear` / `Ignore` | `RequireClear` | `RequireClear` wartet bei der Siegauswertung auf den Abbau der AllowWin-Marken. `Ignore` überspringt diese Prüfung. Der Ergebnis-Timer gilt weiterhin. |
| `ComputerParanoid` | `Disabled` / `React` | `Disabled` | `React` lässt bei einer Anfrage zur Feindwahl oder Diplomatie die Computer_Paranoid-Logik zu. Die Fraktions- und Regelprüfungen gelten weiter. |
| `Crates` | `Keep` / `Skip` | `Keep` | Beim Laden der Overlays behält `Keep` platzierte Kisten. `Skip` lässt diese weg. Für zeitgesteuerte Kisten dient `UsesMatchCrates`. |
| `AiBase` | `RetainAuthoredNodes` / `ReplaceUnavailableNodes` | `RetainAuthoredNodes` | Bei der KI-Basisplanung behält `RetainAuthoredNodes` die gesetzten Positionen. `ReplaceUnavailableNodes` kann ungültige oder wiederholt abgelehnte Positionen ersetzen. |
| `DoubleOwned` | `OwnerMask` / `AllCountries` | `OwnerMask` | Bei der Fabriksuche verwendet `OwnerMask` die normale Ländernutzermaske. `AllCountries` weitet sie bei Typen mit `DoubleOwned=Yes` aus. |
| `TeamCreationLimitScope` | `AllHouses` / `PerHouse` | `AllHouses` | Bei der Teamerstellung zählt `Max=` entweder vorhandene Teams aller Fraktionen oder nur die der erstellenden Fraktion. |
| `CountsPlayerControlAsHuman` | `No` | `No` | Die Spielerfraktionen stehen beim Start über die Rollen fest. `CountsPlayerControlAsHuman=No` hält diese Vorgabe ausdrücklich fest. |
| `AllowsConstructionYardRedeploy` | `Yes` / `No` | `No` | Bei einem Abbauauftrag kann ein geeigneter Bauhof als Baufahrzeug ausgefahren werden. Die Prüfung von Bauhof und Spieloption bleibt bestehen. |
| `AllowsIdleObjectSleep` | `Yes` / `No` | `No` | Bei der Bildaktualisierung können geeignete untätige Objekte schlafen, soweit Trigger und Reparatur es erlauben. |
| `AllowsOwnerlessTerrainLogic` | `Yes` / `No` | `Yes` | Bei der Geländeaktualisierung werden besitzlose Objekte einbezogen, die der native Identitätstest sonst überspringt. |
| `UsesCountryDifficultyBiases` | `Yes` / `No` | `No` | Bei der Einstellung des Schwierigkeitsgrads wirken auch die Modifikatoren des gewählten Landes. |
| `SkipsLocalPlayerAuthoredBuildings` | `Yes` / `No` | `No` | Der Kartenlader prüft diesen Schlüssel nur bei lokaler Ausführung. Im Lockstep werden platzierte Gebäude geladen. Entferne ein Gebäude direkt aus der Karte, wenn es fehlen soll. |
| `UsesManagedAiBase` | `Yes` / `No` | `No` | Während der Partie können geeignete KI-Fraktionen automatisch bauen, Standorte wählen und Gebäude einsetzen. |
| `UsesAiEconomy` | `Yes` / `No` | `No` | Bei KI-Wirtschaftsvorgängen gelten die Mehrspielerregeln für Erzbezahlung und zulässige Gebäudeverkäufe. |
| `RunsAiStrategy` | `Yes` / `No` | `No` | Bei der Strategieaktualisierung sucht die KI regelmäßig Gegner und trifft Bau- und Strategieentscheidungen. |
| `UsesHouseTeamRules` | `Yes` / `No` | `No` | Beim Einlesen und Aktivieren der KI-Teams gelten die fraktionsbezogenen Triggerprüfungen für Schwierigkeit und TechLevel. |
| `UsesCampaignDiscovery` | `Yes` / `No` | `Yes` | Bei Sicht und Zielwahl gelten Kampagnenregeln. Der Entdeckungspfad für einen einzelnen Client setzt weiterhin lokale Ausführung voraus. |
| `UsesCampaignBaseDefense` | `Yes` / `No` | `Yes` | Wählt bei einem Basisangriff die Kampagnenregeln für die Reaktion des getroffenen Objekts und der Verteidiger. |
| `UsesDirectedHunterSeekerTargeting` | `Yes` / `No` | `No` | Beim Ares-Jäger kann eine KI-Fraktion ihren festgelegten Gegner wählen. Spielerfraktionen können aktive Ziele bevorzugen, soweit `RandomOnly` es zulässt. |
| `UsesMatchCrates` | `Yes` / `No` | `No` | Während des Spiels gelten Mehrspielerkisten mit Zeitgebern und Prüfungen beim Einsammeln. Automatisches Erscheinen setzt die Kistenoption voraus. |
| `UsesEliminationResults` | `Yes` / `No` | `No` | Bei der Ergebnisauswertung entscheidet die Ausschaltung von Fraktionen und die entsprechende Bereinigung folgt. Bei der Vorgabe können Kartentrigger Sieg und Niederlage setzen. |
| `UsesRoomOptions` | `Yes` / `No` | `No` | Beim Start verwendet die Karte die Raumregeln für Sammlerimmunität und Wachstum und Ausbreitung von Erz. Die Werte kommen dann aus dem Startpfad. |
| `UsesCampaignDiplomacy` | `Yes` / `No` | `Yes` | Beim Klick auf Diplomatie gelten Kampagnenbriefings. Der Schlüssel wählt auch das Kampagnenverhalten für passive und zivile Fraktionen. |
| `AllowsManualAlliances` | `Yes` / `No` | `No` | Beim Bündnisbefehl wird der Auftrag zugelassen, soweit die Regeln und Befehlsrechte ihn erlauben. |
| `ModMapAdditions` | `Yes` / `No` | `No` | Beim Laden im grafischen Client können Kartenergänzungen des aktiven Mods einfließen. Verteile dieselben Moddateien. Setze für die lokale Autorenprüfung benötigte Werte in die Karte. |
| `SharedAlliedVictory` | `Yes` / `No` | `No` | Bei Lockstep-Auswertung nach Ausschaltung können qualifizierte verbündete Überlebende den Sieg teilen. |
| `SkipsNetworkScoreDialog` | `Yes` / `No` | `No` | Überspringt die normale Mehrspieler-Punktetafel bei ihrem Aufruf. Die Kampagnenfortsetzung zeigt den eigenen Ergebnisdialog. |
| `ParanoidIgnoresAlliesAllowed` | `Yes` / `No` | `No` | Wenn `ComputerParanoid=React` die Bündnisprüfung erreicht, kann die KI `AlliesAllowed` übergehen. Befugnisse der Spieler werden getrennt geprüft. |
| `NetworkDiplomacyBriefing` | `Yes` / `No` | `No` | Beim Klick auf Diplomatie erscheinen Briefing und Rollenrechte zum Nachlesen. Beispiel `NetworkDiplomacyBriefing=Yes` und Kartentext unter `[Basic] Briefing`. |
| `StartingHouseRoleAlliances` | `Yes` / `No` | `No` | Beim Aufbau aus Spielerplätzen verbindet die Regel Fraktionen gleicher Spieler- oder KI-Klasse. Lege bei Karten mit festen Fraktionen die Startbündnisse in der Karte fest. |
| `AresPsychicRevealExclusions` | `Yes` / `No` | `No` | Während einer Ares-Psychoenthüllung im Lockstep bleiben die drei kartengebundenen Ausschlussfelder der konkurrierenden Modi verborgen. |
| `DissolveUnfilledTeams` | `Yes` / `No` | `No` | Bei der KI-Teamaktualisierung können leere, nie vollständige Teams nach `DissolveUnfilledTeamDelay` aufgelöst werden. |
