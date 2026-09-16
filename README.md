<img src="docs/images/new-conscirpt-hero-image.png" alt="VERA20k hero image" width="100%">

<img src="docs/images/vera20k-screenshots.png" alt="VERA20k skirmish setup screen and in-game view" width="100%">

[![VERA20K Discord](https://img.shields.io/badge/VERA20K%20Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/kmjRUn5m5F)

Red Alert 2: Yuri's Revenge — rebuilt from scratch in Rust for large multiplayer battles.

This project stands on the shoulders of giants. Thanks to OpenRA, XCC mixer, ModEnc website, PPM website, EA for open source RA1, World Altering Editor, Final Alert,YRpp, Ares, Phobos and much more.

VERA20K operates after contributor-owned cooperative principles. Contributors earn equity shares through contributions or donations. Those shares give automatically income rights and governance weight.

---

## Project Goals

<small>

**1.**
Engine true to the original Westwood game: visuals, atmosphere, gameplay.

**2.**
Built for big multiplayer: targeting support for 30 players, 20,000 units, larger maps.

**3.**
Room for RTS features the original never had.

## Development process

The reference is the original `gamemd.exe`. Rust behavior cites its native function and address, supported by executable native comparisons and [focused research evidence](docs/research/README.md). Historical investigations remain available in the linked archive. Playtesting in retail and in VERA decides what gets worked on next.

For executable comparisons, see the [native oracle workflow](tools/native_oracle.md):
run original game code under Unicorn and compare its outputs with production Rust.

## Current Status

**Early development** — Work is focused on the core engine. Approximately 40% complete.

**Runs today** (still being matched to the original engine):

- Retail Yuri's Revenge maps with the original menus, loading screen and sidebar
- Base building, power, tech tree, placement, selling and repair
- Harvesting with War, Chrono and Slave miners
- Ground, naval, air and spawned-unit combat; garrisons, transports, engineer capture, mind control, cloak
- Lightning Storm, Iron Curtain, Force Shield, Genetic Mutator, Psychic Reveal, Paradrop
- Save/load mid-match, match recording and replay

**Not yet**

- Network play (lockstep and checksums exist, no transport)
- A real AI opponent (the current one builds a base and sends waves)
- Nuke, Chronosphere, Psychic Dominator, Spy Plane
- Campaign missions and most map triggers


## Requirements

- [Rust](https://rustup.rs/) 1.85+ (edition 2024)
- A copy of **Red Alert 2: Yuri's Revenge** (the engine reads .mix files from your install)


## Setup

1. Clone the repo:
   ```
   git clone https://github.com/YuriPlanet/vera20k.git
   cd vera20k
   ```

2. Copy the example config and set your RA2 install path:
   ```
   cp config.toml.example config.toml
   ```
   Edit `config.toml` and set `ra2_dir` to where your RA2/YR is installed.

3. Build and run:
   ```
   cargo run --bin vera20k
   ```

## Contributing

Read the [architecture overview](https://yuriplanet.github.io/vera20k/) before diving in.

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Make your changes and commit
4. Push to your fork and open a Pull Request

## License

- [GNU General Public License v3.0](LICENSE-GPL)
