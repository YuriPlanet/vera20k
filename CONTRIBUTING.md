# Contributing to VERA20k

Thanks for your interest in VERA20k. This guide explains how to set up the
project, where you can help, and what we look for in a pull request.

You don't need reverse-engineering experience, Ghidra or a full picture of the
codebase to start. Many useful tasks need none of that, and some need no code.

Questions are welcome at any point. Ask in the issue you are working on, or on
[Discord](https://discord.gg/kmjRUn5m5F).

## Ways in, by time

- **An evening.** Pick an issue labeled
  [`good first issue`](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue).
  Or, if you have the game, build VERA20k and report where it differs from the
  original (see [Reporting bugs and differences](#reporting-bugs-and-differences)).
  Or tell us how the build goes on Linux or macOS.
- **A weekend.** Pick an issue labeled
  [`help wanted`](https://github.com/YuriPlanet/vera20k/labels/help%20wanted).
  These are bigger, but each one says what "done" means.
- **Longer.** Take care of an area over time, such as Linux/macOS support, the
  menus, or a rendering effect. Start a conversation on Discord or in an issue
  first.

## Set up

### 1. Get the game

VERA20k contains no game files. It reads them from your own copy of Red Alert 2
and Yuri's Revenge. EA sells both games in *Command & Conquer The Ultimate
Collection*, on [Steam](https://store.steampowered.com/bundle/39394/) and from
[EA](https://www.ea.com/games/command-and-conquer/command-and-conquer-the-ultimate-collection/buy/pc).

You need a complete install, with all of its `.mix` files (including `ra2.mix`
and `ra2md.mix`) in one folder.

You can still build VERA20k, run the tests and work on many issues without the
game. Issues say whether they need it.

**Linux and macOS.** The game itself is a Windows release. On Linux, one option
is to install it through Steam with Steam Play (Proton) and point VERA20k at that
install folder. On macOS, you can copy the game folder from a Windows install.
VERA20k has been played on both; if these steps don't work for you, tell us. VERA20k's
asset loader finds files in the game folder without regard to upper or lower
case.

### 2. Install the tools

- Rust from [rustup](https://rustup.rs/). The repository's
  `rust-toolchain.toml` selects the stable toolchain for you. You need 1.88 or
  newer.
- A GPU that supports Vulkan, DirectX 12 or Metal. There is no OpenGL fallback.
- On Debian or Ubuntu, install `libasound2-dev` and `pkg-config` first. This
  list may be incomplete; please tell us what else you needed.

So far, VERA20k has only been played on Windows.

### 3. Build and run

```sh
git clone https://github.com/YuriPlanet/vera20k.git
cd vera20k
cp config.toml.example config.toml    # Windows cmd: copy config.toml.example config.toml
```

Open `config.toml` and set `ra2_dir` to your game folder. Use forward slashes
(`C:/Games/RA2`), because TOML treats `\` as an escape character. Then run the
game from the repository root, where `config.toml` lives:

```sh
cargo run --release --bin vera20k
```

Always use `--release` to play. A debug build is too slow. The first release
build takes a while. The game writes its log to `logs/ra2.log` in the folder you
ran it from.

### 4. Run the tests

```sh
cargo test -p vera20k --lib
```

This is the project's standard test suite. Always add `--lib`. Plain
`cargo test` also builds every program in `tests/`, which takes longer and
mostly needs the game.

On a fresh clone without the game, the suite passes on Windows and Linux; CI
also runs it on macOS. Tests that need the original game's rules files (the
`ini/` folder) print `SKIPPED` and pass without checking anything. To see those
lines, add `-- --show-output`.

To run those tests for real, fill `ini/` from your install:

```sh
cargo run --bin extract-ini
```

Run it from the repository root. It uses a game folder you pass as an argument
(`cargo run --bin extract-ini -- "<game folder>"`), otherwise the `RA2_DIR`
environment variable, otherwise `ra2_dir` in `config.toml`. To make a missing
INI file fail the tests instead of skipping them, set
`VERA20K_REQUIRE_RETAIL_INI=1`.

Once `config.toml` points at your install and `ini/` is filled, more tests run
against your own game files. If one fails only then, tell us which install you
have (Steam, EA app or disc): your files may differ from the ones these tests
were written against.

### 5. Never commit game files

Don't commit anything from the game or extracted from it: no `.mix` archives,
no INI, art (`.shp`, `.vxl`, `.pal`), audio, video or `.exe` files taken from the
game, and nothing from `ini/`.
The `ini/` folder and `config.toml` are already ignored by Git. If a test needs
game data, ask in the issue how to handle it.

## Ways to help, by skill

- **Simulation and gameplay (Rust).** The game logic lives in `src/sim/`.
  Gameplay starter issues come with the evidence about what the original game
  does, so you can focus on the Rust.
- **Rendering.** `src/render/` uses [wgpu](https://wgpu.rs/). The goal is to
  match what the original draws, checked against screenshots or captures from
  the original game.
- **UI, menus and input.** The shell screens, sidebar and hotkeys live in
  `src/app/`, `src/ui/` and `src/sidebar/`.
- **Platforms.** Build and run VERA20k on Linux or macOS and report what
  happens. [Get the game](#1-get-the-game) explains how to get the game files
  there. Fixes for input, audio, file paths and build problems are very welcome.
- **Reverse engineering.** Read the original `gamemd.exe` in Ghidra, or run its
  code with the native comparison tools (Python and Unicorn). See
  [`tools/native_oracle.md`](tools/native_oracle.md) and the
  [Ghidra working notes](docs/research/ghidra-workflow.md). The comparison tools
  accept only one `gamemd.exe` build; its SHA-256 is listed in
  `tools/native_oracle.md`. If your copy has a different hash, please tell us.
  The maintainer's Ghidra project is not public, so you work from your own
  import.
- **Testing against the original.** Play the same situation in Yuri's Revenge
  and in VERA20k, and report what differs, ideally with a short clip of each.
  This needs no code and is one of the most useful things you can do.
- **Docs and tooling.** Fix unclear or outdated docs, and improve the helper
  programs in `src/bin/` and `tools/`.
- **Media.** Screenshots, GIFs and short videos of VERA20k for the README and
  announcements. Record the game window only, so no paths, usernames or
  notifications show. Ask on Discord before starting something big.

## Your first pull request

1. **Pick an issue.** Start with a `good first issue`.
2. **Claim it.** Comment on the issue, and the maintainer will assign it to you.
   Hold one claim at a time. If a claimed issue sees no update for 14 days, it
   becomes free for someone else. A short progress comment keeps your claim.
3. **Branch from `main`** in your fork. `main` changes often, so update your
   branch from `main` before you ask for review.
4. **Keep it small.** One issue per PR, and at most one gameplay mechanism.
   Ask first before anything bigger than a single issue, or before large
   renames or reformatting.
5. **Check your work:**

   ```sh
   cargo test -p vera20k --lib
   cargo clippy -p vera20k --lib
   ```

6. **Format only the files you changed**, with `rustfmt --edition 2024 <file>`.
   Don't run `cargo fmt` on the whole crate. rustfmt also formats the modules a
   file declares (for example, everything under a `mod.rs`), so leave any other
   files it touches out of your PR.
7. **Keep the status honest.** If your PR makes a player-visible feature work
   (or stop working), update the status list in the README in the same PR.
8. **Open the PR against `main`** and fill in the template.

## The project rules, in plain language

Most of the code in this repository is written by AI coding assistants that the
maintainer directs. [`AGENTS.md`](AGENTS.md) and [`CLAUDE.md`](CLAUDE.md) are the
instructions those assistants follow. You don't have to read them. These are the
rules that matter for people:

1. **The original game is the reference.** The original `gamemd.exe` decides
   what correct behavior is. When you change gameplay, say where the behavior
   comes from. Put the native function and address in a comment next to the
   code, like this:

   ```rust
   /// gamemd: `ClassName::FunctionName @ 0x00XXXXXX`
   ```

   If you don't know where something comes from, say so. Don't guess.
2. **One owner per piece of state.** Before you add a field or a decision, find
   where it already lives and extend that code. Don't add a second copy. If you
   replace an old code path, delete the old one in the same PR.
3. **The simulation is deterministic.** The same inputs must give the same
   result on every OS and CPU. In `src/sim/`, use `SimFixed` (fixed-point) for
   math. A float needs a comment that explains why it is there. Nothing in the
   simulation may depend on the platform, the clock or the iteration order of a
   hash map. `src/sim/` never uses `render`, `ui`, `sidebar`, `audio` or `net`;
   a test in `src/architecture_guards.rs` checks this.
4. **Keep the original order.** Random numbers come from the simulation's
   random-number streams. Each draw must use the same stream as the original,
   in the same order and count. Effects that happen within one frame happen in
   the original's order. You can batch or parallelize work only when the result
   stays identical.
5. **Say exactly what you checked.** "Tested" means a named test or command.
   "Matches the original" needs a comparison with the original game. Numbers,
   rounding, random draws and timer lengths are only settled once they are
   checked against the original running, not just read from the code.
6. **Run the tests and clippy** (see above) before you open the PR.

`AGENTS.md` and `CLAUDE.md` also mention Windows/PowerShell commands and items
that exist only on the maintainer's machine, such as `LOCAL.md` and the "main
checkout". The Ghidra notes also mention a machine-local `ghidra-up` helper.
Skip those, or use your platform's equivalent. If you use an AI assistant, it
will probably read these files, so tell it the same.

## Evidence

The project sorts evidence into three levels:

- **Native behavior established:** the original code or data shows what the
  game does (a function, address, caller or retail data).
- **Rust regression tested:** named tests check the Rust implementation.
- **Parity demonstrated:** the Rust code was compared with the original game
  running the same input, and the PR says what that comparison covered.

What we expect from you depends on the change:

| Your change | What to provide |
|---|---|
| No gameplay change (docs, tools, refactoring, platform fixes) | Tests pass, and the PR says "no behavior change" |
| A gameplay starter issue | The maintainer puts the native evidence in the issue. Cite it next to your code and add a regression test |
| Gameplay change from your own research | Ask first. You'll need the native address and your reasoning. Numbers, random draws and timers also need a check against the original running (rule 5). The maintainer can help you find evidence |

If some evidence is out of your reach, say so in the PR. The maintainer can help
fill the gap. Missing evidence about control flow or ordering can become a
recorded follow-up. Numbers, rounding, random draws and timers are different:
they must be checked against the original running before the mechanism merges
(rule 5), but you don't have to be the one who runs that check.

## Using AI

AI tools are welcome, as long as you say so and understand the result.

- Say in your PR which parts an AI tool wrote or helped with.
- You must be able to explain and defend every change in review. You are
  responsible for what you submit.
- The same evidence rules apply to everyone, with or without AI.

**Why is most of the code AI-written?** One maintainer is porting a large
original program one mechanism at a time, and AI coding assistants make that
pace possible. They don't decide what is correct. A change counts when it says
where the behavior comes from in the original and passes the tests. Where it
matters, it must also match the original when both run the same input (see
[`tools/native_oracle.md`](tools/native_oracle.md)). The notes in
`docs/research/` were written the same way and can be wrong, so treat them as
leads, not proof.

## How review works

The maintainer reviews every outside PR, often with help from an AI reviewer.
Comments from an AI reviewer are suggestions; the maintainer decides. We aim to
reply within a few days. If your PR has been quiet for longer, leave a comment
or ask on Discord.

The game files never reach GitHub, so before merging, the maintainer also runs
the tests that need them.

**Credit.** When your PR is merged, your commits keep you as the author. If a
maintainer finishes or reworks your change in another PR, you are credited with
a `Co-authored-by:` line.

## Reporting bugs and differences

Use the [issue forms](https://github.com/YuriPlanet/vera20k/issues/new/choose):

- **Bug report:** something crashes or clearly breaks.
- **Differs from the original game:** VERA20k behaves, looks or sounds different
  from Yuri's Revenge.
- **Platform report:** how building and running went on your machine.

The most useful reports say whether the original game does the same thing, and
include the VERA20k commit (`git rev-parse --short HEAD`), the map, your OS and
GPU, and a clip, screenshot or log. Before you share a log, remove anything
personal, such as your username in file paths.

## Where to learn the code

- The [architecture overview](https://yuriplanet.github.io/vera20k/) (source:
  `docs/index.md`).
- The `//!` comment at the top of each module, which says what it owns.
- `SimRuntime::advance_frame` in `src/sim/runtime.rs` and
  `Simulation::advance_master_frame` in `src/sim/world/mod.rs`, which run one
  simulation frame in the original's order. (`advance_tick` is a test-only
  wrapper.)
- [`tools/native_oracle.md`](tools/native_oracle.md), for how Rust is compared
  with the original.
- [`docs/research/README.md`](docs/research/README.md), for the research notes
  and the archive of older investigations.

## License

VERA20k is licensed under the GNU General Public License v3 (see
[`LICENSE-GPL`](LICENSE-GPL)). By contributing, you agree that your contribution
is licensed under the same terms. There is no contributor license agreement to
sign.
