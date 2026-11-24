# Shell, storage and network native comparisons

Each `<name>.py` runs original shell dialog, saved-game, keyboard, sound-theme or
LAN code under Unicorn and checks or writes its `<name>.json` with a
`<name>.meta.json` sidecar. See the [shared native runner](../native_oracle.md)
for setup, `--check`/`--write` and evidence limits.
`lan_shell_controls.json` holds six packets captured once from an original LAN
host; no harness here rebuilds them.

## Shared fixture owners

| Module | Provides | Importers |
| --- | --- | ---: |
| `shell_relayout` | `Machine`, `Image`, `parse_template`: dialog templates and the original WM_INITDIALOG relayout | ~12 |
| `keyboard_bindings` | `KeyboardFixture` and `stock_csf`: the original keyboard registry and command strings | ~8 |
| `saved_game_layout` | `code_guard`/`check_code` (every executable section unchanged after a run) and the saved-game dialog resources | ~6 |
| `in_game_shell_geometry` | Original in-game shell frame geometry (72FC60) reused by the abort, diplomacy, keyboard, saved-game and sound layouts | ~5 |

`code_guard`/`check_code` are also used by sidebar, input, rules and RMG producers.

## Finding producers and consumers

```sh
git grep -l 'storage_oracle.shell_relayout' -- tools   # Python importers
git grep -n 'storage_oracle/<name>.json' -- src        # Rust tests reading a corpus
python -m tools.storage_oracle.<name> --check          # replay
```

A harness stays only while a Rust test reads its output (AGENTS.md).
