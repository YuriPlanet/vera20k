<img src="docs/images/new-conscirpt-hero-image.png" alt="Заглавное изображение VERA20K" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <a href="README.es.md" lang="es">Español</a> · <strong>Русский</strong> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
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

<img src="docs/images/vera20k-screenshots.png" alt="VERA20K: настройки схватки и кадр из игры" width="100%">

## Цели проекта

1. Сохранить игровой процесс, графику и атмосферу оригинальной Red Alert 2: Yuri's Revenge.
2. Поддерживать более крупные сражения: до **30 игроков** и **20 000 юнитов** на картах 500×500.
3. Добавлять известные возможности RTS, а также такие, каких ещё не было.
4. Встроенный клиент для мультиплеера

## Текущее состояние

**Активная разработка.**

- **Схватки:** Доступны на Windows, Linux и macOS: карты из оригинальной игры и случайные
  карты, строительство баз, сбор ресурсов, бои, сохранение и загрузка.
- **Кампании:** Миссии можно запускать; поддержка остаётся неполной.
- **ИИ:** Части оригинального ИИ отвечают за строительство баз и атаки по скриптам.
- **Текущая работа:** Точность игрового поведения, графика и производительность.
- **Мультиплеер:** Сетевая игра пока недоступна.
- **Масштаб:** Сражения с 30 игроками и 20 000 юнитами ещё не продемонстрированы.

## Скачать и играть

Скачай установщик для Windows, macOS или Linux на
[странице выпусков](https://github.com/YuriPlanet/vera20k/releases). Он сам найдёт твою копию
Yuri's Revenge из [Steam](https://store.steampowered.com/app/2229850/) или
[приложения EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection)
и установит VERA20K в новую папку. Туда же будут попадать моды и новый контент, так что файлы самой
игры никогда не меняются.

Чтобы запускать одиночные схватки или кампании YR через клиент CnCNet, см.
[настройку офлайн-клиента](integrations/cncnet/README.md).

## Настройки

В **Options** настраиваются звук и управление, а в **Vera Options** — функции, специфичные для VERA.
Предпочтения игрока, сочетания клавиш и настройки лобби сохраняются в `VERA.INI`:

- **Windows (версия с установщиком):** рядом с `vera20k.exe`.
- **macOS:** `~/Library/Application Support/VERA20k/VERA.INI`.
- **Linux:** `~/.local/share/VERA20k/VERA.INI` или в твоей папке данных XDG.

В `config.toml` задаётся папка игры; там же можно выбрать другую папку настроек.

Если `VERA.INI` отсутствует, VERA создаёт его и один раз импортирует существующие настройки.
Если старых настроек нет, файл создаётся пустым, а VERA использует встроенные значения
по умолчанию, пока настройки не будут сохранены. Оригинальные `RA2MD.INI` и `KeyboardMD.ini`
читаются только для импорта и никогда не перезаписываются.

## Архитектура

Движок состоит из трёх основных частей:

- **Симуляция:** Отвечает за состояние игры и игровую логику.
- **Рендеринг:** Совершенно новый конвейер отрисовки на GPU, использующий оригинальные игровые
  ресурсы и созданный для сохранения оригинального облика.
- **Приложение:** Обрабатывает ввод, меню, звук и интеграцию с платформой, связывая системы между собой.

Такое разделение поддерживает детерминированный игровой процесс.

Стрелки показывают поток данных:

![Приложение передаёт команды симуляции и получает состояние мира; данные отрисовки передаются рендерингу.](docs/images/architecture.ru.png)

## Системные требования

- **Игра:** Red Alert 2: Yuri's Revenge 1.001.
- **ОС:** Windows 10+, Linux (glibc 2.35+, например Ubuntu 22.04 или Steam Deck) или macOS 11+
  на Apple Silicon.
- **Графика:** Vulkan, DirectX 12 или Metal.

## Сборка и запуск

Понадобится:

- **Rust и Git:** [Rust](https://rustup.rs/) 1.97 или новее, установленный через rustup, и
  [Git](https://git-scm.com/).
- **Инструменты сборки:** на Windows — C++ Build Tools из Visual Studio, которые предложит
  установить установщик Rust; на macOS — `xcode-select --install`; в Debian и Ubuntu —
  `sudo apt install build-essential libasound2-dev pkg-config`.

В VERA20K уже играли на Windows, Linux и macOS.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Перед запуском отредактируй config.toml и укажи папку с игрой в ra2_dir:
cargo run --release --bin vera20k
```

Сборка занимает несколько ГБ в `target/`.

## Как мы работаем

Большую часть кода пишут ИИ-агенты для программирования. Они изучают оригинальный движок в
Ghidra, затем переносят его поведение на Rust и проверяют его
[инструментами сравнения](tools/native_oracle.md) и игровыми тестами. Агенты следуют
[AGENTS.md](AGENTS.md).

## Настройка

По умолчанию VERA20K ведёт себя так же, как оригинальная игра. Каждая новая функция, включая
расширения из модов вроде Ares, должна оставаться выключенной, пока её не включат одним из способов:

- **Пользовательским параметром VERA** в **Options → Vera Options**, который сохраняется в секции `[VERA]` файла `VERA.INI`.
- **Параметром сборки**, например `--features ares` для поддерживаемых [расширений Ares](docs/ares.md).
- **Параметром в игровом лобби**, который выбирается для каждого матча.
- **Ключом INI**, который задаёт мод или карта.

Настройки звука, графики и управления сохраняются в `VERA.INI`. Правила игры задаются
настройками лобби, модами или картами.

## Как помочь

Мы рады помощи. Можно писать код, рефакторить движок, тестировать игру, предлагать идеи или
играть параллельно с оригиналом и рассказывать, что кажется неправильным. Открой PR, а дальше
мы разберёмся сами.

Для каждого нового параметра укажи файл, секцию, ключ, значение по умолчанию и его действие.

Все функции должны оставаться в одной сборке; параметры сборки добавляй только для различий между
платформами или необязательных библиотек.

Обсуждение идёт в [Discord](https://discord.gg/kmjRUn5m5F) и в QQ-группе (1097193563).

## Поддержка проекта

<a href="https://paypal.me/vera20k"><img src="https://img.shields.io/badge/Donate_via_PayPal-FFC439?style=flat&amp;logo=paypal&amp;logoColor=003087" alt="Поддержать VERA20K через PayPal" height="23"></a>

Все полученные пожертвования
распределяются между участниками проекта согласно
[журналу вкладов TIBTRE](https://github.com/YuriPlanet/tibtre-ledger).

Участники получают выплаты через:

<p><img src="https://img.shields.io/badge/PayPal-003087?style=flat&amp;logo=paypal&amp;logoColor=white" alt="PayPal" height="23"> <img src="https://img.shields.io/badge/China_UnionPay-E21836?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9Ii02IC02IDI2OCAxNzIiPjxwYXRoIGQ9Ik01MC4wNDE2MzEsMC4wMDA2Njc1NjMzMDUgTDExMy44MjAyMTEsMC4wMDA2Njc1NjMzMDUgQzEyMi43MjMxMjcsMC4wMDA2Njc1NjMzMDUgMTI4LjI2MDQzOCw3LjI1NzMxMzQ1IDEyNi4xODM1NTgsMTYuMTg5NzY1NyBMOTYuNDkwMDcxNSwxNDMuNjc1NDE2IEM5NC4zOTQ1MzY1LDE1Mi41NzY3NzcgODUuNDgwNzM5LDE1OS44Mzk2NDEgNzYuNTcxNjA1LDE1OS44Mzk2NDEgTDEyLjc5OTI0MzMsMTU5LjgzOTY0MSBDMy45MDg3NjQsMTU5LjgzOTY0MSAtMS42NDA5ODQ0NCwxNTIuNTc2Nzc3IDAuNDM1ODk2MjA4LDE0My42NzU0MTYgTDMwLjE0MTgxODYsMTYuMTg5NzY1NyBDMzIuMjE4Njk5Myw3LjI1NzMxMzQ1IDQxLjEyNzgzMzMsMC4wMDA2Njc1NjMzMDUgNTAuMDQxNjMxLDAuMDAwNjY3NTYzMzA1IiBmaWxsPSIjRTIxODM2IiBzdHJva2U9IiNmZmYiIHN0cm9rZS13aWR0aD0iOCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIvPjxwYXRoIGQ9Ik0xMDguNTEzMjgzLDAuMDAwNjY3NTYzMzA1IEwxODEuODU3MDE4LDAuMDAwNjY3NTYzMzA1IEMxOTAuNzU4Mzc4LDAuMDAwNjY3NTYzMzA1IDE4Ni43NDQ1MjcsNy4yNTczMTM0NSAxODQuNjUwNTQ2LDE2LjE4OTc2NTcgTDE1NC45NjE3MjQsMTQzLjY3NTQxNiBDMTUyLjg3ODYyNSwxNTIuNTc2Nzc3IDE1My41Mjg0MjcsMTU5LjgzOTY0MSAxNDQuNjA4NDExLDE1OS44Mzk2NDEgTDcxLjI2NDY3NzUsMTU5LjgzOTY0MSBDNjIuMzQ0NjYxNSwxNTkuODM5NjQxIDU2LjgyNDQ0OTUsMTUyLjU3Njc3NyA1OC45MTk5ODUsMTQzLjY3NTQxNiBMODguNjA3MjUyNSwxNi4xODk3NjU3IEM5MC43MTUyMjQsNy4yNTczMTM0NSA5OS42MDU3MDM1LDAuMDAwNjY3NTYzMzA1IDEwOC41MTMyODMsMC4wMDA2Njc1NjMzMDUiIGZpbGw9IiMwMDQ0N0MiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSI4IiBzdHJva2UtbGluZWpvaW49InJvdW5kIi8%2BPHBhdGggZD0iTTE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSBMMjQyLjcyNjQwOSwwLjAwMDY2NzU2MzMwNSBDMjUxLjY0MTc2MiwwLjAwMDY2NzU2MzMwNSAyNTcuMTc5MDc0LDcuMjU3MzEzNDUgMjU1LjA4NTA5MywxNi4xODk3NjU3IEwyMjUuMzk2MjcxLDE0My42NzU0MTYgQzIyMy4zMDA3MzYsMTUyLjU3Njc3NyAyMTQuMzgwNzE5LDE1OS44Mzk2NDEgMjA1LjQ2NjkyMiwxNTkuODM5NjQxIEwxNDEuNzE3ODc5LDE1OS44Mzk2NDEgQzEzMi43OTc4NjMsMTU5LjgzOTY0MSAxMjcuMjY1MjE1LDE1Mi41NzY3NzcgMTI5LjM1NDUzMiwxNDMuNjc1NDE2IEwxNTkuMDQ4MDE3LDE2LjE4OTc2NTcgQzE2MS4xMjQ4OTksNy4yNTczMTM0NSAxNzAuMDI3ODE0LDAuMDAwNjY3NTYzMzA1IDE3OC45NDc4MywwLjAwMDY2NzU2MzMwNSIgZmlsbD0iIzAwN0I4NCIgc3Ryb2tlPSIjZmZmIiBzdHJva2Utd2lkdGg9IjgiIHN0cm9rZS1saW5lam9pbj0icm91bmQiLz48L3N2Zz4%3D" alt="China UnionPay" height="23"> <img src="https://img.shields.io/badge/Alipay-1677FF?style=flat&amp;logo=alipay&amp;logoColor=white" alt="Alipay" height="23"> <img src="https://img.shields.io/badge/Vipps-FF5B24?style=flat&amp;logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjQzLjUgMjEuNSAyNyAyNyI%2BPHBhdGggZD0iTTU3LjMsNDAuNmMzLjcsMCw1LjgtMS44LDcuOC00LjRjMS4xLTEuNCwyLjUtMS43LDMuNS0wLjlzMS4xLDIuMywwLDMuN2MtMi45LDMuOC02LjYsNi4xLTExLjMsNi4xIGMtNS4xLDAtOS42LTIuOC0xMi43LTcuN2MtMC45LTEuMy0wLjctMi43LDAuMy0zLjRzMi41LTAuNCwzLjQsMUM1MC41LDM4LjMsNTMuNSw0MC42LDU3LjMsNDAuNnogTTY0LjIsMjguM2MwLDEuOC0xLjQsMy0zLDMgcy0zLTEuMi0zLTNzMS40LTMsMy0zQzYyLjgsMjUuMyw2NC4yLDI2LjYsNjQuMiwyOC4zeiIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg%3D%3D" alt="Vipps" height="23"> <img src="https://img.shields.io/badge/SEPA-2D4B9A?style=flat&amp;logo=sepa&amp;logoColor=white" alt="SEPA" height="23"> <img src="https://img.shields.io/badge/Bitcoin-F7931A?style=flat&amp;logo=bitcoin&amp;logoColor=white" alt="Bitcoin" height="23"></p>

## Благодарности и правовая информация

Спасибо OpenRA, XCC Mixer, вики ModEnc, Project Perfect Mod, EA за публикацию исходного кода
Command & Conquer и Red Alert под GPL, World-Altering Editor, Final Alert, YRpp, Ares,
Phobos и многим другим.

Проект распространяется под лицензией [GPLv3](LICENSE-GPL). В этом репозитории нет файлов игры.
Command & Conquer и Red Alert — товарные знаки Electronic Arts Inc., а на скриншотах показана
игровая графика, принадлежащая Electronic Arts. VERA20K не связан с Electronic Arts и
не одобрен компанией.

Перевод [README.md](README.md); поддерживайте его в соответствии с английской версией.
