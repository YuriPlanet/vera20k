---
layout: default
title: VERA20k Engine
---

# VERA20k

[Svenska](sv/) · [简体中文](zh-CN/) · [Deutsch](de/) · [العربية](ar/) · [Русский](ru/) · [ไทย](th/) · [Türkçe](tr/) · **English**

Red Alert 2: Yuri's Revenge — rebuilt from scratch in Rust.

A short map of the code. If it disagrees with the code, the code is right. Each module's `//!` header says what it does and what it may depend on.

## The code

- `sim/` — all game state and deterministic gameplay.
- `render/` — the wgpu renderer.
- `app/` — window, input, loading and saving; wires sim, render, ui, audio and net together.
- `ui/` and `sidebar/` — menus, dialogs, in-game screens and the sidebar.
- `audio/` — sound effects and music.
- `net/` — deterministic lockstep. There is no network transport yet.
- `rules/` — the retail INI files and their layers.
- `map/` — maps, theaters, terrain, triggers and the random map generator.
- `assets/` — parsers for the original file formats (`.mix`, `.shp`, `.vxl`, `.pal`, Bink video and more).
- `util/`, `asset_tools/` and `bin/` — shared helpers, asset inspection and extra programs.

## How it fits together

All mutable game state lives in one struct, `Simulation` (`src/sim/world/mod.rs`). `SimRuntime::advance_frame()` runs one frame in the original game's frame order.

The app turns player input into commands for the simulation. After each frame it hands the new state to the renderer and plays the sounds the frame produced. Rendering only reads simulation state, and `sim/` never depends on `render/`, `ui/`, `sidebar/`, `audio/` or `net/`.

## Determinism

The same state, inputs and random seed must give the same result on every platform and CPU.

- Simulation math prefers `SimFixed` fixed-point over floats.
- Three random streams match the original's three generators. A draw on the wrong stream changes every later draw.
- Objects take their turns in the original's active-object order.

Each frame ends with a state hash that lockstep multiplayer will rely on.
