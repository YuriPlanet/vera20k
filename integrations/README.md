# Integrations

This folder connects VERA20k to programs outside the repository, such as
community launchers, game clients and online services. VERA itself stays a
standalone game; an integration lets one of those programs start it, configure
it or pass it a game to play.

## What belongs here

- Setup scripts and configuration profiles that adapt a third-party program to
  launch VERA20k.
- A README per integration that explains what it supports, how to install it,
  which files it changes and how to undo it.

Code that VERA runs belongs in `src/`, even when it serves an integration (for
example the `-SPAWN` startup parser in `src/app/frontend/cncnet_spawn.rs`).
Developer tools belong in `tools/`.

## Rules

- Each integration lives in its own folder and is self-contained.
- Never commit the third-party program, retail game assets or executables.
  Users supply their own copies.
- Setup scripts change only the copy the user points them at, and back up what
  they replace.

## Current integrations

| Folder | What it does |
|---|---|
| [`cncnet/`](cncnet/README.md) | Launches single-player YR skirmishes and campaigns from a dedicated CnCNet client copy through VERA's `-SPAWN` startup adapter. |
