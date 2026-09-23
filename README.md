<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k hero image" width="100%">

[![VERA20k Discord](https://img.shields.io/badge/VERA20k%20Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/kmjRUn5m5F)
[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue?style=for-the-badge)](LICENSE-GPL)

# VERA20k

**Red Alert 2: Yuri's Revenge, rebuilt in Rust. Faithful to the original, with a goal of
30 players and 20,000 units.**

VERA20k is an open-source reimplementation of the Yuri's Revenge engine (`gamemd.exe`). It
loads the original game's rules, maps, art and sound from your own copy of the game, and it
aims to play exactly like the original: the same unit behavior, timings and quirks.

Gameplay is ported from the original executable one mechanism at a time. Newer gameplay code
cites the native functions and addresses it was ported from, and a growing set of native
comparison harnesses runs the original code on the same inputs, so the Rust results can be
checked against it.

Beyond that faithful core, the goal is scale the original was never built for: up to 30
players and 20,000 units on larger maps, on Windows, Linux and macOS. That scale has not been
demonstrated yet.

**You need the original game.** VERA20k contains no game files. It reads them from a complete
Red Alert 2 + Yuri's Revenge install in one folder. EA sells both games in *Command & Conquer
The Ultimate Collection*, on [Steam](https://store.steampowered.com/bundle/39394/) and from
[EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc).

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k skirmish setup screen and in-game view" width="100%">

### How is this different from OpenRA?

[OpenRA](https://www.openra.net) is its own engine that recreates the classic Command & Conquer
games with modernized gameplay. Its Red Alert 2 mod rebuilds RA2 on that engine. VERA20k aims
for the opposite: reproduce Yuri's Revenge itself, mechanism by mechanism from the original
executable, so a match plays, looks and sounds like the original. Going past the original's
limits is a separate goal on top of that.

### Why not just play the original?

You can, and you should. VERA20k is for what the original can't offer: an open-source engine
that anyone can study, fix and extend, with larger battles and more platforms as goals.

## Status (September 2026)

**Pre-alpha.** Development started in March 2026. You can play a local skirmish on retail
maps against a placeholder AI on Windows. Multiplayer, the campaign, the original AI and
several unit abilities are still missing. So far there are 44 native harnesses and over 9,000
library tests.

**Playable today** (local skirmish on Windows; still being matched to the original)

- Retail Yuri's Revenge maps and the random map generator, with the original menus, loading
  screen and sidebar
- Base building, power, tech tree, placement, walls, selling and repair
- Harvesting with War Miners, Chrono Miners and Slave Miners
- Infantry, vehicle, naval and base-defense combat; garrisons, transports, engineer capture
  and cloaking
- Attack dogs and Terror Drones
- Lightning Storm, Iron Curtain, Force Shield, Genetic Mutator, Psychic Reveal and paradrops
- Saving and loading mid-match

**Partly working**

- Aircraft take off, fly, fire, land and reload. Parts of the attack run and Carrier Hornet
  strikes are still incomplete.
- Unit and building deaths are ported from the original, including death timing, debris,
  crews and survivors, and building explosions. Ship sinking and a few other death effects
  (flying-unit crashes, crushed-vehicle explosions, passengers escaping a destroyed transport)
  are still missing.
- Crates appear on the map, but they can't be picked up yet (in progress).
- A match writes a diagnostic command log for debugging. There is no replay viewer yet.
- Linux and macOS: nobody has played a match on either yet. Reports are welcome.

**Not yet**

- Mind control (Yuri, Psychic Tower, Mastermind): in progress
- Crazy Ivan bombs, Chrono Legionnaire, Magnetron, Boris's air strike, Spy disguise and bomb
  defusal
- Gattling spin-up, Prism Tower chaining, and Tesla Troopers charging Tesla Coils
- Nuke, Chronosphere, Psychic Dominator and Spy Plane
- Network play (deterministic lockstep and checksums exist, but there is no network transport)
- The original AI (today's opponent is a placeholder that builds a base and sends attack
  waves)
- Campaign missions and most map triggers
- The Sneak Preview, the Movies menu, campaign movies and the credits roll
- The 30-player / 20,000-unit scale target: not demonstrated yet. Skirmish keeps the
  original's 8 player slots, and there is no large-battle benchmark.

## Quick start

### Requirements

- Rust stable from [rustup](https://rustup.rs/), version 1.88 or newer.
  `rust-toolchain.toml` selects the stable toolchain for you.
- A GPU with Vulkan, DirectX 12 or Metal. There is no OpenGL fallback.
- A complete Red Alert 2 + Yuri's Revenge install in one folder (see above).
- Windows 11 is the only platform tested so far.
  - On Linux or macOS you still need the Windows game files, for example copied from a
    Windows install. This has not been tested.
  - On Debian or Ubuntu, the audio library needs `libasound2-dev` and `pkg-config`. Other
    packages may be missing from this list; please tell us what you needed.

### Build and run

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml   # Windows Command Prompt: copy config.toml.example config.toml
```

Open `config.toml` and set `ra2_dir` to your game folder. Use forward slashes, as in the
example (`C:/Westwood/RA2`), because TOML treats `\` as an escape character. On Steam the
folder is usually `steamapps/common/Command & Conquer Red Alert II` inside your Steam library.

Then build and start the game from the repository root, where `config.toml` lives:

```sh
cargo run --release --bin vera20k
```

Always use `--release` to play. The first release build takes a while, and debug builds are
much slower.

### Tests

```sh
cargo test -p vera20k --lib
```

This runs without the game. Tests that need the retail INI files print `SKIPPED` and pass;
add `-- --show-output` to see those lines. To run them too, extract the INI files from your
install into the gitignored `ini/` folder:

```sh
cargo run --bin extract-ini
```

It uses a game folder you pass as an argument
(`cargo run --bin extract-ini -- "<game folder>"`), otherwise the `RA2_DIR` environment
variable, otherwise `ra2_dir` in `config.toml`. Run it from the repository root.

Use `--lib`: plain `cargo test` also builds the probe programs in `tests/`, and most of them
need the game.

## How it's built

The reference is the original `gamemd.exe`. Gameplay mechanisms are traced through the
original executable and ported into Rust with their own tests. Newer gameplay code names the
native function and address it came from; some older code, such as the placeholder AI, does
not. The [native oracle workflow](tools/native_oracle.md) runs original game code under
Unicorn. It lists 44 native harnesses so far, and most of them are paired with a Rust test
that checks the same cases.
The reverse-engineering notes in [docs/research](docs/research/README.md) can be wrong, so
treat them as leads.

Most of the code is written by AI coding assistants that the maintainer directs. The
assistants follow the project rules in [AGENTS.md](AGENTS.md) (CLAUDE.md holds the same
rules). Every change is expected to pass the library tests. Larger changes also get an
independent AI review pass, and gameplay changes must cite where the behavior comes from in
the original. Numbers, random draws and timer lengths must also match the original running
the same input before a mechanism merges. The maintainer decides what merges.

## Contributing

Help is welcome, and you don't need reverse-engineering experience to start.

- **Play and compare.** Tell us where VERA20k behaves differently from the original game. A
  short clip of each helps a lot.
- **Try Linux or macOS.** Report what happened, whether it worked or not.
- **Write code.** Gameplay, rendering (wgpu), menus and UI, tools and docs. Look for issues
  labeled [good first issue](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue).
- **Reverse engineering.** If you know `gamemd.exe`, see the
  [native oracle workflow](tools/native_oracle.md).

The full guide is in [CONTRIBUTING.md](CONTRIBUTING.md): setup, project rules, evidence and
how review works.

Before a large change, open an issue or ask on [Discord](https://discord.gg/kmjRUn5m5F) first.
Keep each pull request to one topic. Run `cargo test -p vera20k --lib` and
`cargo clippy -p vera20k --lib`, and format only the files you changed
(`rustfmt --edition 2024 <file>`).

AI coding tools are welcome. Say in your pull request which parts they wrote, and make sure
you understand the change well enough to explain it in review.

Never commit files from the game: no `.mix` archives, no INI files copied or extracted from
the game, no extracted art or sound, and no `gamemd.exe`. The `ini/` folder is gitignored for
that reason. (`config.toml` is gitignored too, because it holds your local game path.)

## Project goals

- An engine true to the original Westwood game: visuals, atmosphere and gameplay.
- Built for big multiplayer: a target of 30 players, 20,000 units and larger maps.
- Room for RTS features the original never had.

## Documentation

- [Architecture overview](https://yuriplanet.github.io/vera20k/): the source layout, the
  `Simulation` struct, the frame order and the determinism rules.
- Module headers: most source files start with a `//!` comment that says what the file does
  and what it may depend on.
- [Native oracle workflow](tools/native_oracle.md): running original game code to check the
  Rust code.
- [Research archive](docs/research/README.md): reverse-engineering notes. Treat them as leads.
- [AGENTS.md](AGENTS.md): the project rules the AI assistants follow. Parts of it describe the
  maintainer's own machine.

## Credits

VERA20k stands on the shoulders of giants. Thanks to OpenRA, XCC Mixer, the ModEnc wiki,
Project Perfect Mod (PPM), EA's GPL source release of the original Command & Conquer and
Red Alert, World-Altering Editor, Final Alert, YRpp, Ares, Phobos and many others.

## License and legal

VERA20k is licensed under the [GNU General Public License v3.0](LICENSE-GPL).

This repository contains no game files from Red Alert 2 or Yuri's Revenge. You need your own
copy of the game. Command & Conquer and Red Alert are trademarks of Electronic Arts Inc.
Screenshots show game art owned by Electronic Arts. VERA20k is not affiliated with or
endorsed by Electronic Arts.
