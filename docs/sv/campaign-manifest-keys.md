---
layout: default
title: Nycklar i kampanjmanifestet
lang: sv
---

# Nycklar i kampanjmanifestet

[English](../campaign-manifest-keys.md) · [简体中文](../zh-CN/campaign-manifest-keys.md) · [Deutsch](../de/campaign-manifest-keys.md) · [العربية](../ar/campaign-manifest-keys.md) · [Русский](../ru/campaign-manifest-keys.md) · **Svenska** · [ไทย](../th/campaign-manifest-keys.md) · [Türkçe](../tr/campaign-manifest-keys.md)

Byt ut `<id>` mot uppdragets ID. Booleska nycklar tar `Yes`, `No`, `True`, `False`, `1` eller `0`. Tabellerna visar standardvärden för uppdrag i manifestet. Ett streck betyder att nyckeln saknas. Utelämna valfria uppgifter som du inte använder. [Installation, exempel och kontroll](campaign-manifest.md)

## `[Campaign]`

| Nyckel | Värden | Standard | När och hur nyckeln används |
| --- | --- | --- | --- |
| `Title` | Text | Seriemappen | Klienten visar namnet i listan över spellägen när den läser installerade serier. Exempel `Title=Broken Line`. |
| `Title.<locale>` | Text | `Title` | När serielistan byggs väljer klienten titeln på gränssnittets språk. Exempel `Title.sv=Bruten linje`. Därefter provas basspråket och `Title`. |

## `[Stage <id>]`

| Nyckel | Värden | Standard | När och hur nyckeln används |
| --- | --- | --- | --- |
| `Map` | Relativ sökväg | Obligatoriskt | Rummet och kartladdaren öppnar filen i seriemappen. Exempel `Map=maps/opening.map`. Filen måste ligga kvar inom serien när sökvägen löses. |
| `MapSha256` | 64 hexsiffror | — | Kartkontrollen jämför filens SHA-256 med värdet. Deltagarna jämför kartfiler också när nyckeln saknas. |
| `MinPlayers` | 1–128 | `1` | Minst så många spelare måste vara med före start. Sätt `MinPlayers=2` om båda rollerna krävs. Åskådare räknas inte. |
| `MaxPlayers` | 1–128 | Roll- och rumsgräns | Vid anslutning gäller det minsta av värdet, antalet roller och rumsgränsen. Ett uppdrag utan roller har en plats. |
| `Title` | Text | `ID` | Uppdragsväljaren och resultatskärmen visar titeln. Exempel `Title=Genombrott`. |
| `Title.<locale>` | Text | `Title` | Uppdragsväljaren och resultatet använder titeln för gränssnittets språk. Exempel `Title.sv=Genombrott`. Reservordningen är densamma som för `[Campaign] Title.<locale>`. |

## `[Roles <id>]`

| Nyckel | Värden | Standard | När och hur nyckeln används |
| --- | --- | --- | --- |
| `RoleId=HouseId` | Fraktions-ID på kartan | `[Basic] Player` för en spelare | Vid start knyts rollen till en fraktion i kartans `[Houses]`. `Lead=Americans` och `Support=Americans` delar huvudfraktion. Varje roll tilldelas en spelare. |

## `[Grants <id>]`

| Nyckel | Värden | Standard | När och hur nyckeln används |
| --- | --- | --- | --- |
| `RoleId.HouseId` | `FullControl` / `UnitsOnly` | — | Vid start ger `FullControl` taktiska kommandon och kommandon som kräver full kontroll över en annan fraktion. `UnitsOnly` ger taktiska kommandon. Exempel `Lead.Russians=FullControl`. Huvudfraktionens egna kommandon hör till dess roll. |

## `[Routes <id>]`

| Nyckel | Värden | Standard | När och hur nyckeln används |
| --- | --- | --- | --- |
| `BranchId=StageId` | Deklarerat uppdrags-ID | — | Manifestläsningen slår upp målet. `Coast=Finale` erbjuder det deklarerade slutuppdraget efter matchen. `Retry` får bara peka på det aktuella uppdraget. **Försök igen** använder starttillståndet också när nyckeln saknas. |

## `[CarryOver <id>]`

| Nyckel | Värden | Standard | När och hur nyckeln används |
| --- | --- | --- | --- |
| `Globals` | `Yes` / `No` | `No` | Vid fortsatt spel sparas globala triggervariabler och återställs före nästa kartas första bildruta. |
| `Countdown` | `Yes` / `No` | `No` | Vid uppdragets slut sparas nedräkningen. Sätt `Countdown=Yes` och ange `[Basic] TimerInherit=Yes` på målkartan. |
| `MapStage` | `Yes` / `No` | `No` | Vid uppdragets slut sparas kartans numeriska kampanjvärde. `MapStage=Yes` för det till nästa karta. Målets ID väljs via vägen. |
| `House.<id>` | `Credits` / `Difficulty` / `Credits,Difficulty` | — | Vid uppdragets slut hämtas pengar, svårighet eller båda för den namngivna fraktionen. `House.Americans=Credits,Difficulty` kräver `Americans` i båda kartorna. Målkartans `CarryOverMoney` och `CarryOverCap` styr beloppet. |

## `[Rules <id>]`

| Nyckel | Värden | Standard | När och hur nyckeln används |
| --- | --- | --- | --- |
| `Bootstrap` | `Campaign` | `Campaign` | Vid uppstart byggs fraktionerna från kartan. Skriv `Bootstrap=Campaign` för att ange det uttryckligen. |
| `AllowWinTags` | `RequireClear` / `Ignore` | `RequireClear` | Vid segerkontrollen väntar `RequireClear` på att AllowWin-antalet går till noll. `Ignore` hoppar över den kontrollen. Resultattimern gäller ändå. |
| `ComputerParanoid` | `Disabled` / `React` | `Disabled` | Vid omprövning av fiender eller förbund släpper `React` fram Computer_Paranoid-logiken. Andra regel- och fraktionskontroller gäller. |
| `Crates` | `Keep` / `Skip` | `Keep` | Vid overlayladdning sparar `Keep` utplacerade lådor. `Skip` hoppar över dem. Tidsstyrda lådor styrs av `UsesMatchCrates`. |
| `AiBase` | `RetainAuthoredNodes` / `ReplaceUnavailableNodes` | `RetainAuthoredNodes` | När AI:n bearbetar basplanen behåller `RetainAuthoredNodes` utplacerade noder. `ReplaceUnavailableNodes` kan ersätta otillgängliga eller upprepat avvisade noder. |
| `DoubleOwned` | `OwnerMask` / `AllCountries` | `OwnerMask` | Vid fabriksval använder `OwnerMask` den vanliga landsmasken. `AllCountries` utökar masken för typer med `DoubleOwned=Yes`. |
| `TeamCreationLimitScope` | `AllHouses` / `PerHouse` | `AllHouses` | Vid teamskapande räknas befintliga team mot TeamTypes `Max=` för alla fraktioner eller bara den skapande fraktionen. |
| `CountsPlayerControlAsHuman` | `No` | `No` | Vid start bestämmer de antagna rollerna vilka fraktioner som är mänskliga. Skriv `CountsPlayerControlAsHuman=No` för att ange det uttryckligen. |
| `AllowsConstructionYardRedeploy` | `Yes` / `No` | `No` | Vid en order om att fälla ihop en byggplats tillåts åtgärden för en lämplig byggplats. Byggnadsvillkor och spelalternativ gäller. |
| `AllowsIdleObjectSleep` | `Yes` / `No` | `No` | Vid bilduppdatering får behöriga inaktiva objekt vila enligt trigger- och reparationskontrollerna. |
| `AllowsOwnerlessTerrainLogic` | `Yes` / `No` | `Yes` | Vid terränguppdatering tas ägarlösa objekt med som det vanliga identitetstestet annars hoppar över. |
| `UsesCountryDifficultyBiases` | `Yes` / `No` | `No` | När fraktionens svårighet ställs in kombineras den med det valda landets modifierare. |
| `SkipsLocalPlayerAuthoredBuildings` | `Yes` / `No` | `No` | Vid byggnadsladdning används nyckeln bara för lokal körning. Lockstep laddar kartans placerade byggnader. Ändra kartan för att ta bort en byggnad. |
| `UsesManagedAiBase` | `Yes` / `No` | `No` | Under spelet får lämpliga AI-fraktioner bygga, välja basplats och placera byggnader automatiskt. |
| `UsesAiEconomy` | `Yes` / `No` | `No` | Vid AI-ekonomi gäller flerspelarregler för malmbetalning och tillåten försäljning av byggnader. |
| `RunsAiStrategy` | `Yes` / `No` | `No` | Vid strategiuppdateringen söker AI:n regelbundet efter fiender och fattar egna bygg- och strategibeslut. |
| `UsesHouseTeamRules` | `Yes` / `No` | `No` | När AI-team laddas och aktiveras används det fraktionsbaserade triggerförloppet för svårighet och TechLevel. |
| `UsesCampaignDiscovery` | `Yes` / `No` | `Yes` | Vid sikt och målsökning används kampanjens upptäcktsregler. Engångsklientens upptäckt förutsätter lokal körning. |
| `UsesCampaignBaseDefense` | `Yes` / `No` | `Yes` | När en bas angrips används kampanjregler för det träffade objektets reaktion och för behöriga försvarare. |
| `UsesDirectedHunterSeekerTargeting` | `Yes` / `No` | `No` | Vid en Ares-sökares start får AI:n använda sin utpekade fiende. Spelarfraktioner kan föredra aktiva mål när `RandomOnly` tillåter det. |
| `UsesMatchCrates` | `Yes` / `No` | `No` | Under uppdraget används flerspelarlådornas tidsluckor och kontroller när någon tar en låda. Automatisk placering kräver lådalternativet. |
| `UsesEliminationResults` | `Yes` / `No` | `No` | Vid resultatet avgörs utfallet genom att fraktioner slås ut och städas bort. Med standardvärdet kan kartans trigger bestämma seger eller förlust. |
| `UsesRoomOptions` | `Yes` / `No` | `No` | Vid konstruktion används rummets startflaggor för skördarimmunitet samt malmtillväxt och spridning. |
| `UsesCampaignDiplomacy` | `Yes` / `No` | `Yes` | Diplomatiknappen öppnar kampanjbriefingen. Nyckeln väljer också kampanjens diplomatiförlopp för passiva eller civila fraktioner. |
| `AllowsManualAlliances` | `Yes` / `No` | `No` | Vid en alliansorder tas ordern emot när alliansreglerna och spelarens befogenhet tillåter den. |
| `ModMapAdditions` | `Yes` / `No` | `No` | Före grafisk kartladdning kan den aktiva moddens karttillägg användas. Dela samma modfiler. Lägg värden som behövs vid lokal kontroll i kartan. |
| `SharedAlliedVictory` | `Yes` / `No` | `No` | När resultatet avgörs genom utslagning i lockstep kan behöriga allierade överlevare dela segern. |
| `SkipsNetworkScoreDialog` | `Yes` / `No` | `No` | När spelet når resultattavlan för flerspelare hoppas den över. Kampanjens fortsättning visar en egen resultatvy. |
| `ParanoidIgnoresAlliesAllowed` | `Yes` / `No` | `No` | När `ComputerParanoid=React` prövar en allians kan AI:n kringgå `AlliesAllowed`. Spelarkommandon kontrolleras separat. |
| `NetworkDiplomacyBriefing` | `Yes` / `No` | `No` | Diplomatiknappen öppnar briefingen och visar rollernas befogenheter. Exempel `NetworkDiplomacyBriefing=Yes` och kartans `[Basic] Briefing`. |
| `StartingHouseRoleAlliances` | `Yes` / `No` | `No` | Vid uppstart från rumsplatser förenas fraktioner med samma klass, människa eller AI. För fasta kampanjfraktioner anges allianserna på kartan. |
| `AresPsychicRevealExclusions` | `Yes` / `No` | `No` | Vid Ares psykiska avslöjande i lockstep undantas tre kartrelativa rutor på samma sätt som i tävlingsläget. |
| `DissolveUnfilledTeams` | `Yes` / `No` | `No` | Vid uppdatering av AI-team kan tomma team som aldrig fylldes lösas upp efter `DissolveUnfilledTeamDelay`. |
