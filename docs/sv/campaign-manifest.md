---
layout: default
title: Kampanjserier
lang: sv
---

# Kampanjserier

[English](../campaign-manifest.md) · [简体中文](../zh-CN/campaign-manifest.md) · [Deutsch](../de/campaign-manifest.md) · [العربية](../ar/campaign-manifest.md) · [Русский](../ru/campaign-manifest.md) · **Svenska** · [ไทย](../th/campaign-manifest.md) · [Türkçe](../tr/campaign-manifest.md)

I `campaigns/<serie>/campaign.ini` anger du kartan för varje uppdrag i serien. Där väljer du också regler, roller, vägar och vad som ska följa med till nästa uppdrag. Börja med en karta.

Kör `asset campaign-check` för att kontrollera ett uppdrag utan att öppna ett rum. För spel kan du öppna ett rum för en person eller dela ut roller till flera spelare. Internetspel kräver en lobby med stöd för kampanjrum. Det vanliga **Kampanj**-valet använder spelets lista i `battlemd.ini`.

[Nyckelreferensen](campaign-manifest-keys.md) anger värden, standardvärden och när varje nyckel används.

## Börja med ett uppdrag

Spara `campaign.ini` som UTF‑8 i `campaigns/broken-line/` och placera kartan under `maps/opening.map` i samma serie. Manifestet får vara högst 64 KiB. Skriv kartans sökväg från seriemappen och använd `/`.

```ini
[Stage Opening]
Map=maps/opening.map
```

Ett uppdrag utan roller använder fraktionen i `[Basic] Player` och har en spelarplats.

## Kontrollera ett uppdrag utan rum

```text
asset campaign-check campaigns/broken-line/campaign.ini Opening 120 --ra2-dir "/path/to/Yuri's Revenge"
```

Ange uppdragets ID och ett positivt antal bildrutor. Verktyget kör uppdraget, bygger upp det igen från en diagnostisk repris och jämför tillståndet bildruta för bildruta. Fel i manifestet anger fil, avsnitt, nyckel och rad, med ett förslag på rättelse. Jämför även kartans inkluderade INI-filer om spelarna får olika resultat vid kontrollen.

## Lägg till roller

För uppdrag med flera roller använder du fraktionsnamnen i kartans `[Houses]`.

```ini
[Roles Opening]
Lead=Americans
Support=Americans

[Grants Opening]
Lead.Russians=FullControl
Support.Russians=UnitsOnly
```

`Lead` och `Support` delar `Americans`. `FullControl` ger även kommandon som kräver full kontroll över `Russians`. `UnitsOnly` ger taktiska kommandon. Sätt `MinPlayers` och `MaxPlayers` för att ange spelarantalet. I `[Routes Opening]` kan `Coast=Finale` leda till ett deklarerat uppdrag. `[CarryOver Opening]` väljer vad som följer med.

## Installera och öppna ett rum

Lägg serien under `campaigns/<serie>/` i spelmappen eller den aktiva modden. Alla spelare behöver samma spelbygge, regler och kampanjfiler. Behåll mappstrukturen när du kopierar serien och ta med de INI-filer som kartorna läser in.

1. Skapa ett LAN-rum och välj kampanjen i listan över spellägen. **Vanligt flerspelarläge** tar dig tillbaka till de vanliga spellägena.
2. Välj uppdrag och svårighetsgrad. Om uppdraget har roller delar du ut en per spelare i kolumnen **Roll**. Två roller kan styra samma fraktion.
3. Starta när alla är redo. Kontrollera kampanjfilerna, rollerna och uppdragets spelargränser om starten nekas.

Internetspel kräver en WOL-lobby som annonserar `campaign_rooms` och skickar kampanjinställningarna vidare vid start och till åskådare. Använd LAN om lobbyn saknar stödet. En anslutning i åskådarläge tar ingen roll och får ingen röst. Frigör vanliga åskådarplatser i rummet före start.

Vid en filmsekvens väntar spelet på att spelarna når samma punkt. Efter uppdraget byter **Annan väg** till nästa valbara mål. **Fortsätt** röstar för målet som visas. **Försök igen** startar om uppdraget med tillståndet vid dess början. Alla spelare måste välja samma alternativ. Åskådare deltar inte i omröstningen.

[Fullständigt INI-format och repriser på engelska](../campaign-manifest.md) · [Nätverksprotokoll på engelska](../Networking.md#multiplayer-campaign-rooms)
