<img src="docs/images/new-conscirpt-hero-image.png" alt="Заглавное изображение VERA20k" width="100%">

<p align="center" dir="ltr">
  <a href="README.sv.md" lang="sv">Svenska</a> · <a href="README.zh-CN.md" lang="zh-CN">简体中文</a> · <a href="README.de.md" lang="de">Deutsch</a> · <a href="README.ar.md" lang="ar" dir="rtl">العربية</a> · <strong>Русский</strong> · <a href="README.th.md" lang="th">ไทย</a> · <a href="README.tr.md" lang="tr">Türkçe</a> · <a href="README.md" lang="en">English</a>
  &nbsp;&nbsp;
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml?query=branch%3Amain" title="Последний запуск тестов библиотеки на macOS (запускаются вручную)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/macos.yml/badge.svg?branch=main" alt="Тесты библиотеки на macOS" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml?query=branch%3Amain" title="Последний запуск тестов библиотеки на Linux (запускаются вручную)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/linux.yml/badge.svg?branch=main" alt="Тесты библиотеки на Linux" height="20" align="middle"></a>
  <a href="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml?query=branch%3Amain" title="Последний запуск тестов библиотеки на Windows (запускаются вручную)"><img src="https://github.com/YuriPlanet/vera20k/actions/workflows/windows.yml/badge.svg?branch=main" alt="Тесты библиотеки на Windows" height="20" align="middle"></a>
  <a href="https://discord.gg/kmjRUn5m5F"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat&amp;logo=discord&amp;logoColor=white" alt="Присоединиться к Discord" height="20" align="middle"></a>
</p>

# VERA20k

Red Alert 2: Yuri's Revenge — переписанная на Rust для масштабных многопользовательских сражений.

VERA20k — новая реализация оригинального движка `gamemd.exe`. Она использует оригинальные
файлы игры, поэтому тебе понадобится собственная копия Red Alert 2: Yuri's Revenge. Игра входит в
*Command & Conquer The Ultimate Collection*, доступную в
[Steam](https://store.steampowered.com/bundle/39394/) и
[EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc).

VERA20k делают игроки для игроков, и последнее слово в том, куда движется проект, остаётся за
игроками.

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k: настройки схватки и кадр из игры" width="100%">

## Цели проекта

1. Сохранить игровой процесс, графику и атмосферу оригинальной Red Alert 2: Yuri's Revenge.
2. Поддерживать более крупные сражения: до **30 игроков** и **20 000 юнитов** на больших картах.
3. Добавлять новые возможности RTS.
4. Встроенный клиент для мультиплеера

## Текущее состояние

**Ранний этап разработки.** На Windows можно играть в локальные схватки против простого ИИ.
Карты из оригинальной игры и случайные карты, меню, строительство баз, сбор ресурсов,
бои, сохранение и загрузка уже есть, но многое ещё нужно исправить и закончить.

Многопользовательского режима, кампаний и оригинального ИИ пока нет. Авиация, контроль
разума, мосты, некоторые виды оружия и эффекты требуют доработки. Сражения с 30 игроками
и 20 000 юнитами мы пока не продемонстрировали.

Обычные здания, объекты ландшафта и юниты теперь попадают в отрисовку до раскрытия их опорной клетки, поэтому туман войны постепенно открывает их пиксели. Особые пути отрисовки ещё требуют сверки с оригиналом.

## Сборка и запуск

Нужны Rust 1.88 или новее, видеокарта с поддержкой Vulkan, DirectX 12 или Metal и
установленная игра. В VERA20k уже играли на Windows, Linux и macOS.

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml
# Перед запуском отредактируй config.toml и укажи папку с игрой в ra2_dir:
cargo run --release --bin vera20k
```

Для игры используй `--release`; отладочные сборки слишком медленные. Инструкции по
настройке на разных платформах и запуску тестов есть в [CONTRIBUTING.md](CONTRIBUTING.md#set-up).

## Как мы работаем

Большую часть кода пишут ИИ-агенты для программирования, которыми я руковожу. Мы изучаем
оригинальный движок в Ghidra, затем переносим его поведение на Rust и проверяем его
[инструментами сравнения](tools/native_oracle.md) и игровыми тестами. Правила работы описаны
в [AGENTS.md](AGENTS.md).

## Как помочь

Мы рады помощи. Можно писать код, тестировать игру, улучшать документацию или играть
параллельно с оригиналом и рассказывать, что кажется неправильным. Опыт обратной разработки
для этого не нужен.

Прочитай [CONTRIBUTING.md](CONTRIBUTING.md), посмотри
[задачи для начинающих](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue)
или поздоровайся в [Discord](https://discord.gg/kmjRUn5m5F).
[Обзор архитектуры](https://yuriplanet.github.io/vera20k/ru/) объясняет, как устроен движок.

## Благодарности и правовая информация

Спасибо OpenRA, XCC Mixer, вики ModEnc, Project Perfect Mod, EA за публикацию исходного кода
Command & Conquer и Red Alert под GPL, World-Altering Editor, Final Alert, YRpp, Ares,
Phobos и многим другим.

Проект распространяется под лицензией [GPLv3](LICENSE-GPL). В этом репозитории нет файлов игры.
Command & Conquer и Red Alert — товарные знаки Electronic Arts Inc., а на скриншотах показана
игровая графика, принадлежащая Electronic Arts. VERA20k не связан с Electronic Arts и
не одобрен компанией.

Перевод [README.md](README.md); поддерживайте его в соответствии с английской версией.
