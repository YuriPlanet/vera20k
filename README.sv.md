<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20K huvudbild" width="100%">

<p align="center" dir="ltr">
  <strong>Svenska</strong> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för macOS (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Bibliotekstester för macOS" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för Linux (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Bibliotekstester för Linux" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för Windows (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Bibliotekstester för Windows" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Gå med i Discord-servern" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_grupp-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ-grupp" height="23" align="middle">
</p>

# VERA20K

VERA20K är en communitydriven nyimplementation av originalmotorn, `gamemd.exe`. Den använder
de ursprungliga spelfilerna, så du behöver en egen kopia av Red Alert 2: Yuri's Revenge.

VERA20K görs av spelare, för spelare, och det är spelarna som har sista ordet om vart projektet
ska gå.

**Vi behöver fler underhållare.** Om du vill hjälpa till att granska PR:er, stötta nya
medverkande eller ta hand om en del av motorn, säg till på [Discord](https://discord.gg/kmjRUn5m5F) eller i
QQ-gruppen (1097193563).

**Var inte rädd för att skicka in en PR.** Den behöver inte vara perfekt; vi hjälper till med resten.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K: inställningar för skirmish och bild från spelet" width="100%">

## Projektets mål

1. Bevara spelmekaniken, utseendet och stämningen i originalversionen av Red Alert 2: Yuri's Revenge.
2. Stödja större slag: upp till **30 spelare** och **20 000 enheter** på 500×500-kartor.
3. Integrera kända RTS-funktioner och några som aldrig setts förut.
4. Integrerad flerspelarklient

Se [färdplanen](https://github.com/orgs/YuriPlanet/projects/1) för aktuella prioriteringar och framtida planer.

## Ladda ner och spela

Ladda ner installationsprogrammet för Windows, macOS eller Linux från
[releasesidan](https://github.com/YuriPlanet/vera20k/releases). Det hittar ditt Yuri's Revenge och
installerar VERA20K i en ny mapp.

För att starta skirmish-matcher eller kampanjer i YR för en spelare från en CnCNet-klient, se
[installationen av offlineklienten](integrations/cncnet/README.md).

## Bidra

All hjälp är välkommen, och de här områdena behöver den mest. Varje område länkar till sin sida
i färdplanen, med pågående arbete och nästa steg:

- **[Spelmekanik och strid](https://github.com/YuriPlanet/vera20k/issues/3830):** enheternas beteende, skada och pålitliga skirmish-matcher.
- **[Prestanda](https://github.com/YuriPlanet/vera20k/issues/3831):** bildfrekvens och respons i stora slag och på äldre hårdvara.
- **[Flerspelarläge](https://github.com/YuriPlanet/vera20k/issues/3832):** att gå med i, starta och hålla igång onlinematcher, och pålitligt samarbetsspel.
- **[Gränssnitt och ljud](https://github.com/YuriPlanet/vera20k/issues/3833):** menyer, text, översättningar och ljud.
- **[Kampanjer](https://github.com/YuriPlanet/vera20k/issues/3834):** uppdragen startar, men att spela dem till slutet kräver mer arbete: mål, triggers, skriptade övergångar, spara/ladda och övergången till nästa uppdrag.
- **[Moddkompatibilitet](https://github.com/YuriPlanet/vera20k/issues/3835):** funktioner från Ares och Phobos, utan att ändra originalspelets standardbeteende.
- **[Testning](https://github.com/YuriPlanet/vera20k/issues?q=is%3Aopen%20label%3Aparity):** spela VERA20K sida vid sida med originalet och rapportera det som spelar annorlunda.

Medverkande kan ta ansvar för delar av motorn, med stor frihet att göra ändringar som driver
VERA20K framåt.

Underhållarna hjälper till med granskning, integrering och sammanslagning. Du får fullt
erkännande för ditt bidrag.

Diskussionen sker på [Discord](https://discord.gg/kmjRUn5m5F) och i QQ-gruppen (1097193563).

## Bygg och kör

Du behöver:

- **Rust och Git:** [Rust](https://rustup.rs/) 1.97 eller senare, installerad med rustup, och
  [Git](https://git-scm.com/).
- **Byggverktyg:** på Windows Visual Studios C++-byggverktyg, som Rusts installationsprogram
  erbjuder sig att installera; på macOS `xcode-select --install`; på Debian och Ubuntu
  `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Redigera config.toml och ange din spelmapp i ra2_dir innan du kör:
cargo run --release --bin vera20k
```

## Så arbetar vi

Större delen av koden skrivs av AI-agenter. De använder Ghidra för att studera originalmotorn,
portar sedan dess beteende till Rust och kontrollerar det med
[jämförelseverktyg](tools/native_oracle.md) och speltester. Agenterna följer
[AGENTS.md](AGENTS.md).

## Anpassning

Kartkapaciteten väljs automatiskt utan byggflagga: summan av bredd och höjd i `Size` får vara högst 1024.

VERA20K fungerar som standard precis som originalspelet. Varje annan ny funktion, även tillägg från
moddar som Ares, måste vara avstängd tills den slås på med något av följande:

- **En spelarinställning i VERA**, under **Options**, som sparas i avsnittet `[VERA]` i `VERA.INI`.
- **Ett byggalternativ**, till exempel `--features ares` för de [Ares-tillägg](docs/ares.md) som stöds.
- **En INI-nyckel**, som sätts av en mod eller en karta.

Inställningar för ljud, grafik och kontroller sparas i `VERA.INI`. Spelreglerna kommer från
lobbyinställningar, moddar eller kartor.

## Stöd projektet

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="Stöd VERA20K via PayPal" height="23"></a>

Alla mottagna donationer fördelas till
medverkande enligt [TIBTRE:s bidragsregister](https://github.com/YuriPlanet/tibtre-ledger).

Utbetalningar till medverkande: PayPal, China UnionPay, Alipay, Vipps, SEPA eller Bitcoin.

## Tack och juridisk information

Tack till OpenRA, XCC Mixer, ModEnc-wikin, Project Perfect Mod, EA:s GPL-utgåva av källkoden till
Command & Conquer och Red Alert, World-Altering Editor, Final Alert, YRpp, Ares, Phobos och
många andra.

Copyright © 2026 Halvor Nikolai Rusten och VERA20K:s bidragsgivare.
Licensierat under [GPLv3](LICENSE-GPL) med [ytterligare villkor](LEGAL.md#additional-terms).
Det här repot innehåller inga spelfiler. Command &
Conquer och Red Alert är varumärken som tillhör Electronic Arts Inc., och skärmbilderna och
appikonen, Red Alert 2:s röda stjärna, visar spelgrafik som ägs av Electronic Arts. VERA20K har
ingen koppling till och stöds inte av Electronic Arts.

**Läs [den juridiska informationen](LEGAL.md) innan du delar, säljer eller forkar VERA20K.**

## Dokumentation

- [WebAssembly-förhandsversion](docs/wasm.md): ett valfritt enspelarbygge som körs i webbläsaren.
- [INI-profiler](docs/ini-roots.md): välja regel-, grafik- och AI-filer.
- [Ares](docs/ares.md), [Phobos](docs/phobos.md) och [NPatch](docs/npatch.md): stöd för tilläggen och deras begränsningar.
- [Motoröversikt](docs/index.md): kodens upplägg och hur motorns delar hänger ihop.
- [Utvecklarverktyg](tools/README.md): byggen, tester och granskning av spelresurser.
- [Karttester](tools/map_observation.md): skriptade scenarier, skärmbilder och observationer av matcher.
- [Jämförelser med originalet](tools/native_oracle.md): jämföra beteendet med originalspelet.
- [Arbetsflöde i Ghidra](docs/ghidra-workflow.md): undersöka originalmotorn.

Översättning av [README.md](README.md); håll den uppdaterad i takt med den engelska versionen.
