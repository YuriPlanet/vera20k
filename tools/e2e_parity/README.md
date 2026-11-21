# Yuri’s Revenge end-to-end checks

Run from the checkout root with the licensed retail data configured in
`config.toml` under `[paths] ra2_dir`. The GUI cases need a display and a GPU
adapter. `--ra2-dir` selects another local installation.

```sh
python -m tools.e2e_parity list
python -m tools.e2e_parity run
python -m tools.e2e_parity run --production-only --only first-unit-query
```

The runner builds the selected executables through `tools.cargo_run`. The GUI
cases use hidden windows and private stock-YR settings. Captures, logs, assertion
results and `report.json` go under a new `target/e2e-parity/run-*` directory by
default. `--output` accepts a new directory under an existing parent.

The default suite covers the retail main menu, the transition to Skirmish setup,
the start of the Allied campaign, the LAN lobby, a 300-tick headless match, and
four scripted production scenarios for refinery unloading, MCV deployment,
command-bar input and a quick-save/quick-load round trip. The gameplay scenarios
use the existing [observation and assertion tools](../map_observation.md).

The campaign case runs the retail main menu into the first Allied mission and
checks the campaign receipt: emblem admission, difficulty, seed authority, the
retail map byte source and the loading route. The LAN case drives the main
menu into the network dialog and checks the lobby receipt and its route. The
save/load case takes a real `Ctrl+Shift+M` snapshot, spends credits afterwards
and restores the snapshot with `Ctrl+Shift+N`; the capture itself refuses a
quick load that does not rewind the simulation clock, and the assertions check
that the restored state is the saved one.

To run the strict main-menu comparison, supply the original sealed native
reference guard, which is not included in the checkout:

```sh
python -m tools.e2e_parity run --only main-menu-0xe2-steady \
  --shell-guard /path/to/reference.shell-guard.json
```

`VERA20K_SHELL_GUARD` can provide the same path. A `MATCH` covers only the four
enrolled presentation crops from the [sealed native comparison](../shell_certification/README.md).
Other checks currently report `UNVERIFIED` for route-level native parity. A
bounded original-engine branch comparison can establish a particular outcome,
but not the route's timing or its other effects. The
[stock Skirmish matrix](../exact_shell_ui_matrix/README.md) tracks additional
shell routes and their evidence requirements.

Exit `0` requires every selected production check to pass and every selected
native comparison to report `MATCH`. `--production-only` allows `UNVERIFIED`
results. A failed assertion, invalid comparison, or native drift exits `1`.
Invalid setup or a build failure exits `2`.
