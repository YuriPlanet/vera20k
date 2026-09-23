## What changed

<!-- A few sentences on what this PR does, in plain language. -->

## Linked issue

<!-- For example: Closes #123. Write "none" if there isn't one. -->

## Evidence

<!-- See the Evidence section of CONTRIBUTING.md. Tick every level this PR reaches. -->

- [ ] No gameplay change (docs, tools, refactoring, platform fix)
- [ ] Native behavior established: the original function, address or data is cited next to the code
- [ ] Rust regression tested: new or changed tests are named below
- [ ] Parity demonstrated: compared with the original game running the same input (say what the comparison covered)

<!-- Where the gameplay behavior comes from (address, issue, capture), or "not gameplay". -->

## Tests run

- [ ] `cargo test -p vera20k --lib`
- [ ] `cargo clippy -p vera20k --lib`
- [ ] `rustfmt --edition 2024` on the files I changed
- [ ] Played it with `cargo run --release --bin vera20k` (describe what you checked below)

<!-- Results, named tests, and anything you checked by hand. -->

## AI assistance

- [ ] No AI tools were used
- [ ] AI tools helped (say which parts below)
- [ ] I can explain every change in this PR

<!-- Which parts an AI tool wrote or helped with. -->

## Checklist

- [ ] No files from the game or extracted from it (`.mix` archives, game INI or art files, anything in `ini/`)
- [ ] If a player-visible feature now works (or stopped working), I updated the README status list
