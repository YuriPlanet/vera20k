# Contributing to VERA20k

Thanks for your interest. You don't need reverse-engineering experience to start, and some
useful work needs no code at all. Ask questions in your issue or on
[Discord](https://discord.gg/kmjRUn5m5F).

## Ways to help

- **Pick an issue.** [`good first issue`](https://github.com/YuriPlanet/vera20k/labels/good%20first%20issue)
  fits an evening; [`help wanted`](https://github.com/YuriPlanet/vera20k/labels/help%20wanted)
  is bigger. Each says what "done" means.
- **Compare with the original.** Play the same situation in Yuri's Revenge and in VERA20k and
  report what differs, ideally with a clip of each. No code needed, and very useful.
- **Code.** Gameplay is in `src/sim/`, rendering in `src/render/`, menus and input in
  `src/app/`, `src/ui/` and `src/sidebar/`, tools in `src/bin/` and `tools/`.
- **Reverse engineering.** Study `gamemd.exe` with Ghidra or the
  [native comparison tools](tools/native_oracle.md).
- **Platforms and docs.** Report how it runs on your OS and GPU, or fix outdated docs.

For anything large, ask first.

## Set up

1. **Get the game** (see the [README](README.md#vera20k)). Any edition of Yuri's Revenge 1.001
   works; on macOS copy the folder from a Windows install. Many issues need no game at all.
2. **Install** Rust 1.88 or newer from [rustup](https://rustup.rs/), and have a GPU with
   Vulkan, DirectX 12 or Metal. Debian/Ubuntu also need `libasound2-dev` and `pkg-config`.
3. **Build and run:**

   ```sh
   git clone https://github.com/YuriPlanet/vera20k.git
   cd vera20k
   cp config.toml.example config.toml   # Windows cmd: copy config.toml.example config.toml
   cargo run --release --bin vera20k
   ```

   Set `ra2_dir` in `config.toml` with forward slashes (`C:/Games/RA2`). Always play with
   `--release`; the log is in `logs/ra2.log`.
4. **Run the tests:** `cargo test -p vera20k --lib` (always `--lib`). Tests that need the
   game's INI files print `SKIPPED` until you run `cargo run --bin extract-ini [game folder]`.
5. **Never commit game files** (`.mix`, INI, art, audio, video, `.exe`) or anything in `ini/`.

More build and tool details are in the [tool index](tools/README.md).

## Your first pull request

1. **Claim an issue** by commenting. One at a time; a claim with no update for 14 days is free.
2. **Branch from `main`.** One issue per PR, at most one gameplay mechanism.
3. **Check it:** `cargo test -p vera20k --lib` and `cargo clippy -p vera20k --lib`. Format only
   the files you changed with `rustfmt --edition 2024 <file>`, not `cargo fmt`.
4. **Update the README status** if a feature starts or stops working.
5. **Open the PR against `main`** and fill in the template.

## Project rules

The AI agents' full rules are in [`AGENTS.md`](AGENTS.md). For people, these matter:

1. **The original is the reference.** Cite the native function in a comment:
   `/// gamemd: ClassName::Function @ 0x00XXXXXX`. Don't guess.
2. **One owner per piece of state.** Extend the existing owner; delete the old path you replace.
3. **Deterministic simulation.** Same inputs, same result on every OS and CPU. Use `SimFixed`
   in `src/sim/`, and keep random draws and same-frame effects in the original's order.
4. **AI tools are welcome** if the PR says which parts they wrote and you can explain every change.

## Evidence

Gameplay claims are "native behavior established" (from the original's code or data), "Rust
regression tested" (named tests) or "parity demonstrated" (compared with the original running).
For starter issues the maintainer supplies the evidence; you cite it and add a test.

## Review and bugs

The maintainer reviews every outside PR and aims to reply within a few days; ping the PR or
Discord if it goes quiet. Report bugs and differences with the
[issue forms](https://github.com/YuriPlanet/vera20k/issues/new/choose), including the commit,
map, OS, GPU and a clip or log. To learn the code, start with the
[architecture overview](https://yuriplanet.github.io/vera20k/).

## License

GPLv3 ([`LICENSE-GPL`](LICENSE-GPL)). Contributions are licensed the same way; there is no CLA.
