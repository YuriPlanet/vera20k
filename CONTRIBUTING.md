# Contributing to VERA20k

Thanks for your interest. You don't need reverse-engineering experience or a full picture of
the codebase to start, and some useful work needs no code at all. Questions are welcome in the
issue you're working on or on [Discord](https://discord.gg/kmjRUn5m5F).

## Ways to help

- **Pick an issue.** [`good first issue`](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue)
  fits an evening; [`help wanted`](https://github.com/YuriPlanet/vera20k/labels/help%20wanted)
  is bigger, and each says what "done" means.
- **Compare with the original.** Play the same situation in Yuri's Revenge and in VERA20k and
  report what differs, ideally with a short clip of each. No code needed, and very useful.
- **Platforms.** Tell us how it builds and runs on your OS and GPU, or fix input, audio, path
  and build problems.
- **Code.** Gameplay in `src/sim/`, rendering in `src/render/` ([wgpu](https://wgpu.rs/)),
  menus, sidebar and input in `src/app/`, `src/ui/` and `src/sidebar/`, tools in `src/bin/`
  and `tools/`. Gameplay starter issues come with the evidence about what the original does.
- **Reverse engineering.** Read `gamemd.exe` in your own Ghidra import, or run its code with
  the [native comparison tools](tools/native_oracle.md) (Python and Unicorn; they accept one
  `gamemd.exe` build, whose SHA-256 is listed there). See the
  [Ghidra working notes](docs/research/ghidra-workflow.md).
- **Docs and media.** Fix outdated docs; record screenshots or clips (game window only, so no
  paths or usernames show).

For anything large, ask on Discord or in an issue first.

## Set up

1. **Get the game.** VERA20k contains no game files; see the
   [README](README.md#vera20k) for where to buy it. You need a complete install in one folder.
   On Linux, Steam Play (Proton) works; on macOS, copy the folder from a Windows install. You
   can build, test and work on many issues without the game.
2. **Install the tools.** Rust 1.88 or newer from [rustup](https://rustup.rs/), and a GPU with
   Vulkan, DirectX 12 or Metal. On Debian/Ubuntu also `libasound2-dev` and `pkg-config` (tell
   us if you needed more).
3. **Build and run** from the repository root:

   ```sh
   git clone https://github.com/YuriPlanet/vera20k.git
   cd vera20k
   cp config.toml.example config.toml   # Windows cmd: copy config.toml.example config.toml
   cargo run --release --bin vera20k
   ```

   Set `ra2_dir` in `config.toml` with forward slashes (`C:/Games/RA2`); TOML treats `\` as an
   escape. Always use `--release` to play. The log goes to `logs/ra2.log`.
4. **Run the tests:** `cargo test -p vera20k --lib` (always `--lib`; plain `cargo test` also
   builds probes in `tests/` that need the game). Without the game, tests that need the retail
   INI files print `SKIPPED` and pass (`-- --show-output` shows them). To run them for real,
   fill `ini/` with `cargo run --bin extract-ini [game folder]` (it falls back to `RA2_DIR`,
   then `config.toml`). Set `VERA20K_REQUIRE_RETAIL_INI=1` to make a missing file fail. If a
   test fails only with your own game files, tell us which install you have.
5. **Never commit game files:** no `.mix`, INI, art, audio, video or `.exe` from the game, and
   nothing from `ini/`. Git already ignores `ini/` and `config.toml`.

## Your first pull request

1. **Claim an issue** by commenting on it. One claim at a time; a claim with no update for 14
   days becomes free again.
2. **Branch from `main`** in your fork, and update from `main` before asking for review.
3. **Keep it small:** one issue per PR, at most one gameplay mechanism.
4. **Check it:** `cargo test -p vera20k --lib` and `cargo clippy -p vera20k --lib`. Format only
   the files you changed with `rustfmt --edition 2024 <file>` (not `cargo fmt`; a `mod.rs`
   also reformats its submodules, so leave those out).
5. **Keep the README status honest** if your change makes a feature work or stop working.
6. **Open the PR against `main`** and fill in the template. CI builds and tests Windows, Linux
   and macOS.

## Project rules, in short

Most code here is written by AI assistants that the maintainer directs;
[`AGENTS.md`](AGENTS.md) holds their full rules (skip its maintainer-machine details). For
people, these matter:

1. **The original is the reference.** Gameplay changes say where the behavior comes from, with
   the native function and address in a comment: `/// gamemd: ClassName::Function @ 0x00XXXXXX`.
   If you don't know, say so; don't guess.
2. **One owner per piece of state.** Extend the existing owner instead of adding a second copy,
   and delete an old path when you replace it.
3. **Deterministic simulation.** Same inputs, same result on every OS and CPU. Use `SimFixed` in
   `src/sim/`; nothing there may depend on the platform, the clock or hash-map order, or use
   `render`, `ui`, `sidebar`, `audio` or `net`.
4. **Keep the original order.** Random draws use the same stream, order and count as the
   original, and same-frame effects happen in the original's order. Batch or parallelize only
   when the result is identical.
5. **Say what you checked.** Numbers, rounding, random draws and timers count as matching only
   once they're compared with the original running, not just read from its code.
6. **Tests and clippy pass** before you open the PR.

## Evidence

Claims are "native behavior established" (from the original's code or data), "Rust regression
tested" (named tests) or "parity demonstrated" (compared with the original running the same
input). Non-gameplay changes just need passing tests. For gameplay starter issues the
maintainer supplies the native evidence; you cite it and add a test. If evidence is out of
reach, say so in the PR: the numeric and timing checks against the original are required before
a mechanism merges, but you don't have to run them yourself.

## AI tools

Welcome, as long as you say in the PR which parts they wrote and you can explain every change.
The same evidence rules apply to everyone. The research notes in `docs/research/` were written
the same way and can be wrong; treat them as leads.

## Review and credit

The maintainer reviews every outside PR, often with an AI reviewer's help, and aims to reply
within a few days; ping the PR or Discord if it goes quiet. Before merging, the maintainer also
runs the tests that need game files. Your commits keep you as author, and if a maintainer
finishes your change you're credited with `Co-authored-by:`.

## Reporting bugs and differences

Use the [issue forms](https://github.com/YuriPlanet/vera20k/issues/new/choose): bug report,
differs from the original game, or platform report. Include the commit
(`git rev-parse --short HEAD`), the map, your OS and GPU, whether the original does the same,
and a clip, screenshot or log (remove your username from any paths first).

## Where to learn the code

The [architecture overview](https://yuriplanet.github.io/vera20k/) (`docs/index.md`), the `//!`
header of each module, `SimRuntime::advance_frame` (`src/sim/runtime.rs`) and
`Simulation::advance_master_frame` (`src/sim/world/mod.rs`) for one frame in the original's
order, [`tools/native_oracle.md`](tools/native_oracle.md), and the
[research notes](docs/research/README.md).

## License

VERA20k is GPLv3 ([`LICENSE-GPL`](LICENSE-GPL)). By contributing you agree your contribution is
licensed the same way. There is no contributor license agreement.
