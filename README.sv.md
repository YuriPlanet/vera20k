<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k huvudbild" width="100%">

<p align="center" dir="ltr">
  <strong>Svenska</strong> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.ru.md" lang="ru">Русский</a> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för macOS (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Bibliotekstester för macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för Linux (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Bibliotekstester för Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Senaste körningen av bibliotekstesterna för Windows (startas manuellt)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Bibliotekstester för Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Gå med i Discord-servern" height="20" align="middle"></a>
</p>

# VERA20k

Red Alert 2: Yuri's Revenge — återskapat i Rust för stora flerspelarslag.

VERA20k är en nyimplementation av originalmotorn, `gamemd.exe`. Den använder de ursprungliga
spelfilerna, så du behöver en egen kopia av Red Alert 2: Yuri's Revenge. Spelet ingår i *Command & Conquer
The Ultimate Collection* på [Steam](https://store.steampowered.com/bundle/39394/) och
[EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc).

VERA20k görs av spelare, för spelare, och det är spelarna som har sista ordet om vart projektet
ska gå.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k: inställningar för skirmish och bild från spelet" width="100%">

## Projektets mål

1. Bevara spelmekaniken, utseendet och stämningen i originalversionen av Red Alert 2: Yuri's Revenge.
2. Stödja större slag: upp till **30 spelare** och **20 000 enheter** på större kartor.
3. Integrera nya RTS-funktioner.
4. Integrerad flerspelarklient

## Aktuellt läge

**Tidig utveckling.** Lokala skirmishmatcher går att spela på Windows mot en enkel AI.
Kartor från originalspelet och slumpgenererade kartor, menyer, basbygge, resursinsamling,
strider samt möjligheten att spara och ladda spel finns på plats, men mycket återstår
att fixa och färdigställa.

Flerspelarläge, kampanjer och originalets AI saknas fortfarande. Flygplan, sinneskontroll,
broar och flera vapen och effekter behöver mer arbete. Vi har ännu inte demonstrerat slag
med 30 spelare och 20 000 enheter.

Vanliga byggnader, landskapsobjekt och enheter går nu in i ritningen innan deras ankarruta har utforskats, så att krigsdimman gradvis visar deras pixlar. Särskilda ritvägar behöver fortfarande jämföras med originalet.

## Bygg och kör

Du behöver Rust 1.88 eller senare, ett grafikkort med Vulkan, DirectX 12 eller Metal,
och spelet installerat. VERA20k har spelats på Windows, Linux och macOS.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Redigera config.toml och ange din spelmapp i ra2_dir innan du kör:
cargo run --release --bin vera20k
```

Använd `--release` när du spelar; debugbyggen är för långsamma. Se
[CONTRIBUTING.md](CONTRIBUTING.md#set-up) för plattformsspecifika förberedelser och hur du kör testerna.

## Så arbetar vi

Större delen av koden skrivs av AI-agenter som jag leder. Vi använder Ghidra för att studera
originalmotorn, portar sedan dess beteende till Rust och kontrollerar det med
[jämförelseverktyg](tools/native_oracle.md) och speltester. Arbetsreglerna finns i
[AGENTS.md](AGENTS.md).

## Bidra

All hjälp är välkommen. Du kan skriva kod, testa spelet, förbättra dokumentationen eller spela
det sida vid sida med originalet och berätta vad som känns fel. Du behöver inte ha erfarenhet
av reverse engineering för att hjälpa till.

Läs [CONTRIBUTING.md](CONTRIBUTING.md), titta på
[bra första uppgifter](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue) eller säg
hej på [Discord](https://discord.gg/kmjRUn5m5F).
[Arkitekturöversikten](https://yuriplanet.github.io/vera20k/sv/) förklarar hur motorn hänger ihop.

## Tack och juridisk information

Tack till OpenRA, XCC Mixer, ModEnc-wikin, Project Perfect Mod, EA:s GPL-utgåva av källkoden till
Command & Conquer och Red Alert, World-Altering Editor, Final Alert, YRpp, Ares, Phobos och
många andra.

Licensierat under [GPLv3](LICENSE-GPL). Det här repot innehåller inga spelfiler. Command &
Conquer och Red Alert är varumärken som tillhör Electronic Arts Inc., och skärmbilderna visar
spelgrafik som ägs av Electronic Arts. VERA20k har ingen koppling till och stöds inte av Electronic Arts.

Översättning av [README.md](README.md); håll den uppdaterad i takt med den engelska versionen.
