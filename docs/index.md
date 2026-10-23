---
layout: default
title: VERA20k Engine
---

# VERA20k

[Svenska](sv/) · [简体中文](zh-CN/) · [Deutsch](de/) · [العربية](ar/) · [Русский](ru/) · [ไทย](th/) · [Türkçe](tr/) · **English**

Red Alert 2: Yuri's Revenge — rebuilt from scratch in Rust.

A short map of the code. If it disagrees with the code, the code is right. Each module's `//!` header says what it does and what it may depend on.

## The code

Source paths below are under `src/`. The workspace has two crates:

- `vera20k-core` (`src/Cargo.toml`) contains assets, rules, maps, simulation,
  networking, shared utilities and the headless/skirmish entry points.
- `vera20k` (`Cargo.toml`) contains the app, renderer, UI, audio, asset tools and
  game executable. It re-exports core's modules.

- `sim/` — all game state and deterministic gameplay.
- `render/` — the wgpu renderer.
- `app/` — window, input, loading and saving; wires sim, render, ui, audio and net together.
- `ui/` — menus, dialogs, in-game screens and the sidebar.
- `audio/` — sound effects and music.
- `net/` — LAN UDP transport, reliable delivery and synchronized command exchange.
  [Networking.md](Networking.md) describes the current behavior and remaining work.
- `rules/` — the retail INI files and their layers.
- `map/` — maps, theaters, terrain, triggers and the random map generator.
- `assets/` — parsers for the original file formats (`.mix`, `.shp`, `.vxl`, `.pal`, Bink video and more).
- `util/`, `asset_tools/` and `bin/` — shared helpers, asset inspection and extra programs.

## How it fits together

`Simulation` (`src/sim/world/mod.rs`) owns mutable gameplay state.
`SimRuntime` binds it to match resources; `SimRuntime::advance_frame()` advances
the deterministic frame phases.

The app turns player input into commands for the simulation. After each frame it hands the new state to the renderer and plays the sounds the frame produced. Rendering only reads simulation state, and `sim/` never depends on `render/`, `ui/`, `audio/` or `net/`.

## Determinism

The same state, inputs and random seed must give the same result on every platform and CPU.

- Simulation math prefers `SimFixed` fixed-point over floats.
- Three random streams match the original's three generators. A draw on the wrong stream changes every later draw.
- Objects take their turns in the original's active-object order.

Normal play hashes the whole state once every 15 frames for its diagnostic replay
log; tests and benchmarks can hash every frame. Multiplayer has a separate
per-frame sync checksum, with calculation, exchange and comparison handled by the
simulation, session and network command owners.
