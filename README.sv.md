<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k huvudbild" width="100%">

<p align="center" dir="ltr">
  <strong>Svenska</strong> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för macOS (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Bibliotekstester för macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för Linux (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Bibliotekstester för Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för Windows (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Bibliotekstester för Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Gå med i Discord-servern" height="20" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_grupp-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ-grupp" height="20" align="middle">
</p>

# VERA20k

VERA20k är en communitydriven nyimplementation av originalmotorn, `gamemd.exe`. Den använder
de ursprungliga spelfilerna, så du behöver en egen kopia av Red Alert 2: Yuri's Revenge.

VERA20k görs av spelare, för spelare, och det är spelarna som har sista ordet om vart projektet
ska gå.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k: inställningar för skirmish och bild från spelet" width="100%">

## Projektets mål

1. Bevara spelmekaniken, utseendet och stämningen i originalversionen av Red Alert 2: Yuri's Revenge.
2. Stödja större slag: upp till **30 spelare** och **20 000 enheter** på 500×500-kartor.
3. Integrera kända RTS-funktioner och några som aldrig setts förut.
4. Integrerad flerspelarklient

## Aktuellt läge

**Under aktiv utveckling.**

- **Skirmish:** Går att spela på Windows, Linux och macOS, med kartor från originalspelet
  och slumpgenererade kartor, basbygge, resursinsamling, strider samt möjlighet att spara och ladda spel.
- **Kampanjer:** Uppdrag går att starta; stödet är fortfarande ofullständigt.
- **AI:** Delar av originalets AI sköter basbygge och skriptstyrda anfall.
- **Pågående arbete:** Spelbeteende, grafik och prestanda.
- **Flerspelarläge:** Nätverksspel är ännu inte tillgängligt.
- **Skala:** Slag med 30 spelare och 20 000 enheter har ännu inte demonstrerats.

## Arkitektur

Motorn har tre huvuddelar:

- **Simulering:** Ansvarar för speltillstånd och spellogik.
- **Rendering:** En helt ny GPU-renderingspipeline som använder originalets spelresurser,
  byggd för att bevara originalets utseende.
- **Applikation:** Hanterar inmatning, menyer, ljud och plattformsintegration och kopplar ihop systemen.

Denna uppdelning stöder deterministiskt spel.

Pilarna visar dataflödet:

![Applikationen skickar kommandon till simuleringen, som returnerar världstillståndet; applikationen skickar renderingsdata till renderingen.](docs/images/architecture.sv.png)

## Bygg och kör

Du behöver:

- **Spelet:** Red Alert 2: Yuri's Revenge.
- **Rust och Git:** den senaste stabila versionen av [Rust](https://rustup.rs/), installerad med
  rustup, och [Git](https://git-scm.com/).
- **Byggverktyg:** på Windows Visual Studios C++-byggverktyg, som Rusts installationsprogram
  erbjuder sig att installera; på macOS `xcode-select --install`; på Debian och Ubuntu
  `sudo apt install build-essential libasound2-dev pkg-config`.
- **Grafik:** ett grafikkort med Vulkan, DirectX 12 eller Metal.

VERA20k har spelats på Windows, Linux och macOS.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Redigera config.toml och ange din spelmapp i ra2_dir innan du kör:
cargo run --release --bin vera20k
```

Skriv `ra2_dir` med vanliga snedstreck, till exempel `C:/Games/RA2`. Använd `--release` när du
spelar; debugbyggen är för långsamma. Loggen hamnar i `logs/ra2.log`.

## Så arbetar vi

Större delen av koden skrivs av AI-agenter. De använder Ghidra för att studera originalmotorn,
portar sedan dess beteende till Rust och kontrollerar det med
[jämförelseverktyg](tools/native_oracle.md) och speltester. Agenterna följer
[AGENTS.md](AGENTS.md).

## Anpassning

Valfria funktioner (t.ex. från Ares) ska kunna konfigureras via INI-filer, med möjlighet
att slå på och av dem i klienten där det är lämpligt.

## Bidra

All hjälp är välkommen. Du kan skriva kod, refaktorera motorn, testa spelet, dela idéer eller
spela det sida vid sida med originalet och berätta vad som känns fel. Öppna en PR så tar vi det
därifrån. Diskussionen sker på [Discord](https://discord.gg/kmjRUn5m5F) och i QQ-gruppen (1097193563).

Spelmekaniken finns i `src/sim/`, renderingen i `src/render/` och menyer och inmatning i
`src/app/` och `src/ui/`; Python-verktygen i `tools/` behöver du inte.

Speltesta dina ändringar med `cargo run --profile fast --bin vera20k`. Den är optimerad som
`--release` men bygger bara om det du har ändrat, så du kommer snabbare tillbaka till spelet.
Mät prestanda med `--release`.

Bidrag licensieras under GPLv3, precis som resten av projektet; det finns inget CLA.

## Stöd projektet <a href="https://paypal.me/vera20k"><img src="docs/images/paypal-support.svg" alt="Stöd VERA20k via PayPal" height="40" align="middle"></a>

[Stöd VERA20k via PayPal](https://paypal.me/vera20k). Alla mottagna donationer fördelas till
medverkande enligt [TIBTRE:s bidragsregister](https://github.com/YuriPlanet/tibtre-ledger).

Utbetalningar till medverkande görs via PayPal, UnionPay (Kina), Alipay, Vipps,
SEPA-banköverföring eller Bitcoin.

## Tack och juridisk information

Tack till OpenRA, XCC Mixer, ModEnc-wikin, Project Perfect Mod, EA:s GPL-utgåva av källkoden till
Command & Conquer och Red Alert, World-Altering Editor, Final Alert, YRpp, Ares, Phobos och
många andra.

Licensierat under [GPLv3](LICENSE-GPL). Det här repot innehåller inga spelfiler. Command &
Conquer och Red Alert är varumärken som tillhör Electronic Arts Inc., och skärmbilderna visar
spelgrafik som ägs av Electronic Arts. VERA20k har ingen koppling till och stöds inte av Electronic Arts.

Översättning av [README.md](README.md); håll den uppdaterad i takt med den engelska versionen.
