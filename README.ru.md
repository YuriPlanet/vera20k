<img src="docs/images/new-conscirpt-hero-image.png" alt="Заглавное изображение VERA20K" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <a href="README.ja.md" lang="ja">日本語</a> · <strong>Русский</strong> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
</p>
<p align="center" dir="ltr">
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Последний запуск тестов библиотеки на macOS (запускаются вручную)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Тесты библиотеки на macOS" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Последний запуск тестов библиотеки на Linux (запускаются вручную)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Тесты библиотеки на Linux" height="23" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Последний запуск тестов библиотеки на Windows (запускаются вручную)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Тесты библиотеки на Windows" height="23" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Присоединиться к Discord" height="23" align="middle"></a>
  <img src="https://img.shields.io/badge/QQ_группа-1097193563-1EBAFC?style=flat&amp;logo=qq&amp;logoColor=white" alt="QQ-группа" height="23" align="middle">
</p>

# VERA20K

VERA20K — новая реализация оригинального движка `gamemd.exe`, которую развивает сообщество.
Она использует оригинальные файлы игры, поэтому тебе понадобится собственная копия
Red Alert 2: Yuri's Revenge.

VERA20K делают игроки для игроков, и последнее слово в том, куда движется проект, остаётся за
игроками.

**Нам нужно больше мейнтейнеров.** Если хочешь помогать с ревью PR, поддерживать новых
участников или взять на себя часть движка, напиши об этом в [Discord](https://discord.gg/kmjRUn5m5F) или
в QQ-группе (1097193563).

**Не бойся отправлять PR.** Он не обязан быть идеальным: с остальным мы поможем.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K: настройки схватки и кадр из игры" width="100%">

## Цели проекта

1. Сохранить игровой процесс, графику и атмосферу оригинальной Red Alert 2: Yuri's Revenge.
2. Поддерживать более крупные сражения: до **30 игроков** и **20 000 юнитов** на картах 500×500.
3. Добавлять известные возможности RTS, а также такие, каких ещё не было.
4. Встроенный клиент для мультиплеера

Текущие приоритеты и будущие планы — в [дорожной карте](https://github.com/orgs/YuriPlanet/projects/1).

## Скачать и играть

Скачай установщик для Windows, macOS или Linux на
[странице выпусков](https://github.com/YuriPlanet/vera20k/releases). Он сам найдёт твою копию
Yuri's Revenge и установит VERA20K в новую папку.

Чтобы запускать одиночные схватки или кампании YR через клиент CnCNet, см.
[настройку офлайн-клиента](integrations/cncnet/README.md).

## Как помочь

Мы рады помощи в любой области, а больше всего она нужна здесь. Каждый пункт ведёт на свою
страницу в дорожной карте, где описаны текущая работа и следующий шаг:

- **[Игровой процесс и бои](https://github.com/YuriPlanet/vera20k/issues/3830):** поведение юнитов, урон и стабильность обычных схваток.
- **[Производительность](https://github.com/YuriPlanet/vera20k/issues/3831):** частота кадров и отзывчивость в больших сражениях и на старом железе.
- **[Мультиплеер](https://github.com/YuriPlanet/vera20k/issues/3832):** подключение к онлайн-матчам, их запуск и продолжение, а также надёжность совместной игры.
- **[Интерфейс и звук](https://github.com/YuriPlanet/vera20k/issues/3833):** меню, тексты, переводы и звук.
- **[Кампании](https://github.com/YuriPlanet/vera20k/issues/3834):** миссии запускаются, но чтобы пройти их до конца, нужна работа: цели, триггеры, скриптовые переходы, сохранение и загрузка, переход к следующей миссии.
- **[Совместимость с модами](https://github.com/YuriPlanet/vera20k/issues/3835):** возможности Ares и Phobos без изменения стандартного поведения оригинальной игры.
- **[Тестирование](https://github.com/YuriPlanet/vera20k/issues?q=is%3Aopen%20label%3Aparity):** играть в VERA20K параллельно с оригиналом и сообщать, что работает иначе.

Участники могут брать на себя ответственность за отдельные части движка и с большой свободой
вносить изменения, которые двигают VERA20K вперёд.

Мейнтейнеры помогут с ревью, интеграцией и слиянием. Авторство вклада полностью остаётся
за тобой.

Обсуждение идёт в [Discord](https://discord.gg/kmjRUn5m5F) и в QQ-группе (1097193563).

## Сборка и запуск

Понадобится:

- **Rust и Git:** [Rust](https://rustup.rs/) 1.97 или новее, установленный через rustup, и
  [Git](https://git-scm.com/).
- **Инструменты сборки:** на Windows — C++ Build Tools из Visual Studio, которые предложит
  установить установщик Rust; на macOS — `xcode-select --install`; в Debian и Ubuntu —
  `sudo apt install build-essential libasound2-dev libxkbcommon-x11-0 pkg-config`.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Перед запуском отредактируй config.toml и укажи папку с игрой в ra2_dir:
cargo run --release --bin vera20k
```

## Как мы работаем

Большую часть кода пишут ИИ-агенты для программирования. Они изучают оригинальный движок в
Ghidra, затем переносят его поведение на Rust и проверяют его
[инструментами сравнения](tools/native_oracle.md) и игровыми тестами. Агенты следуют
[AGENTS.md](AGENTS.md).

## Настройка

Размер сетки выбирается автоматически без флага сборки: сумма ширины и высоты `Size` может достигать 1024.

По умолчанию VERA20K ведёт себя так же, как оригинальная игра. Каждая другая новая функция, включая
расширения из модов вроде Ares, должна оставаться выключенной, пока её не включат одним из способов:

- **Пользовательским параметром VERA** в **Options**, который сохраняется в секции `[VERA]` файла `VERA.INI`.
- **Параметром сборки**, например `--features ares` для поддерживаемых [расширений Ares](docs/ares.md).
- **Ключом INI**, который задаёт мод или карта.

Настройки звука, графики и управления сохраняются в `VERA.INI`. Правила игры задаются
настройками лобби, модами или картами.

## Поддержка проекта

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="Поддержать VERA20K через PayPal" height="23"></a>

Все полученные пожертвования
распределяются между участниками проекта согласно
[журналу вкладов TIBTRE](https://github.com/YuriPlanet/tibtre-ledger).

Участники получают выплаты через PayPal, China UnionPay, Alipay, Vipps, SEPA или Bitcoin.

## Благодарности и правовая информация

Спасибо OpenRA, XCC Mixer, вики ModEnc, Project Perfect Mod, EA за публикацию исходного кода
Command & Conquer и Red Alert под GPL, World-Altering Editor, Final Alert, YRpp, Ares,
Phobos и многим другим.

© 2026 Halvor Nikolai Rusten и участники проекта VERA20K.
Проект распространяется под лицензией [GPLv3](LICENSE-GPL) с [дополнительными условиями](LEGAL.md#additional-terms).
В этом репозитории нет файлов игры.
Command & Conquer и Red Alert — товарные знаки Electronic Arts Inc., а на скриншотах и значке
приложения (красной звезде из Red Alert 2) показана игровая графика, принадлежащая Electronic Arts.
VERA20K не связан с Electronic Arts и не одобрен компанией.

**Прочитай [юридическое уведомление](LEGAL.md), прежде чем распространять, продавать или форкать VERA20K.**

## Документация

- [Предварительная версия на WebAssembly](docs/wasm.md): необязательная однопользовательская сборка, которая работает в браузере.
- [Профили INI](docs/ini-roots.md): выбор файлов правил, графики и ИИ.
- [Ares](docs/ares.md), [Phobos](docs/phobos.md) и [NPatch](docs/npatch.md): поддержка расширений и её ограничения.
- [Обзор движка](docs/index.md): структура кода и то, как части движка работают вместе.
- [Инструменты разработчика](tools/README.md): сборки, тесты и изучение игровых ресурсов.
- [Тестирование карт](tools/map_observation.md): сценарии на скриптах, скриншоты и наблюдения за матчами.
- [Сравнение с оригиналом](tools/native_oracle.md): сравнение поведения с оригинальной игрой.
- [Работа с Ghidra](docs/ghidra-workflow.md): исследование оригинального движка.

Перевод [README.md](README.md); поддерживайте его в соответствии с английской версией.
